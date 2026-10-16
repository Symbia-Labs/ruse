# MUSE documentation

| Page | Covers |
|---|---|
| [Getting started](getting-started.md) | Installing MUSE, making a workspace, connecting an agent and building a first plugin |
| [Tools](tools.md) | The MCP tools, the `muse-mcp` commands, and the files a plugin is made of |
| [Graph format](graph-format.md) | Describing a plugin as a graph: nodes, arguments, expressions, modulators and meters |
| [Blocks](blocks.md) | Every block with its arguments, ranges and defaults |
| [Gates](gates.md) | The seven checks every plugin must pass, and what they do not cover |
| [Packaging](packaging.md) | Building CLAP and Audio Unit bundles, installing them, and sharing a plugin |
| [Examples](examples.md) | The plugins MUSE ships, and things to ask an agent about each |
| [Architecture](architecture.md) | The crates, the plugin contract, the graph runtime and the format wrappers |

[Graph format](graph-format.md) and [Blocks](blocks.md) are generated from the code by `muse-mcp docs`, and are the same text an agent reads through `list_blocks`.
