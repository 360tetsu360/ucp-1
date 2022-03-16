use crate::{Den, CursorWriter};
use byteorder::{LittleEndian, ReadBytesExt, WriteBytesExt};
use std::io::{Cursor, Result};

impl Den for u16 {
    fn decode(bytes: &mut Cursor<&[u8]>) -> Result<u16> {
        bytes.read_u16::<LittleEndian>()
    }

    fn encode(&self, bytes: &mut CursorWriter) -> Result<()>{
        bytes.write_u16::<LittleEndian>(*self)
    }

    fn size(&self) -> usize {
        2
    }
}

impl Den for i16{
    fn decode(bytes: &mut Cursor<&[u8]>) -> Result<i16> {
        bytes.read_i16::<LittleEndian>()
    }

    fn encode(&self, bytes: &mut CursorWriter) -> Result<()> {
        bytes.write_i16::<LittleEndian>(*self)
    }

    fn size(&self) -> usize {
        2
    }
}

impl Den for u32 {
    fn decode(bytes: &mut Cursor<&[u8]>) -> Result<u32> {
        bytes.read_u32::<LittleEndian>()
    }

    fn encode(&self, bytes: &mut CursorWriter) -> Result<()> {
        bytes.write_u32::<LittleEndian>(*self)
    }

    fn size(&self) -> usize {
        4
    }
}

impl Den for i32 {
    fn decode(bytes: &mut Cursor<&[u8]>) -> Result<i32> {
        bytes.read_i32::<LittleEndian>()
    }

    fn encode(&self, bytes: &mut CursorWriter) -> Result<()> {
        bytes.write_i32::<LittleEndian>(*self)
    }

    fn size(&self) -> usize {
        4
    }
}

impl Den for u64 {
    fn decode(bytes: &mut Cursor<&[u8]>) -> Result<u64> {
        bytes.read_u64::<LittleEndian>()
    }

    fn encode(&self, bytes: &mut CursorWriter) -> Result<()> {
        bytes.write_u64::<LittleEndian>(*self)
    }

    fn size(&self) -> usize {
        8
    }
}

impl Den for i64 {
    fn decode(bytes: &mut Cursor<&[u8]>) -> Result<i64> {
        bytes.read_i64::<LittleEndian>()
    }

    fn encode(&self, bytes: &mut CursorWriter) -> Result<()> {
        bytes.write_i64::<LittleEndian>(*self)
    }

    fn size(&self) -> usize {
        8
    }
}
