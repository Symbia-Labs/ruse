//! Each gate has a plugin written to fail it, so a gate that goes quiet shows up here.

use ruse_core::{BusInfo, Channels, Layout, ParamInfo, ParamSet, Plugin, ProcessContext, Setup};
use ruse_host::alloc::CountingAlloc;
use ruse_host::gates::{self, Gate};
use std::sync::atomic::{AtomicU64, Ordering};

#[global_allocator]
static ALLOC: CountingAlloc = CountingAlloc::new();

static COUNTER: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Copy, PartialEq)]
enum Fault {
    None,
    Alloc,
    Nan,
    Random,
    BlockDependent,
    Undescribed,
    Panics,
    /// A filter state that decays by half every sample and is never flushed.
    Subnormal,
    /// 60 dB of gain, but only with the level part way between its limits, as when two feedback paths line up.
    Runaway,
}

const LAYOUT: Layout = Layout {
    inputs: &[BusInfo {
        name: "Main",
        channels: Channels::Range { min: 1, max: 8 },
    }],
    outputs: &[BusInfo {
        name: "Main",
        channels: Channels::SameAsInput,
    }],
};

struct Probe {
    params: ParamSet,
    fault: Fault,
    // Per channel, so the result does not depend on how the host orders the channels and blocks.
    state: [f32; 8],
}

impl Probe {
    fn new(fault: Fault) -> Self {
        let level = match fault {
            Fault::Undescribed => ParamInfo::knob("level", "Level", "dB", -60.0, 12.0, 0.0),
            Fault::Panics => ParamInfo::knob("level", "Level", "dB", 0.0, 1.0, 5.0),
            _ => ParamInfo {
                description: "Output level.",
                ..ParamInfo::knob("level", "Level", "dB", -60.0, 12.0, 0.0)
            },
        };
        Probe {
            params: ParamSet::new(&[level]),
            fault,
            state: [0.0; 8],
        }
    }
}

impl Plugin for Probe {
    fn layout(&self) -> Layout {
        LAYOUT
    }

    fn params(&self) -> &ParamSet {
        &self.params
    }

    fn prepare(&mut self, _setup: &Setup) {}

    fn process(&mut self, channels: &mut [&mut [f32]], _ctx: &ProcessContext) {
        let level = self.params.at(0).get();
        for (c, ch) in channels.iter_mut().enumerate() {
            match self.fault {
                Fault::Alloc => {
                    let v: Vec<f32> = Vec::with_capacity(8);
                    std::hint::black_box(&v);
                }
                Fault::BlockDependent => {
                    if let Some(first) = ch.first_mut() {
                        *first = 0.0;
                    }
                }
                _ => {}
            }
            for s in ch.iter_mut() {
                match self.fault {
                    Fault::Nan if level >= 11.9 => *s = f32::NAN,
                    Fault::Runaway if (-40.0..=-20.0).contains(&level) => *s *= 1000.0,
                    Fault::Random => *s += COUNTER.fetch_add(1, Ordering::Relaxed) as f32 * 1e-9,
                    Fault::Subnormal => {
                        self.state[c] = self.state[c] * 0.5 + *s * 0.5;
                        *s = self.state[c];
                    }
                    _ => {}
                }
            }
        }
    }
}

fn verdict(gates: &[Gate], name: &str) -> bool {
    gates
        .iter()
        .find(|g| g.name == name)
        .unwrap_or_else(|| panic!("no gate named {}: {}", name, gates::report(gates)))
        .passed
}

#[test]
fn a_clean_plugin_passes_every_gate() {
    let g = gates::run_all(|| Probe::new(Fault::None));
    println!("{}", gates::report(&g));
    assert_eq!(g.len(), 7);
    assert!(gates::all_passed(&g));
}

#[test]
fn a_decaying_state_that_is_never_flushed_fails_no_subnormals() {
    let g = gates::run_all(|| Probe::new(Fault::Subnormal));
    assert!(!verdict(&g, "no_subnormals"), "{}", gates::report(&g));
    assert!(verdict(&g, "finite_at_extremes"));
    assert!(verdict(&g, "block_size_independent"));
}

#[test]
fn ruse_gain_passes_every_gate() {
    let g = gates::run_all(ruse_gain::Gain::new);
    println!("{}", gates::report(&g));
    assert!(gates::all_passed(&g));
}

#[test]
fn allocation_in_process_fails_no_alloc() {
    let g = gates::run_all(|| Probe::new(Fault::Alloc));
    assert!(!verdict(&g, "no_alloc"), "{}", gates::report(&g));
    assert!(verdict(&g, "finite_at_extremes"));
}

#[test]
fn gain_that_runs_away_between_the_limits_fails_finite_at_extremes() {
    let g = gates::run_all(|| Probe::new(Fault::Runaway));
    let gate = g.iter().find(|g| g.name == "finite_at_extremes").unwrap();
    assert!(!gate.passed, "{}", gates::report(&g));
    assert!(gate.detail.contains("louder than its input"), "{}", gate.detail);
}

#[test]
fn nan_output_fails_finite_at_extremes() {
    let g = gates::run_all(|| Probe::new(Fault::Nan));
    assert!(!verdict(&g, "finite_at_extremes"), "{}", gates::report(&g));
    assert!(verdict(&g, "no_alloc"));
}

#[test]
fn unseeded_randomness_fails_repeatable() {
    let g = gates::run_all(|| Probe::new(Fault::Random));
    assert!(!verdict(&g, "repeatable"), "{}", gates::report(&g));
}

#[test]
fn output_that_depends_on_the_block_size_fails() {
    let g = gates::run_all(|| Probe::new(Fault::BlockDependent));
    assert!(!verdict(&g, "block_size_independent"), "{}", gates::report(&g));
    assert!(verdict(&g, "repeatable"));
}

#[test]
fn a_parameter_without_a_description_fails() {
    let g = gates::run_all(|| Probe::new(Fault::Undescribed));
    assert!(!verdict(&g, "parameters_described"), "{}", gates::report(&g));
}

#[test]
fn a_refused_declaration_is_reported_not_propagated() {
    let g = gates::run_all(|| Probe::new(Fault::Panics));
    assert_eq!(g.len(), 1);
    assert_eq!(g[0].name, "construct");
    assert!(!g[0].passed);
}
