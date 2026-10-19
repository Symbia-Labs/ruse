//! A pitch shifter made of two moving taps on a delay line.
//!
//! A tap whose delay shrinks steadily plays the audio back faster, and so higher, the way a passing siren rises.
//! The delay cannot shrink forever, so each tap sweeps across a window and jumps back. Two taps half a window
//! apart take turns, and each fades out before it jumps, so the jumps are never heard.
//!
//! Shifting moves the formants along with the harmonics, the resonances that give a voice its character. With
//! `formant` up, the shifter puts them back. Every 5 ms, linear prediction measures the spectral envelope of the input
//! and of the shifted sound as the reflection coefficients of an all-pole filter. One lattice filter flattens the
//! shifted sound by its own envelope, and a second gives it the input's, so the harmonics move and the resonances
//! stay. The coefficients move in straight lines between measurements, and a lattice with every coefficient under 1
//! is stable, so the filters stay stable while they change.

use crate::block::{ArgInfo, Block, BlockInfo};
use crate::frame::{ramp, Frame};
use std::f64::consts::PI;

const SEMITONES: usize = 0;
const CENTS: usize = 1;
const WINDOW_MS: usize = 2;
const FORMANT: usize = 3;

/// The shortest delay a tap reads at, so the interpolation always has samples on both sides.
const MIN_DELAY: f64 = 4.0;
const LONGEST_WINDOW_MS: f32 = 200.0;

/// The envelopes are measured over the last 20 ms, four times in each 20 ms.
const FRAME_MS: f32 = 20.0;
const HOPS_PER_FRAME: usize = 4;
/// The filters take new coefficients every 16 samples, which a hop always holds a whole number of.
const STEP: usize = 16;
/// The envelope has one pole for every 1.5 kHz of sample rate, so 32 at 48 kHz, and no more than this.
const MAX_ORDER: usize = 48;
/// The measurement is smoothed across about this many hertz, so the envelope follows the formants of a voice and
/// not its single harmonics.
const SMOOTH_HZ: f64 = 100.0;
/// A noise floor added to the measurement, relative to the signal. It keeps the envelope's peaks within about 30 dB
/// of its average.
const FLOOR: f64 = 1e-3;
/// How much louder than the shifted sound the corrected one may get, in power. Where the two envelopes disagree,
/// for a moment at the start of a note or for good on a pure tone, the filters can boost a frequency that the
/// shifted sound barely has, and this keeps that within 6 dB.
const GUARD: f64 = 4.0;
/// How fast the level followers rise and fall, in ms.
const RISE_MS: f64 = 1.0;
const FALL_MS: f64 = 20.0;

pub static INFO: BlockInfo = BlockInfo {
    name: "pitch_shift",
    doc: "Moves the pitch up or down without changing the length, by reading a delay line with two taps that sweep faster or slower than the audio and take turns. It suits harmonies, octave layers and detune. The window is how far each tap sweeps: long windows keep tones steady but smear drums, and short ones keep drums tight but make tones warble. With formant up, a shifted voice keeps the resonances that give it its character, so it sounds like the same singer in a new key. It outputs only the shifted sound, so give the node a mix to keep the original.",
    args: &[
        ArgInfo::num("semitones", "st", -24.0, 24.0, 0.0, "The shift in semitones. 12 is an octave up, -12 an octave down, 7 a fifth up."),
        ArgInfo::num("cents", "ct", -100.0, 100.0, 0.0, "A fine shift in hundredths of a semitone. A few cents either side thickens a sound."),
        ArgInfo::num("window_ms", "ms", 10.0, LONGEST_WINDOW_MS, 50.0, "How far each tap sweeps before the other takes over."),
        ArgInfo::num("formant", "", 0.0, 1.0, 0.0, "How much of the original's formants to keep. At 0 they move with the pitch, so a voice shifted up sounds smaller and one shifted down sounds larger. At 1 they stay where they were. It suits voices and other sounds with many harmonics under one spectral shape. A pure tone has no formants apart from its pitch, so on one formant lowers the level, and lowers it more for bigger shifts."),
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
    /// How much of the input's envelope the shifted sound takes, at the last control step and this one.
    formant: (f32, f32),
    /// Whether the envelopes are being followed. They are measured only while `formant` is above 0.
    tracking: bool,
    /// The frame and the hop between measurements, in samples.
    frame: usize,
    hop: usize,
    /// Samples since the start. It sets when the envelopes are measured, so they are measured at the same samples
    /// whatever the host's block size.
    count: u64,
    hann: Vec<f64>,
    lag: Vec<f64>,
    /// The level followers' rise and fall coefficients.
    follow: (f64, f64),
    scratch: Scratch,
    envelopes: Vec<Envelopes>,
}

/// Room for one measurement, made once in `prepare`.
#[derive(Default)]
struct Scratch {
    windowed: Vec<f64>,
    acf: Vec<f64>,
    poly: Vec<f64>,
    last: Vec<f64>,
}

/// The formant correction for one channel.
#[derive(Default)]
struct Envelopes {
    /// The shifted sound's last frame, which its envelope is measured from.
    shifted: Vec<f32>,
    /// The input's envelope and the shifted sound's, at the last measurement and the next, as log area ratios: the
    /// inverse hyperbolic tangents of the reflection coefficients. A straight line between two sets of these is a
    /// steady change of sound, and every point on the line is a stable filter.
    input: (Vec<f64>, Vec<f64>),
    output: (Vec<f64>, Vec<f64>),
    /// The reflection coefficients the two lattices use now, and the gain that keeps the level.
    own: Vec<f64>,
    mixed: Vec<f64>,
    gain: f64,
    /// The state of the lattice that flattens the shifted sound, and of the one that gives it the input's envelope.
    flatten: Vec<f64>,
    shape: Vec<f64>,
    /// The power of the shifted sound and of the corrected one, followed quickly up and slowly down.
    level: (f64, f64),
}

/// The order of the envelope at a sample rate.
fn order(rate: f32) -> usize {
    ((rate / 1500.0).round() as usize).clamp(12, MAX_ORDER)
}

/// The length of a measurement frame at a sample rate, a whole number of steps in every hop.
fn frame(rate: f32) -> usize {
    let unit = HOPS_PER_FRAME * STEP;
    ((FRAME_MS * 0.001 * rate) as usize / unit).max(1) * unit
}

/// Fits an all-pole envelope to `samples` seen through `hann`, and writes it to `k` as log area ratios.
///
/// The autocorrelation is smoothed by `lag` and lifted by the floor at lag 0, so the Levinson-Durbin recursion gives
/// reflection coefficients well inside -1 to 1. A silent frame gives all zeros, a filter that changes nothing.
fn fit(samples: impl Iterator<Item = f32>, hann: &[f64], lag: &[f64], s: &mut Scratch, k: &mut [f64]) {
    for ((v, x), h) in s.windowed.iter_mut().zip(samples).zip(hann) {
        *v = x as f64 * h;
    }
    let w = &s.windowed;
    for (j, (r, l)) in s.acf.iter_mut().zip(lag).enumerate() {
        *r = w[..w.len() - j].iter().zip(&w[j..]).map(|(a, b)| a * b).sum::<f64>() * l;
    }
    k.fill(0.0);
    if s.acf[0] < 1e-12 {
        return;
    }
    s.acf[0] *= 1.0 + FLOOR;
    let (r, a, last) = (&s.acf, &mut s.poly, &mut s.last);
    a.fill(0.0);
    a[0] = 1.0;
    let mut error = r[0];
    for i in 1..=k.len() {
        let acc: f64 = (0..i).map(|j| a[j] * r[i - j]).sum();
        let ki = (-acc / error).clamp(-0.999, 0.999);
        last[..i].copy_from_slice(&a[..i]);
        for j in 1..i {
            a[j] = last[j] + ki * last[i - j];
        }
        a[i] = ki;
        k[i - 1] = ki.atanh();
        error *= 1.0 - ki * ki;
    }
}

fn lerp(a: f64, b: f64, t: f64) -> f64 {
    a + (b - a) * t
}

impl Envelopes {
    /// Sets the coefficients `t` of the way from the last measurement to the next, with `amount` of the input's
    /// envelope in place of the shifted sound's. The level is kept: an all-pole filter with reflection coefficients k
    /// gives white noise 1 / prod(1 - k^2) of its power, so the gain is the square root of the ratio between the two.
    fn update(&mut self, t: f64, amount: f64) {
        let (mut own, mut kept) = (1.0, 1.0);
        for i in 0..self.own.len() {
            let y = lerp(self.output.0[i], self.output.1[i], t);
            let x = lerp(self.input.0[i], self.input.1[i], t);
            let (ky, km) = (y.tanh(), (y + amount * (x - y)).tanh());
            self.own[i] = ky;
            self.mixed[i] = km;
            own *= 1.0 - ky * ky;
            kept *= 1.0 - km * km;
        }
        self.gain = (kept / own).sqrt();
    }

    /// Flattens the shifted sample `y` by its own envelope, and gives it the mixed one in its place. With no input
    /// envelope in the mix, the second lattice undoes the first exactly. `follow` holds the level followers' rise and
    /// fall coefficients.
    fn correct(&mut self, y: f32, follow: (f64, f64)) -> f32 {
        let (mut f, mut b) = (y as f64, y as f64);
        for (state, &k) in self.flatten.iter_mut().zip(&self.own) {
            let old = *state;
            *state = b;
            b = k * f + old;
            f += k * old;
        }
        let p = self.shape.len();
        for i in (0..p).rev() {
            let (k, old) = (self.mixed[i], self.shape[i]);
            f -= k * old;
            if i + 1 < p {
                self.shape[i + 1] = k * f + old;
            }
        }
        self.shape[0] = f;
        let out = f * self.gain;
        let (rise, fall) = follow;
        for (level, x) in [(&mut self.level.0, y as f64), (&mut self.level.1, out)] {
            let p = x * x;
            *level += (p - *level) * if p > *level { rise } else { fall };
        }
        for s in self.shape.iter_mut().chain([&mut self.level.0, &mut self.level.1]) {
            if s.abs() < 1e-20 {
                *s = 0.0;
            }
        }
        let limit = GUARD * self.level.0;
        let out = if self.level.1 > limit { out * (limit / self.level.1).sqrt() } else { out };
        out as f32
    }
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

impl PitchShift {
    /// Measures both envelopes on every channel, and moves the ones measured last to the start of the next straight
    /// line. The input's is measured where the taps read on average, half a window back, so both describe the same
    /// stretch of sound. `fresh` starts the correction over, as it does when `formant` comes up from 0.
    fn measure(&mut self, window: f64, fresh: bool) {
        let PitchShift { lines, envelopes, hann, lag, scratch, frame, count, at, .. } = self;
        let (n, len) = (*frame, lines.first().map_or(0, |l| l.len()));
        let back = (MIN_DELAY + 0.5 * window).round() as usize;
        let start = *at + len - back - n + 1;
        let next = (*count % n as u64) as usize;
        for (line, env) in lines.iter().zip(envelopes.iter_mut()) {
            std::mem::swap(&mut env.input.0, &mut env.input.1);
            std::mem::swap(&mut env.output.0, &mut env.output.1);
            fit((0..n).map(|i| line[(start + i) % len]), hann, lag, scratch, &mut env.input.1);
            let shifted = &env.shifted;
            fit((0..n).map(|i| shifted[(next + i) % n]), hann, lag, scratch, &mut env.output.1);
            if fresh {
                env.input.0.copy_from_slice(&env.input.1);
                env.output.0.copy_from_slice(&env.output.1);
                env.flatten.fill(0.0);
                env.shape.fill(0.0);
                env.level = (0.0, 0.0);
            }
        }
    }
}

impl Block for PitchShift {
    fn prepare(&mut self, rate: f32, channels: usize) {
        let len = (LONGEST_WINDOW_MS * 0.001 * rate) as usize + MIN_DELAY as usize + 8;
        let (p, n) = (order(rate), frame(rate));
        let envelopes = (0..channels)
            .map(|_| Envelopes {
                shifted: vec![0.0; n],
                input: (vec![0.0; p], vec![0.0; p]),
                output: (vec![0.0; p], vec![0.0; p]),
                own: vec![0.0; p],
                mixed: vec![0.0; p],
                gain: 1.0,
                flatten: vec![0.0; p],
                shape: vec![0.0; p],
                level: (0.0, 0.0),
            })
            .collect();
        let smooth = 2.0 * PI * SMOOTH_HZ / rate as f64;
        let coef = |ms: f64| 1.0 - (-1000.0 / (ms * rate as f64)).exp();
        *self = PitchShift {
            rate,
            lines: vec![vec![0.0; len]; channels],
            frame: n,
            hop: n / HOPS_PER_FRAME,
            hann: (0..n).map(|i| (PI * (i as f64 + 0.5) / n as f64).sin().powi(2)).collect(),
            lag: (0..=p).map(|j| (-0.5 * (smooth * j as f64).powi(2)).exp()).collect(),
            follow: (coef(RISE_MS), coef(FALL_MS)),
            scratch: Scratch {
                windowed: vec![0.0; n],
                acf: vec![0.0; p + 1],
                poly: vec![0.0; p + 1],
                last: vec![0.0; p + 1],
            },
            envelopes,
            ..Default::default()
        };
    }

    fn control(&mut self, a: &[f32]) {
        let ratio = ((a[SEMITONES] as f64 + a[CENTS] as f64 * 0.01) / 12.0).exp2();
        let window = a[WINDOW_MS] as f64 * 0.001 * self.rate as f64;
        if !self.started {
            self.ratio = (ratio, ratio);
            self.window = (window, window);
            self.formant = (a[FORMANT], a[FORMANT]);
            self.started = true;
        }
        self.ratio = (self.ratio.1, ratio);
        self.window = (self.window.1, window);
        self.formant = (self.formant.1, a[FORMANT]);
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
            let on = self.formant.0 > 0.0 || self.formant.1 > 0.0;
            if !on {
                self.tracking = false;
            } else if !self.tracking || self.count.is_multiple_of(STEP as u64) {
                if !self.tracking || self.count.is_multiple_of(self.hop as u64) {
                    self.measure(window, !self.tracking);
                    self.tracking = true;
                }
                let t = (self.count % self.hop as u64) as f64 / self.hop as f64;
                let amount = ramp(self.formant.0, self.formant.1, offset, k) as f64;
                for env in self.envelopes.iter_mut() {
                    env.update(t, amount);
                }
            }
            let slot = (self.count % self.frame as u64) as usize;
            for ((ch, line), env) in io.ch.iter_mut().zip(self.lines.iter_mut()).zip(self.envelopes.iter_mut()) {
                line[self.at] = ch[k];
                let y = g1 * read(line, self.at, d1) + g2 * read(line, self.at, d2);
                env.shifted[slot] = y;
                ch[k] = if on { env.correct(y, self.follow) } else { y };
            }
            self.at = (self.at + 1) % len;
            self.count += 1;
        }
    }

    fn tail(&self, args: &[f32], rate: f32) -> u32 {
        let shift = (args[WINDOW_MS] * 0.001 * rate) as u32 + MIN_DELAY as u32;
        // The corrected sound keeps the shifted sound's envelope until the silence has filled a frame, and the
        // coefficients take another hop or two to get there.
        let n = frame(rate) as u32;
        if args[FORMANT] > 0.0 {
            shift + n + 2 * n / HOPS_PER_FRAME as u32
        } else {
            shift
        }
    }
}
