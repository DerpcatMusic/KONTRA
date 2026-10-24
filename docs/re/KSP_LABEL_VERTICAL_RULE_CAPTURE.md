# Capture plan: the vertical rule for script-placed labels

Status: open. Not a fix. The caption-vs-glyph offset is not shipped because one reference
(Areia) is not enough to set the rule. This page says what to capture so a future lane can settle it.

## The defect

Areia (`01 Areia - 16 Violins - Core Techniques.nki`) and Dolce (`Dolce - 01 7 1st Violins - Legato.nki`)
place a `ui_switch` (radio glyph) and a `ui_label` caption with the same script position (`POS_Y` 381) in
the performance view. At 1:1 (editor 1100 px wide, face exactly 800 px, `KONTRA_SHOT_SIZES=1100x700`):

| Quantity (1:1 px) | KONTRA render (Areia Sordino) | Kontakt reference (Areia Niente, scaled back by ~1.43) |
|---|---|---|
| Glyph size | 11 px | 16 px / 1.43 ≈ 11 px |
| Caption centre minus glyph centre | about +5.5 px (caption lower) | about 0 (aligned) |
| Glyph right edge to caption left edge | about 2 px | about 7 px |

The scale factor 1.43 comes from text width: "2nd Violins" is 77 px in the Kontakt capture and 54 px in
ours. The Kontakt capture is not at 1:1.

Dolce's Niente pair is already aligned within 1 px in KONTRA (label and glyph both 18 px tall).

## Where the numbers come from in the code

- Glyph size: `picture_meta_png` gives the per-frame header size (11×11), and `resolved()` takes it on
  axes the picture does not stretch (`src/ui/ir_view.rs`, the picture-size rule; pinned by
  `picture_switch_without_authored_size_takes_the_picture_size`).
- Caption placement: `render_art::words` centres the text in the label's height when `top` (TEXTPOS_Y) is
  unset (`src/ui/render_art.rs`, `top.map_or(((h - tall) / 2.).max(0.), ...)`). The Areia label is
  50×22 (`$CONTROL_PAR_WIDTH` 50, `HEIGHT` 22, script lines 6828–6829).
- Horizontal inset: the label is placed at `POS_X` = switch `POS_X` + 10 (script line 6826), with
  `pad((2 * scale, 0))` inside `words`.

Two hypotheses explain the offset, and only a second reference can tell them apart:

- H1: Kontakt does not centre label text in the label box. It sets the text line near the top of the box,
  so the caption sits at the glyph's level.
- H2: Kontakt centres the glyph in the authored box instead of drawing it at `POS_Y`, so the glyph and
  caption centres coincide.

## What to capture (one Kontakt screenshot each, at a known Kontakt scale)

1. **Scale.** Kontakt's UI scale, from a known width. Measure a long caption in Kontakt and in KONTRA
   (for example "2nd Violins" in Areia's Sordino row), or set Kontakt's UI scale to 100% if the option
   exists. Record the factor.
2. **Label with a stock box.** A `ui_label` with no `HEIGHT`, placed at a known `POS_Y`, next to a
   `ui_switch` with a picture. Dolce's `$label_niente` has no `HEIGHT` set (check the script). Capture its
   caption and glyph. This separates the two hypotheses.
3. **Label with `HEIGHT` 22 and an 11 px glyph** (Areia Sordino). Capture again, at the same scale.
4. **Horizontal gap.** The glyph right edge to caption left edge, in the same capture.
5. **A caption with `TEXT_ALIGNMENT` 0 and one with 1.** Areia uses 0 for both labels; capture a 1 if any
   library has one (grep the NKI scripts for `TEXT_ALIGNMENT`).

Record for each: glyph top, glyph height, caption cap top and cap bottom, all in px at the recorded scale,
then convert to 1:1.

## Decision rule

- If H1 holds on the stock-box label (caption top-aligned at `POS_Y` + a constant inset), change
  `render_art::words` for labels only, with the inset as a named constant.
- If H2 holds, centre the picture glyph in the authored box instead of drawing it at `POS_Y`.
- If both hold, do H2 first; it is the smaller change.

Each change needs a shots test that compares caption and glyph centres within 1 px at 1:1, and a second
library's capture (Dolce) must still match.

## Tooling already in the repo

- `performance_view_shot` (ignored; `KONTRA_SHOT_NKI`, `KONTRA_SHOT_OUT`, `KONTRA_SHOT_SIZES`) writes the
  Interface tab at the given sizes.
- `picture_switch_without_authored_size_takes_the_picture_size` pins the picture-size rule.
- Measurement method: ink extents against the local median, per glyph and per caption box, at 1:1. The
  measurements in this note were made that way on `1100x700` captures.

## Not covered

- The outlined box behind the articulation-list dots (Areia list rows). The switch style flags have not been
  checked against Kontakt.
- Overlap and clipping seen in the 900 px Areia capture (grey box over the BASIC row, list running into the
  bottom switch row). Not investigated.
