//! `ruse-mcp setup AGENT [WORKSPACE]`: the settings that connect one agent to a workspace, with the real
//! paths filled in, so nobody has to swap the placeholders in an example by hand.

use crate::json::Json;
use std::path::Path;

/// The agents `setup` knows: the name it takes, and the name people know the agent by.
pub const AGENTS: [(&str, &str); 7] = [
    ("claude-code", "Claude Code"),
    ("claude-desktop", "Claude Desktop"),
    ("codex", "Codex, and ChatGPT through Codex"),
    ("gemini", "Gemini CLI"),
    ("opencode", "OpenCode"),
    ("cursor", "Cursor"),
    ("vscode", "VS Code with GitHub Copilot"),
];

/// The agent names `setup` takes, for usage messages.
pub fn names() -> String {
    let names: Vec<&str> = AGENTS.iter().map(|(name, _)| *name).collect();
    names.join(", ")
}

/// A word the shell reads back unchanged: as it is when it holds nothing the shell treats specially,
/// otherwise in single quotes.
pub fn shell(word: &str) -> String {
    let plain = |c: char| c.is_ascii_alphanumeric() || "/._-+@%:,=".contains(c);
    if !word.is_empty() && word.chars().all(plain) {
        word.to_string()
    } else {
        format!("'{}'", word.replace('\'', "'\\''"))
    }
}

/// A TOML basic string. TOML escapes quotes, backslashes and control characters the way JSON does.
fn toml_string(s: &str) -> String {
    Json::str(s).compact()
}

/// A config file holding one server, `ruse`, under the key the agent keeps its servers in.
fn servers(key: &str, entry: Vec<(&str, Json)>) -> String {
    Json::obj(vec![(key, Json::obj(vec![("ruse", Json::obj(entry))]))]).pretty()
}

/// What to tell the person to connect `agent` to the workspace in `root`, or None for an agent `setup`
/// does not know.
pub fn instructions(agent: &str, exe: &Path, root: &Path) -> Option<String> {
    let exe = exe.display().to_string();
    let root = root.display().to_string();
    let env = || Json::obj(vec![("RUSE_ROOT", Json::str(root.as_str()))]);
    let text = match agent {
        "claude-code" => format!(
            "Run this in Terminal:\n\n  \
             cd {dir}\n  \
             claude mcp add --env RUSE_ROOT={dir} --transport stdio ruse -- {exe}\n\n\
             This adds RUSE for this folder only. Add --scope user to have it in every project.",
            dir = shell(&root),
            exe = shell(&exe),
        ),
        "claude-desktop" => format!(
            "Claude Desktop reads its servers from\n  \
             ~/Library/Application Support/Claude/claude_desktop_config.json\n\
             Settings, then Developer, then Edit Config finds the file for you. Put this in it. If the file \
             already lists other servers under \"mcpServers\", add the \"ruse\" entry beside them.\n\n\
             {json}\n\
             Then quit Claude Desktop and open it again. If RUSE does not appear, its log is\n  \
             ~/Library/Logs/Claude/mcp-server-ruse.log",
            json = servers("mcpServers", vec![("command", Json::str(exe.as_str())), ("env", env())]),
        ),
        "codex" => format!(
            "Add this to ~/.codex/config.toml, which the Codex CLI, the Codex IDE extension and the ChatGPT \
             desktop app all read:\n\n\
             [mcp_servers.ruse]\n\
             command = {exe}\n\
             tool_timeout_sec = 600\n\n\
             [mcp_servers.ruse.env]\n\
             RUSE_ROOT = {root}\n\n\
             Codex stops a tool call after 60 seconds unless tool_timeout_sec says otherwise, and a plugin's \
             first build or a package run can take longer than that.",
            exe = toml_string(&exe),
            root = toml_string(&root),
        ),
        "gemini" => format!(
            "Run this in Terminal:\n\n  \
             cd {dir}\n  \
             gemini mcp add -e RUSE_ROOT={dir} ruse {exe}\n\n\
             This writes .gemini/settings.json in the workspace. Add -s user to write ~/.gemini/settings.json \
             instead, so RUSE is there in every folder.",
            dir = shell(&root),
            exe = shell(&exe),
        ),
        "opencode" => format!(
            "Save this as opencode.json in {root}, or add the \"ruse\" entry under \"mcp\" in \
             ~/.config/opencode/opencode.json to have it everywhere:\n\n{json}",
            json = Json::obj(vec![
                ("$schema", Json::str("https://opencode.ai/config.json")),
                (
                    "mcp",
                    Json::obj(vec![(
                        "ruse",
                        Json::obj(vec![
                            ("type", Json::str("local")),
                            ("command", Json::Arr(vec![Json::str(exe.as_str())])),
                            ("environment", env()),
                            ("enabled", Json::Bool(true)),
                        ]),
                    )]),
                ),
            ])
            .pretty(),
        ),
        "cursor" => format!(
            "Save this as .cursor/mcp.json in {root}, or add the \"ruse\" entry to ~/.cursor/mcp.json to have \
             it in every project:\n\n{json}",
            json = servers(
                "mcpServers",
                vec![("type", Json::str("stdio")), ("command", Json::str(exe.as_str())), ("env", env())],
            ),
        ),
        "vscode" => format!(
            "Save this as .vscode/mcp.json in {root}, then open that folder in VS Code. RUSE's tools show up \
             in Copilot Chat.\n\n{json}",
            json = servers(
                "servers",
                vec![
                    ("type", Json::str("stdio")),
                    ("command", Json::str(exe.as_str())),
                    ("cwd", Json::str("${workspaceFolder}")),
                ],
            ),
        ),
        _ => return None,
    };
    Some(text.trim_end().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    const EXE: &str = "/Users/sam/.cargo/bin/ruse-mcp";
    const ROOT: &str = "/Users/sam/my plugins";

    fn setup(agent: &str) -> String {
        instructions(agent, Path::new(EXE), Path::new(ROOT)).unwrap()
    }

    /// The JSON in a setup: from the first line that opens an object to the last line that closes one.
    fn json_in(text: &str) -> Option<Json> {
        let lines: Vec<&str> = text.lines().collect();
        let start = lines.iter().position(|l| *l == "{")?;
        let end = lines.iter().rposition(|l| *l == "}")?;
        crate::json::parse(&lines[start..=end].join("\n")).ok()
    }

    #[test]
    fn every_agent_gets_the_real_paths() {
        for (agent, _) in AGENTS {
            let text = setup(agent);
            assert!(text.contains(EXE), "{}: {}", agent, text);
            assert!(!text.contains("/Users/you"), "{}: {}", agent, text);
        }
        assert!(instructions("chatgpt", Path::new(EXE), Path::new(ROOT)).is_none());
    }

    #[test]
    fn the_config_files_are_valid_json_with_the_paths_in_place() {
        for agent in ["claude-desktop", "opencode", "cursor", "vscode"] {
            let json = json_in(&setup(agent)).unwrap_or_else(|| panic!("{} has no valid JSON", agent));
            let compact = json.compact();
            assert!(compact.contains(EXE), "{}", compact);
            if agent != "vscode" {
                assert!(compact.contains(ROOT), "{}", compact);
            }
        }
    }

    #[test]
    fn the_codex_file_quotes_both_paths() {
        let text = setup("codex");
        assert!(text.contains(&format!("command = \"{}\"", EXE)), "{}", text);
        assert!(text.contains(&format!("RUSE_ROOT = \"{}\"", ROOT)), "{}", text);
    }

    #[test]
    fn shell_commands_quote_a_path_with_a_space() {
        for agent in ["claude-code", "gemini"] {
            let text = setup(agent);
            assert!(text.contains("cd '/Users/sam/my plugins'"), "{}", text);
            assert!(text.contains("RUSE_ROOT='/Users/sam/my plugins'"), "{}", text);
        }
        assert_eq!(shell("/plain/path"), "/plain/path");
        assert_eq!(shell("it's"), "'it'\\''s'");
    }
}
