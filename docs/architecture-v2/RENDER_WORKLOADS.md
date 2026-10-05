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


## Coordinated variation cost

`admission_workload --variation` compiles three global sequential takes with four
microphones each, selects four voices per note and retains one decision per note.
`--ids` can be combined with it. Declaration/preparation stays outside timing;
counter/resource assertions run outside each measured phase. Scope lookup here has
one global owner; this does not characterize large channel-scope tables.

Three alternating CPU-2 runs compared the preceding `ce34601` implementation with
the variation implementation, on the same Ryzen 7800X3D/Rust 1.99 release setup and
without concurrent builds/scans. Median-of-median admission times in microseconds:

| Notes | Reserved voices | Before | New, no variation | New, three takes |
| ---: | ---: | ---: | ---: | ---: |
| 16 | 64 | 2.040 | 2.250 | 3.250 |
| 64 | 256 | 8.051 | 9.130 | 12.940 |
| 256 | 1,024 | 33.261 | 37.451 | 53.281 |
| 1,024 | 4,096 | 174.613 | 188.704 | 261.625 |
| 16 | 4,096 | 2.020 | 2.300 | 3.360 |

Without variation, the new grouped preflight and metadata add 8–14% admission cost
in these cases. With variation, admission also evaluates 12 candidates and acquires
one retained decision per note. The largest enabled burst's note-off/retirement
medians were 3,985.505/8.870 microseconds, versus 4,023.145/7.161 before. Input pairing
still dominates that release workload. With external IDs and no variation, the
largest admission medians were 1,031.099 before and 1,022.739 after; this does not
establish a general improvement in duplicate-ID lookup.

Across all 32 steady resident-render cases, the median after/before ratio was 0.994,
range 0.898–1.050. No per-sample variation work was introduced; decision and scope
storage still have real memory/admission costs. These synthetic local timings are
not a production deadline, streaming or competitor result. CSVs and executable
hashes use ignored `artifacts/variation-final-*`.


## Seeded policy cost

`admission_workload --random`, `--no-repeat` and `--shuffle` use seed 42 with
three global takes and four microphones per take; `--variation` remains sequential.
Only one policy flag is accepted, optionally with `--ids`. The CSV variation column
indicates whether variation is enabled; the invocation/file name identifies its policy.
All runs admit the same four voices and one family per note.

Three alternating CPU-2 passes compared `9a72ded` with the seeded-policy implementation
on the Ryzen 7800X3D, Rust 1.99 release, after builds/checks finished. Median-of-median
admission times in microseconds (no external IDs):

| Notes | Reserved voices | Before, plain | After, plain | Before, sequential | After, sequential | Random | No-repeat | Shuffle |
| ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 16 | 64 | 2.280 | 2.270 | 3.250 | 3.450 | 3.621 | 3.600 | 3.600 |
| 64 | 256 | 9.220 | 9.200 | 13.020 | 13.791 | 14.791 | 14.451 | 14.601 |
| 256 | 1,024 | 37.531 | 37.501 | 53.671 | 56.201 | 61.161 | 59.911 | 61.181 |
| 1,024 | 4,096 | 189.824 | 191.814 | 262.085 | 279.436 | 297.625 | 290.625 | 292.816 |
| 16 | 4,096 | 2.300 | 2.310 | 3.390 | 3.450 | 3.780 | 3.620 | 3.660 |

Sequential selection pays roughly 2–7% for the larger transactional policy state;
plain admission stays within about 1.1% here. At 1,024 notes the seeded policies cost
4–7% over the new sequential path. This measures one global owner and tiny bags,
not large channel-scope scans, cold cache behavior or a real host deadline.
No per-frame DSP path changed; this checkpoint did not repeat the resident-render
matrix. Shuffle storage is four bytes per reserved take/owner, in addition to bounded
per-owner generator/progress state. CSVs and hashes use ignored
`artifacts/random-final-*`; the preceding executable is retained as
`artifacts/random-admission-before`.

## Retained release-context cost

Release timestamps/velocity use one control-preallocated payload per reserved note;
key/gate cause markers replace the two note-state booleans. Payload allocation follows
the frequently traversed pools. An initial inline-record prototype regressed the
256-note identified-input admission case from 77.302 to 91.372 microseconds in a
three-pass comparison. Separating the cold payload recovered that case. Allocating
the payload among the hot pools also exposed a slower sparse-shuffle case in two of
three runs (about 5.9 versus 3.6 microseconds); allocating it after those pools removed
that regression in the final three-pass measurement. This is evidence for the local
layout choice, not proof of identical cache behavior on other hardware/allocators.

Final alternating CPU-2 runs compare `247b33d` with retained release context on the
same Ryzen 7800X3D/Rust 1.99 release setup, after builds/scans finished.
Median-of-median admission times in microseconds:

| Notes | Reserved voices | Plain before | Plain after | IDs before | IDs after | Shuffle before | Shuffle after |
| ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 16 | 64 | 2.250 | 2.230 | 2.410 | 2.420 | 3.590 | 3.600 |
| 64 | 256 | 9.150 | 9.261 | 11.400 | 11.360 | 14.520 | 14.650 |
| 256 | 1,024 | 37.171 | 37.581 | 76.831 | 75.722 | 60.052 | 60.811 |
| 1,024 | 4,096 | 192.834 | 190.754 | 1,022.869 | 909.837 | 292.446 | 291.205 |
| 16 | 4,096 | 2.320 | 2.310 | 9.801 | 9.850 | 3.650 | 3.710 |

Plain/shuffle admission stays within about 1.7% in these cases. The largest identified
case improved locally, but the comparison does not establish a general duplicate-ID
lookup improvement. At 1,024 notes, note-off medians before/after were
4,021.696/3,909.134 microseconds plain, 2,303.164/2,372.525 with IDs and
3,901.063/4,019.496 with shuffle. Retirement was 7.510/8.450, 7.530/8.470 and
9.080/9.420 respectively: retained metadata and marker changes are not free.

Across 32 resident-render cases the median after/before ratio was 0.997,
range 0.951–1.034. No source kernel or per-frame release-context query was added.
These synthetic workloads do not certify host deadlines or full-polyphony release
sample bursts; automatic release mapping/reserves remain unimplemented.
Final CSVs use ignored `artifacts/release-context-layout-*`; the inline and first
split-layout investigations remain under `release-context-final-*` and
`release-context-split-*`. Binary provenance is in `release-context-binaries.sha256`.

## Native release selection cost

On 2026-10-06, compare committed `56b0f5f` with the native release-selection
checkpoint on the same Ryzen 7 7800X3D/Linux machine, pinned to CPU 2. Three paired
runs per configuration, no concurrent compiler/scan. Values below are medians of
run medians in microseconds. The existing workload admits four microphones per
note, times note-off and retirement separately, with 32 warmups and 128 measured
bursts. It is a local microbenchmark, not host or vendor comparison evidence.

| Notes | Voice capacity | Plain admission before → after | ID admission before → after | Shuffle admission before → after |
| ---: | ---: | ---: | ---: | ---: |
| 16 | 64 | 2.271 → 2.490 | 2.380 → 2.480 | 3.570 → 3.930 |
| 64 | 256 | 9.200 → 9.651 | 11.500 → 12.060 | 14.980 → 15.901 |
| 256 | 1024 | 37.941 → 39.751 | 75.751 → 78.132 | 60.552 → 66.371 |
| 1024 | 4096 | 187.923 → 195.303 | 898.076 → 929.207 | 289.075 → 313.526 |
| 16 | 4096 | 2.301 → 2.480 | 9.860 → 9.801 | 3.640 → 4.010 |

The initial implementation performed release preparation checks even for empty
phases. Skipping those checks reduced large plain admission overhead from about
11% to 4%; remaining shuffle admission overhead is about 6–10% in these runs.
The added phase metadata/reservation checks are not claimed free. Inlining the
shared scope-position helper did not produce a repeatable improvement and was
not retained. Further optimization should preserve transactional ownership and be
justified by representative workloads.

At 1024 notes, plain/ID/shuffle note-off medians changed respectively from
3894.301/2373.553/4028.314 to 4041.634/2296.232/3961.292 microseconds. Retirement
changed from 8.380/8.450/9.410 to 8.351/8.320/9.090 microseconds. The 32 resident
render configurations had a median after/before ratio of 0.974 (range 0.909–1.025).
This does not establish a general rendering speedup; the playback loop is unchanged.

`admission_workload --releases` additionally reserves four gate-release microphones
per note, then selects them at note-off. `--releases --shuffle` gives attack and gate
phases separate three-take shuffle sequences. Voice/family/decision budgets double;
the printed capacity includes both phases. The release block reaches natural EOF
outside the timed note-off/retirement spans. These are immediate note-off bursts,
not a measurement of one shared pedal-up event; pedal coherence is tested separately.

| Notes | Total voice capacity | Plain admission / note-off | Shuffle admission / note-off |
| ---: | ---: | ---: | ---: |
| 16 | 128 | 2.730 / 4.161 | 4.740 / 5.730 |
| 64 | 512 | 10.940 / 27.820 | 19.190 / 34.270 |
| 256 | 2048 | 44.070 / 297.985 | 77.751 / 327.346 |
| 1024 | 8192 | 214.504 / 4237.967 | 355.956 / 4382.030 |
| 16 | 8192 | 2.710 / 63.482 | 4.810 / 66.052 |

The large note-off and sparse-capacity figures still include input matching and
bounded cleanup scans. They are not acceptable evidence of arbitrary polyphony
meeting a small audio deadline. Streaming, effects, continuous modulation and live
host load remain outside this workload.

Local CSVs use `artifacts/release-selection-final-{before,after}-*.csv` and
`artifacts/release-selection-final-release*.csv`; executable hashes are in
`artifacts/release-selection-final-binaries.sha256`. The earlier exploratory runs
and inlining experiment are retained separately. Release-selection tests retain
allocation/deallocation guards independently of these timing measurements.

## Articulation selection cost

The articulation checkpoint adds fixed performance domains, cold note snapshots and
sparse filtering without growing the hot note record. On 2026-10-06, three paired
CPU-2 runs on the same Ryzen 7 7800X3D/Linux host compare the retained release-selection
executable with this change. No concurrent build/scan ran during measurement. Values
are medians of run medians in microseconds; CSVs retain individual-run tails.

| Notes | Voice capacity | Plain admission before → after | ID admission before → after | Shuffle admission before → after |
| ---: | ---: | ---: | ---: | ---: |
| 16 | 64 | 2.440 → 2.681 | 2.560 → 2.930 | 4.020 → 4.040 |
| 64 | 256 | 9.940 → 10.680 | 12.710 → 13.540 | 16.230 → 16.680 |
| 256 | 1024 | 40.191 → 44.181 | 80.992 → 89.862 | 68.091 → 69.162 |
| 1024 | 4096 | 199.713 → 218.234 | 936.377 → 994.988 | 316.166 → 323.156 |
| 16 | 4096 | 2.520 → 2.691 | 9.830 → 10.601 | 4.130 → 4.100 |

Plain admission costs about 7–10% more and shuffle about -1–3% in these local runs;
ID results vary about 6–14%. These features are not free. At 1024 notes, plain/ID/
shuffle note-off medians are 4038.816/2387.335/4016.074 microseconds after the change,
versus 4122.467/2360.805/4017.715 before. The unchanged physical matching/cleanup work
still dominates large bursts. Retirement remains approximately 8–9 microseconds.
No steady-render speedup is claimed or inferred from admission measurements.

`admission_workload --articulations` authors 64 exclusive labels, selects label 63,
and still creates four microphones per note. It composes with `--shuffle` (three
takes) and `--releases` (four separately reserved release microphones). The following
costs include both sparse filtering and ordinary admission; they are not standalone
binary-search timings.

| Notes | Attack voice capacity | 64-label plain admission | 64-label shuffle admission | 64-label attack+release shuffle admission / note-off |
| ---: | ---: | ---: | ---: | ---: |
| 16 | 64 | 4.150 | 5.730 | 10.230 / 7.830 |
| 64 | 256 | 15.881 | 23.260 | 39.900 / 42.801 |
| 256 | 1024 | 64.441 | 97.572 | 160.753 / 364.346 |
| 1024 | 4096 | 298.165 | 434.189 | 687.633 / 4603.726 |
| 16 | 4096 | 3.960 | 9.370 | 9.880 / 68.101 |

Release configurations double the printed table's attack capacity for the total
voice budget. The sparse shuffle row was slower/noisier than the small dense row;
retain it rather than treating pool capacity as irrelevant. Results do not prove
arbitrary-library callback deadlines. Current-state release policies may require
validating all possible articulation pitches; these indexed release measurements
use known onset snapshots.

The first implementation repeated articulation searches and scanned inactive tags
during snapshot release-pitch validation. Reusing each group's two ranges and
validating only reachable snapshot candidates reduced the measured 1024-note
indexed attack+release admission from 1349.505 to 687.633 microseconds. It also
reduced unarticulated shuffle overhead from roughly 8–15% to the values above.

Artifacts: `artifacts/articulation-final-*.csv` and
`artifacts/articulation-final-binaries.sha256`. Initial measurements remain under
`artifacts/articulation-{before,after,indexed}*`. Heap guards and independent
selection/ownership tests remain separate from timing evidence.


## Controller snapshots and predicate selection cost

2026-10-06, Rust 1.99 release, Ryzen 7800X3D, CPU 2, three interleaved
runs of `admission_workload`. Baseline is the controller-snapshot implementation
`7e17e76` before predicate selection; final binaries and SHA-256 values are retained
in `artifacts/controller-admission-{before,final}` and
`artifacts/predicate-final-binaries.sha256`. Raw final measurements are
`artifacts/predicate-{before,final}-{plain,shuffle,release,cc,cc-shuffle,cc-release}-{0,1,2}.csv`;
CC configurations exist only for final. Values below are medians of the three
per-run medians, in microseconds per admission burst.

| Notes / base voice slots | Plain before → after | Shuffle before → after | Shuffle + release before → after | 64 CC groups | 64 CC groups + shuffle | 64 CC groups + shuffle + release |
| --- | --- | --- | --- | --- | --- | --- |
| 16 / 64 | 3.580 → 2.920 | 4.180 → 4.761 | 5.740 → 6.360 | 7.830 | 10.870 | 21.641 |
| 64 / 256 | 11.490 → 11.681 | 16.820 → 19.421 | 22.661 → 25.791 | 31.271 | 44.700 | 88.432 |
| 256 / 1024 | 45.451 → 48.440 | 70.671 → 80.561 | 93.441 → 105.532 | 129.022 | 179.703 | 355.167 |
| 1024 / 4096 | 221.624 → 233.775 | 329.326 → 373.707 | 422.378 → 468.359 | 567.721 | 788.315 | 1481.568 |
| 16 / 4096 | 2.780 → 3.440 | 4.230 → 4.840 | 5.800 → 6.381 | 8.070 | 10.760 | 21.560 |

`--controllers` authors 64 mutually exclusive CC1 equality conditions and selects
63. Each condition maps four microphones (three coordinated takes with `--shuffle`).
This is sparse predicate traversal after key/phase indexing, not a 64-way Cartesian
state cache. `--releases` adds an independently selected gate phase. Conservative
controller release bounds currently require `base_slots * 65` source capacity for
that last column, versus `base_slots * 2` without controller conditions; family and
decision reservations remain one per phase/sequence. The geometry and resource
assertions are part of the executable, so the over-reservation is visible.

At 1024 notes, final note-off medians were 4011.666 us plain, 3969.365 us shuffle,
4327.391 us shuffle/release and 4880.112 us CC/shuffle/release. Admission and cleanup
bursts are not steady render callbacks; these values do not establish a host
realtime deadline or a competitor ranking. Existing FIFO pairing contributes to
large note-off workloads. Final ordinary admission still costs roughly 5–16% more
in the larger plain/shuffle cases; this is an open performance concern, not a
speedup claim.

The first predicate implementation walked each condition run to find its end.
Preparation now compiles those boundaries and unconditioned traversal uses direct
range iteration. Intermediate binaries/CSVs retain that comparison under
`controller-admission-{after,indexed}` and `controller-*.csv`. Host load varied
substantially during intermediate runs (load average approximately 18–20 was
observed), so cross-run optimization ratios are not treated as controlled evidence.
No local compiler or test process ran concurrently with timed batches.

Seven heap-guarded controller checks cover state ownership and actual selection;
all four native crates pass debug/release, Rust 1.92 and strict all-target Clippy,
with root boundary checks separate. Final validation logs are
`artifacts/predicate-final-{debug,release,msrv,clippy}.log` and
`artifacts/predicate-boundary.log`. No Doctor scan was used for this change.
