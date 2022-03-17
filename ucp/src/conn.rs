use packet_derive::*;
use std::io::Cursor;
use std::net::SocketAddr;

use crate::packets::*;
use crate::receive::ReceiveQueue;
use crate::system_packets::*;
use crate::time::get_time;
use crate::Udp;

const DATAGRAM_FLAG: u8 = 0x80;
const ACK_FLAG: u8 = 0x40;
const NACK_FLAG: u8 = 0x20;

pub struct Conn {
    addr: SocketAddr,
    socket: Udp,
    receive: ReceiveQueue,
}

impl Conn {
    pub fn new(addr: SocketAddr, socket: Udp) -> Self {
        Self {
            addr,
            socket,
            receive: ReceiveQueue::new(),
        }
    }

    pub async fn handle(&mut self, bytes: &[u8]) -> std::io::Result<()> {
        let mut reader = Cursor::new(bytes);
        let id = u8::decode(&mut reader)?;

        if id & ACK_FLAG != 0 {
            self.handle_ack(bytes)?;
        } else if id & NACK_FLAG != 0 {
            self.handle_nack(bytes)?;
        } else if id & DATAGRAM_FLAG != 0 {
            self.handle_datagram(&bytes[1..]).await?;
        }
        Ok(())
    }
    async fn handle_datagram(&mut self, bytes: &[u8]) -> std::io::Result<()> {
        let mut reader = Cursor::new(bytes);
        let sequence = U24::decode(&mut reader)?;
        self.receive.received(sequence);
        while reader.position() < bytes.len() as u64 {
            let frame = Frame::decode(&mut reader)?;
            let data = &reader.get_ref()
                [reader.position() as usize..reader.position() as usize + frame.length as usize];
            reader.set_position(reader.position() + frame.length as usize as u64);
            self.handle_packet(frame, data).await?;
        }
        Ok(())
    }
    fn handle_ack(&mut self, bytes: &[u8]) -> std::io::Result<()> {
        let ack: Ack = decode_syspacket(bytes)?;
        Ok(())
    }
    fn handle_nack(&mut self, bytes: &[u8]) -> std::io::Result<()> {
        let nack: Nack = decode_syspacket(bytes)?;
        Ok(())
    }
    async fn handle_packet(&mut self, frame: Frame, bytes: &[u8]) -> std::io::Result<()> {
        if frame.fragment.is_some() {
            if let Some(data) = self.receive.fragmented(frame, bytes) {
                self.handle_incoming_packet(&data[..]).await?;
            }
        } else if !frame.reliability.sequenced() && !frame.reliability.ordered() {
            self.handle_incoming_packet(bytes).await?;
        } else {
            self.receive.ordered(frame, bytes);
            while let Some(data) = self.receive.next_ordered() {
                self.handle_incoming_packet(&data[..]).await?;
            }
        }
        Ok(())
    }
    async fn handle_incoming_packet(&mut self, bytes: &[u8]) -> std::io::Result<()> {
        let mut reader = std::io::Cursor::new(bytes);
        match u8::decode(&mut reader)? {
            ConnectionRequest::ID => self.handle_connection_request(bytes).await?,
            _ => {}
        }
        Ok(())
    }
    async fn handle_connection_request(&mut self, bytes: &[u8]) -> std::io::Result<()> {
        let connection_request: ConnectionRequest = decode_syspacket(bytes)?;
        if connection_request.use_encryption {
            todo!();
        }

        let accepted = ConnectionRequestAccepted {
            client_address: self.addr,
            system_index: 0,
            request_timestamp: connection_request.time,
            accepted_timestamp: get_time(),
        };
        println!("Connected!");
        Ok(())
    }

    async fn send_bytes(&self, bytes: &[u8]) -> std::io::Result<()> {
        self.socket.send_to(bytes, self.addr).await?;
        Ok(())
    }

    async fn send_syspacket<T: SystemPacket>(
        &self,
        packet: T,
        reliability: Reliability,
    ) -> std::io::Result<()> {
        todo!();
        let mut bytes = vec![];
        encode_syspacket(packet, &mut bytes)?;
        self.send_bytes(&bytes[..]).await
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

    pub async fn update(&mut self) -> std::io::Result<()> {
        self.flush_ack().await?;
        self.flush_nack().await
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
}
