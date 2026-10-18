//! Waveshaping, the nonlinearity behind saturation, overdrive, fuzz and wavefolding.

use super::util::*;
use crate::block::{ArgInfo, Block, BlockInfo};
use crate::frame::{ramp, Frame};

const CURVE: usize = 0;
const DRIVE_DB: usize = 1;
const BIAS: usize = 2;
const OUTPUT_DB: usize = 3;

pub static INFO: BlockInfo = BlockInfo {
    name: "shaper",
    doc: "Pushes the signal through a curve. Drive sets how hard, and every curve adds harmonics. Bias moves the signal off centre before the curve, which adds even harmonics. On its own it runs at the plugin's rate, so high drive can alias. Put it in an oversample node to keep the harmonics it makes above Nyquist from folding back.",
    args: &[
        ArgInfo::choice(
            "curve",
            &["tanh", "hard", "softclip", "fold", "tube"],
            0,
            "tanh rounds the peaks smoothly. hard cuts them flat. softclip is a gentle cubic knee. fold wraps the peaks back on themselves. tube is asymmetric, saturating one half harder than the other.",
        ),
        ArgInfo::num("drive_db", "dB", -24.0, 48.0, 0.0, "Gain before the curve."),
        ArgInfo::num("bias", "", -1.0, 1.0, 0.0, "Moves the signal off centre before the curve. The constant this adds is removed after."),
        ArgInfo::num("output_db", "dB", -48.0, 24.0, 0.0, "Gain after the curve."),
    ],
    meters: &[],
    make,
};

pub fn make() -> Box<dyn Block> {
    Box::new(Shaper::default())
}

#[inline]
fn curve(kind: usize, x: f32) -> f32 {
    match kind {
        0 => x.tanh(),
        1 => x.clamp(-1.0, 1.0),
        2 => {
            if x >= 1.0 {
                1.0
            } else if x <= -1.0 {
                -1.0
            } else {
                1.5 * (x - x * x * x / 3.0)
            }
        }
        3 => (x * std::f32::consts::FRAC_PI_2).sin(),
        _ => {
            if x >= 0.0 {
                1.0 - (-x).exp()
            } else {
                -0.8 * (1.0 - (1.25 * x).exp())
            }
        }
    }
}

#[derive(Default)]
struct Shaper {
    kind: usize,
    bias: f32,
    centre: f32,
    drive: (f32, f32),
    output: (f32, f32),
    started: bool,
}

impl Block for Shaper {
    fn prepare(&mut self, _rate: f32, _channels: usize) {
        *self = Shaper::default();
    }

    fn control(&mut self, a: &[f32]) {
        self.kind = a[CURVE] as usize;
        self.bias = a[BIAS];
        self.centre = curve(self.kind, self.bias);
        let drive = db_to_lin(a[DRIVE_DB]);
        let output = db_to_lin(a[OUTPUT_DB]);
        if !self.started {
            self.drive = (drive, drive);
            self.output = (output, output);
            self.started = true;
        }
        self.drive = (self.drive.1, drive);
        self.output = (self.output.1, output);
    }

    fn process(&mut self, io: &mut Frame, offset: usize) {
        let n = io.len;
        for buf in io.ch.iter_mut() {
            for (k, x) in buf[..n].iter_mut().enumerate() {
                let d = ramp(self.drive.0, self.drive.1, offset, k);
                let o = ramp(self.output.0, self.output.1, offset, k);
                *x = (curve(self.kind, *x * d + self.bias) - self.centre) * o;
            }
        }
    }
}
