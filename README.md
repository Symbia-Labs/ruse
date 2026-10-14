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

## Crates

| Crate | Holds |
|---|---|
| `muse-core` | The `Plugin` trait, layouts, parameters, `Handoff` and the JSON description |
| `muse-gain` | The minimal plugin above |
| `muse-tape` | A tape machine port, used as the large test case for channels and parameters |

## Current state

Working today:

- The core API described above.
- A gain plugin and a tape plugin that both run on 1 to 64 channels.
- 68 tests, including tests that count allocations while `process` runs on 24 channels and fail if there are any.

Not built yet:

- **Format wrappers.** No host can load a MUSE plugin. CLAP is next.
- **The MCP server.** This is the main piece still to build. Its tools would scaffold a plugin, declare its layout and parameters, build it, run the checks and render audio. `describe_params` produces the description today, and nothing serves it yet.
- **A window.** There is no editor UI.
- **Linked instances.** The plan is a shared hub that many one-channel inserts attach to by machine name, so a DAW without wide channel support can still run a multichannel machine.
- **Channel grouping,** so that a stereo pair moves together.
- **Latency changes.** A plugin cannot yet tell the host that its latency changed.
- **Licence files.** The crates declare `MIT OR Apache-2.0`, and the two licence texts are not in the repository yet.

## Build and test

```
cargo test
```

The dev profile builds at `opt-level = 3`, since the DSP tests render seconds of audio.
