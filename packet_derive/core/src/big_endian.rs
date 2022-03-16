use crate::{DenWith, CursorWriter};
use byteorder::{BigEndian, ReadBytesExt, WriteBytesExt};
use std::io::{Cursor, Result};

pub struct Big;

impl DenWith<u16> for Big {
    fn decode(bytes: &mut Cursor<&[u8]>) -> Result<u16> {
        bytes.read_u16::<BigEndian>()
    }

    fn encode(v: &u16, bytes: &mut CursorWriter) -> Result<()> {
        bytes.write_u16::<BigEndian>(*v)
    }

    fn size(_: &u16) -> usize {
        2
    }
}

impl DenWith<i16> for Big {
    fn decode(bytes: &mut Cursor<&[u8]>) -> Result<i16> {
        bytes.read_i16::<BigEndian>()
    }

    fn encode(v: &i16, bytes: &mut CursorWriter) -> Result<()> {
        bytes.write_i16::<BigEndian>(*v)
    }

    fn size(_: &i16) -> usize {
        2
    }
}

impl DenWith<u32> for Big {
    fn decode(bytes: &mut Cursor<&[u8]>) -> Result<u32> {
        bytes.read_u32::<BigEndian>()
    }

    fn encode(v: &u32, bytes: &mut CursorWriter) -> Result<()> {
        bytes.write_u32::<BigEndian>(*v)
    }

    fn size(_: &u32) -> usize {
        4
    }
}

impl DenWith<i32> for Big {
    fn decode(bytes: &mut Cursor<&[u8]>) -> Result<i32> {
        bytes.read_i32::<BigEndian>()
    }

    fn encode(v: &i32, bytes: &mut CursorWriter) -> Result<()> {
        bytes.write_i32::<BigEndian>(*v)
    }

    fn size(_: &i32) -> usize {
        4
    }
}

impl DenWith<u64> for Big {
    fn decode(bytes: &mut Cursor<&[u8]>) -> Result<u64> {
        bytes.read_u64::<BigEndian>()
    }

    fn encode(v: &u64, bytes: &mut CursorWriter) -> Result<()> {
        bytes.write_u64::<BigEndian>(*v)
    }

    fn size(_: &u64) -> usize {
        8
    }
}

impl DenWith<i64> for Big {
    fn decode(bytes: &mut Cursor<&[u8]>) -> Result<i64> {
        bytes.read_i64::<BigEndian>()
    }

    fn encode(v: &i64, bytes: &mut CursorWriter) -> Result<()> {
        bytes.write_i64::<BigEndian>(*v)
    }

    fn size(_: &i64) -> usize {
        8
    }
}