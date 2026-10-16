<!-- Generated from crates/blocks by `muse-mcp docs`. Change the source, then run it again. -->
# Blocks

Every block a graph can use. An agent reads the same list through `list_blocks`. [graph-format.md](graph-format.md) describes how to put blocks together and bind their arguments to parameters.

- [`gain`](#gain)
- [`filter`](#filter)
- [`onepole`](#onepole)
- [`shaper`](#shaper)
- [`dynamics`](#dynamics)
- [`delay`](#delay)
- [`tremolo`](#tremolo)
- [`stereo`](#stereo)
- [`harmonic_eq`](#harmonic_eq)

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

Pushes the signal through a curve. Drive sets how hard, and every curve adds harmonics. Bias moves the signal off centre before the curve, which adds even harmonics. It does not oversample yet, so very high drive can alias. Follow it with a low-pass to tame the top.

| Argument | Range | Default | |
|---|---|---|---|
| `curve` | `tanh`, `hard`, `softclip`, `fold`, `tube` | `tanh` | tanh rounds the peaks smoothly. hard cuts them flat. softclip is a gentle cubic knee. fold wraps the peaks back on themselves. tube is asymmetric, saturating one half harder than the other. |
| `drive_db` | -24 to 48 dB | 0 dB | Gain before the curve. |
| `bias` | -1 to 1 | 0 | Moves the signal off centre before the curve. The constant this adds is removed after. |
| `output_db` | -48 to 24 dB | 0 dB | Gain after the curve. |

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
