use std::{collections::HashMap, net::SocketAddr, sync::Arc};

use conn::Conn;
use packet_derive::*;
use packets::Reliability;
use system_packets::*;
use tokio::{
    net::{ToSocketAddrs, UdpSocket},
    sync::Mutex,
};

pub(crate) mod conn;
pub(crate) mod fragment;
pub mod packets;
pub(crate) mod receive;
pub(crate) mod send;
pub(crate) mod system_packets;
pub(crate) mod time;

pub const PROTOCOL_VERSION: u8 = 0xA;

type Udp = Arc<UdpSocket>;
type Session = Arc<Mutex<Conn>>;

pub struct UcpSession {
    conn: Session,
}

impl UcpSession {
    pub async fn recv(&self) -> std::io::Result<Vec<u8>> {
        loop {
            if let Some(packet) = self.conn.lock().await.receive() {
                return Ok(packet);
            }
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            self.conn.lock().await.update().await?
        }
    }
    pub async fn send(&self, bytes: Vec<u8>, reliability: Reliability) -> std::io::Result<()> {
        self.conn.lock().await.send(bytes, reliability)
    }
}

pub struct UcpListener {
    socket: Udp,
    guid: u64,
    title: String,
    conns: HashMap<SocketAddr, Session>,
}

impl UcpListener {
    pub fn get_raw_socket(&self) -> Arc<UdpSocket> {
        self.socket.clone()
    }
    pub fn local_addr(&self) -> std::io::Result<SocketAddr> {
        self.socket.local_addr()
    }
    pub async fn bind(addr: impl ToSocketAddrs, guid: u64, title: String) -> std::io::Result<Self> {
        Ok(Self {
            socket: Arc::new(UdpSocket::bind(addr).await?),
            guid,
            title,
            conns: HashMap::new(),
        })
    }
    pub async fn accept(&mut self) -> std::io::Result<UcpSession> {
        loop {
            let mut v = [0u8; 2048];
            let (size, src) = self.socket.recv_from(&mut v).await?;
            if self.conns.contains_key(&src) {
                self.conns
                    .get(&src)
                    .unwrap()
                    .lock()
                    .await
                    .handle(&v[..size])?;
            } else {
                let mut reader = std::io::Cursor::new(&v[..size]);
                match u8::decode(&mut reader)? {
                    UnconnectedPing::ID => self.handle_ping(&v[..size], src).await?,
                    OpenConnectionRequest1::ID => self.handle_ocrequest1(&v[..size], src).await?,
                    OpenConnectionRequest2::ID => {
                        return Ok(UcpSession {
                            conn: self.handle_ocrequest2(&v[..size], src).await?,
                        })
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

    async fn handle_ocrequest2(&mut self, v: &[u8], src: SocketAddr) -> std::io::Result<Session> {
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
        let session = Arc::new(Mutex::new(Conn::new(src, self.socket.clone())));
        self.conns.insert(src, session.clone());
        Ok(session)
    }
}
