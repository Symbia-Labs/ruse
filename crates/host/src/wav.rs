//! Reading and writing WAV files, with no dependencies.
//!
//! Reads 16, 24 and 32-bit integer PCM and 32-bit float. Writes 32-bit float.

use std::path::Path;

pub struct Wav {
    pub sample_rate: u32,
    pub channels: Vec<Vec<f32>>,
}

fn u16le(b: &[u8], i: usize) -> u16 {
    u16::from_le_bytes([b[i], b[i + 1]])
}

fn u32le(b: &[u8], i: usize) -> u32 {
    u32::from_le_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]])
}

pub fn read(path: &Path) -> Result<Wav, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("{}: {}", path.display(), e))?;
    parse(&bytes)
}

pub fn parse(b: &[u8]) -> Result<Wav, String> {
    if b.len() < 12 || &b[0..4] != b"RIFF" || &b[8..12] != b"WAVE" {
        return Err("not a RIFF/WAVE file".to_string());
    }
    let mut i = 12;
    let mut fmt: Option<(u16, u16, u32, u16)> = None;
    let mut data: Option<&[u8]> = None;
    while i + 8 <= b.len() {
        let id = &b[i..i + 4];
        let size = u32le(b, i + 4) as usize;
        let start = i + 8;
        let end = (start + size).min(b.len());
        if id == b"fmt " {
            if end - start < 16 {
                return Err("the fmt chunk is too short".to_string());
            }
            let mut tag = u16le(b, start);
            let channels = u16le(b, start + 2);
            let rate = u32le(b, start + 4);
            let bits = u16le(b, start + 14);
            if tag == 0xFFFE && end - start >= 26 {
                tag = u16le(b, start + 24);
            }
            fmt = Some((tag, channels, rate, bits));
        } else if id == b"data" {
            data = Some(&b[start..end]);
        }
        i = start + size + (size & 1);
    }
    let (tag, channels, rate, bits) = fmt.ok_or("no fmt chunk")?;
    let data = data.ok_or("no data chunk")?;
    if channels == 0 {
        return Err("the file has zero channels".to_string());
    }
    let width = (bits / 8) as usize;
    if width == 0 {
        return Err("the file has zero bits per sample".to_string());
    }
    let n = channels as usize;
    let frames = data.len() / (width * n);
    let mut out: Vec<Vec<f32>> = (0..n).map(|_| Vec::with_capacity(frames)).collect();
    for f in 0..frames {
        for (c, ch) in out.iter_mut().enumerate() {
            let o = (f * n + c) * width;
            let v = match (tag, bits) {
                (1, 16) => i16::from_le_bytes([data[o], data[o + 1]]) as f32 / 32768.0,
                (1, 24) => {
                    let x = ((data[o] as i32) << 8) | ((data[o + 1] as i32) << 16) | ((data[o + 2] as i32) << 24);
                    (x >> 8) as f32 / 8_388_608.0
                }
                (1, 32) => i32::from_le_bytes([data[o], data[o + 1], data[o + 2], data[o + 3]]) as f32 / 2_147_483_648.0,
                (3, 32) => f32::from_le_bytes([data[o], data[o + 1], data[o + 2], data[o + 3]]),
                _ => return Err(format!("unsupported WAV format: tag {} with {} bits", tag, bits)),
            };
            ch.push(v);
        }
    }
    Ok(Wav {
        sample_rate: rate,
        channels: out,
    })
}

pub fn write(path: &Path, sample_rate: u32, channels: &[Vec<f32>]) -> Result<(), String> {
    let n = channels.len();
    if n == 0 {
        return Err("no channels to write".to_string());
    }
    let frames = channels[0].len();
    let data_len = frames * n * 4;
    let mut o: Vec<u8> = Vec::with_capacity(data_len + 80);
    o.extend_from_slice(b"RIFF");
    o.extend_from_slice(&((4 + 26 + 12 + 8 + data_len) as u32).to_le_bytes());
    o.extend_from_slice(b"WAVE");
    o.extend_from_slice(b"fmt ");
    o.extend_from_slice(&18u32.to_le_bytes());
    o.extend_from_slice(&3u16.to_le_bytes());
    o.extend_from_slice(&(n as u16).to_le_bytes());
    o.extend_from_slice(&sample_rate.to_le_bytes());
    o.extend_from_slice(&(sample_rate * n as u32 * 4).to_le_bytes());
    o.extend_from_slice(&((n * 4) as u16).to_le_bytes());
    o.extend_from_slice(&32u16.to_le_bytes());
    o.extend_from_slice(&0u16.to_le_bytes());
    o.extend_from_slice(b"fact");
    o.extend_from_slice(&4u32.to_le_bytes());
    o.extend_from_slice(&(frames as u32).to_le_bytes());
    o.extend_from_slice(b"data");
    o.extend_from_slice(&(data_len as u32).to_le_bytes());
    for f in 0..frames {
        for ch in channels {
            o.extend_from_slice(&ch.get(f).copied().unwrap_or(0.0).to_le_bytes());
        }
    }
    std::fs::write(path, o).map_err(|e| format!("{}: {}", path.display(), e))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn float_files_round_trip() {
        let path = std::env::temp_dir().join("muse-host-wav-roundtrip.wav");
        let left: Vec<f32> = (0..100).map(|n| (n as f32 * 0.01).sin()).collect();
        let right: Vec<f32> = (0..100).map(|n| -(n as f32 * 0.02).sin()).collect();
        write(&path, 44100, &[left.clone(), right.clone()]).unwrap();
        let back = read(&path).unwrap();
        assert_eq!(back.sample_rate, 44100);
        assert_eq!(back.channels.len(), 2);
        assert_eq!(back.channels[0], left);
        assert_eq!(back.channels[1], right);
    }

    #[test]
    fn sixteen_bit_files_are_read() {
        let mut b: Vec<u8> = Vec::new();
        b.extend_from_slice(b"RIFF");
        b.extend_from_slice(&(36u32 + 4).to_le_bytes());
        b.extend_from_slice(b"WAVEfmt ");
        b.extend_from_slice(&16u32.to_le_bytes());
        b.extend_from_slice(&1u16.to_le_bytes());
        b.extend_from_slice(&1u16.to_le_bytes());
        b.extend_from_slice(&8000u32.to_le_bytes());
        b.extend_from_slice(&16000u32.to_le_bytes());
        b.extend_from_slice(&2u16.to_le_bytes());
        b.extend_from_slice(&16u16.to_le_bytes());
        b.extend_from_slice(b"data");
        b.extend_from_slice(&4u32.to_le_bytes());
        b.extend_from_slice(&16384i16.to_le_bytes());
        b.extend_from_slice(&(-32768i16).to_le_bytes());
        let w = parse(&b).unwrap();
        assert_eq!(w.sample_rate, 8000);
        assert_eq!(w.channels[0], vec![0.5, -1.0]);
    }

    #[test]
    fn garbage_is_refused() {
        assert!(parse(b"not a wav file at all").is_err());
    }
}
