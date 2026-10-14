//! A delay line with feedback, damping, ping-pong and modulation. With a short, modulated delay and some
//! feedback it is a chorus, a flanger or a vibrato.

use super::util::*;
use crate::block::{ArgInfo, Block, BlockInfo};
use crate::frame::{ramp, Frame};

const TIME_MS: usize = 0;
const FEEDBACK: usize = 1;
const DAMP_HZ: usize = 2;
const MODE: usize = 3;
const MOD_DEPTH: usize = 4;
const MOD_RATE: usize = 5;
const MAX_MS: usize = 6;

pub static INFO: BlockInfo = BlockInfo {
    name: "delay",
    doc: "A delay line. The block outputs only the delayed signal, so set the node's `mix` to blend it with the dry sound. For an echo use 100 ms or more with feedback. For a chorus use 10 to 30 ms with depth and no feedback and a mix of 0.5. For a flanger use 1 to 5 ms with depth and feedback. For a vibrato use mix 1.",
    args: &[
        ArgInfo::num("time_ms", "ms", 0.1, 4000.0, 250.0, "The delay. Bind it to sync(beats) to lock it to the tempo."),
        ArgInfo::num("feedback", "", 0.0, 0.97, 0.35, "How much of the output returns to the input. It stays below 1 so the echoes die away."),
        ArgInfo::num("damp_hz", "Hz", 200.0, 20000.0, 8000.0, "A low-pass in the feedback path, so each repeat is darker."),
        ArgInfo::choice("mode", &["normal", "pingpong"], 0, "pingpong bounces the echoes between left and right when there are two channels."),
        ArgInfo::num("mod_depth_ms", "ms", 0.0, 20.0, 0.0, "How far the delay time swings from an LFO."),
        ArgInfo::num("mod_rate_hz", "Hz", 0.02, 20.0, 0.5, "The speed of that swing."),
        ArgInfo::num("max_ms", "ms", 10.0, 4000.0, 2000.0, "The longest delay the line can hold. Sets the memory used.").fixed(),
    ],
    meters: &[],
    make,
};

pub fn make() -> Box<dyn Block> {
    Box::new(Delay::default())
}

#[derive(Default)]
struct Line {
    buf: Vec<f32>,
    at: usize,
    lp: f32,
}

impl Line {
    fn new(len: usize) -> Line {
        Line {
            buf: vec![0.0; len],
            at: 0,
            lp: 0.0,
        }
    }

    /// The signal `d` samples ago, by cubic interpolation.
    fn read(&self, d: f32) -> f32 {
        let len = self.buf.len();
        let d = d.clamp(2.0, (len - 4) as f32);
        let i = d.floor();
        let t = d - i;
        let at = |k: isize| self.buf[(self.at as isize - k).rem_euclid(len as isize) as usize];
        let n = i as isize;
        let (y0, y1, y2, y3) = (at(n - 1), at(n), at(n + 1), at(n + 2));
        let c0 = y1;
        let c1 = 0.5 * (y2 - y0);
        let c2 = y0 - 2.5 * y1 + 2.0 * y2 - 0.5 * y3;
        let c3 = 0.5 * (y3 - y0) + 1.5 * (y1 - y2);
        ((c3 * t + c2) * t + c1) * t + c0
    }

    fn write(&mut self, x: f32) {
        self.buf[self.at] = x;
        self.at = (self.at + 1) % self.buf.len();
    }
}

#[derive(Default)]
struct Delay {
    rate: f32,
    max_ms: f32,
    lines: Vec<Line>,
    pingpong: bool,
    time: (f32, f32),
    started: bool,
    feedback: f32,
    damp: f32,
    depth: f32,
    // The LFO phase is kept in f64. A phase that is added up in f32 drifts by a different amount for each way
    // the host splits the audio, and a drifting delay time is heard.
    lfo_inc: f64,
    phase: f64,
    time_ms: f32,
}

impl Block for Delay {
    fn configure(&mut self, a: &[f32]) {
        self.max_ms = a[MAX_MS];
    }

    fn prepare(&mut self, rate: f32, channels: usize) {
        let len = (self.max_ms * 0.001 * rate) as usize + 24;
        *self = Delay {
            rate,
            max_ms: self.max_ms,
            lines: (0..channels).map(|_| Line::new(len)).collect(),
            ..Default::default()
        };
    }

    fn control(&mut self, a: &[f32]) {
        let time = (a[TIME_MS].min(self.max_ms)) * 0.001 * self.rate;
        if !self.started {
            self.time = (time, time);
            self.started = true;
        }
        self.time = (self.time.1, time);
        self.time_ms = a[TIME_MS].min(self.max_ms);
        self.feedback = a[FEEDBACK];
        self.damp = onepole_coef(a[DAMP_HZ], self.rate);
        self.depth = a[MOD_DEPTH] * 0.001 * self.rate;
        self.lfo_inc = a[MOD_RATE] as f64 / self.rate as f64;
        self.pingpong = a[MODE] >= 0.5 && self.lines.len() == 2;
    }

    fn process(&mut self, io: &mut Frame, offset: usize) {
        let n = io.len;
        let channels = self.lines.len().min(io.channels());
        let start = self.phase;
        if self.pingpong {
            let (la, lb) = self.lines.split_at_mut(1);
            let (a, b) = (&mut la[0], &mut lb[0]);
            let (left, right) = io.ch.split_at_mut(1);
            for k in 0..n {
                let d = ramp(self.time.0, self.time.1, offset, k) + self.depth * (start + self.lfo_inc * k as f64).sin_tau();
                let ya = a.read(d);
                let yb = b.read(d);
                a.lp += self.damp * (ya - a.lp);
                b.lp += self.damp * (yb - b.lp);
                let input = (left[0][k] + right[0][k]) * 0.5;
                a.write(input + self.feedback * b.lp);
                b.write(self.feedback * a.lp);
                left[0][k] = ya;
                right[0][k] = yb;
            }
        } else {
            for c in 0..channels {
                let line = &mut self.lines[c];
                let lag = c as f64 * 0.25;
                for k in 0..n {
                    let d = ramp(self.time.0, self.time.1, offset, k)
                        + self.depth * (start + lag + self.lfo_inc * k as f64).sin_tau();
                    let y = line.read(d);
                    line.lp += self.damp * (y - line.lp);
                    let x = io.ch[c][k];
                    line.write(x + self.feedback * line.lp);
                    io.ch[c][k] = y;
                }
            }
        }
        self.phase = (start + self.lfo_inc * n as f64).fract();
        for line in self.lines.iter_mut() {
            flush(&mut line.lp);
        }
    }

    fn tail(&self) -> u32 {
        let repeats = if self.feedback <= 0.01 {
            1.0
        } else {
            ((0.001f32).ln() / self.feedback.ln()).ceil().clamp(1.0, 60.0)
        };
        ((self.time_ms * 0.001 * self.rate + self.depth) * repeats) as u32
    }
}

trait SinTau {
    fn sin_tau(self) -> f32;
}

impl SinTau for f64 {
    /// The sine of a phase measured in turns.
    fn sin_tau(self) -> f32 {
        ((self.fract() * std::f64::consts::TAU).sin()) as f32
    }
}
