use crate::config::{ConfigSet, TapeConfig};
use crate::stage::TapeStage;
use crate::tables::*;
use ruse_core::{BusInfo, Channels, Handoff, Layout, ParamInfo, ParamSet, Plugin, ProcessContext, Setup};
use std::sync::{Arc, Mutex};

const MAX_TRACKS: usize = 64;

fn db_text(v: f32) -> String {
    format!("{:+.1} dB", v)
}

fn bias_text(v: f32) -> String {
    if v.abs() < 0.5 {
        "Aligned".to_string()
    } else if v < 0.0 {
        format!("Under {:.0}", -v)
    } else {
        format!("Over {:.0}", v)
    }
}

fn percent_text(v: f32) -> String {
    format!("{:.2} %", v)
}

fn hiss_text(v: f32) -> String {
    if v <= -96.0 {
        "Off".to_string()
    } else {
        format!("{:.0} dB", v)
    }
}

// What belongs to each track is per channel. What belongs to the machine every track is on is global.
// Which is which is this plugin's own decision. Another plugin could make speed per channel too.
const INPUT: ParamInfo = ParamInfo {
    group: "Track",
    description: "Level onto tape.",
    display: Some(db_text),
    ..ParamInfo::knob("input", "Input", "dB", -12.0, 18.0, 0.0).per_channel()
};
const BIAS: ParamInfo = ParamInfo {
    group: "Track",
    description: "Under-biased tape is gritty and bright. Over-biased tape is clean in the lows and dull on top.",
    display: Some(bias_text),
    ..ParamInfo::knob("bias", "Bias", "", -100.0, 100.0, 0.0).per_channel()
};
const OUTPUT: ParamInfo = ParamInfo {
    group: "Track",
    description: "Level off tape.",
    display: Some(db_text),
    ..ParamInfo::knob("output", "Output", "dB", -18.0, 12.0, 0.0).per_channel()
};
const LINK: ParamInfo = ParamInfo {
    group: "Track",
    description: "A linked track bleeds into its neighbours and hears them. An unlinked track does neither.",
    ..ParamInfo::toggle("link", "Link", true).per_channel()
};
const FORMAT: ParamInfo = ParamInfo {
    group: "Machine",
    description: "How many tracks share what width of tape. Wider tracks are quieter.",
    ..ParamInfo::choice("format", "Format", &FORMAT_NAMES, 0)
};
const SPEED: ParamInfo = ParamInfo {
    group: "Machine",
    description: "Tape speed. Slower tape runs out of treble headroom first.",
    ..ParamInfo::choice("speed", "Speed", &SPEED_NAMES, 1)
};
const TAPE: ParamInfo = ParamInfo {
    group: "Machine",
    description: "Tape formulation.",
    ..ParamInfo::choice("tape", "Tape", &FORMULA_NAMES, 1)
};
const WOW: ParamInfo = ParamInfo {
    group: "Machine",
    description: "Slow speed drift, as peak deviation.",
    display: Some(percent_text),
    ..ParamInfo::knob("wow", "Wow", "%", 0.0, 1.0, 0.06)
};
const FLUTTER: ParamInfo = ParamInfo {
    group: "Machine",
    description: "Fast speed drift, as peak deviation.",
    display: Some(percent_text),
    ..ParamInfo::knob("flutter", "Flutter", "%", 0.0, 1.0, 0.04)
};
const HISS: ParamInfo = ParamInfo {
    group: "Machine",
    description: "Tape noise, as dBFS RMS for a cassette track at 3.75 IPS on Type I.",
    display: Some(hiss_text),
    ..ParamInfo::knob("hiss", "Hiss", "dB", -96.0, -40.0, -72.0)
};

const PARAMS: [ParamInfo; 10] = [INPUT, BIAS, OUTPUT, LINK, FORMAT, SPEED, TAPE, WOW, FLUTTER, HISS];

const LAYOUT: Layout = Layout {
    inputs: &[BusInfo {
        name: "Tracks",
        channels: Channels::Range { min: 1, max: MAX_TRACKS as u32 },
    }],
    outputs: &[BusInfo {
        name: "Tracks",
        channels: Channels::SameAsInput,
    }],
};

pub struct Preset {
    pub name: &'static str,
    pub settings: TapeSettings,
}

#[allow(clippy::too_many_arguments)]
const fn preset(
    name: &'static str,
    input_db: f32,
    bias: f32,
    speed: usize,
    formula: usize,
    wow: f32,
    flutter: f32,
    hiss_db: f32,
    output_db: f32,
    format: usize,
) -> Preset {
    Preset {
        name,
        settings: TapeSettings {
            input_db,
            bias,
            speed,
            formula,
            wow,
            flutter,
            hiss_db,
            output_db,
            format,
            link: true,
        },
    }
}

pub const PRESETS: [Preset; 6] = [
    preset("Portastudio", 3.0, 0.0, 1, 1, 0.08, 0.05, -68.0, -1.0, 0),
    preset("Worn cassette", 6.0, -0.35, 0, 0, 0.30, 0.15, -58.0, -2.0, 0),
    preset("Clean cassette", 0.0, 0.1, 0, 2, 0.05, 0.04, -74.0, 0.0, 0),
    preset("Half-inch 15", 4.0, 0.0, 3, 1, 0.02, 0.02, -77.0, -1.0, 2),
    preset("Two-inch 30", 2.0, 0.05, 4, 2, 0.01, 0.01, -81.0, 0.0, 5),
    preset("Into the red", 12.0, -0.1, 3, 0, 0.03, 0.02, -73.0, -6.0, 4),
];

/// The settings for one channel: its own track values and the machine's global ones.
fn settings_from(ps: &ParamSet, channel: usize) -> TapeSettings {
    TapeSettings {
        input_db: ps.at(0).get_ch(channel),
        bias: ps.at(1).get_ch(channel) / 100.0,
        output_db: ps.at(2).get_ch(channel),
        link: ps.at(3).get_ch(channel) >= 0.5,
        format: ps.at(4).get().round() as usize,
        speed: ps.at(5).get().round() as usize,
        formula: ps.at(6).get().round() as usize,
        wow: ps.at(7).get(),
        flutter: ps.at(8).get(),
        hiss_db: ps.at(9).get(),
    }
}

/// Builds configs off the audio thread and hands them over.
struct Builder {
    state: Mutex<BuilderState>,
    handoff: Arc<Handoff<ConfigSet>>,
}

struct BuilderState {
    fs: f32,
    last: Vec<Arc<TapeConfig>>,
}

impl Builder {
    fn rebuild(&self, ps: &ParamSet) -> Arc<ConfigSet> {
        let n = ps.channels();
        let mut st = self.state.lock().unwrap_or_else(|e| e.into_inner());
        let mut out: Vec<Arc<TapeConfig>> = Vec::with_capacity(n);
        for c in 0..n {
            let s = settings_from(ps, c);
            // Channels with the same settings share one config.
            if let Some(same) = out.iter().find(|cfg| cfg.settings == s).cloned() {
                out.push(same);
                continue;
            }
            // A channel nothing changed on keeps the config it had.
            if let Some(prev) = st.last.get(c) {
                if prev.settings == s && prev.fs == st.fs {
                    out.push(prev.clone());
                    continue;
                }
            }
            let reuse = st.last.get(c).map(|a| &**a);
            out.push(Arc::new(TapeConfig::new(&s, st.fs, reuse)));
        }
        st.last = out.clone();
        let set = Arc::new(ConfigSet { channels: out });
        self.handoff.post(set.clone());
        set
    }

    fn set_rate(&self, fs: f32) {
        let mut st = self.state.lock().unwrap_or_else(|e| e.into_inner());
        if st.fs != fs {
            st.fs = fs;
            st.last.clear();
        }
    }
}

/// Bias Tape as a plugin: one instance, any number of tracks.
pub struct BiasTape {
    params: ParamSet,
    builder: Arc<Builder>,
    handoff: Arc<Handoff<ConfigSet>>,
    stage: Option<TapeStage>,
}

impl BiasTape {
    pub fn new() -> Self {
        let params = ParamSet::with_channels(&PARAMS, MAX_TRACKS);
        let handoff = Arc::new(Handoff::new());
        let builder = Arc::new(Builder {
            state: Mutex::new(BuilderState {
                fs: 44100.0,
                last: Vec::new(),
            }),
            handoff: handoff.clone(),
        });
        {
            let builder = builder.clone();
            params.on_change(move |ps| {
                builder.rebuild(ps);
            });
        }
        BiasTape {
            params,
            builder,
            handoff,
            stage: None,
        }
    }

    /// The settings on channel 0, which is also every global setting.
    pub fn settings(&self) -> TapeSettings {
        settings_from(&self.params, 0)
    }

    pub fn settings_for(&self, channel: usize) -> TapeSettings {
        settings_from(&self.params, channel)
    }

    /// Loads a factory preset on every track. The link setting stays as it is.
    pub fn apply_preset(&self, index: usize) -> Result<(), String> {
        let p = PRESETS.get(index).ok_or_else(|| format!("no preset {}", index))?;
        let s = &p.settings;
        let ps = &self.params;
        ps.at(0).set(s.input_db);
        ps.at(1).set(s.bias * 100.0);
        ps.at(2).set(s.output_db);
        ps.at(4).set(s.format as f32);
        ps.at(5).set(s.speed as f32);
        ps.at(6).set(s.formula as f32);
        ps.at(7).set(s.wow);
        ps.at(8).set(s.flutter);
        ps.at(9).set(s.hiss_db);
        ps.notify();
        Ok(())
    }
}

impl Default for BiasTape {
    fn default() -> Self {
        Self::new()
    }
}

impl Plugin for BiasTape {
    fn layout(&self) -> Layout {
        LAYOUT
    }

    fn params(&self) -> &ParamSet {
        &self.params
    }

    fn prepare(&mut self, setup: &Setup) {
        let channels = (setup.channels as usize).clamp(1, MAX_TRACKS);
        self.params.set_channels(channels);
        self.builder.set_rate(setup.sample_rate);
        let set = self.builder.rebuild(&self.params);
        // The stage starts on this set, so the copy waiting in the handoff is not needed.
        let _ = self.handoff.take();
        self.stage = Some(TapeStage::new(setup.sample_rate, channels, self.handoff.clone(), set));
    }

    fn latency_samples(&self) -> u32 {
        self.stage.as_ref().map(|s| s.latency_samples()).unwrap_or(0)
    }

    fn tail_samples(&self) -> u32 {
        // Bias reports 0.05 s. The framework does not pass the rate here yet.
        0
    }

    fn process(&mut self, channels: &mut [&mut [f32]], ctx: &ProcessContext) {
        if let Some(stage) = self.stage.as_mut() {
            stage.process(channels, ctx.sample_time);
        }
    }
}
