//! The tools an agent uses to design, build and check a plugin.

use crate::gen;
use crate::json::{self, Json};
use crate::manifest::{self, Manifest};
use std::path::{Path, PathBuf};
use std::process::Command;

pub struct Ctx {
    pub root: PathBuf,
}

const PARAM_SCHEMA: &str = r#"{"type":"object","properties":{
"id":{"type":"string","description":"Lowercase letters, digits and underscores, starting with a letter. The signal processing reads it as p.<id>()."},
"name":{"type":"string","description":"The label a person sees."},
"description":{"type":"string","description":"One sentence on what the parameter does. Required."},
"kind":{"type":"string","enum":["knob","choice","toggle","meter"],"description":"Defaults to knob. A choice holds one of a list of names, a toggle is on or off, and a meter is written by the plugin."},
"unit":{"type":"string","description":"For example dB, Hz, ms or %."},
"min":{"type":"number","description":"Lowest value of a knob or meter."},
"max":{"type":"number","description":"Highest value of a knob or meter."},
"default":{"description":"A number for a knob. A choice takes its index or its name. A toggle takes true or false."},
"curve":{"type":"string","enum":["linear","log"],"description":"Use log for frequencies. It needs min above zero."},
"scope":{"type":"string","enum":["global","per_channel"],"description":"per_channel gives every channel its own value."},
"group":{"type":"string","description":"A heading that collects related parameters."},
"choices":{"type":"array","items":{"type":"string"},"description":"The names of a choice, two or more."}
},"required":["id","name","description"]}"#;

const TOOLS: &str = r#"[
{"name":"list_plugins","description":"List the plugins in this MUSE workspace with their descriptions.","inputSchema":{"type":"object","properties":{}}},
{"name":"create_plugin","description":"Create a new plugin from a description of its parameters. MUSE generates the whole crate. The only file left to write is the signal processing, src/dsp.rs, through write_dsp. The result lists the accessor each parameter gets, such as p.drive(). The new plugin starts out passing audio through unchanged.","inputSchema":{"type":"object","properties":{
"name":{"type":"string","description":"Lowercase letters, digits and hyphens, such as warm-saturator."},
"title":{"type":"string","description":"The name a person sees. Defaults to the name in title case."},
"description":{"type":"string","description":"One or two sentences on what the plugin does."},
"channels_min":{"type":"integer","description":"Fewest channels accepted. Defaults to 1."},
"channels_max":{"type":"integer","description":"Most channels accepted, up to 64. Defaults to 64."},
"parameters":{"type":"array","items":@PARAM@,"description":"Every parameter the plugin has. May be empty."}
},"required":["name","description","parameters"]}},
{"name":"update_plugin","description":"Change a plugin's title, description, channel range or parameters. Give the whole parameter list again, because it replaces the old one. MUSE regenerates the declaration and builds. The signal processing in dsp.rs is kept, so rewrite it afterwards if an accessor it uses has changed.","inputSchema":{"type":"object","properties":{
"name":{"type":"string"},
"title":{"type":"string"},
"description":{"type":"string"},
"channels_min":{"type":"integer"},
"channels_max":{"type":"integer"},
"parameters":{"type":"array","items":@PARAM@}
},"required":["name"]}},
{"name":"read_dsp","description":"Return the current signal processing source, src/dsp.rs, and the accessor each parameter has.","inputSchema":{"type":"object","properties":{"name":{"type":"string"}},"required":["name"]}},
{"name":"write_dsp","description":"Replace the signal processing, src/dsp.rs, with new source and build it. The file defines `pub struct State` with new(), prepare(&mut self, &Setup), latency_samples(), tail_samples() and process(&mut self, &Params, &mut [&mut [f32]], &ProcessContext). process runs on the audio thread, once per block, with one slice per channel, in place. It must not allocate, lock or block, so keep every buffer in State and size it in prepare. Per-channel memory such as filter state lives in State and is sized from setup.channels. Unsafe code is refused. The previous file is kept as dsp.rs.prev. The result is the build output.","inputSchema":{"type":"object","properties":{"name":{"type":"string"},"source":{"type":"string","description":"The complete contents of dsp.rs."}},"required":["name","source"]}},
{"name":"build","description":"Build the plugin and report compiler errors and warnings.","inputSchema":{"type":"object","properties":{"name":{"type":"string"}},"required":["name"]}},
{"name":"check","description":"Run the gates every plugin must pass. They cover the channel layout, parameter descriptions, finite and bounded output with every parameter at its minimum and maximum, no allocation in process, identical output from two fresh instances, and output that does not depend on the host's block size. Each failing gate comes with a hint on how to fix it.","inputSchema":{"type":"object","properties":{"name":{"type":"string"}},"required":["name"]}},
{"name":"render","description":"Run the plugin offline on a test signal or a WAV file and measure what it did. The result lists the parameter values, latency, input and output levels per channel, and octave-band levels before and after, so a filter, a gain change or a distortion shows up in numbers. The output is also written as a 32-bit float WAV file under the plugin's renders folder.","inputSchema":{"type":"object","properties":{
"name":{"type":"string"},
"signal":{"type":"string","description":"silence, sine:HZ:DB, noise:DB, impulse:DB, sweep:DB or nyquist:DB. Defaults to noise:-18."},
"input_wav":{"type":"string","description":"Path to a WAV file to use instead of a generated signal."},
"channels":{"type":"integer","description":"Channels for a generated signal. Defaults to 2."},
"seconds":{"type":"number","description":"Length of a generated signal. Defaults to 2."},
"sample_rate":{"type":"number","description":"Defaults to 48000."},
"block":{"type":"integer","description":"Host block size. Defaults to 128."},
"settings":{"type":"object","description":"Parameter values to apply, such as {\"drive\": 12}. A key like \"trim@2\" writes channel 2 of a per-channel parameter, counting from 0."},
"tail":{"type":"boolean","description":"Render the plugin's tail after the input ends. Defaults to true."},
"output_name":{"type":"string","description":"File name for the rendered WAV, without the extension. Defaults to latest."}
},"required":["name"]}},
{"name":"describe","description":"Return the parameter description MCP clients read, as JSON, with the values at the given settings.","inputSchema":{"type":"object","properties":{
"name":{"type":"string"},
"channels":{"type":"integer","description":"Channel count to describe. Defaults to 2."},
"settings":{"type":"object","description":"Parameter values to apply first."}
},"required":["name"]}}
]"#;

pub fn definitions() -> Json {
    json::parse(&TOOLS.replace("@PARAM@", PARAM_SCHEMA)).expect("the tool definitions are valid JSON")
}

fn text_arg<'a>(args: &'a Json, key: &str) -> Option<&'a str> {
    args.get(key).and_then(|v| v.as_str())
}

fn name_arg(args: &Json) -> Result<String, String> {
    text_arg(args, "name")
        .map(|s| s.to_string())
        .ok_or_else(|| "name is required".to_string())
}

fn plugin_dir(ctx: &Ctx, name: &str) -> Result<PathBuf, String> {
    if !manifest::valid_slug(name) {
        return Err(format!("'{}' is not a valid plugin name", name));
    }
    let dir = ctx.root.join("plugins").join(name);
    if !dir.join("manifest.json").is_file() {
        return Err(format!("there is no plugin named '{}'. list_plugins shows what exists", name));
    }
    Ok(dir)
}

fn read_manifest(dir: &Path) -> Result<Manifest, String> {
    let path = dir.join("manifest.json");
    let text = std::fs::read_to_string(&path).map_err(|e| format!("{}: {}", path.display(), e))?;
    manifest::parse(&json::parse(&text)?)
}

fn write_file(path: &Path, text: &str) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("{}: {}", parent.display(), e))?;
    }
    std::fs::write(path, text).map_err(|e| format!("{}: {}", path.display(), e))
}

fn write_generated(dir: &Path, m: &Manifest) -> Result<(), String> {
    write_file(&dir.join("Cargo.toml"), &gen::cargo_toml(m))?;
    write_file(&dir.join("manifest.json"), &m.to_json().pretty())?;
    write_file(&dir.join("src/declaration.rs"), &gen::declaration(m))?;
    write_file(&dir.join("src/lib.rs"), &gen::lib_rs(m))?;
    write_file(&dir.join("src/bin/render.rs"), &gen::render_bin(m))?;
    write_file(&dir.join("tests/gates.rs"), &gen::gates_test(m))?;
    if !dir.join("src/dsp.rs").exists() {
        write_file(&dir.join("src/dsp.rs"), &gen::dsp_template())?;
    }
    Ok(())
}

/// The accessor each parameter gets, so the agent writes the right call without reading generated code.
fn accessors(m: &Manifest) -> String {
    let mut o = String::new();
    for p in &m.params {
        let per = p.scope == "per_channel";
        let line = match (p.kind.as_str(), per) {
            ("knob", false) => format!("p.{}() -> f32, {} {} to {}", p.id, p.name, p.min, p.max),
            ("knob", true) => format!("p.{}(channel) -> f32, {} {} to {}", p.id, p.name, p.min, p.max),
            ("choice", false) => format!("p.{}() -> usize, index into {:?}", p.id, p.choices),
            ("choice", true) => format!("p.{}(channel) -> usize, index into {:?}", p.id, p.choices),
            ("toggle", false) => format!("p.{}() -> bool", p.id),
            ("toggle", true) => format!("p.{}(channel) -> bool", p.id),
            _ => format!("p.set_{}(value: f32), a meter the plugin writes", p.id),
        };
        o.push_str(&format!("  {}\n", line));
    }
    if o.is_empty() {
        o.push_str("  (no parameters)\n");
    }
    o
}

fn cargo_path() -> String {
    if let Ok(c) = std::env::var("CARGO") {
        if !c.is_empty() {
            return c;
        }
    }
    let home = std::env::var("HOME").unwrap_or_default();
    for candidate in [
        format!("{}/.cargo/bin/cargo", home),
        "/opt/homebrew/bin/cargo".to_string(),
        "/usr/local/bin/cargo".to_string(),
    ] {
        if Path::new(&candidate).exists() {
            return candidate;
        }
    }
    "cargo".to_string()
}

struct Run {
    ok: bool,
    stdout: String,
    stderr: String,
}

fn cargo(ctx: &Ctx, args: &[String]) -> Result<Run, String> {
    let out = Command::new(cargo_path())
        .args(args)
        .current_dir(&ctx.root)
        .env("CARGO_TERM_COLOR", "never")
        .output()
        .map_err(|e| format!("could not run cargo: {}. Set the CARGO environment variable to its path", e))?;
    Ok(Run {
        ok: out.status.success(),
        stdout: String::from_utf8_lossy(&out.stdout).to_string(),
        stderr: String::from_utf8_lossy(&out.stderr).to_string(),
    })
}

fn strings(items: &[&str]) -> Vec<String> {
    items.iter().map(|s| s.to_string()).collect()
}

/// Cargo's progress lines are noise to an agent. What is left is the diagnostics, cut to a readable length.
fn tidy(text: &str) -> String {
    let kept: Vec<&str> = text
        .lines()
        .filter(|l| {
            let t = l.trim_start();
            !(t.starts_with("Compiling ")
                || t.starts_with("Finished ")
                || t.starts_with("Running ")
                || t.starts_with("Blocking ")
                || t.starts_with("Fresh ")
                || t.starts_with("Doc-tests "))
        })
        .collect();
    let mut s = kept.join("\n").trim().to_string();
    const MAX: usize = 7000;
    if s.len() > MAX {
        let mut cut = MAX;
        while !s.is_char_boundary(cut) {
            cut -= 1;
        }
        s.truncate(cut);
        s.push_str("\n... the output is cut here. Fix the first errors and build again");
    }
    s
}

fn build_plugin(ctx: &Ctx, name: &str) -> Result<(bool, String), String> {
    let run = cargo(ctx, &strings(&["build", "-p", &gen::package_name(name)]))?;
    let notes = tidy(&run.stderr);
    if run.ok {
        Ok((
            true,
            if notes.is_empty() {
                "Build succeeded with no warnings.".to_string()
            } else {
                format!("Build succeeded.\n{}", notes)
            },
        ))
    } else {
        Ok((false, format!("Build failed.\n{}", notes)))
    }
}

fn create_plugin(ctx: &Ctx, args: &Json) -> Result<String, String> {
    let m = manifest::parse(args)?;
    let dir = ctx.root.join("plugins").join(&m.name);
    if dir.join("manifest.json").exists() {
        return Err(format!("a plugin named '{}' already exists. Use update_plugin to change it", m.name));
    }
    write_generated(&dir, &m)?;
    let (ok, build) = build_plugin(ctx, &m.name)?;
    let text = format!(
        "Created plugin '{}' ({}) in plugins/{} with {} parameters on {} to {} channels.\n\nAccessors for dsp.rs:\n{}\n{}\n\nNext: write_dsp with the signal processing, then check and render.",
        m.name,
        m.title,
        m.name,
        m.params.len(),
        m.min_channels,
        m.max_channels,
        accessors(&m),
        build
    );
    if ok {
        Ok(text)
    } else {
        Err(text)
    }
}

fn update_plugin(ctx: &Ctx, args: &Json) -> Result<String, String> {
    let name = name_arg(args)?;
    let dir = plugin_dir(ctx, &name)?;
    let existing = json::parse(&std::fs::read_to_string(dir.join("manifest.json")).map_err(|e| e.to_string())?)?;
    let mut merged: Vec<(String, Json)> = vec![("name".to_string(), Json::str(&name))];
    for key in ["title", "description", "channels_min", "channels_max", "parameters"] {
        if let Some(v) = args.get(key).or_else(|| existing.get(key)) {
            merged.push((key.to_string(), v.clone()));
        }
    }
    let m = manifest::parse(&Json::Obj(merged))?;
    write_generated(&dir, &m)?;
    let (ok, build) = build_plugin(ctx, &m.name)?;
    let text = format!(
        "Updated plugin '{}'. Accessors for dsp.rs:\n{}\n{}",
        m.name,
        accessors(&m),
        build
    );
    if ok {
        Ok(text)
    } else {
        Err(format!("{}\nrewrite dsp.rs with write_dsp so it matches the accessors above.", text))
    }
}

fn list_plugins(ctx: &Ctx) -> Result<String, String> {
    let mut lines: Vec<String> = Vec::new();
    if let Ok(entries) = std::fs::read_dir(ctx.root.join("plugins")) {
        let mut dirs: Vec<PathBuf> = entries.filter_map(|e| e.ok().map(|e| e.path())).collect();
        dirs.sort();
        for d in dirs {
            if d.join("manifest.json").is_file() {
                if let Ok(m) = read_manifest(&d) {
                    lines.push(format!(
                        "{}: {}. {} parameters, {} to {} channels.",
                        m.name,
                        m.description.trim_end_matches('.'),
                        m.params.len(),
                        m.min_channels,
                        m.max_channels
                    ));
                }
            }
        }
    }
    if lines.is_empty() {
        Ok("No plugins yet. create_plugin makes one.".to_string())
    } else {
        Ok(lines.join("\n"))
    }
}

fn read_dsp(ctx: &Ctx, args: &Json) -> Result<String, String> {
    let name = name_arg(args)?;
    let dir = plugin_dir(ctx, &name)?;
    let m = read_manifest(&dir)?;
    let source = std::fs::read_to_string(dir.join("src/dsp.rs")).map_err(|e| e.to_string())?;
    Ok(format!("Accessors:\n{}\nsrc/dsp.rs:\n{}", accessors(&m), source))
}

fn write_dsp(ctx: &Ctx, args: &Json) -> Result<String, String> {
    let name = name_arg(args)?;
    let dir = plugin_dir(ctx, &name)?;
    let source = text_arg(args, "source").ok_or("source is required: the complete contents of dsp.rs")?;
    if source.trim().is_empty() {
        return Err("source is empty".to_string());
    }
    if source.contains("unsafe") {
        return Err("dsp.rs may not use unsafe code. The plugin crate forbids it".to_string());
    }
    let path = dir.join("src/dsp.rs");
    if path.exists() {
        let _ = std::fs::copy(&path, dir.join("src/dsp.rs.prev"));
    }
    write_file(&path, source)?;
    let (ok, build) = build_plugin(ctx, &name)?;
    if ok {
        Ok(format!("Wrote dsp.rs for '{}'. {}\nNext: check, then render.", name, build))
    } else {
        Err(format!(
            "Wrote dsp.rs for '{}', and it does not build. The previous version is in src/dsp.rs.prev.\n{}",
            name, build
        ))
    }
}

fn hint(gate: &str) -> &'static str {
    match gate {
        "construct" => "A parameter declaration was refused. Check each default is inside min and max, a log curve has min above zero, and a choice has two or more names.",
        "layout" => "The plugin must accept the same channel count in and out for at least one of 1, 2, 6, 8, 24 or 64 channels. Check channels_min and channels_max.",
        "parameters_described" => "Give every parameter a name and a one-sentence description with update_plugin.",
        "finite_at_extremes" => "Output went non-finite or far too loud at a parameter limit. Look for division by a parameter that can reach zero, log or sqrt of a negative, a filter whose coefficients blow up near Nyquist or at the lowest frequency, and feedback gain at or above 1.",
        "no_alloc" => "process allocated. Remove Vec::new, vec!, push on a Vec that can grow, Box, String, format! and clone of a collection from process. Create buffers in prepare, size them from setup.channels and setup.max_block, and reuse them.",
        "repeatable" => "Two fresh instances disagreed. Look for random numbers seeded from the clock or a counter shared between instances, and for state that is not reset in new() and prepare().",
        "block_size_independent" => "The audio changed with the host's block size. Smooth parameters per sample or with a time constant that does not depend on the block length, and do not restart filters, envelopes or LFOs at block boundaries.",
        _ => "",
    }
}

fn check_plugin(ctx: &Ctx, args: &Json) -> Result<String, String> {
    let name = name_arg(args)?;
    plugin_dir(ctx, &name)?;
    let run = cargo(
        ctx,
        &strings(&["test", "-p", &gen::package_name(&name), "--test", "gates", "--", "--nocapture"]),
    )?;
    let lines: Vec<&str> = run
        .stdout
        .lines()
        .filter(|l| l.starts_with("GATE ") || l.starts_with("GATES "))
        .collect();
    if lines.is_empty() {
        return Err(format!(
            "The checks could not run.\n{}\n{}",
            tidy(&run.stderr),
            tidy(&run.stdout)
        ));
    }
    let mut out = lines.join("\n");
    let mut hints: Vec<String> = Vec::new();
    for l in &lines {
        let mut parts = l.splitn(4, ' ');
        if parts.next() == Some("GATE") {
            let gate = parts.next().unwrap_or("");
            if parts.next() == Some("FAIL") {
                let h = hint(gate);
                if !h.is_empty() {
                    hints.push(format!("{}: {}", gate, h));
                }
            }
        }
    }
    if hints.is_empty() {
        out.push_str("\nEvery gate passed.");
    } else {
        out.push_str("\n\nHow to fix:\n");
        out.push_str(&hints.join("\n"));
    }
    Ok(out)
}

fn push_settings(args: &Json, flags: &mut Vec<String>) -> Result<(), String> {
    if let Some(Json::Obj(items)) = args.get("settings") {
        for (key, value) in items {
            let v = match value {
                Json::Num(n) => *n,
                Json::Bool(b) => {
                    if *b {
                        1.0
                    } else {
                        0.0
                    }
                }
                _ => return Err(format!("settings: '{}' needs a number", key)),
            };
            flags.push("--set".to_string());
            flags.push(format!("{}={}", key, v));
        }
    }
    Ok(())
}

fn render_plugin(ctx: &Ctx, args: &Json) -> Result<String, String> {
    let name = name_arg(args)?;
    let dir = plugin_dir(ctx, &name)?;
    let mut flags: Vec<String> = Vec::new();
    if let Some(s) = text_arg(args, "signal") {
        flags.extend(strings(&["--signal", s]));
    }
    if let Some(s) = text_arg(args, "input_wav") {
        flags.extend(strings(&["--in", s]));
    }
    for (key, flag) in [
        ("channels", "--channels"),
        ("seconds", "--seconds"),
        ("sample_rate", "--rate"),
        ("block", "--block"),
    ] {
        if let Some(n) = args.get(key).and_then(|v| v.as_f64()) {
            flags.push(flag.to_string());
            flags.push(format!("{}", n));
        }
    }
    if args.get("tail").and_then(|v| v.as_bool()) == Some(false) {
        flags.push("--no-tail".to_string());
    }
    push_settings(args, &mut flags)?;
    let stem: String = text_arg(args, "output_name")
        .unwrap_or("latest")
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
        .collect();
    let stem = if stem.is_empty() { "latest".to_string() } else { stem };
    let out_path = dir.join("renders").join(format!("{}.wav", stem));
    flags.push("--out".to_string());
    flags.push(out_path.to_string_lossy().to_string());

    let mut cargo_args = strings(&["run", "-q", "-p", &gen::package_name(&name), "--bin", "render", "--"]);
    cargo_args.extend(flags);
    let run = cargo(ctx, &cargo_args)?;
    if run.ok {
        Ok(run.stdout.trim().to_string())
    } else {
        Err(format!("The render failed.\n{}\n{}", tidy(&run.stderr), tidy(&run.stdout)))
    }
}

fn describe_plugin(ctx: &Ctx, args: &Json) -> Result<String, String> {
    let name = name_arg(args)?;
    plugin_dir(ctx, &name)?;
    let mut flags: Vec<String> = strings(&["--describe"]);
    if let Some(n) = args.get("channels").and_then(|v| v.as_f64()) {
        flags.push("--channels".to_string());
        flags.push(format!("{}", n));
    }
    push_settings(args, &mut flags)?;
    let mut cargo_args = strings(&["run", "-q", "-p", &gen::package_name(&name), "--bin", "render", "--"]);
    cargo_args.extend(flags);
    let run = cargo(ctx, &cargo_args)?;
    if run.ok {
        Ok(run.stdout.trim().to_string())
    } else {
        Err(format!("The description failed.\n{}\n{}", tidy(&run.stderr), tidy(&run.stdout)))
    }
}

/// Runs one tool. `Ok` is the result text, `Err` is a failure the agent should act on.
pub fn call(ctx: &Ctx, tool: &str, args: &Json) -> Result<String, String> {
    match tool {
        "list_plugins" => list_plugins(ctx),
        "create_plugin" => create_plugin(ctx, args),
        "update_plugin" => update_plugin(ctx, args),
        "read_dsp" => read_dsp(ctx, args),
        "write_dsp" => write_dsp(ctx, args),
        "build" => {
            let name = name_arg(args)?;
            plugin_dir(ctx, &name)?;
            let (ok, text) = build_plugin(ctx, &name)?;
            if ok {
                Ok(text)
            } else {
                Err(text)
            }
        }
        "check" => check_plugin(ctx, args),
        "render" => render_plugin(ctx, args),
        "describe" => describe_plugin(ctx, args),
        other => Err(format!("there is no tool named '{}'", other)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_tool_is_defined_with_a_schema() {
        let defs = definitions();
        let list = defs.as_array().unwrap();
        let names: Vec<&str> = list.iter().map(|t| t.get("name").unwrap().as_str().unwrap()).collect();
        assert_eq!(
            names,
            ["list_plugins", "create_plugin", "update_plugin", "read_dsp", "write_dsp", "build", "check", "render", "describe"]
        );
        for t in list {
            assert!(!t.get("description").unwrap().as_str().unwrap().is_empty());
            assert_eq!(t.get("inputSchema").unwrap().get("type").unwrap().as_str(), Some("object"));
        }
        let create = &list[1];
        let items = create
            .get("inputSchema")
            .unwrap()
            .get("properties")
            .unwrap()
            .get("parameters")
            .unwrap()
            .get("items")
            .unwrap();
        assert!(items.get("properties").unwrap().get("scope").is_some());
    }

    #[test]
    fn cargo_progress_lines_are_dropped() {
        let t = tidy("   Compiling a v0\n    Finished dev\nerror[E0425]: cannot find value `x`\n --> src/dsp.rs:3:5");
        assert!(t.starts_with("error[E0425]"));
        assert!(!t.contains("Compiling"));
    }

    #[test]
    fn a_missing_plugin_is_named() {
        let ctx = Ctx {
            root: std::env::temp_dir().join("muse-mcp-no-such-root"),
        };
        let err = call(&ctx, "build", &json::parse(r#"{"name":"nothing"}"#).unwrap()).err().unwrap();
        assert!(err.contains("no plugin named 'nothing'"), "{}", err);
        assert!(call(&ctx, "build", &json::parse(r#"{"name":"../x"}"#).unwrap()).is_err());
        assert!(call(&ctx, "frobnicate", &Json::Null).is_err());
    }
}
