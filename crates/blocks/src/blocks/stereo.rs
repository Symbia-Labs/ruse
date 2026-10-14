//! Mid/side and stereo width.

use super::util::*;
use crate::block::{ArgInfo, Block, BlockInfo};
use crate::frame::{ramp, Frame};

const MODE: usize = 0;
const WIDTH: usize = 1;
const MID_DB: usize = 2;
const SIDE_DB: usize = 3;

pub static INFO: BlockInfo = BlockInfo {
    name: "stereo",
    doc: "Mid/side work on a two-channel signal. width changes the level of the side signal and mid_db and side_db change each one's level. encode turns left and right into mid and side, so a filter or compressor on one channel, with the node's `only` option, works on the mid or the side alone. decode turns them back. With any other channel count the block does nothing.",
    args: &[
        ArgInfo::choice(
            "mode",
            &["width", "encode", "decode", "mono", "swap"],
            0,
            "width scales mid and side. encode puts mid in channel 0 and side in channel 1. decode reverses it. mono sums to the middle. swap exchanges left and right.",
        ),
        ArgInfo::num("width", "", 0.0, 2.0, 1.0, "Side level multiplier for width mode. 0 is mono, 1 is unchanged, 2 is twice as wide."),
        ArgInfo::num("mid_db", "dB", -24.0, 24.0, 0.0, "Mid level for width mode."),
        ArgInfo::num("side_db", "dB", -24.0, 24.0, 0.0, "Side level for width mode."),
    ],
    meters: &[],
    make,
};

pub fn make() -> Box<dyn Block> {
    Box::new(Stereo::default())
}

#[derive(Default)]
struct Stereo {
    on: bool,
    mode: usize,
    mid: (f32, f32),
    side: (f32, f32),
    started: bool,
}

impl Block for Stereo {
    fn prepare(&mut self, _rate: f32, channels: usize) {
        *self = Stereo {
            on: channels == 2,
            ..Default::default()
        };
    }

    fn control(&mut self, a: &[f32]) {
        self.mode = a[MODE] as usize;
        let mid = db_to_lin(a[MID_DB]);
        let side = db_to_lin(a[SIDE_DB]) * a[WIDTH];
        if !self.started {
            self.mid = (mid, mid);
            self.side = (side, side);
            self.started = true;
        }
        self.mid = (self.mid.1, mid);
        self.side = (self.side.1, side);
    }

    fn process(&mut self, io: &mut Frame, offset: usize) {
        if !self.on {
            return;
        }
        let n = io.len;
        let (left, right) = io.ch.split_at_mut(1);
        let (l, r) = (&mut left[0], &mut right[0]);
        for k in 0..n {
            let (a, b) = (l[k], r[k]);
            let (x, y) = match self.mode {
                0 => {
                    let m = (a + b) * 0.5 * ramp(self.mid.0, self.mid.1, offset, k);
                    let s = (a - b) * 0.5 * ramp(self.side.0, self.side.1, offset, k);
                    (m + s, m - s)
                }
                1 => ((a + b) * 0.5, (a - b) * 0.5),
                2 => (a + b, a - b),
                3 => {
                    let m = (a + b) * 0.5;
                    (m, m)
                }
                _ => (b, a),
            };
            l[k] = x;
            r[k] = y;
        }
    }
}
