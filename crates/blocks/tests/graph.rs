//! The graph runtime: bindings, wiring, latency, block-size independence, allocation and error messages.

mod common;
use common::*;
use ruse_blocks::Graph;
use ruse_core::{ParamInfo, ParamSet, ProcessContext, Setup};

#[global_allocator]
static ALLOC: ruse_host::alloc::CountingAlloc = ruse_host::alloc::CountingAlloc::new();

const NONE: [ParamInfo; 0] = [];

fn near(got: f32, want: f32, tol: f32, what: &str) {
    assert!((got - want).abs() <= tol, "{}: got {:.4}, wanted {:.4} within {}", what, got, want, tol);
}

fn params() -> Vec<ParamInfo> {
    vec![
        ParamInfo::knob("g", "G", "dB", -24.0, 24.0, 0.0),
        ParamInfo::knob("tone", "Tone", "Hz", 100.0, 16000.0, 4000.0),
        ParamInfo::choice("mode", "Mode", &["Soft", "Hard"], 0),
        ParamInfo::toggle("bypass", "Bypass", false),
        ParamInfo::knob("wet", "Wet", "", 0.0, 1.0, 1.0),
        ParamInfo::meter("gr", "GR", "dB", 0.0, 60.0),
    ]
}

#[test]
fn arguments_follow_the_parameters_through_expressions() {
    let g = r#"{"block":"gain","args":{"gain_db":"g * 2"}}"#;
    let steady = vec![vec![1.0; 2048]];
    let r = run(g, &params(), &[("g", -3.0)], &steady, 64);
    near(r.out[0][2000], 10f32.powf(-6.0 / 20.0), 1e-4, "g * 2 with g = -3 is -6 dB");
    // A choice parameter picks a curve by index, as Warm Saturator's Mode does.
    let shape = r#"{"block":"shaper","args":{"curve":"mode","drive_db":20}}"#;
    let hot = vec![vec![0.5; 2048]];
    let soft = run(shape, &params(), &[("mode", 0.0)], &hot, 64);
    let hard = run(shape, &params(), &[("mode", 1.0)], &hot, 64);
    near(hard.out[0][2000], 1.0, 1e-5, "Hard clips at 1");
    assert!(soft.out[0][2000] < 1.0 && soft.out[0][2000] > 0.99, "Soft rounds toward 1: {}", soft.out[0][2000]);
}

#[test]
fn a_parameter_change_glides_and_does_not_click() {
    let mut s = Session::new(r#"{"block":"gain","args":{"gain_db":"g"}}"#, &params(), 1);
    s.set("g", 0.0);
    let mut block = vec![vec![1.0; 256]];
    s.feed(&mut block);
    s.set("g", -24.0);
    let mut block = vec![vec![1.0; 256]];
    s.feed(&mut block);
    let steps: Vec<f32> = block[0].windows(2).map(|w| (w[1] - w[0]).abs()).collect();
    let biggest = steps.iter().cloned().fold(0.0f32, f32::max);
    assert!(biggest < 0.05, "the level moves in small steps, not one jump: {}", biggest);
    assert!(block[0][255] < 0.1, "and arrives");
}

#[test]
fn parallel_branches_add_and_take_a_gain() {
    let g = r#"{"parallel":[{"node":{"chain":[]}},{"node":{"block":"gain","args":{"gain_db":0}},"gain_db":-6.0206}]}"#;
    let r = run(g, &NONE, &[], &[vec![1.0; 1024]], 64);
    near(r.out[0][1000], 1.5, 1e-3, "1 + 0.5");
}

#[test]
fn branches_with_different_latency_stay_in_time() {
    let g = r#"{"parallel":[
        {"node":{"chain":[]}},
        {"node":{"block":"dynamics","args":{"threshold_db":0,"ratio":1,"lookahead_ms":5}}}]}"#;
    let r = run(g, &NONE, &[], &[impulse(4800, 1000)], 128);
    assert_eq!(r.latency, 240, "the graph reports the longest branch");
    let at = argmax(&r.out[0]);
    assert_eq!(at, 1240);
    near(r.out[0][1240], 2.0, 1e-3, "both branches arrive together, so they add");
    assert!(r.out[0][1000].abs() < 1e-6, "nothing early from the dry branch");
}

#[test]
fn bypass_and_mix_blend_and_never_jump() {
    let g = r#"{"block":"gain","args":{"gain_db":6.0206},"bypass":"bypass","mix":"wet"}"#;
    let steady = vec![vec![1.0; 1024]];
    near(run(g, &params(), &[], &steady, 64).out[0][1000], 2.0, 1e-3, "on");
    near(run(g, &params(), &[("bypass", 1.0)], &steady, 64).out[0][1000], 1.0, 1e-6, "bypassed it passes the input");
    near(run(g, &params(), &[("wet", 0.5)], &steady, 64).out[0][1000], 1.5, 1e-3, "half wet");

    let mut s = Session::new(g, &params(), 1);
    let mut block = vec![vec![1.0; 256]];
    s.feed(&mut block);
    s.set("bypass", 1.0);
    let mut block = vec![vec![1.0; 256]];
    s.feed(&mut block);
    let biggest = block[0].windows(2).map(|w| (w[1] - w[0]).abs()).fold(0.0f32, f32::max);
    assert!(biggest < 0.1, "switching bypass crossfades: {}", biggest);
    near(block[0][255], 1.0, 1e-4, "and lands on the dry signal");
}

#[test]
fn a_dry_path_waits_for_the_latency_of_what_it_mixes_with() {
    let g = r#"{"block":"dynamics","args":{"threshold_db":0,"ratio":1,"lookahead_ms":5},"mix":0.5}"#;
    let r = run(g, &NONE, &[], &[impulse(4800, 1000)], 128);
    assert_eq!(argmax(&r.out[0]), 1240);
    near(r.out[0][1240], 1.0, 1e-3, "dry and wet arrive together, so a half and a half make one");
    assert!(r.out[0][1000].abs() < 1e-6);
}

#[test]
fn only_applies_a_node_to_the_channels_named() {
    let g = r#"{"block":"gain","args":{"gain_db":-6.0206},"only":[0]}"#;
    let r = run(g, &NONE, &[], &[vec![1.0; 512], vec![1.0; 512]], 64);
    near(r.out[0][500], 0.5, 1e-3, "channel 0 is processed");
    near(r.out[1][500], 1.0, 1e-6, "channel 1 is not");
}

#[test]
fn modulators_drive_arguments() {
    let g = r#"{"modulators":[{"id":"lfo","shape":"sine","rate":2}],
                "block":"gain","args":{"gain_db":"lfo * 6"}}"#;
    let r = run(g, &NONE, &[], &[vec![1.0; 48000]], 100);
    let lo = r.out[0].iter().cloned().fold(f32::MAX, f32::min);
    let hi = r.out[0].iter().cloned().fold(f32::MIN, f32::max);
    near(lo, 10f32.powf(-6.0 / 20.0), 0.01, "bottom of the swing is -6 dB");
    near(hi, 10f32.powf(6.0 / 20.0), 0.01, "top is +6 dB");

    // Two seconds at 2 Hz is four cycles.
    let crossings = r.out[0].windows(2).filter(|w| (w[0] - 1.0) * (w[1] - 1.0) < 0.0).count();
    assert!((1..=5).contains(&(crossings / 2 + 1)), "about one second of a 2 Hz wave, two cycles: {}", crossings);

    let held = r#"{"modulators":[{"id":"r","shape":"random","rate":10}],
                  "block":"gain","args":{"gain_db":"r * 6"}}"#;
    let r = run(held, &NONE, &[], &[vec![1.0; 48000]], 100);
    // Each new value is reached by a 32-sample glide, ten times a second. Everywhere else the level is flat.
    let flat = r.out[0].windows(2).filter(|w| (w[0] - w[1]).abs() < 1e-7).count();
    assert!(flat > 47000 - 10 * 33, "sample and hold holds its level between steps: {} flat of 47999", flat);
    assert!(flat < 47990, "and it does step");
    let again = run(held, &NONE, &[], &[vec![1.0; 48000]], 7);
    assert_eq!(r.out[0], again.out[0], "and it is the same sequence every time");
}

#[test]
fn tempo_sync_follows_the_host() {
    let g = r#"{"block":"delay","args":{"time_ms":"sync(0.5)","feedback":0}}"#;
    let r = run(g, &NONE, &[], &[impulse(24000, 0)], 128);
    let at = argmax(&r.out[0]);
    assert!((at as i64 - 12000).abs() <= 2, "an eighth note at 120 bpm is 250 ms, 12000 samples: {}", at);

    let mut s = Session::new(g, &NONE, 1);
    let ctx = ProcessContext {
        tempo_bpm: Some(60.0),
        ..ProcessContext::offline(RATE)
    };
    let mut x = impulse(48000, 0);
    {
        let mut sl: Vec<&mut [f32]> = vec![&mut x[..]];
        s.graph.process(&s.params, &mut sl, &ctx);
    }
    let at = argmax(&x);
    assert!((at as i64 - 24000).abs() <= 2, "at 60 bpm it is 500 ms: {}", at);
}

fn busy_graph() -> &'static str {
    r#"{"modulators":[{"id":"lfo","shape":"triangle","rate":0.7},
                      {"id":"env","shape":"envelope","attack_ms":3,"release_ms":80,"detector":"rms"},
                      {"id":"duck","shape":"envelope","source":"sidechain","attack_ms":2,"release_ms":50},
                      {"id":"band","shape":"envelope","source":"hi","attack_ms":1,"release_ms":40}],
        "chain":[
          {"block":"harmonic_eq","args":{"fundamental_hz":"tone / 20","harmonics":6,"gain_db":4,"odd_db":3}},
          {"split":{"freqs":["tone / 8 * 2 ^ (0.5 * lfo)",3000],"bands":[{"block":"gain","args":{"gain_db":2}},{"chain":[]},{"block":"shaper","args":{"drive_db":3}}]}},
          {"tap":{"name":"hi","node":{"block":"filter","args":{"type":"highpass","freq":4000}}}},
          {"block":"filter","args":{"type":"lowpass","freq":"tone * 2 ^ (lfo - min(band, 1))","q":2}},
          {"block":"shaper","args":{"curve":"mode","drive_db":"g + 12 + 6 * env","bias":0.1}},
          {"oversample":{"factor":4,"node":{"chain":[
            {"block":"shaper","args":{"curve":"hard","drive_db":"6 + 6 * lfo"}},
            {"block":"dynamics","args":{"key":"sidechain","threshold_db":-30,"ratio":"2 + 2 * min(duck, 1)"}},
            {"block":"filter","args":{"type":"lowpass","freq":"tone * 2"}}]}}},
          {"parallel":[
            {"node":{"chain":[]}},
            {"node":{"chain":[
               {"block":"delay","args":{"time_ms":7,"feedback":0.3,"mod_depth_ms":1,"mod_rate_hz":0.9}},
               {"block":"tremolo","args":{"rate_hz":3,"depth":0.3,"stereo_phase":90}}]},"gain_db":-6}]},
          {"block":"dynamics","args":{"threshold_db":-24,"ratio":3,"lookahead_ms":2},"meters":{"gain_reduction_db":"gr"}},
          {"block":"stereo","args":{"width":1.3}},
          {"feedback":{"node":{"block":"filter","args":{"type":"lowpass","freq":3000}},"gain":0.4,"delay_ms":"40 + 5 * lfo"},"mix":0.3},
          {"feedback":{"node":{"block":"onepole","args":{"type":"lowpass","freq":8000}},"gain":0.7,"delay_ms":"0.5 + 0.3 * lfo"},"mix":0.2},
          {"block":"reverb","args":{"decay_s":0.4,"size":"0.3 + 0.1 * lfo","modulation":0.6},"mix":0.2},
          {"block":"bitcrusher","args":{"bits":"12 + 2 * lfo","rate_hz":"20000 + 4000 * lfo","dither":"on"},"mix":0.3},
          {"block":"pitch_shift","args":{"semitones":"7 * lfo","cents":"g","window_ms":"40 + 10 * lfo"},"mix":0.3},
          {"block":"limiter","args":{"input_db":"g + 6","ceiling_db":"-3 + 2 * lfo","release_ms":"60 + 40 * lfo"}}
        ]}"#
}

#[test]
fn the_audio_does_not_depend_on_the_hosts_block_size() {
    let input = vec![noise(30000, 11), noise(30000, 12)];
    let reference = run(busy_graph(), &params(), &[], &input, 32);
    for block in [1usize, 7, 31, 33, 100, 480, 512, 4096] {
        let r = run(busy_graph(), &params(), &[], &input, block);
        let worst = reference
            .out
            .iter()
            .zip(&r.out)
            .flat_map(|(a, b)| a.iter().zip(b).map(|(x, y)| (x - y).abs()))
            .fold(0.0f32, f32::max);
        assert!(worst < 1e-6, "block size {} differs from 32 by {}", block, worst);
    }
    assert!(reference.out.iter().all(|c| c.iter().all(|v| v.is_finite())));
}

#[test]
fn two_instances_give_identical_audio() {
    let input = vec![noise(20000, 21), noise(20000, 22)];
    let a = run(busy_graph(), &params(), &[], &input, 128);
    let b = run(busy_graph(), &params(), &[], &input, 128);
    assert_eq!(a.out, b.out);
}

#[test]
fn processing_never_allocates() {
    assert!(ruse_host::alloc::installed());
    let infos = params();
    let set = ParamSet::with_channels(&infos, 8);
    let ids: Vec<&str> = infos.iter().map(|i| i.id).collect();
    let mut g = Graph::from_str(busy_graph(), &ids).unwrap();
    g.prepare(&Setup {
        sample_rate: RATE,
        max_block: 512,
        channels: 2,
    });
    let ctx = ProcessContext::offline(RATE);
    let mut a = noise(2000, 1);
    let mut b = noise(2000, 2);
    let key = noise(2000, 3);
    let before = ruse_host::alloc::count();
    for start in (0..1900).step_by(77) {
        // Arrays, so the test itself does not allocate. Every other block has a sidechain, and the rest none.
        let mut sl = [&mut a[start..start + 77], &mut b[start..start + 77]];
        let keys = [&key[start..start + 77]];
        let keys: &[&[f32]] = if start % 2 == 0 { &keys } else { &[] };
        g.process_with_sidechain(&set, &mut sl, keys, &ctx);
    }
    assert_eq!(ruse_host::alloc::count() - before, 0, "process allocated");
}

#[test]
fn a_decaying_tail_never_goes_subnormal() {
    let mut input = noise(24000, 31);
    input.extend(vec![0.0; 96000]);
    let input = vec![input.clone(), input];
    let r = run(busy_graph(), &params(), &[], &input, 256);
    let bad = r.out.iter().flat_map(|c| c.iter()).filter(|v| **v != 0.0 && v.abs() < f32::MIN_POSITIVE).count();
    assert_eq!(bad, 0, "subnormal samples in the output");
    assert!(r.out[0][100000..].iter().all(|v| v.abs() < 1e-6), "and the tail does end");
}

#[test]
fn an_envelope_follows_the_input_level_with_its_attack_and_release() {
    // The gain is the envelope itself, so the output divided by a steady input reads the envelope.
    let g = r#"{"modulators":[{"id":"env","shape":"envelope","attack_ms":10,"release_ms":50}],
                "block":"gain","args":{"gain_db":"lin2db(max(env, 0.00001))"}}"#;
    let mut x = vec![1.0f32; 24000];
    x.extend(vec![0.01f32; 24000]);
    let r = run(g, &NONE, &[], &[x.clone()], 128);
    let env_at = |i: usize| r.out[0][i] / x[i];
    // One time constant in, an envelope has gone 63% of the way. 10 ms is 480 samples and 50 ms is 2400.
    near(env_at(480), 0.632, 0.05, "the attack after 10 ms");
    near(env_at(23000), 1.0, 0.01, "settled on a full-scale input");
    near(env_at(24000 + 2400), 0.01 + 0.99 * 0.368, 0.05, "the release after 50 ms");
    near(env_at(47000), 0.01, 0.002, "settled on the quieter input");
}

#[test]
fn an_rms_envelope_reads_a_sine_lower_than_its_peak() {
    let read = |detector: &str, attack_ms: f32| {
        let g = format!(
            r#"{{"modulators":[{{"id":"env","shape":"envelope","attack_ms":{},"release_ms":200,"detector":"{}"}}],
                "block":"gain","args":{{"gain_db":"lin2db(max(env, 0.00001)) + 40"}}}}"#,
            attack_ms, detector
        );
        // A probe at -40 dB on channel 1 shows the envelope itself, which the louder sine on channel 0 drives.
        let r = run(&g, &NONE, &[], &[sine(100.0, 0.5, 48000), vec![0.01; 48000]], 128);
        r.out[1][47000]
    };
    near(read("peak", 1.0), 0.5, 0.03, "a fast peak envelope holds near the sine's peak");
    // With attack equal to release, the rms detector is a plain average of the square.
    near(read("rms", 200.0), 0.354, 0.02, "the rms envelope reads the sine's rms, 0.5 / sqrt 2");
}

#[test]
fn an_envelope_is_declared_with_attack_and_release_not_a_rate() {
    let err = |m: &str| {
        let g = format!(r#"{{"modulators":[{}],"block":"gain"}}"#, m);
        Graph::from_str(&g, &[]).err().unwrap_or_default()
    };
    assert!(err(r#"{"id":"env","shape":"envelope","rate":2}"#).contains("instead of running at a rate"));
    assert!(err(r#"{"id":"lfo","shape":"sine","attack_ms":5}"#).contains("belong to an envelope"));
    assert!(err(r#"{"id":"env","shape":"envelope","detector":"loud"}"#).contains("Use peak or rms"));
    assert!(err(r#"{"id":"env","shape":"envelope","attack_ms":5,"release_ms":"g * 10"}"#).contains("'g'"));
    assert!(Graph::from_str(r#"{"modulators":[{"id":"env","shape":"envelope"}],"block":"gain"}"#, &[]).is_ok());
}

#[test]
fn multiply_with_an_oscillator_is_a_ring_modulator() {
    let ring = r#"{"multiply":[{"chain":[]},{"block":"oscillator","args":{"freq_hz":300,"level_db":0}}]}"#;
    let r = run(ring, &NONE, &[], &[sine(1000.0, 0.5, 48000)], 128);
    let x = &r.out[0][4800..];
    near(tone(x, 700.0), 0.25, 0.005, "the difference tone");
    near(tone(x, 1300.0), 0.25, 0.005, "the sum tone");
    assert!(tone(x, 1000.0) < 0.002, "and none of the input: {}", tone(x, 1000.0));
    assert_eq!(r.tail, 0, "a product is silent once its input is, whatever the oscillator does");
    // Offset 1 keeps the input and adds the sidebands: amplitude modulation.
    let am = r#"{"multiply":[{"chain":[]},{"block":"oscillator","args":{"freq_hz":300,"level_db":-6,"offset":1}}]}"#;
    let r = run(am, &NONE, &[], &[sine(1000.0, 0.5, 48000)], 128);
    let x = &r.out[0][4800..];
    near(tone(x, 1000.0), 0.5, 0.005, "the input stays");
    near(tone(x, 1300.0), 0.125, 0.005, "with a sideband at a quarter of it");
}

#[test]
fn an_oscillator_swept_by_an_lfo_does_not_depend_on_the_hosts_block_size() {
    let g = r#"{"modulators":[{"id":"lfo","shape":"sine","rate":3}],
                "block":"oscillator","args":{"shape":"saw","freq_hz":"220 * 2 ^ lfo","level_db":"-9 + 3 * lfo"}}"#;
    let silent = vec![vec![0.0f32; 20000]];
    let reference = run(g, &NONE, &[], &silent, 32);
    for block in [1usize, 7, 33, 100, 512] {
        let r = run(g, &NONE, &[], &silent, block);
        let worst = reference.out[0].iter().zip(&r.out[0]).map(|(a, b)| (a - b).abs()).fold(0.0f32, f32::max);
        assert!(worst < 1e-6, "block size {} differs by {}", block, worst);
    }
}

#[test]
fn the_parametric_eq_subgraph_boosts_and_cuts_where_it_is_told() {
    let flat = r#"{"subgraph":"parametric_eq"}"#;
    near(gain_at(flat, &NONE, &[], 1000.0), 0.0, 0.05, "flat with every band at 0 dB");
    let infos = vec![
        ParamInfo::knob("bass", "Bass", "dB", -12.0, 12.0, 0.0),
        ParamInfo::knob("presence", "Presence", "dB", -12.0, 12.0, 0.0),
    ];
    let eq = r#"{"subgraph":"parametric_eq","args":{"low_gain_db":"bass","mid2_freq":3000,"mid2_gain_db":"presence * 0.5"}}"#;
    near(gain_at(eq, &infos, &[("bass", 9.0)], 30.0), 9.0, 0.5, "the low shelf follows Bass");
    near(gain_at(eq, &infos, &[("presence", 12.0)], 3000.0), 6.0, 0.2, "the bell follows half of Presence at 3 kHz");
    near(gain_at(eq, &infos, &[("presence", 12.0)], 300.0), 0.0, 0.3, "and leaves 300 Hz alone");
}

#[test]
fn a_graph_declares_its_own_subgraphs_with_arguments() {
    let infos = vec![ParamInfo::knob("tilt", "Tilt", "dB", -6.0, 6.0, 0.0)];
    // "type" takes a choice name through an argument, "-amount" an expression, and "max(...)" stays a function.
    let g = r#"{"subgraphs":{
          "shelf":{"args":{"kind":"lowshelf","at":800,"amount":0},
                   "node":{"block":"filter","args":{"type":"kind","freq":"at","gain_db":"max(amount, -12)"}}},
          "tilt":{"args":{"amount":0,"pivot":800},
                  "node":{"chain":[{"subgraph":"shelf","args":{"amount":"-amount","at":"pivot"}},
                                   {"subgraph":"shelf","args":{"kind":"highshelf","amount":"amount","at":"pivot"}}]}}},
        "subgraph":"tilt","args":{"amount":"tilt","pivot":1000}}"#;
    near(gain_at(g, &infos, &[("tilt", 6.0)], 20.0), -6.0, 0.3, "the lows fall by the tilt");
    near(gain_at(g, &infos, &[("tilt", 6.0)], 16000.0), 6.0, 0.3, "and the highs rise by it");
    near(gain_at(g, &infos, &[("tilt", 0.0)], 1000.0), 0.0, 0.05, "flat at no tilt");
}

#[test]
fn subgraphs_say_what_is_wrong() {
    let err = |g: &str| Graph::from_str(g, &[]).err().unwrap_or_default();
    assert!(err(r#"{"subgraph":"nope"}"#).contains("no subgraph called 'nope'"));
    assert!(err(r#"{"subgraph":"parametric_eq","args":{"bass":3}}"#).contains("Its arguments are: low_freq"));
    let loops = r#"{"subgraphs":{"again":{"node":{"subgraph":"again"}}},"subgraph":"again"}"#;
    assert!(err(loops).contains("eight deep") || err(loops).contains("8 deep"), "{}", err(loops));
    assert!(err(r#"{"subgraphs":{"parametric_eq":{"node":{"chain":[]}}},"chain":[]}"#).contains("one RUSE ships"));
    assert!(err(r#"{"subgraph":"parametric_eq","args":{"low_gain_db":90}}"#).contains("90"));
    // mix on the use applies to the whole subgraph.
    assert!(Graph::from_str(r#"{"subgraph":"parametric_eq","mix":0.5}"#, &[]).is_ok());
}

#[test]
fn a_split_adds_back_to_a_flat_level() {
    for slope in [24, 48] {
        let g = format!(
            r#"{{"split":{{"freqs":[150, 1200, 6000],"slope":{},"bands":[{{"chain":[]}},{{"chain":[]}},{{"chain":[]}},{{"chain":[]}}]}}}}"#,
            slope
        );
        for hz in [40.0, 150.0, 500.0, 1200.0, 3000.0, 6000.0, 14000.0] {
            near(gain_at(&g, &NONE, &[], hz), 0.0, 0.05, &format!("flat at {} Hz at {} dB per octave", hz, slope));
        }
    }
}

#[test]
fn each_band_of_a_split_keeps_to_its_range() {
    let low_only = |slope: u32| {
        format!(
            r#"{{"split":{{"freqs":[500],"slope":{},"bands":[{{"chain":[]}},{{"block":"gain","args":{{"gain_db":-96}}}}]}}}}"#,
            slope
        )
    };
    near(gain_at(&low_only(24), &NONE, &[], 500.0), -6.0, 0.1, "a Linkwitz-Riley crossover is 6 dB down");
    near(gain_at(&low_only(24), &NONE, &[], 60.0), 0.0, 0.1, "well below it the low band is whole");
    near(gain_at(&low_only(24), &NONE, &[], 2000.0), -48.0, 2.0, "two octaves above, LR4 is 48 dB down");
    assert!(gain_at(&low_only(48), &NONE, &[], 2000.0) < -80.0, "and LR8 far further");
    let bands = r#"{"split":{"freqs":[300, 3000],"bands":[{"block":"gain","args":{"gain_db":6}},{"chain":[]},{"block":"gain","args":{"gain_db":-6}}]}}"#;
    near(gain_at(bands, &NONE, &[], 40.0), 6.0, 0.2, "the low band's gain");
    near(gain_at(bands, &NONE, &[], 1000.0), 0.0, 0.5, "the middle band's");
    near(gain_at(bands, &NONE, &[], 18000.0), -6.0, 0.3, "the high band's");
}

#[test]
fn split_bands_with_different_latency_line_up() {
    // The dynamics block never acts at this level, so its band is only a 5 ms delay that the other must wait for.
    let g = r#"{"split":{"freqs":[1000],"bands":[{"block":"dynamics","args":{"threshold_db":0,"lookahead_ms":5}},{"chain":[]}]}}"#;
    for hz in [200.0, 1000.0, 5000.0] {
        near(gain_at(g, &NONE, &[], hz), 0.0, 0.05, &format!("still flat at {} Hz", hz));
    }
    assert_eq!(run(g, &NONE, &[], &[vec![0.0; 64]], 64).latency, 240, "and the split reports the latency");
}

#[test]
fn a_split_says_what_is_wrong_with_it() {
    let err = |g: &str| Graph::from_str(g, &[]).err().unwrap_or_default();
    assert!(err(r#"{"split":{"freqs":[500],"bands":[{"chain":[]}]}}"#).contains("needs 2 bands"));
    assert!(err(r#"{"split":{"freqs":[2000, 500],"bands":[{"chain":[]},{"chain":[]},{"chain":[]}]}}"#).contains("must rise"));
    assert!(err(r#"{"split":{"freqs":[500],"slope":36,"bands":[{"chain":[]},{"chain":[]}]}}"#).contains("24 or 48"));
    assert!(err(r#"{"split":{"freqs":[1,2,3,4],"bands":[]}}"#).contains("one to three"));
}

#[test]
fn oversampling_is_flat_and_reports_its_delay() {
    for (factor, delay) in [(2, 32), (4, 38), (8, 40)] {
        let g = format!(r#"{{"oversample":{{"factor":{},"node":{{"chain":[]}}}}}}"#, factor);
        for hz in [30.0, 1000.0, 10000.0, 18000.0] {
            near(gain_at(&g, &NONE, &[], hz), 0.0, 0.01, &format!("{}x at {} Hz", factor, hz));
        }
        let r = run(&g, &NONE, &[], &[impulse(4800, 1000)], 100);
        assert_eq!(r.latency, delay, "{}x reports the delay of its filters", factor);
        assert_eq!(argmax(&r.out[0]), 1000 + delay as usize, "{}x delays an impulse by what it reports", factor);
    }
}

#[test]
fn oversampling_keeps_a_clippers_harmonics_from_folding_back() {
    // A 7 kHz tone hard clipped. Its 5th and 7th harmonics, at 35 and 49 kHz, fold back to 13 and 1 kHz at 48 kHz.
    let clip = r#"{"block":"shaper","args":{"curve":"hard","drive_db":12}}"#;
    let input = vec![sine(7000.0, 0.5, 48000)];
    let level = |g: &str, hz: f32| {
        let out = run(g, &NONE, &[], &input, 256).out.remove(0);
        db(tone(&out[12000..36000], hz))
    };
    for alias in [13000.0, 1000.0] {
        let plain = level(clip, alias) - level(clip, 7000.0);
        assert!(plain > -36.0, "without oversampling the alias at {} Hz is plain to hear: {:.1} dB", alias, plain);
        for factor in [2, 4, 8] {
            let g = format!(r#"{{"oversample":{{"factor":{},"node":{}}}}}"#, factor, clip);
            let over = level(&g, alias) - level(&g, 7000.0);
            // A hard clip's harmonics never end, so some fold back even at the higher rate: about 26 dB less alias
            // at 2x and 33 dB less at 4x and 8x, where the 55th harmonic is what lands on 1 kHz.
            assert!(over < plain - 25.0, "{}x leaves the alias at {} Hz at {:.1} dB, against {:.1}", factor, alias, over, plain);
        }
    }
    // The harmonic that stays below Nyquist comes through.
    let g = format!(r#"{{"oversample":{{"factor":4,"node":{}}}}}"#, clip);
    near(level(&g, 21000.0), level(clip, 21000.0), 1.0, "the 3rd harmonic, at 21 kHz");
}

#[test]
fn oversampling_lines_up_a_node_with_latency() {
    // At 192 kHz this lookahead is 2 samples, half a sample at 48 kHz, so the node pads it to a whole one.
    let g = r#"{"oversample":{"factor":4,"node":{"block":"dynamics","args":{"threshold_db":0,"lookahead_ms":0.01}}},"mix":0.5}"#;
    let r = run(g, &NONE, &[], &[impulse(4800, 1000)], 100);
    assert_eq!(r.latency, 39);
    for hz in [200.0, 5000.0, 15000.0] {
        near(gain_at(g, &NONE, &[], hz), 0.0, 0.05, &format!("dry and oversampled add flat at {} Hz", hz));
    }
}

#[test]
fn an_oversampled_node_follows_its_arguments() {
    let g = r#"{"oversample":{"factor":4,"node":{"block":"gain","args":{"gain_db":"g"}}}}"#;
    near(gain_at(g, &params(), &[("g", -9.0)], 1000.0), -9.0, 0.01, "a gain inside");
    let lp = r#"{"oversample":{"factor":2,"node":{"block":"filter","args":{"type":"lowpass","freq":"tone"}}}}"#;
    near(gain_at(lp, &params(), &[("tone", 2000.0)], 2000.0), -3.0, 0.2, "a filter inside, set by a parameter");
    // The inner node's tail is counted at the plugin's rate.
    let echo = |os: bool| {
        let d = r#"{"block":"delay","args":{"time_ms":250,"feedback":0.5}}"#;
        let g = if os { format!(r#"{{"oversample":{{"factor":4,"node":{}}}}}"#, d) } else { d.to_string() };
        run(&g, &NONE, &[], &[vec![0.0; 64]], 64).tail as i64
    };
    assert!((echo(true) - echo(false)).abs() <= 40, "tails {} and {}", echo(true), echo(false));
}

/// A quarter second of a loud 60 Hz kick-like tone, then a quarter second of silence, over and over.
fn bursts(frames: usize) -> Vec<f32> {
    sine(60.0, 0.8, frames)
        .into_iter()
        .enumerate()
        .map(|(i, v)| if (i / 12000) % 2 == 0 { v } else { 0.0 })
        .collect()
}

/// The level in dB of `x` over the middle of each loud quarter second of `bursts`, and of each quiet one.
fn loud_and_quiet(x: &[f32]) -> (f32, f32) {
    let part = |odd: usize| {
        let mut sum = Vec::new();
        for q in (odd..x.len() / 12000).step_by(2) {
            sum.extend_from_slice(&x[q * 12000 + 3000..q * 12000 + 9000]);
        }
        db(rms(&sum))
    };
    (part(0), part(1))
}

#[test]
fn a_compressor_keyed_by_the_sidechain_ducks_the_input() {
    let g = r#"{"block":"dynamics","args":{"key":"sidechain","threshold_db":-30,"ratio":20,"attack_ms":1,"release_ms":10}}"#;
    let input = vec![sine(440.0, 0.25, 96000), sine(440.0, 0.25, 96000)];
    let key = vec![bursts(96000)];
    let (loud, quiet) = loud_and_quiet(&run_keyed(g, &NONE, &input, &key, 100).out[1]);
    let level = db(rms(&input[0]));
    assert!(loud < level - 15.0, "while the sidechain plays, the input is ducked: {:.1} dB against {:.1}", loud, level);
    near(quiet, level, 0.1, "and between its bursts it comes back");
    // With nothing on the sidechain, nothing happens.
    let alone = run_keyed(g, &NONE, &input, &[], 100);
    near(db(rms(&alone.out[0][4800..])), level, 0.01, "an empty sidechain ducks nothing");
    let plain = run(g, &NONE, &[], &input, 100);
    near(db(rms(&plain.out[0][4800..])), level, 0.01, "nor does a host that passes none");
}

#[test]
fn an_envelope_can_follow_the_sidechain() {
    let g = r#"{"modulators":[{"id":"duck","shape":"envelope","source":"sidechain","attack_ms":2,"release_ms":40}],
               "block":"gain","args":{"gain_db":"-60 * min(duck, 0.5)"}}"#;
    let input = vec![sine(440.0, 0.25, 96000)];
    let (loud, quiet) = loud_and_quiet(&run_keyed(g, &NONE, &input, &[bursts(96000)], 64).out[0]);
    assert!(loud - quiet < -20.0, "the sidechain's level turns the input down: {:.1} against {:.1}", loud, quiet);
    // Inside an oversample node, a keyed compressor hears the sidechain too.
    let os = r#"{"oversample":{"factor":4,"node":{"block":"dynamics","args":{"key":"sidechain","threshold_db":-30,"ratio":20,"attack_ms":1,"release_ms":30}}}}"#;
    let (loud, quiet) = loud_and_quiet(&run_keyed(os, &NONE, &input, &[bursts(96000)], 64).out[0]);
    assert!(loud - quiet < -15.0, "oversampled, still ducked: {:.1} against {:.1}", loud, quiet);
}

#[test]
fn a_keyed_graph_does_not_depend_on_the_hosts_block_size() {
    let g = r#"{"modulators":[{"id":"duck","shape":"envelope","source":"sidechain","detector":"rms","attack_ms":3,"release_ms":60}],
               "chain":[{"block":"dynamics","args":{"key":"sidechain","threshold_db":-24,"ratio":4,"lookahead_ms":1}},
                        {"block":"filter","args":{"type":"lowpass","freq":"8000 - 6000 * min(duck, 1)"}}]}"#;
    let input = vec![noise(30000, 41), noise(30000, 42)];
    let key = vec![bursts(30000), noise(30000, 43)];
    let reference = run_keyed(g, &NONE, &input, &key, 32);
    for block in [1usize, 7, 100, 4096] {
        let r = run_keyed(g, &NONE, &input, &key, block);
        let worst = reference.out.iter().zip(&r.out).flat_map(|(a, b)| a.iter().zip(b).map(|(x, y)| (x - y).abs())).fold(0.0f32, f32::max);
        assert!(worst < 1e-6, "block size {} differs by {}", block, worst);
    }
}

#[test]
fn an_envelope_on_a_tap_makes_a_de_esser() {
    // A steady 500 Hz tone with bursts of 7 kHz, the sibilance, every other quarter second.
    let tone_part = sine(500.0, 0.3, 96000);
    let ess: Vec<f32> = sine(7000.0, 0.3, 96000)
        .into_iter()
        .enumerate()
        .map(|(i, v)| if (i / 12000) % 2 == 0 { v } else { 0.0 })
        .collect();
    let input = vec![tone_part.iter().zip(&ess).map(|(a, b)| a + b).collect::<Vec<f32>>()];
    let g = r#"{"modulators":[{"id":"s","shape":"envelope","source":"ess","attack_ms":1,"release_ms":20}],
                "chain":[{"tap":{"name":"ess","node":{"block":"filter","args":{"type":"highpass","freq":4000,"slope":"24"}}}},
                         {"block":"gain","args":{"gain_db":"-12 * min(s * 4, 1)"}}]}"#;
    let out = &run(g, &NONE, &[], &input, 100).out[0];
    // The 500 Hz part, measured in the middle of a quarter second with sibilance and one without.
    let during = db(tone(&out[30000..33000], 500.0));
    let between = db(tone(&out[42000..45000], 500.0));
    near(during - between, -12.0, 0.5, "the whole signal comes down 12 dB while the sibilance plays");
    near(between, db(0.3), 0.1, "and is untouched between");
    // The tap's node shapes only what the envelope hears.
    let passing = r#"{"tap":{"name":"t","node":{"block":"gain","args":{"gain_db":-40}}}}"#;
    let r = run(passing, &NONE, &[], &input, 64);
    assert_eq!(r.out[0], input[0], "the signal passes a tap unchanged");
}

#[test]
fn a_tap_in_a_split_band_hears_only_its_band() {
    let g = r#"{"modulators":[{"id":"low","shape":"envelope","source":"lows","attack_ms":1,"release_ms":20}],
                "split":{"freqs":[1000],"bands":[{"tap":{"name":"lows"}},
                                                 {"block":"gain","args":{"gain_db":"-40 * min(low * 4, 1)"}}]}}"#;
    // Bass ducks the highs; highs alone leave them be.
    let both = vec![sine(100.0, 0.4, 48000).iter().zip(sine(5000.0, 0.2, 48000)).map(|(a, b)| a + b).collect::<Vec<f32>>()];
    let highs = vec![sine(5000.0, 0.2, 48000)];
    let ducked = db(tone(&run(g, &NONE, &[], &both, 64).out[0][24000..], 5000.0));
    let alone = db(tone(&run(g, &NONE, &[], &highs, 64).out[0][24000..], 5000.0));
    assert!(ducked < alone - 30.0, "with bass the highs are ducked: {:.1} against {:.1}", ducked, alone);
    near(alone, db(0.2), 0.2, "without it they are not");
}

#[test]
fn taps_say_what_is_wrong_with_them() {
    let err = |g: &str| Graph::from_str(g, &[]).err().unwrap_or_default();
    assert!(err(r#"{"chain":[{"tap":{"name":"a"}},{"tap":{"name":"a"}}]}"#).contains("two taps are called 'a'"));
    assert!(err(r#"{"modulators":[{"id":"e","shape":"envelope","source":"b"}],"tap":{"name":"a"}}"#).contains("The taps are: a"));
    assert!(err(r#"{"modulators":[{"id":"e","shape":"envelope","source":"b"}],"chain":[]}"#).contains("has no taps"));
    assert!(err(r#"{"oversample":{"node":{"tap":{"name":"a"}}}}"#).contains("inside it"));
    assert!(err(r#"{"tap":{"name":"sidechain"}}"#).contains("not a usable tap name"));
    assert!(err(r#"{"tap":{"node":{"chain":[]}}}"#).contains("needs a name"));
}

#[test]
fn a_graph_knows_whether_it_needs_a_sidechain() {
    let uses = |g: &str| Graph::from_str(g, &["k"]).unwrap_or_else(|e| panic!("{}", e)).uses_sidechain();
    assert!(!uses(r#"{"block":"dynamics"}"#));
    assert!(!uses(r#"{"block":"dynamics","args":{"key":"input"}}"#));
    assert!(!uses(r#"{"block":"dynamics","args":{"key":0}}"#));
    assert!(uses(r#"{"block":"dynamics","args":{"key":"sidechain"}}"#));
    assert!(uses(r#"{"block":"dynamics","args":{"key":"k"}}"#), "a key that follows a parameter can be the sidechain");
    assert!(uses(r#"{"split":{"freqs":[200],"bands":[{"block":"dynamics","args":{"key":1}},{"chain":[]}]}}"#));
    assert!(uses(r#"{"modulators":[{"id":"e","shape":"envelope","source":"sidechain"}],"chain":[]}"#));
    assert!(!uses(r#"{"modulators":[{"id":"e","shape":"envelope","source":"input"}],"chain":[]}"#));
    let err = |g: &str| Graph::from_str(g, &[]).err().unwrap_or_default();
    assert!(err(r#"{"modulators":[{"id":"e","shape":"sine","source":"sidechain"}],"chain":[]}"#).contains("belong to an envelope"));
    assert!(err(r#"{"modulators":[{"id":"e","shape":"envelope","source":"aux"}],"chain":[]}"#).contains("the sidechain or a tap"));
}

#[test]
fn an_oversample_says_what_is_wrong_with_it() {
    let err = |g: &str| Graph::from_str(g, &[]).err().unwrap_or_default();
    assert!(err(r#"{"oversample":{"factor":3,"node":{"chain":[]}}}"#).contains("2, 4 or 8"));
    assert!(err(r#"{"oversample":{"factor":2}}"#).contains("needs a node"));
    assert!(err(r#"{"oversample":{"node":{"chain":[]},"rate":2}}"#).contains("rate"));
}

#[test]
fn a_feedback_loop_returns_echoes_that_fall_by_its_gain() {
    // An empty chain in a loop is a plain echo: the first return after the loop delay, then each at the gain.
    let g = r#"{"feedback":{"node":{"chain":[]},"gain":0.5,"delay_ms":100}}"#;
    let r = run(g, &NONE, &[], &[impulse(24000, 0)], 128);
    let x = &r.out[0];
    near(x[0], 0.0, 1e-6, "the loop outputs its returns alone, with nothing at once");
    near(x[4800], 1.0, 1e-4, "the first return after 100 ms");
    near(x[9600], 0.5, 1e-4, "the second at the gain");
    near(x[14400], 0.25, 1e-4, "the third");
    let between: f32 = x.iter().enumerate().filter(|(i, _)| i % 4800 != 0).map(|(_, v)| v.abs()).sum();
    assert!(between < 1e-3, "nothing between the returns: {}", between);
    // 0.5 takes ten trips to fall 60 dB, and the tail counts one more.
    assert!((r.tail as i64 - 4800 * 11).abs() <= 2, "the tail covers eleven loop delays: {}", r.tail);
}

#[test]
fn a_loop_shorter_than_a_control_step_goes_round_every_sample() {
    // 10 samples, then 1: both far shorter than a 32-sample piece.
    for (ms, step) in [(10.0 / 48.0, 10usize), (1.0 / 48.0, 1)] {
        let g = format!(r#"{{"feedback":{{"node":{{"chain":[]}},"gain":0.5,"delay_ms":{}}}}}"#, ms);
        for block in [1usize, 7, 64] {
            let r = run(&g, &NONE, &[], &[impulse(4000, 0)], block);
            for (n, want) in [(1, 1.0f32), (2, 0.5), (3, 0.25), (4, 0.125)] {
                near(r.out[0][n * step], want, 1e-4, &format!("return {} of a {}-sample loop, in blocks of {}", n, step, block));
            }
            let stray: f32 = r.out[0][..40].iter().enumerate().filter(|(i, _)| i % step != 0).map(|(_, v)| v.abs()).sum();
            assert!(stray < 1e-5, "nothing between the returns");
        }
    }
}

#[test]
fn a_short_loop_with_a_low_pass_is_a_plucked_string() {
    // Karplus-Strong: a burst of noise round a loop of 18.2 samples, 2640 Hz, darker on every trip.
    let g = r#"{"feedback":{"node":{"block":"onepole","args":{"type":"lowpass","freq":16000}},"gain":0.999,"delay_ms":"1000 / 2640"}}"#;
    let mut burst = noise(18, 3);
    burst.resize(48000, 0.0);
    let r = run(g, &NONE, &[], &[burst], 128);
    // From 20 to 140 ms: a string this high goes round 145 times every 50 ms, losing a little each time.
    let x = &r.out[0][960..6720];
    // Its pitch is the loop's, a little lower for the low-pass's own delay.
    let (pitch, level) = (2000..3300)
        .step_by(5)
        .map(|f| (f as f32, tone(x, f as f32)))
        .fold((0.0, 0.0), |best, (f, a)| if a > best.1 { (f, a) } else { best });
    near(pitch / 2640.0, 1.0, 0.02, "the string rings at the loop's pitch");
    assert!(tone(x, pitch * 1.5) < level * 0.1, "with nothing between the harmonics");
    let early = rms(&r.out[0][2000..6000]);
    let late = rms(&r.out[0][40000..44000]);
    assert!(late < early * 0.7 && late > 0.0, "and dies away: {} then {}", early, late);
    let reference = run(g, &NONE, &[], &[r.out[0].iter().map(|_| 0.0).collect()], 128);
    assert!(reference.out[0].iter().all(|v| *v == 0.0), "silence in, silence out");
}

#[test]
fn a_filter_in_the_loop_darkens_each_return() {
    let g = r#"{"feedback":{"node":{"block":"onepole","args":{"type":"lowpass","freq":2000}},"gain":0.9,"delay_ms":50}}"#;
    let r = run(g, &NONE, &[], &[impulse(48000, 0)], 128);
    // How much of each return's energy is in its sample-to-sample changes, which is to say its high end.
    let brightness = |n: usize| {
        let w = &r.out[0][2400 * (n + 1)..2400 * (n + 2)];
        let changes: f32 = w.windows(2).map(|p| (p[1] - p[0]).powi(2)).sum();
        changes / w.iter().map(|v| v * v).sum::<f32>().max(1e-20)
    };
    let (first, second, third) = (brightness(0), brightness(1), brightness(2));
    assert!(second < first * 0.8 && third < second, "each return is darker: {} {} {}", first, second, third);
}

#[test]
fn a_saturator_in_the_loop_holds_a_hot_loop() {
    let g = r#"{"feedback":{"node":{"chain":[{"block":"gain","args":{"gain_db":12}},{"block":"shaper","args":{"curve":"tanh"}}]},
                "gain":0.99,"delay_ms":20}}"#;
    let r = run(g, &NONE, &[], &[noise(48000, 5)], 128);
    let p = peak(&r.out[0]);
    assert!(p <= 1.0 + 1e-4 && p > 0.5, "12 dB of gain at 0.99 runs hot, and tanh holds it: {}", p);
    assert!(r.out[0].iter().all(|v| v.is_finite()));
}

#[test]
fn a_feedback_loop_says_what_is_wrong_with_it() {
    let err = |g: &str| Graph::from_str(g, &[]).err().unwrap_or_default();
    assert!(err(r#"{"feedback":{"gain":0.5}}"#).contains("needs a node"));
    assert!(err(r#"{"feedback":{"node":{"chain":[]},"gain":1.5}}"#).contains("outside the range"));
    assert!(err(r#"{"feedback":{"node":{"chain":[]},"delay_ms":3000}}"#).contains("Raise max_ms"));
    assert!(err(r#"{"feedback":{"node":{"chain":[]},"max_ms":"time"}}"#).contains("must be a number"));
    assert!(err(r#"{"feedback":{"node":{"tap":{"name":"t"}}}}"#).contains("inside the loop"));
    assert!(Graph::from_str(r#"{"feedback":{"node":{"chain":[]},"delay_ms":3000,"max_ms":3000}}"#, &[]).is_ok());
}

#[test]
fn the_tail_follows_the_parameters_before_any_audio_runs() {
    let infos = vec![
        ParamInfo::knob("time", "Time", "ms", 20.0, 1000.0, 400.0),
        ParamInfo::knob("fb", "Feedback", "", 0.0, 0.9, 0.5),
    ];
    let set = ParamSet::with_channels(&infos, 2);
    let ids: Vec<&str> = infos.iter().map(|i| i.id).collect();
    // Two echoes in a row beside a short one: a chain adds its tails, and parallel branches take the longest.
    let text = r#"{"parallel":[
        {"node":{"chain":[{"block":"delay","args":{"time_ms":"time","feedback":"fb"}},
                          {"block":"delay","args":{"time_ms":"time/2","feedback":"fb"}}]}},
        {"node":{"block":"delay","args":{"time_ms":"time/4","feedback":0}}}]}"#;
    let mut g = Graph::from_str(text, &ids).unwrap();
    g.prepare(&Setup {
        sample_rate: RATE,
        max_block: 512,
        channels: 2,
    });
    let close = |got: u32, want: u32, what: &str| {
        assert!((got as i64 - want as i64).abs() <= 2, "{}: got {} samples, wanted {}", what, got, want);
    };
    // At 0.5 the echoes take 10 repeats to fall 60 dB. 400 ms and 200 ms are 19200 and 9600 samples.
    close(g.tail_for(&set), (19200 + 9600) * 10, "at the defaults, before any audio");
    set.by_id("fb").unwrap().set(0.9);
    close(g.tail_for(&set), (19200 + 9600) * 60, "feedback 0.9 needs the most repeats counted");
    set.by_id("time").unwrap().set(100.0);
    close(g.tail_for(&set), (4800 + 2400) * 60, "a shorter time shortens the tail");
    // A CLAP host may ask on the audio thread.
    let before = ruse_host::alloc::count();
    let _ = g.tail_for(&set);
    assert_eq!(ruse_host::alloc::count() - before, 0, "tail_for allocated");
}

#[test]
fn the_meter_parameter_shows_the_gain_reduction() {
    let g = r#"{"block":"dynamics","args":{"threshold_db":-18,"ratio":4,"knee_db":0,"attack_ms":0.1},"meters":{"gain_reduction_db":"gr"}}"#;
    let r = run(g, &params(), &[], &[vec![vec![0.5; 20000]].remove(0)], 256);
    near(r.params.by_id("gr").unwrap().get(), 8.99, 0.1, "gr");
}

#[test]
fn mistakes_in_a_graph_say_what_and_where() {
    let ids: Vec<&str> = params().iter().map(|i| i.id).collect();
    let err = |s: &str| Graph::from_str(s, &ids).err().unwrap_or_else(|| panic!("this should not build: {}", s));

    let e = err(r#"{"block":"spring"}"#);
    assert!(e.contains("no block called 'spring'") && e.contains("reverb") && e.contains("graph"), "{}", e);

    let e = err(r#"{"chain":[{"block":"gain","args":{"gane_db":3}}]}"#);
    assert!(e.contains("graph.chain[0]") && e.contains("no argument 'gane_db'") && e.contains("gain_db"), "{}", e);

    let e = err(r#"{"block":"gain","args":{"gain_db":90}}"#);
    assert!(e.contains("90") && e.contains("-96") && e.contains("48") && e.contains("gain_db"), "{}", e);

    let e = err(r#"{"block":"gain","args":{"gain_db":"drive"}}"#);
    assert!(e.contains("'drive' is not a parameter") && e.contains("tone"), "the message lists what exists: {}", e);

    let e = err(r#"{"block":"dynamics","args":{"lookahead_ms":"g"}}"#);
    assert!(e.contains("fixed") && e.contains("lookahead_ms"), "{}", e);

    let e = err(r#"{"block":"filter","args":{"type":"lowpas"}}"#);
    assert!(e.contains("not one of lowpass"), "{}", e);

    let e = err(r#"{"block":"gain","meters":{"loudness":"gr"}}"#);
    assert!(e.contains("no meter 'loudness'") && e.contains("no meters"), "{}", e);

    let e = err(r#"{"block":"dynamics","meters":{"gain_reduction_db":"nowhere"}}"#);
    assert!(e.contains("'nowhere', which is not a parameter"), "{}", e);

    let e = err(r#"{"modulators":[{"id":"g"}],"chain":[]}"#);
    assert!(e.contains("same name as a parameter"), "{}", e);

    let e = err(r#"{"modulators":[{"id":"a","shape":"wobble"}],"chain":[]}"#);
    assert!(e.contains("wobble") && e.contains("triangle"), "{}", e);
}
