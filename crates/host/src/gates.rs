//! The checks every plugin has to pass, whoever wrote it.
//!
//! A plugin built by an agent has no human reader behind it. These gates are what let anyone trust it.
//! Each gate reports a name, a verdict and one line of detail.

use crate::alloc;
use crate::measure;
use crate::render::{self, Options, Setting};
use crate::signal::{self, Signal};
use ruse_core::{Kind, ParamInfo, Plugin, ProcessContext};
use std::panic::{catch_unwind, AssertUnwindSafe};

pub struct Gate {
    pub name: &'static str,
    pub passed: bool,
    pub detail: String,
}

const RATE: f32 = 48000.0;
const CANDIDATES: [usize; 6] = [1, 2, 6, 8, 24, 64];

fn pass(name: &'static str, detail: impl Into<String>) -> Gate {
    Gate {
        name,
        passed: true,
        detail: detail.into(),
    }
}

fn fail(name: &'static str, detail: impl Into<String>) -> Gate {
    Gate {
        name,
        passed: false,
        detail: detail.into(),
    }
}

/// `GATE name PASS|FAIL detail`, one line each, then a total.
pub fn report(gates: &[Gate]) -> String {
    let mut out = String::new();
    for g in gates {
        out.push_str(&format!(
            "GATE {} {} {}\n",
            g.name,
            if g.passed { "PASS" } else { "FAIL" },
            g.detail
        ));
    }
    let passed = gates.iter().filter(|g| g.passed).count();
    out.push_str(&format!("GATES {} of {} passed", passed, gates.len()));
    out
}

pub fn all_passed(gates: &[Gate]) -> bool {
    !gates.is_empty() && gates.iter().all(|g| g.passed)
}

fn accepted(p: &dyn Plugin) -> Vec<usize> {
    CANDIDATES
        .iter()
        .copied()
        .filter(|&n| p.layout().accepts_main(n as u32))
        .collect()
}

/// Noise for a sidechain, unlike the noise the gates play into the main input.
fn key_noise(db: f32, frames: usize) -> Vec<Vec<f32>> {
    vec![signal::generate(&Signal::Noise { db }, RATE, frames, 8).swap_remove(7)]
}

/// What a plugin with a sidechain is tried with there: silence, and loud noise. A plugin without one gets silence.
fn key_levels(p: &dyn Plugin) -> &'static [Option<f32>] {
    if p.layout().sidechain().is_some() {
        &[None, Some(0.0)]
    } else {
        &[None]
    }
}

fn key_label(p: &dyn Plugin, level: Option<f32>) -> String {
    match (p.layout().sidechain().is_some(), level) {
        (false, _) => String::new(),
        (true, None) => ", sidechain silent".to_string(),
        (true, Some(db)) => format!(", sidechain white noise at {} dBFS", db),
    }
}

/// The smallest accepted count, and the largest accepted count up to 24.
fn test_counts(accepted: &[usize]) -> Vec<usize> {
    let mut v = Vec::new();
    if let Some(&first) = accepted.first() {
        v.push(first);
    }
    if let Some(&wide) = accepted.iter().rev().find(|&&n| n <= 24) {
        if !v.contains(&wide) {
            v.push(wide);
        }
    }
    v
}

fn layout_gate(p: &dyn Plugin, accepted: &[usize]) -> Gate {
    if accepted.is_empty() {
        return fail(
            "layout",
            "none of 1, 2, 6, 8, 24 or 64 channels is accepted when the input and output counts match",
        );
    }
    let list: Vec<String> = accepted.iter().map(|n| n.to_string()).collect();
    let key = match p.layout().sidechain() {
        Some(b) => format!(", with a sidechain input called {}", b.name),
        None => String::new(),
    };
    pass("layout", format!("accepts {} channels{}", list.join(", "), key))
}

fn described_gate(p: &dyn Plugin) -> Gate {
    let mut missing: Vec<&str> = Vec::new();
    for param in p.params().iter() {
        let info = param.info();
        if info.name.trim().is_empty() || info.description.trim().is_empty() {
            missing.push(info.id);
        }
    }
    if missing.is_empty() {
        pass(
            "parameters_described",
            format!("{} parameters, each with a name and a description", p.params().len()),
        )
    } else {
        fail(
            "parameters_described",
            format!("these parameters need a name and a description: {}", missing.join(", ")),
        )
    }
}

fn extremes(p: &dyn Plugin) -> Vec<(String, f32, f32)> {
    p.params()
        .iter()
        .filter(|param| param.info().kind != Kind::Meter)
        .map(|param| (param.info().id.to_string(), param.info().min, param.info().max))
        .collect()
}

/// How much louder than its input any setting may make a plugin. A gain stage, an equalizer boost or a
/// compressor's makeup gain fits inside it. Feedback that builds on itself does not.
const MAX_GAIN_DB: f32 = 30.0;

/// The level of the signals the gain check plays.
const PROBE_DB: f32 = -18.0;

/// A plugin with up to this many parameters has every combination of their limits tried.
const ALL_LIMITS_UP_TO: usize = 7;

/// Settings between the limits that the gain check also tries. They come from a fixed sequence, so every run
/// tries the same ones.
const BETWEEN: usize = 24;

/// A value for a gate's message, without trailing zeros.
fn short(v: f32) -> String {
    let s = format!("{:.3}", v);
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

/// The settings the gain check tries. Runaway gain usually needs several parameters pushed at once, and sometimes
/// one of them part way, as when feedback is high and two delay times line up. So a plugin with few parameters
/// gets every combination of their limits, a larger one gets each parameter flipped away from all-minimum and
/// all-maximum, and both get settings in between.
fn gain_cases(p: &dyn Plugin) -> Vec<(String, Vec<Setting>)> {
    let infos: Vec<ParamInfo> = p
        .params()
        .iter()
        .map(|q| *q.info())
        .filter(|i| i.kind != Kind::Meter)
        .collect();
    let n = infos.len();
    if n == 0 {
        return vec![("the default settings".to_string(), Vec::new())];
    }
    let mut picks: Vec<Vec<f32>> = Vec::new();
    if n <= ALL_LIMITS_UP_TO {
        for mask in 0..(1usize << n) {
            picks.push(
                infos
                    .iter()
                    .enumerate()
                    .map(|(i, info)| if (mask >> i) & 1 == 1 { info.max } else { info.min })
                    .collect(),
            );
        }
    } else {
        let lo: Vec<f32> = infos.iter().map(|i| i.min).collect();
        let hi: Vec<f32> = infos.iter().map(|i| i.max).collect();
        picks.push(lo.clone());
        picks.push(hi.clone());
        for (k, info) in infos.iter().enumerate() {
            let mut up = lo.clone();
            up[k] = info.max;
            picks.push(up);
            let mut down = hi.clone();
            down[k] = info.min;
            picks.push(down);
        }
    }
    let mut seed = 0x2545_F491u32;
    for _ in 0..BETWEEN {
        picks.push(
            infos
                .iter()
                .map(|info| {
                    seed ^= seed << 13;
                    seed ^= seed >> 17;
                    seed ^= seed << 5;
                    info.to_plain((seed >> 8) as f32 / (1u32 << 24) as f32)
                })
                .collect(),
        );
    }
    picks
        .iter()
        .map(|values| {
            let settings: Vec<Setting> = infos
                .iter()
                .zip(values)
                .map(|(info, &value)| Setting {
                    id: info.id.to_string(),
                    channel: None,
                    value,
                })
                .collect();
            let label: Vec<String> = settings.iter().map(|s| format!("{}={}", s.id, short(s.value))).collect();
            (label.join(" "), settings)
        })
        .collect()
}

fn finite_gate<P: Plugin>(make: &impl Fn() -> P, counts: &[usize], gain_channels: usize) -> Gate {
    const NAME: &str = "finite_at_extremes";
    let probe = make();
    let mut cases: Vec<(String, Vec<Setting>)> = vec![("defaults".to_string(), Vec::new())];
    for (id, lo, hi) in extremes(&probe) {
        for (label, value) in [("min", lo), ("max", hi)] {
            cases.push((
                format!("{}={}", id, label),
                vec![Setting {
                    id: id.clone(),
                    channel: None,
                    value,
                }],
            ));
        }
    }
    let signals = [
        Signal::Silence,
        Signal::Impulse { db: 0.0 },
        Signal::Sine { hz: 1000.0, db: -6.0 },
        Signal::Noise { db: 0.0 },
        Signal::Nyquist { db: 0.0 },
    ];
    let mut loudest = f32::MIN;
    let mut renders = 0usize;
    for &n in counts {
        for (label, settings) in &cases {
            for (sig, &key) in signals.iter().flat_map(|s| key_levels(&probe).iter().map(move |k| (s, k))) {
                let frames = RATE as usize / 4;
                let input = signal::generate(sig, RATE, frames, n);
                let mut p = make();
                // Anything that runs away does so while the input plays, so the tail is left out.
                let opts = Options {
                    settings: settings.clone(),
                    tail: false,
                    sidechain: key.map(|db| key_noise(db, frames)).unwrap_or_default(),
                    ..Options::default()
                };
                let label = format!("{}{}", label, key_label(&probe, key));
                let rendered = match catch_unwind(AssertUnwindSafe(|| render::render(&mut p, &input, &opts))) {
                    Ok(Ok(r)) => r,
                    Ok(Err(e)) => return fail(NAME, format!("{} channels, {}: {}", n, label, e)),
                    Err(_) => {
                        return fail(
                            NAME,
                            format!("process panicked with {} channels, {}, {}", n, label, sig.describe()),
                        )
                    }
                };
                for (c, ch) in rendered.output.iter().enumerate() {
                    let bad = measure::nonfinite(ch);
                    if bad > 0 {
                        return fail(
                            NAME,
                            format!(
                                "{} non-finite samples on channel {} with {} channels, {}, input {}",
                                bad,
                                c + 1,
                                n,
                                label,
                                sig.describe()
                            ),
                        );
                    }
                    let peak = measure::peak_db(ch);
                    if peak > 60.0 {
                        return fail(
                            NAME,
                            format!(
                                "output reaches {:.0} dBFS on channel {} with {} channels, {}, input {}",
                                peak,
                                c + 1,
                                n,
                                label,
                                sig.describe()
                            ),
                        );
                    }
                    loudest = loudest.max(peak);
                }
                renders += 1;
            }
        }
    }
    // Then the gain check: quieter signals, long enough for feedback to build, at combinations of settings.
    let limit = PROBE_DB + MAX_GAIN_DB;
    let gain_signals = [
        Signal::Noise { db: PROBE_DB },
        Signal::Sine { hz: 55.0, db: PROBE_DB },
    ];
    let mut most_gain = f32::MIN;
    let mut settings_tried = 0usize;
    for (label, settings) in gain_cases(&probe) {
        for (sig, &key) in gain_signals.iter().flat_map(|s| key_levels(&probe).iter().map(move |k| (s, k))) {
            let frames = RATE as usize * 2;
            let input = signal::generate(sig, RATE, frames, gain_channels);
            let mut p = make();
            let opts = Options {
                settings: settings.clone(),
                tail: false,
                sidechain: key.map(|db| key_noise(db, frames)).unwrap_or_default(),
                ..Options::default()
            };
            let label = format!("{}{}", label, key_label(&probe, key));
            let rendered = match catch_unwind(AssertUnwindSafe(|| render::render(&mut p, &input, &opts))) {
                Ok(Ok(r)) => r,
                Ok(Err(e)) => return fail(NAME, format!("{} channels, {}: {}", gain_channels, label, e)),
                Err(_) => {
                    return fail(
                        NAME,
                        format!("process panicked with {} channels, {}, {}", gain_channels, label, sig.describe()),
                    )
                }
            };
            for (c, ch) in rendered.output.iter().enumerate() {
                let bad = measure::nonfinite(ch);
                if bad > 0 {
                    return fail(
                        NAME,
                        format!(
                            "{} non-finite samples on channel {} with {}, input {}",
                            bad,
                            c + 1,
                            label,
                            sig.describe()
                        ),
                    );
                }
                let peak = measure::peak_db(ch);
                if peak > limit {
                    return fail(
                        NAME,
                        format!(
                            "output reaches {:+.0} dBFS on channel {}, {:.0} dB louder than its input, with {}, input {}. No setting may add more than {:.0} dB",
                            peak,
                            c + 1,
                            peak - PROBE_DB,
                            label,
                            sig.describe(),
                            MAX_GAIN_DB
                        ),
                    );
                }
                most_gain = most_gain.max(peak - PROBE_DB);
            }
        }
        settings_tried += 1;
    }
    pass(
        NAME,
        format!(
            "{} renders stayed finite with every parameter at its minimum and maximum, loudest output {:.1} dBFS. Across {} combinations of settings the most gain was {:+.1} dB, within {:.0} dB",
            renders, loudest, settings_tried, most_gain, MAX_GAIN_DB
        ),
    )
}

fn alloc_gate<P: Plugin>(make: &impl Fn() -> P, counts: &[usize]) -> Gate {
    const NAME: &str = "no_alloc";
    if !alloc::installed() {
        return fail(
            NAME,
            "the counting allocator is not installed. The test file needs #[global_allocator] static A: ruse_host::alloc::CountingAlloc",
        );
    }
    let mut blocks = 0usize;
    for &n in counts {
        let mut p = make();
        if let Err(e) = render::configure(&mut p, n, &Options::default()) {
            return fail(NAME, e);
        }
        let ranges = extremes(&p);
        let mut bufs = signal::generate(&Signal::Noise { db: -12.0 }, RATE, 512, n);
        let key = render::sidechain_input(
            p.layout().sidechain_channels(n as u32).unwrap_or(0) as usize,
            &key_noise(-12.0, 512),
            512,
        );
        let mut total = 0usize;
        for b in 0..60usize {
            if b % 10 == 5 {
                for (id, lo, hi) in &ranges {
                    let value = if (b / 10) % 2 == 0 { *hi } else { *lo };
                    let _ = p.params().set_plain(id, value);
                }
            }
            let mut chans: Vec<&mut [f32]> = bufs.iter_mut().map(|c| &mut c[..]).collect();
            let ctx = ProcessContext {
                sample_rate: RATE,
                sample_time: Some((b * 512) as f64),
                playing: true,
                tempo_bpm: None,
            };
            // Built before counting: the slices themselves are the host's, not the plugin's.
            let keys: Vec<&[f32]> = key.iter().map(|c| &c[..]).collect();
            let before = alloc::count();
            if key.is_empty() {
                p.process(&mut chans, &ctx);
            } else {
                p.process_with_sidechain(&mut chans, &keys, &ctx);
            }
            total += alloc::count() - before;
            blocks += 1;
        }
        if total > 0 {
            return fail(
                NAME,
                format!(
                    "process allocated {} times over 60 blocks with {} channels, parameters changing every 10 blocks",
                    total, n
                ),
            );
        }
    }
    pass(
        NAME,
        format!("process made no allocations over {} blocks, with parameters changing between them", blocks),
    )
}

fn render_default<P: Plugin>(make: &impl Fn() -> P, n: usize, block: usize) -> Result<Vec<Vec<f32>>, String> {
    let input = signal::generate(&Signal::Noise { db: -12.0 }, RATE, RATE as usize / 2, n);
    let mut p = make();
    let opts = Options {
        block,
        sidechain: key_noise(-12.0, RATE as usize / 2),
        ..Options::default()
    };
    catch_unwind(AssertUnwindSafe(|| render::render(&mut p, &input, &opts)))
        .map_err(|_| "process panicked".to_string())?
        .map(|r| r.output)
}

fn repeatable_gate<P: Plugin>(make: &impl Fn() -> P, counts: &[usize]) -> Gate {
    const NAME: &str = "repeatable";
    for &n in counts {
        let a = match render_default(make, n, 128) {
            Ok(a) => a,
            Err(e) => return fail(NAME, e),
        };
        let b = match render_default(make, n, 128) {
            Ok(b) => b,
            Err(e) => return fail(NAME, e),
        };
        let same = a
            .iter()
            .zip(&b)
            .all(|(x, y)| x.len() == y.len() && x.iter().zip(y).all(|(p, q)| p.to_bits() == q.to_bits()));
        if !same {
            return fail(
                NAME,
                format!(
                    "two fresh instances rendered different audio from the same input with {} channels. Check for unseeded randomness or state shared between instances",
                    n
                ),
            );
        }
    }
    pass(NAME, "two fresh instances rendered identical audio from the same input")
}

fn block_gate<P: Plugin>(make: &impl Fn() -> P, counts: &[usize]) -> Gate {
    const NAME: &str = "block_size_independent";
    let mut worst = 0f32;
    for &n in counts {
        let small = match render_default(make, n, 32) {
            Ok(a) => a,
            Err(e) => return fail(NAME, e),
        };
        let large = match render_default(make, n, 512) {
            Ok(a) => a,
            Err(e) => return fail(NAME, e),
        };
        for (x, y) in small.iter().zip(&large) {
            for (p, q) in x.iter().zip(y) {
                worst = worst.max((p - q).abs());
            }
        }
    }
    if worst > 1e-4 {
        fail(
            NAME,
            format!(
                "output differs by up to {:.1} dBFS between a block size of 32 and 512. Hosts choose their own block size, so the audio must not depend on it",
                20.0 * worst.log10()
            ),
        )
    } else {
        pass(
            NAME,
            "audio is the same at a block size of 32 and 512 (largest difference under -80 dBFS)",
        )
    }
}

/// Subnormal floats are not wrong, but a processor handles each one far more slowly than a normal number, and a
/// decaying filter or feedback path is where they come from. Wrappers flush them while the plugin runs, but a host
/// or a format that does not would stall on a plugin that makes them. This gate runs a burst of noise followed by
/// silence, with the default settings and with every parameter at each limit, and counts them.
fn subnormal_gate<P: Plugin>(make: &impl Fn() -> P, counts: &[usize]) -> Gate {
    const NAME: &str = "no_subnormals";
    let probe = make();
    let limits = extremes(&probe);
    let at = |pick: fn(&(String, f32, f32)) -> f32| -> Vec<Setting> {
        limits
            .iter()
            .map(|l| Setting {
                id: l.0.clone(),
                channel: None,
                value: pick(l),
            })
            .collect()
    };
    let cases: Vec<(&str, Vec<Setting>)> = vec![
        ("default settings", Vec::new()),
        ("every parameter at its minimum", at(|l| l.1)),
        ("every parameter at its maximum", at(|l| l.2)),
    ];
    let quiet = RATE as usize * 3 / 2;
    for &n in counts {
        for (label, settings) in &cases {
            let mut input = signal::generate(&Signal::Noise { db: -6.0 }, RATE, RATE as usize / 2, n);
            for ch in input.iter_mut() {
                ch.extend(std::iter::repeat_n(0.0, quiet));
            }
            let mut p = make();
            // A sidechain gets the same burst and silence as the input.
            let opts = Options {
                settings: settings.clone(),
                sidechain: vec![input[0].clone()],
                ..Options::default()
            };
            let rendered = match catch_unwind(AssertUnwindSafe(|| render::render(&mut p, &input, &opts))) {
                Ok(Ok(r)) => r,
                Ok(Err(e)) => return fail(NAME, format!("{} channels, {}: {}", n, label, e)),
                Err(_) => return fail(NAME, format!("process panicked with {} channels, {}", n, label)),
            };
            let bad: usize = rendered
                .output
                .iter()
                .map(|ch| ch.iter().filter(|v| **v != 0.0 && v.abs() < f32::MIN_POSITIVE).count())
                .sum();
            if bad > 0 {
                return fail(
                    NAME,
                    format!(
                        "{} subnormal samples in the output after the input went silent, with {} channels and {}",
                        bad, n, label
                    ),
                );
            }
        }
    }
    pass(NAME, "no subnormal samples in the output while a burst of noise decays")
}

/// Runs every gate. `make` builds a fresh instance each time it is called.
pub fn run_all<P: Plugin>(make: impl Fn() -> P) -> Vec<Gate> {
    let first = match catch_unwind(AssertUnwindSafe(&make)) {
        Ok(p) => p,
        Err(_) => {
            return vec![fail(
                "construct",
                "creating the plugin panicked. The message above says which parameter declaration was refused",
            )]
        }
    };
    let acc = accepted(&first);
    let counts = test_counts(&acc);
    let mut gates = vec![layout_gate(&first, &acc), described_gate(&first)];
    if counts.is_empty() {
        return gates;
    }
    // The gain check runs once, in stereo when the plugin takes it, because ping-pong and other cross-channel
    // paths only exist with two channels.
    let gain_channels = if acc.contains(&2) { 2 } else { counts[0] };
    gates.push(finite_gate(&make, &counts, gain_channels));
    gates.push(alloc_gate(&make, &counts));
    gates.push(repeatable_gate(&make, &counts));
    gates.push(block_gate(&make, &counts));
    gates.push(subnormal_gate(&make, &counts));
    gates
}
