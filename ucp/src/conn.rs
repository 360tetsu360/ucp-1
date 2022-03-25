use packet_derive::*;
use std::io::Cursor;
use std::net::SocketAddr;
use tokio::sync::mpsc;

use crate::packets::*;
use crate::receive::ReceiveQueue;
use crate::send::DatagramSender;
use crate::system_packets::*;
use crate::Udp;

const DATAGRAM_FLAG: u8 = 0x80;
const ACK_FLAG: u8 = 0x40;
const NACK_FLAG: u8 = 0x20;

pub(crate) struct Conn {
    address: SocketAddr,
    udp: Udp,
    receive: ReceiveQueue,
    send: DatagramSender,
    received_sender: tokio::sync::mpsc::Sender<Vec<u8>>,
}

impl Conn {
    pub fn new(address: SocketAddr, mtu: usize, udp: Udp, sender: mpsc::Sender<Vec<u8>>) -> Self {
        Self {
            address,
            udp: udp.clone(),
            receive: ReceiveQueue::new(),
            send: DatagramSender::new(udp, address, mtu),
            received_sender: sender,
        }
    }

    pub async fn handle(&mut self, bytes: &[u8]) -> std::io::Result<()> {
        let mut reader = Cursor::new(bytes);
        let id = u8::decode(&mut reader)?;

        if id & ACK_FLAG != 0 {
            self.handle_ack(bytes).await?;
        } else if id & NACK_FLAG != 0 {
            self.handle_nack(bytes).await?;
        } else if id & DATAGRAM_FLAG != 0 {
            self.handle_datagram(&bytes[1..]).await?;
        }
        Ok(())
    }
    async fn handle_datagram(&mut self, bytes: &[u8]) -> std::io::Result<()> {
        let mut reader = Cursor::new(bytes);
        let sequence = U24::decode(&mut reader)?;
        while reader.position() < bytes.len() as u64 {
            let frame = Frame::decode(&mut reader)?;
            let data = &reader.get_ref()
                [reader.position() as usize..reader.position() as usize + frame.length as usize];
            reader.set_position(reader.position() + frame.length as usize as u64);
            self.handle_packet(frame, data).await?;
        }
        self.receive.received(sequence);
        Ok(())
    }
    async fn handle_ack(&mut self, bytes: &[u8]) -> std::io::Result<()> {
        let ack: Ack = decode_syspacket(bytes)?;
        self.send.ack(ack).await?;
        Ok(())
    }
    async fn handle_nack(&mut self, bytes: &[u8]) -> std::io::Result<()> {
        let nack: Nack = decode_syspacket(bytes)?;
        self.send.nack(nack).await?;
        Ok(())
    }
    async fn handle_packet(&mut self, frame: Frame, bytes: &[u8]) -> std::io::Result<()> {
        if frame.fragment.is_some() {
            if let Some(data) = self.receive.fragmented(frame, bytes) {
                self.handle_incoming_packet(data).await?;
            }
        } else if !frame.reliability.sequenced() && !frame.reliability.ordered() {
            self.handle_incoming_packet(bytes.to_vec()).await?;
        } else {
            self.receive.ordered(frame, bytes);
            while let Some(data) = self.receive.next_ordered() {
                self.handle_incoming_packet(data).await?;
            }
        }
        Ok(())
    }

    async fn handle_incoming_packet(&mut self, bytes: Vec<u8>) -> std::io::Result<()> {
        let mut reader = Cursor::new(&bytes[..]);
        match u8::decode(&mut reader)? {
            ConnectedPing::ID => {}
            ConnectedPong::ID => {}
            _ => match self.received_sender.send(bytes).await {
                Ok(_) => {}
                Err(_) => todo!(),
            },
        }
        Ok(())
    }

    async fn send_bytes(&self, bytes: &[u8]) -> std::io::Result<()> {
        self.udp.send_to(bytes, self.address).await?;
        Ok(())
    }

    pub async fn send_syspacket<T: SystemPacket>(
        &mut self,
        packet: T,
        reliability: Reliability,
    ) -> std::io::Result<()> {
        let mut bytes = vec![];
        encode_syspacket(packet, &mut bytes)?;
        self.send.send(bytes, reliability).await?;
        Ok(())
    }

    async fn send_ack(&self, seqs: (u32, u32)) -> std::io::Result<()> {
        let ack = Ack {
            ack: Acknowledge {
                record_count: 1,
                max_equals_min: seqs.0 == seqs.1,
                sequences: seqs,
            },
        };
        let mut bytes = vec![];
        encode_syspacket(ack, &mut bytes)?;
        self.send_bytes(&bytes[..]).await
    }

    async fn send_nack(&self, seqs: (u32, u32)) -> std::io::Result<()> {
        let nack = Nack {
            nack: Acknowledge {
                record_count: 1,
                max_equals_min: seqs.0 == seqs.1,
                sequences: seqs,
            },
        };
        let mut bytes = vec![];
        encode_syspacket(nack, &mut bytes)?;
        self.send_bytes(&bytes[..]).await
    }

    pub async fn send(&mut self, bytes: &[u8], reliability: Reliability) -> std::io::Result<()> {
        self.send.send_ref(bytes, reliability).await
    }

    pub async fn update(&mut self) -> std::io::Result<()> {
        self.flush_ack().await?;
        self.flush_nack().await?;
        self.send.tick().await?;
        Ok(())
    }

    async fn flush_ack(&mut self) -> std::io::Result<()> {
        while let Some(seqs) = self.receive.get_ack() {
            self.send_ack(seqs).await?
        }
        Ok(())
    }

    async fn flush_nack(&mut self) -> std::io::Result<()> {
        while let Some(nacks) = self.receive.get_nack() {
            self.send_nack(nacks).await?
        }
        Ok(())
    }

    pub fn set_nodelay(&mut self, nodelay: bool) {
        self.send.set_nodelay(nodelay);
    }

    pub fn nodelay(&self) -> bool {
        self.send.nodelay()
    }
}
