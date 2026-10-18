//! A brickwall peak limiter.
//!
//! For each sample the limiter works out the gain that would keep it under the ceiling. The smallest of those
//! gains over the lookahead is held for the length of the lookahead, allowed to rise only at the release, and
//! then averaged over the lookahead. The average is a straight ramp down that reaches each peak's gain exactly when
//! the delayed audio brings that peak out, so no sample goes over and the gain never jumps.

use super::util::*;
use crate::block::{ArgInfo, Block, BlockInfo};
use crate::frame::{ramp, Frame};

const INPUT: usize = 0;
const CEILING: usize = 1;
const RELEASE: usize = 2;
const LOOKAHEAD: usize = 3;

pub static INFO: BlockInfo = BlockInfo {
    name: "limiter",
    doc: "A brickwall peak limiter. It sees each peak a lookahead early and ramps the gain down to meet it, so no sample goes over the ceiling, then lets the gain back up at the release. The channels share one gain, so the stereo image holds still. Raise input to push the level up against the ceiling. The lookahead is the plugin's latency. It limits sample peaks: the waveform between samples can still go a little over, which an oversample node around it reduces.",
    args: &[
        ArgInfo::num("input_db", "dB", -24.0, 36.0, 0.0, "Gain into the limiter. More pushes the level harder against the ceiling."),
        ArgInfo::num("ceiling_db", "dB", -24.0, 0.0, -1.0, "The highest any sample of the output reaches."),
        ArgInfo::num("release_ms", "ms", 1.0, 2000.0, 80.0, "How fast the gain comes back after a peak. Short is louder, long distorts less."),
        ArgInfo::num(
            "lookahead_ms",
            "ms",
            0.1,
            10.0,
            1.5,
            "How early it sees a peak, which is also the time the gain takes to come down and the latency it adds. Longer is cleaner on bass.",
        )
        .fixed(),
    ],
    meters: &["gain_reduction_db"],
    make,
};

pub fn make() -> Box<dyn Block> {
    Box::new(Limiter::default())
}

/// The smallest value among the last `span` pushed, kept as a queue of candidates that rise from front to back.
#[derive(Default)]
struct MinHold {
    vals: Vec<f32>,
    times: Vec<u64>,
    head: usize,
    len: usize,
    span: u64,
    now: u64,
}

impl MinHold {
    fn new(span: usize) -> MinHold {
        MinHold {
            vals: vec![0.0; span + 1],
            times: vec![0; span + 1],
            head: 0,
            len: 0,
            span: span as u64,
            now: 0,
        }
    }

    fn push(&mut self, v: f32) -> f32 {
        let cap = self.vals.len();
        // A newer value at or below the ones before it means they can never be the smallest again.
        while self.len > 0 && self.vals[(self.head + self.len - 1) % cap] >= v {
            self.len -= 1;
        }
        let slot = (self.head + self.len) % cap;
        self.vals[slot] = v;
        self.times[slot] = self.now;
        self.len += 1;
        while self.times[self.head] + self.span <= self.now {
            self.head = (self.head + 1) % cap;
            self.len -= 1;
        }
        self.now += 1;
        self.vals[self.head]
    }
}

#[derive(Default)]
struct Limiter {
    rate: f32,
    lookahead_ms: f32,
    /// The lookahead in samples, which is the delay on the audio.
    delay: usize,
    input: (f32, f32),
    ceiling: (f32, f32),
    release: f32,
    started: bool,
    hold: MinHold,
    /// The released gain, and the last `delay + 1` of it with their sum, for the average.
    gain: f32,
    window: Vec<f32>,
    sum: f64,
    at: usize,
    ring: Vec<Vec<f32>>,
    ring_at: usize,
    reduction: f32,
}

impl Block for Limiter {
    fn configure(&mut self, a: &[f32]) {
        self.lookahead_ms = a[LOOKAHEAD];
    }

    fn prepare(&mut self, rate: f32, channels: usize) {
        let delay = ((self.lookahead_ms * 0.001 * rate).round() as usize).max(1);
        *self = Limiter {
            rate,
            lookahead_ms: self.lookahead_ms,
            delay,
            hold: MinHold::new(delay + 1),
            gain: 1.0,
            window: vec![1.0; delay + 1],
            sum: (delay + 1) as f64,
            ring: vec![vec![0.0; delay]; channels],
            ..Default::default()
        };
    }

    fn control(&mut self, a: &[f32]) {
        let input = db_to_lin(a[INPUT]);
        let ceiling = db_to_lin(a[CEILING]);
        if !self.started {
            self.input = (input, input);
            self.ceiling = (ceiling, ceiling);
            self.started = true;
        }
        self.input = (self.input.1, input);
        self.ceiling = (self.ceiling.1, ceiling);
        self.release = (-1.0 / (a[RELEASE] * 0.001 * self.rate)).exp();
    }

    fn process(&mut self, io: &mut Frame, offset: usize) {
        let n = io.len;
        let span = self.window.len();
        let mut lowest = 1.0f32;
        for k in 0..n {
            let input = ramp(self.input.0, self.input.1, offset, k);
            let ceiling = ramp(self.ceiling.0, self.ceiling.1, offset, k);
            let mut peak = 0f32;
            for ch in io.ch.iter_mut() {
                ch[k] *= input;
                peak = peak.max(ch[k].abs());
            }
            // Below a millionth the gain is silence anyway, and the sum stays exact.
            let need = if peak > ceiling { (ceiling / peak).max(1e-6) } else { 1.0 };
            let held = self.hold.push(need);
            self.gain = if held < self.gain {
                held
            } else {
                held + self.release * (self.gain - held)
            };
            self.sum += self.gain as f64 - self.window[self.at] as f64;
            self.window[self.at] = self.gain;
            self.at = (self.at + 1) % span;
            let g = (self.sum / span as f64) as f32;
            lowest = lowest.min(g);
            for (ch, ring) in io.ch.iter_mut().zip(self.ring.iter_mut()) {
                let x = std::mem::replace(&mut ring[self.ring_at], ch[k]);
                ch[k] = x * g;
            }
            self.ring_at = (self.ring_at + 1) % self.delay;
        }
        if self.gain > 1.0 - 1e-7 {
            self.gain = 1.0;
        }
        self.reduction = -lin_to_db(lowest);
    }

    fn latency(&self) -> u32 {
        self.delay as u32
    }

    fn meter(&self, index: usize) -> f32 {
        if index == 0 {
            self.reduction
        } else {
            0.0
        }
    }
}
