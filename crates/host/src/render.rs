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
}

impl Default for Options {
    fn default() -> Self {
        Options {
            sample_rate: 48000.0,
            block: 128,
            settings: Vec::new(),
            tail: true,
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
    if !p.layout().accepts(&[n], &[n]) {
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
        p.process(&mut chans, &ctx);
        pos += len;
    }
    Ok(Rendered {
        output: out,
        latency,
        tail,
        frames_in,
    })
}
