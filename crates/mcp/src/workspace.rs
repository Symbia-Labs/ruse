//! `muse-mcp new DIR`: a workspace of the person's own, separate from the MUSE checkout.
//!
//! The workspace depends on the MUSE crates through `[workspace.dependencies]`, so updating MUSE never
//! touches the person's plugins, and their plugins never conflict with a pull. It starts with copies of the
//! examples MUSE ships, to try, change, or use as a starting point.

use crate::gen;
use std::path::{Path, PathBuf};

/// What `new` leaves out when it copies an example: build output, renders and the server's dsp.rs backup.
fn skipped(name: &str) -> bool {
    matches!(name, "dist" | "renders" | "target" | "dsp.rs.prev" | ".DS_Store")
}

fn copy_example(src: &Path, dst: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let name = entry.file_name();
        if skipped(&name.to_string_lossy()) {
            continue;
        }
        let to = dst.join(&name);
        if entry.file_type()?.is_dir() {
            copy_example(&entry.path(), &to)?;
        } else {
            std::fs::copy(entry.path(), &to)?;
        }
    }
    Ok(())
}

/// The examples a new workspace starts with: those MUSE builds from a manifest. Packaging crates are left
/// out, because package_plugin writes them again, and so are hand-built ones such as the 808, whose bundle
/// script expects the MUSE checkout around it.
fn examples(muse: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(muse.join("examples")) else {
        return Vec::new();
    };
    let mut dirs: Vec<PathBuf> = entries
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|d| d.join("manifest.json").is_file())
        .collect();
    dirs.sort();
    dirs
}

const GITIGNORE: &str = "target\nplugins/*/renders\nplugins/*/dist\nplugins/*/src/dsp.rs.prev\n";

/// Creates the workspace and returns what to tell the person.
pub fn create(dir: &Path, muse: &Path, exe: &Path) -> Result<String, String> {
    if !muse.join("crates/core/Cargo.toml").is_file() {
        return Err(format!("{} is not a MUSE checkout: crates/core/Cargo.toml is missing", muse.display()));
    }
    if dir.join("Cargo.toml").exists() {
        return Err(format!("{} already has a Cargo.toml. Pick an empty or new folder", dir.display()));
    }
    let plugins = dir.join("plugins");
    std::fs::create_dir_all(&plugins).map_err(|e| format!("{}: {}", plugins.display(), e))?;
    let write = |path: PathBuf, text: &str| std::fs::write(&path, text).map_err(|e| format!("{}: {}", path.display(), e));
    write(dir.join("Cargo.toml"), &gen::workspace_toml(muse))?;
    write(dir.join(".gitignore"), GITIGNORE)?;

    let mut copied = Vec::new();
    for src in examples(muse) {
        let name = src.file_name().unwrap_or_default().to_string_lossy().to_string();
        copy_example(&src, &plugins.join(&name)).map_err(|e| format!("copying the example {}: {}", name, e))?;
        copied.push(name);
    }

    let dir = std::fs::canonicalize(dir).unwrap_or_else(|_| dir.to_path_buf());
    Ok(format!(
        "Created a MUSE workspace in {dir}, building against the MUSE checkout in {muse}.\n\
         {examples}\n\n\
         To connect an agent, add an MCP server to its settings with\n  \
         command: {exe}\n  \
         environment: MUSE_ROOT={dir}\n\
         Started from inside the workspace folder, muse-mcp finds it without MUSE_ROOT.\n\n\
         Setup for Claude Code, Claude Desktop, Codex, Gemini CLI, OpenCode, Cursor and VS Code is in\n  \
         {muse}/docs/getting-started.md",
        dir = dir.display(),
        muse = muse.display(),
        exe = exe.display(),
        examples = if copied.is_empty() {
            "plugins/ is empty.".to_string()
        } else {
            format!("plugins/ starts with copies of the examples: {}.", copied.join(", "))
        }
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn muse() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
    }

    #[test]
    fn a_new_workspace_points_at_muse_and_copies_the_examples_without_build_output() {
        let dir = std::env::temp_dir().join(format!("muse-new-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let said = create(&dir, &muse(), Path::new("/usr/local/bin/muse-mcp")).unwrap();
        assert!(said.contains("MUSE_ROOT="), "{}", said);
        let toml = std::fs::read_to_string(dir.join("Cargo.toml")).unwrap();
        assert!(toml.contains("members = [\"plugins/*\"]"));
        assert!(toml.contains("crates/core"));
        for name in ["warm-saturator", "bus-compressor", "harmonic-eq"] {
            let p = dir.join("plugins").join(name);
            assert!(p.join("manifest.json").is_file(), "{} was not copied", name);
            assert!(!p.join("dist").exists() && !p.join("renders").exists(), "{} copied build output", name);
        }
        assert!(!dir.join("plugins/kit-808").exists());
        assert!(!dir.join("plugins/warm-saturator-au").exists());
        assert!(create(&dir, &muse(), Path::new("muse-mcp")).is_err(), "a second new in the same folder must refuse");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_folder_that_is_not_muse_is_refused() {
        let dir = std::env::temp_dir().join(format!("muse-new-bad-{}", std::process::id()));
        assert!(create(&dir, Path::new("/nonexistent"), Path::new("muse-mcp")).is_err());
        assert!(!dir.exists());
    }
}
