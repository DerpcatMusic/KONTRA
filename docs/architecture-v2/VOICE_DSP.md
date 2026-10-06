# Native voice processing

This is executable partial V2-09 work, following the supplied architecture §8.5.
It does not establish Kontakt/Falcon filter equivalence or complete the DSP graph.

`Prepared::with_voice_chains` binds immutable chains to regions in authored order.
Multiple regions may reference one chain; each admitted voice has separate stereo
history. `VoiceChain` specifies ordered processors before and after the amplitude
envelope and an explicit maximum tail duration in output frames. Current processors
are finite static/control-driven linear gain (including polarity inversion) and prepared native biquads:
low-pass, high-pass, unity-peak band-pass, notch, all-pass and peaking EQ.

## Ownership and execution

Mutable processor banks belong to retained plan generations and are prepared on the
control side, including queued replacements. Each bank reserves `voice_capacity *
max_chain_stages` cells, currently 32 bytes per stage/voice; checked multiplication
and layout validation reject impossible sizes. There is no fixed hidden stage limit.
Coefficients are shared; state is addressed by the admitted voice slot and reset
before that slot starts a new chain. Concurrent generations cannot alias state.
Banks travel through the existing retirement queue and are destroyed off audio.

Execution is source and region/velocity gain → pre-envelope stages → amplitude
envelope → post-envelope stages → expression gains/pan → voice sum. An expression
mute still advances source, envelope and processor histories. Unprocessed regions
retain the existing contiguous source-rendering path. Processed voices read source batches into a fixed 64-frame stack buffer, then
advance DSP and envelope state per sample. Source reads stop at source/envelope or
choke boundaries; chunk boundaries do not reset state. The source renderer returns
its produced frame count so zero-input tail processing begins at the exact frame.
This path is measured separately, with no automatic quality downgrade.

The current pipeline is stereo, serial and voice-local. It has no additional buffered
algorithmic latency; filter phase response is not a constant-delay compensation
claim. Bus/family/master scopes, sends, channel-layout conversion, oversampling,
nonlinear processors, time-varying filter controls and broader destination
modulation/smoothing remain required graph work. The model does not move filters across the
envelope or sum independent voice histories to save work.

## Numerical and tail policy

Coefficients follow the [RBJ Audio EQ Cookbook published by W3C](https://www.w3.org/TR/audio-eq-cookbook/),
with normalized double-precision transposed direct-form II state. Frequency must
lie strictly inside `(0, sample_rate/2)` and Q must be positive. Prepared filters
must match the plan rate. Nonfinite parameters and rounded coefficients that fail
strict second-order stability conditions are rejected. Half-angle numerator
identities avoid cancellation near DC/Nyquist. These native parameters are not
proprietary resonance/drive controls and must not be reported as vendor emulation.

A source ending feeds zeros into its pre-envelope chain while its envelope advances;
an envelope ending silences the pre-envelope result while the post chain drains.
A chain's declared tail budget begins at the first completed source/envelope frame.
It is an explicit maximum, not a silence detector or a claim of finite IIR support.
Physical keys, note identities and source EOF remain independent. Zero-length
musical release preserves a configured post-envelope tail. Hard stop/panic ends it
immediately. A finite choke fades the complete chain output, preserving the current
fade level if shortened, and can never extend an existing choke or shorter tail.

Double-precision subnormal state and subnormal f32 output are zeroed locally; no
thread-wide floating-point environment is changed. Nonfinite/unrepresentable output
or poisoned processor state clears that voice's chain state, suppresses the affected
frame and increments the existing fault counter. Other voices keep their state.
This fault counter remains observable; overflow is not silently clipped to full scale.

## Evidence

`src/dsp.rs` compares rendered impulses with independent direct-form-I recurrence,
checks DC/center/Nyquist responses for every implemented response at 44.1/48/96 kHz,
and validates parameter/stability boundaries. `tests/dsp.rs` checks independently
expected PCM, stereo isolation, two overlapping voices sharing coefficients,
pre/post-envelope placement, muted history, source/release tails, slot reuse,
non-extension of finite choke, panic, generation replacement and retained terminal
backpressure. Runtime paths run under allocation/deallocation instrumentation.

The pinned Shortcircuit voice implementation was inspected for processor ownership
and explicit channel-layout routing; see [REFERENCE_REVIEW.md](REFERENCE_REVIEW.md).
No external implementation was copied and no Shortcircuit test was executed here.

`render_workloads --filters COUNT [--muted]` measures the actual new voice path with
1–16 serial filters at 64/256/1,024 voices and 64/256-frame callbacks. It warms the
filters before timing and independently validates every constant-input PCM block.
Its static coefficients and predictable PCM are a local baseline, not a competitor
comparison, modulation benchmark, production deadline guarantee or parity claim.


The first implementation read one source frame per pipeline step. A local CPU-2
release run measured 1,024 voices / one filter / 48 kHz / 64 frames at 1,404.746 us
median, exceeding the 1,333.333 us callback deadline. Fixed-stack source batching
reduced the observed median to 470.399 us (2.99×), with p99 1,082.410 us. For 256
frames the corresponding medians were 5,550.984 and 1,890.766 us (2.94×). These are
single-run local observations with uncontrolled scheduler/frequency variation;
no worst-case or competitor claim follows. Source/envelope/DSP output is checked
against independent convolution and is bit-identical across blocks 1/7/64/129,
including a mid-block release and source EOF. Both one- and four-filter benchmark
runs independently check each rendered output block.

Raw CSVs: `artifacts/voice-dsp-{one-filter,four-filters,batched-one-filter,batched-four-filters}.csv`.

The four-filter workload still exceeds the callback deadline in its 1,024-voice p99
observations: 1,905.066 us for 64 frames and 9,151.052 us for 256 frames at 48 kHz.
Its medians are 1,145.391 and 4,565.206 us. This remains a performance limitation to
address with representative active-modulation/routing workloads; no claim of
superior Kontakt/Falcon performance or production-safe maximum polyphony is made.


Validation: all 215 native tests pass in debug, release and Rust 1.92. Strict
all-target Clippy and both root ownership/KSP boundary tests pass. All unprocessed,
one-filter and four-filter workload output assertions pass. Logs use
`artifacts/voice-dsp-{debug,release,msrv,clippy,boundary}.log`. No DAW plugin was
replaced, no production UI was switched and no broader conformance gate was closed.


## Shared controls driving gain

`VoiceProcessor::ControlGain(GainControl)` binds a stable `ControlId` to linear
amplitude endpoints and an explicit ramp length in output sample frames. A control's
declared integer/real range maps to the endpoints; toggles map false/true, and a
constant domain maps to the low endpoint. Raw typed values remain intact. Integer
projection subtracts in i128, real projection handles spans wider than f64::MAX,
and endpoint values are exact. Gains and their difference must be finite; interpolation
stays inside the endpoint range. This is a native linear-amplitude contract, not a
claim about Kontakt/Falcon decibel curves or smoothing rates.

Preparation compiles authored chains into indexed processors, validates referenced
controls and builds a sorted control-to-binding index. Replacing the control schema
revalidates those identities. Neither rendering nor control writes search voices or
resolve source names. Control updates visit only the matching binding range after
all batch values/revision checks pass.

A gain trajectory belongs to the retained generation, once per DSP binding, shared
by all voices using it. It is evaluated at absolute sample time, so additional voices
and block partitions cannot advance it again or restart it. A write at T with ramp N
uses the current gain at T, reaches the target at T+N, and changes immediately for
N=0. Retargeting begins from the interpolated gain at that timestamp. Repeating an
unchanged target preserves the existing trajectory. Control reads expose the authored
target immediately; the DSP trajectory does not overwrite script/UI state.

The existing atomic edit function drives direct writes, script assignments, recall,
and acknowledged UI requests. A new generation starts at its own default values;
old voices retain the old values/trajectories through terminal backpressure and
retirement. No per-voice gain-ramp allocation, lock, callback into a UI, or new value
owner is introduced. Control banks still exist with no window open.

`tests/control_dsp.rs` checks independent PCM, overlapping voices, ramp reversal,
repeated values, rejection atomicity, multiple bindings, stable IDs after schema
reordering, extreme numeric domains, queued recall and retained generations.
`sampler-ksp/tests/controls.rs` executes a waiting plan-owned UI handler that changes
live DSP through the same state, at blocks 1/7/64 under the heap guard. This is
headless integration; production UI/CLAP wiring and host automation routing remain open. Native
timestamped values now use `Event::Control`, described in [CONTROL_STATE.md](CONTROL_STATE.md#timestamped-control-values). No vendor DSP/performance parity is inferred.

Control/DSP validation: all 229 native tests pass in debug, release and Rust 1.92;
strict all-target Clippy and both root boundary tests pass. Logs use
`artifacts/control-dsp-{debug,release,msrv,clippy,boundary}.log`.
