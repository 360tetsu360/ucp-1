use std::time::Instant;

const BETA_SCALE: f64 = 0.7;
const C_SCALE: f64 = 0.4;

pub(crate) struct Cubic {
    wmax: usize,
    pub cwnd: usize,

    last_reduction : Instant,
}

impl Cubic {
    pub fn new() -> Self {
        Self {
            wmax: 0,
            cwnd: 1,
            last_reduction: Instant::now(),
        }
    }

    pub fn compute(&mut self) {
        let wmax_f = self.wmax as f64;
        let val = wmax_f * (1. - BETA_SCALE) / C_SCALE;
        let k = val.cbrt();
        let t = Instant::now().duration_since(self.last_reduction).as_secs();
    }

    pub fn timeout(&mut self) {}
}
