use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering};
use std::sync::OnceLock;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Curve {
    Linear,
    Log,
}

impl Curve {
    pub fn name(self) -> &'static str {
        match self {
            Curve::Linear => "linear",
            Curve::Log => "log",
        }
    }
}

/// How an editor or an agent should treat a parameter.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Knob,
    /// One of a list of named values. The plain value is the index.
    Choice,
    /// Off (0) or on (1).
    Toggle,
    /// Written by the plugin, read by everyone else.
    Meter,
}

impl Kind {
    pub fn name(self) -> &'static str {
        match self {
            Kind::Knob => "knob",
            Kind::Choice => "choice",
            Kind::Toggle => "toggle",
            Kind::Meter => "meter",
        }
    }
}

/// Whether a parameter has one value for the whole instance or one per channel.
/// Each plugin decides this for each of its parameters.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scope {
    /// One value, shared across every channel.
    Global,
    /// One value for each channel. A write can address one channel or all of them.
    PerChannel,
}

impl Scope {
    pub fn name(self) -> &'static str {
        match self {
            Scope::Global => "global",
            Scope::PerChannel => "per_channel",
        }
    }
}

/// Everything a person or an agent needs to know about a parameter.
#[derive(Clone, Copy, Debug)]
pub struct ParamInfo {
    pub id: &'static str,
    pub name: &'static str,
    pub description: &'static str,
    pub unit: &'static str,
    pub min: f32,
    pub max: f32,
    pub default: f32,
    pub curve: Curve,
    pub kind: Kind,
    pub scope: Scope,
    pub group: &'static str,
    pub choices: &'static [&'static str],
    /// The plugin's own wording for a value, such as "Under 35" or "Aligned".
    pub display: Option<fn(f32) -> String>,
}

impl ParamInfo {
    pub const fn knob(
        id: &'static str,
        name: &'static str,
        unit: &'static str,
        min: f32,
        max: f32,
        default: f32,
    ) -> ParamInfo {
        ParamInfo {
            id,
            name,
            description: "",
            unit,
            min,
            max,
            default,
            curve: Curve::Linear,
            kind: Kind::Knob,
            scope: Scope::Global,
            group: "",
            choices: &[],
            display: None,
        }
    }

    pub const fn choice(
        id: &'static str,
        name: &'static str,
        choices: &'static [&'static str],
        default: usize,
    ) -> ParamInfo {
        ParamInfo {
            id,
            name,
            description: "",
            unit: "",
            min: 0.0,
            max: (choices.len() - 1) as f32,
            default: default as f32,
            curve: Curve::Linear,
            kind: Kind::Choice,
            scope: Scope::Global,
            group: "",
            choices,
            display: None,
        }
    }

    pub const fn toggle(id: &'static str, name: &'static str, default: bool) -> ParamInfo {
        ParamInfo {
            id,
            name,
            description: "",
            unit: "",
            min: 0.0,
            max: 1.0,
            default: if default { 1.0 } else { 0.0 },
            curve: Curve::Linear,
            kind: Kind::Toggle,
            scope: Scope::Global,
            group: "",
            choices: &[],
            display: None,
        }
    }

    pub const fn meter(
        id: &'static str,
        name: &'static str,
        unit: &'static str,
        min: f32,
        max: f32,
    ) -> ParamInfo {
        ParamInfo {
            id,
            name,
            description: "",
            unit,
            min,
            max,
            default: min,
            curve: Curve::Linear,
            kind: Kind::Meter,
            scope: Scope::Global,
            group: "",
            choices: &[],
            display: None,
        }
    }

    /// One value for each channel instead of one for the whole instance.
    pub const fn per_channel(self) -> ParamInfo {
        ParamInfo {
            scope: Scope::PerChannel,
            ..self
        }
    }

    pub fn to_normalized(&self, plain: f32) -> f32 {
        let p = plain.clamp(self.min, self.max);
        match self.curve {
            Curve::Linear => (p - self.min) / (self.max - self.min),
            Curve::Log => (p / self.min).ln() / (self.max / self.min).ln(),
        }
    }

    pub fn to_plain(&self, normalized: f32) -> f32 {
        let n = normalized.clamp(0.0, 1.0);
        let v = match self.curve {
            Curve::Linear => self.min + n * (self.max - self.min),
            Curve::Log => self.min * (self.max / self.min).powf(n),
        };
        self.snap(v)
    }

    /// Choices and toggles only hold whole numbers.
    pub fn snap(&self, v: f32) -> f32 {
        match self.kind {
            Kind::Choice | Kind::Toggle => v.round(),
            _ => v,
        }
    }

    /// The value as a person would read it.
    pub fn display(&self, plain: f32) -> String {
        if let Some(f) = self.display {
            return f(plain);
        }
        match self.kind {
            Kind::Choice => {
                let i = (plain.round() as usize).min(self.choices.len().saturating_sub(1));
                self.choices.get(i).copied().unwrap_or("").to_string()
            }
            Kind::Toggle => (if plain >= 0.5 { "On" } else { "Off" }).to_string(),
            _ => {
                if self.unit.is_empty() {
                    format!("{:.2}", plain)
                } else {
                    format!("{:.2} {}", plain, self.unit)
                }
            }
        }
    }
}

/// A parameter value shared between the audio thread and the control thread.
/// A per-channel parameter holds one value for each channel the plugin could be given.
pub struct Param {
    info: ParamInfo,
    values: Vec<AtomicU32>,
}

impl Param {
    pub fn new(info: ParamInfo, slots: usize) -> Self {
        assert!(info.min < info.max, "{}: min must be below max", info.id);
        assert!(
            info.default >= info.min && info.default <= info.max,
            "{}: default outside range",
            info.id
        );
        if info.curve == Curve::Log {
            assert!(info.min > 0.0, "{}: a log curve needs min above zero", info.id);
        }
        match info.kind {
            Kind::Choice => {
                assert!(info.choices.len() >= 2, "{}: a choice needs two or more names", info.id);
                assert!(
                    info.min == 0.0 && info.max == (info.choices.len() - 1) as f32,
                    "{}: a choice runs from 0 to its last index",
                    info.id
                );
            }
            Kind::Toggle => {
                assert!(info.min == 0.0 && info.max == 1.0, "{}: a toggle runs from 0 to 1", info.id);
            }
            _ => {}
        }
        let n = if info.scope == Scope::PerChannel { slots.max(1) } else { 1 };
        Param {
            values: (0..n).map(|_| AtomicU32::new(info.default.to_bits())).collect(),
            info,
        }
    }

    pub fn info(&self) -> &ParamInfo {
        &self.info
    }

    /// How many values this parameter holds: one if global, one per possible channel if not.
    pub fn slots(&self) -> usize {
        self.values.len()
    }

    /// The value for one channel. A global parameter gives the same value for every channel.
    pub fn get_ch(&self, channel: usize) -> f32 {
        let i = channel.min(self.values.len() - 1);
        f32::from_bits(self.values[i].load(Ordering::Relaxed))
    }

    /// The value for channel 0, which is the value of a global parameter.
    pub fn get(&self) -> f32 {
        self.get_ch(0)
    }

    fn clean(&self, plain: f32) -> f32 {
        if plain.is_nan() {
            self.info.default
        } else {
            self.info.snap(plain.clamp(self.info.min, self.info.max))
        }
    }

    /// Stores the value, clamped to the range, on every channel. Returns what was stored.
    pub fn set(&self, plain: f32) -> f32 {
        let v = self.clean(plain);
        for slot in &self.values {
            slot.store(v.to_bits(), Ordering::Relaxed);
        }
        v
    }

    /// Stores the value on one channel of a per-channel parameter. A global parameter
    /// has one value, so this sets it. Returns what was stored.
    pub fn set_ch(&self, channel: usize, plain: f32) -> f32 {
        let v = self.clean(plain);
        let i = channel.min(self.values.len() - 1);
        self.values[i].store(v.to_bits(), Ordering::Relaxed);
        v
    }

    pub fn get_normalized(&self) -> f32 {
        self.info.to_normalized(self.get())
    }

    pub fn set_normalized(&self, normalized: f32) -> f32 {
        self.set(self.info.to_plain(normalized))
    }

    pub fn text(&self) -> String {
        self.info.display(self.get())
    }

    pub fn text_ch(&self, channel: usize) -> String {
        self.info.display(self.get_ch(channel))
    }
}

type Listener = Box<dyn Fn(&ParamSet) + Send + Sync>;

pub struct ParamSet {
    params: Vec<Param>,
    max_channels: usize,
    channels: AtomicUsize,
    listener: OnceLock<Listener>,
}

impl ParamSet {
    /// For a plugin with no per-channel parameters, or one that only ever has one channel.
    pub fn new(infos: &[ParamInfo]) -> Self {
        Self::with_channels(infos, 1)
    }

    /// `max_channels` is the most channels the plugin will be given. Each per-channel
    /// parameter holds that many values, allocated now so the audio thread never allocates.
    pub fn with_channels(infos: &[ParamInfo], max_channels: usize) -> Self {
        let max_channels = max_channels.max(1);
        let params: Vec<Param> = infos.iter().map(|i| Param::new(*i, max_channels)).collect();
        for (i, p) in params.iter().enumerate() {
            assert!(
                !params[..i].iter().any(|q| q.info.id == p.info.id),
                "duplicate parameter id {}",
                p.info.id
            );
        }
        ParamSet {
            params,
            max_channels,
            channels: AtomicUsize::new(1),
            listener: OnceLock::new(),
        }
    }

    /// Registers the one function called, on the setting thread, after every control-side
    /// write. A plugin uses it to rebuild whatever depends on its parameters off the audio thread.
    pub fn on_change(&self, f: impl Fn(&ParamSet) + Send + Sync + 'static) {
        let _ = self.listener.set(Box::new(f));
    }

    /// Runs the listener. Format wrappers call this after applying host automation off the audio thread.
    pub fn notify(&self) {
        if let Some(l) = self.listener.get() {
            l(self);
        }
    }

    /// How many channels the plugin is running with.
    pub fn channels(&self) -> usize {
        self.channels.load(Ordering::Relaxed)
    }

    pub fn max_channels(&self) -> usize {
        self.max_channels
    }

    /// Called when the host settles on a channel count. Clamped to what was allocated.
    pub fn set_channels(&self, n: usize) {
        self.channels.store(n.clamp(1, self.max_channels), Ordering::Relaxed);
    }

    pub fn len(&self) -> usize {
        self.params.len()
    }

    pub fn is_empty(&self) -> bool {
        self.params.is_empty()
    }

    pub fn at(&self, index: usize) -> &Param {
        &self.params[index]
    }

    pub fn by_id(&self, id: &str) -> Option<&Param> {
        self.params.iter().find(|p| p.info.id == id)
    }

    pub fn iter(&self) -> std::slice::Iter<'_, Param> {
        self.params.iter()
    }

    /// The control-side write path. `channel` is `None` to write across every channel, or
    /// the channel to write. A global parameter takes only `None`. Returns the value stored.
    /// Meters belong to the plugin, so this refuses them.
    pub fn set_plain_ch(&self, id: &str, channel: Option<usize>, plain: f32) -> Result<f32, String> {
        let p = self
            .by_id(id)
            .ok_or_else(|| format!("no parameter with id '{}'", id))?;
        if p.info.kind == Kind::Meter {
            return Err(format!("parameter '{}' is a read-only meter", id));
        }
        let stored = match (p.info.scope, channel) {
            (_, None) => p.set(plain),
            (Scope::Global, Some(_)) => {
                return Err(format!("parameter '{}' is global, so it has no per-channel value", id));
            }
            (Scope::PerChannel, Some(c)) => {
                if c >= self.channels() {
                    return Err(format!("channel {} is out of range, the plugin has {}", c, self.channels()));
                }
                p.set_ch(c, plain)
            }
        };
        self.notify();
        Ok(stored)
    }

    /// Writes across every channel.
    pub fn set_plain(&self, id: &str, plain: f32) -> Result<f32, String> {
        self.set_plain_ch(id, None, plain)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CUTOFF: ParamInfo = ParamInfo {
        curve: Curve::Log,
        description: "Filter cutoff frequency.",
        ..ParamInfo::knob("cutoff", "Cutoff", "Hz", 20.0, 20000.0, 1000.0)
    };

    const SPEED: ParamInfo = ParamInfo::choice("speed", "Speed", &["3.75 IPS", "7.5 IPS", "15 IPS"], 1);

    const TRIM: ParamInfo = ParamInfo::knob("trim", "Trim", "dB", -12.0, 12.0, 0.0).per_channel();

    fn bias_text(v: f32) -> String {
        if v.abs() < 0.5 {
            "Aligned".to_string()
        } else if v < 0.0 {
            format!("Under {:.0}", -v)
        } else {
            format!("Over {:.0}", v)
        }
    }

    #[test]
    fn log_curve_round_trips() {
        for hz in [20.0_f32, 100.0, 1000.0, 20000.0] {
            let back = CUTOFF.to_plain(CUTOFF.to_normalized(hz));
            assert!((back - hz).abs() / hz < 1e-4, "{} came back as {}", hz, back);
        }
    }

    #[test]
    fn log_curve_midpoint_is_geometric() {
        let mid = CUTOFF.to_plain(0.5);
        assert!((mid - 632.455).abs() < 0.1, "{}", mid);
    }

    #[test]
    #[should_panic(expected = "duplicate parameter id")]
    fn duplicate_ids_are_refused() {
        ParamSet::new(&[CUTOFF, CUTOFF]);
    }

    #[test]
    fn choice_snaps_and_names_its_values() {
        let set = ParamSet::new(&[SPEED]);
        assert_eq!(set.at(0).text(), "7.5 IPS");
        assert_eq!(set.set_plain("speed", 1.7).unwrap(), 2.0);
        assert_eq!(set.at(0).text(), "15 IPS");
        assert_eq!(set.set_plain("speed", 9.0).unwrap(), 2.0);
        assert_eq!(set.set_plain("speed", -4.0).unwrap(), 0.0);
    }

    #[test]
    fn toggle_reads_on_and_off() {
        let set = ParamSet::new(&[ParamInfo::toggle("link", "Link", true)]);
        assert_eq!(set.at(0).text(), "On");
        set.set_plain("link", 0.2).unwrap();
        assert_eq!(set.at(0).text(), "Off");
    }

    #[test]
    fn meter_is_read_only_from_the_control_side() {
        let set = ParamSet::new(&[ParamInfo::meter("level", "Level", "dB", -96.0, 6.0)]);
        assert!(set.set_plain("level", -3.0).is_err());
        set.at(0).set(-3.0);
        assert_eq!(set.at(0).get(), -3.0);
    }

    #[test]
    fn custom_display_wins() {
        let info = ParamInfo {
            display: Some(bias_text),
            ..ParamInfo::knob("bias", "Bias", "", -100.0, 100.0, 0.0)
        };
        assert_eq!(info.display(0.0), "Aligned");
        assert_eq!(info.display(-35.0), "Under 35");
        assert_eq!(info.display(12.0), "Over 12");
    }

    #[test]
    #[should_panic(expected = "a choice needs two or more names")]
    fn a_choice_with_one_name_is_refused() {
        ParamSet::new(&[ParamInfo {
            max: 1.0,
            ..ParamInfo::choice("x", "X", &["only"], 0)
        }]);
    }

    #[test]
    fn a_per_channel_parameter_holds_one_value_for_each_channel() {
        let set = ParamSet::with_channels(&[TRIM, CUTOFF], 8);
        set.set_channels(4);
        assert_eq!(set.by_id("trim").unwrap().slots(), 8);
        assert_eq!(set.by_id("cutoff").unwrap().slots(), 1);

        set.set_plain_ch("trim", Some(2), 6.0).unwrap();
        let trim = set.by_id("trim").unwrap();
        assert_eq!(trim.get_ch(2), 6.0);
        assert_eq!(trim.get_ch(0), 0.0);
        assert_eq!(trim.get_ch(3), 0.0);
        assert_eq!(trim.text_ch(2), "6.00 dB");
    }

    #[test]
    fn writing_across_channels_sets_every_one() {
        let set = ParamSet::with_channels(&[TRIM], 8);
        set.set_channels(4);
        set.set_plain_ch("trim", Some(1), 3.0).unwrap();
        set.set_plain("trim", -2.0).unwrap();
        let trim = set.by_id("trim").unwrap();
        for c in 0..8 {
            assert_eq!(trim.get_ch(c), -2.0, "channel {}", c);
        }
    }

    #[test]
    fn a_global_parameter_has_no_per_channel_value() {
        let set = ParamSet::with_channels(&[CUTOFF], 8);
        set.set_channels(4);
        assert!(set.set_plain_ch("cutoff", Some(1), 500.0).is_err());
        assert!(set.set_plain_ch("cutoff", None, 500.0).is_ok());
        // Every channel reads the one value.
        assert_eq!(set.by_id("cutoff").unwrap().get_ch(3), 500.0);
    }

    #[test]
    fn a_channel_past_the_count_is_refused_and_the_count_is_clamped() {
        let set = ParamSet::with_channels(&[TRIM], 8);
        set.set_channels(4);
        assert!(set.set_plain_ch("trim", Some(4), 1.0).is_err());
        assert!(set.set_plain_ch("trim", Some(3), 1.0).is_ok());
        set.set_channels(99);
        assert_eq!(set.channels(), 8);
        set.set_channels(0);
        assert_eq!(set.channels(), 1);
    }

    #[test]
    fn the_listener_hears_every_control_side_write() {
        use std::sync::atomic::AtomicUsize;
        use std::sync::Arc;
        let set = ParamSet::with_channels(&[TRIM], 4);
        let heard = Arc::new(AtomicUsize::new(0));
        let h = heard.clone();
        set.on_change(move |_| {
            h.fetch_add(1, Ordering::Relaxed);
        });
        set.set_channels(2);
        set.set_plain_ch("trim", Some(0), 1.0).unwrap();
        set.set_plain("trim", 2.0).unwrap();
        assert!(set.set_plain_ch("trim", Some(5), 2.0).is_err());
        assert_eq!(heard.load(Ordering::Relaxed), 2);
    }
}
