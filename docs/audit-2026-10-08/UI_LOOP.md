# Script ↔ UI loop audit, 2026-10-08

Scope 4. Baseline **`origin/integrate/core-v2@7e82b152`**; v1 reference **`0cb7a8a0`**. All implementation locations below refer to that baseline, before the audit-only test-module imports. No product fixes were made. Companion render/widgets/params/census audits own pixel fidelity, gestures, individual property semantics, and rendered corpus classifications respectively.

## Verdict and evidence limits

**Worse than v1 for state continuity and interaction plumbing.** An identical interface publication resets Original to Vector. The scalar readback path has no idle-editor wake signal. Queued edits can mutate displayed state despite queue rejection and can cross a load replacement. Four executable baseline failure witnesses reproduce these mechanisms. V1 stores presentation in the part/settings, watches live revisions and pending edits, and memoizes authored controls; those mechanisms were lost in the current v2 adapter.

**Provisional prior full-editor Conflux measurement:** the UI audit agent (`audit/ui-20261008`, worktree `audit-ui`) measured warm CPU frame medians **35.994 ms Original / 31.423 ms Vector** (approximately 36.0 / 31.4 ms), with **434 widgets across three interfaces**. Mean totals were 36.430 / 31.918 ms; sample maxima 53.436 / 47.826 ms. The machine was under heavy shared load. These are provisional CPU software-render observations, not GPU/DAW FPS or a measured isolated regression.

**All other timing results in this scope are unmatched.** The prior agent’s subsequently completed `docs/audit-2026-10-08/ui.md` at `3daacecf` owns its v1 `0cb7a8a0` idle-frame comparison and methodology. It records v2 confirmation medians 6.716 / 6.584 ms (means 6.729 / 6.597 ms) and v1 medians 6.193 / 5.583 ms (means 6.333 / 5.724 ms). Its captures are degraded and visually unequal; v1 Original/Vector screenshots are identical. Retain those as **unmatched scene-quality evidence**, not a performance-parity ratio. This scope’s direct-IR load/publication/build/paint/admission measurements also remain explicitly **unmatched**. The repeat variance requires an isolated scheduling/CPU trace, not a guessed renderer explanation.

Provenance: `~/.cache/kontakto-audit-ui/{v2-conflux.log,v2-conflux-confirm.log,v1-conflux-idle.log}`; audit branch `3daacecf`; optimized build confirmed by `build2.log`/`final-build.log`. Its v1 animated-control benchmark **skipped** Conflux (`no_animated_knob_or_slider`), so it is not an interaction benchmark. The idle harness reports 378 controls, 118 visible, two pictures and no wallpaper. This cannot certify the user's fully working v1 screen.

Our fresh optimized probe loads the installed Conflux NKI with sample keys restricted to 0; this exercises the actual v2 script preparation, UI models and resource resolver without a full sample-residency/audio benchmark. It measures direct UI-IR drawing at 1000×600, eight warmup + 24 sampled frames, and eight complete publications. Its software painter creates fresh CPU context/resources on each call; only UI build/layout is warm. All timings from this probe are **unmatched**. These timings must not be compared directly with the earlier full-editor harness's retained CPU painter or a DAW GPU session.

## Exact default-view regression: Original must be the default

User fact: v1 rendered most authored library interfaces correctly; v2 opens a semi-vectorized view instead. The precise baseline decision is **`src/ui/part.rs:59`**:

```rust
let start = if from[main].unsupported.is_empty() {
    Presentation::Vector
} else {
    Presentation::Bitmap
};
```

`Presentation::Bitmap` is the local **Original** choice (`part.rs:88–91`). The preceding comment explicitly says “Vector unless the source needed something we only approximate.” Thus an empty unsupported-feature list selects Vector; any unsupported metadata selects Original. That is the implemented policy, not a demonstrated rendering capability test. Clearing unsupported diagnostics can consequently switch a library from Original to Vector even though the user never requested a presentation change.

The decision runs on first opening **and every outer-interface `Arc` replacement** (`part.rs:55–60`). It ignores both `cx.settings.view_mode` and any retained prior presentation. The existing settings enum already defaults to Original (`src/library.rs:118–125`), and the header settings menu writes `view_mode` (`src/ui/header.rs:273–280`), but the performance-view constructor never reads it. Changing the enum default cannot repair this path. The executable identical-publication witness proves a manually selected Original is lost at this constructor.

V1 at `0cb7a8a0` uses a different policy: `src/library.rs:101–104` defaults `ViewMode` to Original; `src/ui/perf_view.rs:268–277` honors an explicit stored part choice (`1=Original`, `2=KONTRA`, `3=Vectorized`) and otherwise uses the configured default. `shows` (`:292–294`) supplies the part choice and `cx.settings.view_mode`. Only an unavailable authored performance view triggers its generated-KONTRA fallback. Its decision does not inspect unsupported-property diagnostics. Availability itself (`:260–264`) checks a source performance flag, nonempty controls and authored pictures/background; this is useful reference policy, not proof that v1 implemented every frontend.

**Required view model:** for a fresh part with no explicit saved/user override, choose **Original**, independently of unsupported diagnostics and asset-cache state. Explicit user choices for Original/Vector/generated KONTRA remain three distinct persisted presentations; a property/value/key publication retains the selected presentation, source script/page, focus/scroll/drag and resources. Global settings and the header picture button must use the same effective-mode resolver. Source epoch replacement may reinitialize source-specific state according to that persisted policy; ordinary snapshot revision may not. If the requested original/native frontend is unavailable, show its capability/resource failure explicitly rather than silently selecting Vector and claiming fidelity. Retaining a backdrop with substituted controls is Vector, never an automatically equivalent Original.

Completion gates for W1: fresh interfaces with zero, one or many unsupported entries all start Original; removing or adding a diagnostic does not change presentation; no-op/label/key/geometry publication retains an explicit choice; saved part override wins over global preference; absent override follows the persisted app preference whose factory default is Original; header/settings/body agree; unavailable original frontend reports incompleteness rather than a false successful fallback. These gates extend rank 1 and the publication-continuity test below. No product change is made in this audit.

## Conflux: first witness and root causes

Installed witness: `Kontakt/Conflux 1.1.0 [Native Instruments]/Instruments/Conflux.nki`.

The first fresh run loaded in **4811.024 ms**, produced **390 scalar definitions**, and published three interfaces for original source slots **2, 3, 4**. Slot 2 is the selected main face: **411 widgets, 134 visible, 313 bindings present in the prepared scalar schema**, four assets and one unresolved required picture. Original holds only **11,112 bytes** of decoded pixels, Vector **5,064 bytes**. Slots 3/4 contain 22/1 widgets (5/0 visible). Eight complete `ScriptUi::interfaces` calls took **0.370–0.442 ms** each (median 0.380 ms). The initial slot-2 warm build/layout median was 0.536 ms Original / 0.540 ms Vector; fresh-context CPU painting/raster/readback was 3.913 / 3.883 ms. Later probe results and callback/drag checks are tabulated in the measurement appendix.

The screenshot `~/.cache/kontakto-audit-ui-loop/conflux-0-Bitmap.png` reproduces a pale panel, faint text and scattered controls. That is a rendered fallback, not evidence of a complete translated native performance view. The render owner's independent record reports background RGBA `(240,239,228,255)`, an unresolved wallpaper, and **94.94% ground-colour pixels**. It also finds a `load_native_ui` request. `sampler-ksp/src/eval.rs:1258` records that request; `sampler-kontakt/src/load.rs:643` exports stock KSP interfaces, and the production stage (`src/ui/part.rs:51`) only consumes those interfaces. There is no instantiated Komplete UI/reactive frontend in this loop. Renaming the fallback “Original” does not load the requested frontend. Resource-name resolution and native view interpretation are the render/frontend owners' mechanisms, not a Conflux exception.

The loop compounds that incomplete initial scene:

1. **Publication is mistaken for replacement.** `Shared::apply_effects` (`src/plugin.rs:891`) regenerates every script interface for the part and replaces the outer `Arc`. `interface` (`src/ui/part.rs:55`) detects that pointer change and creates a new `Face` using the largest script and a recomputed default presentation. Original/page choice, local values, resource resolver and decoded assets are discarded. This happens for a key-name edit as well as a geometry/picture edit. The synthetic pixel witness reproduces the reset with byte-identical interface data, so a library-specific fix would be wrong.
2. **A successful scalar edit does not guarantee observable script output.** The editor sends a numeric value; the audio bridge ignores `Core::set_control`'s success boolean (`src/plugin.rs:1586`). Scalar readback runs approximately every 100 ms, before ingress edits in that block (`src/plugin.rs:1532`), and is absent from `Watch`'s signature (`src/ui/mod.rs:175`). Idle script changes can remain invisible until unrelated input/meters/loader changes wake the editor. This is sufficient to produce stale/snapback behavior, but does not prove every visible Conflux knob has this failure.
3. **Not every emitted UI command has a consumer.** Runtime `set_text`, movement, hide aliases, menu mutation, waveform attachment and related commands are emitted in `sampler-ksp/src/lower.rs:2027`; `ScriptView::apply_ui_effect` (`sampler-ksp/src/lib.rs:278`) only consumes `set_key_{color,type,pressed,name}` and the `set_control_par*` family. An emitted effect is not UI implementation. This breaks callback-driven pages/labels/geometry without any widget paint bug.
4. **Input and renderer evidence must stay separate.** The direct renderer sample tested a visible bound knob in each visible script, not every knob and not the native view. The initial upward-only main-page sample did not change, while the secondary-script sample did; the final bidirectional probe still changes **0/1** sampled main-page controls in both Original and Vector and **1/1** secondary-page controls in both. No knob/slider has a zero-length range. Separately, the sampled main control admits a native callback in 18.2 µs, finishes within 512 rendered frames, and emits three effects: two applied property effects and one **discarded `set_knob_label`**. The secondary sample admits without a callback in 0.81 µs. Neither emits a fault or drops an effect. These are native-core admission/512-frame observations, not production input-to-photon latency or proof of every control. The widgets audit must explain hit testing/sensitivity/native controls; the loop findings above explain the missing freshness, rejection reporting and feedback path.

Conflux is therefore **not a performance-only problem** and is not fixed by throttling screenshots, substituting a background, or changing only a knob's drag direction. The requested frontend/resources, input binding, callback effects, snapshot freshness and retained view state all have to connect.

## Actual end-to-end flow and timing

| Stage | Baseline path | Rate / ownership / cost | Status and consequence |
|---|---|---|---|
| Source initialization | `sampler-kontakt/src/load.rs:578–695`; KSP evaluator and persistence | Worker; compile/init, restore, export UI, bind modules | Partial; preserved original script slots differ from dense successful-instance order. Failed scripts are reported; successful siblings survive. |
| Script lifetime | `sound/v2.rs:158`; native `Runtime`/prepared plan | Audio-owned generation, globals, callback state | Correct substrate; independent of editor lifetime. Strings/arrays/other UI semantics remain incomplete. |
| User edit | `ui/part.rs:105–115` | Per rendered frame: allocate all-control `Vec`, extend `HashMap`, draw, scan all values for changes | Partial; scanning isn't an explicit semantic input event. Momentary/indexed/string events need their own payload. Passive rendering can submit edits (menu coercion). |
| Ingress | `plugin.rs:881`, queue at `:424` | Shared queue depth 256; optimistic atomic write precedes queue admission | Wrong; unknown ID/NaN accepted, full queue changes UI, no source generation/request ID/reply. |
| Audio admission | `plugin.rs:1586`; `sound/v2.rs:807` | Next process call/block boundary; all available ingress drained | Partial; native validation/admission exists, adapter ignores rejection. Hardcoded performance 0 / MIDI1 port 0 group 0 channel 0 / channels mask 1 needs routed UI-context tests. |
| Callback execution | native `invoke_control`, continuations, block fuel | No fabricated note; fixed continuation/fuel storage; production default fuel 10,000 | Good basis; synchronous acceptance, completion/fault, waits and UI display acknowledgement are distinct stages. |
| Scalar feedback | `plugin.rs:354`, `:1532` | All control IDs polled every ≈100 ms using `try_lock` | Wrong freshness; contended lock skips whole refresh; unchanged and changed scans cost the same. Poll occurs before that block's UI ingress. |
| Script effects | `sound/v2.rs:759`; `plugin.rs:1686` | Drain at block end into shared queue depth 1024 | Partial; runtime's own outbox holds 256. Its overflow drops later effects; GUI closure/slow loader can fill it. |
| Apply effects | `plugin.rs:891`; `sound/mod.rs:229` | Serialized `Load`, nominally about 10 Hz | Wrong completeness/freshness; ignores `Effect.plan`/part load epoch; unknown commands silently consumed by caller. |
| Publish properties | `sound/mod.rs:256` | Rebuild all `ScriptView::ui`s, re-read picture metadata, regenerate keys; one `Arc`/part/batch | Wrong invalidation granularity; scalar UI-property updates invalidate every script and every selected asset. No equality check for repeated values. |
| Worker scheduling | `plugin.rs:1025–1055`; `ui/mod.rs:859` | Same serialized worker loads presets, scans libraries, trims streams, updates UI | Partial; a long load head-of-line blocks other loaded parts' UI publication. GUI polls stopped hosts for loading, not for audio-owned edit execution. |
| Wake / readouts | `ui/mod.rs:175–262` | Display tick signature; readouts 100 ms; meter/loading animations 33 ms | Partial; not a universal 30 Hz UI cap. Pointer events have their own frame path. Property `Arc` wakes; scalar/key-only atomics lack complete signatures. |
| Frame snapshot | `ui/mod.rs:265–277`, `:869` | Clone whole small `View`, `Arc`s for larger data | Useful immutable boundary; no coherent scalar batch revision, pointer identity conflates data change with reload. |
| Face preparation | `ui/part.rs:25–60` | UI thread; resolve/cloning/scan/decode whenever outer `Arc` changes | Wrong cost boundary; synchronous filesystem/decryption/PNG decode and asset retirement can occur during an input frame. |
| Layout/render | `ui/ir_view.rs:174–195`; UI IR `:552–603` | Rebuild every visible node, compute draw order twice, walk parents for geometry/visibility | Partial; no retained geometry or per-widget memo reuse like v1. Cache by semantic revision, not pointer. |
| Native present | `vendor/mui-baseview/src/lib.rs:515,577` | MUI driver gating, CPU/GPU surface/present; existing stage timing hook | Good instrumentation foundation; present return is not scanout. Host lifecycle evidence does not establish script/frontend correctness. |

At 48 kHz/64 samples, a running host calls process every **1.333 ms**. An edit queued just after a boundary waits up to one block before admission, plus callback budget/waits. Scalar publication adds up to roughly **100 ms + block alignment**; worker property publication adds its own up-to-100-ms scheduling phase **and unbounded worker backlog/load time**, then a display interval/build/present. These are derived bounds under a continuously running host, not measured hardware input-to-photon percentiles. With no process calls, this audio-owned ingress is not serviced; the GUI loading poll does not execute the callback.

## Exhaustive loop matrix

“Correct” means the stated loop primitive is present and checked; “partial” includes unverified vendor fidelity. Counts are **static source exposure**, not instruments proved usable/unlocked. Symbol/property counts are in the census appendix. The widgets/params reports determine their precise source gesture, units and painter behavior.

| Widget | Required semantic loop payload and feedback | v2 status | Root cause → systemic fix; evidence |
|---|---|---|---|
| `ui_knob` | Integer range/default, source drag contract, one UI callback per actual edit, script/automation readback | Partial | Scalar substrate connects; no admission reply/coherent wake, global renderer IDs → retained typed binding + acknowledged edits; `part.rs:105`, `v2.rs:807`, `ir_view.rs:223`. |
| `ui_slider` | Same integer semantics, source axis/track and sensitivity | Partial | Same scalar path; metadata is not complete input handling → same bridge, preserve gesture contract. |
| `ui_button` | Press/release or source button semantics, distinguish programmatic assignment | Partial | Numeric snapshot/difference polling alone cannot preserve all edges under pressure → explicit source event edges with ordered acknowledgements. |
| `ui_switch` | Toggle with programmatic readback and callback distinction | Partial | Native toggle/integer semantics exist; freshness/rejection/reset mechanisms remain → same shared bridge. |
| `ui_menu` | Authored semantic item value, direct selection, dynamic items, readback without editing | Wrong | `ir_view.rs:307` substitutes first entry for unmatched value even without activation; part diff submits it → render read-only, explicit selection event; dynamic item effects consumed. Prior branch fixes passive coercion. |
| `ui_value_edit` | Integer commit/default, typed input/edit callback and readback | Partial | Numeric binding exists, full editor/gesture contract incomplete → explicit commit events through same native service. |
| `ui_table` | Indexed integer array edits, selected index context, coherent array updates | Missing loop | IR variable binding is not consumed by scalar `Values`; runtime array changes never arrive as table state → typed indexed array read/write, bounded revisioned payload. |
| `ui_xy` | Real array/cursor index, both axes, down/up/drag context | Missing loop | Scalar `f64` map cannot represent cursor array + index/event kind → preserve real array and event metadata. |
| `ui_text_edit` | String editing/commit/persistence/feedback | Missing loop | Native scalar control schema excludes strings; renderer placeholder → bounded string payload and explicit commit/callback. |
| `ui_label` | Script-set text/lines, scroll position and source drop context | Wrong runtime loop | `set_text`/`add_text_line` emitted but not applied; per-property writes rebuild face → canonical text effects, retained scroll state. |
| `ui_panel` | Parent/child hierarchy, visibility/position changes without control-state ownership | Partial | IR hierarchy exists; every property triggers global rebuild/reset → layout subtree invalidation, retain child input identity. |
| `ui_mouse_area` | Pointer down/up/drag/over, file/array drop and source context | Missing loop | No semantic event bridge/context in scalar comparison → bounded source event payload and capability/epoch checks. |
| `ui_waveform` | Attached zone/sample, cursor/flags/table edits and async waveform completion | Missing loop | `attach_zone`/`set_ui_wf_property` emitted but discarded, no async source completion bridge → owned zone/asset identities and generation-checked feedback. |
| `ui_wavetable` | Zone/frame/view parameters, presentation changes, asset updates | Missing loop | Metadata/placeholder is not live zone binding → same source/asset service with explicit updates. |
| `ui_file_selector` | File selection/navigation, selected filename and async callback | Missing loop | `fs_navigate` effect lacks consumer and selected string path → bounded file service, exact source epoch, response event. |
| `ui_level_meter` | Read-only program/group/bus/channel levels, attachment and colours | Partial/missing attachment | Binding can describe a meter; attachment effects have no consumer and KSP widget paints default → shared atomic meter source, paint without a full model publication. |

| UI command/callback family | Expected loop behavior | v2 status / evidence / fix |
|---|---|---|
| `make_perfview`, `expose_controls` | Select authored performance view separately from script-editor controls | Partial; evaluator records view flag but `main_face` accepts every nonempty model and chooses most widgets (`part.rs:45`) → source-declared performance-view eligibility and source identities. |
| `load_performance_view` | Resolve/import `.nckp`, apply control hierarchy and variables during initialization | Partial; loader imports literal view name (`load.rs:580`), native schema/state still incomplete → preserve exact source frontend and validation diagnostics. |
| `load_native_ui` | Instantiate requested versioned reactive/native frontend and bind to existing KSP values | Missing consumer; evaluator records request → shared semantic control service plus separate Komplete frontend; never label stock fallback complete. |
| `set_ui_color`, `set_ui_height`, `set_ui_height_px`, `set_ui_width_px`, `set_skin_offset`, `set_script_title` | Apply authored init window/page/background/title metadata; use source title in script selector | Partial; init model populated (`ksp/ui.rs:245`), runtime calls flagged init-only (`lower.rs:2093`); selector uses “Script n” (`part.rs:71`) → retain init context restrictions, consume page/title metadata in performance view. |
| `get_ui_id` | Source UI numbering distinct from stable musical ID and dense slot | Partial; source mapper exists; renderer still identifies `ir-{ordinal}` globally (`ir_view.rs:223`) → `(part epoch, source slot, source widget identity)` keys. |
| `$CONTROL_PAR_ALLOW_AUTOMATION`, `_AUTOMATION_ID`, `_AUTOMATION_NAME` | Export eligible controls to the host with stable parameter identity, begin/end gestures, ordered writes and automation readback | Missing production binding; frontend fills `Widget.automation` (`ksp/ui.rs:498`), but no reader of that metadata exists in the plugin/UI edit path. Native `Event::Control`/timeline primitives exist separately → stable per-part source-to-host parameter map, gesture notification, timestamped writes and shared readback; metadata alone is not automation support. |
| `set_control_par`, `_real`, `_str`, `_arr`, `_str_arr` | Typed property write; scalar state/native callback semantics distinct from presentation | Partial; effect consumer recognizes these (`ksp/lib.rs:308`), indiscriminate publication/reset; array/text state incomplete → canonical property changes with equality and dependency revisions. |
| `get_control_par`, `_arr`, `_str`, `_str_arr`, `_real_arr` | Read current source property/state in correct script context, never pull UI-thread state into audio | Partial; compiler/runtime mirrors are params owner's audit; publication model is separate → one authoritative source state + coherent UI snapshots, verify all getters against writes. |
| `move_control`, `move_control_px`, `hide_part` | Apply position/grid/hide aliases like corresponding properties | Wrong runtime delivery; emitted but not consumed → lower to common property mutations or canonical typed effects, not separate per-library paint behavior. |
| `set_text`, `add_text_line`, `set_control_help`, `set_knob_label`, `set_knob_unit`, `set_knob_defval` | Update text/help/display/reset metadata; changed appearance does not replace input value | Wrong runtime delivery; emitted but not consumed → same canonical properties; retain type-in/focus/gesture state. |
| `add_menu_item`, `set_menu_item_str/value/visibility`; getters | Dynamic menu collection changes and exact selection values | Wrong runtime delivery for emitted mutations → bounded collection snapshot and source callback event; avoid passive correction. |
| `set_table_steps_shown` | Presentation/table grid changes while preserving indexed data | Missing runtime consumer → canonical table presentation change, typed array state separately. |
| `attach_zone`, `set_ui_wf_property`, `get_ui_wf_property` | Widget/sample ownership and live cursor/flags/table feedback | Missing consumer; getter fidelity unverified → generation-owned zone/async waveform service. |
| `attach_level_meter` | Bind read-only UI to actual source meter | Missing runtime consumer → meter identity/destination service independent of script scene rebuilding. |
| `fs_navigate`, `fs_get_filename` | Navigate/select filename in source widget and callback context | Missing host/UI loop; init getter returns empty (`eval.rs:1246`) → explicit file service/result context. |
| `get_font_id` | Resolve font resource identity in correct source context | Partial init representation, missing bitmap font painter/async preparation → resource worker; runtime/init legality remains source-specific. |
| `set_key_color/name/type/pressed` | Publish script keyboard state, preserving unaffected performance view | Partial; consumer handles four aliases but rebuilds interfaces and keys for each changed part → separate key revision/snapshot. |
| Key ranges / pressed-support | Keyboard ranges/support contract and keyboard readback | Missing effect consumer for other key commands → canonical keyboard-state update; separate from control schema. |
| `on ui_control` | Admit value+callback together, independent plan owner, correct ordering/waits/outcomes | Partial; native ownership present, production shell drops failures/outcomes → connect native replies and lifecycle. |
| `on ui_controls`, `on ui_update` | Vendor aggregate/update callback semantics | Missing; `sema.rs:108–126` rejects unsupported callbacks → implement only from reference trace; do not invent an editor tick callback. |
| `on listener`, `on pgs_changed`, `on persistence_changed`, `on async_complete` | Source-timed/property/restore/completion behavior may modify any UI state | Partial runtime; surviving scalar/property changes use defective feedback path → connect signals to revisioned native/source state; async admission/completion is its own obligation. |
| Wheel/key/fine-adjust/reset/drag/drop | Semantic edit/event independent of presentation, correct capture/focus/modifiers | Partial scalar gestures; array/text/file/context paths missing → source gesture profiles + common event transport; detailed failures in UI_WIDGETS. |
| Fonts/images/strips/wallpapers/alpha/z-layer/parenting | Property writes affect relevant asset/layout/paint dependencies; no audio-side decoding | Partial metadata, wrong publication/cache lifetime → source asset IDs + worker preparation and granular revisions; detailed semantics in UI_RENDER/UI_PARAMS. |

Every `$CONTROL_PAR_*` known to the supplied manifests, plus observed corpus extensions, is listed individually in the census appendix. The adapter's acceptance of an arbitrary property name does **not** mean its semantic renderer/input behavior is implemented.

## Ranked systemic fixes, effort and unlock accounting

These are general mechanisms. “Unlocked” is not the static exposure count: an instrument may have several blockers. Use the shared rendered census to compute union coverage after each fix; no single-cause pass claim is justified by source-token presence.

| Rank | Severity | Systemic change / files | Effort | Instruments exposed / unlock test |
|---|---|---|---|---|
| 1 | P0 | Keep presentation, script selection and pending input across property publications; separate source/load epoch from data revision. `ui/part.rs`, `plugin.rs`, UI state. Factory default Original independently of unsupported diagnostics; read/use persisted per-part choice and global Original/Vector/generated setting through one effective-mode resolver. | S–M | Every authored UI publication; Conflux proven. Pixel witness must preserve Original, selected script, scroll/focus/drag. |
| 2 | P0 | Consume every canonical UI effect, including aliases; report unsupported/invalid effects instead of dropping them. `sampler-ksp/lib.rs`, `sound/mod.rs`, source lowering. | M | Source UI command counts below; Conflux callback-driven labels/pages. Exact replay tests for all command families. |
| 3 | P0 | Instantiate the requested source frontend; validate native/reactive UI requests against actual capabilities. `sampler-kontakt/load.rs`, frontend package resolver, `ui/part.rs`. | L | `load_native_ui`/`.nckp` exposure counts; Conflux mandatory. “Fallback drew” must fail native completeness gate. |
| 4 | P0 | Use existing native `ControlClient`/request/reply and plan/context ownership; retain producer storage off audio, reject stale edits/effects, and retire/report callback outcomes through the existing native lifecycle. `plugin.rs`, `sound/v2.rs`, `sampler-core/control/transfer.rs`. | M | All editable interfaces; Conflux owns 390 scalar definitions. No admitted edit loses an acknowledgement; old epoch never changes replacement. |
| 5 | P0 | Publish coherent changed scalar batches/revisions; wake editor independently of meters. Refresh after admitted work, preserve optimistic pending values until reply. `plugin.rs`, `ui/mod.rs`, `ui/part.rs`. | S–M | All scalar script/automation updates; scalar witness fails until connected. Idle UI must update without a played note. |
| 6 | P0 | Namespace renderer/input identity by part/source script/widget. `ui/ir_view.rs`, `ui/part.rs`. | S | Multi-part/UI instances; prior branch already supplies implementation. Two simultaneous same-library parts must not share focus/drag IDs or values. |
| 7 | P1 | Move asset preparation off UI; cache by source epoch/resource identity/presentation need; retain unchanged decoded/GPU assets and parsed picture metadata. `pictures.rs`, `sound/ScriptUi`, `ui/part.rs`. | M | Image/property UI exposure below. Repeated labels/key changes must perform zero filesystem/decrypt/decode work; first-frame worker timing tracked. |
| 8 | P1 | Decouple short UI publication/reply work from serialized long preset loads. Reuse existing task/worker ownership, bounded wake/deadline, not one worker per widget. `plugin::Load`, UI publisher. | M | Every active UI while another part loads. A deliberately blocked load must not stall updates of an already loaded part. |
| 9 | P0/P1 | Add typed string/indexed int/real array/event and read-only meter/zone bindings, preserving source callback index/context and transaction semantics. Core schema/services, source adapter, UI IR/value bridge. | L | Table/XY/text/file/waveform/meter counts below. Every requested widget has a real round-trip contract, not a scalar encoding/placeholder. |
| 10 | P1 | Retain resolved page geometry/draw order and per-widget MUI memo dependencies. Separate meter/cursor drawing from full script rebuild. Add publication/ingress/outcome/paint stage measurements. `ir_view.rs`, `part.rs`, existing native timing hooks. | M | All visible faces; main Conflux has 411 widgets/134 visible. Identical publication zero rebuild/decode; one value change invalidates only dependent nodes. |

**Shared P0 callback lifecycle:** `src/sound/v2.rs:574–717` renders and takes faults/ended notes but never calls `Runtime::flush_behaviors`. The CPU auditor’s authored probe proves one released note remains retained until outcomes are accepted. Plan-owned UI callbacks also retain their completion records: `sampler-core/behavior.rs:984–1005` decrements plan callback ownership only on accepted outcomes. Successful `Finished` outcomes can be reclaimed under admission pressure (`:1014–1046`); faults/cancellations cannot. Thus this is not proof that every successful knob eventually stops, but failures can exhaust admission and completion reporting is absent. Rank 4 includes lifecycle integration; CPU owner W9 is implementing the shared sound-adapter fix. Gate repeated UI callbacks, waits, failures, replacement and outcome backpressure through the actual adapter, including zero retained terminal ownership after accepted completion. Evidence: `audit/cpu-20261008@63f6423b`, `docs/audit-2026-10-08/cpu.md:135–139`.

Also P1: runtime effect overflow is an observable correctness fault, not an invisible performance throttle. `EFFECT_CAPACITY=256` (`sampler-core/ops.rs:14`), shell queue=1024 (`plugin.rs:539`); `dropped_effects()` is not included in `sound/v2.rs:769` runtime report. Preserve ordered semantic events, backpressure where possible, and report irrecoverable drops. Do not coalesce button edges, table cell edits or callbacks. Last-write compaction is valid only for proven idempotent final presentation properties within a batch.

## Target architecture: complete translation and faster steady state

Use the services already implemented; the missing work is composition and source semantics.

1. **The prepared source owns meaning.** Keep original source slot/widget identities, language/version, typed controls, source parameters, arrays/strings, callback descriptors, assets and unsupported operations. Separate immutable control schema from mutable source values and presentation properties. Keep original UI-id numbering and persistent/native `ControlId` mappings explicit. The source adapter translates source operations once; it does not search or reinterpret library names.
2. **Audio owns musical mutation.** One bounded native request/reply channel per live part generation, with the captured performance/routing context, sequence and typed payload. Existing `ControlRequest` retains `PlanId`, optional expected revision, `Invoke/Edit/Recall/Capture`; replies return request identity, coherent revision, behavior identity and exact rejection (`control/transfer.rs:7–180`). Connect this rather than inventing another queue. Accepted value and callback admission remain atomic; callback completion/fault is a later retained outcome. Validate finite/range/type/input at the boundary. Eligible exported controls also need stable host parameter IDs, begin/end gesture notifications and timestamped native control events; host automation and source programmatic assignments must not accidentally invoke UI callbacks.
3. **Presentation publication is short, off audio and independently scheduled.** Runtime/source changes carry generation and separate value/property/key/schema revisions. Apply all source UI effects to a bounded source model. Validate/batch at callback/publication boundaries; emit a coherent snapshot and changed identities. A schema/load change replaces the prepared scene; a value, label, colour or key change does not. Stop scanning/reserializing all properties or rereading PNGs to discover change. Reuse current immutable `Arc` snapshots for slow structural data; use coherent revisioned double buffers/prepared arrays for changing typed state, with ownership and audio allocation checks.
4. **Assets are source-owned worker products.** Resolve/decrypt/read/parse/decode fonts/images/strips and waveform peaks off the editor/audio threads. Cache resource metadata and decoded preparations by source epoch and real resource identity. Validate completion generation before publication. Original/Vector change resource needs, never control values/callback ownership. Retain backgrounds in Vector; generated KONTRA has a separate semantic layout and does not masquerade as authored Vector. Replacement retires resources off audio. Expose unavailable meaning rather than silently dropping it.
5. **Editor state is editor state.** Factory default **Original**, independent of unsupported metadata; one resolver honors explicit persisted part/global user choices. Per-part presentation, selected source script/page, scroll, focus and drag/type-in/drop state persist independently of source property snapshots. Stored settings/part overrides actually drive the performance view and header icon. Namespaced stable IDs preserve input capture. MUI retains resolved geometry/memoized subtrees keyed by relevant revisions; only affected widgets/layout descendants/asset users rebuild. The current original/script title is visible in the selector; script-editor-only controls never win merely by being numerous.
6. **Wake only for work, at the right urgency.** Input/reply/pending scalar changes wake the next possible display frame; values never wait for a meter to become nonzero. Coherent value feedback and event acknowledgements are independent of the 100-ms CPU/loading readouts. Meter/playhead animation remains separately rate-limited. An idle unchanged editor should not rebuild/paint. A closed editor must not stall/drop authoritative source state; reopen takes one coherent snapshot. Stopped-host behavior must be specified and tested: no second thread may call the audio-owned runtime concurrently just to make a stopped knob appear live.

The minimum safe first step is retaining view state + connecting existing scalar replies/revisions. Complete support then requires all typed/event/source frontends, not a broader `HashMap<ControlId,f64>` or library-specific fallback. “Better than v1” should mean complete semantics with bounded, measured per-change work; v1's memo/revision/asset practices are reusable examples, not Kontakt/Falcon behavior oracles.

## Tests that prove the fixes

The committed `loop_audit` tests intentionally assert **observed baseline failure behavior**. Their names/comments say so; invert them or replace with the following completion gates when implementing fixes. Merely keeping these tests green does not certify a repaired product.

| Gate | Runnable check required |
|---|---|
| Publication continuity | Select Original and a non-largest script, start a drag/type-in/scroll; emit label/key/no-op/picture/geometry changes; verify selected presentation/script/input identity and pending value survive. Actual instrument replacement alone resets source-specific state according to persisted policy. |
| Idle freshness | Change scalar only via a script/MIDI/host automation while meters/keys/readouts remain unchanged; assert native revision and displayed value change by the next permitted publication/display, with zero asset decode. |
| Admission/backpressure | Fill ingress/continuations/replies independently; reject NaN, stale generation/unknown ID/type/range; rejected requests do not become displayed authoritative values. Every admitted edit is applied once and returns exactly one admission reply. Callback failure is reported distinctly. |
| Replacement | Queue edit, callback wait, property effect and asset/file completion for A; install B in same slot with same public IDs; prove none mutates B. Late replies may settle A's pending request but cannot retarget B. |
| Exhaustive UI effects | Authored fixture emits every property setter and alias from `ui_control`, listener, PGS/restore/async contexts; source readback and UI snapshot must agree. Repeating idempotent same value produces no scene/resource invalidation. Unsupported semantics produce a structured report. |
| Multiple scripts/parts | Two same-library parts, each with several UI scripts, failed/empty siblings, duplicate source widget ordinals and different values; simultaneous keyboard/focus/drag changes stay scoped. Choose source-declared performance pages and retain original slot titles. |
| Typed widgets | One fixture for all 16 UI widgets; string, integer-array/index, real-array/XY event, file selection, waveform/zone attachment and read-only meter round-trip. UI closure/presentation change cannot alter source state. |
| Performance | Same authorized native view, state, layout, device scale, renderer and compiler profile for v1/v2/vendor comparison. Warm 8; sample ≥24 idle and ≥24 changing frames; report build/layout/publication/decode/CPU paint/raster/GPU/present separately. A 24-sample “p99” is effectively a maximum, not a population p99. |
| Worker isolation | Hold another part's file/compile worker for 5 s; loaded part's admitted UI edits/readback/property updates still meet target. Measure request→admission→callback completion→snapshot→display with monotonic timestamps. |
| Audio safety | Existing native heap guards cover requests/replies, callback waits, capture and rejection. Add production adapter guard: zero audio allocation/free/blocking locks/file I/O; bounded drained work per block; pressure/fault counters visible in report. |
| Corpus completion | Join per-item frontend/source/widget/property exposure with the shared rendered census. Re-run affected IDs after a fix; count category unions and fully usable instruments, not aggregate widgets or token occurrences. |

Proposed engineering budgets (targets, not NI timing claims): 48 kHz/64-frame audio deadline 1.333 ms; UI input admission next processed boundary under available capacity; active scalar feedback next display opportunity with no deliberate 100-ms delay; unchanged UI zero build/decode/paint; incremental Conflux build/layout p99 <2 ms, full frame p99 <16.67 ms at 60 Hz on a declared renderer/device. Measure and adjust after the native frontend/assets work; current incomplete fallback is not the benchmark acceptance scene.

## Available prior work: merge/review, not reimplement

| Branch / frozen revision | Reusable work | Remaining boundary |
|---|---|---|
| `origin/v2/gpt-kontakt-ui@0be9ed3f` | Per-control change revision in `PartShared`; Watch wake; finite/known-ID/full-queue ingress checks; load-generation-tagged edits; post-ingress scalar refresh; namespaced render IDs; removes passive menu coercion; saved control IDs/values and recall. NICNT/NKR routing and explicit unavailable-native diagnostics. | It still rebuilds a new `Face` on interface-pointer change; it does not connect native admission/completion replies or generation-validate property effects. Its own parity report says header/generated paths are pending; do not count them as shipped. Review changed source semantics with params/widgets owners. |
| `origin/v2/gpt-uvi-ui@24c70a25` (`dcbc07c1` core UI commit) | Bounded worker edit handoff, Lua widget callbacks, coherent immutable UI snapshots, typed float/bool/table/XY state, resources/fonts and independent widget/custom persistence; mapper/default fixes. | UVI engine readback/parameter definition and native-reference order remain open in its report. Worker widget callbacks alone do not establish musical real-time Lua safety. Reuse shared service ideas; keep UVI language/event semantics separate. |
| Baseline native `sampler-core` (`CONTROL_STATE.md`) | Stable typed scalar IDs, generation ownership, atomic admission, `ControlClient` request/reply, capture/recall, plan-owned callbacks, waits/outcomes, timestamped controls and routed performance context with tests. | The current production bridge bypasses much of this. Strings/arrays/dynamic schema/UI source events remain distinct work. |
| v1 `0cb7a8a0` | `ui/perf_view.rs:268` retained Original/Vector/generated choice; `:346` immutable/source revision dependencies; `:418,513` per-widget memoization; async art and per-source live versions. `plugin.rs:1391–1471` revisioned publication; `ui/mod.rs:338` watches live revisions/pending edits. | Reference implementation only. Preserve useful ownership/cache mechanisms, rewrite against native v2 services, verify behavior against Kontakt rather than copy v1 limitations. |
| RE `UI_NATIVE_PRESERVATION.md` / `FALCON_RUNTIME_UI_GROUNDWORK.md` | Native visibility/accessibility/IME lifecycle and source frontend distinctions; exact UVI callback/event/property obligations. | Linux native surface lifecycle tests do not prove source translation or cross-platform/DAW fidelity. UI docs' “no v2 DAW build” statement is stale against the current V2Core composition; source and executed checks take precedence. |

## Unknowns and how to measure them

- **End-to-end input-to-photon and the 31–36 ms variance:** current probes do not run a live DAW/GPU/native source view. Use existing native timing hook (`diagnostics.rs:374`) plus ingress/reply/publication timestamps and per-run CPU scheduling trace. Hold same rendered content, profile, scale, warm cache and CPU load. Separate presentation submission from scanout; report tail samples and environment. Do not attribute a 5× repeat variance to one function without a trace.
- **All Conflux callbacks and source/native control events:** this audit samples knobs and generic failure mechanisms. Need the native frontend owner to make the requested view available, then widgets owner exercises every visible control, source index/event kind and legal callback path. Programmatic assignments must not invoke UI callbacks recursively.
- **Source callback/update ordering and pressure:** native source fixtures cover ordering/ownership, not Kontakt timing of aggregate `ui_controls`, `ui_update`, GUI-close updates or every async callback. Use authorized vendor traces and authored fixtures. Preserve strict semantic events until evidence permits coalescing.
- **Exact instruments unlocked:** source census counts expose required mechanisms, not compounded render/runtime failures. Join the UI_CENSUS results by item/program/source slot and classify usable/native/frontend-complete separately. No exposure count is a guaranteed unlock count.
- **Host stopped/sleeping/editor reopened:** test standalone plus real CLAP/VST3 hosts with processing suspended and resumed. Specify whether an edit waits, requests host processing, or reports pending. Never invoke one runtime from both GUI and audio threads.
- **Full string/array persistence and generated layout:** headless scalar capture is not complete source persistence; share the params owner's restore evidence. Generated KONTRA must expose real semantic bindings and readable names without losing authored meaning.
- **Whole-corpus unreadable/timeouts:** preserve them as excluded/failed, never as zero widgets or unsupported-native negatives. The source corpus scanner is metadata-only; it does not certify rendered UI or callback execution for every installed instrument.

## Other scopes

White background/resource/native frontend paths, source NCKP geometry, tiny strips, light-text contrast and fonts belong to render; drag axis/sensitivity, hit routing, parent wheel capture, reset/fine/keyboard/file interactions belong to widgets; parameter getter/type/unit/default/array/persistence semantics belong to params. This report identifies their coupling to publication/event services and does not introduce a per-library fix or modify their worktrees. DSP/streaming/load fidelity and UVI musical worker safety remain their owners' scope.

Positive foundations: immutable large-data snapshots; bounded queues and native continuation ownership; original source slot identities; successful script siblings retained; native timing instrumentation; MUI input/layout/accessibility host services; source resource bytes stay in memory. These should be connected and validated, not replaced by a generic UI framework.

## Measurement and corpus appendix

Final probe/census counts, symbol matrices and reproducible commands follow below. They are generated from sanitized metrics and repository specification symbols; scripts, decrypted images, samples and keys are never written by these probes.


### Unmatched Conflux measurements (optimized `ci`, direct IR at fixed dimensions)

Full load 12444.375 ms; publication median 0.654 ms, maximum 0.874 ms. The earlier fresh run had 4.811 s load and 0.380 ms median publication; this repeat had 12.444 s and 0.654 ms. Background machine load was not isolated, so neither is a guaranteed latency. Geometry: main 970×499; secondary 633×92; invisible third 633×100. No zero-range knob/slider in any interface.

| Source slot / mode | Widgets / visible / scalar-bound | Build+layout median / max ms | Fresh CPU paint median / max ms | Decoded bytes / missing | Bidirectional sample changed / tested |
|---|---|---|---|---|---|
| 2 / Bitmap | 411 / 134 / 313 | 1.144 / 1.360 | 6.101 / 7.834 | 11112 / 1 | 0 / 1 |
| 2 / Vector | 411 / 134 / 313 | 1.253 / 3.042 | 6.186 / 8.975 | 5064 / 1 | 0 / 1 |
| 3 / Bitmap | 22 / 5 / 22 | 0.061 / 0.242 | 2.283 / 2.571 | 0 / 0 | 1 / 1 |
| 3 / Vector | 22 / 5 / 22 | 0.063 / 1.080 | 2.271 / 4.224 | 0 / 0 | 1 / 1 |
| 4 / Bitmap | 1 / 0 / 1 | 0.017 / 0.041 | 2.052 / 6.113 | 0 / 0 | 0 / 0 |
| 4 / Vector | 1 / 0 / 1 | 0.014 / 0.023 | 2.017 / 4.483 | 0 / 0 | 0 / 0 |

Callbacks sampled independently in the prepared native runtime (32 voice slots, no input samples, default callback fuel). Each runs eight 64-frame blocks at the engine’s default rate; the “after 512” result is a checkpoint, not an exact completion time. The test invokes the domain midpoint directly, bypassing the failing renderer gesture and production adapter.

| Sample | Admission µs | Callback / checkpoint outcome | Effects emitted / applied / ignored | Fault / drops |
|---|---|---|---|---|
| 1 | 18.200 | True / Finished | 3 / 2 / {'set_knob_label': 1} | None / 0 |
| 2 | 0.810 | False / None | 0 / 0 / {} | None / 0 |

### Whole installed Kontakt source exposure

Manifest **834 rows: 781 NKI + 53 NKM**. Shared scanner metadata snapshot: **1/834** cached items; **1** successfully loaded Kontakt program records. **Whole-corpus counts are Pending**, owned by the ongoing shared census. Coordinator instruction: push this report with counts Pending; census will publish `~/.cache/kontra-scan/results/v2/symbol-aggregates.tsv` and fill the exposure counts alongside UI_PARAMS. Tables below show only observed snapshot incidence, as distinct **NKI / NKM containers / successful programs**; every cell explicitly retains Pending. No absent symbol in this limited snapshot is a corpus-wide negative. NCKP/imported/generated widget declarations are not lexical source tokens; rendered IR widget counts must be joined separately. The current scanner has a restricted identifier whitelist: uncollected names are marked Uncollected, never zero. Lexical source exposure is not callback execution, compiled support, a distinct-library count or a fully usable/unlocked instrument. Source branches, computed identifiers, failed/bypassed scripts and encrypted/unreadable sources limit inference. UVI is outside these counts.

The shared `kontra-scan` binaries and metadata are the sole corpus collector (`tools/kontra-scan@54f7ea57`, baseline 7e82b152). An initial independent 245-item metadata pilot was stopped and its code discarded after re-reading the no-independent-collectors addendum; its counts are excluded. No independent whole-corpus collector is committed.

| Widget identifier (source only) | NKI / NKM / programs |
|---|---|
| `ui_knob` | 1 / 0 / 1; full Pending |
| `ui_slider` | 1 / 0 / 1; full Pending |
| `ui_button` | 0 / 0 / 0; full Pending |
| `ui_switch` | 1 / 0 / 1; full Pending |
| `ui_menu` | 1 / 0 / 1; full Pending |
| `ui_table` | 0 / 0 / 0; full Pending |
| `ui_xy` | 0 / 0 / 0; full Pending |
| `ui_waveform` | 0 / 0 / 0; full Pending |
| `ui_wavetable` | 0 / 0 / 0; full Pending |
| `ui_file_selector` | 0 / 0 / 0; full Pending |
| `ui_level_meter` | 0 / 0 / 0; full Pending |
| `ui_value_edit` | 1 / 0 / 1; full Pending |
| `ui_label` | 0 / 0 / 0; full Pending |
| `ui_text_edit` | 0 / 0 / 0; full Pending |
| `ui_panel` | 0 / 0 / 0; full Pending |
| `ui_mouse_area` | 0 / 0 / 0; full Pending |

### Individual UI command/callback exposure

All command families have required behavior/status/evidence in the exhaustive loop matrix above. Zero means not observed in successful sources, not supported or forbidden. Repository manual aliases `load_komplete_ui`, `komplete_scripts`, `performance_view`, `show_gui`, `show_menu`, `add_text`, and real-array setter/getter spellings are included individually; their exact vendor legality/version remains unverified and a spelling’s presence must not be confused with a working consumer. Source callback tokens also match lexical identifiers outside `on` declarations, so these counts are conservative exposure.

| Identifier | NKI / NKM / programs | Loop family / current status |
|---|---|---|
| `add_menu_item` | 1 / 0 / 1; full Pending | Missing/unverified runtime delivery; init modeling/emission alone is insufficient |
| `add_text` | Uncollected; full Pending | Missing/unverified runtime delivery; init modeling/emission alone is insufficient |
| `add_text_line` | 0 / 0 / 0; full Pending | Missing/unverified runtime delivery; init modeling/emission alone is insufficient |
| `async_complete` | Uncollected; full Pending | Callback/event; partial feedback/outcomes |
| `attach_level_meter` | Uncollected; full Pending | Missing/unverified runtime delivery; init modeling/emission alone is insufficient |
| `attach_zone` | 0 / 0 / 0; full Pending | Missing/unverified runtime delivery; init modeling/emission alone is insufficient |
| `change_listener_par` | Uncollected; full Pending | Missing/unverified runtime delivery; init modeling/emission alone is insufficient |
| `expose_controls` | Uncollected; full Pending | Init/frontend metadata; partial/missing source frontend |
| `fs_get_filename` | Uncollected; full Pending | Missing/unverified runtime delivery; init modeling/emission alone is insufficient |
| `fs_navigate` | Uncollected; full Pending | Missing/unverified runtime delivery; init modeling/emission alone is insufficient |
| `get_control_par` | 1 / 0 / 1; full Pending | Source readback; partial/unverified (params) |
| `get_control_par_arr` | 1 / 0 / 1; full Pending | Source readback; partial/unverified (params) |
| `get_control_par_real` | 0 / 0 / 0; full Pending | Source readback; partial/unverified (params) |
| `get_control_par_real_arr` | 0 / 0 / 0; full Pending | Source readback; partial/unverified (params) |
| `get_control_par_str` | 1 / 0 / 1; full Pending | Source readback; partial/unverified (params) |
| `get_control_par_str_arr` | 0 / 0 / 0; full Pending | Source readback; partial/unverified (params) |
| `get_font_id` | Uncollected; full Pending | Source readback; partial/unverified (params) |
| `get_key_color` | Uncollected; full Pending | Source readback; partial/unverified (params) |
| `get_key_name` | Uncollected; full Pending | Source readback; partial/unverified (params) |
| `get_key_triggerstate` | Uncollected; full Pending | Source readback; partial/unverified (params) |
| `get_key_type` | Uncollected; full Pending | Source readback; partial/unverified (params) |
| `get_keyrange_max_note` | Uncollected; full Pending | Source readback; partial/unverified (params) |
| `get_keyrange_min_note` | Uncollected; full Pending | Source readback; partial/unverified (params) |
| `get_keyrange_name` | Uncollected; full Pending | Source readback; partial/unverified (params) |
| `get_menu_item_str` | Uncollected; full Pending | Source readback; partial/unverified (params) |
| `get_menu_item_value` | Uncollected; full Pending | Source readback; partial/unverified (params) |
| `get_menu_item_visibility` | Uncollected; full Pending | Source readback; partial/unverified (params) |
| `get_ui_id` | 1 / 0 / 1; full Pending | Source readback; partial/unverified (params) |
| `get_ui_wf_property` | Uncollected; full Pending | Source readback; partial/unverified (params) |
| `hide_part` | 1 / 0 / 1; full Pending | Missing/unverified runtime delivery; init modeling/emission alone is insufficient |
| `komplete_scripts` | Uncollected; full Pending | Init/frontend metadata; partial/missing source frontend |
| `listener` | Uncollected; full Pending | Callback/event; partial feedback/outcomes |
| `load_komplete_ui` | 0 / 0 / 0; full Pending | Init/frontend metadata; partial/missing source frontend |
| `load_native_ui` | Uncollected; full Pending | Init/frontend metadata; partial/missing source frontend |
| `load_performance_view` | 1 / 0 / 1; full Pending | Init/frontend metadata; partial/missing source frontend |
| `make_instr_persistent` | Uncollected; full Pending | Persistence; partial, params-owned source fidelity |
| `make_perfview` | 0 / 0 / 0; full Pending | Init/frontend metadata; partial/missing source frontend |
| `make_persistent` | 1 / 0 / 1; full Pending | Persistence; partial, params-owned source fidelity |
| `move_control` | 1 / 0 / 1; full Pending | Missing/unverified runtime delivery; init modeling/emission alone is insufficient |
| `move_control_px` | 1 / 0 / 1; full Pending | Missing/unverified runtime delivery; init modeling/emission alone is insufficient |
| `performance_view` | Uncollected; full Pending | Init/frontend metadata; partial/missing source frontend |
| `persistence_changed` | Uncollected; full Pending | Callback/event; partial feedback/outcomes |
| `pgs_changed` | Uncollected; full Pending | Callback/event; partial feedback/outcomes |
| `read_persistent_var` | 0 / 0 / 0; full Pending | Persistence; partial, params-owned source fidelity |
| `remove_keyrange` | Uncollected; full Pending | Missing/unverified runtime delivery; init modeling/emission alone is insufficient |
| `set_control_help` | Uncollected; full Pending | Missing/unverified runtime delivery; init modeling/emission alone is insufficient |
| `set_control_par` | 1 / 0 / 1; full Pending | Property effect; partial, wholesale invalidation; aliases need source validation |
| `set_control_par_arr` | 1 / 0 / 1; full Pending | Property effect; partial, wholesale invalidation; aliases need source validation |
| `set_control_par_real` | 0 / 0 / 0; full Pending | Property effect; partial, wholesale invalidation; aliases need source validation |
| `set_control_par_real_arr` | 0 / 0 / 0; full Pending | Property effect; partial, wholesale invalidation; aliases need source validation |
| `set_control_par_str` | 1 / 0 / 1; full Pending | Property effect; partial, wholesale invalidation; aliases need source validation |
| `set_control_par_str_arr` | 1 / 0 / 1; full Pending | Property effect; partial, wholesale invalidation; aliases need source validation |
| `set_key_color` | 1 / 0 / 1; full Pending | Consumed key effect; wrong whole-face invalidation |
| `set_key_name` | 0 / 0 / 0; full Pending | Consumed key effect; wrong whole-face invalidation |
| `set_key_pressed` | 1 / 0 / 1; full Pending | Consumed key effect; wrong whole-face invalidation |
| `set_key_pressed_support` | 1 / 0 / 1; full Pending | Missing/unverified runtime delivery; init modeling/emission alone is insufficient |
| `set_key_type` | 1 / 0 / 1; full Pending | Consumed key effect; wrong whole-face invalidation |
| `set_keyrange` | Uncollected; full Pending | Missing/unverified runtime delivery; init modeling/emission alone is insufficient |
| `set_knob_defval` | Uncollected; full Pending | Missing/unverified runtime delivery; init modeling/emission alone is insufficient |
| `set_knob_label` | Uncollected; full Pending | Missing/unverified runtime delivery; init modeling/emission alone is insufficient |
| `set_knob_unit` | Uncollected; full Pending | Missing/unverified runtime delivery; init modeling/emission alone is insufficient |
| `set_listener` | 1 / 0 / 1; full Pending | Missing/unverified runtime delivery; init modeling/emission alone is insufficient |
| `set_menu_item_str` | 1 / 0 / 1; full Pending | Missing/unverified runtime delivery; init modeling/emission alone is insufficient |
| `set_menu_item_value` | Uncollected; full Pending | Missing/unverified runtime delivery; init modeling/emission alone is insufficient |
| `set_menu_item_visibility` | 1 / 0 / 1; full Pending | Missing/unverified runtime delivery; init modeling/emission alone is insufficient |
| `set_script_title` | 1 / 0 / 1; full Pending | Init/frontend metadata; partial/missing source frontend |
| `set_skin_offset` | 0 / 0 / 0; full Pending | Init/frontend metadata; partial/missing source frontend |
| `set_table_steps_shown` | 1 / 0 / 1; full Pending | Missing/unverified runtime delivery; init modeling/emission alone is insufficient |
| `set_text` | 1 / 0 / 1; full Pending | Missing/unverified runtime delivery; init modeling/emission alone is insufficient |
| `set_ui_color` | 1 / 0 / 1; full Pending | Init/frontend metadata; partial/missing source frontend |
| `set_ui_height` | 0 / 0 / 0; full Pending | Init/frontend metadata; partial/missing source frontend |
| `set_ui_height_px` | 1 / 0 / 1; full Pending | Init/frontend metadata; partial/missing source frontend |
| `set_ui_wf_property` | 0 / 0 / 0; full Pending | Missing/unverified runtime delivery; init modeling/emission alone is insufficient |
| `set_ui_width_px` | 1 / 0 / 1; full Pending | Init/frontend metadata; partial/missing source frontend |
| `show_gui` | Uncollected; full Pending | Missing/unverified runtime delivery; init modeling/emission alone is insufficient |
| `show_menu` | Uncollected; full Pending | Missing/unverified runtime delivery; init modeling/emission alone is insufficient |
| `ui_control` | 1 / 0 / 1; full Pending | Callback/event; partial feedback/outcomes |
| `ui_controls` | 0 / 0 / 0; full Pending | Unsupported aggregate/update callback |
| `ui_update` | 0 / 0 / 0; full Pending | Unsupported aggregate/update callback |
| `watch_array_idx` | Uncollected; full Pending | Missing/unverified runtime delivery; init modeling/emission alone is insufficient |
| `watch_var` | Uncollected; full Pending | Missing/unverified runtime delivery; init modeling/emission alone is insufficient |

### Every control-parameter publication obligation

This is the scope-4 **loop** status for each supplied/observed property. Units, getter/setter type, legal widget/context/default and precise source semantics belong to UI_PARAMS; the supplied symbol index is mechanically extracted and explicitly not a semantic manual. Each mutable valid property requires authoritative source readback, ordered generation-scoped mutation and granular feedback. `IR` means the init exporter lists it or emits its colour/value representation; it does **not** certify painter/input support. `Unsupported` means the exporter flags generic writes rather than implementing a complete presentation path. Both routes currently share wholesale property publication/reset. Novel corpus spellings are marked uncertain.

| Parameter | Expected loop dependency / feedback | v2 exporter / loop status | NKI / NKM / programs |
|---|---|---|---|
| `$CONTROL_PAR_ACTIVE_INDEX` | File/menu/array selection or item state; explicit index/string and source event | Unsupported generic presentation; missing/uncertain semantics | 0 / 0 / 0; full Pending |
| `$CONTROL_PAR_ALLOW_AUTOMATION` | Host identity/eligibility/name; stable parameter map and gestures | IR, partial delivery/retention | 1 / 0 / 1; full Pending |
| `$CONTROL_PAR_AUTOMATION_ID` | Host identity/eligibility/name; stable parameter map and gestures | IR, partial delivery/retention | 1 / 0 / 1; full Pending |
| `$CONTROL_PAR_AUTOMATION_NAME` | Host identity/eligibility/name; stable parameter map and gestures | IR, partial delivery/retention | 1 / 0 / 1; full Pending |
| `$CONTROL_PAR_BAR_COLOR` | Corresponding widget colour/opacity; repaint affected users | IR, partial delivery/retention | 0 / 0 / 0; full Pending |
| `$CONTROL_PAR_BASEPATH` | File/menu/array selection or item state; explicit index/string and source event | IR, partial delivery/retention | 0 / 0 / 0; full Pending |
| `$CONTROL_PAR_BG_ALPHA` | Corresponding widget colour/opacity; repaint affected users | Unsupported generic presentation; missing/uncertain semantics | 0 / 0 / 0; full Pending |
| `$CONTROL_PAR_BG_COLOR` | Corresponding widget colour/opacity; repaint affected users | IR, partial delivery/retention | 0 / 0 / 0; full Pending |
| `$CONTROL_PAR_COLUMN_WIDTH` | Corresponding text/label/state/identity metadata; source readback and targeted feedback | IR, partial delivery/retention | 0 / 0 / 0; full Pending |
| `$CONTROL_PAR_CURSOR_PICTURE` | Resource/style/frame identity; worker preparation and changed users only | IR, partial delivery/retention | 0 / 0 / 0; full Pending |
| `$CONTROL_PAR_CUSTOM_ID` | Custom source metadata identity; preserve independently of native ControlId | Unsupported generic presentation; missing/uncertain semantics | 1 / 0 / 1; full Pending |
| `$CONTROL_PAR_DEFAULT_VALUE` | Domain/default/range/unit; validate writes and update dependent input/paint | IR, partial delivery/retention | 1 / 0 / 1; full Pending |
| `$CONTROL_PAR_DISABLE_TEXT_SHIFTING` | Corresponding text/label/state/identity metadata; source readback and targeted feedback | Unsupported generic presentation; missing/uncertain semantics | 0 / 0 / 0; full Pending |
| `$CONTROL_PAR_DND_ACCEPT_ARRAY` | Source gesture/modifier/drop/axis context; retain capture and event ordering | Unsupported generic presentation; missing/uncertain semantics | 0 / 0 / 0; full Pending |
| `$CONTROL_PAR_DND_ACCEPT_AUDIO` | Source gesture/modifier/drop/axis context; retain capture and event ordering | Unsupported generic presentation; missing/uncertain semantics | 0 / 0 / 0; full Pending |
| `$CONTROL_PAR_DND_ACCEPT_MIDI` | Source gesture/modifier/drop/axis context; retain capture and event ordering | Unsupported generic presentation; missing/uncertain semantics | 0 / 0 / 0; full Pending |
| `$CONTROL_PAR_DND_BEHAVIOUR` | Source gesture/modifier/drop/axis context; retain capture and event ordering | Unsupported generic presentation; missing/uncertain semantics | 0 / 0 / 0; full Pending |
| `$CONTROL_PAR_FILEPATH` | File/menu/array selection or item state; explicit index/string and source event | Unsupported generic presentation; missing/uncertain semantics | 0 / 0 / 0; full Pending |
| `$CONTROL_PAR_FILE_TYPE` | File/menu/array selection or item state; explicit index/string and source event | IR, partial delivery/retention | 0 / 0 / 0; full Pending |
| `$CONTROL_PAR_FONT_TYPE` | Resource/style/frame identity; worker preparation and changed users only | IR, partial delivery/retention | 0 / 0 / 0; full Pending |
| `$CONTROL_PAR_FONT_TYPE_OFF_HOVER` | Resource/style/frame identity; worker preparation and changed users only | Unsupported generic presentation; missing/uncertain semantics | 0 / 0 / 0; full Pending |
| `$CONTROL_PAR_FONT_TYPE_OFF_PRESSED` | Resource/style/frame identity; worker preparation and changed users only | Unsupported generic presentation; missing/uncertain semantics | 0 / 0 / 0; full Pending |
| `$CONTROL_PAR_FONT_TYPE_ON` | Resource/style/frame identity; worker preparation and changed users only | Unsupported generic presentation; missing/uncertain semantics | 0 / 0 / 0; full Pending |
| `$CONTROL_PAR_FONT_TYPE_ON_HOVER` | Resource/style/frame identity; worker preparation and changed users only | Unsupported generic presentation; missing/uncertain semantics | 0 / 0 / 0; full Pending |
| `$CONTROL_PAR_FONT_TYPE_ON_PRESSED` | Resource/style/frame identity; worker preparation and changed users only | Unsupported generic presentation; missing/uncertain semantics | 0 / 0 / 0; full Pending |
| `$CONTROL_PAR_GRID_HEIGHT` | Position/size/grid/hierarchy/stacking; invalidate affected layout subtree | Unsupported generic presentation; missing/uncertain semantics | 0 / 0 / 0; full Pending |
| `$CONTROL_PAR_GRID_WIDTH` | Position/size/grid/hierarchy/stacking; invalidate affected layout subtree | Unsupported generic presentation; missing/uncertain semantics | 0 / 0 / 0; full Pending |
| `$CONTROL_PAR_GRID_X` | Position/size/grid/hierarchy/stacking; invalidate affected layout subtree | Unsupported generic presentation; missing/uncertain semantics | 0 / 0 / 0; full Pending |
| `$CONTROL_PAR_GRID_Y` | Position/size/grid/hierarchy/stacking; invalidate affected layout subtree | Unsupported generic presentation; missing/uncertain semantics | 0 / 0 / 0; full Pending |
| `$CONTROL_PAR_HEIGHT` | Position/size/grid/hierarchy/stacking; invalidate affected layout subtree | IR, partial delivery/retention | 0 / 0 / 0; full Pending |
| `$CONTROL_PAR_HELP` | Corresponding text/label/state/identity metadata; source readback and targeted feedback | IR, partial delivery/retention | 0 / 0 / 0; full Pending |
| `$CONTROL_PAR_HIDE` | Visibility/part flags; targeted paint/layout without losing source state | IR, partial delivery/retention | 1 / 0 / 1; full Pending |
| `$CONTROL_PAR_IDENTIFIER` | Source identifier query; preserve source/UI-id mapping | Unsupported generic presentation; missing/uncertain semantics | 0 / 0 / 0; full Pending |
| `$CONTROL_PAR_KEY` | Source gesture/modifier/drop/axis context; retain capture and event ordering | Unsupported generic presentation; missing/uncertain semantics | 0 / 0 / 0; full Pending |
| `$CONTROL_PAR_KEY_ALT` | Source gesture/modifier/drop/axis context; retain capture and event ordering | Unsupported generic presentation; missing/uncertain semantics | 0 / 0 / 0; full Pending |
| `$CONTROL_PAR_KEY_CONTROL` | Source gesture/modifier/drop/axis context; retain capture and event ordering | Unsupported generic presentation; missing/uncertain semantics | 1 / 0 / 1; full Pending |
| `$CONTROL_PAR_KEY_SHIFT` | Source gesture/modifier/drop/axis context; retain capture and event ordering | Unsupported generic presentation; missing/uncertain semantics | 0 / 0 / 0; full Pending |
| `$CONTROL_PAR_LABEL` | Corresponding text/label/state/identity metadata; source readback and targeted feedback | IR, partial delivery/retention | 1 / 0 / 1; full Pending |
| `$CONTROL_PAR_MAX_VALUE` | Domain/default/range/unit; validate writes and update dependent input/paint | IR, partial delivery/retention | 1 / 0 / 1; full Pending |
| `$CONTROL_PAR_MIDI_EXPORT_AREA_IDX` | Zone/waveform/view/export metadata; typed service and generation-scoped completion | Unsupported generic presentation; missing/uncertain semantics | 0 / 0 / 0; full Pending |
| `$CONTROL_PAR_MIN_VALUE` | Domain/default/range/unit; validate writes and update dependent input/paint | IR, partial delivery/retention | 1 / 0 / 1; full Pending |
| `$CONTROL_PAR_MOUSE_BEHAVIOUR` | Source gesture/modifier/drop/axis context; retain capture and event ordering | IR, partial delivery/retention | 0 / 0 / 0; full Pending |
| `$CONTROL_PAR_MOUSE_BEHAVIOUR_X` | Source gesture/modifier/drop/axis context; retain capture and event ordering | IR, partial delivery/retention | 0 / 0 / 0; full Pending |
| `$CONTROL_PAR_MOUSE_BEHAVIOUR_Y` | Source gesture/modifier/drop/axis context; retain capture and event ordering | IR, partial delivery/retention | 0 / 0 / 0; full Pending |
| `$CONTROL_PAR_MOUSE_MODE` | Source gesture/modifier/drop/axis context; retain capture and event ordering | IR, partial delivery/retention | 0 / 0 / 0; full Pending |
| `$CONTROL_PAR_NKS_NUM_VALUES` | Corresponding text/label/state/identity metadata; source readback and targeted feedback | Unsupported generic presentation; missing/uncertain semantics | 1 / 0 / 1; full Pending |
| `$CONTROL_PAR_NKS_STR_VALUES` | Corresponding text/label/state/identity metadata; source readback and targeted feedback | Unsupported generic presentation; missing/uncertain semantics | 1 / 0 / 1; full Pending |
| `$CONTROL_PAR_NKS_STYLE` | Corresponding text/label/state/identity metadata; source readback and targeted feedback | Unsupported generic presentation; missing/uncertain semantics | 1 / 0 / 1; full Pending |
| `$CONTROL_PAR_NKS_TYPE` | Corresponding text/label/state/identity metadata; source readback and targeted feedback | Unsupported generic presentation; missing/uncertain semantics | 1 / 0 / 1; full Pending |
| `$CONTROL_PAR_NONE` | Manual extraction/sentinel semantics uncertain; params owner must validate | Unsupported generic presentation; missing/uncertain semantics | 0 / 0 / 0; full Pending |
| `$CONTROL_PAR_NUM_ITEMS` | File/menu/array selection or item state; explicit index/string and source event | Unsupported generic presentation; missing/uncertain semantics | 1 / 0 / 1; full Pending |
| `$CONTROL_PAR_OFF_COLOR` | Corresponding widget colour/opacity; repaint affected users | IR, partial delivery/retention | 0 / 0 / 0; full Pending |
| `$CONTROL_PAR_ON_COLOR` | Corresponding widget colour/opacity; repaint affected users | IR, partial delivery/retention | 0 / 0 / 0; full Pending |
| `$CONTROL_PAR_OVERLOAD_COLOR` | Corresponding widget colour/opacity; repaint affected users | IR, partial delivery/retention | 0 / 0 / 0; full Pending |
| `$CONTROL_PAR_PARALLAX_X` | Corresponding text/label/state/identity metadata; source readback and targeted feedback | IR, partial delivery/retention | 0 / 0 / 0; full Pending |
| `$CONTROL_PAR_PARALLAX_Y` | Corresponding text/label/state/identity metadata; source readback and targeted feedback | IR, partial delivery/retention | 0 / 0 / 0; full Pending |
| `$CONTROL_PAR_PARENT_PANEL` | Position/size/grid/hierarchy/stacking; invalidate affected layout subtree | IR, partial delivery/retention | 0 / 0 / 0; full Pending |
| `$CONTROL_PAR_PEAK_COLOR` | Corresponding widget colour/opacity; repaint affected users | IR, partial delivery/retention | 0 / 0 / 0; full Pending |
| `$CONTROL_PAR_PICTURE` | Resource/style/frame identity; worker preparation and changed users only | IR, partial delivery/retention | 1 / 0 / 1; full Pending |
| `$CONTROL_PAR_PICTURE_STATE` | Resource/style/frame identity; worker preparation and changed users only | IR, partial delivery/retention | 0 / 0 / 0; full Pending |
| `$CONTROL_PAR_POS_X` | Position/size/grid/hierarchy/stacking; invalidate affected layout subtree | IR, partial delivery/retention | 1 / 0 / 1; full Pending |
| `$CONTROL_PAR_POS_Y` | Position/size/grid/hierarchy/stacking; invalidate affected layout subtree | IR, partial delivery/retention | 1 / 0 / 1; full Pending |
| `$CONTROL_PAR_RANGE_MAX` | Domain/default/range/unit; validate writes and update dependent input/paint | Unsupported generic presentation; missing/uncertain semantics | 0 / 0 / 0; full Pending |
| `$CONTROL_PAR_RANGE_MIN` | Domain/default/range/unit; validate writes and update dependent input/paint | Unsupported generic presentation; missing/uncertain semantics | 0 / 0 / 0; full Pending |
| `$CONTROL_PAR_RECEIVE_DRAG_EVENTS` | Source gesture/modifier/drop/axis context; retain capture and event ordering | Unsupported generic presentation; missing/uncertain semantics | 0 / 0 / 0; full Pending |
| `$CONTROL_PAR_SELECTED_ITEM_IDX` | File/menu/array selection or item state; explicit index/string and source event | Unsupported generic presentation; missing/uncertain semantics | 0 / 0 / 0; full Pending |
| `$CONTROL_PAR_SHORT_NAME` | Host identity/eligibility/name; stable parameter map and gestures | IR, partial delivery/retention | 1 / 0 / 1; full Pending |
| `$CONTROL_PAR_SHOW_ARROWS` | Corresponding text/label/state/identity metadata; source readback and targeted feedback | IR, partial delivery/retention | 0 / 0 / 0; full Pending |
| `$CONTROL_PAR_SLICEMARKERS_COLOR` | Corresponding widget colour/opacity; repaint affected users | IR, partial delivery/retention | 0 / 0 / 0; full Pending |
| `$CONTROL_PAR_TEXT` | Corresponding text/label/state/identity metadata; source readback and targeted feedback | IR, partial delivery/retention | 1 / 0 / 1; full Pending |
| `$CONTROL_PAR_TEXTLINE` | Corresponding text/label/state/identity metadata; source readback and targeted feedback | IR, partial delivery/retention | 1 / 0 / 1; full Pending |
| `$CONTROL_PAR_TEXTPOS_Y` | Corresponding text/label/state/identity metadata; source readback and targeted feedback | IR, partial delivery/retention | 0 / 0 / 0; full Pending |
| `$CONTROL_PAR_TEXT_ALIGNMENT` | Corresponding text/label/state/identity metadata; source readback and targeted feedback | IR, partial delivery/retention | 1 / 0 / 1; full Pending |
| `$CONTROL_PAR_TYPE` | Source widget kind query; stable schema, no fabricated setter mutation | Unsupported generic presentation; missing/uncertain semantics | 0 / 0 / 0; full Pending |
| `$CONTROL_PAR_UNIT` | Domain/default/range/unit; validate writes and update dependent input/paint | IR, partial delivery/retention | 0 / 0 / 0; full Pending |
| `$CONTROL_PAR_VALUE` | Typed scalar/indexed/real-array state, source readback; native revisions | IR, partial delivery/retention | 1 / 0 / 1; full Pending |
| `$CONTROL_PAR_VALUEPOS_Y` | Corresponding text/label/state/identity metadata; source readback and targeted feedback | Unsupported generic presentation; missing/uncertain semantics | 0 / 0 / 0; full Pending |
| `$CONTROL_PAR_VERTICAL` | Source gesture/modifier/drop/axis context; retain capture and event ordering | IR, partial delivery/retention | 0 / 0 / 0; full Pending |
| `$CONTROL_PAR_WAVETABLE` | Zone/waveform/view/export metadata; typed service and generation-scoped completion | Unsupported generic presentation; missing/uncertain semantics | 0 / 0 / 0; full Pending |
| `$CONTROL_PAR_WAVETABLE_ALPHA` | Corresponding widget colour/opacity; repaint affected users | Unsupported generic presentation; missing/uncertain semantics | 0 / 0 / 0; full Pending |
| `$CONTROL_PAR_WAVETABLE_COLOR` | Corresponding widget colour/opacity; repaint affected users | Unsupported generic presentation; missing/uncertain semantics | 0 / 0 / 0; full Pending |
| `$CONTROL_PAR_WAVETABLE_END_ALPHA` | Corresponding widget colour/opacity; repaint affected users | Unsupported generic presentation; missing/uncertain semantics | 0 / 0 / 0; full Pending |
| `$CONTROL_PAR_WAVETABLE_END_COLOR` | Corresponding widget colour/opacity; repaint affected users | Unsupported generic presentation; missing/uncertain semantics | 0 / 0 / 0; full Pending |
| `$CONTROL_PAR_WAVE_ALPHA` | Corresponding widget colour/opacity; repaint affected users | Unsupported generic presentation; missing/uncertain semantics | 0 / 0 / 0; full Pending |
| `$CONTROL_PAR_WAVE_COLOR` | Corresponding widget colour/opacity; repaint affected users | IR, partial delivery/retention | 0 / 0 / 0; full Pending |
| `$CONTROL_PAR_WAVE_CURSOR_COLOR` | Corresponding widget colour/opacity; repaint affected users | IR, partial delivery/retention | 0 / 0 / 0; full Pending |
| `$CONTROL_PAR_WAVE_END_` | Truncated extraction artifact; no claimed source semantics | Manifest artifact; uncertain | 0 / 0 / 0; full Pending |
| `$CONTROL_PAR_WAVE_END_ALPHA` | Corresponding widget colour/opacity; repaint affected users | Unsupported generic presentation; missing/uncertain semantics | 0 / 0 / 0; full Pending |
| `$CONTROL_PAR_WAVE_END_COLOR` | Corresponding widget colour/opacity; repaint affected users | Unsupported generic presentation; missing/uncertain semantics | 0 / 0 / 0; full Pending |
| `$CONTROL_PAR_WF_VIS_MODE` | Zone/waveform/view/export metadata; typed service and generation-scoped completion | Unsupported generic presentation; missing/uncertain semantics | 0 / 0 / 0; full Pending |
| `$CONTROL_PAR_WIDTH` | Position/size/grid/hierarchy/stacking; invalidate affected layout subtree | IR, partial delivery/retention | 1 / 0 / 1; full Pending |
| `$CONTROL_PAR_WT_VIS_MODE` | Zone/waveform/view/export metadata; typed service and generation-scoped completion | IR, partial delivery/retention | 0 / 0 / 0; full Pending |
| `$CONTROL_PAR_WT_ZONE` | Zone/waveform/view/export metadata; typed service and generation-scoped completion | Unsupported generic presentation; missing/uncertain semantics | 0 / 0 / 0; full Pending |
| `$CONTROL_PAR_ZERO_LINE_COLOR` | Corresponding widget colour/opacity; repaint affected users | IR, partial delivery/retention | 0 / 0 / 0; full Pending |
| `$CONTROL_PAR_Z_LAYER` | Position/size/grid/hierarchy/stacking; invalidate affected layout subtree | IR, partial delivery/retention | 0 / 0 / 0; full Pending |

All 91 extracted property spellings and the shared snapshot’s four NKS extensions are retained, including the documented false-positive `$CONTROL_PAR_WAVE_END_`; observed extensions remain unverified rather than silently being declared compatible. Property counts cannot distinguish declaration-time use, runtime setter, getter or indirect computed parameter IDs. The comprehensive typed parameter matrix and vendor comparison must settle those cases.

### Reproduction, validation and retained artifacts

Only `src/plugin.rs` / `src/ui/mod.rs` test-module imports, two test modules and this report changed. No production behavior, dependency or library-specific condition changed. Four synthetic tests intentionally demonstrate baseline defects; the ignored real-instrument test is opt-in.

```bash
cd /home/derpcat/.t3/worktrees/KONTAKTO/audit-ui-loop
~/.cache/kontakto-heavy cargo test --no-run
~/.cache/kontakto-heavy env \
  KONTRA_LOOP_PATCH="/mnt/MAIN_STORAGE/Libraries/Kontakt/Conflux 1.1.0 [Native Instruments]/Instruments/Conflux.nki" \
  KONTRA_LOOP_CACHE="$HOME/.cache/kontakto-audit-ui-loop" \
  cargo test --profile ci --lib loop_audit -- --include-ignored --nocapture
# Corpus results: use the shared scanner only; see its README.
# /home/derpcat/.cache/kontra-scan/bin/README.md
```

Cache: `~/.cache/kontakto-audit-ui-loop/` contains sanitized JSON, probe and validation logs, and six screenshots of our renderer. No decrypted source, sample or source image bytes are saved. Screenshot total remains below 50 MB. Validation: optimized real-library run **5/5 passed**. After strengthening the idle-wake witness to assert a known scalar cell actually changed, the final synthetic rerun **4/4 passed** (one real-library test intentionally ignored on that rerun) and final `cargo test --no-run` passed. `git diff --check` passed; only pre-existing compiler warnings remain. Logs: `probes-final.log` and `validation-final.log` in the audit cache. Exact build and corpus receipts below; counts are stable only against these source/binary/manifest revisions.

- `conflux.json` SHA-256 `e9b091dba32d22b8ec2618110e7433a83069d8c25b4852ce7d8833705ad6761d`
- `items.tsv` SHA-256 `8469c259dc38a94c57d3e1db0a631b7e7c8abea0695fbed3e231e9fc214057e4`
- `src/plugin/loop_audit.rs` SHA-256 `02631bd9b3490571cd8f14b4e502f987908bf6600d84ec445a2d9854b0a4204c`
- `src/ui/loop_audit.rs` SHA-256 `333cc23e0777a7aa85bcf54bb7f9d311642105efed1ddde55f00d190350e1293`
