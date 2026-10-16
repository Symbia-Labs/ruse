#![allow(dead_code)]
use ruse_tape::{TapeSettings, TapeStage};
use std::f32::consts::PI;

pub const FS: f32 = 44100.0;
pub const BLOCK: usize = 128;

pub fn quiet(s: TapeSettings) -> TapeSettings {
    TapeSettings {
        wow: 0.0,
        flutter: 0.0,
        hiss_db: -120.0,
        ..s
    }
}

/// Runs a mono signal through a stage in blocks; `clock` starts the transport's sample clock.
pub fn render(stage: &mut TapeStage, x: &[f32], clock: f64) -> Vec<f32> {
    let mut y = x.to_vec();
    let mut at = 0;
    while at < y.len() {
        let n = BLOCK.min(y.len() - at);
        {
            let mut chans: [&mut [f32]; 1] = [&mut y[at..at + n]];
            stage.process(&mut chans, Some(clock + at as f64));
        }
        at += n;
    }
    y
}

pub fn sine(f: f32, amp: f32, seconds: f32) -> Vec<f32> {
    (0..(seconds * FS) as usize)
        .map(|i| amp * (2.0 * PI * f * i as f32 / FS).sin())
        .collect()
}

/// Amplitude of the component at f in y[from..], over whole cycles only.
pub fn tone(y: &[f32], f: f32, from: usize) -> f32 {
    let (mut s, mut c) = (0.0f64, 0.0f64);
    let w = 2.0 * std::f64::consts::PI * f as f64 / FS as f64;
    let cycles = ((y.len() - from) as f64 * f as f64 / FS as f64).floor();
    let n = (cycles * FS as f64 / f as f64) as usize;
    for i in 0..n {
        let v = y[from + i] as f64;
        s += v * (w * (from + i) as f64).sin();
        c += v * (w * (from + i) as f64).cos();
    }
    (2.0 * (s * s + c * c).sqrt() / n as f64) as f32
}

pub fn db(v: f32) -> f32 {
    if v > 0.0 {
        20.0 * v.log10()
    } else {
        -200.0
    }
}

pub fn gain_at(s: TapeSettings, f: f32, level_db: f32) -> f32 {
    let mut stage = TapeStage::standalone(FS, 1, quiet(s));
    let amp = 10f32.powf(level_db / 20.0);
    let y = render(&mut stage, &sine(f, amp, 1.0), 0.0);
    db(tone(&y, f, (0.5 * FS) as usize) / amp)
}

pub fn thd3(s: TapeSettings, level_db: f32) -> f32 {
    let mut stage = TapeStage::standalone(FS, 1, quiet(s));
    let y = render(&mut stage, &sine(1000.0, 10f32.powf(level_db / 20.0), 1.0), 0.0);
    let a1 = tone(&y, 1000.0, (0.5 * FS) as usize);
    let a3 = tone(&y, 3000.0, (0.5 * FS) as usize);
    if a1 > 0.0 {
        a3 / a1
    } else {
        0.0
    }
}
