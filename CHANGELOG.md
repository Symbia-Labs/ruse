# Changelog

Each release lists what changed for people building plugins with RUSE. A change that needs work on your side, such as a renamed block argument or a new manifest format, is marked **Breaking** and says what to do.

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
