# W9 production consumer for pinned v1 controls

Own branch: `v2/w9-v1-whole-voice-381`. This adapts our v1 `0cb7a8a0:src/engine/voice.rs`, `src/engine/params.rs` and `src/engine/bank.rs` evaluators supplied by W6 to the existing v2 cursor, parameter service and DSP graph. W6's original typed descriptor commit `c1d8f2ca` is applied as `fee28276`; the missing parent Conflux fixture remains deleted rather than importing its unrelated wavetable lane.

The consumer is shared by dry/processed, serial/parallel voices. Its state and claim flags are prepared with the voice pool and transferred during pool growth. Only frames consumed by the cursor advance amplitude/Flex; storage-held frames retain their clocks. A lifetime view of the primary envelope preserves retirement and choke handling without advancing it twice. The v2 cursor owns sample positions; this bounded adapter does not generate a second position buffer.

Admission matches physical original group/source slot identity and raw target order to the current IR program. Both native-wrapper and ordinary v1 AHDSR clocks are admitted. One plain unity-volume Flex uses the exact copied f32 conversion; its current saved projection and synthetic release gate must agree. Original external velocity/key/constant/CC/bend/pressure targets retain ordered v1 shaping, lag and gain/pitch laws. EQ destinations retain physical module aliases and their independent target lag. MIDI 1 inputs retain v1 division bits; other controller/pressure values retain full input resolution before f32 DSP conversion.

Original descriptors are saved evidence, not an init/live mirror. Current route depth remains authoritative and must project losslessly into original f32 units. An unrepresentable depth, scale, changed shape, unknown source, secondary AHDSR, active LFO, unsupported destination or ambiguous route declines the whole-control path. Frame-valued envelope overlays and storage-release/choke controls retain their current runtime owner. No private overlay, hashed identity or module-intensity-as-source-gain is introduced. Counter fields in the numeric CPU probe show whether the real audited voices used this consumer.

This is a bounded control consumer in the production voice, not completion of the entire v1 fused render loop. Q32 LFO pitch/volume, pitch envelopes, full live source-module controls, arbitrary curved shapers and grouped DSP dispatch remain open. Existing v2 expression, parameter, streaming and graph behavior remains authoritative for those cases. CPU parity requires the original quiet audit; source wiring alone does not establish a speedup.

The failing-first ordinary production-clock test is `41c30630`: trusted isolated RED exited101, block0/frame0 had output bits0 versus the v1 oracle918405277. V1 ordinary/Flex affine kernels group eight output frames and round at fragment boundaries. The independent evaluator oracle receives the same existing64-frame control splits as production; this checks exact kernels without asserting arbitrary callback-partition bit invariance. Initial expanded verification established two exact CC/growth passes and exposed one-ULP regrouping differences in the incorrectly unsplit oracle. Focused tests additionally cover Flex dry/processed/release/reuse, full-resolution CC and init-adjusted depth, serial/parallel pool growth, physical-identity rejection and callback heap guards. Receipts and the frozen-cell runner live under `/mnt/Windows11/DEV_WORKSPACE/kontra-runs/w9-whole-controls-20261009/`. Shared-target results from the invalid window are excluded.

NEXT: trusted focused GREEN and area no-run, freeze, quiet Horns256, DIRECT to W8.

## Recovery on 2026-10-12

The donor history above is retained as historical evidence. This recovery joins
native-primary `d8f1c623` and ordinary/Flex/external `0158aefa` to the current
core without replacing the current Step curves, source-phase order, digital
fades, sparse filter indices, or typed live control mappings. It also retains
`2eae7382`, `d6c24fbc` and `f630f08f` fixes and their authored regressions.

Preparation now requires the initialized AHDSR to agree with the raw source
coefficients before native admission. A changed initialized Step/timing/shape
keeps the current envelope owner. Ambiguous/missing physical group identity
also declines admission. Invalid native-primary coefficients still fail with
the typed envelope preparation error. External native source values retain
their shaped/lagged state while current route depth and bypass values are read
from the existing prepared control IDs; schema remapping updates those indices.
This fixes a concrete donor integration gap: its immutable saved intensity
would otherwise override later live edits. Bypass masks the destination while
the source lag keeps advancing, and module destinations retain current units.

State and parallel claim flags are prepared per plan generation and voice slot,
reset on every admitted start and transferred during voice pool growth. The
existing note/plan handles keep old held sources attached to their original
coefficients and controls across replacement. A new authored replacement
regression checks both directions of control isolation without heap work.
Additional authored regressions check initialized Step fallback and held
external depth/bypass updates alongside the recovered oracle/release/reuse/
parallel/growth checks. Admission counters are exposed in RuntimeStats and
numeric probe snapshots.

No builds or native host executions were run in this recovery worktree, by
explicit instruction. Syntax parsing only passed; parent central compilation
and tests remain required. Original-byte leaf fixtures already present in
`v1_voice_controls` remain distinct from the copied-v1 whole consumer oracle.
Full native Kontakt sound/timing and current CPU/RSS status remain UNKNOWN.
The rejected/HOLD voice-ring, fused resampler and fold experiments stay out of
production; their original refs and receipts remain accounted separately.
