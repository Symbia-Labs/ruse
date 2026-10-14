//! Level, polarity and pan.

use super::util::*;
use crate::block::{ArgInfo, Block, BlockInfo};
use crate::frame::{ramp, Frame};

const GAIN_DB: usize = 0;
const PAN: usize = 1;
const INVERT: usize = 2;

pub static INFO: BlockInfo = BlockInfo {
    name: "gain",
    doc: "Changes the level, flips the polarity and pans a stereo signal. A change glides across 32 samples, so automating it does not click.",
    args: &[
        ArgInfo::num("gain_db", "dB", -96.0, 48.0, 0.0, "Level change."),
        ArgInfo::num("pan", "", -1.0, 1.0, 0.0, "Balance from -1 (left) to 1 (right), constant power. Only used with exactly two channels."),
        ArgInfo::choice("invert", &["off", "on"], 0, "Flips the polarity."),
    ],
    meters: &[],
    make,
};

pub fn make() -> Box<dyn Block> {
    Box::new(Gain::default())
}

#[derive(Default)]
struct Gain {
    channels: usize,
    // Left, right, and the gain for every channel when there are not exactly two.
    prev: [f32; 3],
    cur: [f32; 3],
    started: bool,
}

impl Block for Gain {
    fn prepare(&mut self, _rate: f32, channels: usize) {
        *self = Gain {
            channels,
            ..Default::default()
        };
    }

    fn control(&mut self, a: &[f32]) {
        let g = db_to_lin(a[GAIN_DB]) * if a[INVERT] >= 0.5 { -1.0 } else { 1.0 };
        let target = if self.channels == 2 {
            let theta = (a[PAN] + 1.0) * std::f32::consts::FRAC_PI_4;
            let k = std::f32::consts::SQRT_2;
            [g * k * theta.cos(), g * k * theta.sin(), g]
        } else {
            [g, g, g]
        };
        if !self.started {
            self.cur = target;
            self.started = true;
        }
        self.prev = self.cur;
        self.cur = target;
    }

    fn process(&mut self, io: &mut Frame, offset: usize) {
        let n = io.len;
        for (c, buf) in io.ch.iter_mut().enumerate() {
            let i = if self.channels == 2 { c.min(1) } else { 2 };
            let (g0, g1) = (self.prev[i], self.cur[i]);
            if g0 == g1 {
                for x in buf[..n].iter_mut() {
                    *x *= g1;
                }
            } else {
                for (k, x) in buf[..n].iter_mut().enumerate() {
                    *x *= ramp(g0, g1, offset, k);
                }
            }
        }
    }
}
