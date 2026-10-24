# KSP `ui_*` widget audit: manual semantics vs renderer

Date: 2026-10-24. Scope: every control kind in `sampler-ui-ir::Kind`, checked against the KSP manual
(local copy: Kontakt 8 `Documentation/KSP_Manual.pdf`, text extracted with `pdftotext -layout`; the web
manual returns 403 to automated fetches), the renderer (`src/ui/ir_view.rs`), and the usage census in
[KSP_UI_CENSUS.md](KSP_UI_CENSUS.md). Usage is performance-view counts across the 10 libraries.

Renderer line numbers refer to this branch (`feat/ksp-widget-completeness`, from `origin/main` a05a853f8).

## Verdict

Every high-usage control is already implemented: slider, knob, switch, button, menu, value edit, label,
panel, table, level meter and mouse area. The behaviours that the audit found missing or unverified are
listed below with the reason no change was made. No renderer code changed in this audit.

## Per control

| Control | Census use | Renderer behaviour (evidence) | Manual semantics | Status |
|---|---|---|---|---|
| `ui_slider`, `ui_knob` | 10/10 and 3/10 | Drag with fine modifiers, wheel quantises (`ir_view.rs:1517`), ctrl/cmd reset and double-click numeric entry (tests `ir_view.rs:3343-3410`, `3686`) | Standard value controls | ✓ covered by gesture tests |
| `ui_value_edit` | 9/10 | Numeric entry on double-click and Enter (`ir_view.rs:1273`, `numeric_entry` at 1263) | Editable numeric value | ✓ covered |
| `ui_switch`, `ui_button` | 9/10 and 8/10 | Activation, momentary and hold frames (`ir_view.rs:1383`, `1608`, `1633`) | Toggle and momentary | ✓ covered |
| `ui_menu` | 9/10 | Popup list with hierarchy (`ir_view.rs:2518` `menu_popup`, `menu_hierarchy.rs`) | Popup selection | ✓ covered |
| `ui_label` | 10/10 | Text with font, alignment and `text_y` (`ir_view.rs:1820`; `$CONTROL_PAR_TEXT_ALIGNMENT` parsed at `sampler-ksp/src/ui.rs:595-613`) | Text, alignment | ✓ covered |
| `ui_panel` | 3/10 | Layout container, hides children (`ir_view.rs:2365`) | Container | ✓ covered |
| `ui_table` | 3/10 | Array drawing and pointer edits on the visible prefix (test `ir_view.rs:3290`) | Integer array, selected step | ✓ covered |
| `ui_level_meter` | 3/10 | Live level from `Binding::Meter` or `assets.meter` (`ir_view.rs:1825-1857`) | Attached meter | ✓ covered (the older FEATURES.md row is stale) |
| `ui_mouse_area` | 0/10 | Press and release events with mods and hover (`ir_view.rs:2343`); file drops through `file_drop_target` (`ir_view.rs:2626`) | Invisible drop target; accepts audio, MIDI, array; one or many files | ◐ drops work; `$CONTROL_PAR_DND_ACCEPT_*` and `RECEIVE_DRAG_EVENTS` are parsed (`sampler-ksp/src/lib.rs:821-835`) but accept rules were not verified in the drop path |
| `ui_text_edit` | 4/10 (21 total) | Editable field; value committed on submit (Enter), draft reset on unfocus (`ir_view.rs:2217-2251`) | Manual gives no commit rule for text edit | ? unverified: no spec for focus-loss commit; no change without evidence |
| `ui_file_selector` | 1/10 | Directory navigation, file filter, activation sets path (`ir_view.rs:2253-2322`) | Browser within the widget | ◐ covered for basic browse; Kontakt's full embedded browser not reproduced |
| `ui_xy` | 1/10 (Morphology) | Two-axis pad drag and both-axis update (test `ir_view.rs:3809`) | Cursors in an array; `$CONTROL_PAR_MOUSE_MODE` picks mouse behaviour; `MOUSE_BEHAVIOUR_X/Y` set speed | ◐ pad works; `MOUSE_MODE` meaning is not in the manual (values unspecified) and is parsed but not applied (`sampler-ui-ir/src/lib.rs:517`) |
| `ui_waveform` | 0/10 | Peaks, cursor, zone display (`ir_view.rs:2323-2342`; `wave_view.rs`) | Displays zone and slices; slice editing | ◐ display only; slice editing and MIDI drag not implemented (unused by the census) |
| `ui_wavetable` | 1/10 (Morphology) | Peaks through the wave view path (`ir_view.rs:2323`) | Wavetable display | ◐ same path as waveform |
| `ui_pic` / picture | not a census kind (pictures come via `$CONTROL_PAR_PICTURE`) | Strip and frame pictures drawn by `art` (`ir_view.rs:1510-1530`) | Picture controls | ✓ rendered; no dedicated test checked in this audit |
| `ui_zone`, waveform flags | not counted | Not audited in this pass | Waveform flag constants | not audited |
| `$CONTROL_PAR_*` | 124 distinct names recognised | Names parsed in `sampler-ksp/src/ui.rs` and `builtins.rs` | Per-property | ◐ see the ledger (`docs/architecture-v2/KSP_PARITY_LEDGER.md`); `MOUSE_MODE`, `DND_ACCEPT_*` and `RECEIVE_DRAG_EVENTS` are `Approximate` |

## Right-click

The mui bins widget handles a secondary click (`vendor/mui/src/widgets/bins.rs:164`). KSP controls have
no right-click behaviour in the renderer, and the manual does not define one for the controls above.
Nothing to add without a spec.

## Decisions needed before more work

1. **`$CONTROL_PAR_MOUSE_MODE` values (ui_xy).** The local manual says only that the mode changes XY mouse
   behaviour. Values need a recorded Kontakt observation (oracle) or a manual page that defines them.
2. **`ui_text_edit` commit on focus loss.** The manual is silent. Kontakt's behaviour needs an oracle run
   before changing the current Enter-only commit.
3. **`ui_mouse_area` accept rules.** `DND_ACCEPT_AUDIO`, `DND_ACCEPT_MIDI`, `DND_ACCEPT_ARRAY` and
   `RECEIVE_DRAG_EVENTS` are documented. Enforcing them is a small change, but there is no census usage,
   so it is ranked last.
4. **Waveform slice editing.** Not used by any census library. Not in scope until a library uses it.

## Method and limits

- Manual text: `pdftotext -layout` on the local PDF; section lines: `ui_text_edit` 2867, `ui_mouse_area`
  2642, `ui_xy` 2997, `ui_waveform` 2938.
- Renderer evidence is code reading plus the existing gesture tests named above. No Kontakt oracle was
  run, because the oracle lock and unregistered-library refusal still block instrument loads.
- `docs/FEATURES.md` rows for `ui_table`, `ui_level_meter`, `ui_mouse_area`, `ui_text_edit`, `ui_xy` and
  `ui_wavetable` cite `src/ui/perf_view.rs`, which no longer exists. They are stale and should be replaced
  with the `ir_view.rs` references above in a follow-up.
