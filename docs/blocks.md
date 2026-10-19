<!-- Generated from crates/blocks by `ruse-mcp docs`. Change the source, then run it again. -->
# Blocks

Every block a graph can use. An agent reads the same list through `list_blocks`. [graph-format.md](graph-format.md) describes how to put blocks together and bind their arguments to parameters.

- [`gain`](#gain)
- [`filter`](#filter)
- [`onepole`](#onepole)
- [`shaper`](#shaper)
- [`bitcrusher`](#bitcrusher)
- [`dynamics`](#dynamics)
- [`limiter`](#limiter)
- [`delay`](#delay)
- [`reverb`](#reverb)
- [`pitch_shift`](#pitch_shift)
- [`tremolo`](#tremolo)
- [`stereo`](#stereo)
- [`harmonic_eq`](#harmonic_eq)
- [`oscillator`](#oscillator)
- [`noise`](#noise)

## `gain`

Changes the level, flips the polarity and pans a stereo signal. A change glides across 32 samples, so automating it does not click.

| Argument | Range | Default | |
|---|---|---|---|
| `gain_db` | -96 to 48 dB | 0 dB | Level change. |
| `pan` | -1 to 1 | 0 | Balance from -1 (left) to 1 (right), constant power. Only used with exactly two channels. |
| `invert` | `off`, `on` | `off` | Flips the polarity. |

## `filter`

A second-order filter: low-pass, high-pass, band-pass, notch, peaking bell, low shelf, high shelf or all-pass. A low-pass or high-pass can have a steeper slope, made of cascaded Butterworth sections. Frequency, Q and gain glide, so sweeping them does not click.

| Argument | Range | Default | |
|---|---|---|---|
| `type` | `lowpass`, `highpass`, `bandpass`, `notch`, `peak`, `lowshelf`, `highshelf`, `allpass` | `lowpass` | Which response. |
| `freq` | 10 to 24000 Hz | 1000 Hz | Cutoff, centre or corner frequency. |
| `q` | 0.1 to 40 | 0.7071 | Resonance or bandwidth. For a steep low-pass or high-pass the Butterworth values are used instead. |
| `gain_db` | -36 to 36 dB | 0 dB | Boost or cut for peak, lowshelf and highshelf. The other types ignore it. |
| `slope` | `12`, `24`, `36`, `48` | `12` | Decibels per octave for lowpass and highpass. The other types ignore it. |

## `onepole`

A first-order filter with a gentle 6 dB per octave slope. Use it for tone controls, for DC removal with a high-pass near 5 Hz, and wherever a sharper filter would sound harsh.

| Argument | Range | Default | |
|---|---|---|---|
| `type` | `lowpass`, `highpass` | `lowpass` | Low-pass or high-pass. |
| `freq` | 1 to 24000 Hz | 1000 Hz | The corner frequency, where the response is 3 dB down. |

## `shaper`

Pushes the signal through a curve. Drive sets how hard, and every curve adds harmonics. Bias moves the signal off centre before the curve, which adds even harmonics. On its own it runs at the plugin's rate, so high drive can alias. Put it in an oversample node to keep the harmonics it makes above Nyquist from folding back.

| Argument | Range | Default | |
|---|---|---|---|
| `curve` | `tanh`, `hard`, `softclip`, `fold`, `tube` | `tanh` | tanh rounds the peaks smoothly. hard cuts them flat. softclip is a gentle cubic knee. fold wraps the peaks back on themselves. tube is asymmetric, saturating one half harder than the other. |
| `drive_db` | -24 to 48 dB | 0 dB | Gain before the curve. |
| `bias` | -1 to 1 | 0 | Moves the signal off centre before the curve. The constant this adds is removed after. |
| `output_db` | -48 to 24 dB | 0 dB | Gain after the curve. |

## `bitcrusher`

Lowers the resolution and the sample rate on purpose. bits rounds every sample to a coarser grid, which adds a gritty noise that follows the signal. rate_hz holds each sample until the next tick of a slower clock, which folds high frequencies back down as metallic tones. Both move smoothly, fractional bits included, so either can follow a parameter or a modulator. Dither trades the gritty noise for a steady hiss, which stops when the input goes silent.

| Argument | Range | Default | |
|---|---|---|---|
| `bits` | 1 to 24 bits | 8 bits | The resolution. 16 is CD quality, 8 is an early sampler, and below 4 little but the loudest parts survive. |
| `rate_hz` | 50 to 192000 Hz | 192000 Hz | How often a new sample is taken. At or above the plugin's sample rate every sample is. |
| `dither` | `off`, `on` | `off` | Adds a little noise before rounding, so quiet sounds fade into hiss instead of breaking up. |

## `dynamics`

A dynamics processor. As a compressor or limiter it turns down what rises above the threshold. As a gate or expander it turns down what falls below it. The detector can be filtered so bass does not pump the whole mix, and the channels can be linked so the stereo image holds still. A lookahead adds latency, which the plugin reports to the host.

| Argument | Range | Default | |
|---|---|---|---|
| `mode` | `compressor`, `limiter`, `gate`, `expander` | `compressor` | compressor reduces by ratio above the threshold. limiter is a compressor with an infinite ratio. expander reduces by ratio below the threshold, and gate is the same with a steep ratio set by range. |
| `threshold_db` | -80 to 0 dB | -18 dB | The level where the effect starts. |
| `ratio` | 1 to 100 | 4 | For a compressor, the input change for 1 dB of output change above the threshold. For an expander, the extra reduction below it. |
| `knee_db` | 0 to 24 dB | 6 dB | The width of the soft transition around the threshold. |
| `attack_ms` | 0.01 to 200 ms | 10 ms | How fast the gain reduction builds. |
| `release_ms` | 1 to 2000 ms | 120 ms | How fast the gain comes back. |
| `makeup_db` | -24 to 36 dB | 0 dB | Gain after the processing. |
| `auto_makeup` | `off`, `on` | `off` | Adds the gain a typical signal loses, estimated from the threshold and ratio. |
| `detector` | `peak`, `rms` | `peak` | How the level is measured. rms follows loudness and is gentler. |
| `sidechain_hpf_hz` | 10 to 1000 Hz | 10 Hz | A high-pass on the detector alone. Near 10 Hz it does nothing. |
| `link` | 0 to 1 | 1 | 0 measures each channel on its own and 1 uses the loudest channel for all of them. |
| `lookahead_ms` | 0 to 10 ms | 0 ms | Delays the audio so the detector sees peaks early. Adds this much latency. Fixed: a number, set when the plugin is built. |
| `range_db` | 0 to 80 dB | 80 dB | The most the gain can be turned down. |
| `key` | `input`, `sidechain` | `input` | What the detector listens to. sidechain gives the plugin a sidechain input, so a kick drum there can duck a bass here, or a voice duck music. Each channel listens to the sidechain channel with its number, or the last one there is. |

Meters: `gain_reduction_db`.

## `limiter`

A brickwall peak limiter. It sees each peak a lookahead early and ramps the gain down to meet it, so no sample goes over the ceiling, then lets the gain back up at the release. The channels share one gain, so the stereo image holds still. Raise input to push the level up against the ceiling. The lookahead is the plugin's latency. It limits sample peaks: the waveform between samples can still go a little over, which an oversample node around it reduces.

| Argument | Range | Default | |
|---|---|---|---|
| `input_db` | -24 to 36 dB | 0 dB | Gain into the limiter. More pushes the level harder against the ceiling. |
| `ceiling_db` | -24 to 0 dB | -1 dB | The highest any sample of the output reaches. |
| `release_ms` | 1 to 2000 ms | 80 ms | How fast the gain comes back after a peak. Short is louder, long distorts less. |
| `lookahead_ms` | 0.1 to 10 ms | 1.5 ms | How early it sees a peak, which is also the time the gain takes to come down and the latency it adds. Longer is cleaner on bass. Fixed: a number, set when the plugin is built. |

Meters: `gain_reduction_db`.

## `delay`

A delay line. The block outputs only the delayed signal, so set the node's `mix` to blend it with the dry sound. For an echo use 100 ms or more with feedback. For a chorus use 10 to 30 ms with depth and no feedback and a mix of 0.5. For a flanger use 1 to 5 ms with depth and feedback. For a vibrato use mix 1.

| Argument | Range | Default | |
|---|---|---|---|
| `time_ms` | 0.1 to 4000 ms | 250 ms | The delay. Bind it to sync(beats) to lock it to the tempo. |
| `feedback` | 0 to 0.97 | 0.35 | How much of the output returns to the input. It stays below 1 so the echoes die away. |
| `damp_hz` | 200 to 20000 Hz | 8000 Hz | A low-pass in the feedback path, so each repeat is darker. |
| `mode` | `normal`, `pingpong` | `normal` | pingpong bounces the echoes between left and right when there are two channels. |
| `mod_depth_ms` | 0 to 20 ms | 0 ms | How far the delay time swings from an LFO. |
| `mod_rate_hz` | 0.02 to 20 Hz | 0.5 Hz | The speed of that swing. |
| `max_ms` | 10 to 4000 ms | 2000 ms | The longest delay the line can hold. Sets the memory used. Fixed: a number, set when the plugin is built. |

## `reverb`

A feedback delay network reverb: a pre-delay, four all-pass diffusers that smear the onset, and eight delay lines that feed each other through a mixing matrix, each with damping and a slow wobble that keeps the tail from ringing. The block outputs only the reverb, so set the node's mix to blend it with the dry sound. Every channel feeds the reverb, and each output channel takes a different blend of the lines, so the tail is wide on two or more channels.

| Argument | Range | Default | |
|---|---|---|---|
| `size` | 0 to 1 | 0.5 | The space: 0 is a small room and 1 a large hall. It sets the line lengths, and a change glides. |
| `decay_s` | 0.1 to 30 s | 2 s | How long the tail takes to fall 60 dB at low and middle frequencies. |
| `predelay_ms` | 0 to 250 ms | 10 ms | A gap before the reverb starts, which keeps it apart from the dry sound. |
| `damp_hz` | 500 to 20000 Hz | 6000 Hz | A low-pass in every line, so the highs die away sooner than the lows. |
| `diffusion` | 0 to 1 | 0.7 | How fast the echoes blur into a smooth wash. Low values keep distinct early echoes. |
| `modulation` | 0 to 1 | 0.3 | A slow wobble in the line lengths that smooths metallic ringing. High values add a chorus. |
| `width` | 0 to 1 | 1 | Stereo width of the tail. 0 makes every channel the same. |
| `freeze` | `off`, `on` | `off` | on holds the tail as it is, forever: no new input goes in, and nothing decays or darkens. Bind it to a toggle for pads and drones. The change fades over 32 samples. |

## `pitch_shift`

Moves the pitch up or down without changing the length, by reading a delay line with two taps that sweep faster or slower than the audio and take turns. It suits harmonies, octave layers and detune. The window is how far each tap sweeps: long windows keep tones steady but smear drums, and short ones keep drums tight but make tones warble. It outputs only the shifted sound, so give the node a mix to keep the original.

| Argument | Range | Default | |
|---|---|---|---|
| `semitones` | -24 to 24 st | 0 st | The shift in semitones. 12 is an octave up, -12 an octave down, 7 a fifth up. |
| `cents` | -100 to 100 ct | 0 ct | A fine shift in hundredths of a semitone. A few cents either side thickens a sound. |
| `window_ms` | 10 to 200 ms | 50 ms | How far each tap sweeps before the other takes over. |

## `tremolo`

Varies the level with a low-frequency oscillator. With the channels half a cycle apart it is an auto-pan.

| Argument | Range | Default | |
|---|---|---|---|
| `rate_hz` | 0.05 to 40 Hz | 4 Hz | The speed. |
| `depth` | 0 to 1 | 0.5 | 0 does nothing. 1 takes the level all the way down at the bottom of each cycle. |
| `shape` | `sine`, `triangle` | `sine` | The wave. |
| `stereo_phase` | 0 to 180 deg | 0 deg | How far each channel's cycle is offset from the one before. 180 with two channels is an auto-pan. |

## `stereo`

Mid/side work on a two-channel signal. width changes the level of the side signal and mid_db and side_db change each one's level. encode turns left and right into mid and side, so a filter or compressor on one channel, with the node's `only` option, works on the mid or the side alone. decode turns them back. With any other channel count the block does nothing.

| Argument | Range | Default | |
|---|---|---|---|
| `mode` | `width`, `encode`, `decode`, `mono`, `swap` | `width` | width scales mid and side. encode puts mid in channel 0 and side in channel 1. decode reverses it. mono sums to the middle. swap exchanges left and right. |
| `width` | 0 to 2 | 1 | Side level multiplier for width mode. 0 is mono, 1 is unchanged, 2 is twice as wide. |
| `mid_db` | -24 to 24 dB | 0 dB | Mid level for width mode. |
| `side_db` | -24 to 24 dB | 0 dB | Side level for width mode. |

## `harmonic_eq`

A parametric equalizer with its bands on the harmonics of one fundamental. Band k sits at k times the fundamental and has the gain base + tilt * log2(k) + (odd or even lift) + the trim for harmonic k, if there is one. Bandwidth is a fraction of the fundamental, so the bands cover the spectrum evenly and a narrow setting rings like a comb. Harmonics above 45% of the sample rate are left out.

| Argument | Range | Default | |
|---|---|---|---|
| `fundamental_hz` | 20 to 2000 Hz | 110 Hz | The pitch. Use note(n) for a MIDI note, or 50 or 60 to treat mains hum. |
| `harmonics` | 1 to 32 | 8 | How many harmonics, counted from the first one affected. |
| `first` | 1 to 32 | 1 | The lowest harmonic number affected. 2 leaves the fundamental alone. |
| `gain_db` | -24 to 24 dB | 0 dB | Boost or cut for every band. |
| `tilt_db_oct` | -12 to 12 dB | 0 dB | Extra gain per doubling of the harmonic number. Positive brightens the overtones. |
| `odd_db` | -24 to 24 dB | 0 dB | Extra gain on the odd harmonics: 1, 3, 5 and so on. |
| `even_db` | -24 to 24 dB | 0 dB | Extra gain on the even harmonics: 2, 4, 6 and so on. |
| `bandwidth` | 0.02 to 1 | 0.25 | Each band's width as a fraction of the fundamental. Small is narrow and surgical. |
| `stretch` | 0 to 0.02 | 0 | Inharmonicity. Band k sits at k times the fundamental times sqrt(1 + stretch * k * k). |
| `trim_1_db` | -24 to 24 dB | 0 dB | Extra gain on harmonic 1. |
| `trim_2_db` | -24 to 24 dB | 0 dB | Extra gain on harmonic 2. |
| `trim_3_db` | -24 to 24 dB | 0 dB | Extra gain on harmonic 3. |
| `trim_4_db` | -24 to 24 dB | 0 dB | Extra gain on harmonic 4. |
| `trim_5_db` | -24 to 24 dB | 0 dB | Extra gain on harmonic 5. |
| `trim_6_db` | -24 to 24 dB | 0 dB | Extra gain on harmonic 6. |
| `trim_7_db` | -24 to 24 dB | 0 dB | Extra gain on harmonic 7. |
| `trim_8_db` | -24 to 24 dB | 0 dB | Extra gain on harmonic 8. |

## `oscillator`

A tone: a sine, triangle, saw, square or pulse at a frequency a number or an expression sets, such as note(57) or a knob. Saw, square and pulse are band-limited. mode replace puts out the tone alone and add mixes it into the signal. Every channel gets the same tone unless stereo_phase starts each channel further round the cycle. In a multiply node, an oscillator in one branch and an empty chain in the other is a ring modulator, and with offset 1 it is amplitude modulation.

| Argument | Range | Default | |
|---|---|---|---|
| `shape` | `sine`, `triangle`, `saw`, `square`, `pulse` | `sine` |  |
| `freq_hz` | 0.1 to 20000 Hz | 440 Hz | The frequency. note(n) gives MIDI note n. A change glides. |
| `level_db` | -96 to 0 dB | -12 dB | The tone's level. A change glides. |
| `offset` | 0 to 1 | 0 | Added after the level. Through multiply, 1 makes the tone vary the input's level instead of its polarity. |
| `pulse_width` | 0.05 to 0.95 | 0.5 | Pulse only. The part of each cycle spent high. |
| `mode` | `replace`, `add` | `replace` | replace puts out the tone alone. add mixes it into the signal. |
| `stereo_phase` | 0 to 360 deg | 0 deg | How far round the cycle each channel starts from the one before. 180 puts the second channel in opposite phase to the first, which makes a ring modulator or tremolo move across the stereo field. |

## `noise`

Noise at a set RMS level. white is even at every frequency, pink falls 3 dB an octave like most natural sounds, and brown falls 6 dB an octave for a deep rumble. mode replace puts out the noise alone and add mixes it into the signal, for hiss. width 1 gives each channel its own noise and 0 the same noise in all of them. It is seeded, so two instances make the same noise, and its tail never ends.

| Argument | Range | Default | |
|---|---|---|---|
| `color` | `white`, `pink`, `brown` | `white` |  |
| `level_db` | -96 to 0 dB | -24 dB | The RMS level. A change glides. |
| `width` | 0 to 1 | 1 | 0 is the same noise in every channel and 1 a different noise in each, at the same level. |
| `mode` | `replace`, `add` | `replace` | replace puts out the noise alone. add mixes it into the signal. |

# Subgraphs

Subgraphs RUSE ships, used as {"subgraph": name, "args": {...}}. Their arguments take the ranges of the blocks inside.

## `parametric_eq`

A four-band equalizer: a low shelf, two bells and a high shelf, then an output gain. It is four filter blocks and a gain block, so a UI can draw its curve from theirs.

| Argument | Default |
|---|---|
| `low_freq` | 100 |
| `low_gain_db` | 0 |
| `mid1_freq` | 500 |
| `mid1_gain_db` | 0 |
| `mid1_q` | 1 |
| `mid2_freq` | 2500 |
| `mid2_gain_db` | 0 |
| `mid2_q` | 1 |
| `high_freq` | 8000 |
| `high_gain_db` | 0 |
| `output_db` | 0 |
