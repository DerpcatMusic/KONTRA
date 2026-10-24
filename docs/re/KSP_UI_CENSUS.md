# KSP UI census: which `ui_*` controls the corpus uses

Date: 2026-10-24. Scope: the 10 top-level Kontakt libraries under `/data/main/Libraries/Kontakt`, one
representative instrument each (`tools/parity-run` list plus Morphology, Shreddage, Chorus, Vista, Dolce).
Source of truth: what KONTRA decodes from each instrument's KSP (`sampler-ksp` → `sampler-ui-ir`), dumped
by the parity run as `census.json` per face. This is decoded widgets, not raw script text, so property names
are the IR fields (`hidden`, `hide`, `enabled`, `placement`, `text_y`, `value_text`, ...), not `$CONTROL_PAR_*`
keys. `shift_on_press` is reported on nearly every widget and is likely a default, so it is not used for ranking.

Only the performance-view face per instrument is counted here (the single `performance: true` face in each).

## Per library (performance view)

| Library | Page (w×h) | Widgets | Hidden | Kinds (count) |
|---|---|---|---|---|
| Shreddage 3.5 E-Upright | 1000×637 | 1502 | 34 | Slider 480, Panel 280, Label 437, Switch 159, Menu 70, ValueEdit 39, Table 25, Button 12 |
| ANALOG STRINGS | 732×540 | 934 | 3 | Slider 247, Label 255, Switch 117, Button 238, ValueEdit 26, Menu 20, Table 24, LevelMeter 4, TextEdit 2, FileSelector 1 |
| Areia 1.2.0 | 800×412 | 847 | 826 | Switch 294, Label 340, ValueEdit 124, Slider 63, Menu 20, TextEdit 5, Button 1 |
| Audio Imperia Dolce | 800×412 | 690 | 671 | Switch 270, Label 257, ValueEdit 80, Slider 62, Menu 16, TextEdit 4, Button 1 |
| Morphology Evolved | 960×630 | 671 | 0 | Slider 367, Button 67, Label 84, Menu 47, Panel 88, LevelMeter 16, Wavetable 1, Xy 1 |
| Audio Imperia CHORUS | 800×412 | 572 | 552 | Switch 209, Label 230, ValueEdit 54, Slider 59, Menu 15, TextEdit 4, Button 1 |
| Conflux 1.1.0 | 970×499 | 378 | 6 | Knob 200, Panel 46, Switch 38, Label 34, Button 16, Menu 14, ValueEdit 11, LevelMeter 6, Table 6, TextEdit 6, Slider 1 (native NCKP face) |
| Afflatus Chapter II Brass | 900×430 | 143 | 64 | Button 38, Label 55, Slider 30, Switch 17, ValueEdit 2, Menu 1 |
| Una Corda Library | 633×500 | 118 | 111 | Slider 40, Switch 51, Label 17, Menu 10 |
| Pacific Ensemble Strings | 633×190 | 58 | 38 | ValueEdit 37, Switch 7, Knob 5, Slider 3, Label 3, Menu 2, Button 1 |
| Performance Samples Vista | 633×180 | 48 | 25 | Label 13, Slider 7, Switch 11, Knob 4, ValueEdit 9, Button 2, Menu 2 |

## Ranking by how many libraries use each control (performance view)

1. `ui_slider`, 10/10 libraries (1 to 480 per library). Morphology and Shreddage are slider-heavy.
2. `ui_label`, 10/10 (3 to 437). Labels carry most authored text, so text placement is the largest fidelity surface.
3. `ui_switch`, 9/10 (7 to 294). Missing only from Morphology.
4. `ui_menu`, 9/10 (1 to 70). Popup menus.
5. `ui_value_edit`, 9/10 (2 to 124). Pacific is almost all value edits (37).
6. `ui_button`, 8/10 (1 to 238). Single-use buttons in the big sets (ANALOG 238, Morphology 67).
7. `ui_knob`, 3/10 (Conflux 200, Pacific 5, Vista 4). Almost all Conflux controls are knobs.
8. `ui_table`, 3/10 (Shreddage 25, ANALOG 24, Conflux 6).
9. `ui_panel`, 3/10 (Shreddage 280, Morphology 88, Conflux 46).
10. `ui_level_meter`, 3/10 (Morphology 16, ANALOG 4, Conflux 6).
11. `ui_text_edit`, 4/10 (Areia 5, Chorus 4, Dolce 4, Conflux 6, ANALOG 2).
12. `ui_xy` (Morphology 1), `ui_wavetable` (Morphology 1), `ui_file_selector` (ANALOG 1): one use each.
Not used by any library here: `ui_waveform`, `ui_mouse_area`.

## Observations that shape the renderer work

- Hidden widgets dominate some libraries. Areia, Dolce and Chorus hide 552 to 826 of their 572 to 847 widgets (articulation-set swaps). Any per-frame cost that scales with all widgets rather than visible ones is paid by these three.
- Pacific and Vista have short pages (190 and 180 px). Their controls sit in strips, so the page height is the authored clip.
- Conflux is the only native NCKP face in this set; all others are classic KSP `ui_*` faces.
- The Scripts-tab and Interface paths for Areia, Chorus and Dolce failed with `Layout(BudgetExceeded)` in the parity run before this census. That is the same stale-binary issue as the Areia Scripts crash: the census was rerun with the fixed binary, and all three now complete.

## Method and limits

- Command: `KONTRA_PARITY_PATH=<nki> KONTRA_PARITY_OUT=<dir> cargo test --features shots -- --ignored ui::widget_gate::parity_run::parity_run` (one instrument at a time, nice 19, 6 GB cap). `census.json` is written beside the screenshots (`src/ui/widget_gate/parity_run.rs`, `census()`).
- Only the first 24 controls were exercised for behaviour; the census itself reads every decoded widget of every face.
- Raw KSP source is not dumped. A per-property census over `$CONTROL_PAR_*` keys needs the `sampler-ksp` model (`Widget.properties`), which is a follow-up.
