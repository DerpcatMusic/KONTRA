# Native voice processing

This is executable partial V2-09 work, following the supplied architecture §8.5.
It does not establish Kontakt/Falcon filter equivalence or complete the DSP graph.

`Prepared::with_voice_chains` binds immutable chains to regions in authored order.
Multiple regions may reference one chain; each admitted voice has separate stereo
history. `VoiceChain` specifies ordered processors before and after the amplitude
envelope and an explicit maximum tail duration in output frames. Current processors
are finite static/control-driven linear gain (including polarity inversion), static
2×2 stereo matrices and prepared native biquads:
low-pass, high-pass, unity-peak band-pass, notch, all-pass, peaking and low/high-shelf EQ.

## Ownership and execution

Mutable processor banks belong to retained plan generations and are prepared on the
control side, including queued replacements. Each bank reserves `voice_capacity *
max_chain_stages` cells, currently 40 bytes per stage/voice; checked multiplication
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

This pipeline is stereo, serial and voice-local. The separate [bus DAG](BUS_DSP.md)
now processes summed signals using the same processor kernels and distinct histories. It has no additional buffered
algorithmic latency; filter phase response is not a constant-delay compensation
claim. Family scopes, arbitrary channel-layout conversion, oversampling,
nonlinear processors and broader destination
modulation/smoothing remain required graph work. Shared-control state-variable filters
now support sample-clock cutoff/Q automation as described below. The model does not move filters across the
envelope or sum independent voice histories to save work.

## Numerical and tail policy

Stereo matrix rows are output L/R and columns input L/R. Each output uses both
original input samples, evaluated in f64; in-place channel updates cannot feed
themselves accidentally. Coefficients are finite but not clamped or normalized.
This represents swaps, polarity, mono folds, mid/side transforms, stereo width and
balance/pan coefficients without imposing a single vendor law. Importers must
derive the correct source law and place the stage explicitly in the chain.
Coefficients are immutable per prepared generation; audio-rate matrix automation
is not yet implemented. Existing voice tail, fault containment and retirement
owners apply. Tests cover noncommuting stage order, mid/side roundtrip, crossfeed,
filter impulse tails, block partitions, slot reuse and overflow containment.

Coefficients follow the [RBJ Audio EQ Cookbook published by W3C](https://www.w3.org/TR/audio-eq-cookbook/),
with normalized double-precision transposed direct-form II state. Frequency must
lie strictly inside `(0, sample_rate/2)` and Q must be positive. Prepared filters
must match the plan rate. Nonfinite parameters and rounded coefficients that fail
strict second-order stability conditions are rejected. Low/high-pass half-angle numerator
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

`Processor::ControlGain(ControlRange)` binds a stable `ControlId` to linear
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


## Native shelving EQ

Low/high shelves use the same immutable biquad coefficients and independent stereo
voice histories. Gain is in decibels, frequency is the shelf midpoint, and Q controls
resonance. Q=1/sqrt(2) corresponds to RBJ shelf slope S=1 (monotonic); larger Q can
overshoot. Import profiles must translate their source slope/resonance semantics
explicitly. Invalid/nonfinite or rounded-unstable filters fail preparation. No new
processor runtime, per-frame coefficient calculation or allocation is introduced.

The W3C/RBJ formulas and pinned sfizz
[`rbj_filters.dsp`](https://github.com/sfztools/sfizz/blob/f5c6e29f23b8057867c08e88f5f6ac6738baa30b/src/sfizz/dsp/filters/rbj_filters.dsp#L122)
were reviewed as references. sfizz's wrapper clamps Q/frequency; the native prepared
API retains its explicit validation. No SFZ frontend work is involved.

Checks extend the independent recurrence and DC/midpoint/Nyquist response targets
to positive/negative shelves at 44.1/48/96 kHz. A second check cascades opposite-gain
shelves across 40 Hz, 1 kHz and 0.49 times sample rate, low/normal/resonant Q, zero/
6/24 dB and independent stereo inputs. The result matches a wire within 2e-9;
unit-slope frequency grids remain monotonic within numerical tolerance. These test
native EQ semantics, not a Kontakt/Falcon processor model.

Shelf validation: 281 native debug/release tests and strict all-target Clippy pass
(`artifacts/shelf-eq-*`).


## Causal stereo delay

`Processor::Delay(Delay::new(frames, feedback, dry, wet))` uses the same native
kernel in voice pre/post chains and shared buses. Time is a positive integer number
of output frames; the delay is an intentional effect, not processing latency to
compensate out of parallel paths. Dry/wet gains are separate finite linear values.
The two-by-two feedback matrix supports independent channels, polarity and
cross-channel echoes. Each absolute row sum must be strictly below one, a
conservative sufficient stability condition. This rejects some stable noncontractive
matrices as well as self-oscillating settings; no source-profile equivalence follows.

For delayed stereo signal `d[n]`, the ring stores `x[n] + F d[n]` and emits
`dry*x[n] + wet*d[n]`. Reads precede writes even at a one-frame delay. This is the
causal delayed-output feedback model described by
[Julius O. Smith](https://ccrma.stanford.edu/~jos/pasp/Feedback_Comb_Filters.html),
extended to a contractive stereo matrix. All feedback storage is f64. Subnormal
stored samples are zeroed, and nonfinite writes enter existing chain fault containment
before entering the ring. Output overflow clears the owning chain's validity.

Prepared chains assign disjoint delay ranges across pre/post processors; buses
assign disjoint ranges across their graph. Each voice reserves the largest authored
chain's summed delay length, multiplied by the configured voice capacity; buses
reserve exactly their summed lengths. Storage costs 16 bytes per stereo delay frame.
Checked layout/multiplication and fallible reservation happen on control, including
queued replacements. Plans without delays allocate no delay samples. Long per-voice
delays can be expensive; importers must preserve the authored processing scope and
resource admission must account for this memory rather than silently moving effects
to buses. No arbitrary voice-to-bus optimization is performed.

Each stage owns its ring position and valid-history count. Reset, panic, exhausted
tails and faults invalidate history in constant work per stage; they never clear or
free a long ring on audio. Slot reuse cannot read old contents. Ordinary declared
voice/bus tails retain their existing owners, including silent gaps before a first
echo. A bus tail retains its generation but does not retain retired host note IDs.

`tests/delay.rs` compares stereo feedback and cascaded pre/post/bus processing with
an independent whole-timeline recurrence, overlapping voices, delays 1/3/67 and
blocks 1/7/64/129. It verifies silent-gap plan replacement, panic/slot reuse, overflow
containment, invalid feedback and checked capacity under the heap guard. The pinned
Shortcircuit [bus ringout implementation](https://github.com/surge-synthesizer/shortcircuit-xt/blob/8785f09acd9f93682ce4f754fac1d3c62e5b1a9a/src/scxt-core/engine/bus.cpp#L98)
was reviewed for processor lifetime and silent-gap handling; no code was copied.

Fractional/modulated delay, tempo synchronization, feedback-path filtering,
diffusion and Kontakt/Falcon delay parameter/sonic profiles remain open. This exact
integer-delay kernel is not a complete vendor delay model or an interpolation-quality
claim. Full native MSRV and strict all-target Clippy checks pass; logs use
`artifacts/delay-*`.


## Sample-clock state-variable filters

`Processor::StateVariable(StateVariableFilter)` supplies native low/high-pass,
unity-peak band-pass, notch and all-pass responses. It uses trapezoidal integration
with two f64 integrator histories per channel, following the public-domain
[Cytomic SVF derivation](https://cytomic.com/files/dsp/SvfLinearTrapOptimised2.pdf).
These histories occupy the existing processor state cells and inherit voice/bus
reset, tail and fault containment. Static biquads retain their separate algorithm.
This is not a proprietary filter model.

`cutoff_hz` and `q` each accept `Parameter::Constant` or `Parameter::Control`.
The existing gain-only `GainControl` API was replaced directly by `ControlRange`;
gain and filter destinations now share the same prepared bindings, control owner,
sorted fan-out index and absolute-time ramps. No alias or second control bank was
added. Cutoff ramps linearly in hertz and resonance in Q, with independent authored
sample lengths. Coefficients are recalculated from these parameters, never linearly
interpolated as biquad coefficients. Adapters still own source frequency curves,
resonance conventions and smoothing rules.

Preparation validates the complete endpoint ranges: cutoff strictly between zero
and Nyquist, positive Q, finite mappings and positive finite representable filter
coefficients at every endpoint pair. Unknown control IDs and schema replacement
that removes a referenced ID fail before runtime publication. Controls remain
available without a UI and receive direct, queued, scheduled and script edits
through their existing services.

Each authored filter has one lazy 64-sample coefficient window in its retained
generation, shared by voices referencing that chain. The render scheduler segments
these plans into at most 64 samples and resets cache validity at every event boundary.
Coefficients are calculated once per needed filter/sample; unchanged parameters reuse
the last coefficient tuple. Inactive chains perform no coefficient calculations.
Audio history is never shared between voices. The window does not introduce latency
or reduce automation to block rate. Buses use the same kernel with separate caches
and summed-signal histories. Replacement generations own independent trajectories,
coefficient windows and audio histories.

Evidence includes an independent implicit-integrator solver for all five responses,
cutoff ramp reversal, simultaneous Q/cutoff events, two overlapping voices, voice/bus
scope, blocks 1/7/64/129, old-generation scheduled edits, static edge frequencies at
44.1/48/96 kHz, invalid ranges and missing control IDs. A separate impulse-response
check verifies analytic DC/cutoff/Nyquist targets. Audio paths remain heap guarded.
Full per-note modulation graphs, LFO/envelope destination routing and Kontakt/Falcon
parameter/audio equivalence remain open.

`render_workloads --svf COUNT` and `--bus-svf COUNT` exercise moving cutoff through
1–16 filters. Each callback retargets a 256-frame ramp; control submission, warmup,
output validation and sorting are outside the timed renderer. Every block is checked
against a settled constant-input oracle. This isolates renderer cost under parameter
movement; it is not a transient/sonic or competitor benchmark. Raw local observations
are `artifacts/svf-{voice,bus,biquad-baseline,voice-inline}.csv`; native checks use
`artifacts/svf-*`.


The first local four-filter voice run measured 4,417.911 us median at 1,024 voices,
48 kHz and 64 frames. Inlining the measured per-sample kernel reduced the follow-up
median to 2,647.838 us (p99 3,395.853 us), still over the 1,333.333 us deadline. The
same-run four-static-biquad baseline also missed that deadline (1,936.326 us median).
Four automated bus filters measured 50.661 us median / 62.221 us p99 for the same
voice count and block size. These are unpinned local observations with uncontrolled
scheduling/frequency, not a demonstrated worst case or an interchangeable routing
optimization. Production per-voice throughput and competitor performance gates remain
open; no quality reduction or movement of authored voice filters to buses is implied.


## Note-expression filter destinations

`Parameter::Expression { source, low, high }` maps retained 32-bit pressure or timbre
to cutoff hertz or Q. Endpoints undergo the same full-range filter validation as
constant/control parameters. The event-rate expression mapping is immediate; it
does not invent a smoothing law. A filter may combine an expression-mapped cutoff
with a ramped shared-control Q, or map both fields from expression. General additive/
multiplicative modulation graphs and expression smoothing remain open.

Coefficient banks distinguish generation-shared bindings from expression-owned
bindings. Only expression-dependent filters reserve per-expression windows, sized
by `Limits.expressions` with checked multiplication/layout and fallible reservation.
`PlanControl` captures that same limit for off-audio replacement preparation. Lazy
windows are indexed by the native expression slot and tagged with its full generational
handle; MIDI key/channel is never an owner key. Linked notes share coefficients,
snapshots and detached notes use their own entries, and audio histories remain
voice-local. A recycled expression slot cannot reuse a previous owner's window.
Generation-shared filters do not multiply their coefficient memory by polyphony.

Summed buses reject expression-dependent parameters because they have no unique
note owner. The system does not silently pick the latest voice or collapse per-note
filters into one shared response. Both real MPE gestures and direct native expression
updates reach the existing note owners; no second expression state or adapter-specific
DSP path is introduced.

Native fixtures compare four simultaneous notes/children with independent solver
histories through link/snapshot/detach, full-resolution endpoint/intermediate values,
slot reuse, replacement and bus-scope/capacity rejection. The real UMP/MPE fixture
compares rendered PCM to explicit native expression events while a released note's
member channel is reused, including a simultaneous shared Q ramp. Release tails keep
the old member's captured expression. Runtime work is heap guarded; full native MSRV
and Clippy plus core/MIDI release checks pass (`artifacts/expression-filter-*`).

Per-expression windows cost roughly two KiB per dependent filter per configured
expression slot, in addition to metadata; production resource admission must budget
this. The current scalar voice kernels still miss high-polyphony deadlines. Local
shared-control workload reruns are retained in `artifacts/expression-filter-shared.csv`;
this work establishes correct scope and execution, not performance superiority.
