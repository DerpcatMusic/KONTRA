# Shared modulation and DSP CPU plan

Owner: W6. Companion to [the approved shared design](MODULATION_V2.md).
This is an implementation plan, not measured performance.
The numerical ceilings below are proposed acceptance budgets on the shared
benchmark machine; v1 and native results are UNKNOWN until the quiet probes run.
The existing GDB stage proportions cannot establish nanosecond costs.

## One prepared execution plan

Kontakt and Falcon lower onto the same control graph, route reducer, smoothing
lanes and DSP scheduler. A source profile supplies units, native laws and any
specialized kernel; it does not own another callback loop. Reuse the shared
engine-parameter service and existing sampler-core/sampler-simd mechanisms.
Existing engine targets resolve through EngineParameterBinding; generic
processor/source/route registry addresses share prepared ControlId/dense lanes
without a native-name-table entry. Native bindings are aliases to the same owner,
with no second loop or value mirror. Preparation resolves addresses to dense indices and
checks scope, domain and rate; render evaluation performs no name/hash lookup or
registry scan. Typed event admission retains the shared service's validation.
Structural edits prepare a replacement off audio rather than resize a live cascade. Unsupported or invalid bindings remain explicit errors.

Prepare flat dependency spans from source to route to target to coefficient
block. Propagate dirty revisions in the prepared graph order, including depth,
shape, meta-modulated source parameters and explicitly delayed cycle edges.
Active phase/envelope/lag nodes mark their dependent lanes dirty at their declared
cadence; keep a bounded active set and prepared bitset/work list, with no callback
allocation or repeated scan of unrelated nodes. Reduce contributions in saved
order, then perform the lawful destination conversion once per changed target. Broadcast shared plan/group sources only
within their declared scope; voice-retriggered sources retain independent state.
Target identity, source profile, program generation and state ownership are part
of reuse. W15's native-address registry and scratch projection must be included
in measurements and compiled away from repeated whole-registry scans.

## Evaluation rates and smoothing

The modern default control cell is 64 absolute runtime frames (750 Hz at 48 kHz),
independent of host block partitions. Verified native cadence overrides it: v1
filter.rs::CONTROL is 32 frames, so native filter comparisons must retain and
account for that cadence rather than claim a saving by halving updates. Events
retain their exact timestamps and split work where the established law requires
it. Source clocks advance once per cell at their own scope; route fan-out never advances an LFO or envelope again.

A block endpoint plus a declared per-sample ramp is sufficient only when it
passes the target's response/error tests. Gain, pan and pitch have separate unit
and smoothing laws; native profiles retain their verified behavior. Fast LFOs,
sharp custom shapes and modulation that needs audible sidebands use explicit
audio-rate source/target lanes. A faster source cannot be reduced to two endpoints
merely to satisfy a CPU budget. Account for those lanes separately.

Custom shapes are validated and compiled off audio into bounded lookup/polynomial
representations; retain a higher-quality reference for error measurement. Choose
band-limited representations where required by the allowed waveform/rate.
The callback does not search an arbitrary-length knot list or build a table.
Only changed targets run smoothers, and each smoother stops on its specified
settled condition. Reuse coefficient blocks while their controlling values,
rate and topology remain identical. Interpolate only a representation whose
stability has been established for the filter family; arbitrary coefficient
interpolation is not assumed safe. Audio-rate coefficient updates are a separate
benchmark class, including their nonlinear/conversion cost.

## Settled-value reuse

Port v1 0cb7a8a0 src/engine/voice.rs::Voice::plan and
src/engine/params.rs::Mods::modulate input-stamp/settled-result reuse. A prepared
dependency revision marks only affected lanes dirty. A constant source and a held
controller incur no repeated route transforms, exp/log conversion or coefficient
preparation. Keep cached outputs and transition endpoints valid: after a changed
cell, the following held cell must settle its previous endpoint exactly once.
Do not replay the prior ramp. Note onset, changed scope/seed, route depth/shape,
program replacement, sample rate and topology invalidate their dependencies.

The existing W6 port 59a86c89 reuses raw source values only for clockless,
zero-lag programs and preserves addressed-filter endpoints. It is a conservative
starting point, not the complete dependency-revision implementation. Moving clocks,
release counters, random-cycle sources and active lag still advance. DSP delay,
filter and feedback state always processes audio; settled coefficients do not
mean a settled filter output. Do not introduce an unverified epsilon to skip work.

## SIMD and callback storage

Use the existing lane layout across independent voices/channels with compatible
kernel topology, preserving route reduction and final voice-sum order. Cascaded
sections of one voice depend on their predecessors; vectorize independent lanes
at each section, not those dependent sections as if they were independent.
Share/broadcast identical coefficients when scope permits; retain separate
integrators and release tails. Partial vectors and scalar tails preserve exactly
the same state clocks and finite guards. Dispatch the CPU backend outside the
per-sample loop. Group compatible kernels into stable batches; dispatch one block
entry over borrowed audio/state/parameter spans, including a batch entry for
adapter kernels where available. Do not add an indirect call per sample, section
sample or route. Preserve serial cascade dependencies and source scope. Approximate
math or FMA changes need their own fidelity evidence.

Adapter dispatch is conditional, not an architectural performance waiver. Benchmark
an enum/direct entry and an indirect adapter entry wrapping exactly the same kernel,
state layout and parameter law. Quiet paired A/A/B cells use gain, one SVF section,
a long cascade and a nonlinear native kernel, both homogeneous and mixed kernel
batches, plus SIMD-width-minus-one/full/plus-one voice counts. Include packing,
lookup/dispatch, tails and state writes in end-to-end block p50/p99. Check both
32/64/256-frame blocks and changing/held parameter lanes at 48/96 kHz; keep
backend and optimizer settings identical, and consume audio/state to prevent dead
code elimination. Collect batch counts and actual calls outside timing.

Adopt the indirect entry only if every comparable cell has no p50 or p99 regression
against the paired enum baseline and outputs/state/heap behavior match. Establish
the run's noise envelope with bounded A/A before comparing B; retain raw receipts.
A regression or inconclusive result keeps enum dispatch for that family. Enum
fallback selects kernels; open addresses and data-driven native laws remain shared.
Built-in fast paths remain direct. This dispatch A/B is separate from the frozen
v1 per-route/per-section comparison and whole-plugin acceptance.

Preparation allocates every source/route/target lane, coefficient block, section
state, smoother and scratch buffer for the configured maximum voices and admitted
graph. Processing, note onset/release, automation, topology handoff and retirement
must perform zero allocations and zero deallocations, locks, waits, I/O or logging.
Reject capacity overflow before publishing a replacement; do not drop routes,
truncate slopes or allocate when the callback discovers an oversized graph.
Record prepared memory separately from active callback work.

## Proposed measurable budgets

A route is one source-to-target contribution at one scope. A section is one
linear two-pole stage processing one stereo voice frame; one-pole and nonlinear
stages are reported separately. Timing includes lane packing and state writes.
Source evaluation and coefficient preparation are separate costs, never hidden
in a claimed cheap route. Absolute ceilings and relative comparisons both apply
where a semantically matching v1 case exists.

| Work unit | Proposed p50 ceiling | Required v1 comparison |
| --- | ---: | --- |
| Changed affine route / control cell | 40 ns | <= 0.8 times matching v1 changed-route cost; p99 <= 0.9 times v1 |
| Held route / control cell | 4 ns | whole held-source/reuse path p50 and p99 <= 0.9 times v1 held path |
| Shared custom-table source / control cell | 32 ns | <= 0.8 times matching v1 shape where available; otherwise no v1 parity claim |
| Changed linear-section coefficient block / cell | 64 ns | <= 0.8 times matching v1 preparation; p99 <= 0.9 times v1 |
| Linear two-pole section / stereo sample | 4 ns | <= 0.8 times matching v1 section processing; p99 <= 0.9 times v1 |
| Active affine smoother / sample / target | 1 ns | <= 0.8 times matching v1 smoother where available; settled work removed |
| Audio-rate, nonlinear or specialized section | measured family budget | p50 <= 0.8 times matching v1 family; p99 <= 0.9 times v1; full callback deadline still applies |

At 48 kHz / 64 frames, the callback deadline is 1.333333 ms. A proposed stress
case of 128 stereo voices, 16 changed affine routes, one table source, six dirty
linear sections and two active affine smoothers per voice has a 348160 ns
control/filter budget from the table, 26.112% of that deadline. Set its measured
combined p50 ceiling to 30% of the deadline. This example excludes sample
transport, resampling, other FX, scripts and host dispatch; measure all of them
in the full callback. Do not add independently measured percentile values and
call their sum a callback percentile. The full callback p99 target is below 80%
of its deadline with zero underruns, in addition to beating v1 in every admitted
comparison cell.

Cost scales with actual sections, not a nominal filter label. For example, a
nominal 90 dB/octave integer-pole cascade uses seven two-pole stages plus one
one-pole stage; benchmark both costs and their serial processing. Finite-band
or fractional-order approximations must report their achieved slope/error and
additional stages. No universal constant-time arbitrary-slope claim is made.

## Measurement and quality gates

1. Use paired/interleaved quiet runs on identical machines, rates, block sizes
   (32/64/256), voice counts, source schedules, native/control cadence, topology
   and output workload.
   Freeze exact binary/source/profile hashes. Keep original frozen v1 binaries
   untouched; any missing kernel-only probe requires a separately approved,
   probe-only frozen reference with provenance. A source-only estimate is UNKNOWN.
2. Sweep routes 0/1/4/16/64, sections 0/1/2/4/8/16 and voices 1/8/32/128;
   sweep one-source fan-out versus independent sources. Fit per-cell p50 and p99
   slopes versus actual evaluated routes and section-frames, retaining fixed
   overhead and raw distributions. Keep held, every-cell changed, audio-rate,
   complex-shape, smoothing, onset/release and long-tail results separate.
   Nonpositive or unresolved slopes under measurement noise are UNKNOWN, not 0 ns.
3. Compare v1 only for equivalent available source/filter laws. New topology,
   fractional slopes and custom shapes without equivalents get absolute budgets
   plus comparison to the high-quality reference. Budget failure triggers
   optimization or an explicit quality/capacity decision, never silent route loss.
4. Require heap-guard allocation/deallocation counts of zero on all exercised
   callback paths. Run scalar/SIMD output-and-state comparisons, block partitions
   1/7/32/64/256, high-Q sweeps, discontinuous/fast shapes, impulses, release tails,
   automation and program swaps. Pure hot-path ports retain bit-exact output;
   changed quality algorithms need declared numeric and spectral tolerances.
5. Measure the combined current Kontakt/Falcon core, including W15 registry
   scratch, W6 reuse and actual native/specialized kernels. Retain total engine
   work for worker-offloaded Lua/KSP as well as callback wall time; moving work
   between threads does not establish lower total CPU. Final native claims need
   the same authored graph/readback, active voices, tails, quality, rates and
   script workload in Kontakt/Falcon. v1 parity is not native-host parity.

Current evidence: W6's prior GDB profile and three bit-exact PCM witnesses locate
work and establish preservation on the old base, not these budgets. Quiet v1 and
native CPU acceptance for this plan remains open.

