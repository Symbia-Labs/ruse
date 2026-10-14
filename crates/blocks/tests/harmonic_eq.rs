//! The harmonic parametric equalizer, measured at the harmonics and between them.

mod common;
use common::*;
use muse_core::ParamInfo;

const NONE: [ParamInfo; 0] = [];

fn eq(args: &str) -> String {
    format!(r#"{{"block":"harmonic_eq","args":{{{}}}}}"#, args)
}

fn near(got: f32, want: f32, tol: f32, what: &str) {
    assert!((got - want).abs() <= tol, "{}: got {:.3}, wanted {:.3} within {}", what, got, want, tol);
}

#[test]
fn every_harmonic_gets_the_gain_and_the_spaces_between_do_not() {
    let g = eq(r#""fundamental_hz":200,"harmonics":5,"gain_db":12,"bandwidth":0.25"#);
    for k in 1..=5 {
        // The bands are in series, so each one also gets a little of its neighbours' skirts: a bit over 12, never under.
        let got = gain_at(&g, &NONE, &[], 200.0 * k as f32);
        assert!((11.8..13.2).contains(&got), "harmonic {}: {:.2} dB", k, got);
    }
    assert!(gain_at(&g, &NONE, &[], 500.0) < 3.0, "between the 2nd and 3rd the bands have let go");
    near(gain_at(&g, &NONE, &[], 1400.0), 0.0, 0.5, "above the 5th, nothing");
    near(gain_at(&g, &NONE, &[], 60.0), 0.0, 0.3, "below the 1st, nothing");
}

#[test]
fn odd_and_even_harmonics_move_independently() {
    let g = eq(r#""fundamental_hz":200,"harmonics":8,"odd_db":6,"even_db":-6,"bandwidth":0.2"#);
    near(gain_at(&g, &NONE, &[], 200.0), 6.0, 0.4, "1st is odd");
    near(gain_at(&g, &NONE, &[], 400.0), -6.0, 0.4, "2nd is even");
    near(gain_at(&g, &NONE, &[], 600.0), 6.0, 0.4, "3rd");
    near(gain_at(&g, &NONE, &[], 800.0), -6.0, 0.4, "4th");
}

#[test]
fn tilt_brightens_or_darkens_the_overtones_by_the_octave() {
    let g = eq(r#""fundamental_hz":200,"harmonics":8,"tilt_db_oct":6,"bandwidth":0.2"#);
    near(gain_at(&g, &NONE, &[], 200.0), 0.0, 0.3, "the 1st is log2(1) = 0 octaves up");
    near(gain_at(&g, &NONE, &[], 400.0), 6.0, 0.4, "the 2nd is one octave up");
    near(gain_at(&g, &NONE, &[], 800.0), 12.0, 0.5, "the 4th is two");
    near(gain_at(&g, &NONE, &[], 1600.0), 18.0, 0.6, "the 8th is three");
}

#[test]
fn first_and_trims_pick_out_individual_harmonics() {
    let from_second = eq(r#""fundamental_hz":200,"harmonics":6,"gain_db":9,"first":2,"bandwidth":0.2"#);
    near(gain_at(&from_second, &NONE, &[], 200.0), 0.0, 0.5, "the fundamental is left alone");
    near(gain_at(&from_second, &NONE, &[], 400.0), 9.0, 0.4, "and the 2nd is not");

    let trimmed = eq(r#""fundamental_hz":200,"harmonics":6,"trim_3_db":-12,"bandwidth":0.2"#);
    near(gain_at(&trimmed, &NONE, &[], 600.0), -12.0, 0.5, "trim on the 3rd");
    near(gain_at(&trimmed, &NONE, &[], 800.0), 0.0, 0.3, "and none on the 4th");
}

#[test]
fn stretch_moves_the_bands_the_way_a_piano_string_does() {
    // Stretch 0.01 puts the 5th at 200 * 5 * sqrt(1 + 0.01 * 25) = 1118 Hz.
    let g = eq(r#""fundamental_hz":200,"harmonics":5,"first":5,"gain_db":12,"bandwidth":0.1,"stretch":0.01"#);
    near(gain_at(&g, &NONE, &[], 1118.0), 12.0, 0.5, "at the stretched 5th");
    assert!(gain_at(&g, &NONE, &[], 1000.0) < 4.0, "and no longer at the unstretched one");
}

#[test]
fn narrow_bands_take_out_mains_hum_and_its_harmonics() {
    let g = eq(r#""fundamental_hz":50,"harmonics":8,"gain_db":-24,"bandwidth":0.05"#);
    for k in [1.0f32, 3.0, 6.0] {
        assert!(gain_at(&g, &NONE, &[], 50.0 * k) < -20.0, "hum at {} Hz", 50.0 * k);
    }
    near(gain_at(&g, &NONE, &[], 125.0), 0.0, 1.0, "music between the harmonics passes");
    near(gain_at(&g, &NONE, &[], 1000.0), 0.0, 0.3, "and so does everything above");
}

#[test]
fn harmonics_beyond_the_top_of_the_spectrum_are_left_out() {
    let g = eq(r#""fundamental_hz":2000,"harmonics":32,"gain_db":12,"bandwidth":0.25"#);
    near(gain_at(&g, &NONE, &[], 20000.0), 12.0, 1.0, "the 10th still fits under the limit");
    let input = vec![noise(20000, 9)];
    let r = run(&g, &NONE, &[], &input, 100);
    assert!(r.out[0].iter().all(|v| v.is_finite()), "and the rest do not blow up");
}

#[test]
fn sweeping_the_fundamental_does_not_click() {
    let infos = [ParamInfo::knob("f0", "F0", "Hz", 20.0, 2000.0, 100.0)];
    let g = eq(r#""fundamental_hz":"f0","harmonics":12,"gain_db":10,"bandwidth":0.2"#);
    let mut s = Session::new(&g, &infos, 1);
    let tone = sine(440.0, 0.3, 48000);
    let mut out = Vec::new();
    for (i, chunk) in tone.chunks(480).enumerate() {
        s.set("f0", 100.0 + 400.0 * (i as f32 / 100.0));
        let mut block = vec![chunk.to_vec()];
        s.feed(&mut block);
        out.extend(block.remove(0));
    }
    assert!(out.iter().all(|v| v.is_finite() && v.abs() < 5.0));
    let biggest_step = out.windows(2).map(|w| (w[1] - w[0]).abs()).fold(0.0f32, f32::max);
    assert!(biggest_step < 1.5, "no jumps while the bands slide: {}", biggest_step);
}

#[test]
fn it_treats_every_channel_the_same() {
    let g = eq(r#""fundamental_hz":200,"harmonics":4,"gain_db":6"#);
    let input = vec![noise(8000, 3), noise(8000, 3)];
    let r = run(&g, &NONE, &[], &input, 64);
    assert_eq!(r.out[0], r.out[1]);
}
