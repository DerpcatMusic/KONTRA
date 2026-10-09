# Full branch sweep, batch 382 (candidate 0.3.393)

The existing logical-defect ledger derives 0.3.393 from published 0.3.381 plus twelve accepted fixes; batch labels do not replace the version policy. No local installation is authorized.

## Source coverage

The per-branch `git cherry` sweep covers 121 remote branches and 1,566 distinct historical unmerged commits. 176 original commits were selected with integrated-SHA provenance or exact scoped-tree/edit-sequence proofs. Repeated branch ancestry is counted separately. The complete table and proofs are in `/mnt/Windows11/DEV_WORKSPACE/kontra-runs/w0-stable-382/SWEEP.md`, `SWEEP.json`, `decisions.json` and `applied.json`.

Recovered work includes Report/Settings navigation, exclusive MIDI learn, UVI catalog status, path/metadata caches, state retirement, editor allocation fixes, scheduler persistence and getters, embedded FileContainer samples, source-parameter metadata, streaming admission bounds, and checked v1 envelope/modulation/filter/EQ components. Source-parameter records remain RAM-only. Default content discovery does not inspect Wine; explicit KONTRA_KONTAKT_CONTENT remains supported.

Whole-voice callers and fused resampling are not activated. Reverted heap trim, prefetch/page-pool/high-rate experiments, and recorded regressing bank-range, old voice-lane and idle-reclaim changes remain excluded. W9 held EQ/mix is included on its source correctness and admitted Horns improvement; Morph and full CLAP output remain unknown. Opt-in family audio tools are excluded from scored CPU/load observations.

Editor graphics setup uses preinstalled tools or checksum-verified cached packages with bounded retries/backoff; editor jobs perform no runtime apt access.

## Stable-source validation

Checked source: `f1a067a1c9fbb72a72312838f2163a42d6311333`; source tree: `340f66c0120126f5942c2af954df6b4310de77cc`.

- Workspace/shots: 2,074 passed, zero failed, 261 ignored; 186 Cargo harness sections, nested subprocess summaries excluded.
- Workspace and standalone compile checks passed; scan scheduler 48 passed; reader 45 passed, one ignored.
- Fourteen offline check groups passed. Local GPU hidden-first-present passed; all 24 damage frames matched exactly.
- Native15: 15 loaded and audible, 12 Original views passed, two authored fixtures have no UI; the existing Vista image-reference limit remains. All 15 match published381 regression status.
- Quick72 fresh retry: 72 loaded, 68 audible, no new stage/audibility/fault/stuck-note changes, no nonfinite output or audio-thread allocations against published381.

Twelve integration failures were reproduced and corrected: event/physical voice admission capacity, a rack namespace fixture, cache/saved-script fixtures needing owned file headers, the muted-group diagnostic contract, shared offset-aware AIFF decoding, and the authored monolith zone's verified sample suffix. Targeted regressions and the fresh complete gate passed.

The first parallel quick run had two low-output Pacific cells with missing streaming pages. Frozen before/current signal-graph and plain serial pairs both sounded at identical plain peaks (-52.57619748 and -53.87774526 dB), with zero underruns or script faults; numeric graph reports were complete with zero dropped records. No product change or threshold waiver followed. The unchanged two-worker 72-cell retry passed; the first failure and all follow-up receipts remain available. These unpaced probes do not certify real-host timing or streaming deadlines.

Main eb55fa78 and integrate 54d9a5c5 have identical trees. An ancestry reconciliation preserves the exact tested candidate tree; Later CI-only fixes cover missing ripgrep in the isolated fixture and apt-owned package cache permissions (11 workflow checks and actionlint pass). Codex then identified exact embedded-member paths wrongly rejected by suffix ambiguity. Its authored regression failed, and exact container-relative precedence now passes all three monolith tests, 97 Kontakt unit tests and area no-run. Ambiguous fallback still fails closed; The exact-member defect had no previous ledger entry, so it adds one reviewed fix and derives 0.3.393. The full suite/native receipts above identify f1; this later reader change has its separate targeted receipt. Shipping artifacts and hosted checks require their own receipts.

## Limits and next batch

Protected installed UVI payloads remain parked. Published381's 56 Vista/Pacific instruments had zero translated sample/IR resource failures; historical totals counted untranslated features, not missing samples. Full fidelity, complete widgets, native clocks, load/CPU/RSS/underrun parity, and non-Linux crash validation remain open. Ignored tests are not certified.

Newer READY work is queued in `stable-382/next-batch.json` for batch383: W3 editor cycles, W5 Conflux, W6 wavetable/Digital LFO, W8 captured/sparse persistence, W9 ownership documentation, W10 MIDI/Lua/UUID favorites, W11 causal input receipts, W13 host scheduling, W14 browser/order cache, and W15 legacy filter slots. W11's widget axis remains FAIL; its source tests passing is distinct from library parity.

## Post-review portability and reviewed release notes

Hosted Linux failed the held-EQ exact-bit oracle by one f32 ULP; the local optimized test had passed. W9 kept the paired SSE EQ and coefficient cache, moved the recurrence out of wider dispatch contexts, and evaluated oracle coefficients at runtime like playback. A local baseline/v3 comparison did not reproduce an FMA cause. On the integrated correction, both SIMD tests, all 115 core unit tests, all eight DSP integration tests and workspace/shots compilation pass. Held control reads remain four per 32 frames. These are correctness checks, not a new timing or full-corpus claim; the next hosted Linux run is required to accept portability.

The release now has a reviewed, plain-English 0.3.393 CHANGELOG section covering Loading & files, Sound/engine, KSP, UVI, Saved projects, Editor/UI, Memory & CPU, Fixes and Known limits. Nightly publishes that matching version section verbatim; future versions retain the normal ledger fallback. The publication regression failed before the change; seven focused checks and the complete offline Nightly publication scenarios pass. Evidence remains at the bottom of the user-facing notes. The existing twelve-defect count and version remain unchanged.

Machine evidence: `eq-portability-proof.json`, the preserved failed hosted log, targeted green logs and grouped-note RED/GREEN logs under the stable-382 receipt directory. The complete earlier product/native gate remains identified by its original source, separately from these later corrections.
