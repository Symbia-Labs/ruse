<!-- Generated from crates/blocks by `ruse-mcp docs`. Change the source, then run it again. -->
# Describing a plugin as a graph

A graph is a JSON object. It is a node, and it may also hold a list of modulators.

## Nodes

- {"block": "gain", "args": {"gain_db": "drive"}} runs one block. Arguments left out use their defaults.
- {"chain": [node, node, ...]} runs its nodes one after another. An empty chain passes the signal through.
- {"parallel": [{"node": node, "gain_db": -6}, ...]} gives every branch the same input and adds the results. gain_db is optional and may be an expression. Branches with different latency are delayed to line up.
- {"multiply": [node, node, ...]} gives every node the same input and multiplies their outputs, sample by sample. An oscillator in one and an empty chain in the other is a ring modulator: {"multiply": [{"chain": []}, {"block": "oscillator", "args": {"freq_hz": "carrier", "level_db": 0}}]}. With the oscillator's offset at 1 it is amplitude modulation, a tremolo that can run at audio rates. Nodes with different latency are delayed to line up.
- {"split": {"freqs": [200, 2500], "slope": 24, "bands": [node, node, node]}} divides the signal into bands with Linkwitz-Riley crossovers, runs each band through its own node, lowest band first, and adds them back. With every band an empty chain the output has the input's level at every frequency. freqs holds one to three crossover frequencies, lowest first, as numbers or expressions. slope is 24 (LR4, the default) or 48 (LR8) dB per octave. Each crossover is 6 dB down on both sides. Bands with different latency are delayed to line up. A multiband compressor is a dynamics block in each band.
- {"tap": {"name": "ess", "node": node}} is a point an envelope can follow: an envelope modulator with "source": "ess" hears what comes out of the tap's node, a copy of the signal at that point. The signal itself passes the tap unchanged. node is optional, and a band-pass there makes a detector for one range, such as sibilance for a de-esser: {"tap": {"name": "ess", "node": {"block": "filter", "args": {"type": "bandpass", "freq": 7000, "q": 2}}}}. A tap inside a split band hears only that band. Tap names are unique in a graph, and a tap cannot sit inside an oversample node. A tap the audio does not reach, such as one in a bypassed node, hears silence.
- {"oversample": {"factor": 4, "node": node}} runs a node at 2, 4 or 8 times the sample rate, the default 2. The harmonics a shaper makes above the plugin's Nyquist frequency are then filtered out before the rate comes back down, instead of folding back as inharmonic tones. Wrap the shaper, not the whole plugin: oversampling costs processing in proportion to the factor. It adds latency, 32 samples at 2x, 38 at 4x and 40 at 8x, which the plugin reports. Arguments change at the same moments as outside it.
- {"feedback": {"node": node, "gain": 0.6, "delay_ms": 300}} runs a node in a loop. The loop's input is the feedback node's input plus the returns times gain (-0.999 to 0.999). The node runs on that, and its output comes back as the returns after delay_ms. A filter or a shaper inside makes each repeat darker or dirtier, which the delay block's own feedback cannot do. gain and delay_ms may be expressions. delay_ms is at least one sample and at most max_ms, a fixed number from 10 to 4000 that defaults to 2000 and sets the memory used. A loop shorter than 32 samples, 0.67 ms at 48 kHz, runs its node one sample at a time, which costs more processing: that is the range of combs, resonators and plucked strings, where delay_ms is 1000 / frequency, such as "1000 / note(57)". A tap cannot sit inside the loop. The feedback node outputs only the returns, like the delay block, so give it a mix to keep the dry sound. A dub echo: {"feedback": {"node": {"chain": [{"block": "filter", "args": {"type": "lowpass", "freq": "tone"}}, {"block": "shaper", "args": {"curve": "tanh", "drive_db": "dirt"}}]}, "gain": "repeats", "delay_ms": "sync(0.75)"}, "mix": "mix"}.

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

An envelope follows the level of the plugin's input instead: {"id": "env", "shape": "envelope", "attack_ms": 5, "release_ms": 120} reads 0 at silence and 1 for a full-scale peak. attack_ms (0.1 to 500, default 10) and release_ms (1 to 5000, default 150) may be expressions over parameters. "detector": "rms" follows loudness rather than peaks. The loudest channel counts. Examples: "freq": "300 * 2 ^ (5 * env)" opens a band-pass as the playing gets louder, which is an auto-wah, and "drive_db": "6 + 24 * env" drives a shaper harder on loud notes. Use lin2db(env) for a level in dB. "source": "sidechain" follows the sidechain instead, which gives the plugin a sidechain input: {"id": "duck", "shape": "envelope", "source": "sidechain", "attack_ms": 2, "release_ms": 80} with "gain_db": "-24 * min(duck, 1)" ducks the input whenever the sidechain plays. "source" can also name a tap in the graph, to follow the signal at that point.

## Sidechain

A plugin has a sidechain input when its graph listens to one: an envelope with "source": "sidechain", or a dynamics block with "key": "sidechain", which compresses the input by the sidechain's level. The host routes a track to it, such as a kick drum to duck a bass. With nothing routed, the sidechain is silent. render takes a sidechain signal to test it with.

## Subgraphs

A subgraph is a named piece of graph with its own arguments, used as a node: {"subgraph": "parametric_eq", "args": {"low_gain_db": "bass", "high_gain_db": "treble"}}. Arguments left out use their defaults. Each argument can be a number, a choice name, a parameter, or an expression over parameters and modulators, and inside the subgraph it reads like a parameter. A use can carry bypass, mix and only like any node. The subgraphs RUSE ships are listed after the blocks.

Declare your own at the top of the graph, beside modulators, with each argument's default: "subgraphs": {"tilt": {"args": {"amount": 0, "pivot": 800}, "node": {"chain": [{"block": "filter", "args": {"type": "lowshelf", "freq": "pivot", "gain_db": "-amount"}}, {"block": "filter", "args": {"type": "highshelf", "freq": "pivot", "gain_db": "amount"}}]}}}. A subgraph can use another, up to eight deep.

## Meters

A block that reports a value, such as dynamics with gain_reduction_db, can show it on a parameter of kind "meter": {"block": "dynamics", "meters": {"gain_reduction_db": "gr"}}.

## Latency

Blocks that look ahead add latency, and the graph adds them up and reports the total to the host. Lookahead is fixed when the plugin is built.

## Checking

set_graph checks the graph against the parameters and says what is wrong and where. Then use check for the gates and render for measurements.
