# UI audit — 2026-10-08

Audit baseline: `integrate/core-v2@7e82b152`. Reference: **v1 `0cb7a8a0`**, not a current v1 branch. Research only: no production fixes. The only source changes are opt-in frame probes and a small reproduction check in `src/ui/tests.rs`.

## Verdict

**Worse in functional parity.** The authored interface is present, but view selection, control identity, gestures, recall, snapshots, sound editing and several widget types regress. Conflux is **renderable, not fully usable**. A fast fallback or a successful PNG does not establish parity.

The full feature inventory below uses **present** for a retained implementation, **worse** for degraded behavior, and **missing** for absent functionality. Present is not a native-host behavioral certification. The v1 source was inspected at the pinned commit across browser, rack, header, mixer, keyboard/computer, performance view, generated panel, sound visualization, menus and logs. `UI_V1_PARITY.md` on `v2/gpt-kontakt-ui` was read first; its statuses describe that agent's branch, not this integration baseline. In particular, its “editable effect-chain” claim is corrected here: v1 `src/ui/chain.rs:1` explicitly describes a read-only display.

## Measurements

All times below are milliseconds. `v1 mean` for Vista/Areia is the sum of the five measured stage means (an exact whole-frame mean for those consecutive stages); their original harness does not retain total median/p99. No invented aggregate percentile is reported. Full raw stage statistics and encrypted-file SHA-256 fingerprints are in [ui-frames.json](ui-frames.json).

| Real instrument, initial page / mode | v1 mean | v2 mean | v1 total median / p99 | v2 total median / p99 | Qualification |
|---|---:|---:|---:|---:|---|
| Conflux / Original | 6.333 | 36.430 | 6.193 / 9.289 | 35.994 / 53.436 | v1 content visibly degraded; not a same-quality comparison |
| Conflux / Vector | 5.724 | 31.918 | 5.583 / 7.069 | 31.423 / 47.826 | v1 Original/Vector captures identical; shared-load timing |
| Vista — 3 Cellos / Original | 4.943 | 5.152 | unavailable | 5.103 / 6.074 | 48 controls/widgets in each; approximately +4.2% mean, provisional |
| Areia — 6 Celli Core Techniques / Original | 6.234 | 5.967 | unavailable | 5.959 / 6.208 | 847 controls/widgets; approximately −4.3% mean, reduced rendering fidelity |
| Vista / Vector | not run in v1 harness | 9.364 | unavailable | 9.601 / 12.773 | v2 measurement only; no parity ratio |
| Areia / Vector | not run in v1 harness | 6.920 | unavailable | 5.900 / 9.793 | v2 measurement only; no parity ratio |

| Original warm stage mean | Conflux v1 / v2 | Vista v1 / v2 | Areia v1 / v2 |
|---|---:|---:|---:|
| build | 0.231 / 3.169 | 0.175 / 0.221 | 0.214 / 0.264 |
| layout | 0.934 / 7.676 | 0.544 / 0.527 | 0.720 / 0.485 |
| scene clone | 0.081 / 0.134 | 0.042 / 0.037 | 0.061 / 0.036 |
| CPU paint | 1.592 / 9.821 | 0.996 / 1.035 | 1.260 / 0.925 |
| CPU raster | 3.496 / 15.630 | 3.186 / 3.333 | 3.979 / 4.257 |

The measured Vista/Areia frame differences are small and do not establish a general v2 rendering-speed regression. Conflux v2's initial-page CPU workload is substantially larger, but the reference capture fails visual/interactive equivalence. Do not turn the roughly 5.75× mean ratio into a same-quality product claim. NKI parse/setup measurements are retained in logs but are intentionally not presented as library-load-speed comparisons: OS caches, frontend setup and no-PCM preparation differ.


### Method and limits

- AMD Ryzen 7 7800X3D, 8 cores / 16 threads; Linux; libraries on mounted `/mnt/MAIN_STORAGE`. Both builds use the optimized `ci` profile (release inheritance, LTO disabled).
- Full editor at 1180×900 logical/device pixels, plain appearance, initial authored page, Original presentation, CPU Vello renderer. Eight warm frames, 24 sampled frames. Build, layout, scene clone, CPU paint and raster measured separately. The sample maximum is the reported p99; 24 samples cannot estimate a stable population p99.
- v1 Vista/Areia use the existing pinned-tree `ui::audit::tests::real_instrument_frame_benchmark` and its prebuilt test executable at `.../cargo-target/kontakto-inv-load-speed-v1/ci/deps/kontakto-e2dc3eef5dfc58fd`. The read-only `inv-load-speed-v1` and `gpt-kontakt-ui-v1-bench` trees both point to `0cb7a8a0`; the benchmark is present in that commit. Those two binaries are prior builds. Conflux uses a fresh rebuild of pinned v1 in this audit's own `audit-ui-v1-probe` worktree, plus the 56-line idle-only test in [v1-frame-probe.patch](v1-frame-probe.patch). The original benchmark refused Conflux with `no_animated_knob_or_slider`; this is a skipped measurement, not zero frame time.
- v2 uses the new `audit_ui_real_frames` probe against the baseline editor and translator. It reads the real NKI and prepares scripts with group names/saved state retained, but clears zones/assets before lowering to avoid audio PCM loads. The editor receives the original mapping metadata. This isolates UI rendering; it does **not** benchmark audio or scripts running concurrently. No decrypted source, resources or PCM are written to disk.
- The pinned v1 Conflux capture is visibly degraded: 378 declared controls, 118 visible controls, **zero continuous controls**, two pictures, no wallpaper. Its Original and Vector screenshots are identical (SHA-256 checked). Therefore the numerical Conflux comparison is **unequal visual content** and cannot establish same-quality speed superiority; it also does not reproduce the user's earlier working Conflux UI. Capture location: `/home/derpcat/.cache/kontakto-audit-ui/shots/Conflux-v1-{1,3}.png`.
- v1 Vista/Areia also report changing-scalar frames; the matched comparison uses idle frames. v2 probe has no running audio engine/control mirror. It is unsuitable for claiming callback responsiveness, MIDI-to-screen latency, script-state fidelity or total load-speed parity. First compilation/asset decode is excluded from warm frame samples.
- Shared-agent contention is substantial (observed load average 16.97 on 16 logical CPUs). Treat p99 and ratios as provisional; repeat interleaved trials on an otherwise idle machine before an acceptance target. These are CPU software-render times, **not GPU/DAW FPS**.
- Logs are under `/home/derpcat/.cache/kontakto-audit-ui/`. Reproduce with `kontakto-heavy cargo test --profile ci --lib audit_ui_real_frames -- --ignored --nocapture --test-threads=1`, setting `KONTRA_AUDIT_UI_PATCH`. Optional `KONTRA_AUDIT_UI_SHOTS` writes rendered editor screenshots only.

## Ranked findings and concrete fixes

### 1. P0 — Vector is the default and publication overwrites Original selection (S)

**Click → state → derivation → persistence:** `src/ui/part.rs:87` creates the two latches. A click writes only `Face.presentation` at line 95. Each frame clones `PartView.interfaces` and compares **Arc identity** at lines 51–55. Any changed interface recreates `Face` at line 60, choosing Vector when `unsupported` is empty, Bitmap otherwise (line 59), rather than using the player's choice. `src/plugin.rs:892–908` republishes a new Arc when script effects modify the UI; `src/sound/mod.rs:256` regenerates all interfaces. Thus a listener/text/visibility update can immediately undo Original. This is contingent on publication; clicking alone does not always revert.

`src/plugin.rs:37–80` has no per-part `view` field; the global `Settings.view_mode` does exist (`src/library.rs:62`, settings buttons `src/ui/header.rs:271`), but `part::interface` never reads it. The cached face is not serialized in plugin state or multis. A reload/reopen also loses presentation and can reset the selected script. The IR's `Presentation` describes rendering, not a persisted user preference (`crates/sampler-ui-ir/src/lib.rs:509`).

**Required default:** the user specifies Original as the default, like the earlier working v1 installation. The unsupported-empty → Vector branch at `src/ui/part.rs:59` violates that requirement even before the first click. Treat vector-by-default as P0; a semi-vectorized fallback is not original-UI success.

**Fix:** default to Original and reuse v1's saved codes 0=default, 1=Original, 2=generated KONTRA, 3=Vectorized in `src/plugin.rs`, `src/ui/part.rs`, `src/ui/rack.rs`. Derive presentation every frame from saved preference; distinguish actual part generation/path replacement from an updated interface; retain selected script and loaded assets when possible. Keep old code 2 semantically distinct even if its renderer is still missing. Local prior patch `cda4ce23` already covers most of this; validate it, don't rewrite it.

### 2. P0 — “Knobs” authored as sliders drag horizontally; sensitivity is discarded (M)

True `Kind::Knob` already goes vertically (`src/ui/ir_view.rs:247`); shared `theme::drive` already handles Shift fine adjustment, arrow/wheel steps, double-click and Ctrl/Cmd press reset (`src/ui/theme.rs:701–729`). Replacing that shared helper would miss the cause.

KSP `ui_slider` is always translated as **Horizontal** (`crates/sampler-ksp/src/ui.rs:332–335`). Kontakt libraries often use a slider declaration with a round knob sprite. `widget` uses only the kind's orientation for interaction and fixed `TRAVEL` for every range (`src/ui/ir_view.rs:246–248`), even when it draws a knob face at line 272. `Widget.drag` retains mouse behavior (`crates/sampler-ui-ir/src/lib.rs:209–215,130`) but is ignored. The translator additionally reverses the sign convention relative to the pinned v1 reference: negative behavior becomes Horizontal (`crates/sampler-ksp/src/ui.rs:448–455`), whereas v1 `perf_view.rs:133` uses negative=vertical for actual faders. The IR comment “larger is finer” also conflicts with v1's inverse travel law (`100000 / abs(behavior)`, clamped 60–600).

**Fix:** `crates/sampler-ksp/src/ui.rs`, `crates/sampler-ui-ir/src/lib.rs` documentation, `src/ui/ir_view.rs`: honor behavior, recognize knob-like slider art/geometry once, use vertical drag for knobs, and reuse v1's verified axis/travel policy, scaled with the drawn size. Keep the existing shared fine/reset helper. Verify a real Conflux knob-like slider, a vertical fader, a horizontal fader, Shift mid-drag, and closed physical drag at fractional scale. Check native behavior before declaring the sign/magnitude law universal for non-KSP sources.

### 3. P0 — UI IDs alias across rack parts and scripts (S; merge)

Every control is `ir-{index}`, every page root is `ir-view` (`src/ui/ir_view.rs:193,216`), independent of part/script. Two visible instruments therefore share focus/capture/wheel/animation IDs even though the **binding** has a stable `ControlId`. This is an additional cause of wrong knob interaction; no drag-model patch can isolate two parts with identical indices.

**Fix:** pass a namespace containing part and script identity from `src/ui/part.rs` to `ir_view::view`, including menus and root IDs. Already repaired on pushed `v2/gpt-kontakt-ui@47f14b59`; merge/adapt its focused changes.

### 4. P0 — menus mutate script values during passive painting; direct selection missing (S/M; merge then finish)

`src/ui/ir_view.rs:309–312` substitutes the first visible item's value when the current value is not in the displayed list, even without user input. `part::interface` forwards any value difference to `Shared::set_control` (`src/ui/part.rs:113–119`), then the audio thread calls core `set_control` (`src/plugin.rs:1586`). Merely opening the editor can run a callback/change the instrument. Clicking only cycles entries (`ir_view.rs:313–317`), unlike v1's popup with semantic values and visible/enabled handling.

**Fix:** merge `47f14b59` for passive-value preservation, then validate the popup work in local `cda4ce23`. Preserve saved menu **position** vs host semantic **value** as separate restore inputs; do not persist displayed item index as its script value.

### 5. P1 — wheel also moves the rack under a nested list (S)

This is **MUI, not egui**. Articulations have `.scroll().id("arts-{slot}")` (`src/ui/inside.rs:243`); mapping group list also uses `.scroll()` (`inside.rs:336`, without an explicit stable ID). The rack implements its own sticky-header scrolling. After building children it calls `ui.wheel("rack-view")` and increments `state.rack_y` (`src/ui/rack.rs:167–175`); only knobs set the separate `WHEELED` flag via `theme::drive` (`theme.rs:716–717`). Nested `.scroll()` nodes never set it.

MUI at the locked dependency `822b1922`, `crates/mui/src/ui/scroll.rs:144` explicitly says a parent wheel reader still reads the wheel even when an inner scroll node handles it. `land_wheel` scans innermost surfaces and moves them first (`scroll.rs:277–320`). Consequently custom rack scrolling can move **in addition to** the inner scroll; this is not evidence that MUI sends nothing to the child. The cause is two independent consumers, not a generic missing `sense` setting.

**Fix:** `src/ui/rack.rs`, `src/ui/inside.rs`, shared scroll helper in `src/ui/theme.rs` only if needed: suppress manual parent movement while a descendant can consume that wheel axis; keep boundary handoff when the child reaches its end. Give each nested list a part-specific stable ID. Preserve sticky headers and knob wheel capture. Check pointer over inner body, scrollbar, empty padding, and boundary scrolling; assert both child offset and rack position.

**Measured reproduction:** `audit_ui_toggle_and_nested_wheel` passes against the unchanged baseline. Original is selected after activation and becomes unselected after publishing an equivalent newly allocated interface. With 40 synthetic articulations and one +80px wheel event over row 4, the inner `arts-0` offset moves **0→80px** and the rack content top moves **74→−6px**. Both containers move by 80px. This establishes double consumption; it disproves the narrower assumption that the child receives no wheel at all. The test deliberately records/asserts baseline defects; invert its reversion assertion when implementing the fix rather than treating it as a final desired-behavior regression test.

### 6. P1 — keyswitch list loses v1 editing and duplicates authored presentation (M)

The v2 metadata list is one `CONTROL`-height row per `Instrument.articulations` entry (`src/ui/inside.rs:215–243`), neutral dot + name + trigger. It has no inline editor, MIDI learn, reset, reorder or per-row remap. `Part.switching` stores only a global driver byte (`src/plugin.rs:77`). `order()` derives trigger ranks from lowest source key rather than stored display order (`inside.rs:129`). Source keys/range and active articulation repeat in the performance line (`src/ui/part.rs:194–199`), the authored instrument UI and the separate articulation panel; there is no canonical generated-list extraction. Do **not** claim the loop itself emits duplicate records: it emits exactly one row per IR articulation. Real source duplicate labels/multiple axes need stable identity and a source census before merging rows.

V1 `panel.rs:643,758` extracted authored choice rows, including scrolled-hidden rows, associated text keys and controls, and fell back to named colored keys. V1 `panel.rs:1607–1707` had inline note typing/learn/remap. V2 uses translated group/KSP-detection metadata instead, losing authored-list-only articulation names/control actions. See concrete replacement design below.

**Fix:** adapt the v1 extraction and inline-entry paths in `src/ui/inside.rs`, `part.rs`, `keyboard.rs` and `theme.rs`; persist display order and per-row input remaps in `src/plugin.rs`, with core routing updated by its owner. Keep source articulation identities intact. The concrete layout/data plan below defines the behavior; do not add a second articulation model to sampler-ui-ir.

### 7. P1 — Conflux render success does not mean usable authored UI (L, split by owner)

Baseline probe: three compiled scripts/interfaces, **434 widgets** overall. Pinned v1's initial capture here is itself degraded (118 visible, zero continuous controls, no wallpaper); native/user-installed working v1 reference remains necessary. Existing real-library health evidence at `/home/derpcat/.cache/kontakto-gpt-kontakt-ui/conflux-before.jsonl` reports main Creator Tools slot 2: **411 widgets, 134 visible**, six unbound text controls (`@Footer__Macro__Name__1…6`), missing `Resources/pictures/wallpaper.png`, level-meter and text-edit gaps. Slot 3 has 22 widgets/5 visible; slot 4 has one/0 visible. That record is prior-agent evidence, not a newly completed full-host callback test. NKS metadata warnings alone should not be counted as broken interaction.

The exact source flow is NKI read → `crates/sampler-kontakt/src/load.rs:581` resolves `.nckp` → `sampler_ksp::nckp` builds declared hierarchy → script compilation/binding → interface publication → `part::main_face` picks most widgets (`src/ui/part.rs:46`) → `ir_view::resolved/view/widget`. `src/ui/ir_view.rs:347` supplies a silent constant meter, line 356 paints text edit/XY/wave/file widgets as placeholders. `pictures::Source::load` rejects bitmap fonts (`src/ui/pictures.rs:19`). The missing wallpaper requires resource-owner/native evidence; do not invent an asset alias.

**Fix:** restore the six string-widget bindings and committed text edits (KSP/frontend owner), real meter sources (core/UI), validate wallpaper lookup (resource owner), and check Conflux page callbacks with actual script processing. Address findings 1–4 first; then compare Original page screenshots, active drag and callback timings against native and v1. CPU warm render data below is only the first performance gate.

### 8. P1 — full bitmap/font/table/wave geometry is degraded (L)

`ir_view.rs:238,346` forces single-line native text; `pictures.rs:19` never loads bitmap fonts. `Widget.enabled` is retained in IR but never disables the rendered control; `range.step` is also ignored by the gesture path. Stretch margins retained in image metadata are not nine-sliced. `switch_frame` uses only off/on, not hover/pressed (`ir_view.rs:35`); wallpaper always picks first frame and only applies stored offset (`ir_view.rs:181`). Tables ignore IR `cells`/range and draw one-pixel stubs (`ir_view.rs:348–353`), despite data being retained at `crates/sampler-ui-ir/src/lib.rs:259`. Waveform/file selector are inert placeholders (line 356). V1 had bitmap text metrics, real table values, waveform peaks/cursor, validated file picking and slicing.

**Fix:** complete existing IR fields/render paths; use existing picker and asynchronous picture worker patterns. Keep false placeholder support out of “usable” reports. Restore each real behavior with one focused instrument/synthetic check; do not add another UI model beside IR.

### 9. P1 — settings offer modes/zoom that performance view ignores; generated view absent (M)

`header.rs:271–295` stores mode and fit/1×/1.5×/2× settings. `part.rs:104–106` only fits width, clamped 0.5–1.0, and never reads `view_scale`; it also ignores global mode (finding 1). V1 `perf_view::scale_to_fit/room_height` fits available width **and height**, supports explicit scale and scrolls. V1's third presentation rebuilt controls into sections/strips/readable names and concise lists (`panel::sections/view`); v2 Vector is authored geometry with native faces and is not that panel.

**Fix:** connect existing settings in `src/ui/part.rs`, apply height-aware fit and explicit scale, preserve pixel alignment, and restore the generated layout separately using IR/control identity rather than mapping saved code 2 silently to Vector.

### 10. P1 — snapshots, interactive sound editing and output-console controls lost (L)

V1 snapshot picker/prev-next/drop flow was `src/ui/rack.rs:507`, menu `Target::Snapshots`, plugin `Part.snapshot`, browser shelf catalog. Baseline v2 Part lacks snapshot fields; header arrows navigate preset files (`src/ui/rack.rs:526`), not snapshots. Snapshot decoding in the translator does not provide a user UI. V1 editable envelope/filter/override path is in `src/ui/editor.rs` plus `viz`/engine overrides; v2 `inside::sound` only displays graphs/text (`inside.rs:383`).

V1 mixer had part/output/master strips, output rename/map, aux send, clip/peak reset, wide insert detail and selectable per-strip spectrum (`src/ui/mixer.rs`). Baseline v2 tree mixer adds nested groups/buses but `src/ui/mod.rs:1149` replaces the console and `mix_tree.rs:221` reduces insert detail to a count. Existing `Selection.buses` and `Part.aux` are not equivalents to exposed mixer controls.

V1 also exposed sample-folder library creation (`menu::CreateLibrary`), global/per-part Auto/RAM-only sample mode, and timing auto-alignment/transport-only/manual offset/exclude/remeasure (`menu.rs:118–155`); these commands are absent from v2 `menu.rs:44–101`. These are independent user features, not covered by a sample-memory budget or nested bus rendering.

**Fix:** restore those shell commands with the engine owners, restore snapshot actions/state in shell and retain header stepping semantics; restore sound overrides with core-owner support; expose output/master/aux/peak controls alongside the nested tree. Prefer retaining working v1 widgets/commands adapted to v2 data, rather than adding another mixer abstraction.

## Concrete concise keyswitch panel

Use **one compact header**: `Articulations 12 · [Keys ▾]` with overflow actions (`Reset mappings`, `Keep original keys`, `MIDI learn`). Do not put five driver buttons on every row or render every trigger family together. Below it, one scrollable table, one row per stable articulation identity:

`⋮⋮  ▌  Legato                         [ C#2 ]  ⋯`

- 24 logical pixels per row at normal scale (comfortable expansion under global accessibility/UI scale). Drag handle at left, 3px color stripe/active mark, flexible one-line name, fixed-width trigger editor, overflow menu. Name click auditions/selects; editing/dragging must not audition. Expose named row selection, trigger input and Move Up/Down actions to accessibility; keep focus by stable identity after a reorder. Active selection has a filled dot/stripe and accessible selected state, not color alone. Default articulation gets a small default marker in the same row; avoid a second descriptive line.
- Keep source KSP key color where known. Use a stable per-articulation fallback hue from the existing no-orange palette, with the same swatch on its on-screen key; source unchanged and remapped keys can additionally use solid vs outlined swatches. An active row increases contrast. Multiple keys show `C#2 +2` with expand/popover; do not compress noncontiguous keys into a misleading continuous range. Keyless entries show `—` and remain selectable when a control action exists. Distinguish independent articulation **axes** instead of collapsing layers with equal names.
- Click `[C#2]` to type; Enter commits, Escape cancels, blur validates before committing. Accept case-insensitive A–G, optional #/b, signed octave, and optionally MIDI integer 0–127. Use the existing Kontakt octave convention: **MIDI 60=C3, MIDI 49=C#2**, note 0=C-2, note 127=G8 (`src/ui/theme.rs:1178`, tests at 1277); do not copy the old panel tooltip's inconsistent C-1 lower bound. Invalid/out-of-range text stays visible with a short error; it never wraps/clamps silently. At narrow widths, keep name and trigger on one row and move secondary actions into the overflow menu; long/localized names truncate with a full-name tooltip.
- Editing changes the **input remap**, not the library's original switch key/KSP variable. For `SwitchOwner::Behavior`, translate remapped incoming key → original key and invoke the authored script/control action. For `Native`, select the stable articulation/zone references. Conflict with another input offers a concrete **swap**; clearing/reset is explicit. Retain original switch keys as immutable source metadata. Use v1 inline typing/learn patterns, updated to this convention.
- Drag reorder and keyboard Move Up/Down only change stored display order. They must not change `ArticulationRef`/zone assignment, default, active state, KSP source key, or existing channel/velocity/CC/program mappings. “Reassign triggers in this order” is a separate explicit action, since the current `assign_alternatives` sorts by key. Swap trigger cells swaps user mappings; swapping display rows is a different operation. Preserve held-note ownership until release if a mapping changes while a note is down.
- In Channel/Velocity/CC/Program mode, the same right-hand cell changes to `ch 2`, `1–32`, `CC32 3`, or `prog 4`; show only the selected family. Show capacity errors (16 channels, 127 velocity partitions, 128 CC/program values) once in the header. Populate actual `Alternatives`, not independently recomputed UI labels. `inside::trigger` currently regenerates evenly distributed labels and can disagree with custom source values.

### Data and persistence

`PartView.instrument` → `sampler_ir::Instrument.articulations` and `switching` (`crates/sampler-ir/src/lib.rs:523–646`) supply names, original switch keys, defaults, alternate drivers and ownership. `Zone.articulation` is identity by index; don't reorder that vector to reorder a view. `crates/sampler-kontakt/src/keyswitch.rs:97–171` populates group start-on-key ranges or recognized KSP patterns and assigns alternate drivers; recognized scripts retain Behavior ownership. It deliberately diagnoses unrecognized/competing owners, so a pretty list must not imply successful migration.

`PartView.keys` from `ScriptUi::keys` (`src/sound/mod.rs:237`) supplies authored key names/colors/types. The current keyboard doesn't consume those colors in `part_looks` (`src/ui/keyboard.rs:394`); bridge them into the common key/row look rather than creating conflicting palettes. KSP widget bindings live in `sampler_ui_ir::Widget.binding`/`ControlId`; **articulations themselves belong in sampler-ir, not sampler-ui-ir**. Restore v1's authored list/name/control association as a normalized metadata extraction path when keyswitch-pattern detection alone has no rows; preserve script slot+control identity and label-only string widgets.

Persist a small per-part user overlay: display order (stable IDs), remapped trigger inputs, clear/keep policy, driver and user color override if requested. Do not mutate source IR for UI preferences or regenerate alternatives on every draw. Baseline `Part.switching` cannot express these mappings/order. Extend shell state and core input routing together; keys edited only on screen would be a misleading fix. The UI files are `inside.rs`, `keyboard.rs`, `theme.rs`, `part.rs`; persistence/routing requires `plugin.rs` and the sampler MIDI/core owner. Reuse an existing stable source identity; if none survives reload, define it before saving an index-only overlay.

## Complete v1 UI feature inventory

Statuses below are **integration baseline**, including subfeatures omitted from the prior report. File references without a commit prefix are v2; `v1:` means `0cb7a8a0`.

| Area / feature | Status | Evidence / limitation |
|---|---|---|
| Browser split libraries/presets, foldable folders | present | `src/ui/browser.rs`, `browser_geometry.rs`; retained v1 split model |
| Library/preset search; arrows, Enter, Escape, focus shortcuts | present | `browser::view`, cached `Rows`, source/preset cursors |
| Favorites, recent history | present | browser `Source::{Favorites,Recent}`, selection favorites/recent |
| Library sort by name/vendor/use/custom; pin, drag order, rename | present | browser library menu and drag handling; `menu::Target::Library` |
| Native/custom/generated covers, theme/color/artwork/blur | present | `src/ui/{art,cover}.rs`, artwork preferences/menu |
| File/preset drag onto rack, append/replace, multi handling | present | `src/ui/mod.rs` desktop drop and `rack::add_drop` |
| Library root add/remove/picker, Kontakt import/rescan | present | `src/ui/header.rs:163–221` |
| Create playable library from sample folder | missing | v1 `menu::Command::CreateLibrary`, creator flow; removed from v2 UI commands |
| Snapshot catalog/browser/header picker/stepping/drop | missing | v1 rack `snapshot_row`; v2 Part/shell have no equivalent action/state |
| CPU/RAM/disk/voices and dropout activity header | present, worse | stats retained; memory provenance/streaming/DSP counters differ, so matching labels do not establish matching metrics |
| Global header MIDI Thru, master volume/meter, computer keys, panic | present | `src/ui/header.rs:11`, keyboard/computer; core semantics separately audited |
| Save/open multi and changed-name handling | present | `header::save_multi*`, rack multi operations |
| Per-part picture button choosing Original/Vectorized/generated | missing | v1 rack `header_at`; local unpushed `cda4ce23` restores two modes |
| Part title, rename, remove, more menu | present | `rack::header_at`, `menu::Target::Part` |
| Fold, resize, sticky headers, reveal selected, virtualized offscreen rack | present | `src/ui/rack.rs:43` onward |
| Duplicate/reorder parts, append/replace multi | present | rack drag/menu and selection order |
| Previous/next preset within folder | present | `src/ui/rack.rs:526–534`; not snapshot stepping |
| MIDI port/channel/omni, MPE, bend range | present, worse | v1 MPE off/lower/upper zone selector; v2 boolean/lower-zone mode only (`Part.mpe`); header menus/range survive |
| Part output assignment/manual policy, gain/pan/tune, solo/mute | present | rack header controls/`Part` fields |
| Per-part meters/loading/memory/diagnostic badges | present | rack/header and report bridge |
| Auto-align timing, transport-only policy, measured/manual offset, exclude/remeasure | missing | v1 App/Part menu `AutoAlign`, `AlignTransportOnly`, `Lateness`, `ExcludeTiming`, `Remeasure`; no v2 equivalents |
| Authored bitmap KSP positions/hierarchy/declaration order | worse | IR path exists; geometry/style/slicing/state gaps below |
| Creator Tools `.nckp` UI and script tabs | worse | loads Conflux; publication resets tab choice/cache |
| Komplete UI frontend | missing in both | neither v1 nor v2 proves native Komplete runtime; don't count stock fallback as support |
| Authored-layout Vector mode and bitmap release | present, worse | `ir_view`/Assets; text planning/native markers less complete |
| Generated KONTRA sections/readable names/lists/mic strips | missing | v1 `panel::sections/view`; Vector isn't generated layout |
| Original/default per-part choice and state recall | worse | face cache-only state; finding 1 |
| Script values/text/visibility reflect runtime updates | worse | interface effects republish; scalar-only updates absent from fingerprint |
| Control IDs isolate parts/scripts; stale edits rejected | worse | render IDs global; edit queue lacks generation at baseline |
| KSP button/switch scalar callbacks | present, worse | queue works; scalar recall/generation admission gaps |
| Menu popup/direct item selection, hidden/disabled entries | worse | baseline cycles values and passively changes unmatched value |
| Typed numeric entry / arrows / exact saved default | worse | ValueEdit drag only; arrows metadata ignored, no text entry |
| Vertical knob/fine modifier/reset/wheel/keys | worse | true knobs supported; knob-like sliders and source travel fail |
| Sprite strip axis/frame layout from companion TXT | present | IR image metadata/resources and `pictures::load` |
| Hover/pressed/off/on sprite states | worse | v2 off/on only |
| Wallpaper header crop, state frame, giant-strip windowing | worse | first frame/offset only; no v1 68px policy/visible crop |
| Nine-slice / stretch-margin artwork | worse | image margins retained, full frame stretched |
| Custom bitmap fonts/glyph advances | missing | `pictures.rs:19`; v1 bitmap_words/font_key |
| Native text fonts, offsets, multiline/wrap/alignment | worse | subset retained, single-line draw; `text_y` ignored |
| Real table values/dense array display | missing | one-pixel bars despite IR cells |
| Waveform peaks/attached sample/cursor | missing | placeholder vs v1 async waveform |
| File selector picker and epoch-validated callback | missing | placeholder vs v1 script-file picker |
| Text-edit current string display/edit | worse | v1 display exists, v1 mutation itself incomplete; v2 placeholder/unbound strings |
| Mouse areas / XY / arbitrary unknown widgets | present, partial in both | neither pinned path establishes full functional native interaction |
| Authored meter faces/colors/live binding | worse | v2 constant zero; v1 live binding also not fully established |
| Width+height fit, explicit zoom, scrolling | worse | width-only 0.5–1.0; global zoom unused |
| Fractional-scale pixel snapping | worse | authored positions multiply without v1 device snapping |
| Async assets, prepared control subtrees, dependency caching | worse | UI-thread first decode; rebuilds on publication |
| Articulation metadata/current active/driver family | present, worse | v2 IR list; loses authored-list/control associations |
| Keyswitch inline note typing/learn/reset/clear/keep originals | missing | v1 panel routing_cell; v2 global driver byte only |
| Per-articulation channel/velocity editing and exclude/split controls | worse | v1 supports keys/channel/velocity; individual cells/participation lost; v2 adds global CC/program families |
| Compact color-coded list, trigger swap/reorder | missing | baseline neutral rows; concrete design above; full desired reorder is new scope, not claimed v1 parity |
| Key/group mapping selection, zone counts/details | present, worse | `inside::mapping`, simpler than v1 zone inspector |
| Editable envelope/filter handles and live voice overlay | missing | v1 `editor.rs`/`viz`; v2 static `inside::sound` |
| Modulation/effect per-group detail and bypass **display** | worse | v1 `chain` read-only; v2 Sound text / mixer insert count |
| Nested instrument/group/bus strips, fold and tree routing | present | v2 improvement over flat console; `mix_tree`, `bridge` |
| Instrument tree gain/pan/mute/solo, node meters | present | `mix_tree::level`; `bridge::apply/levels` |
| Output/master console strips, bus rename/port map, aux send | missing/worse | v1 mixer exposes controls; v2 tree-only view retains fields without console |
| Peak hold/clip reset and wide insert listing | missing/worse | v1 meter/wide strip; v2 count/level display |
| Master spectrum | present | `src/ui/mod.rs:1154–1168` |
| Selectable per-part/output spectrum | missing | v1 mixer selection; v2 master-only toggle |
| Onscreen notes, velocity by click height, glissando | present | `keyboard::play/glide/key_under` |
| Selected-part/all-part play, octave/velocity/keys UI | present | `keyboard::dock`, computer handling |
| Pitch bend spring-back, latched CC1 wheel, incoming MIDI reflection | present | retained `keyboard::wheels` (v2 `src/ui/keyboard.rs:183`) |
| Computer typing protection and key release/panic | present | `computer::Computer`; existing interaction checks |
| Played-note glow/range strips | present | `keyboard::part_strips/key` |
| Authored key colors/types/names and remapped color movement | worse | `ScriptUi::keys` published but `part_looks` uses own hue, unlike v1 |
| Settings theme/browser visibility/window size/scale persistence | present | header/settings + bridge |
| View default/zoom preferences actually affect instrument view | worse | finding 9 |
| New-part MIDI/output defaults | present | `header::new_part_settings` |
| Sample memory budget setting | present | `menu::MemoryBudget`; engine efficacy separate scope |
| Rack/per-part streaming Auto vs RAM-only selection | missing | v1 `menu::Command::{Streaming,PartStreaming}` / App Performance menu; v2 menu only sample budget |
| Worker-count tuning control | missing in both | no corresponding command in either pinned UI; do not confuse CLI/engine settings with UI parity |
| Load errors/progress, missing samples, diagnosis/info | present | `part::notices`, `inside::info`, load report |
| Structured per-feature load/runtime report | present | v2 improvement; `load_report` + bridge |
| Journal search/level filter/detail, copy/export/redaction, folder reveal/about | present | retained `logs.rs`, background reader/export/copy |
| Idle redraw gating / independently read meters | present, worse | `Watch` retained; scalar control revision missing at baseline |
| Headless screenshots/real-library staged CPU benchmark | present in audit probe | baseline examples absent; prior UI branch `ui_health` exists; this audit adds full-editor measurement |
| Conflux initial authored page renders | present | baseline probe 3 interfaces/434 widgets; real-library health record |
| Conflux full controls/page/callback/persistence usability | worse | unbound strings, placeholders, gesture/state defects; native end-to-end trace still required |

## Prior work: merge versus new implementation

| Branch / exact commit | Already done | Integration / action |
|---|---|---|
| `origin/v2/gpt-kontakt-ui@47f14b59` | passive menu fix, namespaced IDs, scalar control recall/revision, generation-aware edit admission, authored auto-size-axis preservation, UI census | Not ancestor of baseline; use focused merge/cherry-picks in implementation phase |
| same branch `bd7447db`, `7591f35e`, remote tip `0be9ed3f` | resumable census, NKR/NICNT reader routing, bounded resource lookup, explicit unavailable Komplete frontend | Not ancestor; these are useful diagnostics/resource improvements, not proof of Conflux usability |
| local `v2/gpt-kontakt-ui@cda4ce23` | saved `Part.view`, header picture toggle, cache update without presentation reset, authored menu popup draft | Exists in prior worktree; **not pushed at audit fetch**. Prior `parity-bench2.log` records E0616 private `control_edits` test access; request owner validation/push before treating this as merge-ready. Code 2 still maps to Vector |
| `origin/v2/gpt-uvi-ui@dcbc07c1`, tip `24c70a25` | UVI widget/resource/state bindings, mapper positions/defaults | Not ancestor; read `UVI_UI_REPORT.md`. Not a Kontakt gesture/Conflux fix |
| baseline history `cbdb0901`, `fa49d7d5`, `40e80e59` | global driver persistence/live remap, articulation list and marks | Already integrated; don't rebuild global driver plumbing while adding per-row edit/order |

No prior branch found in this inspection completes concise per-row keyswitch editing/reordering, nested wheel arbitration, full bitmap font/wave/table parity, or a working generated panel. Do not merge the entire old UI branch blindly: it predates later integration changes; adapt and rerun targeted checks.

## Still unknown / next measurements

The shared scanner is authoritative for corpus-wide coverage. At this follow-up, `/home/derpcat/.cache/kontra-scan/bin` and `results` do not yet exist. Consume its v1/v2 TSVs when delivered; do not build a parallel collector. The three-library frame probes below are narrowly scoped rendering measurements, not a substitute for the scanner's load/UI/play census. Ask the coordinator/census owner for any extra columns.

1. Native Conflux waveform/fonts/wallpaper/defaults, string editing and page callback sequence. Record same page/control in Kontakt; headless test should process authored `ui_control` and timer/listener effects, publish, and measure callback→visible update and retained Original choice. Render all pages, not only the initial one.
2. Full native editor GPU paint/frame interval, including active drag, window/device scale, host transport and audio load. CPU offscreen numbers are a separate budget; require physical GPU rather than software adapter and interleaved v1/v2 runs.
3. Real libraries with repeated/independent-axis articulation records. Census source IR IDs/names/original keys and KSP authored list associations, then check runtime owner semantics. Deduplicating equal labels is unsafe.
4. Resource decode stalls/RSS during repeated Original/Vector toggling and script UI publication. Count asset decodes/retained bytes and first-frame p99; preserve asset identity and off-thread preparation before claiming lower RAM.
5. Some v1 behaviors are incomplete too: text mutation, XY/mouse-area semantics, live authored meter binding and Komplete UI. Use native reference rather than promoting v1 placeholders into a success target.
6. Snapshot, sound override, output mixer changes require shell/core coordination. This audit establishes missing user paths; it does not claim matching DSP/streaming/automation behavior.

## Validation

- `kontakto-heavy cargo test --profile ci --no-run`: **passed**, including library and integration-test executables. Existing vendor/compiler warnings remain; no production fixes were made to suppress them.
- `audit_ui_toggle_and_nested_wheel --nocapture --test-threads=1`: **passed**; reproduced view reversion and simultaneous child/parent wheel movement.
- All six real-instrument version runs completed. The original v1 Conflux test **skipped timing**; the fresh pinned-tree idle probe completed and its screenshot was visually inspected. V1 Vista/Areia and v2 Conflux/Vista/Areia timing tests passed.
- `git diff --check`: passed. No dev servers were started. Measurements use one heavy job at a time through `kontakto-heavy`; there is no audio PCM/decrypted-source export. The supplemental v1 patch is audit-only and applies to `0cb7a8a0`.
- Checkpoint: `8b539b4c`. The push commit contains the completed report, aggregate measurements, replayable v1 probe patch and v2 opt-in probes.
