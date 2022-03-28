use std::net::SocketAddr;
use packet_derive::*;
use crate::{
    packets::{Reliability, Frame},
    system_packets::{Ack, Nack},
    Udp,
};

const UDP_HEADER: usize = 32;
const DATAGRAM_FLAG: u8 = 0x80;
const NEEDS_B_AND_AS_FLAG: u8 = 0x4;

pub(crate) struct DatagramSender {
    udp: Udp,
    address: SocketAddr,
    max_payload_len: usize, //MTU size - 32(UDP Header) - 1(ID) - 3(Sequence Number)

    sequence : u32,

    mindex : u32,
    sindex : u32,
    oindex : u32,

    nodelay: bool,
}

impl DatagramSender {
    pub fn new(udp: Udp, address: SocketAddr, mtu: usize) -> Self {
        Self {
            udp,
            address,
            max_payload_len: mtu - UDP_HEADER - 4,
            sequence : 0,
            mindex : 0,
            sindex : 0,
            oindex : 0,
            nodelay: false,
        }
    }

    pub async fn tick(&mut self) -> std::io::Result<()> {
        Ok(())
    }

    pub async fn send(&mut self, mut bytes: Vec<u8>, reliability: Reliability) -> std::io::Result<()> {
        let mut frame = Frame {
            reliability,
            length: bytes.len() as u16,
            mindex: 0,
            sindex: 0,
            oindex: 0,
            fragment: None,
        };
        if reliability.reliable() {
            frame.mindex = self.mindex;
            self.mindex += 1;
        }
        if reliability.sequenced() {
            frame.sindex = self.sindex;
            self.sindex += 1;
        }
        if reliability.ordered() {
            frame.oindex = self.oindex;
            self.oindex += 1;
        }

        let mut buff = vec![];
        let mut writer = std::io::Cursor::new(&mut buff);
        let id = DATAGRAM_FLAG | NEEDS_B_AND_AS_FLAG;

        u8::encode(&id, &mut writer)?;
        U24::encode(&self.sequence, &mut writer)?;
        frame.encode(&mut writer)?;
        buff.append(&mut bytes);
        self.udp.send_to(&buff, self.address).await?;

        self.sequence += 1;
        Ok(())
    }

    pub async fn send_ref(
        &mut self,
        bytes: &[u8],
        reliability: Reliability,
    ) -> std::io::Result<()> {
        Ok(())
    }

    pub async fn ack(&mut self, ack: Ack) -> std::io::Result<()> {
        Ok(())
    }

    pub async fn nack(&mut self, nack: Nack) -> std::io::Result<()> {
        Ok(())
    }

    pub fn set_nodelay(&mut self, nodelay: bool) {
        self.nodelay = nodelay;
    }

    pub fn nodelay(&self) -> bool {
        self.nodelay
    }
}
