use plugkit_core::{BusInfo, Channels, Layout, ParamInfo, ParamSet, Plugin, ProcessContext, Setup};

pub const GAIN: ParamInfo = ParamInfo {
    description: "Level change applied to every channel.",
    group: "Output",
    ..ParamInfo::knob("gain", "Gain", "dB", -60.0, 12.0, 0.0)
};

const LAYOUT: Layout = Layout {
    inputs: &[BusInfo {
        name: "Main",
        channels: Channels::Range { min: 1, max: 64 },
    }],
    outputs: &[BusInfo {
        name: "Main",
        channels: Channels::SameAsInput,
    }],
};

pub struct Gain {
    params: ParamSet,
}

impl Gain {
    pub fn new() -> Self {
        Gain {
            params: ParamSet::new(&[GAIN]),
        }
    }
}

impl Default for Gain {
    fn default() -> Self {
        Self::new()
    }
}

impl Plugin for Gain {
    fn layout(&self) -> Layout {
        LAYOUT
    }

    fn params(&self) -> &ParamSet {
        &self.params
    }

    fn prepare(&mut self, _setup: &Setup) {}

    fn process(&mut self, channels: &mut [&mut [f32]], _ctx: &ProcessContext) {
        let g = 10f32.powf(self.params.at(0).get() / 20.0);
        for ch in channels.iter_mut() {
            for s in ch.iter_mut() {
                *s *= g;
            }
        }
    }
}
