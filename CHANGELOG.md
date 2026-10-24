# Changelog

Human-reviewed changes belong in the unreleased section before publication. Nightly
notes use accepted ledger entries added since the previous published source,
with a short user-facing known-issues list and collapsed source/checksum details.
Each published release manifest also retains its versioned changelog. Frozen entries
below record reviewed source checkpoints; they are not claims about pending work.

## Unreleased — playback foundations

- Keep runnable script stages at the same event time, including chords arriving after a preemption.
- Complete runnable startup on the loader before an instrument is ready. Preserve intentional script waits.
- Require complete attack preloads for normal streaming. Use a bounded memory-aware head budget; insufficient capacity is an explicit load error.
- Report execution-budget failures, host-service stalls, late attacks and source-readiness delays through the plugin diagnostics. Over-budget playback callbacks fail explicitly instead of replaying notes in later blocks.

## 0.3.503

Distributes shipping integration-test compilation across eight Linux runners after the 0.3.502 Nightly exhausted its three-hour validation budget before any test harness started. Every integration target remains required exactly once, including names shared by packages. All fourteen first-party package selections and the release ThinLTO profile remain unchanged.

Shipping validation now checks actual enabled features against a reviewed Cargo artifact contract and rejects source changes beyond the deterministic Nightly version stamp. Library and binary tests, documentation tests, example builds, native format/editor checks, signing/notarization, installer rollback and legal/manifest gates remain mandatory. Local workflow and selected Cargo proofs passed; hosted cold-build completion and publication bind the new merged source and are not inferred from those local checks.

This is a release-pipeline correction. The compatibility, finite-loading and performance limits below remain, as do the 69 reviewed error-level Doctor findings and its failed tool gate under contextual review. No local installation is changed by this source checkpoint.

## 0.3.502

Adds one reviewed loader correction to the 0.3.501 source checkpoint: changing a translated instrument's public script roster or changing a script to KSP invalidates the cached initializer shape. Valid unchanged scripts still initialize once; malformed replacements and source-identity changes retain explicit diagnostics. Six authored regressions cover these boundaries.

The complete static audit, using a pinned private resolver correction, has zero producer errors and retains 69 reviewed error-level findings. Its tool gate remains failed; no rule, severity or finding was suppressed. Contextual review and executable/native checks remain required. Hosted shipping, signing/notarization, notice and publication checks bind the actual merged source. The compatibility, finite-loading and performance limits described below still apply.

## 0.3.501

This experimental checkpoint carries the accepted 0.3.490 changes and eleven additional reviewed fixes. Loading remains bounded; UVI/Falcon and Kontakt compatibility remain partial.

- Preset menus resolve saved positions against the live script-owned roster. Explicit preset picks discard stale project captures, and scripted source/zone changes commit with bounded, owned async completions.
- Analog Strings internal browser A→B→A restored the selected musical parameters, actual source groups and fresh-note sound in a real library witness. Async residency callback pressure now defers completion instead of freezing the script/audio clock. Array-file metadata reads have independent ownership from write snapshots.
- UI artwork requests and painting use the same committed or authored default value. Retained assets and presentation owners survive unrelated updates; supported same-instrument recalls retain immutable resources. Presets requiring a full initialization or unsupported native-state transaction still use the full load path.
- Streamed sampler layers belonging to one logical attack wait together for their requested initial demand. One real Areia Full Ensemble Spiccato Slow capture recorded 19 attacks with aligned contribution starts and zero underruns. Cold disk waits remain; pacing misses and round-robin comparison limits prevent a complete perceptual or native-parity verdict. Time-stretch instruments retain their prior path.
- Mandatory resident PCM remains loaded when optional preload anchors exceed the payload quota. The unchanged public UVI exact-budget regression now passes, while insufficient mandatory budgets, read failures and cancellation still refuse safely.
- macOS archives include the bundled assistant-font and FastLZ notices; universal composition and publication reject missing font, BUFFR or other required notices. Signing/notarization/install gates are unchanged.
- Signed modulation mappers preserve interior zero crossings. Malformed KSP get_scale destinations are rejected before they can reach the evaluator's declared-array invariant. The portable logarithm correction is included.

Source validation at the coordinated implementation checkpoint: 1,954 top-level tests passed, zero failed, 69 explicitly ignored. The real Areia trace had zero underruns, stream errors, capacity refusals and trace loss; it still recorded 162 cold admissions and 78 pacing misses. Release-candidate, native format/editor and hosted shipping gates bind their own exact revisions. See docs/diagnostics/20261016-release-checkpoint.md.

Known limits: VWinds has verified KONTRA playback witnesses, including audible Contrabass Clarinet at key 48, but this does not establish every UVI program or full native parity. Native UVI LFO modes, shared bend ownership, GainMatrix and other reported features remain incomplete; private numeric kernels are not production support. There is no unlimited-loading, zero-cold-wait or 300-voice performance claim. Full UI/DSP/library parity remains unproved. Signing, notarization, notices and release verification remain mandatory.

## 0.3.490

Includes the accepted 0.3.489 parity checkpoint below, plus one shipping-pipeline correction: cold release-profile test compilation may use 180 minutes, while ordinary CI retains its 90-minute limit. Every test, native editor, signing, notarization, license and required-check gate remains mandatory. The observed previous job expired before any test harness started; this change does not claim a measured upper completion bound.

The unchanged Rust source passed first-party Clippy, the complete core suite, 820 root/inspector tests (53 ignored) and three typed replacement tests at the preceding 0.3.489 source checkpoint. Final package identity, native format checks and hosted publication bind their own revisions. New experimental menu transactions and the production-loader Areia trace remain outside this accepted checkpoint.

## 0.3.489

Seven reviewed logical corrections follow 0.3.482; diagnostic features and extra reference vectors do not add fix counts.

- Retained interfaces stop republishing unchanged image/font owners. Required in-flight artwork survives unrelated metadata changes, and repeated uses dispatch each exact asset request once. Obsolete requests remain fenced.
- UVI ScriptEvent modulation uses the authored EventId independently of its display name. This fixes an observed VWinds source named with suffix 7 whose actual EventId is 12; malformed explicit IDs remain explicit errors.
- Ladder feedback preparation preserves Kontakt's floating-point operation order. Twelve captured prepared states and 282 static native cases match exactly; production HQ ownership and moving-control parity remain separately gated.
- Final modulation topology is validated before use, and parallel workers retain the gain history of audio actually rendered.
- Inspector additions record bounded physical voice births, evaluated start offsets, first-window readiness and numeric signal crossings. Snapshot reuse reports actual policy/schema/admission stages without treating callback admission as completion.

Focused source validation includes 92 UI/asset/snapshot tests (3 ignored), 78 affected core regressions (1 ignored), 21 UVI importer tests, and 25 bounded onset-trace controls. These are scoped source receipts; combined checkpoint, packaging and native-host checks retain their own exact revisions.

Known limits: Areia's reported spiccato double attack is not yet attributed to its actual runtime trace. Generic cold sibling layers can start apart in authored tests, while Areia also intentionally layers ensembles and retains recorded lead-in. Full native/UI/library parity and 300 voices at 1% on KONTRA's meter remain unproved. Presets requiring initialization, native payload replacement or unsupported transactions still use the full load path. New menu transactions and the production-loader onset probe continue separately and are not part of this checkpoint.

## 0.3.482

This checkpoint brings the accumulated library, scripting, streaming, editor and native-DSP work since the last published 0.3.476 snapshot. Five newly reviewed defects advance the version from the locally installed 0.3.477; follow-up fixes and optimizations are not counted again.

- Preset switching has sequential offline witnesses for actual preparation, publication, callback settlement, stale controls, menus and fresh-note audio. UVI probes now wait for the real Lua owner and ordered command drain, expose sample status and pace audio explicitly rather than treating a fast offline render as readiness.
- Kontakt snapshots keep convolution resource ownership, so a saved impulse index resolves against its own file table while retaining base archive resources. Missing factory dependencies remain explicit.
- UVI module reads retain typed failures and strict UTF-8; required final translation failures roll back. Menus, table fills and authored separators have additional projection coverage. Identical immutable presentation writes no longer rebuild the exported interface.
- All 40 installed VWinds programs passed paced note witnesses; current initialized projection checks cover all 40, with WaveView limitations still reported. These checks establish KONTRA behavior, not full native parity.
- Source-rate dependencies evaluate in order, and onset phase modulation preserves the saved LFO phase. Ladder float operation order now matches 117 complete native stereo vectors exactly within the admitted kernel scope. Skreamer live controls and saved control ownership include authored PCM and stale-generation checks.
- Linux editor windows and callbacks remain on their worker; failed construction, close and reopen paths retain ownership and X11 parent notifications. The macOS open-state query has corrected polarity; native macOS acceptance remains subject to hosted checks.
- Voice preparation copies are reduced, and completed growth owners are reclaimed off audio. A separate interleaved-mixing experiment was rejected after measurements failed its acceptance gates and is absent from this release.

Validation at implementation checkpoint `95ac0d7d0dd6f313e0ead799ab9d8907b31a7680`: 807 root tests passed, 53 explicitly ignored, zero failed; the UI-enabled inspector built successfully. Focused native, core, UVI, KSP and editor receipts retain their narrower scopes. Release packaging and hosted platform validation report their own exact revisions.

Known issues: the reported Areia doubled staccato and stream underruns remain under investigation. Large libraries can exhaust the configured attack budget and wait for disk; 300 voices at 1% on KONTRA's meter is not achieved or promised. Daft/Jump, some convolution size controls, wavetable and nested modulation paths, UVI native LFO modes and several authored UI cases remain incomplete. Full library/native/UI/DSP parity is not claimed. The Rust Doctor audit is retained with its resolver limitations, not reported as a clean result.

## 0.3.476

Nineteen reviewed corrections follow 0.3.457. Kontakt support now checks itself against Kontakt: a native oracle compares KONTRA's load with real Kontakt 8.13.1 (0 mismatches over ~5.3M group/zone/loop values across 11 libraries) and Kontakt objects decode with Kontakt's own reader programs.

- Instruments saved by Kontakt 8.13 load again (external modulation 0x105).
- Hall reverbs no longer play as Room; SSL G-EQ v0x11 decodes completely.
- Script-set bus faders use Kontakt's 6 dB-per-doubling volume law (measured in Kontakt).
- Cycle round robin plays in Kontakt's order; first-position groups are no longer dropped.
- Releases and voice limits stay inside their own instrument; Kontakt exclusive voice groups work.
- `purge_group` frees and reloads sample memory; purged groups cost no CPU; muted groups can be unmuted live.
- Samples in moved or differently spelled archives are found; default voice groups get their limits.
- Silent effect buses and idle instruments stop costing CPU.
- Conflux: controller programs use their own skin, Vector no longer overlaps, custom fonts and press states import.
- Modulation of LFO rate/amount and effect knobs plays (Conflux 2,207 dropped routes -> 0 excluding 550 random sources; ANALOG STRINGS 6,065 -> 1,902).
- Kontakt LFO saw, random and pulse-width shapes use Kontakt's own waveform tables.
- Kontakt objects decode exactly or report what is undecoded (14 -> 105 of 118 classes).

New engine pieces (not counted as fixes): shared envelope/LFO/glide/follower/arp blocks, Time Machine source engine (TM1 native; TM2/Pro approximations), voice-allocation domains.

Validation: ni-file 51, sampler-core 191 lib plus all integration suites, sampler-kontakt 162, native oracle 3, sampler-uvi 85, sampler-ksp 10 passed; all 11 installed Kontakt libraries complete the production load path.

Known limits: 550 Conflux random-source routes, DBD/Flex/glide/follower/arp/group-dynamics Kontakt mappings, Multi-weight LFOs and several effects remain reported (unproved native laws). Time Machine 2/Pro are approximations. Two new skinned-caption/picture-state render tests are ignored pending investigation. Playing-CPU after these changes is not yet measured.

## 0.3.457

Six reviewed corrections follow 0.3.451:

- Kontakt legacy one-band EQ and live controls use the verified native float conversions and scalar recurrence.
- KSP tables display and edit the requested leading columns while preserving hidden cells.
- Failed or canceled instrument replacement keeps active audio, UI and script ownership. Same-source host recall preserves recalled state before loading, after cancellation, and across superseded jobs.
- UVI `run` and `spawn` remain independent of the calling note; release waits follow the documented coroutine ownership.
- UI library sweeps reject missing tests, zero-test successes and incomplete measurements.
- Conflux callback audits service effect backpressure and distinguish finite startup from recurring timer drivers. The original controls, exact-three-effects assertion and label readback remain required.

Validation: 2,325 distinct Cargo-target tests passed after the final affected-target rerun; 153 explicit gates remain ignored in the default suite. Eighteen Python admission tests and the explicit Conflux callback gate passed. Plugin and standalone release builds passed. Fresh source inspection reconciled all 1,495 items across 15 libraries with zero integrity faults; 17 selected initializations reached IR, with nine complete structural UI models and eight incomplete. Conflux, Bartok and A Clarinet editor paint gates passed.

Original-instruction checks passed 1,222 Kontakt records and 3,937 Workstation cases. The corrected EQ probe includes 32 scalar PCM/history cases, with nonzero output in all 28 positive-frame cases. These are scoped kernel checks, not full native library or host acceptance.

Known limits: Conflux Vector still lacks authored NativeUI layout projection. NCKP omissions, UVI OnePole production conversion, async `loadSample`, native multiband/SIMD EQ, full host lifecycle and calibrated audio comparisons remain open. Successful instrument UI/audio acknowledgment remains non-atomic, and live UVI worker timing remains Approximate. No Falcon executable was available; Workstation covers only the demonstrated shared DSP paths.

Implementation checkpoint: `b44473c9e14ae4fcf42af562ee8684331ca1473f`. [Detailed validation](https://github.com/DerpcatMusic/KONTRA/blob/b44473c9e14ae4fcf42af562ee8684331ca1473f/docs/audit-2026-10-13/validation.md). Six accepted defect IDs advance the patch from 451 to 457; follow-up commits and extra fixtures are not counted again.

## 0.3.451

This snapshot retains the consolidation and compatibility changes recorded under 0.3.450 below.

- Mac ticket stapling and validation recover from the observed temporary Apple connection loss with four bounded attempts. Both operations must succeed; Apple approval, signatures and Gatekeeper assessment remain required.
- CI retains compiled first-party workspace files, including after packaging failures, while all existing source and native release checks still run.
- Download notes accurately locate the separate CLAP, VST3 and standalone build details in `release-manifest.json`.

The ticket correction passes seven focused recovery tests, the updated universal-package tests and the full local signing/publication mock gate. Tests cover recovery in both operations without another upload, bounded exhaustion, fatal invalid tickets and private-output suppression. Actual Apple ticket recovery for this corrected source remains subject to the hosted release checks. One newly accepted defect advances the patch from 450 to 451. The native parity limits recorded below still apply.

## 0.3.450

This experimental snapshot includes the source consolidation reviewed on 2026-10-12, together with the previously recorded 0.3.430–0.3.446 compatibility changes.

- Accessibility value changes and button activation now wake the retained controls that own them, including nested views, while unrelated siblings stay cached.
- Validated Kontakt instruments release excess executable and saved zone storage before publication, preserving physical zone identities, holes and ordering.
- The legacy Kontakt multi health probe visits every program and reports real program/zone totals or read failures.
- Mac publication accepts Apple’s upload acknowledgement and then waits for approval of that submission before packaging.

The consolidated source also recovers scoped native control consumers, host release and note ownership, MPE controller precision, authored UVI geometry/value/gesture paths, shared callback budgets and typed effect framing. Historical sources and unfinished snapshots are preserved in main ancestry and recovery refs; their presence alone is not acceptance evidence. Existing logical fixes and follow-up ports are not counted again. Four newly reviewed defect IDs advance the patch from 446 to 450.

Source 14a284fc passed 2,605 tests with zero failures, doctest checks, CI-equivalent Clippy and optimized plugin, standalone and offline-inspector builds. The actual shared library loaded with non-null VST3 and CLAP entry points. Source 0ae6e387 adds the completed consolidation receipts only. The publication pipeline validates this versioned source with the shipping release profile and requires the packaged products and notarized Mac installer before publishing. The Apple upload/wait correction passes 19 notarization regressions and the complete local signing/publication mock gate.

Known limits: full Kontakt/Falcon instrument, UI and DSP parity remain unverified. Preserved v1 program-bank ownership, asynchronous replacement and scheduling, modern UI dialect discovery and several native effect/control laws remain unresolved. Linux host/fixture acceptance does not establish Windows/macOS DAW acceptance or comparative CPU/RAM superiority.

## 0.3.446

- Copy diagnostics now produces a compact, deduplicated error-and-origin summary without JSON, script syntax, configuration dumps or journal history. The full local export remains available.
- NKA arrays use storage sized from their authored declarations, without fixed array-cell or file-byte ceilings. Large snapshots use fewer concurrent job slots; worker file I/O streams through buffers while preserving atomic replacement and failed-read rollback.

Hosted checks compiled these changes and passed the compact-copy, storage-admission and above-legacy-cap streaming/rollback regressions. Two additional logical fixes advance the patch from 444 to 446. An old malformed-file test expectation was corrected; final exact-head CI and local installation remain pending. The reported 5STRNGCharge capacity failure came from installed 0.3.430 and is covered by the already accepted 16,000-number/4,000-text snapshot-pool regression in 0.3.439 and later.

## 0.3.444

Five reviewed compatibility fixes extend 0.3.439:

- Preserve authored step curves when envelope stages are retimed.
- Sort KSP string arrays consistently during initialization and callbacks, including inclusive subranges.
- Return nil for absent or unknown UVI parameter connections.
- Keep the current artwork request pending when an older epoch finishes.
- Open libraries containing empty saved numeric arrays without allocating storage for those arrays.

The exact source merged in PR57 passed all eight hosted checks, including Linux, Windows, macOS and Vulkan/OpenGL/CPU editor checks; all eight newly authored regressions passed. The package and reviewed fix ledger now identify 0.3.444. Final local stamped build and installation remain pending the shared-machine build window.

Known limits: full native instrument/UI/DSP parity and comparative CPU/RAM superiority remain unverified. The existing MUI pin is unchanged.

## 0.3.439

Library search now matches library names and vendors independently of the lower preset/folder search. Clearing the library search keeps the selected library visible and preserves its folder hierarchy.

KSP fixes preserve function-local exit, single evaluation of real sgn operands, supplied extended-display values, documented array equality admission and bounded search, listener registration/ranges, highest physical event-zone IDs, and init-only EQ band gain writes. The NKA snapshot pool sizes its job count to the admitted payload so legal large arrays fit without oversized preallocation.

Retained authored-UI projection and decoration reuse remain enabled. Removing an expensive native primitive proof prepass reduced headless Conflux frame construction from 52.1 to 40.3 ms and total software rendering from 81.0 to 68.8 ms. Fresh script graphs and callbacks still execute; this is not a DAW/GPU or 60 FPS claim.

The reviewed 7971dcc4 source passed the combined release build and all 116 selected regression runs: 1,077 pass invocations, zero failures, 79 ignored. All 359 required entries passed (358 distinct test identities). Nine newly accepted logical fixes advance the patch from 430 to 439; follow-up and audit commits are not counted as extra fixes. Publication and local installation require freshly stamped release artifacts.

Known limits: full native instrument/UI/DSP parity and comparative CPU/RAM superiority remain unverified. Older Kontakt XML still needs a translator. This candidate retains its existing MUI pin; the corrected upstream stability update and the five newly authored parity slices remain separate pending work.

## 0.3.430

Source composition since 0.3.429: add typed explicit-path NKA array I/O, bounded RPN/NRPN callback delivery, owned waveform state with validated attachment/readback and bounded UI projection, explicit JSON `loadData` completion, sample-clock fade curves, live modulation depth/bypass controls with schema remapping, dynamic menu getters, and parameter readback guarded by setter-queue admission. Preserve the published website, 0.3.429 release history and release-fix ledger, plus the newer macOS packaging/notarization fixes.

The ff60de20 release source passed a release build and 101 selected regression runs (593 pass invocations, zero failures, 48 ignored); all 293 required tests passed. The exact Linux CLAP, VST3 and standalone artifacts were locally installed with backups. This does not establish native-host or whole-instrument parity.

Conflux now has an opt-in connected UI phase profiler. Removing temporary Lua intrinsic-layout wrappers reduced authored layout work by about 19% and frame construction by about 6% in paired headless measurements, with byte-identical Original captures. Total software rendering did not consistently improve. The larger retained-UI architecture remains separate pending work.

## 0.3.429

This experimental update restores more authored instrument controls, script callbacks, source modulation and convolution resources.

### Loading & files

- Preserve catalog failure details and retry metadata or artwork when file/access state changes.
- Resolve saved convolution impulses from the owning NKR when loose files are missing, and retain impulse tables in legacy multis.

### Sound/engine (v1 ports)

- Preserve supported Kontakt LFO source frequency and signed target intensity using the checked v1 laws.
- Retain raw modulation descriptors for diagnosis; unsupported source modes remain explicit.

### KSP scripts

- Read runtime UI metadata and native zone group/key fields through computed operands; keep numeric font identifiers.
- Evaluate UI setter arguments once and dispatch global UI callbacks around the local callback in v1 order.
- Schedule beat waits against the live host tempo.

### UVI

- Load authored modules, retain class inheritance/object identity, and support authored UI construction without generated grids.
- Honor native Unit/Mapper metadata and shared source display conventions while preserving raw values and edit payloads.

### Saved projects

- Retain the existing coherent snapshot and host-recall guards while integrating the script/control changes.

### Editor/UI

- Select the declared performance view by default and preserve explicit tab choices.
- Keep authored keyboard colours, reach MIDI note 127, and preserve exclusive MIDI-learn routing.
- Render declared sliders and authored knob captions; retain the selected menu caption when its matching item is hidden.
- Keep fractional image coverage and exclude widgets wholly outside their authored page.
- Restore sound and route menu shortcuts.

### Memory & CPU

- Keep native zone-readback bytecode within the existing 32-byte Instruction budget.
- In the earlier owner's two-preset CPU-editor measurements, installed-to-first-sound fell from 517.556 to 11.648 ms for Areia and from 357.407 to 6.574 ms for Dolce. These remain scoped historical results; this batch adds no timing acceptance.

### Fixes

- These are source-specific corrections backed by the accepted logical-fix ledger, rather than a claim of complete instrument parity.

### Known limits

- Full authored-widget gesture, callback and sound parity remain incomplete. Protected installed UVI playback, real Windows/Linux DAW operation, and quiet CPU/RSS/deadline acceptance remain unverified.
- Unsupported Kontakt modulation cases remain diagnosed; no new whole-voice or Digital Multi fade claim is made.
- Fractional geometry/paint-order projection and the Conflux capture/storage investigation remain separate pending work.

### Evidence

- Product `a8c29411`: 2,224 workspace tests passed, zero failed; 48 scheduler tests, 45 reader tests (one ignored), and three articulation normalization tests passed. Focused bytecode/selector/class/keyboard checks, compilation, offline release checks and GPU regressions passed.
- Native Conflux and Vista checks passed. All ten browser captures match 403 byte for byte; library and preset states were reviewed at both sizes. The native scanner loaded and sounded all 15 cases, retaining 12 Original views. The full native quick retry loaded 72/72, sounded 68, produced finite audio and zero audio-thread allocations, and passed the original matched stage/audibility/stuck-note/voice guards.
- Retirement counts varied in the initial quick and focused frozen 403 runs; the accepted full retry passed the unchanged guards. These results do not certify timing, release-tail stability or complete authored-widget operation. The loose UVI quick fixture still has no Lua frontend in the corpus tool; separate ScriptHost tests cover the authored Lua changes.
- No third-party opener or Wine is used by these regressions. Source, binary and log hashes, including the retained RED and A/B runs, are recorded in the W0 after403 audit and frozen receipts. No plugin install was performed.

## 0.3.403

This experimental update improves saved script state, UVI MIDI handling, supported Kontakt sound sources, library browsing and articulation labels.

### Loading & files

- Request the first sample page in the same block when a resumed script callback starts a note.
- Keep supported oscillator tables resident so they remain available during playback and sample purging.

### Sound/engine (v1 ports)

- Use KONTRA v1's oscillator playback for the admitted saved Conflux wavetable state, including its source clock and supported form mapping.
- Restore Vista's saved legacy low-pass and high-pass filter slots and their cutoff-envelope routes using the v1 proxy.
- Retain 124 of Conflux's 182 saved Digital Multi sine sources, including their signed, subunit depth; the remaining 58 fade cases remain diagnosed.

### KSP scripts

- Temporary script writes no longer force a saved-state snapshot when they do not affect captured values.
- Refresh changed saved array cells while keeping each published snapshot coherent.

### UVI

- Send MIDI note-ons through the authored Lua onNote callback and preserve the input identity used by note-offs.
- Match favourites by bank UUID and program member so they survive library moves; missing and ambiguous matches remain explicit.

### Saved projects

- Capture script values directly into compact snapshot slots, removing the retained wide intermediate buffer.
- Preserve the previous complete snapshot if a new capture cannot finish.

### Editor/UI

- Combine browser search and library banners, keep format-specific navigation, and reuse catalog order across queries.
- Use authored Areia technique names for generated note labels when every key in the range agrees.
- Stop treating Analog Strings preset-browser and zero-height controls as articulation choices.
- Release retained toolkit render owners on window close and rebuild the canvas on reopen.

### Memory & CPU

- Reduce copying and storage for saved script snapshots, with sparse updates for changed cells.
- In the owner's two-preset CPU-editor measurements, installed-to-first-sound fell from 517.556 to 11.648 ms for Areia and from 357.407 to 6.574 ms for Dolce. These are scoped results from the source fixes; v1 still reached first sound sooner.
- Optional scheduler diagnostics help attribute deadline misses; instrumented runs are excluded from performance acceptance.

### Fixes

- Keep favourite identity independent of a bank's old file path.
- Keep articulation source IDs, trigger ranges and existing authored labels intact while improving generated captions.

### Known limits

- This is not complete Kontakt/Falcon replacement or certified authored-UI, script or sound parity. Unsupported source modes, live wavetable controls and Digital Multi fades remain open.
- The legacy filter port restores v1's proxy; native filter topology and control timing are unverified.
- Full widget operation remains incomplete, including the reported Conflux/Analog/Dolce gesture failures and some key-label associations.
- The editor close fix does not establish a production RSS reduction. Quiet CPU/load/RSS acceptance, protected installed UVI playback and real Windows/macOS DAW operation remain unverified for this batch.

### Evidence

- Corrected per-worktree product gate at `4725f39a`: 2,135 workspace tests passed, zero failed; 48 scheduler tests, 45 reader tests (one ignored), and three articulation normalization tests passed. Compilation, offline publication checks and GPU regressions passed.
- Installed Conflux and Vista source/filter checks passed; ten browser captures were produced and the library/preset states at both sizes were reviewed. Native scanner: 15/15 loaded and audible, retaining 12 Original views. Native quick corpus: 72/72 loaded, 68 audible, finite audio, zero audio-thread allocations, and no matched stage/audibility changes from 0.3.393.
- These native-only regressions use no third-party reader or Wine. They retain known covered-note silence and long releases; timings are not performance acceptance. Immutable source/log/binary hashes are in the W0 batch383 audit and release receipts.

## 0.3.393

KONTRA 0.3.393 brings together a broad set of loading, sound, scripting and editor improvements. This is an experimental release; the remaining limits are listed below.

### Loading & files

- Load supported older Kontakt presets and samples stored inside the instrument file.
- Open the requested embedded sample when several stored files have similar names.
- Reuse library path lookups and saved loading information to reduce repeated work.
- Preserve saved effect settings and signed modulation amounts in more Kontakt formats.
- Discover libraries through native paths and explicitly configured content folders.

### Sound/engine (v1 ports)

- Bring KONTRA v1 envelope, modulation and filter calculations into the supported playback paths.
- Use KONTRA v1's paired EQ processing and reuse unchanged effect controls.
- Preserve the order of tone shaping, instrument effects and rack routing.
- Keep larger note combinations within the engine's supported playback capacity.

### KSP scripts

- Read authored engine settings when a script asks for a value that has no live override.
- Preserve scheduled script callbacks when reopening an instrument from saved loading information.
- Restore saved Kontakt controls with one initialization pass instead of applying them twice.
- Carry saved script text, arrays and callback state through supported project recall paths.

### UVI

- Show catalogued banks as “Catalogued · loading limited” when their content cannot be opened.
- Display the affected bank, loading reason and available preset list.
- Prepare held-note storage before the first audio callback.
- Find named bank artwork and include UVI libraries in browser searches.

### Saved projects

- Release old restored-state buffers away from audio processing when reopening CLAP or VST3 projects.
- Preserve engine-setting information in saved loading data.

### Editor/UI

- Group Report entries with labelled details that scroll inside the Report window.
- Organize Settings into clearer sections and explain remembered window sizing on hover.
- Cancel MIDI learn when its control disappears and keep only one active assignment at a time.
- Keep instrument slots within the window and make keyboard shortcuts easier to discover.

### Memory & CPU

- Reduce repeated work for unchanged EQ and effect settings.
- In the measured Horns test with 256-frame blocks, typical audio-processing time fell from 2,884 to 1,942 µs, about 33%; this is a scoped result, and KONTRA v1 remains faster.
- Release temporary editor factories and avoid duplicate artwork pixel allocations.
- Reuse layout information while drawing the Original interface.
- Store repeated loading information more compactly.

### Fixes

- Decode embedded AIFF samples using the same checked layout as standalone samples.
- Distinguish untranslated instrument features from missing samples or impulse files in load reports.
- Prepare the Linux editor test display without downloading packages during display startup, with bounded retries if startup fails.

### Known limits

- Some Kontakt and Falcon effects, modulation, script actions, controls and artwork remain incomplete.
- Protected UVI banks can be catalogued while their sample content remains unavailable.
- All 56 checked Vista and Pacific presets had no translated sample or impulse resource failures; this does not establish complete sound or feature parity.
- Some Kontakt script settings may still fail to restore fully when reopening a project.
- Large libraries can still use substantial memory; CPU, loading speed and memory use are not yet equal to KONTRA v1.
- The newer resampling experiment and full voice-engine replacement remain deferred; complete sound comparisons and real-host timing remain unverified.
- Windows and macOS DAW compatibility and crash handling outside Linux remain unverified.

### Evidence

[Integration and validation](https://github.com/DerpcatMusic/KONTRA/blob/v2/w0-sweep-382/docs/audit-2026-10-09/w0-full-sweep-382.md) · [Horns CPU comparison](https://github.com/DerpcatMusic/KONTRA/blob/v2/w0-sweep-382/docs/audit-2026-10-09/w9-held-shape-acceptance.md) · [Vista and Pacific resource checks](https://github.com/DerpcatMusic/KONTRA/blob/v2/w0-sweep-382/docs/audit-2026-10-09/w12-vista-pacific-resources.md)

## Unreleased

### New

- Add a KSP doctor that reports per-symbol support status for .nki scripts, with JSON output and a strict mode.
- KSP scripts can call exp2, log2 and log10.
- Add a shared Luau runtime for UVI Lua scripts, with a bytecode cache; the interpreter remains the default JIT policy.
- Add an opt-in KSP-to-Luau front end with UI, persistence, menu, key-switch and host-request builtins. The interpreter remains the default.
- Read Modo Drum .pak packs. Their instruments do not play yet.
- Add Kontakt Gater processing and the Stereo Tune core, checked against native reference vectors.
- Add the Kontakt Surround Panner speaker law and block driver for fixed positions, with a static stereo mix.

### Improved

- Keyswitch articulations use zone selectors, and the keyswitch panel lists only real keyswitch and group-start entries.
- Diagnostics report voice drops separately from capacity refusals, so the cause is clear.
- The default streamed head budget is bounded by the host's Auto budget.
- Streamed 16-bit WAV reads decode frames directly and are substantially faster.
- Reduce per-block overhead in the sampler core: serial loops, PCM packing and cubic setup avoid copies and temporary closures.
- Convolution tracks nonzero input per block instead of rescanning the whole buffer.
- Packed sample-head chunks are shared across preset swaps instead of being rebuilt.
- UVI programs load and initialize with less copying: override application and the parameter bank are no longer copied per owner.
- UVI programs with unproved modulation laws load with flagged approximations instead of being refused.
- KSP support reports name the failing operation and input, and a data file that is not loaded says why.
- Filters now match Kontakt native reference vectors for Ladder HQ, Daft, Formant, PRO-53, Monark, Dual SKF, Versatile and Vowel in the tested cases.
- Kolor: the dry-path ramp and several shaper cases now match native reference vectors. Other shaper modes remain open.
- Time Machine uses fractional-period PSOLA and a measured tempo division table. It remains partial.
- Beat Machine zone slice markers are read and played in Beat Machine voices; the machine remains partial.
- S1200 modes and MP60 machine controls are carried into the engine, and MP60 runs as a host-rate source stage.
- Convolution reports only non-default Preserve Length and latency-bypass flags; unmodeled flags stay visible.

### Fixed

- Read stored Kontakt AHDSR envelope times in milliseconds, correcting envelope timing.
- Fix a Conflux level regression by keeping unmeasured $FILTER_TYPE_* constants opaque.
- Stream oversized resident and pinned sources under cold fallback, fixing a Conflux regression.
- Load wavetable zones that end in a partial cycle or start off the 2048-frame grid, playing whole cycles.
- Resample across eight octaves, so authored zone keys no longer narrow the playable range.
- Size script callbacks by held notes rather than a fixed 32 keys, so dense chords are no longer limited.
- find_group matches group names exactly and case-sensitively, as Kontakt does.
- EVENT_PAR_PLAY_POS now reads back the play position instead of being unimplemented.
- Load data-folder arrays synchronously during on init.
- Resolve $EFFECT_TYPE_* and $FILTER_TYPE_* constants to values measured from Kontakt; unmeasured ones stay opaque.
- Clamp UVI loop ends that run past the end of a sample, with a load warning, instead of refusing the program.
- Admit multipart UVI streams and route each bundle member through its program placement, so resident decoding keeps every member.
- Script modulation now drives UVI global Gain and Pan outputs.
- Linux editor gestures survive temporary X11 grabs, and clicks and double-clicks are kept when the host moves focus.
- Parts with sized interfaces no longer grow or overflow their patch.
- Preset swaps drop the previous preset's saved controls, and a Conflux swap now matches a fresh load.
- Instruments with untriggerable zones remain loadable.
- A loose bank beside a library's Samples folder no longer hides any library content.
- An empty control label now shows as empty instead of the default label.
- Fixed picture edges on script buttons and switches stay in place.
- Restore Conflux wavetable modulation.
- Zero-intensity and floating-point residue modulation routes are inert rather than reported as unsupported.
- The release-trigger counter runs only in release groups with a positive time.

### Known issues

- Kontakt and UVI parity is partial. Many effects, modulators and script actions are approximated, unported or unmeasured; Wow/Flutter, Chainsaw, Jump and Flair/Phasis/Vibrato are not yet processing, and Replika Delay is partial.
- Most UVI modulation is approximated. LFO, MultiLFO and DAHDSR parameter writes from scripts are not bound, and approximations are flagged in the load report rather than proven exact.
- Ample Sound .ascf containers are read, but zone mapping is not implemented. Multiple loops per zone, NKB bank program switching and multichannel sample files are unsupported.
- KSP scripts run on the existing interpreter by default. The Luau front end is opt-in, and none of the 696 extracted scripts is yet fully in its subset.
- 300 voices at 1% of the meter is not achieved, and the performance baseline predates much of this work. The Kontakt and Falcon comparison is not measured.

## Superseded drafts before 0.3.503

The material below was written before the reviewed fix ledger became the source of Nightly notes. It is kept for history and is not published.


### 2026-10-01 to 2026-10-23 (main 16f110c11, 436 merged PRs)

Reviewed summary. Status labels follow [ROADMAP.md](ROADMAP.md). "Parity" means a native reference test exists; "approximated" means a documented approximation (UnknownLaw or load-report warning).

- **Kontakt DSP and parity**: Ladder HQ controls and native vectors (#279, #285); Daft and Daft HP kernel (#284, #293, #304); Monark LP2, LP1 and BP at kemu parity (#318, #366); PRO-53 (#307); Dual SKF stereo and multi-rate vectors (#308); Formant I/II control ramps and live lanes (#305); Versatile sweeps (#315); Simple LP/HP driver chain (#292, #310); state-variable res .9 vectors (#290); Kolor shaper cases and dry-path ramp, partial (#410, #414, #417, #419, #422, #462, #470, #471); Gater kernel with kemu vectors (#375); Stereo Tune core bit-exact against kemu vectors (#387); Surround Panner speaker law and block driver, partial (#401, #405, #415, #418, #421); Cry Wah and DStortion parity (#190); Bite live lanes (#397); Chainsaw HM2 probe, still blocked (#376, #385); Kontakt AHDSR times in milliseconds (#476, #479); Wow/Flutter control laws applied, kernel still blocked (#317, #322, #325, #463); Replika mode-1 chain and head kernels, not fully wired (#323, #326, #328, #348, #425, #427, #428).
- **Kontakt sources and modulation**: Beat and S1200/MP60 machine controls carried in the IR (#100, #176, #223, #253, #258, #286, #297, #319); Time Machine PSOLA fractional period and tempo table (#202, #238); Kontakt envelope laws (#122, #183); external modulation route smoothing and zero-intensity routes (#203, #343); script modulation drives global outputs (#214, #215, #222); Kontakt Stereo Tune and bus amplifier laws (#180, #200).
- **Kontakt measurement**: native oracle compares authored mapping with Kontakt 8.13.1 (#60, #110, #205); Kontakt wave of parity probes and decompile ledgers (#152, #164, #166, #168, #233, #235).
- **KSP**: native `find_group` and array-bounds semantics (#261, #355); `$EFFECT_TYPE_*` and `$FILTER_TYPE_*` resolved to measured values (#356, #430, #432, #481); keyswitch articulations migrated to zone selectors (#436); exp2, log2 and log10 (#380); init-only commands promoted to full (#394, #395, #411); `read_persistent_var` full (#406); polyphonic variable scope and initializer semantics pinned (#384, #442); `EVENT_PAR_PLAY_POS` readback (#347); MT19937 start draw (#274); KSP diagnostics per symbol and `.nki` doctor (#379, #399, #81, #267, #270).
- **Scripting (Luau)**: universal `sampler-script` runtime, phase 1 and 2 (#452, #453); KSP-to-Luau front end, phase 3, differential harness and key-switch builtins (#460, #477, #478, #480, #482, #483, #485, #487). Not the default; 0 of 696 extracted scripts are fully in subset.
- **UVI**: approximated where the law is unproved: constant property reads (#364), shared-output activation (#374, #473), Frequency attachment (#391, #435), MultiLFO waveforms (#388), program-level LFO (#408), shared-output connections omitted instead of refused (#438); multipart streams and placement (#392, #413, #420); loop end clamped with a warning instead of refused (#423); script-init hot paths trimmed (#440, #445, #446, #454).
- **Loading, streaming and memory**: oversize resident sources stream under cold fallback (#358); default streamed head budget bounded to the host budget (#294); shared packed sample heads across preset swaps (#259); direct 16-bit WAV frame decode, 23x streamed read (#451); wavetable zones admitted with trailing partial cycles and off-grid starts (#264, #349); resampling covers eight octaves (#212); convolution reports only non-default flags (#118, #189, #360, #441, #449).
- **Performance tooling**: `sampler-perf` UVI scenario fixed and per-block counters, spike report and audio-thread allocation gate (#443, #444, #447, #464); `perf` counters and capacity sweep; DFD preload zero rule (#365).
- **Editor, MIDI and platform**: Linux gesture cancellation during X11 grabs fixed (#79); X11 focus clicks kept (#83); part height and popup fixes (#243, #254); MPE bend and per-note sensitivity tests (#281, #333, #334, #337); 96-semitone bend admitted in stale ingress tests (#333).
- **Corpus and CI**: corpus-health stable diagnostic codes and 0-error gate (#345), split render-thread misses by minor faults and preemption (#365, #437), settled note events before counting stuck voices (#424, #426); nightly keeps only the newest waiting run (#240); shipping examples built with the ci profile (#134); integration test shards (#75); draft PRs skip expensive jobs (#142).
- **Docs**: one-engine architecture (#96); KSP Luau subset and KSP surface map (#372, #484); Kontakt 8 parity matrix (#152) and per-row status updates (#166, #172, #174, #175, #177, #178, #179, #184, #190, #191, #193, #195, #196, #198, #290); full corpus survey and library gap report (#208, #354, #434).

#### Known limitations (2026-10-23)

- Survey 2026-10-23 (base `3ab06104d`, machine loaded): 1,312 of 1,395 instruments and programs load; 1,297 sound; 1,295 no failure. Kontakt: 682 of 682 single instruments and 53 of 53 multis load; one script compile failure.
- UVI: 577 of 660 programs loaded in the survey. The 83 refusals (79 SignalConnection, 4 Frequency) were changed to bindings or approximations by #435 and #438, which merged after the survey base; the count needs a re-survey.
- 15 instruments silent at a covered key (Dolce 10, Pacific Ensemble Strings 5); key 60 may be a probe artifact.
- Pacific Ensemble Strings "Cluster Risers" moved -23.7 dB against the previous run; not investigated.
- 300 voices at 1% of the meter is not achieved; the Kontakt and Falcon comparison is not measured.
- Performance baseline `fbc205f7` (2026-10-07) predates much of the performance work and records 98 audio-thread allocations on the UVI scripted pad scenario; re-measure before claiming zero.

### Fixed after 0.3.326

- Open protected Falcon and UVI content with the native format implementation.
- Bound UVI script evaluation by deterministic callback work.
- Stop zero-wait coroutine wakeups from exhausting a callback indefinitely.
- Publish complete authored UVI controls before audit replies while preserving live work limits.
- Retain member identity without persisting instrument member names.
- Yield long metadata census shards to waiting builds.
- Avoid duplicate runtime fault summaries on unchanged refreshes.
- Place part Tone before instrument effects as in KONTRA v1.
- Execute authored KSP MIDI object commands and return host file completions.
- Reject MIDI completions and captures for unpublished jobs.
- Preserve every completed Nightly snapshot and publish source versions in order.
- Show grouped failed UVI banks with accurate access causes and affected locations.
- Keep the piano keyboard visible in the current Sound editor.
- Keep wheel gestures inside dropdown and picker popups.
- Remove the repeated instrument name in Info.
- Preserve partial redraw coordinates for GPU vector rendering.
- Show each EQ band gain and apply the correct graph drag scale.
- Share identical immutable zone chains without changing audible PCM.

### Added

- Read-only Mapping workbench with shared Sound tabs, zone identity, waveform inspection and balanced audition.
- Optional numeric audit progress and grouped diagnostics with location sidecars.

### Internal

- Scalar and batched voices use one shared biquad sample kernel; no control or coefficient law changes.


### Fixed after 0.3.306

- Native script host recall now restores callback context, text and arrays; UVI host saves retain immediate scalar and custom state.
- Native UI uses deterministic work limits and reports asset/process watchdog expiry as incomplete. Picture cache rejects oversized wanted sets without endless requeues.
- Bank-volume samples reach their existing UVI resolver. Browser artwork, chrome, versioned Settings, upper-zone MPE and editor telemetry ownership are corrected.
- Native Ladder/Daft cutoff, Q and Gain routes are retained, authored zero-weight Multi LFOs stay bipolar Zero, and settled modulation results are reused without changing PCM.
- Correction: Contrabassoon's 748 ms onRelease sample ends at 2,248 ms. Its voice at the old 2,000 ms check was legitimate. Quick checks now wait for zero voices, with a five-second maximum release drain; the separate empty script-note gate cleanup now ships in this follow-up.
- Restore stable Attack/Release/Tone host controls alongside Volume and 2,049 generic IDs (2,053 total). Fallback envelopes exclude authored AHDSR/FLEX/amplitude routes; Tone bypasses exactly by default.

- Retire duration-zero source-owned UVI script gates when their samples finish, preserving explicit looping release tails and physical input ownership.

### Known limits for 0.3.326

- Global Tone is currently after part FX, before faders/rack routing; v1 pre-insert placement remains unverified.
- CPU/RSS and native-law parity remain incomplete. W9 streaming and W8 chain sharing remain excluded pending acceptance. Existing KSP compile and DSP gaps are retained in the fidelity ledger. Native persistence is restored in this follow-up; the exclusion below describes the historical 0.3.306 freeze.

### Known issues

- Some Kontakt and Falcon effects, modulation and script actions remain incomplete; controls and artwork can differ from the original interface.
- Saving and reopening a project may not restore every Kontakt script setting.
- Protected UVI banks can appear as catalogued with loading limits while their content remains unavailable.
- Large libraries can use substantial memory, and performance may be worse than KONTRA v1.
- All 56 installed Vista and Pacific presets passed sample and impulse resource checks on 0.3.381. Some features remain untranslated; full audio parity remains unverified.

### Fixed after 0.3.224

The user explicitly authorized shipping this alpha now. The former all-axis v1
parity release hold is superseded for this release. Falcon/UVI stays always on.
The version ledger accepts 81 additional logical fixes after 0.3.225; diagnostic
features and follow-up commits do not increment the version separately.

- Restore authored Native UI text/arrays, bindings, Canvas budgets, caption types,
  stack-safe lowering and bounded cancellable resource preparation.
- Preserve envelope initialization, exact ignored-event cancellation, delayed
  correct-offset staccato playback, keyboard gates and host lifecycle ownership.
- Reduce startup arenas, share script text/code, compact playback templates and
  construct group envelopes once. Analog's shared KSP code saves about 406 MiB.
- Restore Sound editor controls, native ladder laws, chain ownership, offset-cache
  ownership and validated timing alignment and articulation routes through rack growth. Add persistent catalog metadata and
  separate Kontakt/Falcon browser hierarchies.
- Port v1 cubic interpolation and unchanged pitch-ratio reuse; add native Pan,
  Formant I and LoFi processing. Include UVI bounded initialization, typed host
  lookups, Assistant font resources and fresh custom host-save capture.
- Verify native CLAP source and routing readback before accepting host receipts.
- Improve GPU recovery, diagnostics and embedded native-window teardown.

### Known limits for 0.3.306

- CPU/RSS/load parity with v1 is incomplete. W6 PCM is bit-exact but its quiet CPU
  improvement is unmeasured. W5 fresh A/B passes 8/9 cells; Conflux/256 remains
  slower: p50 11.2→16.2 µs and p99 about +35 µs, above the A/A noise floor.
  Dolce/32's original +14% result is retained; fresh matched A/B is within noise,
  and does not establish a causal improvement. Analog still uses substantial RAM.
- W12's pre-port native census finds dropped enabled/bypassed FX 377/1521,
  filters 2109/4962 and modulators 20404/24478. W15 admits Pan, Formant I and LoFi;
  a post-port recount is pending. Formant/LoFi approximations retain native
  fidelity gaps. Native filter subtypes 30 and 106,
  UVI MS20/Xpander, LFO/random sources and several
  modulation targets still lack full native modeling. See the W12 fidelity ledger.
- 21/22 scripted sample-family cells need native captures; static Pacific matches
  do not prove scripted family parity. Native UI paint does not certify every
  widget, OS/DAW gesture or source resource. Unmatched requested-face controls use
  a provisional disconnected/inert policy.
- Thirty of fifty audited multis retain 45 pre-existing KSP semantic/stage compile
  faults. Earlier Vista/Pacific missing counts described untranslated features;
  the source54d9 census finds no required sample or impulse resource failures
  across7 Vista and49 Pacific installed NKI presets.
- W9's predictive streaming stack is excluded: matched loaded RSS 819.92 MiB
  versus 380.16 MiB on its baseline. Native callback-context persistence 65e345e3
  is excluded because real Conflux recall rejects callbacks (InvalidInput).
- Untested zone-vector trimming, settled-modulation work, nativeAlways and new
  stream residency trials are excluded. Existing full Kontakt/UVI support ships
  as implemented; this release does not claim complete native parity.


### Accepted 0.3.225 — keyswitch panel and Program Change accounting

One reviewed logical defect advances the accepted counter from 0.3.224.
This source version uses the green integration checkpoint `67dafc61`.

### Added after 0.3.224

- Add the compact keyswitch panel, persistent articulation overlays, MIDI learn,
  conflict swaps and the appended input palette.

### Fixed after 0.3.224

- Route MIDI 1 and MIDI 2 Program Change through authored Program selectors.
  Selection changes without an ignored-input count; instruments without a
  Program selector continue counting the message as ignored.

### Known limits for 0.3.225

- Historical 0.3.225 checkpoint: packaging was held pending all-axis parity.
  The alpha authorization above supersedes that hold; the parity gaps remain.
- The full integration shards, reader checks, keyswitch and App-menu tests pass.
  The final shared scanner quick15 has 15 loads, seven Original UIs OK, eight
  with missing images and 14 matched audible auditions, with no measured
  regressions. Afflatus has no safe test key and is not auditioned.
- Historical 0.3.225 checkpoint excluded later worker changes. This alpha
  includes the subsequently validated integration work listed above.
- This is a normal local build using the host glibc. CI targets Ubuntu 22.04
  and checks a 2.35 maximum; actual CI artifact proof remains separate.

### Accepted 0.3.224 — Original view, verified readers and runtime storage

Sixteen reviewed logical fixes advance the accepted counter from 0.3.208.

### Fixed after 0.3.208

- Default to Original and preserve saved view selection, script/page state, control values and artwork identities through sparse runtime UI publications. Both selectors expose Original, Vector and generated KONTRA; fit uses both axes and explicit zoom remains exact.
- Admit native scalar edits through their bounded request/reply service, guard replaced source generations, and route UI alias effects by widget identity. Picker results become visible only after the owned worker operation closes.
- Decode complete verified versioned Program and NIS records. Program resource metadata uses the same public reader; neutral F1/F2 values retain their verified layout without guessed wallpaper semantics. Failed occupied FX slots remain diagnosed while valid siblings survive.
- Traverse live note/callback owners, flush callback outcomes at host-block end, wait for exact offline sample readiness with bounded failure, and retry transient protected pages with backoff and storage counters.
- Build CI Linux packages on Ubuntu 22.04 and reject GLIBC requirements newer than 2.35. Local user builds use the machine's normal toolchain.

### Known limits for 0.3.224

- The urgent local release uses the pinned ready W1 checkpoint and green integration no-run, root library, KSP/UI IR and release-core checks. Remaining post-install integration shards and current-HEAD quick regression follow; broader fix branches are queued.
- Authored image/font gaps, specialized widget support and full native sound parity remain separate acceptance work. The CI baseline check is implemented; its actual KONTRA 2.35 artifact proof follows CI execution.


### Accepted 0.3.148 — primary envelope processing and visible crash receipts

Two reviewed logical fixes advance the accepted counter from 0.3.146.

### Fixed after 0.3.146

- Render strictly admitted saved primary AHDSR sources on their native finite 32-frame control clock, preserving native attack/release curves, captured release state, interpolation, pedal ownership, retrigger and sample EOF. Four focused mathematical/resident-engine gates pass, including fragmented blocks and no audio heap allocations. Unsupported transforms retain diagnosed fallback instead of being silently admitted.
- Keep crash-report delivery status, report ID and an allowed public GitHub issue link above the Logs filters. Default warning/error filters, search changes and replacement histories no longer hide the receipt; Copy issue link accepts only the validated public KONTRA issue URL.

### Known limits for 0.3.148

- Fresh isolated f119048 passes locked all-targets/all-features checking and optimized builds, 600 library regressions (30 ignored), 89 playback regressions (four ignored), 59 reporter checks, manual export, seven saved-volume/phase checks, actual Logs/receipt/native-parent checks, four primary-AHDSR gates, Linux exported CLAP/VST3 processing and strict C++ editor attachment. Final version metadata and hosted release checks are verified separately before publication.
- Primary AHDSR admission is deliberately bounded to the proved saved format and unity target. Held-voice live envelope changes, transformed targets and other unproved formats remain unsupported or diagnosed. These gates do not establish whole-preset sound equivalence or fix the reported AREIA double attack by themselves.
- Mac/Windows DAW runtime, the reported uncaptured Mac opening-crash cause, a genuine production crash-to-issue roundtrip and full native Lua UI rendering remain unverified. Performance counters are test instrumentation, not a GPU or display-FPS result.


### Accepted 0.3.146 — saved phase, native parent validation and diagnostics

Four reviewed logical fixes advance the accepted counter from 0.3.142. Together
with saved sine-LFO volume, five outcomes follow the published 0.3.141 release.

### Fixed after 0.3.142

- Apply finite saved cycle phases from 0 through 1 to admitted retriggered sine pitch and volume LFOs on their existing shared clock. Native field identity, independent phase values, positive lag, fragmented playback, bypass, retrigger and zero audio heap are checked. Other waveforms, free-running ownership, live phase edits and complete Analog Strings preset parity remain unsupported or unproved.
- Validate CLAP parent API/null handles and VST3 platform/null parent/missing callback before interpreting native handles or attaching the editor. Actual adapter and C++ attachment regressions pass; the uncaptured Mac opening crash remains unconfirmed.
- Show readable script callback, argument, array/listener and source-availability context. Copy-all begins with a sanitized warning/error digest and retains structured events; unavailable-source inspection preserves navigation. Logs filter caches invalidate when journal identity or retained history shape changes, including equal-revision replacement.
- Include versioned Added headings and Known limits in generated nightly notes. The 18-outcome release fixture reproduces the prior omission, and all publication/signing scenarios pass. The published 0.3.141 body is corrected; its immutable original manifest omission is disclosed.

### Known limits for 0.3.146

- The exact pre-version candidate bdef149 passes locked all-targets/all-features checking and optimized builds, 595 library regressions (30 ignored), 89 playback regressions (four ignored), 59 reporter checks, seven saved-volume/phase checks, manual export, actual Logs and native-parent checks, and Linux exported CLAP/VST3 processing. Final versioned binaries and hosted release gates are verified separately before publication.
- Mac/Windows DAW playback, production crash-to-issue delivery, full native Lua UI rendering and whole-library DSP parity are unverified. Source-only envelope and drop implementations are excluded until their own acceptance gates pass.


### Accepted 0.3.142 — saved sine-LFO volume

One reviewed logical defect advances the accepted patch counter from 0.3.141.

### Fixed after 0.3.141

- Render admitted saved sine-LFO volume targets with their native bipolar depth and positive lag, shared 32-frame source clock, retained bypass state and decoded amplifier placement. Seven focused engine gates, including nonlinear ordering and zero audio heap, pass with 595 library and 89 playback regressions. Additional volume targets, unknown flags, separate invert/shaper/fade, live volume intensity edits and full preset sonic parity remain unsupported or unproved.

### Accepted 0.3.141 — reporter and native Ladder processing

Eighteen reviewed logical outcomes advance the accepted baseline from 0.3.123 to
0.3.141. Correction fixtures, receipt refinements and the already-counted worker
permit/zero-rate renderer fixes do not add duplicate counts.

### Added after 0.3.123

- Add single-rate native Ladder LP4 processing with the checked nonlinear four-pole kernel, signed Gain and correct insert ordering.
- Expose delivery status, report ID and an allowed public issue link in Logs, and restore only reviewed receipt fields on a later launch.
- Include exact locally archived crash originals and saved queue/delivery records in manual support exports, with missing, busy or changed-source coverage. Original copies are unredacted regardless of the structured-log toggle; OS-wide searches and binary dump copying are excluded.

### Fixed after 0.3.123

- Apply ordinary Ladder Gain, resonance and cutoff steps on the physical control clock, with consistent reset/retarget snapshots and fragmented-block behavior.
- Preserve the stored version 144–146 cutoff boundary-snapshot law independently of the Gain/resonance clock; unknown versions remain diagnosed.
- Keep enabled Ladder routes on persistent control ticks even when their depth is zero or target value is unchanged.
- Advertise configured CLAP MPE input while retaining preferred CLAP notes and existing output dialects. The Linux original-tone gate checks member/manager expression and pedals; automatic zone negotiation is not added.
- Retain filename mappings and raw timestamps when an optional calendar-date view cannot represent the saved value.
- Require the recorded host executable/application and documented fatal OS metadata before confirming a native crash. Missing, simulated, nonfatal, oversized and mismatched evidence stays unconfirmed; confirmation does not assign plug-in fault ownership.
- Redact authored cases of quoted Unix paths, Windows drive paths and UNC assignments while preserving useful exception/frame symbols. Unseen native fields are not certified safe by these fixtures.
- Bound recovered journal submissions to 128 startup and 2,048 recent records, with complete-original size/hash and explicit omitted-slot/drop accounting. Complete private originals survive acknowledgement and are not automatically pruned.
- Keep an active durable publisher’s lock and in-flight files out of stale-session recovery cleanup.
- Retain unconfirmed incidents privately without blocking later confirmed delivery; recheck up to 32 deferred records per registration and keep a discovery cursor across launches.
- Measure the complete serialized UTF-8 JSON body, including escaping, before network submission instead of estimating individual fields.
- Bound legacy/native buffered reads and stream complete oversized originals into a private archive before retiring local source slots. Prefix/suffix views do not confirm partial native JSON, and failed preservation leaves the source intact.
- Keep independent keyed incidents, durable legacy migration and exact-ID acknowledgement. After acknowledgement and permit release, bounded actor scans can drain later confirmed incidents in the same host; offline delivery or failed retirement stops continuation without losing pending evidence.
- Use recorded incident host/build/system metadata with explicit unknowns, and render/hash submitted evidence independently of a later reopening host or current build. Reopening context is a separate local diagnostics event, excluded from the submitted report.
- Preserve complete private evidence before recording a source-digest/recipe/limit-bound manual-export disposition for packets over the 16 MiB JSON limit. No acknowledgement, deletion or delivery is invented. Release the worker permit so other queued incidents can proceed once; changed evidence or policy is reevaluated. Logs and restored receipts truthfully say manual export is required and automatic retry is paused for unchanged evidence.

### Known limits for 0.3.141

- Fresh isolated source 4a4f5ca passes locked all-targets/all-features checking, test compilation and production build; all 477 tracked source hashes remain unchanged. Its frozen executables pass all 590 library regressions (30 ignored), all 89 playback regressions (four ignored), all 59 reporter checks and the manual-export gate. Parent 9c1f7d1 passes six Ladder, 10 BUFFR durable-file, 11 flight-recorder and one optional-date vendor regression plus Linux strict-MPE/native VST3 gates; the 4a correction changes only authored actor-lifecycle tests. Final versioned production identity and host checks are verified separately before export.
- Ladder High Quality oversampling, other Ladder modes and complete audible/native transition equivalence remain unproved. Known control/routing/PCM gates establish only their scoped laws; whole-library sonic parity is not claimed.
- The Linux strict-MPE CLAP and authored-tone VST3 gates do not establish Mac/Windows DAW behavior, automatic MPE negotiation, or the cause of the user’s uncaptured Mac crash. Complete native Lua UI support, unaccepted volume/UI work and new Rad/Keytar sonic claims are not included.
- Automatic reports submit bounded, sanitized evidence, not complete raw local journal/native history. Manual original copies are explicitly unredacted. The deployed service stores full submitted reports privately, applies server redaction and 30-day expiry, and publishes metadata summaries; a real production host-crash-to-private-store-to-public-issue roundtrip remains unverified.
- The already accepted upload-permit-release and zero-rate renderer fixes are not counted again. Queue continuation, receipt refinements and correction fixtures stay within their parent outcomes. Documentation and external service deployment add no plug-in fix count.


### Fixed after 0.3.115

Eight newly accepted logical fixes advance 0.3.115 to0.3.123; closing the already-counted VST3 optional-length outcome adds no duplicate count.

- Source-zone group/key edits now prepare on the worker in order. Init edits are installed before the bank becomes playable; runtime completions retain exact source IDs, budgets and restore freshness. Actual Conflux captures now select different physical wavetables rather than all playing the same fallback source.
- Held-note expression retains its original destination when MIDI channel modes or routing change, through pedals and release ownership.
- Modern version0x90–0x92 Ladder records preserve leading Gain, cutoff, resonance and raw native flags with byte-exact roundtrip. Ladder DSP was unsupported at the 0.3.123 checkpoint; the single-rate LP4 scope added above is separate.
- Legacy Delay saved sync units use host tempo across all rack scopes. Independent absolute/synchronized physical caches survive binary and JSON host state and stale paired restoration; unknown saved units retain explicit diagnostics. Native fixed-ring duration limits remain separate.
- Integer division/remainder by zero returns native zero without generating invented script faults. Bounds, unavailable MIDI context and real nonfinite-result errors remain visible.
- Automatic crash-report delivery releases its worker permit on completion, allowing later confirmed incidents in the same host. Invalid acknowledgements and offline failures retain pending evidence.
- Live and restored bypass reaches admitted saved pitch LFO sources, preserving paused source phase/fade and removing only the selected contribution. Wider waves, free-running clocks and live frequency remain unsupported.
- The native VST3 adapter now accepts negative optional note-length metadata. Real exported processing passes ten authored-tone cases covering anonymous/live note IDs, independent cents-frequency checks, sample offset 16 and explicit note-off. This closes the adapter gap disclosed in 0.3.115; the reported Mac keyboard/arranger payload has not been captured, so its cause remains unconfirmed.
- Frozen coherent core cfef7b0 passes the locked all-targets/all-features check, optimized production build, 555 library regressions (30 ignored), 89 playback regressions (four ignored), and the real exported VST3 gate. These checks establish the listed behaviors, not full sonic parity or Mac DAW crash resolution.

### Verified processing and diagnostics follow-ups

- Tube and Transistor Distortion now use their checked native scalar curves without the previous invented drive compensation. Independent signed boundaries, live Drive readback, group/rack routing and zero-heap gates pass. The separately reviewed Damping/DC corrections are recorded below; whole-effect parity and corrected actual preset replay are not established.
- Optional KSP `note_off` microsecond offsets now replace existing duration timers and retain the exact scheduled event under pool pressure. Concurrent-note, channel, timing, invalid-offset and zero-heap checks pass.
- Stale script restores now cancel before obtaining a fresh epoch or publishing old scripts/effects. Controlled races cover source/state replacement, generation changes and removal; unrelated gain edits preserve valid restores.
- Saved legacy unsynchronized fade-in now reaches eligible retriggered sine Multi pitch sources on the same 32-frame clock. Independent physical cursor/PCM, fragmentation, retrigger, lifetime and zero-heap checks pass. Synchronized fades, wider waveforms, free-running clocks and live frequency remain unsupported.
- The combined follow-up source passes 510 library tests and all 89 playback regressions, with 30 and four deliberately ignored cases respectively.
- Classic Saturation now uses the checked native piecewise polynomial Shape law and linear Output instead of the previous proxy. Independent scalar/ordered-processing/readback/zero-heap gates pass, alongside all 89 playback regressions. Matched Contradiction replay gains 17.78 dB RMS with unchanged onset, events, controls, cursor travel and loop bounds; the bypassed Catastrophic control remains byte-identical. These observations do not certify sonic parity. Enhanced/Drums modes retain explicitly diagnosed approximations.
- Recognized group-effect targets no longer disable an unrelated internal pitch LFO at the same numeric slot. Unknown source-parameter assignments remain guarded; legitimate effect assignments are retained.
- Signed and unsigned intensity writes now reach admitted internal pitch LFO targets using original source/target indices. Callback readback, physical-depth PCM, unchanged phase, alias persistence, seeded restore and zero audio heap pass focused gates.
- Native Constant loop start/length assignments now control eligible forward full-sample sampler loops on 32-frame ticks, preserve fractional cursor/crossfade/streaming, and retain the final release seam. Unsupported geometry and behind-cursor edits remain diagnosed and deferred. Voice diagnostics expose applied bounds separately from virtual position. Six matched real-preset captures confirm applied windows and held wraps; Prelude changes substantially, while Contradiction remains quiet. These observations do not certify sonic parity.
- Saved retriggered, zero-delay sine-only Multi pitch assignments now run through a note-owned clock and audio-rate interpolation. Planner fragmentation no longer changes that clock. Focused semantic/PCM/zero-heap gates and all 89 playback regressions pass; wider waveforms, delayed/free-running ownership and live frequency remain unsupported. Six actual reported-preset captures exposed a separately corrected slot collision; three matched actual captures confirm pitch-source travel in Rad Prelude and Beta Decay, with unchanged controls and a byte-identical Catastrophic negative control. These checks do not establish full sonic parity.

- Legacy and modern signed modulation aliases now share the verified pitch/cutoff laws and exact target identity. Other unsupported legacy target laws remain guarded.
- Saved negative pitch and cutoff modulation now applies the native target sign bit independently of invert, with raw records preserved and both directions checked against physical-depth PCM references. Loop modulation remains incomplete.

- Native convolution early/late Size ratios now survive Reverse, Auto Gain, predelay, callbacks and restore independently; editing one Size leaves the other intact. Older host states retain their prior uniform behavior. Native nonunit time stretch remains incomplete.

- Exact host note IDs now retain their routed ownership through sustain, script children and waits, delayed expressions, release tails, and aggregate NOTE_END delivery. Generated articulation keyswitches cannot take the musical note ID; mixed raw MIDI releases and scoped cleanup preserve unrelated held notes. Unverified host expression types remain diagnosed.

- `wait_async` now retains the original callback until its admitted NKA/IR operation has finished installation or failed. Unknown and completed IDs continue immediately; the bounded wait state retains event/channel context without polling or audio-thread I/O.

- Explicit positive convolution crossover now prepares separate early/late filters at unit Size and matching sample rates, with the checked 50 ms blend and original source duration. Nonunit Size, automatic crossover and resampling remain approximate.

- macOS delivery adds one universal Intel/Apple Silicon installer for CLAP, VST3 and the standalone app, with standard plug-in folders and `/Applications/KONTRA.app`. Publication requires timestamped Developer ID Application product signatures, a Developer ID Installer package signature, Apple acceptance and validated stapled tickets. Separate architecture ZIPs retain notarized DMGs for compatibility. Existing owned signing credentials are reused; there is no unsigned fallback. Real submission IDs and package/product hashes are recorded in the release receipts.
- Modern signed cutoff intensity now uses the independently corroborated cubic law and inverse readback, with normalized bounds retained. The actual Conflux saved value showed the old linear conversion overstating its depth by about 4,877×; other modulation laws are unchanged.
- Native LFO waveform-specific v0x71–v0x73 records now decode and write losslessly with strict size/flag checks. Forty-two authored combinations and 60 actual selected records pass. LFO playback/freewheel behavior is still incomplete.
- Nonunit convolution Size now reports its resampling approximation and affected Auto Gain energy explicitly. Native pitch-preserving IR time stretch remains unavailable.
- Convolution IR high/low-pass bypass follows the checked native frequency/rate boundaries. Digital-pole decay padding fixes the reproduced near-Nyquist response truncation; independent numeric and shared routing/lifetime checks pass. Finite padding does not establish native prepared-length identity.
- VST3 editor attachment now opens the editor before processor activation or state restoration. The actual host-shim regression reproduces the old blank-view path and passes attach/resize/restore/reopen after the fix; the specific Void report remains unconfirmed.
- Native v0x103 external modulation now decodes with exact opaque-footer preservation. Six wavetable group parameters omit the module slot byte, fixing 21 misaligned target records. All 3,752 actual Conflux assignments now decode and roundtrip byte-exact, including 366 pitch-bend sources. Shaper decoding remains strict; footer semantics and target DSP support remain separate.
- Live convolution Reverse and Auto Gain now rebuild on the worker and survive saved-state restoration; stale completions retain the current kernel.
- Enabled eight-knot IR volume envelopes use checked amplitude interpolation before Auto Gain, with predelay kept separate. Malformed active curves retain diagnostics.
- Delayed script callbacks can update prepared short and automation control names without allocating on the audio thread. Three actual Conflux preset probes each pass 750 listener blocks without the property fault and confirm their authored −12/+12 semitone tuning; authored arithmetic warnings remain visible.
- Resident wavetable taps dispatch PCM once per block. Matched-output thread-CPU measurements show 1.55x/2.00x/1.73x speedups for F32/I16/I24 in this kernel; packed decoding and total-plugin performance are not included in that claim.
- Nightly packages build alongside the unchanged shipping checks; publication still requires both to pass for the same source commit. Duplicate ordinary main-push CI is removed. Hosted timing improvement is not yet measured.
- Uncaptured native GPU failures now reach persistent diagnostics after device rebuilds; original stderr and device-loss recovery remain intact. Void blank-editor repair is not yet confirmed.
- Registered per-note MIDI2 brightness now follows its retained note, including start-only modulation. Ambiguous host brightness is explicitly diagnosed instead of changing channel CC74 or a newer same-pitch note.
- Saved convolution Auto Gain now uses the checked prepared stereo-energy rule, threshold and cap while preserving dry output. Unequal early/late shaping remains approximate.
- Saved pitch and filter/EQ envelope bypass now uses the verified source flag, advances its clock and resumes without restarting. Amplitude/Flex bypass lifetime is still under investigation.
- Wavetable source records are read and edited without losing opaque bytes. The engine prepares resident 2048-frame cycles with note-based pitch, live position, and verified Linear/ASYM2MP phase forms. Nine actual Conflux captures across three patches confirm octave pitch ratios and zero underruns. Preset tuning and scripted pedal behavior remain under investigation. Native bandwidth-table preparation is missing for all quality settings, including High/Best anti-aliasing. Unsupported states are counted instead of played as ordinary samples.

### Fixed after 0.3.64

- Structured nightly notes now retain reviewed processing and version-specific fix headings instead of falsely reporting no reviewed changes.
- Modern signed pitch intensity now follows the checked cubic conversion and retains 24/36-semitone values; non-pitch modulation laws are unchanged.
- Convolution Reverse now reverses the source impulse response before rate conversion and predelay. Auto Gain uses the verified prepared-energy law, and live switches and eight-knot IR Volume Envelope passed focused processing checks. Unequal early/late shaping remains approximate.
- Large load dependency lists now use bounded typed journal chunks that can be reassembled completely in support reports. Source excerpts and path redaction remain intact; oversized individual values still report truncation.

### Added

- Bounded Falcon/UVI metadata inspection and format/runtime research document
  the next compatibility requirements. This does not add Falcon playback.
- A Rust dependency audit records reuse opportunities and verified runtime limits.

- Headless `audit-patch` accepts explicit snapshot and program selection while
  using the existing import, script, bank, effect and paced note/chord path.
  It retains structured load stages and diagnostics without opening an editor.

- Experimental Bitwig VST3 project inspection and explicit SavedMulti-to-KONTRA
  migration create a new project copy and report; source bytes are rechecked and
  retained. Shared plug-in-state entries and overlapping device mappings are
  rejected instead of changing multiple devices through one cached state.
- Independent authored performance pages expose each script slot's prepared UI
  buffer. Footer selection keeps callbacks on the selected owning slot; late
  publications and replacement epochs retain that ownership.

- Cached instrument snapshots appear in a compact header preset row with owned
  category labels, alongside the existing explicit snapshot picker/drop workflow.
- Independent user UI zoom preferences and persistent global editor window size.
- Native file-picker callbacks for supported KSP file selectors, with prepared paths
  and retained asynchronous callback routing.
- Imported Creator Tools performance-view controls for supported exported records,
  including null lists for empty menus and the initialization-only constraint.
- Declared JPEG resource names share the image decoder instead of being discarded.
- Prepared rack storage and viewport rows remove the previous fixed part cap; browser
  drops can append parts in the empty canvas. This is a feature, not a fix-count unit.
- Persistent library display aliases use the detached catalog without renaming
  library files. Native Reveal validates its target path before dispatch.
- Bounded source-identity diagnostics identify unsupported Kontakt 8 wavetable
  playback instead of representing it as supported sample playback.

- Every nightly records reviewed Added/Changed/Fixed/Known limits deltas, complete
  shipped public commit messages and merged PR descriptions in its release body and
  versioned manifest. The previous release source is the comparison baseline;
  source checkpoints remain traceable even when exports squash private history.
- Load supported snapshots from the instrument header picker or an explicit
  header drop. Parsing and base validation run on the loader before changing the
  active source; base and snapshot paths survive host state and KONTRA multis.
- Save successfully applied native script parameter edits alongside persistent
  script variables, and seed authored initialization getters during restoration.
  Prepared storage avoids audio-thread growth; refresh visits edited slots rather
  than every default parameter in a large library.
- One package version and build identity across CLI, standalone, plugin metadata,
  About, diagnostics and package manifests. Identity includes the actual full Git
  revision, optional export source revision, UTC timestamp, target, profile and features.
- Deliberate SemVer release preparation and reproducible nightly prereleases, with
  focused local checks and documented contributor workflow.
- Searchable structured Logs with failed-load context, source locations, bounded
  recent history, and private support reports that include retained crash journals.
  Reports identify omitted events, write errors and partial journal coverage.
- Script condition inheritance between successfully initialized slots, native
  pedal/release conditions, and additional supported script syntax and zone queries.
- Native NKSN snapshots applied to an explicit base instrument, including supported
  saved controls, instrument/group FX, known envelopes and modulation assignments.
  Three Analog Strings snapshots have production state checks and two decoded IRs each.
- Bounded asynchronous `load_array_str` reads and explicit-path `save_array_str`
  writes for typed NKA files, with retained array/UI revisions, completion callbacks
  and header, value, resource, capacity and write diagnostics. Three real Analog
  factory loads and the retained rhythm table are verified; read callback/install/
  refresh paths record zero heap operations over 7,800 blocks of 128 samples. Two
  unchanged-script browser-star callbacks write a copied favorites file with fresh
  byte readback, restore its original bytes and record zero audio heap operations
  over 6,300 blocks. All 11 original metadata files remain unchanged. Mode-based
  saves and external file dialogs remain unavailable, returning status 0.
- AHDSR/Flex envelope record writers that preserve opaque metadata. The selected
  782-file / 788-program corpus verifies 230,627 byte-exact record roundtrips and
  edited-value readbacks with zero errors.
- Typed native LFO parsing and writing for known fields, with 9,600 actual-library
  chunks round-tripping byte-for-byte. Eligible retriggered sine Multi pitch
  sources now play with proven legacy unsynchronized fade-in; broader waveforms,
  free-running clocks, synchronized fades and live frequency remain unsupported.
  Some tables remain raw, and typed metadata alone
  does not establish DSP behavior.
- Named bitmap font loading for 256-glyph Windows-1252 RGBA strips declared during
  initialization. Actual Areia Advanced resources verify two 256-glyph, 14-pixel
  fonts with variable advances and the native gray/orange switch-state change;
  broader font compatibility remains unverified.
- Internal slot-to-slot MIDI2 note-controller callbacks, registered/assignable/bend
  values, forwarding, waits and startup persistence delivery. External MIDI2 input
  and multi-script MIDI-input callbacks remain absent; the selected 13-script
  census contains no uses, so this does not establish an actual-library benefit.
- Lossless native v2 filename-table records preserve segment kinds, UTF-16 units,
  full timestamps, uninterpreted sample records and trailing metadata. Three actual
  tables covering 102,026 sample references pass byte-exact and edited readback checks.
  Existing raw chunk writing was already lossless; this adds typed editing access.
- Imported zero-crossfade alternating sample loops share reflected playback mapping
  between resident and streaming readers. Cache and NKI export retain their direction.
  Crossfaded alternating loops retain metadata and warn about forward-crossfade fallback;
  endpoint/interpolation equivalence with Kontakt remains unverified.
- Opt-in native UI timing capture records one bounded ten-second drag window and
  summarizes it on the diagnostics worker. It measures application callback and
  presentation-submission time, without forced GPU synchronization or display-FPS claims.

### Changed

- Paced headless audit JSON records first stream underruns, voice consumption and
  delivery status, and labels the uncapped voice pitch ratio precisely. The checked
  JSON helper and source/onset metrics are observability features, not additional
  fix-count units or throughput improvements.

- Rack headers give instrument titles more room beside
  compact MIDI and output routing controls. Combined 900/1180/1920 viewport
  bounds and routing/navigation/mute/remove callback checks pass.

- Logs search is simpler and copying includes complete retained diagnostic details;
  event text is owned before the query changes. Rack header artwork is more visible.
- Encrypted preset access failures explain the lookup boundary; XML fields tolerate
  surrounding whitespace. These diagnostics do not add keys or bypass encryption.
- Group processing follows decoded pre/post Amplifier insert order for supported
  filter and drive stages. Interleaved filter states remain out of shared lanes;
  effect indicators disclose partial processing instead of implying every FX runs.

### Fixed

- Authored performance-view background colors survive live publications; both
  focused background gates passed. This does not establish every layout or Lua UI.
- Native AR identities select the intended LP/HP/BP response and pole count. Native
  Daft LP/HP identities use IDs 70/71; unproved IDs 106/107 remain unsupported.
  These are two separately reviewed family-identity corrections.
- Native Daft cutoff/resonance decode after the retained leading parameter.
  Coefficient/PCM changes, live readback and zero-heap checks passed; the leading
  parameter remains preserved without an invented gain law.
- External modulation v0x104 records preserve opaque footer bytes through checked
  vendor parsing/roundtrips. This does not implement every route or opaque field.
- Physical note-off ownership survives channel mode changes. CC120 stop fades
  clear held ownership so late key-up cannot start post-stop release tails.
  These are separate routing and All Sound Off corrections; the physical-sibling
  follow-up is part of the same CC120 fix, not another count.
- Higher native LoFi frequency values sample more often without resetting held
  samples or clock phase. Authored direction/endpoints/retuning and zero-heap
  checks passed at 44.1/48/96 kHz. Six matched actual checks changed four outputs
  toward effect-off baselines and left two unchanged; the law is not calibrated
  against Kontakt and sound parity remains unverified.
- The first maximum-consumption stream block remains resident; the 4,096-frame
  RAM-coverage and zero-heap gate passed. This does not claim that all realtime
  underruns are eliminated.
- Documented `ui_menu` `$CONTROL_PAR_VALUE` queries return the selected entry
  index. Native persisted menus restore entry positions with retained origin/
  host state. Getter and persistence are two distinct corrections; both gates passed.
- Template-named snapshots bind to the checked instrument while rejecting foreign
  names/truncated metadata. Explicit modulation removals retain sibling identities;
  additions, replacements, malformed records and incompatible FX topology remain
  rejected. Binding and removal are two independently reviewed fixes.
- GPU startup diagnostics retain their cause and identify Linux embedding failures.
  This improves diagnosis; repair of Void blank windows or every host embedding
  failure remains unverified.
- Headless runtime faults retain callback/event actions, array variable/index/
  length and bounded readable source context. The zero-heap gate passed; repeated
  events update the latest action without growing or duplicating retained faults.
- Ownerless UI/listener CC and MPE actions use the configured part home; the
  routing/zero-heap gate passed. MIDI-owned callbacks retain their ownership.
- Legacy AHDSR cutoff intensity reaches the filter through the checked cubic depth
  law. Authored PCM/readback/zero-heap gates passed; original preset processing and
  calibrated Kontakt sound parity remain unverified.

- Persistent load journals retain warnings beyond the bounded report examples;
  an actual Areia load delivered all 1,125 warnings. Off-thread burst capture and
  export also retain 4,098 warning records with explicit delivery status. These
  are two distinct corrections: capture coverage and delivery under bursts.
- Explicit scripted AHD Only uses the shared envelope kernel; its focused gate
  passed. This does not establish every imported envelope or Kontakt's sound.
- Script-only Kontakt v3 snapshots apply saved script state without replacing
  native instrument state. Vendor checks and independent application of three
  actual Conflux presets with audio outputs passed. Initial realtime underruns
  were observed; later four preloaded Conflux runs at `1f62da4` had zero underruns
  and complete journal delivery, without a general realtime-performance claim.
- Library-root archive samples resolve from nested instrument directories.
  Authored checks, actual archive members and 1,985 playable Conflux zones passed;
  loading does not establish full-library playback or sound parity.
- Counted native modulation arrays preserve 64 external slots, high slot identities
  and unknown bytes. Vendor and actual byte-preservation checks passed; preserving
  records does not demonstrate processing every modulation source or target.
- Bounded Kontakt v0x103/v0x104 source headers retain known identities without
  interpreting opaque source state. Modern v3/v4 compact snapshot records preserve
  mode-dependent source fields and counted slots through checked roundtrips.
  These are parsing/preservation fixes; the later three checked Morphology
  applications below do not establish complete native processing.
- Release-trigger Note Mono cuts only matching sounding release tails and remains
  preserved when writing native start-condition records. Both production checks
  passed; arbitrary imported start conditions remain outside this validation.
- Native group Send Levels taps feed the existing instrument returns in decoded
  order. Analytical dry gain, return tails and zero-heap checks passed; this does
  not establish Kontakt bus assignment or sound equivalence.
- Fractional image resizing preserves coverage and transparent edge colors while
  retaining already enlarged pictures instead of resizing them repeatedly. Both
  focused image/UI checks passed; no GPU throughput or universal resource claim.

- Ordinary import preserves readable modulation slots when one bounded sibling
  record is undecodable. Unknown slots keep their positions and precise warnings;
  valid envelopes and later target identities survive. Snapshot decoding remains
  strict, and malformed container boundaries still fail.

- Process all eight native filter/EQ inserts and up to 32 sections instead of
  truncating supported chains. Eight four-band GEQs match the existing rack
  reference with live slot-7 edits and no audio heap operations; fixed state
  increases by 1,248 bytes per voice.
- Format, clone and retire large persistent script/native values outside the
  editor mutex. Revalidate the script epoch before committing a snapshot;
  unchanged host JSON and prepared audio buffers are retained.
- Clicking either diagnostic row text line selects the event, as does its blank
  area. Native Copy all verification retained 148 events and eight source excerpts.
- Rejected zone mappings identify the offending field/value, original ranges,
  source/version, zone, group and sample. Validity checks remain strict.
- Preserve native GPU surface errors and flush startup stages before driver calls.
  This improves blank-editor diagnosis; a Windows driver crash is not reproduced.
- Read generated dependency license JSON explicitly as UTF-8, fixing Windows
  packaging on a CP1252 default locale. The non-ASCII generation regression passes.

- Native group feedback compressor, limiter, Solid Bus
  Compressor and Transient Master stages reuse bounded rack processing at the
  decoded Amplifier split. Rack-reference PCM, native edit/readback and zero-heap
  checks pass; this does not establish native Kontakt parameter or sound equivalence.
- Import and NKI writing retain native group start records.
  Record preservation does not implement every start condition or establish
  arbitrary imported-preset editing.
- Periodic audio snapshots wake a separate managed worker
  during instrument loads. Cumulative playback counters retain their baseline
  across generation changes, preventing repeated totals from appearing as new
  drops or underruns; independent wakeup, bounded handoff and exact delta
  regression checks pass.

- Parse/runtime diagnostics now show readable bounded source context with slot,
  line/column markers and the relevant command arguments. Serialized event data
  keeps the excerpts visible in journal/export and Logs Copy all; both exact
  diagnostic regressions passed on the corrected compiled binary.

- Decode complete extended PCM WAVE format descriptors, including the checked
  20-byte fmt records; two actual supplied samples decode fully. All 15 authored
  descriptor cases and the combined audio regression passed.
- Reveal actual library/log directories after validating the path, including
  extended Windows drive and UNC spellings. Explorer launch remains unverified.
- Resolve relative encrypted-preset paths and whitespace-delimited library access
  fields with actionable missing-data errors; right-key and wrong-key checks pass.
  The unavailable Emotional Piano payload has not been independently validated.
- Accept documented symbolic MAIN/GROUP/INSERT level-meter chain selectors while
  retaining rejection of invalid selectors. This is not a claim that every meter
  source works. The actual checked preset initializes three script slots with
  378, 22 and 1 controls without errors; Lua, unsupported taps and full playback
  remain separate limits.
- Accept Creator Tools null menu lists for exported empty menus. The actual checked
  Conflux resource loads 378 controls in 11 families; 100 callback operations show
  no measured heap operations. Lua UI and unsupported level taps remain explicit.
- Preserve independent script-slot pages and their callback ownership through
  footer changes, delayed publications and replacement epochs. Authored UI/backend
  checks pass; the Circle Bells payload was not available for validation.

- Forward initialization RPN messages after receiving script slots initialize.
- Persist global editor window size instead of losing the saved size between editors.
- Decode native Kontakt 8 flat filename tables and explicit effect-slot identities;
  retain supported records instead of rejecting their valid layout or misreading slots.
- Archive errors retain directory signatures and exact read boundaries. Rejected
  samples and loop bounds retain their actual failure cause and typed skip counters.
- Count KSP faults omitted by the retained source-location cap, and retain command,
  argument, signal and runtime-value context for invalid note/listener operations.
- Dispatch the documented legacy PGS callback spelling.
- Keep the rack welcome drop area and scrollbar gutter stable while accepting
  append drops on the empty canvas.
- Resolve declared JPEG resources and route decoded module envelope bypass and
  supported modern target depths through the existing processing paths.
- Schedule millisecond and beat listeners independently; registration, disabling
  or retuning one clock no longer overwrites the other clock's phase or generation.
- Preserve release tails during offline overload rendering.
- Preserve native group insert order around the Amplifier, including separate
  state for interleaved filters and honest partial-effect indicators.
- Resolve the authored compressor native ID through its documented KSP names.

- Group drive processing retains all eight native insert slots. A third drive
  stage was previously discarded, leaving Analog Strings' Saturation control
  editable without reaching its DSP.
- Original views retain unchanged control subtrees across live publications,
  while changed table rows, active gestures and replacement epochs rebuild.
- Windows editors default to Direct3D 12 instead of implicitly initializing
  Vulkan. Explicit `WGPU_BACKEND` selections remain authoritative; renderer
  startup, adapter details and recoverable failures enter persistent diagnostics.
  This avoids the reported Intel Vulkan path by default, but has not yet been
  verified against that FL Studio crash on the affected machine.
- Live UI refresh skips unchanged menu rows, and repeated identical indexed
  integer writes no longer dirty entire table snapshots. Listener behavior and
  audio work budgets remain unchanged.
- Import reuses one decoded filename table for samples, resources and impulses,
  avoiding three repeated full-table decodes in large instruments.
- Failed or canceled load reports retain their status and cause when diagnostics
  or artwork from the active instrument arrive later.
- Native SV Notch 4 filter type 58 uses the existing four-pole processing path;
  an actual Accordia resident-sample render now changes its PCM with zero render
  heap operations. This does not establish Kontakt sonic equivalence.
- Vectorized views retain broad pictured value graphs and their authored
  callbacks while continuing to replace ordinary knobs and faders.
- Compiled script programs share immutable UI revision-owner maps between
  runtimes. Revisions and mutable values remain local; matched Areia callback
  measurements show unchanged cost, without a runtime-speedup claim.
- Leaving or unfocusing an editor cancels delayed pointer restoration, including
  release events queued before the next frame. Popup menus capture hover and outside
  dismissal clicks so tooltips and underlying controls cannot cover or activate them.
- Solid G-EQ gain captions use the existing DSP's signed decibel conversion instead
  of raw normalized integers; this changes display text without changing its gain law.

- Shared controls recover movement before the drag threshold, so closed physical
  mouse paths return to their starting values at 100%, 150% and 200% display scale.

- Host position, tempo, play/stop and time signature reach script callbacks before
  MIDI input. Song position advances within the block at callback sample offsets;
  bar duration follows the host meter. Start/stop listener subscriptions are independent.
  Beat listeners still use elapsed-clock phase; missing host timeline validity is
  an upstream limitation, so unavailable beat position cannot be distinguished from zero.
- Repeated same-sample group parameter restores use a preallocated address lookup.
  The checked Areia Core F3 channel-overlap burst drops zero writes instead of 854,
  without increasing queue capacity; measured event-plus-render time stays near
  baseline at 3.56 versus 3.53 ms. Distinct-sample timing and latest-value reads remain.
- Instrument replacement clears previous script state and convolution settings;
  source epochs on both live-request and snapshot queues reject stale updates.
  A native Areia-to-CHORUS transition verifies the new logo, controls, header and
  playable range, resolving the observed cross-instrument state contamination.
- Physical note ownership, delayed callback cancellation, MIDI stop ordering and
  selective sound-off across articulation channels sharing one engine channel.
- Sustain release bookkeeping, generated-note lifecycle and MPE expression/tuning
  routing in the covered playback paths.
- Script-selected native release groups now start for all note durations when
  automatic release triggering is bypassed. Vista Harp and two Pacific Solo Harp
  presets restore exactly four damper voices on ordinary and pedal releases;
  their envelope/swell behavior still lacks Kontakt reference validation.
- Bounded physical-input CC120 cleanup and Panic termination for previously lingering
  Areia, Dolce and CHORUS script lifetimes. Reset restores recorded library device
  defaults alongside standard controllers in native playback and scripts, fixing
  all 12 previously silent legato fresh-note cases in the focused rerun.
  The six-patch, 24-case run covers 670,704 blocks with zero measured render-thread
  heap operations, nonfinite samples, drops, underruns or offline duration overruns;
  every fresh post-Panic voice has positive gain/envelope levels and final held,
  voice and pending state is clear. Three renders with effects disabled confirm
  audible CC121 recovery and baseline-matching CC120/Panic recovery.
- Zone ID mapping after import filtering and source-parser allocations in validated
  preset/container paths.
- Browser scaling/layout and selected DSP effect processing paths.
- RV2 Reverb Time captions use the existing DSP time conversion; the checked
  Areia Advanced state displays 1099.5 ms. This does not establish Kontakt's law.
- Original interface wallpaper page offsets and viewport rendering, authored fader
  travel, factory font color/state inheritance and explicit caption text colors.
  Factory glyphs still use the bundled font approximation.
- Editor publication of live script views through the existing bounded buffers,
  without formatting diagnostic reports or writing journals in editor frames.
- Delayed script callbacks now publish font, caption alignment and text-offset
  changes, including Analog Strings' centered Original-mode volume/FX captions.
  Native Original captures cover 27 playable cases across nine libraries at
  device scale 1.5; selected callbacks are checked, not every control action.
- Warm Vectorized CPU planning drops 78% in one bounded paired Analog trial
  (2.567 to 0.566 ms). This does not establish overall GPU/frame/input latency.
- Scalar edits avoid copying the imported interface under the view lock. A matched
  Analog trial measures mean edit submission at 1.152 to 0.000240 ms; whole observed
  frame means are 4.223 and 4.395 ms, providing no frame-rate improvement evidence.
- Changed publications reuse unchanged control storage; a separate matched Analog
  trial measures publication mean at 0.122 to 0.047 ms. Its final macro update copies
  3 of 934 controls; startup updates copy more. Drawing uses published revisions
  instead of scanning all control properties every frame. Native FPS remains unverified.
- Internal pitch AHDSR routing to voice modulation, selected filter coefficients
  and worker-built convolution cutoff processing. These changes do not establish
  Kontakt parameter-law or sonic equivalence.

### Compatibility

Kontakt preset import, scripts and playback remain partial. Successful import or a
passing synthetic test does not establish sonic parity for every library. Existing
compatibility notes and unsupported-operation diagnostics remain applicable; these
changes do not announce complete format, script or sound equivalence.

The focused offline results do not certify live-host deadlines or every library.
Opaque snapshot source state and unknown saved scalars are warned and remain unapplied.

Analog Strings' live factory-preset and rhythm menus use bounded NKA reads; selected
menu checks do not establish that every action or preset works. The installed
header-favorite preset ID is absent from its registry, preventing that lookup from
updating favorites; no supplied IDs were repaired or compared with Kontakt.


### Known limits

- The `1bbd31c` actual six-Analog replay returned empty runtime-fault lists;
  Prelude3's earlier out-of-bounds fault was absent. Three native convolution IR
  ordinal/caption selections and host roundtrip ran through 750 blocks. These
  validate the checked processing/state paths, not Kontakt IR sound equivalence.
- Three actual Morphology snapshots (Default, PlipPlop and Whistling) applied to
  the checked bank with 933 groups, 3,495 zones and 458 samples, with zero init/
  runtime faults, nonfinite samples or observed underruns. Partial source warnings
  remain. Each run recorded `truncated_events = 1`: its roughly 91.8 KB
  `load_finished` information event exceeded the event bound. Writer-drop,
  write-error and retention-loss counters were zero; truncation was not zero.

#### Semantic coverage and remaining gaps

| Area | Checked scope | Remaining gap |
| --- | --- | --- |
| Snapshots/source records | Known bounded headers, counted slots, template binding/removal; three actual Morphology applications | Opaque source state, arbitrary topology edits and complete-library compatibility remain unsupported/unverified. |
| Native FX | Authored AR/Daft identities/layout, cubic legacy depth, live PCM/readback; six matched actual LoFi checks | Original Kontakt frequency/drive/depth calibration and full processing parity remain unverified. |
| Controllers/menus | Authored configured-home routing, value indices, persisted positions and physical ownership; actual six-Analog replay | Every library callback, Lua UI and host workflow remain outside this evidence. |
| Diagnostics/editor | Bounded actions/excerpts and startup causes; zero writer-drop/write-error/retention-loss in the checked Morphology runs | One truncated information event per Morphology run; Void window/embedding repair remains unverified. |
| Streaming/convolution | First-block RAM/zero-heap gate; checked zero-underrun preloaded runs and native three-IR selection/host roundtrip | Uninterrupted realtime performance and Kontakt convolution/sound parity are not established. |
| Wavetable DSP | Resident 2048-frame cycles, note-based pitch, live position and Linear/ASYM2MP phase forms; nine actual Conflux captures | Native bandwidth-table preparation and High/Best anti-aliasing remain missing; no complete wavetable or preset playback parity claim. |

- The accepted parsing/preservation gates cover bounded known source headers,
  modern compact snapshots and counted modulation slots. Three later checked
  Morphology snapshots apply, but successful decoding/application does not
  implement unsupported DSP or establish complete-library compatibility.
- Three actual Conflux snapshots apply independently, and library-root resolution
  loads 1,985 playable zones. Initial runs had realtime underruns; four later
  preloaded runs at `1f62da4` had none. These limited checks do not certify
  uninterrupted performance, complete-library playback or Kontakt sound parity.

- Root's shipping-profile focused checks through `e68d28c` passed 20 library checks
  and all 83 playback checks (4 ignored). NI's 13 focused checks passed. These results
  do not certify every actual library or a Windows/macOS DAW.
- Actual Conflux import/initialization at `0c0019a` reports no initialization
  errors and exposes 378 controls. This does not validate full playback, Lua UI,
  every control or actual meter signals; unsupported level taps remain explicit.
  Circle Bells' multi-page interface was not available; authored page tests do
  not certify that library.
- Source excerpts retain up to five source lines, each clipped at 512 UTF-8 bytes
  without splitting characters, plus labels/column markers. They intentionally
  retain readable source text; full raw scripts are not included in copied reports
  or support bundles. Missing source/locations cannot produce an excerpt.
- Bitwig migration is an experimental partial copy, not a verified replacement
  project: Kontakt automation/static host values are not translated, routing comes
  from an explicit SavedMulti, and original Kontakt bus assignments remain undecoded.
  VST2/CLAP instances are not classified; Bitwig reopening and sonic parity remain
  unverified. Unmapped devices stay unchanged, and shared-state migration is refused.
- Actual module-envelope callback checks retain 483 groups, 480 bypass sources and
  959 of 960 known filter/formant depth targets across three Analog Strings states;
  one opaque target remains unsupported. Finite PCM differences use an authored
  stimulus and replacement sample map with flattened buses and omitted program FX.
  This does not validate stock legacy depth controls or original-map/Kontakt audio.
- Native group insert-order checks cover rack split references, threshold changes,
  subtype retuning and 83 playback cases. Three factory-state probes perform 4,500
  finite resident renders without heap operations; their reference is the same
  engine with authored routing changes. Added inline fields measure 116 bytes per
  voice on the checked Rust 1.98.1 build, not a throughput or parity improvement.
- Wavetable record decoding alone does not establish DSP behavior. Partial group FX,
  decoded envelopes and selected compressor IDs do not establish Kontakt sonic
  equivalence; unsupported processing remains visible.
- Larger racks remain bounded by memory, voice budgets and host/editor capabilities.
  User zoom, cached snapshots and native file-picker tests do not certify every
  gesture, resource, preset or file-dialog workflow.

### Fixed

The 14 newly accepted logical defects are source-reviewed and recorded individually
in `release-fixes.json`; follow-up and safety fixtures do not add counts. The previous
published 0.3.96 binaries are unchanged. These entries describe the next source cut.

- The direct Rust VST3 event-admission path accepts unused optional note-length
  metadata without shortening retained ownership, including negative hints.
  Sources: `a357e1e`, `401d59e`. The native VST3 adapter in this 0.3.115 cut still
  drops negative optional lengths before this path; end-to-end native delivery
  and the reported Mac keyboard/clip silence are not established by these tests.
- Legacy unsynchronized Delay Time uses the checked conversion across its admitted
  absolute range. Sources: `f191cc2`, fractional fixture follow-up `aaf2959`.
- Delay writes the current input before advancing its ring clock, preserving the
  intended sample position. Source: `4ec2958`.
- Native Distortion Damping uses the checked native law. Source: `b3e453e`.
- Distortion's native DC filter runs after Damping with the checked scalar/SIMD
  summation order. Sources: `74e3c0e`, `cd940e2`; one DC-processing outcome.
- macOS plug-in packages use the signed standalone app’s imported CLAP/VST3
  package-type declarations and user-context Launch Services registration. No
  FinderInfo attributes are added to signed code. ZIP extraction and installed-
  payload gates check Foundation/Workspace package recognition
  and both plug-in factories. Sources: `6001126`. Actual hosted Mac execution is
  still required; this source correction is not a Bitwig scan or playback result.
- VST3 editor views retain their component while alive. Source: `3e76d60`.
- VST3 resize frames retain their owner through attachment and replacement.
  Source: `3784c67`.
- VST3 run-loop registration follows the currently attached view.
  Source: `e3822b2`.
- Attached GUI view ownership is independent of creation order. Source: `be80724`.
- Native editor startup and thread information is persisted before entering the
  platform window path, preserving context if startup fails. Source: `c3763d1`.
- VST3 initial cents tuning is applied atomically with note onset, including bounded
  extreme finite values. Sources: `53ab06f`, `2d5822e`.
- Zero-rate integer sample taps avoid the reproduced zero-stride iterator panic.
  This shared renderer correction is distinct from atomic initial tuning.
  Source: `4529cbf`.

### Added

- Confirmed previous-session host crashes recover accessible native evidence and
  complete sanitized diagnostic activity through the reused BUFFR reporter.
  This is the fourteenth accepted defect outcome: reliable crash-evidence recovery
  and acknowledged delivery, grouped with its retry/cancellation follow-ups.
  The private report includes the full sanitized evidence; the public issue is
  limited to safe platform/host/format/build/failure metadata. Automatic reports
  exclude library source excerpts, samples, access data and credentials. Full-hash
  acknowledgements govern delivery; failed or delayed reports remain retryable.
  No host-wide crash hook is installed, and host crashes do not establish KONTRA
  fault ownership. Sources: `665ffee`, `72d3b9b`, `0c7b622`.

### Known limits

- Frozen optimized source `d7b9e11ad096283aab06c79c15732d9bd5a48a03` passes
  all 547 library tests (30 ignored), including 30 support tests, and 89 playback
  tests (4 ignored). Dedicated extreme-onset, zero-stride PCM and exact scalar/SIMD
  checks pass in executable prefix `eebeb2eb1efd`. The clean CDLL prefix
  `d504c6786bb0` embeds that exact revision; its actual Linux VST3 factory exposes
  one KONTRA class with unchanged class ID and releases all factory references.
  Strict C++ editor attachment/lifetime gates pass on unchanged shim sources.
  These are optimized Linux and driver-free checks, not native Mac or every-host
  embedding validation. The tested executable carries its original 0.3.101
  metadata; the accepted ledger derives the next package version as 0.3.115.
- Native macOS Swift/Finder/factory checks, Developer ID signatures, Installer
  execution and Apple acceptance remain required hosted shipping gates for this
  cut. Existing 0.3.96 notarization does not verify these changed bundles.
- Crash recovery/delivery fixtures do not establish a real-world host-crash-to-server
  end-to-end result. Native evidence depends on accessible host/OS artifacts;
  public/private rendering is a support-service contract. Manual local exports may
  retain bounded, reviewed source excerpts; automatic reports exclude them.
- These corrections do not implement Conflux's native Lua interface or establish
  complete Kontakt processing parity, every host workflow or complete-library
  compatibility. No production crash report or synthetic public issue was sent
  as part of the test evidence.

### Candidates — not shipped

- Audit/source-identity metrics, the checked CLI JSON helper and delivery
  instrumentation are observability features, not additional logical defects.

### Reviewed source changes

> Retain authored performance-view background colors through live publication

> Correct native AR and Daft filter identities and stored parameter order

> Decode Kontakt external modulation v0x104 with opaque footer retention

> Preserve physical note-off ownership after channel mode changes

> Correct native LoFi frequency direction without claiming calibrated parity

> Keep the first maximum-consumption stream block resident

> Decode each RAM reference in the stream-start fixture

> Cancel held voice ownership immediately on All Sound Off

> Return selected menu index from KSP VALUE getter

> Bind template-named snapshots and apply explicit modulation removals

> Retain GPU startup causes and diagnose Linux embedding

> Retain runtime fault actions and source context in headless diagnostics

> Route ownerless script controllers through the configured part home

> Route legacy AHDSR cutoff intensity through measured cubic depth

> Restore native persisted menus by entry position

> Require init callback context in keyboard fault regression

> Journal load warnings beyond bounded report examples

> Implement explicit scripted AHD Only with the shared envelope kernel

> Support script-only Kontakt v3 snapshots without replacing native state

> Resolve library-root archive samples from nested instrument folders

> Decode counted native modulation arrays with 64 external slots

> Read bounded native source identities in Kontakt v0x103 and v0x104

> Preserve load diagnostic bursts and report audit delivery status

> Preserve and apply release-trigger Note Mono

> Decode and preserve modern v3 snapshot and v4 compact source records

> Route native group Send Levels taps into existing instrument returns

> Preserve fractional image coverage and transparent sprite edges

> Forward init RPN messages after receiving script slots initialize

> Simplify Logs search and copy complete retained diagnostics

> Make rack header artwork slightly more visible

> Persist global editor size and add independent UI zoom preferences

> Expose cached instrument snapshots in a compact preset row

> Decode Kontakt 8 flat filename tables and explicit effect slots

> fix(nkx): report directory signatures and read boundaries

> fix(samples): report rejected zone and loop bounds

> fix(diagnostics): count KSP faults omitted by the location cap

> Dispatch the documented legacy PGS callback spelling

> Append browser drops anywhere in the rack empty canvas

> Resolve declared JPEG resource names with shared image decoding

> Preserve native group Amplifier insert split metadata

> fix(load): classify skipped zones by their actual failure cause

> Route decoded module envelope bypass and modern target depths

> feat(ksp): route file selectors through native picker callbacks

> Own event detail text before updating the Logs query

> fix(diagnostics): serialize typed zone skip counters

> Load exported performance-view controls before KSP compilation

> Enforce the documented performance-view initialization constraint

> Keep snapshot categories in owned menu labels

> Check performance-view slot results using the runtime result type

> Keep rack welcome drop area and scrollbar gutter stable

> Inspect bounded source identities and report unsupported Kontakt 8 wavetable playback

> test(ksp): verify selector callbacks and prepared paths

> Accept Creator Tools null lists for empty exported menus

> docs: track generic compatibility gaps and validation boundaries

> fix(ksp): retain command and arguments in note validation faults

> Schedule KSP millisecond and beat listeners independently

> Check independent listener delivery on the allocation-free audio path

> Retain listener command and argument details in bounded faults

> fix(render): retain release tails during offline overload

> Preserve native group insert order around the amplifier

> Report fixed group voice state size in the pipeline proof

> Keep group effect indicators honest about partial processing

> Keep interleaved group filter states out of shared lanes

> Remove the rack part cap with prepared storage and viewport rows

> Resolve the authored compressor native ID through KSP names

> Add persistent library display names and validate native Reveal paths

> Explain encrypted preset access lookup and accept XML field whitespace

> Use the detached catalog for library display aliases

> Record complete nightly notes and count reviewed logical fixes

> fix(audio): decode extended PCM WAVE format descriptors

> Show bounded script source context with parse and runtime diagnostics

> Expose authored performance pages with independent script-slot buffers

> Advance to 0.3.18 for eighteen reviewed fixes

> Use imported tab helper for performance page selectors

> Fix source excerpt regression lease and report serialization

> Accept documented symbolic level-meter chain selectors

> feat(migration): inspect and migrate Bitwig copies with shared-state guard

> Record validated native group pipeline and actual state budget

> docs: record actual module envelope callback validation boundaries

> Describe reviewed source batch and omit unshipped candidates from notes

> Advance to 0.3.22 after four further compatibility fixes pass

> Read source excerpts from serialized diagnostic event data

> Advance to 0.3.23 with verified readable script diagnostics

## 0.3.0-nightly.20261002.g4399f700e590 — 2026-10-02

Public release source: `4399f700e5904114ebfdf84ef50091716f5867c7`.
Reviewed export checkpoint: `e53150559f37407197be3d6182aed9a1c3619e89`.
Previous release source: `521e6954749840ab15c3d9365d7cd3e95a4867ea`.

### Added

- Load supported NKSN snapshots from the instrument header picker or a header
  drop, after loading their base NKI. The loader validates the base and applies
  supported snapshot state before changing the active source. Both source paths
  survive DAW state and KONTRA multis; rejection preserves the existing source
  generation, installed epoch and source services.
- Persist successfully applied native script parameter edits alongside persistent
  variables. Restore authored initialization getters, retain decoded values actually
  applied by the engine, and replay current effect edits after processor rebuilds.
- Process all eight native group drive insert slots. Previously a third drive was
  discarded; actual Analog Strings Tube SHAPE/BYPASS edits now reach both groups'
  DSP. Matched sample renders are finite and change with the edit, with zero render
  heap operations over 2,250 calls per comparison side.

### Changed

- Save active native edits with prepared address storage and a reusable numeric-key
  hasher; refresh edited parameter slots rather than every default parameter.
- Share immutable compiled UI revision-owner maps between runtimes while keeping
  revisions and mutable values local. This reduces duplicate retained metadata;
  native-state persistence separately added about 0.08 ms in the measured Areia case.
- Avoid unchanged menu-row copies and redundant bounded live-refresh passes. Equal
  indexed integer writes leave table revisions current; script callbacks are retained.
- Decode large filename tables once for samples, resources and impulses. All three
  actual-library fingerprint comparisons match the prior implementation.
- Retain unchanged Original control subtrees across live publications. Changed rows,
  active gestures and replacement epochs still rebuild their affected controls.
- Keep compact articulation-mode help readable and preserve broad pictured value
  graphs with authored callbacks in vectorized views.

### Fixed

- Default Windows editors to Direct3D 12 rather than implicit Vulkan. Explicit
  `WGPU_BACKEND` choices remain authoritative. Persist renderer initialization,
  adapter details and recoverable startup failures in diagnostic journals.
- Map native SV Notch 4 filter type 58 to the existing four-pole DSP path. An actual
  Accordia resident-sample render changes its PCM without render heap operations.
- Preserve failed and canceled load status when later active-script diagnostics or
  pending artwork report warnings. A rejected snapshot retains its failure status.
- Replace a cached loading placeholder with the completed failure or empty-source
  state; include loading/status changes in the performance-view cache dependency.
- Preserve NIS/NKS decoder family, cursor/offset, declared lengths, available bytes,
  version and chunk context, including decompression and structured-object errors.
  Recognized malformed NIS files retain the original decoder cause; invalid metadata
  returns an error instead of panicking. Valid synthetic container roundtrips remain
  byte-exact, with truncated-container coverage preserving the underlying EOF cause.
- Make the Logs export regression wait for a newly started request and a re-enabled
  UI scene, rather than accepting a previous completion. It still proves an existing
  destination fails without overwriting the earlier report.
- Extend native-edit, snapshot rejection and save/reload fixtures to use resident
  banks, settle persistence, retain source generations and avoid dumping values.

### Known limits

- The Windows backend policy has not been tested against the reported FL Studio
  crash on the affected machine. Hosted Windows/macOS builds do not certify DAW use.
- Snapshots require their base NKI and cannot open independently or target programs
  inside an NKM. Opaque source state and unknown saved scalars warn and remain unapplied.
- Native drive/filter support and finite sample renders do not establish Kontakt
  parameter-law or sonic equivalence. Compatibility remains partial.
- Control reuse, owner-map sharing and bounded refresh proofs do not establish native
  display FPS, GPU completion latency or a whole-runtime speedup.
- Exact-source hosted CI passed 410 library and 78 playback tests. The downloaded
  Linux package passed 38 CLAP checks (6 skips, no warnings/failures) and strict-level-5
  VST3 validation at 48 kHz/128 samples with GUI checks skipped. These scoped checks
  do not validate every library, host deadline or Windows/macOS runtime.

### Reviewed source changes

- Load factory snapshots through the plugin worker and persist their source
- Keep old snapshot state regression independent of later appended fields
- Map native SV Notch 4 filter id to existing DSP
- Share immutable UI revision ownership across script runtimes
- Journal snapshot validation and preserve active source services on rejection
- Keep snapshot rejection commits in source request view lock order
- Finish validation trace before locking the visible snapshot report
- Correct snapshot trace value and worker proof return types
- Measure notch pass bands by normalized signal power
- Preserve applied native script edits across host-state restoration
- Replay current native effect edits after processor rebuilds
- Validate saved native addresses without scanning the edit list
- Report exact snapshot rejection fixture state changes without dumping values
- Use existing standard storage and share the native-state test instrument
- Install a resident bank in the native edit callback fixture
- Preserve failed load status when publishing the active script diagnostics
- Settle active snapshot persistence before testing rejection and retain failed merge status
- Keep rejected snapshot status when pending artwork reports warnings
- Preserve broad pictured value displays in vectorized views
- Keep articulation mode help readable in compact editors
- Snapshot only active native edits and reuse the locked numeric-key hasher
- Document snapshot loading and native control state restoration
- Assert rejected snapshot retains source generation and installed epoch
- Capture the decoded native value already applied by the engine
- Skip unchanged KSP menu rows during bounded live refresh
- Decode large sample file tables once per import
- Leave KSP table revisions current when indexed integers are unchanged
- Default Windows renderer to DX12 and persist GPU startup diagnostics
- Document renderer policy and large-library refresh/import fixes
- Retain unchanged Original controls across live row publications
- Retain all eight native group drive insert slots
- Record retained UI controls and complete group drive slots
- Wait for fresh completed exports and an enabled UI scene
- Replace cached loading placeholders after instrument failure
- fix(import): preserve container decoder boundaries in load errors
