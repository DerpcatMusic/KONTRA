# Master gain stage receipt — 2026-10-14

The plugin previously advanced the master parameter smoother and evaluated its
exponential dB conversion once per sample. It now uses Moose's block smoother
API, retaining every smoothed sample, and reuses the exponential only when all
samples in the block have exactly the same dB bits. Moving blocks retain the
original scalar conversion. Host automation boundaries still split rendering.
This is a shared master-control change, independent of instrument names.

## Correctness

`master_gain_bulk_reads_preserve_smoothed_samples_and_automation_boundaries`
compares every output bit and final smoother value with the old per-sample
reads. It covers positive/negative gain, signed zero, repeated ramps, and
1/3/17/64/128-frame boundaries at 48 kHz. Both optimized and debug runs pass.
The cache allocates no storage beyond two scalar values; empty blocks do not
advance a sample. No precision, smoothing, effects or meter rules change.

## Measurement

Four CPU5-pinned optimized repetitions use 100,000 blocks of 128 frames per
case, with the real parameter smoother and output passed to `black_box`.
Other workspace workloads were not controlled. Raw local receipts are retained
in `PluginArtifacts/kontra-diagnostic-fixes/master-gain-pinned-receipts.txt`.

| Case | Old wall time range | New wall time range |
| --- | ---: | ---: |
| Constant master | 50.84–59.27 ms | 7.64–8.96 ms |
| Alternating automation target each block | 51.33–60.98 ms | 49.02–52.30 ms |

The constant stage takes about 0.51 microseconds per old block and 0.077
microseconds per new block in typical repetitions. This is a small callback
cost: at 48 kHz / 128 frames, roughly 0.019% versus 0.003% of the block
deadline. Moving-case variance includes one paired regression of about 1.7%;
these data do not establish a uniform speedup for automation.

This receipt is **not** a full plugin callback benchmark, a voice benchmark,
or evidence that 300 voices meet the user's 1% KONTRA-meter target. The ignored
`master_gain_cpu_receipt` test reproduces the four cases. Primary voice,
streaming and microarchitecture receipts belong in `CPU_EVIDENCE.md`.
