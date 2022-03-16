use std::io::Cursor;
use std::io::Read;
use std::io::Result;
use std::io::Write;
use std::net::IpAddr;
use std::net::Ipv4Addr;
use std::net::SocketAddr;

use byteorder::BigEndian;
use byteorder::ReadBytesExt;
use byteorder::WriteBytesExt;

mod big_endian;
mod little_endian;
mod u24;

pub type CursorWriter<'a> = Cursor<&'a mut Vec<u8>>;
pub type CursorReader<'a> = Cursor<&'a [u8]>;
pub use big_endian::*;
pub use little_endian::*;
pub use u24::*;

pub trait Den {
    fn decode(bytes: &mut CursorReader) -> Result<Self>
    where
        Self: Sized;
    fn encode(&self, bytes: &mut CursorWriter) -> Result<()>;
    fn size(&self) -> usize;
}

pub trait DenWith<T> {
    fn decode(bytes: &mut CursorReader) -> Result<T>
    where
        T: Sized;
    fn encode(v: &T, bytes: &mut CursorWriter) -> Result<()>;
    fn size(v: &T) -> usize;
}

impl Den for u8 {
    fn decode(bytes: &mut CursorReader) -> Result<Self> {
        bytes.read_u8()
    }

    fn encode(&self, bytes: &mut CursorWriter) -> Result<()> {
        bytes.write_u8(*self)
    }

    fn size(&self) -> usize {
        1
    }
}

impl Den for i8 {
    fn decode(bytes: &mut CursorReader) -> Result<Self> {
        bytes.read_i8()
    }

    fn encode(&self, bytes: &mut CursorWriter) -> Result<()> {
        bytes.write_i8(*self)
    }

    fn size(&self) -> usize {
        1
    }
}

impl Den for bool {
    fn decode(bytes: &mut CursorReader) -> Result<Self> {
        Ok(bytes.read_u8()? != 0)
    }

    fn encode(&self, bytes: &mut CursorWriter) -> Result<()> {
        bytes.write_u8(*self as u8)
    }

    fn size(&self) -> usize {
        1
    }
}

impl Den for String {
    fn decode(bytes: &mut CursorReader) -> Result<Self> {
        let length = bytes.read_u16::<BigEndian>()?;
        let mut raw_str = vec![0u8; length as usize];
        bytes.read_exact(&mut raw_str)?;
        match String::from_utf8(raw_str) {
            Ok(p) => Ok(p),
            Err(e) => Err(std::io::Error::new(
                std::io::ErrorKind::Other,
                e.to_string(),
            )),
        }
    }

    fn encode(&self, bytes: &mut CursorWriter) -> Result<()> {
        bytes.write_u16::<BigEndian>(self.len() as u16)?;
        bytes.write_all(self.as_bytes())
    }

    fn size(&self) -> usize {
        self.len() + 2
    }
}

impl Den for SocketAddr {
    fn decode(bytes: &mut CursorReader) -> Result<Self> {
        let ip_ver = bytes.read_u8()?;
        if ip_ver == 4 {
            let ip = Ipv4Addr::new(
                0xff - bytes.read_u8()?,
                0xff - bytes.read_u8()?,
                0xff - bytes.read_u8()?,
                0xff - bytes.read_u8()?,
            );
            let port = bytes.read_u16::<BigEndian>()?;
            Ok(SocketAddr::new(IpAddr::V4(ip), port))
        } else {
            todo!()
        }
    }

    fn encode(&self, bytes: &mut CursorWriter) -> Result<()> {
        if self.is_ipv4() {
            bytes.write_u8(0x4)?;
            let ip_bytes = match self.ip() {
                IpAddr::V4(ip) => ip.octets().to_vec(),
                _ => vec![0; 4],
            };

            bytes.write_u8(0xff - ip_bytes[0])?;
            bytes.write_u8(0xff - ip_bytes[1])?;
            bytes.write_u8(0xff - ip_bytes[2])?;
            bytes.write_u8(0xff - ip_bytes[3])?;
            bytes.write_u16::<BigEndian>(self.port())?;
            Ok(())
        } else {
            todo!()
        }
    }

    fn size(&self) -> usize {
        match self {
            SocketAddr::V4(_) => 7,
            SocketAddr::V6(_) => todo!(),
        }
    }
}

pub const OFFLINE_DATA: [u8; 16] = [
    0x00, 0xff, 0xff, 0x00, 0xfe, 0xfe, 0xfe, 0xfe, 0xfd, 0xfd, 0xfd, 0xfd, 0x12, 0x34, 0x56, 0x78,
];

pub struct MAGIC;

impl DenWith<()> for MAGIC {
    fn decode(bytes: &mut CursorReader) -> Result<()> {
        let mut dst = [0u8; 16];
        bytes.read_exact(&mut dst)
    }

    fn encode(_: &(), bytes: &mut CursorWriter) -> Result<()> {
        bytes.write_all(&OFFLINE_DATA[..])
    }

    fn size(_: &()) -> usize {
        16
    }
}
