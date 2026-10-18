//! A small rig for running a graph offline and measuring it.
#![allow(dead_code)]

use ruse_blocks::Graph;
use ruse_core::{ParamInfo, ParamSet, ProcessContext, Setup};

pub const RATE: f32 = 48000.0;

pub struct Run {
    pub out: Vec<Vec<f32>>,
    pub params: ParamSet,
    pub latency: u32,
    pub tail: u32,
}

/// Runs `input` through the graph in host blocks of `block` frames. `values` set parameters by id first.
pub fn run(json: &str, infos: &[ParamInfo], values: &[(&str, f32)], input: &[Vec<f32>], block: usize) -> Run {
    let params = ParamSet::with_channels(infos, 8);
    for (id, v) in values {
        params.by_id(id).unwrap_or_else(|| panic!("no parameter {}", id)).set(*v);
    }
    let ids: Vec<&str> = infos.iter().map(|i| i.id).collect();
    let mut g = Graph::from_str(json, &ids).unwrap_or_else(|e| panic!("{}", e));
    let channels = input.len();
    g.prepare(&Setup {
        sample_rate: RATE,
        max_block: block,
        channels: channels as u32,
    });
    let mut out: Vec<Vec<f32>> = input.to_vec();
    let frames = input[0].len();
    let ctx = ProcessContext::offline(RATE);
    let mut at = 0;
    while at < frames {
        let n = block.min(frames - at);
        let mut slices: Vec<&mut [f32]> = out.iter_mut().map(|c| &mut c[at..at + n]).collect();
        g.process(&params, &mut slices, &ctx);
        at += n;
    }
    Run {
        out,
        params,
        latency: g.latency_samples(),
        tail: g.tail_samples(),
    }
}

/// A graph kept running, so a test can change a parameter between blocks.
pub struct Session {
    pub graph: Graph,
    pub params: ParamSet,
    ctx: ProcessContext,
}

impl Session {
    pub fn new(json: &str, infos: &[ParamInfo], channels: usize) -> Session {
        let params = ParamSet::with_channels(infos, 8);
        let ids: Vec<&str> = infos.iter().map(|i| i.id).collect();
        let mut graph = Graph::from_str(json, &ids).unwrap_or_else(|e| panic!("{}", e));
        graph.prepare(&Setup {
            sample_rate: RATE,
            max_block: 512,
            channels: channels as u32,
        });
        Session {
            graph,
            params,
            ctx: ProcessContext::offline(RATE),
        }
    }

    pub fn set(&self, id: &str, v: f32) {
        self.params.by_id(id).unwrap().set(v);
    }

    pub fn feed(&mut self, block: &mut [Vec<f32>]) {
        let mut slices: Vec<&mut [f32]> = block.iter_mut().map(|c| &mut c[..]).collect();
        self.graph.process(&self.params, &mut slices, &self.ctx);
    }
}

pub fn sine(freq: f32, amp: f32, frames: usize) -> Vec<f32> {
    (0..frames)
        .map(|i| amp * (2.0 * std::f32::consts::PI * freq * i as f32 / RATE).sin())
        .collect()
}

pub fn rms(x: &[f32]) -> f32 {
    (x.iter().map(|v| (*v as f64) * (*v as f64)).sum::<f64>() / x.len().max(1) as f64).sqrt() as f32
}

pub fn db(x: f32) -> f32 {
    20.0 * x.max(1e-12).log10()
}

/// The gain in dB of a steady sine at `freq` through the graph, measured after the start-up has settled.
pub fn gain_at(json: &str, infos: &[ParamInfo], values: &[(&str, f32)], freq: f32) -> f32 {
    let frames = 48000;
    let input = vec![sine(freq, 0.1, frames)];
    let r = run(json, infos, values, &input, 256);
    let tail = &r.out[0][frames / 2..];
    db(rms(tail)) - db(rms(&input[0][frames / 2..]))
}

/// The amplitude of the part of `x` at `hz`. Exact when `x` holds a whole number of its cycles.
pub fn tone(x: &[f32], hz: f32) -> f32 {
    let (mut s, mut c) = (0.0f64, 0.0f64);
    for (i, v) in x.iter().enumerate() {
        let ph = std::f64::consts::TAU * hz as f64 * i as f64 / RATE as f64;
        s += *v as f64 * ph.sin();
        c += *v as f64 * ph.cos();
    }
    (2.0 * (s * s + c * c).sqrt() / x.len() as f64) as f32
}

pub fn peak(x: &[f32]) -> f32 {
    x.iter().fold(0.0f32, |m, v| m.max(v.abs()))
}

pub fn impulse(frames: usize, at: usize) -> Vec<f32> {
    let mut x = vec![0.0; frames];
    x[at] = 1.0;
    x
}

pub fn argmax(x: &[f32]) -> usize {
    x.iter().enumerate().fold((0, 0.0f32), |m, (i, v)| if v.abs() > m.1 { (i, v.abs()) } else { m }).0
}

pub fn noise(frames: usize, seed: u32) -> Vec<f32> {
    let mut s = seed | 1;
    (0..frames)
        .map(|_| {
            s ^= s << 13;
            s ^= s >> 17;
            s ^= s << 5;
            (s as f32 / u32::MAX as f32) * 2.0 - 1.0
        })
        .collect()
}
