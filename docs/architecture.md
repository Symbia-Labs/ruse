# Architecture

This page is for people who write DSP by hand or work on RUSE itself. To build plugins through an agent, [getting-started.md](getting-started.md) is enough.

## Crates

| Crate | Holds |
|---|---|
| `ruse-core` | The `Plugin` trait, layouts, parameters, saved state, `Handoff` and the JSON description |
| `ruse-blocks` | The DSP blocks, the expression language and the graph runtime |
| `ruse-host` | WAV files, test signals, offline rendering with measurements, and the gates |
| `ruse-clap` | The CLAP wrapper, and `ruse-clap-verify`, which loads a built bundle and exercises it |
| `ruse-au` | The Audio Unit v2 wrapper for macOS, and `ruse-au-verify`, which loads a built component and exercises it |
| `ruse-mcp` | The MCP server and the `ruse-mcp` command |
| `ruse-gain` | The smallest complete plugin, shown below |
| `ruse-tape` | A multichannel tape machine, the large test case for channels and parameters |

RUSE has no dependencies outside this repository.

## The plugin contract

`ruse-core` holds the whole contract between a plugin and whatever runs it. A plugin implements one trait.

```rust
pub trait Plugin: Send {
    fn layout(&self) -> Layout;
    fn params(&self) -> &ParamSet;
    fn prepare(&mut self, setup: &Setup);   // not the audio thread, so allocate here
    fn latency_samples(&self) -> u32 { 0 }
    fn tail_samples(&self) -> u32 { 0 }
    fn process(&mut self, channels: &mut [&mut [f32]], ctx: &ProcessContext);
}
```

An instrument also overrides `is_instrument` and takes MIDI through `midi`.

**Layout.** Each bus declares its channel count as `Fixed(n)`, `Range { min, max }` or `SameAsInput`. `Layout::accepts` answers whether a host's proposal suits the plugin.

**Parameters.** `ParamInfo::knob`, `choice`, `toggle` and `meter` build a declaration in a `const`. Adding `.per_channel()` gives a parameter one value for each channel. Without it, one value is shared by all of them. The framework allocates every slot up front, so the audio thread only reads and writes atomics.

Control-side writes go through `ParamSet::set_plain` for every channel, or `set_plain_ch` for one. A global parameter refuses a channel address, and a meter refuses all writes. `on_change` registers one listener that runs after each write, which is where a plugin rebuilds anything slow.

**State.** `ruse_core::state` saves every value of every parameter except meters, which are output and not settings. A state saved with more or fewer parameters than the plugin has loads as far as it fits.

**Handoff.** `Handoff<T>` passes a freshly built value from a control thread to the audio thread. The control thread calls `post`. The audio thread calls `take` at the top of a block, and that call never allocates, locks or frees.

**Description.** `describe_params` turns a `ParamSet` into JSON, with each parameter's scope, group, range and choices, and for a per-channel parameter its value and text on every channel.

## A complete plugin

`ruse-gain` is the smallest one. This is the core of it.

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

## The rules for `process`

`process` runs on the audio thread. It must not allocate, lock, block, read files or print. Allocate in `prepare`, which runs elsewhere. The [gates](gates.md) check the allocations, the output at every parameter limit, repeatability, block-size independence and subnormal numbers.

## Generated plugins

A plugin made through the tools has a `manifest.json` that declares it. `ruse-mcp` generates the parameter declaration, `lib.rs`, a render program and the gates test from it. `dsp.rs` holds the signal processing, either generated from a graph or written through `write_dsp`, and reaches parameters through accessors such as `p.drive()`. Each generated `Cargo.toml` names the RUSE crates as `{ workspace = true }`, so the same plugin builds in this repository and in a workspace made by `ruse-mcp new`.

## The graph runtime

`ruse-blocks` runs a plugin described as a [graph](graph-format.md) of [blocks](blocks.md). The graph is JSON in `manifest.json`, and the generated `dsp.rs` loads it into a `Graph` when an instance of the plugin is created.

Controls update once for every 32 samples, on a grid counted from the start of the stream and not from the host's blocks. Each block glides from the previous control value to the new one by sample position, so the audio at any host block size matches to within a millionth. Blocks that look ahead add latency, which the graph sums and reports, and parallel branches and dry paths are delayed to stay in time. A decaying state is flushed before it can go subnormal, and `process` does not allocate.

Each block has measured tests, such as the response of every filter type, the static curve of the compressor, the echo times of the delay and the gain at each harmonic of the equalizer.

## The CLAP wrapper

The wrapper exposes a plugin's first input bus as one CLAP audio port in and one out. The host can change the channel count within the range the plugin declares. A per-channel parameter appears as one CLAP parameter for each channel, and the ids of the existing ones stay put when the count changes. Latency and tail come from values the audio thread keeps current, and a change of latency reaches the host on its main thread.

The audio thread alone runs `process`. The wrapper's main-thread calls work through a shared handle to the plugin's `ParamSet`. Parameter values arrive at the start of each block, so automation is not yet sample-accurate.

The C layouts are checked against the CLAP 1.2.10 header. `crates/clap/tests/abi/run.sh` compiles a small host in C against that header and loads a built bundle with it. The CLAP project's `clap-validator` also runs clean on a packaged plugin.

## The Audio Unit wrapper

The Audio Unit is a version 2 unit, an effect or a music device, which Logic, GarageBand and other macOS hosts load. The host calls the plugin through a table of numbered selectors, and the wrapper answers the properties a unit needs: stream formats, parameter lists and info, value strings, text conversion, state, latency, tail, host callbacks and the preset name. An effect's input and output each carry a channel count, and the unit refuses to initialize when they differ.

Rendering pulls input from the host's render callback or from a connected unit, runs the plugin in place, and does not allocate. Both wrappers turn on flush-to-zero while `process` runs. Scheduled parameter events take effect at the next render, and a ramp lands on its end value.

The layouts and constants are checked against the macOS SDK headers by `crates/au/tests/abi/layout.c`.

## Building and testing RUSE

```
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

The dev profile builds at `opt-level = 3`, because the DSP tests render seconds of audio. CI runs both on macOS for every push and pull request, then packages every example, installs it on the runner and runs `auval` on each Audio Unit.

After changing a block or the format guide, run `ruse-mcp docs` to rewrite [blocks.md](blocks.md) and [graph-format.md](graph-format.md). A test fails when they are out of date.
