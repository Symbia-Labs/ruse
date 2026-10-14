//! The unit of work. The graph cuts audio into pieces of at most `CHUNK` samples, aligned to a running
//! counter that does not depend on the host's block size. Controls update once at the start of each aligned
//! piece, so the audio comes out the same whatever block size the host uses.

pub const CHUNK: usize = 32;

#[derive(Clone)]
pub struct Frame {
    pub ch: Vec<[f32; CHUNK]>,
    /// How many samples of each channel are in use.
    pub len: usize,
}

impl Frame {
    pub fn new(channels: usize) -> Frame {
        Frame {
            ch: vec![[0.0; CHUNK]; channels],
            len: 0,
        }
    }

    pub fn channels(&self) -> usize {
        self.ch.len()
    }

    /// Copies the samples in use. Both frames must have the same number of channels.
    pub fn copy_from(&mut self, other: &Frame) {
        self.len = other.len;
        for (a, b) in self.ch.iter_mut().zip(&other.ch) {
            a[..other.len].copy_from_slice(&b[..other.len]);
        }
    }

    pub fn silence(&mut self) {
        let n = self.len;
        for c in self.ch.iter_mut() {
            c[..n].fill(0.0);
        }
    }
}

/// The weight, from 0 to 1, that sample `i` of a piece starting at `offset` has gone from the previous value to
/// the new one. A ramp that spans the aligned piece, however the host split it.
#[inline]
pub fn ramp(prev: f32, cur: f32, offset: usize, i: usize) -> f32 {
    prev + (cur - prev) * ((offset + i + 1) as f32 / CHUNK as f32)
}
