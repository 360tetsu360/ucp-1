use packet_derive::*;

use crate::fragment::{FragmentHeader, FRAGMENT_FLAG};

#[repr(u8)]
#[derive(Clone, Copy)]
pub enum Reliability {
    Unreliable = 0,
    UnreliableSequenced = 1,
    Reliable = 2,
    ReliableOrdered = 3,
    ReliableSequenced = 4,
}

impl Reliability {
    pub fn from_u8(v: u8) -> std::io::Result<Self> {
        match v {
            0 => Ok(Reliability::Unreliable),
            1 => Ok(Reliability::UnreliableSequenced),
            2 => Ok(Reliability::Reliable),
            3 => Ok(Reliability::ReliableOrdered),
            4 => Ok(Reliability::ReliableSequenced),
            _ => Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "Invalid byte",
            )),
        }
    }
    pub fn reliable(&self) -> bool {
        matches!(
            self,
            Self::Reliable | Self::ReliableOrdered | Self::ReliableSequenced
        )
    }
    pub fn sequenced(&self) -> bool {
        matches!(self, Self::UnreliableSequenced | Self::ReliableSequenced)
    }
}

pub(crate) struct Frame<'a> {
    pub reliability: Reliability,
    pub fragment: Option<FragmentHeader>, //only if fragmented
    pub mindex: u32,                      //reliable frame index
    pub sindex: u32,                      //sequenced frame index
    pub oindex: u32,                      //ordered frame index
    pub data: &'a [u8],
}

impl<'a> Frame<'a> {
    pub fn decode(reader: &mut std::io::Cursor<&'a [u8]>) -> std::io::Result<Self> {
        let mut ret = Self {
            reliability: Reliability::Unreliable,
            fragment: None,
            mindex: 0,
            sindex: 0,
            oindex: 0,
            data: reader.get_ref(),
        };
        let flag = u8::decode(reader)?;
        let fragment = (flag & FRAGMENT_FLAG) != 0;
        let reliability = Reliability::from_u8((flag & 224) >> 5)?;
        let length = (<Big as DenWith<u16>>::decode(reader)? / 8) as usize;
        if reliability.reliable() {
            ret.mindex = U24::decode(reader)?;
        }
        if reliability.sequenced() {
            ret.sindex = U24::decode(reader)?;
        }
        if let Reliability::ReliableOrdered = reliability {
            ret.oindex = U24::decode(reader)?;
            reader.set_position(reader.position() + 1);
        }
        if fragment {
            let header = FragmentHeader {
                size: Big::decode(reader)?,
                id: Big::decode(reader)?,
                index: Big::decode(reader)?,
            };
            ret.fragment = Some(header);
        }
        ret.data = &reader.get_ref()[reader.position() as usize..reader.position() as usize + length];
        Ok(ret)
    }
    pub fn encode(&self, bytes: &mut Vec<u8>) -> std::io::Result<()> {
        let mut flag = (self.reliability as u8) << 5;
        if self.fragment.is_some() {
            flag |= FRAGMENT_FLAG
        }

        todo!()
    }
}
