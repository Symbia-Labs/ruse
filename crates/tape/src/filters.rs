use crate::fft::fft;
use crate::tables::HALF_TAPS;
use std::f32::consts::PI;
use std::sync::OnceLock;

const N: usize = 1024;

/// The minimum-phase FIR with a given magnitude, by folding the real cepstrum.
/// `magnitude(f)` is the wanted gain at f Hz. Returned reversed, ready for a correlation loop.
pub fn minimum_phase(taps: usize, fs: f32, magnitude: impl Fn(f32) -> f32) -> Vec<f32> {
    let mut re = vec![0.0f64; N];
    let mut im = vec![0.0f64; N];
    for i in 0..N {
        let bin = if i <= N / 2 { i } else { N - i };
        re[i] = magnitude(bin as f32 * fs / N as f32).max(1e-4).ln() as f64;
    }
    fft(&mut re, &mut im, true);
    for i in 0..N {
        let w = if i == 0 || i == N / 2 {
            1.0
        } else if i < N / 2 {
            2.0
        } else {
            0.0
        };
        re[i] = re[i] * w / N as f64;
        im[i] = 0.0;
    }
    fft(&mut re, &mut im, false);
    for i in 0..N {
        let g = re[i].exp();
        let (c, s) = (im[i].cos(), im[i].sin());
        re[i] = g * c;
        im[i] = g * s;
    }
    fft(&mut re, &mut im, true);
    let mut h = vec![0.0f32; taps];
    let fade = taps / 4;
    for i in 0..taps {
        let mut v = (re[i] / N as f64) as f32;
        if i >= taps - fade {
            let x = (i - (taps - fade)) as f32 / fade as f32;
            v *= 0.5 * (1.0 + (PI * x).cos());
        }
        h[taps - 1 - i] = v;
    }
    h
}

/// A half-band low-pass for the 2x stage: windowed sinc, Kaiser beta 8.
pub fn half_band(taps: usize) -> Vec<f32> {
    fn bessel(x: f64) -> f64 {
        let (mut sum, mut term) = (1.0f64, 1.0f64);
        for k in 1..40 {
            let r = x / (2.0 * k as f64);
            term *= r * r;
            sum += term;
        }
        sum
    }
    let mid = (taps - 1) as f64 / 2.0;
    let beta = 8.0;
    let mut h = vec![0.0f32; taps];
    let mut sum = 0.0f64;
    for (i, slot) in h.iter_mut().enumerate() {
        let x = i as f64 - mid;
        let sinc = if x == 0.0 {
            0.5
        } else {
            (std::f64::consts::PI * x / 2.0).sin() / (std::f64::consts::PI * x)
        };
        let r = x / mid;
        let w = bessel(beta * (1.0 - r * r).max(0.0).sqrt()) / bessel(beta);
        *slot = (sinc * w) as f32;
        sum += sinc * w;
    }
    let sum = sum as f32;
    h.iter().map(|v| *v / sum).collect()
}

static HALF_BAND: OnceLock<Vec<f32>> = OnceLock::new();

/// The 2x stage's filter, designed once.
pub fn half_band_taps() -> &'static [f32] {
    HALF_BAND.get_or_init(|| half_band(HALF_TAPS))
}

/// That filter's gain at a frequency given as a fraction of the doubled rate.
pub fn half_band_gain(f: f32) -> f32 {
    let mid = (HALF_TAPS - 1) as f32 / 2.0;
    let mut sum = 0.0f32;
    for (i, h) in half_band_taps().iter().enumerate() {
        sum += h * (2.0 * PI * f * (i as f32 - mid)).cos();
    }
    sum.abs()
}

/// What the playback head loses at f Hz: spacing, coating thickness and gap, for a tape speed in m/s.
pub fn head_loss(f: f32, speed: f32, spacing: f32, coating: f32, gap: f32) -> f32 {
    if f <= 0.0 {
        return 1.0;
    }
    let k = 2.0 * PI * f / speed;
    let space = (-k * spacing).exp();
    let kd = k * coating;
    let thick = if kd < 1e-4 { 1.0 } else { (1.0 - (-kd).exp()) / kd };
    let kg = k * gap / 2.0;
    let gap_loss = if kg < 1e-4 { 1.0 } else { (kg.sin() / kg).abs() };
    (space * thick * gap_loss).max(1e-4)
}

/// Two biquads (b0 b1 b2 a1 a2 each): the head bump and the roll-off beneath it.
// The 0.7071 is the original's literal, kept so the port matches it exactly.
#[allow(clippy::approx_constant)]
pub fn bump(fs: f32, ips: f32) -> [f32; 10] {
    let fb = (4.5 * ips).max(8.0);
    let mut out = [0.0f32; 10];
    {
        // peak, +2 dB
        let a = 10f32.powf(2.0 / 40.0);
        let w = 2.0 * PI * fb / fs;
        let alpha = w.sin() / (2.0 * 1.2);
        let a0 = 1.0 + alpha / a;
        out[0] = (1.0 + alpha * a) / a0;
        out[1] = -2.0 * w.cos() / a0;
        out[2] = (1.0 - alpha * a) / a0;
        out[3] = -2.0 * w.cos() / a0;
        out[4] = (1.0 - alpha / a) / a0;
    }
    {
        // second-order high-pass; also takes out the DC a magnetized tape leaves
        let fc = (0.45 * fb).max(12.0);
        let w = 2.0 * PI * fc / fs;
        let alpha = w.sin() / (2.0 * 0.7071);
        let a0 = 1.0 + alpha;
        let c = w.cos();
        out[5] = (1.0 + c) / 2.0 / a0;
        out[6] = -(1.0 + c) / a0;
        out[7] = (1.0 + c) / 2.0 / a0;
        out[8] = -2.0 * c / a0;
        out[9] = (1.0 - alpha) / a0;
    }
    out
}
