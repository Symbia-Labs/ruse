//! A tone generator. Saw, square and pulse are band-limited with PolyBLEP, which rounds off each jump so it does
//! not fold back as aliasing; a triangle's harmonics fall fast enough to need nothing.

use super::util::*;
use crate::block::{ArgInfo, Block, BlockInfo};
use crate::frame::{ramp, Frame};

const SHAPE: usize = 0;
const FREQ_HZ: usize = 1;
const LEVEL_DB: usize = 2;
const OFFSET: usize = 3;
const PULSE_WIDTH: usize = 4;
const MODE: usize = 5;
const STEREO_PHASE: usize = 6;

/// A generator sounds whether or not anything comes in. CLAP reads a tail this long as one that never ends.
pub const ENDLESS: u32 = i32::MAX as u32;

pub static INFO: BlockInfo = BlockInfo {
    name: "oscillator",
    doc: "A tone: a sine, triangle, saw, square or pulse at a frequency a number or an expression sets, such as note(57) or a knob. Saw, square and pulse are band-limited. mode replace puts out the tone alone and add mixes it into the signal. Every channel gets the same tone unless stereo_phase starts each channel further round the cycle. In a multiply node, an oscillator in one branch and an empty chain in the other is a ring modulator, and with offset 1 it is amplitude modulation.",
    args: &[
        ArgInfo::choice("shape", &["sine", "triangle", "saw", "square", "pulse"], 0, ""),
        ArgInfo::num("freq_hz", "Hz", 0.1, 20000.0, 440.0, "The frequency. note(n) gives MIDI note n. A change glides."),
        ArgInfo::num("level_db", "dB", -96.0, 0.0, -12.0, "The tone's level. A change glides."),
        ArgInfo::num("offset", "", 0.0, 1.0, 0.0, "Added after the level. Through multiply, 1 makes the tone vary the input's level instead of its polarity."),
        ArgInfo::num("pulse_width", "", 0.05, 0.95, 0.5, "Pulse only. The part of each cycle spent high."),
        ArgInfo::choice("mode", &["replace", "add"], 0, "replace puts out the tone alone. add mixes it into the signal."),
        ArgInfo::num(
            "stereo_phase",
            "deg",
            0.0,
            360.0,
            0.0,
            "How far round the cycle each channel starts from the one before. 180 puts the second channel in opposite phase to the first, which makes a ring modulator or tremolo move across the stereo field.",
        ),
    ],
    meters: &[],
    make,
};

pub fn make() -> Box<dyn Block> {
    Box::new(Oscillator::default())
}

/// The PolyBLEP correction near a jump at phase 0, for a phase `t` that moves `dt` each sample.
fn blep(t: f64, dt: f64) -> f64 {
    if t < dt {
        let t = t / dt;
        t + t - t * t - 1.0
    } else if t > 1.0 - dt {
        let t = (t - 1.0) / dt;
        t * t + t + t + 1.0
    } else {
        0.0
    }
}

#[derive(Default)]
struct Oscillator {
    rate: f32,
    /// Kept in f64, so the phase does not drift by a different amount for each way the host splits the audio.
    phase: f64,
    /// Phase step per sample, and level, at the last control step and this one.
    step: (f32, f32),
    level: (f32, f32),
    shape: usize,
    offset: f32,
    width: f64,
    add: bool,
    /// The phase each channel adds to the one before, in cycles.
    spread: f64,
    started: bool,
}

impl Oscillator {
    fn value(&self, t: f64, dt: f64) -> f64 {
        match self.shape {
            0 => (t * std::f64::consts::TAU).sin(),
            1 => 1.0 - 4.0 * (t - 0.5).abs(),
            2 => 2.0 * t - 1.0 - blep(t, dt),
            _ => {
                let w = if self.shape == 3 { 0.5 } else { self.width };
                let high = if t < w { 1.0 } else { -1.0 };
                high + blep(t, dt) - blep((t - w).rem_euclid(1.0), dt)
            }
        }
    }
}

impl Block for Oscillator {
    fn prepare(&mut self, rate: f32, _channels: usize) {
        *self = Oscillator {
            rate,
            ..Default::default()
        };
    }

    fn control(&mut self, a: &[f32]) {
        let step = a[FREQ_HZ].min(self.rate * 0.49) / self.rate;
        let level = db_to_lin(a[LEVEL_DB]);
        if !self.started {
            self.step = (step, step);
            self.level = (level, level);
            self.started = true;
        }
        self.step = (self.step.1, step);
        self.level = (self.level.1, level);
        self.shape = a[SHAPE] as usize;
        self.offset = a[OFFSET];
        self.width = a[PULSE_WIDTH] as f64;
        self.add = a[MODE] >= 0.5;
        self.spread = a[STEREO_PHASE] as f64 / 360.0;
    }

    fn process(&mut self, io: &mut Frame, offset: usize) {
        for k in 0..io.len {
            let dt = ramp(self.step.0, self.step.1, offset, k) as f64;
            let level = ramp(self.level.0, self.level.1, offset, k);
            let same = self.value(self.phase, dt) as f32 * level + self.offset;
            for (c, ch) in io.ch.iter_mut().enumerate() {
                let v = if self.spread == 0.0 || c == 0 {
                    same
                } else {
                    self.value((self.phase + c as f64 * self.spread).fract(), dt) as f32 * level + self.offset
                };
                ch[k] = if self.add { ch[k] + v } else { v };
            }
            self.phase = (self.phase + dt).fract();
        }
    }

    fn tail(&self, _args: &[f32], _rate: f32) -> u32 {
        ENDLESS
    }
}
