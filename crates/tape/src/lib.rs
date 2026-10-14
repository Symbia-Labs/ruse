//! Bias Tape: the signal path of a tape machine, as one plugin instance with any number of tracks.
//!
//! The model is Bias's own tape stage (Jiles-Atherton hysteresis, record and playback
//! equalization, head losses, wow and flutter, neighbour bleed, hiss), ported to Rust and held
//! to the Swift original by golden files.

// The index loops mirror the structure of the Swift original, which keeps the two easy to compare.
#![allow(clippy::needless_range_loop)]

pub mod config;
pub mod fft;
pub mod filters;
pub mod hysteresis;
pub mod plugin;
pub mod stage;
pub mod tables;
pub mod transport;

pub use config::{ConfigSet, TapeConfig};
pub use plugin::{BiasTape, PRESETS};
pub use stage::TapeStage;
pub use tables::TapeSettings;
