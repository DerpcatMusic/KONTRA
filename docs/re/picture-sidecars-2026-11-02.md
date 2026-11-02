# Picture sidecars: a field ends where the next key starts

Written 2026-11-02, porting `feat/integration`'s `crates/app/host/src/ui/picture.rs`
onto v3. `STATUS.md` is the page of truth for what is true now; this file is the
record of what was wrong, why, and what holds it.

## The defect

A Kontakt picture's `.txt` sidecar is not always one field per line. XPERIMENTA
Harpsichord's `Resources/pictures/micsfader.txt` runs two fields together, with no
line break between them:

```
Number of Animations: 127Horizontal Animation:  no
```

Kontakt's reader ends a value where the next key begins, so it reads `127` and
stacks the frames **down** (`Horizontal Animation: no`). v3's reader split each
line at its first `:`, so the value was `127Horizontal Animation:  no`, the frame
count was `0` (clamped to 1), and the axis stayed horizontal.

The consequence is visible rather than subtle. The fader strip is 111×6096: 127
frames of 111×48. Read as one frame, the control takes the whole strip as its
picture size, and all five mic levels draw the identical 111×6096 sheet — five
different mics, five identical pictures, and nothing that moves when a fader
moves.

Two smaller rules came with the same parser:

- **A number is the value's leading digits.** `127Horizontal…` is 127, not an
  error and not 0.
- **A flag is yes when the value starts with `yes`.** Electric Mint's
  `dropdown_topright.txt` ships the typo `Horizontal Resizable: yesm`, and Kontakt
  stretches that picture. Read as `no`, the menu keeps the picture's 32 px width
  instead of the 171 px the script asked for, and the arrow lands under the field
  beside it.

## The rule now

`picture::fields()` finds every one of Kontakt's nine keys in the text
(case-insensitively, wherever it appears), sorts them by position, and gives each
field the text from its own `:` to the start of the next key — or to the end of
the text for the last one. `picture::number()` takes the leading digits;
`meta()` reads a flag with `starts_with("yes")`.

A field also ends at the next key when the next line is *not* one of Kontakt's:
reading `Fixed Right: 0\nHeight: 1` must not take `1` as the margin.

## Evidence

The library that carries the two malformed sidecars is not installed on the
machine this was ported on, so the fix is held by its unit tests rather than by a
re-measurement here (`crates/app/host/src/ui/picture.rs`):

| test | what it pins |
|---|---|
| `a_sidecar_field_that_runs_into_the_next_one_keeps_its_number` | the real `micsfader` text: 127 frames, vertical axis, margins from the fields after it |
| `a_field_reads_its_own_number_and_not_the_next_line_s` | a run of fields where a foreign line follows one of ours; `025` keeps its value |
| `a_sidecar_flag_with_trailing_junk_is_still_yes` | `yesm` stretches, `no` does not |

Source: `feat/integration` `8c64fccd0` ("ui: read a picture sidecar's field even
when it runs into the next key"), ported whole — v3's `picture.rs` had the
line-by-line reader.
