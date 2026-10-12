# Modulation control-schema remap — source ready, not validated

## Custody and decision

- Exclusive checkout: `/home/derpcat/.t3/worktrees/KONTAKTO/modulation-control-remap-fix`, branch `pi/modulation-control-remap-fix`.
- Exact parent: `66eaf072d12387012c3a17c13b4140310f59e985` (combined round6 candidate, version 0.3.403).
- Source/tests commit: **`275494599eb9b3f9758e7c63a99827ea1ef382b0`**.
- Source/tests tree: **`5d356a11294ff601942fc1eca440532db502848a`**.
- Product source changes are only `sampler-core/src/control.rs` and `sampler-core/src/voice_mod.rs`. Tests are confined to the reserved core controls and serialized Kontakt modulation surfaces. No dependency, feature, version, engine law, runtime evaluator, per-voice field or generic framework change.
- This is a successor implementation for the accepted P1 in `handoff/modulation-composed-review-round6.{md,json}`. The original review remains **BLOCKED at66eaf**. It does not approve this successor. Independent successor review and the authorized combined validation are still required.

| Evidence class | Status |
|---|---|
| Source custody, syntax parsing and independent scalar model | PASS_SOURCE_CUSTODY_AND_SCALAR_MODEL_ONLY |
| Rust build/typecheck and the seven new regressions | NOT_RUN |
| Original ten modulation regressions on this successor | NOT_RUN; exact original function bytes preserved |
| Kontakt native runtime parity | UNKNOWN |
| Lower CPU AND RAM than BOTH frozen v1 and Kontakt | UNACHIEVED |

## Root correction and atomicity

`Prepared::with_controls` already owns the complete old sorted schema. It now temporarily retains that schema while validating the replacement. Each sparse compiled modulation input resolves **old index → old ControlId → new sorted index**. Invalid old indices, removed IDs and non-real replacement cells return `Error::InvalidInput`; they do not redirect a cell, mask an error or publish getter-only success.

At codeSHA above:

- `crates/sampler-core/src/control.rs:179–234`, `Prepared::with_controls`: sorted duplicate/default/domain validation; existing program/DSP/callback validation; engine/envelope real-domain validation; existing script-resource integer IDs, registry descriptors and widget control IDs checked; sparse input remap.
- `crates/sampler-core/src/voice_mod.rs:372–388`, `VoiceModulation::remap_controls`: validate every sparse input before any compiled input changes, then replace those indices. Resolver reads only the immutable old/new schemas.
- `voice_mod.rs:433–630`, `VoiceModulation::new_resolved`: original preparation resolver and sparse metadata layout, unchanged.
- `voice_mod.rs:1250–1383`, **VoiceModState's route evaluator**, unchanged. This is not `Shape::evaluate`. Its actual depth/bypass cells remain direct prepared accesses.

Both builders consume an exclusively owned `Prepared`. `VoiceModulation.programs`, `Program.controls` and other program payloads are owned boxed arrays at this pin, not shared `Arc` programs; `Prepared` has no clone implementation. No copy-on-write machinery or new retained identities are needed. The old schema is a preparation-local owner, not a global mirror. A rejected builder returns no candidate; it cannot mutate an already active generation or another plan. Remap validation precedes mutation of program indices.

Replacement also preserves the existing consumer kind checks: real engine/envelope lanes and integer script-resource lanes cannot be converted to unrelated cell types. Generic DSP controls retain their existing normalization of real/integer/toggle domains. Definition/default bounds, duplicate IDs and public control-write validation remain intact.

## Complete caller and consumer census

`source-manifest.json` pins all44 `.with_controls(` call sites at codeSHA, including bounded surrounding source-line digests and whole-file digests. The actual production composition callers are:

1. `sampler-core/src/lower.rs:435`: initial authored schema; compiled sparse inputs do not yet exist.
2. `sampler-core/src/engine_parameters.rs:387`: append group amplitude-envelope lanes; earlier sparse inputs now remap if present.
3. `sampler-ksp/src/lib.rs:614–794`, `bind_modules`, especially copying/appending controls and the final schema replacement: the real scripts-enabled composition which exposed P1.

Other matches are core/root/KSP/MIDI unit or integration fixtures and the render-workloads example. Root `src/plugin/effect_controls.rs` is a test fixture caller, not a second production schema binder. Public Kontakt `read_with_controls` has a different purpose and is not this core schema-replacement API.

Actual public Kontakt path: `sampler-kontakt/src/load.rs:915–1023`, `prepare_inner` → `sampler-core/src/lower.rs:715–725`, modulation preparation before behavior binding → `sampler-ksp::bind_modules` → corrected `Prepared::with_controls` → existing voice route evaluator → PCM. No setter/getter mirror is introduced. `engine_parameters.rs:686–760,761–813` retains actual identity-based setter/getter paths.

Other control-dependent consumers were checked, not blindly remapped:

- LFO rate/phase/delay/fade and generic envelope sources are prepared values; they do not store control-schema indices.
- Amplitude envelope stage lanes retain `ControlId` (`prepare.rs:445`; `engine_parameters.rs:325–390`; `control.rs` onset resolution). They resolve against the current schema, not stale numeric cells.
- `dsp/control.rs`'s `PreparedParameter::Control(usize)` and `voice_mod.rs`'s `CompiledTarget::Control(usize)` index **DSP parameter/ramp lanes**, not the control schema. `prepare.rs:774–902,1483–1516` establishes these lanes. Remapping these as schema cells would create another bug. `project_parameters` is byte-identical.
- DSP/bus `ControlRange` bindings keep IDs; `dsp/control.rs:225–293` validates them and resolves defaults/edits by identity. Per-runtime `engine_index` is constructed from the final schema.
- Programs/callbacks, script-resource controls, widgets and registry descriptors store IDs. Replacement now validates those existing IDs/kinds where applicable. Automation addresses widgets by source slot/UI ID, not schema positions.

The original repository's graft graph had no `with_controls` symbol/hits/callers; the fresh checkout has no tracked graft graph. The mandated graft query/callers attempt preceded exact source-search fallback. No graph build/install was performed.

## Seven new regression sources — all NOT_RUN

Complete per-test package/target/path/line/body digests and witness descriptions are in `source-manifest.json`.

### sampler-core lib

- `control::tests::schema_replacement_rejects_invalid_compiled_input_index` (`control.rs:574–613`): private malformed old cell9 with a two-cell old schema rejects `InvalidInput`. Public preparation cannot construct this invalid index.

### sampler-core test target `controls`

- `modulation_inputs_follow_identity_after_reorder_append_and_real_default_replacement` (`1098–1158`): reorder, integer/toggle insertion, changed valid real default, repeated replacement; audible pan/depth/bypass changes, unchanged sentinels and voice ownership.
- `modulation_schema_rejects_removed_and_non_real_inputs_without_touching_active_plan` (`1161–1207`): remove either live ID or replace it with integer/toggle; reject while an independently owned active plan remains audible with unchanged revision/identity/count.
- `schema_replacement_preserves_native_envelope_and_dsp_identity_consumers` (`1210–1302`): own DAHDSR stage-control schema, removal/kind rejection, inserted control, typed sustain write consumed at onset and a ControlGain/pan input sharing the same stable ID. “Native envelope” here names KONTRA's own envelope API, not an executed Kontakt reference.

### sampler-kontakt test target `production_modulation`

- `serialized_scripts_enabled_schema_shift_preserves_depth_bypass_pcm_and_voice_owners` (`631–685`): own serialized NKS+WAV, public load with scripts enabled, real KSP init/UI binder; non-primary retriggered AHDSR amplitude/pan amount/bypass changes with positive PCM, unchanged sibling, integer UI cell, overlapping voice/release ownership and final silence.
- `serialized_scripts_enabled_live_writes_match_unbound_pcm_across_partitions` (`688–735`): the exact independent review trigger **`$Real`**, derived ID **`0x6eee5e4ce8303263a90ab0ecbf6aea3d`**, init value17. Requires strict interleaving between original IDs and slot12 target1 index5→6. Public binder must keep all original definitions. Positive initial `[0.125,0.5]`, typed main-source depth/bypass mutations and exact PCM equality with the **same serialized NKS, scripts disabled**, under64/7/11-frame and empty partitions. UI sentinel changes must not mask the sibling.
- `serialized_scripts_enabled_schema_shift_keeps_pitch_step_and_sibling_pan` (`738–791`): same real script-enabled fixture and own linear-ramp WAV; six-semitone versus bypassed/zero-depth cursor steps, positive PCM, unchanged sibling pan and voice/cursor continuity.

`fixture_target_script:114–243` adds an independently authored enabled chunk6 KSP object without changing the existing no-script bytes. `scripted_plan:293–366` asserts actual script/interface/widget binding, the deterministic derived ID, preserved definitions and the actual sorted index shift. Host limits use existing `Limits::for_plan` for scripts-enabled plans. Rendering and selected writes/releases retain the existing heap-allocation/free guard. Those guards have not executed.

The original ten `serialized_*` test function full-line digests match66eaf exactly. The original no-script load helper and explicit limits remain intact. Assertions are real PCM/cursor-step/voice witnesses, not getter-only proof; their outcomes remain predictions until Rust execution.

## Authoritative native-equivalent requirements, not parity

REA `current_document` was directly discovered/called through code mode. It returned **target_unavailable: no app open**, saved in `rea-current-document.json`. No native/official reader/host/Wine process was opened. Immutable cached public NI archives were independently rehashed and their exact requirements inspected. This is the allowed requirements equivalent, not a native execution pass.

| Archive | SHA256 | Precise requirement |
|---|---|---|
| Kontakt modulation | `ca330e3207a333061ebca4ee28b0545b5e93c223624ff69ca608c7a85d897c9d` | AHDSR retrigger and source controls |
| KSP engine parameters | `4a15e3b10a9da8993c90ddc7681fee9fd62a7eafc5f3685f47b0c5ab37d1d16d` | positive MOD_TARGET_INTENSITY; separate bipolar MP/Invert; internal-modulator bypass/retrigger |
| KSP engine commands | `df2fe7192f32df7c4a738c713663bafdfec3630c2821fd9afe1887c7d5ed006d` | physical group/modulator/target addresses, set_engine_par consumer and continuous/switch ranges |
| KSP UI commands | `ad269b9e26a32e9f1a8aea9b00d8e199af43551adb3b47cf71b93a7234e09d5e` | get_ui_id widget identity; performance-view controls available to KSP |
| KSP UI controls | `c736e2f0a27e8131a8cf8fa2669a8f1b2ad26a27dadcbc916c20ea00f4ed518d` | ui_slider performance view and declared min/max |

Exact primary URLs/sections:

- https://docs.native-instruments.com/ni-tech-manuals/kontakt-manual/en/modulation#ahdsr
- https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/engine-parameters#modulation-102089
- https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/engine-parameter-commands#set_engine_par--
- https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/engine-parameter-commands#get_mod_idx--
- https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/engine-parameter-commands#get_target_idx--
- https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/user-interface-commands#get_ui_id--
- https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/user-interface-commands#load_performance_view--
- https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/user-interface-controls#ui_slider

`source-manifest.json` retains archived sections, full excerpts, provenance, URLs and byte hashes. The older section index lacked standalone get_mod_idx/get_target_idx records; the audit extracts the exact sections from the same rehashed full engine-command HTML. The first audit attempt stopped on that index gap, then succeeded with the full-archive extraction. No archive or product source was changed to satisfy the check.

NI UI IDs start at32768 and follow declarations; **KONTRA's separate hashed ControlId, schema sort order and old→new remap are our own contracts**. Do not call the FNV hash native UI identity. Official docs require addressing the correct modulator/target and the actual consumer; they do not establish native sample-boundary, coefficients, bypass pause/resume or overlap equality. No isolated v1/AR/DFD receipt is used as modulation parity proof.

## Source checks, performance bounds and limits

Run performed: `python3 docs/audit-2026-10-10/modulation-control-remap-fix/check_source.py`. Result: four exact changed Rust blobs,26 exact consumer spans,seven new test manifests,ten unchanged original test bodies,44 caller spans and five reference archives verified; `git diff --check` and four direct rustfmt syntax parses succeeded. Independent scalar remap/missing/kind/invalid-index examples, `$Real` hash and stated PCM arithmetic passed. **No Rust typecheck/runtime result follows from these checks.**

Schema work is off audio: existing sort O(C log C), sparse remap O(B log C), where C is schema cells and B is sparse bound routes; two passes validate then remap. The old boxed schema is retained transiently until preparation returns. No new per-program identity payload or per-voice storage exists. `Program`, `VoiceModulation`, `ModShape`, `VoiceModState`, the actual route evaluator, `project_parameters` and `bytes_per_voice` bodies are unchanged byte-for-byte. This is source evidence only, not an ABI/layout/allocator/RSS/CPU measurement. Earlier saved-bypass admission may still increase maxima and allocated voice state.

Shared `engine_parameters.rs` is byte-identical to66eaf; composed Switch and UVI exact endpoint laws are preserved. Frozen reference/v1, old bac3 RED and3665 GREEN receipts, validated429e/f4f69ade and all other worktrees remain untouched. No Cargo/rustc/clippy/build/typecheck, runtime test, native host/reader, generic gate/probe, install/publication, schedule, nested agent or dev server was started. No CARGO_TARGET_DIR or RUSTC_WRAPPER was set/exported.

## NEXT — sole coordinator/integration ownership

1. Independently review this exact codeSHA; do not promote source existence or the original66eaf review into approval.
2. Source-compose the ordered code+audit commits into the next candidate. Keep both earlier RED corrections and the original ten fixtures.
3. Only after coordinator authorization, the sole integration owner performs one combined seven-package build/direct-test cycle. Include core lib malformed-index regression, core `controls`, Kontakt `production_modulation` (all13), KSP `mod_values`/typed-widget/script-state neighbors, NCKP resources/UI, root controls/effect/persistence neighbors, core lower/voice_mod/envelope/ownership/multicore/paged/selection/buses/control_dsp/svf and IR source-index/validation neighbors, plus the existing UVI law/own host/PCM regressions. No extra matrix or generic UVI gate/reader fallback.
4. Record compiled/test/artifact custody at the new candidate SHA and only then decide P1 closure. Lower CPU AND RAM than BOTH references needs comparable measurements; it is not achieved here.

Skipped/not checked: Rust execution, independent successor approval, native numerical parity and comparative performance. Risk: this uncompiled successor can still contain a type or expectation error; the blocked66eaf defect is not retired until independent review and the combined scripts-enabled gate pass.
