# UVIScript UI: every widget, property and function

Source: the UVI script reference, <https://lua.uvi.net/group___u_i.html> and one page per class (read 2026-11-14). The probe `crates/script/lua/tests/uvi_ui/probe.lua` checks each member below in one line (`Class.member: ok|FAIL`); `probe.expected` is what KONTRA answers, and `crates/script/lua/tests/uvi_ui.rs` fails when it changes. The same file runs in UVI Workstation (print goes to its console): diff the two when the user's iLok works there. Until then the reference text and the binary are the oracle.

Two columns of status per row.

- **Script**: the script surface (`crates/script/lua/src/prelude.lua`, `widgets.lua`): the member exists, stores what is set, and does what the reference says. Every row marked implemented is a probe line that reads `ok`.
- **Editor**: the editor draws or honours it (`crates/app/face/src/lua.rs`, `crates/script/lua/src/ui.rs`). Every property a script sets reaches the editor in `Widget::props` by name; "draws" means a renderer reads it.

The Editor column comes from reading what `face/src/lua.rs` and `ui.rs` carry to the editor, not from rendering each row; check a row on screen before relying on it.

Status words: **implemented**, **partial** (what is missing is named), **missing**.

## Page functions

| Function | Script | Editor |
|---|---|---|
| `setBackground`, `setBackgroundColour`, `setSize`, `makePerformanceView` | implemented | draws |
| `setHeight` | implemented | draws (moves the page height) |
| `moveControl(widget, col, row)` | partial: stores `gridColumn`/`gridRow` (1-based) | missing: the automatic grid's cell size is not known; a widget without bounds is placed by order |
| `setKeyColour`, `resetKeyColour` | partial: accepted, no effect | missing: the keyboard strip does not take colours |

## Enumerations

| Class | Script | Editor |
|---|---|---|
| `Mapper` (10 values, reference order) | implemented, stored as `mapper`, default `Linear` | missing: a widget's drag law is linear; the formulas are in the reference (exponential is `min·(max/min)^pos`, the roots and powers `pos^(1/n)`, `pos^n`) |
| `Unit` (14 values, reference order) | implemented, default `Generic` | partial: the symbol (`%`, `s`, `ms`, `Hz`, `dB`, `MB`, `st`, `ct`) shows on a knob; the unit's own formatting (`s` to `ms` below 1 s, `Hz` to `kHz`, `Cents` to semitones above 100, `MidiKey` as a note name, `UviFilter`, `Pan`, `PercentNormalized` ×100) is missing |
| `RectanglePlacement` (12 values, JUCE flags) | implemented | missing: pictures always fill their bounds |
| `FileFormat` (`All`, `Audio`, `MIDI`) | implemented | missing with `DnDArea` |
| `ModifierKeys` (`altDown`, `commandDown`, `shiftDown`) | implemented: the second argument of a button's `changed`; nil when a script moved it | partial: the editor reports no modifier keys (all false) |

## Widget (base class, every widget inherits it)

| Member | Script | Editor |
|---|---|---|
| `name`, `x`, `y`, `width`, `height`, `size`, `position`, `pos`, `bounds`, `w`, `h` | implemented | draws |
| `alpha`, `enabled`, `visible` | implemented | draws |
| `tooltip` (default the name), `displayName` (default the name) | implemented | draws tooltip; `displayName` partial (not drawn as a label) |
| `displayText` | implemented (stored) | missing: replaces the value readout |
| `interceptsMouseClicks` | implemented (stored) | missing: every widget takes clicks |
| `showLabel` | implemented (stored) | missing |
| `persistent` | implemented: saved and restored, `changed` runs on reload | n/a |
| `changed(self)` | implemented | n/a |

## ParamWidget (Knob, Slider, OnOffButton, Menu, MultiStateButton, NumBox)

| Member | Script | Editor |
|---|---|---|
| `mapper` | implemented | missing (see `Mapper`) |
| `unit` | implemented | partial (see `Unit`) |
| `paramId` | implemented: assigned in the order widgets are made | n/a |
| `exported` | implemented (stored) | missing: no host automation parameter yet |

## Knob, Slider, NumBox

| Member | Script | Editor |
|---|---|---|
| Constructors (`Knob(name, def, min, max, integer)`, `Slider(..., integer, vertical)`, `NumBox`) and table forms | implemented | draws |
| `setValue(v, call=true)`, `value`, `default`, `min`, `max`, `setRange` | implemented | draws |
| `setStripImage`, `stripImage` | implemented | draws |
| `setLottieAnimation`, `lottieAnimation` | implemented (stored) | missing: no Lottie player |
| Knob: `showPopupDisplay`, `showValue`, `fillColour`, `outlineColour`; Slider: `backgroundColour`, `thumbColour`, `trackColour`, `fillColour`, `backgroundImage`, `handleImage`, `showPopupDisplay`; NumBox: `showPopupDisplay`, `fontSize`, `font`, `align`, `minimumHorizontalScale`, `hitTestOnlyOnText`, `backgroundImage`, `backgroundColour`, `textColour` | implemented (stored) | partial: fill, background, text colours, handle and background pictures, font, size and alignment draw; popup, `showValue`, `minimumHorizontalScale`, `hitTestOnlyOnText`, thumb and track colours missing |

## Button, OnOffButton, MultiStateButton

| Member | Script | Editor |
|---|---|---|
| `Button(name)`, `push(call)` | implemented: `push` runs `changed(self, nil)`, silent for `false` | draws |
| `OnOffButton(name, def)`, `setValue(v, call=true)`, `value`, `default` | implemented | draws |
| `changed(self, mods)` | implemented | partial (see `ModifierKeys`) |
| `normalImage`, `pressedImage`, `overImage`, `overPressedImage` | implemented | partial: normal and pressed draw; hover pictures missing |
| `setStripImage`, `stripImage` | implemented | partial: the strip frames off/on draw; hover and held frames missing |
| `setLottieAnimation(path, mode)` | implemented: `mode` is required (an error without it), `"trigger"` else toggle | missing |
| `backgroundColourOff/On`, `textColourOff/On`, `fontSize`, `font`, `triggeredOnMouseDown`, `dragAndDropFilepath`, `dragAndDropText` | implemented (stored) | partial: text colour, font and size draw; on/off colours, press timing and drag-and-drop missing |
| MultiStateButton: constructor with entries, `addItem` (returns the index), `setItem`, `getText`, `clear`, `length`, `value`, `selected`, `selectedText`, `text`, `default`, `showPopupDisplay`, `backgroundImage`, `backgroundColour`, `textColour` | implemented | draws as a cycling menu (`menu_cycle`) |

## Menu

| Member | Script | Editor |
|---|---|---|
| `Menu(name, items, def)`, `addItem`, `setItem`, `getText`, `clear`, `setValue`, `length`, `value`, `selected`, `selectedText`, `text`, `items`, `default` | implemented | draws |
| `hierarchical`, `displayArrow`, `menuHeight`, `popupFont`, `popupFontSize`, `popup*Colour`, `outlineColour`, `arrowColour`, `align`, `fontSize`, `font`, `minimumHorizontalScale`, `backgroundImage`, `backgroundColour`, `textColour`, `showPopupDisplay` | implemented (stored) | partial: background, text, font, size, alignment draw; sub-menus from `a/b` entries, popup colours and fonts, arrow, height missing |

## Label

| Member | Script | Editor |
|---|---|---|
| `Label(name)`, `text`, `align`, `fontSize`, `font`, `backgroundColour`, `textColour` | implemented | draws |
| `editable`, `backgroundColourWhenEditing`, `textColourWhenEditing`, `minimumHorizontalScale`, `url` | implemented (stored) | missing: a label is not editable, squeezed or a link |
| `changed` on user edit only | implemented: setting `text` runs nothing | missing with `editable` |

## Table

| Member | Script | Editor |
|---|---|---|
| `Table(name, size, def, min, max, integer)`, `getValue`, `setValue(i, v, call=true)`, `setRange`, `min`, `max`, `default` | implemented: an index outside 1..size reads the widget's own value and drops writes (UVIWorkstation 4.0.9, 0x1410c0140, 0x1410de650) | draws |
| `length` (read, assign to resize) | implemented: new entries take `default` | n/a |
| `changed(self, index)` on `setValue` | implemented (the reference: fires when the last argument is true) | n/a |
| `backgroundColour`, `sliderColour`, `fillStyle`, `drawInnerEdge`, `innerEdgeColour`, `showPopupDisplay` | implemented (stored) | partial: colours, fill style and inner edge draw; `innerEdgeColour` and the popup missing |

## Image, SVG, DnDArea

| Member | Script | Editor |
|---|---|---|
| `Image(path)`, `image`, `overImage`, `rectanglePlacement` | implemented | partial: draws `image`; hover and placement missing |
| `SVG(path)`, `svg`, `rectanglePlacement` | implemented | missing: no SVG renderer in the widget path (the native-UI asset preparer rejects text, images, masks and filters) |
| `DnDArea`: `acceptedFileFormat`, `filepath`, `image`, `droppingImage`, `backgroundColour`, `droppingColour`, `acceptedText`, `droppedText`, `fileDropped`, `textDropped` | partial: stored; no drop ever arrives | missing: no drop target |

## Panel, Viewport

| Member | Script | Editor |
|---|---|---|
| `Panel(name)`, `panel:Panel(name)`, children made as methods, `backgroundImage`, `backgroundColour` | implemented | draws |
| `Viewport(name)`, `setViewPosition(x, y)`, `scrollBarThickness`, `scrollBarThumbColour`, `scrollBarTrackColour`, `scrollBarBackgroundColour` | implemented (stored) | partial: clips and scrolls; scroll bar colours missing |

## WaveView, XY, AudioMeter, FileSelector

| Member | Script | Editor |
|---|---|---|
| `WaveView(name)`, `bind(oscillator)`, `sample`, `playbackCursorColour`, `hiWaveColour`, `lowWaveColour`, `showPeak`, `showRms`, `axisColour`, `channelSeparatorColour`, `sampleNameColour`, `sampleNameBackgroundColour` | implemented (stored) | missing: draws an empty panel (the waveform needs the decoded sample and the playback position) |
| `XY(paramX, paramY)`, `handleImage`, `backgroundColour`, `handleColour`, `canOnlyDragHandle` | implemented (stored) | missing: draws an empty panel |
| `AudioMeter(name, element, isVertical, busId, useOutput)`, `setStripImage`, `setLottieAnimation`, colours incl. `["0dBColour"]` to `["10dBColour"]`, `displayScale`, `displayAsMono` | implemented (stored) | partial: draws the part's output level, not the named element or bus |
| `FileSelector(name)`, `prev()`, `next()`, `basepath`, `filepath`, `filetype`, colours, `fillStyle`, `fontSize`, `font`, `scrollBarThickness`, `columnWidth`, `rowHeight`, `displayExtension`, `allowDnD`, `invertColourOnSelection`, `pathSelected`, `pathActivated` | partial: stored; `prev`/`next` do nothing and no path is ever selected | missing: no file list |

## Widgets bound to a parameter

| Member | Script | Editor |
|---|---|---|
| `ParamKnob`, `ParamSlider`, `ParamOnOffButton`, `ParamMenu`, `ParamNumBox`: `(element, name)` and `(name)` constructors, `bind` (errors on a missing element or parameter, a no-op on the same target), `value` mirrors the parameter (nil unbound) and is read-only, never runs `changed` | implemented | partial: drawn as the plain widget at the parameter's value; the value does not follow later edits live, and a menu has no entries |
| `setRange`, `setStripImage`, `setLottieAnimation`, `setNormalImage`, `setPressedImage`, `setOverImage`, `unit`, `default`, `horizontal`, colours, pictures, menu and number-box styling | implemented (stored) | partial (as their unbound classes) |
| `ParameterValue`: `(element, name)`, `(name)`, `bind`, `value` read and write, `bound`, `toString()`, `changed` on each parameter change and once when the interface is built | implemented | n/a (non-visual) |

The path form of `Element|string` (a string naming an element) is not resolved: `bind` with a string raises the missing-element error. Program elements are reachable by object, which is what the corpus scripts use.

## What this closed, and what is left

Closed in this change: the `Param*` family and `ParameterValue`, `Button:push`, `ModifierKeys`, `Table.length` and the Table `changed(self, index)`, `Knob`/`Slider`/`NumBox:setRange`, `Slider` vertical, `Image`/`SVG`/`XY` constructors, the `w`/`h`/`length`/`selected`/`displayName` aliases, `paramId`, the `Mapper`, `Unit`, `RectanglePlacement` and `FileFormat` enumerations, `setHeight`, `moveControl`, `resetKeyColour`, `MultiStateButton` as an item widget, `Menu:addItem`'s return, and every other documented attribute reading back what a script set.

Left, all on the editor side except where marked: the `Mapper` drag laws, the `Unit` formatting, `displayText`, `interceptsMouseClicks`, picture placement, Lottie, SVG, `WaveView`, `XY`, `FileSelector`, drag and drop (editor and engine), editable and linked labels, sub-menus, popup styling, hover pictures, live following of bound widgets, the `Element|string` path form, and host automation of `exported` widgets. Each is a renderer or host feature, listed so the next change picks them by number of corpus scripts that use them.
