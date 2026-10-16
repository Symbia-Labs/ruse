# Security

## Reporting a vulnerability

Report it privately through GitHub: open the repository's Security tab and choose **Report a vulnerability**. Please do not open a public issue for a security problem.

Include what you found, how to reproduce it, and the RUSE commit or release you tested. We will reply as soon as we can.

## What counts

- A plugin built with RUSE that can be made to read or write memory it should not, crash the host, or run code, through its saved state, its parameters or its audio.
- The CLAP or Audio Unit wrapper doing any of those things for any plugin.
- `ruse-mcp`, `ruse-mcp new` or `package_plugin` writing outside the workspace or the plug-in folders, or running anything other than `cargo`, `codesign`, `auval`, RUSE's own verifiers and the install script.

## By design

`ruse-mcp` compiles and runs the plugin code an agent writes, with your user account's permissions. That code cannot use unsafe Rust or outside crates, but it can still read and write files. [docs/getting-started.md](docs/getting-started.md#what-ruse-runs-on-your-mac) explains this, and it is not a vulnerability in itself.
