//! The graph runtime. A graph is a plugin's whole signal path, described as data.
//!
//! The audio is cut into pieces of at most 32 samples, aligned to a counter that survives from one host block
//! to the next. At the start of each aligned piece every argument is evaluated from the current parameters,
//! and every block gets one call to `control`. Within a piece, blocks glide from the old control value to the
//! new one by sample position, so the audio is the same whatever block size the host uses.

use crate::block::{ArgInfo, Block, BlockInfo};
use crate::blocks;
use crate::blocks::biquad::{self, Coef, Kind};
use crate::blocks::oscillator::ENDLESS;
use crate::expr::{Env, Expr};
use crate::frame::{ramp, Frame, CHUNK};
use crate::spec::{self, ArgValue, GraphSpec, NodeKind, NodeSpec};
use ruse_core::{ParamSet, ProcessContext, Setup};

/// The most arguments any block declares, with room to spare. Working out a tail evaluates a block's arguments
/// into a buffer this size on the stack.
const MAX_BLOCK_ARGS: usize = 32;

/// How many parameter and modulator values `Graph::tail_for` copies on the stack before it falls back to the heap.
const TAIL_VARS: usize = 256;

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
    /// The graph's taps, in order: a tap's place here is where it keeps what it heard.
    taps: Vec<String>,
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
    /// A multiply node: the branches' outputs are multiplied, not added.
    product: bool,
}

const FB_GAIN: ArgInfo = ArgInfo::num(
    "gain",
    "",
    -0.999,
    0.999,
    0.5,
    "How much of the loop's output returns to its input. Near 1, a short loop rings for seconds, like a plucked string.",
);
const FB_DELAY: ArgInfo = ArgInfo::num("delay_ms", "ms", 0.02, 4000.0, 250.0, "How long the output takes to come back round, one sample at the least.");
const FB_MAX: ArgInfo = ArgInfo::num("max_ms", "ms", 10.0, 4000.0, 2000.0, "The longest loop delay. Sets the memory used.");

/// A node run in a loop. The loop's input is the node's input plus the returns scaled by the gain. The node runs
/// on that, and its output comes back as the returns after the loop delay. The feedback node outputs the returns
/// alone, the way the delay block outputs only its echoes.
///
/// A loop delay of a control step of 32 samples or more has every return computed before it is read, so the node
/// inside runs on whole pieces like any other. A shorter loop, for a comb or a plucked string, runs the node one
/// sample at a time, which costs more but sounds the same as it would in pieces. Either way the audio does not
/// depend on the host's block size.
struct FeedbackNode {
    inner: Box<Node>,
    gain: Arg,
    delay: Arg,
    max_ms: f32,
    rate: f32,
    ring: Vec<Vec<f32>>,
    at: usize,
    /// The gain, and the loop delay in samples, at the last control step and this one.
    g: (f32, f32),
    d: (f32, f32),
    started: bool,
    returns: Frame,
    /// One sample of the loop, and of the sidechain, for a loop shorter than a control step.
    one: Frame,
    one_key: Frame,
}

impl FeedbackNode {
    /// The loop delay in samples: at least one, at most `max_ms`.
    fn loop_samples(&self, ms: f32, rate: f32) -> f32 {
        let longest = (self.max_ms * 0.001 * rate).max(CHUNK as f32);
        (ms * 0.001 * rate).clamp(1.0, longest)
    }
}

const SPLIT_FREQ: ArgInfo = ArgInfo::num("freq", "Hz", 20.0, 20000.0, 1000.0, "A crossover frequency.");

/// Butterworth section Qs. A Linkwitz-Riley LR4 low-pass or high-pass is a second-order Butterworth twice, and an
/// LR8 a fourth-order Butterworth twice. The same Butterworth, made of all-pass sections, is exactly what a
/// crossover's low-pass and high-pass add up to, which the bands below it need to stay in phase with it.
const BW2: [f32; 1] = [std::f32::consts::FRAC_1_SQRT_2];
const BW4: [f32; 2] = [0.541_196_1, 1.306_563];

/// Bands divided by Linkwitz-Riley crossovers, each through its own node, then added back. Each crossover splits
/// what is above the one before it. A lower band also goes through the all-pass of every crossover above it, so
/// the bands add back to the input with a flat level.
struct SplitNode {
    bands: Vec<Branch>,
    freqs: Vec<Arg>,
    qs: &'static [f32],
    rate: f32,
    /// Coefficients for each crossover: its low-pass and high-pass sections, and its all-pass sections.
    lp: Vec<Vec<Coef>>,
    hp: Vec<Vec<Coef>>,
    ap: Vec<Vec<Coef>>,
    /// Filter states by channel, then crossover, then section. The all-passes by channel, band, crossover, section.
    lp_z: Vec<Vec<Vec<[f32; 2]>>>,
    hp_z: Vec<Vec<Vec<[f32; 2]>>>,
    ap_z: Vec<Vec<Vec<Vec<[f32; 2]>>>>,
}

/// Each 2× step of an oversampler: its half-band filter's length and Kaiser window shape. The first step goes from
/// the plugin's rate to twice it, and must keep everything up to 20 kHz at 48 kHz while removing what would fold
/// back from above 28 kHz, so it is the longest. The later steps have more room and are shorter. Every filter
/// has a length of 4m + 1, which makes the delay through each step a whole number of samples at the plugin's rate.
const HALF_BANDS: [(usize, f64); 3] = [(65, 8.0), (25, 9.0), (17, 9.0)];

/// A linear-phase half-band low-pass, used to double the rate on the way up and to halve it on the way down. Every
/// other tap of a half-band filter is zero, and the centre tap is one half, so only the taps at odd distances from
/// the centre are kept.
struct HalfBand {
    /// The filter is 4m + 1 taps long, centred on tap 2m.
    m: usize,
    /// The taps at odd distances from the centre, from the start of the filter: h[1], h[3], up to h[4m - 1].
    odd: Vec<f32>,
    /// By channel: the last 2m - 1 inputs going up, and the last m even and 2m odd samples coming down.
    up: Vec<Vec<f32>>,
    even_down: Vec<Vec<f32>>,
    odd_down: Vec<Vec<f32>>,
}

/// The modified Bessel function of the first kind and order zero, which the Kaiser window is made from.
fn bessel_i0(x: f64) -> f64 {
    let mut sum = 1.0;
    let mut term = 1.0;
    let mut k = 1.0;
    while term > 1e-14 * sum {
        term *= (x / (2.0 * k)).powi(2);
        sum += term;
        k += 1.0;
    }
    sum
}

impl HalfBand {
    fn new(taps: usize, beta: f64) -> HalfBand {
        let m = (taps - 1) / 4;
        let centre = 2 * m;
        let mut odd: Vec<f64> = (0..2 * m)
            .map(|k| {
                let n = 2 * k + 1;
                let x = std::f64::consts::FRAC_PI_2 * (n as f64 - centre as f64);
                let r = 2.0 * n as f64 / (taps - 1) as f64 - 1.0;
                0.5 * x.sin() / x * bessel_i0(beta * (1.0 - r * r).sqrt()) / bessel_i0(beta)
            })
            .collect();
        // The odd taps add to one half, like the centre tap, so both phases pass a steady level unchanged.
        let sum: f64 = odd.iter().sum();
        odd.iter_mut().for_each(|t| *t *= 0.5 / sum);
        HalfBand {
            m,
            odd: odd.into_iter().map(|t| t as f32).collect(),
            up: Vec::new(),
            even_down: Vec::new(),
            odd_down: Vec::new(),
        }
    }

    fn prepare(&mut self, channels: usize) {
        self.up = vec![vec![0.0; 2 * self.m - 1]; channels];
        self.even_down = vec![vec![0.0; self.m]; channels];
        self.odd_down = vec![vec![0.0; 2 * self.m]; channels];
    }

    /// The delay of going up and back down through this step, in samples at the lower rate.
    fn delay(&self) -> usize {
        2 * self.m
    }

    /// Doubles the rate of `x` into `y`, which is twice as long. `work` holds the history and the input together.
    fn up(&mut self, c: usize, x: &[f32], y: &mut [f32], work: &mut [f32]) {
        let m = self.m;
        let h = 2 * m - 1;
        let n = x.len();
        work[..h].copy_from_slice(&self.up[c]);
        work[h..h + n].copy_from_slice(x);
        for j in 0..n {
            let p = h + j;
            // The even outputs meet only the centre tap, so they are the input delayed.
            y[2 * j] = work[p - m];
            let mut acc = 0.0;
            for (k, t) in self.odd.iter().enumerate() {
                acc += t * work[p - k];
            }
            // Twice the gain, because every other sample of the doubled signal starts as zero.
            y[2 * j + 1] = 2.0 * acc;
        }
        self.up[c].copy_from_slice(&work[n..n + h]);
    }

    /// Halves the rate of `v` into `z`, which is half as long, filtering first so nothing folds back.
    fn down(&mut self, c: usize, v: &[f32], z: &mut [f32], even: &mut [f32], odd: &mut [f32]) {
        let m = self.m;
        let n = v.len() / 2;
        even[..m].copy_from_slice(&self.even_down[c]);
        odd[..2 * m].copy_from_slice(&self.odd_down[c]);
        for i in 0..n {
            even[m + i] = v[2 * i];
            odd[2 * m + i] = v[2 * i + 1];
        }
        for (j, out) in z[..n].iter_mut().enumerate() {
            let mut acc = 0.5 * even[j];
            for (k, t) in self.odd.iter().enumerate() {
                acc += t * odd[2 * m + j - 1 - k];
            }
            *out = acc;
        }
        self.even_down[c].copy_from_slice(&even[n..n + m]);
        self.odd_down[c].copy_from_slice(&odd[n..n + 2 * m]);
    }
}

/// A node run at 2, 4 or 8 times the plugin's sample rate. The audio goes up through half-band filters, through the
/// node in pieces of 32 samples at the higher rate, and back down, so a saturator's harmonics above the plugin's
/// Nyquist frequency are removed instead of folding back as aliases.
///
/// Arguments change when the graph's do, once every 32 samples at the plugin's rate. The node inside gets a
/// control step for each of its own 32-sample pieces with those values, so a glide finishes within the first piece
/// and a smoothed argument moves at the speed it was designed for.
struct OversampleNode {
    inner: Box<Node>,
    factor: usize,
    stages: Vec<HalfBand>,
    /// The audio at the higher rate, by channel, and a buffer the steps take turns with.
    high: Vec<Vec<f32>>,
    scratch: Vec<f32>,
    work: Vec<f32>,
    work2: Vec<f32>,
    frame: Frame,
    /// The sidechain at the higher rate, each sample repeated: a detector needs its level, not its top octave.
    key: Frame,
    /// Extra delay at the higher rate that makes the inner node's latency a whole number of samples at the plugin's.
    pad: Vec<Vec<f32>>,
    pad_at: usize,
    latency: u32,
    /// The values the last control step read, kept for the inner node's later pieces.
    vars: Vec<f32>,
    rate: f32,
    bpm: f32,
    /// How many of the inner node's pieces have had a control step during the graph's current piece.
    begun: usize,
}

impl OversampleNode {
    /// The delay of the filters alone, in samples at the plugin's rate.
    fn filter_delay(&self) -> usize {
        self.stages.iter().enumerate().map(|(s, h)| h.delay() >> s).sum()
    }
}

/// A point an envelope can follow. The signal passes through unchanged. A copy goes through the optional node,
/// and what comes out is kept, piece by piece, for the envelopes whose source is this tap.
struct TapNode {
    index: usize,
    inner: Option<Box<Node>>,
    heard: Frame,
}

enum Node {
    Block(BlockNode),
    Chain(Vec<Node>),
    Parallel(ParallelNode),
    Mix(Box<MixNode>),
    Feedback(Box<FeedbackNode>),
    Split(Box<SplitNode>),
    Oversample(Box<OversampleNode>),
    Tap(Box<TapNode>),
}

/// The most sidechain channels the graph reads. A sidechain is usually mono or stereo.
const MAX_KEY: usize = 8;

/// An empty sidechain frame with room for `MAX_KEY` channels, so setting its channel count never allocates.
fn key_frame() -> Frame {
    Frame {
        ch: Vec::with_capacity(MAX_KEY),
        len: 0,
    }
}

/// Lines up branches that have different latency: each waits for the slowest.
fn align(branches: &mut [Branch], channels: usize) {
    let longest = branches.iter().map(|b| b.node.latency()).max().unwrap_or(0) as usize;
    for b in branches.iter_mut() {
        let wait = longest - b.node.latency() as usize;
        b.ring = vec![vec![0.0; wait]; channels];
        b.at = 0;
        b.out = Frame::new(channels);
        b.prev = 1.0;
        b.cur = 1.0;
        b.started = false;
    }
}

/// What a feedback ring held `back` samples before position `now`, between samples by a straight line.
fn read_ring(ring: &[f32], now: usize, back: f64) -> f32 {
    let len = ring.len();
    let pos = (now as f64 - back).rem_euclid(len as f64);
    let i = pos.floor() as usize % len;
    let t = (pos - pos.floor()) as f32;
    let a = ring[i];
    a + (ring[(i + 1) % len] - a) * t
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
            Node::Split(s) => {
                for b in s.bands.iter_mut() {
                    b.node.prepare(rate, channels);
                }
                align(&mut s.bands, channels);
                s.rate = rate;
                let k = s.freqs.len();
                let n = s.qs.len();
                s.lp_z = vec![vec![vec![[0.0; 2]; 2 * n]; k]; channels];
                s.hp_z = vec![vec![vec![[0.0; 2]; 2 * n]; k]; channels];
                s.ap_z = vec![vec![vec![vec![[0.0; 2]; n]; k]; k]; channels];
            }
            Node::Oversample(o) => {
                let f = o.factor;
                o.inner.prepare(rate * f as f32, channels);
                o.rate = rate * f as f32;
                for s in o.stages.iter_mut() {
                    s.prepare(channels);
                }
                let inner = o.inner.latency() as usize;
                let pad = (f - inner % f) % f;
                o.pad = vec![vec![0.0; pad]; channels];
                o.pad_at = 0;
                o.latency = (o.filter_delay() + (inner + pad) / f) as u32;
                o.high = vec![vec![0.0; CHUNK * f]; channels];
                o.scratch = vec![0.0; CHUNK * f];
                o.work = vec![0.0; CHUNK * f + 2 * HALF_BANDS[0].0];
                o.work2 = vec![0.0; CHUNK * f + 2 * HALF_BANDS[0].0];
                o.frame = Frame::new(channels);
                o.key = key_frame();
                o.begun = 0;
            }
            Node::Feedback(f) => {
                f.inner.prepare(rate, channels);
                let len = (f.max_ms * 0.001 * rate) as usize + 2 * CHUNK + 4;
                f.ring = vec![vec![0.0; len]; channels];
                f.at = 0;
                f.rate = rate;
                f.started = false;
                f.returns = Frame::new(channels);
                f.one = Frame::new(channels);
                f.one_key = key_frame();
            }
            Node::Tap(t) => {
                if let Some(n) = t.inner.as_mut() {
                    n.prepare(rate, channels);
                }
                t.heard = Frame::new(channels);
            }
        }
    }

    fn latency(&self) -> u32 {
        match self {
            Node::Block(b) => b.block.latency(),
            Node::Chain(nodes) => nodes.iter().map(|n| n.latency()).sum(),
            Node::Parallel(p) => p.branches.iter().map(|b| b.node.latency()).max().unwrap_or(0),
            Node::Mix(m) => m.inner.latency(),
            Node::Feedback(f) => f.inner.latency(),
            Node::Split(s) => s.bands.iter().map(|b| b.node.latency()).max().unwrap_or(0),
            Node::Oversample(o) => o.latency,
            // The signal passes a tap untouched.
            Node::Tap(_) => 0,
        }
    }

    /// The tail with every argument evaluated from `env`. It reads nothing `control` set, so it is right before
    /// any audio has run, and it does not allocate.
    fn tail(&self, env: &Env) -> u32 {
        match self {
            Node::Block(b) => {
                let mut args = [0.0f32; MAX_BLOCK_ARGS];
                let n = b.args.len().min(MAX_BLOCK_ARGS);
                for (i, slot) in args[..n].iter_mut().enumerate() {
                    *slot = b.args[i].eval(&b.info.args[i], env);
                }
                b.block.tail(&args[..n], env.rate)
            }
            // A generator's tail never ends, so tails add without overflowing.
            Node::Chain(nodes) => nodes.iter().fold(0u32, |t, n| t.saturating_add(n.tail(env))),
            // A product is silent once any branch is, so it lasts as long as its shortest branch.
            Node::Parallel(p) if p.product => p.branches.iter().map(|b| b.node.tail(env)).min().unwrap_or(0),
            Node::Parallel(p) => p.branches.iter().map(|b| b.node.tail(env)).max().unwrap_or(0),
            Node::Mix(m) => m.inner.tail(env),
            Node::Split(s) => s.bands.iter().map(|b| b.node.tail(env)).max().unwrap_or(0),
            Node::Oversample(o) => {
                let high = Env {
                    vars: env.vars,
                    rate: env.rate * o.factor as f32,
                    bpm: env.bpm,
                };
                let t = o.inner.tail(&high);
                if t >= ENDLESS {
                    ENDLESS
                } else {
                    // The filters ring for about as long as they delay.
                    (t as usize).div_ceil(o.factor).saturating_add(o.filter_delay()).min(ENDLESS as usize) as u32
                }
            }
            Node::Feedback(f) => {
                // The returns fall by the gain each time round, so count the trips until they are 60 dB down.
                let g = f.gain.eval(&FB_GAIN, env).abs();
                let trips = if g <= 0.001 {
                    1.0
                } else {
                    ((0.001f32).ln() / g.ln()).ceil().clamp(1.0, 200.0) + 1.0
                };
                let each = f.loop_samples(f.delay.eval(&FB_DELAY, env), env.rate) + f.inner.tail(env) as f32;
                (each * trips).min(u32::MAX as f32) as u32
            }
            // What a tap's node makes goes only to envelopes, never to the output.
            Node::Tap(_) => 0,
        }
    }

    fn control(&mut self, env: &Env) {
        match self {
            Node::Tap(t) => {
                if let Some(n) = t.inner.as_mut() {
                    n.control(env);
                }
            }
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
            Node::Split(s) => {
                let top = s.rate * 0.45;
                let mut below = 0.0f32;
                for i in 0..s.freqs.len() {
                    // Each crossover sits at least a little above the one before it, whatever the knobs say.
                    let f = s.freqs[i].eval(&SPLIT_FREQ, env).max(below * 1.05).min(top);
                    below = f;
                    // Written in place: control runs on the audio thread.
                    let n = s.qs.len();
                    for (sec, q) in s.qs.iter().enumerate() {
                        let lp = biquad::design(Kind::Lowpass, f, *q, 0.0, s.rate);
                        let hp = biquad::design(Kind::Highpass, f, *q, 0.0, s.rate);
                        s.lp[i][sec] = lp;
                        s.lp[i][sec + n] = lp;
                        s.hp[i][sec] = hp;
                        s.hp[i][sec + n] = hp;
                        s.ap[i][sec] = biquad::design(Kind::Allpass, f, *q, 0.0, s.rate);
                    }
                }
                for b in s.bands.iter_mut() {
                    b.node.control(env);
                }
            }
            Node::Oversample(o) => {
                let n = o.vars.len().min(env.vars.len());
                o.vars[..n].copy_from_slice(&env.vars[..n]);
                o.bpm = env.bpm;
                let OversampleNode { inner, vars, rate, .. } = &mut **o;
                inner.control(&Env {
                    vars,
                    rate: *rate,
                    bpm: env.bpm,
                });
                o.begun = 1;
            }
            Node::Feedback(f) => {
                f.inner.control(env);
                let g = f.gain.eval(&FB_GAIN, env);
                let d = f.loop_samples(f.delay.eval(&FB_DELAY, env), f.rate);
                if !f.started {
                    f.g = (g, g);
                    f.d = (d, d);
                    f.started = true;
                }
                f.g = (f.g.1, g);
                f.d = (f.d.1, d);
            }
        }
    }

    /// Processes `io` in place. `key` is the sidechain for the same samples, and `taps` holds what each tap heard,
    /// for the envelopes that follow them.
    fn process(&mut self, io: &mut Frame, key: &Frame, taps: &mut [Frame], offset: usize) {
        match self {
            Node::Block(b) => {
                b.block.key(key);
                b.block.process(io, offset)
            }
            Node::Chain(nodes) => nodes.iter_mut().for_each(|n| n.process(io, key, taps, offset)),
            Node::Tap(t) => {
                let TapNode { index, inner, heard } = &mut **t;
                match inner {
                    Some(node) => {
                        heard.copy_from(io);
                        node.process(heard, key, taps, offset);
                        taps[*index].copy_from(heard);
                    }
                    None => taps[*index].copy_from(io),
                }
            }
            Node::Parallel(p) => {
                p.input.copy_from(io);
                p.acc.len = io.len;
                p.acc.silence();
                if p.product {
                    for acc in p.acc.ch.iter_mut() {
                        acc[..io.len].fill(1.0);
                    }
                }
                for b in p.branches.iter_mut() {
                    b.out.copy_from(&p.input);
                    b.node.process(&mut b.out, key, taps, offset);
                    delay_frame(&mut b.out, &mut b.ring, b.at);
                    if let Some(r) = b.ring.first() {
                        if !r.is_empty() {
                            b.at = (b.at + io.len) % r.len();
                        }
                    }
                    for (acc, out) in p.acc.ch.iter_mut().zip(&b.out.ch) {
                        for k in 0..io.len {
                            if p.product {
                                acc[k] *= out[k];
                            } else {
                                acc[k] += out[k] * ramp(b.prev, b.cur, offset, k);
                            }
                        }
                    }
                }
                io.copy_from(&p.acc);
            }
            Node::Mix(m) => {
                let whole = m.prev == 1.0 && m.cur == 1.0 && m.only.is_none();
                if whole {
                    m.inner.process(io, key, taps, offset);
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
                    m.inner.process(io, key, taps, offset);
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
            Node::Split(s) => {
                let SplitNode { bands, lp, hp, ap, lp_z, hp_z, ap_z, .. } = &mut **s;
                let k = lp.len();
                let n = io.len;
                let chans = io.channels().min(lp_z.len());
                for b in bands.iter_mut() {
                    b.out.len = n;
                    b.out.silence();
                }
                for c in 0..chans {
                    for t in 0..n {
                        let mut rest = io.ch[c][t];
                        for i in 0..k {
                            let mut lo = rest;
                            for (coef, z) in lp[i].iter().zip(lp_z[c][i].iter_mut()) {
                                lo = biquad::run(coef, z, lo);
                            }
                            let mut hi = rest;
                            for (coef, z) in hp[i].iter().zip(hp_z[c][i].iter_mut()) {
                                hi = biquad::run(coef, z, hi);
                            }
                            for j in i + 1..k {
                                for (coef, z) in ap[j].iter().zip(ap_z[c][i][j].iter_mut()) {
                                    lo = biquad::run(coef, z, lo);
                                }
                            }
                            bands[i].out.ch[c][t] = lo;
                            rest = hi;
                        }
                        bands[k].out.ch[c][t] = rest;
                    }
                }
                // Filter states that have decayed to nothing are written as zero, so they never go subnormal.
                let flush = |z: &mut [f32; 2]| {
                    for v in z.iter_mut() {
                        if v.abs() < 1e-20 {
                            *v = 0.0;
                        }
                    }
                };
                lp_z.iter_mut().chain(hp_z.iter_mut()).flatten().flatten().for_each(flush);
                ap_z.iter_mut().flatten().flatten().flatten().for_each(flush);
                for ch in io.ch.iter_mut() {
                    ch[..n].fill(0.0);
                }
                for b in bands.iter_mut() {
                    b.node.process(&mut b.out, key, taps, offset);
                    delay_frame(&mut b.out, &mut b.ring, b.at);
                    if let Some(r) = b.ring.first() {
                        if !r.is_empty() {
                            b.at = (b.at + n) % r.len();
                        }
                    }
                    for (dst, src) in io.ch.iter_mut().zip(&b.out.ch) {
                        for t in 0..n {
                            dst[t] += src[t];
                        }
                    }
                }
            }
            Node::Oversample(o) => {
                let OversampleNode {
                    inner,
                    factor,
                    stages,
                    high,
                    scratch,
                    work,
                    work2,
                    frame,
                    key: high_key,
                    pad,
                    pad_at,
                    vars,
                    rate,
                    bpm,
                    begun,
                    ..
                } = &mut **o;
                let f = *factor;
                let n = io.len;
                let big = n * f;
                let chans = io.channels().min(high.len());
                let steps = stages.len();
                // Up, one doubling at a time. The steps take turns between the scratch buffer and this channel's
                // own, in the order that leaves the last in its own.
                for (c, (hc, input)) in high.iter_mut().zip(&io.ch).enumerate().take(chans) {
                    let mut len = n;
                    for (s, stage) in stages.iter_mut().enumerate() {
                        let into_high = (steps - 1 - s) % 2 == 0;
                        match (s == 0, into_high) {
                            (true, true) => stage.up(c, &input[..len], &mut hc[..2 * len], work),
                            (true, false) => stage.up(c, &input[..len], &mut scratch[..2 * len], work),
                            (false, true) => stage.up(c, &scratch[..len], &mut hc[..2 * len], work),
                            (false, false) => stage.up(c, &hc[..len], &mut scratch[..2 * len], work),
                        }
                        len *= 2;
                    }
                }
                // The node inside, in pieces aligned to its own 32-sample grid.
                let start = offset * f;
                let mut p = 0;
                while p < big {
                    let at = start + p;
                    while *begun <= at / CHUNK {
                        if *begun > 0 {
                            inner.control(&Env {
                                vars,
                                rate: *rate,
                                bpm: *bpm,
                            });
                        }
                        *begun += 1;
                    }
                    let len = (CHUNK - at % CHUNK).min(big - p);
                    frame.len = len;
                    for (dst, src) in frame.ch.iter_mut().zip(high.iter()).take(chans) {
                        dst[..len].copy_from_slice(&src[p..p + len]);
                    }
                    // Within capacity, so this never allocates.
                    high_key.ch.resize(key.channels(), [0.0; CHUNK]);
                    high_key.len = len;
                    for (dst, src) in high_key.ch.iter_mut().zip(key.ch.iter()) {
                        for (j, d) in dst[..len].iter_mut().enumerate() {
                            *d = src[(p + j) / f];
                        }
                    }
                    // No tap sits inside an oversample node, which compiling refuses.
                    inner.process(frame, high_key, &mut [], at % CHUNK);
                    delay_frame(frame, pad, *pad_at);
                    if let Some(r) = pad.first() {
                        if !r.is_empty() {
                            *pad_at = (*pad_at + len) % r.len();
                        }
                    }
                    for (dst, src) in high.iter_mut().zip(frame.ch.iter()).take(chans) {
                        dst[p..p + len].copy_from_slice(&src[..len]);
                    }
                    p += len;
                }
                // And down again, ending in io.
                for (c, (hc, output)) in high.iter_mut().zip(io.ch.iter_mut()).enumerate().take(chans) {
                    let mut len = big;
                    for (k, (s, stage)) in stages.iter_mut().enumerate().rev().enumerate() {
                        let half = len / 2;
                        let from_high = k % 2 == 0;
                        match (s == 0, from_high) {
                            (true, true) => stage.down(c, &hc[..len], &mut output[..half], work, work2),
                            (true, false) => stage.down(c, &scratch[..len], &mut output[..half], work, work2),
                            (false, true) => stage.down(c, &hc[..len], &mut scratch[..half], work, work2),
                            (false, false) => stage.down(c, &scratch[..len], &mut hc[..half], work, work2),
                        }
                        len = half;
                    }
                }
            }
            Node::Feedback(f) => {
                let len = f.ring.first().map_or(0, |r| r.len());
                if len == 0 {
                    return;
                }
                let channels = io.channels().min(f.ring.len());
                let n = io.len;
                // Returns that have died away are written as zero, so the loop never goes subnormal.
                let keep = |w: f32| if w.abs() < 1e-30 { 0.0 } else { w };
                let FeedbackNode {
                    inner,
                    ring,
                    at,
                    g,
                    d,
                    returns,
                    one,
                    one_key,
                    ..
                } = &mut **f;
                returns.len = n;
                if d.0.min(d.1) >= CHUNK as f32 {
                    // Every return this piece reads was written in a piece already done.
                    for ((r, back), x) in ring.iter().zip(returns.ch.iter_mut()).zip(io.ch.iter_mut()).take(channels) {
                        for k in 0..n {
                            back[k] = read_ring(r, *at + k, ramp(d.0, d.1, offset, k) as f64);
                            x[k] += ramp(g.0, g.1, offset, k) * back[k];
                        }
                    }
                    inner.process(io, key, taps, offset);
                    for (r, x) in ring.iter_mut().zip(io.ch.iter()).take(channels) {
                        for (k, v) in x[..n].iter().enumerate() {
                            r[(*at + k) % len] = keep(*v);
                        }
                    }
                } else {
                    // A loop shorter than a piece: each sample goes round before the next is read.
                    one_key.ch.resize(key.channels().min(MAX_KEY), [0.0; CHUNK]);
                    one.len = 1;
                    one_key.len = 1;
                    for k in 0..n {
                        let back = ramp(d.0, d.1, offset, k) as f64;
                        let gain = ramp(g.0, g.1, offset, k);
                        for (((r, ret), x), o) in ring.iter().zip(returns.ch.iter_mut()).zip(io.ch.iter()).zip(one.ch.iter_mut()).take(channels) {
                            ret[k] = read_ring(r, *at + k, back);
                            o[0] = x[k] + gain * ret[k];
                        }
                        for (dst, src) in one_key.ch.iter_mut().zip(key.ch.iter()) {
                            dst[0] = src[k];
                        }
                        inner.process(one, one_key, taps, offset + k);
                        for (r, o) in ring.iter_mut().zip(one.ch.iter()).take(channels) {
                            r[(*at + k) % len] = keep(o[0]);
                        }
                    }
                }
                for (x, ret) in io.ch.iter_mut().zip(returns.ch.iter()).take(channels) {
                    x[..n].copy_from_slice(&ret[..n]);
                }
                *at = (*at + n) % len;
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
            Node::Feedback(f) => f.inner.write_meters(params),
            Node::Split(s) => s.bands.iter().for_each(|b| b.node.write_meters(params)),
            Node::Oversample(o) => o.inner.write_meters(params),
            Node::Tap(t) => {
                if let Some(n) = t.inner.as_ref() {
                    n.write_meters(params);
                }
            }
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
    /// Follows the level of the graph's input rather than running at a rate.
    Envelope,
}

const MOD_RATE: ArgInfo = ArgInfo::num("rate", "Hz", 0.001, 200.0, 1.0, "How many cycles each second.");
const ENV_ATTACK: ArgInfo = ArgInfo::num("attack_ms", "ms", 0.1, 500.0, 10.0, "How fast the envelope rises.");
const ENV_RELEASE: ArgInfo = ArgInfo::num("release_ms", "ms", 1.0, 5000.0, 150.0, "How fast the envelope falls.");

/// What an envelope follows: the plugin's input, its sidechain, or a tap inside the graph, by its place.
#[derive(Clone, Copy, PartialEq)]
enum Source {
    Input,
    Sidechain,
    Tap(usize),
}

struct Modulator {
    wave: Wave,
    rate: Arg,
    phase: f32,
    held: f32,
    rng: u32,
    /// An envelope's attack and release, and whether it follows the mean square rather than the peak.
    attack: Arg,
    release: Arg,
    rms: bool,
    /// What an envelope follows.
    source: Source,
    /// The followed level, and the smoothing coefficients for the current piece.
    level: f32,
    rise: f32,
    fall: f32,
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

    /// The value for this tick, from -1 to 1, and then the phase moves on by one piece. An envelope gives its
    /// level, from 0 at silence to 1 at full scale, and sets its smoothing for the piece that follows.
    fn tick(&mut self, env: &Env, rate: f32) -> f32 {
        if self.wave == Wave::Envelope {
            let coef = |ms: f32| (-1.0 / (ms * 0.001 * rate)).exp();
            self.rise = coef(self.attack.eval(&ENV_ATTACK, env));
            self.fall = coef(self.release.eval(&ENV_RELEASE, env));
            return if self.rms { self.level.sqrt() } else { self.level };
        }
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
            // An envelope returned above.
            Wave::Random | Wave::Envelope => self.held,
        };
        let next = p + hz * CHUNK as f32 / rate;
        if next >= 1.0 && self.wave == Wave::Random {
            self.held = self.next_random();
        }
        self.phase = next.fract();
        v
    }

    /// An envelope takes in the input, sample by sample, as it reaches the graph. The loudest channel counts. The
    /// value read at the next tick includes every sample before it, so it does not depend on the host's block size.
    fn follow(&mut self, input: &Frame) {
        if self.wave != Wave::Envelope {
            return;
        }
        for k in 0..input.len {
            let peak = input.ch.iter().fold(0.0f32, |m, c| m.max(c[k].abs()));
            let x = if self.rms { peak * peak } else { peak };
            let a = if x > self.level { self.rise } else { self.fall };
            self.level = x + a * (self.level - x);
        }
        if self.level < 1e-20 {
            self.level = 0.0;
        }
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
    /// The sidechain for the current piece, and whether anything reads it.
    key: Frame,
    sidechain: bool,
    /// What each tap heard in the current piece. A tap the audio did not reach, such as one in a bypassed
    /// node, hears silence.
    taps: Vec<Frame>,
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
        let mut taps = Vec::new();
        spec.root.tap_names(&mut taps);
        for (i, t) in taps.iter().enumerate() {
            if taps[..i].contains(t) {
                return Err(format!("two taps are called '{}'. Give each its own name", t));
            }
        }
        let c = Compiler {
            ids,
            mods: spec.modulators.iter().map(|m| m.id.clone()).collect(),
            taps,
        };
        let mut mods = Vec::new();
        for (i, m) in spec.modulators.iter().enumerate() {
            let wave = match m.shape.as_str() {
                "sine" => Wave::Sine,
                "triangle" => Wave::Triangle,
                "saw" => Wave::Saw,
                "square" => Wave::Square,
                "random" => Wave::Random,
                "envelope" => Wave::Envelope,
                other => {
                    return Err(format!(
                        "modulator '{}' has the shape '{}'. The shapes are sine, triangle, saw, square, random and envelope",
                        m.id, other
                    ))
                }
            };
            let envelope = wave == Wave::Envelope;
            if envelope && m.rate.is_some() {
                return Err(format!(
                    "modulator '{}' is an envelope, which follows the input instead of running at a rate. Give it attack_ms and release_ms",
                    m.id
                ));
            }
            if !envelope && (m.attack_ms.is_some() || m.release_ms.is_some() || m.detector.is_some() || m.source.is_some()) {
                return Err(format!(
                    "modulator '{}' is an oscillator. attack_ms, release_ms, detector and source belong to an envelope, with \"shape\": \"envelope\"",
                    m.id
                ));
            }
            let source = match m.source.as_deref() {
                None | Some("input") => Source::Input,
                Some("sidechain") => Source::Sidechain,
                Some(name) => match c.taps.iter().position(|t| t == name) {
                    Some(i) => Source::Tap(i),
                    None => {
                        return Err(format!(
                            "modulator '{}' has the source '{}'. An envelope follows the input, the sidechain or a tap in the graph{}",
                            m.id,
                            name,
                            if c.taps.is_empty() {
                                ", and this graph has no taps".to_string()
                            } else {
                                format!(". The taps are: {}", c.taps.join(", "))
                            }
                        ))
                    }
                },
            };
            let rms = match m.detector.as_deref() {
                None | Some("peak") => false,
                Some("rms") => true,
                Some(other) => return Err(format!("modulator '{}' has the detector '{}'. Use peak or rms", m.id, other)),
            };
            // A modulator's settings cannot follow another modulator, only parameters.
            let here = |what: &str| format!("modulator '{}' {}", m.id, what);
            let arg = |v: &Option<ArgValue>, info: &ArgInfo, what: &str| match v {
                Some(v) => compile_arg(v, info, &c, &here(what)),
                None => Ok(Arg {
                    expr: None,
                    value: info.default,
                }),
            };
            let mut md = Modulator {
                wave,
                rate: arg(&m.rate, &MOD_RATE, "rate")?,
                phase: 0.0,
                held: 0.0,
                rng: 0x9E37_79B9u32.wrapping_mul(i as u32 + 1) | 1,
                attack: arg(&m.attack_ms, &ENV_ATTACK, "attack_ms")?,
                release: arg(&m.release_ms, &ENV_RELEASE, "release_ms")?,
                rms,
                source,
                level: 0.0,
                rise: 0.0,
                fall: 0.0,
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
            key: key_frame(),
            taps: vec![Frame::new(1); c.taps.len()],
            sidechain: spec.uses_sidechain(),
            phase: 0,
        })
    }

    /// Whether the graph listens to a sidechain, so the plugin needs a sidechain input.
    pub fn uses_sidechain(&self) -> bool {
        self.sidechain
    }

    /// Runs off the audio thread. Every block starts from silence.
    pub fn prepare(&mut self, setup: &Setup) {
        self.rate = setup.sample_rate.max(8000.0);
        self.channels = (setup.channels as usize).max(1);
        self.frame = Frame::new(self.channels);
        for t in self.taps.iter_mut() {
            *t = Frame::new(self.channels);
        }
        self.phase = 0;
        for (i, m) in self.mods.iter_mut().enumerate() {
            m.phase = 0.0;
            m.rng = 0x9E37_79B9u32.wrapping_mul(i as u32 + 1) | 1;
            m.held = m.next_random();
            m.level = 0.0;
        }
        self.root.prepare(self.rate, self.channels);
    }

    pub fn latency_samples(&self) -> u32 {
        self.root.latency()
    }

    /// The tail at the parameter values the graph last ran with. Before any audio has run those are all zero, so
    /// a host should be given [`Graph::tail_for`] instead. This stays for plugins generated before it existed.
    pub fn tail_samples(&self) -> u32 {
        self.root.tail(&Env {
            vars: &self.vars,
            rate: self.rate,
            bpm: self.bpm,
        })
    }

    /// The tail at the current values in `params`, which is what a host needs. It works before any audio has run
    /// and follows the knobs as they move. Modulators count at the value they last had.
    pub fn tail_for(&self, params: &ParamSet) -> u32 {
        let n = self.vars.len();
        let mut stack = [0.0f32; TAIL_VARS];
        let mut heap: Vec<f32>;
        let vars: &mut [f32] = if n <= TAIL_VARS {
            &mut stack[..n]
        } else {
            heap = vec![0.0; n];
            &mut heap
        };
        vars.copy_from_slice(&self.vars);
        for (i, v) in vars.iter_mut().enumerate().take(self.params.min(params.len())) {
            *v = params.at(i).get();
        }
        self.root.tail(&Env {
            vars,
            rate: self.rate,
            bpm: self.bpm,
        })
    }

    pub fn process(&mut self, params: &ParamSet, channels: &mut [&mut [f32]], ctx: &ProcessContext) {
        self.process_with_sidechain(params, channels, &[], ctx);
    }

    /// Like `process`, with the sidechain for the same samples, one slice per channel. A sidechain slice
    /// shorter than the main ones counts as silence where it runs out.
    pub fn process_with_sidechain(&mut self, params: &ParamSet, channels: &mut [&mut [f32]], sidechain: &[&[f32]], ctx: &ProcessContext) {
        let n = channels.len().min(self.channels);
        if n == 0 {
            return;
        }
        // Within the frame's capacity, so this never allocates.
        self.key.ch.resize(sidechain.len().min(MAX_KEY), [0.0; CHUNK]);
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
            self.key.len = len;
            for (dst, src) in self.key.ch.iter_mut().zip(sidechain) {
                for (i, d) in dst[..len].iter_mut().enumerate() {
                    *d = src.get(pos + i).copied().unwrap_or(0.0);
                }
            }
            for m in self.mods.iter_mut() {
                match m.source {
                    Source::Input => m.follow(&self.frame),
                    Source::Sidechain => m.follow(&self.key),
                    Source::Tap(_) => {}
                }
            }
            for t in self.taps.iter_mut() {
                t.len = len;
                t.silence();
            }
            self.root.process(&mut self.frame, &self.key, &mut self.taps, offset);
            // A tap hears its part of the graph as the audio passes, so its envelopes follow afterwards. They are
            // read at the next control step, as the others are.
            for m in self.mods.iter_mut() {
                if let Source::Tap(i) = m.source {
                    m.follow(&self.taps[i]);
                }
            }
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
                product: false,
            })
        }
        NodeKind::Multiply(nodes) => {
            let mut out = Vec::new();
            for (i, n) in nodes.iter().enumerate() {
                out.push(Branch {
                    node: compile_node(n, c, &format!("{}.multiply[{}]", path, i))?,
                    gain: None,
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
                product: true,
            })
        }
        NodeKind::Split(ss) => {
            let here = format!("{}.split", path);
            let mut freqs = Vec::new();
            for (i, v) in ss.freqs.iter().enumerate() {
                freqs.push(compile_arg(v, &SPLIT_FREQ, c, &format!("{} freqs[{}]", here, i))?);
            }
            if freqs.iter().all(|a| a.expr.is_none()) && freqs.windows(2).any(|w| w[1].value <= w[0].value) {
                return Err(format!("{} freqs must rise from the lowest crossover to the highest", here));
            }
            let mut bands = Vec::new();
            for (i, b) in ss.bands.iter().enumerate() {
                bands.push(Branch {
                    node: compile_node(b, c, &format!("{}.bands[{}]", here, i))?,
                    gain: None,
                    prev: 1.0,
                    cur: 1.0,
                    started: false,
                    ring: Vec::new(),
                    at: 0,
                    out: Frame::new(1),
                });
            }
            let qs: &'static [f32] = if ss.slope == 48 { &BW4 } else { &BW2 };
            let k = freqs.len();
            Node::Split(Box::new(SplitNode {
                bands,
                freqs,
                qs,
                rate: 48000.0,
                lp: vec![vec![Coef::IDENTITY; 2 * qs.len()]; k],
                hp: vec![vec![Coef::IDENTITY; 2 * qs.len()]; k],
                ap: vec![vec![Coef::IDENTITY; qs.len()]; k],
                lp_z: Vec::new(),
                hp_z: Vec::new(),
                ap_z: Vec::new(),
            }))
        }
        NodeKind::Tap(ts) => {
            let here = format!("{}.tap", path);
            let index = c.taps.iter().position(|t| *t == ts.name).ok_or_else(|| format!("{}: unknown tap '{}'", here, ts.name))?;
            let inner = match &ts.node {
                Some(n) => Some(Box::new(compile_node(n, c, &format!("{}.node", here))?)),
                None => None,
            };
            Node::Tap(Box::new(TapNode {
                index,
                inner,
                heard: Frame::new(1),
            }))
        }
        NodeKind::Oversample(os) => {
            let mut inside = Vec::new();
            os.node.tap_names(&mut inside);
            if let Some(t) = inside.first() {
                return Err(format!(
                    "{}.oversample: the tap '{}' is inside it. Put the tap before or after the oversample node",
                    path, t
                ));
            }
            let inner = compile_node(&os.node, c, &format!("{}.oversample.node", path))?;
            let factor = os.factor as usize;
            let steps = factor.trailing_zeros() as usize;
            Node::Oversample(Box::new(OversampleNode {
                inner: Box::new(inner),
                factor,
                stages: HALF_BANDS[..steps].iter().map(|(taps, beta)| HalfBand::new(*taps, *beta)).collect(),
                high: Vec::new(),
                scratch: Vec::new(),
                work: Vec::new(),
                work2: Vec::new(),
                frame: Frame::new(1),
                key: key_frame(),
                pad: Vec::new(),
                pad_at: 0,
                latency: 0,
                vars: vec![0.0; c.ids.len() + c.mods.len()],
                rate: 48000.0 * factor as f32,
                bpm: 120.0,
                begun: 0,
            }))
        }
        NodeKind::Feedback(fs) => {
            let here = format!("{}.feedback", path);
            let mut inside = Vec::new();
            fs.node.tap_names(&mut inside);
            if let Some(t) = inside.first() {
                return Err(format!(
                    "{}: the tap '{}' is inside the loop. Put the tap before or after the feedback node",
                    here, t
                ));
            }
            let inner = compile_node(&fs.node, c, &format!("{}.node", here))?;
            let arg = |v: &Option<ArgValue>, info: &ArgInfo| match v {
                Some(v) => compile_arg(v, info, c, &format!("{} {}", here, info.name)),
                None => Ok(Arg {
                    expr: None,
                    value: info.default,
                }),
            };
            let gain = arg(&fs.gain, &FB_GAIN)?;
            let delay = arg(&fs.delay_ms, &FB_DELAY)?;
            let max_ms = fs.max_ms.unwrap_or(FB_MAX.default as f64) as f32;
            if !(FB_MAX.min..=FB_MAX.max).contains(&max_ms) {
                return Err(format!("{} max_ms is {}, outside the range 10 to 4000 ms", here, max_ms));
            }
            if delay.expr.is_none() && delay.value > max_ms {
                return Err(format!(
                    "{} delay_ms is {} but max_ms is {}. Raise max_ms to at least the longest delay",
                    here, delay.value, max_ms
                ));
            }
            Node::Feedback(Box::new(FeedbackNode {
                inner: Box::new(inner),
                gain,
                delay,
                max_ms,
                rate: 48000.0,
                ring: Vec::new(),
                at: 0,
                g: (0.0, 0.0),
                d: (CHUNK as f32, CHUNK as f32),
                started: false,
                returns: Frame::new(1),
                one: Frame::new(1),
                one_key: key_frame(),
            }))
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_block_fits_the_tail_argument_buffer() {
        for info in blocks::catalogue() {
            assert!(
                info.args.len() <= MAX_BLOCK_ARGS,
                "{} has {} arguments, more than MAX_BLOCK_ARGS",
                info.name,
                info.args.len()
            );
        }
    }
}
