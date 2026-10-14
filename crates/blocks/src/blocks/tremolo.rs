//! Amplitude modulation: tremolo, and auto-pan with the channels out of phase.

use crate::block::{ArgInfo, Block, BlockInfo};
use crate::frame::{ramp, Frame};

const RATE_HZ: usize = 0;
const DEPTH: usize = 1;
const SHAPE: usize = 2;
const STEREO_PHASE: usize = 3;

pub static INFO: BlockInfo = BlockInfo {
    name: "tremolo",
    doc: "Varies the level with a low-frequency oscillator. With the channels half a cycle apart it is an auto-pan.",
    args: &[
        ArgInfo::num("rate_hz", "Hz", 0.05, 40.0, 4.0, "The speed."),
        ArgInfo::num("depth", "", 0.0, 1.0, 0.5, "0 does nothing. 1 takes the level all the way down at the bottom of each cycle."),
        ArgInfo::choice("shape", &["sine", "triangle"], 0, "The wave."),
        ArgInfo::num("stereo_phase", "deg", 0.0, 180.0, 0.0, "How far each channel's cycle is offset from the one before. 180 with two channels is an auto-pan."),
    ],
    meters: &[],
    make,
};

pub fn make() -> Box<dyn Block> {
    Box::new(Tremolo::default())
}

#[derive(Default)]
struct Tremolo {
    rate: f32,
    // Phases are f64, so the result does not depend on how the host splits the audio.
    inc: f64,
    depth: (f32, f32),
    triangle: bool,
    offset_turns: f64,
    phase: f64,
    started: bool,
}

impl Block for Tremolo {
    fn prepare(&mut self, rate: f32, _channels: usize) {
        *self = Tremolo {
            rate,
            ..Default::default()
        };
    }

    fn control(&mut self, a: &[f32]) {
        self.inc = a[RATE_HZ] as f64 / self.rate as f64;
        if !self.started {
            self.depth = (a[DEPTH], a[DEPTH]);
            self.started = true;
        }
        self.depth = (self.depth.1, a[DEPTH]);
        self.triangle = a[SHAPE] >= 0.5;
        self.offset_turns = a[STEREO_PHASE] as f64 / 360.0;
    }

    fn process(&mut self, io: &mut Frame, offset: usize) {
        let n = io.len;
        for (c, buf) in io.ch.iter_mut().enumerate() {
            for (k, x) in buf[..n].iter_mut().enumerate() {
                let turns = (self.phase + self.inc * k as f64 + self.offset_turns * c as f64).fract();
                let lfo = if self.triangle {
                    (1.0 - 4.0 * (turns - 0.5).abs()) as f32
                } else {
                    (turns * std::f64::consts::TAU).sin() as f32
                };
                // lfo runs from -1 to 1. The gain runs from 1 - depth to 1.
                let depth = ramp(self.depth.0, self.depth.1, offset, k);
                *x *= 1.0 - depth * 0.5 * (1.0 - lfo);
            }
        }
        self.phase = (self.phase + self.inc * n as f64).fract();
    }
}
