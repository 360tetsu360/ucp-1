use packet_derive::*;
use std::io::Cursor;

use crate::system_packets::*;
use crate::packets::*;

const DATAGRAM_FLAG: u8 = 0x80;
const ACK_FLAG: u8 = 0x40;
const NACK_FLAG: u8 = 0x20;

pub struct Conn;

impl Conn {
    pub fn new() -> Self {
        Self
    }

    pub async fn handle(&mut self, bytes: &[u8]) -> std::io::Result<()> {
        let mut reader = Cursor::new(bytes);
        let id = u8::decode(&mut reader)?;

        if id & ACK_FLAG != 0 {
            self.handle_ack(bytes)?;
        } else if id & NACK_FLAG != 0 {
            self.handle_nack(bytes)?;
        } else if id & DATAGRAM_FLAG != 0 {
            self.handle_datagram(bytes)?;
        }
        Ok(())
    }
    fn handle_datagram(&mut self, bytes: &[u8]) -> std::io::Result<()> {
        let mut reader = Cursor::new(&bytes[1..]);
        let sequence = U24::decode(&mut reader)?;
        while reader.position() < bytes.len() as u64 {
            let frame = Frame::decode(&mut reader)?;
            self.handle_packet(frame)?;
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
    fn handle_packet(&mut self,frame : Frame) -> std::io::Result<()> {
        
        Ok(())
    }

    pub fn update(&mut self) {
        dbg!("a");
    }
}
