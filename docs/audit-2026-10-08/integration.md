# Integration and delivery audit — 2026-10-08

Audited source: `integrate/core-v2@7e82b152b46b31e8b9fd85c2ac01d01dad669ddd`.
Research branch: `audit/integration-20261008`. This report changes no implementation.
The branch inventory below pins the fetched refs, rather than following branches as other agents push.
Read-only references: v1 `0cb7a8a0b4d43086596a64c77320caa1b26d6d98`, prior GPT reports, the RE worktrees, GitHub runs/releases, and installed library metadata.
`/mnt/MAIN_STORAGE` was mounted. Every local Cargo job ran sequentially through `~/.cache/kontakto-heavy`; no reference recorder, Wine configuration, other worktree, integration branch, or GitHub setting was modified.

## Verdict

**Worse in integrated product completeness than v1; build/delivery execution is healthy; better separation of the plugin shell.** This is a source-and-delivery verdict, not a measured CPU, RAM, load-time, or sound-quality comparison.

The v1 shell had 101 Rust files / 102,425 lines under `src`; v2 has 45 / 33,750 there, a 67.0% reduction. `src/plugin.rs` fell from 8,818 to 1,923 lines, 78.2%. Across first-party `src` plus `crates`, however, v2 has 263 Rust files / 136,675 lines: 33.4% more than v1. These counts include tests and comments; moving code into crates is useful organization, not evidence of less total complexity or faster audio.

At the exact audited head, the root `cargo test --no-run` passed first. Workspace package shards subsequently passed **1,055 tests, zero failures, 45 ignored**, including doctests. Four additional native-host/X11 checks passed; two of those rerun tests counted as ignored in the default suite. Thus 1,059 successful test executions, with 43 default-ignored tests still unexecuted. Clippy exited zero but emitted warnings. The latest published nightly for main `30953f9c3c55` succeeded on all delivery platforms and shipped CLAP, VST3 and standalone. Those facts do not establish native-host fidelity: the default suite explicitly ignores a test documenting 5–11 dB deviations, and several real-library tests can return successfully when fixtures are absent.

The integration head contains the committed tips of `feat/decipher-readers-v2` and `feat/inspect-daw-compatibility`, **but not their uncommitted work**. The complete public Program reader and FX diagnostic fixes are available in 14 dirty files; 11 supporting RE documents are untracked in the other worktree. Treat them as pending delivery, not as work to rediscover.

## Ranked findings

### 1. P0 — Library control edits are absent from saved plugin/multi state

**Evidence:** `src/plugin.rs:37` defines persisted `Part` without script/control snapshots; `SavedMulti` at `src/plugin.rs:981` contains parts using that same schema. The v1 `Part` at `0cb7a8a0:src/plugin.rs:46` had `script_state`. Current reload identity at `src/plugin.rs:1114` also omits edited library control state. The prior Kontakt UI report documents a repair with callback and menu-recall tests.

**Root cause:** loading the source patch and saving the rack are separate flows; live script values never become host-persisted state. Same-source recall needs an explicit restoration path, too.

**Fix, M:** land `origin/v2/gpt-kontakt-ui@0be9ed3f174d8925f1d8a142d3f709e5e26b840a` for scalar capture/recall, then `origin/v2/gpt-uvi-ui@24c70a25031243d002e24c661c1afbb1408653aa` for UVI worker/state integration. Reconcile `src/plugin.rs`, `src/sound/{mod,v2}.rs`, `src/ui` and frontend state APIs together. Test edited session, multi, and same-source recall. Native binary persistence decoding from `gpt-decipher-persist` is complementary; a decoder alone does not connect host state to either script runtime.

### 2. P0 — Falcon/UVI UI is attached as a static view without a live control path

**Evidence:** `src/sound/v2.rs:1095` attaches Lua and pushes its interface, then returns `controls: Vec::new()` at line 1120. `crates/sampler-uvi/src/scripted/thread.rs:25` only accepts note-on, note-off and host-input messages; the interface is returned once during initialization at line 87. `src/ui/pictures.rs:10` wraps only Kontakt resources; line 19 rejects non-image assets, including bitmap fonts. The prior UVI UI report found 239/239 interactive controls unbound in its partial baseline census; that is historical evidence, not a fresh whole-corpus result.

**Root cause:** audio scripting was integrated without widget-to-worker messages, changing interface snapshots, bank asset/font resolution, or saved worker state.

**Fix, M:** land `origin/v2/gpt-uvi-ui@24c70a25` after the Kontakt state work. It already implements widget bindings, bank assets/fonts, worker snapshots and mapper/table/XY/menu fixes. Validate a real UFS program's control callback, audible effect, artwork/font, reopening and state recall in the actual plugin; do not accept a screenshot alone as proof.

### 3. P0 — “Every library UI” and Conflux parity are not delivered

**Evidence:** `src/ui/ir_view.rs:347` supplies a zero meter, line 348 draws a table as fixed one-pixel bars, and lines 356–359 show XY/waveform/wavetable/file-selector/text-edit as labels and mouse areas as empty blocks. This violates `PRODUCT.md:64`. The Kontakt UI branch detects unsupported Komplete UI and improves reports; it does not implement that frontend. The old `origin/codex/conflux-native-ui@4da706400d69a41d31cc0cec167144f8b6a5bdd0` contains reference work but has no merge base with this history.

**Root cause:** IR vocabulary and fallback rendering were counted as frontend support before interactive/rendering/asset consumers existed. Conflux's native frontend cannot be substituted with stock KSP scalar controls.

**Fix, L:** use the RE native-UI specifications and old authored implementation as references, then port the needed frontend into `sampler-ui-ir`, the Kontakt loader/resources and `src/ui/{ir_view,pictures,inside}.rs`. Land the current Kontakt UI reporting branch first so unavailable frontends are explicit. Measure Conflux in original/vector modes against the installed v1 and Kontakt; this audit did not measure its latency or reproduce the Original-toggle behavior in a running DAW.

### 4. P0 — Recognized KSP host services can silently have no plugin consumer

**Evidence:** `src/sound/v2.rs:759` drains runtime effects into the plugin queue. `src/plugin.rs:892` pops them and calls only `ScriptUi::apply`; `src/sound/mod.rs:231` delegates to `apply_ui_effect`, whose service dispatch in `crates/sampler-ksp/src/lib.rs:307` rejects non-UI services. The popped effect is not then sent to a DSP/file/async host dispatcher. Compiler coverage at `crates/sampler-ksp/src/lower.rs:376` reports an approximation, but recognition as a host effect does not itself produce a missing-consumer warning. Core backpressure at `crates/sampler-core/src/ops.rs:540` preserves effects only until the queue accepts them, not after the plugin UI rejects them.

**Root cause:** compiler exposure and delivery into an outbox are mistaken for executed host semantics. Missing host services may be silent, contrary to `PRODUCT.md:17` and `:29`.

**Fix, L:** land `origin/v2/gpt-ksp-audit@a1446ec6be0162371948cef412dc9e9d0383e6fe` and the partial group volume/pan/tune consumer `origin/v2/ksp-runtime-engine-par@22009d1e8c1f616bb3d6b8e3c5d0862a52b874eb`. Add one shared capability/dispatch boundary for remaining engine/FX/file/async families in the core/plugin adapter, with an explicit report when no consumer exists. The audit branch's 663/676 engine-parameter-without-consumer figure and 40 expected failures are its pinned baseline; recount after each consumer lands. Do not merely mark more builtin names as supported.

### 5. P0 — Occupied Kontakt FX slots can disappear without an unsupported record

**Evidence:** `crates/sampler-kontakt/src/effects.rs:86` uses `filter_map`, `try_from(...).ok()?`, and `params().ok()?`; a malformed occupied slot vanishes. Adjacent program/bus rack decoding also conditionally accepts successful reads. The uncommitted RE FX report explicitly addresses this failure and records slot locations.

**Root cause:** an absent slot and an occupied but unreadable slot share the “skip it” outcome. Playback can omit processing while the load report suggests no issue.

**Fix, M:** land the owner's dirty `effects.rs`/`library.rs` diagnostic work and reconcile `origin/v2/gpt-format-fxmod@b7f6af0bf7bd24876418fead74e57a8afa786af6`. Preserve the already-integrated ladder-offset repair while adding retained modulation payloads/snapshot overlays and malformed-slot coordinates. Touch importer/report tests, not every renderer caller. Unknown processing remains explicitly unsupported until implemented.

### 6. P1 — Linux downloads require glibc 2.39

**Evidence:** nightly builds on `ubuntu-24.04` in `.github/workflows/nightly.yml`; `readelf --version-info` on all three binaries extracted from the actual latest Linux release found **maximum required GLIBC_2.39**. This is measured on the delivered artifacts, not inferred only from the runner name. CLAP and VST3 also need `libstdc++`, `libgcc_s`, `libm`, libc and the ELF loader; standalone additionally needs `libasound.so.2`.

**Root cause:** native linkage inherits the runner's glibc baseline; there is no older sysroot/container contract or maximum-GLIBC gate.

**Fix, M:** set a supported baseline, build against that userspace/sysroot, check versioned symbols and smoke-load in the oldest supported environment. Touch nightly Linux build/validation and installation documentation. If 2.39 is the intended minimum, document it (S); Ubuntu 22.04/glibc 2.35 is not covered by this release.

### 7. P1 — Passing CI does not gate the advertised fidelity

**Evidence:** the default `sampler-native` suite ignores `full_notes_match_kontakt_within_a_decibel` with an explicit reason: Una Cotton is 5/11/11 dB low at velocity 64/100/127; Barbarian is 11 dB low; Vista passes. Other ignored cases cover real UI, articulation and streaming/performance surveys. `crates/sampler-kontakt/tests/real_libraries.rs:107` and `crates/sampler-native/tests/articulation_real.rs:110` can return successfully when a local fixture is missing. The unmerged KSP audit keeps 40 adversarial expected failures outside default success. GitHub API returned `protected:false` for both main and integrate/core-v2 and an empty ruleset list.

**Root cause:** synthetic correctness, optional local fixtures, acknowledged parity gaps and release acceptance are separate contracts, while green delivery is used as a broad readiness signal. The repository's aggregation job is not an enforced branch rule.

**Fix, M:** make a small owned-fixture acceptance tier fail explicitly on missing required fixtures; record source/build identity and accepted tolerances, and promote repaired adversarial probes. Reuse `sampler-native`/`corpus-health`, not another harness. Separately enable the existing required-check aggregation through repository rules (S, coordinator action). Keep surveys outside ordinary CI, but publish a dated coverage receipt for the release.

### 8. P1 — The complete Program reader is available but not committed or integrated

**Evidence:** `feat/decipher-readers-v2@2fb8c926dd39bb7ac26a84d4806f42de14b6630e` is an ancestor, but its worktree has **14 modified files, +1,795/−300 lines**. The dirty `vendor/ni-file/src/kontakt/objects/program.rs:153` adds the complete public-record reader and line 399 exposes `Program::public_record`. Supporting docs are untracked on `feat/inspect-daw-compatibility@0cb7a8a0`. Five file patches do not apply directly to the audited head; see the complete list below.

**Root cause:** branch-tip ancestry cannot deliver working-directory changes. A branch inventory that only looks at refs falsely labels this work complete or missing.

**Fix, M:** its owner commits and pushes the reader/schema work and FX fix in reviewable commits; the coordinator cherry-picks those new commits into a dedicated integration branch. Reconcile Program/resource prefixes with `gpt-format-gaps`, authored field preservation with `gpt-format-objects`, and FX reports with `gpt-format-fxmod`. The documented public grammar spans versions `0x80`, `0x82`, `0x90–0x92`, `0xa0–0xb5`; it is not a private-state reader, semantic importer, or writer by itself. See the dirty-work landing section for exact files and docs.

### 9. P1 — Structured runtime/load evidence is incomplete in the persistent log

**Evidence:** `src/plugin.rs:1168` logs flattened `report.lines()` plus decoded counts; line 1184 retains the typed report for UI. `refresh_problems` at line 1224 updates that in-memory report but emits no corresponding journal entry. A script fault is reconstructed from a 12-value error index at line 1235 rather than preserving detailed origin/context. This falls short of `PRODUCT.md:27–35`'s typed load and runtime report written to a file.

**Root cause:** the journal captures loader prose, while runtime counters/faults are a UI snapshot. Payload-less realtime error transport drops useful worker context.

**Fix, M:** serialize the safe typed load report through the existing diagnostic detail API; emit bounded counter deltas/fault records off the audio thread, with fixed-size script/location context from the runtime. Touch `src/plugin.rs`, `src/sound/{report,v2}.rs`, `sampler-core` fault transport and diagnostics. No strings or logging in the audio callback; no key material or decrypted contents.

### 10. P1 — Mixer-node inserts and sends are metadata, not editable node services

**Evidence:** `src/sound/tree.rs:28` describes inserts/sends, but `NodeMix` at line 92 contains gain/pan/mute/solo/output only. `src/ui/bridge.rs:29` carries insert names and `src/ui/mix_tree.rs:221` renders an insert count. Existing static source DSP and the part-level aux send do not provide editable inserts/sends on every nested node as required by `PRODUCT.md:54`.

**Root cause:** source-tree display and live mixer mutation were integrated to different depths.

**Fix, L:** extend the existing node state and shared core mutation path with typed insert/send identifiers and persist those changes, then expose editors through `src/ui/mix_tree.rs`/bridge. Reuse current graph routing/DSP instead of another mixer engine. Validate nested source bus routing and session recall. No unmerged branch examined establishes completion of this whole contract.

### Additional P1/P2 findings

- **P1, M — MPE is opt-in despite a default-on requirement.** `src/plugin.rs:102` sets `mpe:false`; loader options consume that setting (`src/sound/v2.rs:1057`). Change the default/configuration contract and validate host zone/channel behavior, per-note pitch/pressure/timbre on an instrument not authored for MPE. Support exists; this is a wiring/default gap, not evidence all expression DSP is absent.
- **P1, M/L — “All containers/monoliths” remains false.** `crates/sampler-kontakt/src/container.rs:88` accepts NKS/NIS and rejects FileContainer, with a 128 MiB outer read cap. `origin/v2/gpt-format-legacy@ad12e63d87c2c3bea3a2fc0c86d7b714c576106a` already adds bounded legacy/FileContainer and embedded sample/AIFF work, but its report still excludes XML semantics and NKS monolith completion. Land that work before measuring the residual formats.
- **P1, M — Budget claims need runtime evidence.** `src/plugin.rs:189` defaults memory budget to zero (“keep all”); `trim_streams` at line 915 limits resident streamed heads rather than all process memory. `src/sound/v2.rs:1236` uses an instruction budget of 8,192 per block, not a measured aggregate wall-clock deadline for all parts. Streaming/SIMD implementations are already merged; do not call them missing. Measure with the load/CPU agents before changing limits.
- **P1, M — Integration order matters.** Individually clean branches conflict when accumulated (details below), and compile-compatible merges may still conflict in API/state semantics. Land in an isolated branch, preserve both owners' tests, then run importer/KSP/UI shards before the next layer. Do not blindly reapply old feature forks.
- **P2, S — Architecture coverage docs are stale.** `docs/architecture-v2/CURRENT_STATE.md` still describes the 2026-10-05/v1 paths; `FALCON_MODULE_COVERAGE.md` lists dropped modules that now have implementations at `crates/sampler-uvi/src/inserts.rs:43`. Regenerate against a pinned source head; do not turn stale census counts into release claims.
- **P2, S — Root minimum-Rust metadata understates a shipping dependency.** `Cargo.toml:5` declares 1.92, but the default plugin's `kontra-native-host` dependency declares 1.98 at `vendor/mui-baseview/Cargo.toml:8`. The effective supported compiler is at least 1.98; only 1.99 was tested here. Align the metadata and CI compiler contract rather than promising root 1.92 compatibility.

## Product contract: delivered versus still unwired

All references to PRODUCT below mean `docs/architecture-v2/PRODUCT.md` at the audited head.

| Claim | Current plugin evidence and gap | Available work / next measurement |
|---|---|---|
| No silently wrong libraries, lines 17–18 | FX malformed slots and unconsumed KSP services can disappear | Findings 4–5; land consumer/report fixes and remeasure native probes |
| All Kontakt/Falcon formats, lines 22–23 | Modern Kontakt/UFS loading exists; FileContainer, legacy and public tails are incomplete in this head | Legacy branch + dirty RE reader; XML/NKS monolith/private state remain separate |
| Typed load/runtime report in UI and log, lines 27–35 | Typed UI report exists; log mostly prose/counts; runtime deltas not journaled | Finding 9 |
| Default full MPE, lines 39–40 | Configurable MPE exists, default false | Change/verify default; don't remove script-first behavior |
| Generated articulation remapping, lines 41–42 | `src/ui/inside.rs:140` and core switching modes wire keys/velocity/channel/CC32/program | Generated-map/native behavior test remains ignored; real acceptance still needed |
| Per-node inserts/sends/output, lines 54–56 | Nested tree, gain/pan/mute/solo/output exist; insert/send editing absent | Finding 10; audit depth/source-bus preservation on real patches |
| Every KSP/Falcon UI type, lines 64–66 | Scalar KSP path exists; multiple interactive widgets are placeholders, Lua bindings/assets not connected, Komplete frontend absent | Findings 2–3; UI branches partly repair this |
| Original/vector artwork plus asset release, lines 67–69 | There is a presentation path, but incomplete assets/frontends undermine the contract | UI agent should reproduce toggle and measure retained bitmap memory; no blanket completeness claim |
| Measured callback budget/deadline, lines 13–14, 73–74 | Instruction fuel and block/SIMD code exist; aggregate deadline proof is not a delivery gate | CPU agent's matched-quality real-library callback distribution |
| Global memory budget/purge, lines 75–76 | Streaming/head trimming exists; default unlimited and process-wide budget evidence absent | Load agent's first audible sample/RSS/page residency measurements |

This audit does not independently reproduce knob drag, nested scrolling, articulation row layout, Original-toggle behavior, or relative CPU/load regressions. Those are UI/load/CPU agent scopes. Their absence from this report is not a claim they work.

## Test and build health

Commands ran in `audit-integration`, Rust `1.99.0 (b940084d7)`, LLVM 23.1.1, Linux. The root no-run build preceded all test shards. `cargo test --no-run` is a root-package build, not a workspace-wide compile; the subsequent package shards provide the rest of the workspace coverage. No source changes were applied before testing.

Clippy: `kontakto-heavy cargo clippy --locked --all-targets` with `-p` for the twelve first-party packages (root, core, IR, SIMD, pool, KSP, Kontakt, UVI, MIDI, native, UI IR, perf): **exit 0, 18.97 s including wrapper**. Compiler summaries report root lib 21 warnings (24 in lib-test, 21 duplicates), root integration test 3, core lib 6, MIDI test 1, native targets 3, perf 3; dependencies emit further warnings. Important examples: erased/large diagnostic errors (`src/diagnostics.rs:1049`), complex state tuples (`src/library.rs:849`, `src/ui/mod.rs:368`), and `drop` of a Copy send result (`src/sound/v2.rs:1295`). Clippy and CI currently do not use `-D warnings`. This was default features/all targets, not an exhaustive feature/platform matrix or clippy of every vendored workspace package.

Workflow validation also passed: actionlint through `.github/scripts/check_workflows.sh`; seven Python packaging tests; 24 simulated nightly publication cases from `.github/workflows/check_nightly.py`. The latter uses fake GitHub fixtures and did not publish anything. Both VST3 C++ lifecycle probes (`vendor/moose-vst3/tests/editor_attach.cpp` and `editor_lifetime.cpp`) compiled with `g++ -std=c++17 -Wall -Wextra -Werror -pedantic` and passed through the heavy wrapper; these are additional to the Rust totals. Their receipt is `~/.cache/kontakto-audit-integration/cpp-lifecycle.log`.

Times in the generated table below include wrapper waiting and compilation; they are not performance timings for audio or library loading. All shards were sequential. Cargo's recursive crash-test subprocesses were excluded from double-counting by counting unfiltered summaries only. Excluded vendored dependency crates (`ni-file`, `ncw`, `vello`, etc.) were compiled as dependencies where needed, but their own independent suites were not run.

## Release pipeline and actual shipped artifacts

- CI (`.github/workflows/ci.yml:31`) lists twelve first-party packages explicitly. Linux Ubuntu 24.04 runs clippy/tests and native host/Xvfb checks. Windows 2025 and macOS 15 run all-target/standalone compilation checks, not the Linux-equivalent runtime suite. Rustfmt is advisory (`continue-on-error` at line 113). PR, merge-group, manual and reusable workflows are supported; an ordinary integrate-branch push is not the same enforced gate as nightly's main validation.
- Nightly (`.github/workflows/nightly.yml:7`) triggers on main push/manual, validates the exact source SHA with release validation, and builds Linux x86_64 GNU, Windows x86_64 MSVC, macOS ARM64 and x86_64. `cargo moose --clap --vst3` packages plugins; standalone is built separately. Mac outputs are combined into a universal package with signing/notarization and install/rollback validation before publication.
- Publisher (`.github/workflows/publish_nightly.py`) verifies per-format hashes/provenance, required artifacts/receipts/notices, marks the published nightly `prerelease:false` for GitHub Latest behavior, and retains two complete releases. Per-platform build concurrency cancels superseded builds; publication is serialized without cancellation. These checks validate packaging/provenance, not acoustic fidelity.
- Version ledger: root `Cargo.toml:3`, Cargo.lock and the Moose ledger are checked/updated by `tools/version.py`; `release-fixes.json` has 208 accepted fixes above baseline 0.3.0, producing **0.3.208**. It counts accepted ledger entries, not git commits. Architecture “v2” does not mean package major version 2; it retains the 0.3 product identity. Root declares MSRV 1.92, native-host dependency declares 1.98, CI toolchain is 1.99 (see metadata finding above).

Latest published release examined: [v0.3.208-nightly.20261008.g30953f9c3c55](https://github.com/DerpcatMusic/KONTRA/releases/tag/v0.3.208-nightly.20261008.g30953f9c3c55), built from main `30953f9c3c55136901264721a19023517fe4dcf4`. [Nightly run 37711410591](https://github.com/DerpcatMusic/KONTRA/actions/runs/37711410591) succeeded; created 01:08:31 UTC, release published 01:35:26 UTC. Latest observed integration CI covered parent `eeb948da`, not the exact merge commit audited here; local tests cover `7e82b152`.

| Release asset | Bytes |
|---|---:|
| Linux archive | 50,496,114 |
| Windows archive | 41,784,154 |
| Universal macOS installer | 81,781,644 |
| macOS notarization JSON | 4,767 |
| Release manifest | 65,572 |

Actual Linux artifact inspection:

| Binary | Bytes | Maximum GLIBC requirement | SHA-256 |
|---|---:|---|---|
| KONTRA.clap | 38,815,576 | 2.39 | `c9a136fd77c39784e9172cb250a0131097755fc6aabdaf7b8b5d7bdf696a16ad` |
| KONTRA.so (VST3 bundle binary) | 38,775,384 | 2.39 | `1c29876f366503a1f1b2d91dba0325d5a9fabe3a0c3a555252425bf9e177678d` |
| Standalone | 39,761,912 | 2.39 | `54dd81305f63da7055d19e1421a784df631e00c6d28de1f9534c2d8f528ba775` |

**Formats shipped: CLAP, VST3, standalone. No AU or AAX build/export/package path was found.** Windows/macOS assets were inspected through release metadata/manifest, not locally executed. GitHub reads used `gh` only; no workflow dispatch, repository mutation or release write was performed.

## Code-quality hotspots

The live plugin uses one engine seam: `src/lib.rs:1`, `src/sound/mod.rs:1`, `src/sound/v2.rs`. Deleted v1 `src/engine`, `src/ksp`, `src/fx`, `src/import` paths remain in historical branches, **not as a second live plugin runtime**. `sampler-native` is a reference/test harness, not another shipping audio engine. Format frontends necessarily differ; consolidate truly duplicated decoding helpers only after behavior is measured. Do not delete a frontend simply because there are both Kontakt and UVI loaders.

Largest first-party Rust files, including tests: `src/support/crash.rs` 4,995; KSP lower 3,018; `src/diagnostics.rs` 2,386; UI tests 2,246; `src/sound/v2.rs` 2,244; UVI lib 2,217; core behavior 2,193; plugin 1,923; Kontakt library 1,880; core behavior tests 1,754; resample tests 1,646; UVI modulation 1,620; UVI script 1,615; core lower 1,586; core lib 1,581; Kontakt effects 1,571. Crash tests begin at line 2,898, so about 42% of that file is tests; v2 sound tests begin at line 1,329. Size alone is not an instruction to build another abstraction. Split by existing responsibilities only when changing the relevant behavior (P2, S/M).

| Pattern | Concrete evidence | Smallest useful action |
|---|---|---|
| Anonymous state tuples | `src/plugin.rs:402` Ready `(slot,generation,optional part)`; reload identity at line 1114; `src/sound/v2.rs:994` Plan `(Prepared,optional cache,optional script driver)` | P2/S: name boundary records when modifying these flows; keep simple local pairs |
| Boolean semantic switches | core note return at `crates/sampler-core/src/lib.rs:1243`; linked-release argument near line 1036; Part has multiple independent state booleans | P2/S: use a named state/mode at ambiguous public boundaries; independent user toggles are ordinary booleans |
| Payload-less errors | 12-variant Copy `sampler-core::Error` at lib line 235; loader maps to debug text at `src/sound/v2.rs:1198`; plugin fault index at line 1235 | P1/M: preserve bounded origin/context separately; retain allocation-free realtime error codes |
| Method-overload families | core `note_on` 967, `note_on_with_expression` 973, `note_on_pitched` 984, `note_on_pitched_in` 1001, child variants 1031/1048; behavior start wrappers 771 onward | P2/S: thin wrappers share validation; don't rewrite them just to reduce names. Use one request record if additional variants make call sites ambiguous |
| Dead-code suppression / stale comments | `src/ui/mod.rs:29` says trees/reports are not produced, but bridge/mixer/report paths are live; module-wide allows at lines 31/34 | P2/S: correct comment and narrow allows, not delete working mixer/report views |
| Truly unused minor UI glyphs | `src/ui/theme.rs:287` Play, lines 299/302 MidiOut/AudioIn only retained in glyph match arms | P2/S: remove unused variants/arms after exhaustive callers check |
| Broad metadata allowance | `crates/sampler-uvi/src/ufs.rs:4` allows dead code for the whole reader | P2/S: narrow to intentionally preserved metadata fields; avoid accidental helper accumulation |

No dependency replacement or speculative trait deletion is justified by this audit. The biggest quality gain is finishing shared service/state consumers and retiring stale merge candidates, not gratuitous rewrites.

## Recommended landing order

Do this on a new integration worktree/branch; the coordinator alone updates `integrate/core-v2`. No merges were made in this audit. Merge-tree simulations only wrote temporary git objects, not refs or source files.

1. **Evidence and acceptance:** `gpt-decipher-dsp@aa3430d6`, `gpt-ksp-audit@a1446ec6`, `gpt-uvi-audit@fb86ac57`. These establish tests/specs, not kernel/script fixes. Update receipts to the final combined head.
2. **Format foundation:** `gpt-format-gaps@56ca1e44`, then `gpt-format-objects@c8e2fbce`. Resolve the accumulated `crates/sampler-kontakt/src/lib.rs` conflict while retaining both resource metadata and authored object fields.
3. **Complete Program/NIS public reader:** have the RE owner first commit the 14 dirty files, preferably separate public-reader/schema and FX-diagnostic commits. Land the reader/schema commit here; reconcile Program/header/voice-groups with the two format branches. Do not cherry-pick only a huge Program file and omit its dependent schemas.
4. **Legacy and FX:** `gpt-format-legacy@ad12e63d`, `gpt-format-fxmod@b7f6af0b` plus the RE FX commit. Resolve `effects.rs`/`library.rs` together; keep malformed-slot reports, existing ladder repair, retained modulation and snapshot overlay semantics.
5. **Saved source records:** `gpt-decipher-persist@6ae15820`. Keep allocation-free reader boundaries and connect actual plugin snapshots through step 7.
6. **KSP runtime:** `ksp-runtime-engine-par@22009d1e` after the audit branch. `crates/sampler-ksp/tests/audit.rs` conflicts in the accumulated simulation: promote newly passing probes and retain the remaining gap inventory.
7. **Plugin/UI state:** `gpt-kontakt-ui@0be9ed3f`, then `gpt-uvi-ui@24c70a25`. Resolve Kontakt `resources.rs` against format resource work and reconcile control identifiers, same-source recall and persisted state across both plugin changes. The UVI branch was textually clean in the partial simulation because the conflicting Kontakt branch was skipped; this is not proof the full UI stack is conflict-free.
8. **Focused UVI playback repair:** `fix/uvi-programs-play@082a1053` (covered key selection plus release tails). Do not also merge duplicate `fix/uvi-restore-order@0d906937`; its corpus-health conflict adds no second playback fix.
9. **Reference receipts and authored specs:** review/land `kontakt-reference@7d15e5b8` and owner-committed RE docs from `feat/inspect-daw-compatibility`. Preserve safe aggregate evidence and provenance; don't copy raw proprietary/decompiled payloads.
10. **Selective library-access ports:** review the 22 not-yet-ancestor commits on `v2/library-access@e46b2f39`, especially lossless format replacement `c5b0bf8a`, retained player Content `cfadb2ee`, clear NKR readers `3d4ab7dc`/`ed77fc15`, and census reuse `88dceb`. Some earlier work is already integrated. Do not replace current `bank.rs` wholesale: it has a measured textual conflict and semantic ownership/streaming changes.

Between layers run the directly affected importer/KSP/state/UI shards through the heavy wrapper, then the full acceptance gate at the final combined head. A successful merge-tree result proves only textual compatibility. Old forks `block-voice-dsp`, `midi-fixes`, `semantic-ir` are not in this order: their tip patches are already represented/equivalent; selectively port a demonstrated remaining behavior instead of restoring hundreds of divergent commits. The full conflict inventory below is authoritative for the pinned base; conflicts after resolutions require remeasurement.

Partial accumulated merge-tree simulation: DSP/audits/gaps clean; objects conflicts in Kontakt lib; legacy clean; FX conflicts in effects/library; persistence clean; runtime conflicts in KSP audit tests; Kontakt UI conflicts in resources; UVI UI/reference clean **with earlier conflict branches skipped**. This is a dependency warning, not a claim that a complete combined build was tested.

## Uncommitted RE work: exact inventory and landing

`/home/derpcat/.t3/worktrees/KONTAKTO/decipher-readers-v2`, `feat/decipher-readers-v2@2fb8c926dd39bb7ac26a84d4806f42de14b6630e`: the committed head is already merged. All 14 modified paths are listed in the generated appendix. Five fail a read-only `git apply --check` against this audit worktree: effects, library, header, Program, voice-groups. Nine apply textually. This check used the owner's working-tree diffs without copying them into the source tree; it is not a substitute for a three-way cherry-pick once the owner commits.

The owner reports 15 focused reader tests, 32 compatibility tests and 30 importer tests passing, plus 25 fully consumed public Program records including four Kontakt 8 `0xb5` records. Those are **owner-reported results on dirty work, not tests rerun by this audit**. Reviewed Program source hash: `b95ebc29885b57483db5bb655f00f500631e5bfc51e075841bc53d468003ab84`. Public-record decoding is separate from preserved prefix APIs (`Program::params`), private data, importing every field into semantics, and writing/roundtripping a preset.

`/home/derpcat/.t3/worktrees/KONTAKTO/t3code-80fe786b`, `feat/inspect-daw-compatibility@0cb7a8a0b4d43086596a64c77320caa1b26d6d98`: committed v1 head is already in ancestry. Eleven documents are untracked:

- `docs/DSP_FORMAT_SPECIFICATION.md`
- `docs/DSP_SYSTEM_INVENTORY.md`
- `docs/NI_FILE_BINARY_RECORDS.md`
- `docs/NI_FILE_COVERAGE_AUDIT.md`
- `docs/NI_FILE_FX_DECODE_REPORTING.md`
- `docs/NI_FILE_KONTAKT_READERS.md`
- `docs/NI_FILE_NIS_READERS.md`
- `docs/NI_FILE_PROGRAM_PUBLIC_READER.md`
- `docs/NI_FILE_PROGRAM_PUBLIC_TAILS.md`
- `docs/NI_FILE_VERSIONED_CORPUS.md`
- `docs/PLUGIN_VERSION_ARCHITECTURE.md`

The brief also references tracked `ALTERNATING_LOOPS`, `UI_NATIVE_PRESERVATION`, `RUST_CRATE_PARITY_AUDIT`, `FALCON_FORMAT_GROUNDWORK`, `FALCON_RUNTIME_UI_GROUNDWORK`, and dated artifact directories. A metadata-only inventory found 143 relevant paths; tracked branch ancestry and ignored artifacts are not evidence that the current untracked specs are delivered. The RE owner should commit authored specs and safe aggregate receipts with their dependencies on a research branch, then the coordinator selectively lands those commits alongside the reader. Don't whole-merge the old v1 checkout or copy raw disassembly/decompiled/proprietary content. Neither worktree was modified.

Local `v2/load-speed@fb456b41` exists, but **no `origin/v2/load-speed` ref was present**. Its read-only `inv-load-speed` worktree contains an uncommitted 51-line plugin load probe. It times metadata/ready publication, not an audible sample, so it must be committed by its owner and its milestone clarified before comparing “first playable” times. It is outside the requested prefix inventory but included because the brief names it.

## Unknowns and how to settle them

- Relative load/RSS/CPU/frame-time claims: replay the same owned instruments, quality, sample rate, 64-frame block size, polyphony, source storage and script/control state through v1/v2/native hosts. Record first audible frame, peak/steady RSS, resident stream heads and callback p50/p95/p99/max; cold/warm runs separately. The load/CPU agents own this, not these build-wall times.
- Combined branch behavior: resolve the proposed stack in an isolated integration branch, pin the resulting SHA, and repeat relevant tests/acceptance. This audit deliberately has no combined implementation merge or speculative compatibility claim.
- Full UI/native frontend behavior: UI agent runs Conflux and representative KSP/Lua interfaces with real callback/audio/state tests and frame timings. Prior censuses need their source/binary identities and corpus denominators carried forward.
- Full public-reader semantic coverage: after owner commit, run ni-file focused tests plus importer real-version shards, requiring complete consumption and preserving unknown records. Then separately prove semantic use/private-tail support; parsed bytes are not played behavior.
- Old local branch content: 305 unrelated-history refs below have no normal merge path. Their tip subject records intent; it does not prove the fix is absent from the current engine. Port only after a current failing repro and patch-equivalence/source review. Do not spend integration time merging archived v1/rewrite lineages wholesale.
- Release platform behavior: Linux checks passed locally; Windows/macOS runtime/DAW behavior was not executed here. Add installer/host smoke receipts on actual supported systems; select and test the Linux glibc minimum.

## Exhaustive pinned branch inventory and test appendices

Inventory method: `git for-each-ref` for every `origin/v2/*` and every local/remote `codex/*`, `feat/*`, `fix/*`; ancestry via `git merge-base --is-ancestor`; individual conflicts via `git merge-tree --write-tree --name-only BASE TIP`. No `--allow-unrelated-histories` experiment. Exact local mirrors of remote refs are listed separately with identical SHAs. There are 370 matching refs represented by 363 distinct table rows plus seven mirrors: 25 merged rows, 33 unmerged rows with shared history, 305 unrelated-history rows. “Merged” means ancestor, not that dirty work or every historic feature survives the rewrite. “Obsolete” means unsuitable for whole-branch landing; it does not dismiss its reference evidence.

Generated tables below preserve every pinned ref and every measured conflict path. For unrelated history the normal merge is refused, so there is no honest conflict-free merge prediction. Individual no-conflict results do not predict the accumulated stack described above.

### A. Every origin/v2 and remote codex/feat/fix branch

| Ref @ full SHA | State / landing decision | Individual conflicts |
| --- | --- | --- |
| `origin/codex/conflux-native-ui@4da706400d69a41d31cc0cec167144f8b6a5bdd0` | unrelated history: valuable authored native-UI reference, port-only; obsolete whole merge | normal merge refused: no merge base; forced add/add conflicts not measured |
| `origin/codex/uvi-latest-integration@4bffbb18b867b0a8e84b435d11684784b237693c` | unrelated history: old UVI integration/reference; port-only after current failing repro | normal merge refused: no merge base; forced add/add conflicts not measured |
| `origin/feat/universal-macos-installer@8ee22069adc8c61f7b8c32920dcdef7fa4ea92a4` | obsolete merge candidate: patch-equivalent universal package pipeline already present | C3 (11 paths) |
| `origin/feat/versioned-diagnostics@da882b2620cc990ebc2cebf7a7b417c22ed73230` | merged ancestor; no new branch merge needed | none (already ancestor) |
| `origin/fix/0.3-pointer-popup-eq@940addcf2be0745841da351455c4625de3e79d06` | obsolete merge candidate: patch-equivalent old v1 fix, removed engine paths | C4 (7 paths) |
| `origin/fix/absent-nightly-refs@006ff01ce5cba89f998fbc8a078ca3631c41c450` | obsolete merge candidate: patch-equivalent publisher guard already present | C5 (1 paths) |
| `origin/fix/analog-zero-output-gain@779c529101c5335d9a59e09c0bad437b417c6268` | merged ancestor; no new branch merge needed | none (already ancestor) |
| `origin/fix/idempotent-macos-certificates@1e2ccf3901e006fda7c9f55ade91674997a81bef` | obsolete merge candidate: patch-equivalent signing behavior already present | C6 (2 paths) |
| `origin/fix/latest-download-policy@dc3a0bafd3c65a7a2644bdb78f8ecb79d3f22275` | obsolete merge candidate: current Latest/cleanup/absence behavior supersedes this patch | C7 (4 paths) |
| `origin/fix/latest-mui-20261004@1a1931dc904c6edb91c723ef9dfd70407d3ef55e` | obsolete merge candidate: pinned MUI/portal/CI changes already integrated via later work | C8 (11 paths) |
| `origin/fix/macos-notary-phases-0.3.141@d13fe362139465815e031836c58d846f6e409e00` | obsolete merge candidate: current notarization phase diagnostics supersede it | C9 (1 paths) |
| `origin/fix/macos-package-types@a294a1bdeb2dc54a77e27429a1e7bcccc8c24d9d` | obsolete merge candidate: UTType/bundle preflight already present | C10 (3 paths) |
| `origin/fix/macos-signing-keychain@baef99e10b9c3a0049e3f3492ea98cf7c15400b0` | obsolete merge candidate: patch-equivalent signing fix already present | C6 (2 paths) |
| `origin/fix/mui-visibility-browser-reveal-20261005@b94ad8453ca98e0afb4cae44e6acf948d816d5e8` | merged ancestor; no new branch merge needed | none (already ancestor) |
| `origin/fix/per-format-nightly-manifests@bfa55565acdf87f33def501d3951992741090aed` | obsolete merge candidate: patch-equivalent provenance pipeline already present | C11 (4 paths) |
| `origin/fix/portable-license-preflight@cc5049375ff7d7351ed2c55c1a6d4c561dad4114` | obsolete merge candidate: current portable preflight/install rollback supersedes it | C12 (2 paths) |
| `origin/fix/readable-installer-signatures@10bc65655d55af79e5751313252200ab98760022` | obsolete merge candidate: patch-equivalent signature reporting already present | C13 (1 paths) |
| `origin/fix/settings-keep-unknown@af77fb290b7a660dc8bf0e6bef7734f13f6bae64` | merged ancestor; no new branch merge needed | none (already ancestor) |
| `origin/fix/v1-midi-ingress@ee4d07c9aff9bc3ddf339e336c99ccba531a5695` | obsolete old-engine patch; v2 has its own MIDI/UMP ingress | C14 (2 paths) |
| `origin/fix/v1-terminal-retry-and-persistence-snapshot@0ab033aaee1495370c9a64b1939e9f8fa6442088` | obsolete old-KSP patch; port any current persistence failure through current state branches | C15 (3 paths) |
| `origin/v2/block-voice-dsp@e1568f704aa44e583891b829f6260529adce1197` | obsolete whole fork; predecimated-octave tip patch equivalent/current; port only proven residual | C16 (75 paths) |
| `origin/v2/corpus-health@f82202e9ab639bb04c4ad1c829805bbab5036c6b` | merged ancestor; no new branch merge needed | none (already ancestor) |
| `origin/v2/dsp-kernels@d32a2ba5d01a24a302349fa1b26581f13e98931a` | merged ancestor; no new branch merge needed | none (already ancestor) |
| `origin/v2/expression@54b30453dc71c310f5a05d2fc3e3bfbffc7ac532` | merged ancestor; no new branch merge needed | none (already ancestor) |
| `origin/v2/expression-2@c24ee373442ae8168a21ca90a4dc6a1f19ce0722` | merged ancestor; no new branch merge needed | none (already ancestor) |
| `origin/v2/expression-2-midi@67efc66da15196b5d2a301897195f2fbe0ee967a` | merged ancestor; no new branch merge needed | none (already ancestor) |
| `origin/v2/expression-2-pedal@71463ac5fa5d4f4f41654e896142a7d22b54cd05` | merged ancestor; no new branch merge needed | none (already ancestor) |
| `origin/v2/expression-2-uvi@0e963bb910a2815abd744fcd246022d30e4fd576` | merged ancestor; no new branch merge needed | none (already ancestor) |
| `origin/v2/expression-artic@aefab35f8e145594a5f230f350feb41872af32a1` | merged ancestor; no new branch merge needed | none (already ancestor) |
| `origin/v2/expression-pedal@b2dcb074b63d87418821baf5199672fe4bfb0ba1` | merged ancestor; no new branch merge needed | none (already ancestor) |
| `origin/v2/expression-uvi@e173ed570ee36d8598422bc03015b2b044bc9b1f` | merged ancestor; no new branch merge needed | none (already ancestor) |
| `origin/v2/gpt-decipher-dsp@aa3430d61ebcccb2f3e98e66b57aeba61cdf5392` | valuable: measured DSP laws/specs and vector checks; no kernel fix | none individually; accumulated/semantic conflicts still possible |
| `origin/v2/gpt-decipher-persist@6ae158204b2972fd756a868652d0f68151249cb9` | valuable: typed saved-state records; still needs runtime consumers | none individually; accumulated/semantic conflicts still possible |
| `origin/v2/gpt-format-fxmod@b7f6af0bf7bd24876418fead74e57a8afa786af6` | valuable: FX/modulation records, signed depth, snapshot overlays and reports; reconcile current ladder fix | C17 (1 paths) |
| `origin/v2/gpt-format-gaps@56ca1e44a79c46bdc3cc0c77bd69907d01b7bf1f` | valuable: metadata, script links, resource registry and gap map | none individually; accumulated/semantic conflicts still possible |
| `origin/v2/gpt-format-legacy@ad12e63d87c2c3bea3a2fc0c86d7b714c576106a` | valuable: FileContainer/embedded samples, AIFF/AIFC, bounded legacy NKS; incomplete monolith semantics | none individually; accumulated/semantic conflicts still possible |
| `origin/v2/gpt-format-objects@c8e2fbcee6478005b814cac459c6832f2e2c1c97` | valuable: authored Program/group/zone records and opaque unknown preservation | none individually; accumulated/semantic conflicts still possible |
| `origin/v2/gpt-kontakt-ui@0be9ed3f174d8925f1d8a142d3f709e5e26b840a` | valuable: scalar recall, menu conversion, IDs, resources and frontend reporting; no Komplete renderer | none individually; accumulated/semantic conflicts still possible |
| `origin/v2/gpt-ksp-audit@a1446ec6be0162371948cef412dc9e9d0383e6fe` | valuable: ranked behavioral gaps and adversarial probes; not a service implementation | none individually; accumulated/semantic conflicts still possible |
| `origin/v2/gpt-uvi-audit@fb86ac57477dfef5ad1538aada7aa19ff2501e99` | valuable: Lua tests/gap report; historical cache counts need refreshed identities | none individually; accumulated/semantic conflicts still possible |
| `origin/v2/gpt-uvi-ui@24c70a25031243d002e24c661c1afbb1408653aa` | valuable: live widget bindings, assets/fonts, state and additional widgets | none individually; accumulated/semantic conflicts still possible |
| `origin/v2/kontakt-inserts@22a7231abc549c81da7da68cf0e36718cd146bb5` | merged ancestor; no new branch merge needed | none (already ancestor) |
| `origin/v2/kontakt-reference@7d15e5b899aa77b5fada0e5fb81caaba332b07a5` | valuable: native reference probe/receipts; review artifact provenance | none individually; accumulated/semantic conflicts still possible |
| `origin/v2/ksp-coverage@6355d3afe32e2fd72cca289f28d47561532f637a` | merged ancestor; no new branch merge needed | none (already ancestor) |
| `origin/v2/ksp-frontend@4425732469cad99547431df07005f221261557cd` | merged ancestor; no new branch merge needed | none (already ancestor) |
| `origin/v2/ksp-runtime-engine-par@22009d1e8c1f616bb3d6b8e3c5d0862a52b874eb` | valuable: group volume/pan/tune consumer; partial engine-param coverage | none individually; accumulated/semantic conflicts still possible |
| `origin/v2/library-access@e46b2f39205b68470d5eb99488f276c4d07113e3` | selective value: 22 later commits; preserve current bank/stream ownership, no wholesale replacement | C18 (1 paths) |
| `origin/v2/midi-fixes@678804e449457a1d62cd783d8bccd812b230fd4f` | obsolete whole fork; UMP/full-scale/native-input tip patch equivalent | C19 (76 paths) |
| `origin/v2/pedals-scripts-on@3bb0ad735a1b5275ece7e21b9dd1ea1f5ab00f6a` | merged ancestor; no new branch merge needed | none (already ancestor) |
| `origin/v2/product-spec@4d6738b44cc37614256b1e548b6ead4f6b73aa8c` | merged ancestor; no new branch merge needed | none (already ancestor) |
| `origin/v2/semantic-ir@7d63e543b679ff6bdda5cab88b252e43342dd112` | obsolete whole fork; lower semantic-IR tip patch equivalent | C20 (81 paths) |
| `origin/v2/semantic-ir-main@ec8925e1cceb9e11b2166b7e6784c16ac3f55933` | merged ancestor; no new branch merge needed | none (already ancestor) |
| `origin/v2/simd-voice-dsp@95fd47b1efda449e03dde6489ae9ac2a75f91d72` | merged ancestor; no new branch merge needed | none (already ancestor) |
| `origin/v2/stream-storage@bd3b92efc682c363c2a55017e533036a5ed3ad18` | merged ancestor; no new branch merge needed | none (already ancestor) |
| `origin/v2/ui@a02a558d01c5c17bb1565d0b45ccb21d6037153f` | merged ancestor; no new branch merge needed | none (already ancestor) |

### B. Local-only codex/feat/fix branches

| Ref @ full SHA | State / landing decision | Tip intent | Individual conflicts |
| --- | --- | --- | --- |
| `codex/alternating-loops@434718703e1c10c8e6f807d086ff6edcd74746b2` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | Record combined and actual Una Corda loop validation | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/analog-active-ui-audit@d850a7fae3b23b55247856d2f78b105b07c2c205` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | test(ui): capture actual initialized views with existing high DPI painter | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/analog-animation-audit@d6d52079e334bc2c568d286e1000bfaccc5f9e20` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | test(ui): require visible authored callback benchmark targets | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/analog-control-parity-f63@807f035f8ce2bcef9dcb6b8dfefc99611394e41c` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | Preserve native group Amplifier insert split metadata | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/analog-control-refresh-cost@2ed4b0679f390423191f1f6c3610f6a4bc8d3727` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | Leave KSP table revisions current when indexed integers are unchanged | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/analog-filter-census-f4@cc849f10a6fd30c5034a767979cc082b12b49d41` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | Measure notch pass bands by normalized signal power | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/analog-lfo-amplitude-candidate@f7ea6587565861b011d2d76d905cb0e04dfc8038` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | fix(engine): honor verified saved internal envelope bypass | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/analog-lfo-source@1cebe4b3307999c79c3907e418de5c0e845ad57a` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | fix(import): retain verified native internal source bypass | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/analog-script-performance@e4f266fd5266a24cdad950c5dd2be236b8b34128` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | Resume stopped KSP waits and honor continued cleanup semantics | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/analog-slider-input@7a63205492dee9a415bb3d006bfde7f47ce79668` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | Align native fader pointer motion with visible thumb travel | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/analog-ui-performance@05d02e253569b79fd7c8c8a9251d8b8889a45ed7` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | Add opt-in staged real-instrument CPU and GPU UI benchmark | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/analog-visual-parity@cab8d8bc4aa58be7a5ad3e07417898dfe62d084b` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | fix(ui): render authored wallpaper pixel windows from full atlases | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/artwork-resampling@aca27df80857562cd446f8fcf1a1b0c95dfbd0a7` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | Preserve fractional image coverage and transparent sprite edges | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/audit-patch-recorder@3a36b64721769d5c9398bc7d396c91072ae3e0a2` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | Preserve load diagnostic bursts and report audit delivery status | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/automatic-crash-reports@16e911c37566f840f6476434fa675d98765ab1a7` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | Cancel metadata collection and verify consecutive crash retention | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/bitmap-font-parity@9439421cebcc377bd7a12467911a946d7b9cc979` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | Expose opt-in bitmap glyph counts in the real UI audit | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/browser-efficiency@38fc978de0921bb4e43ca9e699436ba4d285dd3b` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | Keep browser UI rows separate from cached preset rows | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/cancel-outside-pointer-restore@781e8e0f0045e3b13b175fe3ada61f7767c0407f` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | Cancel delayed pointer restoration when leaving an editor | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/cc120-physical-ownership@87d694c46b11ff4c447197e170c5dc83a13e43e4` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | Complete All Sound Off ownership cleanup for physical inputs | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/cc120-release-ownership@bac5e8a9ca33f75b5552a6a62334c318affaed0e` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | Cancel held voice ownership immediately on All Sound Off | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/changelog-heading-aliases@3a878bf5388c3739dc97288bb58899ef1ccab6ff` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | ci: retain versioned changelog categories and release limits | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/channel-controller-scope@c63d89e6829fc9530d6696d64128655626e181f6` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | Preserve authored controller emission policy through waits | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/channel-domain-inputs@b5718a64c673a394225ef9ae04b583f79c94befd` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | Expose performance views in canonical callback fixtures | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/channel-domains@67e0a8f093477ed0074442924bfff301e1fa9df2` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | Prove scalar store remap across an empty array declaration | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/channel-listener-audit@27adc432a90a702606e81d20b3c0ae5741156c60` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | Audit Rust crate reuse against concrete parity gaps | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/channel-listener-scope@72a5670f22c166858af1e3288978f847e5481f18` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | Keep canonical and performance listener subscriptions in their own scope | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/channel-native-scope@0d7cfa448d587bdd5b11c0918d1b4435d4f15ed7` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | Separate canonical group settings from performance event domains | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/channel-origin@490e2885a2eb1fd3bcf9a251ffe0a1ea2ae0d8f3` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | Use rejected callbacks to test failed-slot condition isolation | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/channel-queued-scope@8aaee962a2279f780b36e7ace77a0f3c94544101` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | Carry canonical callback scope through queued controller and RPN work | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/channel-route-owner@01d36e6b1c19d32d0d451a53571f05aea3512704` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | Preserve physical note-off ownership after channel mode changes | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/channel-state-isolation@4c29b45501cc08708ebc0aaeab5155ae996f0ec8` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | Share immutable UI revision ownership across script runtimes | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/channel-unmapped-controller@9712a609f08846c82a974de20ad9a720e2ee568f` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | Retain common IR outcomes for scoped delayed subscribers | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/ci-integration@4fef911d0dc5b92f04f26b933377b7b4190c5964` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | ci: integrate verified snapshot gates and lean PR checks | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/ci-pedal-cleanup@84504d6127372a2242bc2b8890f2ef3a66fe6106` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | Continue cleanup past completed middle release slots | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/ci-target-provenance@1f6513438b8b92f1bc408eded04ce1ac8607edee` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | docs: isolate worktree build targets and freeze release proof | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/classic-saturation-validation@6bf086eb9df9c9425152974d2789ed5bbc958a34` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | Apply native Classic Saturation transfer curve | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/closed-control-drag@a21276f69f8817cc95e0d2afc317b7acac6f828e` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | Keep closed control drags aligned across device scales | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/compatibility-checklist@11b4060fc0a1354cf36900ccabf9f4022b3717f2` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | docs: track generic compatibility gaps and validation boundaries | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/compatibility-checkpoint@91e0ff72b30d46dbd5a8742a2fa61e32146951e1` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | Refresh compatibility evidence and mark next batch validation pending | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/conflux-dynamic-metadata@99bdc379fafc2b40d58961248d092d659f222578` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | Use registered identities for prepared dynamic metadata | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/conflux-format-boundary@595a5d17510c209fb29244ee885f74f291d0cba2` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | Decode Kontakt 8 flat filename tables and explicit effect slots | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/conflux-native-filter-115@b534d453461c42dd61c91796c8166435fccea3da` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | fix: preserve native signed Ladder Gain and report transition limits | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/conflux-sample-resolution@8dd906f1f4b17c62a3c62ba63e57932c517f471e` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | Resolve library-root archive samples from nested instrument folders | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/conflux-source-layout@538eeb558edecab16d8ee24a10fcd15710cf5cc0` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | Inspect bounded source identities and report unsupported Kontakt 8 wavetable playback | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/conflux-zone-combined@114b3ccab96e28d1ae96238d5060d596449b350f` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | Preserve restore freshness and budget during zone bank publication | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/conflux-zone-services@6d6f230ad3c3bc2f94ad06ff8a20155e142bbbc3` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | Honor synchronous init zone edits before bank publication | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/constant-loop-consumer@9ef18b105e81f5afcaf07f14631b33f2346e20ce` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | feat(diagnostics): expose applied loop window and physical cursor | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/constant-loop-validation@b83ed44e4a8cbcc361dacee0c8d1d5795c2813ab` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | fix(audio): preserve saved LFO clock across planning fragments | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/container-buffer-read@cac7be0e60b5c05650f4f3ed623dabfcadd7e949` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | fix(import): preserve container decoder boundaries in load errors | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/contradiction-amplitude@fd00cd4f7d61708837f7980211a8538d75261ac6` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | Make covered-source archives reproducible across Mac architecture jobs | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/control-display-parity@ee434b897ded67dc579a1316c68ad64137ae17a2` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | Keep menu-selected picture names as native slider captions | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/control-parity@6bfc9c879e2640c99d1b3d86ec011bba717f238c` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | Apply scripted filter EQ and stereo controls to instrument racks | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/core-parity-next@131302bbc866690a08c0d7b12456deb20ec51134` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | Seed restore regression epochs through the production allocator | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/core-test-allocation@7cfb6d5f3e84df770484e9d17cf0ea0cd90a32d9` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | Share allocation assertions with core unit tests without plugin features | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/core-timing-introspection@d9975648c31fefb1a42dbf0004eab9679dd8bd4c` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | Declare existing plugin requirement on Solo GUI-discovery integration test | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/core-uvi-integration-20261005@834c56ff9960a21e0483b126c66dbb720d26bb05` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | Checkpoint ongoing core and MIDI refactor before UVI integration | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/core-uvi-ui-verified-20261005@ce94add200f7d69d38e538c0241d7d3bf4a39da3` | obsolete parallel integration; 1,174 divergent commits; port only a proven residual | Selectively merge PR33 native bridges into the core/UVI integration | C1 (69 paths) |
| `codex/core-uvi-verified-20261005@acc3532e240a964aeca16707728d0da0b44ac15a` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | Merge UVI with typed rack and retained instrument-bank ownership | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/crash-audit-115@4410e3c90f1c50e9e90b5b1407575c7b3432c50b` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | Exercise report permits across actual detached worker completion | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/crash-completion-115@49bbc50d593f53b1dae932c6817ff1d752fe1231` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | Continue confirmed crash uploads after acknowledged cleanup and upload permit release | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/crash-completion-review-115@52f361d8c5422c126edbb22bf384d394310d5fcd` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | Resume a confirmed incident adopted before the previous worker releases its permit | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/crash-drain-115@f0f6546a348cec39e62481dc0520be3808df066f` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | Adopt current keyed evidence and conditionally preserve stronger same-incident proof | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/crash-manual-status-115@f2ab6e2c32c8905ce8df3d13e8f019b2b7c94086` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | Persist truthful manual-export crash report status | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/crash-observability-115@20b30ec5850294b63bb5c1122d942a2495acc185` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | Keep crash incidents in separate durable queue files and preserve legacy migration originals | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/crash-oversize-disposition-115@311eae5fbbd5ce3fc266f50a8a39fcf7c1048ded` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | Retain oversized automatic reports for manual export without blocking the queue | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/crash-oversize-review-115@aad6d153b77a2f3f4452e375ab883819ea122a88` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | Continue once after a terminal oversized report disposition | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/crash-provenance-115@e5d50893050cfca9490aa3b072d4f41ee115efe3` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | Use recorded incident metadata in automatic crash reports | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/crash-receipt-visibility@bd10e942e9983faf66bbf2f01a1d4e79c088f543` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | Show cached crash report receipts above Logs filters | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/crash-recorded-process-115@bab4f2168b1da8e2cc44f28c99487d5160886502` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | Fall back to the recorded incident host process | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/crash-restart-lifecycle-115@4a4f5ca7765668c5adcd4928a20ca092be373578` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | Verify crash report restart recovery through actual registration lifecycle | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/crash-retirement-test-barrier@82495ceefb5320fbba83364b2ab55791842203ba` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | Wait for actual failed queue retirement before releasing its test publisher lock | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/crash-stable-evidence-115@d4e3ebddd23c82c2b8f1de4bbcb5929095eb7981` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | Keep automatic crash evidence stable across reopening contexts | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/crash-windows-signatures-141@e8dad7081eb788fabf348de17744d44f3ddb19de` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | fix: match native signature helpers to production and test callers | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/current-library-captures@fd00cd4f7d61708837f7980211a8538d75261ac6` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | Make covered-source archives reproducible across Mac architecture jobs | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/delay-sync-validation@2e6466ac2663ee5d7277573ed4d582ad9c07d251` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | Retain delay fixture buffers outside counted processing closure | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/delay-time-law@b63e0b26d35ddd22a8c2afe27bac6f25514bcfa5` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | Persist coupled physical Delay caches through prepared native snapshots | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/delay-zone-restore-validation@8458e844476737e995bb88bf926b7e4234024914` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | Accept eight scoped core fixes after coherent gates and advance version to 0.3.123 | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/diagnostic-core@9e41a316bd5cca2b3c1dc2b10f46ca58da69b870` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | Include orphan journal rotations in retention and support exports | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/diagnostic-hooks@1a6720d187e027a645f896977209b62896de088a` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | Throttle streaming decode diagnostics per bounded source identity | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/diagnostic-panel@5dc24f376c3107c165ec6a2a3a6edff1780ca366` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | Account for fractional log row font metrics offscreen | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/diagnostic-visibility@a43530cdbc2208b95fce3276fa0b1404a8475517` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | Preserve actionable load failures and document diagnostic coverage | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/distortion-damping-96@6072c8719ae1340362932386a1cd03ea0fab3c8a` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | Apply native steady-state Distortion Damping filter law | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/distortion-dc-101@f633e22023589de181ac1b814b06f820fa7fa05e` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | Use native SIMD sum order for Distortion DC rejection | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/docs-async-nka@bec2c20cb9aa85d6e667e979519af2d2c913e643` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | Document verified async preset reads and scoped font and planning support | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/docs-font-write-proof@526de21a68f77f54e256a18457d1e658a5713416` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | docs: record verified bitmap fonts and explicit NKA writes | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/docs-late-ui-harps@58849001c1d35d3d6db3fe7ee1788da946bee04a` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | Document delayed caption publication and verified harp release voices | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/docs-latest-parity@4d3726846dac94c95a06499dc72fa17d0c2d00bf` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | Show the complete build identity in About | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/docs-overlay-replacement@1aae29176c563b7879b56a81fa2faf4980d01aa8` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | docs: scope replacement proof and scalar edit measurements | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/docs-verified-playback@6e83e532cb973ad1523edd1c7728b4d51ac1b2f6` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | Record verified fresh-note recovery and explicit LFO playback limits | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/dynamic-rack@159c5a2ec544766d825e8204a408ce7d5cc823a1` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | Remove the rack part cap with prepared storage and viewport rows | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/dynamic-rack-ui@ddfa727e9a24feb7b05902d02c5dc8ec5833ca01` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | Capture browser catalog Arcs independently of edited frame | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/emotional-piano-access@744f96c262dda8533f8209de45151f08c4bcd992` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | Explain encrypted preset access lookup and accept XML field whitespace | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/eq-display-parity@64f3128159adfcb0afcd5945a4499e51e129b2f5` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | Display Solid G-EQ gains using the shared DSP conversion | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/excerpt-redaction-fix@7abef791aac1781e440fe6cdc2d20ec968d2a78b` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | Read source excerpts from serialized diagnostic event data | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/explicit-ahd-only@91c7268b18c07ec710a027ac9cd56b2f2afd702f` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | Check AHD symbol through the exported parameter-name API | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/export-123-fixtures@fe6b90983623cbf07fb8753180c17d263f8aa20a` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | Keep accepted 0.3.123 fixes in the generated Fixed release notes | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/external-v103@89bcdf635ad39081c6e5460dc37103af671cd140` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | Describe wavetable quality limitations for every native setting | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/factory-snapshot-loader@5686c13d95f406a8759e540af2dcd9dc0312b0e1` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | Assert rejected snapshot retains source generation and installed epoch | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/falcon-compatibility@8f3c04cf1a445a72b585f8e4b5b05ab604a6672e` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | Enforce UVIP XML trust bounds while constructing the tree | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/fast-development@252cb115c2ad42c0fbf5ba69b521150ec5d468e0` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | Allow core feature scope for exact library development tests | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/file-format@01c7ec676c32c877680b77399f4e48305d760cf1` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | Add lossless NIS container writing and keyed subtree encoding | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/filter-effect-parity@f5db6ab327dfd263d8b87743dc129664638fcbbb` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | Check Daft proxy stability across cutoff and resonance sweeps | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/format-record-audit@7be1b5bd382ddd90733ce06575cabe7b95a4eaab` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | Write Kontakt envelope records without losing opaque metadata | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/global-editor-preferences@294bb9f596fba330ce8d655b075bdd97dd05c833` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | Persist global editor size and add independent UI zoom preferences | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/group-insert-pipeline-8883@d3d7373f354787761d7d54939728469d012e8a81` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | Apply native Classic Saturation transfer curve | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/group-inverter@d9592b564133a9b6123671cb5127e432f751b04f` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | Document actual native Inverter field-mapping limits | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/group-start-conditions@37475ad4721097d1e576f83b2905de71bff9ceeb` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | Preserve native group start records through import and NKI writing | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/harp-native-release@cb8f56e7bc9ffd38d0302b167305f85ba12a5a7f` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | test(engine): select automatic release group in system flag fixture | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/harp-release-fixture@11b43fc04963303439b84af0e7e0d583d2dc96e8` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | test(engine): wait for generated release zone identity handoff | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/header-art-visibility@7218ea8479a100efd2e3b804c00a1ac504089458` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | Make rack header artwork slightly more visible | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/headless-script-context@5fbd616e38d9cd4ac376545ae68334d3a3c87c79` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | Retain runtime fault actions and source context in headless diagnostics | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/host-delay-damping-validation@55a0572e0ab7c9e7c1673668a5ad14fc3f219bba` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | Accept fourteen scoped fixes after frozen gates and advance version to 0.3.115 | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/host-note-ownership@d0267370fd02096d50b0a30db09cec8ef66647f3` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | Keep articulation switch notes separate from exact host input ownership | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/host-ownership-validation@301d720a10aba6405cc18887cc32465a7c9c65dd` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | Keep articulation switch notes separate from exact host input ownership | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/import-scaling@65e20235911a981eb3d615f9cb90ae46739cafdd` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | Decode large sample file tables once per import | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/info-diagnostic-summary@1463536aa0e45742ef8b472ac44fd2a82fdf8c9e` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | Expose retained diagnostic counts and late failure causes in Info | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/init-fault-context-fixture@e7738020807fe6657cceca8e18e529c98ae1c7f1` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | Require init callback context in keyboard fault regression | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/internal-lfo@c8310134002e52d3d18ac816340a81fbc136223d` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | Bound observed LFO rate inference and record unresolved clock semantics | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/internal-modulation@66a3fc9fb619efb47cbd8491bedd2169e1c318a4` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | Check pitch envelopes reach the audio resampler | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/internal-modulation-continuation@30fbcdf6e4d358440ae28879d6b7d4170605607c` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | Route legacy pitch-envelope intensity with inferred cubic scaling | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/ir-size-state-validation@d6eda072f0bfd7c283f767db8ac46191c1978591` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | Merge commit '3476899' into codex/ir-size-state-validation | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/ir-split-validation@0c8fb0a0bf83e5974f685f2a0f384e2281f47851` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | Prepare explicit unit-size convolution early and late filters | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/ksp-async-array-writes@196f34a19142306a5714a5b1f9142085138f63db` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | Write KSP preset arrays atomically through bounded background jobs | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/ksp-async-arrays@5e8df1ac187b6317d0eb39139a4e8746ebafa459` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | Expect dense retained numeric table snapshots in array regression | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/ksp-cc120-lifetime@8be80519f30faf8fdd0330334e7db71bd1a9cfdd` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | Preserve physical note cleanup and routing across MIDI sound-off | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/ksp-compat@83905e32d6faeba83c1641e5ceee852a99f16a1b` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | Honor KSP native sustain and release system-script conditions | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/ksp-control-gap@7b185f621617a60eb1c45d7f8602fafeda00da7d` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | Exercise control text capacity independently of bounded string variables | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/ksp-device-default-fixture@987697bdc30fd96f19742b6c09a19c73669e83bb` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | Use real script initialization in controller default regression | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/ksp-host-read-parity@d6ce28c1b15589010252c540ecfac9b7e3380c6b` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | Report actual KSP event zone identity across mapped and missing samples | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/ksp-host-transport@32bee895690c3abc246a257395be6d644cc7b927` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | Use host time signature for KSP bar durations and sync rates | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/ksp-independent-listeners@4fc9f113235c79258efb49fa868fe5ad21af0c80` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | Accept documented symbolic level-meter chain selectors | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/ksp-init-rpn@64edee6b4954a928f3c5aaf1df67a22002ee81f4` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | Forward init RPN messages after receiving script slots initialize | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/ksp-menu-index-getter@f4f02888eefd7ef1dc50a35e7c52429b3349085a` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | Restore native persisted menus by entry position | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/ksp-native-ui-resource@1baf1be240e53a25f26ce9e9e6ced79196c37612` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | Accept Creator Tools null lists for empty exported menus | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/ksp-note-controller@f68c0327777be60f41209d5dc482fe7d2ed48635` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | Implement internal KSP per-note controller callbacks | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/ksp-panic-cleanup@bace8d125299ebb6da4d405fccd0fbe05f023edb` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | Cover Panic physical cleanup after a downstream script release | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/ksp-post-panic-silence@28bc85ee9097e8a1ea77a5e49bcd17e52dc0e5dc` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | test: cover authored controller defaults after panic and resets | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/ksp-string-boundary@564e353e8c0d73b9ca37054347d889d6326330db` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | Record retained UI controls and complete group drive slots | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/ksp-zero-fixture-ed34@fa0849f74000471cb97aed5311635ac1f5a0d169` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | test: isolate note tracing allocations and retain genuine math faults | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/ksp-zero-math@93452d0512663624a621b09cf44527e070fb687b` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | Match native silent integer zero-divisor semantics | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/ladder-control-clock-123@c74afac5d34b351899f6f5e0526f6fcfba6e0821` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | Snapshot Ladder targets consistently after local reset | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/ladder-core-validation@cabde651d5c3696ad8ce205423b3564952c45068` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | Retain debug formatting for prepared Ladder voice state | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/ladder-legacy-cutoff-clock@ce1d7ccbc8c4f89ebbf2cffdae98a0f4191e9fce` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | Preserve versioned native Ladder cutoff clock mode | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/ladder-modulation-cadence@d64b24917f1a657e5d2fa297147d4346c0a09135` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | Rearm enabled Ladder modulation targets on persistent control ticks | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/late-ui-metadata@695eb47a09ff20257290e84ee56f869d4f55647c` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | Keep the inactive scalar guard borrowed during activation | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/legacy-filter-intensity@d63a9b17494ddb73acd884e1954289de73f90879` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | Route legacy AHDSR cutoff intensity through measured cubic depth | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/legacy-reverb-audit@e8dad7081eb788fabf348de17744d44f3ddb19de` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | fix: match native signature helpers to production and test callers | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/legacy-reverb-native@368e41dec0cbcdc833fe4b1549f6b7a407ff7772` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | Process saved legacy Reverb with prepared native delay network | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/lfo-format@2c50102fd8f7ddc71ebfa0ffd47de6f84fd8e777` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | Decode and losslessly write packed Kontakt LFO records | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/lfo-source-namespace@9c76376f28a8ad1c10a2e7e88cd29e52c58b0727` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | fix(import): separate insert targets from pitch LFO source slots | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/library-name-reveal@776877ba82cb41096f32a69c1136b1b605abc77a` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | Add persistent library display names and validate native Reveal paths | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/library-night-sweep@c6e84fb538cf7da26b6d4d864c565d323db04606` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | Apply compatible compact snapshot group effects and modulation | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/linux-render-init@ddb451e07ddd55b10a6675716a22897e9ae6225d` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | Open VST3 editor on attachment without waiting for audio activation | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/linux-startup-diagnostics@7859c5bc7ed30b2ed2b143efb2e0fc0852f8343e` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | feat(gui): record the first native presentation boundary | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/live-lfo-validation@b5d0a4739b3465f963f886469f7a87842d6c3baa` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | feat(audio): apply native intensity writes to admitted pitch LFO targets | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/live-pitch-lfo-bypass@02a666ccb665c85ee7e0ebd543c363b81923e4cb` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | fix: apply live bypass to admitted native pitch LFO sources | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/live-primary-ahdsr@c446a6c7b041eb98c86d6f8c7d61720682bcdf15` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | Prove rejected phase setter executions through their distinct fallback records | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/live-publication@f7204619731b899fea37b2c874060b304b574320` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | Publish and recycle live script views directly from editor frames | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/live-request-epoch@3c10c69dba56a3008f7d208df59abbb0f467ac3d` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | Keep source epochs on lent script views and snapshots | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/live-revisions@4e12cc1f5fb7efc2d3fb47e168fb231fd5d4022a` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | fix(ui): publish idle script changes before redraw checks | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/load-detail-inventory@d83d6fdd154fe752cfaf861663d3b40b9e001221` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | Preserve bulky load inventories in bounded journal chunks | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/load-warning-journal@46c00c2d3e327204454645ca6a9dfde5016429db` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | Journal load warnings beyond bounded report examples | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/lofi-frequency-law@fb7bae574aa9e179d043af5d792dc44d4f61bc8c` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | Correct native LoFi frequency direction without claiming calibrated parity | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/log-row-hit@71a01e66e000cc0faf2d51a374e5d00d1700591f` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | Select diagnostic event rows when their text is clicked | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/logs-compact@f325093f975f77745a13c2e60c9d02cb3403c58d` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | Own event detail text before updating the Logs query | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/logs-export-race@41dd88b032d61182f8a7b2bda6bb9651cd407fd8` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | Replace cached loading placeholders after instrument failure | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/logs-ux-refine@e8dad7081eb788fabf348de17744d44f3ddb19de` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | fix: match native signature helpers to production and test callers | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/macos-bundle-recognition@eed738fae090178b78666b48a5ceaafc7dde2361` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | Preserve macOS plugin package flags and gate native discovery | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/macos-editor-lifecycle@faf04d855b011d73e3709d7a1fafb562513329c3` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | Track attached VST3 view ownership independently of creation | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/macos-package-types-private@85a04956ebbf268c4c979ced48681b528c02ddb8` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | docs: scope 0.3.115 length-hint evidence to direct Rust admission | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/manual-crash-export-2f@718e167737487e499c45b4d3de37d10004a0daf6` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | Report archived journal identity mismatches and bound export ownership metadata | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/midi-audit-d7@d5404bb21e40a50bb43da8566dee8d929f1cc563` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | fix: preserve held note expression destinations across routing changes | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/modern-cutoff-depth@3d7527f82b0fcdd6bdea6a6f3a3c418690989c9f` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | Use prepared group modulation table in cutoff gate | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/modern-filter-records@8ca2ff8ab4ccbc1604d2fa43ca8278f4d9459cbf` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | Decode and preserve versioned Ladder filter records | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/modern-pitch-depth@3154d4454c786885c0e67439c9dc15fa0e70e4de` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | Decode modern signed pitch depth with corroborated cubic law | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/modulation-slot-recovery@5f231bea0c2b4b174d26d99c9889eadfee9189c0` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | Retain readable modulation siblings when ordinary import rejects one slot | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/module-envelope-controls@628ca1334a50a6f9ffc438ca95dce7c9df2c85c9` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | docs: record actual module envelope callback validation boundaries | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/morphology-source-boundary@3801b2e4f44f9ba15865b7fa3062034019d24663` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | Decode and preserve modern v3 snapshot and v4 compact source records | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/mpe-pedals@1b6a6288543abb2404d226fb57defba633a4888d` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | Apply negotiated MPE sensitivity to manager channel notes | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/native-capture-slot@220b9054b2c8465014c007c451ee41c4b5ea3d63` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | Leave KSP table revisions current when indexed integers are unchanged | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/native-clap-midi@221f50648ed52c148976218723718b3b66dca0e0` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | test: assert exported MPE input ports preserve ordinary MIDI output | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/native-editor-lifecycle@b7b459d7a96db4b4c265009a76af3be29d5c592b` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | Persist native GPU errors across device rebuilds safely | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/native-engine-state@683b2730bb1887f2087c6c661575e896359e093a` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | Capture the decoded native value already applied by the engine | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/native-lfo@649066a694ba9f55763f420f84c472a5fea0e9ea` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | fix(import): decode LFO waveform-specific payloads and versions | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/native-lua-runtime@f9830c67f507ca04daaa097ed9a361c1350b1867` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | test(native-ui): retain full Ring annulus winding and inner radius | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/native-lua-ui@3c806173e132091f14c45e931a049a9a9d16f8ec` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | Bound component nesting and reject incomplete DSL names explicitly | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/native-lua-validation@ea5cd87a988cbaaffcc55bfb3668a29680d8e921` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | merge: synchronize Native UI candidate with accepted 0.3.142 metadata | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/native-midi-audit@682dabc06c7a854f64980da163c89afbbf40dc36` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | test: verify native onset tuning against independent tone frequency | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/native-midi-input@eac5dba8b88061c716003f7244ccdfd6c2adc929` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | Admit optional VST3 note length hints without shortening host ownership | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/native-module-bypass@c68b8e61d6c91dea217fd1630097b0e126c57ec6` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | Initialize pitch and module envelopes from saved source bypass | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/native-parent-contract-141@71b640dd4589ba9b23b16d26dfd88347b22dea9f` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | fix: validate native editor parent representations before attachment | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/native-primary-ahdsr@a7c13ceb378935bca5a029d60b49db2e0c34cc54` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | Include propagated target rounding in AHDSR reference bound | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/native-primary-ahdsr-validation@990a75c8688c4164e9b7d4f6d357fd882da1ea43` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | Keep router-state recovery checks independent of native admission warnings | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/native-shaper-layout@6a782c6f3538561a5d78a3bff0230cc03be45e44` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | Read wavetable modulation targets without a module slot byte | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/native-ui-discovery@80c5465956d3b3d432b202601cc3dc0f14a7e911` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | Resolve native UI entries with explicit language and target metadata | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/native-ui-drop@d7676564bd8257c6a703620f6ec87c2a706eaba1` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | feat(native-ui): map bounded Spacer to existing MUI surplus layout | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/native-ui-journal-isolation@25897ccd64b68d5a1ca977221d78684524fcde14` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | test(diagnostics): isolate global journal contract in subprocess | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/native-ui-lua-probe@68b777eef273477c918a8b78b9687e8f202db4a3` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | Add executable native UI Lua module and connection feasibility probe | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/native-ui-rotation-adapter@4ced974d2c3e2b5bc1b9dccdcbc18662b8964e59` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | feat(native-ui): adapt subtree rotation and captured local pointer pose | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/native-ui-timing@3f539f9fcb0b806ce30b2efdf67d979aa19a3fca` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | Capture bounded opt-in native UI callback timings off thread | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/native-vector-assets@7b3423d68771d63e8e03bb983819fae0091f3a4b` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | Pin MUI readonly current-pointer accessor for native hover events | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/native-wavetable-source@db85397e69e75f166c449203abbde738e4d774f1` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | Test wavetable constants through public KSP initialization | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/next-public-export-review@db68c12ea8d0d326a7e50fe4eaa6ef324fb252a8` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | Review timed-note fixtures and labelled executable validation digest for export | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/nextshipping-ledger@d289917ca01befa2af49fc857632079ec0335420` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | Advance to 0.3.64 with seventeen reviewed semantic corrections | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/nightly-critical-path@378e39094318ef27da2be6dbc68fc14428117e44` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | Overlap nightly packages with release checks and avoid duplicate main CI | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/nightly-format-manifests@520f104863dfcb87395301216e281cbaecf6bae2` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | fix(ci): skip absent nightly refs before deletion | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/nightly-keychain-fix@8311e3319dca9070d196e0b022fc9bbbaa5f582f` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | Handle bundled Developer ID intermediates without hiding import failures | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/nightly-notarization@2022c4d306115538135ef35d5fd4ade578df2599` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | Accept wrapped base64 certificates while validating encoded contents | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/nightly-utf8-notices@bbee8df35a8bdd817eb4971fc4b19e06e88b5744` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | Read generated license JSON as UTF-8 on Windows | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/nis-copy-efficiency@5d3b5971198d13a67590470b3bfeef889b11b44a` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | Reuse validated NIS container during file detection | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/nkx-directory-diagnostics@ea161ac8399daa214345088741e684cec888e84b` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | fix(nkx): report directory signatures and read boundaries | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/notary-phase-diagnostics@7dcd30455ffe1fa1edba43ede81d413eee58ddb7` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | ci: expose safe macOS signing phase failures | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/note-off-offset@8775d97522791ee86775e8266a17fc930c03eee8` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | Honor KSP optional note-off offsets with retained event timing | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/offline-release-tails@43f07b3cc43756dc118e1610d78427049acd6ed7` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | fix(render): retain release tails during offline overload | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/original-ui-text@2aec43871d31edc5c20c01867cded535f6078cc6` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | Respect original control font colors and state inheritance | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/parser-compat-reported@1e79e3ddaa95dadc3e416197db072e1902fff2c1` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | Document bounded Falcon groundwork and current LFO playback limits | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/pending-114-public-audit@5d9b492126264ea804c9b86c7e760dd5078fbd98` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | Draft pending runtime outcomes and preserve BUFFR distribution notices | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/pending-115-ledger@66239187aae5d0257c7c59f5ad71c78fedcfaf40` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | Accept fourteen scoped fixes after frozen gates and advance version to 0.3.115 | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/pending-release-ledger@631c70d5b8f34c817603c48632779da5caf3c6e8` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | Advance to 0.3.47 for eleven validated compatibility fixes | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/per-note-brightness@7881307ef93f581da0fa15a4185e16fd8fb146ee` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | Keep registered per-note brightness in its retained voice expression | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/per-note-brightness-ingress@57ce315557603724dee1d0d8a3a4f143a65ece0b` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | Apply per-note brightness to native voice start-only modulation | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/performance-background@ebcd1fd342aca6a1d02a7379789cf869bcd9dd63` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | Retain authored performance-view background colors through live publication | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/performance-hardening@c446a6c7b041eb98c86d6f8c7d61720682bcdf15` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | Prove rejected phase setter executions through their distinct fallback records | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/persistence-unlocked@1fedf8a3bb98b7dd0c428c18a74d1a71a23696d7` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | Keep unmatched snapshot copies alive until selection unlocks | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/playback-ab@5a5c68a2c419120bb0d298ccc1ca2d8246f75bbe` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | Account for reviewed synthetic KSP regressions in public export | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/playback-night-sweep@3007f4d41ea1c765f5fe97bb396d1ca378808985` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | Review muted fresh voices separately from residual effect tails | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/plugin-owned-panic-context@817eea7a9f87aaabddcd9609f81b8636f22890cf` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | Retain plugin-owned caught editor panic context | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/popup-hover-backdrop@3fc08ee0b677b7d8fe3a607e827e817930d809bc` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | Let popup menus own hover and outside dismissal clicks | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/popup-hover-fixture@9b69011990a1ce222fd7108e6c2aa9dbc5a62aac` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | Provide authored artwork for the popup view-selector fixture | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/preset-header-row@08f4a8d07ffbd2a0ca3b6e43a1618675cad2b23e` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | Keep snapshot categories in owned menu labels | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/preset-replacement-state@870c1fca3c6314d3354f2af0dfd1db37aebc3b54` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | Attach preset helper comments to their functions | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/public96-export-review@3f29d1ea883e8a343d03c1c01e5d98d8b1833b48` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | Review authored callback fixtures and omit two public example checksums from export | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/rack-empty-drop@79f6fc3bcb4c48495c58a4c18a71d2c84fbe16bc` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | Append browser drops anywhere in the rack empty canvas | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/rack-tail-geometry@5843155b66756d67e7204773e83d2283141e6416` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | Keep rack welcome drop area and scrollbar gutter stable | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/rack-viewport-virtualization@e5d22bb712bc5e986137089364204c525706574d` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | Dispatch queued keyboard input before checking rack growth | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/reference-format-audit@f95235358cfcaa2c0670c81a776288aa749c0a35` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | Add lossless native Kontakt filename table records | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/release-changelog@18c68a01ffd5a8369ccfb50ecdaa271ac57aba52` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | Record validated 0.3.23 fixes and bounded migration scope | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/release-note-mono-fast-path@86cdb699ee03af94508c894d70bfbe4782c982fe` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | Skip Note Mono tail scans for inactive banks | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/release-retention@0c685ffaa536f70a16bcd1dbcc634e6d90d2cede` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | Preserve stable source tags when pruning release downloads | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/release-trigger-note-mono@7c09b821feb5ed4ba43e509c63aab18ff07fccab` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | Preserve and apply release-trigger Note Mono | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/replacement-fixture-input@5b55afe81e43dcc92ae8cfe460f0fd090f442c60` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | Drive replacement fixture through a visible preset navigation control | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/reported-format-next@7aeb016f30b54f486598ca764ff230b866ee61d9` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | Keep filename mapping independent of optional calendar dates | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/reporter-release-draft@12904ee25bf5338f6f0dde7b2ca9d989e96c2de6` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | release: accept scoped reporter and Ladder outcomes for 0.3.141 | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/reporting-final-validation@e8dad7081eb788fabf348de17744d44f3ddb19de` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | fix: match native signature helpers to production and test callers | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/reporting-provenance-docs@55317c2ca6bd50a01d59f325110ff5f09baf0fd8` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | Document support journal provenance and separate local export privacy from pending service storage | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/resource-ownership@1fd1380ee0f64078880e649e8b5f9336ed12e814` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | Keep resource discovery within its instrument library owner | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/responsive-rack-header@166e50c17515cf15e025ee301adeae58dd403b64` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | Give responsive header fixture authored artwork for its view selector | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/restore-freshness-96@69304c5a33fc4772ae2d07e19c44137a7b867660` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | Seed restore regression epochs through the production allocator | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/reverb-time-display@782b00914b1b957baa445327ebad33e2f764df18` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | Display scripted reverb time through the effective DSP decay law | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/revision-owner-paged@4f0f24288af2ece2088ea6ab89d995138b330e28` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | Share sparse UI revision ownership across script runtimes | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/same-frame-writes@33b77c133331954411bdbc7f05da15cc1b24348a` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | Index same-sample parameter writes without audio-thread allocation | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/sample-traversal-parity@c446a6c7b041eb98c86d6f8c7d61720682bcdf15` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | Prove rejected phase setter executions through their distinct fallback records | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/saved-lfo-fade@b8885133bd4a0a9523e7ad00e37a40a1f46ee2ff` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | fix(engine): render proven saved legacy pitch LFO fade-in | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/saved-lfo-phase@28ce8a579e8773fc39bcc219a523d3c428e9c072` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | Apply saved cycle phase to admitted retriggered sine LFOs | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/saved-lfo-pitch@7b93fa535663369ee331f3239a74caa037379e76` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | feat(audio): apply native intensity writes to admitted pitch LFO targets | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/saved-sine-volume@184d803f4e9c59764817baf11a224d463d6db3db` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | Gate sine-volume writes after render and verify decoded amplifier placement | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/saved-sine-volume-141-validation@1f1836a156f96ee8e18652e771c49c2d6744cd4c` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | Accept primary AHDSR and visible crash receipts as 0.3.148 | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/screenshot-prepared-fixture@6ca7ccab9076252d2dfcc5a546c45f01e7fa5a38` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | Prepare screenshot rack variants only when selected | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/script-excerpt-diagnostics@870537f3dc45d6bcc200087dc9c52fb0be978de3` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | Fix source excerpt regression lease and report serialization | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/script-slot-ui@d82f6c7e3849934ec8137b7fe5008a4df40fddd9` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | Use imported tab helper for performance page selectors | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/service-controller-home@8925028fa0c9ae6da73d97cef6bbbe70fcc32394` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | Route ownerless script controllers through the configured part home | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/signed-intensity-alias@7007e7514f3346a13978007159411bdc9fdef38d` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | Annotate signed target readback fixture values | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/snapshot-base-binding@69f6fd8766f516654fa47ddb8eff97a9e4a48763` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | Bind template-named snapshots and apply explicit modulation removals | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/snapshot-modern-layout@35c0f39d5b4bcb1eeb858f1d19aa93c29c57e96e` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | Decode counted native modulation arrays with 64 external slots | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/support-cap-honesty@24d12998cd2609a3daa636dc32640e9f181d0a7e` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | fix(diagnostics): count KSP faults omitted by the location cap | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/support-core-reviewed@2f1572c8628e4e200392a8b62b9e62160eea6cb7` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | Report acknowledged queue cleanup failures and correct omitted-slot fixture | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/support-parser-mpe-validation@c0f1d578aefc7cc835f9e885475a42381509ea0a` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | Enforce support service JSON upload limit before network delivery | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/target-depth-polarity@84d3ddcc28a5afcaafe17fdb590706a0ddc07228` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | Decode Kontakt external modulation v0x104 with opaque footer retention | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/transistor-distortion-96@6c963845b32c915e28b01fe84f3747a409544575` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | Apply native Transistor Distortion scalar transfer curve | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/tube-distortion-96@334265511f83c847bf46d9eb80d482a03a808bdf` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | Apply native Tube Distortion scalar transfer curve | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/ui-auto-fit@0c691ed86087c56d791cbb41c10e5cbd741d9b0a` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | Parse UI benchmark metadata following the libtest prefix | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/ui-cost-partition@0896cd5da5c88589cb953430f6e011f534f4cf67` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | Count retained interface array cells in UI cost reports | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/ui-dependency-revisions@7f53de472a6f29a1def48a84cf39a2e077bc55b7` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | Invalidate performance views by published revisions instead of property scans | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/ui-graph-parity@76b3227c08d3f339f28f9b17a825345e6b474d0c` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | Preserve broad pictured value displays in vectorized views | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/ui-integration-fix@b68501fa03074d66a7feb502f33f78d7d582d2ea` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | Keep hidden Logs panes idle and update app menu fixtures | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/ui-library-sweep@1577b173d7c93c938532b2786f6f84510633f996` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | Require callback publication coverage in the UI probe | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/ui-observer-pump@5d85d15d7169a3d3bec86292cf389c4a2c86a381` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | Identify the actual callback target separately from direct frame edits | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/ui-optimistic-overlay@54860eb13785b3727494b86e5e562d870321a31f` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | Draw optimistic control values without copying script snapshots | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/ui-row-publication@010789f49f4239284319a33e451c0768ff1a4a55` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | Keep the static reuse fixture free of temporary string writes | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/ui-section-lag@d661045451939458a513422774788ded8c5738a4` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | Retain unchanged Original controls across live row publications | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/ui-selection-cost@f4d797476b607d3e3fa0519e7f6d887246a2649f` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | Document pointer isolation and consistent EQ gain captions | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/ui-sharpness-audit@dc82d0f91d67349b73edec69a2e96295ab0b5d0b` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | test: distinguish opt-in cold CPU cache from warm observer frames | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/ui-snapshot-reuse@27530d5c949b9397e784f1d0e2bea385bc0cf026` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | Avoid retaining an unused live publication interface Arc | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/universal-macos-installer@08de72fd65d6ac98ba3686358bfb2f3c7dff2a87` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | Make signed installer bundle resources readable by normal users | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/universal-nightly@d798bc47fb22db5fada0d7f37ac61926683672a8` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | Retain reviewed processing and version-specific fixes in release notes | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/uvi-linux-compatibility@9d69ae0b62a1ec73e85a70f50e73a6d016e0fb0a` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | feat(uvi): split audio endpoint and extend measured DSP | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/vector-plan-work@7e9abced65e101227bc6386e057cf8e45e228e6e` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | Remove unused Vectorized switch marks and catalog wrapper | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/version-workflow@b08378378afd671618a23a478b2a6ff75f3abbff` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | docs(build): declare the pinned framework minimum accurately | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/void-linux-portability@e31a6028d96c9f03e5db3e4991a72175fd09d48b` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | Retain GPU startup causes and diagnose Linux embedding | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/vst3-tuning-onset@6a7ebc5934b2bfc1d7349ca598b83f4abaaa9e65` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | Render zero-rate integer taps without a zero-stride iterator | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/wait-async-completion@aeaf340bbb88f293b9060008fcd3efdc88eafd48` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | Wait for worker-backed KSP async operations to finish | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/wait-async-validation@53363885924dd8758041edfe5c4955d1f4d4a902` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | Wait for worker-backed KSP async operations to finish | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/wallpaper-resource-formats@3897fb7ebda4db29412b907cab5b767027c5987a` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | Resolve declared JPEG resource names with shared image decoding | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/wav-pcm-fmt@48a13665148d5afdb41e539682c7850761f77788` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | fix(audio): decode extended PCM WAVE format descriptors | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/windows-allocator-lockpair@ed244da3e2bfe62a76fb38b17f0c14e75844c7e7` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | Restore gpu-allocator Windows ABI pairing with both wgpu-hal versions | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/windows-renderer-stability@a6009f9cc690d974cfa3bfbd0107b7217a19d648` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | Preserve native surface failures and persist GPU init stages | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/windows-renderer-startup@083a0fd5c336d95111a47c693508cecb00f183d1` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | Default Windows renderer to DX12 and persist GPU startup diagnostics | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/zero-phase-depth@20900fb3a794095e948e644cdbf5214b561251f0` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | Admit proven neutral saved phase modifiers and reject unsupported live writes | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/zero-phase-fixture@1e874ab13aeab5e93ee36e416c6725168181a271` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | Prove rejected phase setter executions through their distinct fallback records | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/zone-fixture-identity-c810@6e7ef7163bbb9077e2b9e71a4a06d7e32a5caee8` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | Expect stable zero-based source IDs in loop and release fixtures | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/zone-mapping-diagnostics@5ba3ef5f3898a360c71a9c35bf5149cf4a7988ee` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | fix(import): identify rejected zone mapping fields | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/zone-midi-next-validation@c810865cd14abf7c650abfc1e507adb6924b2bb1` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | Retain absent native flag in authored filter fixtures | normal merge refused: no merge base; forced add/add conflicts not measured |
| `codex/zone-skip-causes@ed9474009a49e224158d564795a527f8c86adeb3` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | fix(diagnostics): serialize typed zone skip counters | normal merge refused: no merge base; forced add/add conflicts not measured |
| `feat/decipher-readers-v1@4bffbb18b867b0a8e84b435d11684784b237693c` | unrelated history: obsolete whole-merge candidate; tip intent only, residual port value unverified | feat(mapping): add owned dry sample preview and record remaining parity gaps | normal merge refused: no merge base; forced add/add conflicts not measured |
| `feat/decipher-readers-v2@2fb8c926dd39bb7ac26a84d4806f42de14b6630e` | merged committed tip; 14 dirty files pending (see RE section) | Read UVI4 state frames, clear UFS names and bounded NI members | none (already ancestor) |
| `feat/inspect-daw-compatibility@0cb7a8a0b4d43086596a64c77320caa1b26d6d98` | merged committed v1 tip; 11 untracked specs pending (see RE section) | Merge pull request #33 from DerpcatMusic/fix/mui-visibility-browser-reveal-20261005 | none (already ancestor) |
| `fix/uvi-programs-play@082a10538c626cbd02c36816a0a51547735111a9` | valuable: covered key selection and release-tail repair | corpus-health: give scripted programs time to finish their own release samples | none individually; accumulated/semantic conflicts still possible |
| `fix/uvi-restore-order@0d9069377ed3b2247c145bc6ae8210a1c2ce8971` | obsolete duplicate of covered-key repair in fix/uvi-programs-play; do not merge twice | corpus-health: pick the covered key nearest middle C for scripted programs; CH_KEY override | C2 (1 paths) |

### C. Exact local mirrors (no additional work)

| Local ref @ full SHA | Remote mirror | Decision |
| --- | --- | --- |
| `codex/conflux-native-ui@4da706400d69a41d31cc0cec167144f8b6a5bdd0` | `origin/codex/conflux-native-ui` | identical SHA; inherit classification/conflicts from A |
| `codex/uvi-latest-integration@4bffbb18b867b0a8e84b435d11684784b237693c` | `origin/codex/uvi-latest-integration` | identical SHA; inherit classification/conflicts from A |
| `fix/analog-zero-output-gain@779c529101c5335d9a59e09c0bad437b417c6268` | `origin/fix/analog-zero-output-gain` | identical SHA; inherit classification/conflicts from A |
| `fix/latest-mui-20261004@1a1931dc904c6edb91c723ef9dfd70407d3ef55e` | `origin/fix/latest-mui-20261004` | identical SHA; inherit classification/conflicts from A |
| `fix/settings-keep-unknown@af77fb290b7a660dc8bf0e6bef7734f13f6bae64` | `origin/fix/settings-keep-unknown` | identical SHA; inherit classification/conflicts from A |
| `fix/v1-midi-ingress@ee4d07c9aff9bc3ddf339e336c99ccba531a5695` | `origin/fix/v1-midi-ingress` | identical SHA; inherit classification/conflicts from A |
| `fix/v1-terminal-retry-and-persistence-snapshot@0ab033aaee1495370c9a64b1939e9f8fa6442088` | `origin/fix/v1-terminal-retry-and-persistence-snapshot` | identical SHA; inherit classification/conflicts from A |

### D. Every measured individual conflict path

#### C1 — `codex/core-uvi-ui-verified-20261005`

- `Cargo.lock`
- `Cargo.toml`
- `README.md`
- `build.rs`
- `crates/sampler-core/src/dsp/convolution.rs`
- `src/access.rs`
- `src/articulate.rs`
- `src/artwork.rs`
- `src/audio.rs`
- `src/cache.rs`
- `src/creator.rs`
- `src/creator/nki.rs`
- `src/engine/ahdsr.rs`
- `src/engine/bank.rs`
- `src/engine/filter.rs`
- `src/engine/host_notes.rs`
- `src/engine/lfo.rs`
- `src/engine/mod.rs`
- `src/engine/native_state.rs`
- `src/engine/overrides.rs`
- `src/engine/params.rs`
- `src/engine/rack.rs`
- `src/engine/residency.rs`
- `src/engine/script.rs`
- `src/engine/stream.rs`
- `src/engine/voice.rs`
- `src/engine/wavetable.rs`
- `src/engine/zone.rs`
- `src/fx/blocks.rs`
- `src/fx/mod.rs`
- `src/fx/processor.rs`
- `src/fx/reverb.rs`
- `src/fx/tests.rs`
- `src/import.rs`
- `src/ksp/builtins.rs`
- `src/ksp/calls.rs`
- `src/ksp/compile.rs`
- `src/ksp/engine.rs`
- `src/ksp/mod.rs`
- `src/ksp/runtime.rs`
- `src/ksp/tests.rs`
- `src/ksp/ui.rs`
- `src/ksp/vm.rs`
- `src/lib.rs`
- `src/library.rs`
- `src/main.rs`
- `src/modulation.rs`
- `src/playback_audit.rs`
- `src/plugin.rs`
- `src/project_migration.rs`
- `src/resources.rs`
- `src/timing.rs`
- `src/ui/audit.rs`
- `src/ui/fitted.rs`
- `src/ui/header.rs`
- `src/ui/instrument.rs`
- `src/ui/keyboard.rs`
- `src/ui/menu.rs`
- `src/ui/mixer.rs`
- `src/ui/mod.rs`
- `src/ui/perf_view.rs`
- `src/ui/rack.rs`
- `src/ui/tests.rs`
- `src/ui/vector.rs`
- `tests/playback.rs`
- `vendor/ni-file/src/file_container/mod.rs`
- `vendor/ni-file/src/kontakt/objects/program_container.rs`
- `vendor/ni-file/src/nis/properties/subtree_item.rs`
- `vendor/ni-file/src/nks/container.rs`

#### C2 — `fix/uvi-restore-order`

- `tools/corpus-health/src/main.rs`

#### C3 — `origin/feat/universal-macos-installer`

- `.github/scripts/notarize_macos.sh`
- `.github/scripts/package_macos.py`
- `.github/scripts/package_macos.sh`
- `.github/scripts/test_package_macos.py`
- `.github/workflows/check_nightly.py`
- `.github/workflows/ci.yml`
- `.github/workflows/nightly.yml`
- `.github/workflows/publish_nightly.py`
- `CHANGELOG.md`
- `README.md`
- `tools/licenses.py`

#### C4 — `origin/fix/0.3-pointer-popup-eq`

- `CHANGELOG.md`
- `README.md`
- `SOURCE_COMMIT`
- `src/engine/filter.rs`
- `src/engine/params.rs`
- `src/ui/tests.rs`
- `vendor/mui-baseview/src/tests.rs`

#### C5 — `origin/fix/absent-nightly-refs`

- `.github/workflows/check_nightly.py`

#### C6 — `origin/fix/idempotent-macos-certificates`, `origin/fix/macos-signing-keychain`

- `.github/scripts/notarize_macos.sh`
- `.github/workflows/check_nightly.py`

#### C7 — `origin/fix/latest-download-policy`

- `.github/workflows/check_nightly.py`
- `.github/workflows/publish_nightly.py`
- `README.md`
- `docs/CI.md`

#### C8 — `origin/fix/latest-mui-20261004`

- `Cargo.lock`
- `Cargo.toml`
- `src/plugin.rs`
- `src/ui/mod.rs`
- `src/ui/picker.rs`
- `src/ui/picker/linux.rs`
- `vendor/mui-baseview/Cargo.toml`
- `vendor/mui-baseview/PATCHES.md`
- `vendor/mui-baseview/src/a11y.rs`
- `vendor/mui-baseview/src/lib.rs`
- `vendor/mui-baseview/src/tests.rs`

#### C9 — `origin/fix/macos-notary-phases-0.3.141`

- `SOURCE_COMMIT`

#### C10 — `origin/fix/macos-package-types`

- `.github/scripts/notarize_macos.sh`
- `.github/workflows/check_nightly.py`
- `README.md`

#### C11 — `origin/fix/per-format-nightly-manifests`

- `.github/workflows/check_nightly.py`
- `.github/workflows/nightly.yml`
- `.github/workflows/publish_nightly.py`
- `docs/CI.md`

#### C12 — `origin/fix/portable-license-preflight`

- `.github/workflows/ci.yml`
- `.github/workflows/nightly.yml`

#### C13 — `origin/fix/readable-installer-signatures`

- `.github/scripts/test_package_macos.py`

#### C14 — `origin/fix/v1-midi-ingress`

- `src/articulate.rs`
- `src/plugin.rs`

#### C15 — `origin/fix/v1-terminal-retry-and-persistence-snapshot`

- `src/ksp/runtime.rs`
- `src/ksp/tests.rs`
- `src/plugin.rs`

#### C16 — `origin/v2/block-voice-dsp`

- `Cargo.lock`
- `Cargo.toml`
- `crates/sampler-core/Cargo.toml`
- `crates/sampler-core/examples/render_workloads.rs`
- `crates/sampler-core/src/behavior.rs`
- `crates/sampler-core/src/bus.rs`
- `crates/sampler-core/src/control.rs`
- `crates/sampler-core/src/controller_event.rs`
- `crates/sampler-core/src/dsp.rs`
- `crates/sampler-core/src/dsp/control.rs`
- `crates/sampler-core/src/dsp/svf.rs`
- `crates/sampler-core/src/envelope.rs`
- `crates/sampler-core/src/gate.rs`
- `crates/sampler-core/src/lib.rs`
- `crates/sampler-core/src/note_event.rs`
- `crates/sampler-core/src/ownership.rs`
- `crates/sampler-core/src/performance.rs`
- `crates/sampler-core/src/plans.rs`
- `crates/sampler-core/src/prepare.rs`
- `crates/sampler-core/src/prepare/predicates.rs`
- `crates/sampler-core/src/prepare/selection.rs`
- `crates/sampler-core/src/release.rs`
- `crates/sampler-core/src/render.rs`
- `crates/sampler-core/src/resample.rs`
- `crates/sampler-core/src/schedule.rs`
- `crates/sampler-core/src/script.rs`
- `crates/sampler-core/src/source.rs`
- `crates/sampler-core/src/source/demand.rs`
- `crates/sampler-core/src/stream.rs`
- `crates/sampler-core/src/tests.rs`
- `crates/sampler-core/tests/behavior.rs`
- `crates/sampler-core/tests/buses.rs`
- `crates/sampler-core/tests/controllers.rs`
- `crates/sampler-core/tests/controls.rs`
- `crates/sampler-core/tests/modulation.rs`
- `crates/sampler-core/tests/note_stages.rs`
- `crates/sampler-core/tests/paged_render.rs`
- `crates/sampler-core/tests/release_selection.rs`
- `crates/sampler-core/tests/resample.rs`
- `crates/sampler-core/tests/script_arrays.rs`
- `crates/sampler-core/tests/support/mod.rs`
- `crates/sampler-core/tests/svf.rs`
- `crates/sampler-kontakt/Cargo.toml`
- `crates/sampler-kontakt/src/lib.rs`
- `crates/sampler-kontakt/tests/source.rs`
- `crates/sampler-ksp/Cargo.toml`
- `crates/sampler-ksp/src/lib.rs`
- `crates/sampler-ksp/tests/arguments.rs`
- `crates/sampler-ksp/tests/arrays.rs`
- `crates/sampler-ksp/tests/compile.rs`
- `crates/sampler-ksp/tests/controllers.rs`
- `crates/sampler-ksp/tests/controls.rs`
- `crates/sampler-ksp/tests/event_ids.rs`
- `crates/sampler-ksp/tests/functions.rs`
- `crates/sampler-ksp/tests/groups.rs`
- `crates/sampler-ksp/tests/select.rs`
- `crates/sampler-ksp/tests/stages.rs`
- `crates/sampler-midi/src/ingress.rs`
- `crates/sampler-midi/src/lib.rs`
- `crates/sampler-midi/src/mpe.rs`
- `crates/sampler-midi/tests/controller_dispatch.rs`
- `crates/sampler-midi/tests/ingress.rs`
- `crates/sampler-midi/tests/mpe.rs`
- `crates/sampler-native/Cargo.toml`
- `crates/sampler-native/src/main.rs`
- `docs/architecture-v2/ARTICULATION.md`
- `docs/architecture-v2/BUS_DSP.md`
- `docs/architecture-v2/MIDI_INGRESS.md`
- `docs/architecture-v2/MODULATION.md`
- `docs/architecture-v2/README.md`
- `docs/architecture-v2/RELEASE_CONTEXT.md`
- `docs/architecture-v2/RELEASE_SELECTION.md`
- `docs/architecture-v2/RESAMPLING.md`
- `docs/architecture-v2/STREAMING.md`
- `docs/architecture-v2/VOICE_DSP.md`

#### C17 — `origin/v2/gpt-format-fxmod`

- `crates/sampler-kontakt/src/effects.rs`

#### C18 — `origin/v2/library-access`

- `crates/sampler-uvi/src/bank.rs`

#### C19 — `origin/v2/midi-fixes`

- `Cargo.lock`
- `Cargo.toml`
- `crates/sampler-core/Cargo.toml`
- `crates/sampler-core/examples/render_workloads.rs`
- `crates/sampler-core/src/behavior.rs`
- `crates/sampler-core/src/bus.rs`
- `crates/sampler-core/src/control.rs`
- `crates/sampler-core/src/controller_event.rs`
- `crates/sampler-core/src/dsp.rs`
- `crates/sampler-core/src/dsp/control.rs`
- `crates/sampler-core/src/dsp/delay.rs`
- `crates/sampler-core/src/dsp/svf.rs`
- `crates/sampler-core/src/envelope.rs`
- `crates/sampler-core/src/gate.rs`
- `crates/sampler-core/src/lib.rs`
- `crates/sampler-core/src/note_event.rs`
- `crates/sampler-core/src/ownership.rs`
- `crates/sampler-core/src/performance.rs`
- `crates/sampler-core/src/plans.rs`
- `crates/sampler-core/src/prepare.rs`
- `crates/sampler-core/src/prepare/predicates.rs`
- `crates/sampler-core/src/prepare/selection.rs`
- `crates/sampler-core/src/release.rs`
- `crates/sampler-core/src/render.rs`
- `crates/sampler-core/src/resample.rs`
- `crates/sampler-core/src/schedule.rs`
- `crates/sampler-core/src/script.rs`
- `crates/sampler-core/src/source.rs`
- `crates/sampler-core/src/source/demand.rs`
- `crates/sampler-core/src/stream.rs`
- `crates/sampler-core/src/tests.rs`
- `crates/sampler-core/tests/behavior.rs`
- `crates/sampler-core/tests/buses.rs`
- `crates/sampler-core/tests/controllers.rs`
- `crates/sampler-core/tests/controls.rs`
- `crates/sampler-core/tests/modulation.rs`
- `crates/sampler-core/tests/note_stages.rs`
- `crates/sampler-core/tests/paged_render.rs`
- `crates/sampler-core/tests/release_selection.rs`
- `crates/sampler-core/tests/resample.rs`
- `crates/sampler-core/tests/script_arrays.rs`
- `crates/sampler-core/tests/support/mod.rs`
- `crates/sampler-core/tests/svf.rs`
- `crates/sampler-kontakt/Cargo.toml`
- `crates/sampler-kontakt/src/lib.rs`
- `crates/sampler-kontakt/tests/source.rs`
- `crates/sampler-ksp/Cargo.toml`
- `crates/sampler-ksp/src/lib.rs`
- `crates/sampler-ksp/tests/arguments.rs`
- `crates/sampler-ksp/tests/arrays.rs`
- `crates/sampler-ksp/tests/compile.rs`
- `crates/sampler-ksp/tests/controllers.rs`
- `crates/sampler-ksp/tests/controls.rs`
- `crates/sampler-ksp/tests/event_ids.rs`
- `crates/sampler-ksp/tests/functions.rs`
- `crates/sampler-ksp/tests/groups.rs`
- `crates/sampler-ksp/tests/select.rs`
- `crates/sampler-ksp/tests/stages.rs`
- `crates/sampler-midi/src/ingress.rs`
- `crates/sampler-midi/src/lib.rs`
- `crates/sampler-midi/src/mpe.rs`
- `crates/sampler-midi/tests/controller_dispatch.rs`
- `crates/sampler-midi/tests/ingress.rs`
- `crates/sampler-midi/tests/mpe.rs`
- `crates/sampler-native/Cargo.toml`
- `crates/sampler-native/src/main.rs`
- `docs/architecture-v2/ARTICULATION.md`
- `docs/architecture-v2/BUS_DSP.md`
- `docs/architecture-v2/MIDI_INGRESS.md`
- `docs/architecture-v2/MODULATION.md`
- `docs/architecture-v2/README.md`
- `docs/architecture-v2/RELEASE_CONTEXT.md`
- `docs/architecture-v2/RELEASE_SELECTION.md`
- `docs/architecture-v2/RESAMPLING.md`
- `docs/architecture-v2/STREAMING.md`
- `docs/architecture-v2/VOICE_DSP.md`

#### C20 — `origin/v2/semantic-ir`

- `Cargo.lock`
- `Cargo.toml`
- `crates/sampler-core/Cargo.toml`
- `crates/sampler-core/examples/render_workloads.rs`
- `crates/sampler-core/src/behavior.rs`
- `crates/sampler-core/src/bus.rs`
- `crates/sampler-core/src/control.rs`
- `crates/sampler-core/src/controller_event.rs`
- `crates/sampler-core/src/dsp.rs`
- `crates/sampler-core/src/dsp/control.rs`
- `crates/sampler-core/src/dsp/delay.rs`
- `crates/sampler-core/src/dsp/svf.rs`
- `crates/sampler-core/src/envelope.rs`
- `crates/sampler-core/src/gate.rs`
- `crates/sampler-core/src/lib.rs`
- `crates/sampler-core/src/lower.rs`
- `crates/sampler-core/src/note_event.rs`
- `crates/sampler-core/src/ownership.rs`
- `crates/sampler-core/src/performance.rs`
- `crates/sampler-core/src/plans.rs`
- `crates/sampler-core/src/prepare.rs`
- `crates/sampler-core/src/prepare/predicates.rs`
- `crates/sampler-core/src/prepare/selection.rs`
- `crates/sampler-core/src/release.rs`
- `crates/sampler-core/src/render.rs`
- `crates/sampler-core/src/resample.rs`
- `crates/sampler-core/src/schedule.rs`
- `crates/sampler-core/src/script.rs`
- `crates/sampler-core/src/source.rs`
- `crates/sampler-core/src/source/demand.rs`
- `crates/sampler-core/src/stream.rs`
- `crates/sampler-core/src/tests.rs`
- `crates/sampler-core/tests/behavior.rs`
- `crates/sampler-core/tests/buses.rs`
- `crates/sampler-core/tests/controllers.rs`
- `crates/sampler-core/tests/controls.rs`
- `crates/sampler-core/tests/lower.rs`
- `crates/sampler-core/tests/modulation.rs`
- `crates/sampler-core/tests/note_stages.rs`
- `crates/sampler-core/tests/paged_render.rs`
- `crates/sampler-core/tests/release_selection.rs`
- `crates/sampler-core/tests/resample.rs`
- `crates/sampler-core/tests/script_arrays.rs`
- `crates/sampler-core/tests/support/mod.rs`
- `crates/sampler-core/tests/svf.rs`
- `crates/sampler-ir/src/lib.rs`
- `crates/sampler-ir/src/units.rs`
- `crates/sampler-ir/src/validate.rs`
- `crates/sampler-kontakt/Cargo.toml`
- `crates/sampler-kontakt/src/lib.rs`
- `crates/sampler-kontakt/tests/source.rs`
- `crates/sampler-ksp/Cargo.toml`
- `crates/sampler-ksp/src/lib.rs`
- `crates/sampler-ksp/tests/arguments.rs`
- `crates/sampler-ksp/tests/arrays.rs`
- `crates/sampler-ksp/tests/compile.rs`
- `crates/sampler-ksp/tests/controllers.rs`
- `crates/sampler-ksp/tests/controls.rs`
- `crates/sampler-ksp/tests/event_ids.rs`
- `crates/sampler-ksp/tests/functions.rs`
- `crates/sampler-ksp/tests/groups.rs`
- `crates/sampler-ksp/tests/select.rs`
- `crates/sampler-ksp/tests/stages.rs`
- `crates/sampler-midi/src/ingress.rs`
- `crates/sampler-midi/src/lib.rs`
- `crates/sampler-midi/src/mpe.rs`
- `crates/sampler-midi/tests/controller_dispatch.rs`
- `crates/sampler-midi/tests/ingress.rs`
- `crates/sampler-midi/tests/mpe.rs`
- `crates/sampler-native/Cargo.toml`
- `crates/sampler-native/src/main.rs`
- `docs/architecture-v2/ARTICULATION.md`
- `docs/architecture-v2/BUS_DSP.md`
- `docs/architecture-v2/MIDI_INGRESS.md`
- `docs/architecture-v2/MODULATION.md`
- `docs/architecture-v2/README.md`
- `docs/architecture-v2/RELEASE_CONTEXT.md`
- `docs/architecture-v2/RELEASE_SELECTION.md`
- `docs/architecture-v2/RESAMPLING.md`
- `docs/architecture-v2/STREAMING.md`
- `docs/architecture-v2/VOICE_DSP.md`


### E. Dirty reader/FX inventory

| Modified path | Added / removed | Applies directly to audit head | Working diff SHA-256 |
| --- | --- | --- | --- |
| `crates/sampler-kontakt/src/effects.rs` | 189 / 29 | no; reconcile | `bc0facc8de9bf4f71ca9b673365a2bb9f08d3bacac3dcdcfb5e65d121b7a3577` |
| `crates/sampler-kontakt/src/library.rs` | 88 / 15 | no; reconcile | `ea6811287e39e4c6e742cc73a6c0e8cb0c01ca205617918898b1e4446ee05982` |
| `vendor/ni-file/src/kontakt/objects/header.rs` | 30 / 6 | no; reconcile | `0f33407cbc5f39700bc73f8eff1d62e6797455ad211650017cb21b9db4848646` |
| `vendor/ni-file/src/kontakt/objects/program.rs` | 797 / 84 | no; reconcile | `e39700018438a4cc7cd0554805c25ff8e2ce2b0bb9ee00895394eebb7aa91f12` |
| `vendor/ni-file/src/kontakt/objects/voice_group.rs` | 14 / 5 | yes (text only) | `2e8f2ccf2c6af2d0842af1c14a4656847d4c72ab6bc8e3bfececf5461ce651fd` |
| `vendor/ni-file/src/kontakt/objects/voice_groups.rs` | 218 / 37 | no; reconcile | `2ae386afcc96dba829eefa57a8520341c681d903af324ac74de2cda931ed0647` |
| `vendor/ni-file/src/kontakt/objects/voice_limit.rs` | 30 / 10 | yes (text only) | `ff9a85e7acca0e2ecee8fc65fcbd862e5893b3b50469d2791641b0da13ff437c` |
| `vendor/ni-file/src/kontakt/schemas/preset.rs` | 104 / 28 | yes (text only) | `fbf9086760ff15edc008582a35d2888d90ed6c1039776c988404f716ae542b7c` |
| `vendor/ni-file/src/nis/items/preset.rs` | 252 / 19 | yes (text only) | `74818b51f8ddf5134e15fba37e4f935ce5190df2b0e622c793566e54b71dfefa` |
| `vendor/ni-file/src/nis/mod.rs` | 4 / 4 | yes (text only) | `9c4e3b4f516ccf9178270e9a56345211aa3248fc1d9a7e2d497b756ecb941ccd` |
| `vendor/ni-file/src/nis/properties/bni_sound_header.rs` | 22 / 7 | yes (text only) | `6949f4ed0f0a45b408c0e266750e69e4b2ff048ebe431aff427e11136fe01339` |
| `vendor/ni-file/src/nis/properties/bni_sound_preset.rs` | 10 / 3 | yes (text only) | `fbd979a5c7b9cfb09a0cd41afe96dfc3e003969b908c224734f9a8ac85f6820b` |
| `vendor/ni-file/src/nis/properties/preset.rs` | 21 / 4 | yes (text only) | `a21bfcfcd490196b44c394e4d0e135bc434d5d5d16cfd089745857edbcc75fea` |
| `vendor/ni-file/src/nis/schemas/kontakt.rs` | 16 / 49 | yes (text only) | `43186fee85a55585a2cab0db8b2ef640ad076b9d2d2f88a784631704cf3b9cca` |

### F. Reproducible build/test results

First command: `~/.cache/kontakto-heavy cargo test --no-run`: exit 0; Cargo reported 36.31 s. It emitted the Copy-drop warning at `src/sound/v2.rs:1295`.

| Shard | Passed | Failed | Ignored | Wrapper wall s | Command following kontakto-heavy |
| --- | --- | --- | --- | --- | --- |
| kontakto | 198 | 0 | 9 | 105.46 | `cargo test --locked -p kontakto` |
| sampler-core | 339 | 0 | 1 | 63.21 | `cargo test --locked -p sampler-core` |
| sampler-ir | 9 | 0 | 0 | 0.53 | `cargo test --locked -p sampler-ir` |
| sampler-simd | 1 | 0 | 0 | 0.21 | `cargo test --locked -p sampler-simd` |
| sampler-pool | 4 | 0 | 0 | 0.27 | `cargo test --locked -p sampler-pool` |
| sampler-ksp | 119 | 0 | 0 | 4.27 | `cargo test --locked -p sampler-ksp` |
| sampler-kontakt | 60 | 0 | 5 | 100.21 | `cargo test --locked -p sampler-kontakt` |
| sampler-uvi | 54 | 0 | 6 | 4.73 | `cargo test --locked -p sampler-uvi` |
| sampler-midi | 47 | 0 | 0 | 0.72 | `cargo test --locked -p sampler-midi` |
| sampler-native | 49 | 0 | 10 | 2.64 | `cargo test --locked -p sampler-native` |
| sampler-ui-ir | 4 | 0 | 0 | 0.26 | `cargo test --locked -p sampler-ui-ir` |
| sampler-perf | 0 | 0 | 0 | 2.05 | `cargo test --locked -p sampler-perf` |
| kontra-native-host | 21 | 0 | 2 | 17.19 | `cargo test --locked -p kontra-native-host --lib` |
| corpus-health | 1 | 0 | 0 | 241.39 | `cargo test --locked -p corpus-health` |
| moose-mui | 19 | 0 | 4 | 11.19 | `cargo test --locked -p moose-mui` |
| moose-params | 61 | 0 | 4 | 0.46 | `cargo test --locked -p moose-params` |
| moose-derive | 9 | 0 | 3 | 2.9 | `cargo test --locked -p moose-derive` |
| moose-clap | 30 | 0 | 1 | 1.26 | `cargo test --locked -p moose-clap` |
| moose-vst3 | 30 | 0 | 0 | 0.87 | `cargo test --locked -p moose-vst3` |
| native-ignored-lib | 2 | 0 | 0 | 7.48 | `env WGPU_BACKEND=gl LIBGL_ALWAYS_SOFTWARE=1 xvfb-run -a cargo test --locked -p kontra-native-host --lib -- --ignored --test-threads=1` |
| native-ignored-x11 | 2 | 0 | 0 | 3.9 | `env WGPU_BACKEND=gl LIBGL_ALWAYS_SOFTWARE=1 xvfb-run -a cargo test --locked -p kontra-native-host --test x11_visibility -- --ignored --test-threads=1` |

The two native-host library ignored tests and two `x11_visibility` tests passed in an isolated Xvfb software-GL display; default library ignores remain visible in the table for accounting. The 12 vendor doctest ignores are documentation examples. Other default ignores were not run.

#### Every default ignored test

**kontakto**

- `sound::mics::tests::census ... ignored`
- `sound::v2::tests::idle_cost_of_a_loaded_instrument ... ignored`
- `ui::tests::editor_opens_while_parts_load ... ignored`
- `ui::tests::frame_cost ... ignored`
- `ui::tests::lag ... ignored`
- `ui::v2_tests::editor_with_a_real_instrument ... ignored, set KONTRA_UI_IR_PATCH to a locally owned instrument`
- `ui::v2_tests::ir_view_draws_the_ksp_corpus ... ignored, needs the extracted KSP corpus (kept outside the repo)`
- `ui::v2_tests::ir_view_real_instrument_memory ... ignored, set KONTRA_UI_IR_PATCH to a locally owned instrument`
- `uvi_mpe_response_per_bank ... ignored, survey`

**sampler-core**

- `measure_modulation_cost_per_voice ... ignored`

**sampler-kontakt**

- `library::census::mic_census ... ignored`
- `library::survey::survey_modulation ... ignored`
- `snapshot::probe::native_state_probe ... ignored, census`
- `survey_modulated_instruments_lower_and_render ... ignored`
- `survey_reported_features ... ignored`

**sampler-uvi**

- `survey::census_modules ... ignored`
- `survey::census_script_api ... ignored`
- `survey::census_scripted_render ... ignored`
- `survey::census_symbols ... ignored`
- `survey::survey_modulation ... ignored`
- `ufs::tests::reference_directories_match_independent_numeric_index ... ignored, requires the user's local UFS corpus and pinned reader image`

**sampler-native**

- `barbarian_cc1_sweep_probe ... ignored, probe`
- `full_note_runtime_trace_probe ... ignored, probe`
- `full_notes_match_kontakt_within_a_decibel ... ignored, known gaps: Una Cotton 5/11/11 dB low at vel 64/100/127, Barbarian 11 dB low; script-driven, Vista passes`
- `generated_maps_drive_like_their_keyswitches ... ignored`
- `probe_instrument ... ignored`
- `saved_state_probe ... ignored, probe`
- `una_script_vs_native_probe ... ignored, probe`
- `una_solo_probe ... ignored, probe`
- `una_velocity_sweep_probe ... ignored, probe`
- `mpe_response_across_the_corpus ... ignored, survey`

**kontra-native-host**

- `tests::native_accessibility_pre_show_close_and_reopen_release_the_model ... ignored, requires an isolated X11 display; run under Xvfb`
- `tests::native_surface_presents_and_reopens ... ignored, requires a live X11 display and graphics driver`

**moose-mui**

- `vendor/moose-mui/src/bridge.rs - bridge::Bridge<P>::bind (line 189) ... ignored`
- `vendor/moose-mui/src/bridge.rs - bridge::Bridge<P>::bind_as (line 218) ... ignored`
- `vendor/moose-mui/src/bridge.rs - bridge::Bridge<P>::bind_bool (line 286) ... ignored`
- `vendor/moose-mui/src/editor.rs - editor::MuiEditor (line 84) ... ignored`

**moose-params**

- `vendor/moose-params/src/types.rs - types::FloatParam::is_smoothing (line 334) ... ignored`
- `vendor/moose-params/src/types.rs - types::FloatParamReadF32 (line 355) ... ignored`
- `vendor/moose-params/src/types.rs - types::FloatParamReadF32::read_into (line 378) ... ignored`
- `vendor/moose-params/src/types.rs - types::MeterSlot (line 794) ... ignored`

**moose-derive**

- `vendor/moose-derive/src/lib.rs - derive_param_enum (line 3098) ... ignored`
- `vendor/moose-derive/src/lib.rs - derive_state (line 3262) ... ignored`
- `vendor/moose-derive/src/lib.rs - resolve_midi (line 73) ... ignored`

**moose-clap**

- `vendor/moose-clap/src/lib.rs - export_clap (line 5497) ... ignored`

#### Receipts and reproduction

Raw authored-code test/build logs and pinned JSON inventories are in `~/.cache/kontakto-audit-integration/`: `branches.json`, `ordered-merges.json`, `re-dirty.json`, `no-run.log`, `test-summary.json`, `extra-checks.json`, `clippy.log`, per-package logs, `gh-runs.json`, `latest-run.json`, `release.json`, `rulesets.json`, and the downloaded release manifest. These cache paths are local receipts, not repository dependencies. No decrypted library data was written.

To refresh safely: fetch in your own worktree; pin the new integration SHA; rerun ancestry and `git merge-tree --write-tree --name-only BASE TIP` for each shared-history ref; inspect each owner worktree with read-only `git status`/`diff --stat`; then run the table commands sequentially through the wrapper. Release checks use read-only `gh run list/view`, `gh release view/download`, `gh api` GET, and `readelf --version-info`/`--dynamic` on shipped binaries. Preserve the snapshot distinction when later branches move.
