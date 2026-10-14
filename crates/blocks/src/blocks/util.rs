//! Small helpers the blocks share.

use crate::frame::CHUNK;

#[inline]
pub fn db_to_lin(db: f32) -> f32 {
    (db * 0.115_129_25).exp()
}

#[inline]
pub fn lin_to_db(x: f32) -> f32 {
    20.0 * x.max(1e-9).log10()
}

/// Turns a state that has decayed to almost nothing into an exact zero, so it never becomes subnormal.
#[inline]
pub fn flush(x: &mut f32) {
    if x.abs() < 1e-20 {
        *x = 0.0;
    }
}

/// The fraction of the way to a target that a value moves in one control tick, for a smoothing time in ms.
pub fn tick_alpha(rate: f32, ms: f32) -> f32 {
    1.0 - (-(CHUNK as f32) / (ms * 0.001 * rate)).exp()
}

/// The coefficient of a one-pole filter, a = 1 - exp(-2 pi f / rate).
pub fn onepole_coef(freq: f32, rate: f32) -> f32 {
    1.0 - (-2.0 * std::f32::consts::PI * freq.clamp(1.0, rate * 0.49) / rate).exp()
}

/// A value that glides toward its target once per control tick, and starts at the first target it is given.
#[derive(Clone, Copy, Default)]
pub struct Glide {
    value: f32,
    set: bool,
}

impl Glide {
    pub fn step(&mut self, target: f32, alpha: f32) -> f32 {
        if !self.set {
            self.value = target;
            self.set = true;
        } else {
            self.value += (target - self.value) * alpha;
        }
        self.value
    }

    pub fn value(&self) -> f32 {
        self.value
    }
}
