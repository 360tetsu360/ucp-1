pub(crate) const FRAGMENT_FLAG: u8 = 0x10;

#[derive(Clone)]
pub(crate) struct FragmentHeader {
    pub size: u32,
    pub id: u16,
    pub index: u32,
}
