use crate::config::{ConfigSet, TapeConfig};
use crate::filters::half_band_taps;
use crate::hysteresis::{langevin, Hysteresis};
use crate::tables::*;
use crate::transport;
use ruse_core::Handoff;
use std::sync::Arc;

const DELAY_SIZE: usize = 8192;

/// What is coming off every track of the machine, kept by the transport's sample
/// count so a track can hear the ones beside it. Each track reads its neighbours a
/// block late, which makes the result the same whatever order the tracks are run in.
struct Headstack {
    tracks: usize,
    lines: Vec<f32>,
    /// The sample each track has been written up to.
    written: Vec<i64>,
}

impl Headstack {
    const SIZE: usize = 2 * MAX_FRAMES;

    fn new(tracks: usize) -> Self {
        Headstack {
            tracks,
            lines: vec![0.0; tracks * Self::SIZE],
            written: vec![i64::MIN; tracks],
        }
    }

    fn write(&mut self, track: usize, t0: i64, x: &[f32]) {
        let line = &mut self.lines[track * Self::SIZE..(track + 1) * Self::SIZE];
        let mask = (Self::SIZE - 1) as i64;
        for (i, v) in x.iter().enumerate() {
            line[((t0 + i as i64) & mask) as usize] = *v;
        }
        self.written[track] = t0 + x.len() as i64;
    }

    /// Add the block before `t0` of `track` into `sum`; false if the track isn't running.
    fn read(&self, track: i64, t0: i64, sum: &mut [f32]) -> bool {
        if track < 0 || track >= self.tracks as i64 {
            return false;
        }
        let frames = sum.len() as i64;
        let w = self.written[track as usize];
        if w < t0 || w > t0 + frames {
            return false;
        }
        let line = &self.lines[track as usize * Self::SIZE..(track as usize + 1) * Self::SIZE];
        let mask = (Self::SIZE - 1) as i64;
        let from = t0 - frames;
        for (i, s) in sum.iter_mut().enumerate() {
            *s += line[((from + i as i64) & mask) as usize];
        }
        true
    }
}

/// One channel's state.
struct TapeChannel {
    up_line: Vec<f32>,
    down_line: Vec<f32>,
    play_line: Vec<f32>,
    delay: Vec<f32>,
    write: i64,
    hysteresis: Hysteresis,
    // two biquads, transposed direct form II
    z0: f32,
    z1: f32,
    z2: f32,
    z3: f32,
    // record emphasis and playback de-emphasis, one pole each
    ex: f32,
    ey: f32,
    dx: f32,
    dy: f32,
    /// The low part of what comes across from the neighbouring tracks.
    bleed_low: f32,
    rng: u32,
    last_noise: f32,
    /// Samples of silence so far, and whether the channel has parked on it.
    idle: usize,
    parked: bool,
    /// The levels this channel is ramping from, so a change glides.
    drive_now: f32,
    output_now: f32,
}

impl TapeChannel {
    fn new(seed: u32, config: &TapeConfig) -> Self {
        TapeChannel {
            up_line: vec![0.0; HALF_TAPS + 2 * MAX_FRAMES],
            down_line: vec![0.0; HALF_TAPS + 2 * MAX_FRAMES],
            play_line: vec![0.0; FIR_TAPS + MAX_FRAMES],
            delay: vec![0.0; DELAY_SIZE],
            write: 0,
            hysteresis: Hysteresis::default(),
            z0: 0.0,
            z1: 0.0,
            z2: 0.0,
            z3: 0.0,
            ex: 0.0,
            ey: 0.0,
            dx: 0.0,
            dy: 0.0,
            bleed_low: 0.0,
            rng: seed | 1,
            last_noise: 0.0,
            idle: 0,
            parked: false,
            drive_now: config.drive,
            output_now: config.output,
        }
    }
}

fn seed_for(channel: usize) -> u32 {
    match channel {
        0 => 0x1234_5679,
        1 => 0x8BAD_F00D,
        c => (c as u32 + 1).wrapping_mul(0x9E37_79B9) ^ 0xA5A5_5A5A,
    }
}

/// A tape machine with any number of tracks, one per channel. The tracks share the transport,
/// and each carries its own settings, so a plugin decides what is per channel and what is global.
pub struct TapeStage {
    fs: f32,
    channels: Vec<TapeChannel>,
    half: Vec<f32>,
    scratch: Vec<f32>,
    scratch2: Vec<f32>,
    excursion: Vec<f32>,
    across: Vec<f32>,
    headstack: Headstack,
    handoff: Arc<Handoff<ConfigSet>>,
    current: Arc<ConfigSet>,
    base_now: f32,
    clock: f64,
}

impl TapeStage {
    /// Allocates everything the audio thread will use. Call off the audio thread.
    pub fn new(fs: f32, channels: usize, handoff: Arc<Handoff<ConfigSet>>, set: Arc<ConfigSet>) -> Self {
        let channels = channels.max(1);
        let _ = langevin();
        TapeStage {
            fs,
            channels: (0..channels)
                .map(|c| TapeChannel::new(seed_for(c), set.for_channel(c)))
                .collect(),
            half: half_band_taps().to_vec(),
            scratch: vec![0.0; 2 * MAX_FRAMES],
            scratch2: vec![0.0; 2 * MAX_FRAMES],
            excursion: vec![0.0; MAX_FRAMES],
            across: vec![0.0; MAX_FRAMES],
            headstack: Headstack::new(channels),
            handoff,
            base_now: set.machine().base_delay,
            current: set,
            clock: 0.0,
        }
    }

    /// A stage with the same settings on every channel and no outside control, for offline renders and tests.
    pub fn standalone(fs: f32, channels: usize, settings: TapeSettings) -> Self {
        let config = Arc::new(TapeConfig::new(&settings, fs, None));
        let set = Arc::new(ConfigSet::uniform(config, channels));
        Self::new(fs, channels, Arc::new(Handoff::new()), set)
    }

    pub fn channel_count(&self) -> usize {
        self.channels.len()
    }

    pub fn latency_samples(&self) -> u32 {
        self.current.machine().latency().ceil() as u32
    }

    /// Audio thread. Processes the channels in place. `sample_time` is the transport's
    /// position in samples; without one the stage keeps its own clock.
    pub fn process(&mut self, io: &mut [&mut [f32]], sample_time: Option<f64>) {
        let frames = match io.first() {
            Some(c) => c.len(),
            None => return,
        };
        if frames == 0 || frames > MAX_FRAMES {
            return;
        }
        if let Some(p) = self.handoff.take() {
            self.current = p;
        }
        let set = Arc::clone(&self.current);
        let machine: &TapeConfig = set.machine();
        let fs = self.fs;
        let start = match sample_time {
            Some(t) if t >= 0.0 => t,
            _ => self.clock,
        };
        let first = start as i64;
        let t0 = start / fs as f64;
        self.clock = start + frames as f64;

        // Where the tape is against where it should be, in samples, across this block.
        // The transport is one thing, so wow and flutter are the machine's, not a track's.
        let moving = machine.wow > 0.0 || machine.flutter > 0.0;
        let base_target = machine.base_delay;
        if moving || self.base_now > 0.0 {
            let mut i = 0usize;
            let mut e0 = transport::excursion(t0, machine.wow, machine.flutter, machine.capstan) as f32 * fs;
            while i < frames {
                let n = 64.min(frames - i);
                let t1 = t0 + (i + n) as f64 / fs as f64;
                let e1 = transport::excursion(t1, machine.wow, machine.flutter, machine.capstan) as f32 * fs;
                let step = (e1 - e0) / n as f32;
                for j in 0..n {
                    // The room to swing in opens and closes slowly, so a change of depth glides.
                    if self.base_now < base_target {
                        self.base_now = base_target.min(self.base_now + 0.004);
                    } else if self.base_now > base_target {
                        self.base_now = base_target.max(self.base_now - 0.004);
                    }
                    self.excursion[i + j] = (self.base_now + e0 + step * j as f32).max(0.0);
                }
                e0 = e1;
                i += n;
            }
        }
        let delay_open = moving || self.base_now > 0.0;

        let lan = langevin();
        let t = 1.0 / (2.0 * fs);
        let hb = HALF_TAPS;
        let fir = FIR_TAPS;

        let TapeStage {
            channels,
            half,
            scratch,
            scratch2,
            excursion,
            across,
            headstack,
            ..
        } = &mut *self;
        let count = io.len().min(channels.len());

        for c in 0..count {
            if io[c].len() < frames {
                continue;
            }
            let cfg: &TapeConfig = set.for_channel(c);
            let bq = &cfg.biquads;
            let x: &mut [f32] = &mut io[c][..frames];
            let ch = &mut channels[c];
            let drive_now = ch.drive_now;
            let output_now = ch.output_now;
            let drive_step = (cfg.drive - drive_now) / frames as f32;
            let out_step = (cfg.output - output_now) / frames as f32;

            // Nothing that isn't a number, and nothing past +12 dBFS, gets onto the tape:
            // the filters would carry it for ever.
            for v in x.iter_mut() {
                *v = if v.is_finite() { (*v).clamp(-4.0, 4.0) } else { 0.0 };
            }

            // A strip with nothing on it costs nothing: after a second below -90 dBFS
            // every filter has run out, so the channel parks and only hisses.
            let mut peak = 0.0f32;
            for v in x.iter() {
                peak = peak.max(v.abs());
            }
            ch.idle = if peak < 3.2e-5 { (ch.idle + frames).min(usize::MAX / 2) } else { 0 };
            if ch.idle > fs as usize {
                if !ch.parked {
                    ch.delay.fill(0.0);
                    ch.parked = true;
                }
                scratch[..frames].fill(0.0);
                ch.write += frames as i64;
            } else {
                ch.parked = false;

                // Level onto tape, in field units.
                for i in 0..frames {
                    scratch[i] = x[i] * (drive_now + drive_step * i as f32);
                }

                // Record emphasis: the treble lift the standard's playback curve takes back off.
                let (b0, b1, a1) = (cfg.emphasis_b0, cfg.emphasis_b1, cfg.emphasis_a1);
                let (mut ex, mut ey) = (ch.ex, ch.ey);
                for s in scratch[..frames].iter_mut() {
                    let v = *s;
                    ey = b0 * v + b1 * ex - a1 * ey;
                    ex = v;
                    *s = ey;
                }
                ch.ex = ex;
                ch.ey = ey + 1e-20;

                // Up to twice the rate.
                for i in 0..frames {
                    ch.up_line[hb - 1 + 2 * i] = 2.0 * scratch[i];
                    ch.up_line[hb - 1 + 2 * i + 1] = 0.0;
                }
                for n in 0..2 * frames {
                    let w = &ch.up_line[n..n + hb];
                    let mut s = 0.0f32;
                    for p in 0..hb {
                        s += w[p] * half[p];
                    }
                    scratch2[n] = s;
                }
                ch.up_line.copy_within(2 * frames..2 * frames + hb - 1, 0);

                // Onto the tape.
                let mut hy = ch.hysteresis;
                let mk = cfg.makeup;
                for i in 0..2 * frames {
                    ch.down_line[hb - 1 + i] =
                        mk * hy.step(scratch2[i], t, cfg.k, cfg.c, cfg.alpha, &lan.l, &lan.d, lan.scale);
                }
                ch.hysteresis = hy;

                // Back down.
                for n in 0..frames {
                    let w = &ch.down_line[2 * n..2 * n + hb];
                    let mut s = 0.0f32;
                    for p in 0..hb {
                        s += w[p] * half[p];
                    }
                    ch.play_line[fir - 1 + n] = s;
                }
                ch.down_line.copy_within(2 * frames..2 * frames + hb - 1, 0);

                // Playback de-emphasis: the same shelf turned over, so the pair cancels exactly.
                let (p0, p1, q1) = (1.0 / b0, a1 / b0, b1 / b0);
                let (mut dx, mut dy) = (ch.dx, ch.dy);
                for i in 0..frames {
                    let v = ch.play_line[fir - 1 + i];
                    dy = p0 * v + p1 * dx - q1 * dy;
                    dx = v;
                    ch.play_line[fir - 1 + i] = dy;
                }
                ch.dx = dx;
                ch.dy = dy + 1e-20;

                // What the playback head loses.
                for n in 0..frames {
                    let w = &ch.play_line[n..n + fir];
                    let mut s = 0.0f32;
                    for p in 0..fir {
                        s += w[p] * cfg.playback[p];
                    }
                    scratch[n] = s;
                }
                ch.play_line.copy_within(frames..frames + fir - 1, 0);

                // Head bump and the roll-off under it.
                let (mut z0, mut z1, mut z2, mut z3) = (ch.z0, ch.z1, ch.z2, ch.z3);
                for s in scratch[..frames].iter_mut() {
                    let a = *s;
                    let y = bq[0] * a + z0;
                    z0 = bq[1] * a - bq[3] * y + z1;
                    z1 = bq[2] * a - bq[4] * y;
                    let y2 = bq[5] * y + z2;
                    z2 = bq[6] * y - bq[8] * y2 + z3;
                    z3 = bq[7] * y - bq[9] * y2 + 1e-20;
                    *s = y2;
                }
                ch.z0 = z0;
                ch.z1 = z1;
                ch.z2 = z2;
                ch.z3 = z3;

                // Wow and flutter: read the tape where it actually is.
                let mask = (DELAY_SIZE - 1) as i64;
                let mut w = ch.write;
                if delay_open {
                    for i in 0..frames {
                        ch.delay[(w & mask) as usize] = scratch[i];
                        let d = excursion[i];
                        let whole = d as i64;
                        let frac = d - whole as f32;
                        let at = w - whole;
                        if d >= 2.0 {
                            let xm = ch.delay[((at + 1) & mask) as usize];
                            let x0 = ch.delay[(at & mask) as usize];
                            let x1 = ch.delay[((at - 1) & mask) as usize];
                            let x2 = ch.delay[((at - 2) & mask) as usize];
                            let f = frac;
                            let cm = -f * (f - 1.0) * (f - 2.0) / 6.0;
                            let c0 = (f + 1.0) * (f - 1.0) * (f - 2.0) / 2.0;
                            let c1 = -(f + 1.0) * f * (f - 2.0) / 2.0;
                            let c2 = (f + 1.0) * f * (f - 1.0) / 6.0;
                            scratch[i] = cm * xm + c0 * x0 + c1 * x1 + c2 * x2;
                        } else {
                            let x0 = ch.delay[(at & mask) as usize];
                            let x1 = ch.delay[((at - 1) & mask) as usize];
                            scratch[i] = x0 + frac * (x1 - x0);
                        }
                        w += 1;
                    }
                } else {
                    for i in 0..frames {
                        ch.delay[(w & mask) as usize] = scratch[i];
                        w += 1;
                    }
                }
                ch.write = w;
            }

            // The rest of the machine: leave this track where the ones beside it can
            // hear it, and take in what comes across from them. A silent track still
            // picks up its neighbours. A track whose link is off neither gives nor takes.
            if cfg.bleed > 0.0 {
                headstack.write(c, first, &scratch[..frames]);
                across[..frames].fill(0.0);
                let below = headstack.read(c as i64 - 1, first, &mut across[..frames]);
                let above = headstack.read(c as i64 + 1, first, &mut across[..frames]);
                if below || above {
                    let mut low = ch.bleed_low;
                    let a = cfg.bleed_pole;
                    let g = cfg.bleed;
                    for i in 0..frames {
                        let v = across[i];
                        low += a * (v - low);
                        scratch[i] += g * (v + BLEED_LOW * low);
                    }
                    ch.bleed_low = low + 1e-20;
                }
            }

            // Hiss: flat, with a rise at the bottom, which is where a measured
            // machine's noise sits.
            if cfg.hiss > 0.0 {
                let mut r = ch.rng;
                let mut low = ch.last_noise;
                let a = cfg.noise_pole;
                let amp = cfg.hiss * cfg.noise_norm;
                for s in scratch[..frames].iter_mut() {
                    r ^= r << 13;
                    r ^= r >> 17;
                    r ^= r << 5;
                    let n = (r as i32) as f32 / 2_147_483_648.0;
                    low += a * (n - low);
                    *s += amp * (0.85 * n + cfg.noise_low * low);
                }
                ch.rng = r;
                ch.last_noise = low;
            }

            for i in 0..frames {
                x[i] = scratch[i] * (output_now + out_step * i as f32);
            }
            ch.drive_now = cfg.drive;
            ch.output_now = cfg.output;
        }
    }
}
