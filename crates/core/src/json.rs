use crate::param::{ParamSet, Scope};

fn string(out: &mut String, s: &str) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
}

/// The description an MCP client reads to learn what a plugin can do.
/// A per-channel parameter lists a value and its text for each channel the plugin is running with.
pub fn describe_params(set: &ParamSet) -> String {
    let mut o = format!("{{\"channels\":{},\"params\":[", set.channels());
    for (i, p) in set.iter().enumerate() {
        if i > 0 {
            o.push(',');
        }
        let m = p.info();
        o.push_str("{\"id\":");
        string(&mut o, m.id);
        o.push_str(",\"name\":");
        string(&mut o, m.name);
        o.push_str(",\"description\":");
        string(&mut o, m.description);
        o.push_str(",\"group\":");
        string(&mut o, m.group);
        o.push_str(",\"kind\":");
        string(&mut o, m.kind.name());
        o.push_str(",\"scope\":");
        string(&mut o, m.scope.name());
        o.push_str(",\"unit\":");
        string(&mut o, m.unit);
        o.push_str(&format!(
            ",\"min\":{},\"max\":{},\"default\":{},\"curve\":\"{}\",\"value\":{}",
            m.min,
            m.max,
            m.default,
            m.curve.name(),
            p.get()
        ));
        o.push_str(",\"text\":");
        string(&mut o, &p.text());
        if m.scope == Scope::PerChannel {
            let n = set.channels();
            o.push_str(",\"values\":[");
            for c in 0..n {
                if c > 0 {
                    o.push(',');
                }
                o.push_str(&format!("{}", p.get_ch(c)));
            }
            o.push_str("],\"texts\":[");
            for c in 0..n {
                if c > 0 {
                    o.push(',');
                }
                string(&mut o, &p.text_ch(c));
            }
            o.push(']');
        }
        o.push_str(",\"choices\":[");
        for (j, c) in m.choices.iter().enumerate() {
            if j > 0 {
                o.push(',');
            }
            string(&mut o, c);
        }
        o.push_str("]}");
    }
    o.push_str("]}");
    o
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::param::ParamInfo;

    #[test]
    fn strings_are_escaped() {
        let info = ParamInfo {
            name: "Say \"hi\"",
            description: "line one\nline two",
            ..ParamInfo::knob("q", "", "", 0.0, 1.0, 0.5)
        };
        let json = describe_params(&ParamSet::new(&[info]));
        assert!(json.contains("Say \\\"hi\\\""));
        assert!(json.contains("line one\\nline two"));
    }

    #[test]
    fn choices_groups_and_text_are_described() {
        let speed = ParamInfo {
            group: "Machine",
            ..ParamInfo::choice("speed", "Speed", &["3.75 IPS", "7.5 IPS"], 1)
        };
        let json = describe_params(&ParamSet::new(&[speed]));
        assert!(json.contains("\"kind\":\"choice\""));
        assert!(json.contains("\"group\":\"Machine\""));
        assert!(json.contains("\"choices\":[\"3.75 IPS\",\"7.5 IPS\"]"));
        assert!(json.contains("\"text\":\"7.5 IPS\""));
        assert!(json.contains("\"scope\":\"global\""));
        assert!(!json.contains("\"values\""));
    }

    #[test]
    fn per_channel_parameters_list_a_value_for_each_channel() {
        let trim = ParamInfo::knob("trim", "Trim", "dB", -12.0, 12.0, 0.0).per_channel();
        let set = ParamSet::with_channels(&[trim], 8);
        set.set_channels(3);
        set.set_plain_ch("trim", Some(1), 6.0).unwrap();
        let json = describe_params(&set);
        assert!(json.contains("\"channels\":3"));
        assert!(json.contains("\"scope\":\"per_channel\""));
        assert!(json.contains("\"values\":[0,6,0]"), "{}", json);
        assert!(json.contains("\"texts\":[\"0.00 dB\",\"6.00 dB\",\"0.00 dB\"]"), "{}", json);
    }
}
