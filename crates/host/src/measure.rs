//! Levels and a coarse spectrum, so an agent can tell what a plugin did to a signal.

use std::f64::consts::PI;

const FLOOR_DB: f32 = -240.0;

fn to_db(amplitude: f32) -> f32 {
    if amplitude <= 1e-12 {
        FLOOR_DB
    } else {
        20.0 * amplitude.log10()
    }
}

pub fn peak_db(x: &[f32]) -> f32 {
    to_db(x.iter().fold(0.0f32, |m, v| m.max(v.abs())))
}

pub fn rms_db(x: &[f32]) -> f32 {
    if x.is_empty() {
        return FLOOR_DB;
    }
    let sum: f64 = x.iter().map(|v| (*v as f64) * (*v as f64)).sum();
    to_db((sum / x.len() as f64).sqrt() as f32)
}

pub fn dc(x: &[f32]) -> f32 {
    if x.is_empty() {
        return 0.0;
    }
    (x.iter().map(|v| *v as f64).sum::<f64>() / x.len() as f64) as f32
}

pub fn nonfinite(x: &[f32]) -> usize {
    x.iter().filter(|v| !v.is_finite()).count()
}

/// The centres of the octave bands in the spectrum summary.
pub const BAND_HZ: [f32; 10] = [31.5, 63.0, 125.0, 250.0, 500.0, 1000.0, 2000.0, 4000.0, 8000.0, 16000.0];

fn fft(re: &mut [f64], im: &mut [f64]) {
    let n = re.len();
    let mut j = 0usize;
    for i in 1..n {
        let mut bit = n >> 1;
        while j & bit != 0 {
            j ^= bit;
            bit >>= 1;
        }
        j ^= bit;
        if i < j {
            re.swap(i, j);
            im.swap(i, j);
        }
    }
    let mut len = 2;
    while len <= n {
        let angle = -2.0 * PI / len as f64;
        let (wr, wi) = (angle.cos(), angle.sin());
        let mut i = 0;
        while i < n {
            let (mut cr, mut ci) = (1.0f64, 0.0f64);
            for k in 0..len / 2 {
                let (a, b) = (i + k, i + k + len / 2);
                let vr = re[b] * cr - im[b] * ci;
                let vi = re[b] * ci + im[b] * cr;
                let (ur, ui) = (re[a], im[a]);
                re[a] = ur + vr;
                im[a] = ui + vi;
                re[b] = ur - vr;
                im[b] = ui - vi;
                let next = cr * wr - ci * wi;
                ci = cr * wi + ci * wr;
                cr = next;
            }
            i += len;
        }
        len <<= 1;
    }
}

/// Mean-square level in each octave band, in dB relative to a full-scale amplitude of 1.
/// A sine of amplitude 1 inside a band reads -3 dB there.
pub fn band_levels_db(x: &[f32], rate: f32) -> [f32; 10] {
    const N: usize = 4096;
    let window: Vec<f64> = (0..N).map(|n| 0.5 - 0.5 * (2.0 * PI * n as f64 / N as f64).cos()).collect();
    let window_power: f64 = window.iter().map(|w| w * w).sum();
    let mut starts: Vec<usize> = Vec::new();
    let mut s = 0;
    while s + N <= x.len() {
        starts.push(s);
        s += N / 2;
    }
    if starts.is_empty() {
        starts.push(0);
    }
    let mut sum = [0f64; 10];
    let mut re = vec![0f64; N];
    let mut im = vec![0f64; N];
    for &start in &starts {
        for n in 0..N {
            let v = x.get(start + n).copied().unwrap_or(0.0);
            re[n] = if v.is_finite() { v as f64 * window[n] } else { 0.0 };
            im[n] = 0.0;
        }
        fft(&mut re, &mut im);
        for k in 1..N / 2 {
            let hz = k as f64 * rate as f64 / N as f64;
            let power = 2.0 * (re[k] * re[k] + im[k] * im[k]) / (N as f64 * window_power);
            for (b, centre) in BAND_HZ.iter().enumerate() {
                let lo = *centre as f64 / std::f64::consts::SQRT_2;
                let hi = *centre as f64 * std::f64::consts::SQRT_2;
                if hz >= lo && hz < hi {
                    sum[b] += power;
                }
            }
        }
    }
    let mut out = [FLOOR_DB; 10];
    for b in 0..10 {
        let mean = sum[b] / starts.len() as f64;
        out[b] = if mean <= 1e-24 { FLOOR_DB } else { (10.0 * mean.log10()) as f32 };
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sine(hz: f32, amp: f32, n: usize) -> Vec<f32> {
        (0..n)
            .map(|i| amp * (2.0 * std::f32::consts::PI * hz * i as f32 / 48000.0).sin())
            .collect()
    }

    #[test]
    fn a_half_scale_sine_reads_minus_six_peak_and_minus_nine_rms() {
        let x = sine(1000.0, 0.5, 48000);
        assert!((peak_db(&x) + 6.02).abs() < 0.05);
        assert!((rms_db(&x) + 9.03).abs() < 0.05);
    }

    #[test]
    fn the_band_holding_a_sine_carries_its_level() {
        let x = sine(1000.0, 0.5, 48000);
        let bands = band_levels_db(&x, 48000.0);
        assert!((bands[5] + 9.03).abs() < 0.5, "1k band read {}", bands[5]);
        assert!(bands[2] < -60.0, "125 Hz band read {}", bands[2]);
        assert!(bands[8] < -60.0, "8k band read {}", bands[8]);
    }

    #[test]
    fn silence_reads_the_floor_and_bad_samples_are_counted() {
        assert_eq!(peak_db(&[0.0; 16]), FLOOR_DB);
        assert_eq!(nonfinite(&[0.0, f32::NAN, f32::INFINITY, 1.0]), 2);
        assert_eq!(band_levels_db(&[0.0; 100], 48000.0), [FLOOR_DB; 10]);
    }

    #[test]
    fn dc_is_the_mean() {
        assert!((dc(&[0.5, 0.5, 0.5]) - 0.5).abs() < 1e-6);
    }
}
