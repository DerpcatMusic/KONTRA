# KSP shared code and constant pools

Baseline: `3f49a9a109220c95546596e37204b5b2faa829b7`.
Candidate: `799d6a3b3eb40548ba7210454711ccaabd1dbccf`, following the numeric-only diagnostic commit `6d29a40b0daae18a16ef5a645f502e9c35b29374`.

## Measured cause and change

Functions already used Call/Return instructions. The frontend nevertheless compiled their bodies again for each callback, including each UI control handler. Conflux's main slot emitted 1,287,726 function words against 6,701 root words; Analog emitted 9,846,626 against 18,535. This was callback duplication, rather than per-call inlining or array unrolling.

The candidate adapts the immutable shared program ownership and once-emitted function table from v1 `0cb7a8a0:src/ksp/compile.rs`. Callbacks in the same execution context share one `Arc` code table and one interned, unpadded string pool. Each entry retains its own admission requirements, UI control identity, wait lifetime and source slot. Dynamic mutable VM texts keep their existing fixed capacity. Native instructions remain 32 bytes; this change does not introduce v1's 16-byte fused instruction encoding.

## Code and constants

The instruction totals below include listener drivers and engine-start programs. They are numeric frontend counts, rather than estimates from allocator boxes above a size threshold.

| Instrument | Before instructions | After instructions | Before dense bytes | After dense bytes | After pooled text payload |
| --- | ---: | ---: | ---: | ---: | ---: |
| Conflux | 1,301,968 | 103,636 | 41,662,976 | 3,316,352 | 4,910 B / 596 constants |
| Analog | 9,877,751 | 157,749 | 316,088,032 | 5,047,968 | 21,884 B / 2,261 constants |

W8's separate allocation receipt attributes Analog's script UI phase to 316,040,064 bytes of large instruction boxes and 111,137,334 bytes of padded constants. Small boxes are omitted from that allocator table. The core-binding phase creates no new KSP code or text: its 133,897,320 retained bytes are prepared regions, control ranges/indices and voice programs. W8 owns that separate work. Production constructs envelope controls before attaching behaviors, so repeated envelope construction does not rescan KSP code in that load path.

## Production RSS and playback

The same house root load/editor probe ran against frozen baseline and candidate binaries, with filesystem cache disabled. Measurements were contended and establish RSS and playback only; their load times are not acceptance timings.

| Instrument | Before editor MiB | After editor MiB | Reduction MiB | Before/after PCM peak | First audio frames |
| --- | ---: | ---: | ---: | ---: | ---: |
| Conflux | 337.3047 | 313.2344 | 24.0703 | 0.1260089576 | 64 |
| Analog | 1083.4570 | 677.8945 | 405.5625 | 0.7999154925 | 192 |

Conflux's pre-load RSS differed (34.8477 versus 52.4961 MiB); the table conservatively reports total editor RSS. Analog's pre-load RSS was 35.3750 versus 35.2656 MiB.

Dolce's production envelope-init PCM test passed all seven authored cases with per-block offline failures, stream errors and underruns equal to zero. The test uses the approved deterministic offline host mode. Real-time original-instrument streaming remains covered by W9 and the release gate.

## Validation and receipts

Core: 397 passed, zero failed, one existing ignored test. KSP: 185 passed, zero failed, 40 existing ignored tests. Added checks cover shared function admission, nested calls, per-control UI identity, shared unpadded text without audio allocation, and per-control drag/drop storage. Core/KSP and root no-run builds passed.

Numeric logs and immutable binaries: `/mnt/Windows11/DEV_WORKSPACE/kontra-runs/w5-ksp-rss/`. Build/test logs: `~/.cache/kontakto-w5/ksp-rss/`. No authored source or samples are included in this receipt.

## Frozen v1 numeric comparison

The adapter `fbd8748bb1ddd6abb9f8350fbb4d1e2c727a7473` adds a numeric example to exact v1 `0cb7a8a0b4d43086596a64c77320caa1b26d6d98`. It invokes the production compiler/cache and inherits conditions between slots. Programs are retained as `Arc`s; a repeated same-key compile verifies pointer identity. The frozen binary SHA256 is `d4294bf063cdf5a4a600138542b9f54a05eb0241c7137ada47f46f915add538f`. Numeric slot receipts contain no authored source.

| Instrument | Source bytes | Slot op counts | Total ops | sizeof(v1 Op) | Dense code bytes | Compile errors |
| --- | ---: | --- | ---: | ---: | ---: | ---: |
| Conflux | 374,224 | 51,870 / 5,654 / 91 | 57,615 | 16 B | 921,840 | 0 |
| Analog | 1,444,877 | 86,988 / 6,405 / 6,405 | 99,798 | 16 B | 1,596,768 | 0 |

The candidate removes the duplication but retains more native words and the larger instruction encoding. The remaining difference from v1 is measured, not claimed as parity.

## CPU acceptance: OPEN

The existing `cpu_audit` example was built with identical `cargo build --profile ci --example cpu_audit` commands. Baseline SHA256: `6f5c04aa2a3ca468cdd3f74f4cff99a6890d53149fbec9b5d0cee524e12553a4` (the exact 3f49 gate binary confirmed by W0). Candidate SHA256: `3ea90f262b583841d9ab087ed76424dbbaa61cc45954826ab9a05b5e9db4674f`.

One granted W5 quiet window produced 27 accepted A0/A1/B0 cells across Conflux, Analog and Dolce at 32/64/256 frames. One scanner-contended cell was rejected and repeated. Four additional accepted runs (A2/B1 for Dolce32 and Dolce256) resolved uncertain results. Every accepted run has house observer status QUIET and zero render/event heap operations; no separate KSP timer was introduced.

Comparison: mean(B) minus mean(A) must not exceed the observed baseline range(A). Repeats are included in both means; all original data remain in `cpu-summary.json`.

| Instrument | Block | A mean p50 µs | B mean p50 µs | A mean p99 µs | B mean p99 µs | Verdict |
| --- | ---: | ---: | ---: | ---: | ---: | --- |
| Conflux | 32 | 1.505 | 1.440 | 64.627 | 59.991 | PASS |
| Conflux | 64 | 2.1855 | 1.680 | 104.632 | 99.582 | PASS |
| Conflux | 256 | 6.980 | 6.671 | 378.782 | 377.687 | PASS |
| Analog | 32 | 32.9555 | 32.300 | 95.622 | 86.132 | PASS |
| Analog | 64 | 47.811 | 46.421 | 118.748 | 115.212 | PASS |
| Analog | 256 | 177.078 | 177.934 | 340.997 | 329.476 | PASS |
| Dolce | 32 | 32.2373 | 37.0455 | 232.271 | 242.4245 | FAIL p50 |
| Dolce | 64 | 49.571 | 51.311 | 302.941 | 287.895 | PASS |
| Dolce | 256 | 186.167 | 185.7285 | 1275.9373 | 1330.5145 | PASS within noise |

Dolce32's p50 increase is 4.8082 µs against 0.289 µs baseline variation. CPU acceptance is not waived. Stream errors, underruns, faults and nonfinite outputs are zero, and original silent-note counters are unchanged for each A/B cell. Round-robin PCM peaks vary and are not treated as exact-sample identity.

NEXT: diagnose Dolce32 total per-block cost before a READY handoff.
