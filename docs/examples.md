# Examples

MUSE ships four plugins in `examples/`. The first three are built from blocks, and a workspace made with `muse-mcp new` starts with copies of them, so you can try them, change them or build on them. Each section ends with a few things to ask your agent.

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

## MUSE 808

An 808-style drum machine: kick, snare, clap, rim, hats, toms, ride, crash and cowbell, synthesized from oscillators, noise and filters and played from MIDI on the General MIDI drum notes. A closed hat chokes a ringing open hat. Its parameters include Level, Tune, Kick decay and Snappy.

It is written by hand in `src/`, and runs as an Audio Unit instrument. `package_plugin` packages effects only, so `examples/kit-808-au/bundle.sh` builds and installs it. There are no generator blocks yet, so an instrument cannot be built from a graph.
