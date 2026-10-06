# KONTRA v2 UI

The v2 editor design: screens, layout, components and interaction, and why. It
implements the UI section of [PRODUCT.md](PRODUCT.md), follows the design system in
[DESIGN.md](../../DESIGN.md) (graphite surfaces, Noto Sans, coral only for real
errors, no orange), and keeps what users liked in v1's UI: the persistent rack beside
the browser, the dense quiet surfaces, and library interfaces that actually load.

Status markers: **[built]** is in `src/ui` on `v2/ui`; **[next]** is designed here
and not built yet; **[core]** waits on data the Core does not expose yet (listed
at the end).

## What we took from other players, and why

| From | What | Why it fits |
|---|---|---|
| SINE Player | One persistent articulation list per instrument, each row showing its trigger (key, CC, program, channel). "Remap all" changes every trigger at once. | Articulations are the thing players switch most often. A list that stays put beats a script page that changes per library. "Remap all" is our articulation migration (PRODUCT: keyswitches to velocity, channel or CC). |
| SINE Player | Mixer: the instrument is a parent strip, and its mic channels expand under it. Each mic has its own output selector under its name and a load/unload toggle. | This is the nested tree the product asks for. Load/unload per mic is the biggest RAM lever in orchestral libraries. |
| SINE Player (pain) | Slow loads and an all-or-nothing purge. | Loads never block the UI, and purge acts per node. |
| KODA | The same gestures everywhere: drag for value, double-click to reset, wheel for fine. Separate Mapping, Structure and Mix views over one instrument. | One control language across our own UI and vector-mode library UIs. Views switch the workspace, never the rack. |
| HISE | The component list order is the z-order. Each component names its parent component. Filmstrips declare their frame count. Properties are plain data (JSON). | This is the UI IR's model: `Widget.parent`, `draw_order`, `ImageMeta.frames`. Plain data means one renderer serves KSP, performance views and Falcon Lua. |
| HISE | A routing matrix per module. | Rejected as the main view because it is too abstract for players. The tree with per-node output covers the same ground in place. |
| Kontakt 7/8 | A tag browser with favourites. Single click auditions, double click loads. "Batch re-save" and "Content Missing" dialogs. Batch output configuration. | Keep click-to-audition and double-click-to-load. Replace the dialogs with the load report, and the batch output configuration with automatic per-instrument pairs. |
| Kontakt (pain) | "Library not found" with no reason. Manual batch output setup. | Every failure has a reason and an impact line (see Load report). |
| Falcon / UVI Workstation | Program tree with Parts, Layers and Keygroups. Tag tiers with AND/OR. Auto-preview. 17 output pairs. | Keep the tag tiers and auto-preview. Reviewers find Falcon's deep tree confusing, so our tree shows only mixer nodes, folds by node, and always shows the instrument at its root. |

General pain points we design against: vague errors, blocking loads, all-or-nothing
purge, slow indexing, deep trees with no breadcrumb, features only reachable by
right-click, and confusing multi-output routing.

## Shell

```
┌ toolbar: wordmark · Rack | Mixer | Report | Logs · CPU/voices/RAM · settings ──────┐
├───────────────────────────────────────────────────────────────┬────────────────────┤
│ workspace (rack / mixer / report / logs)                      │ browser            │
│                                                               │ libraries (art)    │
│                                                               │ instruments/multis │
├───────────────────────────────────────────────────────────────┴────────────────────┤
│ keyboard (mapped keys, keyswitches) · MIDI/host status                             │
└────────────────────────────────────────────────────────────────────────────────────┘
```

- **The rack and browser are always visible (DESIGN: Persistent Rack Rule).** Tabs
  change only the workspace.
- **The tabs.** Rack is where instruments play and show their own interface. Mixer is
  the output tree. Report is the selected part's load report. Logs is the raw trace.
  v1's Mapping, Sound and Info tabs become views inside a part (below), not global
  tabs, because they are always about one instrument.
- **Keyboard.** Mapped keys come from `Decoded.keys`. Keyswitch keys
  (`Articulation.switch_keys`) are deep in the part's hue, and the playing
  articulation's key is lit. **[built]** Script key colours (`set_key_color`) wait for
  Core gap 6. **[core]**

## Browser

- **Library cards.** Each card shows the library's own artwork (wallpaper.png or the
  product art in its .nicnt or .nkr) and a name/count row. **[built, v1 carried over]**
- **Instruments and Multis.** These are separate lists. Click selects; double click,
  Enter or drag loads. **[built]** Auditioning on click (Kontakt, UVI auto-preview)
  plays a short phrase on the selected part's channel. **[next]**
- **Tag tiers with AND/OR** (from UVI). Tags come from NKS metadata and folder names.
  **[next]**
- **Loading never blocks the UI.** A loading part shows progress in its header and
  plays as soon as the plan is installed. Indexing runs in the background and the
  browser fills in as it goes.

## Rack and instrument view

Each part is one row: a header (number, name, MIDI port/channel, output pair, level,
pan, mute/solo, collapse), and below it the part's **stage**.

- **The stage shows the library's own interface when its scripts declare one**
  (`PartView.interfaces`, drawn by the UI IR renderer). **[built]**
  - Several scripts with UIs get a segmented "Script n" switch; the rack opens on the
    one with the most controls.
  - An **Original | Vector** switch picks the presentation. Next to it is a readout of
    the decoded picture memory the view holds, so the RAM effect of Vector is visible.
  - The view fits the part's width, scaled between 0.5 and 1.
- **Without a library interface, the stage shows the load summary** (format, zones,
  groups, samples, scripts) and the first untranslated features. **[built]**
- **Notices above the stage.** "Could not be loaded: reason", "N notes dropped",
  "N MIDI messages ignored". These are short and actionable. **[built]**
- **Instrument sub-views per part**, replacing the old global tabs: Interface (the
  stage), Articulations, Mapping, Sound (envelopes, filter, modulation as the core
  exposes them) and Info. They switch with a segmented control above the stage, and
  only views with content are offered. All of them read `PartView.instrument` (the
  translated `sampler_ir::Instrument`). **[built]** (`src/ui/inside.rs`)
  - Mapping: groups listed with zone counts, and a key × velocity map; clicking a
    group picks it out.
  - Sound: each distinct amplitude envelope (drawn, with A/D/S/R and the zones that
    use it), filters, and modulation routes. Read-only until the Core takes edits.
  - Info: file, format, contents, keys, untranslated count, and picture memory.
- **MIDI menu** (the input button in the part header): channel, port, **MPE** and
  **bend range** (as the instrument, ±2, ±12, ±24, ±48). These are stored as
  `Part.mpe` and `Part.bend_range`; the Core applies them (Core gap 7). **[built, core]**
- **Snapshots** wait for script control readback (Core gap 1). A snapshot without
  the script's values would restore only the mixer. **[core]**
- **Disk and RAM meters** wait for Core gap 8. **[core]**

### Articulations (SINE model) **[built, core]**

```
┌ Articulations ─────────── trigger: [Keyswitch ▾]  Remap all… ┐
│ ● Legato              C0        ⟂ velocity 1–127             │
│ ○ Staccato            C#0                                    │
│ ○ Pizzicato           D0                                     │
│ ○ Tremolo             D#0                                    │
└──────────────────────────────────────────────────────────────┘
```

- **One row per articulation.** Click to switch. The active row is filled graphite and
  follows incoming MIDI.
- **The trigger column shows how the row is reached.** Trigger types are Keyswitch, CC
  value, Program, MIDI channel and velocity range. "Remap all" converts every row at
  once (for example, "channels 1–8 from channel 1"). This is PRODUCT's articulation
  migration.
- **Stacking.** A row can be stacked with others (SINE poly stacks) on a velocity or CC
  split.
- **Core data needed:** articulation names, the source's trigger per articulation, and
  a setter (Core gap 3).

## Mixer: the nested tree **[built]**

`src/ui/mix_tree.rs` draws it; `src/ui/bridge.rs` maps `PartView.tree` (`MixTree`)
and `Part.nodes` (`NodeMix`) into it.

```
 ┌Cellos┐ ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━   ┌Harp──┐
 │ ▾ 4  │ ┌Close┐┌Tree─┐┌Room─┐┌Legato┐    │      │
 │  ◢   │ │ ◢   ││ ◢   ││ ◢   ││ ▸ 2  │    │  ◢   │
 │ ┃ ▌▌ │ │┃ ▌▌ ││┃ ▌▌ ││┃ ▌▌ ││┃ ▌▌  │    │ ┃ ▌▌ │
 │ S  M │ │S  M ││S  M ││S  M ││S  M  │    │ S  M │
 │▮ 1/2 │ │Up   ││Up   ││▮ 5/6││Up    │    │▮ 3/4 │
 └──────┘ └─────┘└─────┘└─────┘└──────┘    └──────┘
```

- **Hierarchy you can read without lines.**
  - Children sit to the right of their parent, under a 3 px bar in the parent's colour.
  - Each level is 2 × SPACE shorter than its parent.
  - All strips are bottom-aligned, so faders, S/M and outputs line up across depths
    while the bars stack above.
  - A 1 px hairline in the parent colour on each child's left edge keeps the tie
    visible when the bar scrolls away.
- **Colour.**
  - Each instrument's hue is a golden-angle step from 250° by rack index, with the
    orange band skipped (No Orange Rule).
  - Deeper nodes are darker shades of the same hue.
- **Fold and unfold per node.** The chevron in the header shows the child count, and
  folding hides the whole subtree.
- **Each strip has:**
  - pan wedge, fader with its meter, and dB readout;
  - solo and mute;
  - the insert count;
  - an output button with a colour chip: hollow for "Up", filled for a host pair.
- **Output picking happens in place.**
  - The output button swaps the fader for a list: Into parent, Host 1/2…16/17, and
    (on instruments) Automatic.
  - Picking marks the node manual; Automatic hands it back to `routing::apply`.
  - By default every instrument gets its own pair. "One per mic" (the Outputs
    menu in the mixer toolbar) also gives each top-level bus its own pair.
- **Solo follows the tree.** Ancestors and descendants of a soloed node stay audible
  (`tree::audible`).
- **The tree is the only mixer.** The flat Console is gone; the toolbar keeps Outputs
  and a master spectrum.
- **Mic load/unload per node** (SINE) is the purge control for RAM. It needs a
  per-node "resident" flag from the Core (Core gap 4). **[core]**

## Load report **[built]**

`src/ui/load_report.rs` draws it; `bridge::report` builds it from `LoadReport`,
`PartView.status` and live `PartShared::problems()`.

```
┌ Vista Cellos ─────────────── Plays incompletely · 3 problems ┐
│ NOT PLAYING AS AUTHORED                                       │
│ ● Script "Main" did not compile        line 412:9  unknown …  │
│   The script does not run: its controls, keyswitches and …    │
│ ● 340 samples missing                  Samples/Cello_C3_pp…   │
│ ◦ Effect not translated: Convolution   group 3 · size 1.2 …   │
│ WHILE PLAYING                                                 │
│ ● Scripts over their time budget 12 times                     │
│ TRANSLATED                                                    │
│ ✓ Mapping  Kontakt (v0x80) · 3412 zones · 48 groups           │
└───────────────────────────────────────────────────────────────┘
```

- **A one-line verdict comes first**: "Plays as authored", "Plays with N gaps" or
  "Plays incompletely · N problems".
- **Rows are grouped by kind.** Severe kinds come first: script errors, access
  failures, missing samples. Each group shows a count, the first three rows and
  "Show all N".
- **Every row has three parts**: a title in the player's words, the location in source
  terms (script line:col, zone, group, module and parameters), and an **impact line**
  saying what the player will hear.
- **Colour.** A coral dot marks problems that make the instrument play wrong; a hollow
  dot marks translation gaps. It is never a log dump: the raw trace stays in Logs.
- **Script rows.** Translator script entries (`"script"` with `line:col: message`,
  `"script Kind: builtin"` at `"Name line N"`) become script-error and builtin rows.
- **Runtime rows** come from the core counters: budget overruns, dropped notes,
  underruns, non-finite frames, narrowed and ignored MIDI.

## Library UIs: the UI IR and its two presentations **[built]**

`crates/sampler-ui-ir` is plain data with no dependencies: pages, widgets (kind,
rect or grid placement, parent panel, z, hide parts, binding, colours, text styles,
image uses with roles), assets (images with frames, axis, per-axis stretch, nine-slice
margins and frame size, plus bitmap fonts) and an `unsupported` list. `needed_assets`,
`draw_order` and `validate` are defined on the data.

`src/ui/ir_view.rs` draws it:

- **`resolved()` applies Kontakt's layout rules once:**
  - grid placement (92 × 21 cells from 66, 2) to pixels;
  - grid-row page height (rows × 68);
  - stock sizes for unsized controls (knob 85 × 52, others 85 × 18, …);
  - **a control takes its picture's frame size along each axis the picture does not
    stretch**.
- **Original (Bitmap).** Wallpaper, background art and control strips, each control
  showing the frame for its value. Buttons and switches show the off/on frame; a
  fixed `PICTURE_STATE` frame wins.
- **Vector.**
  - Keeps the wallpaper and widget background art. Releases strips, handles and bitmap
    fonts; they are never decoded in this mode and are dropped on a switch.
  - Every control is drawn by our faces at its authored rect: dials for knobs and
    near-square sliders, faders for long sliders, text boxes for buttons, menus and
    value edits, bars for tables, meters for level meters.
  - Over light artwork the faces sit on a dark plate, chosen by sampling the art
    under each control.
- **Pictures** are read by `src/ui/pictures.rs` from loose `Resources/pictures` folders
  or the library's `.nkr` (decrypting with the library key), and cut into frames.
  It also fills `ImageMeta` (.txt layout, frame size from the PNG header) for the
  layout rule. This belongs in the loader eventually (Core gap 5).

Measured decoded picture memory (`ir_view_real_instrument_memory`, v2 Kontakt
loader, the interface with the most controls):

| Instrument | Original | Vector | Saved |
|---|---|---|---|
| Vista – 3 Cellos | 984 KiB | 618 KiB | 37 % |
| Afflatus – Barbarian Brass ¹ | 4488 KiB | 1782 KiB | 60 % |
| Dolce – 7 1st Violins Legato ² | 21229 KiB | 9905 KiB | 53 % |

¹ The wallpaper is chosen by a persistent menu that `Script::ui` does not restore
yet, so it was measured with the "Barbarian" wallpaper named by hand.
² Measured through the pre-merge path. Through the v2 loader the "Pyramid" script
fails ("source byte budget exceeded"), and one failed script currently drops every
interface.

The remaining Vector memory is the wallpaper and background art the product says to
keep.

The renderer also draws all 49 interfaces the KSP frontend emits from the 52-script
corpus (10 616 widgets) in both modes (`ir_view_draws_the_ksp_corpus`).

Known limits of Vector:

- Labels baked into button bitmaps (BASIC/ADVANCED, icon buttons) are lost, because
  their meaning is in pixels. These are candidates for "keep this bitmap" exceptions;
  see open questions.
- Controls hidden by fully transparent pictures (invisible hit areas) become visible.
- Menus cycle on click instead of opening a list. **[next]**

## Open questions for the user

1. **Baked-label buttons in Vector.** Options:
   - keep those few bitmaps (small RAM cost);
   - draw the control's name or tooltip as text;
   - or accept blank buttons.
   The current build draws blank buttons.
2. Should **Vector** be the default for libraries over some RAM threshold, or always
   start in Original?
3. Should the flat **Console** mixer stay, or should the tree be the only mixer?
4. **Mic gain ownership.** Should a node's fader be ours (in the tree), or follow the
   library script's own mic faders when it has them? Both at once double-apply.

## Core gaps (plain-data shapes the UI needs)

1. **Script controls.** `Core::set_control(part, ControlId, f64)` and
   `fn control_values(part) -> Vec<(ControlId, f64)>`. Without these, widget edits
   change only the view.
2. **Per-node meters.** `PartShared.node_meters: Vec<[AtomicU32; 2]>`, one per
   `MixTree` node. Today only the root strip shows a level.
3. **Articulations.** `Vec<Articulation { name, trigger: Trigger::{Key(u8), Cc(u8, RangeInclusive<u8>), Program(u8), Channel(u8), Velocity(RangeInclusive<u8>)} }>`
   and a setter for triggers.
4. **Per-node residency.** `NodeMix.resident: bool` (load/unload a mic's samples).
5. **Pictures in the loader.** The loader fills `ImageMeta.size` and reads `.txt` from
   `.nkr` containers. The KSP UI snapshot should be taken after persistence is
   restored, and empty picture names should not be emitted.
6. **Script key colours.** `PartView.keys: Arc<[KeyLook; 128]>` with
   `KeyLook { color: Option<u8> /* $KEY_COLOR_* index */, name: Option<String>, control: bool }`,
   snapshotted after `on init` and persistence, and again when a script changes them.
7. **MPE and bend range.** Read `Part.mpe` (lower zone, manager channel 1, so
   `sampler_midi::Mpe` binds the part's domain) and `Part.bend_range` (semitones; 0
   keeps the instrument's own) at ingress.
8. **Disk and RAM.** `Decoded.resident_bytes: u64` (decoded PCM the part holds) and
   `PartShared.stream: { cache_fill: AtomicU32 /* 0..=1000 */, reads_per_s: AtomicU32 }`.
   Underruns already reach `RuntimeProblems`.
9. **Articulation switching.** Persist `Part.switching: sampler_ir::Switching` (from
   `v2/expression`). The UI's "Remap all" writes its `driver` field; the core applies
   `assign_alternatives`. The articulation playing should come back as
   `PartShared.articulation: AtomicU32`; today the UI follows switch keys itself.
