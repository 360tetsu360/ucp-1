use std::{collections::HashMap, net::SocketAddr, sync::Arc};

use conn::Conn;
use packet_derive::*;
use packets::Reliability;
use system_packets::*;
use tokio::{
    net::{ToSocketAddrs, UdpSocket},
    sync::{mpsc, Mutex, Notify},
};

pub(crate) mod conn;
pub(crate) mod fragment;
pub mod packets;
pub(crate) mod receive;
pub(crate) mod send;
pub(crate) mod system_packets;
pub(crate) mod time;
pub(crate) mod cubic;

pub const PROTOCOL_VERSION: u8 = 0xA;

type Udp = Arc<UdpSocket>;
type Session = Arc<Mutex<Conn>>;

pub struct UcpSession {
    receiver: mpsc::Receiver<Vec<u8>>,
    addr: SocketAddr,
    conn: Session,

    drop_notifyor: Arc<Notify>,
    drop_sender: mpsc::Sender<SocketAddr>,
}

impl UcpSession {
    fn init_with_conn(
        conn: Session,
        receiver: mpsc::Receiver<Vec<u8>>,
        addr: SocketAddr,
        sender: mpsc::Sender<SocketAddr>,
    ) -> Self {
        let ticker = conn.clone();
        let n1 = Arc::new(Notify::new());
        let n2 = n1.clone();
        tokio::spawn(async move {
            loop {
                let tick = async {
                    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                    ticker.lock().await.update().await.unwrap();
                };
                tokio::select! {
                    _ = tick => {},
                    _ = n1.notified() => {
                        break;
                    }
                }
            }
        });
        Self {
            receiver,
            addr,
            conn,
            drop_notifyor: n2,
            drop_sender: sender,
        }
    }

    pub async fn recv(&mut self) -> std::io::Result<Vec<u8>> {
        loop {
            if let Some(packet) = self.receiver.recv().await {
                //handle packet here
                return Ok(packet);
            }
        }
    }

    pub async fn send(&self, bytes: &[u8], reliability: Reliability) -> std::io::Result<()> {
        self.conn.lock().await.send(bytes, reliability).await
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
        let s = self.drop_sender.clone();
        let n = self.drop_notifyor.clone();
        let addr = self.addr;
        tokio::spawn(async move {
            n.notify_one();
            s.send(addr).await.unwrap();
        });
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
    pub async fn accept(&mut self) -> std::io::Result<UcpSession> {
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
                        return Ok(UcpSession::init_with_conn(
                            conn,
                            r,
                            src,
                            self.drop_sender.clone(),
                        ));
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
            mtu_size: packet.mtu_size,
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
    ) -> std::io::Result<(Session, mpsc::Receiver<Vec<u8>>)> {
        let packet: OpenConnectionRequest2 = decode_syspacket(v)?;
        let reply = OpenConnectionReply2 {
            magic: (),
            guid: self.guid,
            address: src,
            mtu: packet.mtu,
            use_encryption: false,
        };
        let mut bytes = vec![];
        encode_syspacket(reply, &mut bytes)?;
        self.socket.send_to(&bytes[..], src).await?;
        let (s, r) = mpsc::channel(128);
        let session = Arc::new(Mutex::new(Conn::new(
            src,
            packet.mtu as usize,
            self.socket.clone(),
            s,
        )));
        self.conns.insert(src, session.clone());
        Ok((session, r))
    }
}
