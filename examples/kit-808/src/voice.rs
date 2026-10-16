//! The drum voices: each hit is one voice that computes its own samples from sine and
//! square oscillators, noise, state-variable filters and decaying envelopes, the way the
//! TR-808's circuits did. No samples.

use std::f32::consts::PI;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Drum {
    Kick,
    Snare,
    Clap,
    Rim,
    ClosedHat,
    OpenHat,
    LowTom,
    HighTom,
    Ride,
    Crash,
    Cowbell,
}

impl Drum {
    /// The drum a General MIDI percussion note plays, if any.
    pub fn for_note(note: u8) -> Option<Drum> {
        Some(match note {
            35 | 36 => Drum::Kick,
            38 | 40 => Drum::Snare,
            39 => Drum::Clap,
            37 => Drum::Rim,
            42 | 44 => Drum::ClosedHat,
            46 => Drum::OpenHat,
            41 | 43 | 45 => Drum::LowTom,
            47 | 48 | 50 => Drum::HighTom,
            51 | 53 | 59 => Drum::Ride,
            49 | 52 | 55 | 57 => Drum::Crash,
            56 => Drum::Cowbell,
            _ => return None,
        })
    }

    /// Seconds until the sound is below about −70 dB at its longest decay setting.
    fn length(self) -> f32 {
        match self {
            Drum::Kick => 6.0,
            Drum::Snare | Drum::Clap => 1.2,
            Drum::Rim => 0.2,
            Drum::ClosedHat => 0.8,
            Drum::OpenHat => 6.0,
            Drum::LowTom | Drum::HighTom => 2.6,
            Drum::Ride | Drum::Crash => 4.0,
            Drum::Cowbell => 1.4,
        }
    }
}

/// Settings a voice reads when it starts.
#[derive(Clone, Copy, Debug)]
pub struct Shape {
    /// Pitch multiplier for the tuned drums.
    pub tune: f32,
    /// Kick decay multiplier.
    pub kick_decay: f32,
    /// Snare wire level, 0 to 1.
    pub snappy: f32,
    /// Hat and cymbal decay multiplier.
    pub hat_decay: f32,
}

impl Default for Shape {
    fn default() -> Self {
        Shape { tune: 1.0, kick_decay: 1.0, snappy: 0.8, hat_decay: 1.0 }
    }
}

/// Zavalishin's state-variable filter; `run` returns (low, band, high).
#[derive(Clone, Copy, Default)]
struct Svf {
    ic1: f32,
    ic2: f32,
    a1: f32,
    a2: f32,
    a3: f32,
    k: f32,
}

impl Svf {
    fn new(fc: f32, q: f32, rate: f32) -> Svf {
        let g = (PI * (fc / rate).min(0.49)).tan();
        let k = 1.0 / q;
        let a1 = 1.0 / (1.0 + g * (g + k));
        Svf { ic1: 0.0, ic2: 0.0, a1, a2: g * a1, a3: g * g * a1, k }
    }

    fn run(&mut self, x: f32) -> (f32, f32, f32) {
        let v3 = x - self.ic2;
        let v1 = self.a1 * self.ic1 + self.a2 * v3;
        let v2 = self.ic2 + self.a2 * self.ic1 + self.a3 * v3;
        self.ic1 = 2.0 * v1 - self.ic1;
        self.ic2 = 2.0 * v2 - self.ic2;
        (v2, v1, x - self.k * v1 - v2)
    }
}

/// The six square-wave oscillators of the 808's cymbal and hat section.
const METAL: [f32; 6] = [205.3, 304.4, 369.6, 522.7, 540.0, 800.0];

#[derive(Clone, Copy)]
pub struct Voice {
    pub drum: Drum,
    vel: f32,
    t: f32,
    dt: f32,
    phase: [f32; 6],
    tone: f32,
    seed: u32,
    f1: Svf,
    f2: Svf,
    /// Falls from 1 toward 0 once the voice is choked.
    choke: f32,
    choking: bool,
    finished: bool,
    shape: Shape,
}

impl Voice {
    pub fn new(drum: Drum, velocity: f32, rate: f32, shape: Shape) -> Voice {
        let (f1, f2) = match drum {
            Drum::Snare => (Svf::new(1800.0, 0.7, rate), Svf::new(9000.0, 0.7, rate)),
            Drum::Clap => (Svf::new(1100.0, 2.5, rate), Svf::new(1100.0, 2.5, rate)),
            Drum::ClosedHat | Drum::OpenHat => (Svf::new(7500.0, 0.8, rate), Svf::new(10_000.0, 1.2, rate)),
            Drum::Ride => (Svf::new(5000.0, 0.8, rate), Svf::new(3800.0, 3.0, rate)),
            Drum::Crash => (Svf::new(4000.0, 0.7, rate), Svf::new(9000.0, 0.8, rate)),
            Drum::Cowbell => (Svf::new(2640.0, 3.0, rate), Svf::new(800.0, 0.7, rate)),
            Drum::Rim => (Svf::new(600.0, 0.7, rate), Svf::new(600.0, 0.7, rate)),
            _ => (Svf::new(1000.0, 0.7, rate), Svf::new(1000.0, 0.7, rate)),
        };
        Voice {
            drum,
            vel: velocity * velocity,
            t: 0.0,
            dt: 1.0 / rate,
            phase: [0.0; 6],
            tone: 0.0,
            seed: 0x2545_F491 ^ (drum as u32).wrapping_mul(2_654_435_761),
            f1,
            f2,
            choke: 1.0,
            choking: false,
            finished: false,
            shape,
        }
    }

    pub fn choke(&mut self) {
        self.choking = true;
    }

    /// Marks the voice finished, for removal after the block.
    pub fn choke_now(&mut self) {
        self.finished = true;
    }

    pub fn done(&self) -> bool {
        self.finished
    }

    fn noise(&mut self) -> f32 {
        self.seed ^= self.seed << 13;
        self.seed ^= self.seed >> 17;
        self.seed ^= self.seed << 5;
        self.seed as f32 / u32::MAX as f32 * 2.0 - 1.0
    }

    fn metal(&mut self, freqs: &[f32]) -> f32 {
        let mut s = 0.0;
        for (i, f) in freqs.iter().enumerate() {
            self.phase[i] = (self.phase[i] + f * self.dt).fract();
            s += if self.phase[i] < 0.5 { 1.0 } else { -1.0 };
        }
        s / freqs.len() as f32
    }

    fn sine(&mut self, f: f32) -> f32 {
        self.tone = (self.tone + f * self.dt).fract();
        (2.0 * PI * self.tone).sin()
    }

    /// The next sample, or None once the voice has died away.
    pub fn next(&mut self) -> Option<f32> {
        if self.finished {
            return None;
        }
        let t = self.t;
        self.t += self.dt;
        let sh = self.shape;
        let e = |tau: f32| (-t / tau).exp();
        let out = match self.drum {
            Drum::Kick => {
                let f = (48.0 + 110.0 * e(0.028)) * sh.tune;
                let body = self.sine(f) * e(0.42 * sh.kick_decay);
                let click = if t < 0.003 { self.noise() * 0.4 * (1.0 - t / 0.003) } else { 0.0 };
                (1.6 * (body + click)).tanh() * 0.9
            }
            Drum::Snare => {
                let tone = (self.sine(185.0 * sh.tune) * 0.6 + (2.0 * PI * 330.0 * sh.tune * t).sin() * 0.3) * e(0.07);
                let n = self.noise();
                let hp = self.f1.run(n).2;
                let lp = self.f2.run(hp).0;
                tone * 0.7 + lp * e(0.16) * sh.snappy
            }
            Drum::Clap => {
                let n = self.noise();
                let bp = self.f1.run(n).1 * 1.5;
                let burst = [0.0f32, 0.011, 0.022].iter().map(|&s| if t >= s { (-(t - s) / 0.005).exp() } else { 0.0 }).fold(0.0f32, f32::max);
                let tail = if t >= 0.03 { (-(t - 0.03) / 0.14).exp() } else { 0.0 };
                bp * burst.max(tail) * 0.9
            }
            Drum::Rim => {
                let s = self.sine(1667.0 * sh.tune) * 0.6 + (2.0 * PI * 455.0 * sh.tune * t).sin() * 0.5;
                self.f1.run(s).2 * e(0.012) * 1.2
            }
            Drum::ClosedHat | Drum::OpenHat => {
                let m = self.metal(&METAL) + self.noise() * 0.15;
                let hp = self.f1.run(m).2;
                let bp = self.f2.run(hp).1;
                let tau = if self.drum == Drum::ClosedHat { 0.045 } else { 0.38 } * sh.hat_decay;
                (bp * 1.6 + hp * 0.5) * e(tau) * 0.7
            }
            Drum::Ride => {
                let m = self.metal(&METAL);
                let hp = self.f1.run(m).2;
                let bell = self.f2.run(m).1;
                (hp * 0.5 * e(1.1 * sh.hat_decay) + bell * 0.6 * e(0.25)) * 0.8
            }
            Drum::Crash => {
                let m = self.metal(&METAL) * 0.6 + self.noise() * 0.5;
                let hp = self.f1.run(m).2;
                let lp = self.f2.run(hp).0;
                lp * e(1.3 * sh.hat_decay) * 0.75
            }
            Drum::Cowbell => {
                let m = self.metal(&[540.0 * sh.tune, 800.0 * sh.tune]);
                let bp = self.f1.run(m).1;
                let env = if t < 0.02 { 1.0 - t / 0.02 * 0.5 } else { 0.5 * (-(t - 0.02) / 0.16).exp() };
                bp * env * 0.6
            }
            Drum::LowTom | Drum::HighTom => {
                let base = if self.drum == Drum::LowTom { 98.0 } else { 160.0 } * sh.tune;
                let f = base * (1.0 + 0.35 * e(0.06));
                self.sine(f) * e(0.32) * 0.9
            }
        };
        if self.choking {
            self.choke *= 0.995;
        }
        if t > self.drum.length() || (self.choking && self.choke < 0.001) {
            self.finished = true;
            return None;
        }
        Some(out * self.vel * self.choke)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL: [Drum; 11] = [
        Drum::Kick,
        Drum::Snare,
        Drum::Clap,
        Drum::Rim,
        Drum::ClosedHat,
        Drum::OpenHat,
        Drum::LowTom,
        Drum::HighTom,
        Drum::Ride,
        Drum::Crash,
        Drum::Cowbell,
    ];

    #[test]
    fn every_drum_rings_bounded_and_ends() {
        for drum in ALL {
            let mut v = Voice::new(drum, 1.0, 48_000.0, Shape::default());
            let (mut peak, mut n) = (0.0f32, 0usize);
            while let Some(x) = v.next() {
                assert!(x.is_finite(), "{drum:?}");
                peak = peak.max(x.abs());
                n += 1;
            }
            assert!(peak > 0.05 && peak <= 1.2, "{drum:?} peak {peak}");
            assert!(n as f32 <= (drum.length() + 0.01) * 48_000.0, "{drum:?} ran {n} samples");
        }
    }

    #[test]
    fn general_midi_drum_notes_map() {
        assert_eq!(Drum::for_note(36), Some(Drum::Kick));
        assert_eq!(Drum::for_note(38), Some(Drum::Snare));
        assert_eq!(Drum::for_note(42), Some(Drum::ClosedHat));
        assert_eq!(Drum::for_note(46), Some(Drum::OpenHat));
        assert_eq!(Drum::for_note(60), None);
    }

    #[test]
    fn tune_moves_the_kick() {
        let crossings = |tune: f32| {
            let mut v = Voice::new(Drum::Kick, 1.0, 48_000.0, Shape { tune, ..Shape::default() });
            let mut last = 0.0f32;
            let mut c = 0;
            for _ in 0..24_000 {
                let x = v.next().unwrap_or(0.0);
                if last <= 0.0 && x > 0.0 {
                    c += 1;
                }
                last = x;
            }
            c
        };
        assert!(crossings(2.0) > crossings(1.0) + 10);
    }
}
