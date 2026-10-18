//! Fewer bits and a lower sample rate, on purpose.
//!
//! Each sample is rounded to a grid of 2^(bits - 1) steps per unit, and a sample-and-hold takes a new sample only
//! at the reduced rate. Nothing smooths either one: the steps and the folded-back images are the sound.

use crate::block::{ArgInfo, Block, BlockInfo};
use crate::frame::Frame;

const BITS: usize = 0;
const RATE_HZ: usize = 1;
const DITHER: usize = 2;

pub static INFO: BlockInfo = BlockInfo {
    name: "bitcrusher",
    doc: "Lowers the resolution and the sample rate on purpose. bits rounds every sample to a coarser grid, which adds a gritty noise that follows the signal. rate_hz holds each sample until the next tick of a slower clock, which folds high frequencies back down as metallic tones. Both move smoothly, fractional bits included, so either can follow a parameter or a modulator. Dither trades the gritty noise for a steady hiss, which stops when the input goes silent.",
    args: &[
        ArgInfo::num("bits", "bits", 1.0, 24.0, 8.0, "The resolution. 16 is CD quality, 8 is an early sampler, and below 4 little but the loudest parts survive."),
        ArgInfo::num("rate_hz", "Hz", 50.0, 192000.0, 192000.0, "How often a new sample is taken. At or above the plugin's sample rate every sample is."),
        ArgInfo::choice("dither", &["off", "on"], 0, "Adds a little noise before rounding, so quiet sounds fade into hiss instead of breaking up."),
    ],
    meters: &[],
    make,
};

pub fn make() -> Box<dyn Block> {
    Box::new(Bitcrusher::default())
}

#[derive(Default)]
struct Bitcrusher {
    rate: f32,
    step: f32,
    tick: f64,
    phase: f64,
    dither: bool,
    held: Vec<f32>,
    /// By channel, how many samples the input has been all but silent, and how many it takes to stop the dither.
    quiet: Vec<u32>,
    hush: u32,
    seed: u32,
}

/// A uniform random number from -0.5 to 0.5.
fn uniform(seed: &mut u32) -> f32 {
    let mut s = *seed;
    s ^= s << 13;
    s ^= s >> 17;
    s ^= s << 5;
    *seed = s;
    s as f32 / u32::MAX as f32 - 0.5
}

impl Block for Bitcrusher {
    fn prepare(&mut self, rate: f32, channels: usize) {
        *self = Bitcrusher {
            rate,
            step: 1.0 / 128.0,
            tick: 1.0,
            // The first sample is always taken.
            phase: 1.0,
            held: vec![0.0; channels],
            quiet: vec![u32::MAX; channels],
            hush: (0.05 * rate) as u32,
            seed: 0x2545_F491,
            ..Default::default()
        };
    }

    fn control(&mut self, a: &[f32]) {
        self.step = (1.0 - a[BITS]).exp2();
        self.tick = (a[RATE_HZ] as f64 / self.rate as f64).min(1.0);
        self.dither = a[DITHER] >= 0.5;
    }

    fn process(&mut self, io: &mut Frame, _offset: usize) {
        let step = self.step;
        for k in 0..io.len {
            self.phase += self.tick;
            let take = self.phase >= 1.0;
            if take {
                self.phase -= 1.0;
            }
            for ((ch, held), quiet) in io.ch.iter_mut().zip(self.held.iter_mut()).zip(self.quiet.iter_mut()) {
                let x = ch[k];
                // Dither stops after 50 ms of input under a hundredth of a step, so silence comes out as silence.
                *quiet = if x.abs() > step * 0.01 { 0 } else { quiet.saturating_add(1) };
                if take {
                    // Two uniform numbers add to a triangle from -1 to 1 step, which leaves no trace of the signal
                    // in the noise.
                    let noise = if self.dither && *quiet < self.hush {
                        uniform(&mut self.seed) + uniform(&mut self.seed)
                    } else {
                        0.0
                    };
                    *held = (x / step + noise).round() * step;
                }
                ch[k] = *held;
            }
        }
    }
}
