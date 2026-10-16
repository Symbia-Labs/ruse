# Contributing to MUSE

MUSE is in public beta. Bug reports, block requests, examples, new blocks, fixes and documentation are all welcome.

## Reporting a problem

Use the [bug report form](https://github.com/Symbia-Labs/muse/issues/new?template=bug_report.yml). It asks for the plugin's `manifest.json`, what `check`, `render` or `package_plugin` printed, the host and its version, and your MUSE commit. A failing gate's line of output usually says enough to find the cause, so include it whole.

## Asking for a block

If the blocks cannot build a plugin you want, use the [block request form](https://github.com/Symbia-Labs/muse/issues/new?template=block_request.yml). Describe the plugin, and what the blocks cannot do yet. [docs/blocks.md](docs/blocks.md) lists every block that exists today.

## Before you write code

For anything larger than a small fix, open an issue first, so we can agree on the approach before you write the code.

## Ground rules

- **New code only.** Do not copy code from JUCE, another framework or any other project, and do not port it line by line. Algorithms from papers and textbooks are fine. Name the source in a comment.
- **No new dependencies without a discussion.** MUSE has no dependencies outside this repository, on purpose. Open an issue first.
- **A quiet audio thread.** `process` must not allocate, lock, block, read files or print.
- **Plugins stay safe Rust.** Unsafe code belongs only in the format wrappers, `muse-clap` and `muse-au`, where MUSE talks to the host. New unsafe code needs a comment that says why it is sound.
- **Agents are welcome.** Code an agent wrote meets the same standard as code a person wrote, and the person who opens the pull request is responsible for that code.

## Checks

```
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

CI runs both on macOS for every pull request, then packages every example, installs it on the runner and runs `auval` on each Audio Unit.

## Adding a block

1. Write the block in `crates/blocks/src/blocks/`, implementing the `Block` trait. Declare each argument in its `ArgInfo` table with a unit, a range, a default and a one-sentence description. That table becomes the block's entry in [docs/blocks.md](docs/blocks.md), and it is what an agent reads through `list_blocks`.
2. Add the block to `catalogue()` in `crates/blocks/src/blocks/mod.rs`.
3. Add tests in `crates/blocks/tests/` that measure what the block does against what it should do, such as a filter's gain at set frequencies or a delay's echo times. Run it in a graph at more than one block size to confirm the output does not depend on the host's block size.
4. Run `muse-mcp docs` to rewrite the reference pages. A test fails when they are out of date.

[docs/architecture.md](docs/architecture.md) explains the `Block` trait, the 32-sample control grid and the rest of the runtime.

## Pull requests

- Keep each pull request to one change.
- Say in plain words what changed and why.
- Add a line to [CHANGELOG.md](CHANGELOG.md) under the unreleased version. Mark anything that needs work from people building plugins as **Breaking**, and say what to do.

## License

By contributing, you agree that your work is licensed under the [MIT License](LICENSE), the same as the rest of MUSE.
