use std::time::Instant;

const BETA_SCALE : f32 = 0.7;
const C : f32 = 0.4;

pub(crate) struct Cubic {
    wmax : usize,
    pub cwnd : usize,

    t : Instant,
}

impl Cubic {

}