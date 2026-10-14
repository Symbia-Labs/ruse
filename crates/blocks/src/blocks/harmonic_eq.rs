//! A parametric equalizer whose bands sit on a harmonic series.
//!
//! One fundamental places up to 32 peaking bands at its multiples. The gain of each band follows a shape: a
//! base gain, a tilt per octave of harmonic number, a separate lift for odd and for even harmonics, and
//! individual trims for the first eight. This is what lets you thicken or thin a tone's overtones, take the
//! odd harmonics out of a hollow sound, or notch the hum and buzz of a mains fundamental, all with one
//! control for the pitch. Stretch bends the series the way a piano's strings do.

use super::biquad::{design, run, Coef, Kind};
use super::util::*;
use crate::block::{ArgInfo, Block, BlockInfo};
use crate::frame::Frame;

const FUNDAMENTAL: usize = 0;
const HARMONICS: usize = 1;
const FIRST: usize = 2;
const GAIN_DB: usize = 3;
const TILT: usize = 4;
const ODD_DB: usize = 5;
const EVEN_DB: usize = 6;
const BANDWIDTH: usize = 7;
const STRETCH: usize = 8;
const TRIM_0: usize = 9;

const MAX_BANDS: usize = 32;

pub static INFO: BlockInfo = BlockInfo {
    name: "harmonic_eq",
    doc: "A parametric equalizer with its bands on the harmonics of one fundamental. Band k sits at k times the fundamental and has the gain base + tilt * log2(k) + (odd or even lift) + the trim for harmonic k, if there is one. Bandwidth is a fraction of the fundamental, so the bands cover the spectrum evenly and a narrow setting rings like a comb. Harmonics above 45% of the sample rate are left out.",
    args: &[
        ArgInfo::num("fundamental_hz", "Hz", 20.0, 2000.0, 110.0, "The pitch. Use note(n) for a MIDI note, or 50 or 60 to treat mains hum."),
        ArgInfo::num("harmonics", "", 1.0, 32.0, 8.0, "How many harmonics, counted from the first one affected."),
        ArgInfo::num("first", "", 1.0, 32.0, 1.0, "The lowest harmonic number affected. 2 leaves the fundamental alone."),
        ArgInfo::num("gain_db", "dB", -24.0, 24.0, 0.0, "Boost or cut for every band."),
        ArgInfo::num("tilt_db_oct", "dB", -12.0, 12.0, 0.0, "Extra gain per doubling of the harmonic number. Positive brightens the overtones."),
        ArgInfo::num("odd_db", "dB", -24.0, 24.0, 0.0, "Extra gain on the odd harmonics: 1, 3, 5 and so on."),
        ArgInfo::num("even_db", "dB", -24.0, 24.0, 0.0, "Extra gain on the even harmonics: 2, 4, 6 and so on."),
        ArgInfo::num("bandwidth", "", 0.02, 1.0, 0.25, "Each band's width as a fraction of the fundamental. Small is narrow and surgical."),
        ArgInfo::num("stretch", "", 0.0, 0.02, 0.0, "Inharmonicity. Band k sits at k times the fundamental times sqrt(1 + stretch * k * k)."),
        ArgInfo::num("trim_1_db", "dB", -24.0, 24.0, 0.0, "Extra gain on harmonic 1."),
        ArgInfo::num("trim_2_db", "dB", -24.0, 24.0, 0.0, "Extra gain on harmonic 2."),
        ArgInfo::num("trim_3_db", "dB", -24.0, 24.0, 0.0, "Extra gain on harmonic 3."),
        ArgInfo::num("trim_4_db", "dB", -24.0, 24.0, 0.0, "Extra gain on harmonic 4."),
        ArgInfo::num("trim_5_db", "dB", -24.0, 24.0, 0.0, "Extra gain on harmonic 5."),
        ArgInfo::num("trim_6_db", "dB", -24.0, 24.0, 0.0, "Extra gain on harmonic 6."),
        ArgInfo::num("trim_7_db", "dB", -24.0, 24.0, 0.0, "Extra gain on harmonic 7."),
        ArgInfo::num("trim_8_db", "dB", -24.0, 24.0, 0.0, "Extra gain on harmonic 8."),
    ],
    meters: &[],
    make,
};

pub fn make() -> Box<dyn Block> {
    Box::new(HarmonicEq::default())
}

/// The control values that shape the bands. They glide together, so a sweep of the fundamental or a change
/// of gain never clicks.
#[derive(Clone, Copy, Default)]
struct Shape {
    f0_log: Glide,
    gain: Glide,
    tilt: Glide,
    odd: Glide,
    even: Glide,
    bw_log: Glide,
    stretch: Glide,
    trims: [Glide; 8],
}

#[derive(Default)]
struct HarmonicEq {
    rate: f32,
    alpha: f32,
    shape: Shape,
    // One slot for each harmonic number, so a band keeps its filter memory when its neighbours switch on or off.
    coefs: [Coef; MAX_BANDS],
    on: [bool; MAX_BANDS],
    z: Vec<[[f32; 2]; MAX_BANDS]>,
}

impl Block for HarmonicEq {
    fn prepare(&mut self, rate: f32, channels: usize) {
        *self = HarmonicEq {
            rate,
            alpha: tick_alpha(rate, 8.0),
            z: vec![[[0.0; 2]; MAX_BANDS]; channels],
            ..Default::default()
        };
    }

    fn control(&mut self, a: &[f32]) {
        let al = self.alpha;
        let s = &mut self.shape;
        let f0 = s.f0_log.step(a[FUNDAMENTAL].log2(), al).exp2();
        let gain = s.gain.step(a[GAIN_DB], al);
        let tilt = s.tilt.step(a[TILT], al);
        let odd = s.odd.step(a[ODD_DB], al);
        let even = s.even.step(a[EVEN_DB], al);
        let bw = s.bw_log.step(a[BANDWIDTH].log2(), al).exp2();
        let stretch = s.stretch.step(a[STRETCH], al);
        let mut trims = [0f32; 8];
        for (i, t) in trims.iter_mut().enumerate() {
            *t = s.trims[i].step(a[TRIM_0 + i], al);
        }

        let first = a[FIRST].round() as usize;
        let count = a[HARMONICS].round() as usize;
        let last = (first + count - 1).min(MAX_BANDS);
        let limit = self.rate * 0.45;
        self.on = [false; MAX_BANDS];
        for k in first..=last {
            let kf = k as f32;
            let freq = f0 * kf * (1.0 + stretch * kf * kf).sqrt();
            if freq >= limit {
                break;
            }
            let mut g = gain + tilt * kf.log2() + if k % 2 == 1 { odd } else { even };
            if k <= 8 {
                g += trims[k - 1];
            }
            if g.abs() < 0.02 {
                continue;
            }
            // The Q that gives a bandwidth of `bw` times the fundamental at this centre frequency.
            let q = (freq / (bw * f0)).clamp(0.5, 300.0);
            self.coefs[k - 1] = design(Kind::Peak, freq, q, g.clamp(-36.0, 36.0), self.rate);
            self.on[k - 1] = true;
        }
    }

    fn process(&mut self, io: &mut Frame, _offset: usize) {
        let len = io.len;
        for (buf, z) in io.ch.iter_mut().zip(self.z.iter_mut()) {
            for (slot, zs) in z.iter_mut().enumerate() {
                if !self.on[slot] {
                    *zs = [0.0; 2];
                }
            }
            if self.on.iter().any(|b| *b) {
                for x in buf[..len].iter_mut() {
                    let mut v = *x;
                    for (slot, zs) in z.iter_mut().enumerate() {
                        if self.on[slot] {
                            v = run(&self.coefs[slot], zs, v);
                        }
                    }
                    *x = v;
                }
            }
            for zs in z.iter_mut() {
                flush(&mut zs[0]);
                flush(&mut zs[1]);
            }
        }
    }
}
