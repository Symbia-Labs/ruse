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

/// What a host shows and uses to recognise the plugin when it is packaged.
#[derive(Clone, Debug, Default)]
pub struct Package {
    pub vendor: String,
    pub url: String,
    pub version: String,
    /// The plugin's identity, in reverse-domain style such as `com.example.warm-saturator`.
    pub id: String,
    pub features: Vec<String>,
    /// Which plugin formats to build: `clap`, `au` or both. Defaults to `clap`.
    pub formats: Vec<String>,
    /// The four-character codes an Audio Unit is known by. Only used when `au` is a format.
    pub au_subtype: String,
    pub au_manufacturer: String,
}

#[derive(Clone, Debug)]
pub struct Manifest {
    pub name: String,
    pub title: String,
    pub description: String,
    pub min_channels: u32,
    pub max_channels: u32,
    pub params: Vec<Param>,
    pub package: Option<Package>,
    /// The signal path as a graph of blocks, when the plugin is assembled from blocks instead of written as code.
    /// It is kept as the JSON the agent gave, and checked against the parameters.
    pub graph: Option<Json>,
}

/// Compiles the graph against the declared parameters, so a mistake is reported when the graph is set and not
/// when the plugin is built.
fn check_graph(g: &Json, params: &[Param]) -> Result<(), String> {
    let spec = muse_blocks::spec::from_json(g).map_err(|e| format!("graph: {}", e))?;
    let ids: Vec<&str> = params.iter().map(|p| p.id.as_str()).collect();
    muse_blocks::Graph::from_spec(&spec, &ids).map_err(|e| format!("graph: {}", e))?;
    for id in spec.meter_params() {
        if let Some(p) = params.iter().find(|p| p.id == id) {
            if p.kind != "meter" {
                return Err(format!(
                    "graph: '{}' shows a block's meter, so it must be declared with kind \"meter\", not \"{}\"",
                    id, p.kind
                ));
            }
        }
    }
    Ok(())
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
    s.len() <= 40
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        && !s.ends_with('-')
        // The packaging crate for a plugin is named with this suffix.
        && !s.ends_with("-clap")
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

fn words_to_slug(s: &str) -> String {
    s.split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(|w| w.to_ascii_lowercase())
        .collect::<Vec<_>>()
        .join("-")
}

impl Package {
    pub fn to_json(&self) -> Json {
        let mut items: Vec<(&str, Json)> = vec![
            ("vendor", Json::str(&self.vendor)),
            ("url", Json::str(&self.url)),
            ("version", Json::str(&self.version)),
            ("id", Json::str(&self.id)),
            ("features", Json::Arr(self.features.iter().map(Json::str).collect())),
            ("formats", Json::Arr(self.formats.iter().map(Json::str).collect())),
        ];
        if !self.au_subtype.is_empty() || !self.au_manufacturer.is_empty() {
            items.push((
                "au",
                Json::obj(vec![
                    ("subtype", Json::str(&self.au_subtype)),
                    ("manufacturer", Json::str(&self.au_manufacturer)),
                ]),
            ));
        }
        Json::obj(items)
    }

    /// Reads whichever of the fields are present. Call `complete` before using the result.
    pub fn from_json(j: &Json) -> Package {
        Package {
            vendor: text(j, "vendor").unwrap_or_default(),
            url: text(j, "url").unwrap_or_default(),
            version: text(j, "version").unwrap_or_default(),
            id: text(j, "id").unwrap_or_default(),
            features: j
                .get("features")
                .and_then(|v| v.as_array())
                .map(|a| a.iter().filter_map(|f| f.as_str().map(|s| s.trim().to_string())).collect())
                .unwrap_or_default(),
            formats: j
                .get("formats")
                .and_then(|v| v.as_array())
                .map(|a| a.iter().filter_map(|f| f.as_str().map(|s| s.trim().to_ascii_lowercase())).collect())
                .unwrap_or_default(),
            au_subtype: j.get("au").and_then(|a| text(a, "subtype")).unwrap_or_default(),
            au_manufacturer: j.get("au").and_then(|a| text(a, "manufacturer")).unwrap_or_default(),
        }
    }

    /// Fills the defaults for a plugin called `name` and checks every field.
    pub fn complete(&mut self, name: &str) -> Result<(), String> {
        self.vendor = self.vendor.trim().to_string();
        if self.vendor.is_empty() {
            return Err("package: vendor is required. It is the company or person name a host shows beside the plugin".to_string());
        }
        if self.vendor.len() > 64 || self.vendor.chars().any(|c| c.is_control()) {
            return Err("package: vendor must be 64 characters or fewer, on one line".to_string());
        }
        if self.id.trim().is_empty() {
            let slug = words_to_slug(&self.vendor);
            if slug.is_empty() {
                return Err("package: the vendor needs letters or digits so an id can be made from it. Give an id as well".to_string());
            }
            self.id = format!("{}.{}", slug, name);
        }
        self.id = self.id.trim().to_string();
        let id_ok = self.id.len() <= 128
            && self.id.contains('.')
            && self.id.chars().next().is_some_and(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
            && self.id.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '.' || c == '-');
        if !id_ok {
            return Err(format!(
                "package: id '{}' must be lowercase letters, digits, dots and hyphens with at least one dot, such as com.example.{}",
                self.id, name
            ));
        }
        if self.version.trim().is_empty() {
            self.version = "0.0.1".to_string();
        }
        self.version = self.version.trim().to_string();
        if self.version.len() > 32 || !self.version.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '+')) {
            return Err("package: version may hold letters, digits, dots, hyphens and plus signs, up to 32 characters".to_string());
        }
        self.url = self.url.trim().to_string();
        if !self.url.is_empty() && !(self.url.starts_with("https://") || self.url.starts_with("http://")) {
            return Err("package: url must start with https:// or http://, or be left empty".to_string());
        }
        if self.features.is_empty() {
            self.features.push("audio-effect".to_string());
        }
        for f in &self.features {
            if f.is_empty() || !f.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-') {
                return Err(format!("package: feature '{}' must be lowercase letters, digits and hyphens, such as audio-effect", f));
            }
        }
        self.formats = self.formats.iter().map(|f| f.trim().to_ascii_lowercase()).collect();
        if self.formats.is_empty() {
            self.formats.push("clap".to_string());
        }
        let mut seen: Vec<String> = Vec::new();
        for f in &self.formats {
            if f != "clap" && f != "au" {
                return Err(format!("package: format '{}' is not one of clap or au", f));
            }
            if !seen.contains(f) {
                seen.push(f.clone());
            }
        }
        self.formats = seen;
        if self.formats.iter().any(|f| f == "au") {
            if self.au_manufacturer.trim().is_empty() {
                self.au_manufacturer = default_au_manufacturer(&self.vendor);
            }
            if self.au_subtype.trim().is_empty() {
                self.au_subtype = default_au_subtype(name);
            }
            self.au_manufacturer = self.au_manufacturer.trim().to_string();
            self.au_subtype = self.au_subtype.trim().to_string();
            let code_ok = |c: &str| c.len() == 4 && c.chars().all(|ch| ch.is_ascii_alphanumeric());
            if !code_ok(&self.au_subtype) {
                return Err(format!("package: the Audio Unit subtype '{}' must be four letters or digits", self.au_subtype));
            }
            if !code_ok(&self.au_manufacturer) || !self.au_manufacturer.chars().any(|c| c.is_ascii_uppercase()) {
                return Err(format!(
                    "package: the Audio Unit manufacturer code '{}' must be four letters or digits with at least one capital, because Apple reserves all-lowercase codes",
                    self.au_manufacturer
                ));
            }
        }
        Ok(())
    }
}

/// Four characters from the vendor name, capitalised so Apple's rule for manufacturer codes holds.
fn default_au_manufacturer(vendor: &str) -> String {
    let letters: String = vendor.chars().filter(|c| c.is_ascii_alphanumeric()).collect();
    let mut code: String = letters.chars().take(4).collect();
    while code.len() < 4 {
        code.push('x');
    }
    let mut chars: Vec<char> = code.chars().collect();
    chars[0] = chars[0].to_ascii_uppercase();
    let code: String = chars.into_iter().collect();
    if code.chars().any(|c| c.is_ascii_uppercase()) {
        code
    } else {
        "Muse".to_string()
    }
}

/// The first letter of the first word and the first three of the second, or four letters of a single word.
fn default_au_subtype(name: &str) -> String {
    let words: Vec<&str> = name.split('-').filter(|w| !w.is_empty()).collect();
    let mut code = String::new();
    if words.len() >= 2 {
        code.extend(words[0].chars().take(1));
        code.extend(words[1].chars().take(3));
    } else if let Some(w) = words.first() {
        code.extend(w.chars().take(4));
    }
    code = code.chars().filter(|c| c.is_ascii_alphanumeric()).collect();
    while code.len() < 4 {
        code.push('x');
    }
    let mut chars: Vec<char> = code.chars().collect();
    chars[0] = chars[0].to_ascii_uppercase();
    chars.into_iter().collect()
}

/// The version of the manifest format this MUSE writes. A manifest without one is format 1.
/// Raise it when a change means an older MUSE would read a manifest wrongly, and say what changed
/// in CHANGELOG.md.
pub const FORMAT: u32 = 1;

pub fn parse(j: &Json) -> Result<Manifest, String> {
    let format = number(j, "format", "format")?.unwrap_or(1.0);
    if format.fract() != 0.0 || format < 1.0 {
        return Err("format must be a whole number from 1".to_string());
    }
    if format as u32 > FORMAT {
        return Err(format!(
            "this manifest is format {}, and this MUSE reads format {} and earlier. Update MUSE to work on it",
            format, FORMAT
        ));
    }
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
    let package = match j.get("package") {
        Some(p @ Json::Obj(_)) => {
            let mut pkg = Package::from_json(p);
            pkg.complete(&name)?;
            Some(pkg)
        }
        _ => None,
    };
    let graph = match j.get("graph") {
        None | Some(Json::Null) => None,
        Some(g) => {
            check_graph(g, &params)?;
            Some(g.clone())
        }
    };
    Ok(Manifest {
        name,
        title,
        description,
        min_channels: min_channels as u32,
        max_channels: max_channels as u32,
        params,
        package,
        graph,
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
        let mut items: Vec<(&str, Json)> = vec![
            ("format", Json::Num(FORMAT as f64)),
            ("name", Json::str(&self.name)),
            ("title", Json::str(&self.title)),
            ("description", Json::str(&self.description)),
            ("channels_min", Json::Num(self.min_channels as f64)),
            ("channels_max", Json::Num(self.max_channels as f64)),
            ("parameters", Json::Arr(params)),
        ];
        if let Some(p) = &self.package {
            items.push(("package", p.to_json()));
        }
        if let Some(g) = &self.graph {
            items.push(("graph", g.clone()));
        }
        Json::obj(items)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::json;

    fn with_graph(graph: &str) -> Result<Manifest, String> {
        parse(&json::parse(&format!(
            r#"{{"name":"warm-saturator","description":"Adds warmth.","parameters":[
                {{"id":"drive","name":"Drive","description":"How hard.","unit":"dB","min":0,"max":24,"default":6}},
                {{"id":"gr","name":"Reduction","description":"Shown.","kind":"meter","unit":"dB","min":0,"max":30}}],
                "graph":{}}}"#,
            graph
        ))
        .unwrap())
    }

    #[test]
    fn a_graph_is_kept_and_written_back() {
        let m = with_graph(r#"{"block":"gain","args":{"gain_db":"drive"}}"#).unwrap();
        assert!(m.graph.is_some());
        assert!(m.to_json().get("graph").is_some());
        let again = parse(&m.to_json()).unwrap();
        assert_eq!(again.graph, m.graph, "the manifest round-trips");
        assert!(with_graph("null").unwrap().graph.is_none(), "null takes the graph away");
    }

    #[test]
    fn a_graph_that_does_not_fit_the_parameters_is_refused_with_the_reason() {
        let e = with_graph(r#"{"block":"gain","args":{"gain_db":"tone"}}"#).unwrap_err();
        assert!(e.starts_with("graph: ") && e.contains("'tone' is not a parameter") && e.contains("drive"), "{}", e);
        let e = with_graph(r#"{"block":"nothing"}"#).unwrap_err();
        assert!(e.contains("no block called 'nothing'"), "{}", e);
        let e = with_graph(r#"{"block":"dynamics","meters":{"gain_reduction_db":"drive"}}"#).unwrap_err();
        assert!(e.contains("must be declared with kind \"meter\""), "{}", e);
        assert!(with_graph(r#"{"block":"dynamics","meters":{"gain_reduction_db":"gr"}}"#).is_ok());
    }

    fn manifest(params: &str) -> Result<Manifest, String> {
        parse(&json::parse(&format!(
            r#"{{"name":"warm-saturator","description":"Adds warmth.","parameters":{}}}"#,
            params
        ))
        .unwrap())
    }

    fn with_format(format: &str) -> Result<Manifest, String> {
        parse(&json::parse(&format!(r#"{{"format":{},"name":"x","description":"d","parameters":[]}}"#, format)).unwrap())
    }

    #[test]
    fn the_format_is_written_and_a_newer_one_is_refused() {
        let m = manifest("[]").unwrap();
        assert_eq!(m.to_json().get("format").and_then(|f| f.as_f64()), Some(FORMAT as f64));
        assert!(with_format("1").is_ok(), "format 1 reads");
        let e = with_format(&(FORMAT + 1).to_string()).unwrap_err();
        assert!(e.contains("Update MUSE"), "{}", e);
        assert!(with_format("0").is_err() && with_format("1.5").is_err());
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
    fn package_settings_get_defaults_and_are_checked() {
        let mut p = Package {
            vendor: "Symbia Labs".to_string(),
            ..Package::default()
        };
        p.complete("warm-saturator").unwrap();
        assert_eq!(p.id, "symbia-labs.warm-saturator");
        assert_eq!(p.version, "0.0.1");
        assert_eq!(p.features, ["audio-effect"]);
        assert_eq!(p.formats, ["clap"]);
        assert!(p.au_subtype.is_empty(), "no Audio Unit codes until au is asked for");

        let mut both = Package {
            vendor: "Symbia Labs".to_string(),
            formats: vec!["AU".to_string(), "clap".to_string(), "au".to_string()],
            ..Package::default()
        };
        both.complete("warm-saturator").unwrap();
        assert_eq!(both.formats, ["au", "clap"], "lowercased and without repeats");
        assert_eq!(both.au_subtype, "Wsat");
        assert_eq!(both.au_manufacturer, "Symb");
        let mut one_word = Package {
            vendor: "42".to_string(),
            formats: vec!["au".to_string()],
            ..Package::default()
        };
        one_word.complete("gain").unwrap();
        assert_eq!(one_word.au_subtype, "Gain");
        assert_eq!(one_word.au_manufacturer, "Muse", "a vendor with no capital letter still gets a valid code");

        let bad = |mutate: &dyn Fn(&mut Package)| {
            let mut p = Package {
                vendor: "V".to_string(),
                ..Package::default()
            };
            mutate(&mut p);
            p.complete("x").err().unwrap()
        };
        assert!(bad(&|p| p.vendor.clear()).contains("vendor is required"));
        assert!(bad(&|p| p.id = "NoDots".to_string()).contains("id"));
        assert!(bad(&|p| p.id = "Has.Caps".to_string()).contains("lowercase"));
        assert!(bad(&|p| p.url = "ftp://x".to_string()).contains("url"));
        assert!(bad(&|p| p.version = "1 2".to_string()).contains("version"));
        assert!(bad(&|p| p.features = vec!["Audio Effect".to_string()]).contains("feature"));
        assert!(bad(&|p| p.formats = vec!["vst3".to_string()]).contains("format"));
        assert!(bad(&|p| {
            p.formats = vec!["au".to_string()];
            p.au_manufacturer = "abcd".to_string();
        })
        .contains("capital"));
        assert!(bad(&|p| {
            p.formats = vec!["au".to_string()];
            p.au_subtype = "toolong".to_string();
        })
        .contains("four"));
    }

    #[test]
    fn a_manifest_keeps_its_package_settings() {
        let m = parse(
            &json::parse(
                r#"{"name":"warm-saturator","description":"Adds warmth.","parameters":[],
                "package":{"vendor":"Symbia Labs","url":"https://example.com","version":"1.2.3","features":["audio-effect","distortion"]}}"#,
            )
            .unwrap(),
        )
        .unwrap();
        let p = m.package.as_ref().unwrap();
        assert_eq!(p.id, "symbia-labs.warm-saturator");
        let again = parse(&json::parse(&m.to_json().pretty()).unwrap()).unwrap();
        assert_eq!(again.package.unwrap().features, ["audio-effect", "distortion"]);
        let without = manifest("[]").unwrap();
        assert!(without.package.is_none());
        assert!(!without.to_json().compact().contains("package"));
    }

    #[test]
    fn names_are_checked() {
        assert!(!valid_slug("warm-clap"));
        assert!(valid_slug("warm-saturator"));
        assert!(!valid_slug("Warm"));
        assert!(!valid_slug("2fast"));
        assert!(!valid_slug("ends-"));
        assert!(!valid_slug("a_b"));
    }
}
