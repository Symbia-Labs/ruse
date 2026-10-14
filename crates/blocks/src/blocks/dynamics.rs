//! Compressor, limiter, gate and expander.
//!
//! The detector measures the level, the gain computer turns the level into a gain change in decibels, and the
//! gain change is smoothed with an attack and a release before it scales the signal. The gain follows in the
//! decibel domain, the way most hardware does. A lookahead delays the audio but not the detector, so the gain
//! is already down when a peak arrives.

use super::util::*;
use crate::block::{ArgInfo, Block, BlockInfo};
use crate::frame::Frame;

const MODE: usize = 0;
const THRESHOLD: usize = 1;
const RATIO: usize = 2;
const KNEE: usize = 3;
const ATTACK: usize = 4;
const RELEASE: usize = 5;
const MAKEUP: usize = 6;
const AUTO_MAKEUP: usize = 7;
const DETECTOR: usize = 8;
const SC_HPF: usize = 9;
const LINK: usize = 10;
const LOOKAHEAD: usize = 11;
const RANGE: usize = 12;

pub static INFO: BlockInfo = BlockInfo {
    name: "dynamics",
    doc: "A dynamics processor. As a compressor or limiter it turns down what rises above the threshold. As a gate or expander it turns down what falls below it. The detector can be filtered so bass does not pump the whole mix, and the channels can be linked so the stereo image holds still. A lookahead adds latency, which the plugin reports to the host.",
    args: &[
        ArgInfo::choice(
            "mode",
            &["compressor", "limiter", "gate", "expander"],
            0,
            "compressor reduces by ratio above the threshold. limiter is a compressor with an infinite ratio. expander reduces by ratio below the threshold, and gate is the same with a steep ratio set by range.",
        ),
        ArgInfo::num("threshold_db", "dB", -80.0, 0.0, -18.0, "The level where the effect starts."),
        ArgInfo::num("ratio", "", 1.0, 100.0, 4.0, "For a compressor, the input change for 1 dB of output change above the threshold. For an expander, the extra reduction below it."),
        ArgInfo::num("knee_db", "dB", 0.0, 24.0, 6.0, "The width of the soft transition around the threshold."),
        ArgInfo::num("attack_ms", "ms", 0.01, 200.0, 10.0, "How fast the gain reduction builds."),
        ArgInfo::num("release_ms", "ms", 1.0, 2000.0, 120.0, "How fast the gain comes back."),
        ArgInfo::num("makeup_db", "dB", -24.0, 36.0, 0.0, "Gain after the processing."),
        ArgInfo::choice("auto_makeup", &["off", "on"], 0, "Adds the gain a typical signal loses, estimated from the threshold and ratio."),
        ArgInfo::choice("detector", &["peak", "rms"], 0, "How the level is measured. rms follows loudness and is gentler."),
        ArgInfo::num("sidechain_hpf_hz", "Hz", 10.0, 1000.0, 10.0, "A high-pass on the detector alone. Near 10 Hz it does nothing."),
        ArgInfo::num("link", "", 0.0, 1.0, 1.0, "0 measures each channel on its own and 1 uses the loudest channel for all of them."),
        ArgInfo::num("lookahead_ms", "ms", 0.0, 10.0, 0.0, "Delays the audio so the detector sees peaks early. Adds this much latency.").fixed(),
        ArgInfo::num("range_db", "dB", 0.0, 80.0, 80.0, "The most the gain can be turned down."),
    ],
    meters: &["gain_reduction_db"],
    make,
};

pub fn make() -> Box<dyn Block> {
    Box::new(Dynamics::default())
}

#[derive(Default)]
struct Dynamics {
    rate: f32,
    mode: usize,
    thr: f32,
    ratio: f32,
    knee: f32,
    attack: f32,
    release: f32,
    makeup: f32,
    rms: bool,
    hpf: f32,
    link: f32,
    range: f32,
    delay: usize,
    // Per channel.
    gain_db: Vec<f32>,
    ms: Vec<f32>,
    hp: Vec<f32>,
    ring: Vec<Vec<f32>>,
    at: usize,
    level: Vec<f32>,
    reduction: f32,
    lookahead_ms: f32,
}

impl Block for Dynamics {
    fn prepare(&mut self, rate: f32, channels: usize) {
        let delay = (self.lookahead_ms * 0.001 * rate).round() as usize;
        *self = Dynamics {
            rate,
            delay,
            lookahead_ms: self.lookahead_ms,
            gain_db: vec![0.0; channels],
            ms: vec![0.0; channels],
            hp: vec![0.0; channels],
            level: vec![0.0; channels],
            ring: vec![vec![0.0; delay.max(1)]; channels],
            ..Default::default()
        };
    }

    fn configure(&mut self, a: &[f32]) {
        self.lookahead_ms = a[LOOKAHEAD];
    }

    fn control(&mut self, a: &[f32]) {
        self.mode = a[MODE] as usize;
        self.thr = a[THRESHOLD];
        self.ratio = match self.mode {
            1 => 1000.0,
            2 => a[RATIO].max(20.0),
            _ => a[RATIO],
        };
        self.knee = a[KNEE];
        self.attack = (-1.0 / (a[ATTACK] * 0.001 * self.rate)).exp();
        self.release = (-1.0 / (a[RELEASE] * 0.001 * self.rate)).exp();
        let auto = if a[AUTO_MAKEUP] >= 0.5 && self.mode == 0 {
            (-self.thr * (1.0 - 1.0 / self.ratio) * 0.5).min(24.0)
        } else {
            0.0
        };
        self.makeup = a[MAKEUP] + auto;
        self.rms = a[DETECTOR] >= 0.5;
        self.hpf = if a[SC_HPF] > 15.0 { onepole_coef(a[SC_HPF], self.rate) } else { 0.0 };
        self.link = a[LINK];
        self.range = a[RANGE];
    }

    fn process(&mut self, io: &mut Frame, _offset: usize) {
        let n = io.len;
        let channels = io.channels();
        let rms_alpha = 1.0 - (-1.0 / (0.01 * self.rate)).exp();
        for k in 0..n {
            // The detector reads the undelayed signal.
            let mut loudest = 0f32;
            for c in 0..channels {
                let mut x = io.ch[c][k];
                if self.hpf > 0.0 {
                    self.hp[c] += self.hpf * (x - self.hp[c]);
                    x -= self.hp[c];
                }
                let l = if self.rms {
                    self.ms[c] += rms_alpha * (x * x - self.ms[c]);
                    self.ms[c].sqrt()
                } else {
                    x.abs()
                };
                self.level[c] = l;
                loudest = loudest.max(l);
            }
            let mut worst = 0f32;
            for c in 0..channels {
                let l = self.level[c] + (loudest - self.level[c]) * self.link;
                let target = self.gain_change(lin_to_db(l));
                let g = &mut self.gain_db[c];
                let coef = if target < *g { self.attack } else { self.release };
                *g = target + coef * (*g - target);
                worst = worst.min(*g);
                let x = io.ch[c][k];
                let heard = if self.delay > 0 {
                    let slot = &mut self.ring[c][self.at];
                    std::mem::replace(slot, x)
                } else {
                    x
                };
                io.ch[c][k] = heard * db_to_lin(*g + self.makeup);
            }
            if self.delay > 0 {
                self.at = (self.at + 1) % self.delay;
            }
            self.reduction = -worst;
        }
        for g in self.gain_db.iter_mut() {
            flush(g);
        }
        for m in self.ms.iter_mut() {
            flush(m);
        }
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

impl Dynamics {
    /// The gain change, in dB and never positive, for an input level in dB.
    fn gain_change(&self, level_db: f32) -> f32 {
        let over = level_db - self.thr;
        let k = self.knee;
        let g = if self.mode < 2 {
            let slope = 1.0 / self.ratio - 1.0;
            if k > 0.0 && over.abs() < k * 0.5 {
                slope * (over + k * 0.5).powi(2) / (2.0 * k)
            } else if over > 0.0 {
                slope * over
            } else {
                0.0
            }
        } else {
            let slope = self.ratio - 1.0;
            if k > 0.0 && over.abs() < k * 0.5 {
                -slope * (k * 0.5 - over).powi(2) / (2.0 * k)
            } else if over < 0.0 {
                slope * over
            } else {
                0.0
            }
        };
        g.clamp(-self.range, 0.0)
    }
}
