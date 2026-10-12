# Envelope/modulation ports from v1

Status: READY for integration: 92 core unit tests, 33 targeted integration tests, sampler-core no-run and nine QUIET timed executions passed. Whole-cell v1 performance remains HOLD. Runtime candidate `6556184c`. Branch `v2/w6-envmod-perf`, runtime baseline `b1e4e036`. No resampling or mixing edits; W9 owns those.

## Source and profile

Read `0cb7a8a0:src/engine/voice.rs` (`Envelope::skip`, `run`, `affine_until`), `src/engine/params.rs` (`Mod::shape`, settled `ModTable::modulate`), `src/engine/lfo.rs` (retained 32-frame points), and `src/engine/filter.rs` (group-local modulation rows).

The frozen Horns profiles' env/mod categories include DSP finite checks and DSP levels. Reclassifying by the nearest project's **file path**, rather than symbols containing envelope types, gives approximately 324.86 µs/block for Horns256 and 69.22 for Horns64 (saved category: 401.46/82.39). Sampling estimates only, not acceptance. Largest shaped lookup location: `voice_mod.rs:496` in frozen 5fc; repeated level/curve queries are also visible. Mixing samples remain W9's scope.

## Mechanism classification

- **port:** v1 direct table indexing (`params.rs::Mod::shape`, vendored `ShaperCurve::evaluate`). Prepare regular-grid detection off audio; retain v2's exact original interpolation arithmetic and binary search for irregular/duplicate breakpoints. Do not resample arbitrary curves to v1's 128-point approximation.
- **port:** v1 skip-without-output envelope stage traversal (`voice.rs::Envelope::run`/`affine_until`). Adapt endpoint-only advancement to v2's durations and anchored f64 curve recurrence; retain every observable output/state bit. V1's ordinary exponential decay/release law cannot replace v2's verified normalized finite-stage law.
- **port:** v1 group-local filter modulation rows (`filter.rs::VoiceFilter::process_amplified`), reusing W9's source-only `f85c388c`/`5d71710b` handoff. Prepare unique addressed indices; reset only previous/current program rows in each plan-owned FilterBank. Keep route order, midpoint-before-reduction, native enabled bits, neutral factor bits and parallel lane isolation.
- **already-equal:** retained native control-grid reuse, unchanged-source result reuse for clockless/unlagged programs, off-audio source/state allocation, and existing neutral factor conversion. This labels retained mechanisms, **not equal whole-cell CPU**.
- **v2-only:** arbitrary LFO waves, arbitrary breakpoint geometry, addressed engine-parameter controls, and normalized finite-stage envelope semantics beyond v1's admitted models. Retain their coverage and arithmetic.
- **port (W9):** fused resample/mix, excluded from this branch. Whole-cell filter/FX SIMD/coefficient tuning beyond addressed projection remains unmeasured; a matching processor name is not proof of an equivalent native law.

## Filter/FX source disposition

The existing v2 live EQ (`dsp/eq.rs`) already ports v1 `filter.rs::Section::process_body`; Lo-Fi (`dsp/lofi.rs`) ports v1 `fx/blocks.rs`; LP4 (`dsp/ladder.rs`) retains the pinned v1 kernel. Their disposition is **already-equal at the imported arithmetic mechanism**, with no claim of equal whole-cell CPU. The shared Biquad recurrence has already been consolidated by W6. General SVF, native Daft and complete per-node control addressing retain v2's coverage; replacing them with a narrower v1 law would lose output semantics (**v2-only** where v1 lacks an admitted equivalent). Sparse filter projection is this branch's **port**; W9 owns the fused resample/mix **port**. None of the saved three profiles contains a sampled Daft frame. Kernel name equality is insufficient evidence to replace a native law.

## All thirteen admitted slow cells

Classification describes shared fast-path applicability; only the three coordinator-selected rows receive new timed A/Bs. Every whole-cell p99 was slower in frozen CPU42, so none is labelled CPU-equal.

| Corpus id | Block | Classification | New timing |
|---|---:|---|---|
| `9f470d1a095b` | 256 | port: shared env/mod; W9 resample/mix | outside short A/B; prior CPU42 retained |
| `479783f469f0` | 32 | port: shared env/mod; W9 resample/mix | outside short A/B; prior CPU42 retained |
| `479783f469f0` | 64 | port: shared env/mod; W9 resample/mix | paired QUIET v1/baseline/candidate complete |
| `479783f469f0` | 256 | port: shared env/mod; W9 resample/mix | paired QUIET v1/baseline/candidate complete |
| `ac80977a6a94` | 64 | port: shared env/mod; W9 resample/mix | outside short A/B; prior CPU42 retained |
| `ac80977a6a94` | 256 | port: shared env/mod; W9 resample/mix | outside short A/B; prior CPU42 retained |
| `451af2d0b06e` | 256 | port: shared env/mod; W9 resample/mix | paired QUIET v1/baseline/candidate complete |
| `53f9c27e4ea3` | 32 | port: shared env/mod; W9 resample/mix | outside short A/B; prior CPU42 retained |
| `53f9c27e4ea3` | 64 | port: shared env/mod; W9 resample/mix | outside short A/B; prior CPU42 retained |
| `53f9c27e4ea3` | 256 | port: shared env/mod; W9 resample/mix | outside short A/B; prior CPU42 retained |
| `751d9d879baf` | 32 | port: shared env/mod; W9 resample/mix | outside short A/B; prior CPU42 retained |
| `751d9d879baf` | 64 | port: shared env/mod; W9 resample/mix | outside short A/B; prior CPU42 retained |
| `751d9d879baf` | 256 | port: shared env/mod; W9 resample/mix | outside short A/B; prior CPU42 retained |

## Validation contract

Failing-first work budgets: a curved 32-frame advance computes one requested level, a regular 128-point shaper uses at most three point comparisons, and one addressed filter among 1024 performs at most two scope queries. Numeric oracles cover every curvature including step/subnormals, phase boundaries, release/reuse, partial blocks, shape boundaries and MIDI values. Callback allocation guards and baseline/candidate PCM hashes are required. Timing uses paired frozen v1, untouched v2 baseline and candidate on Horns256/64 plus Morph256, with no PCM retained on disk. An unchanged baseline/candidate hash proves this optimization did not worsen the existing v1 audio difference; it does not assert native-host parity.

## Measured result

Receipt directory: `/mnt/Windows11/DEV_WORKSPACE/kontra-runs/w6-envmod-perf/`. `PROFILE-SOURCE.json`, `CHECKS.json`, frozen `*-BUILD.json`, `AB.json`, `SUMMARY.json` and `RELEASE.json` retain source attribution, RED/GREEN exits, binary hashes, numeric rows, whole-run activity and direct W9 handback. Runtime source `6556184c`, production baseline `b1e4e036`, pinned v1 `0cb7a8a0` adapter `42a0ae91` (reference hashes verified). No W9 fused mixing/resampling port is included.

Steady-state per-block CPU, µs; one paired run per version, all nine executions QUIET:

| Cell | v1 p50 / p99 | v2 baseline p50 / p99 | candidate p50 / p99 | p50 / p99 reduction |
|---|---:|---:|---:|---:|
| Horns64 | 46.770 / 84.392 | 725.433 / 960.318 | 713.114 / 943.718 | 1.70% / 1.73% |
| Horns256 | 167.873 / 301.256 | 2945.126 / 3365.363 | 2905.545 / 3300.182 | 1.34% / 1.94% |
| Morph256 | 40.131 / 49.221 | 682.343 / 840.006 | 640.153 / 766.264 | 6.18% / 8.78% |

This is an isolated source improvement, not the v1 CPU acceptance gate: candidate p99 is still 11.18×, 10.95× and 15.57× v1. The small Horns changes are point estimates; this short run does not establish statistical significance. All peak voice counts match v1 (192 Horns, 32 Morph), but average voice counts/tails and DSP coverage differ. Baseline/candidate counts match exactly. Zero event/render heap calls in every version. Candidate versus baseline deadline misses: 3 versus 3 on Horns64, zero versus zero elsewhere; zero underruns throughout.

All three baseline/candidate offline witnesses are nonzero, finite and bit-exact (384000 stereo scalar samples each); all numeric envelope and shaper oracles pass. Horns silent-note diagnostics already present in baseline persist unchanged. This preserves the candidate's difference to the same fixed v1 audio reference; it does not establish native-host parity or exported CLAP timing. The initial scored-start guard refused a transient KURV build before v1 launched; its failed preflight is retained separately and contributes no timing.

The quiet request/grant were removed after completion; direct successor W9 acknowledged, then W8. Full-workspace/scanner batch acceptance and the remaining ten cell timings were deliberately left to W0/the approved queue.

NEXT: W0 integrate this READY stack; W9 fused resample/mix timing → W8 load/onset/RSS → W0 quick playback.
