//! muse-mcp: an MCP server over stdio for designing, building and checking MUSE plugins.
//!
//! Connect it to any MCP client. The client's agent then creates plugins, writes their signal processing,
//! runs the gates and renders audio through the tools in `tools.rs`.

mod gen;
mod json;
mod manifest;
mod tools;

use json::Json;
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};

const INSTRUCTIONS: &str = "MUSE builds audio plugins in Rust. The usual order is create_plugin with the parameters the plugin needs, \
write_dsp with the signal processing, check to run the gates, and render to measure what the plugin does to a signal. \
Repeat write_dsp, check and render until the numbers match what the person asked for. \
Every parameter needs a description. The signal processing in process must not allocate, lock or block. \
Parameters are read through accessors such as p.drive(), which create_plugin and update_plugin list. \
Render reports levels and octave-band energy before and after, so a filter, a gain change or a distortion shows up in numbers.";

/// The MUSE workspace: MUSE_ROOT if set, else the folder above this program's target directory, else the working directory.
fn find_root() -> PathBuf {
    if let Ok(r) = std::env::var("MUSE_ROOT") {
        if !r.is_empty() {
            return PathBuf::from(r);
        }
    }
    if let Ok(exe) = std::env::current_exe() {
        for dir in exe.ancestors() {
            if dir.join("crates/core/Cargo.toml").is_file() && dir.join("plugins").is_dir() {
                return dir.to_path_buf();
            }
        }
    }
    std::env::current_dir().unwrap_or_else(|_| Path::new(".").to_path_buf())
}

fn reply(id: &Json, result: Json) -> Json {
    Json::obj(vec![("jsonrpc", Json::str("2.0")), ("id", id.clone()), ("result", result)])
}

fn failure(id: &Json, code: i64, message: &str) -> Json {
    Json::obj(vec![
        ("jsonrpc", Json::str("2.0")),
        ("id", id.clone()),
        (
            "error",
            Json::obj(vec![("code", Json::Num(code as f64)), ("message", Json::str(message))]),
        ),
    ])
}

fn tool_result(text: String, is_error: bool) -> Json {
    Json::obj(vec![
        (
            "content",
            Json::Arr(vec![Json::obj(vec![("type", Json::str("text")), ("text", Json::Str(text))])]),
        ),
        ("isError", Json::Bool(is_error)),
    ])
}

/// Handles one message. Notifications, which carry no id, get no reply.
fn handle(ctx: &tools::Ctx, message: &Json) -> Option<Json> {
    let id = message.get("id")?;
    let method = message.get("method").and_then(|m| m.as_str()).unwrap_or("");
    let params = message.get("params").cloned().unwrap_or(Json::Null);
    Some(match method {
        "initialize" => {
            let version = params
                .get("protocolVersion")
                .and_then(|v| v.as_str())
                .unwrap_or("2025-06-18")
                .to_string();
            reply(
                id,
                Json::obj(vec![
                    ("protocolVersion", Json::Str(version)),
                    ("capabilities", Json::obj(vec![("tools", Json::obj(vec![]))])),
                    (
                        "serverInfo",
                        Json::obj(vec![("name", Json::str("muse")), ("version", Json::str(env!("CARGO_PKG_VERSION")))]),
                    ),
                    ("instructions", Json::str(INSTRUCTIONS)),
                ]),
            )
        }
        "ping" => reply(id, Json::obj(vec![])),
        "tools/list" => reply(id, Json::obj(vec![("tools", tools::definitions())])),
        "tools/call" => {
            let name = params.get("name").and_then(|n| n.as_str()).unwrap_or("");
            let args = params.get("arguments").cloned().unwrap_or_else(|| Json::obj(vec![]));
            match tools::call(ctx, name, &args) {
                Ok(text) => reply(id, tool_result(text, false)),
                Err(text) => reply(id, tool_result(text, true)),
            }
        }
        other => failure(id, -32601, &format!("method not found: {}", other)),
    })
}

/// `muse-mcp call TOOL ARGS` runs one tool and prints its result, for trying the tools without a client.
/// ARGS is JSON, or @FILE to read the JSON from a file.
fn call_once(ctx: &tools::Ctx, args: &[String]) -> i32 {
    let Some(tool) = args.first() else {
        eprintln!("usage: muse-mcp call TOOL [JSON | @FILE]");
        return 2;
    };
    let raw = match args.get(1) {
        None => "{}".to_string(),
        Some(a) => match a.strip_prefix('@') {
            Some(path) => match std::fs::read_to_string(path) {
                Ok(t) => t,
                Err(e) => {
                    eprintln!("{}: {}", path, e);
                    return 2;
                }
            },
            None => a.clone(),
        },
    };
    let parsed = match json::parse(&raw) {
        Ok(j) => j,
        Err(e) => {
            eprintln!("the arguments are not valid JSON: {}", e);
            return 2;
        }
    };
    match tools::call(ctx, tool, &parsed) {
        Ok(text) => {
            println!("{}", text);
            0
        }
        Err(text) => {
            println!("{}", text);
            1
        }
    }
}

fn main() {
    let ctx = tools::Ctx { root: find_root() };
    let argv: Vec<String> = std::env::args().skip(1).collect();
    if argv.first().map(|s| s.as_str()) == Some("call") {
        std::process::exit(call_once(&ctx, &argv[1..]));
    }
    eprintln!("muse-mcp: workspace {}", ctx.root.display());
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout();
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        if line.trim().is_empty() {
            continue;
        }
        let out = match json::parse(&line) {
            Ok(message) => handle(&ctx, &message),
            Err(e) => Some(failure(&Json::Null, -32700, &format!("parse error: {}", e))),
        };
        if let Some(response) = out {
            let _ = writeln!(stdout, "{}", response.compact());
            let _ = stdout.flush();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ask(text: &str) -> Json {
        let ctx = tools::Ctx {
            root: std::env::temp_dir().join("muse-mcp-protocol-test"),
        };
        handle(&ctx, &json::parse(text).unwrap()).unwrap()
    }

    #[test]
    fn initialize_echoes_the_clients_protocol_version() {
        let r = ask(r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05"}}"#);
        let result = r.get("result").unwrap();
        assert_eq!(result.get("protocolVersion").unwrap().as_str(), Some("2024-11-05"));
        assert!(result.get("capabilities").unwrap().get("tools").is_some());
        assert_eq!(result.get("serverInfo").unwrap().get("name").unwrap().as_str(), Some("muse"));
        assert_eq!(r.get("id").unwrap().as_f64(), Some(1.0));
    }

    #[test]
    fn the_tool_list_has_nine_tools() {
        let r = ask(r#"{"jsonrpc":"2.0","id":"a","method":"tools/list"}"#);
        assert_eq!(r.get("result").unwrap().get("tools").unwrap().as_array().unwrap().len(), 9);
        assert_eq!(r.get("id").unwrap().as_str(), Some("a"));
    }

    #[test]
    fn a_failing_tool_is_marked_as_an_error() {
        let r = ask(r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"build","arguments":{"name":"nothing"}}}"#);
        assert_eq!(r.get("result").unwrap().get("isError").unwrap().as_bool(), Some(true));
    }

    #[test]
    fn notifications_get_no_reply_and_unknown_methods_are_refused() {
        let ctx = tools::Ctx {
            root: std::env::temp_dir(),
        };
        let note = json::parse(r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#).unwrap();
        assert!(handle(&ctx, &note).is_none());
        let r = ask(r#"{"jsonrpc":"2.0","id":3,"method":"resources/list"}"#);
        assert_eq!(r.get("error").unwrap().get("code").unwrap().as_f64(), Some(-32601.0));
    }
}
