use crate::layout::Layout;
use crate::param::ParamSet;

/// What the host settled on before processing starts.
#[derive(Clone, Copy, Debug)]
pub struct Setup {
    pub sample_rate: f32,
    pub max_block: usize,
    /// Channels on the main bus.
    pub channels: u32,
}

/// Where the block sits in the host's timeline.
#[derive(Clone, Copy, Debug)]
pub struct ProcessContext {
    pub sample_rate: f32,
    /// The transport's position in samples at the start of the block, when the host has one.
    /// Plugins that must stay locked across instances, such as wow and flutter, read this.
    pub sample_time: Option<f64>,
    pub playing: bool,
    pub tempo_bpm: Option<f64>,
}

impl ProcessContext {
    /// A context with no transport, for offline renders and tests.
    pub fn offline(sample_rate: f32) -> Self {
        ProcessContext {
            sample_rate,
            sample_time: None,
            playing: false,
            tempo_bpm: None,
        }
    }
}

pub trait Plugin: Send {
    fn layout(&self) -> Layout;

    fn params(&self) -> &ParamSet;

    /// Called on a non-audio thread before processing starts. Allocate here.
    fn prepare(&mut self, setup: &Setup);

    fn latency_samples(&self) -> u32 {
        0
    }

    fn tail_samples(&self) -> u32 {
        0
    }

    /// Called on the audio thread, in place, with one slice per channel of the main bus.
    /// Must not allocate, lock, or block. An instrument's slices start silent.
    fn process(&mut self, channels: &mut [&mut [f32]], ctx: &ProcessContext);

    /// True for an instrument: it has no audio input and plays the MIDI it is sent.
    /// Its layout declares output buses only.
    fn is_instrument(&self) -> bool {
        false
    }

    /// One MIDI message for an instrument, `offset` frames into the next block. Called on
    /// the audio thread just before `process`, in the order the host sent them.
    fn midi(&mut self, _status: u8, _data1: u8, _data2: u8, _offset: u32) {}
}
