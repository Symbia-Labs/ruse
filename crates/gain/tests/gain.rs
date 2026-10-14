use plugkit_core::json::describe_params;
use plugkit_core::{Plugin, ProcessContext, Setup};
use plugkit_gain::Gain;

fn setup(channels: u32) -> Setup {
    Setup {
        sample_rate: 48000.0,
        max_block: 512,
        channels,
    }
}

/// Renders `n` frames of a constant on every channel and returns channel 0.
fn render(p: &mut Gain, channels: usize, input: f32, n: usize) -> Vec<Vec<f32>> {
    let mut bufs: Vec<Vec<f32>> = (0..channels).map(|_| vec![input; n]).collect();
    {
        let mut chans: Vec<&mut [f32]> = bufs.iter_mut().map(|b| &mut b[..]).collect();
        p.process(&mut chans, &ProcessContext::offline(48000.0));
    }
    bufs
}

#[test]
fn unity_by_default() {
    let mut p = Gain::new();
    p.prepare(&setup(2));
    let out = render(&mut p, 2, 0.5, 64);
    assert!(out[0].iter().all(|s| (*s - 0.5).abs() < 1e-6));
}

#[test]
fn minus_six_db_halves_the_level() {
    let mut p = Gain::new();
    p.prepare(&setup(2));
    p.params().set_plain("gain", -6.0206).unwrap();
    let out = render(&mut p, 2, 0.5, 64);
    assert!(out[0].iter().all(|s| (*s - 0.25).abs() < 1e-4));
}

#[test]
fn twenty_four_channels_all_get_the_gain() {
    let mut p = Gain::new();
    p.prepare(&setup(24));
    p.params().set_plain("gain", -6.0206).unwrap();
    let out = render(&mut p, 24, 0.5, 64);
    assert_eq!(out.len(), 24);
    for ch in &out {
        assert!(ch.iter().all(|s| (*s - 0.25).abs() < 1e-4));
    }
}

#[test]
fn layout_accepts_many_channels() {
    let p = Gain::new();
    assert!(p.layout().accepts(&[1], &[1]));
    assert!(p.layout().accepts(&[24], &[24]));
    assert!(!p.layout().accepts(&[24], &[2]));
}

#[test]
fn setter_clamps_to_the_range() {
    let p = Gain::new();
    assert_eq!(p.params().set_plain("gain", 99.0).unwrap(), 12.0);
    assert_eq!(p.params().set_plain("gain", -999.0).unwrap(), -60.0);
}

#[test]
fn unknown_id_is_an_error() {
    let p = Gain::new();
    assert!(p.params().set_plain("nope", 1.0).is_err());
}

#[test]
fn description_names_the_parameter() {
    let p = Gain::new();
    let j = describe_params(p.params());
    assert!(j.contains("\"id\":\"gain\""));
    assert!(j.contains("\"unit\":\"dB\""));
    assert!(j.contains("\"curve\":\"linear\""));
    assert!(j.contains("\"group\":\"Output\""));
}
