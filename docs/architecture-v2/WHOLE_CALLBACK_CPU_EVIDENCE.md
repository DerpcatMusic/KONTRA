# Whole callback CPU witness

`plugin::cpu_receipt::whole_callback_300_voice_cpu_receipt` runs the real
`Sampler::process` callback with exact CLAP-style host note identities. It reads
the callback's actual `busy_ns` and `span_ns` counters, which supply KONTRA's CPU
meter. The receipt reports their unsmoothed ratio; it does not change, round down,
or substitute a process/core-count percentage for that meter.

The fixture owns 300 independently allocated PCM sources with different audio,
or one shared source, and requires exactly 300 active and sounding voices
throughout the measured interval. Velocity bands choose one distinct source per
exact host owner. It uses the production pool-budget helper: generic core limits
reserve only 256 families and would not establish a 300-owner witness. The
production policy initially reserves 1,024 voices and 4,096 note/family slots for
these fixtures. Every exact host ID must release successfully afterward. Nonzero
PCM energy, finite output, zero runtime fault/drop/starvation/steal counters and
a numeric fingerprint prevent silent or undersized runs from passing.

Cases include integer-rate playback, fractional resampling and three independent
Kontakt-law EQs plus stereo processing per voice. Readback, master gain, mixing
and normal callback bookkeeping run in the actual plugin process function.
Authored instrument translation, disk decoding, Lua/KSP callbacks, the exported
CLAP adapter, a DAW and the UI painter are **outside this witness**. It is a
callback seam measurement, not evidence of complete live-host performance.
Output observers and receipt serialization run outside the measured meter span.
Admission is reported separately from sustained rendering.

## Reproduction

Build the ignored test with `cargo test --profile corpus --features shots --lib
whole_callback_300_voice_cpu_receipt --no-run`, then freeze the returned test
executable and its hash before running it. Use an otherwise quiet measurement
window and record compiler and competing CPU activity throughout, not just at
the boundaries. Run its exact test name with `--ignored --nocapture
--test-threads=1`. The optional environment variables are:

| Variable | Values/default |
| --- | --- |
| `KONTRA_CPU_RECEIPT_BLOCK` | 64 or 256; default 256 |
| `KONTRA_CPU_RECEIPT_BLOCKS` | at least 100; default 1,000 |
| `KONTRA_CPU_RECEIPT_LAYOUT` | shared or distinct; default distinct |
| `KONTRA_CPU_RECEIPT_WORKLOAD` | plain, fractional, eq-stereo; default fractional |

Test scratch/cache directories must remain on the development drive. The result
includes the active pool capacities, debug-assertion state, busy/span totals,
callback p50/p99/max, deadline misses, PCM fingerprint and runtime counters.
The target is an actual ratio at or below 1%, not merely a displayed integer 1
after UI smoothing and rounding. A 48 kHz/256-frame block allows 53.33 microseconds
for that target; a 64-frame block allows 13.33 microseconds.

## Current evidence, 2026-10-14

The initial fixture validation passed all three distinct-source cases for 100
blocks each, after 512 warmup blocks, with 300 sounding voices and complete
release and zero runtime counters. These runs used `dsp-dev` (optimization level
1, debug assertions enabled) under substantial unrelated CPU activity. Their
timings are retained in `work/whole-callback-cpu-fixture-*-smoke.log` and are not
release-performance acceptance or a 1% result.

The subsequent `corpus` release-profile binary was pinned to CPU5. All six
distinct-source cases passed 1,000 measured blocks after 512 warmup blocks,
including heap allocation/destruction guards on admission, steady playback and
release. Every case retained 300 sounding voices, released all exact host IDs,
produced nonzero finite PCM and reported zero runtime fault/drop/steal counters.
The executable hash was unchanged before/after the runs. Continuous 250 ms
observation found zero compiler observations; ordinary desktop activity and two
brief unrelated laboratory-process intervals remain in the receipt. This is
not an isolated machine or a paced live host.

| Block | Workload | KONTRA unsmoothed mean | Callback p50 us | p99 us | Max us |
| --- | --- | --- | --- | --- | --- |
| 256 | Integer rate | 1.886% | 98.122 | 176.223 | 273.745 |
| 256 | Fractional | 18.205% | 880.527 | 2370.004 | 2617.409 |
| 256 | Three EQ + stereo | 39.007% | 1995.208 | 2910.165 | 3210.570 |
| 64 | Integer rate | 3.438% | 44.841 | 80.441 | 124.782 |
| 64 | Fractional | 20.377% | 252.754 | 920.808 | 1210.853 |
| 64 | Three EQ + stereo | 39.070% | 499.699 | 1103.331 | 1269.274 |

There were no measured callback deadline misses in these cases. The large
tails, especially at 64 frames, remain material: no miss in this unpaced
resident-only interval does not establish live-host dropout safety. None of
the cases achieves the 1% target. These are baseline absolute callback costs,
not a paired before/after claim for the concurrent source-reuse experiment.

Raw numeric receipts and per-case logs are under
`/mnt/Windows11/DEV_WORKSPACE/PluginArtifacts/kontra-cpu-20261014/whole-callback-release.json`.
The original executable is frozen alongside it as
`whole-callback-release-baseline` (SHA-256
`a24fbc61099ec998ee279ca253cbd96001011df37e9d455f027457c326f58e73`);
the receipt's historical shared-target path can later refer to a rebuilt binary.
