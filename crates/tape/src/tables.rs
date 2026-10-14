#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TapeSettings {
    /// Level onto tape, dB.
    pub input_db: f32,
    /// -1 underbiased (gritty, bright) to +1 overbiased (clean lows, dull top).
    pub bias: f32,
    /// Index into SPEEDS_IPS.
    pub speed: usize,
    /// Index into FORMULA_NAMES.
    pub formula: usize,
    /// Peak speed deviation, percent.
    pub wow: f32,
    pub flutter: f32,
    /// Hiss level, dBFS RMS at 3.75 IPS on Type I. -96 and below is off.
    pub hiss_db: f32,
    pub output_db: f32,
    /// Index into FORMAT_NAMES: how many tracks share what width of tape.
    pub format: usize,
    /// Neighbouring tracks bleed into each other.
    pub link: bool,
}

impl Default for TapeSettings {
    fn default() -> Self {
        TapeSettings {
            input_db: 0.0,
            bias: 0.0,
            speed: 1,
            formula: 1,
            wow: 0.06,
            flutter: 0.04,
            hiss_db: -72.0,
            output_db: 0.0,
            format: 0,
            link: true,
        }
    }
}

pub const SPEEDS_IPS: [f32; 5] = [1.875, 3.75, 7.5, 15.0, 30.0];
pub const SPEED_NAMES: [&str; 5] = ["1⅞ IPS", "3¾ IPS", "7½ IPS", "15 IPS", "30 IPS"];
pub const FORMULA_NAMES: [&str; 3] = ["Type I", "Type II", "Type IV"];
pub const MAX_FRAMES: usize = 4096;
pub const HALF_TAPS: usize = 47;
pub const FIR_TAPS: usize = 64;
/// Delay through the two half-band filters, in samples at the stage's rate.
pub const FIXED_LATENCY: usize = (HALF_TAPS - 1) / 2;

/// Level (dBFS, 1 kHz, input at 0 dB) where third-harmonic distortion reaches 3%.
pub const MOL_DBFS: [f32; 3] = [-5.0, -4.0, -1.0];
/// Coercivity, in units of the anhysteretic knee: how wide the loop is.
pub const COERCIVITY: [f32; 3] = [0.5, 0.65, 0.9];
/// Hiss against Type I, dB.
pub const HISS_OFFSET_DB: [f32; 3] = [0.0, -3.0, -3.5];
/// Capstan rotation, Hz, per speed: where the flutter sits.
pub const CAPSTAN_HZ: [f32; 5] = [5.8, 11.6, 9.1, 12.1, 24.2];
/// Playback equalization time constants, microseconds, per speed and formulation.
pub const EQUALIZATION_MICROS: [[f32; 3]; 5] = [
    [120.0, 70.0, 70.0],
    [70.0, 50.0, 50.0],
    [50.0, 50.0, 50.0],
    [35.0, 35.0, 35.0],
    [17.5, 17.5, 17.5],
];
/// The most that record emphasis is allowed to reach.
pub const EMPHASIS_MAX: f32 = 6.3;

pub const FORMAT_NAMES: [&str; 8] = [
    "Cassette 4-track",
    "¼″ 8-track",
    "½″ 8-track",
    "1″ 16-track",
    "2″ 16-track",
    "2″ 24-track",
    "¼″ 2-track",
    "½″ 2-track",
];
/// Width of one track, mm. Noise falls 3 dB for each doubling.
pub const TRACK_WIDTH_MM: [f32; 8] = [0.6, 0.5, 1.0, 1.0, 1.8, 1.1, 2.0, 5.0];
/// What a track picks up from the one beside it, dB at 1 kHz.
pub const BLEED_DB: [f32; 8] = [-45.0, -45.0, -50.0, -50.0, -60.0, -55.0, -55.0, -60.0];
/// How many times over the part of the bleed below 200 Hz is added.
pub const BLEED_LOW: f32 = 3.0;
