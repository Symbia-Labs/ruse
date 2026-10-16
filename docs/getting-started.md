# Getting started

RUSE builds and packages plugins on macOS. You need these installed:

- **Rust**, from [rustup.rs](https://rustup.rs). RUSE builds on the stable toolchain.
- **The Xcode Command Line Tools**, from `xcode-select --install`. They provide `codesign` and `auval`.
- **An agent that speaks MCP**, such as Claude Code or Claude Desktop. Every tool also runs from the shell, so an agent is optional.

## Install

```
git clone https://github.com/Symbia-Labs/ruse
cd ruse
cargo install --path crates/mcp
```

This builds `ruse-mcp` and puts it in `~/.cargo/bin`. Keep the checkout where it is, because `ruse-mcp` builds your plugins against that copy of RUSE.

If cargo warns that `~/.cargo/bin` is not in your PATH, your shell can't find `ruse-mcp` yet. This happens when Rust came from Homebrew rather than rustup. Add the folder to your PATH, then open a new Terminal window:

```
echo 'export PATH="$HOME/.cargo/bin:$PATH"' >> ~/.zprofile
```

## Make a workspace

```
ruse-mcp new ~/my-plugins
```

Your plugins live in this workspace, apart from the RUSE checkout, so updating RUSE never conflicts with your work. Its `plugins/` folder starts with copies of Warm Saturator, Bus Compressor and Harmonic EQ. Change them, take them apart, or delete them.

`new` ends by telling you how to connect an agent, which [the next section](#connect-your-agent) covers.

## What RUSE runs on your Mac

RUSE builds and runs the plugins your agent makes, on your Mac, with your user account's permissions. `build`, `check`, `render` and `package_plugin` compile and run the plugin's code.

For a plugin built from blocks, RUSE writes every line of that code. With `write_dsp`, the agent writes the plugin's `dsp.rs` itself. That file cannot contain unsafe code, and a plugin can depend only on RUSE's own crates, but it is still a program running on your computer, and it can read and write files like any other. Use an agent you trust, and read what it writes when it matters to you.

RUSE itself makes no network connections. `package_plugin` writes to your plug-in folders only when you ask it to install.

## Connect your agent

`ruse-mcp` is a local MCP server. Your agent starts it on your Mac and talks to it over stdio. Every setup gives the agent the same two things: the full path to `ruse-mcp`, and `RUSE_ROOT` set to your workspace.

`ruse-mcp setup` prints the setup for your agent with your own paths filled in. Run it inside your workspace and name your agent:

```
cd ~/my-plugins
ruse-mcp setup claude-desktop
```

It knows `claude-code`, `claude-desktop`, `codex`, `gemini`, `opencode`, `cursor` and `vscode`. The sections below show what it prints for each, for a Mac user named `you`.

| Agent | Setup |
|---|---|
| Claude Code | [One command](#claude-code) |
| Claude Desktop | [Config file](#claude-desktop) |
| ChatGPT and Codex | [Codex config file](#chatgpt-and-codex) |
| Gemini CLI | [One command](#gemini-cli) |
| OpenCode | [Config file](#opencode) |
| Cursor | [Config file](#cursor) |
| VS Code with GitHub Copilot | [Config file](#vs-code-with-github-copilot) |
| Anything else | [Command and environment](#other-clients) |

### Claude Code

From inside the workspace:

```
cd ~/my-plugins
claude mcp add --env RUSE_ROOT=$HOME/my-plugins --transport stdio ruse -- $HOME/.cargo/bin/ruse-mcp
```

This adds the server for this folder only. Add `--scope user` to have it in every project. `ruse-mcp setup claude-code` prints the same command with your workspace's path.

### Claude Desktop

Run `ruse-mcp setup claude-desktop` in your workspace. It prints this, with your own paths in place of `/Users/you`:

```json
{
  "mcpServers": {
    "ruse": {
      "command": "/Users/you/.cargo/bin/ruse-mcp",
      "env": { "RUSE_ROOT": "/Users/you/my-plugins" }
    }
  }
}
```

Put it in `~/Library/Application Support/Claude/claude_desktop_config.json`, which Settings, then Developer, then Edit Config finds for you. If the file already lists other servers under `mcpServers`, add the `ruse` entry beside them. Quit Claude Desktop and open it again. If RUSE does not appear, its log is `~/Library/Logs/Claude/mcp-server-ruse.log`.

### ChatGPT and Codex

ChatGPT in the browser connects only to MCP servers on the internet, so it cannot start `ruse-mcp` on your Mac. Use Codex, OpenAI's coding agent, which comes with every ChatGPT plan and signs in with your ChatGPT account. The Codex CLI, the Codex IDE extension and the ChatGPT desktop app all read `~/.codex/config.toml`. Run `ruse-mcp setup codex` in your workspace to print what to add, with your own paths in place of `/Users/you`:

```toml
[mcp_servers.ruse]
command = "/Users/you/.cargo/bin/ruse-mcp"
tool_timeout_sec = 600

[mcp_servers.ruse.env]
RUSE_ROOT = "/Users/you/my-plugins"
```

Codex stops a tool call after 60 seconds unless `tool_timeout_sec` says otherwise, and a plugin's first build or a package run can take longer than that.

### Gemini CLI

From inside the workspace:

```
cd ~/my-plugins
gemini mcp add -e RUSE_ROOT=$HOME/my-plugins ruse $HOME/.cargo/bin/ruse-mcp
```

This writes `.gemini/settings.json` in the workspace. Add `-s user` to write `~/.gemini/settings.json` instead, so RUSE is there in every folder. `ruse-mcp setup gemini` prints the command with your workspace's path. To edit the file by hand, add RUSE under `mcpServers`, with your own paths in place of `/Users/you`:

```json
{
  "mcpServers": {
    "ruse": {
      "command": "/Users/you/.cargo/bin/ruse-mcp",
      "env": { "RUSE_ROOT": "/Users/you/my-plugins" }
    }
  }
}
```

### OpenCode

Run `ruse-mcp setup opencode` in your workspace. It prints this, with your own paths in place of `/Users/you`. Save it as `opencode.json` at the top of the workspace, or add the `ruse` entry under `mcp` in `~/.config/opencode/opencode.json` to have it everywhere:

```json
{
  "$schema": "https://opencode.ai/config.json",
  "mcp": {
    "ruse": {
      "type": "local",
      "command": ["/Users/you/.cargo/bin/ruse-mcp"],
      "environment": { "RUSE_ROOT": "/Users/you/my-plugins" },
      "enabled": true
    }
  }
}
```

### Cursor

Run `ruse-mcp setup cursor` in your workspace. It prints this, with your own paths in place of `/Users/you`. Save it as `.cursor/mcp.json` in the workspace, or add the `ruse` entry to `~/.cursor/mcp.json` to have it in every project:

```json
{
  "mcpServers": {
    "ruse": {
      "type": "stdio",
      "command": "/Users/you/.cargo/bin/ruse-mcp",
      "env": { "RUSE_ROOT": "/Users/you/my-plugins" }
    }
  }
}
```

### VS Code with GitHub Copilot

Run `ruse-mcp setup vscode` in your workspace. It prints this, with your own path in place of `/Users/you`. Save it as `.vscode/mcp.json` in the workspace, then open the workspace folder in VS Code:

```json
{
  "servers": {
    "ruse": {
      "type": "stdio",
      "command": "/Users/you/.cargo/bin/ruse-mcp",
      "cwd": "${workspaceFolder}"
    }
  }
}
```

VS Code starts the server in the folder you opened, and `ruse-mcp` finds its workspace there, so this one needs no `RUSE_ROOT`. RUSE's tools then show up in Copilot Chat.

### Other clients

Any MCP client that can start a local server over stdio works. Give it the full path to `ruse-mcp` as the command, with no arguments, and set `RUSE_ROOT` to the workspace in its environment. `ruse-mcp new` prints both paths when it makes the workspace. Started from inside the workspace folder, `ruse-mcp` finds it without `RUSE_ROOT`.

If the client stops slow tool calls, allow at least ten minutes. A plugin's first build, and packaging, can take a minute or more.

A client that only reaches MCP servers on the internet, such as ChatGPT in the browser, cannot connect yet. That needs RUSE served over HTTPS with sign-in, which is not built.

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

RUSE generates the plugin's code from that graph and builds the plugin. `check` runs the [gates](gates.md), and `render` plays test signals through the plugin and reports levels and octave bands before and after, so the agent can confirm the plugin does what you asked.

To hear it in a host, ask for the next step:

> Package it as an Audio Unit and a CLAP plugin, with my name as the vendor, and install it.

`package_plugin` builds both bundles, loads each the way a host does and runs its checks, installs them, and runs Apple's `auval` on the Audio Unit. Open Logic, GarageBand or any CLAP host, and the plugin is in its list. A host that was already open may need to rescan its plug-ins first. [packaging.md](packaging.md) has the details.

## Without an agent

`ruse-mcp call` runs one tool from inside the workspace:

```
ruse-mcp call list_plugins
ruse-mcp call check '{"name":"warm-saturator"}'
ruse-mcp call render '{"name":"harmonic-eq"}'
ruse-mcp call package_plugin '{"name":"bus-compressor","install":true}'
```

[tools.md](tools.md) lists every tool.

## Updating RUSE

```
cd ruse
git pull
cargo install --path crates/mcp
```

[CHANGELOG.md](../CHANGELOG.md) marks any change that needs work on your side.
