# KSP UI probe suite

Five probe instruments that use every KSP UI control type with a spread of control
parameters, self-made pictures and fonts. Each page logs what its init reads and every
`ui_control` / `ui_controls` / `persistence_changed` to an array it saves by path, so the
oracle Kontakt and KONTRA can be diffed on screen and on what scripts see.

| page | what it covers |
|---|---|
| widgets | every control type at its defaults; type and size reads |
| geometry | sizes, positions, `move_control(_px)`, hide states, Z layers, nested panels |
| pictures | skins and frames, picture states, nine-slice, wallpaper, icon, skin offset, mouse behaviours, knob unit/label/default |
| text | fonts 0–25, alignment, multi-line labels, text edit, value edit modes, colours, `set_ui_color` |
| data | menus (rename, values, visibility, selection), tables, xy pad, level meter, waveform, wavetable, file selector, mouse area, key modifiers |

## Run

```sh
python3 tools/ksp-ui/gen.py                       # probes/*.ksp, probes/*.drive, lib/Resources/pictures
# Kontakt (oracle slot): builds lib/Instruments/PAGE.nki, shoots, drives, keeps the trace.
tools/oracle/oracle-slot tools/ksp-ui/shoot.sh OUT/k-PAGE-150.png PAGE
tools/oracle/oracle-slot env DPI=96 NODRIVE=1 tools/ksp-ui/shoot.sh OUT/k-PAGE-100.png PAGE
# KONTRA: the standalone window under Xvfb, the same drive, its trace.
tools/ksp-ui/ours.sh path/to/kontra-window OUT PAGE...
python3 tools/ksp-ui/trace.py OUT/k-PAGE-150.nka OUT/o-PAGE.nka   # the rows that differ
```

`cargo test -p kontra-ksp --test ui_probes` checks that every probe compiles.

- `drive.py` replays `probes/PAGE.drive` (click, double-click, drag, modifier drag, keys, typing)
  in page pixels. It finds the page from its background, or from `ORIGIN=x,y,scale`; Kontakt's is
  fixed and `shoot.sh` passes it.
- `trace.py` matches init reads by name. It compares the callbacks as runs of one control,
  ended by their last value: each host sends a different number of motion events per drag.
- Shots at 150 % and 100 % feed the UI lane's `sheet.py` / `drift.py`.

## Kontakt laws the suite found

- `on ui_control` of a `ui_label` or `ui_level_meter` does not compile; KONTRA accepts it.
- Every control type reads 87 × 18 for `$CONTROL_PAR_WIDTH`/`_HEIGHT` until its script sets them,
  a knob too, and `$CONTROL_PAR_TYPE` reads its `$NI_CONTROL_TYPE_*`.
- After `move_control(x, y)`, `$CONTROL_PAR_POS_X`/`_Y` read `(x−1)·92` and `(y−1)·21`, and (0, 0)
  reads −1, −1. It draws 66 and 2 px further in.
- A ctrl-press resets a control to its default, and a drag goes on from there.
- A file selector on a large folder stalls Kontakt's editor; the probe lists a small one.
- A wallpaper is drawn from the top of the instrument header, not from the page.
- `get_font_id(25)` answers 6789. Scripts hand it back to `FONT_TYPE`, which takes either.
