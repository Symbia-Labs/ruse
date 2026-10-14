//! Test signals for rendering and for the checks.

use std::f64::consts::PI;

#[derive(Clone, Debug, PartialEq)]
pub enum Signal {
    Silence,
    Sine { hz: f32, db: f32 },
    Noise { db: f32 },
    Impulse { db: f32 },
    Sweep { db: f32 },
    /// Alternating plus and minus, the hardest case for a filter.
    Nyquist { db: f32 },
}

pub fn gain(db: f32) -> f32 {
    10f32.powf(db / 20.0)
}

impl Signal {
    pub fn describe(&self) -> String {
        match self {
            Signal::Silence => "silence".to_string(),
            Signal::Sine { hz, db } => format!("sine {} Hz at {} dBFS", hz, db),
            Signal::Noise { db } => format!("white noise at {} dBFS", db),
            Signal::Impulse { db } => format!("impulse at {} dBFS", db),
            Signal::Sweep { db } => format!("log sweep at {} dBFS", db),
            Signal::Nyquist { db } => format!("alternating samples at {} dBFS", db),
        }
    }
}

/// Reads `silence`, `sine:HZ:DB`, `noise:DB`, `impulse:DB`, `sweep:DB` or `nyquist:DB`.
pub fn parse(spec: &str) -> Result<Signal, String> {
    let parts: Vec<&str> = spec.trim().split(':').collect();
    let num = |i: usize, default: f32| -> Result<f32, String> {
        match parts.get(i) {
            Some(s) => s
                .parse::<f32>()
                .map_err(|_| format!("'{}' in signal '{}' is not a number", s, spec)),
            None => Ok(default),
        }
    };
    match parts[0] {
        "silence" => Ok(Signal::Silence),
        "sine" => Ok(Signal::Sine {
            hz: num(1, 1000.0)?,
            db: num(2, -12.0)?,
        }),
        "noise" => Ok(Signal::Noise { db: num(1, -18.0)? }),
        "impulse" => Ok(Signal::Impulse { db: num(1, -6.0)? }),
        "sweep" => Ok(Signal::Sweep { db: num(1, -12.0)? }),
        "nyquist" => Ok(Signal::Nyquist { db: num(1, -12.0)? }),
        other => Err(format!(
            "unknown signal '{}'. Use silence, sine:HZ:DB, noise:DB, impulse:DB, sweep:DB or nyquist:DB",
            other
        )),
    }
}

struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Self {
        Rng(seed | 1)
    }

    /// Uniform in -1 to 1.
    fn next(&mut self) -> f32 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        let r = x.wrapping_mul(0x2545_F491_4F6C_DD1D);
        ((r >> 40) as f32 / (1u64 << 24) as f32) * 2.0 - 1.0
    }
}

/// The same signal on every channel. Noise differs from channel to channel and is the same on every run.
pub fn generate(sig: &Signal, rate: f32, frames: usize, channels: usize) -> Vec<Vec<f32>> {
    (0..channels).map(|c| one(sig, rate, frames, c)).collect()
}

fn one(sig: &Signal, rate: f32, frames: usize, channel: usize) -> Vec<f32> {
    match sig {
        Signal::Silence => vec![0.0; frames],
        Signal::Sine { hz, db } => {
            let g = gain(*db) as f64;
            let w = 2.0 * PI * (*hz as f64) / (rate as f64);
            (0..frames).map(|n| (g * (w * n as f64).sin()) as f32).collect()
        }
        Signal::Noise { db } => {
            let g = gain(*db);
            let mut rng = Rng::new(0x9E37_79B9_7F4A_7C15 ^ ((channel as u64 + 1).wrapping_mul(0xD1B5_4A32_D192_ED03)));
            (0..frames).map(|_| rng.next() * g).collect()
        }
        Signal::Impulse { db } => {
            let mut v = vec![0.0; frames];
            if frames > 0 {
                v[16.min(frames - 1)] = gain(*db);
            }
            v
        }
        Signal::Sweep { db } => {
            let g = gain(*db) as f64;
            let f0 = 20.0f64;
            let f1 = 20000.0f64.min(0.45 * rate as f64);
            let total = frames as f64 / rate as f64;
            let k = f1 / f0;
            (0..frames)
                .map(|n| {
                    let t = n as f64 / rate as f64;
                    let phase = 2.0 * PI * f0 * total / k.ln() * (k.powf(t / total) - 1.0);
                    (g * phase.sin()) as f32
                })
                .collect()
        }
        Signal::Nyquist { db } => {
            let g = gain(*db);
            (0..frames).map(|n| if n % 2 == 0 { g } else { -g }).collect()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn specs_are_read_with_defaults() {
        assert_eq!(parse("silence").unwrap(), Signal::Silence);
        assert_eq!(parse("sine:440:-6").unwrap(), Signal::Sine { hz: 440.0, db: -6.0 });
        assert_eq!(parse("noise").unwrap(), Signal::Noise { db: -18.0 });
        assert!(parse("sine:abc").is_err());
        assert!(parse("square").is_err());
    }

    #[test]
    fn noise_repeats_and_differs_between_channels() {
        let a = generate(&Signal::Noise { db: 0.0 }, 48000.0, 256, 2);
        let b = generate(&Signal::Noise { db: 0.0 }, 48000.0, 256, 2);
        assert_eq!(a, b);
        assert_ne!(a[0], a[1]);
        assert!(a[0].iter().all(|v| v.abs() <= 1.0));
    }

    #[test]
    fn the_impulse_is_one_sample() {
        let v = &generate(&Signal::Impulse { db: 0.0 }, 48000.0, 100, 1)[0];
        assert_eq!(v.iter().filter(|x| **x != 0.0).count(), 1);
        assert_eq!(v[16], 1.0);
    }
}
