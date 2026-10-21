# What the reference's UI code says

Read from the restored soothe3 image (see `smooth-core/research`), by string
and RTTI inventory; no host, no yabridge. Nothing here is copied code: it is
the facts the code states, which the interface is built to.

## Framework

`kali` -- the vendor's own UI layer over JUCE. RTTI classes: `UserInterface`,
`Editor`, `Component`, `Knob`, `TextSlider` (with `Arrows`), `Toggle`,
`ToggleWithIcon`, `ChoiceMenu`, `LockableControl<Toggle|ChoiceMenu>` (the
padlocks), `ABSwitchButton`, `ABCopyButton`, `FontIconButton`,
`SvgIconButton`, `TextButton`, `TextInput`, `TextArea`, `TextLabel`,
`ScrollView`, `VectorGraphics`, and a preset system: `PresetMenu`,
`PresetMenuDialog`, `PresetListRow`, `CategoryButton`, `CategoryColumn`,
`PresetActionsBar`, `PresetContextMenu`, `PresetSaveDialog`,
`PresetEditDialog`, `PresetConfirmationDialog`.

## Renderer

Own GPU renderer (Metal/D3D, one shader source) with programs: reduction
curve, outline, shape, texture, gradient, render target.

- **Reduction curve**: up to 1024 samples (`float4 ys[256]`) as a polyline
  over `x_min..x_max`; linear interpolation between samples; the fill is a
  vertical gradient `top_colour -> bottom_colour`; the edge is a 1 px
  smoothstep on the signed distance to the segment (anti-aliased, no halo).
- **Outline**: a stroke built from a 1-D lookup of points; the offset at each
  vertex is the miter of the two neighbouring segments at distance
  `thickness`; two edges (`+1`/`-1`) with a `feather` fade at the rim.
- **Texture**: `bw_amount`, `brightness`, optional mask (used to dim/grey).

## Theme tokens the reference names

root foreground; button fg / bg / (toggled) fg / bg; button corner radius;
toggle fg/bg; text input bg / active / editing / foreground / caret; text
slider arrow and side-arrow colour; slider groove / indicator / handle;
scrollbar; preset menu dialog bg / backdrop / secondary text; preset list
row accent (and selected); favourite colour. Fonts: *Cofo* regular, medium.

## Structure the reference has that SMOOTH does not yet

- The rail's three views: **stereo** (mode L/R|M/S, link, focus), **tilt**
  (detail / attack / release, low below 500 Hz and high above 2 kHz), and
  **scale** (the reduction graph's display range).
- A per-band panel: shape, frequency, depth, bandwidth, slope, stereo
  balance, channel set, listen-to-delta, bypass, delete; up to eight bands.
- Max cut, wet trim, quality (normal/ultra, minimum/linear phase), lock
  padlocks, "indicators show if settings differ from default".
- Sidechain (enable, listen, level meter), LFE/surround/ceiling channel sets
  for multichannel, "link all" groups.
- The preset menu (search, categories, creator/favourite filters, save, edit,
  delete, default pin, user folder), undo/redo, A/B and copy.
- A side panel that hides (double-click its background) and a display-size
  control. The tooltip text for all of it is in `reference-ui-text.txt`.
