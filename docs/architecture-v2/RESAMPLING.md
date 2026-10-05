# Resident fractional playback and rate conversion

The native source renderer now combines the asset/output sample-rate ratio with
`Playback::transpose_semitones` and optional `Region::root_key` tracking.
Prepared assets may have different rates from the
runtime. Preparation and direct source admission both validate the resulting step;
invalid, nonfinite and unsupported ratios fail before taking a voice slot or command.
Prepared regions retain fresh cursor templates rather than authoring playback objects.
The prepared key-candidate index stores each selected key's complete playback step.
`root_key: None` is fixed pitch; `Some(key)` adds equal-tempered key displacement.
Each step is computed directly from authored tuning and key displacement, preserving
exact unity at the root rather than multiplying rounded adjacent-key ratios.
Every mapped key must satisfy the declared rate bounds; an unmapped root need not.
The candidate budget now bounds records containing a region index and an f64 step
(16 bytes per candidate on 64-bit targets). Logical playback keys select these
records; physical input addresses remain unchanged for key-up and terminal pairing.
Selection copies the template and compiled step into a voice; it does not repeat `exp2`, rate conversion
or source-view validation at note-on. Both prepared selection and explicit manual
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

Live expression-to-pitch consumption, pitch ramps, broader rate
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
