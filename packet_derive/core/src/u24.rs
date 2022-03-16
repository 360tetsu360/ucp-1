use crate::{CursorWriter, DenWith};
use byteorder::{LittleEndian, ReadBytesExt, WriteBytesExt};
use std::io::Result;

pub struct U24;

impl DenWith<u32> for U24 {
    fn decode(bytes: &mut std::io::Cursor<&[u8]>) -> std::io::Result<u32> {
        bytes.read_u24::<LittleEndian>()
    }

    fn encode(v: &u32, bytes: &mut CursorWriter) -> Result<()> {
        bytes.write_u24::<LittleEndian>(*v)
    }

    fn size(_: &u32) -> usize {
        3
    }
}
