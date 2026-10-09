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

V1 `0cb7a8a0:src/engine/filter.rs:1382` applies only nonzero modulation deltas: its enumerate/filter loop skips normalized/stored conversions for neutral knobs. The first narrow port will copy that nonzero-delta iteration into v2 factor conversion, adapting neutral output to multiplicative1 and preserving cutoff/resonance units and existing errors. This targets the measured modulation path, with no resampler change. Acceptance uses the separate 40-run original-instrument before/after gate, with the coordinator's subsequent A/A calibration and quiet-machine underrun attribution rules described below. Earlier neutral-factor work on v2/fix-cpu is not present at73e6089b; this is a pinned-baseline port trial, not a new resampler improvement.

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


## Neutral-delta original 40 and calibrated gate (pending)

All 40 before/after runs completed; event/render heap calls are zero and all 20 cold mincore receipts verify pages_after=0. Before/after deadlines total 7/7, storage underruns 2/4. The final after FX64 warm run was delayed when the target disk dropped below the initial 18 GiB assertion; only that missing run was resumed after verifying frozen binaries and deleting this worktree's idle incremental directory. The failed driver status and its log remain retained; no completed run was replaced.

| Cell | Before median/p99 µs | After median/p99 µs | Underruns before/after | Deadlines before/after |
|---|---:|---:|---:|---:|
| fx 32 warm | 50.861/104.642 | 60.801/129.452 | 0/0 | 1/2 |
| fx 32 cold | 52.271/114.262 | 52.591/109.452 | 2/0 | 1/1 |
| fx 64 warm | 54.321/118.102 | 55.072/120.082 | 0/4 | 0/0 |
| fx 64 cold | 52.721/113.653 | 51.971/115.222 | 0/0 | 0/0 |
| fx 256 warm | 193.204/273.695 | 201.434/296.096 | 0/0 | 0/0 |
| fx 256 cold | 194.904/267.885 | 190.143/275.615 | 0/0 | 0/0 |
| piano 32 warm | 36.581/54.141 | 21.911/42.560 | 0/0 | 1/1 |
| piano 32 cold | 36.421/58.812 | 21.950/39.381 | 0/0 | 2/1 |
| piano 64 warm | 45.251/76.051 | 30.611/53.061 | 0/0 | 0/0 |
| piano 64 cold | 45.471/79.932 | 33.221/71.081 | 0/0 | 0/0 |
| piano 256 warm | 180.093/269.855 | 115.422/160.953 | 0/0 | 1/1 |
| piano 256 cold | 182.414/309.526 | 120.863/193.074 | 0/0 | 1/1 |

64-frame entries summarize three runs (median of run medians / median of run p99s); 32/256 entries initially have one run each. All raw pairs remain included in `neutral-port-matrix/`.

Matched three-pair piano64 profiles reproduce the libm hypothesis: cold unattributed libm drops 12.813→0.178 µs/block and envelope/mod 12.991→8.008; warm libm 13.880→0.415 and envelope/mod 12.219→7.771. These are exclusive sampled user-CPU estimates, not direct stage timers; unknown caller attribution remains explicit. All 12 diagnostic receipts and per-IP/source mappings are retained in `neutral-port-profiles/` and `neutral-port-attribution/`.

The coordinator then refined acceptance to measure the A/A noise floor rather than rejecting every positive p99 delta. Three alternating baseline/baseline pairs are running on FX32 warm, FX64 cold/warm, and FX256 cold/warm. The protocol records maximum absolute paired p99 and deadline-count differences as the empirical spread and compares the candidate's paired losses; cells beyond it receive up to three additional A/B pairs. Underruns remain zero-tolerance in the release gate. When A/A itself reports storage underruns, the affected A/B cell is repeated with other heavy/census jobs inactive before attributing the fault to code. The original 40/profile runs lack contemporaneous heavy-unit snapshots; their quietness is unknown. New runs record timestamped `systemctl --user list-units 'kontakto-*' '*census*'` plus heavy/scanner process activity, with a two-second timeline covering the ongoing A/A run. No historical quietness is inferred.

New gate receipts are symlinked to `/mnt/Windows11/DEV_WORKSPACE/kontra-runs/w9-neutral-aa`; read-mostly gate runs may continue at root ≥16 GiB under the coordinator's instruction. **HOLD: calibration and quiet attribution are pending. No accepted SHA has been handed to W0.**


## Calibrated neutral-port decision: ACCEPT for performance attribution; release HOLD

The A/A set completed 30 runs: three alternating pairs on each of the five FX cells. All return 0, event/render heap counters are zero, and cold mincore checks report pages_after=0. Baseline itself reported 15 storage underruns (FX64 cold 5; FX256 cold 10). Maximum absolute paired p99 spreads were 23.460 µs at FX32 warm, 14.620/9.520 at FX64 cold/warm, and 47.130/66.481 at FX256 cold/warm. All initial candidate p99 losses fit those measured spreads except FX32 warm's 24.810 µs; its initial extra deadline also required repeats.

After A/A, W9 created the authorized quiet request. Census granted it at 17:25:16 UTC. Twelve additional alternating pairs (repeats 4–6) covered FX32 warm, FX64 cold/warm and FX256 cold, with pre-pair guards, start/end activity snapshots and a two-second heavy/census/scanner timeline. All 24 probes returned 0 with no audio heap calls. The request was removed after the final run at 17:28 UTC, within the 45-minute cap. Two runs experienced a mid-run external build intrusion despite passing the pre-pair guard: repeat4 before FX64 cold overlapped W7's fix-render test, and repeat4 after FX256 cold overlapped W6's sampler-uvi no-run build. These runs remain in every raw/aggregate table and are not claimed quiet. The second had two candidate underruns; all uncontaminated candidate runs had zero. An uncontaminated baseline repeat5 FX256 cold itself reported four underruns. Candidate-only storage faults therefore did not repeat under the quiet attribution rule. The original four candidate FX64 warm underruns also did not repeat in any additional pair.

| Additional cell (3 pairs) | Before median/p99 µs | After median/p99 µs | Underruns before/after | Deadlines before/after |
|---|---:|---:|---:|---:|
| FX 32 warm | 45.671/87.662 | 44.161/86.882 | 0/0 | 3/3 |
| FX 64 warm | 52.161/107.842 | 49.341/98.432 | 0/0 | 0/0 |
| FX 64 cold | 47.171/85.482 | 47.711/85.802 | 0/0 | 0/0 |
| FX 256 cold | 178.674/262.175 | 180.633/258.185 | 4/2 | 0/0 |

The extra FX32 warm deadline did not repeat: all three additional pairs have one deadline per side. Initial/additional combined aggregate p99 changes are FX32 warm −7.225 µs, FX64 warm −1.025, FX64 cold +9.536 (below its 14.620 A/A spread), and FX256 cold −4.800. FX256 warm's initial +22.401 fits its 66.481 spread and needed no extra pair. The raw FX32 total remains four before / five after deadlines over four pairs; its sole extra count is in the initial unobserved-contended run, not a repeated quiet loss. Maximum paired outliers remain visible (including the +28.420 µs repeat4 FX64 cold pair); final attribution uses the repeated aggregate evidence rather than permanently deciding from the first noisy maximum.

Source commit `8b2929cd953038a9f4faf98717d826d53c0da0f0` was handed to W0 and the coordinator as the accepted narrow performance port under the updated A/A and quiet-attribution instructions. The source change preserves mathematical units, validation and callback/storage ownership, and the original piano improvements plus measured libm drop stand. **This is not a zero-underrun release pass: original, A/A and additional storage faults remain recorded, and v2 still trails v1. Global release HOLD remains.** A fresh full eight-cell matched v1/accepted-v2 stage profile is running before the separately gated mixing port.

Receipts: `neutral-port-aa/`, `neutral-port-quiet/`, `neutral-port-calibration-summary.json`, `neutral-port-all-ab.json`, and per-run `quiet-validation.json`. New gate output directories are symlinked under the target volume; the root read-mostly floor is 16 GiB. Activity capture began after the coordinator's request; older runs do not acquire a retrospective quiet claim.


## Post-acceptance stage table: v1 versus 8b2929cd

Fresh 48-run matched rotation: three alternating pairs per original piano32/64 and FX64/256 cold/warm cell. Frozen binaries: v1 profile `5537c8fbfa29bb2b23860bf789d5122b313e78c614419bf870fbd3146907569f`; accepted v2 `0f9fbd59a0760f5d7899798c664f27c5ac1eee26ee938cccf4af48afd5837201`. All run/metadata receipts and per-IP source mappings are retained in `neutral-stage-profiles/` and `neutral-stage-attribution/`. Epoch/audio-TID/steady-window selection and exclusive attribution use the same method and limits as the first table. Shared libm and unknown/fused categories remain explicit; sampled stage costs are estimates, not a decomposition of timed median/p99. Each entry below is v1→v2 µs/block. Zero means no sampled cost, not proof of free work. Instrument voice/source semantics are unchanged within each engine, not asserted equivalent across engines.

### piano 32 cold

| Stage | v1→accepted v2 µs/block |
|---|---:|
| voice render | 0.356→3.912 |
| resample | 0.267→4.712 |
| envelope/mod | 0.119→8.090 |
| filter | 0.889→0.059 |
| FX | 2.282→0.207 |
| mixing | 1.008→4.534 |
| stream service | 0.030→0.919 |
| callback/event | 0.415→0.296 |
| fused resample/mix | 0.059→0.000 |
| DSP dispatch/validation | 0.000→2.371 |
| harness/timing | 0.207→0.089 |
| unattributed libm | 0.000→0.178 |
| unattributed other | 0.682→1.956 |

v1: profiled median/p99 4.451/16.580 µs; underruns 0, deadlines 0.

v2: profiled median/p99 22.970/44.451 µs; underruns 0, deadlines 4.
### piano 32 warm

| Stage | v1→accepted v2 µs/block |
|---|---:|
| voice render | 0.059→3.171 |
| resample | 0.030→5.008 |
| envelope/mod | 0.000→6.578 |
| filter | 0.148→0.000 |
| FX | 1.541→0.178 |
| mixing | 0.326→4.356 |
| stream service | 0.000→0.593 |
| callback/event | 0.415→0.385 |
| fused resample/mix | 0.000→0.000 |
| DSP dispatch/validation | 0.000→2.726 |
| harness/timing | 0.356→0.267 |
| unattributed libm | 0.000→0.207 |
| unattributed other | 0.622→1.689 |

v1: profiled median/p99 4.330/14.060 µs; underruns 0, deadlines 0.

v2: profiled median/p99 22.461/47.121 µs; underruns 0, deadlines 3.
### piano 64 cold

| Stage | v1→accepted v2 µs/block |
|---|---:|
| voice render | 0.297→3.678 |
| resample | 0.237→6.584 |
| envelope/mod | 0.059→7.830 |
| filter | 0.534→0.059 |
| FX | 1.957→0.178 |
| mixing | 0.475→8.186 |
| stream service | 0.000→0.534 |
| callback/event | 0.297→0.534 |
| fused resample/mix | 0.059→0.000 |
| DSP dispatch/validation | 0.000→3.144 |
| harness/timing | 0.237→0.059 |
| unattributed libm | 0.000→0.119 |
| unattributed other | 0.712→1.720 |

v1: profiled median/p99 5.270/20.851 µs; underruns 0, deadlines 0.

v2: profiled median/p99 30.801/53.661 µs; underruns 0, deadlines 0.
### piano 64 warm

| Stage | v1→accepted v2 µs/block |
|---|---:|
| voice render | 0.475→4.923 |
| resample | 0.297→6.762 |
| envelope/mod | 0.059→9.313 |
| filter | 0.652→0.059 |
| FX | 1.898→0.119 |
| mixing | 0.534→7.474 |
| stream service | 0.119→0.534 |
| callback/event | 0.534→0.178 |
| fused resample/mix | 0.000→0.000 |
| DSP dispatch/validation | 0.000→3.025 |
| harness/timing | 0.356→0.059 |
| unattributed libm | 0.000→0.178 |
| unattributed other | 1.246→1.542 |

v1: profiled median/p99 6.210/21.790 µs; underruns 0, deadlines 0.

v2: profiled median/p99 30.830/55.151 µs; underruns 0, deadlines 0.
### fx 64 cold

| Stage | v1→accepted v2 µs/block |
|---|---:|
| voice render | 4.627→5.932 |
| resample | 0.830→0.297 |
| envelope/mod | 1.008→14.058 |
| filter | 10.677→0.652 |
| FX | 5.695→4.567 |
| mixing | 0.178→9.372 |
| stream service | 0.415→12.397 |
| callback/event | 13.702→0.771 |
| fused resample/mix | 0.593→0.000 |
| DSP dispatch/validation | 0.000→2.254 |
| harness/timing | 5.398→0.237 |
| unattributed libm | 1.483→0.356 |
| unattributed other | 5.695→14.652 |

v1: profiled median/p99 43.940/76.541 µs; underruns 0, deadlines 1.

v2: profiled median/p99 54.741/119.422 µs; underruns 0, deadlines 3.
### fx 64 warm

| Stage | v1→accepted v2 µs/block |
|---|---:|
| voice render | 4.686→6.822 |
| resample | 1.008→0.712 |
| envelope/mod | 1.364→13.228 |
| filter | 9.906→1.008 |
| FX | 3.559→5.161 |
| mixing | 0.000→7.415 |
| stream service | 0.415→11.567 |
| callback/event | 13.109→0.534 |
| fused resample/mix | 0.771→0.000 |
| DSP dispatch/validation | 0.000→2.135 |
| harness/timing | 6.110→0.119 |
| unattributed libm | 1.305→0.178 |
| unattributed other | 5.635→12.575 |

v1: profiled median/p99 44.751/78.982 µs; underruns 0, deadlines 1.

v2: profiled median/p99 54.121/112.982 µs; underruns 0, deadlines 0.
### fx 256 cold

| Stage | v1→accepted v2 µs/block |
|---|---:|
| voice render | 19.624→24.589 |
| resample | 2.601→1.655 |
| envelope/mod | 4.492→58.398 |
| filter | 46.813→3.783 |
| FX | 18.678→9.457 |
| mixing | 0.000→29.790 |
| stream service | 1.891→22.224 |
| callback/event | 45.395→2.128 |
| fused resample/mix | 0.709→0.000 |
| DSP dispatch/validation | 0.000→6.856 |
| harness/timing | 16.787→0.473 |
| unattributed libm | 6.856→1.891 |
| unattributed other | 23.643→41.139 |

v1: profiled median/p99 180.053/308.296 µs; underruns 0, deadlines 0.

v2: profiled median/p99 201.923/308.966 µs; underruns 1, deadlines 0.
### fx 256 warm

| Stage | v1→accepted v2 µs/block |
|---|---:|
| voice render | 20.333→31.918 |
| resample | 3.783→1.419 |
| envelope/mod | 3.546→57.689 |
| filter | 41.139→2.128 |
| FX | 18.205→14.659 |
| mixing | 0.236→32.864 |
| stream service | 1.891→28.608 |
| callback/event | 48.468→2.837 |
| fused resample/mix | 1.419→0.000 |
| DSP dispatch/validation | 0.000→11.349 |
| harness/timing | 17.732→0.473 |
| unattributed libm | 6.147→1.891 |
| unattributed other | 19.151→52.488 |

v1: profiled median/p99 179.323/300.186 µs; underruns 0, deadlines 0.

v2: profiled median/p99 204.104/291.965 µs; underruns 1, deadlines 3.

The updated piano64 cold gaps are mixing 0.475→8.186, resample 0.237→6.584, voice render 0.297→3.678, and DSP dispatch/validation 0→3.144. Unattributed libm is now 0→0.119. At FX256 cold, stream service is 1.891→22.224 µs/block and mixing 0→29.790; the accepted v2 profile still reports one storage underrun versus v1 zero. Quiet-baseline underruns are therefore not dismissed as contention: release tolerance remains zero.

### Streaming comparison and diagnostic limits

On original FX256 cold, v1's adapter reports preload4048 source frames, 514,033,080 resident sample/stream bytes, and 23,595 samples. `0cb7a8a0:src/engine/bank.rs` plans a 4096-frame source preload (MAX_BLOCK128 × MAX_STEP32), reducing it to fit the bank budget. `src/engine/stream.rs` uses RING8192 source frames, CHUNK2048, urgent lead4096, and serves urgent slots across all banks before speculative ring fill. Its worker-local reader cache retains 64 readers. V2 Auto uses lazy heads (0 resident head bytes at publication; measured head hint628 output frames in the original profile), an 8 MiB head budget, and service horizon max(head,4096)+128=4224 output frames; four workers retain 16 readers each. Source/output units differ with pitch; the numerical horizons are comparable at unity pitch only.

An openat/close-only syscall diagnostic on each frozen engine's FX256 cold cell records no new OS opens during the four-second note sequence. This does not establish absence of reader churn: archive samples share already-open file handles while constructing new NCW readers. The diagnostic remains retained, records metadata only, and its traced CPU timings are not acceptance results. It cannot justify blindly raising the reader cache.

W8 owns non-stream runtime/script UI RSS attribution; W9 has no active voice/FX/convolution allocation-sizing changes. Streaming reader/cache/head/page-pool sizing stays W9's. The mixing draft caches settled slot levels using the literal v1 slot loop, with the moving-ramp fallback and all validation retained; it has not been built, committed or gated yet.
