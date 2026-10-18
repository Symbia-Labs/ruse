//! A pitch shifter made of two moving taps on a delay line.
//!
//! A tap whose delay shrinks steadily plays the audio back faster, and so higher, the way a passing siren rises.
//! The delay cannot shrink forever, so each tap sweeps across a window and jumps back. Two taps half a window
//! apart take turns, and each fades out before it jumps, so the jumps are never heard.

use crate::block::{ArgInfo, Block, BlockInfo};
use crate::frame::{ramp, Frame};
use std::f64::consts::PI;

const SEMITONES: usize = 0;
const CENTS: usize = 1;
const WINDOW_MS: usize = 2;

/// The shortest delay a tap reads at, so the interpolation always has samples on both sides.
const MIN_DELAY: f64 = 4.0;
const LONGEST_WINDOW_MS: f32 = 200.0;

pub static INFO: BlockInfo = BlockInfo {
    name: "pitch_shift",
    doc: "Moves the pitch up or down without changing the length, by reading a delay line with two taps that sweep faster or slower than the audio and take turns. It suits harmonies, octave layers and detune. The window is how far each tap sweeps: long windows keep tones steady but smear drums, and short ones keep drums tight but make tones warble. It outputs only the shifted sound, so give the node a mix to keep the original.",
    args: &[
        ArgInfo::num("semitones", "st", -24.0, 24.0, 0.0, "The shift in semitones. 12 is an octave up, -12 an octave down, 7 a fifth up."),
        ArgInfo::num("cents", "ct", -100.0, 100.0, 0.0, "A fine shift in hundredths of a semitone. A few cents either side thickens a sound."),
        ArgInfo::num("window_ms", "ms", 10.0, LONGEST_WINDOW_MS, 50.0, "How far each tap sweeps before the other takes over."),
    ],
    meters: &[],
    make,
};

pub fn make() -> Box<dyn Block> {
    Box::new(PitchShift::default())
}

#[derive(Default)]
struct PitchShift {
    rate: f32,
    /// The speed of the playback, and the window in samples, at the last control step and this one.
    ratio: (f64, f64),
    window: (f64, f64),
    started: bool,
    /// Where the first tap is in its sweep, from 0 to 1. The second is half a sweep on.
    phase: f64,
    lines: Vec<Vec<f32>>,
    at: usize,
}

/// Reads `line` `delay` samples before `at`, between samples with a cubic through the four around it.
fn read(line: &[f32], at: usize, delay: f64) -> f32 {
    let len = line.len();
    let pos = at as f64 - delay;
    let base = pos.floor();
    let t = (pos - base) as f32;
    let i = (base as i64).rem_euclid(len as i64) as usize;
    let s = |k: usize| line[(i + len + k - 1) % len];
    let (y0, y1, y2, y3) = (s(0), s(1), s(2), s(3));
    let c1 = 0.5 * (y2 - y0);
    let c2 = y0 - 2.5 * y1 + 2.0 * y2 - 0.5 * y3;
    let c3 = 0.5 * (y3 - y0) + 1.5 * (y1 - y2);
    ((c3 * t + c2) * t + c1) * t + y1
}

impl Block for PitchShift {
    fn prepare(&mut self, rate: f32, channels: usize) {
        let len = (LONGEST_WINDOW_MS * 0.001 * rate) as usize + MIN_DELAY as usize + 8;
        *self = PitchShift {
            rate,
            lines: vec![vec![0.0; len]; channels],
            ..Default::default()
        };
    }

    fn control(&mut self, a: &[f32]) {
        let ratio = ((a[SEMITONES] as f64 + a[CENTS] as f64 * 0.01) / 12.0).exp2();
        let window = a[WINDOW_MS] as f64 * 0.001 * self.rate as f64;
        if !self.started {
            self.ratio = (ratio, ratio);
            self.window = (window, window);
            self.started = true;
        }
        self.ratio = (self.ratio.1, ratio);
        self.window = (self.window.1, window);
    }

    fn process(&mut self, io: &mut Frame, offset: usize) {
        let len = self.lines.first().map_or(0, |l| l.len());
        if len == 0 {
            return;
        }
        for k in 0..io.len {
            let ratio = ramp(self.ratio.0 as f32, self.ratio.1 as f32, offset, k) as f64;
            let window = ramp(self.window.0 as f32, self.window.1 as f32, offset, k) as f64;
            // Playing back at `ratio` means the delay changes by 1 - ratio samples for every sample.
            self.phase = (self.phase + (1.0 - ratio) / window).rem_euclid(1.0);
            let other = (self.phase + 0.5).fract();
            let d1 = MIN_DELAY + self.phase * window;
            let d2 = MIN_DELAY + other * window;
            // Each tap is silent at the ends of its sweep, where it jumps, and the two weights add to one.
            let g1 = (PI * self.phase).sin().powi(2) as f32;
            let g2 = 1.0 - g1;
            for (ch, line) in io.ch.iter_mut().zip(self.lines.iter_mut()) {
                line[self.at] = ch[k];
                ch[k] = g1 * read(line, self.at, d1) + g2 * read(line, self.at, d2);
            }
            self.at = (self.at + 1) % len;
        }
    }

    fn tail(&self, args: &[f32], rate: f32) -> u32 {
        (args[WINDOW_MS] * 0.001 * rate) as u32 + MIN_DELAY as u32
    }
}
