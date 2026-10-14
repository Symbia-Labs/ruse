//! Second-order sections, from the Audio EQ Cookbook, and the filter block built on them.

use super::util::*;
use crate::block::{ArgInfo, Block, BlockInfo};
use crate::frame::Frame;

#[derive(Clone, Copy, Debug)]
pub struct Coef {
    pub b0: f32,
    pub b1: f32,
    pub b2: f32,
    pub a1: f32,
    pub a2: f32,
}

impl Coef {
    pub const IDENTITY: Coef = Coef {
        b0: 1.0,
        b1: 0.0,
        b2: 0.0,
        a1: 0.0,
        a2: 0.0,
    };
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Kind {
    Lowpass,
    Highpass,
    Bandpass,
    Notch,
    Peak,
    Lowshelf,
    Highshelf,
    Allpass,
}

impl Kind {
    pub fn from_index(i: usize) -> Kind {
        match i {
            0 => Kind::Lowpass,
            1 => Kind::Highpass,
            2 => Kind::Bandpass,
            3 => Kind::Notch,
            4 => Kind::Peak,
            5 => Kind::Lowshelf,
            6 => Kind::Highshelf,
            _ => Kind::Allpass,
        }
    }
}

/// Coefficients for one section, normalized so a0 is 1.
pub fn design(kind: Kind, freq: f32, q: f32, gain_db: f32, rate: f32) -> Coef {
    let rate = rate as f64;
    let f = (freq as f64).clamp(5.0, rate * 0.49);
    let q = (q as f64).max(0.05);
    let w0 = 2.0 * std::f64::consts::PI * f / rate;
    let (sin, cos) = w0.sin_cos();
    let alpha = sin / (2.0 * q);
    let a = 10f64.powf(gain_db as f64 / 40.0);
    let (b0, b1, b2, a0, a1, a2) = match kind {
        Kind::Lowpass => ((1.0 - cos) / 2.0, 1.0 - cos, (1.0 - cos) / 2.0, 1.0 + alpha, -2.0 * cos, 1.0 - alpha),
        Kind::Highpass => ((1.0 + cos) / 2.0, -(1.0 + cos), (1.0 + cos) / 2.0, 1.0 + alpha, -2.0 * cos, 1.0 - alpha),
        Kind::Bandpass => (alpha, 0.0, -alpha, 1.0 + alpha, -2.0 * cos, 1.0 - alpha),
        Kind::Notch => (1.0, -2.0 * cos, 1.0, 1.0 + alpha, -2.0 * cos, 1.0 - alpha),
        Kind::Peak => (
            1.0 + alpha * a,
            -2.0 * cos,
            1.0 - alpha * a,
            1.0 + alpha / a,
            -2.0 * cos,
            1.0 - alpha / a,
        ),
        Kind::Lowshelf => {
            let s = 2.0 * a.sqrt() * alpha;
            (
                a * ((a + 1.0) - (a - 1.0) * cos + s),
                2.0 * a * ((a - 1.0) - (a + 1.0) * cos),
                a * ((a + 1.0) - (a - 1.0) * cos - s),
                (a + 1.0) + (a - 1.0) * cos + s,
                -2.0 * ((a - 1.0) + (a + 1.0) * cos),
                (a + 1.0) + (a - 1.0) * cos - s,
            )
        }
        Kind::Highshelf => {
            let s = 2.0 * a.sqrt() * alpha;
            (
                a * ((a + 1.0) + (a - 1.0) * cos + s),
                -2.0 * a * ((a - 1.0) + (a + 1.0) * cos),
                a * ((a + 1.0) + (a - 1.0) * cos - s),
                (a + 1.0) - (a - 1.0) * cos + s,
                2.0 * ((a - 1.0) - (a + 1.0) * cos),
                (a + 1.0) - (a - 1.0) * cos - s,
            )
        }
        Kind::Allpass => (1.0 - alpha, -2.0 * cos, 1.0 + alpha, 1.0 + alpha, -2.0 * cos, 1.0 - alpha),
    };
    Coef {
        b0: (b0 / a0) as f32,
        b1: (b1 / a0) as f32,
        b2: (b2 / a0) as f32,
        a1: (a1 / a0) as f32,
        a2: (a2 / a0) as f32,
    }
}

/// One sample through a section, in transposed direct form II.
#[inline]
pub fn run(c: &Coef, z: &mut [f32; 2], x: f32) -> f32 {
    let y = c.b0 * x + z[0];
    z[0] = c.b1 * x - c.a1 * y + z[1];
    z[1] = c.b2 * x - c.a2 * y;
    y
}

const TYPE: usize = 0;
const FREQ: usize = 1;
const Q: usize = 2;
const GAIN_DB: usize = 3;
const SLOPE: usize = 4;

pub static INFO: BlockInfo = BlockInfo {
    name: "filter",
    doc: "A second-order filter: low-pass, high-pass, band-pass, notch, peaking bell, low shelf, high shelf or all-pass. A low-pass or high-pass can have a steeper slope, made of cascaded Butterworth sections. Frequency, Q and gain glide, so sweeping them does not click.",
    args: &[
        ArgInfo::choice(
            "type",
            &["lowpass", "highpass", "bandpass", "notch", "peak", "lowshelf", "highshelf", "allpass"],
            0,
            "Which response.",
        ),
        ArgInfo::num("freq", "Hz", 10.0, 24000.0, 1000.0, "Cutoff, centre or corner frequency."),
        ArgInfo::num("q", "", 0.1, 40.0, std::f32::consts::FRAC_1_SQRT_2, "Resonance or bandwidth. For a steep low-pass or high-pass the Butterworth values are used instead."),
        ArgInfo::num("gain_db", "dB", -36.0, 36.0, 0.0, "Boost or cut for peak, lowshelf and highshelf. The other types ignore it."),
        ArgInfo::choice(
            "slope",
            &["12", "24", "36", "48"],
            0,
            "Decibels per octave for lowpass and highpass. The other types ignore it.",
        ),
    ],
    meters: &[],
    make,
};

pub fn make() -> Box<dyn Block> {
    Box::new(Filter::default())
}

#[derive(Default)]
struct Filter {
    rate: f32,
    alpha: f32,
    coefs: [Coef; 4],
    sections: usize,
    z: Vec<[[f32; 2]; 4]>,
    freq: Glide,
    q: Glide,
    gain: Glide,
}

impl Default for Coef {
    fn default() -> Self {
        Coef::IDENTITY
    }
}

impl Block for Filter {
    fn prepare(&mut self, rate: f32, channels: usize) {
        *self = Filter {
            rate,
            alpha: tick_alpha(rate, 5.0),
            sections: 1,
            z: vec![[[0.0; 2]; 4]; channels],
            ..Default::default()
        };
    }

    fn control(&mut self, a: &[f32]) {
        let kind = Kind::from_index(a[TYPE] as usize);
        let freq = self.freq.step(a[FREQ].log2(), self.alpha).exp2();
        let q = self.q.step(a[Q].log2(), self.alpha).exp2();
        let gain = self.gain.step(a[GAIN_DB], self.alpha);
        let steep = matches!(kind, Kind::Lowpass | Kind::Highpass) && a[SLOPE] as usize > 0;
        if steep {
            let n = a[SLOPE] as usize + 1;
            for k in 0..n {
                let qk = 1.0 / (2.0 * (std::f32::consts::PI * (2 * k + 1) as f32 / (4 * n) as f32).sin());
                self.coefs[k] = design(kind, freq, qk, 0.0, self.rate);
            }
            self.sections = n;
        } else {
            self.coefs[0] = design(kind, freq, q, gain, self.rate);
            self.sections = 1;
        }
    }

    fn process(&mut self, io: &mut Frame, _offset: usize) {
        let n = io.len;
        for (buf, z) in io.ch.iter_mut().zip(self.z.iter_mut()) {
            for x in buf[..n].iter_mut() {
                let mut v = *x;
                for (c, zs) in self.coefs[..self.sections].iter().zip(z.iter_mut()) {
                    v = run(c, zs, v);
                }
                *x = v;
            }
            for zs in z.iter_mut() {
                flush(&mut zs[0]);
                flush(&mut zs[1]);
            }
        }
    }
}
