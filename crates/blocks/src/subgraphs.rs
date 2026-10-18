//! The subgraphs RUSE ships. Any graph can use them by name, as it uses a block.
//!
//! Each is written in the same form as a subgraph a graph declares for itself: its arguments with their defaults,
//! and the node they are read in.

pub struct BuiltIn {
    pub name: &'static str,
    pub doc: &'static str,
    pub json: &'static str,
}

pub const BUILT_IN: &[BuiltIn] = &[BuiltIn {
    name: "parametric_eq",
    doc: "A four-band equalizer: a low shelf, two bells and a high shelf, then an output gain. It is four filter blocks and a gain block, so a UI can draw its curve from theirs.",
    json: r#"{
  "args": {
    "low_freq": 100, "low_gain_db": 0,
    "mid1_freq": 500, "mid1_gain_db": 0, "mid1_q": 1,
    "mid2_freq": 2500, "mid2_gain_db": 0, "mid2_q": 1,
    "high_freq": 8000, "high_gain_db": 0,
    "output_db": 0
  },
  "node": {"chain": [
    {"block": "filter", "args": {"type": "lowshelf", "freq": "low_freq", "gain_db": "low_gain_db"}},
    {"block": "filter", "args": {"type": "peak", "freq": "mid1_freq", "gain_db": "mid1_gain_db", "q": "mid1_q"}},
    {"block": "filter", "args": {"type": "peak", "freq": "mid2_freq", "gain_db": "mid2_gain_db", "q": "mid2_q"}},
    {"block": "filter", "args": {"type": "highshelf", "freq": "high_freq", "gain_db": "high_gain_db"}},
    {"block": "gain", "args": {"gain_db": "output_db"}}
  ]}
}"#,
}];
