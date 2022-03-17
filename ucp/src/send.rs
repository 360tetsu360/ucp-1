use std::collections::VecDeque;

use crate::{
    packets::{Frame, Reliability},
    system_packets::{Ack, Nack},
};

pub(crate) struct OutPacket {
    pub frame: Frame,
    pub data: Vec<u8>,
}

pub(crate) struct SendQueue {
    buffer: VecDeque<OutPacket>,
    sequence: u32,
}

impl SendQueue {
    pub fn new() -> Self {
        Self {
            buffer: VecDeque::new(),
            sequence: 0,
        }
    }

    pub fn ack(&mut self, ack: Ack) {}

    pub fn nack(&mut self, nack: Nack) {}

    pub fn send(&mut self, bytes: Vec<u8>, reliability: Reliability) -> std::io::Result<()> {
        todo!()
    }

    pub fn get(&mut self) -> Option<Vec<u8>> {
        todo!()
    }
}
