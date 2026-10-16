# Tools

`ruse-mcp` is an MCP server that runs on your Mac. An agent connected to it designs and builds plugins through these tools.

| Tool | What it does |
|---|---|
| `create_plugin` | Creates a plugin from a name, a description, a list of parameters and, optionally, a graph |
| `update_plugin` | Changes the parameters, channel range, description or package settings, and regenerates the code |
| `list_blocks` | Describes the graph format, the expressions and every block, with ranges and defaults |
| `set_graph` | Sets the signal path as a graph of blocks, checks it against the parameters and builds. No code to write |
| `write_dsp` | Replaces the signal processing in `src/dsp.rs` and builds it, for what the blocks cannot do |
| `read_dsp` | Returns the current `dsp.rs` and the accessor each parameter has |
| `build` | Builds the plugin and reports compiler output |
| `check` | Runs the [gates](gates.md) every plugin must pass |
| `render` | Plays a test signal or a WAV file through the plugin and reports levels and octave bands before and after |
| `describe` | Returns the plugin's parameter description as JSON |
| `package_plugin` | Builds CLAP and Audio Unit bundles, checks them, and installs them when asked. See [packaging.md](packaging.md) |
| `list_plugins` | Lists the plugins in the workspace and its examples |

The agent never writes a parameter declaration or glue code. RUSE generates both from the plugin's `manifest.json`. Most plugins need no code at all, because their signal path is a [graph](graph-format.md) of [blocks](blocks.md). For the rest, the agent writes `dsp.rs`, where each parameter arrives through an accessor such as `p.drive()`.

## Commands

| Command | What it does |
|---|---|
| `ruse-mcp` | Runs the MCP server over stdio. This is the command an MCP client starts |
| `ruse-mcp new DIR` | Creates a workspace for your own plugins. See [getting-started.md](getting-started.md) |
| `ruse-mcp call TOOL [JSON \| @FILE]` | Runs one tool and prints its result |
| `ruse-mcp docs` | Writes [blocks.md](blocks.md) and [graph-format.md](graph-format.md) from the block catalogue |

## Which workspace

`ruse-mcp` works in the first of these that applies:

1. The folder named by `RUSE_ROOT`.
2. The working directory, when it holds a `Cargo.toml` and a `plugins` or `examples` folder.
3. The RUSE checkout used to build the program.

New plugins go in the workspace's `plugins/`. The RUSE checkout's own examples are in `examples/`, and the tools find a plugin in either.

## What a plugin is made of

| File | Written by |
|---|---|
| `manifest.json` | The tools. Name, description, channel range, parameters, package settings and graph |
| `src/declaration.rs` | RUSE, from the manifest. Never edit it |
| `src/dsp.rs` | RUSE from the graph, or the agent through `write_dsp` |
| `src/lib.rs`, `src/bin/render.rs`, `tests/gates.rs` | RUSE |
| `../NAME-clap`, `../NAME-au` | `package_plugin`. The crates that wrap the plugin for each format |
