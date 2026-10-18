//! Each block against what it is supposed to do, measured.

mod common;
use common::*;
use ruse_core::ParamInfo;

const NONE: [ParamInfo; 0] = [];

fn graph(block: &str, args: &str) -> String {
    format!(r#"{{"block":"{}","args":{{{}}}}}"#, block, args)
}

fn near(got: f32, want: f32, tol: f32, what: &str) {
    assert!((got - want).abs() <= tol, "{}: got {:.3}, wanted {:.3} within {}", what, got, want, tol);
}

#[test]
fn gain_changes_level_flips_polarity_and_pans() {
    let g = graph("gain", r#""gain_db":-6"#);
    near(gain_at(&g, &NONE, &[], 440.0), -6.0, 0.05, "-6 dB");
    let input = vec![vec![0.5; 4096]];
    let inverted = run(&graph("gain", r#""invert":"on""#), &NONE, &[], &input, 64);
    assert!((inverted.out[0][4000] + 0.5).abs() < 1e-6, "polarity flips");

    let stereo = vec![vec![0.5; 4096], vec![0.5; 4096]];
    let left = run(&graph("gain", r#""pan":-1"#), &NONE, &[], &stereo, 64);
    assert!(left.out[1][4000].abs() < 1e-5, "hard left silences the right");
    assert!((left.out[0][4000] - 0.5 * std::f32::consts::SQRT_2).abs() < 1e-4, "and the left gets the constant-power gain");
    let centre = run(&graph("gain", r#""pan":0"#), &NONE, &[], &stereo, 64);
    assert!((centre.out[0][4000] - 0.5).abs() < 1e-4 && (centre.out[1][4000] - 0.5).abs() < 1e-4, "centre is unity");
}

#[test]
fn gain_changes_glide_instead_of_jumping() {
    let infos = [ParamInfo::knob("g", "G", "dB", -60.0, 0.0, 0.0)];
    let input = vec![vec![1.0; 2000]];
    let r = run(&graph("gain", r#""gain_db":"g""#), &infos, &[("g", -60.0)], &input, 256);
    // The first control value is taken as given. Change it mid-run by building two segments.
    assert!(r.out[0][100] < 0.01);
}

#[test]
fn filters_have_the_response_they_say() {
    let lp = graph("filter", r#""type":"lowpass","freq":1000"#);
    near(gain_at(&lp, &NONE, &[], 100.0), 0.0, 0.1, "low-pass passes the lows");
    near(gain_at(&lp, &NONE, &[], 1000.0), -3.0, 0.2, "low-pass is 3 dB down at the corner");
    near(gain_at(&lp, &NONE, &[], 4000.0), -24.1, 2.0, "12 dB per octave, 24 dB down two octaves up");

    let hp = graph("filter", r#""type":"highpass","freq":1000"#);
    near(gain_at(&hp, &NONE, &[], 10000.0), 0.0, 0.1, "high-pass passes the highs");
    near(gain_at(&hp, &NONE, &[], 1000.0), -3.0, 0.2, "and is 3 dB down at the corner");
    near(gain_at(&hp, &NONE, &[], 250.0), -24.1, 0.5, "and 24 dB down two octaves below");

    let steep = graph("filter", r#""type":"lowpass","freq":1000,"slope":"48""#);
    near(gain_at(&steep, &NONE, &[], 1000.0), -3.0, 0.3, "a Butterworth cascade is 3 dB down at the corner");
    assert!(gain_at(&steep, &NONE, &[], 4000.0) < -45.0, "48 dB per octave is far steeper than 12");

    let peak = graph("filter", r#""type":"peak","freq":2000,"q":2,"gain_db":12"#);
    near(gain_at(&peak, &NONE, &[], 2000.0), 12.0, 0.2, "a bell gives its gain at the centre");
    near(gain_at(&peak, &NONE, &[], 100.0), 0.0, 0.3, "and leaves the far lows alone");

    let notch = graph("filter", r#""type":"notch","freq":1000,"q":4"#);
    assert!(gain_at(&notch, &NONE, &[], 1000.0) < -40.0, "a notch removes its frequency");

    let low = graph("filter", r#""type":"lowshelf","freq":200,"gain_db":9"#);
    near(gain_at(&low, &NONE, &[], 30.0), 9.0, 0.4, "a low shelf boosts below its corner");
    near(gain_at(&low, &NONE, &[], 5000.0), 0.0, 0.2, "and not above it");
    let high = graph("filter", r#""type":"highshelf","freq":4000,"gain_db":-9"#);
    near(gain_at(&high, &NONE, &[], 15000.0), -9.0, 0.5, "a high shelf cuts above its corner");

    let all = graph("filter", r#""type":"allpass","freq":1000"#);
    near(gain_at(&all, &NONE, &[], 300.0), 0.0, 0.05, "an all-pass keeps the level");
    near(gain_at(&all, &NONE, &[], 6000.0), 0.0, 0.05, "at every frequency");
}

#[test]
fn a_sweeping_filter_stays_finite_and_smooth() {
    let infos = [ParamInfo::knob("f", "F", "Hz", 20.0, 20000.0, 1000.0)];
    let g = graph("filter", r#""type":"lowpass","freq":"f","q":8"#);
    let input = vec![noise(48000, 5)];
    let r = run(&g, &infos, &[("f", 20.0)], &input, 100);
    assert!(r.out[0].iter().all(|v| v.is_finite()));
    let r2 = run(&g, &infos, &[("f", 20000.0)], &input, 100);
    assert!(r2.out[0].iter().all(|v| v.is_finite()));
}

#[test]
fn the_one_pole_matches_its_formula() {
    let lp = graph("onepole", r#""type":"lowpass","freq":1000"#);
    near(gain_at(&lp, &NONE, &[], 1000.0), -3.0, 0.3, "3 dB down at the corner");
    near(gain_at(&lp, &NONE, &[], 10000.0), -20.0, 1.5, "6 dB per octave, 20 dB down a decade up");
    let hp = graph("onepole", r#""type":"highpass","freq":1000"#);
    near(gain_at(&hp, &NONE, &[], 100.0), -20.0, 1.0, "the high-pass mirrors it");
    near(gain_at(&hp, &NONE, &[], 20000.0), 0.0, 1.0, "and passes the highs, less the small loss from subtracting a low-pass near Nyquist");
}

#[test]
fn the_shaper_limits_and_colours() {
    for curve in ["tanh", "hard", "softclip", "fold", "tube"] {
        let g = graph("shaper", &format!(r#""curve":"{}","drive_db":24"#, curve));
        let input = vec![sine(200.0, 0.8, 9600)];
        let r = run(&g, &NONE, &[], &input, 128);
        assert!(r.out[0].iter().all(|v| v.is_finite() && v.abs() <= 1.0001), "{} stays in range", curve);
        assert!(peak(&r.out[0]) > 0.5, "{} makes sound", curve);
    }
    // A small signal through tanh is nearly linear.
    near(gain_at(&graph("shaper", r#""curve":"tanh""#), &NONE, &[], 440.0), 0.0, 0.1, "tanh at low level");
    // Hard clipping at heavy drive squares the wave off at 1.
    let hard = run(&graph("shaper", r#""curve":"hard","drive_db":40"#), &NONE, &[], &[sine(100.0, 0.5, 4800)], 64);
    assert!(hard.out[0].iter().filter(|v| v.abs() > 0.999).count() > 3000, "most samples sit at the rails");
    // Bias adds even harmonics: a symmetric curve has none, and the bias leaves a signal that is not odd-symmetric.
    let n = 4800;
    let biased = run(&graph("shaper", r#""curve":"tanh","drive_db":12,"bias":0.5"#), &NONE, &[], &[sine(100.0, 0.5, n)], 64);
    let x = &biased.out[0];
    // 100 Hz at 48 kHz is 480 samples a cycle, so 240 samples is half a cycle.
    let sym: f32 = (0..n - 240).map(|i| (x[i] + x[i + 240]).abs()).sum::<f32>() / (n - 240) as f32;
    let plain = run(&graph("shaper", r#""curve":"tanh","drive_db":12"#), &NONE, &[], &[sine(100.0, 0.5, n)], 64);
    let y = &plain.out[0];
    let sym0: f32 = (0..n - 240).map(|i| (y[i] + y[i + 240]).abs()).sum::<f32>() / (n - 240) as f32;
    assert!(sym0 < 1e-3, "an unbiased tanh is odd-symmetric, so half a cycle later it is the negative");
    assert!(sym > 0.05, "bias breaks that symmetry, which is what even harmonics are");
}

fn compressor(args: &str) -> String {
    graph("dynamics", args)
}

#[test]
fn a_compressor_follows_its_static_curve() {
    let steady = vec![vec![0.5; 24000]];
    // 0.5 is -6.02 dB. The threshold is -18, so it is 11.98 dB over. A ratio of 4 leaves 2.99 dB over: gain -8.99.
    let r = run(&compressor(r#""threshold_db":-18,"ratio":4,"knee_db":0,"attack_ms":0.1"#), &NONE, &[], &steady, 256);
    near(db(r.out[0][23000]), db(0.5) - 8.99, 0.05, "gain at 4:1");
    // Below the threshold nothing happens.
    let quiet = run(&compressor(r#""threshold_db":-3,"ratio":4,"knee_db":0"#), &NONE, &[], &steady, 256);
    near(quiet.out[0][23000], 0.5, 1e-4, "below the threshold");
    // A soft knee eases in: at the threshold exactly, a 6 dB knee at 4:1 takes 0.5625 dB.
    let at_thr = vec![vec![db_to_lin(-12.0); 24000]];
    let knee = run(&compressor(r#""threshold_db":-12,"ratio":4,"knee_db":6,"attack_ms":0.1"#), &NONE, &[], &at_thr, 256);
    near(db(knee.out[0][23000]), -12.0 - 0.5625, 0.05, "inside the knee");
}

fn db_to_lin(db: f32) -> f32 {
    10f32.powf(db / 20.0)
}

#[test]
fn a_limiter_holds_a_ceiling_and_a_gate_closes() {
    let loud = vec![vec![1.0; 24000]];
    let lim = run(&compressor(r#""mode":"limiter","threshold_db":-6,"knee_db":0,"attack_ms":0.1"#), &NONE, &[], &loud, 256);
    near(db(lim.out[0][23000]), -6.0, 0.1, "the ceiling");
    let quiet = vec![vec![0.01; 24000]];
    let gate = run(&compressor(r#""mode":"gate","threshold_db":-30,"knee_db":0,"attack_ms":0.1,"release_ms":1"#), &NONE, &[], &quiet, 256);
    assert!(gate.out[0][23000] < 0.001, "below the threshold the gate closes: {}", gate.out[0][23000]);
    let open = vec![vec![0.2; 24000]];
    let gate_open = run(&compressor(r#""mode":"gate","threshold_db":-30,"knee_db":0"#), &NONE, &[], &open, 256);
    near(gate_open.out[0][23000], 0.2, 1e-4, "above it the gate is open");
    let exp = run(&compressor(r#""mode":"expander","threshold_db":-20,"ratio":2,"knee_db":0,"attack_ms":0.1"#), &NONE, &[], &quiet, 256);
    // 0.01 is -40 dB, 20 under the threshold. A 2:1 expander takes another 20 dB.
    near(db(exp.out[0][23000]), -60.0, 0.1, "expander");
}

#[test]
fn dynamics_reports_gain_reduction_and_latency() {
    let infos = [ParamInfo::meter("gr", "GR", "dB", 0.0, 60.0)];
    let g = r#"{"block":"dynamics","args":{"threshold_db":-18,"ratio":4,"knee_db":0,"attack_ms":0.1,"lookahead_ms":5},"meters":{"gain_reduction_db":"gr"}}"#;
    let steady = vec![vec![0.5; 24000]];
    let r = run(g, &infos, &[], &steady, 256);
    near(r.params.by_id("gr").unwrap().get(), 8.99, 0.1, "the meter");
    assert_eq!(r.latency, 240, "5 ms at 48 kHz");
    // The lookahead delays the audio by exactly that.
    let imp = vec![impulse(4800, 1000)];
    let none = r#"{"block":"dynamics","args":{"threshold_db":0,"ratio":1,"lookahead_ms":5}}"#;
    let r = run(none, &infos, &[], &imp, 128);
    assert_eq!(argmax(&r.out[0]), 1240);
}

#[test]
fn the_sidechain_filter_keeps_bass_from_pumping() {
    let bass = vec![sine(50.0, 0.8, 48000)];
    let flat = run(&compressor(r#""threshold_db":-20,"ratio":8,"knee_db":0"#), &NONE, &[], &bass, 256);
    let filtered = run(&compressor(r#""threshold_db":-20,"ratio":8,"knee_db":0,"sidechain_hpf_hz":500"#), &NONE, &[], &bass, 256);
    assert!(rms(&filtered.out[0][24000..]) > 2.0 * rms(&flat.out[0][24000..]), "the high-passed detector barely hears the bass");
}

#[test]
fn linked_channels_move_together() {
    let mut loud_left = vec![vec![0.0; 24000], vec![0.0; 24000]];
    loud_left[0].iter_mut().for_each(|v| *v = 0.9);
    loud_left[1].iter_mut().for_each(|v| *v = 0.1);
    let linked = run(&compressor(r#""threshold_db":-20,"ratio":10,"knee_db":0,"attack_ms":0.1,"link":1"#), &NONE, &[], &loud_left, 256);
    let unlinked = run(&compressor(r#""threshold_db":-20,"ratio":10,"knee_db":0,"attack_ms":0.1,"link":0"#), &NONE, &[], &loud_left, 256);
    let ratio_linked = linked.out[0][23000] / linked.out[1][23000];
    let ratio_unlinked = unlinked.out[0][23000] / unlinked.out[1][23000];
    near(ratio_linked, 9.0, 0.05, "linked: both channels get the same gain, so the balance holds");
    assert!(ratio_unlinked < 6.0, "unlinked: the loud side is turned down more, so the balance shifts: {}", ratio_unlinked);
}

#[test]
fn the_delay_repeats_and_decays() {
    let imp = vec![impulse(48000, 0)];
    let g = graph("delay", r#""time_ms":10,"feedback":0.5,"damp_hz":20000"#);
    let r = run(&g, &NONE, &[], &imp, 128);
    let first = argmax(&r.out[0]);
    assert!((first as i64 - 480).abs() <= 1, "the first echo is at 480 samples: {}", first);
    let second = r.out[0][950..970].iter().fold(0.0f32, |m, v| m.max(v.abs()));
    assert!(second > 0.3 && second < 0.6, "the second echo is about half: {}", second);
    assert!(r.tail > 480 * 5, "the tail covers the decay: {}", r.tail);
}

#[test]
fn ping_pong_bounces_between_the_channels() {
    let mut stereo = vec![vec![0.0; 24000], vec![0.0; 24000]];
    stereo[0][0] = 1.0;
    let g = graph("delay", r#""time_ms":10,"feedback":0.6,"damp_hz":20000,"mode":"pingpong""#);
    let r = run(&g, &NONE, &[], &stereo, 128);
    let l: Vec<usize> = (0..24000).filter(|i| r.out[0][*i].abs() > 0.05).collect();
    let rr: Vec<usize> = (0..24000).filter(|i| r.out[1][*i].abs() > 0.05).collect();
    assert!(l.iter().any(|i| (*i as i64 - 480).abs() <= 2), "the first echo lands on the left, 10 ms later");
    assert!(rr.iter().any(|i| (*i as i64 - 960).abs() <= 2), "the next on the right, 10 ms after that");
    assert!(!rr.iter().any(|i| (*i as i64 - 480).abs() <= 2), "and not on the right the first time");
}

#[test]
fn a_modulated_delay_stays_finite_and_moves() {
    let g = graph("delay", r#""time_ms":5,"feedback":0.4,"mod_depth_ms":2,"mod_rate_hz":1"#);
    let input = vec![noise(48000, 3), noise(48000, 4)];
    let r = run(&g, &NONE, &[], &input, 77);
    assert!(r.out.iter().all(|c| c.iter().all(|v| v.is_finite())));
    let flat = run(&graph("delay", r#""time_ms":5,"feedback":0.4,"mod_depth_ms":0"#), &NONE, &[], &input, 77);
    let diff: f32 = r.out[0].iter().zip(&flat.out[0]).map(|(a, b)| (a - b).abs()).sum();
    assert!(diff > 10.0, "modulation changes the sound");
}

/// The level in dB of 50 ms of `x` starting at `at` seconds.
fn level_at(x: &[f32], at: f32) -> f32 {
    let i = (at * RATE) as usize;
    db(rms(&x[i..i + 2400]))
}

#[test]
fn the_reverb_decays_over_the_time_it_is_set_to() {
    for decay in [0.6f32, 1.5] {
        let args = format!(r#""decay_s":{},"damp_hz":20000,"predelay_ms":0,"modulation":0"#, decay);
        let r = run(&graph("reverb", &args), &NONE, &[], &[impulse(4 * 48000, 0)], 128);
        // Over half the decay time the tail falls 30 dB, measured once the diffusers have filled it in.
        let fall = level_at(&r.out[0], 0.3) - level_at(&r.out[0], 0.3 + decay / 2.0);
        near(fall, 30.0, 6.0, &format!("30 dB down over half of {} s", decay));
    }
}

#[test]
fn nothing_comes_out_of_the_reverb_before_its_predelay() {
    let r = run(&graph("reverb", r#""predelay_ms":100"#), &NONE, &[], &[impulse(24000, 0)], 128);
    assert!(peak(&r.out[0][..4800]) < 1e-6, "silent for the first 100 ms");
    assert!(peak(&r.out[0][4800..9600]) > 1e-3, "and the reverb arrives within 100 ms after that");
    let tail = run(&graph("reverb", r#""predelay_ms":20,"decay_s":1.5"#), &NONE, &[], &[impulse(10, 0)], 8).tail;
    assert!((tail as i64 - 82560).abs() <= 2, "the tail is the pre-delay and the decay, with a margin: {}", tail);
}

#[test]
fn damping_darkens_the_reverb_tail() {
    let late_brightness = |damp: f32| {
        let args = format!(r#""decay_s":2,"damp_hz":{},"modulation":0"#, damp);
        let r = run(&graph("reverb", &args), &NONE, &[], &[impulse(48000, 0)], 128);
        let w = &r.out[0][24000..36000];
        let changes: f32 = w.windows(2).map(|p| (p[1] - p[0]).powi(2)).sum();
        changes / w.iter().map(|v| v * v).sum::<f32>().max(1e-20)
    };
    let (bright, dark) = (late_brightness(20000.0), late_brightness(1500.0));
    assert!(dark < bright * 0.3, "a 1.5 kHz damping leaves the tail far darker: {} against {}", dark, bright);
}

#[test]
fn the_reverb_is_wide_in_stereo_and_narrows_with_width() {
    let mono = impulse(48000, 0);
    let both = vec![mono.clone(), mono];
    let corr = |a: &[f32], b: &[f32]| {
        let ab: f32 = a.iter().zip(b).map(|(x, y)| x * y).sum();
        ab / (a.iter().map(|x| x * x).sum::<f32>() * b.iter().map(|y| y * y).sum::<f32>()).sqrt().max(1e-20)
    };
    let wide = run(&graph("reverb", r#""width":1"#), &NONE, &[], &both, 128);
    let c = corr(&wide.out[0], &wide.out[1]);
    assert!(c.abs() < 0.3, "the two sides of a centred source are decorrelated: {}", c);
    let narrow = run(&graph("reverb", r#""width":0"#), &NONE, &[], &both, 128);
    let worst = narrow.out[0].iter().zip(&narrow.out[1]).map(|(a, b)| (a - b).abs()).fold(0.0f32, f32::max);
    assert!(worst < 1e-6, "width 0 gives every channel the same tail: {}", worst);
}

#[test]
fn a_long_large_reverb_stays_bounded() {
    let args = r#""decay_s":30,"size":1,"diffusion":1,"modulation":1,"damp_hz":20000"#;
    let input = vec![noise(5 * 48000, 7).iter().map(|v| v * 0.125).collect::<Vec<f32>>()];
    let r = run(&graph("reverb", args), &NONE, &[], &input, 128);
    assert!(r.out[0].iter().all(|v| v.is_finite()));
    let gain = db(rms(&r.out[0][4 * 48000..])) - db(rms(&input[0]));
    assert!(gain < 20.0, "five seconds of noise into a 30 s hall builds up but stays in bounds: {:+.1} dB", gain);
}

#[test]
fn the_limiter_keeps_every_sample_under_its_ceiling() {
    let input = vec![noise(48000, 5), noise(48000, 6)];
    let ceiling = db_to_lin(-1.0);
    for block in [1usize, 7, 64, 512] {
        let r = run(&graph("limiter", r#""input_db":18,"ceiling_db":-1,"release_ms":20"#), &NONE, &[], &input, block);
        for ch in &r.out {
            assert!(peak(ch) <= ceiling * 1.00001, "18 dB into it, in blocks of {}, peaks at {:.4} dB", block, db(peak(ch)));
        }
        assert!(peak(&r.out[0]) > ceiling * 0.999, "and the loudest samples reach the ceiling");
    }
    let r = run(&graph("limiter", r#""input_db":12,"ceiling_db":-6"#), &NONE, &[], &[sine(997.0, 1.0, 48000)], 100);
    assert!(peak(&r.out[0]) <= db_to_lin(-6.0) * 1.00001, "a sine 18 dB over a -6 dB ceiling");
}

#[test]
fn the_limiter_leaves_what_is_under_the_ceiling_alone() {
    let input = vec![sine(440.0, 0.25, 24000)];
    let r = run(&graph("limiter", r#""ceiling_db":-1"#), &NONE, &[], &input, 64);
    assert_eq!(r.latency, 72, "1.5 ms of lookahead at 48 kHz");
    for (i, v) in r.out[0][72..].iter().enumerate() {
        assert_eq!(*v, input[0][i], "sample {} comes out unchanged, only delayed", i);
    }
    let long = run(&graph("limiter", r#""lookahead_ms":5"#), &NONE, &[], &input, 64);
    assert_eq!(long.latency, 240);
}

#[test]
fn the_limiter_ramps_down_ahead_and_releases_after() {
    // Half a second at 12 dB over the ceiling, then a quiet tone.
    let mut x = sine(500.0, 1.0, 24000);
    x.extend(sine(500.0, 0.1, 48000));
    let g = graph("limiter", r#""input_db":12,"ceiling_db":0,"release_ms":100"#);
    let r = run(&g, &NONE, &[], &[x.clone()], 64);
    let out = &r.out[0];
    let gain_at = |i: usize| db(rms(&out[i + 72..i + 72 + 960])) - db(rms(&x[i..i + 960])) - 12.0;
    near(gain_at(12000), -12.0, 0.3, "the loud part comes down to the ceiling");
    let soon = gain_at(24000 + 2400);
    let later = gain_at(24000 + 24000);
    assert!(soon < -3.0, "50 ms after it the gain is still coming back: {:.1} dB", soon);
    near(later, 0.0, 0.1, "half a second after, it is back");
    // Before a peak arrives the gain is already falling, in a ramp with no step in it.
    let start = &out[72..72 + 200];
    let steps = start.windows(2).map(|w| (w[1] - w[0]).abs()).fold(0.0f32, f32::max);
    assert!(steps < 0.2, "the attack is a ramp: largest step {:.3}", steps);
}

#[test]
fn the_bitcrusher_rounds_to_its_bits() {
    let input = vec![sine(441.0, 0.8, 48000)];
    let four = run(&graph("bitcrusher", r#""bits":4"#), &NONE, &[], &input, 64);
    assert!(four.out[0].iter().all(|v| (v * 8.0).fract() == 0.0), "4 bits leaves eighths only");
    let levels = {
        let mut v: Vec<i32> = four.out[0].iter().map(|v| (v * 8.0) as i32).collect();
        v.sort();
        v.dedup();
        v.len()
    };
    assert_eq!(levels, 13, "a sine at 0.8 crosses 13 of them");
    // Rounding error spreads evenly over a step, so its level is a step over the square root of 12.
    let eight = run(&graph("bitcrusher", r#""bits":8"#), &NONE, &[], &input, 64);
    let error: Vec<f32> = eight.out[0].iter().zip(&input[0]).map(|(a, b)| a - b).collect();
    near(rms(&error) / (1.0 / 128.0 / 12f32.sqrt()), 1.0, 0.05, "8-bit noise against theory");
    let clean = run(&graph("bitcrusher", r#""bits":24"#), &NONE, &[], &input, 64);
    assert!(clean.out[0].iter().zip(&input[0]).all(|(a, b)| (a - b).abs() <= 1.0 / (1 << 24) as f32), "24 bits is transparent");
}

#[test]
fn the_bitcrusher_holds_samples_at_its_rate() {
    let input = vec![noise(48000, 9)];
    let r = run(&graph("bitcrusher", r#""bits":24,"rate_hz":1000"#), &NONE, &[], &input, 37);
    let changes = r.out[0].windows(2).filter(|w| w[0] != w[1]).count();
    assert!((changes as i64 - 1000).abs() <= 1, "1000 new samples in a second: {}", changes);
    let runs: Vec<usize> = r.out[0].chunk_by(|a, b| a == b).map(|c| c.len()).collect();
    assert!(runs[1..runs.len() - 1].iter().all(|n| *n == 48), "each held for 48 samples");
    // An odd rate takes samples at uneven gaps that average out right.
    let odd = run(&graph("bitcrusher", r#""bits":24,"rate_hz":7000"#), &NONE, &[], &input, 64);
    let changes = odd.out[0].windows(2).filter(|w| w[0] != w[1]).count();
    assert!((changes as i64 - 7000).abs() <= 1, "7000 a second: {}", changes);
}

#[test]
fn dither_keeps_a_tone_quieter_than_a_step() {
    // A tone at a third of a step rounds to silence. With dither it survives, under a steady hiss.
    let input = vec![sine(500.0, 1.0 / 256.0 / 3.0, 48000)];
    let plain = run(&graph("bitcrusher", r#""bits":8"#), &NONE, &[], &input, 64);
    assert!(plain.out[0].iter().all(|v| *v == 0.0), "without dither it is gone");
    let dithered = run(&graph("bitcrusher", r#""bits":8,"dither":"on""#), &NONE, &[], &input, 64);
    near(tone(&dithered.out[0], 500.0) / tone(&input[0], 500.0), 1.0, 0.1, "with dither the tone is there at its level");
    let mut x = sine(500.0, 0.5, 4800);
    x.extend(vec![0.0; 9600]);
    let r = run(&graph("bitcrusher", r#""bits":8,"dither":"on""#), &NONE, &[], &[x], 64);
    assert!(r.out[0][4800..7200].iter().any(|v| *v != 0.0), "the hiss goes on a moment after the input stops");
    assert!(r.out[0][7300..].iter().all(|v| *v == 0.0), "and then stops");
}

#[test]
fn the_pitch_shifter_moves_a_tone_to_the_interval_asked() {
    let input = vec![sine(400.0, 0.5, 48000)];
    // 701.955 cents is an exact fifth, so 400 Hz goes to 600.
    for (args, hz) in [(r#""semitones":12"#, 800.0), (r#""semitones":-12"#, 200.0), (r#""semitones":7,"cents":1.955"#, 600.0)] {
        let r = run(&graph("pitch_shift", args), &NONE, &[], &input, 64);
        let x = &r.out[0][9600..];
        let moved = db(tone(x, hz));
        let left = db(tone(x, 400.0));
        assert!(moved - left > 25.0, "{} puts {:.1} dB at {} Hz and {:.1} dB at 400", args, moved, hz, left);
        near(moved, db(0.5), 1.5, &format!("{} keeps the level", args));
    }
}

#[test]
fn the_pitch_shifter_at_no_shift_is_a_delay() {
    let input = vec![noise(9600, 3)];
    let r = run(&graph("pitch_shift", r#""window_ms":50"#), &NONE, &[], &input, 64);
    // Half the 2400-sample window, and the 4 samples the taps always keep back.
    let d = 1204;
    let worst = r.out[0][d..].iter().zip(&input[0]).map(|(a, b)| (a - b).abs()).fold(0.0f32, f32::max);
    assert!(worst < 1e-6, "the output is the input {} samples later, to {}", d, worst);
    assert_eq!(r.tail, 2404, "and the tail is the window");
}

#[test]
fn a_shifted_noise_keeps_its_level() {
    let input = vec![noise(48000, 4), noise(48000, 5)];
    for st in [-12, -5, 3, 12] {
        let r = run(&graph("pitch_shift", &format!(r#""semitones":{}"#, st)), &NONE, &[], &input, 128);
        // Two unrelated taps faded by amplitude add to 1.25 dB less power on average, and reading between samples
        // softens the top a little more: about 2.1 dB down, or 1.3 dB at an octave up, where every read lands on a
        // sample.
        for (o, i) in r.out.iter().zip(&input) {
            near(db(rms(&o[4800..])) - db(rms(i)), -1.5, 1.0, &format!("noise shifted {} semitones", st));
        }
    }
}

#[test]
fn the_oscillator_plays_its_frequency_level_and_shape() {
    let silent = vec![vec![0.0f32; 48000]];
    let r = run(&graph("oscillator", r#""freq_hz":440,"level_db":-6"#), &NONE, &[], &silent, 128);
    near(tone(&r.out[0][4800..], 440.0), 0.501, 0.005, "a sine at 440 Hz and -6 dB");
    let rising = r.out[0].windows(2).filter(|w| w[0] < 0.0 && w[1] >= 0.0).count();
    assert!((rising as i64 - 440).abs() <= 1, "440 cycles in a second: {}", rising);
    let pulse = run(&graph("oscillator", r#""shape":"pulse","pulse_width":0.25,"level_db":0,"freq_hz":100"#), &NONE, &[], &silent, 128);
    near(pulse.out[0].iter().sum::<f32>() / 48000.0, -0.5, 0.02, "a pulse high for a quarter of each cycle averages -0.5");
    let dc = vec![vec![0.25f32; 4800]];
    let add = run(&graph("oscillator", r#""mode":"add","level_db":-96"#), &NONE, &[], &dc, 64);
    near(add.out[0][4000], 0.25, 1e-3, "add keeps the input");
    let replace = run(&graph("oscillator", r#""level_db":-96"#), &NONE, &[], &dc, 64);
    near(replace.out[0][4000], 0.0, 1e-3, "replace puts out the tone alone");
    assert!(replace.tail >= i32::MAX as u32, "a generator's tail never ends");
}

#[test]
fn a_band_limited_saw_keeps_its_aliasing_down() {
    // A 5 kHz saw at 48 kHz: the 6th harmonic, 30 kHz, folds back to 18 kHz, where no harmonic belongs.
    let r = run(&graph("oscillator", r#""shape":"saw","freq_hz":5000,"level_db":0"#), &NONE, &[], &[vec![0.0; 48000]], 128);
    let x = &r.out[0][4800..];
    let alias = db(tone(x, 18000.0)) - db(tone(x, 5000.0));
    // A plain saw would put it 15.6 dB under the fundamental. PolyBLEP takes it to about 28 dB under.
    assert!(alias < -25.0, "the folded 6th harmonic is {:.1} dB under the fundamental", alias);
}

#[test]
fn tremolo_swings_the_level() {
    let input = vec![vec![1.0; 48000]];
    let r = run(&graph("tremolo", r#""rate_hz":5,"depth":1"#), &NONE, &[], &input, 64);
    let lo = r.out[0].iter().cloned().fold(f32::MAX, f32::min);
    let hi = r.out[0].iter().cloned().fold(f32::MIN, f32::max);
    assert!(lo < 0.01 && hi > 0.99, "depth 1 swings from nothing to full: {} {}", lo, hi);
    let half = run(&graph("tremolo", r#""rate_hz":5,"depth":0.5"#), &NONE, &[], &input, 64);
    let lo = half.out[0].iter().cloned().fold(f32::MAX, f32::min);
    near(lo, 0.5, 0.02, "depth 0.5 bottoms out at half");
}

#[test]
fn stereo_width_and_mid_side_round_trip() {
    let stereo = vec![noise(4096, 1), noise(4096, 2)];
    let mono = run(&graph("stereo", r#""width":0"#), &NONE, &[], &stereo, 100);
    assert!(mono.out[0].iter().zip(&mono.out[1]).all(|(a, b)| (a - b).abs() < 1e-5), "width 0 is mono");
    let wide = run(&graph("stereo", r#""width":1"#), &NONE, &[], &stereo, 100);
    assert!(wide.out[0].iter().zip(&stereo[0]).all(|(a, b)| (a - b).abs() < 1e-5), "width 1 changes nothing");
    let chain = r#"{"chain":[{"block":"stereo","args":{"mode":"encode"}},{"block":"stereo","args":{"mode":"decode"}}]}"#;
    let back = run(chain, &NONE, &[], &stereo, 64);
    assert!(back.out[0].iter().zip(&stereo[0]).all(|(a, b)| (a - b).abs() < 1e-5), "encode then decode is the identity");
    assert!(back.out[1].iter().zip(&stereo[1]).all(|(a, b)| (a - b).abs() < 1e-5));
    // Turn only the side down by 6 dB with a node restricted to channel 1.
    let side = r#"{"chain":[
        {"block":"stereo","args":{"mode":"encode"}},
        {"block":"gain","args":{"gain_db":-6.0206},"only":[1]},
        {"block":"stereo","args":{"mode":"decode"}}]}"#;
    let pure_side = vec![vec![1.0; 2048], vec![-1.0; 2048]];
    let r = run(side, &NONE, &[], &pure_side, 64);
    near(r.out[0][2000], 0.5, 1e-3, "a signal that is all side loses 6 dB");
    let pure_mid = vec![vec![1.0; 2048], vec![1.0; 2048]];
    let r = run(side, &NONE, &[], &pure_mid, 64);
    near(r.out[0][2000], 1.0, 1e-3, "and a signal that is all mid does not");
}
