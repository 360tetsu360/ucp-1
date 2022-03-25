use std::net::SocketAddr;

use crate::{
    packets::Reliability,
    system_packets::{Ack, Nack},
    Udp,
};

const UDP_HEADER: usize = 32;
pub(crate) struct DatagramSender {
    udp: Udp,
    address: SocketAddr,
    max_payload_len: usize, //MTU size - 32(UDP Header) - 1(ID) - 3(Sequence Number)

    nodelay: bool,
}

impl DatagramSender {
    pub fn new(udp: Udp, address: SocketAddr, mtu: usize) -> Self {
        Self {
            udp,
            address,
            max_payload_len: mtu - UDP_HEADER - 4,
            nodelay: false,
        }
    }

    pub async fn tick(&mut self) -> std::io::Result<()> {
        Ok(())
    }

    pub async fn send(&mut self, bytes: Vec<u8>, reliability: Reliability) -> std::io::Result<()> {
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
