# Gates

A plugin built by an agent may never be read by a person. The gates are what make it safe to load anyway. `check` runs all seven, and `package_plugin` refuses a plugin that fails one. Each plugin also carries them as `tests/gates.rs`, so `cargo test` runs them too.

The gates run the plugin offline at 48 kHz. Apart from the layout gate, each runs at two channel counts: the smallest the plugin accepts, and the largest it accepts up to 24.

| Gate | Passes when |
|---|---|
| `layout` | The plugin accepts at least one of 1, 2, 6, 8, 24 or 64 channels, with the same count in and out |
| `parameters_described` | Every parameter has a name and a description |
| `finite_at_extremes` | The output stays finite and under +60 dBFS, and `process` does not panic, at the default settings and with each parameter at its minimum and its maximum. The inputs are silence, an impulse, a 1 kHz sine at -6 dBFS, full-scale noise and a full-scale tone at the Nyquist frequency |
| `no_alloc` | `process` allocates nothing over 60 blocks of 512 samples, while every parameter jumps between its limits every 10 blocks |
| `repeatable` | Two fresh instances turn the same half second of noise into identical output, bit for bit |
| `block_size_independent` | The output at a block size of 32 matches the output at 512 to within -80 dBFS. Hosts pick their own block size, so the sound cannot depend on it |
| `no_subnormals` | A burst of noise followed by 1.5 seconds of silence leaves no subnormal numbers in the output, at the default settings, with every parameter at its minimum and with every parameter at its maximum |

Each gate prints one line, and a failing gate says what failed and where, such as the parameter, the channel count and the input. Every gate has a test plugin written to fail it, so a gate that stops catching problems fails RUSE's own tests.

## Why these

- A non-finite sample or a huge level from one parameter setting can damage speakers and hearing, and it can take down the host's mix.
- An allocation on the audio thread can stall it, which the listener hears as a dropout. It may happen rarely enough to slip through any test that only listens.
- A plugin that sounds different each time it loads, or at a different buffer size, cannot be trusted in a mix or a bounce.
- Subnormal numbers make a processor run far slower. They come from filters and feedback paths that decay towards silence, so a plugin can be fine while playing and spike the CPU once the track stops.

## What the gates do not cover

Passing every gate means a plugin is safe to load. It does not mean the plugin sounds good. The gates do not measure:

- CPU cost.
- Aliasing, such as a shaper driven hard without oversampling.
- How a parameter sounds at every value between its limits, or across two or more parameters moving at once.
- Behaviour that only a particular host or format brings out. [packaging.md](packaging.md) covers the checks a packaged bundle goes through, and `auval` for Audio Units.

`render` reports levels and octave bands before and after, so an agent can confirm what a plugin does to a signal. Trust your ears for the rest.
