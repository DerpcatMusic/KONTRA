# KONTRA diagnostic map — 0.3.476 / 94980eb10df7

Evidence: the actual Bitwig journal `~/.cache/kontra/logs/session-870086-1791982211981886051-1.jsonl`, its diagnostic-location sidecars, the installed library files, and production source. The original failure mapping was checked against revision `94980eb10df7`; subsequent implementation changes are described below. The tables below record the original failure mechanisms. The implementation status section records subsequent changes; remaining requirements are not claimed complete.

The full session contains **17 instruments plus Application: 1,749 error occurrences and 172 warnings**. Error counts are occurrences, not independent defects. The 1,746 Pizzicato errors are one callback defect; the two Dual Splangs errors report one failed load twice.

## Current implementation status — 2026-10-15

The following fixes are shared runtime/translator behavior, with no preset-name exceptions:

- **U1 sample task and replacement:** `loadSample` now prepares real sources on a worker, commits a bounded immutable source revision, and completes on the script owner after acknowledgment. Empty/missing initial players retain their complete compiled recipes; replacement affects future admissions while existing voices retain their old sources. Callback identity, polling, no-callback loading, cancellation, stale generations and allocation-free commit/render/retirement are covered. A real attached script-thread fixture streams all 40,000 frames bit for bit against an independently loaded resident plan after explicit offline page readiness. This proves ownership and decode correctness, not real-time disk deadlines or native whole-instrument parity. Current dynamic source support is SamplePlayer WAV/AIFF/FLAC; private metadata and other oscillator replacement remain explicit gaps. `loadImpulse` still returns a valid failed task for unsupported replacement. See [dynamic source scope and evidence](../architecture-v2/DYNAMIC_SAMPLE_REVISIONS.md).
- **U1 actual authored batch now verified:** the visible AO LayerNext gesture completes all 1,746 requests with successful owner acknowledgments, zero failures/pending tasks, and exactly 1,746 replaced source identities. An independent reader reopened every registered source and matched 1,191,936 resident frames bit for bit. The generic fix preserves raw paths and documented ordered script/preset/bank contexts; no preset-name exception or basename search is used. Dynamic physical heads used 28,785,138 bytes. The original all-failed and zero-request receipts remain preserved. Total 32.09-second loading was heavily contended and still leaves an owner/UI publication bottleneck to measure; this is not a native whole-instrument/audio parity claim.
- **Streaming readiness:** default Auto now stages bounded attack heads before part publication with explicit capacity fallback diagnostics; mandatory documented UVI full-resident sources share the startup physical quota. Active sample-start modulation ranges are included in forward/reverse head planning. Real-bank complete-head receipts include a retained 24-tail-underrun failure; arbitrary script offsets and dense SSD deadline safety remain open. See [DFD evidence](../architecture-v2/DFD_READINESS_EVIDENCE.md).
- **CPU/UI measurements:** the exact-output in-place pitch-step update improves compiler-free plain 300-voice callback medians by 13–15%; the 256-frame meter is still about 1.64–1.65%, above the 1% target. A SIMD mix wrapper was rejected for short-span/reverse losses. Seeded standalone Evolved and AO rendering have complete model/geometry/pixel oracles and exact asset accounting; this does not establish the user's old installed plugin build was updated.
- **K1 snapshots:** shared instrument identity resolution reads embedded content identity behind generic `Kontakt` names, searches bounded library/category ancestors, rejects ambiguous bases and validates the selected base. The installed Dual Splangs snapshot resolves and applies its state successfully.
- **C6, C7, C11 wavetable routes:** form 1, form 2 and position have separate per-voice modulation lanes, composing live base values without accumulation. Serial/parallel, block partition and multi-voice output tests pass. Native whole-instrument smoothing and clock equivalence remain unproven. C8–C10 require additional oscillator kernels and are still omitted.
- **C13–C14 pressure callbacks:** KSP admits `ignore_controller` in `poly_at`, maintains `%POLY_AT` and `$POLY_AT_NUM`, preserves callback stages and converts MIDI precision explicitly. Host adapter tests verify one callback for overlapping IDs, physical channel isolation after internal wire mapping, suppression and exact CLAP note-ID recipients. Core deferred callbacks retain physical selectors without audio-thread allocation.
- **S2 startup marker:** an in-memory instance marker exists before worker registration, while durable readiness remains false until storage succeeds. Delayed registration preserves lifecycle and host identity. S1 incomplete delivery still retains the complete report locally; remote delivery is not claimed repaired.
- **C2 impact reporting:** an omitted unverified random generator is reported as omitted, rather than as an approximation. Rejected filter/intensity routes retain their specific reasons.
- **Inspector coverage:** literal Lua require closures include transitive engine modules. Offline UVI initialization now uses the same loose/bank image authority as playback, so image dimensions can influence geometry correctly. Dedicated inspector rendering, resource tracing and real-bank measurements are tracked separately in `docs/library-inspector.md` and `docs/architecture-v2/ASSET_LOADING_EVIDENCE.md`.

All other rows remain evidence requirements, not fixes. Native decompilation shows UVI FeedbackMachine is a nested feedback processor graph, rather than a simple delay; replacing it with a guessed delay would not establish parity. Negative Kontakt modulation intensity, random reset/cadence, shared tempo-synchronized LFO rate and extreme sample pitch ranges also need their own verified laws.

## Can the inspector see what the user sees?

`tools/library-inspector.py` and `corpus-health inspect` read installed libraries, conversion and script initialization. Optional UI rendering links the production editor implementation without constructing the plugin or requiring a DAW. It can expose translated bounds, visibility, bindings, resources and timings. It does not attach to the user's Bitwig instance, reproduce arbitrary host interaction history, or prove equivalence with Kontakt/Falcon's original UI or audio.

The original journal supplied exact observations and the omitted inventory below. Direct bank member inspection confirmed the Lua task failure. Source-side image services and transitive module enumeration are now shared with the inspector rather than inferred from a raw JSON report.

## Complete instrument inventory

| Instrument or application | Errors | Warnings | Map |
|---|---:|---:|---|
| Application | 1 | 1 | S1–S2 |
| V Strings Pizzicato 2.uvip | 1746 | 4 | U1–U5 |
| Dual Splangs.nksn | 2 | 0 | K1 |
| Atomic Siren.nksn | 0 | 14 | C1–C14 |
| Black Hole.nksn | 0 | 14 | C1–C14 |
| Brass Tribe.nksn | 0 | 14 | C1–C14 |
| Column Fire.nksn | 0 | 14 | C1–C14 |
| Deep Lie.nksn | 0 | 14 | C1–C14 |
| First Chapter.nksn | 0 | 14 | C1–C14 |
| Follow That Car.nksn | 0 | 14 | C1–C14 |
| Gigatron.nksn — omitted in pasted summary | 0 | 14 | C1–C14 |
| Ground Up.nksn — omitted | 0 | 14 | C1–C14 |
| Trick Of The Light.nksn — omitted | 0 | 14 | C1–C14 |
| A Triple MW.uvip — omitted | 0 | 5 | U4–U5, W1–W3 |
| W Strings Bartok.uvip — omitted | 0 | 11 | U4–U5, W2–W10 |
| 03 Areia - 6 Celli - Core Techniques.nki — omitted | 0 | 3 | A1–A3 |
| Solo - 12 Solo Soprano Angelic.nki — omitted | 0 | 2 | A1, A4 |
| Solo - 01 Pads.nki — omitted | 0 | 6 | P1–P6 |

## Failures and UVI Pizzicato warnings

| ID / diagnostic | Why it happens | Actual impact and required work |
|---|---|---|
| **U1 — `layer.lua:1133`, nil `.success`** | Installed `Layer.lua:1116` calls `loadSample(oscillator, samplepath, callback)`; line 1133 reads `task.success`. KONTRA's `loadSample` binding queues the callback with `Value::Nil` and returns no task. This matches the failure exactly. | The callback faults and sample replacement is unavailable. First return/pass a valid failed task on failure, once, without claiming success. Then implement bank-relative resource resolution, background decode/prepare, oscillator replacement, cancellation/reload isolation, and old/new voice resource lifetimes. Test callback and polling contracts, missing samples, repeated loads and unload. Reproduce this preset in the same CLAP host. A nil guard alone cannot make its sample switching work. |
| **U2 — Program-level LFO** | Shared, retriggered program LFO behavior is implemented using an instrument-wide instance restarted by note-on. Overlapping-note behavior is inferred rather than measured. | An approximation, potentially different phase/retrigger behavior on chords. Measure native overlapping notes, retrigger/release and free-running behavior; match the shared clock and clear the warning only when verified. |
| **U3 — Program-level MultiLFO** | Same shared-instance uncertainty, applied to the combined LFO sources. | An approximation of shared triggering. Verify component clocks, noise, reset and overlapping notes; preserve shared ownership rather than creating an independent cycle per note. |
| **U4 — FeedbackMachine, Aux3 / InsertFX0** | Parameter metadata exists, but no supported processor translation handles this module. | That effect is omitted. Implement the actual delay/feedback/mix processor, aux routing and live controls; validate impulse/tail and modulation behavior against UVI. A parameter catalog alone is not DSP support. Applies to all three listed UVI programs. |
| **U5 — Silence.wav zone 1740, keys narrowed** | Resampler admissible playback ratios are `1/256..16`. Root key, tuning, sample/output rates and authored range put some notes beyond this bound, so loading narrows the range. | This is not a missing-file error. The excluded notes no longer trigger that zone. A silent asset suggests little direct audible loss, but its scripting/trigger role needs checking. Preserve unpitched silent zones where appropriate, or extend supported resampling with proper limits; verify zone coverage and note behavior. Applies to all three UVI programs. |
| **K1 — Dual Splangs: no instrument `Kontakt` found** | The base file `Morphology Evolved.nki` exists. `read_snapshot` retains the first metadata name, `Kontakt`. The direct `.nksn` loader searches for that name or the immediate folder name `Sound Design`; neither names the actual base file. Another metadata-binding helper already understands generic `Kontakt` identities, but this path does not use it. | The load stops before translation/audio. Unify direct-load and catalog identity handling; consider the second metadata name and enclosing snapshot-library directory, then verify actual base identity and state compatibility. Provide explicit base selection when ambiguous. Test this installed snapshot and ambiguous/mismatched bases; do not rename the instrument blindly. |
| **S1 — complete crash evidence not delivered** | The retained local status remains `sent:false`, with no report ID. A read-only audit of the support service matches this exact error text to its HTTP 502 private object-evidence delivery failure path. This is distinct from the client receipt-hash mismatch message. | Preserve the complete local incident. The specific storage/private-issue/public-summary operation still requires the service error log; no production retry/upload or backend repair is claimed. This is not evidence that the Pizzicato Lua error crashed Bitwig. |
| **S2 — crash marker not ready at construction** | Plugin startup checks the reporter's ready flag. Registration has a bounded wait; worker startup/persistence failure or a delayed acknowledgment can leave it false. | Crash capture was not confirmed at construction. Inspect host stderr and marker storage/worker errors; distinguish permanent failure from delayed readiness and make readiness transitions observable. This warning does not by itself prove a crash. |

UVI documents a task passed to the `loadSample` completion callback; when a callback is supplied the API also returns a pollable task. References: [async API](https://lua.uvi.net/group___async.html) and [AsyncTask](https://lua.uvi.net/class_async_task.html). No native host audio equivalence was measured here.

## All 14 Conflux warnings — shared by ten snapshots

The ten snapshots all completed preparation with 92 groups, 1,985 zones, 1,962 samples and three scripts, with partial status. Their identical warnings come from the same base instrument. They are not fourteen separate preset-loading failures. A warning's audible effect depends on active groups, nonzero route depth and the preset's script state.

| ID / diagnostic | Cause | Impact and work |
|---|---|---|
| **C1 — Filter: filter law**, group 0 insert 0 | The selected filter mapping is marked below exact fidelity; `note_fidelity` emits UnknownLaw. | A configured stand-in/approximation can change tone, cutoff/resonance response or level. Identify the saved filter type and prove its kernel and parameter/modulation laws; compare sweeps and impulse/audio responses. |
| **C2 — RandomBipolar → pitch**, group 0 external slot 37 | RandomBipolar has no admitted generator/law. The translator records the warning and `continue`s before constructing its source/routes. | **Omitted, despite the UI saying “approximated.”** Implement the signed random distribution, sample/hold cadence, seeding, voice/reset ownership and pitch-depth law. Correct the diagnostic impact immediately. |
| **C3 — keys narrowed**, zone 170 cello | Same resampling bound as U5; this is a musical sample. | Some authored notes are removed from that zone. Report old/new ranges and root/rate/tuning; extend supported ratios or use a verified source model. Do not treat it as a missing NCW. |
| **C4 — modulator intensity**, group 5 Tension Cello slot 1 | Only a subset of source-to-source intensity assignments is admitted. Missing destination, unsupported flags/depth/inversion, shaped bipolar input, route constraints or incompatible chains can reject it. The UI drops the specific rejection detail. | That intensity assignment is omitted; the destination's base modulation may remain. Recover/report the exact rejection from the affected snapshot, implement its combination law and order, and verify amplitude/depth movement. “All intensity modulation is unsupported” would be inaccurate. |
| **C5 — frequency → LFO 1 rate**, group 14 Horn Stab slot 1 → slot 2 | Rate routing is admitted only for a resolved, voice-scope LFO with a supported free-running Hertz frequency lane. Shared/beat-synced/unresolved destinations fail this path. | This envelope-to-rate route is omitted. Inspect the destination mode and implement the missing rate/clock law and addressing, including tempo behavior. Other supported rate modulation should remain intact. |
| **C6 — `warpFactor` / WT FORM 1** | No general modulation target binding for this source parameter. | Envelope movement of form 1 is omitted. Add the live form-1 amount lane with correct normalization, bounds and DSP update. |
| **C7 — `warpFactor2` / WT FORM 2** | Same, second form parameter. | Envelope movement of form 2 is omitted. Add its distinct live lane and verify processing order and interaction with form 1. |
| **C8 — `wavetableInharmonic`** | No admitted modulation destination; full underlying native processing also has remaining gaps. | Envelope-driven inharmonic movement is omitted. Implement/verify the inharmonic source processing and live parameter law; a route alone is insufficient. |
| **C9 — `wavetableModAmount`** | No admitted destination for auxiliary wavetable modulation depth. | Envelope-driven modulation amount is omitted. Add the mode-correct amount lane and prove the oscillator/modulation kernel. |
| **C10 — `wavetableModFrequency`** | No admitted destination for modulation oscillator frequency. | Envelope-driven modulator frequency is omitted. Add frequency scaling, clock/reset and correct oscillator mode semantics. |
| **C11 — `wavetablePosition` / WT POS** | Generic routing recognizes amplitude/pitch/pan/sample-start and selected addressed targets, but this wavetable destination is not wired. | Envelope scanning through the wavetable is omitted; static/script controls are a separate path. Bind the route to the existing position consumer with correct depth, interpolation and smoothing. |
| **C12 — `$name` unbound UI handle**, GUI line 40 | A referenced variable is absent from the loaded performance-view description; the compiler retains an unresolved handle. | Compilation continues, but that handle has no confirmed UI binding. Determine whether it is an authored control or helper/local parameter; preserve scope/identity and bind only actual controls. Verify the relevant view and script callback. |
| **C13 — `ignore_controller` outside controller**, GUI line 5521 — hidden | Lowering emits controller suppression only in controller context; other contexts are ignored with a warning. | No controller suppression at that site. Determine whether it is actually called from a legal controller/pressure callback and whether context propagation through helper functions is wrong. If unreachable or invalid natively, classify it accurately rather than changing global suppression. |
| **C14 — `PolyAt` reads zero**, GUI line 5524 — hidden | `%POLY_AT` is not among runtime-maintained system arrays; lowering substitutes zero. The display normalizes the literal zero to `N`. | Script reads cannot respond to the current per-key pressure value. Maintain the 128-note pressure array, connect ingress and callback ordering, and define reset/channel behavior. Other poly-pressure modulation support does not supply this KSP array automatically. |

## UVI setter warnings from the omitted programs

All ten rows have the same mechanism: script initialization writes retained XML, then translation tracks which node attributes it actually reads. These written attributes were not consumed, so they did not become an effective translated setting. **The warning alone does not prove the source is audible or connected.** Check reachability, units and a real DSP consumer before deciding whether to implement or reclassify it. Runtime live control also needs its own wiring.

| ID / written parameter | Instrument | Intended control / needed consumer |
|---|---|---|
| W1 — `StepEnvelope.Smooth` | A Triple MW | Step-envelope smoothing/interpolation, matching UVI timing and curve. |
| W2 — `MultiLFO.Freq` | A Triple MW; W Strings Bartok | Combined LFO rate/tempo lane. |
| W3 — `MultiLFO.Smooth` | A Triple MW; W Strings Bartok | Combined-wave/noise smoothing. |
| W4 — `LFO.Freq` | W Strings Bartok | LFO rate with correct units and tempo mode. |
| W5 — `LFO.Retrigger` | W Strings Bartok | Trigger mode and shared/voice reset ownership. |
| W6 — `LFO.SyncToHost` | W Strings Bartok | Host tempo synchronization and rate conversion. |
| W7 — `SignalConnection.Ratio` | W Strings Bartok | The actual route's modulation depth/combination law. |
| W8 — `MultiLFO.NoiseDepth` | W Strings Bartok | Noise contribution, cadence and scaling. |
| W9 — `MultiLFO.Retrigger` | W Strings Bartok | MultiLFO reset/trigger behavior. |
| W10 — `MultiLFO.SyncToHost` | W Strings Bartok | Combined-source host tempo synchronization. |

## Areia and Solo diagnostics from the omitted instruments

| ID / diagnostic | Cause | Impact and required work |
|---|---|---|
| A1 — keys narrowed | Resampling bounds; Areia zone 18 and Solo Soprano zone 303. | Excluded notes no longer trigger those zones. Same investigation/fix as C3, using each sample's actual rate/root/tuning. |
| A2 — `find_group` not found, returns -1 | Exact-name lookup found no matching group; the displayed `-N` is normalized `-1`. Areia script slot 0, line 23623. | That lookup cannot select its requested group. Inspect the requested name, complete group table and branch reachability. It could be an optional absent group or a KONTRA environment/mapping issue; preserve native “not found” behavior rather than inventing a group. |
| A3 — polyphonic variable read outside note callback is zero | Lowering has no originating note cell in this context. Areia slot 0, line 47119. | Note-specific state is lost at that read. Trace the helper/callback/resume chain and preserve event ownership where native KSP supplies it; otherwise identify an invalid/unreachable authored access. |
| A4 — keyswitch script, Solo Soprano | The articulation extractor reports an unrecognized/unmapped/conflicting switching pattern. This is separate from whether the KSP script compiled. | Automatic articulation mapping is incomplete; the warning alone does not prove every scripted keyswitch fails. Inspect the retained reason and note routing, admit the valid pattern or expose behavior-owned switching accurately; test articulation changes. |
| P1 — Rotator, Solo Pads instrument insert 3 | Saved Rotator record is recognized, but its rotational/acceleration/distance DSP laws are not extracted/admitted. | The effect is omitted. Implement the actual summed-input rotary processor, buffers/state, rate acceleration, distance and live controls; validate stereo motion and tails. |
| P2 — `attach_zone` simplified, slot 0 line 4434 | A bounded waveform attachment service exists; native semantics are only partly covered. | Waveform attachment/display can differ. Complete source identity, attachment flags, reload/reset and UI ownership. Do not treat this warning as proof the entire API is missing. |
| P3 — `get_event_par` ignored, line 8821 | This lowering context lacks the originating note event required by the builtin. | The intended event query has no valid result at that site. Preserve native event ownership through the callback/helper chain when valid; verify the queried event/parameter and lifecycle. |
| P4 — `set_ui_wf_property` simplified, line 8822 | Only admitted waveform properties/indices are serviced, with simplified semantics. | Display/cursor/slice properties may differ. Inspect the requested property and index, extend the owned waveform service and verify UI update/readback. |
| P5 — waveform source zone absent, line 2599 | Initialization rejects a zone not present in its physical-source environment, including invalid/nonpositive IDs. | Attachment is rejected and the waveform may be absent. Reconcile authored zone IDs, source mapping and init availability; preserve missing-zone diagnostics for truly absent sources. |
| P6 — `play_note(...,-1)` needs note callback, line 12900 | Duration -1 means a generated note gated by the originating note. This call's lowering context has no such note owner. | If that path executes, it can fault; this report contains the warning, not an observed execution of that fault. Implement valid originating-note lifetime/ownership or preserve the native error for invalid calls; test release, cancellation and reload. |

## Work order and acceptance

1. **Stop the Pizzicato callback exception and implement real UVI sample replacement.** Acceptance: valid task on success/failure, one completion, actual requested sample identity, no stale completion after reload, and no line-1133 errors in the matching host.
2. **Repair direct snapshot base resolution.** Acceptance: Dual Splangs resolves Morphology Evolved without admitting unrelated/mismatched bases, and snapshot state reaches translation.
3. **Correct diagnostic impact reporting.** Track implemented/approximated/omitted separately from reason. Keep exact rejection detail, callback context and parameter addressing; do not normalize away useful constants in human-facing explanations.
4. **Wire source/modulator destinations and pressure/event ownership.** Prioritize WT position/forms, source rate/intensity and `%POLY_AT`; verify active route depths and reachable callbacks before claiming an audible regression. More complex WT kernels need their own parity work.
5. **Complete active UVI parameter consumers and omitted effects.** Check unused-node warnings first; implement FeedbackMachine and Rotator with measured DSP behavior, then the needed LFO/step/connection setters.
6. **Fix sample-range/UI/source identity issues and support reporting.** Each needs its own receipt: preserved key coverage, correct waveform/control identity, marker readiness and acknowledged complete delivery.

For every audio change, use one targeted installed-preset regression plus synthetic ownership/failure checks. Compare with the native host using the same notes, controllers, tempo and block size. A successful inspection or warning-free load is not evidence of sound equivalence.

## Current implementation references

- UVI async contracts: `crates/sampler-uvi/src/script.rs` and `script/async_data.rs`; shared image authority: `crates/sampler-uvi/src/lib.rs` and `bank.rs`.
- Snapshot identity, search and validation: `crates/sampler-kontakt/src/snapshot.rs`; plugin adapter: `src/sound/v2.rs`.
- Pressure admission and staged projection: `crates/sampler-core/src/controller_event.rs`; KSP lowering: `crates/sampler-ksp/src/lower.rs`; ingress: `crates/sampler-midi/src/ingress.rs`.
- Wavetable route binding: `crates/sampler-kontakt/src/library.rs`; lane compilation and source consumers: `crates/sampler-core/src/voice_mod.rs`, `source.rs`, `render.rs` and `parallel.rs`.
- Pitch bounds: `crates/sampler-core/src/resample.rs`; range fitting: `crates/sampler-kontakt/src/load.rs`.
- Crash marker and delivery ownership: `src/support.rs`, `src/support/crash.rs`, `src/support/report.rs`.

Official contracts: [UVI asynchronous APIs](https://lua.uvi.net/group___async.html), [NI KSP callbacks](https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/callbacks), [NI general commands](https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/general-commands). Native code evidence remains local and is not included in this document.
