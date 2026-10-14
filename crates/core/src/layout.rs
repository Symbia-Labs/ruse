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
}
