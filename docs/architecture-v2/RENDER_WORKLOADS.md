# Resident render workload and sustain optimization

This supplies partial V2-01/V2-19 evidence. It measures the new resident source
renderer, not file import, host dispatch, streaming, resampling, effects, or a
comparison with other samplers.

## Reproduction

```sh
CARGO_TARGET_DIR="$PWD/target-core" cargo run --locked --release -p sampler-core --example render_workloads
```

The example prepares four independent 4,096-frame stereo PCM layers per logical
note with continuous loops. It measures 64, 256 and 1,024 active voices with matching
reserved capacity, plus 64 active voices in a 4,096-slot pool. Cases cover 48/96 kHz,
64/256-frame blocks, unity envelopes and settled AHDSR sustain at 0.5. Each case
warms 64 callbacks, then times 512 callbacks and reports median, p99, maximum,
p99/deadline percentage and median nanoseconds per voice-frame.

Preparation, note admission, output validation, percentile sorting and retirement
are untimed. Every callback's stereo output is independently checked using exact
binary fractions. This constant-data workload has predictable memory locality and
no event traffic; it cannot substitute for future multisample/cache, modulation,
streaming-stall, dense-onset, host jitter or effect-chain workloads. Reported deadline
ratios are observations, not worst-case guarantees.

## Measured change

Previously, non-unity envelopes called `next` on every frame even after reaching
sustain. The renderer now recognizes a constant held level once per contiguous
source span. Ramps and release still advance per sample. Arithmetic order and
ascending voice-slot summation are unchanged. The existing unity path remains
explicit so the general constant-level path does not add a multiplication there.
No SIMD intrinsics, unsafe code, fast-math settings or new DSP dependency were added.

On a Ryzen 7 7800X3D, x86_64 Linux, Rust 1.99 release with thin LTO, three paired
before/after runs were pinned to CPU 2. Builds and scans finished before timing.
The table is the median of the three per-run medians, in microseconds, with 1,024
voices and matching capacity:

| Envelope | Rate | Frames | Before | After | Speedup |
| --- | ---: | ---: | ---: | ---: | ---: |
| Sustain 0.5 | 48 kHz | 64 | 62.281 | 27.371 | 2.28× |
| Sustain 0.5 | 48 kHz | 256 | 224.404 | 77.422 | 2.90× |
| Sustain 0.5 | 96 kHz | 64 | 65.461 | 27.840 | 2.35× |
| Sustain 0.5 | 96 kHz | 256 | 222.555 | 77.772 | 2.86× |
| Unity | 48 kHz | 64 | 26.650 | 27.230 | 0.98× |
| Unity | 48 kHz | 256 | 75.042 | 73.681 | 1.02× |
| Unity | 96 kHz | 64 | 27.021 | 26.460 | 1.02× |
| Unity | 96 kHz | 256 | 72.771 | 72.502 | 1.00× |

Across all 16 sustain configurations, the median speedup was 2.41×. Across unity
configurations it was 1.00×, with ratios between 0.968 and 1.033. CPU frequency,
background work and scheduler outliers remain uncontrolled. These are local results;
no competitor or production-latency claim follows from them.

Existing envelope partition tests cover phase crossings, captured release levels,
zero-duration phases, pedals and source end. Native process tests independently
check complete AHDSR waveforms at 44.1/48/96 kHz. Benchmark output remains bit-exact.
The first general-constant version added a multiply to unity rendering; that measured
regression was removed before this final comparison.

Raw CSVs, executable hashes, source hashes and platform metadata are under ignored
`artifacts/architecture-v2/render-*`. The before executable used core commit
`0358598`; only the benchmark's debug-mode guard changed before the final paired
runs, with no release-mode workload change. The current new-core scan remains 90
with all rules retained; complete gates accompany the implementation checkpoint.

## Sparse voice reservations

A control-side allocated occupancy bitmap now records admitted voice slots: one
64-bit word per 64 reserved voices (512 bytes for 4,096 slots). Admission sets the
bit; the shared voice retirement path clears it. Rendering skips empty words and
iterates contiguous occupied runs in ascending slot order, preserving floating-point
summation order after holes and reuse. Delayed voices remain occupied until retired.

Three paired CPU-2 runs against `ade1401`, with the same workload and no concurrent
builds, measured the following medians in microseconds for 64 voices / 4,096 slots:

| Envelope | Rate | Frames | Before | After |
| --- | ---: | ---: | ---: | ---: |
| Unity | 48 kHz | 64 | 3.730 | 1.660 |
| Unity | 48 kHz | 256 | 6.710 | 4.520 |
| Unity | 96 kHz | 64 | 3.680 | 1.711 |
| Unity | 96 kHz | 256 | 6.660 | 4.490 |
| Sustain 0.5 | 48 kHz | 64 | 3.800 | 1.800 |
| Sustain 0.5 | 48 kHz | 256 | 6.880 | 4.990 |
| Sustain 0.5 | 96 kHz | 64 | 3.630 | 1.820 |
| Sustain 0.5 | 96 kHz | 256 | 6.950 | 5.071 |

Sparse median speedup was 1.74× (range 1.37–2.25×). Dense configurations had
median speedup 1.002×, with individual ratios 0.931–1.102; these measurements do
not establish a universal dense improvement. Raw paired results are in ignored
`artifacts/architecture-v2/verified-*.csv`.

The mixed-ownership test checks every bitmap bit against actual slot occupancy.
A heap-audited test crosses slots 63/64, creates holes, reuses slot zero, checks
order-sensitive cancellation, and exercises EOF, stale handles and panic. Rendering
lives in its own module while admission and retirement retain bitmap ownership.


The workload also accepts trailing `--muted`, including after `--transpose 7`.
A `muted` CSV column distinguishes explicit zero note gain. Preparation/admission
remain untimed, the output oracle checks exact zero, and [resampling evidence](RESAMPLING.md)
records phase/envelope continuity and audible-path comparisons for the silent
advancement optimization. This does not bypass ownership or stop muted voices.

## Prepared note admission

`cargo run --release --locked -p sampler-core --example admission_workload` times
bursts of 16/64/256/1,024 same-key notes with four prepared layers each, plus 16
notes in a 4,096-voice reservation. Each configuration uses 32 warmups and 128 timed
bursts. The original workload timed admission alone; the current version separately
times note admission, paired note-off and accepted terminal retirement. Preparation
and counter assertions stay outside timing. Inputs have no external host ID by
default, so duplicate-ID lookup is not included unless `--ids` is supplied.

Arena capacity and occupancy are now maintained at insertion/removal rather than
reconstructed by scanning reserved slots. Plan transfers use the same accounting:
failed publication restores the exact generation, and an exhausted generation is
never returned to available capacity. A deterministic mixed-operation test compares
the counters with a full slot reconstruction, including stale/foreign handles and
final-generation transfer rollback. Existing runtime and cross-thread plan tests
continue to audit audio-thread allocation and destruction.

Three paired CPU-2 runs against `b56b38c`, with no concurrent builds, measured these
median-of-median admission times in microseconds:

| Notes | Reserved voices | Before | After | Speedup |
| ---: | ---: | ---: | ---: | ---: |
| 16 | 64 | 4.870 | 2.810 | 1.73× |
| 64 | 256 | 39.791 | 26.730 | 1.49× |
| 256 | 1,024 | 592.161 | 375.847 | 1.58× |
| 1,024 | 4,096 | 10,005.507 | 6,083.934 | 1.64× |
| 16 | 4,096 | 63.971 | 4.840 | 13.22× |

At this stage the largest burst remained too expensive for a 256-frame/48 kHz block
before any rendering. Removing count scans alone did not complete admission
optimization. Local binaries, hashes and CSVs use
`artifacts/admission-*`. Scheduling outliers and these synthetic bursts are not a
production polyphony guarantee.

The admission example also accepts `--ids`, assigning a distinct external input
ID to each note and exercising duplicate-ID rejection lookup. After the bitmap
change, three local runs measured 1,024-note medians of 160.2 microseconds without
IDs and 1,010.5 microseconds with IDs; 16 notes in the 4,096-voice reservation took
1.92 and 9.69 microseconds respectively. The existing duplicate guard still scans
reserved notes. This cost is explicitly separate from allocator performance;
`artifacts/admission-address-*` records the comparison. No host-ID index has been
added or claimed by the allocator change.

### Free-slot lookup

Every arena now maintains one free bit per reserved slot. Insertion scans 64-bit
words and chooses the lowest set bit, preserving the previous lowest-slot policy
and therefore voice summation order after holes and reuse. Removal sets a bit only
if the generation can be reused; transfer rollback clears it without incrementing
the generation. Unused bits in the final word remain zero. The bitmap is allocated
on control and costs 512 bytes per 4,096 reserved slots. Lookup remains bounded by
the declared number of words, rather than claiming constant-time allocation at
arbitrary capacities.

The mixed-operation test checks bits, counts and lowest-slot results against full
slot reconstruction at capacities 0/1/63/64/65/128/129. Exhausted generations,
foreign/stale handles and rollback are included. Existing heap-audited voice-order
and real cross-thread plan-transfer checks cover the shared allocator in runtime use.

Three rotated CPU-2 comparisons used the original `b56b38c` binary, counters-only
`c2404d4`, and the bitmap implementation, with identical workloads and no concurrent
builds. Median-of-median burst times in microseconds were:

| Notes | Reserved voices | Original | Counters | Bitmap |
| ---: | ---: | ---: | ---: | ---: |
| 16 | 64 | 3.461 | 2.810 | 1.870 |
| 64 | 256 | 39.920 | 26.751 | 7.670 |
| 256 | 1,024 | 605.551 | 370.087 | 31.201 |
| 1,024 | 4,096 | 10,330.433 | 6,058.363 | 160.853 |
| 16 | 4,096 | 63.961 | 2.840 | 1.950 |

The largest synthetic burst improved 37.66× over counters alone and 64.22× over
the original allocator. Raw data and binary hashes use `artifacts/admission-bitmap-*`.
This does not include external-ID duplicate checks, streaming, rendering, script
bursts or cleanup; it is not a production deadline guarantee.


## Note release and terminal retirement

The same example now reports separate note-off and accepted-terminal medians/p99s.
Each burst starts four zero-release-envelope layers per note, releases every input,
then verifies exactly one accepted terminal per root and no retained notes/voices.
No rendering occurs in these phase timings. Nonzero tails, delayed starts, independent
children and pedal semantics are covered by separate correctness/heap tests.

Release follows direct note/child/family/source links and a preallocated closed-note
stack. Private links are unlinked before slot reuse; public handles remain
runtime/slot/generation identities. This removes ownership-wide release scans,
including family-stop scans, without changing terminal delivery or mixing order.
Input pairing still scans reserved notes and queued-work cancellation still scans
commands; this is not constant-time end-to-end release.

Three alternating CPU-2 comparisons against `fa80b36` (with the same extended phase
workload) used Rust 1.99 release on the local Ryzen 7800X3D, with builds/scans finished.
Median-of-median times in microseconds, with no external IDs:

| Notes | Reserved voices | Admission before | Admission after | Note-off before | Note-off after | Retirement before | Retirement after |
| ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 16 | 64 | 1.890 | 2.000 | 3.620 | 1.960 | 0.080 | 0.110 |
| 64 | 256 | 7.660 | 8.120 | 56.421 | 18.921 | 0.350 | 0.410 |
| 256 | 1,024 | 31.731 | 33.111 | 913.057 | 259.685 | 1.450 | 1.730 |
| 1,024 | 4,096 | 167.813 | 174.183 | 16,286.933 | 4,011.935 | 7.470 | 7.380 |
| 16 | 4,096 | 1.960 | 2.030 | 136.872 | 61.491 | 0.940 | 1.070 |

The largest no-ID release burst improved 4.06×; with distinct external IDs it fell
from 14,550.672 to 2,313.113 microseconds (6.29×). Sparse-reservation input pairing
remains expensive. Links add fixed memory and insertion/removal writes: the table
shows the admission/retirement tradeoff. Across the 32 resident render configurations,
the median after/before time ratio was 1.033, range 0.978–1.153. The measured render
cost is retained explicitly rather than claiming the added ownership metadata is free.
No host deadline or competitor claim follows from these synthetic workloads.
Raw CSVs and binary hashes use ignored `artifacts/release-final-*`.
