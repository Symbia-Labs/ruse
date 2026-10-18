//! What a block is, and how blocks describe themselves.
//!
//! Every block declares its arguments, with units, ranges and defaults, in one static table. The graph reads
//! that table to check a plugin's description, to clamp values while it runs, and to tell an agent what exists.

use crate::frame::Frame;

pub trait Block: Send {
    /// Called once when the plugin is built, with every argument at its constant value or its default. Blocks
    /// read their `fixed` arguments here, because those cannot change while the plugin runs.
    fn configure(&mut self, _args: &[f32]) {}

    /// Runs off the audio thread. Allocate here, and start every piece of state from zero.
    fn prepare(&mut self, rate: f32, channels: usize);

    /// Called once at the start of each aligned 32-sample piece, with the value of every argument in the order
    /// of `BlockInfo::args`, already clamped to its range. Set targets here. Do not run audio.
    fn control(&mut self, args: &[f32]);

    /// Processes `io.len` samples in place. `offset` is where this call starts within the aligned piece, so a
    /// ramp from the previous control value to the new one lands on the same samples whatever the host's block
    /// size. See [`crate::frame::ramp`].
    fn process(&mut self, io: &mut Frame, offset: usize);

    /// The sidechain for the samples the next `process` call gets, one channel per sidechain channel and none
    /// when the plugin has no sidechain. A block that can listen to the sidechain, such as a compressor keyed by
    /// a kick drum, copies what it needs here.
    fn key(&mut self, _key: &Frame) {}

    fn latency(&self) -> u32 {
        0
    }

    /// How many samples the block keeps sounding after its input stops, with its arguments at `args`, in the
    /// order of `BlockInfo::args`, at the sample rate `rate`. A host asks before any audio runs, so this reads
    /// the arguments it is given rather than what the last `control` call set.
    fn tail(&self, _args: &[f32], _rate: f32) -> u32 {
        0
    }

    /// The current value of meter number `index`, in the order of `BlockInfo::meters`.
    fn meter(&self, _index: usize) -> f32 {
        0.0
    }
}

#[derive(Clone, Copy, Debug)]
pub struct ArgInfo {
    pub name: &'static str,
    pub unit: &'static str,
    pub min: f32,
    pub max: f32,
    pub default: f32,
    /// The names of a choice, in index order. Empty for a number.
    pub options: &'static [&'static str],
    /// A fixed argument is read when the plugin is built and cannot follow a parameter.
    pub fixed: bool,
    pub doc: &'static str,
}

impl ArgInfo {
    pub const fn num(name: &'static str, unit: &'static str, min: f32, max: f32, default: f32, doc: &'static str) -> ArgInfo {
        ArgInfo {
            name,
            unit,
            min,
            max,
            default,
            options: &[],
            fixed: false,
            doc,
        }
    }

    pub const fn choice(name: &'static str, options: &'static [&'static str], default: usize, doc: &'static str) -> ArgInfo {
        ArgInfo {
            name,
            unit: "",
            min: 0.0,
            max: (options.len() - 1) as f32,
            default: default as f32,
            options,
            fixed: false,
            doc,
        }
    }

    pub const fn fixed(self) -> ArgInfo {
        ArgInfo { fixed: true, ..self }
    }

    pub fn is_choice(&self) -> bool {
        !self.options.is_empty()
    }

    /// The value a block gets: in range, and the default if the expression gave something that is not a number.
    pub fn clean(&self, v: f32) -> f32 {
        if v.is_nan() {
            self.default
        } else {
            let v = v.clamp(self.min, self.max);
            if self.is_choice() {
                v.round()
            } else {
                v
            }
        }
    }
}

pub struct BlockInfo {
    pub name: &'static str,
    pub doc: &'static str,
    pub args: &'static [ArgInfo],
    /// Values the block reports, which a plugin can bind to a meter parameter.
    pub meters: &'static [&'static str],
    pub make: fn() -> Box<dyn Block>,
}

impl BlockInfo {
    pub fn arg_index(&self, name: &str) -> Option<usize> {
        self.args.iter().position(|a| a.name == name)
    }

    pub fn meter_index(&self, name: &str) -> Option<usize> {
        self.meters.iter().position(|m| *m == name)
    }
}
