use std::{io::Write, net::SocketAddr};

use packet_derive::*;

pub const UDP_HEADER_LEN: u16 = 32;

pub(crate) trait SystemPacket: Den {
    const ID: u8;
}

pub(crate) fn decode_syspacket<T: SystemPacket>(bytes: &[u8]) -> std::io::Result<T> {
    let mut reader = CursorReader::new(bytes);
    if u8::decode(&mut reader)? != T::ID {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "Wrong ID".to_string(),
        ));
    }
    T::decode(&mut reader)
}

pub(crate) fn encode_syspacket<T: SystemPacket>(
    packet: T,
    dst: &mut Vec<u8>,
) -> std::io::Result<()> {
    let mut writer = CursorWriter::new(dst);
    u8::encode(&T::ID, &mut writer)?;
    packet.encode(&mut writer)
}

#[derive(Den)]
pub struct ConnectedPing {
    #[den(with = "Big")]
    pub client_timestamp: u64,
}
impl SystemPacket for ConnectedPing {
    const ID: u8 = 0x0;
}

#[derive(Den)]
pub struct UnconnectedPing {
    #[den(with = "Big")]
    pub time_stamp: u64,
    #[den(with = "MAGIC")]
    pub magic: (),
    #[den(with = "Big")]
    pub guid: u64,
}
impl SystemPacket for UnconnectedPing {
    const ID: u8 = 0x1;
}

#[derive(Den)]
pub struct ConnectedPong {
    #[den(with = "Big")]
    pub client_timestamp: u64,
    #[den(with = "Big")]
    pub server_timestamp: u64,
}
impl SystemPacket for ConnectedPong {
    const ID: u8 = 0x3;
}

pub struct OpenConnectionRequest1 {
    pub magic: (),
    pub protocol_version: u8,
    pub mtu_size: u16,
}
impl Den for OpenConnectionRequest1 {
    fn decode(bytes: &mut CursorReader) -> std::io::Result<Self> {
        Ok(Self {
            magic: MAGIC::decode(bytes)?,
            protocol_version: u8::decode(bytes)?,
            mtu_size: bytes.get_ref().len() as u16 + UDP_HEADER_LEN, //udp header
        })
    }

    fn encode(&self, bytes: &mut CursorWriter) -> std::io::Result<()> {
        MAGIC::encode(&self.magic, bytes)?;
        u8::encode(&self.protocol_version, bytes)?;
        let null_padding = vec![0u8; (self.mtu_size - UDP_HEADER_LEN - 18) as usize]; //32(udp header) + 18(raknet data)
        bytes.write_all(&null_padding)
    }

    fn size(&self) -> usize {
        (self.mtu_size - UDP_HEADER_LEN) as usize
    }
}
impl SystemPacket for OpenConnectionRequest1 {
    const ID: u8 = 0x5;
}

#[derive(Den)]
pub struct OpenConnectionReply1 {
    #[den(with = "MAGIC")]
    pub magic: (),
    #[den(with = "Big")]
    pub guid: u64,
    pub use_encryption: bool,
    #[den(with = "Big")]
    pub mtu_size: u16,
}
impl SystemPacket for OpenConnectionReply1 {
    const ID: u8 = 0x6;
}

#[derive(Den)]
pub struct OpenConnectionRequest2 {
    #[den(with = "MAGIC")]
    pub magic: (),
    pub address: SocketAddr,
    #[den(with = "Big")]
    pub mtu: u16,
    #[den(with = "Big")]
    pub guid: u64,
}
impl SystemPacket for OpenConnectionRequest2 {
    const ID: u8 = 0x7;
}

#[derive(Den)]
pub struct OpenConnectionReply2 {
    #[den(with = "MAGIC")]
    pub magic: (),
    #[den(with = "Big")]
    pub guid: u64,
    pub address: SocketAddr,
    #[den(with = "Big")]
    pub mtu: u16,
    pub use_encryption: bool,
}
impl SystemPacket for OpenConnectionReply2 {
    const ID: u8 = 0x8;
}

#[derive(Den)]
pub struct ConnectionRequest {
    #[den(with = "Big")]
    pub guid: u64,
    #[den(with = "Big")]
    pub time: u64,
    pub use_encryption: bool,
}
impl SystemPacket for ConnectionRequest {
    const ID: u8 = 0x9;
}

pub struct ConnectionRequestAccepted {
    pub client_address: SocketAddr,
    pub system_index: u16,
    pub request_timestamp: u64,
    pub accepted_timestamp: u64,
}
impl Den for ConnectionRequestAccepted {
    fn decode(bytes: &mut CursorReader) -> std::io::Result<Self> {
        let client_address = SocketAddr::decode(bytes)?;
        let system_index = Big::decode(bytes)?;
        bytes.set_position(
            bytes.position() + ((bytes.get_ref().len() - 16) - bytes.position() as usize) as u64,
        );
        let request_timestamp = Big::decode(bytes)?;
        let accepted_timestamp = Big::decode(bytes)?;
        Ok(Self {
            client_address,
            system_index,
            request_timestamp,
            accepted_timestamp,
        })
    }

    fn encode(&self, bytes: &mut CursorWriter) -> std::io::Result<()> {
        SocketAddr::encode(&self.client_address, bytes)?;
        Big::encode(&self.system_index, bytes)?;
        bytes.write_all(&[0x6u8; 10])?;
        Big::encode(&self.request_timestamp, bytes)?;
        Big::encode(&self.accepted_timestamp, bytes)?;
        Ok(())
    }

    fn size(&self) -> usize {
        SocketAddr::size(&self.client_address) + 18 + 10
    }
}
impl SystemPacket for ConnectionRequestAccepted {
    const ID: u8 = 0x10;
}

pub struct NewIncomingConnections {
    pub server_address: SocketAddr,
    pub request_timestamp: u64,
    pub accepted_timestamp: u64,
}
impl Den for NewIncomingConnections {
    fn decode(bytes: &mut CursorReader) -> std::io::Result<Self> {
        Ok(Self {
            server_address: SocketAddr::decode(bytes)?,
            request_timestamp: {
                bytes.set_position(
                    bytes.position()
                        + ((bytes.get_ref().len() - 16) - bytes.position() as usize) as u64,
                );
                Big::decode(bytes)?
            },
            accepted_timestamp: Big::decode(bytes)?,
        })
    }

    fn encode(&self, bytes: &mut CursorWriter) -> std::io::Result<()> {
        SocketAddr::encode(&self.server_address, bytes)?;
        bytes.write_all(&[0x6u8; 10])?;
        Big::encode(&self.request_timestamp, bytes)?;
        Big::encode(&self.accepted_timestamp, bytes)?;
        Ok(())
    }

    fn size(&self) -> usize {
        SocketAddr::size(&self.server_address) + 16 + 10
    }
}
impl SystemPacket for NewIncomingConnections {
    const ID: u8 = 0x13;
}

#[derive(Den)]
pub struct DisconnectionNotification {}
impl SystemPacket for DisconnectionNotification {
    const ID: u8 = 0x15;
}

#[derive(Den)]
pub struct UnconnectedPong {
    #[den(with = "Big")]
    pub time: u64,
    #[den(with = "Big")]
    pub guid: u64,
    #[den(with = "MAGIC")]
    pub magic: (),
    pub motd: String,
}
impl SystemPacket for UnconnectedPong {
    const ID: u8 = 0x1c;
}

#[derive(Den)]
pub struct IncompatibleProtocolVersion {
    pub server_protocol: u8,
    #[den(with = "MAGIC")]
    pub magic: (),
    #[den(with = "Big")]
    pub server_guid: u64,
}
impl SystemPacket for IncompatibleProtocolVersion {
    const ID: u8 = 0x19;
}

pub struct Acknowledge {
    pub record_count: u16,
    pub max_equals_min: bool,
    pub sequences: (u32, u32),
}
impl Den for Acknowledge {
    fn decode(bytes: &mut CursorReader) -> std::io::Result<Self> {
        let record_count = Big::decode(bytes)?;
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
        Big::encode(&self.record_count, bytes)?;
        bool::encode(&self.max_equals_min, bytes)?;
        U24::encode(&self.sequences.0, bytes)?;
        if !self.max_equals_min {
            U24::encode(&self.sequences.1, bytes)?;
        }
        Ok(())
    }

    fn size(&self) -> usize {
        if self.max_equals_min {
            6
        } else {
            9
        }
    }
}

#[derive(Den)]
pub struct Ack {
    pub ack: Acknowledge,
}
impl SystemPacket for Ack {
    const ID: u8 = 0xc0;
}

#[derive(Den)]
pub struct Nack {
    pub nack: Acknowledge,
}
impl SystemPacket for Nack {
    const ID: u8 = 0xa0;
}
