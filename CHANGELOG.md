# Changelog

Each release lists what changed for people building plugins with RUSE. A change that needs work on your side, such as a renamed block argument or a new manifest format, is marked **Breaking** and says what to do.

## Unreleased

### Framework

- An envelope modulator follows the level of the plugin's input, with its own attack and release, and any argument can read it like an LFO. Auto-wah, level-driven saturation and envelope filters are now graphs, with no code to write: `{"id": "env", "shape": "envelope", "attack_ms": 5, "release_ms": 120}`.
- A feedback node runs any node in a loop, so a filter, a saturator or anything else can sit inside the feedback path: `{"feedback": {"node": ..., "gain": "repeats", "delay_ms": "sync(0.75)"}}`. Dub echoes, filtered and saturated feedback and feedback shimmer are now graphs. The loop is at least 32 samples long, about 0.7 ms at 48 kHz.
- `reverb`, a feedback delay network reverb with size, decay time, pre-delay, damping, diffusion, modulation and stereo width. It outputs only the reverb, like `delay`, so give the node a mix.

- A graph plugin reports its real tail to the host. It used to report none, because the graph only read the parameters once audio ran, and a host asks before that, so it could cut off a delay's repeats at the end of a bounce. The tail now follows the parameters as they change. A graph plugin picks this up the next time it is changed or packaged, which regenerates its code.

### Gates

- **Breaking:** `finite_at_extremes` also fails a plugin that some combination of settings makes more than 30 dB louder than its input. It tries every combination of parameter limits and 24 settings in between, with noise and a 55 Hz sine at -18 dBFS. Several delays with feedback in one chain could pass the old gate and still run far past full scale. A plugin that fails now names the settings that did it. Give later feedback stages less feedback, lower the top of a range, or end the chain with a limiter.

### Tools

- `ruse-mcp setup AGENT` prints the settings that connect an agent to your workspace, with your own paths filled in. It knows Claude Code, Claude Desktop, Codex, Gemini CLI, OpenCode, Cursor and VS Code. `ruse-mcp new` now points to it.

### Docs

- Getting started covers adding `~/.cargo/bin` to your PATH when Rust came from Homebrew.

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
