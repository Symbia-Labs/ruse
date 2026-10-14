//! The Rust stage held to the Swift original. The golden files come from
//! tests/golden/ref/run.sh, which builds Bias's TapeDSP.swift and renders six cases.
mod common;
use common::*;
use muse_tape::{TapeConfig, TapeSettings, TapeStage};

fn read(name: &str) -> Vec<f32> {
    let path = format!("{}/tests/golden/{}", env!("CARGO_MANIFEST_DIR"), name);
    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {}", path, e));
    bytes
        .chunks_exact(4)
        .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
        .collect()
}

/// Compares what the settings work out to against the Swift numbers, so a mismatch in the
/// audio can be traced to the design or to the processing.
fn check_config(name: &str, s: TapeSettings) {
    let path = format!("{}/tests/golden/{}.cfg.txt", env!("CARGO_MANIFEST_DIR"), name);
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {}", path, e));
    let cfg = TapeConfig::new(&s, FS, None);
    let mut worst = (0.0f32, String::new());
    for line in text.lines() {
        let (key, val) = line.split_once(' ').unwrap();
        let want: f32 = val.parse().unwrap();
        let got = match key {
            "drive" => cfg.drive,
            "makeup" => cfg.makeup,
            "output" => cfg.output,
            "hiss" => cfg.hiss,
            "noisePole" => cfg.noise_pole,
            "noiseLow" => cfg.noise_low,
            "noiseNorm" => cfg.noise_norm,
            "bleed" => cfg.bleed,
            "bleedPole" => cfg.bleed_pole,
            "k" => cfg.k,
            "c" => cfg.c,
            "fieldScale" => cfg.field_scale,
            "emphasisB0" => cfg.emphasis_b0,
            "emphasisB1" => cfg.emphasis_b1,
            "emphasisA1" => cfg.emphasis_a1,
            "baseDelay" => cfg.base_delay,
            k if k.starts_with("playback") => cfg.playback[k[8..].parse::<usize>().unwrap()],
            k if k.starts_with("biquad") => cfg.biquads[k[6..].parse::<usize>().unwrap()],
            k if k.starts_with("half") => muse_tape::filters::half_band_taps()[k[4..].parse::<usize>().unwrap()],
            other => panic!("unknown key {}", other),
        };
        let err = (got - want).abs() / want.abs().max(1e-3);
        if err > worst.0 {
            worst = (err, format!("{}: rust {:e}, swift {:e}", key, got, want));
        }
    }
    eprintln!("{} config: worst relative difference {:e} ({})", name, worst.0, worst.1);
    assert!(worst.0 < 1e-3, "{} config differs: {}", name, worst.1);
}

/// `max_tol` and `rms_tol` are absolute. The saturating cases get loose ones because the
/// stage amplifies tiny differences: nudging the input of the Swift original by one part in
/// a million moves its output by about 1.7e-3 on the underbiased case.
fn check(name: &str, s: TapeSettings, max_tol: f32, rms_tol: f64) {
    check_config(name, s);
    let x = read(&format!("{}.in.f32", name));
    let want = read(&format!("{}.out.f32", name));
    assert_eq!(x.len(), want.len());
    let got = render(&mut TapeStage::standalone(FS, 1, s), &x, 0.0);
    let (mut max, mut worst, mut sq) = (0.0f32, 0usize, 0.0f64);
    for i in 0..got.len() {
        let d = (got[i] - want[i]).abs();
        sq += (d as f64) * (d as f64);
        if d > max {
            max = d;
            worst = i;
        }
    }
    let rms = (sq / got.len() as f64).sqrt();
    let peak = want.iter().fold(0.0f32, |a, v| a.max(v.abs()));
    eprintln!("{}: max diff {:e} at sample {}, rms diff {:e}, peak {}", name, max, worst, rms, peak);
    assert!(rms <= rms_tol, "{}: rms diff {:e}", name, rms);
    assert!(max <= max_tol, "{}: max diff {:e} at sample {}", name, max, worst);
}

#[test]
fn golden_linear_quiet() {
    // Nearly linear, but the Swift original still moves by 3.2e-6 when its input is nudged
    // by one part in a million, so agreement to 5e-6 is agreement to its own noise.
    check(
        "linear_quiet",
        TapeSettings {
            wow: 0.0,
            flutter: 0.0,
            hiss_db: -120.0,
            ..TapeSettings::default()
        },
        5e-6,
        2e-6,
    );
}

#[test]
fn golden_default() {
    check("default", TapeSettings::default(), 3e-3, 1e-3);
}

#[test]
fn golden_type1_slow_underbiased() {
    check(
        "type1_slow_underbiased",
        TapeSettings {
            formula: 0,
            speed: 0,
            bias: -0.5,
            input_db: 6.0,
            wow: 0.0,
            flutter: 0.0,
            hiss_db: -120.0,
            ..TapeSettings::default()
        },
        3e-3,
        1.5e-3,
    );
}

#[test]
fn golden_type4_15ips_wow() {
    check(
        "type4_15ips_wow",
        TapeSettings {
            formula: 2,
            speed: 3,
            bias: 0.5,
            wow: 0.2,
            flutter: 0.1,
            hiss_db: -60.0,
            format: 5,
            ..TapeSettings::default()
        },
        3e-3,
        1e-3,
    );
}

#[test]
fn golden_hot_30ips() {
    check(
        "hot_30ips",
        TapeSettings {
            speed: 4,
            formula: 1,
            input_db: 12.0,
            wow: 0.0,
            flutter: 0.0,
            hiss_db: -120.0,
            ..TapeSettings::default()
        },
        3e-3,
        1e-3,
    );
}

#[test]
fn golden_reel_7_5ips_hiss() {
    check(
        "reel_7_5ips_hiss",
        TapeSettings {
            speed: 2,
            formula: 1,
            format: 7,
            hiss_db: -60.0,
            link: false,
            ..TapeSettings::default()
        },
        3e-3,
        1e-3,
    );
}
