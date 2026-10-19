//! Noise: white, pink and brown, at a set RMS level.
//!
//! White noise is uniform random numbers with unit variance. Pink is white through Paul Kellet's filter, which
//! falls 3 dB an octave across the audio band, and brown is white through a leaky integrator, which falls 6 dB
//! an octave above 20 Hz. Each is scaled to unit RMS before the level, so the level reads the same for all three.
//! The random numbers are seeded, so two instances make the same noise.

use super::oscillator::ENDLESS;
use super::util::*;
use crate::block::{ArgInfo, Block, BlockInfo};
use crate::frame::{ramp, Frame};

const COLOR: usize = 0;
const LEVEL_DB: usize = 1;
const WIDTH: usize = 2;
const MODE: usize = 3;

/// Kellet's pink filter: the pole and gain of each section, then the direct gains for this sample and the last.
const PINK_POLES: [f32; 6] = [0.99886, 0.99332, 0.969, 0.8665, 0.55, -0.7616];
const PINK_GAINS: [f32; 6] = [0.055_517_9, 0.075_075_9, 0.153_852, 0.310_485_6, 0.532_952_2, -0.016_898];
const PINK_NOW: f32 = 0.5362;
const PINK_LAST: f32 = 0.115_926;
/// One over the filter's RMS gain for unit-variance white noise, worked out from its impulse response.
const PINK_NORM: f32 = 0.327_597_4;
/// Where brown noise stops rising as it falls in frequency.
const BROWN_CORNER_HZ: f32 = 20.0;

pub static INFO: BlockInfo = BlockInfo {
    name: "noise",
    doc: "Noise at a set RMS level. white is even at every frequency, pink falls 3 dB an octave like most natural sounds, and brown falls 6 dB an octave for a deep rumble. mode replace puts out the noise alone and add mixes it into the signal, for hiss. width 1 gives each channel its own noise and 0 the same noise in all of them. It is seeded, so two instances make the same noise, and its tail never ends.",
    args: &[
        ArgInfo::choice("color", &["white", "pink", "brown"], 0, ""),
        ArgInfo::num("level_db", "dB", -96.0, 0.0, -24.0, "The RMS level. A change glides."),
        ArgInfo::num("width", "", 0.0, 1.0, 1.0, "0 is the same noise in every channel and 1 a different noise in each, at the same level."),
        ArgInfo::choice("mode", &["replace", "add"], 0, "replace puts out the noise alone. add mixes it into the signal."),
    ],
    meters: &[],
    make,
};

pub fn make() -> Box<dyn Block> {
    Box::new(Noise::default())
}

/// One stream of noise with its own seed and filter memory.
#[derive(Clone, Default)]
struct Stream {
    seed: u32,
    pink: [f32; 6],
    last: f32,
    brown: f32,
}

impl Stream {
    fn new(index: usize) -> Stream {
        Stream {
            seed: 0x9E37_79B9u32.wrapping_mul(index as u32 + 1) | 1,
            ..Default::default()
        }
    }

    /// Uniform from -sqrt(3) to sqrt(3), which has unit variance.
    fn white(&mut self) -> f32 {
        let mut s = self.seed;
        s ^= s << 13;
        s ^= s >> 17;
        s ^= s << 5;
        self.seed = s;
        (s as f32 / u32::MAX as f32 * 2.0 - 1.0) * 3f32.sqrt()
    }

    fn next(&mut self, color: usize, brown_pole: f32, brown_gain: f32) -> f32 {
        let w = self.white();
        match color {
            1 => {
                let mut sum = w * PINK_NOW + self.last * PINK_LAST;
                for ((b, p), g) in self.pink.iter_mut().zip(PINK_POLES).zip(PINK_GAINS) {
                    *b = p * *b + w * g;
                    sum += *b;
                }
                self.last = w;
                sum * PINK_NORM
            }
            2 => {
                self.brown = brown_pole * self.brown + brown_gain * w;
                self.brown
            }
            _ => w,
        }
    }
}

#[derive(Default)]
struct Noise {
    color: usize,
    level: (f32, f32),
    width: f32,
    add: bool,
    started: bool,
    brown_pole: f32,
    brown_gain: f32,
    /// One stream for each channel, and one shared by all of them for width below 1.
    own: Vec<Stream>,
    shared: Stream,
}

impl Block for Noise {
    fn prepare(&mut self, rate: f32, channels: usize) {
        let pole = (-std::f32::consts::TAU * BROWN_CORNER_HZ / rate).exp();
        *self = Noise {
            brown_pole: pole,
            brown_gain: (1.0 - pole * pole).sqrt(),
            own: (0..channels).map(|c| Stream::new(c + 1)).collect(),
            shared: Stream::new(0),
            width: 1.0,
            ..Default::default()
        };
    }

    fn control(&mut self, a: &[f32]) {
        let level = db_to_lin(a[LEVEL_DB]);
        if !self.started {
            self.level = (level, level);
            self.started = true;
        }
        self.level = (self.level.1, level);
        self.color = a[COLOR] as usize;
        self.width = a[WIDTH];
        self.add = a[MODE] >= 0.5;
    }

    fn process(&mut self, io: &mut Frame, offset: usize) {
        // Mixed by power, so the level holds at every width.
        let own_gain = self.width.sqrt();
        let shared_gain = (1.0 - self.width).sqrt();
        let (pole, gain) = (self.brown_pole, self.brown_gain);
        for k in 0..io.len {
            let level = ramp(self.level.0, self.level.1, offset, k);
            let common = self.shared.next(self.color, pole, gain);
            for (ch, stream) in io.ch.iter_mut().zip(self.own.iter_mut()) {
                let mine = stream.next(self.color, pole, gain);
                let v = (common * shared_gain + mine * own_gain) * level;
                ch[k] = if self.add { ch[k] + v } else { v };
            }
        }
        for s in self.own.iter_mut().chain(std::iter::once(&mut self.shared)) {
            s.pink.iter_mut().for_each(flush);
            flush(&mut s.brown);
        }
    }

    fn tail(&self, _args: &[f32], _rate: f32) -> u32 {
        ENDLESS
    }
}
