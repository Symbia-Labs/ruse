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
                      {"id":"env","shape":"envelope","attack_ms":3,"release_ms":80,"detector":"rms"}],
        "chain":[
          {"block":"harmonic_eq","args":{"fundamental_hz":"tone / 20","harmonics":6,"gain_db":4,"odd_db":3}},
          {"block":"filter","args":{"type":"lowpass","freq":"tone * 2 ^ lfo","q":2}},
          {"block":"shaper","args":{"curve":"mode","drive_db":"g + 12 + 6 * env","bias":0.1}},
          {"parallel":[
            {"node":{"chain":[]}},
            {"node":{"chain":[
               {"block":"delay","args":{"time_ms":7,"feedback":0.3,"mod_depth_ms":1,"mod_rate_hz":0.9}},
               {"block":"tremolo","args":{"rate_hz":3,"depth":0.3,"stereo_phase":90}}]},"gain_db":-6}]},
          {"block":"dynamics","args":{"threshold_db":-24,"ratio":3,"lookahead_ms":2},"meters":{"gain_reduction_db":"gr"}},
          {"block":"stereo","args":{"width":1.3}}
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
    let before = ruse_host::alloc::count();
    for start in (0..1900).step_by(77) {
        // An array, so the test itself does not allocate.
        let mut sl = [&mut a[start..start + 77], &mut b[start..start + 77]];
        g.process(&set, &mut sl, &ctx);
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

    let e = err(r#"{"block":"reverb"}"#);
    assert!(e.contains("no block called 'reverb'") && e.contains("harmonic_eq") && e.contains("graph"), "{}", e);

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
