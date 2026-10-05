# Resident fractional playback and rate conversion

The native source renderer now combines the asset/output sample-rate ratio with
`Playback::transpose_semitones` and optional `Region::root_key` tracking.
Prepared assets may have different rates from the
runtime. Preparation and direct source admission both validate the resulting step;
invalid, nonfinite and unsupported ratios fail before taking a voice slot or command.
Prepared regions retain fresh cursor templates rather than authoring playback objects.
The prepared key-candidate index stores each selected key's complete playback step.
`root_key: None` is fixed pitch; `Some(key)` adds key displacement and optional
native tuning offsets described below.
Each step is computed directly from authored tuning and key displacement, preserving
exact unity at the root rather than multiplying rounded adjacent-key ratios.
Every mapped key must satisfy the declared rate bounds; an unmapped root need not.
The candidate budget now bounds records containing a region index and an f64 step
(16 bytes per candidate on 64-bit targets). Logical playback keys select these
records; physical input addresses remain unchanged for key-up and terminal pairing.
Ordinary key selection copies the template and compiled step into a voice; it does not repeat `exp2`, rate conversion
or source-view validation at note-on. Absolute-pitch selection, described below,
calculates the overridden step at admission. Both prepared selection and explicit manual
source admission use one bounded voice-reservation function.
The current supported step is 1/256 through 16 source frames per output frame.
This is a declared implementation limit, not silent clamping or quality fallback.

## Position and boundary contract

Each voice stores an integer traversal distance and a separate fractional phase.
Advancing a long sample never converts its absolute position to floating point.
Forward and reverse playback use the same increasing traversal coordinate; their
mapping to immutable source storage differs. Stereo channels share this phase.

The traversal maps into the initial source prefix, repeated loop, and optional
post-release tail. Until-release loops record the next boundary at which wrapping
ends. Earlier repeats remain addressable to the filter's past taps after release.
An exact-boundary release skips the pending wrap, while a release between fractional
positions finishes the current iteration before reaching the source tail. Filter
lookahead follows the current loop policy; a future, not-yet-received release is
not predicted. Out-of-view taps are zero, not clamped to unrelated PCM.

Playback ends when its center reaches the exclusive traversal end. It does not
append an unrequested FIR tail beyond the source view. Envelopes run in output
frames and may end a voice earlier. Held physical inputs still retain ownership
until their key-up and accepted terminal. The cursor stops safely on integer-clock
exhaustion; no wrapping address can access unrelated source storage.

## Filter and real-time work

The first measured filter is a Blackman-windowed sinc with radius 48 and cutoff
0.45 cycles per source frame at steps <=1. For downsampling, the kernel is widened
by the step and the cutoff lowered by the same ratio. Its continuous response is
stored at 1,024 table intervals per source-frame unit and linearly interpolated.
The inner loop traverses table coordinates with Q32 integers, avoiding a floating-point
index conversion at each tap; source phase remains independently represented.
The finite-phase coefficient sum is normalized for DC; zero-padded boundary taps
remain in that denominator. This is linear interpolation of **filter coefficients**,
not two-sample linear interpolation of PCM.

A process-wide immutable table occupies 196,612 bytes. Runtime construction prepares
it through `OnceLock` off audio and retains a shared reference; the callback never
initializes, locks, allocates or destroys it. Exact unit step at integer phase uses
the direct-read renderer and retains existing arithmetic. Other ratios use 97 to
1,537 taps per output frame. Interior windows read contiguous PCM directly; boundary
windows resolve the explicit traversal. All work is bounded independently of the
number of loop repetitions, including one-frame loops.

The high-step direct filter remains expensive. This is correctness and measured
performance groundwork, not a completed production resampler or a claim of maximum
polyphony. Future optimization must preserve loop guards, release changes, passband
and stopband behavior; asset-wide downsampled copies alone cannot represent every
region's independently authored loop boundary correctly.

Algorithm reference: Julius O. Smith's [windowed-sinc interpolation treatment](https://www.dsprelated.com/freebooks/pasp/Windowed_Sinc_Interpolation.html)
explains the ratio-dependent low-pass requirement and coefficient-table approach.
The implementation and authored fixtures are original; no implementation source
was copied from a vendor sampler.

## Executable evidence and remaining work

- Analytic quadrature tones check pitch and phase at 44.1/48/96 kHz and static
  transpositions; regular and irregular partitions produce identical PCM.
- Independent direct trigonometric FIR calculations check fractional forward/reverse
  loop exits, guard history and EOF, including exact and fractional release boundaries.
- Kernel tests cover DC, passband amplitude/phase and output-Nyquist attenuation at
  multiple phases and steps including 1/256 and 16. Separate rendered tones above
  output Nyquist must remain below amplitude 0.0002 (about -74 dB). These sampled
  points are regression bounds, not a full-band certification.
- Allocator instrumentation covers trigger, scheduled release, render and retirement.
  A cursor test retains quarter-frame motion beyond integer position 2^54.
- Existing unity source sequences, envelope, behavior, MIDI, plan-transfer and native
  process checks remain required. Release and Rust 1.92 checks cover the new path.

Run `cargo test -p sampler-core --test resample` and the core unit tests. The resident
workload accepts `--transpose SEMITONES` for a smaller 4/16/64-voice filtered workload;
its normal invocation remains the original unity workload. Exact binary DC output
is checked outside timed callbacks. Local evidence uses ignored
`artifacts/architecture-v2/resample-*`.

Pitch ramps, broader rate
ranges, quality tiers, streaming demand windows, ping-pong and crossfade loops remain
open. The host plugin and UI have not been switched to this core. Imported formats
will lower into these native source and expression contracts rather than selecting
another format-specific playback engine.

## Local performance evidence

The same Ryzen 7 7800X3D / Rust 1.99 release / CPU-2 workload used for the earlier
renderer was run with no builds or scans in parallel. Three paired unity-workload
runs against `e700dcc` measured a median before/after speedup of 1.027 across the
32 configurations (range 0.977–1.113). At 1,024 voices and 256 frames, measured
medians were 71.6–76.4 microseconds across the four rate/envelope cases. Separating
the filtered loop from the inlined direct renderer removed the initial roughly
10% median direct-path regression. Raw final pairs are `resample-split-*.csv`.

The filtered prototypes demonstrate the remaining cost rather than hiding it:
at +7 semitones, 64 voices / 256 frames initially took about 10.0 ms median; direct
interior windows plus fixed-point coefficient traversal reduced it to about 3.84 ms.
The corresponding 48 kHz block lasts 5.33 ms, and observed scheduling outliers still
exceeded that duration. This is not sufficient evidence for a production polyphony
limit. Raw stages are `resample-pitch-*`, `resample-span-pitch-*` and
`resample-fixed-pitch-*`; the filter shape and numerical acceptance bounds were not
reduced to achieve that improvement. Wider ratios remain proportionally expensive.

## Live note-scoped pitch

`Expression::pitch_semitones` now affects resident playback. Every voice retains
its immutable admitted base step (asset/output rate, authored tuning and logical
key tracking). Rendering combines that base with a cached ratio on the expression
owner. Pitch changes do not restart the source, clear fractional phase, rewrite PCM
or change physical input identity. Returning to unit step at a fractional position
continues through the filter rather than snapping onto a source sample.

Immediate and scheduled changes validate the resulting step for every admitted
voice sharing that expression owner, including delayed starts. Invalid changes
return `InvalidInput` without changing the expression or taking a command slot.
Later source admission validates its base step against both the current expression
and all queued expressions currently targeting that owner. This reciprocal check
keeps a previously accepted scheduled change executable after later admissions.
Detachment only splits an owner's note set; linked child admission checks the same
pending constraints, while snapshots inherit the current value only. Generated
selection checks these constraints before creating a child or family, so a rejected
Play produces a normal retained behavior fault without partial source ownership.

An expression value may be retained on an owner without audio even when no supported
source step can currently realize it. Source admission then fails explicitly. This
preserves the canonical expression value without silently clamping it or selecting
a lower-quality source path. Usual gate cancellation still cancels pending events;
a behavior fault closes its gate and follows that existing cancellation policy.

The ratio is computed when expression changes, not inside the sample loop. Unchanged
pitch avoids the voice validation walk. Changed pitch visits the occupancy bitmap
and admitted voices, skipping empty voice storage. Source admission visits the
bounded pending queue. Dense modulation/admission costs still need workload evidence
before assigning production budgets or introducing further indices.

New heap-audited checks compare analytic phase across +12/-12/zero-semitone changes,
including return to unity with a retained half-frame phase, with block sizes
1/7/64/256/512. Other checks cover linked/snapshot/independent inheritance, physical
channel reuse, delayed sources, failed immediate and queued changes, detach, and
transactional generated-source rejection. Existing gain/pan inheritance checks
remain at unity pitch; the separate audio checks establish actual pitch behavior.

These native services now support the separate [fixed-zone MPE adapter](MIDI_INGRESS.md),
including member-channel reuse and frozen released-member expression. Full MPE/MIDI
2.0 receiver behavior and raw-versus-consumed scripting stages remain pending. Vendor
imports will use that shared service rather than defining the engine's expression
limits. Pitch ramps and external tuning protocols remain separate pending work.

The unmodulated resident workload was repeated after live pitch was connected:
three paired CPU-2 runs against the preserved pre-expression renderer measured
median speedup 1.013 (individual configuration ratios 0.955–1.089). This checks the
steady unity path, not dense MPE traffic. Logs, CSVs and hashes use `live-pitch-*`.

Initial expression can now be supplied to `note_on_with_expression` and
`trigger_with_expression`. The existing entry points use the default expression.
The supplied value belongs to the note before source selection or a bound program
runs, so immediate snapshot children inherit it without a default-valued first
frame. Plain prepared selection validates all source rates before publishing any
ownership. A bound program still reports its own later source failures through
the normal retained behavior outcome; admitting a program is not a promise that
every generated source will succeed.

The heap-audited regression compares direct playback with an immediately generated
snapshot child, changes the parent before rendering, and checks analytic pitch,
gain and stereo balance. It also verifies exact 32-bit pressure/timbre retention,
invalid initial values, failed source-rate admission, and complete cleanup.


## Prepared native tuning

`Tuning::new([f64; 128])` validates a native table of finite semitone offsets from
each logical key's nominal twelve-tone equal-tempered pitch. Zero is the default.
`Prepared::new_tuned` compiles these offsets directly into existing key candidates;
the runtime retains neither a tuning table nor a second pitch lookup. PCM handles
can be shared with the preceding plan. Every mapped source rate still must lie
within 1/256–16; unsupported values fail preparation instead of being clamped.
Unused keys need not satisfy a particular region's rate constraint.

For tracked regions, the compiled semitone displacement is authored transpose plus
played key minus recorded root key plus the **played key's** tuning offset. The
recorded root remains nominal, so tuning the root key changes its sound as well.
Nonmonotonic tables are supported. Fixed-pitch regions (`root_key: None`) bypass
the table. Live note expression remains an additional independent displacement;
neither the logical key nor the physical input address is rewritten.

Changes use the existing prepared-plan adoption boundary: new root inputs use the
new tuning, while already-admitted notes and their later generated descendants
keep their original plan's tuning. This is deliberately a future-root policy,
not live retuning of held notes. Scala/MTS adapters, unmapped-key semantics and
live retuning are not implemented by this native table.

`NotePitch` separates inherent pitch from both physical input identity and live
expression. `Key` uses the retained plan's tuning; `Absolute` uses the A4=69=440 Hz
semitone scale and overrides that table. `trigger_pitched` selects regions with the
integer part of absolute pitch. Tracked regions retain recorded-root/authored
transpose metadata to compute that override without subtracting a rounded or
potentially extreme tuning correction from an already compiled rate. Absolute
selection evaluates the rate during preflight and admission, not per sample.
Default key candidates keep their compiled path. No tuning table is added to the
runtime, and fixed-pitch regions remain exempt from key and absolute tracking.

`note_pitch` exposes the full inherent pitch. Generated `Play` transposes this
value without discarding its fractional part; independent expression inheritance
resets expression rather than the inherent pitch. Explicit `child_pitched` accepts
its own pitch, while `child` requests a tuned key. Manual source-start APIs still
use their explicitly supplied playback parameters; they have no recorded-root
mapping to infer. Invalid/nonfinite or out-of-domain pitches fail admission.

Core tests compare absolute playback with independent statically transposed sources,
including fractional domain endpoints, generated-note transposition and independent
expression. MIDI tests exercise the actual Pitch 7.9 attribute path and note pairing.

Three paired ordinary-key render workloads against the pre-absolute-pitch binary
measured median configuration speed ratio 0.996 (range 0.945–1.049), on the same
pinned local CPU with no parallel builds. This measures steady rendering, not the
new absolute-pitch admission cost or a host deadline guarantee. Local CSVs and
binary hashes are `artifacts/absolute-pitch-*`.

Heap-guarded tests compare analytic tones for irregular/nonmonotonic offsets,
fixed-pitch exemption and live expression across block sizes 1/7/64/256. They
reject nonfinite tables and invalid later key candidates. Plan-adoption tests
compare overlapping generations and delayed generated children against separate
statically transposed reference runtimes, then verify control-side retirement.

## Silent source advancement

An explicitly zero source gain or zero gains on both output channels now bypasses
PCM reads and sinc evaluation. This is not silence detection or voice retirement.
The cursor and envelope advance, release-loop exits stay intact, and ordinary
ownership cleanup occurs at the same source/envelope endpoint. Fractional strides
use the exact per-frame phase recurrence; a rounded block-size product would break
partition invariance. Zero-phase integer strides can advance the bounded virtual
source distance in bulk. Dynamic envelope stages still advance through their
normal transitions before a constant level can skip work.

Heap-guarded comparisons against always-audible reference voices cover forward and
reverse views, continuous/until-release loops, attack/hold/decay/release, repeated
mute/unmute including tails, transpositions 0/-12/7/48, and partitions 1/11/128.
Restored PCM and voice retirement match the reference. Gain changes do not reset
source position or create another voice.

`render_workloads --muted` and `--transpose 7 --muted` now expose this case with an
exact zero-output oracle. On the local pinned CPU, unity-rate silent configurations
measured a median 3.62× speedup (1.90–6.88×). Fractional +7-semitone configurations
measured 41.5–44.1×; 64 voices/256 frames dropped from 4547.8 to 104.0 microseconds.
Three paired audible runs measured a median configuration ratio of 1.025, with
configuration medians 0.964–1.082. These are process-level local observations, not
callback deadline guarantees. CSVs, logs and binary hashes use `artifacts/muted-*`.
