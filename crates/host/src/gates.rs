//! The checks every plugin has to pass, whoever wrote it.
//!
//! A plugin built by an agent has no human reader behind it. These gates are what let anyone trust it.
//! Each gate reports a name, a verdict and one line of detail.

use crate::alloc;
use crate::measure;
use crate::render::{self, Options, Setting};
use crate::signal::{self, Signal};
use muse_core::{Kind, Plugin, ProcessContext};
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
        .filter(|&n| p.layout().accepts(&[n as u32], &[n as u32]))
        .collect()
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

fn layout_gate(accepted: &[usize]) -> Gate {
    if accepted.is_empty() {
        return fail(
            "layout",
            "none of 1, 2, 6, 8, 24 or 64 channels is accepted when the input and output counts match",
        );
    }
    let list: Vec<String> = accepted.iter().map(|n| n.to_string()).collect();
    pass("layout", format!("accepts {} channels", list.join(", ")))
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

fn finite_gate<P: Plugin>(make: &impl Fn() -> P, counts: &[usize]) -> Gate {
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
            for sig in &signals {
                let input = signal::generate(sig, RATE, RATE as usize / 4, n);
                let mut p = make();
                let opts = Options {
                    settings: settings.clone(),
                    ..Options::default()
                };
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
    pass(
        NAME,
        format!(
            "{} renders stayed finite with every parameter at its minimum and maximum, loudest output {:.1} dBFS",
            renders, loudest
        ),
    )
}

fn alloc_gate<P: Plugin>(make: &impl Fn() -> P, counts: &[usize]) -> Gate {
    const NAME: &str = "no_alloc";
    if !alloc::installed() {
        return fail(
            NAME,
            "the counting allocator is not installed. The test file needs #[global_allocator] static A: muse_host::alloc::CountingAlloc",
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
            let before = alloc::count();
            p.process(&mut chans, &ctx);
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
            let opts = Options {
                settings: settings.clone(),
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
    let mut gates = vec![layout_gate(&acc), described_gate(&first)];
    if counts.is_empty() {
        return gates;
    }
    gates.push(finite_gate(&make, &counts));
    gates.push(alloc_gate(&make, &counts));
    gates.push(repeatable_gate(&make, &counts));
    gates.push(block_gate(&make, &counts));
    gates.push(subnormal_gate(&make, &counts));
    gates
}
