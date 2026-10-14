//! The blocks, and the table that lists them.

pub mod biquad;
pub mod delay;
pub mod dynamics;
pub mod gain;
pub mod harmonic_eq;
pub mod onepole;
pub mod shaper;
pub mod stereo;
pub mod tremolo;
pub mod util;

use crate::block::BlockInfo;

/// Every block, in the order an agent should read about them.
pub fn catalogue() -> Vec<&'static BlockInfo> {
    vec![
        &gain::INFO,
        &biquad::INFO,
        &onepole::INFO,
        &shaper::INFO,
        &dynamics::INFO,
        &delay::INFO,
        &tremolo::INFO,
        &stereo::INFO,
        &harmonic_eq::INFO,
    ]
}

pub fn find(name: &str) -> Option<&'static BlockInfo> {
    catalogue().into_iter().find(|b| b.name == name)
}
