# Envelope/modulation ports from v1

Status: failing-first fixtures; performance acceptance pending. Branch `v2/w6-envmod-perf`, runtime baseline `b1e4e036`. No resampling or mixing edits; W9 owns those.

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

## All thirteen admitted slow cells

Classification describes shared fast-path applicability; only the three coordinator-selected rows receive new timed A/Bs. Every whole-cell p99 was slower in frozen CPU42, so none is labelled CPU-equal.

| Corpus id | Block | Classification | New timing |
|---|---:|---|---|
| `9f470d1a095b` | 256 | port: shared env/mod; W9 resample/mix | outside short A/B; prior CPU42 retained |
| `479783f469f0` | 32 | port: shared env/mod; W9 resample/mix | outside short A/B; prior CPU42 retained |
| `479783f469f0` | 64 | port: shared env/mod; W9 resample/mix | Horns/Morph paired v1/v2 A/B pending |
| `479783f469f0` | 256 | port: shared env/mod; W9 resample/mix | Horns/Morph paired v1/v2 A/B pending |
| `ac80977a6a94` | 64 | port: shared env/mod; W9 resample/mix | outside short A/B; prior CPU42 retained |
| `ac80977a6a94` | 256 | port: shared env/mod; W9 resample/mix | outside short A/B; prior CPU42 retained |
| `451af2d0b06e` | 256 | port: shared env/mod; W9 resample/mix | Horns/Morph paired v1/v2 A/B pending |
| `53f9c27e4ea3` | 32 | port: shared env/mod; W9 resample/mix | outside short A/B; prior CPU42 retained |
| `53f9c27e4ea3` | 64 | port: shared env/mod; W9 resample/mix | outside short A/B; prior CPU42 retained |
| `53f9c27e4ea3` | 256 | port: shared env/mod; W9 resample/mix | outside short A/B; prior CPU42 retained |
| `751d9d879baf` | 32 | port: shared env/mod; W9 resample/mix | outside short A/B; prior CPU42 retained |
| `751d9d879baf` | 64 | port: shared env/mod; W9 resample/mix | outside short A/B; prior CPU42 retained |
| `751d9d879baf` | 256 | port: shared env/mod; W9 resample/mix | outside short A/B; prior CPU42 retained |

## Validation contract

Failing-first work budgets: a curved 32-frame advance computes one requested level, a regular 128-point shaper uses at most three point comparisons, and one addressed filter among 1024 performs at most two scope queries. Numeric oracles cover every curvature including step/subnormals, phase boundaries, release/reuse, partial blocks, shape boundaries and MIDI values. Callback allocation guards and baseline/candidate PCM hashes are required. Timing uses paired frozen v1, untouched v2 baseline and candidate on Horns256/64 plus Morph256, with no PCM retained on disk. An unchanged baseline/candidate hash proves this optimization did not worsen the existing v1 audio difference; it does not assert native-host parity.

NEXT: targeted RED→GREEN and no-run → first-ready quiet A/B → W0 READY.
