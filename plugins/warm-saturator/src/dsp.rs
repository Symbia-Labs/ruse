use crate::Params;
use muse_core::{ProcessContext, Setup};

pub struct State {
    sample_rate: f32,
    low: Vec<f32>,
}

impl State {
    pub fn new() -> Self {
        State {
            sample_rate: 48000.0,
            low: Vec::new(),
        }
    }

    pub fn prepare(&mut self, setup: &Setup) {
        self.sample_rate = setup.sample_rate;
        self.low = vec![0.0; setup.channels as usize];
    }

    pub fn latency_samples(&self) -> u32 {
        0
    }

    pub fn tail_samples(&self) -> u32 {
        0
    }

    pub fn process(&mut self, p: &Params, channels: &mut [&mut [f32]], _ctx: &ProcessContext) {
        if p.bypass() {
            return;
        }
        let gain = 10f32.powf(p.drive() / 20.0);
        let a = 1.0 - (-2.0 * std::f32::consts::PI * p.tone() / self.sample_rate).exp();
        let hard = p.mode() == 1;
        for (ch, low) in channels.iter_mut().zip(self.low.iter_mut()) {
            for s in ch.iter_mut() {
                let x = *s * gain;
                let y = if hard { x.clamp(-1.0, 1.0) } else { x.tanh() };
                *low += a * (y - *low);
                *s = *low;
            }
        }
    }
}

impl Default for State {
    fn default() -> Self {
        Self::new()
    }
}
