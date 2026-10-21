# Look audit: SMOOTH vs the soothe3 reference

Target: soothe3's layout, control styling and graph treatment, painted in the
**graphite** palette (white + graphite; `theme::GRAPHITE`, the default). The
reference is the Voice (blue) capture, so colours are read as roles, not hues.

Evidence:
- Reference: `/tmp/opencode/soothe3-ui/soothe3.DiyS39KP_Z1gNtUl.png` (full, 2000 px
  wide = 2.5x the 800x473 logical editor), plus crops `03_side_panel`,
  `06_band_controls`, `07_node_controls`, `08_footer` in the same folder.
- Ours: `smooth-preview` at 1x, `/data/dev/workspace/ui-shots/look-base-m.png`
  (baseline, before any edit).
- Note: `/mnt/Windows11/DEV_WORKSPACE` is not mounted on this machine, so the
  unpacer sweep index was not consulted for this pass. Items that need it are
  marked *RE-open*.

## Defects

| # | Area | Defect | Evidence | Status |
|---|------|--------|----------|--------|
| 1 | Knob | Value arc is `mark` (white) drawn over a light ring track, so the arc disappears in graphite. The reference's arc is the high-contrast colour against its track. | baseline `depth` knob: arc not visible; ref `depth` knob: white arc on blue track | fixed |
| 2 | Graph ticks | Frequency labels are 8.5 px in `ink_quiet`; reference labels are ~12 px in dark ink. | ref `100 250 500 1k…` at y≈93/1183; baseline ticks are tiny grey | fixed |
| 3 | Graph level | `-3`, `-6` level labels are 8 px; reference ~12 px, dark. | ref `-3` at x≈1975, `-6` at x≈1975 | fixed |
| 4 | Range chip | `-9 ▾` chip text is 8 px. Reference reads at body size. | ref bottom-right chip | fixed |
| 5 | Side panel | Section titles (`stereo`, `tilt`, `scale`) are plain text. Reference sets each as `——title——` with hairline rules either side, in accent ink. | `03_side_panel` | fixed |
| 5b | Side panel | Rows were label-left / value-right. Reference stacks each name centred over a full-width plate, and pairs the low/high tilt plates under one name. Stereo switch reads `L/R`, `M/S`. | `03_side_panel` | fixed |
| 18 | Preset bar | An empty outline box is drawn when the preset has no category (the `init` preset). Reads as a control. | baseline header | fixed |
| 19 | Tests | `marks_come_from_the_icon_pack` flagged the `"M/S"` label as path data. Tightened to match real path data (`M` + number). | `cargo test -p smooth-ui` | fixed (test) |
| 6 | Rail | Rail words are 9 px; reference rail words are larger and read clearly beside the dots. | ref rail `stereo`/`tilt`/`scale` vs baseline | fixed (10 px) |
| 7 | Band bar | `x` delete is a text glyph; reference uses a trash icon in a disc at the bar's right end. | `06_band_controls`, `07_node_controls` | fixed (trash glyph, hand-drawn path) |
| 8 | Band bar | Band bar has no coloured band rail at its left and right ends. Reference has a band-colour bar on each side inside a white-edged plate. | `06_band_controls` | fixed (band-tint edge rails) |
| 9 | Header | No sidechain/accent mark left of the `sc` button. Reference has a short vertical accent bar. | ref top-left `|` at x≈418 | open |
| 10 | Band bar | Bypass is a text pill; reference uses a power-glyph disc (dark when on) beside the band index. | `07_node_controls` | open (needs power glyph) |
| 11 | Band bar | Shape is a text dropdown (`bell`); reference uses an 8-cell icon grid of shape glyphs with the chosen one in a dark square. | `06_band_controls` | open (larger refactor, shape glyphs) |
| 12 | Band bar | Band index badge (`2` in a disc) absent. | `06_band_controls` | open |
| 13 | Readouts | Value text is 11 px; reference values read ~12–13 px at the same plate size. | ref `5.0`, `100 %`, `0.0 dB` | open (needs `ty` ladder decision) |
| 14 | Wordmark | Reference wordmark is a heavier, rounder lockup with a light last glyph (`soothe3`) and a logo mark beside `oeksound`. Ours is the product name in ink. | ref top-left | open (identity; not a parity item) |
| 15 | Footer | Padlock icons are small next to labels; reference padlocks are the same size as the label cap height. | `08_footer` | open |
| 16 | Plot | The white-curve treatment is graphite here, which is correct for the palette; band-coloured own curve is drawn in a blue that does not belong to the graphite set. | baseline graph | open (needs palette decision) |
| 17 | Graph fill | Reduction fill is a flat grey block; reference fill is a light tint above the trace and a deeper tint under it. | ref graph; baseline with no signal | open (needs live-signal capture) |

## Not yet audited (in this pass)

- Live-signal captures (`--live`) for defects 17 and the decay of the fill.
- The `xs`..`xl` sizes: the preview is 1x only. The plug-in's window sizes are
  checked with `gui_shot.py` once the static fixes land.
- Tooltips and dialogs: the reference tooltip strings are in
  `research/reference-ui-text.txt`; the text is owned by `tips.rs`. Visual
  tooltip chrome is not in the reference captures, so it is not a parity item.
- Preset menu (`menus.rs`, `presets.rs`): not captured in the reference set.
