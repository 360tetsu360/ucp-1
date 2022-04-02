use crate::{
    cubic::Cubic,
    packets::{FragmentHeader, Frame, Reliability},
    system_packets::{Ack, Nack},
    Udp,
};
use packet_derive::*;
use std::{
    cmp,
    collections::VecDeque,
    net::SocketAddr,
    time::{Duration, Instant},
};

const UDP_HEADER: usize = 32;
const DATAGRAM_FLAG: u8 = 0x80;
const NEEDS_B_AND_AS_FLAG: u8 = 0x4;
const MAX_RTO: Duration = Duration::from_secs(10);
const MIN_RTO: Duration = Duration::from_millis(1000);

#[derive(Clone)]
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
        let mut writer = std::io::Cursor::new(&mut frame_bytes);
        self.frame.encode(&mut writer)?;
        Ok([frame_bytes, self.data.clone()].concat()) // Fix : don't clone
    }
}

struct SentConf {
    pub sequence: u32,
    pub time: Instant,
}

const ALPHA : f32 = 0.125;
const BETA : f32 = 0.25;
const K : u32 = 1;

struct Rtts {
    pub srtt: Duration,
    pub rttvar: Duration,
}

struct Rto {
    pub rto: Duration,
    rtts: Option<Rtts>,
}

impl Rto {
    pub fn new() -> Self {
        Self {
            rto: Duration::from_secs(1),
            rtts: None,
        }
    }

    pub fn compute(&mut self, rtt: Duration) {
        if let Some(rtts) = self.rtts.as_mut() {
            let new_rttvar = rtts.rttvar.mul_f32(1. - BETA) + absolute_div(rtts.srtt, rtt).mul_f32(BETA);
            let new_srtt = rtts.srtt.mul_f32(1. - ALPHA) + rtt.mul_f32(ALPHA);
            let mut rto = rtts.srtt + K * rtts.rttvar;
            rto = cmp::max(cmp::min(rto, MAX_RTO), MIN_RTO);
            self.rto = rto;
            rtts.srtt = new_srtt;
            rtts.rttvar = new_rttvar;
        } else {
            let srtt = rtt;
            let rttvar = rtt / 2;
            let rto = srtt + K * rttvar;
            self.rto = cmp::max(cmp::min(rto, MAX_RTO), MIN_RTO);
            self.rtts = Some(Rtts { srtt, rttvar });
        }
    }
}

pub(crate) struct DatagramSender {
    udp: Udp,
    address: SocketAddr,
    max_payload_len: usize, //MTU size - 32(UDP Header) - 1(ID) - 3(Sequence Number)

    buffer: VecDeque<(Vec<OutPacket>, Option<SentConf>, Option<u32>)>,
    sent: Vec<u32>,

    cubic: Cubic,

    rto: Rto,

    sequence: u32,

    is_congestion : bool,

    mindex: u32,
    sindex: u32,
    oindex: u32,

    fragment_id: u16,

    nodelay: bool,
}

impl DatagramSender {
    pub fn new(udp: Udp, address: SocketAddr, mtu: usize) -> Self {
        Self {
            udp,
            address,
            max_payload_len: mtu - UDP_HEADER - 4,
            buffer: VecDeque::new(),
            sent: vec![],
            cubic: Cubic::new(mtu),
            rto: Rto::new(),
            sequence: 0,
            is_congestion : false,
            mindex: 0,
            sindex: 0,
            oindex: 0,
            fragment_id: 0,
            nodelay: false,
        }
    }

    fn sendable_packet_index(&self) -> Option<usize> {
        self.buffer.iter().position(|stack| stack.1.is_none())
    }

    async fn send_next(&mut self) -> std::io::Result<()> {
        if self.sent.len() >= self.cubic.cwnd as usize {
            return Ok(());
        }

        if let Some(next_packet) = self.sendable_packet_index() {
            if !self.nodelay {
                // Skip if this is the only packet that can be sent
                if next_packet + 1 == self.buffer.len() && !self.sent.is_empty() {
                    return Ok(());
                }
            }

            let mut buff = vec![];
            let mut writer = std::io::Cursor::new(&mut buff);
            let id = DATAGRAM_FLAG | NEEDS_B_AND_AS_FLAG;

            u8::encode(&id, &mut writer)?;
            U24::encode(&self.sequence, &mut writer)?;

            for out in self.buffer[next_packet].0.iter() {
                buff.append(&mut out.encode()?);
            }
            self.udp.send_to(&buff, self.address).await?;

            self.buffer[next_packet].1 = Some(SentConf {
                sequence: self.sequence,
                time: Instant::now(),
            });

            self.sent.push(self.sequence);

            self.sequence += 1;
        }
        Ok(())
    }

    fn push_outpacket(&mut self, out: OutPacket) {
        if let Some((dst, None, None)) = self.buffer.back_mut() {
            let length = dst.iter().map(|out| out.length()).sum::<usize>();

            if length + out.length() > self.max_payload_len {
                self.buffer.push_back((vec![out], None, None))
            } else {
                dst.push(out);
            }
        } else {
            self.buffer.push_back((vec![out], None, None));
        }
    }

    pub async fn send(
        &mut self,
        bytes: Vec<u8>,
        reliability: Reliability,
    ) -> std::io::Result<()> {
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

        let out_packet = OutPacket { frame, data: bytes };
        self.push_outpacket(out_packet);
        self.send_next().await?;
        Ok(())
    }

    pub async fn send_ref(
        &mut self,
        bytes: &[u8],
        mut reliability: Reliability,
    ) -> std::io::Result<()> {
        if bytes.len() > self.max_payload_len - Frame::size(reliability, false) {
            match reliability {
                Reliability::Unreliable => reliability = Reliability::Reliable,
                Reliability::UnreliableSequenced => reliability = Reliability::ReliableSequenced,
                _ => {}
            }
            let header_size = Frame::size(reliability, true);
            let payload_size = self.max_payload_len - header_size;
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
                self.push_outpacket(OutPacket {
                    frame,
                    data: bytes[pos..pos + length].to_vec(),
                });
                self.send_next().await?;
            }
            self.fragment_id += 1;
            if reliability.ordered() {
                self.oindex += 1;
            }
            if reliability.sequenced() {
                self.sindex += 1;
            }
            return Ok(());
        }
        self.send(bytes.to_vec(), reliability).await?;
        Ok(())
    }

    pub async fn ack(&mut self, ack: Ack) -> std::io::Result<()> {
        let mut ack_cnt = 0;
        let mut sent = None;
        for seq in ack.ack.sequences.0..ack.ack.sequences.1 + 1 {
            if self.sent.contains(&seq) {
                ack_cnt += 1;

                let index = self.sent.iter().position(|x| *x == seq).unwrap();
                self.sent.remove(index);

                let index = self
                    .buffer
                    .iter()
                    .position(|(_, x, _)| x.as_ref().unwrap().sequence == seq)
                    .unwrap();
                
                let sent_packet = self.buffer.remove(index).unwrap();
                sent = Some(sent_packet.1.as_ref().unwrap().time);
                
                if let Some(_) = sent_packet.2 {
                    if self.buffer.iter().all(|p|p.2.is_none()) {
                        self.is_congestion = false;
                    }
                }

                self.send_next().await?;
            }
        }

        if let Some(time) = sent {
            let rtt = Instant::now().duration_since(time);
            self.cubic.on_ack(ack_cnt, rtt);

            self.rto.compute(rtt);
        }

        Ok(())
    }

    pub async fn nack(&mut self, nack: Nack) -> std::io::Result<()> {
        let mut sent = None;
        for seq in nack.nack.sequences.0..nack.nack.sequences.1 + 1 {
            if self.sent.contains(&seq) {
                let index = self.sent.iter().position(|x| *x == seq).unwrap();
                self.sent.remove(index);

                let index = self
                    .buffer
                    .iter()
                    .position(|(_, x, _)| x.as_ref().unwrap().sequence == seq)
                    .unwrap();

                while let Some(out) = self.buffer[index].0.pop() {
                    if out.frame.reliability.reliable() {
                        self.buffer[index].0.insert(0, out);
                    }
                }
                sent = Some(self.buffer[index].1.as_ref().unwrap().time);
                self.buffer[index].1 = None;
            }
        }
        
        if let Some(time) = sent {
            self.cubic.on_congestion_event(time);

            if self.cubic.cwnd > self.sent.len() as u32 {
                for _ in self.sent.len() as u32..self.cubic.cwnd {
                    self.send_next().await?;
                }
            }
        }

        Ok(())
    }

    pub async fn tick(&mut self) -> std::io::Result<bool> {
        let now = Instant::now();
        let timeouted = self
            .buffer
            .iter_mut()
            .filter(|p| p.1.is_some())
            .filter(|(_, conf, _)| now.duration_since(conf.as_ref().unwrap().time) > self.rto.rto);

        let mut sent = None;

        for (stack, conf, count) in timeouted {
            let mut resends = vec![];
            while let Some(out) = stack.pop() {
                if out.frame.reliability.reliable() {
                    resends.push(out);
                }
            }
            *stack = resends;

            sent = Some(conf.as_ref().unwrap().time);

            let seq = conf.as_ref().unwrap().sequence;
            let index = self.sent.iter().position(|x| *x == seq).unwrap();
            self.sent.remove(index);


            *conf = None;

            if let Some(count) = count {
                if *count >= 4 {
                    // connection lost;
                    return Ok(true);
                }
                *count += 1;
            }
        }

        if let Some(time) = sent {
            if !self.is_congestion {
                // congestion event.
                self.cubic.on_congestion_event(time);
                self.rto.rto = cmp::min(self.rto.rto * 2,MAX_RTO);
                self.is_congestion = true;
            }

            if self.cubic.cwnd > self.sent.len() as u32 {
                for _ in self.sent.len() as u32..self.cubic.cwnd {
                    self.send_next().await?;
                }
            }
        }

        Ok(false)
    }

    pub fn set_nodelay(&mut self, nodelay: bool) {
        self.nodelay = nodelay;
    }

    pub fn nodelay(&self) -> bool {
        self.nodelay
    }
}

fn absolute_div(p: Duration, o: Duration) -> Duration {
    if p > o {
        p - o
    } else {
        o - p
    }
}
