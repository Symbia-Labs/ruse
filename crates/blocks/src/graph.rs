//! The graph runtime. A graph is a plugin's whole signal path, described as data.
//!
//! The audio is cut into pieces of at most 32 samples, aligned to a counter that survives from one host block
//! to the next. At the start of each aligned piece every argument is evaluated from the current parameters,
//! and every block gets one call to `control`. Within a piece, blocks glide from the old control value to the
//! new one by sample position, so the audio is the same whatever block size the host uses.

use crate::block::{ArgInfo, Block, BlockInfo};
use crate::blocks;
use crate::expr::{Env, Expr};
use crate::frame::{ramp, Frame, CHUNK};
use crate::spec::{self, ArgValue, GraphSpec, NodeKind, NodeSpec};
use ruse_core::{ParamSet, ProcessContext, Setup};

struct Arg {
    expr: Option<Expr>,
    value: f32,
}

impl Arg {
    fn eval(&self, info: &ArgInfo, env: &Env) -> f32 {
        info.clean(match &self.expr {
            Some(e) => e.eval(env),
            None => self.value,
        })
    }
}

/// Compiles one argument. A number is checked against the range. A string is a choice's name or an expression.
fn compile_arg(v: &ArgValue, info: &ArgInfo, c: &Compiler, what: &str) -> Result<Arg, String> {
    let out_of_range = |x: f32| {
        format!(
            "{} is {}, outside the range {} to {}{}",
            what,
            x,
            info.min,
            info.max,
            if info.unit.is_empty() { String::new() } else { format!(" {}", info.unit) }
        )
    };
    match v {
        ArgValue::Num(n) => {
            let x = *n as f32;
            if !x.is_finite() || x < info.min || x > info.max {
                return Err(out_of_range(x));
            }
            Ok(Arg { expr: None, value: x })
        }
        ArgValue::Text(s) => {
            if info.is_choice() {
                if let Some(i) = info.options.iter().position(|o| o.eq_ignore_ascii_case(s.trim())) {
                    return Ok(Arg {
                        expr: None,
                        value: i as f32,
                    });
                }
            }
            let expr = c.parse(s).map_err(|e| {
                if info.is_choice() {
                    format!("{}: '{}' is not one of {} and not a valid expression ({})", what, s, info.options.join(", "), e)
                } else {
                    format!("{}: {}", what, e)
                }
            })?;
            if let Some(k) = expr.constant() {
                if !k.is_finite() || k < info.min || k > info.max {
                    return Err(out_of_range(k));
                }
                return Ok(Arg { expr: None, value: k });
            }
            if info.fixed {
                return Err(format!("{} is fixed when the plugin is built, so it must be a number and cannot follow a parameter", what));
            }
            Ok(Arg {
                expr: Some(expr),
                value: info.default,
            })
        }
    }
}

struct Compiler<'a> {
    ids: &'a [&'a str],
    mods: Vec<String>,
}

impl Compiler<'_> {
    fn lookup(&self, name: &str) -> Option<usize> {
        self.ids
            .iter()
            .position(|x| *x == name)
            .or_else(|| self.mods.iter().position(|m| m == name).map(|i| self.ids.len() + i))
    }

    fn parse(&self, src: &str) -> Result<Expr, String> {
        Expr::parse(src, &|n| self.lookup(n)).map_err(|e| {
            let mut names: Vec<String> = self.ids.iter().map(|s| s.to_string()).collect();
            names.extend(self.mods.iter().cloned());
            format!("{}. The names available are: {}", e, names.join(", "))
        })
    }
}

struct BlockNode {
    block: Box<dyn Block>,
    info: &'static BlockInfo,
    args: Vec<Arg>,
    resolved: Vec<f32>,
    /// Meter index in the block, then parameter index.
    meters: Vec<(usize, usize)>,
}

struct MixNode {
    inner: Box<Node>,
    bypass: Option<Expr>,
    mix: Option<Expr>,
    only: Option<Vec<usize>>,
    prev: f32,
    cur: f32,
    started: bool,
    dry: Frame,
    ring: Vec<Vec<f32>>,
    at: usize,
}

struct Branch {
    node: Node,
    gain: Option<Expr>,
    prev: f32,
    cur: f32,
    started: bool,
    ring: Vec<Vec<f32>>,
    at: usize,
    out: Frame,
}

struct ParallelNode {
    branches: Vec<Branch>,
    input: Frame,
    acc: Frame,
}

enum Node {
    Block(BlockNode),
    Chain(Vec<Node>),
    Parallel(ParallelNode),
    Mix(Box<MixNode>),
}

/// Passes `frame` through a delay of `ring[0].len()` samples, channel by channel.
fn delay_frame(frame: &mut Frame, ring: &mut [Vec<f32>], at: usize) {
    let len = ring.first().map_or(0, |r| r.len());
    if len == 0 {
        return;
    }
    let n = frame.len;
    for (buf, r) in frame.ch.iter_mut().zip(ring.iter_mut()) {
        let mut i = at;
        for x in buf[..n].iter_mut() {
            *x = std::mem::replace(&mut r[i], *x);
            i += 1;
            if i == len {
                i = 0;
            }
        }
    }
}

impl Node {
    fn prepare(&mut self, rate: f32, channels: usize) {
        match self {
            Node::Block(b) => b.block.prepare(rate, channels),
            Node::Chain(nodes) => nodes.iter_mut().for_each(|n| n.prepare(rate, channels)),
            Node::Parallel(p) => {
                for b in p.branches.iter_mut() {
                    b.node.prepare(rate, channels);
                }
                let longest = p.branches.iter().map(|b| b.node.latency()).max().unwrap_or(0) as usize;
                for b in p.branches.iter_mut() {
                    let wait = longest - b.node.latency() as usize;
                    b.ring = vec![vec![0.0; wait]; channels];
                    b.at = 0;
                    b.out = Frame::new(channels);
                    b.prev = 1.0;
                    b.cur = 1.0;
                    b.started = false;
                }
                p.input = Frame::new(channels);
                p.acc = Frame::new(channels);
            }
            Node::Mix(m) => {
                m.inner.prepare(rate, channels);
                let l = m.inner.latency() as usize;
                m.ring = vec![vec![0.0; l]; channels];
                m.at = 0;
                m.dry = Frame::new(channels);
                m.prev = 1.0;
                m.cur = 1.0;
                m.started = false;
            }
        }
    }

    fn latency(&self) -> u32 {
        match self {
            Node::Block(b) => b.block.latency(),
            Node::Chain(nodes) => nodes.iter().map(|n| n.latency()).sum(),
            Node::Parallel(p) => p.branches.iter().map(|b| b.node.latency()).max().unwrap_or(0),
            Node::Mix(m) => m.inner.latency(),
        }
    }

    fn tail(&self) -> u32 {
        match self {
            Node::Block(b) => b.block.tail(),
            Node::Chain(nodes) => nodes.iter().map(|n| n.tail()).sum(),
            Node::Parallel(p) => p.branches.iter().map(|b| b.node.tail()).max().unwrap_or(0),
            Node::Mix(m) => m.inner.tail(),
        }
    }

    fn control(&mut self, env: &Env) {
        match self {
            Node::Block(b) => {
                for (i, a) in b.args.iter().enumerate() {
                    b.resolved[i] = a.eval(&b.info.args[i], env);
                }
                b.block.control(&b.resolved);
            }
            Node::Chain(nodes) => nodes.iter_mut().for_each(|n| n.control(env)),
            Node::Parallel(p) => {
                for b in p.branches.iter_mut() {
                    b.node.control(env);
                    let g = match &b.gain {
                        Some(e) => crate::blocks::util::db_to_lin(e.eval(env).clamp(-96.0, 48.0)),
                        None => 1.0,
                    };
                    let g = if g.is_finite() { g } else { 1.0 };
                    if !b.started {
                        b.cur = g;
                        b.started = true;
                    }
                    b.prev = b.cur;
                    b.cur = g;
                }
            }
            Node::Mix(m) => {
                m.inner.control(env);
                let wet = match &m.mix {
                    Some(e) => e.eval(env),
                    None => 1.0,
                };
                let wet = if wet.is_finite() { wet.clamp(0.0, 1.0) } else { 1.0 };
                let on = match &m.bypass {
                    Some(e) => e.eval(env) < 0.5,
                    None => true,
                };
                let target = if on { wet } else { 0.0 };
                if !m.started {
                    m.cur = target;
                    m.started = true;
                }
                m.prev = m.cur;
                m.cur = target;
            }
        }
    }

    fn process(&mut self, io: &mut Frame, offset: usize) {
        match self {
            Node::Block(b) => b.block.process(io, offset),
            Node::Chain(nodes) => nodes.iter_mut().for_each(|n| n.process(io, offset)),
            Node::Parallel(p) => {
                p.input.copy_from(io);
                p.acc.len = io.len;
                p.acc.silence();
                for b in p.branches.iter_mut() {
                    b.out.copy_from(&p.input);
                    b.node.process(&mut b.out, offset);
                    delay_frame(&mut b.out, &mut b.ring, b.at);
                    if let Some(r) = b.ring.first() {
                        if !r.is_empty() {
                            b.at = (b.at + io.len) % r.len();
                        }
                    }
                    for (acc, out) in p.acc.ch.iter_mut().zip(&b.out.ch) {
                        for k in 0..io.len {
                            acc[k] += out[k] * ramp(b.prev, b.cur, offset, k);
                        }
                    }
                }
                io.copy_from(&p.acc);
            }
            Node::Mix(m) => {
                let whole = m.prev == 1.0 && m.cur == 1.0 && m.only.is_none();
                if whole {
                    m.inner.process(io, offset);
                    return;
                }
                m.dry.copy_from(io);
                delay_frame(&mut m.dry, &mut m.ring, m.at);
                if let Some(r) = m.ring.first() {
                    if !r.is_empty() {
                        m.at = (m.at + io.len) % r.len();
                    }
                }
                if m.prev > 0.0 || m.cur > 0.0 {
                    m.inner.process(io, offset);
                }
                for c in 0..io.channels() {
                    let masked = m.only.as_ref().is_some_and(|o| !o.contains(&c));
                    for k in 0..io.len {
                        let w = if masked { 0.0 } else { ramp(m.prev, m.cur, offset, k) };
                        let d = m.dry.ch[c][k];
                        io.ch[c][k] = d + (io.ch[c][k] - d) * w;
                    }
                }
            }
        }
    }

    fn write_meters(&self, params: &ParamSet) {
        match self {
            Node::Block(b) => {
                for (mi, pi) in &b.meters {
                    params.at(*pi).set(b.block.meter(*mi));
                }
            }
            Node::Chain(nodes) => nodes.iter().for_each(|n| n.write_meters(params)),
            Node::Parallel(p) => p.branches.iter().for_each(|b| b.node.write_meters(params)),
            Node::Mix(m) => m.inner.write_meters(params),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Wave {
    Sine,
    Triangle,
    Saw,
    Square,
    Random,
}

const MOD_RATE: ArgInfo = ArgInfo::num("rate", "Hz", 0.001, 200.0, 1.0, "How many cycles each second.");

struct Modulator {
    wave: Wave,
    rate: Arg,
    phase: f32,
    held: f32,
    rng: u32,
}

impl Modulator {
    fn next_random(&mut self) -> f32 {
        // xorshift32, seeded per modulator, so two instances give the same sequence.
        let mut x = self.rng;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.rng = x;
        (x as f32 / u32::MAX as f32) * 2.0 - 1.0
    }

    /// The value for this tick, from -1 to 1, and then the phase moves on by one piece.
    fn tick(&mut self, env: &Env, rate: f32) -> f32 {
        let hz = self.rate.eval(&MOD_RATE, env);
        let p = self.phase;
        let v = match self.wave {
            Wave::Sine => (p * std::f32::consts::TAU).sin(),
            Wave::Triangle => 1.0 - 4.0 * (p - 0.5).abs(),
            Wave::Saw => p * 2.0 - 1.0,
            Wave::Square => {
                if p < 0.5 {
                    1.0
                } else {
                    -1.0
                }
            }
            Wave::Random => self.held,
        };
        let next = p + hz * CHUNK as f32 / rate;
        if next >= 1.0 && self.wave == Wave::Random {
            self.held = self.next_random();
        }
        self.phase = next.fract();
        v
    }
}

pub struct Graph {
    root: Node,
    params: usize,
    mods: Vec<Modulator>,
    vars: Vec<f32>,
    rate: f32,
    bpm: f32,
    channels: usize,
    frame: Frame,
    phase: usize,
}

impl Graph {
    /// `ids` are the plugin's parameter ids in order, which is how an expression's names find their values.
    pub fn from_str(text: &str, ids: &[&str]) -> Result<Graph, String> {
        Graph::from_spec(&spec::from_str(text)?, ids)
    }

    pub fn from_spec(spec: &GraphSpec, ids: &[&str]) -> Result<Graph, String> {
        let mut seen: Vec<&str> = Vec::new();
        for m in &spec.modulators {
            if ids.contains(&m.id.as_str()) {
                return Err(format!("the modulator '{}' has the same name as a parameter", m.id));
            }
            if seen.contains(&m.id.as_str()) {
                return Err(format!("two modulators are called '{}'", m.id));
            }
            if m.id.is_empty() || !m.id.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') || m.id.chars().next().unwrap().is_ascii_digit() {
                return Err(format!("'{}' is not a usable modulator name. Use letters, digits and underscores, starting with a letter", m.id));
            }
            seen.push(&m.id);
        }
        let c = Compiler {
            ids,
            mods: spec.modulators.iter().map(|m| m.id.clone()).collect(),
        };
        let mut mods = Vec::new();
        for (i, m) in spec.modulators.iter().enumerate() {
            let wave = match m.shape.as_str() {
                "sine" => Wave::Sine,
                "triangle" => Wave::Triangle,
                "saw" => Wave::Saw,
                "square" => Wave::Square,
                "random" => Wave::Random,
                other => {
                    return Err(format!("modulator '{}' has the shape '{}'. The shapes are sine, triangle, saw, square and random", m.id, other))
                }
            };
            // A modulator's rate cannot follow another modulator, only parameters.
            let rate = compile_arg(&m.rate, &MOD_RATE, &c, &format!("modulator '{}' rate", m.id))?;
            let mut md = Modulator {
                wave,
                rate,
                phase: 0.0,
                held: 0.0,
                rng: 0x9E37_79B9u32.wrapping_mul(i as u32 + 1) | 1,
            };
            md.held = md.next_random();
            mods.push(md);
        }
        let root = compile_node(&spec.root, &c, "graph")?;
        let vars = vec![0.0; ids.len() + mods.len()];
        Ok(Graph {
            root,
            params: ids.len(),
            mods,
            vars,
            rate: 48000.0,
            bpm: 120.0,
            channels: 1,
            frame: Frame::new(1),
            phase: 0,
        })
    }

    /// Runs off the audio thread. Every block starts from silence.
    pub fn prepare(&mut self, setup: &Setup) {
        self.rate = setup.sample_rate.max(8000.0);
        self.channels = (setup.channels as usize).max(1);
        self.frame = Frame::new(self.channels);
        self.phase = 0;
        for (i, m) in self.mods.iter_mut().enumerate() {
            m.phase = 0.0;
            m.rng = 0x9E37_79B9u32.wrapping_mul(i as u32 + 1) | 1;
            m.held = m.next_random();
        }
        self.root.prepare(self.rate, self.channels);
    }

    pub fn latency_samples(&self) -> u32 {
        self.root.latency()
    }

    pub fn tail_samples(&self) -> u32 {
        self.root.tail()
    }

    pub fn process(&mut self, params: &ParamSet, channels: &mut [&mut [f32]], ctx: &ProcessContext) {
        let n = channels.len().min(self.channels);
        if n == 0 {
            return;
        }
        let frames = channels[0].len();
        self.bpm = ctx.tempo_bpm.map(|b| b as f32).filter(|b| b.is_finite() && *b >= 20.0).unwrap_or(120.0);
        let mut pos = 0;
        while pos < frames {
            let offset = self.phase;
            let len = (CHUNK - offset).min(frames - pos);
            if offset == 0 {
                for i in 0..self.params.min(params.len()) {
                    self.vars[i] = params.at(i).get();
                }
                for j in 0..self.mods.len() {
                    let env = Env {
                        vars: &self.vars,
                        rate: self.rate,
                        bpm: self.bpm,
                    };
                    let v = self.mods[j].tick(&env, self.rate);
                    self.vars[self.params + j] = v;
                }
                let env = Env {
                    vars: &self.vars,
                    rate: self.rate,
                    bpm: self.bpm,
                };
                self.root.control(&env);
            }
            self.frame.len = len;
            for (c, ch) in channels.iter().take(n).enumerate() {
                self.frame.ch[c][..len].copy_from_slice(&ch[pos..pos + len]);
            }
            for c in n..self.channels {
                self.frame.ch[c][..len].fill(0.0);
            }
            self.root.process(&mut self.frame, offset);
            for (c, ch) in channels.iter_mut().take(n).enumerate() {
                for (dst, src) in ch[pos..pos + len].iter_mut().zip(&self.frame.ch[c][..len]) {
                    // A value this small is silence. Writing it as zero keeps a decaying tail from going subnormal.
                    *dst = if src.abs() < 1e-30 { 0.0 } else { *src };
                }
            }
            pos += len;
            self.phase = (offset + len) % CHUNK;
        }
        self.root.write_meters(params);
    }
}

fn compile_node(spec: &NodeSpec, c: &Compiler, path: &str) -> Result<Node, String> {
    let node = match &spec.kind {
        NodeKind::Block { name, args, meters } => {
            let info = blocks::find(name).ok_or_else(|| {
                format!(
                    "{}: there is no block called '{}'. The blocks are: {}",
                    path,
                    name,
                    blocks::catalogue().iter().map(|b| b.name).collect::<Vec<_>>().join(", ")
                )
            })?;
            for (k, _) in args {
                if info.arg_index(k).is_none() {
                    return Err(format!(
                        "{}: the block '{}' has no argument '{}'. Its arguments are: {}",
                        path,
                        name,
                        k,
                        info.args.iter().map(|a| a.name).collect::<Vec<_>>().join(", ")
                    ));
                }
            }
            let mut compiled = Vec::new();
            let mut consts = Vec::new();
            for ai in info.args {
                let given = args.iter().find(|(k, _)| k == ai.name);
                let arg = match given {
                    Some((_, v)) => compile_arg(v, ai, c, &format!("{}: {}.{}", path, name, ai.name))?,
                    None => Arg {
                        expr: None,
                        value: ai.default,
                    },
                };
                consts.push(if arg.expr.is_none() { arg.value } else { ai.default });
                compiled.push(arg);
            }
            let mut block = (info.make)();
            block.configure(&consts);
            let mut bound = Vec::new();
            for (m, id) in meters {
                let mi = info.meter_index(m).ok_or_else(|| {
                    format!(
                        "{}: the block '{}' has no meter '{}'. {}",
                        path,
                        name,
                        m,
                        if info.meters.is_empty() {
                            "It has no meters.".to_string()
                        } else {
                            format!("Its meters are: {}", info.meters.join(", "))
                        }
                    )
                })?;
                let pi = c
                    .ids
                    .iter()
                    .position(|x| x == id)
                    .ok_or_else(|| format!("{}: the meter '{}' is bound to '{}', which is not a parameter", path, m, id))?;
                bound.push((mi, pi));
            }
            Node::Block(BlockNode {
                block,
                info,
                resolved: vec![0.0; compiled.len()],
                args: compiled,
                meters: bound,
            })
        }
        NodeKind::Chain(nodes) => {
            let mut out = Vec::new();
            for (i, n) in nodes.iter().enumerate() {
                out.push(compile_node(n, c, &format!("{}.chain[{}]", path, i))?);
            }
            Node::Chain(out)
        }
        NodeKind::Parallel(branches) => {
            let mut out = Vec::new();
            for (i, b) in branches.iter().enumerate() {
                let here = format!("{}.parallel[{}]", path, i);
                let node = compile_node(&b.node, c, &here)?;
                let gain = match &b.gain_db {
                    None => None,
                    Some(v) => {
                        let info = ArgInfo::num("gain_db", "dB", -96.0, 48.0, 0.0, "");
                        let a = compile_arg(v, &info, c, &format!("{} gain_db", here))?;
                        Some(match a.expr {
                            Some(e) => e,
                            None => Expr::number(a.value),
                        })
                    }
                };
                out.push(Branch {
                    node,
                    gain,
                    prev: 1.0,
                    cur: 1.0,
                    started: false,
                    ring: Vec::new(),
                    at: 0,
                    out: Frame::new(1),
                });
            }
            Node::Parallel(ParallelNode {
                branches: out,
                input: Frame::new(1),
                acc: Frame::new(1),
            })
        }
    };
    if spec.bypass.is_none() && spec.mix.is_none() && spec.only.is_none() {
        return Ok(node);
    }
    let expr = |v: &Option<ArgValue>, what: &str, lo: f32, hi: f32| -> Result<Option<Expr>, String> {
        match v {
            None => Ok(None),
            Some(v) => {
                let info = ArgInfo::num("value", "", lo, hi, lo, "");
                let a = compile_arg(v, &info, c, &format!("{} {}", path, what))?;
                Ok(Some(match a.expr {
                    Some(e) => e,
                    None => Expr::number(a.value),
                }))
            }
        }
    };
    Ok(Node::Mix(Box::new(MixNode {
        inner: Box::new(node),
        bypass: expr(&spec.bypass, "bypass", 0.0, 1.0)?,
        mix: expr(&spec.mix, "mix", 0.0, 1.0)?,
        only: spec.only.clone(),
        prev: 1.0,
        cur: 1.0,
        started: false,
        dry: Frame::new(1),
        ring: Vec::new(),
        at: 0,
    })))
}
