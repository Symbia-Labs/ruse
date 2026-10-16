//! `ruse-mcp new DIR`: a workspace of the person's own, separate from the RUSE checkout.
//!
//! The workspace depends on the RUSE crates through `[workspace.dependencies]`, so updating RUSE never
//! touches the person's plugins, and their plugins never conflict with a pull. It starts with copies of the
//! examples RUSE ships, to try, change, or use as a starting point.

use crate::{gen, setup};
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

/// The examples a new workspace starts with: those RUSE builds from a manifest. Packaging crates are left
/// out, because package_plugin writes them again, and so are hand-built ones such as the 808, whose bundle
/// script expects the RUSE checkout around it.
fn examples(ruse: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(ruse.join("examples")) else {
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
pub fn create(dir: &Path, ruse: &Path, exe: &Path) -> Result<String, String> {
    if !ruse.join("crates/core/Cargo.toml").is_file() {
        return Err(format!("{} is not a RUSE checkout: crates/core/Cargo.toml is missing", ruse.display()));
    }
    if dir.join("Cargo.toml").exists() {
        return Err(format!("{} already has a Cargo.toml. Pick an empty or new folder", dir.display()));
    }
    let plugins = dir.join("plugins");
    std::fs::create_dir_all(&plugins).map_err(|e| format!("{}: {}", plugins.display(), e))?;
    let write = |path: PathBuf, text: &str| std::fs::write(&path, text).map_err(|e| format!("{}: {}", path.display(), e));
    write(dir.join("Cargo.toml"), &gen::workspace_toml(ruse))?;
    write(dir.join(".gitignore"), GITIGNORE)?;

    let mut copied = Vec::new();
    for src in examples(ruse) {
        let name = src.file_name().unwrap_or_default().to_string_lossy().to_string();
        copy_example(&src, &plugins.join(&name)).map_err(|e| format!("copying the example {}: {}", name, e))?;
        copied.push(name);
    }

    let dir = std::fs::canonicalize(dir).unwrap_or_else(|_| dir.to_path_buf());
    Ok(format!(
        "Created a RUSE workspace in {dir}, building against the RUSE checkout in {ruse}.\n\
         {examples}\n\n\
         To connect an agent, run\n  \
         ruse-mcp setup AGENT {quoted}\n\
         with AGENT one of {agents}. It prints that agent's setup with this workspace's paths filled in.\n\n\
         For any other MCP client, the command is {exe} and RUSE_ROOT is {dir}.",
        dir = dir.display(),
        ruse = ruse.display(),
        exe = exe.display(),
        quoted = setup::shell(&dir.display().to_string()),
        agents = setup::names(),
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

    fn ruse() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
    }

    #[test]
    fn a_new_workspace_points_at_ruse_and_copies_the_examples_without_build_output() {
        let dir = std::env::temp_dir().join(format!("ruse-new-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let said = create(&dir, &ruse(), Path::new("/usr/local/bin/ruse-mcp")).unwrap();
        assert!(said.contains("ruse-mcp setup AGENT"), "{}", said);
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
        assert!(create(&dir, &ruse(), Path::new("ruse-mcp")).is_err(), "a second new in the same folder must refuse");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_folder_that_is_not_ruse_is_refused() {
        let dir = std::env::temp_dir().join(format!("ruse-new-bad-{}", std::process::id()));
        assert!(create(&dir, Path::new("/nonexistent"), Path::new("ruse-mcp")).is_err());
        assert!(!dir.exists());
    }
}
