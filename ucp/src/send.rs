use std::collections::{HashMap, VecDeque};
use std::net::SocketAddr;
use std::time::{Duration, Instant};

use packet_derive::{Den, DenWith, U24};

use crate::fragment::FragmentHeader;
use crate::system_packets::UDP_HEADER_LEN;
use crate::Udp;
use crate::{
    packets::{Frame, Reliability},
    system_packets::{Ack, Nack},
};

const DATAGRAM_FLAG: u8 = 0x80;
const NEEDS_B_AND_AS_FLAG: u8 = 0x4;
const CONTINUOUS_SEND_FLAG: u8 = 0x8;
const MAX_RTO: Duration = Duration::from_secs(60);
const MIN_RTO: Duration = Duration::from_secs(1);

type Rtts = (Duration, Duration); // srtt,rttvar

pub(crate) struct OutPacket {
    pub frame: Frame,
    pub data: Vec<u8>,
}

impl OutPacket {
    pub fn length(&self) -> usize {
        self.frame.length as usize
            + Frame::size(self.frame.reliability, self.frame.fragment.is_some())
    }
    pub fn encode(&self) -> std::io::Result<Vec<u8>> {
        let mut frame_bytes = vec![];
        self.frame.encode(&mut frame_bytes)?;
        Ok([frame_bytes, self.data.clone()].concat()) // Fix : don't clone
    }
}

pub(crate) struct SendQueue {
    udp: Udp,
    addr: SocketAddr,

    buffer: VecDeque<OutPacket>,
    resend: VecDeque<OutPacket>,
    sequence: u32,
    mindex: u32,
    oindex: u32,
    sindex: u32,
    fragment_id: u16,

    mtu: usize,

    window_size: usize,

    rto: Duration,
    rtts: Option<Rtts>,

    sent: HashMap<u32, (Vec<OutPacket>, Instant)>,
}

impl SendQueue {
    pub fn new(udp: Udp, addr: SocketAddr, mtu: usize) -> Self {
        Self {
            udp,
            addr,
            buffer: VecDeque::new(),
            resend: VecDeque::new(),
            sequence: 0,
            mindex: 0,
            oindex: 0,
            sindex: 0,
            fragment_id: 0,
            mtu,
            window_size: 1,
            rto: Duration::from_secs(1),
            rtts: None,
            sent: HashMap::new(),
        }
    }

    pub fn ack(&mut self, ack: Ack) {
        let now = Instant::now();

        for i in ack.ack.sequences.0..ack.ack.sequences.1 + 1 {
            if self.sent.contains_key(&i) {
                if i == ack.ack.sequences.1 {
                    let sent_time = self.sent.remove(&i).unwrap().1;
                    let rtt = now.duration_since(sent_time);
                    self.compute_rto(rtt);
                } else {
                    self.sent.remove(&i);
                }
            }
        }
    }

    pub fn nack(&mut self, nack: Nack) {
        for i in nack.nack.sequences.0..nack.nack.sequences.1 + 1 {
            self.resend(i);
        }
    }

    fn compute_rto(&mut self, rtt: Duration) {
        if let Some((srtt, rttvar)) = self.rtts {
            let new_rttvar = rttvar.mul_f32(0.75) + absolute_div(srtt, rtt).mul_f32(0.25);
            let new_srtt = srtt.mul_f32(0.875) + rtt.mul_f32(0.125);
            let mut rto = srtt + 4 * rttvar;
            if rto > MAX_RTO {
                rto = MAX_RTO;
            }
            if rto < MIN_RTO {
                rto = MIN_RTO;
            }
            self.rto = rto;
            self.rtts = Some((new_srtt, new_rttvar));
        } else {
            let srtt = rtt;
            let rttvar = rtt / 2;
            self.rto = srtt + 4 * rttvar;
            self.rtts = Some((srtt, rttvar));
        }
    }

    fn resend(&mut self, seq: u32) {
        if let Some((set, _)) = self.sent.remove(&seq) {
            for packet in set {
                if packet.frame.reliability.reliable() {
                    self.resend.push_back(packet);
                }
            }
        }
    }

    fn send_out_packet(&mut self, packet: OutPacket) {
        self.buffer.push_back(packet);
    }

    pub fn send(&mut self, bytes: Vec<u8>, reliability: Reliability) {
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
        self.send_out_packet(OutPacket { frame, data: bytes })
    }

    pub fn send_ref(&mut self, bytes: &[u8], mut reliability: Reliability) {
        if bytes.len() > self.mtu - UDP_HEADER_LEN as usize - 1 - Frame::size(reliability, false) {
            match reliability {
                Reliability::Unreliable => reliability = Reliability::Reliable,
                Reliability::UnreliableSequenced => reliability = Reliability::ReliableSequenced,
                _ => {}
            }
            let header_size = Frame::size(reliability, true);
            let payload_size = self.mtu - UDP_HEADER_LEN as usize - 1 - 3 - header_size;
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

                let header = FragmentHeader {
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
                self.send_out_packet(OutPacket {
                    frame,
                    data: bytes[pos..pos + length].to_vec(),
                });
            }
            self.fragment_id += 1;
            if reliability.ordered() {
                self.oindex += 1;
            }
            if reliability.sequenced() {
                self.sindex += 1;
            }
            return;
        }
        self.send(bytes.to_vec(), reliability);
    }

    pub async fn tick(&mut self) -> std::io::Result<()> {
        self.check_timout();
        let max_payload_len = self.mtu - UDP_HEADER_LEN as usize - 4;
        let mut break_flag = false;
        for _ in 0..self.window_size {
            let mut packets = vec![];
            let mut length = 0;
            let mut is_fragment = false;
            loop {
                //////////////////////////////////////////////////////////////////////////////////////
                if self.resend.front().is_some() {
                    let packet = self.resend.front().unwrap();
                    if packet.frame.fragment.is_some() {
                        is_fragment = true;
                        packets.push(self.resend.pop_front().unwrap());
                        break;
                    }
                    let packet_len = packet.length();
                    if packet_len + length < max_payload_len {
                        packets.push(self.resend.pop_front().unwrap());
                        length += packet_len;
                    } else {
                        break;
                    }
                }
                //////////////////////////////////////////////////////////////////////////////////////
                if self.buffer.front().is_some() {
                    let packet = self.buffer.front().unwrap();
                    let packet_len = packet.length();
                    if packet.frame.fragment.is_some() {
                        is_fragment = true;
                        packets.push(self.resend.pop_front().unwrap());
                        break;
                    }
                    if packet_len + length < max_payload_len {
                        packets.push(self.buffer.pop_front().unwrap());
                        length += packet_len;
                    } else {
                        break;
                    }
                } else {
                    break_flag = true;
                    break;
                }
            }

            let mut buff = vec![];
            let mut writer = std::io::Cursor::new(&mut buff);
            let id = if is_fragment {
                DATAGRAM_FLAG | NEEDS_B_AND_AS_FLAG | CONTINUOUS_SEND_FLAG
            } else {
                DATAGRAM_FLAG | NEEDS_B_AND_AS_FLAG
            };
            u8::encode(&id, &mut writer)?;
            U24::encode(&self.sequence, &mut writer)?;
            for packet in packets.iter() {
                buff.append(&mut packet.encode()?);
            }
            self.udp.send_to(&buff, self.addr).await?;

            self.sent.insert(self.sequence, (packets, Instant::now()));
            self.sequence += 1;

            if break_flag {
                break;
            }
        }
        Ok(())
    }

    fn check_timout(&mut self) {
        let now = Instant::now();
        let resends: Vec<u32> = self
            .sent
            .iter()
            .filter(|x| now.duration_since(x.1 .1) > self.rto)
            .map(|x| *x.0)
            .collect();
        for seq in resends {
            self.resend(seq);
        }
    }
}

fn absolute_div(p: Duration, o: Duration) -> Duration {
    if p > o {
        p - o
    } else {
        o - p
    }
}
