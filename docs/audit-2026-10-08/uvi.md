# UVI/Falcon audit — 2026-10-08

## Verdict and reference identities

**Current shared-sweep revision:** `tools/kontra-scan@01178ba443e2b409c23282509f57d35a60753c36` (CLI cache environment/README follow-up to verifiedabf248cd), unchanged v2 binary SHA-256 `d4534838916e008d32a6e0763541a8bd9285651d77f130bad18fe926d0975f5a`, pinned v1 `870cea2140b5c9db5361831664966848302a2f57e82fcb6c7ed1b3534545ce5e`. The shared owner installed concurrent UVI J source026bdbb4; this audit verified both canonical sidecar and auditor alias SHA-256 `34a61e82ca6f09afc0eab692a1bf0b6765edbd7d063fccca94d95e699a93b9c1`. This supersedes earlier shared digests for future extension merges. Earlier smoke records below are historical revision-scoped evidence, not current complete census rows. Full1494 paired coverage remains pending; require matching actual notes and exclude fallback auditions for parity.

**v2 is worse than the UVI development implementation on `codex/uvi-latest-integration` in interactive UI, scoped live parameters, graph routing, persistence and identified verified DSP surfaces.** v2 has real protected-bank streaming, so “UVI disk streaming is missing” is incorrect. The shared Clarinet A smoke now proves audible output on the same declared native note in both, with lower observed v2 process RSS (140.36 vs418.60 MiB) and sample residency (26,175,632 vs218,923,526 B). Corpus-wide load/CPU/memory superiority remains unproved; scanner load stages differ. Admission is a v2 advantage in the stopped sample: its offline loader admits80/80 matched AO IDs while v1 rejects all80 before Ready. That is not native parity. Successful import, exported widgets and finite nonzero PCM are separate milestones; none establishes native Falcon parity.

Frozen integration under audit: `integrate/core-v2@7e82b152b46b31e8b9fd85c2ac01d01dad669ddd`. This audit branch adds only probes, original authored checks recovered from the prior audit, and this report. Product implementation was not changed. A fresh fetch verifies `origin/integrate/core-v2@be4c5c21928b462d7a604f4e07b05eac13931616`: no changes relative to the frozen revision in `crates/sampler-uvi`, `src/sound/v2.rs`, `src/ui/part.rs`, `src/ui/ir_view.rs` or the audited plugin snapshot path. The shared scanner glue changed; no runtime finding is claimed fixed by those merges.

References must not be conflated:

- `codex/uvi-latest-integration@4bffbb18b867b0a8e84b435d11684784b237693c`: v1's separate UVI implementation, examined read-only at `/home/derpcat/.codex/worktrees/kontakto-uvi-latest`.
- Local v1 `main@1f1836a156f96ee8e18652e771c49c2d6744cd4c`: no `src/uvi` tree or `uvi` Cargo feature. The development UVI branch adds 47 UVI files / 58,840 lines over it; that is scope evidence, not a quality metric.
- Requested benchmark `0cb7a8a0b4d43086596a64c77320caa1b26d6d98`: existing `kontakto-v1-gate/release/kontakto --version` reports this clean revision and features `clap,default,library-access,plugin,vst3`; `uvi-check` returns `Unknown command`. It cannot supply a UVI load/render A/B. The backed-up CLAP/VST3 binaries were preserved and not installed or loaded into a DAW.
- Fetched `origin/main@30953f9c3c55136901264721a19023517fe4dcf4` is already v2 (“KONTRA v2 batch 3”), not the requested v1 reference.
- `v2/gpt-uvi-audit@fb86ac57477dfef5ad1538aada7aa19ff2501e99`: report and opt-in conformance/census probes, **no runtime fixes**. Its local worktree HEAD `da3be219` differs from the pushed report branch. Recovered tests were read from that worktree; provenance is recorded separately from the pushed branch.
- `v2/gpt-uvi-ui@24c70a25031243d002e24c661c1afbb1408653aa`: unmerged UI implementation; measured independently, not merged into this audit.

The evidence supports different verdicts at different boundaries:

| Boundary | Verdict against v1 UVI development branch | Evidence/limit |
| --- | --- | --- |
| Interactivity and state | Worse | v2 installs0 controls in three production probes; v1 retains actual typed edits and eight real-library restored controls |
| Parameter/connection scope and DSP breadth | Worse in identified surfaces | v2 catalog0 typed descriptors and dropped scoped writes/shelves; v1 original-node routing, connection arrays and broader verified subset |
| AO admission | Better in matched stopped sample | v1 production Ready0/80; v2 supplementary offline loader80/80 on the same IDs; different adapters, no fidelity claim |
| Streaming architecture | Improved capability | v2 protected UFS streaming executes with bounded sample heads; v1 UVI preparation loads full PCM |
| Load latency / total RSS / CPU | v2 lower observed RSS in one matched smoke; corpus/CPU unproved | Clarinet A140.36 vs418.60 MiB; timing stages differ; full paired sweep pending |
| Native Falcon playback fidelity | Not achieved/certified |32 failed contract checks; no native PCM/gesture/transport reference captures |
| Capability vs stock v1 main/0cb7a8a0 | Added capability | Those revisions do not include UVI; they are not an equivalent UVI benchmark |

## Method and census

The storage mount was verified with `findmnt /mnt/MAIN_STORAGE`. All library access used the approved, hash-verified reader at `/home/derpcat/.codex/cache/kontakto-uvi-official-reader/app/UVIWorkstationx64.exe`, selected with `KONTRA_UVI_READER`, read-only. No Wine configuration, library payload, reader or other agent's worktree was modified. XML, Lua, fonts, images, audio and access state remained in memory; private output contains metadata and counts only.

Addendum 2 designates `~/.cache/kontra-scan/bin/{kontra-scan-v1,kontra-scan-v2}` and `results/{v1,v2}.tsv` as the only shared full-corpus scanner. They were absent when the addendum was read; the coordinator was notified and forwarded the required UVI columns. The first shared optimized scanner binaries and README became available with SHA-256 v2 `1d28987a6aa6afd089277221b1249561b478a9ae387181ee945e9df6ac4c919a`, v1 `4ab053cde8eb1197591cc3696ef38e99709a6ac52174c56fccc24f5596129caf`. Those initial digests are historical and superseded by Addendum4/the extension section below; use the current extended builds for corpus/before-after claims. The pinned v1 scanner manifest supports Kontakt only, consistent with the absence of UVI at its pinned revision. This audit stopped its independent serial census immediately and removed its standalone collectors. **The full installed census and matched v1/v2 load/play regression list remain pending the shared results.** Cached inventory is 26 banks / 660 programs: 620 Augmented Orchestra (AO), 40 VWinds. Do not extrapolate the AO-only sample to all 660.

Measurements already completed before that instruction are supplementary:

| Measurement | Frozen integration | Unmerged UI branch |
| --- | ---: | ---: |
| Same AO programs, matched against prior 239 baseline | 200 | 200 |
| Program reads successful | 200/200 | 200/200 |
| Lua init without error findings | 198/200 | 198/200 |
| Declared controls bound | 0/328,200 (0%) | 458,947/458,947 (100%) |
| Image references resolved | 0/36,253 | 0/88,000 |
| Font references resolved | Not exported | 0/598 |
| UI IR validation failures | 0 | 0 |
| Actual CPU layout/paint completed | Not measured | 0/200: tree budget error |

The integration matches the prior baseline exactly for those 200 IDs: 328,200 unbound controls and 36,253 failed images. Different branch control/image totals reflect changed extraction; 100% binding means declared controls have destinations, not that gestures or sound were validated. Both branches have initialization faults on two sampled programs despite successful import.

The branch whole-editor tree was measured using its optimized `ci/examples/uvi_ui_health` executable, SHA-256 `bf03ad218fa531b2e2ac634716cac3bf8fa4c6d2ab5c5554a9a9c3dc3e1855d6`; the owner confirmed clean `24c70a25` build provenance. Its probe uses the real production resource service, UI layout and CPU painter. Every measured branch whole-editor scene fails during layout, before painting; this does not describe the later frozen-baseline standalone authored-page scanner.

Supplementary paced offline render: 160/160 AO programs load, 138/160 exceed peak 0.0001, 48/160 stay at or below 0.001, zero nonfinite samples and zero stream underruns. Three initialization error findings occur in one program. Load p50 **1,316.0 ms**, p95 **2,971.9 ms**, maximum **6,989.6 ms**; maximum observed process RSS **210,223,104 B**, resident streamed sample heads **33,112,128 B**. These are sequential warm/cache-affected measurements, not isolated process peak RSS or matched v1 timing.

Each render uses 48 kHz/256 frames, CC1=100/CC2=100/CC11=127, median covered zone key, velocity100, 384 ms held +128 ms release. Median inline render time is 1.05% of audio duration, but **Marcato Ensemble takes 4,358.7 ms for 512 ms (851%)** with initialization faults. The offline driver calls Lua synchronously with its default generous budget; this is a repro candidate, not the worker's DAW CPU cost. Every other sampled render remains below 3.5%. PCM was inspected then discarded. The offline loader applies initialization overrides; production worker attachment does not.

The production loader/worker probe, `examples/uvi_audit_live.rs`, uses 48 kHz/64 frames with the same controllers and 512 ms horizon. Each case runs in a fresh process, but caches and machine load were not controlled:

| Program | Load ms | Init error findings | Controls installed | Peak | Max audio block ms | Deadline misses | Stream resident bytes |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| AO V Strings Bartok | 4,427.5 | 0 | 0 | 0.563 | 0.265 | 0 | 37,285,744 |
| VWinds Clarinet A | 1,120.7 | 1 | 0 | 0.174 | 0.458 | 0 | 26,181,648 |
| AO Marcato Ensemble | 4,221.1 | 1 | 0 | 2.434 | 1.245 | 0 | 37,984,304 |

All first exceed 0.0001 at frame64; no nonfinite samples or underruns were observed. Clarinet reports one silent-note outcome alongside audible PCM. The worker/audio adapter records zero runtime script overruns even on the two init-fault cases; this supports the diagnostic finding, not a claim of error-free Lua. Production is not equivalent to the offline initialization path, and the Marcato inline stall does not recur in this short production block measurement.

`audit_resources.rs` and `audit_structure.rs` are single-program metadata probes. No standalone collector is delivered. Supplementary metadata/check logs remain private at `~/.cache/kontakto-audit-uvi/`; aggregate counts and record SHA-256 values are in [uvi-measurements.json](uvi-measurements.json). Source timestamps and reader provenance accompany the branch measurements.

Historical opt-in conformance on frozen integration: **1 passes / 32 fail**; one inherited failure used an invalid ScriptData/onLoad fixture, corrected below. Do not treat the old result as33 certified native contracts. The exhaustive RE catalog probe reports **1,269/2,877 correct omitted defaults (44.1%), 0/2,877 typed numeric descriptors and 12/2,877 exact ranges**; failure is expected and opt-in. Root `cargo test --profile ci --no-run` passes, including the latest corrected Lua-budget-filter gate (13.24s compilation); the targeted `runaway_scripts_are_aborted_and_reported` check passes1/1; normal sampler-uvi library tests pass43/43 (six opt-in ignored). Checks do not decrypt installed sources to disk.

Counts below overlap; never sum them. “Observed” means measured, “exposed” means loading the affected shared surface/API, and native parity requires a reference-host comparison. All 200 sampled AO programs expose `setParameter`, `getParameter`, `getParameterConnections`, `onLoad`, `onSave`, `loadData`, `loadImpulse`, `loadSample`, `playNote`, `postEvent`, `waitBeat` and `uvi.ChordRec` in loaded modules. Exposure is not proof that the selected note executes every branch. All 200 have ScriptData and one ScriptProcessor.

### v1 UVI shared-scanner adapter

The coordinator assigned this audit the additional v1 UVI sweep because the pinned v1 scanner has no UVI implementation. It reuses the shared Python CLI, Rust main and metrics source; only the v1-UVI backend adapter differs. An owned worktree is based exactly on `4bffbb18`; its adapter source is committed on `audit/uvi-v1-scanner-20261008@1c198e60ff7b7018c54efb2986e3b6ae83a5abcb`. [v1-uvi-scanner.patch](v1-uvi-scanner.patch) contains the probe-only adapter and trace suppression; SHA-256 `aab3862db2065d793d7be982b019e3665d930c5ce4a3d8e173d6b15456aa4326`. In an owned4bffbb18 worktree, apply with `git apply --unidiff-zero /path/to/v1-uvi-scanner.patch`. Build: `kontakto-heavy cargo build --release --example kontra_scan --features shots,uvi`. The pre-note-plan executable SHA-256 is `9463ebd6b77d2a7c0137ee27b9540cba1b55decff429c0fff45173e832fbcda5`; committed source1c198e60 implements the shared note-plan contract. Its optimized executable SHA-256 is `d565661afb3ae1cab3100d1e83b7f12c0f5b02c51b7593451a6fe77929af8ea1`, built with `--release --example kontra_scan --features shots,uvi` in the owned UVI-v1 worktree; absolute executable path is recorded in the measurements JSON. Only owned probe targets were built; the scanner owner retains installs of `~/.cache/kontra-scan/bin/kontra-scan-v1`.

Pure `recover_content_state` is used in memory; `ensure_content_state`/save, optional PCM cache and diagnostics journaling are not used. It calls the real v1 `Worker`, snapshots, asset/font cache and `uvi_instrument::view` in Original mode, then CPU paints and sends fixed 256-frame audio packets. No source/art/audio/access records are saved. Its pixel digest is only a digest of the application's rendered output. This scanner instrumentation is not a product patch to merge into v2.

Witness Clarinet A: **loads and Original CPU paint succeed, 21/21 visible controls bound, zero image/font failures, 273 widgets /24 visible, 50,159,568 B decoded UI assets, 218,923,526 B resident PCM, peak RSS417.88 MiB, prepare1,708.6 ms**. v2 production installed0 controls and reports an init fault; the UI branch exports71 widgets /9 visible with22/23 resources resolving. Those measured differences substantiate the UI/state regression; the v1 image is not a native-host visual comparison.

The initial density-based note selection chosekey40 and emitted no signal. A separate covered-key-nearest60 witness with executable9463ebd6 picked `[60,64]` and **played audible finite PCM, peak0.0648209, load1,558.1 ms, peak RSS419.81 MiB**, Original21/21 bound; render CPU317,345,010 ns over512ms, two deadline misses, zero runtime errors. It retained218,923,526 B PCM. v2's Clarinet production case usedkey62/velocity100, so these are not matched CPU/PCM measurements. Do not call the oldkey40 silence a regression.

The stopped shared-CLI v1 run has **80/660 attempted**, all AO, frozen digest9463ebd6:80 program reads,0 Ready admissions,0 timeouts; all80 have graph-preflight rejections, with CombFilter/DiodeClipper/Drive/FeedbackMachine/Flanger/LFO/MS20/MultiLFO each present in80. `worker.rs:1828` rejects unsupported native graphs before preparing PCM, backed by `playback.rs:60`. All80 IDs match the supplementary v2 offline records, which admit80/80. This establishes an admission difference, not equivalent execution or a matched shared-scanner/native verdict. Failed v1 records never reach UI or audition; `ui:error` and `plays_note:no` must not be counted as independent UI/audio regressions. Their typed control/artwork capabilities remain unexercised. No whole660 projection is justified.

**Native-note source handoff:** `/home/derpcat/.t3/worktrees/KONTAKTO/audit-uvi-v1-bench/src/uvi/scan.rs` is the adapter; `src/ui/scan_uvi.rs` is its actual Original painter. The native accessor exists on v1 UVI4bffbb18: `UiSnapshot.root.key_colours: Option<BTreeMap<u8,String>>` (`src/uvi/host.rs:251`), populated from retained Lua root `keyColours` by `read_ui_key_colours` (`:509`). Use `Worker::ui_processors()`/`request_ui_snapshot(processor)`/`poll_ui_snapshot()` (`worker.rs:1089`,`:1104`,`:1126`), matching reply request/activation. These are control-thread APIs. `src/ui/keyboard.rs:58` recognizes `#00FFFFFF` as explicit valid; `#00000000` means explicit invalid. Other colours are highlights; missing or conflicting declarations are unknown. The displayed low/high span is not a contiguous-range promise. The probe reuses `plugin/uvi_ui.rs:17`'s conservative merge, with visibility changed only for probe reuse.

The adapter exports `native_valid_keys`, `native_key_conflicts`, `native_preferred_note`, and `programs[0].pick`/`pick_source` (`shared-note-plan`, `native-valid-keys` or `active-zone-nearest60-fallback`). Preference intersects explicit valid keys with nonbypassed/nonpurged zone coverage at velocity64, then picks nearest60; without a qualifying declaration it uses its recorded zone-based fallback. Authored validity cannot prove that a default script actually sounds a note. `metrics::note(0)` from shared scanner source overrides that preference via `KONTRA_SCAN_NOTE_PLAN`; both audition paths must consume the same persisted `{programs:{"0":{key,velocity}},policy:"native-valid-keys-v1"}` plan. Baseline v2 ScriptHost exports **no keyboard metadata**; the persisted plan bridges picks without pretending it does. Sidecar `cargo test --profile ci --features shots,uvi --no-run` passes (2m13s), existing native key merge/reset tests pass2/2, and source branch is pushed. The sweep is paused for the scanner owner's rebuild and **v2-before-v1** note-plan ordering. The scanner owner will install it as the adjacent `bin/kontra-scan-v1-uvi` sidecar for `::` IDs, preserving pinned0cb7a8a0 Kontakt; its base is the separate UVI branch4bffbb18. The owner installs the shared binaries; this audit handed over source and did not overwrite them. Private stopped results remain `~/.cache/kontakto-audit-uvi/v1-scan/results.tsv`; canonical paired/all660 counts are pending.

Committed-sidecar witness: native preference `[60,64]`, shared plan `[62,64]`; actual `programs[0].pick=[62,64]`, `pick_source:shared-note-plan`,26 explicit valid keys (noncontiguous), zero conflicts, Original21/21 bound, peak0.0540957 with zero nonfinite samples, load1,593.6 ms. This proves the hook overrides native preference on the production worker; it does not compare native Falcon PCM. Runnable metadata check: `python3 docs/audit-2026-10-08/check-uvi-note-plan.py ~/.cache/kontakto-audit-uvi/note-plan-witness/result.json ~/.cache/kontakto-audit-uvi/note-plan-witness/plan.json`.

## Ranked findings

### 1. P0 — interactive controls have no UVI edit destination

Population: observed 200/200 AO programs, 328,200 controls; full installed exposure awaits shared scanner. The earlier baseline had **0/392,199 controls bound across239/239 programs**. Current integration exports `Binding::Variable` (`crates/sampler-uvi/src/script/ui.rs:95`), its worker `Message` has no UI edit (`scripted/thread.rs:28`), and production installs `controls: Vec::new()` (`src/sound/v2.rs:1120`). A knob can be painted while its Lua `changed` callback is unreachable. The integration census verifies the actual exported binding counts.

**Fix — merge, M:** bring `dcbc07c1` and `24c70a25` from `v2/gpt-uvi-ui`, preserving its worker edit/snapshot service and native mapper identities. Touch `script/ui.rs`, `script_prelude.lua`, `scripted/thread.rs`, UI IR, `src/sound/v2.rs`, `src/ui/ir_view.rs` and plugin state as in that branch. Do not rebuild this work independently. The branch census demonstrates control-binding repair, not audio-parameter parity; findings2–8 still need attention.

v1 advantage: `src/uvi/host.rs:958` applies typed edits with range checks to the actual VM-owned widget, and `worker.rs:3620` onward tests admission and callbacks. `UVI_VWINDS_PERFORMANCE_EVIDENCE.md` records nine real-library performance tapes and eight restored control values, with explicit native-fidelity and MPE limitations.

### 2. P0 — authored artwork/font resolution still fails after control-binding repair

Population: observed 200/200 AO programs, 88,000 failed images and 598 failed fonts on the UI branch; full census pending. Earlier integration baseline: **0/43,207 image references resolved across239/239 programs**. Frozen frontend `src/ui/pictures.rs:10–14` constructs only `sampler_kontakt::Resources`, and `:20` reads UVI assets through that service. This is an independent frontend authority bug: the shared Clarinet smoke has40/40 lookup failures even though the direct bank helper resolves39/40. A direct `Bank::resource` probe is not the production Original resource path. The UVI bank helper also reuses the audio resolver (`bank.rs:168`, relative-only `resource` at`:300`). The UI branch adds `ui_resource` (`bank.rs:169`) and native font/image loading, but the fresh branch executable still reports resource failures; the census table separates resource lookup from decode/font parse.

**Fix — M:** merge the existing resource service, then preserve each script resource base while resolving authored relative paths in `bank.rs::ui_resource`/`resource_base` and `resources.rs`; keep exact bank authority and ambiguity rejection. Verify direct member identity, qualified-volume handling, root-relative vs script-relative paths and @2x variants using metadata-only diagnostics. A resolver declaration is insufficient; require successful lookup, image decode and font parse on real programs. Baseline single-program diagnostics show all182 AO image references contain parent traversal segments and fail preset-relative/direct-suffix lookup, yet180 have one matching bank basename and2 have two; while39/40 Clarinet references load via preset-relative resolution; qualified-bank mismatch is not observed in either case. The AO script-relative resource base is lost; falling back to normalize(path) independently rejects those parent segments before suffix matching (`v2/gpt-uvi-ui:bank.rs:174`). Preserve the owning script location and rooted path rather than accepting arbitrary basenames; the two ambiguous names demonstrate why basename fallback is unsafe. On clean UI revision24c70a25, Bartok exports448 assets (445 images, three fonts): all contain parent segments, all fail resolution;443 have a unique bank basename,4 have two,1 has none. That independently confirms the broken base/fallback path on the UI branch. A blanket all-library missing-art claim would be wrong. Clarinet A on the same clean branch resolves22/23 requested assets and has9 visible widgets /71 total, parent depth1; its remaining font/member is absent under basename lookup. This is a different failure population from AO. The UI owner independently confirmed the executable hash/revision and had not established the asset failure cause; initially the node/depth split was unknown; the single-program scene probe now resolves it for Bartok.

v1 advantage: `src/uvi/ui_assets.rs:59` uses UVI-owned `Library::data` resource authority; dedicated assets, fonts and panel refresh exist. This is source capability evidence, not a new all-program render result.

### 3. P0 — Original is not the default authored interface

Population: **200/200 sampled integration AO interfaces** have an empty UI unsupported list, so `src/ui/part.rs:59` selects `Presentation::Vector` on load. This is the explicit Addendum 2 product violation. The condition uses extraction warnings as a display policy; it does not honor the user's Original-by-default requirement. On the UI branch all sampled faces have warnings and therefore happen to select Bitmap, but that conditional behavior is not a durable default policy.

The same view checks interface Arc identity at `part.rs:54` and reconstructs the Face when it changes; a property snapshot can reset the selected mode, page and assets. The UI-loop owner independently traced that publication/reset route. This report attributes the default directly to the source and collected unsupported lists; it does not claim a pointer-level toggle repro on scenes that never paint.

**Fix — S/M:** make Original/Bitmap the unconditional initial mode in `src/ui/part.rs`; retain selected presentation/page by instrument source identity across property snapshots, and treat explicit user changes as state. Merge/reconcile the shared UI-loop owner's work when available. Test load default and Original persistence after one widget property update. This is shared frontend scope, not another UVI engine fix.

### 4. P0 — live engine writes are largely Lua-only overlays

Population: observed200/200 sampled AO initializations call the unmodeled SamplePlayer.Pitch, Layer.PlayMode and Layer.PortamentoTime writes; prior unversioned corpus reported657/660, which is not a current count. `script.rs:604` accepts numeric Gain/Pan for Program/Layer/Keygroup and Program.Polyphony only. `script_prelude.lua:236` stores everything else in `__set` and reports it. Insert metadata locations exist (`inserts.rs:17`), but no live insert write command consumes them. SamplePlayer.Gain/Pitch, filter cutoff/bypass, effect controls, layer mute and sends therefore cannot follow the authored performance logic.

The offline loader applies `insert_overrides()` before translating (`lib.rs:1429`), while production translates first and calls `attach_script` without patching (`src/sound/v2.rs:1097–1102`). Thus a successful offline probe can be materially better than the installed live path.

**Fix — L:** add typed, original-node-to-runtime binding destinations in `script.rs`, `scripted.rs`, `inserts.rs` and core parameter services; apply the same initialization/live semantics in both loaders. Reuse `InsertNode` and existing IR/core setters; support enabled/dormant processors when scripts toggle bypass. Promote `oscillator_gain_emits_an_engine_write`, then run airflow/filter/FX UI gestures with PCM/control readback. v1's `host.rs:1242` emits scoped `Action::Parameter`; `playback.rs:2469` consumes original-node changes rather than just updating a Lua mirror.

### 5. P0 — keygroups alias their layer; stacked layer writes affect oscillator1 only

Population: multi-keygroup/stacked-layer programs exposed; topology probe confirms 388/388 stacked keygroups for Bartok and 93/179 for Clarinet A; whole-corpus topology count pending shared scanner. `script.rs:396` gives every keygroup its layer scope. `scripted.rs:376` chooses just the first `(layer,osc=1)` group; other oscillators retain their authored values. The driver's value is authored-relative (`scripted.rs:388`) while the shared core stores absolute group overrides; this also needs a measured gain/pan contract, not merely a wider loop.

**Fix — M:** retain independent Program/Layer/Keygroup/Oscillator identity and exact destination sets in `script.rs`, translator `OscGroup`/node mapping and `scripted.rs::parameter`; apply core absolute/relative laws consistently. The authored checks `keygroup_writes_keep_distinct_scopes` and `driver_noteoff_closes_original_physical_note` separate scope and ownership failures. Add a two-keygroup/two-oscillator audio isolation check before enabling broad writes. v1's node-identity table and `getParameterConnections` (`host.rs:1190–1303`) preserve separate elements.

### 6. P0 — definitions/defaults/types and real connections are replaced by invented values

Population: all programs loading the shared parameter surface; exhaustive catalog probe covers167 element types/2,877 definitions. `script_prelude.lua:217` emits string IDs and generic0..1 ranges with no types; `getParameter` returns0 for omitted XML attributes (`:230`). `script.rs:643` converts serialized booleans to numbers, making0 truthy in Lua. `getParameterConnections` returns a fabricated truthy object at every numeric index (`script_prelude.lua:249`), even when no connection exists. Scripts can therefore branch incorrectly before any audio command.

**Fix — M:** use the RE catalog as typed descriptor input, retain numeric IDs and authored/default values, expose actual ordered connection nodes and mapper/bypass/ratio values. Touch `script.rs`, `script_prelude.lua` and modulation/node identity translation. Run `uvi_audit_catalog`; handle36 catalog rows with reversed Min/Max as descriptor evidence requiring native semantics, not blind clamping. The official [Element contract](https://lua.uvi.net/class_element.html) requires typed values and definitions. v1 implements `hasParameter`, typed original-node writes and real connection arrays, but its own retained-parameter coverage is not equivalent to all2,877 native descriptors.

### 7. P0 — custom state is not restored or saved by integration

Population: 200/200 sampled AO programs have automatic ScriptData and expose onLoad/onSave. This proves automatic widget-persistence exposure, **not200 custom `<state>` payloads**; their custom-state population is unknown. `lib.rs:428` initializes behavior state empty, `script.rs:536` retains automatic ScriptData/widget values, and `load_scripts` calls widget restore/onInit but no custom onLoad (`script.rs:943–970`). There is no integration host save/restore service. A preset's custom sequencer/voice-selection data can differ even when widget export succeeds.

**Fix — merge + M:** merge the UI branch's automatic-widget/custom-state snapshots and plugin source identity checks, then validate native ordering and lexical ScriptData decoding against the RE spec. It fixes the authored widget persistence/Table cases, but that alone is not proof of real native custom state or restored ScriptData semantics. Touch `script/ui.rs`, prelude, script worker, plugin state and load adapters. v1 `src/uvi/script.rs:1464`,`:2158`,`:3084` has onLoad/onSave routes and real-library eight-control restore evidence. Its custom data is JSON in a direct ScriptProcessor `<state>` child (`script.rs:1300`,`:2140`; `host.rs:2138`,`:2284`), not automatic `<ScriptData>` attributes. Scalars live on ScriptProcessor attributes; table-cell vectors live on ScriptData attributes. The inherited `custom_scriptdata_invokes_onload` probe wrongly required onLoad for automatic-only data. It is replaced by `custom_saved_state_invokes_onload` with synthetic JSON `<state>{"counter":42}</state>` and an automatic-only negative check. The targeted pair is1 pass/1 expected failure: automatic-only data correctly does not call onLoad; actual custom `<state>` still does not invoke it on frozen integration. v1 initial load calls custom onLoad before widget restoration and onInit (`script.rs:2133`), while explicit loadState restores widgets/callbacks then onLoad, without rerunning onInit (`host.rs:2060`,`:2166`). These are v1 implementation/retained-observation facts; do not collapse the two lifecycle paths or infer a universal native order from them. [UVI callbacks](https://lua.uvi.net/group___event_callbacks.html) distinguish custom data from automatic widget persistence.

### 8. P0 — FX/sends/modulation graphs remain partially discarded

Population: 200/200 sampled AO programs carry active CombFilter, Drive, Gain, Maximizer, OnePole, SampledReverb, SparkVerb, ThreeBandShelves and XpanderFilter inserts; DualDelay/FeedbackMachine appear in199/200. Use counts by programs, not element multiplicity. Scope is checked separately on two programs. The old `FALCON_MODULE_COVERAGE.md` “all inserts dropped” statement is stale: integration now lowers Gain, stereo GainMatrix, OnePole, DigitalEq, bus convolution, partial SampledReverb and parallel EffectRack branches (`inserts.rs:57–238`). Nevertheless:

- Program inserts execute only Gain (`lib.rs:402–412`); enabled Program-level FX are skipped even if `inserts.rs` supports that type elsewhere.
- Program/Layer connections are explicitly reported and dropped (`lib.rs:384`). Effect-parameter modulation does not become live FX control.
- Keygroup BusRouters are not included in the layer-send loop (`lib.rs:489`), so authored routing is flattened.
- GainMatrix uses only its first stereo2×2 submatrix (`inserts.rs:67`), not the authored up-to12-channel graph.
- Convolver/SampledReverb are accepted only when `!voice` (`inserts.rs:175`); unmodeled types fall through. OnePole key tracking uses keygroup midpoint (`:153`); DigitalEq assumes band numbering and loses some native laws; reverb Time/PreDelay/Width/damping are reported rather than rendered.

Active stored insert population in the sampled 200 AO programs (scopes combined; activation does not prove the routed mix is nonzero):

| Kind | Programs | Integration consumption gap |
| --- | ---: | --- |
| ThreeBandShelves / Maximizer | 200 each | Bartok confirms enabled layer/program shelves and program maximizer; no corresponding lowering |
| CombFilter / Drive / SparkVerb / XpanderFilter | 200 each | Unsupported insert kinds; native DSP absent |
| DualDelay / FeedbackMachine | 199 each | Unsupported time/feedback processors |
| SampledReverb | 200 | Partial bus convolution; time/width/predelay/damping absent |
| WaveShaper | 11 | Partial rectifier modes only; do not call all eleven dropped |
| DiodeClipper / MS20 | 4 each | Unsupported active cases |
| Flanger / Phasor | 3 / 2 | Unsupported active cases |

Clarinet A confirms 10 active Convolvers, 6 EffectRacks, 9 aux +90 keygroup GainMatrices and 4 TrackDelays. Existing static convolution/parallel lowering must be retained; graph location, microphone channels and runtime IR/control changes remain the gaps. A generic `unsupported.module` row also lists wrapper/recognized nodes and is not by itself proof that every such module was dropped.

**Fix — L:** preserve graph scope/routing and use existing v1 owned DSP laws as porting evidence (independent source is already in the repo), with native measurements retained as limits. Prioritize active program counts from the table, then model Program insert chains, node destinations, keygroup sends and channels before adding more kernels. Touch `lib.rs`, `inserts.rs`, modulation, IR chain/routing and core DSP. Do not report a bypassed stored effect as an audible default failure.

v1 advantage: `src/uvi/effects.rs:37` supports ThreeBandShelves and native-tested EQ/IR behavior; `time_effects.rs` supports authored delay branches; `playback.rs:1498` and`:1747` route effective parameter changes. v1 explicitly rejects several unverified CombFilter/MS20/Flanger/Drive/Diode/MultiLFO cases in `preflight` (`playback.rs:212–288`), so its file inventory is not a claim that all those modules execute.

### 9. P0 — note identity/release and event forwarding violate the host contract

Population: all scripted-note programs exposed; no claim that every default note reproduces every defect. Generated IDs start at 0 (`script.rs:498`), physical IDs at1 (`scripted.rs:184`), and both share `Driver.notes`. Repeated same-key inputs also lose ownership because releases are keyed by note number. `postEvent` creates a new ID; delayed postEvent reserves a different ID from the eventual play (`script_prelude.lua:278`). Fade/release then address the wrong logical voice.

CC/bend/pressure packets are delivered to Lua **and** directly to the engine (`src/sound/v2.rs:388`), preventing suppression and allowing forwarded events to be doubled. Lua notes are always called with channel0 in the driver and worker (`scripted.rs:61`, `thread.rs:101`), dropping physical expressive channel identity. The official [callback forwarding rules](https://lua.uvi.net/group___event_callbacks.html) require explicit forwarding when a handler exists.

**Fix — M:** disjoint/stable note identities, physical input tuples including channel/port, ordered per-processor routing and callback-owned forwarding; touch `script.rs`, prelude, driver/worker and `src/sound/v2.rs`. Preserve shared core note ownership. Promote the identity/repeated-note/event-field checks and test overlapped repeated keys, channels, pedals and postEvent return IDs with PCM. v1 has broader node/worker ownership evidence but its MPE parent-context failure remains open.

### 10. P0 — unmerged UI branch whole-editor AO tree exceeds the layout budget

Population: observed 200/200 AO whole-editor scenes on `v2/gpt-uvi-ui@24c70a25` fail layout. This is not the frozen7e82 authored-page Bitmap scanner: its AO authored pages paint successfully while182 resources are missing. Root trees and extraction baselines differ. Report a budget hit only when the actual renderer returns a budget error; missing artwork and a successful standalone page paint do not imply one. UI IR validation success and bound controls are not paint success. The CPU collector reaches the actual renderer (`v2/gpt-uvi-ui:src/ui/ir_view.rs:520`) and reports `the tree exceeds its node or depth limit`. MUI's default layout budget is4,096 nodes/64depth (`mui-layout/src/lib.rs:202`), enforced by incremental scan (`incremental.rs:225`). The clean UI branch Bartok probe exports10,145 widgets, **6,481 visible on page0**, with maximum widget parent depth4. The renderer builds at least one scene element per visible widget (`ir_view.rs:217`);6,481 exceeds the4,096 node ceiling even before control wrappers. This proves node count is sufficient to cause this case; parent-depth overflow is not needed. Correcting asset paths alone cannot make this scene paint.

**Fix — M:** inspect visible-widget/expanded-node counts, repair parent visibility/layout first, build only the visible scene and measured viewport contents in `script/ui.rs`/`src/ui/ir_view.rs`. Preserve the safety budget; increase it only with evidence for both scene size and frame costs. Add one real-AO scene plus one VWinds scene to the branch CPU paint probe, then pointer/keyboard and drag/reset checks. This is a blocker remaining after the existing UI merge, not a reason to reimplement its binding service.

### P1 — worker runtime failures and scheduling/context are not exposed faithfully

Population: scripted hosts; 200/200 sampled AO module sets expose waitBeat and event APIs. Worker findings are copied only at initialization (`scripted/thread.rs:90`) and not published afterwards. Historical `full3.jsonl` had307/660 zero-voice records, all with empty script faults and null UVI selection diagnosis; those records cannot identify the current cause. Unbounded silence claims based on them are invalid.

`Driver` ignores Play/Change/Fade timestamps (`scripted.rs:311–339`); worker polling is1ms (`thread.rs:23`). Production `V2Core::begin_block` is a no-op (`src/sound/v2.rs:555`), so actual tempo/song position/meter/transport do not reach Lua. Offline scripted loaders retain48kHz `Config::default()` at other rates (`lib.rs:1429`), while `attach_script` also does not replace config.rate with its supplied rate (`:995`). Spawn ordering and inherited note contexts have independent authored failures.

**Fix — M:** bounded runtime findings/snapshot updates, explicit late/overflow counters, timestamped core commands and real block transport/sample-rate propagation; touch worker, driver, host and v2 block adapter. Feed failures into the load/runtime report and corpus UVI rig. Promote clock/FIFO/note-held checks; measure64-frame worker latency and tempo changes against native host traces. Do not count a failed script as a successfully playing loaded instrument.

## Additional gaps and evidence limits

- Async sample/impulse/data/state operations are inert; their completion callbacks are swallowed (`script_prelude.lua:40`). `uvi.ChordRec` is a stub, undefined globals may become truthy inert objects (v2 prelude:32 invents a stub for any unassigned name; no `GlobalPanel` export was found in v1 UVI source, which does not establish a native-host export), and multiple/bypassed ScriptProcessors share one callback namespace. See authored conformance results and supplementary exposure/topology counts above. Fix async completion failures and actual bank authority rather than inventing a successful task. v1 `host.rs:2322`,`:2433` has resource-task routes.
- Fade layer selection, `change*` immediate smoothing and `setSampleOffset` are discarded; first prioritize the40 VWinds programs exposing offsets and observed voice-manipulation failures. They need scoped sample/voice commands and native timing tests, not only more Lua globals.
- Knob vertical drag/modifier fine/reset, wheel ownership, Original/Vector mode switching and frontend responsiveness are shared-UI integration tasks. UVI integration has no edit route, so those gestures cannot be certified as functional. The UI branch includes normalized mapper/reset behavior and momentary-button tests, but real-library scene painting must work before gesture claims. Conflux is Kontakt and outside this UVI census.
- SampleMappingOscillator/DMAP dimensions, MinBlepGenerator, file/drop/WaveView and exported host automation are Falcon breadth requirements outside the installed sample-player corpus. Do not infer full Falcon support from these660 presets. The RE groundwork's two official examples exercise exactly those missing boundaries; its earlier25-bank/5.8GB header inventory is historical and does not supersede the current mounted inventory/shared census.

## Existing work to integrate

| Branch/revision | Work already available | Audit disposition |
| --- | --- | --- |
| `v2/gpt-uvi-ui@dcbc07c1` | Widget edit route, presentation snapshots, resource service, float/bool/Table/XY semantics, custom/widget state, native artwork/font support | Merge existing implementation; census identifies remaining asset/paint/engine defects |
| `v2/gpt-uvi-ui@24c70a25` | Native mapper positions and constructor defaults for reset | Merge with the preceding commit; authored binding success is not DSP parity |
| `v2/gpt-uvi-audit@34979dae`, `fb86ac57` | Original conformance checks, catalog/corpus probes and adversarial report | Reuse/promote checks; no runtime fix exists to merge |
| Integration `4c505003`, `e9b9d3ee`, `c2912175` ancestry | Static aux/keygroup inserts, InsertNode map, parallel EffectRack branches | Already present in frozen integration; correct stale coverage docs, do not reimplement |
| `codex/uvi-latest-integration@4bffbb18` | v1 UVI node identity, typed controls, state/resource tasks, module DSP and retained reference evidence | Port verified semantics into v2 boundaries selectively; do not merge the whole old architecture |

## Required next measurements

1. Matched v1-UVI/v2/native host: same exact bank/member, controller/UI state, rate, block size, cold/warm cache policy, playable keys, sample quality and polyphony. Capture prepare-to-ready and first-audible latency, process peak RSS, resident PCM/page/IR/UI bytes, worker+audio CPU time, blockp50/p99/max, late commands and underruns. Existing pinned v1 benchmark lacks UVI; use the v1 UVI adapter described above and consume the shared v2 records. Require matching note picks for audio regression claims. Native reader decoding is not native playback.
2. For40 VWinds presets: airflow sweeps, overlapping same-key/different-key legato, fast/slow glide, pitch-bend modes, three vibrato modes, mic mixing, EQ/room and state reload. Compare envelopes, spectra and aligned pitch trajectories, then listen. Retain reference PCM only where authorized by the coordinator's data rules; this audit saved none.
3. For620 AO presets: actual arpeggiator/step/harmonizer controls, transport/tempo changes, multiple oscillators and layer/FX modulation. Repeat silent/near-silent cases over several covered keys/velocities and authored state. Do not attribute silence to Lua from one default-key test.
4. UI: repeat239 matched IDs separately from all660, consume shared lookup/decode/font/layout/painter results and request visible/expanded node counts from its owner; then test edited callback outcomes, engine readback, gestures, automation and persistence. Capture frame times only once a real scene paints.

## RE sources used

Read fully, read-only: `t3code-80fe786b/docs/FALCON_FORMAT_GROUNDWORK.md`, `FALCON_RUNTIME_UI_GROUNDWORK.md`, and relevant `DSP_FORMAT_SPECIFICATION.md`/catalog sections. The groundwork explicitly distinguishes metadata, executable module support and native fidelity. Its event forwarding, processor scope, widget/custom-state ordering, typed definitions, async ownership and graph distinctions informed this audit. Its2026-10-02 header-only inability to decode banks is historical; the current reader and fresh program census supersede that access boundary, not the behavior requirements.

Prior-agent reports: `v2/gpt-uvi-audit:docs/architecture-v2/UVI_AUDIT.md`, `v2/gpt-uvi-ui:docs/architecture-v2/UVI_UI_REPORT.md`, integration `UVI_SCRIPT_COVERAGE.md`, `UVI_LUA.md`, `FALCON_MODULE_COVERAGE.md`, `PRODUCT.md`, and v1 `UVI_VWINDS_PERFORMANCE_EVIDENCE.md`, `UVI_LOADING_ORDER_EVIDENCE.md`, `UVI_ENGINE_FOUNDATIONS.md`. Old status tables were checked against current source instead of treated as authoritative completion claims.

## Shared extension and diagnostic accessor handoff

The census owner pushed `tools/kontra-scan@9dcf05e59cbc67e45c56872e1418817693150085` and installed the extended common CLI. Observed digests: v2 `18fe63fe07e62ea3c012ce29fc2d01b08f518ea808459446478cc46aca924b6d`; pinned-v1 scanner `a4b3f8c76483ea06b36b9fef46911093d7e8df8021bd9e54e711018dbf700399`; CLI `cd472de4950506591f3a709eea19116445a1d41cbb2a9b0fb58267f41c7c5f93`. UVI sidecar remainsd565661a/base4bffbb18/source1c198e60. On explicit coordinator instruction this audit also installed the identical adapter as `bin/kontra-scan-v1uvi` and added its README section. It prepared a235-second-shard runner gated on completed v2 records/plans, with synthetic metadata checks. That runner was stopped when the census owner confirmed its paired1494-ID sweep already includes UVI-v1 through the sidecar; no duplicate library sweep is needed. Canonical detailed records are `results/{v1,v2}/cache`, with TSVs at `results/{v1,v2}.tsv`. The first partial extension observation has3 v2 detailed rows,2 AO UVI,0 v1 rows: both AO examples load/play, paint Original with missing images and0/68 or0/69 visible bindings; both use matched60/64 zone-coverage plans. These are smoke cases, not a660-row census or UI-branch paint result. Published TSV rollups were still empty at that check.

Baseline `ScriptHost::findings()` (`script.rs:1232`) returns `Finding {feature,value,count}` (`:238`); `Shared::find` (`:284`) aggregates by feature and retains its first value. Fault feature is exactly `lua error` (`resume` at`:1246`). Deadline text is `time budget exceeded` (`:575`) in **value**, not feature. The original production probe's budget filter incorrectly inspected feature; it is corrected to inspect value. Historic `budget_findings:0` fields cannot establish absence of budget faults. Typed counts are available on the host before `attach_script` folds count into `Unsupported.value` as `(xN)` (`lib.rs:1007`). The worker publishes only the init snapshot (`scripted/thread.rs:85`); baseline runtime first-message/count is unavailable. The shared extension's phase hooks are needed for actual runtime fault statistics. Sanitize retained values with the existing shared `metrics::message`/`metrics::budget` helpers, never dump authored diagnostic strings. A lexical findings vector is not chronological global first-fault order.


### Historical exact shared Clarinet A paired smoke (9dcf05e5)

Item: `VWinds-AClarinet.ufs::Presets/Clarinet A.uvip`, under `VWinds - Clarinets`. At the time of that smoke, matching-signature canonical cache records used v2 digest18fe63fe, pinned-v1 dispatcher digest a4b3f8c7 and unchanged UVI sidecar digestd565661a, base4bffbb18/source1c198e60. Both records report `matched-note-plan`, select60/64 and play audibly. Both expose exactly the same26 declared native valid keys and native preference60/64. Preserve **common-plan origin** `native_declared` separately from v1's **local application route** `shared-note-plan`; the override is not a zone-selection/native-valid claim.

| Observation | v1 UVI sidecar | v2 frozen baseline + shared instrumentation |
| --- | ---: | ---: |
| Loads / audible at60/64 | Yes / yes | Yes / yes |
| Original UI classification | original-ok | missing-images |
| Actual Original CPU paint (different root trees) | Yes | Yes |
| Visible controls bound |21/21|0/21|
| Published root load_ms |1,912.94|704.51|
| Whole worker process_ms |2,707.75|1,402.75|
| Peak process RSS MiB |418.60|140.36|
| Resident sample bytes |218,923,526|26,175,632|
| Lookup requested / succeeded |Unknown|40/0|
| Decode requested / succeeded |Unknown|0/0|
| Lua init / runtime faults |Unknown / unknown|0/0 observed|
| Actual Lua budget hits |Unknown|0 observed|

These are one warm/cache-affected pair, not660-program results or native PCM parity. v1 root load time includes bank metadata/UI-assets opening and wait-until-worker-Ready (`src/uvi/scan.rs:20–114`); v2 root time covers `V2Loader.prepare` (`tools/kontra-scan:src/ui/scan.rs:295–341`). v2 `programs[0].load_ms=1,296.59` also includes later UI/audition work (`:517`), so it is not another prepare-to-Ready metric. Keep the stages explicit instead of asserting a precise load speedup.

**v1 diagnostic precision:** installedd565661a exports `asset_failures`, `font_failures`, `asset_limit` from `UiAssets::diagnostics()` (`src/uvi/scan.rs:198–215`) and generic `runtime_errors` from `Worker::stats().errors` (`:301`). These are safe aggregate fields, not lookup/decode/font-request counts, Lua init/runtime phase faults, or an actual layout/Lua budget-hit accessor. `asset_limit` is an asset cache limit, not a tree/VM budget. The extended columns remain **unknown**, not0. No existing adapter export can fill the requested missing fields accurately. New v1 baseline instrumentation would be required; the installed sidecar/source and common CLI are unchanged.


### Superseded shared sweep: e340c39a

Installed binaries were SHA-256 verified after the census owner's handoff:

| Component | Frozen source/base | Binary SHA-256 |
| --- | --- | --- |
| Shared v2 | `tools/kontra-scan@e340c39a6666752866015b5ee8f06a3cc978c416`, product7e82b152 | `742c24e295358a7631fd5ae4fb85d576e51d03ef669eaa2efbf7ed2dfa6030c4` |
| Pinned Kontakt v1 dispatcher | adapter788f41fafa7e21ddf7b1917bc4cf43e0a83876b8, product0cb7a8a0 | `ac5aed734bb7fca40d6d000ff1f3e89128436b8f38d9d674fd70466f90dcdf08` |
| Separate UVI v1 sidecar | source1c198e60, product4bffbb18 | `d565661afb3ae1cab3100d1e83b7f12c0f5b02c51b7593451a6fe77929af8ea1` |

The scanner owner reports both optimized builds, three checks per scanner and wrapper release/no-run gates passed. This audit verified installed digests and read the updated README; it did not rebuild or change the common collector. Both canonical TSVs contained only headers at this check. The owner restarts the serial <=235-second paired1494-ID sweep, Conflux/Clarinet first. This audit's duplicate UVI runner remains stopped.

Current schema adds strict saved-record framing/parse classification and separate actual init/persistence telemetry, `bound_typed`, `sample_zone_count`/`zero_zone_reason`, and actual fallback/keyswitch observations. The generated fixed symbol whitelist has246 entries including29 missing names per the owner; incidence aggregates are `results/v2/symbol-aggregates.tsv`, authored-UI regression output is `~/.cache/kontra-scan/v1ok-v2missing.tsv`. New fields remain unknown when the observed UVI sidecar path does not expose them; frozen `phantom_free_controls` is unknown rather than inferred by library-specific subtraction.

For new audio comparisons, use actual `note_picked`/`note_policy`/`audition_status` with the common per-item numeric plan. No safe key means `plays_note=no` with its explicit reason. A fallback audition can test whether script callbacks enable zones, but is excluded from parity; neither no-audition nor old null-pick output is a silence regression. Native declarations, zone coverage and fallback remain distinct. The historical Clarinet pair proves the earlier matching native keys/note and audible outcome, but must be rerun under the new collector signature before inclusion in the new matched census.


**Unchanged UVI sidecar note-plan boundary:** source1c198e60's compiled `metrics::note(program)` (`tools/kontra-scan/metrics.rs:59–66`) returns only `Option<(u8,u8)>`; it reads key/velocity and has no optional keyswitch or explicit no-audition command. `src/uvi/scan.rs:189–194` falls back to its local selection when no numeric override exists. Thus common valid numeric plans are honored, but a keyswitch in the new plan does not establish that the old sidecar sent it; keep v1 keyswitch execution unknown. Likewise, when v2 has no safe pick, do not regard an independent sidecar fallback as a matched safe audition. Require both actual picks/statuses and compatible keyswitch evidence before parity claims; the audit does not modify the frozen adapter.


### Superseded shared sweep: 2355155b (zone-count correction)

The scanner owner pushed and installed `tools/kontra-scan@2355155b56a5ae26add8bdbc287d46d42e7825f2`; this audit independently verified v2 binary SHA-256 `f1105599c13b14410ed891b29a014b8af43596e238850bb8e4439596e96e013a`, pinned-v1 SHA-256 `ac5aed734bb7fca40d6d000ff1f3e89128436b8f38d9d674fd70466f90dcdf08` and unchanged UVI-sidecar SHA-256 `d565661afb3ae1cab3100d1e83b7f12c0f5b02c51b7593451a6fe77929af8ea1`. Use2355155b for future scanner extension merges/rebuilds; installed `bin/README.md` records that exact rebuild revision. The owner reports its fresh required wrapper/no-run gate passed and the paired serial sweep restarted after Conflux-first smoke. Results remain pending; no independent collector is running here.

`sample_zone_count` now means retained instrument IR mapping count, **unknown if the retained instrument is unavailable**. Raw `decoded_zone_count` remains separate in detailed JSON. Neither mapping count measures runtime purge/gating, playable current-state voices, or audible PCM; reserved cache residency is not zone existence. No-safe-key reason explicitly says **not auditioned**, rather than claiming a silent played note. The unchanged UVI sidecar's unknown fields and keyswitch/no-audition-command boundary above still apply.


### Historical first section J stage: 95a1f01b (superseded for f7b2 timing acceptance)

Scanner-only source is pushed on `audit/uvi-v1-scanner-20261008@95a1f01b95234cf954e2f149c711ad501c704acd`, still based on product4bffbb18. Only the existing scanner and CPU-paint probe change. Optimized executable is staged at `/home/derpcat/.cache/kontakto-audit-uvi/staged/kontra-scan-v1-uvi-onset-95a1f01b`, SHA-256 `7f628b151c3dd31ddef1e09ac71876367c543d65e940b490132afcb089ba9b8b`; metadata-only manifest is beside it as `v1-uvi-onset-95a1f01b.json`. The auditor does not install it or edit the shared CLI; installedd565 is preserved until the owner updates it. The old d565 executable is also archived at `staged/kontra-scan-v1-uvi-d565`; the mutable Cargo build-output path now holds the new build.

Root and `programs[0]` export numeric-or-null `first_audio_ms` and `ui_first_frame_ms`, plus `cache_state="cold"`. Both timers use the existing monotonic load-start Instant. Audio onset observes the first packet with actual finite nonzero output before pacing sleep (`src/uvi/scan.rs:285`), never a load_ms fallback; silent, failed and no-output auditions retain null. UI timing is taken immediately after the first successful Original CPU bitmap completes (`src/ui/scan_uvi.rs:62`); unavailable/failed painting retains null. `load_ms` is unchanged and includes v1's deferred preload, so it is not first sound. Cold describes disabled persistent product PCM cache, not OS filesystem page-cache warmth. The scanner still probes UI before audio; these are worker-output/CPU-bitmap availability timings, not DAW DAC or display-presentation timestamps.

One shared-plan Clarinet A probe establishes: load**1,668.39ms**, first UI bitmap**1,708.94ms**, first finite nonzero audio**1,759.16ms**; Original-ok, audible,0 nonfinite samples, pick60/64, preference60/64 and the same26 native valid keys as the earlier v2 witness. The existing override source stays `shared-note-plan`; native declarations/selection code is unchanged. Missing-item failure exports both onset fields null. The synthetic onset check passes1/1 (zeros/nonfinite do not create onset; later output does not overwrite the first observation); full `shots,uvi` no-run gate passes1m51s; release scanner build passes1m46s. No independent census or raw resources/audio are persisted. Lua/asset diagnostic and keyswitch/no-audition-command limitations of the earlier sidecar remain unchanged.


### Superseded shared section J: f7b2a8cd

Installed source `tools/kontra-scan@f7b2a8cdc6773ff569799c256ad9b0d638e3a824`, pinned adapter11db26e72650f373465b6534d6ddb3684e1e9970; this audit verified binary digests v2`2c7c50d150ef168f46924dc4f90ea3edc2d96fa9350a299922858be2031237d0`, pinned v1`bdbd24d642aaacf4511bdf8b717db14676d2ae46dc922ef13f24b9734c4a1438`, installed UVI sidecar unchangedd565. Frozen product bases remain7e82/0cb7; optimized builds and fresh no-run gates passed per owner. The common driver now exposes `first_audio_ms`, `ui_first_frame_ms`, `cache_state`; retained-IR zone counts and strict saved parser remain available. Canonical results reset for this revision; full1494 paired coverage is pending.

The new contract starts its clock at actual first production import after the metadata prepass, excludes isolated-worker startup/pixel hashing/PNG writing, and observes concurrent Original CPU paint/audio. First finite exactly nonzero output is distinct from the1e-5 audible test; absent output is unknown. This supersedes the first UVI stage's earlier explicit silent=>null and UI-before-audio requirement. That95a1f01b stage remains a reproducible historical receipt but must not be treated as a matching f7b2 onset comparator. A new stage retains pre-audition native declarations, begins its clock at `Worker::start`, paints off the audio observation thread and preserves the existing load_ms basis. No plugin release/install occurs.


### Final aligned UVI J stage: 026bdbb4

Source pushed `audit/uvi-v1-scanner-20261008@026bdbb49f29a5ad752b3470a5f6f64a20a8957d`, product4bffbb18 unchanged. READY staged executable: `/home/derpcat/.cache/kontakto-audit-uvi/staged/kontra-scan-v1-uvi-onset-026bdbb4`, SHA-256 `34a61e82ca6f09afc0eab692a1bf0b6765edbd7d063fccca94d95e699a93b9c1`; adjacent `v1-uvi-onset-026bdbb4.json` records source/base/hash and checks. This supersedes95a1f01b/7f628b15 for current J timing acceptance. The shared owner has installed this exact executable as `~/.cache/kontra-scan/bin/kontra-scan-v1-uvi` and alias `kontra-scan-v1uvi`; this audit verified both digests match the staged binary. Historical1c198e60/d565 is preserved by the owner under `../frozen`; the auditor did not alter shared binaries.

Both onset clocks begin at **production `Worker::start` after the metadata/UI-assets identity prepass** (`src/uvi/scan.rs:108`), include the necessary pre-audition native snapshots and use that same monotonic origin. Retained native declarations/preference are read before NoteOn; then a scoped CPU painter runs concurrently with the audio observation loop (`:227`). This removes serial extra CPU painting from first-sound observation. First audio is the first received packet with finite exactly nonzero output, before pacing (`:304`), independent of the1e-5 audible criterion. Physically zero/no-output or missing-item onsets remain null; near-silent finite output can have numeric onset while `plays_note` remains silent. UI onset is actual bitmap completion before pixel hashing. `load_ms` retains its old prepass+deferred-preload basis, so it is neither first sound nor the same clock origin. Product cache stays cold/disabled; OS page-cache condition is uncontrolled.

Shared60/64 Clarinet proof: first audio**2,126.57ms** is observed **before** first CPU bitmap**2,128.44ms**; legacy load_ms**2,349.89ms** has the different origin above. Original-ok,21/21 visible bindings, audible,0 nonfinite samples, preference60/64 and all26 native valid keys unchanged against the earlier frozen-v2 witness. This one cache/load-affected case proves concurrent observation, not a performance regression/speedup relative to the prior serial smoke. Missing-item output has both onset fields null. The tiny finite1e-8 detector check passes1/1; fresh `cargo test --profile ci --features shots,uvi --no-run` passes**41.58s**; optimized release build passes**52.52s**. Source and staged handoff were sent directly to the census owner/W8. No independent census, product install, reader/Wine change or raw resources/audio persisted.

### Verified shared Rust publication: abf248cd

Shared source `tools/kontra-scan@abf248cd0b99d884b9f2456914362a7bd8e81869`, pinned Kontakt adapter44d03cecbd3b5e47b1564d17ac239239dc2ba4ea. Installed verified digests: v2`d4534838916e008d32a6e0763541a8bd9285651d77f130bad18fe926d0975f5a`, v1`870cea2140b5c9db5361831664966848302a2f57e82fcb6c7ed1b3534545ce5e`. User reports both optimized builds, three checks per scanner, Python checks and fresh required no-run gates passed. Schema remains72 columns with J timers/cache state. Decoded source disposition is independent of saved-table integrity; incomplete raw sigil totals are unknown. The owner's new Conflux pair has5 slots (3 inline/2 empty/0 decode failures), same60/64 audible and preserved UI/scalar/typed metrics; raw saved histogram v1 unknown/v2 decoded. Conflux remains outside this UVI audit population. Serial235-second Kontakt pass restarted; owner publishes only current-signature canonical TSV/cache rows, with final1494 paired coverage still pending. No plugin release/install.

### Current shared driver and completed UVI J handoff: 01178ba4

The owner pushed `tools/kontra-scan@01178ba443e2b409c23282509f57d35a60753c36`, changing only CLI cache environment/README relative toabf248cd. The driver forces `KONTRA_UVI_STATIC_PCM_CACHE=0`; all three adapters expose the72-column schema including J onset/cache fields. This audit independently verified installed v2d4534838, pinned v1870cea21 and both UVI sidecar aliases34a61e82 unchanged from their documented builds. Product UVI base4bffbb18 is unchanged. Sidecar checks and concurrent Clarinet proof above remain valid.

The owner runs the serial834-ID Kontakt pass, then the automatic660-ID paired UVI pass with common notes and235-second one-heavy-call shards, followed by exact1494 signature/coverage assertions. Canonical TSV/cache coverage remains partial; this report has no final corpus parity or timing claim. No independent census is running here and no plugin release/install occurred.
