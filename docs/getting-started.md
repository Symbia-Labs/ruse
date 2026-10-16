# Getting started

MUSE builds and packages plugins on macOS. You need these installed:

- **Rust**, from [rustup.rs](https://rustup.rs). MUSE builds on the stable toolchain.
- **The Xcode Command Line Tools**, from `xcode-select --install`. They provide `codesign` and `auval`.
- **An agent that speaks MCP**, such as Claude Code or Claude Desktop. Every tool also runs from the shell, so an agent is optional.

## Install

```
git clone https://github.com/Symbia-Labs/muse
cd muse
cargo install --path crates/mcp
```

This builds `muse-mcp` and puts it in `~/.cargo/bin`. Keep the checkout where it is, because `muse-mcp` builds your plugins against it.

## Make a workspace

```
muse-mcp new ~/my-plugins
```

Your plugins live in this workspace, apart from the MUSE checkout, so updating MUSE never conflicts with your work. Its `plugins/` folder starts with copies of Warm Saturator, Bus Compressor and Harmonic EQ. Change them, take them apart, or delete them.

`new` ends by printing the command and the `MUSE_ROOT` setting to give your agent.

## Connect your agent

### Claude Code

From inside the workspace:

```
cd ~/my-plugins
claude mcp add --env MUSE_ROOT=$HOME/my-plugins --transport stdio muse -- $HOME/.cargo/bin/muse-mcp
```

This adds the server for this folder only. Add `--scope user` to have it in every project.

### Claude Desktop

Open Settings, then Developer, then Edit Config. Claude Desktop keeps the file at `~/Library/Application Support/Claude/claude_desktop_config.json`. Add MUSE under `mcpServers`, with full paths in place of `you`:

```json
{
  "mcpServers": {
    "muse": {
      "command": "/Users/you/.cargo/bin/muse-mcp",
      "env": { "MUSE_ROOT": "/Users/you/my-plugins" }
    }
  }
}
```

Quit Claude Desktop and open it again. If MUSE does not appear, its log is `~/Library/Logs/Claude/mcp-server-muse.log`.

### Other clients

Any MCP client that starts a local server over stdio works. Give it `muse-mcp` as the command and set `MUSE_ROOT` to the workspace. Started from inside the workspace folder, `muse-mcp` finds it without `MUSE_ROOT`.

## Your first plugin

Describe the plugin to your agent in plain words. For example:

> Make a tilt equalizer with one Tilt knob from -6 to 6 dB. Positive brightens and negative darkens. Check it and render it.

The agent calls `create_plugin` with the parameter and a graph, here a low shelf and a high shelf at 800 Hz that move in opposite directions:

```json
{
  "chain": [
    { "block": "filter", "args": { "type": "lowshelf", "freq": 800, "gain_db": "-tilt" } },
    { "block": "filter", "args": { "type": "highshelf", "freq": 800, "gain_db": "tilt" } }
  ]
}
```

MUSE generates the plugin's code from that and builds it. `check` runs the [gates](gates.md), and `render` plays test signals through the plugin and reports levels and octave bands before and after, so the agent can confirm the plugin does what you asked.

To hear it in a host, ask for the next step:

> Package it as an Audio Unit and a CLAP plugin, with my name as the vendor, and install it.

`package_plugin` builds both bundles, loads each the way a host does and runs its checks, installs them, and runs Apple's `auval` on the Audio Unit. Open Logic, GarageBand or any CLAP host, and the plugin is in its list. A host that was already open may need to rescan its plug-ins first. [packaging.md](packaging.md) has the details.

## Without an agent

`muse-mcp call` runs one tool from inside the workspace:

```
muse-mcp call list_plugins
muse-mcp call check '{"name":"warm-saturator"}'
muse-mcp call render '{"name":"harmonic-eq"}'
muse-mcp call package_plugin '{"name":"bus-compressor","install":true}'
```

[tools.md](tools.md) lists every tool.

## Updating MUSE

```
cd muse
git pull
cargo install --path crates/mcp
```

[CHANGELOG.md](../CHANGELOG.md) marks any change that needs work on your side.
