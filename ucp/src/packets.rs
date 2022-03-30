use packet_derive::*;

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
    pub fn ordered(&self) -> bool {
        matches!(self, Self::ReliableOrdered) | self.sequenced()
    }
}

#[derive(Clone)]
pub(crate) struct Frame {
    pub reliability: Reliability,
    pub length: u16,
    pub mindex: u32,                      //reliable frame index
    pub sindex: u32,                      //sequenced frame index
    pub oindex: u32,                      //ordered frame index
    pub fragment: Option<FragmentHeader>, //only if fragmented
}

impl Frame {
    pub fn decode(reader: &mut std::io::Cursor<&[u8]>) -> std::io::Result<Self> {
        let mut ret = Self {
            reliability: Reliability::Unreliable,
            length: 0,
            mindex: 0,
            sindex: 0,
            oindex: 0,
            fragment: None,
        };
        let flag = u8::decode(reader)?;
        let fragment = (flag & FRAGMENT_FLAG) != 0;
        let reliability = Reliability::from_u8((flag & 224) >> 5)?;
        ret.length = <Big as DenWith<u16>>::decode(reader)? / 8;
        if reliability.reliable() {
            ret.mindex = U24::decode(reader)?;
        }
        if reliability.sequenced() {
            ret.sindex = U24::decode(reader)?;
        }
        if reliability.ordered() {
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
        Ok(ret)
    }
    pub fn encode(&self, writer: &mut CursorWriter) -> std::io::Result<()> {
        let mut flag = (self.reliability as u8) << 5;
        if self.fragment.is_some() {
            flag |= FRAGMENT_FLAG
        }
        u8::encode(&flag, writer)?;
        Big::encode(&((self.length * 8) as u16), writer)?;
        if self.reliability.reliable() {
            U24::encode(&self.mindex, writer)?;
        }
        if self.reliability.sequenced() {
            U24::encode(&self.sindex, writer)?;
        }
        if self.reliability.ordered() {
            U24::encode(&self.oindex, writer)?;
            u8::encode(&0, writer)?;
        }
        if let Some(fragment) = &self.fragment {
            Big::encode(&fragment.size, writer)?;
            Big::encode(&fragment.id, writer)?;
            Big::encode(&fragment.index, writer)?;
        }
        Ok(())
    }
    pub fn size(reliability: Reliability, fragment: bool) -> usize {
        let mut ret = 1 + 2; // reliability flag + length(octet)
        if reliability.reliable() {
            ret += 3;
        }
        if reliability.sequenced() {
            ret += 3;
        }
        if reliability.ordered() {
            ret += 4;
        }
        if fragment {
            ret += 4;
            ret += 2;
            ret += 4;
        }
        ret
    }
}

pub(crate) const FRAGMENT_FLAG: u8 = 0x10;

#[derive(Clone)]
pub(crate) struct FragmentHeader {
    pub size: u32,
    pub id: u16,
    pub index: u32,
}
