use std::collections::{VecDeque, HashMap};
use std::net::SocketAddr;
use std::time::{Instant, Duration};

use packet_derive::{U24, DenWith};

use crate::Udp;
use crate::fragment::FragmentHeader;
use crate::system_packets::UDP_HEADER_LEN;
use crate::{
    packets::{Frame, Reliability},
    system_packets::{Ack, Nack},
};

pub(crate) struct OutPacket {
    pub frame: Frame,
    pub data: Vec<u8>,
}

impl OutPacket {
    pub fn length(&self) -> usize {
        self.frame.length as usize + Frame::size(self.frame.reliability, self.frame.fragment.is_some())
    }
    pub fn encode(&self) -> std::io::Result<Vec<u8>> {
        let mut frame_bytes = vec![];
        self.frame.encode(&mut frame_bytes)?;
        Ok([frame_bytes,self.data.clone()].concat()) // Fix : don't clone
    }
}

pub(crate) struct SendQueue {
    udp : Udp,
    addr : SocketAddr,

    buffer: VecDeque<OutPacket>,
    sequence: u32,
    mindex : u32,
    oindex : u32,
    sindex : u32,
    fragment_id : u16,

    mtu : usize,

    window_size : usize,

    rto : Duration,

    sent : HashMap<u32,(Vec<OutPacket>,Instant)>,
}

impl SendQueue {
    pub fn new(udp : Udp,addr : SocketAddr,mtu : usize) -> Self {
        Self {
            udp,
            addr,
            buffer: VecDeque::new(),
            sequence: 0,
            mindex : 0,
            oindex : 0,
            sindex : 0,
            fragment_id : 0,
            mtu,
            window_size : 1,
            rto : Duration::from_secs(6),
            sent : HashMap::new()
        }
    }

    pub fn ack(&mut self, ack: Ack) {
        let now = Instant::now();
        for i in ack.ack.sequences.0..ack.ack.sequences.1 + 1 {
            if self.sent.contains_key(&i) {
                let sent_time = self.sent.remove(&i).unwrap().1;
            }
        }
    }

    pub fn nack(&mut self, nack: Nack) {}

    fn resend(&mut self) {

    }

    fn send_out_packet(&mut self,packet : OutPacket) {
        self.buffer.push_back(packet);
    }

    pub fn send(&mut self, bytes: Vec<u8>, reliability: Reliability) {
        let mut frame = Frame {
            reliability,
            length: bytes.len() as u16,
            mindex: 0,
            sindex: 0,
            oindex: 0,
            fragment : None
        };
        if reliability.reliable() {
            frame.mindex = self.mindex;
            self.mindex += 1;
        }
        if reliability.sequenced() {
            frame.sindex = self.sequence;
            self.sindex += 1;
        }
        if reliability.ordered() {
            frame.oindex = self.oindex;
            self.oindex += 1;
        }
        self.send_out_packet(OutPacket { frame, data: bytes })
    }

    pub fn send_ref(&mut self,bytes : &[u8],mut reliability: Reliability) {
        if bytes.len() > self.mtu - UDP_HEADER_LEN as usize - 1 - Frame::size(reliability, false) {
            match reliability {
                Reliability::Unreliable => reliability = Reliability::Reliable,
                Reliability::UnreliableSequenced => reliability = Reliability::ReliableSequenced,
                _ => {}
            }
            let header_size = Frame::size(reliability, true);
            let payload_size = self.mtu - UDP_HEADER_LEN as usize - 1 - header_size;
            let m = bytes.len() % payload_size;
            let mut count = (bytes.len() - m) / payload_size;
            if m != 0 {
                count += 1;
            }

            for i in 0..count {
                let mut length = payload_size;
                if i == count - 1 {
                    length = m;
                }

                let header = FragmentHeader{
                    size: count as u32,
                    id: self.fragment_id,
                    index: i as u32,
                };
                let frame = Frame {
                    reliability,
                    length: length as u16,
                    mindex: self.mindex,
                    sindex: self.sindex,
                    oindex: self.oindex,
                    fragment: Some(header),
                };
                self.mindex += 1;

                let pos = i * payload_size;
                self.send_out_packet(OutPacket { frame, data: bytes[pos..pos + length].to_vec() });
            }
            self.fragment_id += 1;
            if reliability.ordered() {
                self.oindex += 1;
            }
            if reliability.sequenced() {
                self.sindex += 1;
            }
        }
        self.send(bytes.to_vec(), reliability);
    }

    pub async fn tick(&mut self) -> std::io::Result<()> {
        let max_payload_len = self.mtu - UDP_HEADER_LEN as usize;
        let mut break_flag = false;
        for _ in 0..self.window_size {
            let mut packets = vec![];
            let mut length = 0;
            loop {
                if self.buffer.front().is_some() {
                    let packet = self.buffer.front().unwrap();
                    let packet_len = packet.length();
                    if packet_len + length < max_payload_len {
                        packets.push(self.buffer.pop_front().unwrap());
                        length += packet_len;
                    }else {
                        break;
                    }
                }else {
                    break_flag = true;
                    break;
                }
            }

            let mut buff = vec![];
            let mut writer = std::io::Cursor::new(&mut buff);
            U24::encode(&self.sequence, &mut writer)?;
            for packet in packets.iter() {
                buff.append(&mut packet.encode()?);
            }
            self.udp.send_to(&buff, self.addr).await?;
            

            self.sent.insert(self.sequence, (packets,Instant::now()));

            if break_flag {
                break;
            }
        }
        Ok(())
    }

    pub fn get(&mut self) -> Option<Vec<u8>> {
        todo!()
    }
}
