//! One instance, many tracks: independence when unlinked, and bleed between neighbours when linked.
mod common;
use common::*;
use plugkit_tape::{TapeSettings, TapeStage};

fn render_n(stage: &mut TapeStage, inputs: &[Vec<f32>], clock: f64) -> Vec<Vec<f32>> {
    let mut out: Vec<Vec<f32>> = inputs.to_vec();
    let len = out[0].len();
    let mut at = 0;
    while at < len {
        let n = BLOCK.min(len - at);
        {
            let mut chans: Vec<&mut [f32]> = out.iter_mut().map(|c| &mut c[at..at + n]).collect();
            stage.process(&mut chans, Some(clock + at as f64));
        }
        at += n;
    }
    out
}

#[test]
fn unlinked_tracks_match_the_same_signals_on_separate_mono_stages() {
    let s = TapeSettings {
        link: false,
        ..quiet(TapeSettings::default())
    };
    let inputs: Vec<Vec<f32>> = (0..24)
        .map(|c| sine(200.0 + 150.0 * c as f32, 0.05 + 0.03 * (c % 5) as f32, 0.25))
        .collect();
    let together = render_n(&mut TapeStage::standalone(FS, 24, s), &inputs, 0.0);
    for c in 0..24 {
        let alone = render(&mut TapeStage::standalone(FS, 1, s), &inputs[c], 0.0);
        assert!(together[c] == alone, "track {} differs from its own mono stage", c);
    }
}

#[test]
fn bleed_reaches_the_neighbours_and_nobody_else() {
    // Cassette format bleeds -45 dB at 1 kHz.
    let s = quiet(TapeSettings::default());
    let n = (1.5 * FS) as usize;
    let mut inputs = vec![vec![0.0f32; n]; 24];
    inputs[5] = sine(1000.0, 0.2, 1.5);
    let out = render_n(&mut TapeStage::standalone(FS, 24, s), &inputs, 0.0);
    let from = FS as usize / 2;
    let level = |c: usize| tone(&out[c], 1000.0, from);
    let main = level(5);
    let rel = |c: usize| db(level(c) / main);
    assert!((rel(4) + 45.0).abs() < 2.0, "track 4 sits {:.1} dB below track 5", rel(4));
    assert!((rel(6) + 45.0).abs() < 2.0, "track 6 sits {:.1} dB below track 5", rel(6));
    for c in [0usize, 2, 3, 7, 8, 12, 23] {
        assert!(level(c) < 1e-6, "track {} picked up {:e}", c, level(c));
    }
}

#[test]
fn unlinked_tracks_do_not_bleed() {
    let s = TapeSettings {
        link: false,
        ..quiet(TapeSettings::default())
    };
    let n = FS as usize;
    let mut inputs = vec![vec![0.0f32; n]; 8];
    inputs[3] = sine(1000.0, 0.2, 1.0);
    let out = render_n(&mut TapeStage::standalone(FS, 8, s), &inputs, 0.0);
    assert!(tone(&out[2], 1000.0, 4000) < 1e-6);
    assert!(tone(&out[4], 1000.0, 4000) < 1e-6);
}

#[test]
fn a_middle_track_hears_both_neighbours_and_the_sum_is_linear() {
    let s = quiet(TapeSettings::default());
    let n = (0.5 * FS) as usize;
    let zero = vec![0.0f32; n];
    let a = sine(440.0, 0.2, 0.5);
    let b = sine(1230.0, 0.1, 0.5);
    let only_below = render_n(&mut TapeStage::standalone(FS, 3, s), &[a.clone(), zero.clone(), zero.clone()], 0.0);
    let only_above = render_n(&mut TapeStage::standalone(FS, 3, s), &[zero.clone(), zero.clone(), b.clone()], 0.0);
    let both = render_n(&mut TapeStage::standalone(FS, 3, s), &[a, zero, b], 0.0);
    let peak = both[1].iter().fold(0.0f32, |m, v| m.max(v.abs()));
    assert!(peak > 1e-4, "the middle track heard nothing");
    for i in 0..n {
        let sum = only_below[1][i] + only_above[1][i];
        assert!((both[1][i] - sum).abs() < 1e-6, "sample {}: {} against {}", i, both[1][i], sum);
    }
}
