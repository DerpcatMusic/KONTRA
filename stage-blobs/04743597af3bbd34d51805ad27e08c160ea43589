# Original Kontakt controls for W9's whole-voice consumer

Base: `484baae4` on `v2/w6-conflux-modfx-381`. W6 supplies forward source descriptors and the copied Flex conversion; W9 alone owns preparation admission, live overlays, whole-voice callers, callback storage and CPU acceptance.

## Forward contract

`Instrument.source_indices.modulators` remains the canonical physical registry. Each Kontakt row now carries `settings: Option<kontakt::Modulation>` alongside its existing original group, slot, external flag, name and optional admitted runtime reference. Synthetic/non-Kontakt rows may have no settings. Original settings are collected before script initialization, muted-group filtering, source admission and target lowering. They survive even when the source or a target cannot execute. They are not an initialized or live parameter mirror.

`Modulation.version` is the serialized assignment version. Its source enum retains every decoded internal AHDSR/Flex/LFO/unknown-chunk identity or external source kind, including RandomBipolar, unassigned and script IDs. Internal flags retain router UI/bypass/retrigger/unknown byte order. Numeric IDs, external source bytes and versioned footer bytes remain opaque. Existing reader bounds and errors remain authoritative; a malformed source never receives a fabricated descriptor.

Targets retain serialized order, physical owning module slots, magnitude, signed-depth flag, invert, lag in milliseconds, names and unknown fields. `signed_intensity` reads the independent sign bit; it does not admit a destination law. Both table and graphical shapers retain their enable flag and exact values, including disabled shapers and segment curvature. Duplicate names do not collapse rows or targets. Dense runtime modulator/processor indices must never replace these physical source identities.

LFO descriptors retain original version/waveform, serialized object flag, all four initial values, both packed records, both flags, multi weights and version-73 extra flag. Packed fields cross sync-record boundaries. Keep count/note-value and delay records intact instead of recovering them from an IR Hertz/beat clock. Forward decoding does not broaden the strict admitted source clock in `v1_voice_controls::ControlPlan` or the measured Digital Multi subset in `484baae4`.

Flex descriptors retain delta milliseconds, linear level, serialized curve, sustain index, unknown index and opaque timing/mode tail. A generic IR breakpoint envelope cannot recover all these original f32 values and metadata. Unknown loop/one-shot state still needs an admission decision.

## Pinned v1 conversion and admission

The reference is our `0cb7a8a0:src/modulation.rs::read_group_impl` and `src/engine/params.rs::{Mod,ModTable}`: external assignments flatten in physical source-slot order and original target order, including unmodeled entries. W9 should preserve that flattened ordinal when constructing `ModTable`, using `route: None` for retained but unexecuted assignments; do not compress script addresses. More than eight voiced routes must fall back, never truncate. Module destinations retain their physical module slot/parameter and are not automatically ordinary voice targets. Filter consumers own their separate source/lag clocks.

`v1_voice_controls::Flex::from_kontakt(points, sustain)` copies `0cb7a8a0:src/engine/bank.rs::Flex::from`. It preserves f32 division `time_ms / 1000`, computes `bulge = 2 * curve - 1`, and negates bulge only when the next level falls. Equal levels retain the original sign. Input validation rejects empty/oversized points, invalid sustain, nonfinite/negative times and levels/curves outside 0..1. It does not select Flex as the group's amplitude owner or infer its opaque mode switches.

Pinned v1 admits a strict retriggered version-71/type-5 saved sine source subset, not Conflux's type-6 records. It rejects source clocks driven by external module targets. Those restrictions remain explicit: W9 cannot admit every retained descriptor merely because its fields are now available. Existing v2 runtime sources/routes remain the fallback. Native primary-AHDSR wrapper admission continues to use W9's separately proven `SourceAhdsr.native_amplitude`; this raw scalar record does not establish wrapper geometry.

## Initialization and live writes

Saved descriptors are immutable preparation evidence. Apply initialized and live writes through W5's existing addressed engine-parameter service and lawful conversion, keyed by physical group/source slot/target ordinal. Source/target parameter laws and live consumers must agree. Do not overwrite original descriptors, introduce a private overlay, use hashed identities, resynthesize original f32 values from rounded generic curves, or advance amplitude twice. No new bindings or control-lane compiler are introduced here. Full init/live LFO/Flex/target overlay is not implemented by this slice.

## Verification and limits

Receipt directory: `/mnt/Windows11/DEV_WORKSPACE/kontra-runs/w6-original-control-descriptors/`. Shared-target RED/GREEN results are untrusted under the coordinator's BUILD CORRECTION. Only `corrected-*` logs from the restored per-worktree wrapper count as acceptance.

The synthetic fixture retains sparse internal slots 7/12 and external slots 3/31, bypass and unknown flags, synchronized Digital Multi clock records, a two-point Flex, RandomBipolar and CC127, three ordered destinations including unsupported frequency, separate sign/invert flags, a disabled 128-point table and enabled graphical curvature. A truncated array must fail before publishing descriptors. The installed Conflux census checks every decoded physical source/target, including all 182 Digital Multi sources, rather than counting only the admitted 124 zero-delay clocks. The Flex helper tests exact bits, falling/equal-level curves, zero-time points and malformed input boundaries. Existing v1 evaluator tests retain their bit/heap oracles.

Corrected per-worktree acceptance passed: RED exited101 at the missing bypassed/unadmitted settings assertion; GREEN21 Kontakt modulation tests and10 pinned-v1 control tests passed. Installed Conflux retained364 internal sources and3752 external sources (4116 total),5590 ordered targets and182 Digital Multi records, from zero original typed descriptors before this slice. The synthetic fixture additionally covers Flex and RandomBipolar descriptors. Area `--no-run` passed for sampler-ir/core/kontakt/ksp/uvi. Paths in every corrected test executable name the owned `kontakto-w6-v1-voice-controls` target. This is descriptor coverage; admitted audio-source counts remain unchanged.

This metadata/helper slice does not enable whole-voice execution, native live source control, RandomBipolar audio, source-module intensity/frequency, new FX, native host parity, or a timed CPU/RSS/corpus verdict. Descriptors and copies allocate during preparation, not the callback; added preparation memory has not been measured. Broad gates remain W0's batch work.

NEXT: W9 consumes original descriptors; W6 verifies Conflux source-module intensity/frequency, then remaining Digital Multi fades.
