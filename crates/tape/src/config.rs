use crate::filters::{bump, half_band_gain, head_loss, minimum_phase};
use crate::hysteresis::{langevin, Hysteresis};
use crate::tables::*;
use crate::transport;
use std::f32::consts::PI;
use std::sync::Arc;

/// One config for each channel. Channels with identical settings share one. The machine-wide
/// settings (wow, flutter, speed and the rest) are global, so they are the same in every entry.
pub struct ConfigSet {
    pub channels: Vec<Arc<TapeConfig>>,
}

impl ConfigSet {
    pub fn uniform(config: Arc<TapeConfig>, channels: usize) -> Self {
        ConfigSet {
            channels: vec![config; channels.max(1)],
        }
    }

    /// The config that carries the machine-wide settings.
    pub fn machine(&self) -> &TapeConfig {
        &self.channels[0]
    }

    pub fn for_channel(&self, channel: usize) -> &TapeConfig {
        &self.channels[channel.min(self.channels.len() - 1)]
    }
}

/// Everything worked out from the settings. Built off the audio thread whenever a
/// setting changes; the audio thread only reads it.
pub struct TapeConfig {
    pub settings: TapeSettings,
    pub fs: f32,
    pub drive: f32,
    pub makeup: f32,
    pub output: f32,
    pub hiss: f32,
    pub noise_pole: f32,
    pub noise_low: f32,
    pub noise_norm: f32,
    /// What comes across from the tracks either side, and the pole of the low part
    /// added on top so the bottom comes across hardest. Zero when unlinked.
    pub bleed: f32,
    pub bleed_pole: f32,
    pub k: f32,
    pub c: f32,
    pub alpha: f32,
    /// Field at full scale with the input at 0 dB, in units of the knee.
    pub field_scale: f32,
    /// Record emphasis, y = b0 x + b1 x' - a1 y'. Playback undoes it with the same numbers swapped.
    pub emphasis_b0: f32,
    pub emphasis_b1: f32,
    pub emphasis_a1: f32,
    pub playback: Vec<f32>,
    pub biquads: [f32; 10],
    pub wow: f64,
    pub flutter: f64,
    pub capstan: f64,
    /// Samples of delay the wow and flutter need to swing around.
    pub base_delay: f32,
}

impl TapeConfig {
    pub fn new(s: &TapeSettings, fs: f32, reuse: Option<&TapeConfig>) -> TapeConfig {
        let formula = s.formula.min(FORMULA_NAMES.len() - 1);
        let speed_index = s.speed.min(SPEEDS_IPS.len() - 1);
        let ips = SPEEDS_IPS[speed_index];
        let k = COERCIVITY[formula];
        // Bias is what makes tape linear: aligned, the tape all but follows its
        // anhysteretic curve and only saturates at the top. Starved of bias the
        // loop opens up and quiet signals go gritty.
        let c = if s.bias <= 0.0 {
            0.96 + 0.56 * s.bias.max(-1.0)
        } else {
            0.96 + 0.03 * s.bias.min(1.0)
        };
        let alpha = 0.02f32;

        let tc = EQUALIZATION_MICROS[speed_index][formula] * 1e-6;
        // The emphasis shelf's gain at f Hz.
        let emphasis = |f: f32| {
            let x = 2.0 * PI * f * tc;
            ((1.0 + x * x) / (1.0 + x * x / (EMPHASIS_MAX * EMPHASIS_MAX))).sqrt()
        };

        // The field at full scale and the gain back to signal level depend on the
        // formulation, the bias and the speed's equalization, so a level or
        // wow-and-flutter change reuses them.
        let (field_scale, makeup) = match reuse {
            Some(r)
                if r.settings.formula == s.formula
                    && r.settings.bias == s.bias
                    && r.settings.speed == s.speed
                    && r.fs == fs =>
            {
                (r.field_scale, r.makeup)
            }
            _ => Self::calibrate(k, c, alpha, fs, MOL_DBFS[formula], emphasis(1000.0), emphasis(1000.0) / emphasis(3000.0)),
        };

        let drive = 10f32.powf(s.input_db / 20.0) * field_scale;
        let output = 10f32.powf(s.output_db / 20.0);
        let format = s.format.min(FORMAT_NAMES.len() - 1);
        let hiss_db = s.hiss_db + HISS_OFFSET_DB[formula]
            - 3.0 * (ips / 3.75).log2()
            - 3.0 * (TRACK_WIDTH_MM[format] / TRACK_WIDTH_MM[0]).log2();
        let hiss = if s.hiss_db <= -96.0 { 0.0 } else { 10f32.powf(hiss_db / 20.0) };

        // The table's figure is for 1 kHz, so take out what the low part adds there.
        let corner = 200.0f32;
        let ratio = 1000.0 / corner;
        let re = 1.0 + BLEED_LOW / (1.0 + ratio * ratio);
        let im = BLEED_LOW * ratio / (1.0 + ratio * ratio);
        let bleed = if s.link {
            10f32.powf(BLEED_DB[format] / 20.0) / (re * re + im * im).sqrt()
        } else {
            0.0
        };
        let bleed_pole = 1.0 - (-2.0 * PI * corner / fs).exp();

        // White noise plus the same noise through a one-pole low-pass at 150 Hz,
        // the low part at half the white part's level; the sum scaled back to unit RMS.
        let pole = 1.0 - (-2.0 * PI * 150.0 / fs).exp();
        let low_ratio = (pole / (2.0 - pole)).sqrt();
        let low = 0.5 / low_ratio;
        let variance = 0.85 * 0.85 + 0.25 + 2.0 * 0.85 * low * pole;
        let noise_norm = (3.0 / variance).sqrt();

        // Head geometry: a cassette-sized head at the slow speeds, a studio head above.
        let slow = ips < 7.0;
        let speed = ips * 0.0254;
        let spacing: f32 = if slow { 0.2e-6 } else { 0.5e-6 };
        let gap: f32 = if slow {
            1.0e-6
        } else if ips < 12.0 {
            2.5e-6
        } else {
            4.0e-6
        };
        // What is left of the head's losses once the machine is aligned: spacing and gap.
        let loss = |f: f32| head_loss(f, speed, spacing, 0.0, gap);
        // Bias moves the top end: under-biased tape is brighter, over-biased tape erases its own treble.
        let tilt_db = -4.0 * s.bias;
        let tilt = |f: f32| 10f32.powf(tilt_db * (f / 10000.0).min(2.0) / 20.0);
        // The 2x stage's own filters droop towards the top of the band; the playback side gives that back.
        let droop = |f: f32| {
            let g = half_band_gain(f / (2.0 * fs));
            (1.0 / (g * g).max(1e-6)).min(2.0)
        };
        // Emphasis on the way in and its exact inverse on the way out: a first-order
        // shelf (1 + sT) / (1 + sT/G), bilinear.
        let kk = 2.0 * fs * tc;
        let a0 = 1.0 + kk / EMPHASIS_MAX;
        let emphasis_b0 = (1.0 + kk) / a0;
        let emphasis_b1 = (1.0 - kk) / a0;
        let emphasis_a1 = (1.0 - kk / EMPHASIS_MAX) / a0;
        let playback = minimum_phase(FIR_TAPS, fs, |f| loss(f) * tilt(f) * droop(f));
        let biquads = bump(fs, ips);

        let wow = f64::from(s.wow.max(0.0)) / 100.0;
        let flutter = f64::from(s.flutter.max(0.0)) / 100.0;
        let capstan = f64::from(CAPSTAN_HZ[speed_index]);
        let reach = transport::reach(wow, flutter, capstan);
        let base_delay = if reach > 0.0 { reach as f32 * fs + 3.0 } else { 0.0 };

        // Touch the table now, so the audio thread never builds it.
        let _ = langevin();

        TapeConfig {
            settings: *s,
            fs,
            drive,
            makeup,
            output,
            hiss,
            noise_pole: pole,
            noise_low: low,
            noise_norm,
            bleed,
            bleed_pole,
            k,
            c,
            alpha,
            field_scale,
            emphasis_b0,
            emphasis_b1,
            emphasis_a1,
            playback,
            biquads,
            wow,
            flutter,
            capstan,
            base_delay,
        }
    }

    /// Latency at these settings, in samples at the stage's rate.
    pub fn latency(&self) -> f32 {
        FIXED_LATENCY as f32 + self.base_delay
    }

    /// Third-harmonic distortion and gain of the bare hysteresis for a sine of peak field `field`.
    pub fn probe(field: f32, k: f32, c: f32, alpha: f32, fs2: f32) -> (f32, f32) {
        let lan = langevin();
        let mut st = Hysteresis::default();
        let period = 64usize;
        let t = 1.0 / fs2;
        let (mut s1, mut c1, mut s3, mut c3) = (0.0f32, 0.0f32, 0.0f32, 0.0f32);
        let (settle, measure) = (6usize, 4usize);
        for n in 0..period * (settle + measure) {
            let ph = 2.0 * PI * (n % period) as f32 / period as f32;
            let y = st.step(field * ph.sin(), t, k, c, alpha, &lan.l, &lan.d, lan.scale);
            if n >= period * settle {
                s1 += y * ph.sin();
                c1 += y * ph.cos();
                s3 += y * (3.0 * ph).sin();
                c3 += y * (3.0 * ph).cos();
            }
        }
        let norm = 2.0 / (period * measure) as f32;
        let a1 = (s1 * s1 + c1 * c1).sqrt() * norm;
        let a3 = (s3 * s3 + c3 * c3).sqrt() * norm;
        (a1 / field, if a1 > 0.0 { a3 / a1 } else { 0.0 })
    }

    /// The field that full scale stands for, so 3% third harmonic falls at the
    /// formulation's level, and the gain that brings a quiet signal back to where it came in.
    /// `hot`: how much the record emphasis lifts the test tone; `third`: how much
    /// playback turns its third harmonic down against it.
    pub fn calibrate(k: f32, c: f32, alpha: f32, fs: f32, mol_dbfs: f32, hot: f32, third: f32) -> (f32, f32) {
        let fs2 = 2.0 * fs;
        let (mut lo, mut hi) = (0.05f32, 12.0f32);
        for _ in 0..18 {
            let mid = (lo * hi).sqrt();
            if Self::probe(mid * hot, k, c, alpha, fs2).1 * third < 0.03 {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        let field_at_mol = (lo * hi).sqrt();
        let field_scale = field_at_mol / 10f32.powf(mol_dbfs / 20.0);
        let quiet = Self::probe(field_scale * 0.0316, k, c, alpha, fs2).0;
        (field_scale, if quiet > 0.0 { 1.0 / (quiet * field_scale) } else { 1.0 })
    }
}
