//! The tools an agent uses to design, build and check a plugin.

use crate::gen;
use crate::json::{self, Json};
use crate::manifest::{self, Manifest, Package};
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
"parameters":{"type":"array","items":@PARAM@,"description":"Every parameter the plugin has. May be empty."},
"graph":{"type":"object","description":"Optional. The signal path as a graph of ready-made blocks, instead of writing dsp.rs. See list_blocks."}
},"required":["name","description","parameters"]}},
{"name":"update_plugin","description":"Change a plugin's title, description, channel range or parameters. Give the whole parameter list again, because it replaces the old one. MUSE regenerates the declaration and builds. The signal processing in dsp.rs is kept, so rewrite it afterwards if an accessor it uses has changed.","inputSchema":{"type":"object","properties":{
"name":{"type":"string"},
"title":{"type":"string"},
"description":{"type":"string"},
"channels_min":{"type":"integer"},
"channels_max":{"type":"integer"},
"parameters":{"type":"array","items":@PARAM@},
"graph":{"type":["object","null"],"description":"The signal path as a graph of blocks. Omit to keep the current one. See list_blocks."}
},"required":["name"]}},
{"name":"list_blocks","description":"Describe how to assemble a plugin from ready-made blocks without writing code: the graph format, the expression language that binds a block's arguments to the plugin's parameters, and every block with its arguments, ranges and defaults. The blocks cover filters, parametric and harmonic equalizers, compressors, limiters, gates and expanders, saturation and waveshaping, delay, chorus and flanger, tremolo and mid/side. Read it before set_graph.","inputSchema":{"type":"object","properties":{}}},
{"name":"set_graph","description":"Set a plugin's signal path as a graph of ready-made blocks, with no code to write. The graph is checked against the plugin's parameters, saved in manifest.json, turned into src/dsp.rs and built. Block arguments follow parameters through expressions, so a Drive knob can set a gain and a Tone knob a cutoff. The plugin must already exist with its parameters declared. Anything the blocks cannot do still goes through write_dsp. Pass null as the graph to go back to writing dsp.rs by hand. Call list_blocks for the format.","inputSchema":{"type":"object","properties":{"name":{"type":"string"},"graph":{"type":["object","null"],"description":"A node, with an optional modulators list. See list_blocks."}},"required":["name","graph"]}},
{"name":"read_dsp","description":"Return the current signal processing source, src/dsp.rs, and the accessor each parameter has.","inputSchema":{"type":"object","properties":{"name":{"type":"string"}},"required":["name"]}},
{"name":"write_dsp","description":"Replace the signal processing, src/dsp.rs, with new source and build it. The file defines `pub struct State` with new(), prepare(&mut self, &Setup), latency_samples(), tail_samples() and process(&mut self, &Params, &mut [&mut [f32]], &ProcessContext). process runs on the audio thread, once per block, with one slice per channel, in place. It must not allocate, lock or block, so keep every buffer in State and size it in prepare. Per-channel memory such as filter state lives in State and is sized from setup.channels. Unsafe code is refused. The previous file is kept as dsp.rs.prev. The result is the build output.","inputSchema":{"type":"object","properties":{"name":{"type":"string"},"source":{"type":"string","description":"The complete contents of dsp.rs."}},"required":["name","source"]}},
{"name":"build","description":"Build the plugin and report compiler errors and warnings.","inputSchema":{"type":"object","properties":{"name":{"type":"string"}},"required":["name"]}},
{"name":"check","description":"Run the gates every plugin must pass. They cover the channel layout, parameter descriptions, finite and bounded output with every parameter at its minimum and maximum, no allocation in process, identical output from two fresh instances, and output that does not depend on the host's block size. Each failing gate comes with a hint on how to fix it.","inputSchema":{"type":"object","properties":{"name":{"type":"string"}},"required":["name"]}},
{"name":"package_plugin","description":"Package a plugin as a CLAP plugin, an Audio Unit or both, so a DAW can load it. The Audio Unit is a macOS v2 effect, which Logic, GarageBand and other hosts load. MUSE saves the settings given here in manifest.json, generates the packaging crate, builds the shared library in release mode, assembles each bundle (.clap or .component) under the plugin's dist folder and signs it ad hoc on macOS. It then loads the bundle the way a host does and checks it. The check covers the entry or factory, ports or formats, parameters and their text, processing, parameter changes, state save and load, and changing the channel count. Later calls need only the name, because the settings persist. vendor is required the first time. Set install to true only when the person has asked for the plugin to be installed. It then copies each bundle into the user's plug-in folder, after every check has passed, and for an Audio Unit runs Apple's auval validator on the installed component. A plugin that fails the gates in check is not packaged.","inputSchema":{"type":"object","properties":{
"name":{"type":"string"},
"vendor":{"type":"string","description":"The company or person a host shows beside the plugin."},
"url":{"type":"string","description":"A web address for the plugin or its maker. Optional."},
"version":{"type":"string","description":"Defaults to 0.0.1."},
"id":{"type":"string","description":"The plugin's identity in reverse-domain style, such as com.example.warm-saturator. Defaults to the vendor and the plugin name joined by a dot."},
"features":{"type":"array","items":{"type":"string"},"description":"CLAP feature names such as audio-effect, distortion, filter, delay or reverb. Defaults to audio-effect."},
"formats":{"type":"array","items":{"type":"string","enum":["clap","au"]},"description":"Which formats to build, clap, au or both. Defaults to clap and is saved."},
"au_subtype":{"type":"string","description":"Four letters or digits that identify an Audio Unit, such as Wsat. Defaults to a code made from the plugin name."},
"au_manufacturer":{"type":"string","description":"Four letters or digits with at least one capital that identify the maker of an Audio Unit, such as Symb. Defaults to a code made from the vendor."},
"install":{"type":"boolean","description":"Copy each verified bundle into the user's plug-in folder, and validate an Audio Unit with auval. Defaults to false."}
},"required":["name"]}},
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

/// Where plugins live: `plugins/` for the person's own, `examples/` for the ones MUSE ships.
/// New plugins always go in `plugins/`.
const PLUGIN_FOLDERS: [&str; 2] = ["plugins", "examples"];

/// The folder of an existing plugin, whichever of the plugin folders holds it.
fn find_plugin(ctx: &Ctx, name: &str) -> Option<PathBuf> {
    PLUGIN_FOLDERS
        .iter()
        .map(|f| ctx.root.join(f).join(name))
        .find(|d| d.join("manifest.json").is_file())
}

fn plugin_dir(ctx: &Ctx, name: &str) -> Result<PathBuf, String> {
    if !manifest::valid_slug(name) {
        return Err(format!("'{}' is not a valid plugin name", name));
    }
    find_plugin(ctx, name).ok_or_else(|| format!("there is no plugin named '{}'. list_plugins shows what exists", name))
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
    // A graph writes dsp.rs. Hand-written code is kept as dsp.rs.prev when a graph replaces it. When the graph is
    // taken away, a dsp.rs that was generated from it goes back to the empty template, and written code stays.
    let dsp = dir.join("src/dsp.rs");
    let existing = std::fs::read_to_string(&dsp).ok();
    match &m.graph {
        Some(g) => {
            if existing.as_deref().is_some_and(|s| !s.starts_with(gen::GRAPH_DSP_MARKER)) {
                let _ = std::fs::copy(&dsp, dir.join("src/dsp.rs.prev"));
            }
            write_file(&dsp, &gen::graph_dsp(m, g))?;
        }
        None => {
            if existing.as_deref().is_none_or(|s| s.starts_with(gen::GRAPH_DSP_MARKER)) {
                write_file(&dsp, &gen::dsp_template())?;
            }
        }
    }
    if let Some(p) = &m.package {
        if p.formats.iter().any(|f| f == "clap") {
            let clap_dir = dir.with_file_name(gen::clap_dir_name(&m.name));
            write_file(&clap_dir.join("Cargo.toml"), &gen::clap_cargo_toml(m))?;
            write_file(&clap_dir.join("src/lib.rs"), &gen::clap_lib_rs(m, p))?;
        }
        if p.formats.iter().any(|f| f == "au") {
            let au_dir = dir.with_file_name(gen::au_dir_name(&m.name));
            write_file(&au_dir.join("Cargo.toml"), &gen::au_cargo_toml(m))?;
            write_file(&au_dir.join("src/lib.rs"), &gen::au_lib_rs(m))?;
        }
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
    if let Some(existing) = find_plugin(ctx, &m.name) {
        return Err(format!(
            "a plugin named '{}' already exists in {}. Use update_plugin to change it, or pick another name",
            m.name,
            existing.display()
        ));
    }
    let dir = ctx.root.join("plugins").join(&m.name);
    write_generated(&dir, &m)?;
    let (ok, build) = build_plugin(ctx, &m.name)?;
    let text = format!(
        "Created plugin '{}' ({}) in plugins/{} with {} parameters on {} to {} channels.\n\nAccessors for dsp.rs:\n{}\n{}\n\n{}",
        m.name,
        m.title,
        m.name,
        m.params.len(),
        m.min_channels,
        m.max_channels,
        accessors(&m),
        build,
        if m.graph.is_some() {
            "The signal path is the graph you gave, so src/dsp.rs is generated. Next: check, then render."
        } else {
            "Next: set_graph to assemble the signal path from blocks (list_blocks describes them), or write_dsp for code the blocks cannot do. Then check and render."
        }
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
    for key in ["format", "title", "description", "channels_min", "channels_max", "parameters", "package", "graph"] {
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

fn set_graph(ctx: &Ctx, args: &Json) -> Result<String, String> {
    let name = name_arg(args)?;
    let graph = args
        .get("graph")
        .ok_or("graph is required: a node object, or null to go back to writing dsp.rs by hand. list_blocks describes the format")?;
    let change = Json::Obj(vec![
        ("name".to_string(), Json::str(&name)),
        ("graph".to_string(), graph.clone()),
    ]);
    let built = update_plugin(ctx, &change)?;
    if matches!(graph, Json::Null) {
        Ok(format!(
            "Removed the graph from '{}'. src/dsp.rs is the empty template again, so write it with write_dsp.\n{}",
            name, built
        ))
    } else {
        Ok(format!(
            "Set the graph of '{}'. src/dsp.rs is generated from it, and the plugin builds.\n{}\nNext: check, then render to measure it.",
            name, built
        ))
    }
}

fn list_plugins(ctx: &Ctx) -> Result<String, String> {
    let mut lines: Vec<String> = Vec::new();
    for folder in PLUGIN_FOLDERS {
        let Ok(entries) = std::fs::read_dir(ctx.root.join(folder)) else {
            continue;
        };
        let mut dirs: Vec<PathBuf> = entries.filter_map(|e| e.ok().map(|e| e.path())).collect();
        dirs.sort();
        for d in dirs {
            if d.join("manifest.json").is_file() {
                if let Ok(m) = read_manifest(&d) {
                    lines.push(format!(
                        "{}: {}. {} parameters, {} to {} channels. In {}/.",
                        m.name,
                        m.description.trim_end_matches('.'),
                        m.params.len(),
                        m.min_channels,
                        m.max_channels,
                        folder
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
    if read_manifest(&dir)?.graph.is_some() {
        return Err(format!(
            "'{}' has a graph, so src/dsp.rs is generated from it. Change the signal path with set_graph. To write dsp.rs by hand instead, call set_graph with the graph set to null first.",
            name
        ));
    }
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
        "no_subnormals" => "Subnormal floats came out after the input went silent. A decaying filter or feedback state reaches them, and they can slow a processor to a crawl in a host that does not flush them. Zero a state when it falls below about 1e-20, once per block, or build the signal path from blocks, which do it already.",
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

    let mut cargo_args = strings(&["run", "-q", "-p", &gen::package_name(&name), "--bin", &gen::render_bin_name(&name), "--"]);
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
    let mut cargo_args = strings(&["run", "-q", "-p", &gen::package_name(&name), "--bin", &gen::render_bin_name(&name), "--"]);
    cargo_args.extend(flags);
    let run = cargo(ctx, &cargo_args)?;
    if run.ok {
        Ok(run.stdout.trim().to_string())
    } else {
        Err(format!("The description failed.\n{}\n{}", tidy(&run.stderr), tidy(&run.stdout)))
    }
}

fn target_dir(ctx: &Ctx) -> PathBuf {
    match std::env::var("CARGO_TARGET_DIR") {
        Ok(d) if !d.is_empty() => PathBuf::from(d),
        _ => ctx.root.join("target"),
    }
}

fn library_path(ctx: &Ctx, name: &str) -> PathBuf {
    let stem = gen::clap_library_stem(name);
    let file = if cfg!(target_os = "macos") {
        format!("lib{}.dylib", stem)
    } else if cfg!(target_os = "windows") {
        format!("{}.dll", stem)
    } else {
        format!("lib{}.so", stem)
    };
    target_dir(ctx).join("release").join(file)
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn bundle_file_name(title: &str, fallback: &str) -> String {
    let cleaned: String = title
        .chars()
        .filter(|c| !c.is_control() && !matches!(c, '/' | '\\' | ':'))
        .collect();
    let cleaned = cleaned.trim().to_string();
    if cleaned.is_empty() {
        fallback.to_string()
    } else {
        cleaned
    }
}

/// Builds the `.clap` from the shared library. On macOS that is a bundle folder. Elsewhere it is the library renamed.
fn assemble_bundle(dist: &Path, file: &str, id: &str, version: &str, lib: &Path) -> Result<(PathBuf, String), String> {
    let bundle = dist.join(format!("{}.clap", file));
    if bundle.is_dir() {
        std::fs::remove_dir_all(&bundle).map_err(|e| format!("{}: {}", bundle.display(), e))?;
    } else if bundle.exists() {
        std::fs::remove_file(&bundle).map_err(|e| format!("{}: {}", bundle.display(), e))?;
    }
    if !cfg!(target_os = "macos") {
        std::fs::create_dir_all(dist).map_err(|e| format!("{}: {}", dist.display(), e))?;
        std::fs::copy(lib, &bundle).map_err(|e| format!("{}: {}", bundle.display(), e))?;
        return Ok((bundle, String::new()));
    }
    let macos = bundle.join("Contents/MacOS");
    std::fs::create_dir_all(&macos).map_err(|e| format!("{}: {}", macos.display(), e))?;
    std::fs::copy(lib, macos.join(file)).map_err(|e| format!("{}: {}", lib.display(), e))?;
    let plist = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n<plist version=\"1.0\">\n<dict>\n\t<key>CFBundleDevelopmentRegion</key>\n\t<string>English</string>\n\t<key>CFBundleExecutable</key>\n\t<string>{file}</string>\n\t<key>CFBundleIdentifier</key>\n\t<string>{id}</string>\n\t<key>CFBundleInfoDictionaryVersion</key>\n\t<string>6.0</string>\n\t<key>CFBundleName</key>\n\t<string>{file}</string>\n\t<key>CFBundlePackageType</key>\n\t<string>BNDL</string>\n\t<key>CFBundleSignature</key>\n\t<string>????</string>\n\t<key>CFBundleShortVersionString</key>\n\t<string>{version}</string>\n\t<key>CFBundleVersion</key>\n\t<string>{version}</string>\n\t<key>NSHighResolutionCapable</key>\n\t<true/>\n</dict>\n</plist>\n",
        file = xml_escape(file),
        id = xml_escape(id),
        version = xml_escape(version)
    );
    write_file(&bundle.join("Contents/Info.plist"), &plist)?;
    write_file(&bundle.join("Contents/PkgInfo"), "BNDL????")?;
    let note = match Command::new("/usr/bin/codesign").args(["--force", "--sign", "-"]).arg(&bundle).output() {
        Ok(o) if o.status.success() => "Signed ad hoc, which is enough to load on this machine.".to_string(),
        Ok(o) => format!(
            "Ad hoc signing failed, so macOS may refuse to load the bundle: {}",
            String::from_utf8_lossy(&o.stderr).trim()
        ),
        Err(e) => format!("Could not run codesign, so the bundle is unsigned: {}", e),
    };
    Ok((bundle, note))
}

fn copy_dir_all(src: &Path, dst: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let to = dst.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir_all(&entry.path(), &to)?;
        } else {
            std::fs::copy(entry.path(), &to)?;
        }
    }
    Ok(())
}

/// The shared install script, when the workspace sits beside one, as the Symbia Labs repos do:
/// `<workspace>/../scripts/install-app.sh`. Every build on such a machine then installs the same way.
fn install_script(ctx: &Ctx) -> Option<PathBuf> {
    let script = ctx.root.parent()?.join("scripts").join("install-app.sh");
    script.is_file().then_some(script)
}

/// Installs a bundle into one of the user's plug-in folders, such as `Audio/Plug-Ins/CLAP`.
/// The shared install script does it when there is one; otherwise the bundle is copied in here.
fn install_bundle(ctx: &Ctx, bundle: &Path, library_folder: &str) -> Result<PathBuf, String> {
    if !cfg!(target_os = "macos") {
        return Err("installing is only implemented for macOS. The bundle is in the dist folder".to_string());
    }
    let home = std::env::var("HOME").map_err(|_| "HOME is not set".to_string())?;
    let folder = PathBuf::from(home).join("Library").join(library_folder);
    let target = folder.join(bundle.file_name().ok_or("the bundle has no file name")?);
    if let Some(script) = install_script(ctx) {
        let out = Command::new("/bin/zsh")
            .arg(&script)
            .arg(bundle)
            .output()
            .map_err(|e| format!("could not run {}: {}", script.display(), e))?;
        if !out.status.success() {
            let said = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
            return Err(format!("{} failed: {}", script.display(), said.trim()));
        }
        return Ok(target);
    }
    // A folder that exists but cannot be read (a sandbox, or missing permission) makes
    // create_dir_all report "File exists", which points at the wrong cause.
    if let Err(e) = std::fs::create_dir_all(&folder) {
        return Err(if e.kind() == std::io::ErrorKind::AlreadyExists {
            format!("{} exists but this process cannot read or write it; a sandbox or missing permission is the usual cause", folder.display())
        } else {
            format!("{}: {}", folder.display(), e)
        });
    }
    if target.is_dir() {
        std::fs::remove_dir_all(&target).map_err(|e| format!("{}: {}", target.display(), e))?;
    } else if target.exists() {
        std::fs::remove_file(&target).map_err(|e| format!("{}: {}", target.display(), e))?;
    }
    if bundle.is_dir() {
        copy_dir_all(bundle, &target).map_err(|e| format!("{}: {}", target.display(), e))?;
    } else {
        std::fs::copy(bundle, &target).map_err(|e| format!("{}: {}", target.display(), e))?;
    }
    Ok(target)
}

fn package_plugin(ctx: &Ctx, args: &Json) -> Result<String, String> {
    let name = name_arg(args)?;
    let dir = plugin_dir(ctx, &name)?;
    let mut m = read_manifest(&dir)?;

    // The settings given now are saved, so later calls only need the name.
    let mut pkg = m.package.clone().unwrap_or_default();
    if let Some(v) = text_arg(args, "vendor") {
        pkg.vendor = v.to_string();
    }
    if let Some(v) = text_arg(args, "url") {
        pkg.url = v.to_string();
    }
    if let Some(v) = text_arg(args, "version") {
        pkg.version = v.to_string();
    }
    if let Some(v) = text_arg(args, "id") {
        pkg.id = v.to_string();
    }
    if let Some(Json::Arr(list)) = args.get("features") {
        pkg.features = list.iter().filter_map(|f| f.as_str().map(|s| s.trim().to_string())).collect();
    }
    if let Some(Json::Arr(list)) = args.get("formats") {
        pkg.formats = list.iter().filter_map(|f| f.as_str().map(|s| s.trim().to_ascii_lowercase())).collect();
    }
    if let Some(v) = text_arg(args, "au_subtype") {
        pkg.au_subtype = v.to_string();
    }
    if let Some(v) = text_arg(args, "au_manufacturer") {
        pkg.au_manufacturer = v.to_string();
    }
    pkg.complete(&name)?;
    m.package = Some(pkg.clone());
    write_generated(&dir, &m)?;

    // A plugin that fails its gates does not get packaged.
    let gates = check_plugin(ctx, &Json::obj(vec![("name", Json::str(&name))]))?;
    if !gates.ends_with("Every gate passed.") {
        return Err(format!(
            "The plugin fails its gates, so it was not packaged. The package settings were saved.\n{}",
            gates
        ));
    }

    let install = args.get("install").and_then(|v| v.as_bool()) == Some(true);
    let mut text = format!("Packaged '{}'.\nSettings, saved in manifest.json: {}\n", name, package_summary(&pkg));
    for format in &pkg.formats {
        let section = match format.as_str() {
            "clap" => package_clap(ctx, &name, &m, &pkg, install),
            "au" => package_au(ctx, &name, &m, &pkg, install),
            other => Err(format!("unknown format {}", other)),
        };
        match section {
            Ok(s) => {
                text.push('\n');
                text.push_str(&s);
            }
            Err(e) => {
                text.push('\n');
                text.push_str(&e);
                return Err(text);
            }
        }
    }
    Ok(text)
}

fn package_summary(pkg: &Package) -> String {
    let mut s = format!(
        "vendor {}, id {}, version {}, features {}, formats {}",
        pkg.vendor,
        pkg.id,
        pkg.version,
        pkg.features.join(", "),
        pkg.formats.join(", ")
    );
    if pkg.formats.iter().any(|f| f == "au") {
        s.push_str(&format!(", Audio Unit codes aufx/{}/{}", pkg.au_subtype, pkg.au_manufacturer));
    }
    s
}

/// Builds the CLAP plugin, bundles it, checks it by loading it, and installs it when asked.
fn package_clap(ctx: &Ctx, name: &str, m: &Manifest, pkg: &Package, install: bool) -> Result<String, String> {
    let dir = plugin_dir(ctx, name)?;
    let run = cargo(
        ctx,
        &strings(&["build", "--release", "-p", &gen::clap_package_name(name), "-p", "muse-clap"]),
    )?;
    if !run.ok {
        return Err(format!("The release build failed.\n{}", tidy(&run.stderr)));
    }
    let lib = library_path(ctx, name);
    if !lib.is_file() {
        return Err(format!("The build succeeded but {} is missing", lib.display()));
    }
    let file = bundle_file_name(&m.title, &m.name);
    let (bundle, sign_note) = assemble_bundle(&dir.join("dist"), &file, &pkg.id, &pkg.version, &lib)?;

    let verifier = target_dir(ctx).join("release").join("muse-clap-verify");
    let out = Command::new(&verifier)
        .arg(&bundle)
        .output()
        .map_err(|e| format!("could not run {}: {}", verifier.display(), e))?;
    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
    let lines: Vec<&str> = stdout
        .lines()
        .filter(|l| l.starts_with("CHECK ") || l.starts_with("CHECKS "))
        .collect();
    let mut text = format!(
        "CLAP bundle: {}\n{}\nVerification, loading the bundle the way a host does:\n{}",
        bundle.display(),
        sign_note,
        lines.join("\n")
    );
    if lines.last().is_none_or(|l| !l.starts_with("CHECKS ")) {
        text.push_str(&format!(
            "\nThe verifier stopped before it finished ({}). {}",
            out.status,
            String::from_utf8_lossy(&out.stderr).trim()
        ));
        return Err(text);
    }
    if !out.status.success() {
        text.push_str("\nAt least one check failed, so the plugin was not installed. Fix the failing check, then call package_plugin again.");
        return Err(text);
    }
    if install {
        match install_bundle(ctx, &bundle, "Audio/Plug-Ins/CLAP") {
            Ok(target) => text.push_str(&format!("\nInstalled to {}. Rescan plugins in the host to see it.", target.display())),
            Err(e) => {
                text.push_str(&format!("\nThe install failed, so the bundle stays in the dist folder: {}", e));
                return Err(text);
            }
        }
    } else {
        text.push_str("\nNot installed. Call package_plugin again with install set to true once the person asks for it.");
    }
    Ok(text)
}

/// The version as the single number an Audio Unit's Info.plist wants, major in the top bytes.
fn au_version_number(version: &str) -> u32 {
    let parts: Vec<u32> = version
        .split(|c: char| !c.is_ascii_digit())
        .filter(|p| !p.is_empty())
        .filter_map(|p| p.parse().ok())
        .collect();
    let part = |i: usize| parts.get(i).copied().unwrap_or(0).min(255);
    let n = (part(0) << 16) | (part(1) << 8) | part(2);
    n.max(1)
}

/// Builds a `.component` bundle from the shared library, with the entry an Audio Unit host reads.
fn assemble_component(dist: &Path, file: &str, m: &Manifest, pkg: &Package, lib: &Path) -> Result<(PathBuf, String), String> {
    let bundle = dist.join(format!("{}.component", file));
    if bundle.is_dir() {
        std::fs::remove_dir_all(&bundle).map_err(|e| format!("{}: {}", bundle.display(), e))?;
    }
    let macos = bundle.join("Contents/MacOS");
    std::fs::create_dir_all(&macos).map_err(|e| format!("{}: {}", macos.display(), e))?;
    std::fs::copy(lib, macos.join(file)).map_err(|e| format!("{}: {}", lib.display(), e))?;
    let description = m.description.lines().next().unwrap_or("").trim().to_string();
    let plist = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n<plist version=\"1.0\">\n<dict>\n\t<key>CFBundleDevelopmentRegion</key>\n\t<string>English</string>\n\t<key>CFBundleExecutable</key>\n\t<string>{file}</string>\n\t<key>CFBundleIdentifier</key>\n\t<string>{id}.au</string>\n\t<key>CFBundleInfoDictionaryVersion</key>\n\t<string>6.0</string>\n\t<key>CFBundleName</key>\n\t<string>{file}</string>\n\t<key>CFBundlePackageType</key>\n\t<string>BNDL</string>\n\t<key>CFBundleSignature</key>\n\t<string>????</string>\n\t<key>CFBundleShortVersionString</key>\n\t<string>{version}</string>\n\t<key>CFBundleVersion</key>\n\t<string>{version}</string>\n\t<key>AudioComponents</key>\n\t<array>\n\t\t<dict>\n\t\t\t<key>name</key>\n\t\t\t<string>{vendor}: {title}</string>\n\t\t\t<key>description</key>\n\t\t\t<string>{description}</string>\n\t\t\t<key>manufacturer</key>\n\t\t\t<string>{manufacturer}</string>\n\t\t\t<key>subtype</key>\n\t\t\t<string>{subtype}</string>\n\t\t\t<key>type</key>\n\t\t\t<string>aufx</string>\n\t\t\t<key>version</key>\n\t\t\t<integer>{number}</integer>\n\t\t\t<key>factoryFunction</key>\n\t\t\t<string>{factory}</string>\n\t\t\t<key>sandboxSafe</key>\n\t\t\t<true/>\n\t\t\t<key>tags</key>\n\t\t\t<array>\n\t\t\t\t<string>Effects</string>\n\t\t\t</array>\n\t\t</dict>\n\t</array>\n</dict>\n</plist>\n",
        file = xml_escape(file),
        id = xml_escape(&pkg.id),
        version = xml_escape(&pkg.version),
        vendor = xml_escape(&pkg.vendor),
        title = xml_escape(&m.title),
        description = xml_escape(&description),
        manufacturer = xml_escape(&pkg.au_manufacturer),
        subtype = xml_escape(&pkg.au_subtype),
        number = au_version_number(&pkg.version),
        factory = xml_escape(&gen::au_factory_name(&m.name)),
    );
    write_file(&bundle.join("Contents/Info.plist"), &plist)?;
    write_file(&bundle.join("Contents/PkgInfo"), "BNDL????")?;
    let note = match Command::new("/usr/bin/codesign").args(["--force", "--sign", "-"]).arg(&bundle).output() {
        Ok(o) if o.status.success() => "Signed ad hoc, which is enough to load on this machine.".to_string(),
        Ok(o) => format!(
            "Ad hoc signing failed, so macOS may refuse to load the bundle: {}",
            String::from_utf8_lossy(&o.stderr).trim()
        ),
        Err(e) => format!("Could not run codesign, so the bundle is unsigned: {}", e),
    };
    Ok((bundle, note))
}

/// Runs Apple's validator on an installed component and returns whether it passed and the lines worth reading.
fn run_auval(subtype: &str, manufacturer: &str) -> (bool, String) {
    let mut last = String::new();
    // A component only becomes visible to auval once the system has registered it, which can take a moment.
    for attempt in 0..3 {
        if attempt > 0 {
            std::thread::sleep(std::time::Duration::from_secs(3));
        }
        let out = match Command::new("/usr/bin/auval").args(["-v", "aufx", subtype, manufacturer]).output() {
            Ok(o) => o,
            Err(e) => return (false, format!("could not run auval: {}", e)),
        };
        let all = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
        let passed = all.contains("AU VALIDATION SUCCEEDED");
        let missing = all.contains("didn't find") || all.contains("Cannot open component") || all.contains("FATAL ERROR: Cannot");
        let lines: Vec<&str> = all.lines().collect();
        let mut picked: Vec<&str> = lines
            .iter()
            .copied()
            .filter(|l| l.contains("FAIL") || l.contains("ERROR") || l.contains("WARNING") || l.contains("VALIDATION"))
            .collect();
        if picked.is_empty() {
            picked = lines.iter().rev().take(8).rev().copied().collect();
        }
        last = picked.into_iter().take(40).collect::<Vec<_>>().join("\n");
        if passed || !missing {
            return (passed, last);
        }
    }
    (false, last)
}

/// Builds the Audio Unit, bundles it, checks it by loading it, and installs and validates it when asked.
fn package_au(ctx: &Ctx, name: &str, m: &Manifest, pkg: &Package, install: bool) -> Result<String, String> {
    if !cfg!(target_os = "macos") {
        return Err("Audio Units can only be built on macOS".to_string());
    }
    let dir = plugin_dir(ctx, name)?;
    let run = cargo(
        ctx,
        &strings(&["build", "--release", "-p", &gen::au_package_name(name), "-p", "muse-au"]),
    )?;
    if !run.ok {
        return Err(format!("The Audio Unit release build failed.\n{}", tidy(&run.stderr)));
    }
    let lib = target_dir(ctx).join("release").join(format!("lib{}.dylib", gen::au_library_stem(name)));
    if !lib.is_file() {
        return Err(format!("The build succeeded but {} is missing", lib.display()));
    }
    let file = bundle_file_name(&m.title, &m.name);
    let (bundle, sign_note) = assemble_component(&dir.join("dist"), &file, m, pkg, &lib)?;

    let verifier = target_dir(ctx).join("release").join("muse-au-verify");
    let out = Command::new(&verifier)
        .arg(&bundle)
        .arg(gen::au_factory_name(name))
        .arg(&pkg.au_subtype)
        .arg(&pkg.au_manufacturer)
        .output()
        .map_err(|e| format!("could not run {}: {}", verifier.display(), e))?;
    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
    let lines: Vec<&str> = stdout
        .lines()
        .filter(|l| l.starts_with("CHECK ") || l.starts_with("CHECKS "))
        .collect();
    let mut text = format!(
        "Audio Unit bundle: {}\n{}\nVerification, loading the bundle the way a host does:\n{}",
        bundle.display(),
        sign_note,
        lines.join("\n")
    );
    if lines.last().is_none_or(|l| !l.starts_with("CHECKS ")) {
        text.push_str(&format!(
            "\nThe verifier stopped before it finished ({}). {}",
            out.status,
            String::from_utf8_lossy(&out.stderr).trim()
        ));
        return Err(text);
    }
    if !out.status.success() {
        text.push_str("\nAt least one check failed, so the plugin was not installed. Fix the failing check, then call package_plugin again.");
        return Err(text);
    }
    if !install {
        text.push_str("\nNot installed. Call package_plugin again with install set to true once the person asks for it. Apple's auval runs after the install, because it only sees installed components.");
        return Ok(text);
    }
    match install_bundle(ctx, &bundle, "Audio/Plug-Ins/Components") {
        Ok(target) => text.push_str(&format!("\nInstalled to {}.", target.display())),
        Err(e) => {
            text.push_str(&format!("\nThe install failed, so the bundle stays in the dist folder: {}", e));
            return Err(text);
        }
    }
    let (passed, report) = run_auval(&pkg.au_subtype, &pkg.au_manufacturer);
    text.push_str(&format!(
        "\nApple's auval, auval -v aufx {} {}: {}\n{}",
        pkg.au_subtype,
        pkg.au_manufacturer,
        if passed { "PASSED" } else { "FAILED" },
        report
    ));
    if passed {
        Ok(text)
    } else {
        Err(text)
    }
}

/// Runs one tool. `Ok` is the result text, `Err` is a failure the agent should act on.
pub fn call(ctx: &Ctx, tool: &str, args: &Json) -> Result<String, String> {
    match tool {
        "list_plugins" => list_plugins(ctx),
        "create_plugin" => create_plugin(ctx, args),
        "update_plugin" => update_plugin(ctx, args),
        "list_blocks" => Ok(muse_blocks::guide()),
        "set_graph" => set_graph(ctx, args),
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
        "package_plugin" => package_plugin(ctx, args),
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
            [
                "list_plugins",
                "create_plugin",
                "update_plugin",
                "list_blocks",
                "set_graph",
                "read_dsp",
                "write_dsp",
                "build",
                "check",
                "package_plugin",
                "render",
                "describe"
            ]
        );
        for t in list {
            assert!(!t.get("description").unwrap().as_str().unwrap().is_empty());
            assert_eq!(t.get("inputSchema").unwrap().get("type").unwrap().as_str(), Some("object"));
        }
        let package = list.iter().find(|t| t.get("name").unwrap().as_str() == Some("package_plugin")).unwrap();
        let required = package.get("inputSchema").unwrap().get("required").unwrap().as_array().unwrap();
        assert_eq!(required.len(), 1, "only the name is required, the settings persist");
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

    #[test]
    fn the_shared_install_script_is_found_beside_the_workspace() {
        let base = std::env::temp_dir().join(format!("muse-mcp-install-script-{}", std::process::id()));
        let ctx = Ctx { root: base.join("MUSE") };
        std::fs::create_dir_all(&ctx.root).unwrap();
        assert_eq!(install_script(&ctx), None);
        std::fs::create_dir_all(base.join("scripts")).unwrap();
        std::fs::write(base.join("scripts").join("install-app.sh"), "#!/bin/zsh\n").unwrap();
        assert_eq!(install_script(&ctx), Some(base.join("scripts").join("install-app.sh")));
        std::fs::remove_dir_all(&base).unwrap();
    }
}
