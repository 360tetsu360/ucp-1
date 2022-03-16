use std::{net::SocketAddr, io::Write};

use packet_derive::*;

pub(crate) trait SystemPacket : Den {
    const ID : u8;
}

pub(crate) fn decode_syspacket<T : SystemPacket>(bytes : &[u8]) -> std::io::Result<T> {
    if bytes[0] != T::ID {
        return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "Wrong ID".to_string()));
    }
    let mut reader = CursorReader::new(bytes);
    T::decode(&mut reader)
}

pub(crate) fn encode_syspacket<T : SystemPacket>(packet : T,dst : &mut Vec<u8>) -> std::io::Result<()> {
    let mut writer = CursorWriter::new(dst);
    writer.write(&[T::ID;1])?;
    packet.encode(&mut writer)
}

#[derive(Den)]
pub struct ConnectedPing {
    pub client_time_stamp : u64
}
impl SystemPacket for ConnectedPing {
    const ID : u8 = 0x0;
}

#[derive(Den)]
pub struct UnconnectedPing {
    pub time_stamp : u64,
    #[den(with="MAGIC")]
    pub magic : (),
    pub guid : u64
}
impl SystemPacket for UnconnectedPing {
    const ID : u8 = 0x1;
}

#[derive(Den)]
pub struct ConnectedPong {
    pub client_timestamp: u64,
    pub server_timestamp: u64,
}
impl SystemPacket for ConnectedPong {
    const ID : u8 = 0x3;
}

pub struct OpenConnectionRequest1 {
    pub magic: (),
    pub protocol_version: u8,
    pub mtu_size: u16,
}
impl Den for OpenConnectionRequest1 {
    fn decode(bytes: &mut CursorReader) -> std::io::Result<Self> {
        Ok(Self {
            magic : MAGIC::decode(bytes)?,
            protocol_version : u8::decode(bytes)?,
            mtu_size : bytes.get_ref().len() as u16 + 32, //udp header
        })
    }

    fn encode(&self, bytes: &mut CursorWriter) -> std::io::Result<()> {
        MAGIC::encode(&self.magic, bytes)?;
        u8::encode(&self.protocol_version, bytes)?;
        let mut null_padding = vec![0u8;(self.mtu_size - 50) as usize]; //32(udp header) + 18(raknet data)
        bytes.write_all(&mut null_padding)
    }

    fn size(&self) -> usize {
        (self.mtu_size - 32) as usize
    }
}
impl SystemPacket for OpenConnectionRequest1 {
    const ID : u8 = 0x5;
}

#[derive(Den)]
pub struct OpenConnectionReply1 {
    #[den(with="MAGIC")]
    pub magic: (),
    pub guid: u64,
    pub use_encryption: bool,
    pub mtu_size: u16,
}
impl SystemPacket for OpenConnectionReply1 {
    const ID : u8 = 0x6;
}

#[derive(Den)]
pub struct OpenConnectionRequest2 {
    #[den(with="MAGIC")]
    pub magic: (),
    pub address: SocketAddr,
    pub mtu: u16,
    pub guid: u64,
}
impl SystemPacket for OpenConnectionRequest2 {
    const ID : u8 = 0x7;
}

#[derive(Den)]
pub struct OpenConnectionReply2 {
    #[den(with="MAGIC")]
    pub magic: (),
    pub guid: u64,
    pub address: SocketAddr,
    pub mtu: u16,
    pub use_encryption: bool,
}
impl SystemPacket for OpenConnectionReply2 {
    const ID : u8 = 0x8;
}

#[derive(Den)]
pub struct ConnectionRequest {
    pub guid: u64,
    pub time: i64,
    pub use_encryption: u8,
}
impl SystemPacket for ConnectionRequest {
    const ID : u8 = 0x9;
}

#[derive(Den)]
pub struct ConnectionRequestAccepted {
    pub client_address: SocketAddr,
    pub system_index: u16,
    pub request_timestamp: u64,
    pub accepted_timestamp: u64,
}
impl SystemPacket for ConnectionRequestAccepted {
    const ID : u8 = 0x10;
}

#[derive(Den)]
pub struct AlreadyConnected {
    #[den(with="MAGIC")]
    pub magic: (),
    pub guid: u64,
}
impl SystemPacket for AlreadyConnected {
    const ID : u8 = 0x12;
}

#[derive(Den)]
pub struct NewIncomingConnections {
    pub server_address: SocketAddr,
    pub request_timestamp: i64,
    pub accepted_timestamp: i64,
}
impl SystemPacket for NewIncomingConnections {
    const ID : u8 = 0x13;
}

#[derive(Den)]
pub struct DisconnectionNotification {}
impl SystemPacket for DisconnectionNotification {
    const ID : u8 = 0x15;
}

#[derive(Den)]
pub struct UnconnectedPong {
    pub time: u64,
    pub guid: u64,
    #[den(with="MAGIC")]
    pub magic: (),
    pub motd: String,
}
impl SystemPacket for UnconnectedPong {
    const ID : u8 = 0x1c;
}

pub struct Acknowledge {
    pub record_count: u16,
    pub max_equals_min: bool,
    pub sequences: (u32, u32),
}
impl Den for Acknowledge {
    fn decode(bytes: &mut CursorReader) -> std::io::Result<Self> {
        let record_count = u16::decode(bytes)?;
        let max_equals_min = bool::decode(bytes)?;
        let sequences;
        let sequence = U24::decode(bytes)?;
        if max_equals_min {
            sequences = (sequence, sequence)
        } else {
            let sequence_max = U24::decode(bytes)?;
            sequences = (sequence, sequence_max)
        }
        Ok(Self {
            record_count,
            max_equals_min,
            sequences,
        })
    }

    fn encode(&self, bytes: &mut CursorWriter) -> std::io::Result<()> {
        u16::encode(&self.record_count, bytes)?;
        bool::encode(&self.max_equals_min,bytes)?;
        U24::encode(&self.sequences.0, bytes)?;
        if !self.max_equals_min {
            U24::encode(&self.sequences.1, bytes)?;
        }
        Ok(())
    }

    fn size(&self) -> usize {
        if self.max_equals_min {
            6
        }else {
            9
        }
    }
}

#[derive(Den)]
pub struct ACK {
    pub ack : Acknowledge
}
impl SystemPacket for ACK {
    const ID : u8 = 0xc0;
}

#[derive(Den)]
pub struct NACK {
    pub nack : Acknowledge
}
impl SystemPacket for NACK {
    const ID : u8 = 0xa0;
}