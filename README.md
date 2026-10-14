# MUSE

MUSE is a Rust framework for writing audio plugins. All of it is new code, with nothing forked or ported from JUCE or any other framework.

The aim is to replace JUCE. A person downloads MUSE, connects an agent and describes the plugin they want. The agent designs, builds and tests it through MUSE's MCP tools, so the person never has to deal with the code.

A plugin still declares what it is one time. Every tool, format wrapper and host window then reads that same declaration.

## Goals

- **MCP is how plugins get made.** The agent and the framework's tools carry the whole job of designing and building a plugin. MCP hooks inside a finished plugin are welcome and come second.
- **All new code.** MUSE starts from an empty repository and stays free of anyone else's source.
- **Parameters describe themselves.** Each one carries a name, unit, range, curve, choices, scope and group. A knob in a window and a tool call from an agent therefore share one definition.
- **Many channels from the start.** A plugin can run on one channel or on sixty-four. Its layout declares the range up front.
- **A quiet audio thread.** `process` never allocates, locks or blocks.
- **MIT OR Apache-2.0.** Commercial plugins can be built on it.

## How it fits together

The `muse-core` crate holds the whole contract between a plugin and whatever runs it. A plugin implements one trait.

```rust
pub trait Plugin: Send {
    fn layout(&self) -> Layout;
    fn params(&self) -> &ParamSet;
    fn prepare(&mut self, setup: &Setup);   // non-audio thread, allocate here
    fn latency_samples(&self) -> u32 { 0 }
    fn tail_samples(&self) -> u32 { 0 }
    fn process(&mut self, channels: &mut [&mut [f32]], ctx: &ProcessContext);
}
```

**Layout.** Each bus declares its channel count as `Fixed(n)`, `Range { min, max }` or `SameAsInput`. `Layout::accepts` answers whether a host's proposal suits the plugin.

**Parameters.** `ParamInfo::knob`, `choice`, `toggle` and `meter` build a declaration in a `const`. Adding `.per_channel()` gives a parameter one value for each channel. Without it, one value is shared by all of them. The framework allocates every slot up front, so the audio thread only reads and writes atomics.

Control-side writes go through `ParamSet::set_plain` for every channel, or `set_plain_ch` for one. A global parameter refuses a channel address, and a meter refuses all writes. `on_change` registers one listener that runs after each write, which is where a plugin rebuilds anything slow.

**Handoff.** `Handoff<T>` passes a freshly built value from a control thread to the audio thread. The control thread calls `post`. The audio thread calls `take` at the top of a block. That call never allocates, locks or frees.

**Description.** `describe_params` turns a `ParamSet` into JSON. It lists each parameter's scope, group, range and choices. A per-channel parameter also lists its current value and text for every channel. An MCP client reads this document, whether it is building the plugin or controlling a finished one.

## A complete plugin

`muse-gain` is the smallest one. This is the core of it.

```rust
pub const GAIN: ParamInfo = ParamInfo {
    description: "Level change applied to every channel.",
    group: "Output",
    ..ParamInfo::knob("gain", "Gain", "dB", -60.0, 12.0, 0.0)
};

const LAYOUT: Layout = Layout {
    inputs: &[BusInfo { name: "Main", channels: Channels::Range { min: 1, max: 64 } }],
    outputs: &[BusInfo { name: "Main", channels: Channels::SameAsInput }],
};

impl Plugin for Gain {
    fn layout(&self) -> Layout { LAYOUT }
    fn params(&self) -> &ParamSet { &self.params }
    fn prepare(&mut self, _setup: &Setup) {}

    fn process(&mut self, channels: &mut [&mut [f32]], _ctx: &ProcessContext) {
        let g = 10f32.powf(self.params.at(0).get() / 20.0);
        for ch in channels.iter_mut() {
            for s in ch.iter_mut() {
                *s *= g;
            }
        }
    }
}
```

## Building a plugin with an agent

`muse-mcp` is an MCP server that runs on your machine. Connect an agent to it, describe the plugin you want, and the agent builds it through these tools.

| Tool | What it does |
|---|---|
| `create_plugin` | Generates a plugin crate from a list of parameters |
| `update_plugin` | Changes the parameters, channel range or description and regenerates the declaration |
| `write_dsp` | Replaces the signal processing in `src/dsp.rs` and builds it |
| `read_dsp` | Returns the current source and the accessor each parameter has |
| `build` | Builds the plugin and reports compiler output |
| `check` | Runs the gates every plugin must pass |
| `package_plugin` | Builds a CLAP bundle, an Audio Unit or both, checks each by loading it the way a host does, and installs and validates them when asked |
| `render` | Runs the plugin on a test signal or a WAV file and reports levels and octave bands before and after |
| `describe` | Returns the parameter description as JSON |
| `list_plugins` | Lists what the workspace holds |

The agent never writes the parameter declaration or the glue code. MUSE generates those from `manifest.json`. The agent writes only `dsp.rs`, where parameters arrive through accessors such as `p.drive()`.

The gates are what make a plugin built this way trustworthy. `check` fails a plugin whose `process` allocates, whose output goes non-finite or far too loud at any parameter limit, whose two fresh instances disagree, or whose audio changes with the host's block size. Every parameter also needs a description. Unsafe code is refused outright.

`package_plugin` is configured through the same tools. The first call needs a vendor name, and the settings are saved in `manifest.json`, so later calls need only the plugin's name. A plugin that fails the gates is not packaged. The `formats` setting picks `clap`, `au` or both, and is saved too. The tool builds the shared library, assembles and signs the bundle, loads it the way a host does and runs the checks, 17 for CLAP and 25 for an Audio Unit. The Audio Unit's four-character codes come from the plugin and vendor names unless the call gives them. Installing into the plug-in folders happens only when the call sets `install` to true, and only after every check has passed. An installed Audio Unit is then run through Apple's `auval`.

To connect it, build the server and point an MCP client at the binary. Claude Code, for example, takes the second line below.

```
cargo build --release -p muse-mcp
claude mcp add muse -- /path/to/MUSE/target/release/muse-mcp
```

`muse-mcp call TOOL JSON` runs a single tool from a terminal, which is handy for trying one without a client.

## Crates

| Crate | Holds |
|---|---|
| `muse-core` | The `Plugin` trait, layouts, parameters, `Handoff` and the JSON description |
| `muse-gain` | The minimal plugin above |
| `muse-tape` | A tape machine port, used as the large test case for channels and parameters |
| `muse-host` | WAV files, test signals, offline rendering with measurements, and the gates |
| `muse-clap` | The CLAP wrapper, and `muse-clap-verify`, which loads a built bundle and exercises it |
| `muse-au` | The Audio Unit (v2 effect) wrapper for macOS, and `muse-au-verify`, which loads a built component and exercises it |
| `muse-mcp` | The MCP server an agent uses to create, build, check, package and render plugins |

Plugins made through the server land in `plugins/`. The first is `warm-saturator`, a clipper followed by a low-pass, which an agent wrote through these tools. Packaging it adds a companion crate beside it, `plugins/warm-saturator-clap`, so the plugin's own crate stays free of unsafe code.

## The CLAP wrapper

The wrapper exposes a plugin's first input bus as one CLAP audio port in and one out. The host can change the channel count within the range the plugin declares. A per-channel parameter appears as one CLAP parameter for each channel, and the ids of the existing ones stay put when the count changes. State saves every value of every parameter. Latency and tail come from values the audio thread keeps current, and a change of latency reaches the host on its main thread.

The audio thread alone runs `process`. The wrapper's main-thread calls work through a shared handle to the plugin's `ParamSet`, and tests count allocations to confirm that `process` makes none. Parameter values arrive at the start of each block, so automation is not yet sample-accurate.

The C layouts are checked against the real CLAP 1.2.10 header. `crates/clap/tests/abi/run.sh` compiles a small host in C against that header and loads a built bundle with it. The CLAP project's own `clap-validator` also runs clean on a packaged plugin.

## The Audio Unit wrapper

The Audio Unit is a version 2 effect, which Logic, GarageBand and other macOS hosts load. The host calls the plugin through a table of numbered selectors, and the wrapper answers the properties an effect needs: stream formats, parameter lists and info, value strings, text conversion, state, latency, tail, host callbacks and the preset name. The input and the output each carry a channel count, and the unit refuses to initialize when they differ. Parameters follow the output.

Rendering pulls its input from the host's render callback or from a connected unit, runs the plugin in place, and does not allocate. Both wrappers turn on flush-to-zero while `process` runs, so a filter that decays toward silence never hands the host a subnormal number. Scheduled parameter events take effect at the next render, and a ramp lands on its end value.

The layouts and constants are checked against the macOS SDK headers by `crates/au/tests/abi/layout.c`. Apple's `auval` validates an installed unit.

## Current state

Working today:

- The core API described above.
- A gain plugin and a tape plugin that both run on 1 to 64 channels.
- An MCP server with ten tools. An agent can create a plugin from a list of parameters, write its signal processing, build it, run the gates, render audio with measurements and package it as a CLAP plugin or an Audio Unit, without a person reading any code.
- Six gates. Each has a faulty plugin written to fail it, so a gate that goes quiet shows up in the tests.
- A CLAP wrapper. A packaged plugin passes every check in `muse-clap-verify`, in a host written in C against the real header, and in `clap-validator`.
- An Audio Unit wrapper. A packaged plugin passes every check in `muse-au-verify`, and Apple's `auval` reports AU VALIDATION SUCCEEDED for it.
- 148 tests, including tests that count allocations while `process` runs and fail if there are any.

Not built yet:

- **A real DAW.** Nobody has loaded a MUSE plugin in one yet.
- **Other formats.** CLAP and the Audio Unit v2 effect exist. VST3 and Audio Unit v3 are not built, and neither are Audio Unit instruments or MIDI effects.
- **MCP hooks inside a finished plugin.** `describe_params` produces the description, and no running plugin serves it yet. This comes second.
- **A DSP block library.** Agents write `dsp.rs` from scratch today. Named filters, delays and saturators would let them compose instead.
- **More measurements.** `render` reports levels and octave bands. Distortion, loudness and a spectrogram would widen what an agent can verify.
- **A window.** There is no editor UI.
- **Linked instances.** The plan is a shared hub that many one-channel inserts attach to by machine name, so a DAW without wide channel support can still run a multichannel machine.
- **Channel grouping,** so that a stereo pair moves together.
- **Licence files.** The crates declare `MIT OR Apache-2.0`, and the two licence texts are not in the repository yet.

## Build and test

```
cargo test
```

The dev profile builds at `opt-level = 3`, since the DSP tests render seconds of audio.
