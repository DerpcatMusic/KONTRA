# Gold standard: script host, UI controls and automation

Status: research spec, 2026-10-27. Read-only study; no code changed, no build run.
Tags: `[source]` KONTRA code, `[doc]` vendor manual text (file:line of the extracted text),
`[binary]` Kontakt8.exe / UVIWorkstationx64.exe strings, `[artifact]` decoded data already in
PluginArtifacts, `[web]` online docs, `[inferred]` reasoning from the above, not measured.

## 1. Scope and what was actually checked

- Kontakt manuals, extracted with `pdftotext -layout`: `PluginArtifacts/fresh-engine/docs-txt/{ksp,kontakt,lua}.txt`.
- The UVIScript folder named in the brief (`~/Documents/UVI/Falcon/UVIScript`) is **empty** on this machine.
  UVI behaviour below comes from one binary string check and community posts, not from UVI docs.
- Areia: `fresh-engine/areia-script/0.txt` (script) and `fresh-engine/areia-old-ir.json` (KONTRA's decoded
  instrument, `instrument.script_automation`). The raw Areia `.nki` could **not** be decoded here: it has no
  zlib streams and no plain-text automation tags. KONTRA's own parser is the decoder.
- Library scripts: only the artifact copies exist (`layer-onset-20261024/{areia,vista}-scripts`,
  `KURV/research-20261021/C-scripts`). The `.nki` files in `/data/main/Libraries/Kontakt` are present
  (682 files; Morphology, ANALOG STRINGS, Conflux, Vista, Una Corda, Pacific, Shreddage, ...) but their
  containers are not decoded here. Ample, MODO DRUM `.pak`, EZDrummer were **not analysed** in this pass.
  UVI `.ufs` files (26 in `Libraries/UVI`, VWinds + Augmented Orchestra) were **not parsed**.
- Web: sfzformat.com control header, docs.hise.audio Synth API. Decent Sampler and EastWest/Opus/Koda docs
  were **not reached** in this pass.

## 2. The Areia question: where CC1 → Dynamics lives

**Answer.** The assignment is a program-private automation record in the instrument's `.nki` program
(Kontakt object `BAutomationObject`, SER class 0x01, versions 0x70/0x71). It is not in the script and not in
the UI layout. The record says: `mode 1` (controller), `address 1` (CC1), slot 0, and a slider ordinal.
Kontakt applies it by moving the slider; the slider's `on ui_control` then does the work.

Chain, each link evidenced:

1. **Storage** `[binary]` Kontakt8.exe contains `NewAutomationObject(…BParTag, eAdressMode…)` and
   `RemoveAutomationObject`, class `BAutomationObject`, and UI strings "MIDI Automation", "Host Automation",
   "Soft Takeover", "From - To" (binary strings 29596–29602; 32287–32295). `[doc]` KONTRA_Manual 5894–5955
   (Automation tab: drag a CC onto a knob, From%/To%, Soft Takeover, Learn) and 13902–13933 (MIDI Learn).
   `[source]` `vendor/ni-file/src/kontakt/native_schema/programs.rs:9668` (`sertype 1 BAutomationObject`,
   current 113); `docs/re/kontakt-catalog/reader_schemas.tsv:2-3` (`IBBHIFFS` for 0x70, `IBHIIFFS` for 0x71).
2. **Decode** `[source]` `crates/sampler-kontakt/src/automation.rs:20-110`: `program_automation()` reads
   `mode` (u32, 0..2), `soft_takeover` (bool), `address` (u16), `id` (i32), `secondary_id` (0x71 only),
   `low`/`high` (f32), and a tag string (`pts_script_slider_<slot>_<ordinal>` or `pts_script_switch_…`).
3. **Map to a binding** `[source]` `crates/sampler-kontakt/src/library.rs:324-362`: `mode 1` with
   `address < 128` becomes `ScriptAutomationSource::Controller(cc)`; `mode 2` becomes `HostParameter(addr)`.
   `automation_target(tag)` (`library.rs:3723`) turns the tag into (slot, **ordinal**, switch).
4. **Bind to a widget** `[source]` `crates/sampler-kontakt/src/load.rs:1569-1601`: the widget is
   `widgets.filter(kind == Slider).nth(binding.slider)`, i.e. the Nth declared slider of that script slot.
5. **Apply** `[source]` `crates/sampler-core/src/automation.rs:95-230` (`dispatch_automation`): range mapping
   `low + (high-low)*input`, soft takeover latch on crossing, then `invoke_widget_with_targets`, which runs
   the widget's `on ui_control`.
6. **Script side** `[source]` Areia `0.txt:4556` declares `slider_controller_dynamics` (range 0–127),
   `:8033` sets `$CONTROL_PAR_AUTOMATION_ID` = 10 and name "Dynamics". `:118178-118187` is
   `on ui_control($slider_controller_dynamics)`: it guards `if ($store… # $slider…)`, then calls
   `set_controller(110,$store_slider_controller_dynamics)`. `:118141` shows the `stop_wait` callback-id pattern.
   `set_controller(110, …)` also appears at `0.txt:22142, 88531, 89074, 99668`.
7. **Modulator** `[artifact]` `areia-old-ir.json` `modulators`: 432 voice modulators with `source: Controller 110`.
   The volume chain follows CC110, so the path is CC1 → slider → ui_control → set_controller(110) → volume.

**Measured mapping** `[artifact]` `areia-old-ir.json` → `instrument.script_automation`:

| source | slider ordinal | Areia name (N-th `declare ui_slider`, `0.txt`) | line |
|---|---|---|---|
| Controller 1 (CC1) | 40 | `slider_controller_dynamics` | 4556 |
| Controller 11 | 42 | `slider_controller_expression` | 4619 |
| Controller 16 | 41 | `slider_controller_dynamic_range` | 4591 |
| Controller 17 | 44 | `slider_controller_samplestart` | 4689 |
| Controller 21 | 43 | `slider_controller_vibrato` | 4654 |

The script declares 63 sliders and 294 switches. Ordinal 40 counted over `declare ui_slider` in document
order is the dynamics slider, which matches its `AUTOMATION_ID` of 10. So the **ordinal ≠ automation id**
(40 vs 10). The record's own `id` field is never stored in the IR (see gap G1).

Other CC→control facts in Areia: `on controller` (`0.txt:101213-101230`) runs `ignore_controller` for CC
100–114, so those CCs do not reach the built-in CC path. `[inferred]` That stops the script's own
`set_controller(110)` from being processed again.

## 3. Kontakt: how the host model works (reference)

**Callbacks** `[doc]` KSP_Manual: `on init` (2-7) runs on Apply, preset/instrument load, engine restart,
and snapshot load when `set_snapshot_type` is 0 or 2. `on persistence_changed` (1213-1214, 659-672) is the
recommended place to refresh UI after restore. `on ui_control` (1046-1092) runs on user interaction; `on
ui_controls` (1095-1111) runs **first**, then the individual `on ui_control`. `on listener` (678-700): fixed
interval via `set_listener`, and transport start/stop. `on controller` (`ksp.txt` ~ line 6-7 area, "MIDI
controller callback"). `on async_complete` (`ksp.txt:488`). Callback-id pattern: `stop_wait` and
`$NI_CALLBACK_ID` (Areia `0.txt:118179-118180`).

**Automation sources** `[doc]` Kontakt_Manual 5894–5955: MIDI automation list + host automation list; drag a
source onto a knob; multiple parameters per source allowed; From/To range; Soft Takeover ("the assigned
parameter will not be changed until an automation value is received that matches the parameter's current
value"). MIDI Learn: right-click → "Learn MIDI CC# Automation"; Learn single/multiple. `[doc]` Kontakt_Manual
14084-14088: "Unwind automation IDs for additional loaded patches" re-numbers IDs of a second instrument
in a Multi (e.g. 0–9 → 10–19). `[binary]` the same option string exists (`Unwind Automation IDs…`, 30042).
Host automation slots: up to 2048 (`kontakt.txt:14745`).

**Controllers** `[doc]` KSP_Manual `set_controller` (3311-3320): `set_controller(<controller>, <value>)`
sends a CC, pitch bend or channel pressure; CC 0–127 and value 0–127. "cannot be used in the on init
callback". `ignore_controller` (3135-3148): valid only in `on controller` and drops the event; example
transforms mod wheel into aftertouch.

**Persistence** `[doc]` KSP_Manual 1640-1700: `make_persistent` saved with instrument, snapshot and `.nkp`;
restored at the **end** of `on init`, read earlier with `read_persistent_var`. `make_instr_persistent` saved
with the instrument only, not snapshots, so loading a snapshot does not change it.
`[artifact]` `docs/re/ksp/PERSISTENCE_CENSUS.md`: 16 instruments; `make_persistent` in all of them; `on
persistence_changed` in 9 (Conflux, Morphology, Dolce, Areia, Una Corda, Shreddage ×2, ANALOG, Chorus);
`read_persistent_var` in 7; `make_instr_persistent` in 4 (Conflux, Una Corda, Shreddage, ANALOG).

**Engine parameters** `[doc]` KSP_Manual `set_engine_par` (line 534 area) and `purge_group` (5560-5600):
purge is allowed only in `on ui_control`, `on ui_controls`, and `on persistence_changed`, and the manual
advises against it in UI callbacks of automatable controls.

**Kontakt Lua** `[doc]` LUA_API_Manual: start conditions "controller" mode (`lua.txt:1140-1217`). It uses the
same CC and controller concepts; no separate automation model documented in the extracted text.

## 4. UVI

- `[binary]` UVIWorkstationx64.exe has the string "Host Automation" (`rabin2 -z` 2040, 33933, 33934). Nothing
  with "MIDI learn" or "CC assign" matched.
- `[web]` Community posts (VI-Control, KVR) say Falcon's knobs take MIDI Learn with a per-knob CC range (a
  range dial sets how far the CC moves the parameter), and host automation shows as "host automation 1…n".
  Those are user reports, not UVI docs. A UVI support Tips & Tricks article covers pitch bend via host
  automation or MIDI Learn. Not opened.
- `[web]` WebSearch returned no UVIScript documentation.
- Conclusion: UVI's model, as far as it can be seen here, is the same two sources (CC and host automation) with
  per-knob range. The gold model should accept it without needing UVI's exact internals.

## 5. HISE

- `[web]` docs.hise.audio Synth API: `setMacroControl(index, value)`, index from 1, eight macros, value
  0–127 (may be fractional). `setModulatorAttribute(chain, index, attr, value)`. `sendController` /
  `addController` for MIDI-side events. Warning: a macro set from a control wired to that same macro can
  recurse and freeze. `deferCallbacks` moves callbacks to the message thread; MIDI callbacks become read-only
  in that mode. `setUseUniformVoiceHandler` adds sync cost and is off by default.
- `[source]` Research/samplers/hise/hi_core/hi_core/MainControllerHelpers.cpp:705-761: MIDI learn is a
  `MidiControllerAutomationHandler` whose state is a `ValueTree("MidiAutomation")` inside the user preset.
  So a CC assignment is saved in the preset, not per script variable. Line 73: `deactivateMidiLearning()`.
- Design intent: the host owns MIDI learn state and presets; the script only sets values and macros.

## 6. sfizz (no scripting)

- `[web]` sfzformat.com `<control>` header: `default_path`, `note_offset`, `octave_offset`, `label_ccN`,
  `set_ccN`, `#define`. Multiple `<control>` headers allowed; `default_path` applies until the next one.
  The page does not document set_cc persistence or host behaviour.
- `[source]` sfizz/src/sfizz/Region.cpp:1720-1746: a CC number is a region opcode parameter
  (`ccNumber < numCCs`). Each CC has a modulation connection with `ModKey::createCC(ccNumber, …)`
  (per-voice vs global via `ccModulationIsPerVoice`). CC triggers: `ccTriggers[…]` (Region.cpp:391-414).
- Conclusion: SFZ needs only "a CC number → modulation destination" plus a few trigger ranges. There is no
  control widget, no callback and no saved assignment. The host (not the format) owns MIDI learn.

## 7. KONTRA now (file:line)

- **Script host** `[source]` `crates/engine/engine/src/script.rs`: callbacks `Callbacks{init,note,release,
  controller,persistence_changed}` (:50-60) and `plan()`-side mapping in `crates/app/host/src/script.rs:12-42`.
  `on ui_control`, `on ui_controls`, `on listener`, `on async_complete` are compiled in
  `crates/script/ksp/src/compile/mod.rs:132-138, 267-268`, but are not in `Callbacks` (so they go through
  the VM's coroutine path, not `ScriptPlan.callbacks`).
- **CC in** `[source]` `crates/engine/engine/src/lib.rs:660-711` (`ump`): reads CC, writes `cc_values`, runs
  the script `controller` callback, returns early on `ignore_controller`, then `behaviour.set_cc` and
  refreshes notes. `script.rs:425` (`CcNum`), `:444` (`ignore_controller`).
- **set_controller** `[source]` `script.rs:538-540` → `params.rs:79-93`: writes `cc_values`, sets the CC on all
  16 channels, refreshes voices. It does **not** run `on controller` and does **not** move any bound control.
- **Controls** `[source]` `crates/engine/engine/src/controls.rs:11-55`: a control is its variable cell; other
  parameters (hide, picture, range) live in `control_pars` and are allocated on first write
  (`controls.rs:36-40`, comment "a parameter first set after init allocates its slot on the audio thread").
- **Automation dispatch** `[source]` `crates/sampler-core/src/automation.rs:95-230` (above) and
  `receiver.rs:500-507` / `controller_event.rs:189-192` (CC and host sources reach `dispatch_automation`).
- **Bindings** `[source]` `load.rs:1569-1631` (saved + fixed), `load.rs:1849-1882` (`fixed_automation`:
  every widget with `$CONTROL_PAR_AUTOMATION_ID` becomes `HostParameter(id)`, range 0–1, no soft takeover).
- **Persistence** `[source]` `crates/script/ksp/src/persist.rs`; snapshot path in `sampler-kontakt/src/snapshot.rs`
  (`docs/re/ksp/PERSISTENCE_CENSUS.md` says snapshot writes overwrite any named entry regardless of
  `make_instr_persistent`).
- **Tests** `[source]` `load.rs:1884-1930` (automation id binds host slot and runs its callback; a controller
  source binds to a widget). Read only the first test in full.

## 8. Gaps, ranked by audible / behavioural impact

**G1. Saved CC binding uses the slider ordinal, and the record's `id` is discarded.** `[source]`
`library.rs:330-341` keeps `address` and `tag` only; `automation.rs:20-110` parses `id` (i32) and
`secondary_id`; `load.rs:1576-1581` binds `.nth(slider)`. `[artifact]` ordinal 40 ↔ automation id 10 for
Areia dynamics (table, section 2). Impact: CC1 moves the right control *only while* the Nth-slider order
matches Kontakt's. A script that declares sliders conditionally, in another order, or in a different slot
puts the CC on the wrong control. Audible for every library with mod-wheel dynamics or expression.
Measurement: a fixture where declaration order differs from the automation id, showing the wrong control moves
(not written). Fix: bind by `id` (the automation id), keep the tag as a fallback.

**G2. Real-time risks in the automation path.** `[source]` `sampler-core/src/automation.rs:120,152,161,192,
226`: `.unwrap()` on `plans.get` and `widget_definitions().find()` run per CC, and a stale plan id panics on
the audio path. `[source]` `controls.rs:36-40`: first write of a non-value parameter allocates on the audio
thread. Impact: a panic or dropout when a plan is swapped during a CC. Measurement: a loop of CC events
during a plan swap with an allocation counter (not run).

**G3. Script `set_controller` does not move the bound control.** `[source]` `params.rs:79-93` vs Kontakt
`[doc]` KSP_Manual set_controller (3311-3320): sends the CC downstream. What Kontakt does to an
automation-bound slider when the script sends its CC is **not documented and not verified**. Areia does not
depend on it: its loop is CC1 → slider → ui_control → set_controller(110), never the reverse. Medium impact;
needs an oracle run (`tools/ksp-conformance` style) before a rule is written.

**G4. Snapshot restore does not respect `make_instr_persistent`.** `[artifact]` `PERSISTENCE_CENSUS.md`:
`snapshot.rs` overwrites any named entry; `[doc]` KSP_Manual 1640-1660: `make_instr_persistent` values must
survive snapshot load. Impact: UI elements that should stay (Conflux, Una Corda, Shreddage, ANALOG) change
on snapshot load.

**G5. Unwind automation IDs (multi with the same instrument twice).** `[doc]` Kontakt_Manual 14084-14088;
`[binary]` option string. KONTRA: no collision handling found in `sampler-kontakt/src` or `app/host/src`
(grep, no hits). Impact: two instances of an instrument share host IDs in a DAW multi. Medium, host-dependent.

**G6. purge_group placement.** `[doc]` KSP_Manual 5560-5600: only in ui_control / ui_controls /
persistence_changed; avoid in automatable UI callbacks. KONTRA: `crates/sampler-ksp/src/lower.rs:2612-2620`
lowers purge as a native write, but the placement check and its effect on a host-driven ui_control call are
not verified here. Low-medium: a purge fired from automation can stall a CC.

**G7. Script CC transform (CC→CC re-mapping) is supported but its re-entry rule is not written down.** The Vista
script `layer-onset-20261024/vista-scripts/*.ksp:780-781, 1405-1409` does `set_controller(100, %CC[…])` and
`ignore_controller` on a CC. KONTRA `lib.rs:660-711` covers the pass-through, but there is no test for a
script-generated CC re-entering `on controller`. Low.

**G8. Soft takeover and From/To range: implemented, parity-level.** `[source]` `sampler-core/src/automation.rs:
155-205` (latch on crossing, range `low + (high-low)*input`). `[doc]` Kontakt_Manual 5941-5950 (same semantics).
No gap, but there is no measurement against Kontakt; the crossing test is the only check.

**G9. UVI and HISE: no measured gap.** Not measured; UVI binary/doc coverage too thin (section 4). Track
under the gold model rather than as a gap.

## 9. Gold standard (dialect-neutral host model)

A single host model that KSP, UVI Lua, HISE and SFZ compile onto. Concrete types:

```
Control {
  id: u32                  // stable automation id (Kontakt AUTOMATION_ID; UVI/HISE: their param id)
  name: str                // automation display name
  domain: Int(min,max) | Real(min,max) | Toggle | Choice(n)
  value: cell              // the one value; a script variable is a view on this cell
  persistence: None | Snapshot | Instrument   // make_persistent | make_instr_persistent
  callbacks: ui_control    // runs once per value change from ANY source (UI, host, MIDI)
  sources: [Binding]       // zero or more; many-to-one allowed
}
Binding {
  source: Cc(0..127) | Host(id: u32) | Pitch | ChannelPressure | Learn-only
  low, high: f32 (0..1)    // From% / To%
  soft_takeover: bool      // latch until the input crosses the current value
  mode: absolute            // (all three libraries use absolute; relative not needed yet)
}
```

**Rules:**

1. **One write path.** Every value change (UI drag, host automation, CC binding, script write) goes through
   `set_value(control, v, origin)`. `on ui_control` runs once per accepted change. A script write that
   re-enters with the same value is dropped (the Areia `if ($store # $slider)` guard, made automatic).
   `[source]` `automation.rs:207-225` already routes CC and host input through `invoke_widget_with_targets`;
   the rule makes that the only path.
2. **Bind by stable id, not by declaration order.** The saved record keeps Kontakt's `id`; the tag is a
   fallback only. Fixes G1.
3. **Persistence is a property of the control, not of the widget kind.** `Snapshot` values are overwritten
   on snapshot load; `Instrument` values are not (G4).
4. **set_controller sends a CC downstream and never fires `on controller` on the same CC.** It does not move
   a bound control, until an oracle run says otherwise (G3).
5. **ignore_controller drops the event from the built-in CC path only in `on controller`.** Automation
   bindings still see the CC (`[inferred]`; needs confirmation before code).
6. **Purge and load are control-thread operations.** Allowed from `ui_control`, `ui_controls`,
   `persistence_changed`, and async; never from a note or CC callback on the audio thread (G6).
7. **MIDI learn state is preset data.** Save the binding table with the preset (as HISE does), not with the
   script (G5 adds the unwind rule).

**Real-time rules (concrete):**

- No `unwrap()`/`expect()` on the audio path (G2). Return `Error` and drop the event.
- Preallocate every control's parameter slot at `plan` time; the first write after init must not allocate
  (`controls.rs:36-40`).
- Callbacks run in the coroutine budget (`COROUTINES = 64`, `crates/app/host/src/script.rs`). What happens when a
  callback exceeds it is not verified here; check before writing the rule.
- Automation dispatch reserves its capacity before any write (`automation.rs:118-130`, keep).

## 10. Measurements that prove each claim (and what is still open)

| Claim | Evidence in this pass | Still open |
|---|---|---|
| Areia CC1 → dynamics via automation id 10 | IR `script_automation` (Controller 1, ordinal 40); name at N-th `declare ui_slider` = 40; script `:8033` ID 10 | decode the raw `.nki` tag text to confirm ordinal↔id mapping |
| Ordinal ≠ id | 40 vs 10 for the same control | a fixture where ordinal and id both move (G1 test) |
| CC110 is the volume chain | 432 voice modulators with Controller 110 in IR | runtime probe |
| Vista uses CC transform | script lines 780-781, 1405-1409 | none |
| Persistence census (16 instruments) | `PERSISTENCE_CENSUS.md` (artifact, not re-run) | UVI/Ample/MODO/EZ not in census |
| set_controller does not move bound slider | `params.rs:79-93` read; no test seen | Kontakt oracle run (G3) |
| Unwind IDs unhandled | grep, no hits | Multi with duplicate instrument |
| HISE saves MIDI learn in preset | `MainControllerHelpers.cpp:705-761` | none needed |
| SFZ CC is a region opcode, no control | sfzformat control header page; sfizz `Region.cpp:1720-1746` | none needed |

Not done (named, not hidden): decode `.nki` containers without a build; read UVI `.ufs`; read Ample, MODO DRUM
and EZDrummer scripts or presets; reach Decent Sampler, EastWest/Opus/Koda docs; open the UVI Tips article.
Each is a research task of its own; none is needed to fix G1–G4, which are settled from the code and the
Areia IR.

## 11. Library reliance (what is verified vs assumed)

- **Areia 16 Violins**: CC1 → dynamics (slider 40, id 10), CC11 → expression, CC16 → dynamic range, CC17 →
  sample start, CC21 → vibrato. Verified against the IR dump. The biggest reliance on G1.
- **Vista 5 Violins** `[artifact]`: `set_controller` + `on ui_control` + CC100/101 transform and `ignore_controller`
  on a CC. No automation ids in the artifact copy.
- **Conflux, Una Corda, Shreddage, ANALOG**: `make_instr_persistent` (census). Depend on G4.
- **Morphology, Dolce, Chorus, ANALOG, Una Corda, Shreddage, Conflux, Areia**: `on persistence_changed`
  (census). Depend on the restore order (not measured here).
- **Ample, MODO DRUM, EZDrummer, UVI libraries**: not analysed. Listed so the gold standard is not tested on
  Kontakt alone; the UVI and MODO pass is the next step.

## 12. Sources

- `[doc]` Kontakt 8 manuals: `/home/derpcat/Desktop/Kontakt 8/Documentation/{KSP_Manual,KONTAKT_Manual,LUA_API_Manual}.pdf`
  (extracted to `PluginArtifacts/fresh-engine/docs-txt/`).
- `[web]` https://sfzformat.com/headers/control (control header; the ccN opcode page was not opened).
- `[web]` https://docs.hise.audio/scripting/scripting-api/synth/index.html (Synth API; macros, controllers).
- `[web]` WebSearch on UVI Falcon automation (community posts; not UVI docs).
- `[binary]` `/data/dev/workspace/re-binaries/Kontakt8.exe`, `UVIWorkstationx64.exe` (rabin2 -z).
- `[artifact]` `fresh-engine/areia-old-ir.json`, `fresh-engine/areia-script/0.txt`,
  `layer-onset-20261024/vista-scripts/*.ksp`, `docs/re/ksp/PERSISTENCE_CENSUS.md`.
- `[source]` KONTRA files as cited above.
- `[source]` Research/samplers/hise, Research/samplers/sfizz (read-only).
