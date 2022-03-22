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
const MAX_RTO: Duration = Duration::from_secs(10);
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
    mtu: usize,

    //The frames to be sent are stacked here.
    buffer: VecDeque<OutPacket>,
    //Framesets waiting to be acked are stacked here.
    sent: HashMap<u32, (Instant, Vec<OutPacket>)>,

    //Recovery time objective. If the Ack is not received after this time, it is assumed that the packet was discarded.
    //
    rto: Duration,
    rtts: Option<Rtts>,
    //Number of Framesets that can be sent at one time
    cwnd: usize,

    nodelay: bool,

    pub timeout: bool,

    sequence: u32,
    mindex: u32,
    oindex: u32,
    sindex: u32,
    fragment_id: u16,
}

impl SendQueue {
    pub fn new(udp: Udp, addr: SocketAddr, mtu: usize) -> Self {
        Self {
            mtu,
            udp,
            addr,
            buffer: VecDeque::new(),
            sent: HashMap::new(),
            rto: Duration::from_secs(1),
            rtts: None,
            cwnd: todo!(),
            nodelay: true,
            timeout: false,
            sequence: 0,
            mindex: 0,
            oindex: 0,
            sindex: 0,
            fragment_id: 0,
        }
    }

    pub fn set_nodelay(&mut self, nodelay: bool) {
        self.nodelay = nodelay;
    }

    pub fn nodelay(&self) -> bool {
        self.nodelay
    }

    pub async fn ack(&mut self, ack: Ack) -> std::io::Result<()> {
        if ack.ack.sequences.0 < ack.ack.sequences.1 + 1 {
            for seq in ack.ack.sequences.0..ack.ack.sequences.1 + 1 {
                if seq == ack.ack.sequences.1 && self.sent.contains_key(&seq) {
                    let now = Instant::now();
                    let rtt = self.sent.get(&seq).unwrap().0.duration_since(now);
                    self.compute_rto(rtt);
                }
                self.sent.remove(&seq);
                self.send_next().await?;
            }
        }
        Ok(())
    }

    pub async fn nack(&mut self, nack: Nack) -> std::io::Result<()> {
        if nack.nack.sequences.0 < nack.nack.sequences.1 + 1 {
            for seq in nack.nack.sequences.0..nack.nack.sequences.1 + 1 {
                self.resend_nack(seq).await?;
            }
        }
        Ok(())
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

    async fn resend_nack(&mut self, seq: u32) -> std::io::Result<()> {
        if let Some((_, sent)) = self.sent.remove(&seq) {
            //dont remove
            for out in sent {
                if out.frame.reliability.reliable() {
                    self.buffer.push_front(out);
                }
            }
            self.send_next().await?;
        }
        Ok(())
    }

    fn get_next(&mut self) -> Vec<OutPacket> {
        //sendable packets , is fragment
        let max_payload_len = self.mtu - UDP_HEADER_LEN as usize - 4;
        let mut packets = vec![];
        let mut length = 0;

        loop {
            if self.buffer.front().is_some() {
                let packet = self.buffer.front().unwrap();
                let packet_len = packet.length();
                if packet_len + length < max_payload_len {
                    packets.push(self.buffer.pop_front().unwrap());
                    length += packet_len;
                } else {
                    break;
                }
            } else {
                break;
            }
        }
        packets
    }

    async fn send_next(&mut self) -> std::io::Result<()> {
        if self.sent.len() >= self.cwnd {
            return Ok(());
        }

        if self.buffer.is_empty() {
            return Ok(());
        }

        if self.nodelay {
            let sendable = self.get_next();
            self.send_frameset(sendable).await?;
        } else {
            if self.sent.is_empty() {
                let sendable = self.get_next();
                self.send_frameset(sendable).await?;
                return Ok(());
            }
            let size: usize = self.buffer.iter().map(|x| x.length()).sum();
            if size > self.mtu - UDP_HEADER_LEN as usize - 4 {
                let sendable = self.get_next();
                self.send_frameset(sendable).await?;
            }
        }
        Ok(())
    }

    async fn send_frameset(&mut self, frames: Vec<OutPacket>) -> std::io::Result<()> {
        let mut buff = vec![];
        let mut writer = std::io::Cursor::new(&mut buff);
        let id = DATAGRAM_FLAG | NEEDS_B_AND_AS_FLAG;

        u8::encode(&id, &mut writer)?;
        U24::encode(&self.sequence, &mut writer)?;
        for packet in frames.iter() {
            buff.append(&mut packet.encode()?);
        }
        self.udp.send_to(&buff, self.addr).await?;

        self.sent.insert(self.sequence, (Instant::now(), frames));
        self.sequence += 1;
        Ok(())
    }

    async fn send_out_packet(&mut self, packet: OutPacket) -> std::io::Result<()> {
        self.buffer.push_back(packet);
        self.send_next().await?;
        Ok(())
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
        self.send_out_packet(OutPacket { frame, data: bytes }).await
    }

    pub async fn send_ref(
        &mut self,
        bytes: &[u8],
        mut reliability: Reliability,
    ) -> std::io::Result<()> {
        if bytes.len() > self.mtu - UDP_HEADER_LEN as usize - 4 - Frame::size(reliability, false) {
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
                })
                .await?;
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

    pub async fn tick(&mut self) -> std::io::Result<()> {
        self.check_timout().await
    }

    async fn check_timout(&mut self) -> std::io::Result<()> {
        let now = Instant::now();
        let resends: Vec<u32> = self
            .sent
            .iter()
            .filter(|x| now.duration_since(x.1 .0) > self.rto)
            .map(|x| *x.0)
            .collect();

        if !resends.is_empty() {
            self.rto *= 2;
            if self.rto > MAX_RTO {
                self.rto = MAX_RTO;
            }
        }
        for seq in resends {
            //self.resend(seq).await?;
            //
        }
        Ok(())
    }
}

fn absolute_div(p: Duration, o: Duration) -> Duration {
    if p > o {
        p - o
    } else {
        o - p
    }
}
