# Prepared note-expression modulation

This is the first audible native modulation scope, not the complete modulation
or DSP graph. It implements event-updated logical-note expression as required by
section 10 of the preserved architecture brief. Native and imported instruments
will use the same service; no vendor or old-core dependency is involved.

## Authoring and execution contract

`Modulation::new(routes, max_routes)` validates an explicit authored-route budget
off audio and removes identity mappings. `Prepared::with_modulation` binds the
program to an immutable instrument generation. Each route reads full-resolution
canonical pressure or timbre, normalized from `u32` only during evaluation.
Destination variants define units and combination:

- `LinearGain`: interpolate endpoints in 0..1, then multiply note gain.
- `StereoBalance`: interpolate endpoints in -1..1, add to note balance, and saturate
  the final sum to -1..1. This retains the core's stereo balance law.
- `PitchSemitones`: interpolate finite semitone endpoints and add to note pitch.
  Actual source-rate limits still apply; pitch is never silently clamped.

Endpoints may be reversed; routes execute in authored order. Computed nonfinite
values reject the operation. The program is a bounded prepared route list, not a
general graph compiler. Updates are immediate at exact event boundaries. There is
no implicit smoothing, audio-rate source approximation, or host-block quantization.
The current gain-factor domain only attenuates; amplification and more destination
units need their own explicit contracts.

One note-scoped program applies per prepared instrument. Voice/layer/bus bindings,
curves, envelopes/LFOs as graph sources, control/audio-rate execution and destination
ramps are still open. Those need explicit state scope and rate; they must not be
smuggled into this event-rate evaluator as per-block updates.

## Ownership and admission

Canonical `Expression` values remain unchanged and publicly observable. Each
expression owner caches projected stereo gains, semitone pitch and source ratio.
Rendering reads that cache rather than traversing routes or calculating transforms
for every voice/sample. Linked notes share the cache; snapshot/detached owners copy
it; independent children evaluate default input against their original plan.

A nonempty program is identified by its retained `PlanId`. Counted notes already
retain that plan, so program lifetime follows existing ownership without another
reference-counted runtime object. New plan adoption affects new roots; old tails,
children and expression changes keep the original program. Empty programs have no
program lookup, and unchanged unmapped destinations reuse cached values.

Initial source selection, immediate expression, queued expression, multi-owner
batches and later source admission all validate *projected* pitch. Pressure/timbre
routes therefore cannot bypass rate constraints merely because raw pitch stayed
unchanged. Accepted queued changes constrain later voices before publication. A
failed gesture changes neither canonical nor cached expression. Batch scratch is
allocated with the expression arena, and occupied voices are checked once per batch.

Zero gain does not retire or rewind a source. Its cursor/envelope continue through
the [silent advancement path](RESAMPLING.md), which skips unnecessary PCM/filter
work, so later expression restores the correctly advanced sound. All preparation/destruction stays
off audio; event projection, rendering and retirement use existing bounded storage.

## Evidence and remaining costs

`sampler-core/tests/modulation.rs` checks exact PCM against independently supplied
projected expression across 1/7/64/256-frame partitions; projected initial, immediate,
queued and batch rate failures; pending-change constraints on later sources;
linked/snapshot/independent inheritance across plan adoption; terminal backpressure;
balance saturation; nonfinite projection; route budgets; and adjacent 32-bit pressure
values reaching distinct audio levels. Actual operations run under heap guards.

`sampler-midi/tests/mpe.rs` also drives pressure and CC74 through real UMP packets
into prepared gain/balance/pitch mappings. It checks analytic PCM after muting and
unmuting, continuity of source position, and preservation of raw input expression.
This establishes an audible native MPE path, not Falcon/Kontakt import conformance.

The gesture workload accepts `--modulated` for three routes. Initial measurements
exposed additional empty-program overhead; the current path avoids unnecessary
lookup/normalization and caches unchanged destinations. Larger owner/cache and
transaction-scratch records still have a measured cost compared with the earlier
pitch-only receiver. Logs and timing iterations are retained under ignored
`artifacts/modulation-*`; performance claims must name the workload and executable.

Three paired CPU-2 unmodulated resident-render runs against the preserved `49a3b84`
renderer measured a median configuration speedup of 1.026, with configuration
medians ranging 0.954–1.083. This is a historical baseline across accumulated changes,
not an isolated attribution to caching. An individual timing ratio reached 0.520
amid scheduler outliers; these process measurements do not establish realtime
maximum latency. Filtering cost, dense control traffic, route budgets and audio
quality remain separate workload requirements.

The final local gesture run measured 1024-note medians of 19.28/14.98/15.02
microseconds for pitch/pressure/timbre with no routes, versus 14.99/10.11/9.90 in
the preserved pre-modulation receiver. With the three-route program the medians
were 28.49/20.21/28.45 microseconds. That overhead is retained explicitly in the
workload record; no regression-free controller-cost claim is made. At 64 notes the
three-route medians were 1.73/1.19/1.76 microseconds. Source filtering is not included.

All four new crates pass release tests, strict all-target Clippy and Rust 1.92
checks; the root v2 integration boundary also passes. No Doctor rescan was run.


## Expression-driven filter parameters

Prepared voice filters now also accept pressure/timbre through
[`Parameter::Expression`](VOICE_DSP.md#note-expression-filter-destinations).
The event mapping reads the existing retained expression owner at its full 32-bit
resolution; filter coefficients share work by expression identity while stereo DSP
history stays per voice. This is a separate explicitly voice-scoped destination,
not an extension that changes the existing gain/balance/pitch projection contract.
Native link/snapshot/detach and MPE released-member reuse checks cover audible output.
Full modulation graphs and source-profile parameter laws remain required work.

## Per-voice modulation programs

`crates/sampler-core/src/voice_mod.rs` executes voice-scoped modulation lowered
from the IR (`Zone::routes`, `Instrument::routes/shapes/modulators`). It is a
second scope beside the event-rate note projection above, with its own rate.

- **Sources**: LFOs (sine, triangle, square, saw up/down, sample-and-hold, random
  ramp; Hz or beats at `Runtime::set_tempo`; start phase, delay, fade-in;
  retriggered per voice or free-running on the runtime clock), extra
  delay-attack-hold-decay-sustain-release envelopes gated by the voice's family,
  velocity, key, controller (effective performance CC), note pressure, note
  timbre, per-voice random and constant, breakpoint envelopes (Kontakt flex),
  and the release-trigger counter `clamp(1 − held / T, 0, 1)` (`held` from
  admission to key-up, or to now while the key is down; Kontakt manual,
  Source module "T"). LFOs are bipolar, the rest unipolar.
- **Route pipeline**: invert (unipolar `1 − v`, bipolar `−v`), piecewise-linear
  shape, one-pole lag reaching 99% in the authored time (Kontakt's lag law).
  An optional route scale multiplies the depth by `shape(x)` of a second
  source's unipolar value (modulator × modulator products such as Falcon's
  LFO depth by mod wheel or ratio by key).
- **Kontakt shapers and invert**: an enabled shaper replaces the invert flag
  (Vista Full Strings stores shaped crossfade copies differing only in the
  flag that must play alike; v1 corpus audit). Breakpoint shaper segment
  curvature (-1..1) has no known law: segments are linear and each curved one
  is reported as UnknownLaw.
- **Controllers** start at the MIDI RP-015 reset state (CC11 expression full),
  so expression-to-volume routes are identity until the controller moves.
- **Targets and laws**: attenuate `gain × (1 − d(1 − u))` (Kontakt volume),
  decibels `gain × 10^(d·v/20)`, pan `+ d·v` (balance, saturated), pitch
  `+ d·v` semitones, chain SVF cutoff `× 2^(d·v/12)` and Q `× 10^(d·v/20)`, a
  per-voice tone low-pass that closes `d·v` semitones below 0.45·rate when the
  sum is negative (bypassed at 0), and sample start `+ d·u × start_range`
  evaluated once at voice start.
- **Rate**: control points sit on the runtime's absolute 64-frame grid
  (`dsp::BLOCK`; render chunks end on it whenever any plan carries programs)
  plus the voice onset. A segment entering a grid cell evaluates every source
  and route once at the cell end; later segments in the cell reuse it, so
  output is identical for any host block partition. Gain and pan ramp
  linearly between points; pitch, cutoff, Q and tone hold the cell midpoint.
  Modulated pitch is clamped to the resampler's step range. There is no
  per-sample enum dispatch.
- **Memory**: programs are flat boxed slices; per-voice state (LFO phases,
  envelope states, lagged route values, last control point, tone integrators)
  is structure-of-arrays sized at plan preparation for the largest program and
  the voice limit. Rendering and starting voices never allocate (heap-guarded
  tests in `tests/voice_mod.rs`).
- **Cutoff/Q** reach the voice chain through `FilterBank::modulation`, which
  bypasses the shared coefficient cache only for a modulated voice. Lowering
  accepts cutoff/Q routes only when the zone chain has exactly one 2-pole SVF.

## Native MPE defaults

`lower::Options::default()` adds to every zone: note pressure → `+6 dB` gain at
full pressure, and timbre → the tone low-pass, open from centre (CC74 64) up and
closing to 60 semitones below open at CC74 0. Per-note pitch bend is the note's
native expression bend (sampler-midi member-channel or plain channel bend; the
authored depth becomes the plain-MIDI bend range), so IR pitch-bend →
pitch routes are not lowered as modulation. All defaults are identity at rest;
`Expression::default().timbre` is centre. `Options { mpe: None }` (and
`sampler_kontakt::Options::mpe`) turns them off.

Measured on the ignored `measure_modulation_cost_per_voice` (64 looping
resampled voices, release build): plain voice 24.7 ns per voice-frame; with the
pressure route 26.5; with a closed tone filter 31.3; an SVF voice chain 32.3,
plus LFO pitch/pan, envelope gain and LFO cutoff 35.0.

## Measured Kontakt segment and crossfade laws

Not in the Kontakt manual (KONTAKT_Manual.pdf states no crossfade, shaper
curvature or flex-curve law); measured on Kontakt 8 by the reference harness
(docs/architecture-v2/KONTAKT_REFERENCE.md on v2/kontakt-reference).

- **Shaper segment curvature.** A breakpoint's stored curvature `c` bows the
  segment to the next point: `k = 17 |c|`, `e(t) = (e^kt - 1)/(e^k - 1)`; the
  segment is `y0 + (y1 - y0) e(t)` when `c > 0` and it rises or `c < 0` and it
  falls (below the chord), else `y0 + (y1 - y0)(1 - e(1 - t))` (above).
  Vista 3 Cellos group 37 swept over CC100 (0..96): 0.08 dB RMS residual with
  K = 18 after a constant, K = 17 is the shared compromise (Una Corda "Depth"
  GUI reading implies about 15). Expanded to 16 linear pieces per curved
  segment in `crates/sampler-kontakt/src/library.rs` (`curved_shaper`).
- **Flex envelope segment curve.** Stored `s` (0..1, 0.5 linear), `c = s - 0.5`,
  `k = 17 |c|`; `c > 0` starts fast (`Exponential(-k)`), `c < 0` slowly.
  Measured on rising 0..1 segments only; falling segments assume the same
  progress function.
- **Zone crossfades.** Linear amplitude ramp in integer steps with a nonzero
  first step: a fade-in of `F` over low edge `L` has gain `(v - L + 1)/(F + 1)`
  for `L <= v <= L + F`, the fade-out mirrors it `(H - v + 1)/(F + 1)`; key and
  velocity alike, multiplied. Not equal-power. `ir::Fades`,
  `Prepared::with_zone_fades`. The mapping field order (low velocity, high
  velocity, low key, high key) is from the v1 importer; the velocity pair is
  confirmed on the installed libraries (a 1..40 zone fades out, upper zones fade
  in), no installed library uses the key pair.

## Monophonic release note

See RELEASE_CONTEXT.md.
