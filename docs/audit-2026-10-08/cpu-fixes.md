# W9 CPU / streaming implementation evidence

Branch `v2/fix-cpu`, baseline `7e82b152`, lifecycle checkpoint `ffd858c4`.
The CPU/streaming goal is **not yet met**. The first checkpoint removes the capacity-dependent idle floor and fixes callback retirement. More CPU work remains.

## Findings 1–2: sparse retirement and callback flush

The sound seam consumes completed outcomes before terminal notifications. Arena live-owner links preserve generation checks, parent retirement, and NOTE_END sink backpressure. The callback seam regression failed before the fix; 32 repeated cycles now retire exactly once, with zero allocations/frees. All 340 sampler-core tests and 25 sound seam tests pass; `cargo test --no-run` passed before the push. One ignored test in each suite was not executed.

Empty `flush_ended` median/p99 (µs): 64 slots **0.020 / 0.020**, 16,384 slots **0.020 / 0.030**, compared with audit **0.080** and **24.770** medians respectively. This removes the idle capacity floor without reducing reserved-note capacity.

The audit’s unchanged runners measured all 18 cells at this checkpoint. Every cell had nonzero output and zero event/render heap calls. All had zero underruns; the audit’s warm ANALOG STRINGS/256 had two. Linux source file pages were verified cold (`pages_after=0`) for every cold cell. Concurrent builds and storage activity still affect p99 and individual medians; higher cold 256-frame medians are reported, not hidden. These are scheduled-MIDI comparisons; no claim of equal PCM or native fidelity follows from them.

Times below are steady block median / p99 in microseconds. Baseline values are the audit’s committed evidence, whose preserved binary digest was verified. “Warm” means unforced file cache, as in the original audit.

| Cell | v1 | v2 audit | lifecycle | Underruns audit / lifecycle |
|---|---:|---:|---:|---:|
| fx-256-cold | 176.713 / 304.084 | 209.023 / 345.544 | 258.395 / 551.561 | 0 / 0 |
| fx-256 | 172.513 / 290.346 | 478.908 / 2125.584 | 181.385 / 289.447 | 2 / 0 |
| fx-32-cold | 15.380 / 45.851 | 75.081 / 132.312 | 44.510 / 97.073 | 0 / 0 |
| fx-32 | 20.801 / 55.331 | 73.921 / 120.162 | 48.031 / 113.453 | 0 / 0 |
| fx-64-cold | 68.431 / 241.434 | 163.253 / 335.675 | 114.902 / 258.125 | 0 / 0 |
| fx-64 | 45.571 / 84.912 | 112.422 / 233.953 | 75.562 / 144.224 | 0 / 0 |
| piano-256-cold | 29.920 / 79.192 | 210.923 / 351.585 | 90.262 / 124.533 | 0 / 0 |
| piano-256 | 16.660 / 40.701 | 144.523 / 398.179 | 87.533 / 124.603 | 0 / 0 |
| piano-32-cold | 10.471 / 24.871 | 44.570 / 60.841 | 25.330 / 54.381 | 0 / 0 |
| piano-32 | 4.581 / 13.890 | 44.671 / 67.172 | 15.361 / 30.821 | 0 / 0 |
| piano-64-cold | 12.170 / 30.750 | 88.771 / 171.823 | 24.190 / 46.241 | 0 / 0 |
| piano-64 | 6.920 / 21.111 | 52.951 / 76.652 | 39.041 / 64.412 | 0 / 0 |
| strings-256-cold | 22.250 / 37.900 | 167.132 / 234.253 | 242.515 / 638.074 | 0 / 0 |
| strings-256 | 10.701 / 18.180 | 171.634 / 236.745 | 141.633 / 261.476 | 0 / 0 |
| strings-32-cold | 6.300 / 9.100 | 58.580 / 90.291 | 25.021 / 43.791 | 0 / 0 |
| strings-32 | 2.730 / 7.430 | 56.352 / 73.021 | 34.641 / 76.432 | 0 / 0 |
| strings-64-cold | 3.530 / 8.750 | 69.520 / 101.142 | 35.271 / 64.192 | 0 / 0 |
| strings-64 | 3.500 / 8.400 | 69.092 / 108.333 | 36.431 / 72.122 | 0 / 0 |

## Interleaved cold256 lifecycle comparison

Three repeats per cell, fresh output directories, frozen original audit binary versus `ffd858c4` lifecycle binary. Each individual run evicts the library file cache using the original audit runner; order is before/after, after/before, before/after. Runs alternate within the same window; competing machine work was not disabled. Values are steady per-block median / p99 in microseconds; each cell has 141 steady blocks. All 18 runs report zero event/render heap calls.

| Cell | Repeat | Before median / p99 | After median / p99 | Underruns before / after |
|---|---:|---:|---:|---:|
| piano | 1 | 120.962 / 149.682 | 93.932 / 153.853 | 0 / 0 |
| piano | 2 | 247.213 / 1430.872 | 113.292 / 163.473 | 0 / 0 |
| piano | 3 | 322.885 / 494.627 | 140.582 / 701.970 | 0 / 0 |
| strings | 1 | 225.554 / 396.396 | 199.023 / 273.394 | 0 / 0 |
| strings | 2 | 166.942 / 252.354 | 226.994 / 525.678 | 0 / 0 |
| strings | 3 | 305.754 / 566.699 | 130.674 / 175.086 | 0 / 0 |
| fx | 1 | 214.014 / 308.654 | 250.654 / 444.476 | 5 / 2 |
| fx | 2 | 209.703 / 301.934 | 170.523 / 248.664 | 4 / 5 |
| fx | 3 | 201.676 / 272.418 | 176.185 / 266.159 | 5 / 4 |

This does **not** establish that every cold256 regression was noise. Median-of-run medians and median-of-run p99s improve for all three cells, but Vista repeat 2 and ANALOG STRINGS repeat 1 regress. ANALOG STRINGS also has 14 versus 11 total underruns. Do not infer sonic or streaming parity from CPU medians.

## Findings 4–5: offline readiness and transient page retry

Checkpoint `87b23907` waits at offline render boundaries after due starts and pitch preparation, with a bounded timeout and visible failure counters. Transient decode failures retry three times with wall-clock backoff; corrupt data and exhausted retries report a failure. Realtime streaming never waits. Delayed storage, delayed starts, script pitch changes, timeout/disconnection, corrupt pages, transient retry, and counter round trips have regression checks. The delayed-start/pitch fixture and adapter fixture reproduce lost PCM before this fix, then match resident rendering exactly after it with zero audio heap work.

Each entry below is steady block median / p99 in microseconds. These runs retain machine contention. Storage is a correctness checkpoint, not a realtime CPU optimization; nonzero underruns are explicit. The integrated column is a separately frozen merge baseline `88b89722`, including origin/integrate/core-v2 at `1ed8c470`, before further W9 changes. All 36 runs have zero event/render allocations/frees; every cold eviction reports `pages_after=0`.

| Cell | Lifecycle | Storage | Integrated baseline | Underruns lifecycle / storage / integrated |
|---|---:|---:|---:|---:|
| piano-32 | 15.361 / 30.821 | 24.371 / 52.701 | 14.371 / 34.811 | 0 / 0 / 0 |
| piano-64 | 39.041 / 64.412 | 39.281 / 70.461 | 24.391 / 46.341 | 0 / 0 / 0 |
| piano-256 | 87.533 / 124.603 | 162.133 / 229.965 | 80.182 / 117.203 | 0 / 0 / 0 |
| strings-32 | 34.641 / 76.432 | 25.181 / 42.741 | 26.850 / 51.681 | 0 / 0 / 0 |
| strings-64 | 36.431 / 72.122 | 34.991 / 53.731 | 36.810 / 60.472 | 0 / 0 / 0 |
| strings-256 | 141.633 / 261.476 | 139.233 / 191.234 | 130.933 / 161.854 | 0 / 0 / 0 |
| fx-32 | 48.031 / 113.453 | 52.290 / 125.582 | 48.131 / 110.763 | 0 / 0 / 0 |
| fx-64 | 75.562 / 144.224 | 114.672 / 403.208 | 75.432 / 129.243 | 0 / 0 / 0 |
| fx-256 | 181.385 / 289.447 | 236.465 / 358.056 | 175.543 / 253.096 | 0 / 5 / 0 |
| piano-32-cold | 25.330 / 54.381 | 16.990 / 41.121 | 16.671 / 37.311 | 0 / 0 / 0 |
| piano-64-cold | 24.190 / 46.241 | 39.561 / 102.132 | 23.380 / 43.701 | 0 / 0 / 0 |
| piano-256-cold | 90.262 / 124.533 | 89.692 / 127.792 | 95.602 / 215.974 | 0 / 14 / 0 |
| strings-32-cold | 25.021 / 43.791 | 24.810 / 41.440 | 37.900 / 113.862 | 0 / 0 / 0 |
| strings-64-cold | 35.271 / 64.192 | 55.791 / 173.583 | 36.970 / 62.862 | 0 / 0 / 0 |
| strings-256-cold | 242.515 / 638.074 | 191.544 / 666.951 | 126.112 / 158.013 | 0 / 0 / 0 |
| fx-32-cold | 44.510 / 97.073 | 50.891 / 119.204 | 40.201 / 88.222 | 0 / 0 / 0 |
| fx-64-cold | 114.902 / 258.125 | 88.412 / 234.216 | 64.221 / 113.413 | 0 / 0 / 0 |
| fx-256-cold | 258.395 / 551.561 | 187.534 / 332.799 | 167.254 / 254.675 | 0 / 4 / 4 |

The 64-frame target is still unmet. Storage warm medians are piano 39.281, Vista 34.991, and ANALOG STRINGS 114.672 µs versus v1 6.920, 3.500, and 45.571 µs. Streaming robustness also remains unproven: this storage run has 14 piano cold/256, five ANALOG STRINGS warm/256, and four ANALOG STRINGS cold/256 underruns.

## Finding 3: common streaming and DSP admission

Checkpoint `aaa811db` reserves incoming cursor demand before voice stealing. Current voices and each newly requested page are protected together; storage refusal preserves live DSP voices, without audio allocation/free. Tests exercise incompatible assets, high pitch and crossfade source windows, refusal before stealing, and 300 distinct live assets without a hidden 256-voice ceiling. Plugin nominal streaming capacity is now 1024 voices, matching v1, with DSP growth and release reserves bounded by the same ceiling. The default sampler-kontakt API policy remains unchanged. Later pitch/control changes can still widen a live horizon and return a service capacity error; initial admission cannot predict future controls.

Full sampler-core and sampler-pool-dependent tests pass; 27 sound seam tests pass (one ignored); default `cargo test --no-run` passed before pushing. Frozen admission binary SHA256 `eeb8f58eac6d2d7191c06e2f79554f89ed1a805e692779e7db55ee7801e66a58`. No voice-count reduction occurred on the audited cells. All 18 runs have zero event/render heap calls and cold file pages verify zero after eviction. This is a correctness improvement; these measurements do not establish CPU improvement.

| Cell | Integrated baseline median / p99 | Admission median / p99 | Underruns before / after |
|---|---:|---:|---:|
| piano-32 | 14.371 / 34.811 | 16.370 / 34.001 | 0 / 0 |
| piano-64 | 24.391 / 46.341 | 24.660 / 47.721 | 0 / 0 |
| piano-256 | 80.182 / 117.203 | 90.862 / 140.342 | 0 / 0 |
| strings-32 | 26.850 / 51.681 | 25.771 / 46.811 | 0 / 0 |
| strings-64 | 36.810 / 60.472 | 37.821 / 78.042 | 0 / 0 |
| strings-256 | 130.933 / 161.854 | 147.543 / 245.284 | 0 / 0 |
| fx-32 | 48.131 / 110.763 | 47.001 / 103.202 | 0 / 0 |
| fx-64 | 75.432 / 129.243 | 80.142 / 150.983 | 0 / 0 |
| fx-256 | 175.543 / 253.096 | 177.623 / 256.015 | 0 / 0 |
| piano-32-cold | 16.671 / 37.311 | 27.681 / 146.333 | 0 / 0 |
| piano-64-cold | 23.380 / 43.701 | 38.791 / 346.046 | 0 / 0 |
| piano-256-cold | 95.602 / 215.974 | 145.093 / 285.225 | 0 / 0 |
| strings-32-cold | 37.900 / 113.862 | 25.560 / 44.691 | 0 / 0 |
| strings-64-cold | 36.970 / 62.862 | 39.520 / 74.072 | 0 / 0 |
| strings-256-cold | 126.112 / 158.013 | 136.682 / 225.565 | 0 / 0 |
| fx-32-cold | 40.201 / 88.222 | 45.961 / 112.032 | 0 / 0 |
| fx-64-cold | 64.221 / 113.413 | 118.313 / 394.567 | 0 / 0 |
| fx-256-cold | 167.254 / 254.675 | 210.304 / 382.527 | 4 / 5 |

## Admission regression gate: diagnosis and rejected intermediate

`aaa811db` is held from landing. Audio-TID `perf cycles:u` profiles at 9999 Hz and source tracing identify extra per-start two-pass live demand service, a scalar 4096-frame incoming demand walk, sorted-key insertion/removal and decoder wakes. No new futex or memmove leaf was sampled; userspace-only cycles do not exclude syscalls. V1 (`0cb7a8a0`, engine/mod.rs:1865–1955) admitted a streamed voice through `free.pop()` of a preallocated ring slot and returned it at voice end.

The replacement start uses geometric page credits and one total, then queues no page job or worker wake. Normal block service owns requests. Immutable residency generations replace audio-side locks; the last reader only decrements a counter and control collection destroys old PCM. Existing render completion loops refresh each live credit as its cursor advances; pitch edits refresh their affected projection before subsequent starts. A failing-first two-page-cache test caught the initial stale-credit implementation admitting a third page. Geometry accounts for the last read at N-1 rather than a phantom next output frame, and native step-one integer positions need no interpolation guard.

The intermediate `admission-corrected2` (SHA256 `ecb1946d87ad2ca7f82eeccf42727e6d40af66e3b993acfeb1572299e00e0313`) keeps correctness but fails the CPU gate. Before/after binaries alternate in one window, with reverse order in repeat 2. All runs have zero event/render heap calls, identical peak voice counts, nonzero audio peaks and verified zero cold file pages after eviction. Numbers are steady median / p99 in microseconds.

| Cell | Repeat | Integrated baseline | Corrected2 | Underruns before / after |
|---|---:|---:|---:|---:|
| piano cold/64 | 1 | 22.591 / 41.121 | 30.781 / 66.871 | 0 / 0 |
| piano cold/64 | 2 | 22.990 / 44.081 | 23.190 / 50.011 | 0 / 0 |
| piano cold/64 | 3 | 23.670 / 50.321 | 24.400 / 48.901 | 0 / 0 |
| fx cold/64 | 1 | 77.231 / 169.404 | 70.272 / 121.553 | 0 / 0 |
| fx cold/64 | 2 | 82.852 / 160.293 | 63.421 / 115.912 | 0 / 0 |
| fx cold/64 | 3 | 81.151 / 174.353 | 80.131 / 148.043 | 0 / 0 |

Median-of-run medians / median-of-run p99s: piano 22.990 / 44.081 → 24.400 / 50.011 (regresses); FX 81.151 / 169.404 → 70.272 / 121.553 (improves); total underruns zero on both sides. Warm FX/64 in the same window is 80.122 / 151.583 → 91.552 / 198.464, also a regression. This intermediate is not accepted. Redundant ownership lookups in each credit refresh are the next measured change; no claim that scheduler noise explains these results.

## Corrected admission checkpoint

The current implementation reuses the render loop's existing projected pitch instead of traversing family/note/expression ownership again for every credit update. All core/pool tests pass; Kontakt has 37 passed/3 ignored and the adapter has 27 passed/1 ignored; default `cargo test --no-run` passed. There are 12 stream-policy tests, including cursor advance, pitch edits, no-start-page-jobs, last-reader heap freedom, offline readiness and retries. Frozen binary SHA256 `66e04227bcc50c0b1fb449f3b5867f24a74eabc0d07ce6221dea0e31c313dac6`.

| Cell | Repeat | Integrated baseline median / p99 | Corrected median / p99 | Underruns before / after |
|---|---:|---:|---:|---:|
| piano cold/64 | 1 | 23.660 / 42.311 | 23.531 / 42.341 | 0 / 0 |
| piano cold/64 | 2 | 26.461 / 52.701 | 25.490 / 45.101 | 0 / 0 |
| piano cold/64 | 3 | 27.241 / 52.161 | 35.391 / 92.502 | 0 / 0 |
| fx cold/64 | 1 | 78.441 / 153.553 | 74.681 / 143.192 | 0 / 0 |
| fx cold/64 | 2 | 82.072 / 159.433 | 73.451 / 145.612 | 2 / 0 |
| fx cold/64 | 3 | 70.511 / 165.283 | 82.661 / 163.153 | 0 / 0 |

Median-of-run medians / median-of-run p99s improve in this matched window: piano **26.461 / 52.161 → 25.490 / 45.101**, FX **78.441 / 159.433 → 74.681 / 145.612**. Total underruns are piano 0 → 0 and FX 2 → 0. Warm FX/64 is **79.341 / 149.183 → 81.001 / 148.963**, underruns 0 → 0. All event/render heap counts are zero; cold eviction has zero file pages remaining; peak voices stay 12 and 24.

**HOLD remains:** repeat 3 regresses (piano median and p99, FX median), warm FX median increases, and the historical absolute baseline and v1 target are not met. These results do not prove every regression is scheduler noise. The corrected audio-TID piano profile has zero lost samples; protection falls to 1.82% of sampled userspace cycles, and admission/reservation no longer appears above the 1% leaf threshold. Function inlining and separate windows prohibit treating that threshold as a complete cost comparison.

## Full corrected admission matrix (fresh alternating cells)

Before is frozen integrated `94448100…`; after is `c2cfa510`, frozen `66e04227…`. All 36 event/render heap counters are zero; all cold evictions leave zero file pages, peak audio is nonzero and peak voice counts agree. Each entry is steady block median / p99 in microseconds. Every run obtains its own FIFO slot; the tighter follow-up will hold one bounded shard for consecutive pairs.

| Cell | Before | After | Underruns before / after |
|---|---:|---:|---:|
| piano/32 warm | 23.590 / 74.001 | 14.830 / 29.971 | 0 / 0 |
| piano/64 warm | 27.350 / 52.081 | 28.611 / 49.841 | 0 / 0 |
| piano/256 warm | 91.932 / 135.352 | 100.652 / 193.764 | 0 / 0 |
| strings/32 warm | 22.190 / 38.630 | 32.231 / 62.361 | 0 / 0 |
| strings/64 warm | 39.161 / 81.191 | 39.661 / 86.212 | 0 / 0 |
| strings/256 warm | 141.512 / 229.674 | 133.212 / 205.904 | 0 / 0 |
| fx/32 warm | 74.991 / 406.448 | 50.041 / 123.382 | 0 / 0 |
| fx/64 warm | 83.262 / 166.853 | 98.921 / 269.735 | 0 / 0 |
| fx/256 warm | 298.656 / 1188.382 | 276.685 / 870.826 | 0 / 2 |
| piano/32 cold | 15.910 / 35.060 | 17.280 / 37.711 | 0 / 0 |
| piano/64 cold | 26.040 / 51.691 | 26.560 / 59.262 | 0 / 0 |
| piano/256 cold | 89.231 / 139.493 | 90.551 / 141.523 | 0 / 0 |
| strings/32 cold | 27.401 / 50.861 | 39.540 / 347.947 | 20 / 0 |
| strings/64 cold | 40.951 / 65.001 | 41.371 / 62.541 | 0 / 0 |
| strings/256 cold | 125.242 / 165.453 | 228.954 / 360.637 | 0 / 0 |
| fx/32 cold | 48.841 / 107.712 | 47.721 / 114.693 | 0 / 0 |
| fx/64 cold | 82.541 / 174.944 | 84.272 / 174.323 | 0 / 0 |
| fx/256 cold | 188.764 / 318.686 | 314.126 / 1526.258 | 1 / 5 |

This full matrix **fails acceptance**: several CPU cells and FX/256 underruns regress. The cold64 historical spikes are reduced but no noise-only conclusion is established. Do not land this checkpoint. The remaining sorted page-index shifts in normal block service are the next admission mechanism to measure and remove.

The synthetic `page_admission` probe excludes decoding from its request timer and asserts zero allocations/frees across request, decode ownership transfer and completion. Before removing vector shifts, 768 slots have fill 0.150 / 1.310 µs and churn 0.200 / 0.290 µs; 6144 slots have fill 0.870 / 2.450 µs and churn 0.900 / 1.690 µs. Frozen probe SHA256 `f14d6b5d7ec0a34c5c38ea98dc31eaf703382f8ff3507f63c4e3b28c0e29d4cc`. Synthetic logical fill/churn is separate from the library warm/cold disk-cache cells above.

## Fixed page index (finding 7, admission gate remains held)

The sorted key vector is replaced by a fixed bucket table with intrusive slot links. Insert and removal visit only a bucket, with expected amortized O(1) work; neither shifts other resident keys. A per-epoch protected count makes saturated refusal constant work. The existing installed rustc-hash dependency avoids SipHash on process-owned asset/page keys. Collision-chain removal/reuse, repeated hits, saturated refusal, stale completions and existing heap guards pass. Full sampler-core tests and default `cargo test --no-run` passed for the final hash implementation.

| Pool slots / phase | Sorted vector median / p99 µs | Fixed index median / p99 µs | Heap calls before / after |
|---|---:|---:|---:|
| 768 / fill | 0.150 / 1.310 | 0.040 / 1.120 | 0 / 0 |
| 768 / churn | 0.200 / 0.290 | 0.050 / 0.090 | 0 / 0 |
| 6144 / fill | 0.870 / 2.450 | 0.040 / 1.330 | 0 / 0 |
| 6144 / churn | 0.900 / 1.690 | 0.050 / 0.140 | 0 / 0 |

A preliminary DefaultHasher build was profiled on the cold64 audio TID: piano 958 samples / 318.9M cycles, DSP 25.32%, filtered source 9.45%, bus 9.42%, hashing 1.78%; FX approximately 3K samples / 1,365.6M cycles, behavior VM 18.60%, DSP 11.77%, bus 6.41%, SipHash 1.68% and bucket 1.58%. These profiles have zero lost samples. They motivated using the existing faster hasher, but do not prove the final library CPU gate. That preliminary build measured piano 24.310 / 47.981 and FX 123.172 / 570.841 µs steady median / p99, both zero underruns and zero event/render heap calls; it is rejected.

The preliminary paired run failed with exit 124 in the **before** integrated warm FX binary after a 90-second timeout, with no stderr. Its failure is retained, not scored as a successful comparison or attributed to the new index. Final fixed-index warm/cold pairs are running consecutively inside bounded heavy shards, reversing binary order in repeat 2, with the audit's original collectors and cold-cache helper. Every failure remains visible. **HOLD remains; v1 and the historical cold64 targets are not met.** The canonical frozen v1 reference currently has no CPU-audit adapter binary; the audit's historical v1 medians/p99 remain the comparison until that original binary can be recovered without rebuilding v1.

## Direct slot selection (admission hold continues)

The remaining clock search is linear even with the fixed page index: one idle slot behind 6,143 protected slots costs 7.630 / 9.970 µs median / p99. The new allocator ports v1 `0cb7a8a0:src/engine/mod.rs` free-slot `pop`/`push` operations and adapts them to shared decoded pages. Idle and current-epoch chains permit direct replacement; epoch rollover splices a chain in constant work. Admission never searches the slot array. A collision bucket still has expected amortized O(1) lookup. This is allocator reuse, not a completed port of v1's virtual per-voice rings.

| Pool slots / phase | Clock median / p99 µs | Direct selection median / p99 µs | Heap calls before / after |
|---|---:|---:|---:|
| 768 / fill | 0.040 / 1.080 | 0.040 / 1.100 | 0 / 0 |
| 768 / churn | 0.050 / 0.090 | 0.050 / 0.090 | 0 / 0 |
| 768 / protected churn | 0.980 / 1.260 | 0.040 / 0.060 | 0 / 0 |
| 6144 / fill | 0.040 / 1.370 | 0.050 / 1.480 | 0 / 0 |
| 6144 / churn | 0.050 / 0.150 | 0.050 / 0.180 | 0 / 0 |
| 6144 / protected churn | 7.630 / 9.970 | 0.080 / 0.220 | 0 / 0 |

Epoch splicing, protected-page preservation, free-slot reuse, oldest idle replacement and list ownership have a regression check. Existing collision, stale-completion, heap-free, offline readiness, retry and admission tests pass; default `cargo test --no-run` passes. The additional horizon-change test refuses new work when a live source's widened horizon consumes capacity. Native kernels, amp routing and Cursor cold behavior are untouched.

Final frozen candidate SHA256 `30f9aa2ab224c50c9fecac731f6b89f985c6118ffcadae4a31fdff92e036f5a5`. Each pair holds one bounded heavy shard and reverses binary order in repeat 2. Before is integrated `94448100…`. All event/render heap counts are zero, every cold eviction has zero file pages remaining, and all peak voice counts agree (piano 12, FX 24). Numbers are steady block median / p99 in microseconds. V1 is the historical audit reference, **not** a simultaneous third binary.

| Cell | Repeat | Integrated before | Direct selection after | v1 historical reference | Underruns before / after |
|---|---:|---:|---:|---:|---:|
| piano/64 cold | 1 | 30.910 / 61.941 | 25.430 / 50.621 | 12.170 / 30.750 | 0 / 0 |
| piano/64 cold | 2 | 27.341 / 52.691 | 27.860 / 57.491 | 12.170 / 30.750 | 0 / 0 |
| piano/64 cold | 3 | 26.780 / 56.001 | 39.301 / 139.803 | 12.170 / 30.750 | 0 / 0 |
| piano/64 warm | 1 | 27.100 / 60.871 | 25.990 / 61.821 | 6.920 / 21.111 | 0 / 0 |
| piano/64 warm | 2 | 26.800 / 50.741 | 25.390 / 47.171 | 6.920 / 21.111 | 0 / 0 |
| piano/64 warm | 3 | 27.151 / 52.041 | 28.600 / 83.011 | 6.920 / 21.111 | 0 / 0 |
| fx/64 cold | 1 | 80.131 / 155.273 | 76.521 / 159.613 | 68.431 / 241.434 | 0 / 0 |
| fx/64 cold | 2 | 84.021 / 167.453 | 83.632 / 168.743 | 68.431 / 241.434 | 0 / 0 |
| fx/64 cold | 3 | 81.102 / 175.984 | 82.432 / 168.643 | 68.431 / 241.434 | 0 / 0 |
| fx/64 warm | 1 | 78.411 / 167.133 | 83.192 / 183.993 | 45.571 / 84.912 | 0 / 0 |
| fx/64 warm | 2 | 84.191 / 176.193 | 79.701 / 167.603 | 45.571 / 84.912 | 0 / 0 |
| fx/64 warm | 3 | 79.522 / 162.823 | 108.132 / 244.775 | 45.571 / 84.912 | 0 / 0 |

**This candidate fails the library CPU gate and remains held.** Median-of-run median/p99: cold piano 27.341 / 56.001 → 27.860 / 57.491; cold FX 81.102 / 167.453 → 82.432 / 168.643; warm piano 27.100 / 52.041 → 25.990 / 61.821; warm FX 79.522 / 167.133 → 83.192 / 183.993. Total underruns are zero on both sides. The historical absolute integrated cold64 baseline (piano 23.380 / 43.701, FX 64.221 / 113.413) and v1 targets are unmet. Piano repeat 3 regresses strongly. The structural O(1) result does not prove that the library tails are noise. The remaining 32/256/Vista cells and final scanner gate have not been measured for this candidate.

The between-page credit-cache experiment was rejected and removed before this candidate. Its cold median-of-run median/p99 was piano 25.291 / 48.061 → 25.381 / 55.071 and FX 78.291 / 163.583 → 111.842 / 261.875, all cold underruns zero. Its horizon negative control reproduced stale credits, but its additional fields and helpers did not meet the CPU gate. Frozen binary `0c2b496e…` and source snapshot `refs/wip/v2/fix-cpu/20261008T102704Z` retain provenance outside production. The fixed-index-only series is also retained: piano cold median 28.510 / 61.841 → 39.010 / 71.302; one baseline cold FX timeout (124) invalidates that pair, and warm FX repeat 2 has one after underrun and a 2534.787 µs p99. No failed pair is treated as a pass.

## Worker priority queue and returned-buffer wake (still held)

Increasing the page pool also amplified the off-audio worker's full pending-array scan. Empty `next_job` calls cost 0.340 / 0.460 µs at 768 slots and 3.360 / 4.041 µs at 6,144 slots. A preallocated bounded deadline heap selects pending work, coalesces serial/priority updates and discards stale entries; a worker-only rebuild bounds duplicate priorities at twice the pool size. Empty calls now cost 0.020 / 0.020 and 0.020 / 0.030 µs respectively. All synthetic transfer/admission calls remain heap free. Ordinary page batches wake decoders separately from the head reloader; cold starts retain their reloader wake.

A failing-first test reproduced a queued decoder sleeping after its only stale buffer was returned: exit 101, then green after returned buffers request a wake. Tests cover bounded priority updates with all storage in flight, unchanged allocation capacity, oldest serial rejection, deadline order, no duplicate launches and zero allocations/frees through repeated priority rebuilds. Six internal admission tests and seven stream-cache integration tests pass; Kontakt and default root no-run pass for this source. Source cold-hold behavior, DSP lanes and amp routing are untouched.

Candidate SHA256 `81646c56086c00fd4953006dd5ba68ce06d3cfc7f9668d80c45cbde665cd96b0`; before is frozen integrated `94448100…`. Each consecutive pair has one bounded heavy slot; repeat 2 reverses order. All cold evictions verify zero remaining Linux file pages, and all event/render heap counts are zero. Times are steady median / p99 in µs.

| Cell | Repeat | Integrated before | Worker queue after | Underruns before / after |
|---|---:|---:|---:|---:|
| piano/64 cold | 1 | 39.900 / 1201.592 | 39.271 / 157.393 | 0 / 0 |
| piano/64 cold | 2 | 42.520 / 144.803 | 39.920 / 74.771 | 0 / 0 |
| piano/64 cold | 3 | 25.000 / 47.960 | 21.650 / 39.350 | 0 / 0 |
| piano/64 warm | 1 | 26.541 / 59.241 | 26.371 / 47.051 | 0 / 0 |
| piano/64 warm | 2 | 22.870 / 43.741 | 31.480 / 60.301 | 0 / 0 |
| piano/64 warm | 3 | 24.531 / 46.581 | 23.650 / 41.900 | 0 / 0 |
| fx/64 cold | 1 | 119.873 / 491.220 | 81.211 / 158.533 | 0 / 0 |
| fx/64 cold | 2 | 77.371 / 141.162 | 77.281 / 160.753 | 0 / 0 |
| fx/64 cold | 3 | 72.972 / 129.263 | 85.431 / 172.353 | 0 / 8 |
| fx/64 warm | 1 | 79.532 / 164.723 | 101.992 / 209.664 | 0 / 0 |
| fx/64 warm | 2 | 77.691 / 162.433 | 69.191 / 129.432 | 0 / 0 |
| fx/64 warm | 3 | 79.932 / 171.844 | 74.722 / 149.953 | 0 / 0 |

**FAIL/HOLD:** cold piano median-of-run median/p99 improves 39.900 / 144.803 → 39.271 / 74.771; cold FX worsens 77.371 / 141.162 → 81.211 / 160.753 and underruns 0 → 8. Warm piano 24.531 / 46.581 → 26.371 / 47.051; warm FX 79.532 / 164.723 → 74.722 / 149.953, underruns zero. Cold piano repeat 3 reaches 21.650 / 39.350, below the historical integrated target, but the repeated aggregate does not. The historical cold64 targets (piano 23.380 / 43.701, FX 64.221 / 113.413) and v1 target remain unmet. No noise-only conclusion follows. The after FX cold run with eight underruns also took 59.378 s to load; before warm FX repeat 3 took 81.270 s. These stalls are retained, not excluded or used to excuse failed streaming. Remaining 32/256/Vista cells and final scanner gate are unmeasured for this held checkpoint.

The follow-up cold64 profiles (`cycles:u`, 9,999 Hz, audio TID only, zero lost samples) measured integrated → candidate audio cycles: piano 301,970,179 → 264,793,596; FX 959,254,129 → 824,726,286. Sampled stream-service share falls piano 6.53% → 4.29%, FX 6.73% → 3.73%. The main candidate leaf costs are piano DSP 22.98%, buses 12.09%, filtered source 12.08%; FX behavior VM 22.69%, DSP 9.36%, buses 7.64%. No new lock/syscall leaf is established by these userspace samples; kernel waits and inline costs remain outside that claim. Profile cell median/p99: piano 25.221 / 48.541 → 25.481 / 46.891; FX 88.032 / 181.354 → 84.631 / 164.763, all underruns zero. Profile overhead and separate FIFO acquisitions make these attribution cells distinct from the bounded A/B gate.

`CPU_AUDIT_TRACE_STREAM=1` now optionally samples aggregate underruns outside the timer/allocation-counted section of the same audit collector. Its JSON lists `(output frame, new underruns, timed block ns)` only when the count changes; it is explicitly diagnostic, not scored as an original performance cell. Three candidate cold FX diagnostics all report zero underruns (loads 30.904 / 10.717 / 7.810 s); therefore they did **not** reproduce or explain the eight-underrun failure. The original failed streaming result remains open and held. No library sample/script bytes are recorded.

## Integration 9993db69 and native-slot admission

The branch is updated to integration `9993db69`, preserving the cancelable lazy-head loader, native loop slots/cold hold, restored controls and W9's lock-free residency plus 1,024-stream/DSP admission budget. The existing constructor already normalizes one untuned native slot to the fast single-loop cursor; an initial concern about that path was incorrect, and there is no W9 production edit to `source.rs`.

A new failing-first check establishes an integration/admission interaction for native tuned repeats: the old fallback reserves **3** pages while the source reads **15**. Admission now takes the maximum possible rate and physical-leg bound across the fixed eight slots, clipped to the source view's page count. This remains constant work in horizon length, allocation free and conservative; uncommon multi-slot sources can reserve more pages than their immediate exact footprint. Serial wrap/crossfade/ping-pong slots, tuning 0.5/4/16, finite passes, both directions and release exits cover every actual read in the regression property. The 14 targeted native core tests pass; 46 source/page-render/cache/policy tests, Kontakt no-run and default root no-run pass.

New frozen sound-seam candidate SHA256 `8e264ab72e60495206af8749ea9e4668faec0649f5e1af953db5c48612003fb3`. Fresh piano/Vista/FX 64-frame warm/cold pairs are pending. The comparison includes the integration graph/selection/load changes and therefore cannot isolate W9's native-slot credit correction. CPU and streaming acceptance remain **HOLD**, including the earlier eight-underrun failure. The original frozen pre-admission binary remains intact.

## Foundation comparison: integration 9993db69

Frozen `88b89722` / integration `1ed8c470` before versus W9 `f3093c08` / integration `9993db69` after. This includes other owners’ routing, gain, selection, lazy-head, and load fixes, so it is **not an isolated admission comparison**. Three alternating repeats (before/after, after/before, before/after), original audit collectors and cache eviction. All 36 runs completed and report zero event/render heap calls; cold file-cache eviction reports zero pages after eviction.

| Cell | Repeat | Before median / p99 µs | After median / p99 µs | Underruns before / after |
|---|---:|---:|---:|---:|
| piano-64-cold | 1 | 25.031 / 47.781 | 43.651 / 65.291 | 0 / 0 |
| piano-64-cold | 2 | 26.580 / 48.741 | 46.000 / 80.531 | 0 / 0 |
| piano-64-cold | 3 | 26.390 / 54.031 | 44.061 / 72.571 | 0 / 0 |
| piano-64-warm | 1 | 23.980 / 50.441 | 45.041 / 66.611 | 0 / 0 |
| piano-64-warm | 2 | 24.930 / 49.151 | 37.981 / 59.411 | 0 / 0 |
| piano-64-warm | 3 | 39.321 / 305.476 | 64.422 / 108.152 | 0 / 0 |
| strings-64-cold | 1 | 38.071 / 67.261 | UNKNOWN (no steady voices) | 0 / 0 |
| strings-64-cold | 2 | 56.321 / 287.655 | UNKNOWN (no steady voices) | 0 / 0 |
| strings-64-cold | 3 | 55.731 / 233.285 | UNKNOWN (no steady voices) | 0 / 0 |
| strings-64-warm | 1 | 35.020 / 56.921 | UNKNOWN (no steady voices) | 0 / 0 |
| strings-64-warm | 2 | 55.381 / 88.821 | UNKNOWN (no steady voices) | 0 / 4 |
| strings-64-warm | 3 | 58.991 / 163.873 | UNKNOWN (no steady voices) | 0 / 0 |
| fx-64-cold | 1 | 81.461 / 162.433 | 43.651 / 96.472 | 0 / 0 |
| fx-64-cold | 2 | 70.931 / 141.913 | 44.761 / 88.522 | 0 / 0 |
| fx-64-cold | 3 | 65.341 / 119.242 | 76.672 / 256.475 | 0 / 0 |
| fx-64-warm | 1 | 70.371 / 137.013 | 48.141 / 98.372 | 0 / 0 |
| fx-64-warm | 2 | 70.032 / 118.272 | 48.481 / 105.232 | 0 / 0 |
| fx-64-warm | 3 | 87.301 / 196.124 | 51.731 / 121.922 | 0 / 0 |

**HOLD.** Piano median-of-run median / p99 regresses: cold 26.390 / 48.741 → 44.061 / 72.571 µs; warm 24.930 / 50.441 → 45.041 / 66.611 µs. ANALOG STRINGS improves in aggregate (cold 70.931 / 141.913 → 44.761 / 96.472; warm 70.371 / 137.013 → 48.481 / 105.232), but cold repeat 3 regresses and the v1 warm target remains unmet. Piano and ANALOG STRINGS have zero underruns in these runs.

Vista after has no sustained voices during the audit steady interval in all six runs: peak voices 64, mean ~0.901 versus before peak 80, mean ~9.565; output peak ~0.003 versus ~0.108. Warm repeat 2 after has four underruns. This is a functional regression requiring the owning signal-graph trace, **not a CPU win**. W6 and coordinator have been notified. Piano peak also changes slightly; ANALOG STRINGS peak approximately halves. No PCM or native fidelity certification follows from this comparison.

Evidence: `~/.cache/kontakto-fix-cpu/integrated-9993-pairs/{1,2,3}/{before,after}/`. Frozen after binary SHA256 `8e264ab72e60495206af8749ea9e4668faec0649f5e1af953db5c48612003fb3`. A subsequent isolated optimization must compare against this same foundation.

## Exact neutral filter-factor projection

Isolated candidate after `f3093c08`, binary `admission-filter-identity` SHA256 `52eb977d48d61d86fee060eaaf1d2913d682952f6bace4705e7e80deb13c9317`. The measured before audio-thread profile spends 15.62% in full-bank projection, 14.32% in `pow`, and 9.36% in `exp2`; stream service is 2.11%. The candidate returns exact unity for zero accumulated modulation and preserves all nonzero f64 math. This is a performance change; its failing-before witness is the audited timing/profile, not a claimed failing semantic test. A new unit fixture checks midpoint summation, cancelling routes and voice reset. Existing filter/modulation/lowering tests pass (38 passed, one unrelated ignored), and default `cargo test --no-run` passes.

Original audit64 collectors, three alternating A/B repeats on the same foundation. Values are median / p99 µs. All24 runs have zero event/render heap calls; all cold evictions report zero file pages after eviction.

| Cell | Repeat | Before median / p99 | After median / p99 | Underruns before / after |
|---|---:|---:|---:|---:|
| piano-64-cold | 1 | 47.701 / 85.041 | 32.640 / 54.481 | 0 / 0 |
| piano-64-cold | 2 | 45.941 / 76.161 | 30.181 / 58.471 | 0 / 0 |
| piano-64-cold | 3 | 47.081 / 86.412 | 31.400 / 53.321 | 0 / 0 |
| piano-64-warm | 1 | 62.921 / 104.632 | 29.591 / 52.711 | 0 / 0 |
| piano-64-warm | 2 | 45.051 / 73.362 | 31.730 / 56.111 | 0 / 0 |
| piano-64-warm | 3 | 45.511 / 71.731 | 29.941 / 53.561 | 0 / 0 |
| fx-64-cold | 1 | 48.591 / 120.652 | 57.461 / 118.722 | 0 / 0 |
| fx-64-cold | 2 | 53.131 / 114.732 | 54.101 / 113.282 | 0 / 0 |
| fx-64-cold | 3 | 50.931 / 118.253 | 54.711 / 116.732 | 0 / 0 |
| fx-64-warm | 1 | 53.061 / 112.862 | 51.291 / 112.772 | 0 / 0 |
| fx-64-warm | 2 | 51.131 / 108.242 | 51.361 / 106.432 | 0 / 0 |
| fx-64-warm | 3 | 52.431 / 112.522 | 51.501 / 115.912 | 0 / 0 |

**HOLD.** Piano improves consistently but still exceeds the historical `1ed8c470` cold baseline (23.380 / 43.701 µs). ANALOG STRINGS does not establish an aggregate CPU improvement. The whole-bank factor walk remains for the next profile to assess. Vista is unscorable on this foundation and has not been relabeled as a timing success; remaining audited32/256 cells are not measured for this candidate.

Evidence: `/home/derpcat/.cache/kontakto-fix-cpu/filter-identity-pairs/{1,2,3}/{before,after}/`.

Follow-up exact audio-TID profile: approximate sampled cycles 510,499,115 → 350,520,014; no lost samples in either run. `pow` and `exp2` are absent above the 1% reporting threshold; `fill_filter_factors` remains 8.88%. This is explanatory profiling, not an acceptance substitute for the paired timings. Profile evidence: `~/.cache/kontakto-fix-cpu/{integrated-9993-profile,filter-identity-profile}/`.

## Sparse addressed filter-factor projection

Candidate `admission-filter-sparse` SHA256 `0f49127d32f1ae0e195329326c21bd20d288fdb30cc84132de83769015ce0b3b`, before frozen `6f41449f` neutral-only candidate. Compile unique addressed filter indices off the audio thread; reset only the preceding program’s targets, then sum/project only the current program’s targets. No per-voice state expansion, audio allocations or locks. The PCM fixture compares two voices (addressed/unaddressed) against separate static-filter oracles through voice release and block sizes1/17/64/128. The factor unit and 39 filter/modulation/lowering integration tests pass (one unrelated ignored); default no-run passes.

Three alternating original-audit64 pairs, same foundation. All24 event/render heap counters are zero and every cold eviction reports zero remaining file pages.

| Cell | Repeat | Before median / p99 µs | After median / p99 µs | Underruns before / after |
|---|---:|---:|---:|---:|
| piano-64-cold | 1 | 47.551 / 2431.645 | 27.570 / 49.041 | 0 / 0 |
| piano-64-cold | 2 | 33.021 / 59.341 | 26.820 / 46.281 | 0 / 0 |
| piano-64-cold | 3 | 33.080 / 58.441 | 32.520 / 59.361 | 0 / 0 |
| piano-64-warm | 1 | 32.581 / 56.521 | 24.421 / 45.821 | 0 / 0 |
| piano-64-warm | 2 | 31.701 / 53.361 | 28.541 / 52.771 | 0 / 0 |
| piano-64-warm | 3 | 31.951 / 61.642 | 32.630 / 63.442 | 0 / 0 |
| fx-64-cold | 1 | 56.931 / 131.093 | 51.781 / 111.672 | 0 / 1 |
| fx-64-cold | 2 | 53.141 / 117.612 | 48.851 / 99.982 | 0 / 1 |
| fx-64-cold | 3 | 55.351 / 126.572 | 77.002 / 200.884 | 0 / 0 |
| fx-64-warm | 1 | 53.271 / 114.642 | 52.680 / 116.842 | 0 / 0 |
| fx-64-warm | 2 | 55.111 / 112.422 | 52.521 / 114.862 | 0 / 0 |
| fx-64-warm | 3 | 68.411 / 175.634 | 55.071 / 122.392 | 0 / 0 |

**HOLD.** Piano cold aggregate improves33.080 /59.341 →27.570 /49.041 µs but exceeds historical baseline23.380 /43.701. ANALOG STRINGS cold aggregate55.351 /126.572 →51.781 /111.672 meets the historical CPU threshold in aggregate, but repeat3 after regresses and there are two after underruns versus zero before. Warm piano31.951 /56.521 →28.541 /52.771; warm FX55.111 /114.642 →52.680 /116.842 (p99 regression). No streaming or v1 gate pass. All failures, including piano before repeat1 p99=2431.645 µs, are retained.

Evidence: `/home/derpcat/.cache/kontakto-fix-cpu/filter-sparse-pairs/{1,2,3}/{before,after}/`. Profiles and separately tagged underrun-timing diagnostics follow;32/256 and Vista acceptance remain unmeasured/unscorable for this candidate.

Completed sparse-factor audio-TID profiles (explanatory, not the acceptance pairs): piano37.011 /76.922 →31.901 /68.052 µs with3 →0 underruns; ANALOG STRINGS59.401 /117.452 →53.831 /121.672 with0 →0. Piano’s full-bank factor walk goes8.54% →below1%; after DSP18.92%, bus11.19%, resampler window8.75%, stream service3.38%. ANALOG STRINGS stream service remains4.54% →4.51%, DSP13.50% →11.61%. Approximate sampled cycles **piano357,687,816 →382,201,279 (worse)**; FX623,528,342 →598,652,632. No lost samples. All profiles use frozen binaries at the same foundation. They do not certify the timing gate. Evidence: `~/.cache/kontakto-fix-cpu/filter-sparse-profile/{before,after}/`.

Disk fell below18GiB after own idle incrementals were pruned. The profiling unit was mistakenly started below that boundary; its four profiles completed before it was stopped, and only the first-before stream diagnostic completed. The incomplete after diagnostic is retained and unscored. New heavy jobs are held until headroom returns; no foreign targets were pruned.

## Script and gain/pitch lane admission (unverified working change)

The private eligibility fixture fails on the preceding commit for gain modulation and passes with the working change. It allows gain/pan/pitch/start programs and script layers while retaining scalar admission for per-voice filter/tone programs. The shared amp helper uses existing native envelope levels and Ramp interpolation; streamed reservation refresh retains the base-step bound. The97-voice PCM oracle now covers resident and preloaded streamed sources crossing a4096-frame page boundary,1/3 threads, block sizes1/17/64/128, gain/pan/pitch edits, fades and release, with heap/underrun assertions. **The expanded PCM oracle, area tests, default no-run and timing are pending; this working change is not pushed or accepted.** Disk15.38GiB is below the18GiB new-job floor. Own idle incrementals are empty and permission to prune broader own debug artifacts remains pending; frozen before/after benchmark ELFs are preserved outside target.

Static cold-start correctness finding (not yet executed): PreparedVoiceChain::begin passes a temporary unity envelope into the held cursor; later dsp::levels advances the real voice envelope for those silent frames. The new paged_render cold-start fixture admits both oracles cold, publishes one page immediately to one and128 frames later to the other, then compares their first256 resumed frames. Both have the same cold fade-in. Cases cover no-chain control, scalar chain and two-voice lane chain, with zero-heap/underrun assertions. W8/W6 own the shared cursor/DSP seam and have been notified. No source.rs or DSP implementation changes made by W9. This fixture is pending the same disk floor; no failing-run claim.

W6 local cold-hold checkpoint0af58a6d pauses the envelope, processor state, sends and choke clock and routes waiting voices through scalar rendering; it is unverified/unpushed and not merged into W9. Static W9 review found its64-voice fixture only allocated two page reservations, so it would fail admission before exercising the hold. W6 corrected the fixture in local78af0703 to voices.max(2). This is a source-review finding, not a failing-run receipt. W9 lane helper needs levels(v,len,0) when the verified W6 dependency lands. The guarded W9 job expired its240-second disk wait without starting a build/test; latest target17.45GiB, own incremental directories absent, no own heavy unit live.

## Lane candidate verification after disk unblock

The97-voice PCM oracle now passes on the working lane change for resident and streamed/page-boundary sources,1/3 threads, and1/17/64/128 blocks, including gain/pan/pitch edits, fades, release, heap guards and zero underruns.50 targeted DSP/multicore/paged/stream/modulation integration tests pass (one unrelated ignored); the new known-broken cold-start regression is excluded from that area run after a separately recorded semantic failure. Default root cargo test --no-run passes. Frozen lane-only audit binary admission-lane-ramps SHA25673f270fe035b935d1d41105a75d624b8735c7c879108944551bc53498381d803 preserves the same4cdc8920 foundation without W6’s cold fix. Three alternating same-window A/B64 pairs against admission-filter-sparse are in progress, so there is no timing acceptance claim yet.

The separate delayed-page regression fails at chained=true/voices=1: resumed first frame0.0013020821 versus the immediate-cold oracle0, confirming envelope consumption during the hold. No-chain controls pass. This is a runtime assertion failure, not a compilation failure. Receipt:~/.cache/kontakto-fix-cpu/cold-chain-red.log. W6 has published verified bbb143bf with implementation0af58a6d and fixture78af0703; merge and W9’s delayed-page green check follow the isolated lane timing.

Own idle incremental cleanup after the build removed1,005,699,072 allocated bytes;25 frozen ELFs outside target were hash-verified first. No foreign target or frozen v1 artifact was changed.

## Completed isolated lane pairs and verified cold-hold merge

Lane checkpoint c94661b8, before4cdc8920. Three alternating same-window original-audit64 pairs, all24event/render heap counters zero and all12 cold evictions pages_after=0. Values are median/p99µs; all repeats retained.

| Cell | Repeat | Before median / p99 | After median / p99 | Underruns before / after |
|---|---:|---:|---:|---:|
| piano-64-cold | 1 | 26.681 / 47.251 | 27.210 / 49.611 | 0 / 0 |
| piano-64-cold | 2 | 27.280 / 52.371 | 26.880 / 49.581 | 0 / 0 |
| piano-64-cold | 3 | 28.191 / 55.141 | 26.480 / 45.831 | 0 / 0 |
| piano-64-warm | 1 | 26.111 / 46.980 | 26.431 / 52.731 | 0 / 0 |
| piano-64-warm | 2 | 26.570 / 47.111 | 26.500 / 45.751 | 0 / 0 |
| piano-64-warm | 3 | 27.551 / 49.351 | 28.310 / 51.731 | 0 / 0 |
| fx-64-cold | 1 | 53.481 / 111.612 | 55.951 / 127.812 | 1 / 0 |
| fx-64-cold | 2 | 52.521 / 123.342 | 54.181 / 114.312 | 0 / 0 |
| fx-64-cold | 3 | 54.061 / 118.083 | 51.631 / 108.632 | 0 / 0 |
| fx-64-warm | 1 | 53.861 / 110.612 | 54.391 / 122.093 | 0 / 0 |
| fx-64-warm | 2 | 53.361 / 119.612 | 52.151 / 112.302 | 0 / 0 |
| fx-64-warm | 3 | 53.051 / 113.172 | 55.561 / 112.892 | 0 / 0 |

**HOLD.** Aggregates: piano cold27.280/52.371→26.880/49.581, warm26.570/47.111→26.500/51.731(p99 worse). FX cold53.481/118.083→54.181/114.312(median worse), warm53.361/113.172→54.391/112.892(median worse). Underruns before1/after0 across these24runs; this does not resolve the earlier two after underruns or prove robust streaming. The lane capability is PCM/heap-correct, but these cells show no substantial uniform CPU improvement. Piano still misses1ed baseline23.380/43.701 and the v1 gate remains unmet. Evidence:~/.cache/kontakto-fix-cpu/lane-ramps-pairs/.

Verified W6 checkpoint bbb143bf is merged into the CPU worktree, retaining W9 amp ramps, prepared pitch, bounded reservations, addressed factors and lock-free residency. Adapted the shared amp helper to levels(v,len,0), with waiting voices excluded from lane admission. W9 delayed-page regression goes semantic RED→GREEN.62 area tests plus publication and eligibility units pass(one unrelated ignored); default root cargo test --no-run passes. Fresh binary admission-cold-hold SHA25616878c71327ee0b1b1d4578177d6f1f8a5435ee3f4194072612fcfaf1901b9ca includes W6’s trace infrastructure and cold fix; its timing comparison is a merged-checkpoint comparison, not an isolated attribution to the hold alone. New FX cold diagnostics/pairs follow.

## Paired verified cold-hold checkpoint

Before c94661b8 frozen admission-lane-ramps; after 765a7db7 frozen admission-cold-hold (SHA25616878c71327ee0b1b1d4578177d6f1f8a5435ee3f4194072612fcfaf1901b9ca). Three same-window AB/BA/AB original-audit64 pairs. W6 trace is disabled during timing; this comparison includes the merged infrastructure and does not isolate the hold alone. All24 runs exit0 with event/render heap0; all12 cold evictions pages_after0.

| Cell | Repeat | Before median / p99 µs | After median / p99 µs | Underruns before / after |
|---|---:|---:|---:|---:|
| piano-64-cold | 1 | 26.380 / 48.151 | 27.360 / 50.501 | 0 / 0 |
| piano-64-cold | 2 | 27.100 / 47.041 | 26.531 / 46.711 | 0 / 0 |
| piano-64-cold | 3 | 27.241 / 48.211 | 28.600 / 57.291 | 0 / 0 |
| piano-64-warm | 1 | 25.711 / 45.061 | 27.611 / 48.601 | 0 / 0 |
| piano-64-warm | 2 | 26.220 / 48.461 | 26.561 / 46.520 | 0 / 0 |
| piano-64-warm | 3 | 27.920 / 56.961 | 27.581 / 51.911 | 0 / 0 |
| fx-64-cold | 1 | 52.401 / 105.782 | 53.961 / 112.782 | 0 / 0 |
| fx-64-cold | 2 | 52.641 / 114.502 | 52.131 / 114.072 | 0 / 0 |
| fx-64-cold | 3 | 51.501 / 105.242 | 52.831 / 116.382 | 0 / 0 |
| fx-64-warm | 1 | 55.701 / 119.883 | 54.081 / 124.203 | 0 / 0 |
| fx-64-warm | 2 | 51.081 / 114.982 | 51.401 / 104.722 | 0 / 0 |
| fx-64-warm | 3 | 54.171 / 115.652 | 52.041 / 124.383 | 0 / 0 |

**HOLD.** Median-of-run aggregates: piano cold27.100/48.151→27.360/50.501, warm26.220/48.461→27.581/48.601; FX cold52.401/105.782→52.831/114.072, warm54.171/115.652→52.041/124.203. Cold piano misses1ed23.380/43.701, FX cold p99 exceeds113.413; warm FX p99 also worsens. Zero underruns on both sides does not prove the earlier sparse-factor after0→2 regression resolved; separately tagged timing diagnostics follow. Evidence:~/.cache/kontakto-fix-cpu/cold-hold-pairs/. Remaining32/256/Vista and matched frozen-v1 gates remain open.

Own idle incrementals removed1,187,184,640 allocated bytes after the merged build;26 frozen ELFs outside target were verified first. No foreign targets or v1 artifacts changed.

The separately tagged cold FX recheck compares sparse-factor4cdc8920→verified hold765a7db7, AB/BA/AB three repeats. All6exit0, event/render heap0, coldpages_after0 and underruns0; diagnostic_stream_trace is empty on both sides. The earlier two misses were not reproduced, so no robustness acceptance follows. Diagnostic-only p50/p99µs: before53.701/120.072,53.361/124.792,51.621/113.502; after53.601/128.862,56.021/119.552,55.311/121.642. Evidence:~/.cache/kontakto-fix-cpu/cold-hold-stream-diags/.

## Settled mix coefficients candidate

Port from v1 0cb7a8a0:src/fx/processor.rs Slot::process: evaluate fixed wet/dry coefficients once for a block. Adapt to v2's ramped bypass and controls by using this path only after all three trajectories settle, preserving the current blend arithmetic including the zero dry term, fault result, trace records and inner-state bypass policy. Active ramps keep the existing per-frame path. No new processor state, locks or allocations. This is a performance candidate; no semantic failing-before claim.

New independent blend-equation fixture checks settled/ramping/retargeted/fully-bypassed states on bus and scalar/lane voice paths, block sizes1/7/64, with heap guards. It and49 area integration tests pass(one unrelated ignored; the signal-trace test's filtered child is included in the outer count), including97-voice exactPCM resident/streamed and parallel rendering. Default root cargo test --no-run passes. Frozen candidate admission-mix-settled SHA2564e7139880c3ba2ac822a55706e431801c1475a2eb027ded581ed38c03a1d321a. Before is frozen admission-cold-hold765a7db7. Original-cell A/B measurements are pending; no acceptance claim.

Own idle incremental cleanup removed1,032,077,312 allocated bytes after hash-verifying all27 frozen ELFs; no other artifacts changed.

Completed24 original-audit64 alternating pairs, before765a→settled candidate64919587. All runs exit0, event/render heap0, all coldpages_after0, all underruns0.

| Cell | Repeat | Before median / p99 µs | After median / p99 µs | Underruns before / after |
|---|---:|---:|---:|---:|
| piano-64-cold | 1 | 33.590 / 59.471 | 27.630 / 46.701 | 0 / 0 |
| piano-64-cold | 2 | 26.020 / 47.151 | 24.200 / 50.071 | 0 / 0 |
| piano-64-cold | 3 | 29.530 / 56.221 | 24.230 / 45.081 | 0 / 0 |
| piano-64-warm | 1 | 30.081 / 55.791 | 24.971 / 52.161 | 0 / 0 |
| piano-64-warm | 2 | 29.240 / 58.861 | 23.060 / 43.470 | 0 / 0 |
| piano-64-warm | 3 | 28.060 / 50.171 | 23.790 / 45.271 | 0 / 0 |
| fx-64-cold | 1 | 54.771 / 118.432 | 49.661 / 114.272 | 0 / 0 |
| fx-64-cold | 2 | 54.171 / 119.652 | 51.711 / 125.972 | 0 / 0 |
| fx-64-cold | 3 | 54.841 / 124.822 | 52.981 / 119.502 | 0 / 0 |
| fx-64-warm | 1 | 52.381 / 126.512 | 52.831 / 119.543 | 0 / 0 |
| fx-64-warm | 2 | 57.402 / 129.962 | 49.911 / 114.522 | 0 / 0 |
| fx-64-warm | 3 | 51.631 / 111.462 | 54.801 / 121.302 | 0 / 0 |

**HOLD.** Median-of-run aggregates: piano cold29.530/56.221→24.230/46.701, warm29.240/55.791→23.790/45.271; FX cold54.771/119.652→51.711/119.502, warm52.381/126.512→52.831/119.543 (median worse). Piano improves but remains above1ed23.380/43.701; FX cold p99 remains above113.413. Zero underruns on both sides does not establish previous rare misses resolved. Evidence:~/.cache/kontakto-fix-cpu/mix-settled-pairs/. Updated audio-TID profiles follow before the next root optimization.

Updated explanatory audio-TID profiles of settled candidate: piano25.551/49.191µs, underruns0; DSP6.00%, bus12.01%, streamed filtered source10.43% and resample window8.89%, stream service4.24%. FX52.551/126.482µs, underruns0; DSP4.89%, bus8.58%, fused source7.70%, convolution6.36%, stream service4.14%. No lost samples. These are not matched before/after profile acceptance or operator-specific attribution. Evidence:~/.cache/kontakto-fix-cpu/mix-settled-profile/.

## Finding10: restored blob ownership

The actual existing wrapper handoff alias, with8192 params,256KiB extra and1MiB persist, fails a semantic heap guard: three frees on the audio consumer, expected zero. Receipt state-retirement-red.log (exit101). CLAP and VST3 now use the same latest-wins pending queue plus two bounded retirement slots. Audio applies by reference and retires ownership even on authored panic; host/editor writers, inactive drains and main callbacks collect it. CLAP requests its main callback, VST3 reuses the existing main restart drain/wake without changing the C++ ABI. Full retirement capacity defers pending application rather than destroying audio-owned data. The realtime section begins before restore application.

Five targeted fixtures pass in each wrapper: large-state heap guard, a recall published during application, panic retirement, concurrent recalls, newest-wins. All70 wrapper-area tests pass, and default root cargo test --no-run passes. No native DAW large-rack recall or C++ allocator claim. Hosts without a main-thread pump can retain up to two consumed blobs until the next recall/deactivate; arbitrary authored load_state allocation is now visible to the guard, not automatically made realtime-safe. Frozen sound-seam candidate admission-state-retirement SHA2562f9bf6c7ab5be4beacc529dc19569eb697d6ba37bb59ad5bb4aa3c0e836c1c15; paired original-cell measurements follow. The original sound seam does not perform wrapper state recalls, so its timings cannot certify this allocation fix.

Completed one same-window A/B pair for all nine cells, warm and cold: frozen64919587→3a288897. All36exit0, event/render heap0, underruns0, all18coldpages_after0. These sound-seam numbers do not exercise restored-wrapper ownership, and a single pair does not establish noise bounds.

| Cell | Temperature | Before median / p99 µs | After median / p99 µs | Underruns before / after | Deadline misses before / after |
|---|---|---:|---:|---:|---:|
| piano-32 | cold | 14.910 / 29.901 | 15.350 / 30.231 | 0 / 0 | 0 / 0 |
| piano-32 | warm | 17.280 / 39.900 | 15.890 / 33.101 | 0 / 0 | 0 / 0 |
| strings-32 | cold | unscorable (no steady voices) | unscorable (no steady voices) | 0 / 0 | 23 / 23 |
| strings-32 | warm | unscorable (no steady voices) | unscorable (no steady voices) | 0 / 0 | 23 / 23 |
| fx-32 | cold | 44.511 / 93.862 | 44.331 / 103.102 | 0 / 0 | 0 / 0 |
| fx-32 | warm | 47.321 / 111.272 | 44.240 / 91.802 | 0 / 0 | 0 / 0 |
| piano-64 | cold | 21.710 / 40.211 | 21.601 / 42.980 | 0 / 0 | 0 / 0 |
| piano-64 | warm | 21.441 / 39.171 | 22.841 / 48.551 | 0 / 0 | 0 / 0 |
| strings-64 | cold | unscorable (no steady voices) | unscorable (no steady voices) | 0 / 0 | 15 / 15 |
| strings-64 | warm | unscorable (no steady voices) | unscorable (no steady voices) | 0 / 0 | 15 / 15 |
| fx-64 | cold | 48.961 / 113.052 | 46.801 / 111.242 | 0 / 0 | 0 / 0 |
| fx-64 | warm | 45.221 / 100.892 | 47.951 / 112.102 | 0 / 0 | 0 / 0 |
| piano-256 | cold | 83.331 / 109.032 | 87.152 / 120.102 | 0 / 0 | 0 / 0 |
| piano-256 | warm | 88.882 / 130.592 | 86.001 / 115.252 | 0 / 0 | 0 / 0 |
| strings-256 | cold | 14207.156 / 22156.184 | 12788.319 / 19213.640 | 0 / 0 | 591 / 591 |
| strings-256 | warm | 14586.553 / 21974.352 | 15479.780 / 16473.389 | 0 / 0 | 591 / 591 |
| fx-256 | cold | 177.093 / 252.715 | 172.513 / 249.455 | 0 / 0 | 0 / 0 |
| fx-256 | warm | 166.363 / 273.255 | 177.574 / 268.395 | 0 / 0 | 0 / 0 |

**HOLD.** The favorable single cold64piano21.601/42.980 andFX46.801/111.242 meet historical1ed CPU thresholds in this window, but prior3-rep settled candidate aggregate24.230/46.701 andFX51.711/119.502 miss them; no acceptance from a favorable repeat. Warm64piano remains above historicalv1~6.920/21.111. Vista32/64 has no steady voices and15–23deadline misses despite zero reported underruns. Vista256 has591deadline misses on both sides and12.8–15.5milliseconds per block, not microseconds. W6 received the exact path/checkpoint/originaleventplan for sharedsignalgraph diagnosis. No parity claim on any full9-cell matrix. Evidence:~/.cache/kontakto-fix-cpu/state-retirement-matrix/.

## Finding9: idle decoded page memory

Ported v1 `0cb7a8a0:src/engine/stream.rs` `Slot::reclaim` and its five-second idle policy. `sampler-pool::discard_f32` owns the narrow unsafe boundary: only complete OS pages inside an exclusively borrowed f32 payload are discarded, excluding allocator metadata and neighboring storage; unaligned edges remain allocated. Linux releases physical RSS without changing bounded Box ownership; unsupported/refused discard returns zero. Core remains unsafe-free. New buffers are discarded after construction; worker free buffers are discarded once after five wall-clock seconds without decoded jobs. Every requested range is initialized again before publication.

After five runtime seconds without voices, audio returns at most16 idle entries per streaming service through existing invalidation/recycle queues. Protected entries and active voices/tails prevent reclamation; stale results retain serial identity and return storage to the worker. No audio madvise, reader close, allocation, or buffer destruction. Decode threads park for five seconds between idle checks and close only their reader LRU; source catalog, archive handles, header caches and reload path remain intact.

Five new targeted checks pass: owned-page/neighbor/edge bounds; protected pages and stale results; live-voice retention; threshold/bounded return plus PCM reactivation under the heap guard; actual five-second Kontakt reader close/reopen from the retained source. All54 targeted streaming/cold/lane integration tests pass (one independent timing test ignored), including the97voice resident/streamed/parallel PCM fixture. Default root `cargo test --no-run` passes. No native-host suspend/resume or non-Linux physical memory claim.

Synthetic16part×768page pools (384MiB allocated decoded storage); same probe, frozen before/after:

| Measurement | Before | After |
|---|---:|---:|
| Initial RSS KiB | 2484 | 2552 |
| Loaded RSS KiB | 399356 | 55104 |
| Warm populated RSS KiB | 400864 | 400868 |
| Idle RSS after6seconds KiB | 400992 | 56740 |
| Reactivated RSS KiB | 400992 | 57188 |
| Setup milliseconds | 113.965 | 107.500 |
| Audio heap calls | 0 | 0 |
| Reactivation PCM equal | yes | yes |

Idle RSS decreases85.85%; storage capacity stays384MiB and physical edge pages remain resident. Single synthetic A/B; setup time is not a real-library load gate. Frozen `idle-pool-before` SHA256`4ad2b87240e73bf7347996fe9cf33a56b3ac2a6d1723c2d75cedb9e300a34588`, after`93777e30604e86b29fcc03786f2e729b00725286645502d2e9d5dfc81acd0935`. CPU candidate `admission-idle-pool` SHA256`8e69ea921f4dcddc14b0af51b43d502da966101c9a22e9cead6e497104330f44`; original warm/cold nine-cell same-window A/B matrix and repeated64 cells running. **HOLD** until CPU/underrun checks complete; overall v1 gate remains unmet.

Completed52 original-cell runs: one same-window A/B pair for all nine warm/cold cells plus two more alternating64pairs for piano/FX. All exits0; all event/render heap calls0 and underruns0. Every26 cold eviction reports pages_after0.

| Cell | Temperature | Before median / p99 µs | After median / p99 µs | Underruns before / after | Deadline misses before / after |
|---|---|---:|---:|---:|---:|
| piano-32 | cold | 16.410 / 38.341 | 15.500 / 34.811 | 0 / 0 | 0 / 0 |
| piano-32 | warm | 15.650 / 37.821 | 16.280 / 37.071 | 0 / 0 | 0 / 0 |
| strings-32 | cold | unscorable (no steady voices) | unscorable (no steady voices) | 0 / 0 | 23 / 23 |
| strings-32 | warm | unscorable (no steady voices) | unscorable (no steady voices) | 0 / 0 | 23 / 23 |
| fx-32 | cold | 45.801 / 113.812 | 72.702 / 320.426 | 0 / 0 | 0 / 12 |
| fx-32 | warm | 47.951 / 113.762 | 50.931 / 113.812 | 0 / 0 | 0 / 0 |
| piano-64 | cold | 25.260 / 47.531 | 27.320 / 52.531 | 0 / 0 | 0 / 0 |
| piano-64 | warm | 25.780 / 45.861 | 26.140 / 55.001 | 0 / 0 | 0 / 0 |
| strings-64 | cold | unscorable (no steady voices) | unscorable (no steady voices) | 0 / 0 | 15 / 15 |
| strings-64 | warm | unscorable (no steady voices) | unscorable (no steady voices) | 0 / 0 | 15 / 15 |
| fx-64 | cold | 52.621 / 120.142 | 53.121 / 125.292 | 0 / 0 | 0 / 0 |
| fx-64 | warm | 50.991 / 105.402 | 52.621 / 125.462 | 0 / 0 | 0 / 0 |
| piano-256 | cold | 86.731 / 127.793 | 87.731 / 112.762 | 0 / 0 | 0 / 0 |
| piano-256 | warm | 87.621 / 149.783 | 87.952 / 136.442 | 0 / 0 | 0 / 0 |
| strings-256 | cold | 15636.884 / 21268.149 | 12885.151 / 13726.968 | 0 / 0 | 591 / 591 |
| strings-256 | warm | 13465.873 / 14551.753 | 14004.723 / 14496.402 | 0 / 0 | 591 / 591 |
| fx-256 | cold | 178.803 / 343.456 | 184.313 / 258.575 | 0 / 0 | 0 / 0 |
| fx-256 | warm | 185.393 / 277.685 | 180.443 / 272.315 | 0 / 0 | 0 / 0 |

Three-pair64 aggregate (median of the three run medians / median of the three run p99s):
| Cell | Before µs | After µs |
|---|---:|---:|
| piano-cold | 25.260 / 47.071 | 24.901 / 51.081 |
| piano-warm | 25.780 / 48.011 | 25.630 / 47.131 |
| fx-cold | 52.621 / 119.582 | 52.271 / 123.752 |
| fx-warm | 50.991 / 110.062 | 52.401 / 121.762 |

**HOLD.** Repeated piano cold p99 rises47.071→51.081; FX cold119.582→123.752 and warm110.062→121.762. No claim that these increases are noise, and no v1/1ed parity acceptance. Vista32/64 still has no steady voices; Vista256 remains a severe deadline failure. RSS correctness does not certify CPU parity or historical rare underruns resolved. Evidence:~/.cache/kontakto-fix-cpu/idle-pool-matrix/.

Follow-up diagnostics of frozen3a→8c cold64 (perf9999Hz; timings perturbed and not acceptance): piano30.130/66.311→23.820/43.011µs; FX54.321/117.072→55.421/133.393µs, all heap/underruns/deadlines0. Audio-TID samples have zero lost samples. Stream service2.95%→3.88% piano and5.54%→4.95% FX; bus11.05%→13.51% and8.76%→8.66%; source filtered8.84%→9.58% piano, resample window10.20%→8.94%. No evidence of madvise/sysconf/decoder-reader cleanup on the audio path. These sparse leaf percentages do not establish a cause for the p99 increases. Evidence:~/.cache/kontakto-fix-cpu/idle-pool-profile/.

Cold32FX repeated alternating pairs:
| Pair | Before median / p99 µs | After median / p99 µs | Deadline misses before / after |
|---|---:|---:|---:|
| 1 | 45.801 / 113.812 | 72.702 / 320.426 | 0 / 12 |
| 2 | 45.401 / 98.782 | 45.731 / 99.412 | 0 / 0 |
| 3 | 46.111 / 102.622 | 52.051 / 111.193 | 0 / 0 |

Cold32FX aggregate 45.801 / 102.622→52.051 / 111.193µs; the12 deadline misses in the first after run did not recur in repeats2/3, but were not proven noise or fixed. **HOLD**, cold64 and warmFX p99 remain above the acceptance baseline.

## Streamed high-rate prepared resampling (continuation)

Extends the existing realtime polyphase bank through the admitted maximum16x rate. The original eight rows covering1–2x retain their exact coefficient bits. Above2x, sixteen stretches per octave preserve the existing passband limit; the provisional eight-stretch extension failed at step2.01 and was corrected rather than relaxing the test. Row preparation evaluates the existing Q32 reference impulse weights once, retains its f32 normalization rounding, and avoids the old quadratic impulse evaluation. Coefficients use4,388,800 shared bytes, allocated during control-side construction. High-quality conversion retains the long reference kernel. Narrow stack windows remain52frames; wider fallbacks are bounded at388frames.

Scope: private resample.rs bank construction/selection; existing Cursor::sample, sample_window and render_run callers consume it unchanged; new paged_render PCM fixture; synthetic streamed_resample example. No public API, config, admission, DSP or residency changes. This accelerates the streamed high-rate source fallback; it does not yet provide resident octave levels to PagedFrames, so audit finding6 is only partly addressed.

Verification:43 targeted checks pass (3 kernel units,3 cold-chain,19 paged-render,18 resample). The new fixture compares streamed and resident PCM over forward/reverse page and loop seams at2.01/2.5/4/8/16x, partitions1/7/64/128, with zero heap calls and underruns. Unit coverage checks each extended-bank boundary against the unchanged passband/alias thresholds and preserves all narrow-row bits. Default root cargo test --no-run passes. Evidence:~/.cache/kontakto-fix-cpu/streamed-resample-check.log.

Before synthetic binary SHA256de85991754b29123bba7b158530574a17758d64c396d6d6b1f8004958fd852ea is preserved. Optimized after build and original piano/FX32/64/256 warm/cold A/B are pending. Vista remains W7's lifecycle dependency and is not scored as a CPU win. **HOLD; no v1 parity claim.**

### High-rate candidate measurements

Frozen original-cell candidate SHA256 `fd0f76a08aad65b12a9f8ddfc0c23f24c2ca97fb41c5845f3d3f3b74fd8e49d7`, source-probe SHA256 `e7dea035291f57b473f5aab5018d5158c58a54ee0dd0ebb8e1a439d90385c40e`. Before is `admission-idle-pool` /8c74dc71 (CPU SHA256 `8e69ea921f4dcddc14b0af51b43d502da966101c9a22e9cead6e497104330f44`). After source is4c3c8f07. The original cold helper verifies pages_after=0 before each cold launch; all40 library runs complete successfully and retain zero event/render heap calls.

First matched original matrix, steady median / p99 in µs. Counters are before / after.

| Cell | Before µs | After µs | Underruns | Deadline misses |
|---|---:|---:|---:|---:|
| piano-32-cold | 20.281 / 43.061 | 30.490 / 94.692 | 0 / 0 | 0 / 1 |
| piano-32-warm | 28.991 / 55.181 | 30.431 / 126.682 | 0 / 0 | 2 / 1 |
| piano-64-cold | 31.510 / 63.571 | 25.811 / 52.641 | 0 / 0 | 0 / 0 |
| piano-64-warm | 27.260 / 48.711 | 26.260 / 51.951 | 0 / 0 | 0 / 0 |
| piano-256-cold | 133.142 / 232.884 | 144.933 / 807.695 | 0 / 0 | 1 / 0 |
| piano-256-warm | 147.753 / 482.449 | 161.213 / 295.336 | 0 / 0 | 0 / 0 |
| fx-32-cold | 70.101 / 386.048 | 70.561 / 334.217 | 0 / 0 | 7 / 19 |
| fx-32-warm | 57.451 / 133.683 | 70.141 / 260.185 | 0 / 0 | 7 / 5 |
| fx-64-cold | 52.321 / 119.732 | 54.751 / 111.662 | 0 / 0 | 0 / 0 |
| fx-64-warm | 52.101 / 131.932 | 59.971 / 131.863 | 0 / 0 | 0 / 0 |
| fx-256-cold | 285.076 / 490.929 | 270.855 / 458.969 | 0 / 2 | 0 / 0 |
| fx-256-warm | 236.605 / 444.718 | 264.355 / 594.481 | 0 / 0 | 0 / 0 |

Alternating64-frame repeats; repeat2 reverses binary order. All failures remain counted.

| Cell | Repeat | Before µs | After µs | Underruns | Deadline misses |
|---|---:|---:|---:|---:|---:|
| piano-64-cold | 1 | 31.510 / 63.571 | 25.811 / 52.641 | 0 / 0 | 0 / 0 |
| piano-64-cold | 2 | 25.131 / 50.241 | 39.481 / 68.862 | 0 / 0 | 0 / 2 |
| piano-64-cold | 3 | 25.610 / 45.531 | 26.361 / 55.791 | 0 / 0 | 0 / 0 |
| piano-64-warm | 1 | 27.260 / 48.711 | 26.260 / 51.951 | 0 / 0 | 0 / 0 |
| piano-64-warm | 2 | 37.590 / 75.592 | 26.651 / 52.111 | 0 / 0 | 0 / 0 |
| piano-64-warm | 3 | 26.041 / 48.561 | 26.700 / 50.371 | 0 / 0 | 0 / 0 |
| fx-64-cold | 1 | 52.321 / 119.732 | 54.751 / 111.662 | 0 / 0 | 0 / 0 |
| fx-64-cold | 2 | 51.911 / 114.663 | 73.061 / 399.727 | 0 / 0 | 0 / 0 |
| fx-64-cold | 3 | 51.901 / 122.153 | 54.571 / 114.552 | 0 / 0 | 0 / 0 |
| fx-64-warm | 1 | 52.101 / 131.932 | 59.971 / 131.863 | 0 / 0 | 0 / 0 |
| fx-64-warm | 2 | 52.741 / 117.632 | 52.711 / 111.582 | 0 / 0 | 0 / 0 |
| fx-64-warm | 3 | 52.251 / 110.002 | 54.391 / 126.972 | 0 / 0 | 0 / 0 |

Repeated aggregates (median of the three run medians / p99):

| Cell | Before µs | After µs |
|---|---:|---:|
| piano-64-cold | 25.610 / 50.241 | 26.361 / 55.791 |
| piano-64-warm | 27.260 / 48.711 | 26.651 / 51.951 |
| fx-64-cold | 51.911 / 119.732 | 54.751 / 114.552 |
| fx-64-warm | 52.251 / 117.632 | 54.391 / 126.972 |

**FAIL/HOLD.** Piano64 cold aggregate regresses25.610/50.241→26.361/55.791µs; warm27.260/48.711→26.651/51.951(p99 worse). FX64 cold51.911/119.732→54.751/114.552(median worse); warm52.251/117.632→54.391/126.972(both worse). All40 library runs: before17 / after28 deadline misses; before0 / after2 storage underruns (the after coldFX256 cell). No scheduling-noise explanation or v1 parity claim. The favorable source medians cannot certify the release gate. Piano32 adverse cells and coldFX256 are being repeated, then matched cold piano32/64 audio-TID profiles will assess the tail.

Synthetic preloaded source aggregates from3 alternating pairs; these are not cold-storage cells or native-host PCM comparisons. All120 records retain zero heap calls/underruns, stable voice counts and DC PCM.

| Voices | Storage | Rate | Before median/p99 µs | After median/p99 µs |
|---:|---|---:|---:|---:|
| 8 | resident | 1.5 | 6.670 / 8.260 | 6.590 / 7.871 |
| 8 | resident | 2.5 | 51.351 / 80.581 | 8.710 / 43.360 |
| 8 | resident | 4 | 77.551 / 122.343 | 13.530 / 62.092 |
| 8 | resident | 8 | 139.592 / 231.895 | 20.770 / 112.322 |
| 8 | resident | 16 | 266.465 / 444.108 | 40.601 / 266.915 |
| 8 | paged | 1.5 | 6.800 / 10.380 | 7.030 / 11.990 |
| 8 | paged | 2.5 | 51.501 / 103.092 | 9.051 / 71.731 |
| 8 | paged | 4 | 77.932 / 159.113 | 13.760 / 100.652 |
| 8 | paged | 8 | 138.292 / 317.926 | 21.820 / 190.864 |
| 8 | paged | 16 | 281.525 / 606.192 | 80.881 / 428.938 |
| 64 | resident | 1.5 | 52.851 / 74.902 | 53.351 / 88.002 |
| 64 | resident | 2.5 | 419.128 / 888.217 | 69.581 / 340.477 |
| 64 | resident | 4 | 612.952 / 987.389 | 121.803 / 804.525 |
| 64 | resident | 8 | 1102.080 / 1810.065 | 183.933 / 1630.720 |
| 64 | resident | 16 | 2145.590 / 4870.142 | 371.637 / 2947.146 |
| 64 | paged | 1.5 | 54.361 / 76.461 | 54.481 / 61.891 |
| 64 | paged | 2.5 | 412.208 / 814.315 | 71.321 / 554.820 |
| 64 | paged | 4 | 614.471 / 1268.614 | 133.752 / 1186.432 |
| 64 | paged | 8 | 1122.472 / 2576.008 | 213.564 / 2847.683 |
| 64 | paged | 16 | 2336.404 / 8905.688 | 649.143 / 3247.921 |

64-voice paged medians improve at high rates, but8x p99 regresses2576.008→2847.683µs. Per-run records, including earlier single-probe outliers and setup times, remain in `~/.cache/kontakto-fix-cpu/streamed-resample-{micro,matrix}/` and streamed-resample-check.log. No rows are excluded.

### Tail follow-up: repeated adverse cells and matched audio-TID profiles

| Cell | Repeat | Before median/p99 µs | After median/p99 µs | Underruns before/after | Deadlines before/after |
|---|---:|---:|---:|---:|---:|
| v2-piano-32-cold | 2 | 16.891/34.111 | 18.770/37.080 | 0/0 | 0/0 |
| v2-piano-32-cold | 3 | 18.531/36.121 | 17.771/36.701 | 0/0 | 0/0 |
| v2-piano-32 | 2 | 16.271/33.060 | 17.820/38.081 | 0/0 | 0/0 |
| v2-piano-32 | 3 | 20.550/45.601 | 18.080/47.041 | 0/0 | 0/0 |
| v2-fx-256-cold | 2 | 180.623/261.055 | 176.073/287.836 | 0/0 | 0/0 |
| v2-fx-256-cold | 3 | 175.253/266.025 | 179.734/250.645 | 0/0 | 0/0 |

Repeated32-frame piano aggregate cold18.531/36.121→18.770/37.080µs; warm20.550/45.601→18.080/47.041. ColdFX256 aggregate180.623/266.025→179.734/287.836; its initial two after underruns remain counted even though repeats2/3 have none. The p99 gate remains failed.

Matched piano cold profiles use9999Hz cycles:u with the exact audio TID and zero lost samples.32-frame approximate cycles382,861,145→415,606,759: sample_window5.70%→8.50%, new out-of-line polyphase1.72%.64-frame cycles314,831,920→310,651,961: sample_window8.98%→9.58%, polyphase2.19%. Profile median/p99 µs32:18.360/40.331→20.271/39.961;64:28.381/55.371→29.950/51.961. All profile heap/underrun/deadline counters zero. These explanatory profiles do not replace the unprofiled acceptance runs.

Instruction annotation establishes a concrete added cost: the56-entry iter.find is fully unrolled into an approximately1.6KiB out-of-line function. Most sampled instructions are early quality/rate guards; the common narrow-rate loop now calls that function. This does not prove the95µs initial outlier is caused by lookup or exclude cache/page-fault effects. Evidence:~/.cache/kontakto-fix-cpu/high-rate-polyphase-annotate.txt and streamed-resample-profile/.

ColdFX256 metadata tracing, three alternating pairs of frozen8c→4c binaries, reports no underruns or new stream error/capacity counters on either side; all diagnostic_stream_trace arrays are empty. Original underruns therefore remain unresolved. No reduced prefetch horizon or demand radius is introduced: demand uses the unchanged long48×step radius, which covers every new short-bank window including chunk padding. Runtime::new prepares the bank before timed note input; all measured first-note event/render heap counters are zero. Neither lazy first-note initialization nor audio allocation is established in this probe. Evidence:~/.cache/kontakto-fix-cpu/streamed-resample-underrun-trace/.

Next candidate preserves the original inline eight-entry lookup through2x and isolates higher-rate selection in a non-inlined bounded binary search. Its new unit compares every bank boundary with the preceding linear selection (including invalid ratios) and asserts each padded read window fits the existing streaming demand radius.44 area checks and default root no-run pass. Optimized candidate and original40-cell gate are pending; **HOLD, no W0 release handoff.**

### Lookup correction: original-instrument three-way rejection

20 original cells per binary,60 runs total, with rotating baseline/before/after order. Baseline is frozen8c; before is4c; after is50b2b8db. After CPU SHA256 `df7ae1aa9ad77b644c81360f2fd1d87dc6caacc36506efef5a60d7b5b2211f06`. All runs return0 and all event/render heap counters are0;30 cold evictions verify pages_after=0.

First complete matrix (steady median/p99 µs); each counter triplet is baseline /4c /50.

| Cell | 8c baseline µs | 4c µs | 50 µs | Underruns | Deadlines |
|---|---:|---:|---:|---:|---:|
| piano-32-cold | 17.030/35.141 | 16.940/38.961 | 17.700/37.021 | 0 / 0 / 0 | 0 / 0 / 0 |
| piano-32-warm | 16.440/36.111 | 17.530/39.411 | 16.620/35.800 | 0 / 0 / 0 | 0 / 0 / 0 |
| piano-64-cold | 25.450/48.070 | 26.601/49.371 | 27.910/51.391 | 0 / 0 / 0 | 0 / 0 / 0 |
| piano-64-warm | 26.050/45.110 | 27.221/52.041 | 25.931/45.861 | 0 / 0 / 0 | 0 / 0 / 0 |
| piano-256-cold | 92.352/121.772 | 92.292/127.183 | 92.322/140.833 | 0 / 0 / 0 | 0 / 0 / 0 |
| piano-256-warm | 89.072/149.973 | 92.012/144.882 | 91.252/114.213 | 0 / 0 / 0 | 0 / 0 / 0 |
| fx-32-cold | 48.260/103.642 | 48.700/110.972 | 48.551/109.102 | 0 / 0 / 4 | 0 / 0 / 0 |
| fx-32-warm | 50.271/109.232 | 48.131/111.962 | 46.691/111.272 | 0 / 0 / 0 | 0 / 0 / 0 |
| fx-64-cold | 52.741/113.762 | 50.341/98.672 | 54.041/101.022 | 0 / 0 / 0 | 0 / 0 / 0 |
| fx-64-warm | 53.651/112.032 | 52.211/116.023 | 53.291/113.872 | 0 / 0 / 0 | 0 / 0 / 0 |
| fx-256-cold | 177.544/265.725 | 177.013/263.765 | 183.683/256.295 | 0 / 0 / 0 | 0 / 0 / 0 |
| fx-256-warm | 178.183/266.235 | 179.543/272.295 | 184.343/300.875 | 0 / 0 / 0 | 0 / 0 / 0 |

Three-run64-frame aggregates; full per-run JSON retained in resample-lookup-matrix/.

| Cell | 8c baseline µs | 4c µs | 50 µs |
|---|---:|---:|---:|
| piano-64-cold | 25.450/46.201 | 26.601/53.451 | 27.381/50.911 |
| piano-64-warm | 25.330/50.441 | 27.060/52.041 | 26.601/50.131 |
| fx-64-cold | 52.741/113.762 | 50.341/116.792 | 53.531/106.612 |
| fx-64-warm | 52.801/112.032 | 51.521/111.692 | 53.151/117.203 |

**REJECT/HOLD.** Baseline and4c each have0 deadlines/underruns;50 has0 deadlines and4 underruns in first coldFX32.50 piano64 cold aggregate27.381/50.911 remains above baseline25.450/46.201, warm26.601/50.131 versus25.330/50.441 (median worse). FX64 cold53.531/106.612 versus52.741/113.762 (median worse); warm53.151/117.203 versus52.801/112.032. Restoring the narrow inline lookup removes an added instruction cost but does not solve the full instrument gate. Earlier4c failures remain recorded; this batch does not cancel them. No W0 release handoff.

Source-only median preservation is separately measured in resample-lookup-micro/: three alternating4c→50 pairs,120 records. The source benchmark does not override the failed original-instrument gate.

Splitting next: retain only the control-side quadratic-to-linear coefficient preparation as a trial, restoring the exact pre4c eight-entry bank,52-frame windows and render lookup. The extended bank, wider fallbacks and lookup change are removed from that trial. All experiment code remains reachable at4c3c8f07 and50b2b8db, and its receipts are preserved. The preparation-only part receives its own40 original-instrument runs before anything is retained.

### Preparation-only split checkpoint

The working branch restores the exact945098b2 render implementation, original eight bank entries and52-frame stack window. Its only production diff from945098b2 is linear control-side coefficient construction; the reference impulse-row test proves all eight banks'65 phases bit-exact. The extended bank, wider stack fallbacks and lookup correction remain rejected experiments reachable in the history, with all measurements retained.

43 area checks and default root cargo test --no-run pass. Candidate CPU SHA256 `0adbdef8d357dafd379185509960eab21baaabf8230b237cf847e0633fbcb505`. The next rotating60-run original-instrument batch compares8c baseline,50 extended-bank experiment and preparation-only. Its two overlapping40-run comparisons isolate coefficient preparation (baseline→preparation-only) and the extended bank/window/lookup (preparation-only→50). No part is accepted from a source-only synthetic win. **HOLD pending that gate.**
