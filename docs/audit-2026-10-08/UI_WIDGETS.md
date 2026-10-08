# Widget and input audit — 2026-10-08

Scope 2. Baseline: `origin/integrate/core-v2@7e82b152b46b31e8b9fd85c2ac01d01dad669ddd`.
Branch: `audit/ui-widgets-20261008`. This branch changes audit probes and this report only.

## Verdict and evidence boundaries

**Worse than v1 for authored widget interaction.** V2 has a native scalar value and
callback service, but its renderer drops authored drag direction/sensitivity,
menu semantics and typed value editing. Several recognized widgets are passive
placeholders. V1 is comparison evidence, not the Kontakt behavioral oracle.

Evidence labels: **code** = traced implementation; **probe** = this branch's
executable result; **usage** = source identifier census, not successful execution;
**unknown** = requires a reference-host or integrated-runtime measurement.
Recognizing a declaration/property is not equivalent to supporting its behavior.
The matrices cover all 16 KSP widget declarations listed by
`crates/sampler-ksp/src/hir.rs:93`, and their input/value obligations. Full parameter
get/set and callback scheduling belong to the params/loop auditors.

Requirements were read from `PRODUCT.md`, `UI.md`, `UI_FRONTENDS.md`,
`CONTROL_STATE.md`, `KSP_SEMANTICS.md`, `KSP_SYMBOLS.md`, `KSP_SURFACE.json`, the
prior `v2/gpt-kontakt-ui` report, and the two RE documents named in the brief.
The RE document named `UI_NATIVE_PRESERVATION.md` concerns window visibility and
native accessibility/IME; it is not a widget compatibility specification.
`FALCON_RUNTIME_UI_GROUNDWORK.md` distinguishes source language/widget contracts;
its UVI findings must not silently become KSP semantics.

## Conflux root cause

Mandatory witness:
`/mnt/MAIN_STORAGE/Libraries/Kontakt/Conflux 1.1.0 [Native Instruments]/Instruments/Conflux.nki`.
SHA-256 `dbe61912507d2ab46404f68a468a5194c039f1e7ca17864af92f2b6f0634d676`
(cross-check against the original UI auditor's fixture manifest).

General drag defects found while tracing the witness are:

1. `crates/sampler-ksp/src/ui.rs:332` emits every `ui_slider` with
   `orientation: Horizontal`.
2. `crates/sampler-ksp/src/ui.rs:448` separately stores `MOUSE_BEHAVIOUR` in
   `Widget.drag`, but maps negative to **Horizontal**, positive to **Vertical**.
   NI documents the opposite signs, and picture-relative sensitivity.
   [NI control parameters](https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/control-parameters#ui_slider-731964).
3. `src/ui/ir_view.rs:247` derives input direction solely from `Kind::Slider.orientation`,
   ignoring `Widget.drag`. It calls `drive` with the fixed `TRAVEL=180` logical
   pixels from `src/ui/theme.rs:32`.
4. `src/ui/ir_view.rs:267` may draw a near-square **slider as a dial**. Its appearance
   then suggests an up/down gesture, while its actual input still uses horizontal
   deltas. `theme.rs:698` ultimately calls MUI's `Ui::drag`, which selects X or Y
   according to that Boolean. A purely vertical drag produces exactly zero for
   a horizontal slider.

`ui_knob` itself selects vertical drag. Do not rename this finding to "all knobs
are unbound" or fix it by treating one library specially. Correct source axis
mapping and consume authored gesture metadata in the shared renderer. Stock
fallbacks should preserve input semantics even when picture decoding fails.

The existing frontend mapping check also asserts the reversed negative-axis result (`crates/sampler-ksp/tests/ui.rs:99`); update that expectation with the systemic fix. A green pre-existing test suite currently protects the wrong mapping.

The scalar handoff exists: `src/ui/part.rs:104` reads shared control values,
`part.rs:110` forwards changed IDs, `src/plugin.rs:881` queues them,
`src/sound/v2.rs:807` rounds/clamps according to the native domain and invokes the
plan's callback. `src/sound/v2.rs:1701` has a callback/readback check. The audit
must distinguish a pointer defect from missing runtime control definitions.

**Native witness measurements (this branch):** 3 compiled script views, 434 total widgets and 390 native scalar definitions. The main view (source slot 2) has 411 widgets: 233 knobs, 1 slider, 16 buttons, 38 switches, 14 menus, 11 value edits, 34 labels, 46 panels, 6 tables, 6 level meters and 6 text edits. Its 78 visible continuous widgets are knobs with no explicit `Widget.drag` override. Across all three views, all 81 visible continuous controls are bound, all 81 native edits were admitted, and all 81 returned the edited value. Therefore the slider-axis defect does **not** explain most main-view Conflux knobs, and missing scalar definitions/admission are ruled out for this initialized witness.

**Proven Conflux capture failure:** **78 visible main-view knobs; 46 capture and change on a 30px vertical drag; 32 never capture; zero change horizontally**. Targets **378–409** all resolve to the same stock 85×52 rectangle at **(0,0)**. Their center (42.5,26) presses **`ir-410`**, the last overlapping knob, every time. Binding exists for every target. `ui.rs:424` defaults absent positions to zero, `ir_view.rs:149` assigns stock size, and `ir_view.rs:184` draws every visible widget in flattened paint order. The pointer engine chooses the topmost named surface. This is a concrete reproduction of immovable controls caused by collapsed geometry and occlusion, not a claim that every authored Conflux dial is immovable.

**Second, independent systemic capture defect:** `ir_view.rs:370` assigns a gesture ID to every widget, including labels/panels. At pinned MUI `822b192`, `crates/mui/src/ui/mod.rs:1180` registers every named surface as a pointer target, regardless of focusability, and later targets win. The owned synthetic overlay fixture proves a label captures `ir-1` and prevents the knob beneath from moving. That fixture proves the passive hit-policy defect; the 32 measured Conflux misses above are blocked by another knob, not a label.

**Independent substep defect:** a 0..2 knob moves by >0.5 in a free 60px gesture but by **0** when each of 30 two-pixel steps receives integer-rounded readback. The renderer adds each frame's delta to a value that `part.rs:104` overwrites from shared state. It has no retained fractional accumulator. Conflux contains real 0..2 and other narrow-range knobs; measure production block scheduling before assigning this as the cause for a particular live-host knob.

The real input sweep uses Vector with no decoded assets; it preserves initialized image-size metadata. It does not exercise audio feedback. The editor-tree check uses the actual asset loader and shared cells; callback execution is a separate native probe. No decrypted source or pictures are written.

White artwork and interface publication resets are other scopes. They can
compound an input failure; axis repair alone does not establish a complete
Conflux fix. Headless gestures are renderer evidence, not a live DAW interaction
or exact Kontakt sensitivity calibration.

**Native callback-block validation:** all **81/81** edits also return the requested value after a 64-frame render block. This rules out an immediate callback reset in this initialized probe, not every delayed/live-host effect.

**Upstream cause confirmed: assumed controls without an authored view.** The selected real NCKP parses **378 controls, zero skipped types**, and its raw hierarchy also has **378 distinct control paths**. The initialized main model adds **33 knobs (378–410)** with **zero properties**, no position and no hide write. None of their names matches a parsed control (case-insensitive exact or suffix), a raw qualified path, a raw leaf ID, or any raw JSON string. The script contains **no `ui_knob` declaration token**. Sixteen tail names have explicit retained “assumed” diagnostics; `compile_with` sorts and truncates diagnostics at 1,000 (`sampler-ksp/src/lib.rs:788`), so the other seventeen missing diagnostics are not a negative result.

The creation path is `sampler-ksp/src/sema.rs:476–507`: after `load_performance_view`, an unresolved `$` name is synthesized as a knob. `model.rs:62–78` gives that assumed control range 0..1,000,000 and **empty properties**. `sema.rs:423` allocates its UI/native scalar identity. The real view's property pass (`eval.rs:631`) only applies properties to described controls, so these names never acquire placement or hiding. `ui.rs:424` then supplies visible zero-position/default sizing, producing the overlapping stock knobs. The owned `missing_performance_description_creates_visible_unsized_knob` test reproduces the same path without library data.

**Owner: W2 (KSP widget/view lowering).** Genuinely undescribed fallbacks must remain hidden/unplaced and diagnostically explicit; they must not become stacked clickable knobs. Preserve legitimate source values/callback identities. For this selected NCKP, the 33 names are absent rather than authored geometry dropped by publication or a case/path lookup. The collapse is already present in `load_read` before any shared editor publication, so **W1 publication is not the cause of this initial failure**. W8's removal of two empty-view init runs addresses a separate load/order problem; its candidate should still prove the real-view model has no visible phantom tail. A different dynamically selected view or Kontakt-generated alias remains a reference-host question; this probe does not claim every resource/version is equivalent.

**Editor-tree validation:** target 18 captures and changes the shared value by **166,666.67** on a 30px vertical gesture in **both Original and Vector**. The probe uses the real asset loader and waits 30 idle frames between gestures to avoid double-click reset. The four audit input checks pass in **83.04s**; the existing native UI callback regression also passes. These establish baseline headless editor behavior, not native DAW parity.

## Exhaustive widget matrix

Counts below are census usage counts to be filled from the complete installed
manifest. A count of zero is corpus absence, never proof of implementation.

| Widget | Expected value/input contract | V2 status and evidence at baseline | Root cause and systemic fix | Items using it |
|---|---|---|---|---|
| `ui_knob` | Integer range; up/down relative drag; fine adjustment and source default | **Partial**. Vertical drag, wheel, arrows, reset in `ir_view.rs:246` / `theme.rs:704`; declaration ratio affects display | Fixed 180px travel; range step ignored; rounded-feedback fixture proves lost fractional accumulation. Consume gesture policy and retain grab accumulator | pending |
| `ui_slider` | Integer range; authored direction/sensitivity; skinned dial still preserves intended gesture | **Wrong**. Always horizontal `ui.rs:332`; signed metadata reversed `ui.rs:448`; renderer ignores it `ir_view.rs:247` | Preserve axis/picture-relative sensitivity and use it for both presentations | pending |
| `ui_button` | Integer 0/1; click changes value and dispatches its handler; distinct from an automatable switch | **Partial**. Scalar binding and activation toggle `ir_view.rs:291`; KSP emits nonmomentary button `ui.rs:336` | Basic toggle exists; picture hover/press states and source modifier payload do not. Preserve those states without inventing momentary KSP behavior | pending |
| `ui_switch` | 0/1 toggle with host automation identity | **Partial**. Same activation branch `ir_view.rs:291`, automation metadata `ui.rs:502` | Metadata is not a host automation/gesture bridge. Bind source automation plus modifier and picture states | pending |
| `ui_menu` | Select visible entry by its semantic integer value; drawing does not edit state | **Wrong**. `ir_view.rs:311` coerces unmatched value to first visible entry even while idle; click cycles entries | Reuse shared popup; preserve unknown current values; select exact semantic value once | pending |
| `ui_table` | Integer array, declared bipolar range/visible steps; pointer edits report the column index | **Missing interaction / wrong display**. IR retains cells `ui.rs:354`; renderer draws identical 1px bars `ir_view.rs:348` | Scalar `Values` cannot submit indexed edits; draw cells and add typed indexed transactions/callback index | pending |
| `ui_xy` | Real array of cursor coordinates; independent axes/mode, cursor selection and reset | **Missing**. IR cursor/sensitivity metadata `ui.rs:383`; placeholder `ir_view.rs:357`; `Binding::Variable` | Typed cursor values/events are absent from view and edit service; implement them without scalar encoding | pending |
| `ui_waveform` | Attached zone/sample view, cursor/slices/selection; enabled MIDI drag export | **Missing**. Placeholder `ir_view.rs:357` | No owned waveform data, selection or export input path. Add async peaks and versioned source edits, use existing asset lifetime | pending |
| `ui_wavetable` | Attached zone and position/mode visualization; source-prescribed interaction | **Missing**. Retained mode/parallax `ui.rs:391`; placeholder `ir_view.rs:357` | No renderer consumes data/properties. Bind zone/position before adding interactive behavior; exact gestures need reference | pending |
| `ui_file_selector` | Navigate/filter allowed base directory; selected path and callback | **Missing**. Base/type/column metadata `ui.rs:404`; placeholder `ir_view.rs:357` | No picker/navigation/selected-path bridge. Reuse native picker with source epoch and directory validation | pending |
| `ui_level_meter` | Read-only levels from attached output/bus; source orientation/range | **Wrong**. Binding constructed `ui.rs:585`, renderer `ir_view.rs:347` returns `[0.;2]` | Wire existing live meter service to the binding. No click/drag editing should be invented | pending |
| `ui_value_edit` | Integer value; typed entry, optional arrows and source units | **Partial**. Drag/wheel/reset only `ir_view.rs:327`; arrow metadata `ui.rs:351` ignored | Reuse native text input and bounded numeric parsing; honor arrows. Double-click should enter typing, not reset | pending |
| `ui_label` | Text/lines; overflow scrolling and configured MIDI export drag | **Partial display / missing scrolling and DnD**. Passive caption `ir_view.rs:346`; no scrolling/export handlers | Implement clipped multiline content and innermost wheel ownership; bind configured MIDI export area | pending |
| `ui_text_edit` | String value with focus, selection, keyboard/IME edit and callback | **Missing**. Passive placeholder `ir_view.rs:357` | No typed string edit/readback service; reuse MUI text/IME component rather than numeric `f64` | pending |
| `ui_panel` | Noneditable container; parent-relative geometry and inherited visibility | **Partial**. `page_rect`/`visible` `sampler-ui-ir/src/lib.rs:552`, `:567`; flat draw `ir_view.rs:184` | Container inheritance exists; pointer clipping/stacking needs overlapping scene probes. Treat panel as presentation container, not scalar control | pending |
| `ui_mouse_area` | Typed drag/drop target with file filtering and enter/leave/drop callbacks | **Missing**. Passive block `ir_view.rs:360`; no event payload/input registration | Add typed DnD events and source callback context; use shared native desktop drop service | pending |

The source compiler allocates native scalar controls only for button, knob,
menu, value edit, slider and switch (`crates/sampler-ksp/src/hir.rs:143`). Other
widget values use `Binding::Variable` (`ui.rs:470`). The renderer resolves only
`Binding::Control` and a `HashMap<ControlId,f64>` (`ir_view.rs:219`, `:73`). Thus
adding a pointer listener alone cannot make arrays, strings, file paths or XY
functional. The already implemented scalar service should stay scalar; add
explicit source-typed edit payloads for these obligations.

The matrix's brief expected contracts use the repo's surface/spec inventory and
[NI's widget catalog](https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/user-interface-widgets).
Exact gestures on waveform/wavetable controls remain unknown where the public
contract does not specify them. Do not equate a picture with an editable value.

## Exhaustive shared input/value matrix

| Obligation | Status / evidence | Systemic implementation and proof |
|---|---|---|
| Vertical knob drag | **Present**, `ir_view.rs:247`; slider-dials differ | Separate authored behavior from appearance; compare pure X/Y trajectories |
| Signed slider axis | **Wrong**, `ui.rs:448`, `ir_view.rs:247` | Negative vertical / positive horizontal; negative fixture, positive fixture and zero/default reference |
| Authored sensitivity | **Missing consumption**, `Widget.drag.sensitivity` unused by renderer | Picture-relative travel; test two speeds with equal geometry and two scales |
| Fine adjustment | **Partial**, Shift used in `theme.rs:690` and MUI drag | Verify sustained Shift drags after integer audio readback and modifier changes mid-grab |
| Integer quantization / `Range.step` | **Wrong accumulation under readback**, owned rounded-feedback probe loses all 30 substep edits; `v2.rs:812` rounds on audio | Keep pointer accumulator independent of acknowledged integer value; tiny repeated moves must eventually cross one step |
| Range limits and defaults | **Partial**, domain rounding/clamp `v2.rs:812`, IR default `ui.rs:315` | Avoid using declaration minimum as restored current value; reference dynamic range writes against immutable native domains |
| Ctrl/Cmd click default | **Present for continuous widgets**, `theme.rs:727` | Validate actual declared default and native units; source modifier reporting remains separate |
| Double-click | **Partial / wrong for value edit**, `theme.rs:727`, `ir_view.rs:328` | Keep product reset gesture for knobs; value edit enters typed editor; test accept/cancel |
| Wheel over scalar controls | **Present with limits**, `theme.rs:716` and `wheel_taken` | Step is span/50 or span/500 and uses delta sign only; document vs reference accelerated/pixel wheel behavior |
| Wheel over nested panels/labels | **Incomplete ownership**, `rack.rs:168` excludes only `wheel_taken()` from scalar `drive` | Route to innermost consumer first; parent custom scrolling must respect native child capture and boundaries |
| Keyboard arrows | **Partial**, `stepped` invoked `theme.rs:726`; control focusable | Step/range/source-unit tests; keyboard menu and typed widgets need actual components |
| Tab/focus and accessible values | **Partial**, scalar controls focusable/A11y Slider/Toggle | Typed controls currently have no usable semantics. Test focus clipping, keyboard activation and source names |
| Text input / IME | **Missing authored widget hookup** | Reuse existing native accessibility/IME bridge; keep string state source-owned |
| Source Shift/Alt/Ctrl parameters | **Missing event transport**, `plugin.rs:881` queues only `(slot,id,value)` | Carry modifier snapshot into callback context; renderer fine/reset gestures do not supply KSP getters |
| Mouse-area drag/drop types | **Missing**, no event registration in `ir_view.rs:360` | Carry file type, enter/leave/drop and multiple-item identity; validate filters/base paths |
| Label MIDI export / waveform MIDI drag | **Missing**, no desktop drag initiation in renderer | Use declared MIDI object/area and source epoch; prove exact exported event metadata |
| Typed/indexed widget state | **Missing renderer edit path**, `Values=f64`, `hir.rs:143` | String, arrays and XY remain typed; callbacks receive edited indices/cursor IDs |
| Passive drawing must not mutate values | **Wrong for menu**, `ir_view.rs:315` | Idle nonmatching semantic menu value must remain unchanged and emit no callback |
| Cross-part/control identity | **Wrong renderer identity at baseline**, `ir_view.rs:215`, `:193` | IDs are `ir-N` / `ir-view` across every part. Namespace by part/script generation; drag one of two identical faces |
| Scalar callback delivery | **Partial but implemented**, `v2.rs:807`, existing test `:1701` | Admission succeeds before callback; queue/generation/rollback belong to loop audit |
| `on ui_controls` / `on ui_update` | **Missing language/event surface**, callback enum `hir.rs:259` lacks both | Implement source dispatch semantics; rendering frequency must not fabricate UI callbacks |
| Button press/release/hover pictures | **Partial**, `switch_frame` `ir_view.rs:35` uses only off/on | Derive visual state separately from semantic toggle; no extra value writes on hover |
| Hide/parent/z hit semantics | **Wrong passive-hit policy / collapsed geometry**, all widgets named `ir_view.rs:370`; real 32 misses and synthetic label blocker | Keep source stacking; decorative widgets must pass through unless they own an input obligation; preserve placement/hide and test topmost target |
| Pointer capture outside editor / cancel | **Inherited MUI support; native-host parity unknown** | Test leave/release/cancel/reopen on Linux and host embedding without stuck grabs |
| Scale invariance | **Wrong sensitivity**: geometry scales in `ir_view.rs:185`, travel stays 180 | Scale authored pixel travel once; compare 0.5/1/2 view scales independently of HiDPI |

NI specifies that programmatic `CONTROL_PAR_VALUE` changes do not call the UI
handler, and modifier-default reset applies to knobs/sliders. It also defines
source modifier getters, typed DnD and table edited-index context.
[NI values, modifiers and widget-specific parameters](https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/control-parameters).
The exact sensitivity law and wheel conventions require a Kontakt reference
trace; the sign error does not.

For the reported label/panel case, the first cause is simpler: authored labels render as a passive single caption and tables as a canvas, with **no inner scrolling component or wheel handler**. Their wheel reaches the rack because the child never consumes it. `ui_panel` supplies containment metadata, not a scrollable view. Fix the missing child behavior before interpreting every rack scroll as an arbitration failure.

The custom rack wheel path is evaluated during tree construction, before MUI
settles native scroll nodes. At pinned MUI `822b192`, `Ui::wheel` returns the raw
response and registers a claim (`crates/mui/src/ui/scroll.rs:144`);
`land_wheel` arbitrates native nodes afterward (`:277`). A child native scroller can therefore consume its own wheel while the custom rack has already changed its offset. This is an additional code-level ordering risk where such a child exists; a real nested rack fixture should quantify it. The baseline authored label has no child scroller to arbitrate.

## Whole-corpus measurement

Manifest: `~/.cache/kontakto-corpus/items.tsv`: **781 NKI + 53 NKM = 834 Kontakt
files**, alongside 660 UVI programs outside this scope. NKM usage unions all
contained programs once per file. The directory was verified mounted.

`tools/widget-audit.py` runs container/script metadata reads in separate <=240-second
`kontakto-heavy` shards, with <=20 seconds per item and resumable hash-keyed JSON
in `~/.cache/kontakto-audit-ui-widgets/corpus-raw/`. It excludes strings/comments and
stores only identifier-presence counts, item size/mtime identity, timing/status and error hashes. Decrypted
scripts, samples, resource images and saved strings are never written. Reader
errors/timeouts are reported as unknown coverage, not as zero use.

**Results: pending final shard.** The raw decoder reuses `read_chunks`, `Program`/`Bank` and `BParScript::params`, matching active inline-script selection in `library.rs:213` while avoiding the reader's unrelated two KSP init executions. The abandoned translated-reader attempt had timeouts; its counts are not used below. Initialized NCKP-only controls and dynamically generated
resource behavior are not captured by source token presence. The census auditor's
initialized/rendered measurements should complement these usage denominators.
Do not quote the older 2,741-instrument source census as this manifest's size.

## Ranked systemic fixes and proving tests

Effort S: localized component/adapter change; M: several typed services/components;
L: source execution/async feature family. Reach is eligible measured usage, not a
promise that all counted instruments become fully compatible. Mechanisms overlap.

| Rank | Severity / mechanism | Concrete files/approach | Effort | Reach and test that proves it |
|---|---|---|---|---|
| 1 | P0 visible phantom controls and passive surfaces steal capture | `sema.rs`/`model.rs`/`ui.rs`: keep undescribed assumed controls hidden/unplaced; `ir_view.rs`: deliberate passive hit-through policy | S/M | Conflux has33 phantom tail knobs; no origin ghost is visible/hittable after fix. Owned missing-view and label-overlay fixtures invert; preserve all378 authored NCKP controls and valid callbacks |
| 2 | P0 authored slider gestures lost | `sampler-ksp/src/ui.rs`, `ui/ir_view.rs`; correct sign, consume direction/speed, preserve fallback geometry policy | S | Slider-use items; synthetic negative/positive trajectories change intended axis only |
| 3 | P0 duplicate widget IDs across parts | `ui/part.rs`, `ui/ir_view.rs`; existing MUI namespaces keyed by part/script identity | S | Any two simultaneous faces; same widget ordinal edits only hovered part, focus/wheel/capture remain isolated |
| 4 | P0 render-time menu writes | Reuse popup work on prior branch; retain semantic value while idle | S | Menu-use items; nonconsecutive values, hidden entries, unmatched value and passive 100-frame run |
| 5 | P0 indexed/string values have no edit route | Explicit typed source edits/readback; keep native numeric service and no lossy f64 encoding | M | Union of table/XY/text-edit/file-selector; nonzero bipolar cells/visible steps render correctly, source callbacks report values/index and readback |
| 6 | P1 value edit cannot type | Existing MUI text entry with integer validation and arrows | S | Value-edit items; enter commits clamped parsed value, Esc cancels; double-click does not reset |
| 7 | P1 nested wheel leaks to rack | One innermost input owner shared by custom/native scroll consumers | S/M | Nested-scroll usage unmeasured; overflowing child + overflowing parent, both wheel directions and boundaries |
| 8 | P1 small/fine moves erased by readback; fixed travel | Retained grab accumulator, source speed and acknowledged values separated | S/M | Continuous controls; 100 substep Shift drags with audio readback accumulate; external edits reconcile |
| 9 | P1 attached meters stay silent | Resolve `Binding::Meter` to existing live peak owner and orientation/range | S/M | Meter-use items; known stereo signal makes two authored meters show routed independent levels |
| 10 | P1 file/drop/waveform families are placeholders | Existing file picker, native DnD, owned sample peaks and versioned async transactions | M/L | Corresponding use unions; stale completion cannot mutate replacement; exact path/file types/MIDI payload |

Further acceptance gates: every gesture tested in Original/Vector, view zoom and
HiDPI; initialized current/default/range distinguished; every typed edit emits
source callback once; arbitrary source property updates affect the next accepted
scene without dropping pointer capture. Headless support, reference semantic
support and native DAW interaction are three separately reported gates.

## Prior work to merge before writing replacements

`v2/gpt-kontakt-ui@cda4ce23` (latest observed local branch; remote observed at `0be9ed3f`) already contains popup
selection, passive-menu preservation, UI namespacing, scalar revision/readback
and queue/generation admission work; `47f14b59` records scalar recall/passive
rendering, `cda4ce23` checkpoints header/popup work. Its `UI_V1_PARITY.md` explicitly
leaves drag sensitivity, typed entry, arrays, waveform/file selector and mouse
area/XY coverage incomplete. **It retains the same hardcoded slider orientation
and ignored drag metadata in `ir_view.rs:247`**, so merging it does not solve the
axis defect. Check its current commit before merging; the branch was advancing
during this audit. No branch was merged into integration here.

V1 `0cb7a8a0:src/ui/perf_view.rs:133` selects negative slider mouse behavior as
vertical, considers shape/knob-like controls, and `:147` computes travel.
`:557` implements value typing, `:966` opens source file selectors and `:980`
opens semantic menus. These are existing requirements/patterns to reuse, not
evidence that every v1 widget was complete. V1 also had incomplete authored
text-edit, mouse-area and XY paths according to the prior parity audit.

## Other scopes

- **Render:** missing resource images, unavailable Komplete UI, picture frame/size
  decoding, wallpaper blankness, z/alpha/font fidelity. Incorrect image dimensions
  can also change hit rectangles; preserve this dependency in widget fixtures.
- **Params:** dynamic min/max/default/property writes must update both frontend
  state and native control domain coherently. Modifier/index/context getters need
  an event payload. Persistence must separate current value from reset default.
- **Loop:** `part.rs:104` refreshes values every frame; integer roundtrip can erase
  a fractional grab. `plugin.rs:892` republishes interface Arcs and `part.rs:54`
  rebuilds local face state; verify selection/capture survival and latency.
- **Census:** source counts miss pure NCKP/Komplete resources; render failure and
  source use are different denominators. Correlate via item path/hash.
- **UVI:** the shared renderer's placeholders also affect UVI IR, but source
  Lua widgets, callbacks and automation require that frontend's own audit.

## Unknowns and how to measure them

1. Native Kontakt sensitivity law, zero `MOUSE_BEHAVIOUR`, fine modifiers, wheel
   step and picture-relative scaling: same NKI/version, controlled pixel trajectories,
   log only resulting values and geometry; compare against this branch's probe.
2. Reference-host handling of undescribed controls/aliases and dynamic resource choice: compare this exact selected NCKP/version, source tab changes and Kontakt runtime. The baseline fallback creation, empty model and absence from the selected raw view are proven; whether another host synthesizes aliases is unmeasured.
3. Tiny steps under production audio readback: feed Shift moves below one integer
   step, process host blocks between moves, inspect final raw value and acknowledgements.
4. Complete typed/source callback family: table/XY/string/path fixtures with
   programmatic writes and UI writes; callback order belongs to the loop owner.
5. Nested wheel boundaries: scene with long multiline label/table and long rack;
   assert only intended owner changes unless source explicitly permits bubbling.
6. Native-host cancellation and accessibility: pointer leaves/release, hidden UI,
   lost focus, reparent/reopen; keyboard/IME and semantic actions in both presentations.
7. Actual instruments unlocked: rerun initialized/rendered corpus after each
   mechanism fix; source use is an eligible upper bound, not an unlock count.

## Reproduction and validation

```sh
~/.cache/kontakto-heavy cargo build -p sampler-kontakt --example widget_audit
~/.cache/kontakto-heavy cargo test -p sampler-kontakt --example widget_audit
~/.cache/kontakto-heavy cargo test --no-run --lib
~/.cache/kontakto-heavy cargo test --lib audit_widget_negative_mouse_behaviour_baseline -- --nocapture --test-threads=1
KONTRA_AUDIT_WIDGET_PATCH='/path/to/Conflux.nki' ~/.cache/kontakto-heavy cargo test --lib audit_widget_real_input -- --ignored --nocapture --test-threads=1
python3 tools/widget-audit.py /path/to/widget_audit ~/.cache/kontakto-audit-ui-widgets/corpus-raw
```

The negative-axis check intentionally records the broken baseline. After the
systemic fix, invert its axis/value expectations to become the regression gate.
Gesture deltas start at the declared midpoint, avoiding clamp-boundary false negatives; initial saved-value correctness is a separate obligation. No product behavior is changed by these tests.

Checkpoint validation: native example build and lexer check passed; root `cargo test --locked --no-run --lib` passed after the final editor-probe edit. The final gesture run passed all four audit checks: negative axis, passive overlay, rounded substeps and real Conflux sweep/editor tree (83.04s). The existing native callback regression passed (0.06s). Both native example checks passed, including undescribed-view fallback. `python3 tools/widget-audit.py --self-check` passed. The upstream metadata/raw-resource result above is complete. Whole-corpus totals remain pending in this follow-up; native DAW/reference-host parity remains unmeasured.
