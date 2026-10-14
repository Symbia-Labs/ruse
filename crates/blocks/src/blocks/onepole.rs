//! A first-order filter, six decibels per octave.

use super::util::*;
use crate::block::{ArgInfo, Block, BlockInfo};
use crate::frame::Frame;

const TYPE: usize = 0;
const FREQ: usize = 1;

pub static INFO: BlockInfo = BlockInfo {
    name: "onepole",
    doc: "A first-order filter with a gentle 6 dB per octave slope. Use it for tone controls, for DC removal with a high-pass near 5 Hz, and wherever a sharper filter would sound harsh.",
    args: &[
        ArgInfo::choice("type", &["lowpass", "highpass"], 0, "Low-pass or high-pass."),
        ArgInfo::num("freq", "Hz", 1.0, 24000.0, 1000.0, "The corner frequency, where the response is 3 dB down."),
    ],
    meters: &[],
    make,
};

pub fn make() -> Box<dyn Block> {
    Box::new(OnePole::default())
}

#[derive(Default)]
struct OnePole {
    rate: f32,
    alpha: f32,
    freq: Glide,
    coef: f32,
    high: bool,
    low: Vec<f32>,
}

impl Block for OnePole {
    fn prepare(&mut self, rate: f32, channels: usize) {
        *self = OnePole {
            rate,
            alpha: tick_alpha(rate, 5.0),
            low: vec![0.0; channels],
            ..Default::default()
        };
    }

    fn control(&mut self, a: &[f32]) {
        let f = self.freq.step(a[FREQ].log2(), self.alpha).exp2();
        self.coef = onepole_coef(f, self.rate);
        self.high = a[TYPE] >= 0.5;
    }

    fn process(&mut self, io: &mut Frame, _offset: usize) {
        let n = io.len;
        for (buf, low) in io.ch.iter_mut().zip(self.low.iter_mut()) {
            for x in buf[..n].iter_mut() {
                *low += self.coef * (*x - *low);
                *x = if self.high { *x - *low } else { *low };
            }
            flush(low);
        }
    }
}
