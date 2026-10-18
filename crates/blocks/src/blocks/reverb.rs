//! A feedback delay network reverb. The input goes through a pre-delay and four all-pass diffusers, then into
//! eight delay lines that feed each other through a Hadamard matrix. Each line loses a little level and a little
//! of its high end on every trip, set from the decay time and the damping, and its length wobbles slowly so the
//! tail does not ring.
//!
//! It is a block rather than a graph because the lines feed each other through the matrix, which nodes cannot
//! express.

use super::delay::Line;
use super::util::*;
use crate::block::{ArgInfo, Block, BlockInfo};
use crate::frame::{ramp, Frame};

const SIZE: usize = 0;
const DECAY_S: usize = 1;
const PREDELAY_MS: usize = 2;
const DAMP_HZ: usize = 3;
const DIFFUSION: usize = 4;
const MODULATION: usize = 5;
const WIDTH: usize = 6;

pub static INFO: BlockInfo = BlockInfo {
    name: "reverb",
    doc: "A feedback delay network reverb: a pre-delay, four all-pass diffusers that smear the onset, and eight delay lines that feed each other through a mixing matrix, each with damping and a slow wobble that keeps the tail from ringing. The block outputs only the reverb, so set the node's mix to blend it with the dry sound. Every channel feeds the reverb, and each output channel takes a different blend of the lines, so the tail is wide on two or more channels.",
    args: &[
        ArgInfo::num("size", "", 0.0, 1.0, 0.5, "The space: 0 is a small room and 1 a large hall. It sets the line lengths, and a change glides."),
        ArgInfo::num("decay_s", "s", 0.1, 30.0, 2.0, "How long the tail takes to fall 60 dB at low and middle frequencies."),
        ArgInfo::num("predelay_ms", "ms", 0.0, 250.0, 10.0, "A gap before the reverb starts, which keeps it apart from the dry sound."),
        ArgInfo::num("damp_hz", "Hz", 500.0, 20000.0, 6000.0, "A low-pass in every line, so the highs die away sooner than the lows."),
        ArgInfo::num("diffusion", "", 0.0, 1.0, 0.7, "How fast the echoes blur into a smooth wash. Low values keep distinct early echoes."),
        ArgInfo::num("modulation", "", 0.0, 1.0, 0.3, "A slow wobble in the line lengths that smooths metallic ringing. High values add a chorus."),
        ArgInfo::num("width", "", 0.0, 1.0, 1.0, "Stereo width of the tail. 0 makes every channel the same."),
    ],
    meters: &[],
    make,
};

pub fn make() -> Box<dyn Block> {
    Box::new(Reverb::default())
}

/// Line lengths at size 0.5, in ms, chosen so no two share a common period. Size scales them from 0.35 to 1.6 times.
const LINE_MS: [f32; 8] = [29.7, 37.1, 41.1, 43.7, 47.9, 53.3, 59.9, 67.7];
/// The rate of each line's wobble, all different so the lines never move together.
const WOBBLE_HZ: [f64; 8] = [0.31, 0.37, 0.43, 0.53, 0.61, 0.71, 0.83, 0.97];
/// The deepest wobble, at modulation 1.
const WOBBLE_MS: f32 = 0.3;
const DIFFUSER_MS: [f32; 4] = [4.77, 3.59, 12.73, 9.3];
const MAX_PREDELAY_MS: f32 = 250.0;
/// 1 / sqrt(8), which keeps the matrix, the input spread and each output blend at unity power.
const NORM: f32 = 0.353_553_4;
/// Which row of the Hadamard matrix each output channel takes, so neighbouring channels are decorrelated.
const OUT_ROWS: [usize; 8] = [1, 2, 3, 4, 5, 6, 7, 0];
/// 6 dB at the output, which puts the default reverb of a full-range signal about 5 dB under the dry sound.
const OUT_GAIN: f32 = 2.0;

fn scale(size: f32) -> f32 {
    0.35 + 1.25 * size
}

/// An eight-point Hadamard transform, scaled to keep the power the same. It mixes every line into every other.
fn hadamard8(x: &mut [f32; 8]) {
    let mut h = 1;
    while h < 8 {
        let mut i = 0;
        while i < 8 {
            for j in i..i + h {
                let (a, b) = (x[j], x[j + h]);
                x[j] = a + b;
                x[j + h] = a - b;
            }
            i += 2 * h;
        }
        h *= 2;
    }
    for v in x.iter_mut() {
        *v *= NORM;
    }
}

/// A Schroeder all-pass: every frequency passes at the same level, with its phase smeared over the delay.
#[derive(Default)]
struct Diffuser {
    buf: Vec<f32>,
    at: usize,
}

impl Diffuser {
    fn process(&mut self, x: f32, g: f32) -> f32 {
        let delayed = self.buf[self.at];
        let w = x + g * delayed;
        let y = delayed - g * w;
        self.buf[self.at] = if w.abs() < 1e-20 { 0.0 } else { w };
        self.at = (self.at + 1) % self.buf.len();
        y
    }
}

#[derive(Default)]
struct Reverb {
    rate: f32,
    pre: Line,
    diffusers: Vec<Diffuser>,
    lines: Vec<Line>,
    lp: [f32; 8],
    phase: [f64; 8],
    /// Each line's length in samples at the last control step and this one, so a size change glides.
    len: [(f32, f32); 8],
    pre_d: (f32, f32),
    loss: [f32; 8],
    damp: f32,
    diffuse: f32,
    depth: f32,
    width: f32,
    started: bool,
    out: Vec<f32>,
}

impl Block for Reverb {
    fn prepare(&mut self, rate: f32, channels: usize) {
        let samples = |ms: f32| (ms * 0.001 * rate) as usize;
        let longest = LINE_MS[7] * scale(1.0) + WOBBLE_MS + 1.0;
        *self = Reverb {
            rate,
            pre: Line::new(samples(MAX_PREDELAY_MS) + 8),
            diffusers: DIFFUSER_MS
                .iter()
                .map(|ms| Diffuser {
                    buf: vec![0.0; samples(*ms).max(1)],
                    at: 0,
                })
                .collect(),
            lines: (0..8).map(|_| Line::new(samples(longest) + 8)).collect(),
            out: vec![0.0; channels.max(1)],
            ..Default::default()
        };
    }

    fn control(&mut self, a: &[f32]) {
        let rate = self.rate;
        let s = scale(a[SIZE]);
        let decay = a[DECAY_S].max(0.01);
        for (i, ms) in LINE_MS.iter().enumerate() {
            let len = ms * s * 0.001 * rate;
            // Each trip round line i loses enough to fall 60 dB over the decay time.
            self.loss[i] = 10f32.powf(-3.0 * len / (decay * rate));
            if !self.started {
                self.len[i] = (len, len);
            }
            self.len[i] = (self.len[i].1, len);
        }
        let pre = (a[PREDELAY_MS] * 0.001 * rate).max(2.0);
        if !self.started {
            self.pre_d = (pre, pre);
            self.started = true;
        }
        self.pre_d = (self.pre_d.1, pre);
        self.damp = onepole_coef(a[DAMP_HZ], rate);
        self.diffuse = 0.75 * a[DIFFUSION];
        self.depth = a[MODULATION] * WOBBLE_MS * 0.001 * rate;
        self.width = a[WIDTH];
    }

    fn process(&mut self, io: &mut Frame, offset: usize) {
        let n = io.len;
        let chans = io.channels().min(self.out.len());
        if chans == 0 || self.lines.is_empty() {
            return;
        }
        let into = 1.0 / chans as f32;
        let tau = std::f64::consts::TAU;
        let inc: [f64; 8] = std::array::from_fn(|i| WOBBLE_HZ[i] / self.rate as f64);
        for k in 0..n {
            let x = (0..chans).map(|c| io.ch[c][k]).sum::<f32>() * into;
            let mut p = self.pre.read(ramp(self.pre_d.0, self.pre_d.1, offset, k));
            self.pre.write(x);
            for d in self.diffusers.iter_mut() {
                p = d.process(p, self.diffuse);
            }

            let mut y = [0.0f32; 8];
            for (i, yi) in y.iter_mut().enumerate() {
                let wobble = self.depth * ((self.phase[i] + inc[i] * k as f64) * tau).sin() as f32;
                *yi = self.lines[i].read(ramp(self.len[i].0, self.len[i].1, offset, k) + wobble);
            }
            let mut back = [0.0f32; 8];
            for i in 0..8 {
                self.lp[i] += self.damp * (y[i] - self.lp[i]);
                back[i] = self.lp[i] * self.loss[i];
            }
            hadamard8(&mut back);
            let spread = p * NORM;
            for (line, b) in self.lines.iter_mut().zip(back) {
                let v = b + spread;
                line.write(if v.abs() < 1e-20 { 0.0 } else { v });
            }

            for (c, o) in self.out.iter_mut().enumerate().take(chans) {
                let row = OUT_ROWS[c % 8];
                let mut s = 0.0;
                for (i, yi) in y.iter().enumerate() {
                    s += if (row & i).count_ones().is_multiple_of(2) { *yi } else { -*yi };
                }
                *o = s * NORM * OUT_GAIN;
            }
            if chans >= 2 {
                let mid = self.out[..chans].iter().sum::<f32>() * into;
                for o in self.out[..chans].iter_mut() {
                    *o = mid + self.width * (*o - mid);
                }
            }
            for c in 0..chans {
                io.ch[c][k] = self.out[c];
            }
        }
        for ((phase, step), lp) in self.phase.iter_mut().zip(inc).zip(self.lp.iter_mut()) {
            *phase = (*phase + step * n as f64).fract();
            flush(lp);
        }
    }

    /// The pre-delay, then the decay time and a little more for the diffusers and the highest line's last trip.
    fn tail(&self, a: &[f32], rate: f32) -> u32 {
        ((a[PREDELAY_MS] * 0.001 + a[DECAY_S] * 1.1 + 0.05) * rate) as u32
    }
}
