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

pub fn node_from_json(j: &Json, path: &str) -> Result<NodeSpec, String> {
    only_keys(
        j,
        &["block", "args", "meters", "chain", "parallel", "multiply", "feedback", "bypass", "mix", "only", "modulators", "id"],
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
    let kinds = ["block", "chain", "parallel", "multiply", "feedback"].iter().filter(|k| j.get(k).is_some()).count();
    if kinds != 1 {
        return Err(format!("{} must have exactly one of block, chain, parallel, multiply or feedback", path));
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
            nodes.push(node_from_json(n, &format!("{}.chain[{}]", path, i))?);
        }
        NodeKind::Chain(nodes)
    } else if let Some(m) = j.get("multiply") {
        let a = m.as_array().ok_or_else(|| format!("{} multiply must be a list of nodes", path))?;
        if a.len() < 2 {
            return Err(format!("{} multiply needs at least two nodes to multiply", path));
        }
        let mut nodes = Vec::new();
        for (i, n) in a.iter().enumerate() {
            nodes.push(node_from_json(n, &format!("{}.multiply[{}]", path, i))?);
        }
        NodeKind::Multiply(nodes)
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
            node: node_from_json(node, &format!("{}.node", here))?,
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
                node: node_from_json(node, &format!("{}.node", here))?,
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
    let root = node_from_json(j, "graph")?;
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
