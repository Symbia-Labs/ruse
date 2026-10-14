//! Bias Tape as a plugin: layout, parameters, presets, and the path from a parameter to the audio thread.
use plugkit_core::json::describe_params;
use plugkit_core::{Plugin, ProcessContext, Setup};
use plugkit_tape::{BiasTape, PRESETS};

fn setup(channels: u32) -> Setup {
    Setup {
        sample_rate: 44100.0,
        max_block: 512,
        channels,
    }
}

fn run(p: &mut BiasTape, channels: usize, blocks: usize) {
    let mut bufs: Vec<Vec<f32>> = (0..channels).map(|_| vec![0.1f32; 128]).collect();
    let ctx = ProcessContext::offline(44100.0);
    for _ in 0..blocks {
        let mut chans: Vec<&mut [f32]> = bufs.iter_mut().map(|b| &mut b[..]).collect();
        p.process(&mut chans, &ctx);
    }
}

#[test]
fn the_layout_takes_up_to_64_tracks() {
    let p = BiasTape::new();
    assert!(p.layout().accepts(&[1], &[1]));
    assert!(p.layout().accepts(&[24], &[24]));
    assert!(!p.layout().accepts(&[24], &[2]));
    assert!(!p.layout().accepts(&[65], &[65]));
}

#[test]
fn the_description_carries_groups_choices_and_display_text() {
    let p = BiasTape::new();
    let j = describe_params(p.params());
    assert!(j.contains("\"group\":\"Track\""));
    assert!(j.contains("\"group\":\"Machine\""));
    assert!(j.contains("\"id\":\"speed\""));
    assert!(j.contains("15 IPS"));
    assert!(j.contains("\"text\":\"Aligned\""));
    assert!(j.contains("\"kind\":\"toggle\""));
}

#[test]
fn the_defaults_match_bias() {
    let p = BiasTape::new();
    assert_eq!(p.settings(), plugkit_tape::TapeSettings::default());
}

#[test]
fn a_preset_sets_the_parameters_and_keeps_the_link() {
    let p = BiasTape::new();
    p.params().set_plain("link", 0.0).unwrap();
    p.apply_preset(3).unwrap();
    let s = p.settings();
    assert_eq!(s.speed, 3);
    assert_eq!(s.format, 2);
    assert!(!s.link);
    assert_eq!(p.params().by_id("speed").unwrap().text(), "15 IPS");
    assert_eq!(PRESETS[3].name, "Half-inch 15");
    assert!(p.apply_preset(99).is_err());
}

#[test]
fn a_parameter_change_reaches_the_audio_thread() {
    let mut p = BiasTape::new();
    p.prepare(&setup(24));
    run(&mut p, 24, 2);
    let before = p.latency_samples();
    p.params().set_plain("wow", 1.0).unwrap();
    p.params().set_plain("flutter", 1.0).unwrap();
    run(&mut p, 24, 2);
    let after = p.latency_samples();
    assert!(after > before, "latency stayed at {} after wow and flutter went up", before);
}

#[test]
fn processing_before_prepare_does_nothing_and_does_not_panic() {
    let mut p = BiasTape::new();
    run(&mut p, 2, 1);
}
