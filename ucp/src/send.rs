use crate::{
    cubic::Cubic,
    packets::{Frame, Reliability,FragmentHeader},
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
const MIN_RTO: Duration = Duration::from_secs(1);

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
            let new_rttvar = rtts.rttvar.mul_f32(0.75) + absolute_div(rtts.srtt, rtt).mul_f32(0.25);
            let new_srtt = rtts.srtt.mul_f32(0.875) + rtt.mul_f32(0.125);
            let mut rto = rtts.srtt + 4 * rtts.rttvar;
            rto = cmp::max(cmp::min(rto, MAX_RTO), MIN_RTO);
            self.rto = rto;
            rtts.srtt = new_srtt;
            rtts.rttvar = new_rttvar;
        } else {
            let srtt = rtt;
            let rttvar = rtt / 2;
            self.rto = srtt + 4 * rttvar;
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
                if next_packet + 1 == self.buffer.len() {
                    return Ok(());
                }

                if !self.sent.is_empty() {
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

    pub async fn send(&mut self, bytes: Vec<u8>, reliability: Reliability) -> std::io::Result<()> {
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
        for seq in ack.ack.sequences.0..ack.ack.sequences.1 {
            if self.sent.contains(&seq) {
                ack_cnt += 1;

                let index = self.sent.iter().position(|x| *x == seq).unwrap();
                self.sent.remove(index);

                let index = self
                    .buffer
                    .iter()
                    .position(|(_, x, _)| x.as_ref().unwrap().sequence == seq)
                    .unwrap();
                sent = Some(self.buffer.remove(index).unwrap().1.unwrap().time);

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
        for seq in nack.nack.sequences.0..nack.nack.sequences.1 {
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
                self.buffer[index].1 = None;
            }
        }

        // TODO : update congestion window
        // TODO : send_next()

        Ok(())
    }

    pub async fn tick(&mut self) -> std::io::Result<bool> {
        let now = Instant::now();
        let timeouted = self
            .buffer
            .iter_mut()
            .filter(|p| p.1.is_some())
            .filter(|(_, conf, _)| now.duration_since(conf.as_ref().unwrap().time) > self.rto.rto);

        for (stack, conf, count) in timeouted {
            while let Some(out) = stack.pop() {
                if out.frame.reliability.reliable() {
                    stack.insert(0, out);
                }
            }
            *conf = None;

            if let Some(count) = count {
                if *count >= 5 {
                    // connection lost;
                    return Ok(true);
                }
                *count += 1;
            }
        }

        // TODO : update congestion window and rto
        // TODO : send_next()

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
