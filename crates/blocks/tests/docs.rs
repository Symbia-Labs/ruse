//! The block reference in docs/ is generated from the catalogue. This fails when the two drift apart.

fn checked_in(name: &str) -> String {
    let path = format!("{}/../../docs/{}", env!("CARGO_MANIFEST_DIR"), name);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {}. Run muse-mcp docs", path, e))
}

#[test]
fn the_reference_pages_match_the_blocks() {
    assert!(
        checked_in("blocks.md") == muse_blocks::blocks_markdown(),
        "docs/blocks.md is out of date. Run muse-mcp docs"
    );
    assert!(
        checked_in("graph-format.md") == muse_blocks::format_markdown(),
        "docs/graph-format.md is out of date. Run muse-mcp docs"
    );
}

#[test]
fn every_block_and_argument_is_in_the_reference() {
    let page = muse_blocks::blocks_markdown();
    for b in muse_blocks::blocks::catalogue() {
        assert!(page.contains(&format!("## `{}`", b.name)), "{} is missing", b.name);
        for a in b.args {
            assert!(page.contains(&format!("| `{}` |", a.name)), "{}.{} is missing", b.name, a.name);
        }
    }
}
