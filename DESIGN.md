---
name: Kontakto
description: A native multisampler editor with a compact graphite rack and local library artwork.
colors:
  graphite-background: "#1b1b1b"
  graphite-surface: "#252525"
  graphite-raised: "#303030"
  graphite-field: "#101010"
  selection-neutral: "#474747"
  text-primary: "#e8e8e8"
  text-muted: "#808080"
  key-black: "#0d0d0d"
  key-white: "#ebebeb"
  status-error: "#ff837b"
typography:
  title:
    fontFamily: "Noto Sans, sans-serif"
    fontSize: "24px"
  body:
    fontFamily: "Noto Sans, sans-serif"
    fontSize: "13px"
  control:
    fontFamily: "Noto Sans, sans-serif"
    fontSize: "12px"
  label:
    fontFamily: "Noto Sans, sans-serif"
    fontSize: "11px"
rounded:
  square: "0px"
  control: "1px"
spacing:
  xs: "4px"
  sm: "8px"
  md: "12px"
components:
  navigation-current:
    backgroundColor: "{colors.selection-neutral}"
    textColor: "{colors.text-primary}"
    typography: "{typography.control}"
    rounded: "{rounded.control}"
  button-ghost:
    backgroundColor: "transparent"
    textColor: "{colors.text-primary}"
    typography: "{typography.control}"
    rounded: "{rounded.control}"
  search-field:
    backgroundColor: "{colors.graphite-field}"
    textColor: "{colors.text-primary}"
    typography: "{typography.body}"
    rounded: "{rounded.square}"
    height: "28px"
  rack-part-row:
    backgroundColor: "{colors.graphite-surface}"
    textColor: "{colors.text-primary}"
    rounded: "{rounded.square}"
  keyboard-white:
    backgroundColor: "{colors.key-white}"
    rounded: "{rounded.square}"
  keyboard-black:
    backgroundColor: "{colors.key-black}"
    rounded: "{rounded.square}"
  load-error:
    textColor: "{colors.status-error}"
    typography: "{typography.control}"
---

# Design System: Kontakto

## Overview

**Creative North Star: "The Graphite Rack"**

Kontakto is a dense, quiet instrument workstation: a dark continuous work area, a multi-instrument rack, a separate library browser, and a light piano bed. Neutral grays carry selection and panel hierarchy; color comes from the user's own library artwork or a real compatibility error.

One bundled sans face keeps instrument names and diagnostics direct and readable. Rack, Mapping, Groups, Info, and folder settings share the same shell, so browsing and inspection do not replace the instruments being assembled. Compatibility limits remain visible in context.

**Key Characteristics:**
- Flat graphite surfaces with thin rules and compact controls.
- A persistent rack beside separate artwork and preset lists.
- Neutral piano keys; local library artwork is the only decorative image.

## Colors

The palette stays achromatic through normal interaction; the single warm status color signals an actual loading or sample failure.

### Primary
- **Graphite Selection:** neutral gray marks the active view and other selected controls without introducing a brand accent.

### Neutral
- **Graphite Background, Surface, Raised, and Field:** stacked fills distinguish the work area, rack rows, toolbar, and editable controls.
- **Primary and Muted Text:** light text carries names and values; subdued gray is reserved for counts, labels, and secondary guidance.
- **Piano Keys:** near-black and near-white keys provide the instrument's strongest value contrast.

### Status
- **Compatibility Coral:** reserved for actual load and missing-sample errors; it is not a selection or decoration color.

**The Neutral Selection Rule.** Keep active states in the graphite family; use coral only when the instrument reports a real error.

**The No Orange Rule.** Nothing is drawn orange, amber or mustard (OKLCH hues 40–105): focus, tabs and selection are light grey; part and bus hues walk the circle with that band cut out; a library whose artwork is orange gets no library color, and its curves take the part's color.

## Typography

**Display Font:** Noto Sans (sans-serif fallback)  
**Body Font:** Noto Sans (sans-serif fallback)  
**Label/Mono Font:** Noto Sans; the interface has no separate mono face.

**Character:** A single plainspoken sans face keeps dense names, controls, and diagnostics easy to scan. Noto Sans is bundled with its OFL notice in `assets/OFL.txt`.

### Hierarchy
- **Title** (24px theme role): reserved for MUI title-scale text.
- **Body** (13px): instrument names, messages, and everyday content.
- **Label** (11px): counts, status metadata, and secondary prompts.
- **Control** (12px): buttons and the inline compatibility error.
- The wordmark (16px) uses a local size override rather than another font family.

**The Bundled Face Rule.** Use the bundled Noto Sans face across the editor; do not add a display or monospace family.

## Layout

The native editor opens at 1180 by 780 logical pixels and reflows at fixed zoom from a 900 by 640 minimum. A toolbar spans the top. The main row keeps the growing, scrollable rack on the left and a separate Libraries / Instruments browser on the right; the browser takes 30% of the width, with a 270px minimum and 360px maximum. Library artwork keeps its source aspect ratio, and the preset list stays separate beneath it.

The keyboard and host I/O status occupy stable bottom strips. Mapping, Groups, Info, and folder settings change the rack workspace while the browser and keyboard stay in place. Small repeated gaps and padding keep the layout dense; long instrument names wrap instead of forcing fixed-width rows.

Selecting a preset or rack name opens Instrument under the compact rack summary. A bounded init subset previews Vista Harp (47 controls) and Vista Cellos (48), preserving authored positions, labels, and hide state as disabled native glyphs over the named local wallpaper; it clips the top 68px for Kontakt's header, with Kontakto's title outside. Manual gain/pan/audition stay live above, with sample groups below; an on-screen notice says saved values and callbacks are unavailable, while original skins and KSP runtime are not reproduced. The resolved wallpaper comes only from its named local picture or kind-4 NKR resource and is never bundled.

The right browser separates Instruments (NKI) from Multis (NKM) and lists only whole preset files from the selected library. Instrument view carries that library breadcrumb with Previous/Next across the whole preset set; raw group lists stay in Groups. Loading an NKM expands embedded programs, including silent controller parts, into the rack; original multi routing, scripts, and resource behavior remain explicitly unsupported.

**The Persistent Rack Rule.** Keep the rack and the separate library/preset browser visible together as the user changes workspace views.

## Elevation & Depth

The editor uses flat fills rather than shadows. Background, surface, raised, and field roles distinguish regions; thin horizontal rules divide repeated rows and major bands. The review captures show no shadow vocabulary.

**The Flat Plane Rule.** Convey hierarchy with neutral fills and hairlines, not floating cards or decorative depth.

## Shapes

Panels, keyboard keys, and search fields are square. Action controls use only a hairline corner; the mapping canvas is also square in the rendered workspace. Clipping contains the library browser and scroll regions, while rows keep a continuous rectangular silhouette.

**The Square Edge Rule.** Keep large surfaces and keys square; reserve the tiny control radius for compact actions.

## Components

### Buttons and Navigation
- **Character:** compact, text-led controls sit directly in the toolbar and rack.
- **Current view:** a soft graphite fill distinguishes the selected Rack, Instrument, Mapping, Groups, or Info tab.
- **Ghost action:** unselected controls remain transparent with light text; mute, solo, remove, duplicate, group, and audition actions keep their labels visible.

### Inputs / Fields
- **Style:** the search and library-folder fields use a dark inset fill, square corners, and a short fixed height.
- **Behavior:** search filters both libraries and instruments; folder settings remain in the same editor shell.

### Rack Rows
- **Style:** each instrument is a flat row with a numbered name header and compact MIDI, output, level, pan, group, and transport controls.
- **State:** the selected part receives a raised neutral fill; load errors appear inline and retain the name of any still-playing instrument.

### Library Browser
- **Style:** library cards show local artwork and a wrapped name/count row; the independent instrument list uses compact text rows.
- **Artwork:** discover local `wallpaper.png` or embedded PNGs in NICNT/NKR files at runtime and contain them at their source aspect ratio. No artwork is bundled; review screenshots are not product assets.

### Keyboard
- **Style:** a stable four-octave bed uses near-white naturals and near-black sharps, with octave controls and a restrained range label.
- **Behavior:** mouse press and release produce note-on and note-off events.

### Compatibility Status
- **Style:** concise status remains in the rack footer; actual loading and missing-sample failures use the semantic error color. The Info view states unsupported behavior plainly.

## Do's and Don'ts

### Do:
- **Do** keep the multi-instrument rack, library artwork, and separate preset list visible in the same editor shell.
- **Do** use only locally discovered library artwork, preserve its aspect ratio, and leave it out of bundled product assets.
- **Do** keep names, counts, load state, and compatibility limits readable inside the dense layout.
- **Do** preserve the neutral keybed and bundled Noto Sans typography.

### Don't:
- **Don't** add navy/gold color, invented or bundled artwork, or ornamental meters to the editor.
- **Don't** replace the flat rack with nested rounded cards or large empty gaps.
- **Don't** imply full KSP, DFD, or Kontakt parity; the Instrument notice describes the preview's limits.
