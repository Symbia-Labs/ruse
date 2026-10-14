//! DSP building blocks, and a runtime that runs a plugin described as a graph of them.
//!
//! The graph lives in a plugin's manifest as data. The runtime reads it, evaluates each block's arguments from
//! the plugin's parameters, and runs the audio through it, allocation-free, with the same output at every host
//! block size. See [`spec`] for the format and [`blocks::catalogue`] for what is available.
#![forbid(unsafe_code)]

pub mod block;
pub mod blocks;
pub mod expr;
pub mod frame;
pub mod graph;
pub mod spec;

pub use block::{ArgInfo, Block, BlockInfo};
pub use graph::Graph;

/// How to write a graph, for an agent to read before it writes one.
pub const FORMAT_GUIDE: &str = r#"# Describing a plugin as a graph

A graph is a JSON object. It is a node, and it may also hold a list of modulators.

## Nodes

- {"block": "gain", "args": {"gain_db": "drive"}} runs one block. Arguments left out use their defaults.
- {"chain": [node, node, ...]} runs its nodes one after another. An empty chain passes the signal through.
- {"parallel": [{"node": node, "gain_db": -6}, ...]} gives every branch the same input and adds the results. gain_db is optional and may be an expression. Branches with different latency are delayed to line up.

Any node can also have:
- "bypass": a parameter name or expression. At 0.5 or more the node is skipped, with a short crossfade.
- "mix": 0 to 1, a number or expression. 0 is the dry signal and 1 is the node's output. A block such as delay outputs only its wet signal, so set mix on it.
- "only": a list of channel numbers, such as [1]. The node applies to those channels and the rest pass through. Use it with the stereo block's encode and decode to work on mid or side alone.

## Arguments

An argument is a number, or a string. A string is an expression over the plugin's parameters, or for a choice the name of an option, such as "lowpass". A choice can also be driven by a choice parameter, whose index picks the option, so a Mode parameter with the choices Soft and Hard can drive the shaper's curve when the options are in the same order. Values are clamped to each argument's range. A number outside the range is an error. An argument marked fixed must be a number.

## Expressions

Names: any parameter id, any modulator id, rate (sample rate), bpm and pi.
Operators: + - * / ^ and the comparisons < > <= >=, which give 1 or 0. Division by zero gives 0.
Functions: min(a,b) max(a,b) clamp(x,lo,hi) abs(x) pow(a,b) exp(x) ln(x) log2(x) sqrt(x) sin(x) cos(x) tanh(x) floor(x) round(x) db2lin(db) lin2db(x) semitones(n) note(n) mix(a,b,t) if(c,a,b) sync(beats).
note(n) is the frequency of MIDI note n. sync(0.5) is an eighth note at the host's tempo, in milliseconds. if(c,a,b) gives a when c is 0.5 or more.
Expressions are read once for every 32 samples, and each block glides to the new value, so a parameter can change as fast as it likes without clicks. A per-channel parameter is read as its value for channel 0.

## Modulators

"modulators": [{"id": "lfo1", "shape": "sine", "rate": "speed"}] adds an oscillator that any expression can read by its id, from -1 to 1. Shapes: sine, triangle, saw, square, random (sample and hold). rate is in Hz and may be an expression over parameters. Examples: "freq": "800 * 2 ^ lfo1" sweeps a filter over two octaves for a wah or a phaser. A phaser is a parallel of an empty chain and a chain of allpass filters, with their freq driven by a modulator.

## Meters

A block that reports a value, such as dynamics with gain_reduction_db, can show it on a parameter of kind "meter": {"block": "dynamics", "meters": {"gain_reduction_db": "gr"}}.

## Latency

Blocks that look ahead add latency, and the graph adds them up and reports the total to the host. Lookahead is fixed when the plugin is built.

## Checking

set_graph checks the graph against the parameters and says what is wrong and where. Then use check for the gates and render for measurements.
"#;

/// The format guide and every block, for the `list_blocks` tool.
pub fn guide() -> String {
    format!("{}\n# Blocks\n\n{}", FORMAT_GUIDE, describe_blocks())
}

/// A readable list of every block with its arguments, ranges and defaults, for an agent to read.
pub fn describe_blocks() -> String {
    let mut out = String::new();
    for b in blocks::catalogue() {
        out.push_str(&format!("## {}\n{}\n", b.name, b.doc));
        for a in b.args {
            let range = if a.is_choice() {
                format!("one of {}, default {}", a.options.join(" | "), a.options[a.default as usize])
            } else {
                format!(
                    "{} to {}{}, default {}",
                    a.min,
                    a.max,
                    if a.unit.is_empty() { String::new() } else { format!(" {}", a.unit) },
                    a.default
                )
            };
            out.push_str(&format!("- {}: {}{}. {}\n", a.name, range, if a.fixed { " (fixed: a number only)" } else { "" }, a.doc));
        }
        if !b.meters.is_empty() {
            out.push_str(&format!("- meters: {}\n", b.meters.join(", ")));
        }
        out.push('\n');
    }
    out
}
