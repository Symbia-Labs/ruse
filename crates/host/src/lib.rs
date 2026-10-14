//! Running and checking plugins without a DAW.
//!
//! This crate is what lets an agent build a plugin and know whether it works. It reads and writes WAV files,
//! generates test signals, renders a plugin offline with measurements, and runs the gates every plugin must pass.

pub mod alloc;
pub mod cli;
pub mod gates;
pub mod measure;
pub mod render;
pub mod signal;
pub mod wav;
