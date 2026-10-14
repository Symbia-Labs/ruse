//! What is per channel and what is global in Bias Tape, from the plugin's parameters to the audio.
mod common;
use common::*;
use plugkit_core::json::describe_params;
use plugkit_core::{Plugin, ProcessContext, Scope, Setup};
use plugkit_tape::BiasTape;

fn setup(channels: u32) -> Setup {
    Setup {
        sample_rate: FS,
        max_block: 512,
        channels,
    }
}

/// Runs the same 1 kHz tone on every channel for `seconds` and returns each channel's output.
fn run_tone(p: &mut BiasTape, channels: usize, amp: f32, seconds: f32) -> Vec<Vec<f32>> {
    let x = sine(1000.0, amp, seconds);
    let mut out: Vec<Vec<f32>> = (0..channels).map(|_| x.clone()).collect();
    let ctx = ProcessContext::offline(FS);
    let mut at = 0;
    while at < x.len() {
        let n = BLOCK.min(x.len() - at);
        {
            let mut chans: Vec<&mut [f32]> = out.iter_mut().map(|c| &mut c[at..at + n]).collect();
            p.process(&mut chans, &ctx);
        }
        at += n;
    }
    out
}

fn level_db(y: &[f32]) -> f32 {
    db(tone(y, 1000.0, (0.5 * FS) as usize))
}

#[test]
fn the_plugin_declares_which_parameters_are_per_channel() {
    let p = BiasTape::new();
    for id in ["input", "bias", "output", "link"] {
        assert_eq!(p.params().by_id(id).unwrap().info().scope, Scope::PerChannel, "{}", id);
    }
    for id in ["format", "speed", "tape", "wow", "flutter", "hiss"] {
        assert_eq!(p.params().by_id(id).unwrap().info().scope, Scope::Global, "{}", id);
    }
}

#[test]
fn machine_settings_refuse_a_channel_and_track_settings_take_one() {
    let mut p = BiasTape::new();
    p.prepare(&setup(4));
    assert!(p.params().set_plain_ch("speed", Some(1), 3.0).is_err());
    assert!(p.params().set_plain_ch("speed", None, 3.0).is_ok());
    assert!(p.params().set_plain_ch("input", Some(1), 6.0).is_ok());
    assert!(p.params().set_plain_ch("input", Some(4), 6.0).is_err());
    assert_eq!(p.settings_for(1).input_db, 6.0);
    assert_eq!(p.settings_for(0).input_db, 0.0);
    assert_eq!(p.settings_for(1).speed, 3);
    assert_eq!(p.settings_for(0).speed, 3);
}

#[test]
fn the_description_lists_a_value_for_each_track() {
    let mut p = BiasTape::new();
    p.prepare(&setup(3));
    p.params().set_plain_ch("bias", Some(2), -40.0).unwrap();
    let j = describe_params(p.params());
    assert!(j.contains("\"channels\":3"));
    assert!(j.contains("\"texts\":[\"Aligned\",\"Aligned\",\"Under 40\"]"), "{}", j);
}

#[test]
fn input_set_on_one_channel_changes_only_that_channel() {
    let mut p = BiasTape::new();
    p.prepare(&setup(4));
    // Quiet and unlinked, so the only difference between channels is the setting under test.
    p.params().set_plain("wow", 0.0).unwrap();
    p.params().set_plain("flutter", 0.0).unwrap();
    p.params().set_plain("hiss", -96.0).unwrap();
    p.params().set_plain("link", 0.0).unwrap();
    p.params().set_plain_ch("input", Some(2), 6.0).unwrap();
    let out = run_tone(&mut p, 4, 0.003, 1.2);
    let base = level_db(&out[0]);
    let hot = level_db(&out[2]);
    assert!(((hot - base) - 6.0).abs() < 0.5, "channel 2 sits {:.2} dB above channel 0", hot - base);
    assert!((level_db(&out[1]) - base).abs() < 0.1);
    assert!((level_db(&out[3]) - base).abs() < 0.1);
}

#[test]
fn writing_across_channels_changes_every_channel() {
    let mut p = BiasTape::new();
    p.prepare(&setup(4));
    p.params().set_plain("wow", 0.0).unwrap();
    p.params().set_plain("flutter", 0.0).unwrap();
    p.params().set_plain("hiss", -96.0).unwrap();
    p.params().set_plain("link", 0.0).unwrap();
    p.params().set_plain("output", -6.0).unwrap();
    let out = run_tone(&mut p, 4, 0.003, 1.2);
    let levels: Vec<f32> = out.iter().map(|y| level_db(y)).collect();
    for l in &levels {
        assert!((l - levels[0]).abs() < 0.05);
    }
    // Quiet tone through an aligned tape comes back at its own level, less the 6 dB of output.
    assert!(((levels[0] - db(0.003)) + 6.0).abs() < 1.0, "{:.2}", levels[0] - db(0.003));
}

#[test]
fn a_track_with_link_off_neither_gives_nor_takes_bleed() {
    let mut p = BiasTape::new();
    p.prepare(&setup(3));
    p.params().set_plain("wow", 0.0).unwrap();
    p.params().set_plain("flutter", 0.0).unwrap();
    p.params().set_plain("hiss", -96.0).unwrap();
    // Track 0 is unlinked. Tracks 1 and 2 are linked.
    p.params().set_plain_ch("link", Some(0), 0.0).unwrap();

    let n = (1.5 * FS) as usize;
    let mut out: Vec<Vec<f32>> = vec![vec![0.0; n]; 3];
    out[1] = sine(1000.0, 0.2, 1.5);
    let ctx = ProcessContext::offline(FS);
    let mut at = 0;
    while at < n {
        let m = BLOCK.min(n - at);
        {
            let mut chans: Vec<&mut [f32]> = out.iter_mut().map(|c| &mut c[at..at + m]).collect();
            p.process(&mut chans, &ctx);
        }
        at += m;
    }
    let from = FS as usize / 2;
    let main = tone(&out[1], 1000.0, from);
    let to_linked = db(tone(&out[2], 1000.0, from) / main);
    assert!((to_linked + 45.0).abs() < 2.0, "linked neighbour sits {:.1} dB below", to_linked);
    assert!(tone(&out[0], 1000.0, from) < 1e-6, "the unlinked track picked up bleed");

    // And the other way: an unlinked track's own signal reaches no one.
    let mut p2 = BiasTape::new();
    p2.prepare(&setup(3));
    p2.params().set_plain("wow", 0.0).unwrap();
    p2.params().set_plain("flutter", 0.0).unwrap();
    p2.params().set_plain("hiss", -96.0).unwrap();
    p2.params().set_plain_ch("link", Some(1), 0.0).unwrap();
    let mut out2: Vec<Vec<f32>> = vec![vec![0.0; n]; 3];
    out2[1] = sine(1000.0, 0.2, 1.5);
    let mut at = 0;
    while at < n {
        let m = BLOCK.min(n - at);
        {
            let mut chans: Vec<&mut [f32]> = out2.iter_mut().map(|c| &mut c[at..at + m]).collect();
            p2.process(&mut chans, &ctx);
        }
        at += m;
    }
    assert!(tone(&out2[0], 1000.0, from) < 1e-6);
    assert!(tone(&out2[2], 1000.0, from) < 1e-6);
}

#[test]
fn channels_with_different_bias_get_different_sound() {
    let mut p = BiasTape::new();
    p.prepare(&setup(2));
    p.params().set_plain("wow", 0.0).unwrap();
    p.params().set_plain("flutter", 0.0).unwrap();
    p.params().set_plain("hiss", -96.0).unwrap();
    p.params().set_plain("link", 0.0).unwrap();
    p.params().set_plain("speed", 0.0).unwrap();
    p.params().set_plain_ch("bias", Some(1), -80.0).unwrap();
    // A loud tone, where bias changes how hard the tape saturates.
    let out = run_tone(&mut p, 2, 0.5, 1.0);
    let differ = out[0].iter().zip(&out[1]).any(|(a, b)| (a - b).abs() > 1e-3);
    assert!(differ, "an aligned and an underbiased track sounded the same");
}
