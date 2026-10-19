# Examples

RUSE ships nine plugins in `examples/`. All but RUSE 808 are built from blocks, and a workspace made with `ruse-mcp new` starts with copies of them, so you can try them, change them or build on them. Each section ends with a few things to ask your agent.

## Warm Saturator

A clipper followed by a low-pass, for warmth and grit.

| Parameter | Range |
|---|---|
| Drive | 0 to 24 dB |
| Mode | Soft or Hard |
| Tone | 200 to 16000 Hz |
| Bypass | on or off |

```json
{ "bypass": "bypass", "chain": [
  { "block": "gain",    "args": { "gain_db": "drive" } },
  { "block": "shaper",  "args": { "curve": "mode" } },
  { "block": "onepole", "args": { "type": "lowpass", "freq": "tone" } }
] }
```

It began as hand-written code. Rebuilt as three blocks, it renders the same audio to within -127 dBFS.

Ask your agent:

- Add a tube curve as a third Mode, with a Bias knob.
- Add a Mix knob for parallel saturation.
- Put a high-pass before the drive so low end does not get muddy.

## Bus Compressor

A feed-forward compressor with a soft knee, a parallel mix and a gain reduction meter.

| Parameter | Range |
|---|---|
| Threshold | -60 to 0 dB |
| Ratio | 1 to 20 |
| Knee | 0 to 24 dB |
| Attack | 0.1 to 100 ms |
| Release | 10 to 1000 ms |
| Makeup | 0 to 24 dB |
| Mix | 0 to 1 |
| Bypass | on or off |
| Reduction | meter, 0 to 30 dB |

One `dynamics` block, with the node's `mix` set by the Mix parameter and the block's `gain_reduction_db` shown on the Reduction meter.

Ask your agent:

- Add a sidechain high-pass so the low end does not pump the compressor.
- Add a Stereo Link knob.
- Make a version with 5 ms of lookahead, and render both to compare peak levels.

## Harmonic EQ

A parametric equalizer with its bands on the harmonics of one fundamental. It boosts, cuts or tilts the overtones of a tone, favours the odd or even harmonics, or notches mains hum and its harmonics.

| Parameter | Range |
|---|---|
| Fundamental | 20 to 2000 Hz |
| Harmonics | 1 to 32 |
| First | 1 to 32 |
| Gain | -24 to 24 dB |
| Tilt | -12 to 12 dB per octave of harmonic number |
| Odd | -24 to 24 dB |
| Even | -24 to 24 dB |
| Bandwidth | 0.02 to 1, as a fraction of the fundamental |
| Stretch | 0 to 0.02 |
| Mix | 0 to 1 |
| Bypass | on or off |

One `harmonic_eq` block. [blocks.md](blocks.md#harmonic_eq) has its arguments in full.

Ask your agent:

- Make a hum remover for 60 Hz mains that cuts the first 16 harmonics by 24 dB with narrow bands.
- Add a Note knob that sets the fundamental from a MIDI note number.

## Multiband Compressor

Splits the signal into low, mid and high bands and compresses each on its own, so a loud bass note no longer pulls down the vocals and cymbals above it.

| Parameter | Range |
|---|---|
| Low Crossover | 40 to 1000 Hz |
| High Crossover | 1000 to 12000 Hz |
| Low, Mid and High Threshold | -60 to 0 dB each |
| Ratio | 1 to 20 |
| Attack | 0.1 to 100 ms |
| Release | 10 to 1000 ms |
| Output | -12 to 12 dB |
| Mix | 0 to 1 |
| Bypass | on or off |
| Low, Mid and High Reduction | meters, 0 to 30 dB |

```json
{ "bypass": "bypass", "mix": "mix", "chain": [
  { "split": { "freqs": ["low_cross", "high_cross"], "bands": [
    { "block": "dynamics", "args": { "threshold_db": "low_threshold", ... }, "meters": { "gain_reduction_db": "gr_low" } },
    { "block": "dynamics", "args": { "threshold_db": "mid_threshold", ... }, "meters": { "gain_reduction_db": "gr_mid" } },
    { "block": "dynamics", "args": { "threshold_db": "high_threshold", ... }, "meters": { "gain_reduction_db": "gr_high" } }
  ] } },
  { "block": "gain", "args": { "gain_db": "output" } }
] }
```

With only the low band's threshold pulled down, noise at -12 dBFS loses about 10 dB below 125 Hz, and the octaves from 500 Hz up stay within 0.2 dB. The low band's meter reads 9.5 dB and the others 0.

Ask your agent:

- Add a solo switch for each band.
- Give each band its own ratio and makeup.
- Add a fourth band with a third crossover.

## Ducker

Turns the track down whenever its sidechain plays: a bass under a kick drum, or music under a voice. In the DAW, route the other track to the plugin's sidechain input. With nothing routed, it does nothing.

| Parameter | Range |
|---|---|
| Threshold | -60 to 0 dB, of the sidechain |
| Ratio | 1 to 20 |
| Attack | 0.1 to 50 ms |
| Release | 10 to 1000 ms |
| Sidechain High-Pass | 10 to 500 Hz |
| Pump | 0 to 12 dB |
| Mix | 0 to 1 |
| Reduction | meter, 0 to 40 dB |

```json
{ "modulators": [{ "id": "hit", "shape": "envelope", "source": "sidechain", "attack_ms": 1, "release_ms": "release" }],
  "mix": "mix", "chain": [
  { "block": "dynamics", "args": { "key": "sidechain", "threshold_db": "threshold", ... }, "meters": { "gain_reduction_db": "gr" } },
  { "block": "gain", "args": { "gain_db": "-pump * min(hit, 1)" } }
] }
```

A dynamics block with `"key": "sidechain"` compresses the track by the sidechain's level, and Pump adds a dip that follows the sidechain's envelope. With a 60 Hz sine at -6 dBFS in the sidechain, noise on the track comes down 20.6 dB at every octave. With the sidechain silent, nothing changes. `render` takes the sidechain signal as `sidechain`.

Ask your agent:

- Add a Range knob so the ducking stops at a set depth.
- Add a band-pass on the sidechain so only the kick's thump triggers it.
- Make a version that ducks only the low end, inside a split.

## Ring Mod

Multiplies the input by a tone. At Blend 0 it is a ring modulator, which replaces each pitch with the sum and difference of it and the carrier, for bells and robot voices. At Blend 1 the input stays and gains sidebands, which is amplitude modulation.

| Parameter | Range |
|---|---|
| Frequency | 1 to 5000 Hz |
| Shape | Sine, Triangle, Saw or Square |
| Drift | 0 to 1 octave |
| Blend | 0 to 1 |
| Mix | 0 to 1 |

```json
{ "modulators": [{ "id": "lfo", "shape": "sine", "rate": 0.2 }],
  "mix": "mix", "multiply": [
  { "chain": [] },
  { "block": "oscillator", "args": { "shape": "shape", "freq_hz": "freq * 2 ^ (drift * lfo)", "level_db": "blend * -6", "offset": "blend" } }
] }
```

A 1 kHz tone through a 300 Hz carrier comes out at 700 and 1300 Hz: the octave band at 500 Hz rises from -99 to -15 dBFS, and the 1 kHz band holds only the 1300 Hz half.

Ask your agent:

- Let the carrier follow a MIDI note.
- Add a tremolo mode that runs the carrier below 20 Hz.
- Feed the carrier from an envelope of the input, so loud notes ring harder.

## Clean Clipper

A clipper run at four times the sample rate. The harmonics a clipper makes above the top of the audio band are filtered out before the rate comes back down, instead of folding back as harsh tones that do not belong to the note.

| Parameter | Range |
|---|---|
| Drive | 0 to 36 dB |
| Curve | Tanh, Hard, Softclip, Fold or Tube |
| Tone | 1000 to 20000 Hz |
| Output | -24 to 0 dB |
| Mix | 0 to 1 |

```json
{ "mix": "mix", "chain": [
  { "oversample": { "factor": 4, "node": { "block": "shaper", "args": { "curve": "curve", "drive_db": "drive" } } } },
  { "block": "filter", "args": { "type": "lowpass", "freq": "tone" } },
  { "block": "gain", "args": { "gain_db": "output" } }
] }
```

A 7 kHz sine at -6 dBFS hard-clipped with 24 dB of drive has no harmonics below it, so whatever lands there is aliasing. The same graph without the oversample node puts it at -24.5 dBFS in the 1 kHz band and -31.5 in the 4 kHz band. Oversampled, those fall to -61.1 and -56.4. The oversample node adds 38 samples of latency, which the plugin reports.

Ask your agent:

- Add a switch between 2x and 8x, and compare the renders.
- Put the tone filter inside the oversample node too.
- Add an Asymmetry knob through the shaper's bias.

## Maximizer

A brickwall limiter for the end of a mix. Gain pushes the level up against the ceiling, and no sample of the output goes over it.

| Parameter | Range |
|---|---|
| Gain | 0 to 24 dB |
| Ceiling | -12 to 0 dB |
| Release | 1 to 1000 ms |
| Reduction | meter, 0 to 30 dB |

One `limiter` block with 1.5 ms of lookahead, which is 72 samples of latency at 48 kHz. Noise peaking at -12 dBFS with 18 dB of gain comes out peaking at exactly -1.0 dBFS and 11 dB louder.

Ask your agent:

- Run the limiter inside a 4x oversample node and measure how far the peaks between samples drop.
- Add an Auto Release that shortens on dense material.
- Add a loudness readout once loudness metering exists.

## RUSE 808

An 808-style drum machine: kick, snare, clap, rim, hats, toms, ride, crash and cowbell, synthesized from oscillators, noise and filters and played from MIDI on the General MIDI drum notes. A closed hat chokes a ringing open hat. Its parameters include Level, Tune, Kick decay and Snappy.

It is written by hand in `src/`, and runs as an Audio Unit instrument. `package_plugin` packages effects only, so `examples/kit-808-au/bundle.sh` builds and installs it. A graph has an oscillator block but no MIDI input or voices yet, so an instrument cannot be built from a graph.
