# v1 prefetch lead trial — HOLD pending original-instrument gate

The trial is based on d75ae827edc7c7b19f42f9cb7eb63633f9f002cb, which includes the accepted nonzero-delta filter port. It copies the 8192-frame RING lead from v1 0cb7a8a0:src/engine/stream.rs into the host adapter's minimum request horizon, keeping its existing MAX_BLOCK guard and any larger measured head requirement. v1 measures source frames; this adapter measures output frames. Their lead matches at unity pitch; no exact equivalence at other rates is claimed.

The existing preload budget, stream readers and page-pool capacity are unchanged. The root fixture proves both future pages are requested before a resident page is consumed, and rendering, completion admission and both page crossings do no heap work. That fixture, the offline-readiness fixture, stream-cache/policy tests (10), and root cargo test --no-run passed on the rebased source. Baseline and candidate probes use the same Cargo.lock, corpus profile, line-tables-only debug setting, audit sequence and build target.

The all-QUIET integration gate at ~/.cache/kontra-runs/20261008T174526.445032Z-d75ae827edc7 supplies additional acceptance targets:

| Instrument | Frames | v1 steady median/p99 µs | v2 steady median/p99 µs | v1 underruns/deadlines | v2 underruns/deadlines |
|---|---:|---:|---:|---:|---:|
| Areia 16 Violins | 32 | 22.301 / 27.721 | 208.183 / 266.805 | 0 / 2 | 19 / 9 |
| Areia 16 Violins | 64 | 30.891 / 41.111 | 283.016 / 451.419 | 0 / 1 | 0 / 5 |
| Areia 16 Violins | 256 | 137.203 / 172.254 | 1185.963 / 1305.545 | 0 / 0 | 0 / 1 |
| Areia Full Ensemble | 32 | 47.291 / 71.482 | 456.938 / 561.451 | 0 / 2 | 24 / 112 |
| Areia Full Ensemble | 64 | 72.122 / 92.182 | 601.972 / 758.634 | 0 / 2 | 24 / 13 |
| Areia Full Ensemble | 256 | 258.135 / 321.666 | 2341.305 / 2686.931 | 0 / 0 | 25 / 1 |

These instruments have different observed voice counts in v1 and v2; the table does not claim equivalent playback. The source gate, CPU JSON and integration-regression JSON remain intact. The trial must pass matched quiet A/B on the original 40 runs, including FX256 cold, plus cold/warm pairs for both Areia instruments at 32/64/256 (64 runs total). Per-run unit/process snapshots and a cgroup CPU/I/O timeline accompany the measurements. Release underrun tolerance remains zero. Acceptance and release readiness are pending; this commit is a separately gated trial.
