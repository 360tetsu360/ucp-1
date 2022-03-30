use std::{
    cmp,
    time::{Duration, Instant},
};

const BETA_CUBIC: f64 = 0.7;
const C: f64 = 0.4;

pub(crate) struct Cubic {
    min_window: u32,
    wmax: f64,
    k: f64,

    pub cwnd: u32,
    cwnd_inc: u32,

    ssthresh: u32,
    last_reduction: Option<Instant>,
}

impl Cubic {
    pub fn new(mtu: u16) -> Self {
        Self {
            min_window: 2,
            wmax: 0.,
            k: 0.,
            cwnd: iw(mtu),
            cwnd_inc: 0,
            ssthresh: u32::MAX,
            last_reduction: None,
        }
    }

    // K = cubic_root(W_max*(1-beta_cubic)/C)
    fn k(&self) -> f64 {
        (self.wmax * (1. - BETA_CUBIC) / C).cbrt()
    }

    // W_cubic(t) = C*(t-K)^3 + W_max
    fn w_cubic(&self, t: Duration) -> f64 {
        let t = t.as_secs() as f64;
        C * (t - self.k()).powi(3) + self.wmax
    }

    // W_est(t) = W_max*beta_cubic + [3*(1-beta_cubic)/(1+beta_cubic)] * (t/RTT)
    fn w_est(&self, t: Duration, rtt: Duration) -> f64 {
        self.wmax * BETA_CUBIC
            + 3.0 * (1.0 - BETA_CUBIC) / (1.0 + BETA_CUBIC) * t.as_secs_f64() / rtt.as_secs_f64()
    }

    pub fn on_ack(&mut self, ack_cnt: u32, rtt: Duration) {
        if self.cwnd < self.ssthresh {
            // Slow start
            self.cwnd += ack_cnt;
        } else {
            let now = Instant::now();
            let ca_start_time;

            match self.last_reduction {
                Some(t) => ca_start_time = t,
                None => {
                    // When we come here without congestion_event() triggered,
                    // initialize congestion_recovery_start_time, w_max and k.
                    ca_start_time = now;
                    self.last_reduction = Some(now);

                    self.wmax = self.cwnd as f64;
                    self.k = 0.0;
                }
            }

            let t = now - ca_start_time;

            let w_cubic = self.w_cubic(t + rtt);

            // w_est(t)
            let w_est = self.w_est(t, rtt);

            let mut cubic_cwnd = self.cwnd;

            if w_cubic < w_est {
                // TCP friendly region.
                cubic_cwnd = cmp::max(cubic_cwnd, w_est as u32);
            } else if cubic_cwnd < w_cubic as u32 {
                // Concave region or convex region use same increment.
                let cubic_inc = (w_cubic - cubic_cwnd as f64) / cubic_cwnd as f64;

                cubic_cwnd += cubic_inc as u32;
            }

            // Update the increment and increase cwnd by MSS.
            self.cwnd_inc += cubic_cwnd - self.cwnd;

            // cwnd_inc can be more than 1 MSS in the late stage of max probing.
            // however RFC9002 §7.3.3 (Congestion Avoidance) limits
            // the increase of cwnd to 1 max_datagram_size per cwnd acknowledged.
            if self.cwnd_inc >= 1 {
                self.cwnd += 1;
                self.cwnd_inc = 0;
            }
        }
    }

    pub fn on_congestion_event(&mut self, sent: Instant, is_persistent_congestion: bool) {
        if self
            .last_reduction
            .map(|recovery_start_time| sent <= recovery_start_time)
            .unwrap_or(false)
        {
            return;
        }

        let now = Instant::now();
        self.last_reduction = Some(now);

        if (self.cwnd as f64) < self.wmax {
            self.wmax = self.cwnd as f64 * (1.0 + BETA_CUBIC) / 2.0;
        } else {
            self.wmax = self.cwnd as f64;
        }

        self.ssthresh = cmp::max((self.wmax * BETA_CUBIC) as u32, self.min_window);

        self.cwnd = self.ssthresh;
        self.k = self.k();

        self.cwnd_inc = (self.cwnd_inc as f64 * BETA_CUBIC) as u32;

        if is_persistent_congestion {
            self.last_reduction = None;
            self.wmax = self.cwnd as f64;

            // 4.7 Timeout - reduce ssthresh based on BETA_CUBIC
            self.ssthresh = cmp::max((self.cwnd as f64 * BETA_CUBIC) as u32, self.min_window);

            self.cwnd_inc = 0;

            self.cwnd = self.min_window;
        }
    }
}

fn iw(mtu: u16) -> u32 {
    if mtu > 1095 {
        return 3;
    }
    4
}
