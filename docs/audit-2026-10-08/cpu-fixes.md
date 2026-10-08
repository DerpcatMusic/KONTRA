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
