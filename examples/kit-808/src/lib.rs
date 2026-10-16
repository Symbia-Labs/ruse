//! MUSE 808: an 808-style drum machine instrument. MIDI notes on the General MIDI drum
//! map play synthesized voices; a closed hat chokes a ringing open hat.
#![forbid(unsafe_code)]

pub mod voice;

use muse_core::{BusInfo, Channels, Layout, ParamInfo, ParamSet, Plugin, ProcessContext, Setup};
use voice::{Drum, Shape, Voice};

const LAYOUT: Layout = Layout {
    inputs: &[],
    outputs: &[BusInfo {
        name: "Main",
        channels: Channels::Range { min: 1, max: 2 },
    }],
};

pub const PARAMS: [ParamInfo; 5] = [
    ParamInfo {
        description: "Output level of the whole kit.",
        group: "Kit",
        ..ParamInfo::knob("level", "Level", "dB", -24.0, 6.0, 0.0)
    },
    ParamInfo {
        description: "Pitch of the kick, snare tone, toms, rim and cowbell, in semitones.",
        group: "Kit",
        ..ParamInfo::knob("tune", "Tune", "st", -12.0, 12.0, 0.0)
    },
    ParamInfo {
        description: "How long the kick rings, as a share of its normal length.",
        group: "Kick",
        ..ParamInfo::knob("kick_decay", "Kick decay", "%", 25.0, 400.0, 100.0)
    },
    ParamInfo {
        description: "Level of the snare wires against the drum's tone.",
        group: "Snare",
        ..ParamInfo::knob("snappy", "Snappy", "%", 0.0, 100.0, 80.0)
    },
    ParamInfo {
        description: "How long the hats, ride and crash ring, as a share of their normal length.",
        group: "Cymbals",
        ..ParamInfo::knob("hat_decay", "Hat decay", "%", 50.0, 200.0, 100.0)
    },
];

const VOICES: usize = 32;
const PENDING: usize = 128;

#[derive(Clone, Copy)]
struct Hit {
    offset: u32,
    drum: Drum,
    velocity: f32,
}

pub struct Kit808 {
    params: ParamSet,
    rate: f32,
    voices: Vec<Voice>,
    pending: [Option<Hit>; PENDING],
    count: usize,
    silence_all: bool,
}

impl Kit808 {
    pub fn new() -> Self {
        Kit808 {
            params: ParamSet::new(&PARAMS),
            rate: 48_000.0,
            voices: Vec::with_capacity(VOICES),
            pending: [None; PENDING],
            count: 0,
            silence_all: false,
        }
    }

    fn shape(&self) -> Shape {
        let p = |id| self.params.by_id(id).map(|p| p.get()).unwrap_or(0.0);
        Shape {
            tune: 2f32.powf(p("tune") / 12.0),
            kick_decay: p("kick_decay") / 100.0,
            snappy: p("snappy") / 100.0,
            hat_decay: p("hat_decay") / 100.0,
        }
    }

    fn start(&mut self, drum: Drum, velocity: f32, shape: Shape) {
        if drum == Drum::ClosedHat {
            for v in self.voices.iter_mut().filter(|v| v.drum == Drum::OpenHat) {
                v.choke();
            }
        }
        if self.voices.len() == VOICES {
            self.voices.remove(0);
        }
        self.voices.push(Voice::new(drum, velocity, self.rate, shape));
    }
}

impl Default for Kit808 {
    fn default() -> Self {
        Self::new()
    }
}

impl Plugin for Kit808 {
    fn layout(&self) -> Layout {
        LAYOUT
    }

    fn params(&self) -> &ParamSet {
        &self.params
    }

    fn prepare(&mut self, setup: &Setup) {
        self.rate = setup.sample_rate;
        self.voices.clear();
        self.count = 0;
    }

    fn is_instrument(&self) -> bool {
        true
    }

    fn midi(&mut self, status: u8, data1: u8, data2: u8, offset: u32) {
        match status & 0xF0 {
            0x90 if data2 > 0 => {
                if let Some(drum) = Drum::for_note(data1) {
                    if self.count < PENDING {
                        self.pending[self.count] = Some(Hit { offset, drum, velocity: data2 as f32 / 127.0 });
                        self.count += 1;
                    }
                }
            }
            // All sound off and all notes off silence the kit.
            0xB0 if data1 == 120 || data1 == 123 => self.silence_all = true,
            _ => {}
        }
    }

    fn process(&mut self, channels: &mut [&mut [f32]], _ctx: &ProcessContext) {
        if self.silence_all {
            for v in self.voices.iter_mut() {
                v.choke();
            }
            self.silence_all = false;
        }
        let shape = self.shape();
        let gain = 10f32.powf(self.params.by_id("level").map(|p| p.get()).unwrap_or(0.0) / 20.0) * 0.5;
        let frames = channels.first().map(|c| c.len()).unwrap_or(0);
        // Hits in the order they fall in the block.
        self.pending[..self.count].sort_by_key(|h| h.map(|h| h.offset).unwrap_or(u32::MAX));
        let mut next = 0;
        for i in 0..frames {
            while next < self.count {
                match self.pending[next] {
                    Some(h) if h.offset as usize <= i => {
                        self.start(h.drum, h.velocity, shape);
                        next += 1;
                    }
                    _ => break,
                }
            }
            let mut sum = 0.0;
            for v in self.voices.iter_mut() {
                match v.sample() {
                    Some(x) => sum += x,
                    None => v.choke_now(),
                }
            }
            let s = (sum * gain).clamp(-1.0, 1.0);
            for c in channels.iter_mut() {
                c[i] = s;
            }
        }
        // Hits past the end of a short block start at the end of it.
        while next < self.count {
            if let Some(h) = self.pending[next] {
                self.start(h.drum, h.velocity, shape);
            }
            next += 1;
        }
        self.count = 0;
        self.voices.retain(|v| !v.done());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn render(kit: &mut Kit808, frames: usize) -> Vec<f32> {
        let mut l = vec![0.0f32; frames];
        let mut r = vec![0.0f32; frames];
        {
            let mut chans: Vec<&mut [f32]> = vec![&mut l, &mut r];
            kit.process(&mut chans, &ProcessContext::offline(48_000.0));
        }
        l
    }

    fn prepared() -> Kit808 {
        let mut k = Kit808::new();
        k.prepare(&Setup { sample_rate: 48_000.0, max_block: 512, channels: 2 });
        k
    }

    #[test]
    fn silent_until_a_note() {
        let mut k = prepared();
        assert!(render(&mut k, 512).iter().all(|s| *s == 0.0));
    }

    #[test]
    fn a_kick_note_sounds_at_its_offset() {
        let mut k = prepared();
        k.midi(0x99, 36, 127, 100);
        let out = render(&mut k, 512);
        assert!(out[..100].iter().all(|s| *s == 0.0), "sound before the offset");
        assert!(out[100..].iter().any(|s| s.abs() > 0.01), "no sound after it");
    }

    #[test]
    fn note_offs_and_unknown_notes_are_ignored() {
        let mut k = prepared();
        k.midi(0x89, 36, 0, 0);
        k.midi(0x99, 60, 100, 0);
        k.midi(0x99, 36, 0, 0);
        assert!(render(&mut k, 512).iter().all(|s| *s == 0.0));
    }

    #[test]
    fn closed_hat_chokes_open_hat() {
        let mut k = prepared();
        k.midi(0x99, 46, 127, 0);
        render(&mut k, 4800);
        k.midi(0x99, 42, 127, 0);
        for _ in 0..10 {
            render(&mut k, 4800);
        }
        assert!(k.voices.iter().all(|v| v.drum != Drum::OpenHat));
    }

    #[test]
    fn all_sound_off_silences() {
        let mut k = prepared();
        k.midi(0x99, 49, 127, 0);
        render(&mut k, 4800);
        k.midi(0xB9, 120, 0, 0);
        for _ in 0..10 {
            render(&mut k, 4800);
        }
        assert!(k.voices.is_empty());
    }

    #[test]
    fn instrument_with_outputs_only() {
        let k = Kit808::new();
        assert!(k.is_instrument());
        assert!(k.layout().inputs.is_empty());
        assert!(k.layout().accepts(&[], &[2]));
    }
}
