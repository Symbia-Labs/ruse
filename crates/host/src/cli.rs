//! The command line a generated plugin crate exposes as its `render` binary.
//!
//! ```text
//! render [--signal SPEC | --in FILE.wav] [--channels N] [--seconds S] [--rate HZ] [--block N]
//!        [--set id=value | --set id@channel=value]... [--out FILE.wav] [--no-tail] [--describe]
//! ```

use crate::measure::{self, BAND_HZ};
use crate::render::{self, Options, Setting};
use crate::{signal, wav};
use muse_core::{Plugin, Scope};
use std::path::Path;

pub fn render_main<P: Plugin>(name: &str, make: impl Fn() -> P) {
    match run(name, &make, std::env::args().skip(1).collect()) {
        Ok(text) => println!("{}", text),
        Err(e) => {
            eprintln!("error: {}", e);
            std::process::exit(2);
        }
    }
}

fn next(args: &[String], i: &mut usize, flag: &str) -> Result<String, String> {
    *i += 1;
    args.get(*i).cloned().ok_or_else(|| format!("{} needs a value", flag))
}

fn number<T: std::str::FromStr>(text: &str, flag: &str) -> Result<T, String> {
    text.parse::<T>()
        .map_err(|_| format!("{} needs a number, not '{}'", flag, text))
}

fn db_text(v: f32) -> String {
    if v <= -200.0 {
        "-inf".to_string()
    } else {
        format!("{:+.1}", v)
    }
}

pub fn run<P: Plugin>(name: &str, make: &dyn Fn() -> P, args: Vec<String>) -> Result<String, String> {
    let mut signal_spec = "noise:-18".to_string();
    let mut input_path: Option<String> = None;
    let mut channels = 2usize;
    let mut seconds = 2.0f32;
    let mut rate = 48000.0f32;
    let mut block = 128usize;
    let mut settings: Vec<Setting> = Vec::new();
    let mut out_path: Option<String> = None;
    let mut tail = true;
    let mut describe = false;

    let mut i = 0;
    while i < args.len() {
        let flag = args[i].clone();
        match flag.as_str() {
            "--signal" => signal_spec = next(&args, &mut i, &flag)?,
            "--in" => input_path = Some(next(&args, &mut i, &flag)?),
            "--channels" => channels = number(&next(&args, &mut i, &flag)?, &flag)?,
            "--seconds" => seconds = number(&next(&args, &mut i, &flag)?, &flag)?,
            "--rate" => rate = number(&next(&args, &mut i, &flag)?, &flag)?,
            "--block" => block = number(&next(&args, &mut i, &flag)?, &flag)?,
            "--out" => out_path = Some(next(&args, &mut i, &flag)?),
            "--no-tail" => tail = false,
            "--describe" => describe = true,
            "--set" => {
                let text = next(&args, &mut i, &flag)?;
                let (key, value) = text
                    .split_once('=')
                    .ok_or("--set needs id=value or id@channel=value, with channels counted from 0")?;
                let value: f32 = number(value, "--set")?;
                let (id, channel) = match key.split_once('@') {
                    Some((id, c)) => (id.to_string(), Some(number::<usize>(c, "--set")?)),
                    None => (key.to_string(), None),
                };
                settings.push(Setting { id, channel, value });
            }
            other => return Err(format!("unknown option '{}'", other)),
        }
        i += 1;
    }

    if describe {
        let p = make();
        p.params().set_channels(channels);
        for s in &settings {
            p.params().set_plain_ch(&s.id, s.channel, s.value)?;
        }
        return Ok(muse_core::json::describe_params(p.params()));
    }

    let (input, rate, label) = match &input_path {
        Some(path) => {
            let w = wav::read(Path::new(path))?;
            let label = path.clone();
            (w.channels, w.sample_rate as f32, label)
        }
        None => {
            let sig = signal::parse(&signal_spec)?;
            let frames = (seconds * rate) as usize;
            (signal::generate(&sig, rate, frames, channels), rate, sig.describe())
        }
    };
    let n = input.len();
    let frames_in = input[0].len();

    let mut p = make();
    let opts = Options {
        sample_rate: rate,
        block,
        settings,
        tail,
    };
    let r = render::render(&mut p, &input, &opts)?;

    let out_file = out_path.unwrap_or_else(|| format!("renders/{}.wav", name));
    if let Some(dir) = Path::new(&out_file).parent() {
        if !dir.as_os_str().is_empty() {
            std::fs::create_dir_all(dir).map_err(|e| format!("{}: {}", dir.display(), e))?;
        }
    }
    wav::write(Path::new(&out_file), rate as u32, &r.output)?;

    let mut o = String::new();
    o.push_str(&format!("render of {}\n", name));
    o.push_str(&format!(
        "input: {}, {} ch, {} Hz, {:.2} s, block {}\n",
        label,
        n,
        rate,
        frames_in as f32 / rate,
        block
    ));
    o.push_str("parameters:\n");
    for param in p.params().iter() {
        let info = param.info();
        let text = if info.scope == Scope::PerChannel {
            let texts: Vec<String> = (0..n).map(|c| param.text_ch(c)).collect();
            if texts.iter().all(|t| *t == texts[0]) {
                texts[0].clone()
            } else {
                texts.join(", ")
            }
        } else {
            param.text()
        };
        o.push_str(&format!("  {} = {}\n", info.id, text));
    }
    o.push_str(&format!(
        "latency {} samples, tail {} samples{}\n",
        r.latency,
        r.tail,
        if tail && r.output[0].len() > frames_in {
            format!(", {} samples of tail rendered", r.output[0].len() - frames_in)
        } else {
            String::new()
        }
    ));
    o.push_str("levels in dBFS, over the length of the input:\n");
    for (c, (i_full, o_full)) in input.iter().zip(&r.output).enumerate().take(8) {
        let i_ch = &i_full[..frames_in];
        let o_ch = &o_full[..frames_in];
        o.push_str(&format!(
            "  channel {}: in peak {} rms {} | out peak {} rms {} | rms change {} dB | dc {:+.4} | non-finite {}\n",
            c + 1,
            db_text(measure::peak_db(i_ch)),
            db_text(measure::rms_db(i_ch)),
            db_text(measure::peak_db(o_ch)),
            db_text(measure::rms_db(o_ch)),
            db_text(measure::rms_db(o_ch) - measure::rms_db(i_ch)),
            measure::dc(o_ch),
            measure::nonfinite(o_full)
        ));
    }
    if n > 8 {
        o.push_str(&format!("  ({} more channels not listed)\n", n - 8));
    }
    let bands_in = measure::band_levels_db(&input[0][..frames_in], rate);
    let bands_out = measure::band_levels_db(&r.output[0][..frames_in], rate);
    let labels = ["31", "63", "125", "250", "500", "1k", "2k", "4k", "8k", "16k"];
    debug_assert_eq!(labels.len(), BAND_HZ.len());
    o.push_str("octave bands on channel 1, mean-square level in dBFS:\n");
    o.push_str(&format!("  {:<5}", "Hz"));
    for l in labels {
        o.push_str(&format!("{:>7}", l));
    }
    o.push('\n');
    let row = |title: &str, values: Vec<String>| {
        let mut s = format!("  {:<5}", title);
        for v in values {
            s.push_str(&format!("{:>7}", v));
        }
        s.push('\n');
        s
    };
    o.push_str(&row("in", bands_in.iter().map(|v| db_text(*v)).collect()));
    o.push_str(&row("out", bands_out.iter().map(|v| db_text(*v)).collect()));
    o.push_str(&row(
        "diff",
        bands_in
            .iter()
            .zip(&bands_out)
            .map(|(a, b)| {
                if *a <= -200.0 || *b <= -200.0 {
                    "-".to_string()
                } else {
                    format!("{:+.1}", b - a)
                }
            })
            .collect(),
    ));
    o.push_str(&format!("wrote {}", out_file));
    Ok(o)
}
