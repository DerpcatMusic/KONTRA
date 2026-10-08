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
