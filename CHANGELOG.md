# Changelog

Each release lists what changed for people building plugins with RUSE. A change that needs work on your side, such as a renamed block argument or a new manifest format, is marked **Breaking** and says what to do.

## 0.2.0 (2026-10-19)

Six new blocks, five new kinds of node, sidechain input in both wrappers and six new examples. A graph can now split the signal into bands, oversample a shaper, run a filter inside a feedback loop, follow the level of any point in the graph, and generate tones and noise.

### Framework

- An envelope modulator follows the level of the plugin's input, with its own attack and release, and any argument can read it like an LFO. Auto-wah, level-driven saturation and envelope filters are now graphs, with no code to write: `{"id": "env", "shape": "envelope", "attack_ms": 5, "release_ms": 120}`. With `"source"` it follows the sidechain or a tap instead.
- A feedback node runs any node in a loop, so a filter, a saturator or anything else can sit inside the feedback path: `{"feedback": {"node": ..., "gain": "repeats", "delay_ms": "sync(0.75)"}}`. Dub echoes, filtered and saturated feedback and feedback shimmer are now graphs. A loop can be as short as one sample: one under 32 samples runs its node one sample at a time, so combs, resonators and Karplus-Strong strings are graphs too, with `"delay_ms": "1000 / note(57)"`. The loop gain reaches 0.999, so a string can ring.
- `oscillator`, a tone generator: sine, triangle, and band-limited saw, square and pulse, with a frequency any expression can set, such as `note(57)`. It replaces the signal or adds to it. `stereo_phase` starts each channel further round the cycle, for a ring modulator or tremolo that moves across the stereo field. A generator's tail never ends, and tails now add without overflowing.
- `noise`, white, pink or brown noise at a set RMS level, replacing the signal or added to it, with a width from one noise in every channel to a different noise in each. It is seeded, so two instances match.
- A multiply node gives its nodes the same input and multiplies their outputs. With an oscillator in one, it is a ring modulator, or amplitude modulation with the oscillator's offset at 1: `{"multiply": [{"chain": []}, {"block": "oscillator", "args": {"freq_hz": "carrier"}}]}`.
- Subgraphs: a named piece of graph with its own arguments, used as a node with `{"subgraph": "parametric_eq", "args": {"low_gain_db": "bass"}}`. A graph can declare its own under `"subgraphs"`, and one can use another. RUSE ships `parametric_eq`, a four-band equalizer of filter blocks.
- `reverb`, a feedback delay network reverb with size, decay time, pre-delay, damping, diffusion, modulation and stereo width. `freeze` holds the tail without loss and keeps new input out, for pads and drones. `early` sets the level of a dozen early reflections that spread with size and differ in each channel. It outputs only the reverb, like `delay`, so give the node a mix.
- A split node divides the signal into two to four bands with Linkwitz-Riley crossovers at 24 or 48 dB per octave, runs each band through its own node and adds them back flat. A dynamics block in each band makes a multiband compressor: `{"split": {"freqs": ["low_x", "high_x"], "bands": [node, node, node]}}`. Crossovers can follow parameters and modulators, and bands with different latency are lined up.
- An oversample node runs any node at 2, 4 or 8 times the sample rate through linear-phase half-band filters, so a clipper or saturator inside it stops folding its harmonics back as aliases: `{"oversample": {"factor": 4, "node": {"block": "shaper", "args": {"curve": "hard", "drive_db": "drive"}}}}`. It is flat to 20 kHz at 48 kHz and adds 32, 38 or 40 samples of latency, which the plugin reports.
- `limiter`, a brickwall peak limiter with input gain, ceiling, release and lookahead. It ramps the gain down ahead of each peak so no sample goes over the ceiling, and leaves anything under it untouched. It reports its gain reduction as a meter.
- `bitcrusher`, lower resolution and a slower sample clock on purpose, with optional TPDF dither that stops when the input goes silent. Bits can be fractional, so both settings can follow a parameter or a modulator.
- `pitch_shift`, a shifter of up to two octaves either way in semitones and cents, made of two sweeping taps on a delay line that take turns. With a mix it is a harmonizer, an octaver or a detuned double. Its `formant` argument keeps a voice's formants where they were, so a shifted singer still sounds like the same person: linear prediction measures the envelope of the input and of the shifted sound every 5 ms, and two lattice filters swap one for the other. On a sung vowel it brings the formants three quarters of the way back.
- A tap node marks a point an envelope can follow, with `"source"` set to the tap's name: `{"tap": {"name": "ess", "node": {"block": "filter", ...}}}`. The signal passes it unchanged, and the envelope hears what the tap's optional node makes of it. A tap in a split band hears only that band, so a de-esser or a band that ducks another is a graph. A tap cannot sit inside an oversample node or a feedback loop.
- Sidechain input. `Plugin::process_with_sidechain` gives a plugin with a second input bus the audio on it. The CLAP wrapper offers that bus as a second input port, and the Audio Unit as a second input element, which auval validates. In a graph, an envelope with `"source": "sidechain"` follows it and the dynamics block's new `key` argument compresses by it, so a kick on the sidechain can duck a bass. A graph that listens to a sidechain gets the bus without asking. Blocks receive the sidechain through `Block::key`.
- A graph plugin reports its real tail to the host. It used to report none, because the graph only read the parameters once audio ran, and a host asks before that, so it could cut off a delay's repeats at the end of a bounce. The tail now follows the parameters as they change. A graph plugin picks this up the next time it is changed or packaged, which regenerates its code.

### Examples

- Six new examples, each a graph that shows one of the new parts at work: Multiband Compressor (`split`), Ducker (the sidechain), Ring Mod (`multiply` and `oscillator`), Clean Clipper (`oversample`), Maximizer (`limiter`) and De-Esser (a tap inside a split band). Each passes the gates and packages as an Audio Unit and a CLAP plugin, and CI validates every Audio Unit with auval.

### Gates

- **Breaking:** `finite_at_extremes` also fails a plugin that some combination of settings makes more than 30 dB louder than its input. It tries every combination of parameter limits and 24 settings in between, with noise and a 55 Hz sine at -18 dBFS. Several delays with feedback in one chain could pass the old gate and still run far past full scale. A plugin that fails now names the settings that did it. Give later feedback stages less feedback, lower the top of a range, or end the chain with a limiter.
- A plugin with a sidechain is run with noise in it, and `finite_at_extremes` tries each case with the sidechain silent and with full-scale noise.

### Tools

- `render` takes `sidechain`, a test signal, or `sidechain_wav`, a file, to play into a plugin's sidechain. The CLAP check in `package_plugin` processes a sidechain when the plugin has one.
- `ruse-mcp setup AGENT` prints the settings that connect an agent to your workspace, with your own paths filled in. It knows Claude Code, Claude Desktop, Codex, Gemini CLI, OpenCode, Cursor and VS Code. `ruse-mcp new` now points to it.
- The server's instructions and the `list_blocks` description name the new blocks and nodes, so an agent knows to reach for them before writing code.

### Docs

- Getting started covers adding `~/.cargo/bin` to your PATH when Rust came from Homebrew.
- Examples describes all ten examples with their measured results. Architecture covers the new nodes and the sidechain in both wrappers.

### Known limits

- Packaging and installing work on macOS only. VST3 and Audio Unit v3 are not built.
- `package_plugin` packages effects only. The oscillator and noise make sound, but with no MIDI input a graph cannot make an instrument yet.
- Parameter changes take effect at the start of each block, so automation is not sample-accurate.
- In a graph, a per-channel parameter reads as its value for channel 0.
- The dynamics block's `key` takes the input or the sidechain, not a tap. An envelope can follow a tap.
- `formant` on the pitch shifter suits voices. A pure tone has no formants apart from its pitch, so on one the setting lowers the level.
- There is no editor window.

## 0.1.0 (2026-10-16)

The first public beta. Plugins build and package on macOS.

### Framework

- The `Plugin` trait: channel layouts from 1 to 64 channels, parameters that apply to every channel or to each channel on its own, saved state, latency and tail.
- A CLAP wrapper, and an Audio Unit v2 wrapper that serves effects and instruments that take MIDI.
- `ruse-blocks`: nine DSP blocks (`gain`, `filter`, `onepole`, `shaper`, `dynamics`, `delay`, `tremolo`, `stereo` and `harmonic_eq`), an expression language that binds block arguments to parameters, low-frequency modulators, and a graph runtime that compensates for latency.
- Seven gates every plugin must pass: channel layout, described parameters, finite output at every parameter extreme, no allocation in `process`, identical output from two instances, output independent of the host's block size, and no subnormal numbers.

### Tools

- `ruse-mcp`, an MCP server with twelve tools to create, build, check, render and package plugins.
- `ruse-mcp new DIR` creates a workspace for your own plugins, apart from the RUSE checkout.
- `ruse-mcp call TOOL ARGS` runs one tool from the shell.
- `package_plugin` builds CLAP and Audio Unit bundles, loads each the way a host does and runs its checks, and installs it and runs Apple's `auval` when asked.

### Examples

- `warm-saturator`, `bus-compressor` and `harmonic-eq`, built from blocks.
- `kit-808`, an 808-style drum machine written by hand, which runs as an Audio Unit instrument and is packaged by a script.

### Manifest format

- `manifest.json` carries `"format": 1`. A manifest without it is read as format 1.

### Known limits

- Packaging and installing work on macOS only. VST3 and Audio Unit v3 are not built.
- `package_plugin` packages effects only. There are no generator blocks yet, so a graph cannot make an instrument.
- The shaper does not oversample, so it aliases at high drive.
- Parameter changes take effect at the start of each block, so automation is not sample-accurate.
- In a graph, a per-channel parameter reads as its value for channel 0.
- There is no editor window.
