<!-- Generated from crates/blocks by `muse-mcp docs`. Change the source, then run it again. -->
# Describing a plugin as a graph

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
