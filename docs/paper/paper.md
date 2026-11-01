## Abstract

A musician who loads a faulty audio plugin can lose the session to a crash or send a full-scale burst to the speakers. Ruse, an MIT-licensed framework written in Rust, lets an AI agent build audio plugins whose code no person may ever read. The person and the agent describe a plugin as a manifest of self-describing parameters and a JSON graph of fifteen tested digital signal processing (DSP) blocks, and they edit both through twelve Model Context Protocol (MCP) tools. Ruse generates the code and refuses to package the plugin as CLAP or Audio Unit until it passes seven behavioral gates: channel layout, described parameters, bounded output at every parameter extreme and combination, no allocation on the audio thread, bit-identical repeatability, block-size independence, and no subnormal numbers. We test each gate against a probe plugin built to fail it.

We compared Ruse with five other plugin tools: JUCE, nih-plug, Faust, Cmajor and RNBO. We ran its test suite on a second platform (281 of 281 non-macOS tests pass) and checked its DSP against closed-form references and independent re-implementations. The biquads, Linkwitz-Riley crossovers, oversampler, delay, reverb matrix, compressor, limiter (at a fixed ceiling), pitch ratio and LPC formant mathematics hold exactly. Six claims hold only in part, and the one-pole filter misses its documented corner by 3 dB at 5 kHz. In a case study, the gates caught a runaway feedback loop that a listening check at default settings would have missed, named the settings that caused it, and passed the plugin once we added one block.

## 1. Introduction

Musicians and producers run audio plugins inside a host such as Logic or Bitwig. When a plugin crashes, lets a feedback loop run away or stalls the audio thread, the damage does not stay inside the plugin: the host stops the session, the audio drops out, or the speakers take a full-scale burst. An AI agent can now write a plugin from a description, and nobody may ever read that code. Ruse is a Rust framework built for that author. A person and an agent make each plugin together, and Ruse refuses to package it until it passes a fixed set of automated checks, so the framework makes the code safe to load into a session with no reviewer in the loop.

For twenty years, developers have built plugins the standard way, with [JUCE](https://www.synthtopia.com/content/2020/04/22/pace-acquires-juce-audio-development-platform-from-roli/), a C++ framework first released in 2004 and owned by PACE since April 2020. JUCE assumes a person writing C++ by hand, and it is licensed under the AGPLv3 or a [paid license](https://juce.com/get-juce/) for closed-source products over $20,000 a year in revenue.

Ruse makes four design choices that follow from its premise:

1. **A plugin starts as a manifest.** One JSON file lists the plugin's parameters, each with a name, description, unit, range, curve and group, and its signal path as a graph of tested blocks. The person and the agent edit the manifest; Ruse generates the code. Most plugins need no hand-written DSP.
2. **Gates before packaging.** Seven checks (layout, described parameters, finite output at every parameter extreme, no allocation in `process`, bit-identical repeatability, block-size independence and no subnormal numbers) run on every plugin, and `package_plugin` refuses one that fails. Each gate has a test plugin written to fail it, so a gate that stops catching faults fails Ruse's own tests.
3. **Safe Rust only.** Rust's compiler proves memory safety and the absence of data races for code that uses no `unsafe`, which matters in a plugin whose audio and control threads share parameter state. Ruse confines unsafe code to the CLAP and Audio Unit wrappers and a few lines of its core (the flush-to-zero register writes and a lock-free handoff between threads). The block library and every generated plugin declare `#![forbid(unsafe_code)]`, so the compiler gives each plugin body that proof, whoever wrote it.
4. **Agents as the interface.** Twelve tools served over the Model Context Protocol (MCP) create, build, check, render and package plugins, and the same tools run from a shell. An agent reads the block catalog from the same generated text a person reads in the docs.

Ruse is all new code with no dependencies outside its repository, MIT licensed, and in public beta at version 0.2.0 (October 19, 2026) on [GitHub](https://github.com/Symbia-Labs/ruse). It builds CLAP plugins and Audio Units on macOS. In this paper we describe how it works, measure its DSP against independent references, and list the defects those measurements found.

## 2. Related work

Developers build plugins with two families of tools: frameworks for writing plugins in a general-purpose language, and DSP languages or visual patchers that compile to plugins. Ruse is a framework, but most Ruse plugins are written in a small graph language, so it borrows from both.

| Tool | Language and model | Formats | License | Checks before shipping |
| --- | --- | --- | --- | --- |
| [JUCE](https://juce.com/get-juce/) | C++ classes written by hand | VST3, AU, AAX, LV2, standalone | AGPLv3, or paid above $20,000 revenue | None built in; developers use the format validators |
| [nih-plug](https://github.com/robbert-vdh/nih-plug) | Rust, declarative parameters, hand-written DSP | VST3, CLAP | ISC, VST3 bindings GPLv3; in maintenance mode | None built in |
| [Faust](https://faust.grame.fr/) | Functional DSP language compiled to C++, C, LLVM, WebAssembly or Rust | Through "architecture" wrappers, including CLAP | Open-source compiler; terms for generated code set by its architecture files | Compiler checks of the signal graph |
| [Cmajor](https://cmajor.dev/) | C-family DSP language, JIT through LLVM | VST, AU, AAX through a generated JUCE project; WebAssembly | Commercial and educational options | Language-level |
| [RNBO](https://rnbo.cycling74.com/learn/audio-plugin-target-export-overview) | Max patcher, gen~ for sample-level code | VST3 and AU, built in the cloud | Commercial, with Max | None built in |
| **Ruse** | JSON graph of 15 tested blocks in safe Rust, or hand-written Rust; edited through MCP tools | CLAP, AU v2 (macOS) | MIT | Seven gates required before packaging; format checks and `auval` |

[CLAP](https://en.wikipedia.org/wiki/CLever_Audio_Plug-in), released in 2022 by u-he and Bitwig under the MIT license, made it practical for a new framework to target a modern open format without a license from a format owner. Ruse writes its CLAP and Audio Unit wrappers from the published C headers and checks its struct layouts against them with small C programs.

Ruse's difference from the others is where it puts the burden of correctness. JUCE and nih-plug trust the author; Faust and Cmajor constrain what can be written; Ruse lets an agent write anything a graph or safe Rust allows, then refuses to package the result unless it passes behavioral tests that a listening check would miss: an allocation on the audio thread every few minutes, a gain explosion that needs three parameters at their limits at once, or a difference in output between a 32-sample and a 512-sample host buffer. None of the tools above require such checks by default.

## 3. Architecture

Ruse is eight crates (Rust packages) and about 26,000 lines of Rust with no outside dependencies. A plugin travels one path from idea to installed bundle, and the gates sit on that path where nothing can go around them.

[[figure:build-path|Ruse build path, from manifest to installed bundle]]

| Crate | Holds |
| --- | --- |
| `ruse-core` | The `Plugin` trait, channel layouts from 1 to 64, self-describing parameters, saved state, and a lock-free `Handoff` from control thread to audio thread |
| `ruse-blocks` | Fifteen DSP blocks, the expression language and the graph runtime; `#![forbid(unsafe_code)]` |
| `ruse-host` | WAV I/O, test signals, offline rendering with measurements, and the gates |
| `ruse-clap`, `ruse-au` | The format wrappers, each with a verifier that loads a built bundle the way a host does |
| `ruse-mcp` | The MCP server and the `ruse-mcp` command; generates each plugin's code from its manifest |
| `ruse-gain`, `ruse-tape` | The smallest complete plugin, and a multichannel tape machine used as the large test case |

### 3.1 The plugin contract

A plugin implements one trait: it declares a layout and a parameter set, prepares (where it may allocate) and processes (where it may not). Parameters are declared as constants with a name, description, unit, range, curve, choices, scope and group, so a knob, a host's automation lane and an agent's tool call share one definition. A parameter can be global or per channel; the framework allocates every slot up front, so the audio thread only reads and writes atomics.

### 3.2 The graph runtime

A graph plugin's `dsp.rs` loads its JSON graph when an instance is created. Five properties make graph plugins behave the same in any host:

- **A control grid tied to the stream.** Controls update once every 32 samples on a grid counted from the start of the stream, not from the host's blocks, and each block glides from the previous control value to the next by sample position. Output at any host block size matches to within a millionth.
- **Latency compensation.** Blocks that look ahead report latency; the graph sums it, delays parallel branches and dry paths to match, and reports the total to the host.
- **No allocation in `process`,** enforced by a counting allocator in the gates rather than by convention.
- **No subnormals.** Decaying state is flushed before it can go subnormal, and both wrappers set flush-to-zero while `process` runs.
- **Determinism.** Seeded modulators and noise, so two instances produce the same output bit for bit.

### 3.3 The wrappers

The CLAP wrapper exposes the main bus as one port in and one out, with the channel count negotiable within the plugin's declared range, and a second input port for a sidechain. The Audio Unit wrapper is a version 2 unit, effect or music device, answering the property selectors a host asks for. Both are checked against the format's C headers by small C programs compiled in the test suite (CLAP 1.2.10 and the macOS SDK), and packaging loads each built bundle through a verifier, runs the CLAP project's `clap-validator`, and runs Apple's `auval` on each Audio Unit.

## 4. Blocks, graphs and the tape model

Most Ruse plugins are a few lines of JSON. The Warm Saturator example began as hand-written Rust; rebuilt as three blocks, it renders the same audio to within −127 dBFS:

```json
{ "bypass": "bypass", "chain": [
  { "block": "gain",    "args": { "gain_db": "drive" } },
  { "block": "shaper",  "args": { "curve": "mode" } },
  { "block": "onepole", "args": { "type": "lowpass", "freq": "tone" } }
] }
```

### 4.1 Blocks

| Block | What it does | Method |
| --- | --- | --- |
| `gain` | Level, polarity, constant-power pan | Gain glides over 32 samples |
| `filter` | Low-pass, high-pass, band-pass, notch, bell, shelves, all-pass; 12 to 48 dB per octave | RBJ biquads in transposed direct form II, cascaded Butterworth sections |
| `onepole` | 6 dB per octave tone control and DC removal | One-pole recursion |
| `shaper` | tanh, hard, soft-clip, fold and tube curves with bias | Memoryless curve; oversample it to stop aliasing |
| `bitcrusher` | Fewer bits and a slower sample clock | Optional TPDF dither |
| `dynamics` | Compressor, expander, gate; lookahead; sidechain key | Soft-knee gain computer in dB, attack and release smoothing |
| `limiter` | Brickwall peak limiter | Running minimum of needed gain, then a moving average, against a matching audio delay |
| `delay` | Echo, chorus, flanger, ping-pong, tempo sync | Catmull-Rom interpolated delay line, feedback up to 0.97 |
| `reverb` | Size, decay, pre-delay, damping, diffusion, freeze, early reflections | Eight-line feedback delay network with a Hadamard matrix |
| `pitch_shift` | ±24 semitones with formant control | Two crossfaded taps on a delay line; LPC envelope swap through lattice filters |
| `tremolo` | Amplitude modulation and auto-pan | LFO on gain |
| `stereo` | Width and mid/side | Exact mid/side encode and decode |
| `harmonic_eq` | Bells on the harmonics of one fundamental | Bank of RBJ peaking filters |
| `oscillator` | Sine, triangle, saw, square, pulse | PolyBLEP at the discontinuities |
| `noise` | White, pink, brown at a set RMS | Kellett's pink filter, seeded |

### 4.2 Nodes, expressions and modulators

Nodes arrange blocks: `chain`, `parallel` (with gains), `multiply` (ring modulation), `split` (two to four bands through Linkwitz-Riley crossovers at 24 or 48 dB per octave, summed back flat), `oversample` (2, 4 or 8 times, through linear-phase half-band filters), `feedback` (any node in a loop, down to one sample), `tap` (a point an envelope can follow) and named `subgraph`s with their own arguments. Any argument can be a number, a parameter name or an expression over parameters, tempo (`sync(0.75)`), MIDI notes (`note(57)`) and modulators. Modulators are LFOs in five shapes and envelope followers on the input, the sidechain or a tap. An auto-wah, a de-esser, a multiband compressor and a Karplus-Strong string are each a graph with no code.

### 4.3 The tape model

`ruse-tape` is a port of the tape stage from Bias, Symbia Labs' TASCAM 2400 companion app, written first in Swift. Each track runs record equalization, Jiles-Atherton magnetic hysteresis, playback equalization, head losses (spacing, thickness and gap), a playback head bump, wow and flutter, bleed from neighboring tracks and hiss, for any number of tracks in one instance. Its hysteresis follows the normalized form used in Chowdhury's [real-time tape model](https://dafx.de/paper-archive/details/Z5Ow_E5YHVxvndTAyeEuzA) (DAFx 2019), solved with an explicit midpoint step at twice the sample rate. Golden files rendered by the Swift original hold the Rust port to it across six configurations, within a peak difference of 0.003 of full scale. The tolerance is that loose because the saturating stage amplifies tiny differences: nudging the Swift original's input by one part in a million moves its output by about 0.0017.

## 5. Method: what is checked, and how

Ruse checks every plugin at four levels: the gates, measured block tests, format checks and continuous integration from a clean machine.

### 5.1 The gates

The gates run each plugin offline at 48 kHz, at the smallest channel count it accepts and at the largest up to 24, and with noise on the sidechain when it has one.

| Gate | Passes when | Fault it catches |
| --- | --- | --- |
| `layout` | The plugin accepts 1, 2, 6, 8, 24 or 64 channels, same count in and out | A plugin no host can open |
| `parameters_described` | Every parameter has a name and a description | An agent or a person cannot tell what a control does |
| `finite_at_extremes` | Output stays finite and under +60 dBFS at every parameter limit with silence, an impulse, a sine, full-scale noise and a Nyquist tone; and no combination of limits, plus 24 settings between, makes the plugin more than 30 dB louder than its input | Runaway feedback and gain that needs several parameters at once |
| `no_alloc` | `process` allocates nothing over 60 blocks while every parameter jumps between its limits | Dropouts that may occur once in minutes |
| `repeatable` | Two fresh instances give bit-identical output on the same noise | Unseeded randomness, uninitialized state |
| `block_size_independent` | Output at a 32-sample block matches output at 512 to within −80 dBFS | Sound that depends on the host's buffer |
| `no_subnormals` | A noise burst then 1.5 s of silence leaves no subnormal samples, at default, all-minimum and all-maximum | CPU spikes after the track stops |

Each gate has a probe plugin built to fail it (an unflushed decay, an allocation in `process`, gain that runs away between the limits, NaN output, unseeded randomness, block-size-dependent output, an undescribed parameter), and Ruse's own tests check that each probe fails its gate, and for several probes that the neighboring gates still pass. A gate that silently stops catching its fault therefore fails the build.

### 5.2 Measured block tests

Each block has tests that measure it rather than run it: the response of every filter type at its corner, the compressor's static curve, the delay's echo times, the gain at each harmonic of the harmonic equalizer, the limiter's ceiling, the oversampler's passband, the reverb's decay. The repository holds 295 test functions; on our x86\_64 Linux machine the 281 that do not need macOS all passed, with 2 ignored.

### 5.3 Format checks and CI

The CLAP and Audio Unit wrappers' struct layouts are compiled against the CLAP 1.2.10 header and the macOS SDK in the test suite. `package_plugin` loads each built bundle through a verifier and runs Apple's `auval` on each Audio Unit it installs; the CLAP project's `clap-validator` has also been run clean on a packaged plugin, though it is not part of the tool. On every push, CI on a clean macOS runner follows the README's quick start as a new user would: it installs `ruse-mcp`, makes a workspace, checks and renders two examples, creates a tilt equalizer from a one-line tool call, and packages and installs it as an Audio Unit and a CLAP plugin. A second job packages all ten examples and runs `auval` on each.

What the gates do not cover is stated in Ruse's own documentation: CPU cost, aliasing, and whether a plugin sounds good. Section 6 measures the second and parts of the third.

## 6. Accuracy

We checked Ruse's DSP claims against independent references: closed-form responses, a direct DFT, the RBJ cookbook, published models, and our own Python and Rust re-implementations run outside the repository. Most of the mathematics holds exactly. Six claims hold only in part, and one fails above a few kilohertz.

### 6.1 What holds

| Claim | Independent check | Result |
| --- | --- | --- |
| Biquad coefficients follow the RBJ cookbook | Coefficients compared term by term; Butterworth Q values 1/(2 sin((2k+1)π/4n)) | Exact. 24, 36 and 48 dB cascades measure −24.2, −36.3 and −48.5 dB one octave above the corner |
| Linkwitz-Riley bands sum back flat | Sum of low and high equals the Butterworth all-pass | Magnitude error 2.5e-14 (LR4) and 5.3e-8 (LR8) |
| Oversampling is flat to 20 kHz | 65-tap Kaiser half-band, β = 8 | Within ±0.001 dB to 20 kHz, −79 dB above 28 kHz; latencies 32, 38 and 40 samples as stated |
| Limiter: no sample over the ceiling | Port of the gain path; proof that every gain in the averaging window is at or below the need of the sample leaving | Holds: peak output 1.0000 of the ceiling, for a fixed ceiling |
| Delay feedback is stable | Catmull-Rom interpolator response | Gain never above 1.0000000; with feedback at most 0.97 the loop is stable |
| Reverb matrix is lossless | Hadamard × 1/√8 | Orthogonal to 1.2e-7; per-line loss 10^(−3L/(T60·fs)) is the right formula |
| Compressor soft knee | Giannoulis, Massberg and Reiss gain computer | Matches; 0.5625 dB of reduction at threshold |
| Pitch ratio is exact | Delay slope 1 − ratio per sample | Exact; the sin²/cos² crossfade averages −1.25 dB in power, as the tests document |
| Formant method | Levinson-Durbin, analysis and synthesis lattices, log-area-ratio interpolation | Recursions correct; interpolated filters are stable; the level correction is the right white-noise power ratio |
| Pink noise | Kellett's filter | Normalization exact (0.3275974); slope within ±0.05 dB of −3 dB per octave at 44.1 and 48 kHz |
| Pan, mid/side, dither | Closed form | L² + R² = 2 at every pan; mid/side round trip exact; dither is true TPDF at ±1 LSB |
| Tape filters | Head-loss formulas, cepstral minimum phase, record and playback emphasis | Textbook forms; emphasis and de-emphasis are an exact bilinear inverse pair; FFT matches a direct DFT |

### 6.2 What holds only in part

The one-pole filter is documented as "3 dB down at the corner." Its coefficient, a = 1 − e^(−2πf/fs), maps the analog pole without prewarping. The low-pass stays near −3 dB to a few kilohertz; the high-pass, built as the input minus the low-pass, drifts much sooner. The same coefficient sets delay and reverb damping, the dynamics sidechain filter and envelope smoothing.

[[figure:onepole|Computed from onepole_coef in crates/blocks/src/blocks/util.rs, 48 kHz]]

| Claim | What we measured | Gap |
| --- | --- | --- |
| Reverb `decay_s` sets the decay time "at low and middle frequencies" | With the default 6 kHz damping and 2 s set, T60 is 2.00 s at 100 Hz, 1.85 s at 1 kHz, 1.52 s at 2 kHz, 0.96 s at 4 kHz | The damping filter sits inside the loop and shortens the mid band too; the test turns damping off and allows about ±20% |
| Limiter ceiling holds under automation | Ceiling dropped from 0 to −12 dB during play | 39 samples over the new ceiling, by up to 10 dB: the gain is computed from the old ceiling and applied D samples later |
| Tape hysteresis at 2× the sample rate | Against an RK4 reference at 64 substeps, default settings | At 1 kHz, 0 dBFS: fundamental 0.674 against 0.668, third harmonic 7.7% against 7.8%. At 10 kHz, −10 dBFS: 0.774 against 0.619 (+1.9 dB) and 8.9% against 6.7% third harmonic. The scheme converges to the reference at 4× to 64×, so treble saturation depends on the step size |
| Formants come "three quarters of the way back" | Read from the test | The figure is a spectral centroid of harmonics under 5 kHz on one synthetic vowel at 150 Hz; the test asserts more than 65%. A fair proxy, not measured formant positions |
| Feedback loops tune a plucked string with `1000 / note(57)` | Karplus-Strong graph with a 5 kHz one-pole in the loop | 8.6 cents flat at A3 and 33 cents flat at A5: the inner node's own delay is not subtracted from the loop |
| Lookahead means "the gain is already down when a peak arrives" | Default 10 ms attack, 5 ms lookahead | 1 − e^(−0.5): about 39% of the reduction is in place when the peak arrives |
| Oscillators are "band-limited" | 5 kHz PolyBLEP saw | Aliasing reduced, not removed: the folded 18 kHz component sits at −28.4 dB against −15.3 dB for a naive saw |
| Pink noise at any rate | Kellett's coefficients at 192 kHz | Slope off by up to 1.1 dB; the coefficients are tuned for 44.1 kHz |

## 7. Case study: how plugins and blocks get made

### 7.1 An agent's gap list became the 0.2.0 roadmap

Ruse 0.1.0 shipped nine blocks. The agent that built plugins with it kept a list of what it could not express and proposed an order; every item on that order and on the follow-up gap list is built in 0.2.0, and each new node has an example plugin that uses it.

### 7.2 A dub echo, built through the tools

For this paper we built a plugin the way the README intends: one `create_plugin` call with five parameters and a graph, run through the Ruse MCP server on the author's Mac. The first graph put a low-pass and a +12 dB "warmth" bell inside a feedback loop with feedback up to 0.95. Any loop gain over 1 runs away, and 0.95 × 4 at 250 Hz is 3.8.

The gates caught it on the first `check`, and named the settings that did it:

```
GATE finite_at_extremes FAIL output reaches +382 dBFS on channel 1, 400 dB louder than its input,
with time=50 repeats=0.95 tone=500 warmth=12 mix=1, input white noise at -18 dBFS.
GATES 6 of 7 passed
```

The other six gates passed, which is the point: a listening test at the default settings (feedback 0.6, warmth +3) would have heard nothing wrong. Adding a `tanh` shaper inside the loop bounds every trip around it. The second `check` passed all seven gates, with 110 renders at the parameter limits and a largest gain of +18.0 dB across 56 combinations of settings.

`render` then measured what the plugin does. With noise at −18 dBFS, Tone at 1.5 kHz and Warmth at +6 dB, the wet signal rose 10.1 dB in the 250 Hz octave and fell 30.7 dB at 8 kHz and 47.5 dB at 16 kHz, which is the dark, thick dub repeat the graph describes. An impulse shows the loop itself:

[[figure:dub-echo|Render of paper-dub-echo, impulse at −6 dBFS, 48 kHz; echo energy measured by the author from the WAV]]

With Repeats at 0.6 alone each echo would fade 4.4 dB. The bell lifts the loop gain near 250 Hz to about 0.6 × 10^(3/20) = 0.85, or −1.4 dB a trip, and once the higher frequencies have died away that band sets the decay: the measured fade settles to 1.6 dB a repeat. The echoes also arrive a little late, 250.4 ms for the first and about 0.1 ms more each trip, because the filters' own delay inside the loop is not subtracted from `delay_ms`. That is the same effect that leaves a Karplus-Strong string flat (Section 6.2).

The same tool call, rendered by the same Ruse commit on an x86\_64 Linux machine, gave the same levels to the displayed precision (output peak −20.8 dBFS, RMS −58.0 dBFS on both).

## 8. Limitations and known defects

Ruse's changelog states most of its limits. Our analysis found several more that the documentation leaves out.

| Area | Limit or defect | Source |
| --- | --- | --- |
| Platforms and formats | Packaging on macOS only; no VST3, Audio Unit v3 or AAX; effects only, no MIDI-driven instruments from a graph; no editor window | Changelog |
| Automation | Parameter changes take effect at the start of each host block, so automation is not sample-accurate; in a graph a per-channel parameter reads its channel-0 value | Changelog |
| Gates | Do not measure CPU cost, aliasing, or how a plugin sounds between its limits | Gates documentation |
| Gates | `no_subnormals` checks output samples only, not filter state, which is where the CPU cost of subnormals comes from | Our reading of `gates.rs` |
| One-pole filter | No prewarping: the high-pass is 6 dB down at a nominal 5 kHz corner at 48 kHz (Section 6.2) | Our measurement |
| Limiter | A ceiling automated downward lets peaks through for the lookahead time, up to 10 dB over | Our measurement |
| Reverb | `decay_s` holds below about 500 Hz with default damping, not across the middle band | Our measurement |
| Feedback loops | The inner node's delay is not subtracted from the loop time, so tuned loops run flat and echoes drift late | Our measurement and Section 7.2 |
| Tape model | Treble saturation depends on the 2× solver step; neighbor bleed arrives one host block late, so it depends on block size; `ruse-tape` is not run through the gates | Our measurement and reading |
| Tape test | The treble-compression test was set to the Swift original's 1.97 dB rather than the 3 dB the Bias specification asks for | `tests/tapecheck.rs` |
| Pitch shifter | The time-varying lattice is only guaranteed stable in normalized form; a level guard keeps the output bounded in practice | Our reading |
| Precision | Coefficients designed in f64 and stored as f32 lose a little at extremes: a 10 Hz, Q 40 low-pass peaks 0.5 dB low | Our measurement |

None of these makes a plugin unsafe to load, which is what the gates promise. Several make a block do something other than its documentation says, which matters more in a framework whose documentation is also the agent's only model of the blocks: an agent that reads "3 dB down at the corner" will set a 5 kHz high-pass and get 6.

## 9. Future work and conclusion

The defects in Section 8 suggest a short list. Prewarp the one-pole coefficient, or give the high-pass its own. Subtract the inner node's group delay from feedback loops. Recompute the limiter's needed gain from the ceiling in force when a sample leaves, not when it enters. State reverb decay at the frequencies where it holds, or move damping out of the decay path. Run the tape model at 4× or flag treble saturation as step-dependent, and put `ruse-tape` through the gates. Each of these is a small change, and most come with a test this paper's analysis already ran.

The larger gaps are on the roadmap Ruse publishes: VST3 and Audio Unit v3, Windows and Linux packaging, MIDI input so a graph can be an instrument, spectral processing, sample-accurate automation and an editor window.

Ruse's blocks use well-known methods, and most of them are implemented exactly. What Ruse adds is the arrangement: a manifest an agent can edit, blocks whose documentation is generated from the code the agent calls, and gates that sit on the only path to a bundle, each with a probe that proves it still catches its fault. In our case study the gates caught a runaway loop that a quick listen at the default settings would have missed, named the settings that caused it, and passed the plugin once we added a single block. A person who asks an agent for a plugin and never reads its code depends on exactly that.

## Disclosure

We built Ruse and wrote this paper with Claude, an AI system made by Anthropic, working under Brian Gilmore's direction. Brian Gilmore set the goals and made the design decisions, among them MCP as the way plugins get made, all-new code with no outside dependencies, the MIT license and Bias Tape as the large test case. Claude wrote the code, ran the checks in Sections 6 and 7 and wrote the text.

Data for this paper: the Ruse repository at public commit a1ec50b and the author's local commit 5c8d291; its test suite run on x86\_64 Linux; independent re-implementations of the filters, limiter, reverb, hysteresis and formant code in Python and Rust; and a plugin built and rendered through the Ruse MCP server on the author's Mac on October 27, 2026.

## References

1. Symbia Labs. [Ruse repository](https://github.com/Symbia-Labs/ruse), version 0.2.0, October 19, 2026.
2. JUCE. [Licensing and pricing](https://juce.com/get-juce/).
3. Synthtopia. [PACE acquires JUCE audio development platform from ROLI](https://www.synthtopia.com/content/2020/04/22/pace-acquires-juce-audio-development-platform-from-roli/), April 22, 2020.
4. Robbert van der Helm. [nih-plug](https://github.com/robbert-vdh/nih-plug).
5. GRAME-CNCM. [Faust](https://faust.grame.fr/).
6. Cmajor Software. [Cmajor](https://cmajor.dev/).
7. Cycling '74. [RNBO audio plugin export](https://rnbo.cycling74.com/learn/audio-plugin-target-export-overview).
8. [CLever Audio Plug-in (CLAP)](https://en.wikipedia.org/wiki/CLever_Audio_Plug-in), u-he and Bitwig, 2022.
9. Chowdhury, J. [Real-Time Physical Modeling for Analog Tape Machines](https://dafx.de/paper-archive/details/Z5Ow_E5YHVxvndTAyeEuzA). DAFx 2019.
