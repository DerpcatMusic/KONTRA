# Matched v1 / v2 stage profiles — 2026-10-08

V1 product source is pinned to `0cb7a8a0`; the adapter/probe branch is `audit/w9-v1-cpu-adapter-20261008` (`42a0ae91`, then profile epoch/equivalence docs `41a4a5d8`). Product source, Cargo.toml and v1 Cargo.lock are unchanged. The frozen CLI equivalence validation passed three alternating pairs each for piano and ANALOG STRINGS: end-to-end process CPU medians +0.28% / −0.75%, equal preload/voice census/underruns. Frozen gate adapter and BUILD.json live under `~/.cache/kontra-scan/cpu-v1/bin/`; the original `~/.cache/kontra-v1` remains untouched. W8 has the path.

V2 product source is exactly `73e6089b16ca4d396965ecc305b2ca18ebd166ba`. Only its probe has an epoch marker outside timed/counted sections. V1 uses its own release profile and lockfile; v2 uses corpus (release/thin-LTO). Profiles preserve symbols and line tables, with no target-cpu override. Profile binaries SHA256: v1 `5537c8fbfa29bb2b23860bf789d5122b313e78c614419bf870fbd3146907569f`; v2 `d28557b00ec6d896a71617931ec9fe1613e36b2acf679d184a4ccc9ed1d60585`.

## Measurement scope

48 original-instrument profile runs: eight cold/warm cells × two engines × three alternating repeats. Exact audit note sequence and 48kHz block sizes; audio never leaves RAM. Audio-TID-only leaf IP samples use `cpu-clock:u` at9999Hz with realtime timestamps. The explicit probe epoch selects 0.25–1.0s steady-note samples, matching the audit steady interval. Load, idle warmup and teardown are excluded. Decoder/worker costs are excluded from audio-thread costs. No raw user stack, registers or library payloads are recorded.

Costs below are sampled exclusive user-CPU µs per steady block, pooled over the three runs. They are estimates, not instrumented stage timers, p50 stage latencies, or an exact decomposition of a median difference. Function/source-line attribution follows addr2line inline records; ELF file offsets are converted through the executable LOAD segment. Each sample is assigned once and all buckets conserve its period. Explicit DSP branches and inline envelope/control locations are classified separately. Shared libm calls, ambiguous dispatch/validation, fused resample/mix and libc/unknown locations remain separate. A dash means no sample, not zero cost. One nominal sample corresponds to about0.03/0.06/0.24µs per pooled block at32/64/256 respectively. Low v1 sample counts and software-clock IP bias limit fine rankings, especially the timing bucket. Raw per-run ranges, symbol lists and every assigned IP remain in `~/.cache/kontakto-fix-cpu/stage-attribution/`.

These engines do not produce equal PCM/voice selection in these cells; identical input is a CPU comparison, not native sound parity or a per-voice capacity promise. Profiles are diagnostic; the unprofiled40-run original-instrument comparison is separately running and decides end-to-end timing/counters. No acceptance or release claim.

## piano-32-cold

Profile samples v1/v2: 108/1357. Three-run median of profiled block medians/p99s: v1 4.271/12.711µs; v2 38.411/62.391µs. Underruns 0/0; deadlines 0/3.

| Exclusive stage | v1 sampled µs/block | v2 sampled µs/block |
|---|---:|---:|
| voice render | 0.12 | 3.08 |
| resample | 0.06 | 2.55 |
| envelope/mod | — | 10.99 |
| filter | 0.12 | 0.03 |
| FX | 1.33 | 0.15 |
| mixing | 0.24 | 3.53 |
| stream service | 0.03 | 0.65 |
| callback/event | 0.41 | 0.44 |
| fused resample/mix | — | — |
| DSP dispatch/validation | — | 1.39 |
| harness/timing | 0.30 | 0.18 |
| unattributed libm | — | 15.50 |
| unattributed other | 0.59 | 1.72 |
| All sampled buckets | 3.20 | 40.21 |

## piano-32-warm

Profile samples v1/v2: 128/1331. Three-run median of profiled block medians/p99s: v1 4.340/14.111µs; v2 36.740/57.652µs. Underruns 0/0; deadlines 0/2.

| Exclusive stage | v1 sampled µs/block | v2 sampled µs/block |
|---|---:|---:|
| voice render | 0.09 | 3.26 |
| resample | 0.06 | 3.08 |
| envelope/mod | 0.03 | 9.75 |
| filter | 0.15 | — |
| FX | 1.60 | 0.15 |
| mixing | 0.27 | 4.24 |
| stream service | — | 0.50 |
| callback/event | 0.62 | 0.24 |
| fused resample/mix | — | — |
| DSP dispatch/validation | — | 1.54 |
| harness/timing | 0.21 | 0.12 |
| unattributed libm | — | 15.26 |
| unattributed other | 0.77 | 1.30 |
| All sampled buckets | 3.79 | 39.44 |

## piano-64-cold

Profile samples v1/v2: 145/930. Three-run median of profiled block medians/p99s: v1 6.380/23.990µs; v2 47.981/84.152µs. Underruns 0/3; deadlines 0/0.

| Exclusive stage | v1 sampled µs/block | v2 sampled µs/block |
|---|---:|---:|
| voice render | 0.42 | 5.16 |
| resample | 0.42 | 6.29 |
| envelope/mod | 0.12 | 13.70 |
| filter | 1.19 | — |
| FX | 3.32 | 0.59 |
| mixing | 0.83 | 8.60 |
| stream service | — | 1.25 |
| callback/event | 0.53 | 0.65 |
| fused resample/mix | 0.18 | — |
| DSP dispatch/validation | — | 2.73 |
| harness/timing | 0.06 | 0.12 |
| unattributed libm | — | 12.81 |
| unattributed other | 1.54 | 3.26 |
| All sampled buckets | 8.60 | 55.17 |

## piano-64-warm

Profile samples v1/v2: 114/825. Three-run median of profiled block medians/p99s: v1 5.500/24.260µs; v2 46.831/75.861µs. Underruns 0/0; deadlines 0/0.

| Exclusive stage | v1 sampled µs/block | v2 sampled µs/block |
|---|---:|---:|
| voice render | 0.47 | 3.26 |
| resample | 0.24 | 5.28 |
| envelope/mod | — | 13.82 |
| filter | 0.83 | — |
| FX | 2.19 | 0.30 |
| mixing | 0.71 | 5.99 |
| stream service | 0.06 | 0.53 |
| callback/event | 0.65 | 0.53 |
| fused resample/mix | 0.24 | — |
| DSP dispatch/validation | — | 3.03 |
| harness/timing | 0.18 | 0.06 |
| unattributed libm | — | 14.65 |
| unattributed other | 1.19 | 1.48 |
| All sampled buckets | 6.76 | 48.94 |

## fx-64-cold

Profile samples v1/v2: 832/982. Three-run median of profiled block medians/p99s: v1 45.060/81.651µs; v2 54.751/119.433µs. Underruns 0/0; deadlines 1/0.

| Exclusive stage | v1 sampled µs/block | v2 sampled µs/block |
|---|---:|---:|
| voice render | 5.75 | 4.98 |
| resample | 0.89 | 0.36 |
| envelope/mod | 1.07 | 13.05 |
| filter | 10.20 | 0.77 |
| FX | 5.40 | 3.74 |
| mixing | — | 7.00 |
| stream service | 0.24 | 11.27 |
| callback/event | 12.22 | 0.53 |
| fused resample/mix | 0.36 | — |
| DSP dispatch/validation | — | 2.43 |
| harness/timing | 5.34 | 0.06 |
| unattributed libm | 1.96 | 0.24 |
| unattributed other | 5.93 | 13.82 |
| All sampled buckets | 49.35 | 58.25 |

## fx-64-warm

Profile samples v1/v2: 838/969. Three-run median of profiled block medians/p99s: v1 46.900/88.012µs; v2 55.311/118.232µs. Underruns 0/0; deadlines 1/0.

| Exclusive stage | v1 sampled µs/block | v2 sampled µs/block |
|---|---:|---:|
| voice render | 4.03 | 5.93 |
| resample | 1.13 | 0.42 |
| envelope/mod | 1.36 | 12.69 |
| filter | 10.86 | 0.83 |
| FX | 3.74 | 2.79 |
| mixing | 0.12 | 7.47 |
| stream service | 0.30 | 10.80 |
| callback/event | 13.76 | 0.53 |
| fused resample/mix | 0.65 | — |
| DSP dispatch/validation | — | 1.13 |
| harness/timing | 5.40 | 0.12 |
| unattributed libm | 1.60 | 0.47 |
| unattributed other | 6.76 | 14.30 |
| All sampled buckets | 49.71 | 57.48 |

## fx-256-cold

Profile samples v1/v2: 814/843. Three-run median of profiled block medians/p99s: v1 186.603/322.686µs; v2 202.284/291.706µs. Underruns 0/0; deadlines 0/0.

| Exclusive stage | v1 sampled µs/block | v2 sampled µs/block |
|---|---:|---:|
| voice render | 21.52 | 27.19 |
| resample | 3.78 | 1.18 |
| envelope/mod | 5.44 | 55.09 |
| filter | 43.27 | 2.36 |
| FX | 20.57 | 13.95 |
| mixing | 0.24 | 28.14 |
| stream service | 2.13 | 27.19 |
| callback/event | 42.32 | 1.18 |
| fused resample/mix | 1.89 | — |
| DSP dispatch/validation | — | 5.67 |
| harness/timing | 18.21 | 0.24 |
| unattributed libm | 6.38 | 1.42 |
| unattributed other | 26.72 | 35.70 |
| All sampled buckets | 192.45 | 199.31 |

## fx-256-warm

Profile samples v1/v2: 800/869. Three-run median of profiled block medians/p99s: v1 185.643/304.676µs; v2 201.053/350.087µs. Underruns 0/0; deadlines 0/0.

| Exclusive stage | v1 sampled µs/block | v2 sampled µs/block |
|---|---:|---:|
| voice render | 17.73 | 21.52 |
| resample | 5.20 | 1.18 |
| envelope/mod | 3.07 | 55.09 |
| filter | 37.12 | 4.02 |
| FX | 21.99 | 10.64 |
| mixing | 0.47 | 31.45 |
| stream service | 1.66 | 32.63 |
| callback/event | 50.83 | 2.36 |
| fused resample/mix | 1.18 | — |
| DSP dispatch/validation | — | 5.20 |
| harness/timing | 20.33 | 0.71 |
| unattributed libm | 5.20 | 1.66 |
| unattributed other | 24.35 | 39.01 |
| All sampled buckets | 189.14 | 205.46 |

## First port target

Envelope/modulation is the largest identified v2 stage on piano and FX, excluding unattributed shared math. Piano64 cold is0.12→13.70µs/block, warm no v1 sample→13.82; FX64 cold1.07→13.05, warm1.36→12.69. The v2 fill_filter_factors loop clears and converts every filter entry for each voice, including neutral, unaddressed entries. It alone contributes a substantial share of the identified modulation samples; exp2/pow are also expensive in piano, but leaf-only profiles do not prove all of their callers. The observed profile gap is much larger than the earlier8µs estimate; no table is forced to sum to8.

V1 `0cb7a8a0:src/engine/filter.rs:1382` applies only nonzero modulation deltas: its enumerate/filter loop skips normalized/stored conversions for neutral knobs. The first narrow port will copy that nonzero-delta iteration into v2 factor conversion, adapting neutral output to multiplicative1 and preserving cutoff/resonance units and existing errors. This targets the measured modulation path, with no resampler change. It will be accepted only if the separate40-run original-instrument before/after gate worsens no p99, deadline or underrun cell. Earlier neutral-factor work on v2/fix-cpu is not present at73e6089b; this is a pinned-baseline port trial, not a new resampler improvement.

## Unprofiled v1 /73 original-instrument comparison

40 runs: all original piano/ANALOG STRINGS32/64/256 cold+warm cells, plus two additional repeats of the four64-frame cells per engine. Frozen v1 gate binary SHA256 `b9998ca2ce2f2ed4f9f88bbfb11c5e884fa162a87cdf89f26ece6f1248fdc6ab`; v2/73 source probe SHA256 `ca4294d7316ac40c5705bfb3f100f091078bacafe973a57b13773a8389d079b7`. All40 return0, all event/render heap counts0, all20 cold evictions pages_after=0. Warm means unforced Linux file cache. No rows are excluded.

| Cell | Repeat | v1 median/p99 µs | v2/73 median/p99 µs | Underruns v1/v2 | Deadlines v1/v2 |
|---|---:|---:|---:|---:|---:|
| fx-256 | 1 | 175.044/304.516 | 191.333/296.715 | 0/0 | 0/0 |
| fx-256-cold | 1 | 174.894/288.145 | 203.473/313.216 | 0/0 | 0/0 |
| fx-32 | 1 | 16.850/49.641 | 51.261/105.442 | 0/0 | 1/1 |
| fx-32-cold | 1 | 17.981/49.231 | 46.461/94.172 | 0/1 | 1/1 |
| fx-64 | 1 | 42.321/72.281 | 50.981/105.452 | 0/0 | 0/0 |
| fx-64-cold | 1 | 44.071/81.892 | 52.041/110.312 | 0/0 | 1/0 |
| piano-256 | 1 | 15.310/44.250 | 169.693/215.195 | 0/0 | 0/1 |
| piano-256-cold | 1 | 17.660/39.191 | 173.803/220.494 | 0/0 | 0/1 |
| piano-32 | 1 | 4.370/11.860 | 37.041/58.741 | 0/0 | 0/0 |
| piano-32-cold | 1 | 4.190/11.590 | 36.660/56.742 | 0/0 | 0/1 |
| piano-64 | 1 | 5.620/24.180 | 44.091/67.701 | 0/0 | 0/0 |
| piano-64-cold | 1 | 6.000/21.530 | 45.910/75.842 | 0/0 | 0/0 |
| fx-64 | 2 | 50.971/117.382 | 54.711/119.223 | 0/0 | 4/0 |
| fx-64-cold | 2 | 43.831/79.622 | 51.101/115.342 | 0/0 | 2/0 |
| piano-64 | 2 | 5.310/20.101 | 44.291/73.151 | 0/0 | 0/0 |
| piano-64-cold | 2 | 5.340/15.150 | 44.631/65.572 | 0/0 | 0/0 |
| fx-64 | 3 | 44.871/83.401 | 53.341/103.902 | 0/0 | 1/0 |
| fx-64-cold | 3 | 43.591/83.542 | 52.661/118.273 | 0/0 | 0/0 |
| piano-64 | 3 | 5.850/23.260 | 44.631/68.721 | 0/0 | 0/0 |
| piano-64-cold | 3 | 5.051/15.280 | 44.451/82.722 | 0/0 | 0/0 |

Three-run64 aggregates (median of run medians / median of run p99s):

| Cell | v1 µs | v2/73 µs |
|---|---:|---:|
| piano-64-cold | 5.340/15.280 | 44.631/75.842 |
| piano-64-warm | 5.620/23.260 | 44.291/68.721 |
| fx-64-cold | 43.831/81.892 | 52.041/115.342 |
| fx-64-warm | 44.871/83.401 | 53.341/105.452 |

All40 counters: v1 10 deadlines/0 underruns; v2/73 5/1. This pinned-baseline comparison establishes the remaining gap; it is not the neutral port's acceptance gate. That separate40 compares the same debug/epoch probe baseline with the port, with profiling disabled.

## Neutral-delta port checkpoint

Copies v1's enumerate/filter nonzero-delta loop from `0cb7a8a0:src/engine/filter.rs`, adapting neutral modulation to multiplicative1 and converting only nonzero cutoff semitones / resonance dB. Caller remains FilterBank::set_addressed_modulation; no admission, storage, resampler or event behavior changes. The regression fixture checks bit-exact cutoff/resonance units, summed-route cancellation, positive/negative/signed-zero deltas, an unrelated pitch route, untouched filter entries and scratch reuse by an unbound voice.

21 targeted voice-modulation/control-DSP checks pass, with one existing ignored test. Default root `cargo test --no-run` passes. Candidate debug/epoch probe SHA256 `0f9fbd59a0760f5d7899798c664f27c5ac1eee26ee938cccf4af48afd5837201`; matched before probe `d28557b00ec6d896a71617931ec9fe1613e36b2acf679d184a4ccc9ed1d60585`. Both use the same release-derived profile, line symbols and epoch-only probe change. The original40-run before/after gate is underway, with profiling disabled, before any acceptance or W0 handoff. Matched diagnostic profiles will separately assess whether the shared exp2/pow cost drops. **HOLD pending measurement.**
