# v1 prefetch lead trial — rejected after 64 quiet runs

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

## Matched gate verdict

All 64 runs completed. All event/render heap counts were zero and all 32 cold mincore receipts had pages_after=0. All timed intervals had only the own matrix unit, with no other scanner/build/unknown-unit activity; pre-grant census activity is excluded using the recorded run timestamps. The 32 pre-pair guards were clear, with root free space at least 16.658 GiB. Raw receipts: ~/.cache/kontakto-fix-cpu/prefetch-port-matrix/; complete summary: prefetch-port-summary.json.

| Cell | Frames | Cache | Baseline median / p99 µs | Trial median / p99 µs | Baseline underruns / deadlines | Trial underruns / deadlines |
|---|---:|---|---:|---:|---:|---:|
| areia16 | 32 | warm | 218.564 / 281.835 | 226.275 / 300.496 | 0 / 9 | 0 / 9 |
| areia16 | 32 | cold | 214.834 / 287.345 | 225.905 / 301.846 | 0 / 9 | 0 / 9 |
| areia16 | 64 | warm | 291.105 / 422.228 | 300.386 / 432.468 | 0 / 4 | 0 / 4 |
| areia16 | 64 | cold | 282.435 / 390.107 | 300.306 / 435.749 | 0 / 4 | 0 / 4 |
| areia16 | 256 | warm | 1201.422 / 1390.676 | 1244.324 / 1373.345 | 0 / 1 | 0 / 1 |
| areia16 | 256 | cold | 1173.742 / 1308.695 | 1237.844 / 1404.327 | 0 / 1 | 12 / 1 |
| areiafull | 32 | warm | 452.559 / 585.941 | 515.690 / 669.793 | 24 / 50 | 429 / 617 |
| areiafull | 32 | cold | 458.948 / 536.680 | 499.040 / 672.992 | 422 / 88 | 1024 / 607 |
| areiafull | 64 | warm | 622.062 / 875.616 | 678.912 / 854.766 | 24 / 13 | 349 / 15 |
| areiafull | 64 | cold | 608.271 / 840.196 | 686.363 / 863.836 | 23 / 10 | 335 / 12 |
| areiafull | 256 | warm | 2253.983 / 2571.129 | 2462.407 / 2712.642 | 24 / 1 | 356 / 1 |
| areiafull | 256 | cold | 2309.034 / 2678.791 | 2433.426 / 2887.724 | 24 / 1 | 366 / 1 |
| fx | 32 | warm | 47.190 / 88.782 | 48.471 / 106.492 | 0 / 1 | 0 / 1 |
| fx | 32 | cold | 44.810 / 93.251 | 42.731 / 83.632 | 0 / 1 | 0 / 1 |
| fx | 64 | warm | 47.871 / 91.752 | 50.851 / 86.942 | 0 / 0 | 0 / 0 |
| fx | 64 | cold | 50.551 / 106.322 | 49.570 / 99.772 | 0 / 0 | 2 / 0 |
| fx | 256 | warm | 186.833 / 262.825 | 185.513 / 274.955 | 0 / 0 | 0 / 0 |
| fx | 256 | cold | 183.834 / 263.855 | 183.324 / 268.225 | 0 / 0 | 0 / 0 |
| piano | 32 | warm | 19.441 / 34.851 | 18.621 / 32.750 | 0 / 0 | 0 / 0 |
| piano | 32 | cold | 18.201 / 34.381 | 22.291 / 36.800 | 0 / 0 | 0 / 1 |
| piano | 64 | warm | 26.061 / 45.381 | 25.991 / 44.861 | 0 / 0 | 0 / 0 |
| piano | 64 | cold | 26.451 / 44.511 | 25.921 / 45.641 | 0 / 0 | 0 / 0 |
| piano | 256 | warm | 98.512 / 150.203 | 116.062 / 166.313 | 0 / 1 | 0 / 1 |
| piano | 256 | cold | 113.672 / 153.033 | 113.322 / 151.893 | 0 / 1 | 0 / 1 |

The candidate repeatedly adds storage underruns on the quiet machine, so it is rejected without using tail-noise calibration to excuse those faults. Full Ensemble stream-capacity errors rose from 1216/1207 to 5050/4948 at 32 frames cold/warm, 519/508 to 2503/2461 at 64, and 262/257 to 1243/1223 at 256. It admitted 576 peak voices while the default pool was sized for 256 streaming voices × three pages (24 MiB). Increasing protected demand without matching that capacity caused additional pressure. Loaded Full Ensemble RSS remained about 473 MiB on both sides, already above the coordinator-supplied v1 reference of about 309 MiB.

The production adapter has been restored byte-for-byte to d75ae827 before its cfg(test) module. The useful two-page allocation-free runtime fixture remains and explicitly supplies its large test horizon. Trial source 3733552d and both frozen binaries remain as rejected evidence; no improvement was handed to integration.

The wired integration-gate v2 probe uses the ci profile with default features; this matched pair uses the corpus profile with clap/library-access and debug line tables. The matching Areia baseline therefore remains separate from the earlier ci gate measurements; voice counts match but absolute timings and fault counts differ. The next authorized trial must combine v1 resident heads, its ring lead and a pool sized on the control thread, and must satisfy the v1 Areia RSS bound alongside CPU/underrun gates. W8 owns the separate reduction in non-stream startup pools.
