use std::{cmp, collections::HashMap, future::Future, net::SocketAddr, sync::Arc, time::Duration};

use conn::Conn;
use packet_derive::*;
pub use packets::Reliability;
use system_packets::*;
use tokio::{
    net::{ToSocketAddrs, UdpSocket},
    sync::{mpsc, Mutex, Notify},
    time::sleep,
};

pub(crate) mod conn;
pub(crate) mod cubic;
pub(crate) mod packets;
pub(crate) mod receive;
pub(crate) mod send;
pub(crate) mod system_packets;

pub const PROTOCOL_VERSION: u8 = 0xA;
pub const MAX_MTU_SIZE: u16 = 1400;

type Udp = Arc<UdpSocket>;
type Session = Arc<Mutex<Conn>>;

#[derive(Debug)]
pub(crate) enum ConnEvent {
    Packet(Vec<u8>),
    Disconnected,
    Timeout,
}

pub struct UcpSession {
    receiver: mpsc::Receiver<ConnEvent>,
    addr: SocketAddr,
    conn: Session,

    drop_notifyor: Arc<Notify>,
    drop_sender: Option<mpsc::Sender<SocketAddr>>,
}

impl UcpSession {
    pub async fn connect(
        local: impl ToSocketAddrs,
        remote: SocketAddr,
        guid: u64,
    ) -> std::io::Result<Self> {
        let udp: Udp = Arc::new(UdpSocket::bind(local).await?);

        let reply1: OpenConnectionReply1 = async {
            for count in 0..12 {
                let mtu = match count / 4 {
                    0 => 1496,
                    1 => 1204,
                    2 => 584,
                    _ => 0,
                };
                let ocrequest1 = OpenConnectionRequest1 {
                    magic: (),
                    protocol_version: PROTOCOL_VERSION,
                    mtu_size: mtu,
                };
                let mut bytes = vec![];
                encode_syspacket(ocrequest1, &mut bytes)?;
                udp.send_to(&bytes, remote).await?;

                let decode_ocreply1 = async {
                    loop {
                        let mut v = [0u8; 2048];
                        let (size, src) = udp.recv_from(&mut v).await?;

                        if src == remote {
                            let reply: OpenConnectionReply1 = decode_syspacket(&v[..size])?;
                            return Ok(reply);
                        }
                    }
                };

                tokio::select! {
                    r = decode_ocreply1 => {
                        return r
                    },
                    _ = sleep(Duration::from_millis(500)) => {}
                }
            }
            Err(std::io::Error::new(
                std::io::ErrorKind::NotConnected,
                format!("Failed to connect to {}", remote),
            ))
        }
        .await?;

        let reply2: OpenConnectionReply2 = async {
            for _ in 0..4 {
                let decode_ocreply2 = async {
                    let ocrequest2 = OpenConnectionRequest2 {
                        magic: (),
                        address: remote,
                        mtu: cmp::min(reply1.mtu_size, MAX_MTU_SIZE),
                        guid,
                    };
                    let mut bytes = vec![];
                    encode_syspacket(ocrequest2, &mut bytes)?;
                    udp.send_to(&bytes, remote).await?;
                    loop {
                        let mut v = [0u8; 2048];
                        let (size, src) = udp.recv_from(&mut v).await?;
                        if src == remote {
                            return decode_syspacket::<OpenConnectionReply2>(&v[..size]);
                        }
                    }
                };

                tokio::select! {
                    r = decode_ocreply2 => {
                        return r
                    },
                    _ = sleep(Duration::from_millis(500)) => {}
                }
            }
            Err(std::io::Error::new(
                std::io::ErrorKind::NotConnected,
                format!("Failed to connect to {}", remote),
            ))
        }
        .await?;

        let (s, r) = mpsc::channel(128);
        let conn = Arc::new(Mutex::new(Conn::new(
            remote,
            cmp::min(reply2.mtu, MAX_MTU_SIZE) as usize,
            udp.clone(),
            s,
        )));

        let mut session = Self::init_with_conn(conn, r, remote, None, Some(udp));

        let request = ConnectionRequest {
            guid,
            time: time(),
            use_encryption: false,
        };
        session
            .send_syspacket(request, Reliability::ReliableOrdered)
            .await?;
        loop {
            let got = session.recv().await?;
            if got[0] == ConnectionRequestAccepted::ID {
                let accepted: ConnectionRequestAccepted =
                    decode_syspacket::<ConnectionRequestAccepted>(&got)?;
                let new_incoming = NewIncomingConnections {
                    server_address: remote,
                    request_timestamp: accepted.request_timestamp,
                    accepted_timestamp: accepted.accepted_timestamp,
                };
                session
                    .send_syspacket(new_incoming, Reliability::ReliableOrdered)
                    .await?;
                return Ok(session);
            }
        }
    }

    fn init_with_conn(
        conn: Session,
        receiver: mpsc::Receiver<ConnEvent>,
        addr: SocketAddr,
        sender: Option<mpsc::Sender<SocketAddr>>,
        udp: Option<Udp>,
    ) -> Self {
        let ticker = conn.clone();
        let n1 = Arc::new(Notify::new());
        let n2 = n1.clone();
        tokio::spawn(async move {
            loop {
                let tick = async {
                    sleep(Duration::from_millis(50)).await;
                    ticker.lock().await.update().await.unwrap();
                };
                tokio::select! {
                    _ = tick => {},
                    _ = n2.notified() => {
                        break;
                    }
                }
            }
        });

        if let Some(udp) = udp {
            let n3 = n1.clone();
            let conn2 = conn.clone();
            let address = addr;
            tokio::spawn(async move {
                loop {
                    let mut v = [0u8; 1500];
                    tokio::select! {
                        res = udp.recv_from(&mut v) => {
                            let (size,src) = match res {
                                Ok(res) => res,
                                Err(_) => continue,
                            };
                            if src == address {
                                conn2.lock().await.handle(&v[..size]).await.unwrap_or_default();
                            }
                        },
                        _ = n3.notified() => {
                            break;
                        }
                    }
                }
            });
        }

        Self {
            receiver,
            addr,
            conn,
            drop_notifyor: n1,
            drop_sender: sender,
        }
    }

    pub async fn recv(&mut self) -> std::io::Result<Vec<u8>> {
        loop {
            match self.receiver.recv().await {
                Some(ConnEvent::Packet(bytes)) => return Ok(bytes),
                Some(ConnEvent::Disconnected) => {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::ConnectionReset,
                        "Connection closed",
                    ))
                }
                Some(ConnEvent::Timeout) => {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::TimedOut,
                        "Connection timeout",
                    ))
                }
                None => {}
            }
        }
    }

    pub async fn send(&self, bytes: &[u8], reliability: Reliability) -> std::io::Result<()> {
        self.conn.lock().await.send(bytes, reliability).await
    }

    pub(crate) async fn send_syspacket<P: SystemPacket>(
        &self,
        packet: P,
        reliability: Reliability,
    ) -> std::io::Result<()> {
        self.conn
            .lock()
            .await
            .send_syspacket(packet, reliability)
            .await
    }

    pub async fn set_nodelay(&self, nodelay: bool) {
        self.conn.lock().await.set_nodelay(nodelay);
    }

    pub async fn nodelay(&self) -> bool {
        self.conn.lock().await.nodelay()
    }
}

impl Drop for UcpSession {
    fn drop(&mut self) {
        self.drop_notifyor.notify_one();
        if let Some(s) = self.drop_sender.clone() {
            let addr = self.addr;
            tokio::spawn(async move {
                s.send(addr).await.unwrap();
            });
        }
    }
}

async fn into_session(mut session: UcpSession) -> std::io::Result<UcpSession> {
    loop {
        let got = session.recv().await?;
        match got[0] {
            ConnectionRequest::ID => {
                let request: ConnectionRequest = decode_syspacket(&got)?;
                let accept = ConnectionRequestAccepted {
                    client_address: session.addr,
                    system_index: 0,
                    request_timestamp: request.time,
                    accepted_timestamp: time(),
                };
                session
                    .send_syspacket(accept, Reliability::ReliableOrdered)
                    .await?;
            }
            NewIncomingConnections::ID => return Ok(session),
            _ => {}
        }
    }
}

pub struct UcpListener {
    socket: Udp,
    guid: u64,
    title: String,
    conns: HashMap<SocketAddr, Session>,
    drop_receiver: mpsc::Receiver<SocketAddr>,
    drop_sender: mpsc::Sender<SocketAddr>,
}

impl UcpListener {
    pub fn get_raw_socket(&self) -> Arc<UdpSocket> {
        self.socket.clone()
    }
    pub fn local_addr(&self) -> std::io::Result<SocketAddr> {
        self.socket.local_addr()
    }
    pub async fn bind(addr: impl ToSocketAddrs, guid: u64, title: String) -> std::io::Result<Self> {
        let (s, r) = mpsc::channel(32);
        Ok(Self {
            socket: Arc::new(UdpSocket::bind(addr).await?),
            guid,
            title,
            conns: HashMap::new(),
            drop_receiver: r,
            drop_sender: s,
        })
    }
    pub async fn accept(
        &mut self,
    ) -> std::io::Result<impl Future<Output = Result<UcpSession, std::io::Error>>> {
        loop {
            let mut v = [0u8; 2048];
            let (size, src) = tokio::select! {
                rs = self.socket.recv_from(&mut v) => {rs},
                addr = self.drop_receiver.recv() => {
                    self.conns.remove(&addr.unwrap());
                    continue;
                }
            }?;

            if self.conns.contains_key(&src) {
                self.conns
                    .get(&src)
                    .unwrap()
                    .lock()
                    .await
                    .handle(&v[..size])
                    .await?;
            } else {
                let mut reader = std::io::Cursor::new(&v[..size]);
                match u8::decode(&mut reader)? {
                    UnconnectedPing::ID => self.handle_ping(&v[..size], src).await?,
                    OpenConnectionRequest1::ID => self.handle_ocrequest1(&v[..size], src).await?,
                    OpenConnectionRequest2::ID => {
                        let (conn, r) = self.handle_ocrequest2(&v[..size], src).await?;
                        return Ok(into_session(UcpSession::init_with_conn(
                            conn,
                            r,
                            src,
                            Some(self.drop_sender.clone()),
                            None,
                        )));
                    }
                    _ => {}
                }
            }
        }
    }

    async fn handle_ping(&self, v: &[u8], src: SocketAddr) -> std::io::Result<()> {
        let packet: UnconnectedPing = decode_syspacket(v)?;
        let pong = UnconnectedPong {
            time: packet.time_stamp,
            guid: self.guid,
            magic: (),
            motd: self.title.clone(),
        };
        let mut bytes = vec![];
        encode_syspacket(pong, &mut bytes)?;
        self.socket.send_to(&bytes[..], src).await?;
        Ok(())
    }

    async fn handle_ocrequest1(&self, v: &[u8], src: SocketAddr) -> std::io::Result<()> {
        let packet: OpenConnectionRequest1 = decode_syspacket(v)?;

        if packet.protocol_version != PROTOCOL_VERSION {
            let reply = IncompatibleProtocolVersion {
                server_protocol: PROTOCOL_VERSION,
                magic: (),
                server_guid: self.guid,
            };
            let mut bytes = vec![];
            encode_syspacket(reply, &mut bytes)?;
            self.socket.send_to(&bytes[..], src).await?;
            return Ok(());
        }
        let reply = OpenConnectionReply1 {
            magic: (),
            guid: self.guid,
            use_encryption: false,
            mtu_size: cmp::min(packet.mtu_size, MAX_MTU_SIZE),
        };
        let mut bytes = vec![];
        encode_syspacket(reply, &mut bytes)?;
        self.socket.send_to(&bytes[..], src).await?;
        Ok(())
    }

    async fn handle_ocrequest2(
        &mut self,
        v: &[u8],
        src: SocketAddr,
    ) -> std::io::Result<(Session, mpsc::Receiver<ConnEvent>)> {
        let packet: OpenConnectionRequest2 = decode_syspacket(v)?;
        let mtu = cmp::min(packet.mtu, MAX_MTU_SIZE);
        let reply = OpenConnectionReply2 {
            magic: (),
            guid: self.guid,
            address: src,
            mtu,
            use_encryption: false,
        };
        let mut bytes = vec![];
        encode_syspacket(reply, &mut bytes)?;
        self.socket.send_to(&bytes[..], src).await?;
        let (s, r) = mpsc::channel(128);
        let session = Arc::new(Mutex::new(Conn::new(
            src,
            mtu as usize,
            self.socket.clone(),
            s,
        )));
        self.conns.insert(src, session.clone());
        Ok((session, r))
    }
}

pub(crate) fn time() -> u64 {
    let time = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis();
    time as u64
}
