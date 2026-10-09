# Held EQ/mix checkpoint: measured Horns improvement, whole CPU HOLD

Runtime source `ec75697e36556d48a6b3c9ac6b65202d97cf071e`, exact clean base `54d9a5c5`. Production port `27d7ccb0`; checked raw AHDSR preparation/support `6496f199` → `3bb384bc` → `de97ecce` → `2ecad6b0`. No fused native whole-voice caller is enabled. W6 support is the same patch as `4507cb1e`, not a second implementation.

Original sound-seam audit schedule, 48 kHz, twelve notes 48–59 at velocity 100, CC1=110/11=127/64=127, note-off at 48,000, pedal-off at 144,000, total 192,000 frames. One cold and warm execution per side/cell, reversed warm side order. Active quantiles below cover frames 12,000–48,000 (562 blocks at 64; 141 at 256). Whole-run quantiles, startup deadline misses, cache eviction receipts and activity evidence remain in the numeric receipt. Cold describes sample-file cache eviction, not a native host measurement.

| Cell | Temperature | v1 p50 / p99 µs | Base p50 / p99 µs | Candidate p50 / p99 µs | p50 / p99 change |
|---|---|---:|---:|---:|---:|
| Horns64 | cold | 51.441 / 81.812 | 747.055 / 1217.543 | 515.680 / 646.793 | -30.97% / -46.88% |
| Horns64 | warm | 52.431 / 83.572 | 739.624 / 1109.681 | 488.620 / 656.643 | -33.94% / -40.83% |
| Horns256 | cold | 163.143 / 282.125 | 2883.885 / 3273.592 | 1941.697 / 2318.244 | -32.67% / -29.18% |
| Horns256 | warm | 159.393 / 287.855 | 2887.345 / 3332.883 | 2018.708 / 2377.375 | -30.08% / -28.67% |
| Morph256 | cold | 40.521 / 47.651 | 674.972 / 818.365 | UNKNOWN | UNKNOWN |
| Morph256 | warm | UNKNOWN | UNKNOWN | UNKNOWN | UNKNOWN |

Fourteen rows are admitted QUIET, with zero event/render heap calls and zero underruns. Candidate startup/whole-run deadline misses are Horns64 cold 3/warm 4, Horns256 0/0; the active maxima remain below their block deadlines. Active candidate p50 remains 9.3–12.7 times frozen v1. This is an improvement checkpoint, not the required v1 CPU parity.

The Morph cold candidate attempt was CONTENDED after a foreign compiler resumed. Every timing value from that attempt is excluded. Three remaining Morph rows were not run; no admitted Morph regression or win exists. No timing repeat followed the clear Horns whole-CPU HOLD verdict. Full-instrument frozen-v1 CLAP output remains UNKNOWN/unrun; W12’s exact `814b9b47` API/host and numeric-only protocol are prepared, with family capture excluded from CPU acceptance. No PCM was persisted and no Kontakt/native parity is claimed.

Correctness remains checked: literal v1 paired EQ output/state bit-null at 1/2/3/7/31/32/33/63/64 frames; 39 focused DSP tests, six control timeline tests, one SIMD dispatch test, runtime Eq/Mix zero heap; raw descriptor loss RED → four import tests and ten public control bit/heap tests GREEN; three-crate ci no-run 73 executables.

Artifacts: `/mnt/Windows11/DEV_WORKSPACE/kontra-runs/w9-v1-whole-voice-20261009/{SUMMARY,comparison,candidate-BUILD,DIRECT-W0-DRAINED}.json`, `ACCEPTANCE_SHA256SUMS`, `RAW_AHDSR_GREEN.json`, `WHOLE_VOICE_PARKED.txt`. The CPU/freezer units are inactive/MainPID 0; only W9 request/grant were removed and direct drain sent to W0, W6 and W8.

Whole port parked at its 90-minute cutoff: remaining raw LFO/Flex/ordered external descriptors and initialization overlay, fused voice, independent grouped filter/lane consumers are unwired. See `w9-whole-voice-preparation.md` for the concrete consumer boundaries.

NEXT: source-only typed prepared mapping handoff; missing Morph/full-output acceptance waits for a separately admitted window.
