//! The plugin manifest. It is the single declaration of what a plugin is.
//!
//! An agent edits the manifest through the server's tools. The server generates the Rust declaration from it.

use crate::json::Json;

#[derive(Clone, Debug)]
pub struct Param {
    pub id: String,
    pub name: String,
    pub description: String,
    /// knob, choice, toggle or meter
    pub kind: String,
    pub unit: String,
    pub min: f64,
    pub max: f64,
    pub default: f64,
    /// linear or log
    pub curve: String,
    /// global or per_channel
    pub scope: String,
    pub group: String,
    pub choices: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct Manifest {
    pub name: String,
    pub title: String,
    pub description: String,
    pub min_channels: u32,
    pub max_channels: u32,
    pub params: Vec<Param>,
}

const RESERVED: [&str; 55] = [
    "as", "async", "await", "break", "const", "continue", "crate", "dyn", "else", "enum", "extern", "false", "fn", "for",
    "if", "impl", "in", "let", "loop", "match", "mod", "move", "mut", "pub", "ref", "return", "self", "Self", "static",
    "struct", "super", "trait", "true", "type", "unsafe", "use", "where", "while", "abstract", "become", "box", "do",
    "final", "macro", "override", "priv", "try", "typeof", "unsized", "virtual", "yield", "gen", "new", "set", "get",
];

pub fn valid_slug(s: &str) -> bool {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) if c.is_ascii_lowercase() => {}
        _ => return false,
    }
    s.len() <= 40 && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-') && !s.ends_with('-')
}

fn valid_id(s: &str) -> bool {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) if c.is_ascii_lowercase() => {}
        _ => return false,
    }
    s.len() <= 40 && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

/// `warm-saturator` becomes `WarmSaturator`.
pub fn camel(slug: &str) -> String {
    slug.split('-')
        .filter(|p| !p.is_empty())
        .map(|p| {
            let mut c = p.chars();
            match c.next() {
                Some(f) => f.to_ascii_uppercase().to_string() + c.as_str(),
                None => String::new(),
            }
        })
        .collect()
}

/// `warm-saturator` becomes `warm_saturator`.
pub fn snake(slug: &str) -> String {
    slug.replace('-', "_")
}

fn title_of(slug: &str) -> String {
    slug.split('-')
        .filter(|p| !p.is_empty())
        .map(|p| {
            let mut c = p.chars();
            match c.next() {
                Some(f) => f.to_ascii_uppercase().to_string() + c.as_str(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn text(j: &Json, key: &str) -> Option<String> {
    j.get(key).and_then(|v| v.as_str()).map(|s| s.trim().to_string())
}

fn number(j: &Json, key: &str, who: &str) -> Result<Option<f64>, String> {
    match j.get(key) {
        None | Some(Json::Null) => Ok(None),
        Some(Json::Num(n)) if n.is_finite() => Ok(Some(*n)),
        Some(_) => Err(format!("{}: {} must be a finite number", who, key)),
    }
}

fn parse_param(j: &Json) -> Result<Param, String> {
    let id = text(j, "id").ok_or("every parameter needs an id")?;
    let who = format!("parameter '{}'", id);
    if !valid_id(&id) {
        return Err(format!(
            "{}: an id starts with a lowercase letter and holds lowercase letters, digits and underscores, up to 40 characters",
            who
        ));
    }
    if RESERVED.contains(&id.as_str()) {
        return Err(format!("{}: '{}' cannot be used as an id because it is a Rust keyword or a name MUSE reserves", who, id));
    }
    let name = text(j, "name").filter(|s| !s.is_empty()).ok_or(format!("{}: name is required", who))?;
    let description = text(j, "description").filter(|s| !s.is_empty()).ok_or(format!(
        "{}: description is required, because MCP clients and people read it to learn what the parameter does",
        who
    ))?;
    let kind = text(j, "kind").unwrap_or_else(|| "knob".to_string());
    let unit = text(j, "unit").unwrap_or_default();
    let curve = text(j, "curve").unwrap_or_else(|| "linear".to_string());
    let scope = text(j, "scope").unwrap_or_else(|| "global".to_string());
    let group = text(j, "group").unwrap_or_default();
    if curve != "linear" && curve != "log" {
        return Err(format!("{}: curve must be linear or log", who));
    }
    if scope != "global" && scope != "per_channel" {
        return Err(format!("{}: scope must be global or per_channel", who));
    }
    let mut p = Param {
        id,
        name,
        description,
        kind: kind.clone(),
        unit,
        min: 0.0,
        max: 1.0,
        default: 0.0,
        curve,
        scope,
        group,
        choices: Vec::new(),
    };
    match kind.as_str() {
        "knob" | "meter" => {
            p.min = number(j, "min", &who)?.ok_or(format!("{}: min is required", who))?;
            p.max = number(j, "max", &who)?.ok_or(format!("{}: max is required", who))?;
            if p.min >= p.max {
                return Err(format!("{}: min must be below max", who));
            }
            if kind == "knob" {
                p.default = number(j, "default", &who)?.ok_or(format!("{}: default is required for a knob", who))?;
                if p.default < p.min || p.default > p.max {
                    return Err(format!("{}: default {} is outside {} to {}", who, p.default, p.min, p.max));
                }
            } else {
                p.default = p.min;
                if p.scope != "global" {
                    return Err(format!("{}: a meter has one value, so its scope must be global", who));
                }
            }
            if p.curve == "log" && p.min <= 0.0 {
                return Err(format!("{}: a log curve needs min above zero", who));
            }
        }
        "choice" => {
            let list = j
                .get("choices")
                .and_then(|v| v.as_array())
                .ok_or(format!("{}: choices is required, as a list of names", who))?;
            let names: Vec<String> = list.iter().filter_map(|v| v.as_str().map(|s| s.to_string())).collect();
            if names.len() != list.len() || names.len() < 2 {
                return Err(format!("{}: choices needs two or more text names", who));
            }
            p.min = 0.0;
            p.max = (names.len() - 1) as f64;
            p.default = match j.get("default") {
                None | Some(Json::Null) => 0.0,
                Some(Json::Num(n)) => *n,
                Some(Json::Str(s)) => names
                    .iter()
                    .position(|c| c == s)
                    .ok_or(format!("{}: default '{}' is not one of the choices", who, s))? as f64,
                Some(_) => return Err(format!("{}: default must be a choice name or its index", who)),
            };
            if p.default < 0.0 || p.default > p.max || p.default.fract() != 0.0 {
                return Err(format!("{}: default must be an index from 0 to {}", who, names.len() - 1));
            }
            p.choices = names;
        }
        "toggle" => {
            p.min = 0.0;
            p.max = 1.0;
            p.default = match j.get("default") {
                None | Some(Json::Null) => 0.0,
                Some(Json::Bool(b)) => {
                    if *b {
                        1.0
                    } else {
                        0.0
                    }
                }
                Some(Json::Num(n)) if *n == 0.0 || *n == 1.0 => *n,
                Some(_) => return Err(format!("{}: default for a toggle must be true, false, 0 or 1", who)),
            };
        }
        other => return Err(format!("{}: kind '{}' is not one of knob, choice, toggle or meter", who, other)),
    }
    Ok(p)
}

pub fn parse(j: &Json) -> Result<Manifest, String> {
    let name = text(j, "name").ok_or("name is required, such as \"warm-saturator\"")?;
    if !valid_slug(&name) {
        return Err(format!(
            "'{}' is not a valid name. Use lowercase letters, digits and hyphens, starting with a letter",
            name
        ));
    }
    let description = text(j, "description")
        .filter(|s| !s.is_empty())
        .ok_or("description is required: one or two sentences on what the plugin does")?;
    let title = text(j, "title").filter(|s| !s.is_empty()).unwrap_or_else(|| title_of(&name));
    let min_channels = number(j, "channels_min", "channels_min")?.unwrap_or(1.0);
    let max_channels = number(j, "channels_max", "channels_max")?.unwrap_or(64.0);
    if min_channels < 1.0 || max_channels > 64.0 || min_channels > max_channels || min_channels.fract() != 0.0 || max_channels.fract() != 0.0 {
        return Err("channels_min and channels_max must be whole numbers with 1 <= min <= max <= 64".to_string());
    }
    let list = j
        .get("parameters")
        .and_then(|v| v.as_array())
        .ok_or("parameters is required, as a list. It can be empty")?;
    let mut params: Vec<Param> = Vec::new();
    for item in list {
        let p = parse_param(item)?;
        if params.iter().any(|q| q.id == p.id) {
            return Err(format!("parameter id '{}' appears twice", p.id));
        }
        params.push(p);
    }
    Ok(Manifest {
        name,
        title,
        description,
        min_channels: min_channels as u32,
        max_channels: max_channels as u32,
        params,
    })
}

impl Manifest {
    pub fn to_json(&self) -> Json {
        let params: Vec<Json> = self
            .params
            .iter()
            .map(|p| {
                let mut items: Vec<(&str, Json)> = vec![
                    ("id", Json::str(&p.id)),
                    ("name", Json::str(&p.name)),
                    ("description", Json::str(&p.description)),
                    ("kind", Json::str(&p.kind)),
                ];
                if !p.unit.is_empty() {
                    items.push(("unit", Json::str(&p.unit)));
                }
                match p.kind.as_str() {
                    "knob" => {
                        items.push(("min", Json::Num(p.min)));
                        items.push(("max", Json::Num(p.max)));
                        items.push(("default", Json::Num(p.default)));
                        items.push(("curve", Json::str(&p.curve)));
                    }
                    "meter" => {
                        items.push(("min", Json::Num(p.min)));
                        items.push(("max", Json::Num(p.max)));
                        items.push(("curve", Json::str(&p.curve)));
                    }
                    "choice" => {
                        items.push(("choices", Json::Arr(p.choices.iter().map(Json::str).collect())));
                        items.push(("default", Json::Num(p.default)));
                    }
                    _ => items.push(("default", Json::Bool(p.default >= 0.5))),
                }
                items.push(("scope", Json::str(&p.scope)));
                if !p.group.is_empty() {
                    items.push(("group", Json::str(&p.group)));
                }
                Json::obj(items)
            })
            .collect();
        Json::obj(vec![
            ("name", Json::str(&self.name)),
            ("title", Json::str(&self.title)),
            ("description", Json::str(&self.description)),
            ("channels_min", Json::Num(self.min_channels as f64)),
            ("channels_max", Json::Num(self.max_channels as f64)),
            ("parameters", Json::Arr(params)),
        ])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::json;

    fn manifest(params: &str) -> Result<Manifest, String> {
        parse(&json::parse(&format!(
            r#"{{"name":"warm-saturator","description":"Adds warmth.","parameters":{}}}"#,
            params
        ))
        .unwrap())
    }

    const DRIVE: &str = r#"{"id":"drive","name":"Drive","description":"How hard the signal is pushed.","unit":"dB","min":0,"max":24,"default":6}"#;

    #[test]
    fn a_complete_manifest_is_read_with_defaults() {
        let m = manifest(&format!("[{}]", DRIVE)).unwrap();
        assert_eq!(m.title, "Warm Saturator");
        assert_eq!((m.min_channels, m.max_channels), (1, 64));
        assert_eq!(m.params[0].kind, "knob");
        assert_eq!(m.params[0].scope, "global");
        assert_eq!(camel(&m.name), "WarmSaturator");
        assert_eq!(snake(&m.name), "warm_saturator");
    }

    #[test]
    fn the_written_manifest_reads_back_the_same() {
        let m = manifest(&format!(
            r#"[{},{{"id":"mode","name":"Mode","description":"Which curve.","kind":"choice","choices":["Soft","Hard"],"default":"Hard"}},{{"id":"bypass","name":"Bypass","description":"Passes audio through.","kind":"toggle","default":true,"scope":"per_channel"}}]"#,
            DRIVE
        ))
        .unwrap();
        let again = parse(&json::parse(&m.to_json().pretty()).unwrap()).unwrap();
        assert_eq!(again.params.len(), 3);
        assert_eq!(again.params[1].default, 1.0);
        assert_eq!(again.params[2].default, 1.0);
        assert_eq!(again.params[2].scope, "per_channel");
    }

    #[test]
    fn each_mistake_is_refused_with_the_parameter_named() {
        let cases = [
            (r#"[{"id":"x","name":"X","min":0,"max":1,"default":0}]"#, "description is required"),
            (r#"[{"id":"x","name":"X","description":"d","min":1,"max":0,"default":0}]"#, "min must be below max"),
            (r#"[{"id":"x","name":"X","description":"d","min":0,"max":1,"default":2}]"#, "outside"),
            (r#"[{"id":"x","name":"X","description":"d","min":0,"max":1,"default":0,"curve":"log"}]"#, "log curve"),
            (r#"[{"id":"fn","name":"X","description":"d","min":0,"max":1,"default":0}]"#, "keyword"),
            (r#"[{"id":"Bad","name":"X","description":"d","min":0,"max":1,"default":0}]"#, "lowercase"),
            (r#"[{"id":"x","name":"X","description":"d","kind":"choice","choices":["only"]}]"#, "two or more"),
            (r#"[{"id":"x","name":"X","description":"d","kind":"dial"}]"#, "not one of"),
            (r#"[{"id":"x","name":"X","description":"d","kind":"meter","min":0,"max":1,"scope":"per_channel"}]"#, "meter"),
        ];
        for (params, expect) in cases {
            let err = manifest(params).err().unwrap_or_else(|| panic!("accepted {}", params));
            assert!(err.contains(expect), "{} -> {}", params, err);
            assert!(err.contains("parameter 'x'") || err.contains("parameter 'fn'") || err.contains("parameter 'Bad'"), "{}", err);
        }
        assert!(manifest(&format!("[{},{}]", DRIVE, DRIVE)).err().unwrap().contains("twice"));
    }

    #[test]
    fn names_are_checked() {
        assert!(valid_slug("warm-saturator"));
        assert!(!valid_slug("Warm"));
        assert!(!valid_slug("2fast"));
        assert!(!valid_slug("ends-"));
        assert!(!valid_slug("a_b"));
    }
}
