/// How many channels a bus may carry.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Channels {
    Fixed(u32),
    Range { min: u32, max: u32 },
    /// Matches the channel count of the first input bus.
    SameAsInput,
}

#[derive(Clone, Copy, Debug)]
pub struct BusInfo {
    pub name: &'static str,
    pub channels: Channels,
}

#[derive(Clone, Copy, Debug)]
pub struct Layout {
    pub inputs: &'static [BusInfo],
    pub outputs: &'static [BusInfo],
}

fn fits(c: Channels, n: u32, first_input: Option<u32>) -> bool {
    match c {
        Channels::Fixed(k) => n == k,
        Channels::Range { min, max } => n >= min && n <= max,
        Channels::SameAsInput => first_input == Some(n),
    }
}

impl Layout {
    /// The second input bus, which a plugin reads as a sidechain: a signal that controls the processing without
    /// being processed, such as a kick drum that ducks a bass.
    pub fn sidechain(&self) -> Option<&BusInfo> {
        self.inputs.get(1)
    }

    /// How many channels the sidechain carries when the main bus has `main`: the fixed count, the main count
    /// within the range, or the main count itself. None when there is no sidechain.
    pub fn sidechain_channels(&self, main: u32) -> Option<u32> {
        self.sidechain().map(|b| match b.channels {
            Channels::Fixed(k) => k,
            Channels::Range { min, max } => main.clamp(min, max.max(min)),
            Channels::SameAsInput => main,
        })
    }

    /// Whether the plugin runs with `n` channels in and out on its main bus, with the sidechain, when it has one,
    /// at the count `sidechain_channels` gives.
    pub fn accepts_main(&self, n: u32) -> bool {
        match self.sidechain_channels(n) {
            Some(k) => self.accepts(&[n, k], &[n]),
            None => self.accepts(&[n], &[n]),
        }
    }

    /// Whether a host's proposed channel counts, one per bus, suit the plugin.
    pub fn accepts(&self, inputs: &[u32], outputs: &[u32]) -> bool {
        if inputs.len() != self.inputs.len() || outputs.len() != self.outputs.len() {
            return false;
        }
        let first = inputs.first().copied();
        self.inputs.iter().zip(inputs).all(|(b, &n)| fits(b.channels, n, first))
            && self.outputs.iter().zip(outputs).all(|(b, &n)| fits(b.channels, n, first))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TAPE: Layout = Layout {
        inputs: &[BusInfo {
            name: "Tracks",
            channels: Channels::Range { min: 1, max: 64 },
        }],
        outputs: &[BusInfo {
            name: "Tracks",
            channels: Channels::SameAsInput,
        }],
    };

    #[test]
    fn many_channels_are_accepted_when_in_and_out_match() {
        assert!(TAPE.accepts(&[1], &[1]));
        assert!(TAPE.accepts(&[24], &[24]));
        assert!(TAPE.accepts(&[64], &[64]));
    }

    #[test]
    fn mismatches_and_out_of_range_are_refused() {
        assert!(!TAPE.accepts(&[24], &[2]));
        assert!(!TAPE.accepts(&[65], &[65]));
        assert!(!TAPE.accepts(&[0], &[0]));
        assert!(!TAPE.accepts(&[], &[]));
        assert!(!TAPE.accepts(&[2, 2], &[2]));
    }

    #[test]
    fn a_sidechain_is_the_second_input() {
        const DUCKER: Layout = Layout {
            inputs: &[
                BusInfo {
                    name: "Main",
                    channels: Channels::Range { min: 1, max: 8 },
                },
                BusInfo {
                    name: "Sidechain",
                    channels: Channels::Range { min: 1, max: 2 },
                },
            ],
            outputs: &[BusInfo {
                name: "Main",
                channels: Channels::SameAsInput,
            }],
        };
        assert_eq!(TAPE.sidechain_channels(2), None);
        assert_eq!(DUCKER.sidechain_channels(1), Some(1));
        assert_eq!(DUCKER.sidechain_channels(2), Some(2));
        assert_eq!(DUCKER.sidechain_channels(6), Some(2));
        assert!(DUCKER.accepts_main(6) && DUCKER.accepts_main(1));
        assert!(!DUCKER.accepts_main(9));
        assert!(!DUCKER.accepts(&[2], &[2]), "the sidechain bus is part of the layout");
        assert!(TAPE.accepts_main(24));
    }
}
