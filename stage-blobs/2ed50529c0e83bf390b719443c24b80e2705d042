# CPU42 top-three source profile and resampler bank range

Runtime candidate `00a254eafe877503ac0f94e2715de9bb59b0e989`, branch
`v2/w9-cpu42-top3-5fc`, based on frozen CPU42 source `5fc362f3`.
Failing-first test `359dc245` exited 101 because pitched-up realtime voices
above 2x lacked prepared taps. The fix extends the existing eight-per-octave
polyphase bank through the supported 16x limit. It changes one runtime file;
High quality, unity/upsampled cubic and the first octave's prepared rows remain
unchanged. No streaming service code changes; streaming stays **HOLD**.

## Attribution

The original audit sequence was reused on Morphology256, Afflatus2Horns256 and
Afflatus2Horns64. An unstripped release diagnostic was built from exact source
5fc362f3; user CPU sampling ran at 9999Hz. Samples were restricted to the
probe's steady interval (0.25–1s), classified by the closest inlined project
frame. Enclosing generic argument names are excluded from classification.
These are sampled estimates with profiler overhead, **not acceptance timings**.

| Cell | Resample µs/block | Envelope/mod µs/block | Mix µs/block | DSP dispatch µs/block |
|---|---:|---:|---:|---:|
| Morphology256 | 357.5 | 61.7 | 46.1 | 14.9 |
| 2Horns256 | 432.0 | 401.5 | 282.3 | 307.1 |
| 2Horns64 | 86.0 | 82.4 | 64.4 | 60.9 |

Unattributed work remains explicitly recorded. No Daft frames were sampled;
that does not prove it absent from every possible articulation. W6's Daft
candidate remains separate and unmeasured.

Morphology's largest clear leaf is `Table::sample`,
`crates/sampler-core/src/resample.rs:91`, with coefficient lookup at line63.
The prepared bank previously stopped at 2x, so higher streamed pitch repeated
scalar per-tap lookup. V1 `0cb7a8a0:src/engine/voice.rs` uses fused cubic
resampling/mixing (sampled Morphology estimate19.9µs/block); copying cubic for
all ratios here would change anti-aliasing behavior. This fix reuses the
existing prepared sinc path instead. Above2x now follows the same conservative
stretch policy as the first octave, with the band edge at most1/8octave lower.
Native sound parity for those new rows is not established.

## Matched targeted measurement

One pair per temperature, 750blocks per row, block256, original `strings`
audit sequence. All six scored rows were QUIET throughout. Cold evicted only
original Morphology files with the audit's fadvise/mincore runner; all815136
pages were nonresident before each process. Warm rows followed a completed
warm-up. The contended warm-up was unscored. Product cache was disabled.

| Temperature | Version | p50 µs/block | p99 µs/block | Underruns |
|---|---|---:|---:|---:|
| Cold | Frozen v1 | 46.791 | 67.181 | 0 |
| Cold | Frozen5fc before | 626.352 | 1292.554 | 0 |
| Cold | Bank32 after | 430.888 | 781.405 | 0 |
| Warm | Frozen v1 | 46.371 | 79.602 | 0 |
| Warm | Frozen5fc before | 661.253 | 1244.934 | 0 |
| Warm | Bank32 after | 434.739 | 725.303 | 0 |

Cold p50/p99 improved31.2%/39.5%; warm improved34.3%/41.7%.
All rows had zero render/event heap calls and deadline misses, and32peak
voices. Before/after mean voices matched30.4427; v1's mean was31.9573, the
same admission difference as the original CPU42 comparison. These are targeted
single pairs, not a full CPU42 rerun or a statistical noise characterization.
Candidate remains9.2–9.4x v1 p50 and9.1–11.6x v1 p99: **CPU parity HOLD**.

Tradeoff: immutable shared-bank storage increases166400→2321280bytes.
Observed cold load0.600→0.645s; warm0.442→0.545s. Loaded RSS increased
98040→99716KiB cold and98320→100100KiB warm. Preparation and memory still
need the full load/RSS gate; this is not an all-metric acceptance claim.

## Correctness and persistence follow-up

Three resampler unit tests,18resampler integration tests and10source tests
passed, including signal/alias properties, loop and endpoint behavior,
partitioning and allocation checks. Root `cargo test --no-run` passed.
A diagnostic CPU probe was built; no plugin release or install was performed.

Existing persistence candidate5592510e (CLAP sha774bb65d…) was reused.
Dolce Harmonics64 completed1500blocks/4events with verified native readback,
audible finite output and zero underruns under QUIET admission: callback
p50/p9916.061/170.433µs,14callback deadline misses,724wake misses.
Observer wall6.002s includes loading, audition, host save and collector finish;
it is not callback CPU or pure render time. RR was unseeded, so no PCM parity
claim. Shipping-before evidence was contended and cannot form a quiet A/B.

The additional Areia256 row completed375blocks/6events, audible/finite with
native readback and zero underruns, but a foreign build intruded. Its timing
is **UNKNOWN**, excluded; no repeat after that verdict. Earlier contended
Harmonics attempts, including one with37underruns, are retained and excluded.
Dolce streaming remains HOLD because deadline/wake behavior and native parity
are still unresolved.

Receipts: `/mnt/Windows11/DEV_WORKSPACE/kontra-runs/w9-cpu42-top3-20261009/`
contains `attribution.json`, `comparison.json`, `validation-summary.json`,
probe BUILD receipts, per-row activity/metrics and `persistence-decisive/`.
Raw native states, library journals and perf stacks were kept only in tmpfs
and destroyed. Numeric metadata, hashes, source locations and counters remain.
