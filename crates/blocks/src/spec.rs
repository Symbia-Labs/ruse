//! The description of a graph, as it sits in a plugin's manifest.
//!
//! ```json
//! {
//!   "modulators": [{ "id": "lfo1", "shape": "sine", "rate": "rate_hz" }],
//!   "chain": [
//!     { "block": "gain", "args": { "gain_db": "drive" } },
//!     { "block": "shaper", "args": { "curve": "mode" } },
//!     { "parallel": [
//!         { "node": { "chain": [] } },
//!         { "node": { "block": "delay", "args": { "time_ms": "sync(0.5)" } }, "gain_db": -6 }
//!     ] }
//!   ]
//! }
//! ```
//!
//! A node is a `block`, a `chain` that runs its nodes one after another, or a `parallel` that runs its branches
//! on the same input and adds them. Any node can also carry `bypass` and `mix`, which take a number or an
//! expression, and `only`, a list of channel numbers it applies to.

use ruse_core::value::Json;

#[derive(Clone, Debug, PartialEq)]
pub enum ArgValue {
    Num(f64),
    /// An expression, or the name of a choice.
    Text(String),
}

#[derive(Clone, Debug, PartialEq)]
pub struct NodeSpec {
    pub kind: NodeKind,
    pub bypass: Option<ArgValue>,
    pub mix: Option<ArgValue>,
    pub only: Option<Vec<usize>>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum NodeKind {
    Block {
        name: String,
        args: Vec<(String, ArgValue)>,
        /// Block meter name, then the id of the parameter that shows it.
        meters: Vec<(String, String)>,
    },
    Chain(Vec<NodeSpec>),
    Parallel(Vec<BranchSpec>),
    /// Like parallel, but the branches' outputs are multiplied rather than added.
    Multiply(Vec<NodeSpec>),
    Feedback(Box<FeedbackSpec>),
    Split(Box<SplitSpec>),
    Oversample(Box<OversampleSpec>),
}

/// A node run at a higher sample rate, so what it adds above the plugin's Nyquist frequency does not fold back.
#[derive(Clone, Debug, PartialEq)]
pub struct OversampleSpec {
    /// 2, 4 or 8.
    pub factor: u32,
    pub node: NodeSpec,
}

/// Bands divided by Linkwitz-Riley crossovers, each through its own node, then added back.
#[derive(Clone, Debug, PartialEq)]
pub struct SplitSpec {
    /// The crossover frequencies, lowest first.
    pub freqs: Vec<ArgValue>,
    /// 24 or 48 dB per octave.
    pub slope: u32,
    /// One node per band, lowest first: one more than there are frequencies.
    pub bands: Vec<NodeSpec>,
}

/// A node run in a loop: its output returns to its input after `delay_ms`, scaled by `gain`.
#[derive(Clone, Debug, PartialEq)]
pub struct FeedbackSpec {
    pub node: NodeSpec,
    pub gain: Option<ArgValue>,
    pub delay_ms: Option<ArgValue>,
    /// The longest loop delay, fixed when the plugin is built. It sets the memory used.
    pub max_ms: Option<f64>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct BranchSpec {
    pub node: NodeSpec,
    pub gain_db: Option<ArgValue>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ModSpec {
    pub id: String,
    pub shape: String,
    /// An oscillator's rate in Hz. An envelope has none.
    pub rate: Option<ArgValue>,
    /// An envelope's attack and release in ms, and its detector, peak or rms. An oscillator has none.
    pub attack_ms: Option<ArgValue>,
    pub release_ms: Option<ArgValue>,
    pub detector: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct GraphSpec {
    pub modulators: Vec<ModSpec>,
    pub root: NodeSpec,
}

impl NodeSpec {
    fn collect_meter_params(&self, out: &mut Vec<String>) {
        match &self.kind {
            NodeKind::Block { meters, .. } => out.extend(meters.iter().map(|(_, id)| id.clone())),
            NodeKind::Chain(nodes) => nodes.iter().for_each(|n| n.collect_meter_params(out)),
            NodeKind::Parallel(branches) => branches.iter().for_each(|b| b.node.collect_meter_params(out)),
            NodeKind::Multiply(nodes) => nodes.iter().for_each(|n| n.collect_meter_params(out)),
            NodeKind::Feedback(f) => f.node.collect_meter_params(out),
            NodeKind::Split(s) => s.bands.iter().for_each(|n| n.collect_meter_params(out)),
            NodeKind::Oversample(o) => o.node.collect_meter_params(out),
        }
    }
}

impl GraphSpec {
    /// The id of every parameter a block meter is bound to. The plugin must declare each as a meter.
    pub fn meter_params(&self) -> Vec<String> {
        let mut out = Vec::new();
        self.root.collect_meter_params(&mut out);
        out
    }
}

fn value(j: &Json, what: &str) -> Result<ArgValue, String> {
    match j {
        Json::Num(n) => Ok(ArgValue::Num(*n)),
        Json::Str(s) => Ok(ArgValue::Text(s.clone())),
        Json::Bool(b) => Ok(ArgValue::Num(if *b { 1.0 } else { 0.0 })),
        _ => Err(format!("{} must be a number or a string", what)),
    }
}

fn only_keys(obj: &Json, allowed: &[&str], what: &str) -> Result<(), String> {
    if let Json::Obj(items) = obj {
        for (k, _) in items {
            if !allowed.contains(&k.as_str()) {
                return Err(format!("{} has an unknown key '{}'. It can have: {}", what, k, allowed.join(", ")));
            }
        }
        Ok(())
    } else {
        Err(format!("{} must be an object", what))
    }
}

/// How deep subgraphs may use one another. It also stops a subgraph that uses itself.
const MAX_DEPTH: usize = 8;

/// A subgraph: its arguments with their defaults, and the node they are read in.
#[derive(Clone, Debug)]
struct SubgraphDef {
    name: String,
    args: Vec<(String, Json)>,
    node: Json,
}

struct Ctx<'a> {
    defs: &'a [SubgraphDef],
    depth: usize,
}

fn is_name(s: &str) -> bool {
    s.chars().next().is_some_and(|c| c.is_ascii_alphabetic()) && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

fn read_def(name: &str, j: &Json, what: &str) -> Result<SubgraphDef, String> {
    only_keys(j, &["args", "node"], what)?;
    let node = j.get("node").ok_or_else(|| format!("{} needs a node", what))?.clone();
    let mut args = Vec::new();
    if let Some(a) = j.get("args") {
        let Json::Obj(items) = a else {
            return Err(format!("{} args must be an object of names and their defaults", what));
        };
        for (k, v) in items {
            if !is_name(k) {
                return Err(format!(
                    "{} argument '{}' is not a usable name. Use letters, digits and underscores, starting with a letter",
                    what, k
                ));
            }
            if !matches!(v, Json::Num(_) | Json::Str(_) | Json::Bool(_)) {
                return Err(format!("{} argument '{}' needs a default: a number, an expression or a choice name", what, k));
            }
            args.push((k.clone(), v.clone()));
        }
    }
    Ok(SubgraphDef {
        name: name.to_string(),
        args,
        node,
    })
}

/// The subgraphs RUSE ships, read from their JSON.
fn built_ins() -> Vec<SubgraphDef> {
    crate::subgraphs::BUILT_IN
        .iter()
        .map(|b| {
            let j = ruse_core::value::parse(b.json).expect("a built-in subgraph is valid JSON");
            read_def(b.name, &j, b.name).expect("a built-in subgraph is well formed")
        })
        .collect()
}

/// A value as it reads in an expression.
fn as_text(v: &Json) -> String {
    match v {
        Json::Num(n) if n.fract() == 0.0 && n.abs() < 1e15 => format!("{}", *n as i64),
        Json::Num(n) => format!("{}", n),
        Json::Str(s) => s.clone(),
        Json::Bool(b) => (if *b { "1" } else { "0" }).to_string(),
        _ => String::new(),
    }
}

/// A built-in subgraph as the reference pages describe it.
pub struct SubgraphInfo {
    pub name: &'static str,
    pub doc: &'static str,
    /// Each argument's name and default.
    pub args: Vec<(String, String)>,
}

/// Every built-in subgraph with its arguments and their defaults.
pub fn subgraph_reference() -> Vec<SubgraphInfo> {
    let defs = built_ins();
    crate::subgraphs::BUILT_IN
        .iter()
        .zip(defs)
        .map(|(b, d)| SubgraphInfo {
            name: b.name,
            doc: b.doc,
            args: d.args.iter().map(|(n, v)| (n.clone(), as_text(v))).collect(),
        })
        .collect()
}

/// Replaces each argument's name in an expression with its value in parentheses. A function's name, followed by
/// an opening parenthesis, stays as it is, and so does a number such as 1e3.
fn replace_names(s: &str, values: &[(String, Json)]) -> String {
    let chars: Vec<char> = s.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c.is_ascii_digit() || (c == '.' && chars.get(i + 1).is_some_and(|d| d.is_ascii_digit())) {
            while i < chars.len() && (chars[i].is_ascii_digit() || chars[i] == '.') {
                out.push(chars[i]);
                i += 1;
            }
            if i < chars.len() && (chars[i] == 'e' || chars[i] == 'E') {
                let mut j = i + 1;
                if j < chars.len() && (chars[j] == '+' || chars[j] == '-') {
                    j += 1;
                }
                if j < chars.len() && chars[j].is_ascii_digit() {
                    while j < chars.len() && chars[j].is_ascii_digit() {
                        j += 1;
                    }
                    out.extend(&chars[i..j]);
                    i = j;
                }
            }
        } else if c.is_ascii_alphabetic() || c == '_' {
            let start = i;
            while i < chars.len() && (chars[i].is_ascii_alphanumeric() || chars[i] == '_') {
                i += 1;
            }
            let word: String = chars[start..i].iter().collect();
            let mut j = i;
            while j < chars.len() && chars[j] == ' ' {
                j += 1;
            }
            let call = chars.get(j) == Some(&'(');
            match values.iter().find(|(n, _)| *n == word) {
                Some((_, v)) if !call => out.push_str(&format!("({})", as_text(v))),
                _ => out.push_str(&word),
            }
        } else {
            out.push(c);
            i += 1;
        }
    }
    out
}

/// A subgraph's node with its arguments filled in. A string that is just an argument's name becomes the value as
/// given, so a number stays a number and a choice name such as "lowpass" stays a name. Names of blocks and
/// subgraphs are left alone.
fn substitute(j: &Json, values: &[(String, Json)]) -> Json {
    match j {
        Json::Obj(items) => Json::Obj(
            items
                .iter()
                .map(|(k, v)| (k.clone(), if k == "block" || k == "subgraph" { v.clone() } else { substitute(v, values) }))
                .collect(),
        ),
        Json::Arr(a) => Json::Arr(a.iter().map(|v| substitute(v, values)).collect()),
        Json::Str(s) => match values.iter().find(|(n, _)| n == s.trim()) {
            Some((_, v)) => v.clone(),
            None => Json::Str(replace_names(s, values)),
        },
        other => other.clone(),
    }
}

/// Reads a node, with the subgraphs RUSE ships available by name.
pub fn node_from_json(j: &Json, path: &str) -> Result<NodeSpec, String> {
    let defs = built_ins();
    node_in(j, path, &Ctx { defs: &defs, depth: 0 })
}

fn node_in(j: &Json, path: &str, ctx: &Ctx) -> Result<NodeSpec, String> {
    only_keys(
        j,
        &[
            "block", "args", "meters", "chain", "parallel", "multiply", "feedback", "split", "oversample", "subgraph", "bypass",
            "mix", "only", "modulators", "subgraphs", "id",
        ],
        path,
    )?;
    let bypass = j.get("bypass").map(|v| value(v, &format!("{} bypass", path))).transpose()?;
    let mix = j.get("mix").map(|v| value(v, &format!("{} mix", path))).transpose()?;
    let only = match j.get("only") {
        None => None,
        Some(v) => {
            let a = v.as_array().ok_or_else(|| format!("{} only must be a list of channel numbers", path))?;
            let mut out = Vec::new();
            for c in a {
                let n = c.as_f64().filter(|n| *n >= 0.0 && n.fract() == 0.0);
                out.push(n.ok_or_else(|| format!("{} only must hold whole channel numbers from 0", path))? as usize);
            }
            Some(out)
        }
    };
    let kinds = ["block", "chain", "parallel", "multiply", "feedback", "split", "oversample", "subgraph"]
        .iter()
        .filter(|k| j.get(k).is_some())
        .count();
    if kinds != 1 {
        return Err(format!(
            "{} must have exactly one of block, chain, parallel, multiply, feedback, split, oversample or subgraph",
            path
        ));
    }
    if let Some(s) = j.get("subgraph") {
        let name = s.as_str().ok_or_else(|| format!("{} subgraph must be a name", path))?;
        let def = ctx.defs.iter().find(|d| d.name == name).ok_or_else(|| {
            let names: Vec<&str> = ctx.defs.iter().map(|d| d.name.as_str()).collect();
            format!("{}: there is no subgraph called '{}'. The subgraphs are: {}", path, name, names.join(", "))
        })?;
        if ctx.depth >= MAX_DEPTH {
            return Err(format!(
                "{}: subgraphs use one another more than {} deep. Does '{}' use itself?",
                path, MAX_DEPTH, name
            ));
        }
        let mut values = def.args.clone();
        if let Some(a) = j.get("args") {
            let Json::Obj(items) = a else {
                return Err(format!("{} args must be an object", path));
            };
            for (k, v) in items {
                let slot = values.iter_mut().find(|(n, _)| n == k).ok_or_else(|| {
                    let names: Vec<&str> = def.args.iter().map(|(n, _)| n.as_str()).collect();
                    format!("{}: the subgraph '{}' has no argument '{}'. Its arguments are: {}", path, name, k, names.join(", "))
                })?;
                if !matches!(v, Json::Num(_) | Json::Str(_) | Json::Bool(_)) {
                    return Err(format!("{} argument '{}' must be a number or a string", path, k));
                }
                slot.1 = v.clone();
            }
        }
        let body = substitute(&def.node, &values);
        let inner = node_in(&body, &format!("{} (subgraph {})", path, name), &Ctx { defs: ctx.defs, depth: ctx.depth + 1 })?;
        // bypass, mix and only on the use apply to the whole subgraph.
        return Ok(if bypass.is_some() || mix.is_some() || only.is_some() {
            NodeSpec {
                kind: NodeKind::Chain(vec![inner]),
                bypass,
                mix,
                only,
            }
        } else {
            inner
        });
    }
    let kind = if let Some(b) = j.get("block") {
        let name = b.as_str().ok_or_else(|| format!("{} block must be a name", path))?.to_string();
        let mut args = Vec::new();
        if let Some(a) = j.get("args") {
            let Json::Obj(items) = a else {
                return Err(format!("{} args must be an object", path));
            };
            for (k, v) in items {
                args.push((k.clone(), value(v, &format!("{} argument '{}'", path, k))?));
            }
        }
        let mut meters = Vec::new();
        if let Some(m) = j.get("meters") {
            let Json::Obj(items) = m else {
                return Err(format!("{} meters must be an object", path));
            };
            for (k, v) in items {
                let id = v.as_str().ok_or_else(|| format!("{} meter '{}' must name a parameter", path, k))?;
                meters.push((k.clone(), id.to_string()));
            }
        }
        NodeKind::Block { name, args, meters }
    } else if let Some(c) = j.get("chain") {
        let a = c.as_array().ok_or_else(|| format!("{} chain must be a list", path))?;
        let mut nodes = Vec::new();
        for (i, n) in a.iter().enumerate() {
            nodes.push(node_in(n, &format!("{}.chain[{}]", path, i), ctx)?);
        }
        NodeKind::Chain(nodes)
    } else if let Some(m) = j.get("multiply") {
        let a = m.as_array().ok_or_else(|| format!("{} multiply must be a list of nodes", path))?;
        if a.len() < 2 {
            return Err(format!("{} multiply needs at least two nodes to multiply", path));
        }
        let mut nodes = Vec::new();
        for (i, n) in a.iter().enumerate() {
            nodes.push(node_in(n, &format!("{}.multiply[{}]", path, i), ctx)?);
        }
        NodeKind::Multiply(nodes)
    } else if let Some(s) = j.get("split") {
        let here = format!("{}.split", path);
        only_keys(s, &["freqs", "slope", "bands"], &here)?;
        let fs = s
            .get("freqs")
            .and_then(|v| v.as_array())
            .ok_or_else(|| format!("{} needs freqs, a list of one to three crossover frequencies", here))?;
        if fs.is_empty() || fs.len() > 3 {
            return Err(format!("{} freqs has {} frequencies. A split takes one to three", here, fs.len()));
        }
        let mut freqs = Vec::new();
        for (i, f) in fs.iter().enumerate() {
            freqs.push(value(f, &format!("{} freqs[{}]", here, i))?);
        }
        let slope = match s.get("slope") {
            None => 24,
            Some(v) => match v.as_f64() {
                Some(x) if x == 24.0 || x == 48.0 => x as u32,
                _ => return Err(format!("{} slope must be 24 or 48 dB per octave", here)),
            },
        };
        let bs = s
            .get("bands")
            .and_then(|v| v.as_array())
            .ok_or_else(|| format!("{} needs bands, a list of nodes, lowest band first", here))?;
        if bs.len() != freqs.len() + 1 {
            return Err(format!(
                "{} has {} frequencies, so it needs {} bands, and has {}",
                here,
                freqs.len(),
                freqs.len() + 1,
                bs.len()
            ));
        }
        let mut bands = Vec::new();
        for (i, b) in bs.iter().enumerate() {
            bands.push(node_in(b, &format!("{}.bands[{}]", here, i), ctx)?);
        }
        NodeKind::Split(Box::new(SplitSpec { freqs, slope, bands }))
    } else if let Some(o) = j.get("oversample") {
        let here = format!("{}.oversample", path);
        only_keys(o, &["factor", "node"], &here)?;
        let factor = match o.get("factor") {
            None => 2,
            Some(v) => match v.as_f64() {
                Some(x) if x == 2.0 || x == 4.0 || x == 8.0 => x as u32,
                _ => return Err(format!("{} factor must be 2, 4 or 8", here)),
            },
        };
        let node = o.get("node").ok_or_else(|| format!("{} needs a node, the part that runs at the higher rate", here))?;
        NodeKind::Oversample(Box::new(OversampleSpec {
            factor,
            node: node_in(node, &format!("{}.node", here), ctx)?,
        }))
    } else if let Some(f) = j.get("feedback") {
        let here = format!("{}.feedback", path);
        only_keys(f, &["node", "gain", "delay_ms", "max_ms"], &here)?;
        let node = f.get("node").ok_or_else(|| format!("{} needs a node, the part that runs inside the loop", here))?;
        let arg = |key: &str| f.get(key).map(|v| value(v, &format!("{} {}", here, key))).transpose();
        let max_ms = match f.get("max_ms") {
            None => None,
            Some(v) => Some(v.as_f64().ok_or_else(|| format!("{} max_ms is fixed when the plugin is built, so it must be a number", here))?),
        };
        NodeKind::Feedback(Box::new(FeedbackSpec {
            node: node_in(node, &format!("{}.node", here), ctx)?,
            gain: arg("gain")?,
            delay_ms: arg("delay_ms")?,
            max_ms,
        }))
    } else {
        let p = j.get("parallel").unwrap();
        let a = p.as_array().ok_or_else(|| format!("{} parallel must be a list of branches", path))?;
        if a.is_empty() {
            return Err(format!("{} parallel needs at least one branch", path));
        }
        let mut branches = Vec::new();
        for (i, b) in a.iter().enumerate() {
            let here = format!("{}.parallel[{}]", path, i);
            only_keys(b, &["node", "gain_db"], &here)?;
            let node = b.get("node").ok_or_else(|| format!("{} needs a node", here))?;
            let gain_db = b.get("gain_db").map(|v| value(v, &format!("{} gain_db", here))).transpose()?;
            branches.push(BranchSpec {
                node: node_in(node, &format!("{}.node", here), ctx)?,
                gain_db,
            });
        }
        NodeKind::Parallel(branches)
    };
    Ok(NodeSpec { kind, bypass, mix, only })
}

pub fn from_json(j: &Json) -> Result<GraphSpec, String> {
    let mut modulators = Vec::new();
    if let Some(m) = j.get("modulators") {
        let a = m.as_array().ok_or("modulators must be a list")?;
        for (i, item) in a.iter().enumerate() {
            let here = format!("modulators[{}]", i);
            only_keys(item, &["id", "shape", "rate", "attack_ms", "release_ms", "detector"], &here)?;
            let id = item.get("id").and_then(|v| v.as_str()).ok_or_else(|| format!("{} needs an id", here))?;
            let shape = item.get("shape").and_then(|v| v.as_str()).unwrap_or("sine");
            let arg = |key: &str| item.get(key).map(|v| value(v, &format!("{} {}", here, key))).transpose();
            let detector = match item.get("detector") {
                None => None,
                Some(v) => Some(v.as_str().ok_or_else(|| format!("{} detector must be peak or rms", here))?.to_string()),
            };
            modulators.push(ModSpec {
                id: id.to_string(),
                shape: shape.to_string(),
                rate: arg("rate")?,
                attack_ms: arg("attack_ms")?,
                release_ms: arg("release_ms")?,
                detector,
            });
        }
    }
    let mut defs = Vec::new();
    if let Some(s) = j.get("subgraphs") {
        let Json::Obj(items) = s else {
            return Err("subgraphs must be an object of names and subgraphs".to_string());
        };
        for (name, d) in items {
            if !is_name(name) {
                return Err(format!(
                    "'{}' is not a usable subgraph name. Use letters, digits and underscores, starting with a letter",
                    name
                ));
            }
            if crate::subgraphs::BUILT_IN.iter().any(|b| b.name == name) {
                return Err(format!("the subgraph '{}' has the name of one RUSE ships. Pick another name", name));
            }
            defs.push(read_def(name, d, &format!("subgraph '{}'", name))?);
        }
    }
    defs.extend(built_ins());
    let root = node_in(j, "graph", &Ctx { defs: &defs, depth: 0 })?;
    Ok(GraphSpec { modulators, root })
}

pub fn from_str(text: &str) -> Result<GraphSpec, String> {
    let j = ruse_core::value::parse(text).map_err(|e| format!("the graph is not valid JSON: {}", e))?;
    from_json(&j)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_graph_reads_into_its_nodes() {
        let g = from_str(
            r#"{"modulators":[{"id":"lfo1","rate":"2"}],
                "chain":[
                  {"block":"gain","args":{"gain_db":"drive","invert":true},"bypass":"off"},
                  {"parallel":[{"node":{"chain":[]}},{"node":{"block":"delay"},"gain_db":-6}]}
                ]}"#,
        )
        .unwrap();
        assert_eq!(g.modulators.len(), 1);
        assert_eq!(g.modulators[0].shape, "sine");
        let NodeKind::Chain(nodes) = &g.root.kind else {
            panic!("the root is a chain")
        };
        assert_eq!(nodes.len(), 2);
        let NodeKind::Block { name, args, .. } = &nodes[0].kind else {
            panic!("a block")
        };
        assert_eq!(name, "gain");
        assert_eq!(args[1], ("invert".to_string(), ArgValue::Num(1.0)));
        assert_eq!(nodes[0].bypass, Some(ArgValue::Text("off".to_string())));
    }

    #[test]
    fn mistakes_say_where_they_are() {
        let err = |s: &str| from_str(s).unwrap_err();
        assert!(err("not json").contains("not valid JSON"));
        assert!(err(r#"{"block":"gain","chain":[]}"#).contains("exactly one"));
        assert!(err(r#"{"chain":[{"blok":"gain"}]}"#).contains("graph.chain[0] has an unknown key 'blok'"));
        assert!(err(r#"{"chain":[{"block":"gain","args":{"gain_db":[1]}}]}"#).contains("argument 'gain_db'"));
        assert!(err(r#"{"parallel":[]}"#).contains("at least one branch"));
        assert!(err(r#"{"chain":[],"only":[-1]}"#).contains("whole channel numbers"));
    }
}
