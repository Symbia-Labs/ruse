# MUSE

MUSE is a Rust framework for audio plugins. You describe the plugin you want, and an agent builds it through MUSE's MCP tools. You never have to deal with the code.

All of MUSE is new code, with nothing forked or ported from JUCE or any other framework. The aim is to replace JUCE.

MUSE is in public beta at version 0.1.0. Plugins build and package on macOS, as CLAP plugins and Audio Units.

## Quick start

```
git clone https://github.com/Symbia-Labs/muse
cd muse
cargo install --path crates/mcp
muse-mcp new ~/my-plugins
```

Then connect your agent:

| Agent | Setup |
|---|---|
| Claude Code | [One command](docs/getting-started.md#claude-code) |
| Claude Desktop | [Config file](docs/getting-started.md#claude-desktop) |
| ChatGPT, through Codex | [Codex config file](docs/getting-started.md#chatgpt-and-codex) |
| Gemini CLI | [One command](docs/getting-started.md#gemini-cli) |
| OpenCode | [Config file](docs/getting-started.md#opencode) |
| Cursor | [Config file](docs/getting-started.md#cursor) |
| VS Code with GitHub Copilot | [Config file](docs/getting-started.md#vs-code-with-github-copilot) |
| Any other MCP client | [Command and environment](docs/getting-started.md#other-clients) |

Ask it for a plugin, such as a tilt equalizer with one knob. [Getting started](docs/getting-started.md) walks through it.

## What is in it

- **Twelve MCP tools** to create, build, check, render and package plugins. Every tool also runs from the shell.
- **Nine DSP blocks** and a graph format, so most plugins need no code: filters, a parametric and a harmonic equalizer, a compressor, limiter, gate and expander, saturation, delay with chorus and flanger, tremolo and mid/side. See [blocks](docs/blocks.md).
- **Seven gates** every plugin must pass before it can be packaged, covering allocation on the audio thread, output at every parameter limit, repeatability, block-size independence and more. See [gates](docs/gates.md).
- **CLAP and Audio Unit packaging** that loads each bundle the way a host does and checks it, then installs it and runs Apple's `auval`. See [packaging](docs/packaging.md).
- **Plugins from 1 to 64 channels**, with parameters that apply to every channel or to each one on its own.
- **Four examples**: a saturator, a bus compressor, a harmonic equalizer and an 808-style drum machine. See [examples](docs/examples.md).

## Goals

- **MCP is how plugins get made.** The agent and the framework's tools carry the whole job of designing and building a plugin. MCP hooks inside a finished plugin come second.
- **All new code.** MUSE has no dependencies outside this repository.
- **Parameters describe themselves.** Each one carries a name, description, unit, range, curve, choices, scope and group, so a knob in a window and a tool call from an agent share one definition.
- **Many channels from the start.** A plugin can run on one channel or on sixty-four.
- **A quiet audio thread.** `process` never allocates, locks or blocks.

## Not built yet

- VST3, Audio Unit v3 and packaging on Windows and Linux.
- Generator blocks and instrument packaging. MUSE 808 shows an instrument written by hand.
- Oversampling, reverb, convolution, spectral processing, pitch detection and shifting, envelope followers, and loudness and spectrum meters.
- Sample-accurate automation, and per-channel parameters inside a graph.
- An editor window.
- Linked instances, where many one-channel inserts attach to one shared plugin, for hosts without wide channel support.

[CHANGELOG.md](CHANGELOG.md) has the full list of what 0.1.0 holds and its known limits.

## Documentation

[docs/](docs/README.md) covers the tools, the graph format, every block, the gates, packaging, the examples and the architecture.

## License

MIT. See [LICENSE](LICENSE). You can build and sell closed-source plugins with MUSE, as long as MUSE's copyright and license notice travel with them.
