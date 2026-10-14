//! A plugin's saved state: a short header and every value of every parameter.
//!
//! Format wrappers use this for the host's project save, so every format stores the same bytes.
//! Per-channel parameters keep one value for each channel, and a state saved with more or fewer
//! parameters than the plugin has loads as far as it fits.

use crate::param::{Kind, ParamSet};

const MAGIC: &[u8; 4] = b"MUSE";
const VERSION: u32 = 1;

/// The bytes that restore the current values.
pub fn save(params: &ParamSet) -> Vec<u8> {
    let mut bytes: Vec<u8> = Vec::new();
    bytes.extend_from_slice(MAGIC);
    bytes.extend_from_slice(&VERSION.to_le_bytes());
    bytes.extend_from_slice(&(params.len() as u32).to_le_bytes());
    for p in params.iter() {
        bytes.extend_from_slice(&(p.slots() as u32).to_le_bytes());
        for c in 0..p.slots() {
            bytes.extend_from_slice(&p.get_ch(c).to_le_bytes());
        }
    }
    bytes
}

/// Applies saved bytes, then lets the plugin's listener hear about the change. Returns false, changing
/// nothing it can avoid, if the bytes are not a MUSE state or end early.
pub fn load(params: &ParamSet, bytes: &[u8]) -> bool {
    let u32_at = |at: usize| bytes.get(at..at + 4).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]));
    if bytes.get(..4) != Some(&MAGIC[..]) || u32_at(4) != Some(VERSION) {
        return false;
    }
    let Some(count) = u32_at(8) else {
        return false;
    };
    // Read everything before writing anything, so a state that ends early changes nothing.
    let mut values: Vec<(usize, usize, f32)> = Vec::new();
    let mut at = 12usize;
    for pi in 0..count as usize {
        let Some(slots) = u32_at(at) else {
            return false;
        };
        at += 4;
        for c in 0..slots as usize {
            let Some(bits) = u32_at(at) else {
                return false;
            };
            at += 4;
            values.push((pi, c, f32::from_bits(bits)));
        }
    }
    for (pi, c, v) in values {
        if pi < params.len() {
            let p = params.at(pi);
            if p.info().kind != Kind::Meter && c < p.slots() {
                p.set_ch(c, v);
            }
        }
    }
    params.notify();
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::param::ParamInfo;

    fn set() -> ParamSet {
        ParamSet::with_channels(
            &[
                ParamInfo::knob("gain", "Gain", "dB", -60.0, 12.0, 0.0),
                ParamInfo::knob("trim", "Trim", "dB", -12.0, 12.0, 0.0).per_channel(),
            ],
            4,
        )
    }

    #[test]
    fn values_survive_a_round_trip_including_every_channel() {
        let a = set();
        a.at(0).set(-6.5);
        a.at(1).set_ch(0, 1.0);
        a.at(1).set_ch(3, -2.5);
        let b = set();
        assert!(load(&b, &save(&a)));
        assert_eq!(b.at(0).get(), -6.5);
        assert_eq!(b.at(1).get_ch(0), 1.0);
        assert_eq!(b.at(1).get_ch(3), -2.5);
        assert_eq!(save(&a), save(&b), "saving twice gives the same bytes");
    }

    #[test]
    fn bad_and_cut_short_states_are_refused_without_changing_anything() {
        let a = set();
        a.at(0).set(3.0);
        let good = save(&a);
        let b = set();
        assert!(!load(&b, b"NOPE0000"));
        assert!(!load(&b, &good[..good.len() - 2]));
        assert!(!load(&b, &[]));
        assert_eq!(b.at(0).get(), 0.0);
    }

    #[test]
    fn a_state_from_a_different_plugin_loads_as_far_as_it_fits() {
        let small = ParamSet::new(&[ParamInfo::knob("gain", "Gain", "dB", -60.0, 12.0, 0.0)]);
        small.at(0).set(4.0);
        let big = set();
        assert!(load(&big, &save(&small)));
        assert_eq!(big.at(0).get(), 4.0);
        let back = ParamSet::new(&[ParamInfo::knob("gain", "Gain", "dB", -60.0, 12.0, 0.0)]);
        assert!(load(&back, &save(&big)));
        assert_eq!(back.at(0).get(), 4.0);
    }

    #[test]
    fn loading_runs_the_listener_once() {
        use std::sync::atomic::{AtomicU32, Ordering};
        use std::sync::Arc;
        let hits = Arc::new(AtomicU32::new(0));
        let a = set();
        let seen = hits.clone();
        a.on_change(move |_| {
            seen.fetch_add(1, Ordering::Relaxed);
        });
        assert!(load(&a, &save(&set())));
        assert_eq!(hits.load(Ordering::Relaxed), 1);
    }
}
