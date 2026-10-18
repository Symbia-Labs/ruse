//! Running a plugin offline, the way a host would, without a DAW.

use ruse_core::{Plugin, ProcessContext, Setup};

/// One parameter write to apply before rendering. `channel` is `None` to write every channel.
#[derive(Clone, Debug)]
pub struct Setting {
    pub id: String,
    pub channel: Option<usize>,
    pub value: f32,
}

#[derive(Clone, Debug)]
pub struct Options {
    pub sample_rate: f32,
    pub block: usize,
    pub settings: Vec<Setting>,
    /// Keep rendering silence after the input ends, for as long as the plugin reports a tail (up to ten seconds).
    pub tail: bool,
    /// What plays into the sidechain of a plugin that has one, one vector per channel. Silence where it runs out,
    /// and the last channel given repeats when the sidechain has more. Empty is silence.
    pub sidechain: Vec<Vec<f32>>,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            sample_rate: 48000.0,
            block: 128,
            settings: Vec::new(),
            tail: true,
            sidechain: Vec::new(),
        }
    }
}

pub struct Rendered {
    pub output: Vec<Vec<f32>>,
    pub latency: u32,
    pub tail: u32,
    /// How many frames of the output line up with the input. Frames after this are the tail.
    pub frames_in: usize,
}

/// What a host does before processing: settle the channel count, apply settings, prepare.
pub fn configure(p: &mut dyn Plugin, channels: usize, opts: &Options) -> Result<(), String> {
    let n = channels as u32;
    if !p.layout().accepts_main(n) {
        return Err(format!(
            "the plugin does not accept {} channels on its main bus, with the same number out",
            channels
        ));
    }
    p.params().set_channels(channels);
    for s in &opts.settings {
        p.params().set_plain_ch(&s.id, s.channel, s.value)?;
    }
    p.prepare(&Setup {
        sample_rate: opts.sample_rate,
        max_block: opts.block,
        channels: n,
    });
    Ok(())
}

/// The sidechain audio for `channels` channels of `frames` frames, from what `given` holds.
pub fn sidechain_input(channels: usize, given: &[Vec<f32>], frames: usize) -> Vec<Vec<f32>> {
    (0..channels)
        .map(|c| {
            let mut v = match given.len() {
                0 => Vec::new(),
                n => given[c.min(n - 1)].clone(),
            };
            v.resize(frames, 0.0);
            v
        })
        .collect()
}

pub fn render(p: &mut dyn Plugin, input: &[Vec<f32>], opts: &Options) -> Result<Rendered, String> {
    if input.is_empty() {
        return Err("no input channels".to_string());
    }
    if opts.block == 0 {
        return Err("the block size must be at least 1".to_string());
    }
    configure(p, input.len(), opts)?;
    let latency = p.latency_samples();
    let tail = p.tail_samples();
    let extra = if opts.tail {
        (tail as usize).min(10 * opts.sample_rate as usize)
    } else {
        0
    };
    let frames_in = input[0].len();
    let total = frames_in + extra;
    let mut out: Vec<Vec<f32>> = input
        .iter()
        .map(|c| {
            let mut v = c.clone();
            v.resize(total, 0.0);
            v
        })
        .collect();
    let keyed = p.layout().sidechain_channels(input.len() as u32);
    let key = sidechain_input(keyed.unwrap_or(0) as usize, &opts.sidechain, total);
    let mut pos = 0usize;
    while pos < total {
        let len = opts.block.min(total - pos);
        let mut chans: Vec<&mut [f32]> = out.iter_mut().map(|c| &mut c[pos..pos + len]).collect();
        let ctx = ProcessContext {
            sample_rate: opts.sample_rate,
            sample_time: Some(pos as f64),
            playing: true,
            tempo_bpm: None,
        };
        if keyed.is_some() {
            let keys: Vec<&[f32]> = key.iter().map(|c| &c[pos..pos + len]).collect();
            p.process_with_sidechain(&mut chans, &keys, &ctx);
        } else {
            p.process(&mut chans, &ctx);
        }
        pos += len;
    }
    Ok(Rendered {
        output: out,
        latency,
        tail,
        frames_in,
    })
}
