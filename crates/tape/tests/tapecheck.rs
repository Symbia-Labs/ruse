//! The claims from Bias's own tapecheck (T1 to T9), held against the Rust port.
mod common;
use common::*;
use muse_tape::{TapeSettings, TapeStage};
use std::f32::consts::PI;

#[test]
fn t1_unity_at_quiet_levels() {
    let g = gain_at(TapeSettings::default(), 1000.0, -30.0);
    assert!(g.abs() < 1.0, "1 kHz at -30 dBFS came out {:.2} dB", g);
}

#[test]
fn t2_third_harmonic_reaches_three_percent_at_the_formulations_level() {
    let want = [-5.0f32, -4.0, -1.0];
    for (formula, want) in want.into_iter().enumerate() {
        let s = TapeSettings {
            formula,
            ..TapeSettings::default()
        };
        let mut found = None;
        let mut last: Option<(f32, f32)> = None;
        let mut level = -24.0f32;
        while level <= 6.0 {
            let d = thd3(s, level);
            if let Some(l) = last {
                if l.1 < 0.03 && d >= 0.03 {
                    found = Some(l.0 + (0.03 - l.1) / (d - l.1) * (level - l.0));
                    break;
                }
            }
            last = Some((level, d));
            level += 0.5;
        }
        let found = found.unwrap_or_else(|| panic!("formula {} never reached 3%", formula));
        assert!((found - want).abs() < 1.5, "formula {}: {:.2} dBFS, wanted {}", formula, found, want);
    }
}

#[test]
fn t4_treble_compression_matches_the_swift_original() {
    let mut diff = [0.0f32; 2];
    for (k, speed) in [1usize, 4].into_iter().enumerate() {
        let s = TapeSettings {
            formula: 0,
            speed,
            ..TapeSettings::default()
        };
        let c1 = gain_at(s, 1000.0, -40.0) - gain_at(s, 1000.0, -10.0);
        let c10 = gain_at(s, 10000.0, -40.0) - gain_at(s, 10000.0, -10.0);
        diff[k] = c10 - c1;
    }
    // Bias's own tapecheck claims at least 3 dB at 3.75 IPS and under 1.5 dB at 30 IPS, and
    // the Swift original measures 1.97 dB and -1.67 dB. The port holds to what the original does.
    assert!((diff[0] - 1.97).abs() < 0.1, "at 3.75 IPS the difference was {:.2} dB, Swift gives 1.97", diff[0]);
    assert!((diff[1] + 1.67).abs() < 0.1, "at 30 IPS the difference was {:.2} dB, Swift gives -1.67", diff[1]);
}

#[test]
fn t5_wow_is_the_right_size_and_two_stages_on_one_clock_agree() {
    let s = TapeSettings {
        wow: 0.2,
        ..quiet(TapeSettings::default())
    };
    let x = sine(3000.0, 0.1, 20.0);
    let a = render(&mut TapeStage::standalone(FS, 1, s), &x, 1_000_000.0);
    let b = render(&mut TapeStage::standalone(FS, 1, s), &x, 1_000_000.0);
    assert!(a == b, "two stages on one clock differ");

    // Demodulate against 3 kHz, smooth over 4 ms, differentiate the phase.
    let w = 2.0 * std::f64::consts::PI * 3000.0 / FS as f64;
    let win = 176usize;
    let (mut sr, mut si) = (0.0f64, 0.0f64);
    let mut phase: Vec<f64> = Vec::new();
    let term = |i: usize| {
        let v = a[i] as f64;
        (v * (w * i as f64).cos(), -v * (w * i as f64).sin())
    };
    for i in 0..a.len() {
        let (r, m) = term(i);
        sr += r;
        si += m;
        if i >= win {
            let (r0, m0) = term(i - win);
            sr -= r0;
            si -= m0;
        }
        if i >= win && i % 44 == 0 {
            phase.push(si.atan2(sr));
        }
    }
    let mut unwrapped = vec![phase[0]];
    let mut prev = phase[0];
    for p in phase.iter().skip(1) {
        let mut d = p - prev;
        while d > std::f64::consts::PI {
            d -= 2.0 * std::f64::consts::PI;
        }
        while d < -std::f64::consts::PI {
            d += 2.0 * std::f64::consts::PI;
        }
        unwrapped.push(unwrapped.last().unwrap() + d);
        prev = *p;
    }
    let dt = 44.0 / FS as f64;
    let skip = (2.0 / dt) as usize;
    let mut peak = 0.0f64;
    for i in (skip + 1)..unwrapped.len() {
        let hz = (unwrapped[i] - unwrapped[i - 1]) / dt / (2.0 * std::f64::consts::PI);
        peak = peak.max(hz.abs() / 3000.0 * 100.0);
    }
    assert!(peak > 0.15 && peak < 0.25, "peak deviation {:.3} %", peak);
}

#[test]
fn t6_hiss_is_where_it_is_set() {
    let s = TapeSettings {
        formula: 0,
        wow: 0.0,
        flutter: 0.0,
        hiss_db: -60.0,
        ..TapeSettings::default()
    };
    let y = render(&mut TapeStage::standalone(FS, 1, s), &vec![0.0; (2.0 * FS) as usize], 0.0);
    let tail = &y[FS as usize..];
    let rms = (tail.iter().map(|v| v * v).sum::<f32>() / tail.len() as f32).sqrt();
    assert!((db(rms) + 60.0).abs() < 1.5, "hiss came out {:.2} dBFS", db(rms));
}

#[test]
fn t7_abuse_leaves_finite_output_and_no_dc() {
    let mut x: Vec<f32> = Vec::new();
    for i in 0..FS as usize {
        x.push(if (i / 220) % 2 == 0 { 1.0 } else { -1.0 });
    }
    for i in 0..(FS / 2.0) as usize {
        x.push(if i % 1000 == 0 { 1.0 } else { 0.0 });
    }
    x.extend(std::iter::repeat_n(0.0, (FS / 2.0) as usize));
    x.extend([f32::NAN, f32::INFINITY, f32::NEG_INFINITY, 1e30, -1e30]);
    x.extend(sine(440.0, 1.0, 0.5));
    x.extend(std::iter::repeat_n(0.0, (1.25 * FS) as usize));
    let s = TapeSettings {
        input_db: 12.0,
        ..TapeSettings::default()
    };
    let y = render(&mut TapeStage::standalone(FS, 1, s), &x, 0.0);
    assert!(y.iter().all(|v| v.is_finite()), "non-finite output");
    let peak = y.iter().fold(0.0f32, |a, v| a.max(v.abs()));
    assert!(db(peak) < 12.0, "peak {:.2} dBFS", db(peak));
    let tail = &y[y.len() - (0.25 * FS) as usize..];
    let dc = (tail.iter().sum::<f32>() / tail.len() as f32).abs();
    assert!(db(dc) < -80.0, "DC {:.2} dBFS", db(dc));
}

#[test]
fn t9_latency_is_23_samples_and_reported() {
    let mut x = vec![0.0f32; 4000];
    x[1000] = 0.01;
    let s = quiet(TapeSettings::default());
    let mut stage = TapeStage::standalone(FS, 1, s);
    let y = render(&mut stage, &x, 0.0);
    let mut at = 0;
    for i in 0..y.len() {
        if y[i].abs() > y[at].abs() {
            at = i;
        }
    }
    let late = at as i64 - 1000;
    assert!((late - 23).abs() <= 2, "impulse peak {} samples late", late);
    assert_eq!(stage.latency_samples(), 23);
}

#[test]
fn a_tone_arriving_after_silence_comes_out_at_the_right_level() {
    let s = quiet(TapeSettings::default());
    let mut x = vec![0.0f32; (2.0 * FS) as usize];
    x.extend(sine(1000.0, 0.0316, 1.0));
    let y = render(&mut TapeStage::standalone(FS, 1, s), &x, 0.0);
    let g = db(tone(&y, 1000.0, (2.5 * FS) as usize) / 0.0316);
    assert!(g.abs() < 1.0, "{:.2} dB", g);
    let _ = PI;
}
