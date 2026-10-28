# 07 App: threads, channels, plugin shell, UI boundary

Today the plugin's `Shared` struct has 54 fields, including 7 `ArrayQueue`s,
19 `AtomicU64`s and 9 `Mutex`es: one ad-hoc channel per feature. In v3 the
audio thread talks to the rest of the process through three channels.

## 1. Threads

| Thread | Owns | Runs |
|---|---|---|
| audio (host) | `Engine` | `process()` only |
| control (host main / timer) | `Rack` (parts, routing, settings), `Commands` sender | host callbacks, state save/load, automation glue |
| loader (1) | load pipeline | format → IR → plan → hand over |
| I/O (N) | page fills | `kontra-stream` `IoService` |
| UI | editor view state | reads publications, sends commands |
| library (1) | catalog | scanning, artwork |

## 2. The three channels

```rust
// kontra-protocol
pub enum Command {                  // control/UI/loader → audio. SPSC ring, fixed capacity, never dropped silently
    Plan(Arc<Plan>), Purge(PurgeSet), SetParam(ParamAddr, f32), Ui(ControlId, ControlValue),
    Transport(TransportCmd), Panic, ...
}
pub struct Publications {           // audio → others. Latest value only, no queue
    meters: LatestCell<Meters>, voices: LatestCell<VoiceTaps>, controls: ControlStore,   // LatestCell per script control
    params: LatestCell<ParamSnapshot>, stats: LatestCell<StatsSnapshot>, messages: LatestCell<MessageRef>,
}
pub enum Retired { Plan(Arc<Plan>), Buffers(Box<[f32]>), ... }   // audio → control, dropped off-audio
```

- **Commands** are the only way into the engine. When the ring is full, the
  sender gets `Err(Full)` and retries next tick. The audio side never blocks.
- **Publications** are latest-value cells (`kontra-pool::LatestCell`, a
  triple buffer or seqlock). A slow UI skips frames and never builds up a
  backlog. That removes the UI effect outbox and the 10 Hz `control_tick`
  drain.
- **Retirements** carry anything the audio thread lets go of, so it can be
  freed on the control thread.

## 3. Plugin shell (`plugin/`, ~1.5k)

- moose CLAP/VST3/standalone glue: ports, the state chunk, parameters
  (generated from the catalog) and note expressions.
- It stamps host events with sample offsets and calls
  `engine.process(block, events)`. It doesn't split blocks, track held keys
  or apply script logic.
- The host-note ↔ CLAP note-end mapping lives in the ledger (02-ENGINE.md),
  not in `held_keys*`.

## 4. Host (`app/host`)

- `Rack`: parts (instrument slots), each with `PartState`, routing, purge
  set and saved script `persist` data.
- Load pipeline: `Format::load` → IR → validate → `Plan::build` (compile
  chains via the `kontra-dsp` registry, compile scripts via
  `kontra-ksp`/`kontra-lua`, run `on init` on the VM, compute preload law
  ranges, register blobs with the store) → `Command::Plan`.
- State: one serde struct, versioned; no v1 compatibility (memory: v2
  product goals).

## 5. UI boundary

`kontra-ui` depends on `protocol`, `ui-model` and library data types
only. It never depends on `engine`, `vm` or `dsp`. It renders the script UI
from `ui-model` trees and the `ControlStore`, sends `Command::Ui` for user
edits, and reads meters and stats from `Publications`. That's why a UI
edit rebuilds only `ui` and `plugin`, and why engine tests never compile
the GPU stack.

### 5.1 How a library's UI gets on screen (lane D, 2026-10-28)

| Step | Where | Runs |
|---|---|---|
| 1. Tree | `kontra-ui-model::Interface` (v2's `sampler-ui-ir`, moved in P4.9) | Plain data: pages, widgets, assets, styles, paint order; 1x source pixels; parent-relative rects. No pixels, no handles. |
| 2. Build | host, once per load, after `on init` | KSP declarations + the control store as init left it (pos, picture, hide, text, menus) → `Interface`. UVI Lua builds the same tree. |
| 3. Pictures | `kontra-ui` worker | Decode off the UI thread, once, keyed by content; the first frame draws what is ready and never waits on the full set. |
| 4. Frames | `kontra-ui` | Retained: the tree is laid out once per load or resize. A script write after init is a control **update** (value or look) with a generation number; the UI repaints the widgets whose generation moved and rebuilds nothing. |
| 5. Edits | `kontra-ui` → `Command::Ui(ControlId, ControlValue)` | The engine runs `on ui_control`; its writes come back through step 4. |

Scroll and zoom are view state in `kontra-ui` (one transform per scroll area
and one editor scale), so neither touches the tree. What moves from `src/ui`:
the Original renderer (`ir_view`), `pictures`/`picture_worker`, `theme`,
`keyboard` and `menu` first; the browser and inspector views follow with P8.4.

### 5.2 The bar: v2's UI on real libraries (2026-10-28)

v1's binaries are gone, so v2 (`src/ui`, release, CPU paint, headless) is
the bar. Measured with `examples/kontra_scan --worker` (the Original view,
first page): *first frame* is the first layout, with how many pictures
it still waits on; *settled* is from the page's render start until every picture
it asked for is decoded and laid out; *paint* is one full CPU paint; *idle*
is the median `Ui::frame` with nothing moving, once settled. Images are the
decoded picture bytes. Load and peak RSS cover the whole worker, samples
included.

| Library | widgets | first frame ms (pictures pending) | settled ms: v2 → unbounded results | paint ms | idle µs | images MB | load ms | peak RSS MB |
|---|---|---|---|---|---|---|---|---|
| Afflatus Chapter II Brass | 161 | 3.0 (75) | 198 → 16 | 17 | 87 | 2.0 | 3118 | 3229 |
| ANALOG STRINGS | 934 | 4.8 (175) | 847 → 313 | 16 | 247 | 4.6 | 10666 | 1837 |
| Areia 1.2.0 | 847 | 2.8 (53) | 123 → 14 | 8 | 64 | 2.1 | 8776 | 1931 |
| Audio Imperia CHORUS | 572 | 3.0 (59) | 131 → 13 | 7 | 57 | 2.1 | 7904 | 1489 |
| Audio Imperia Dolce | 690 | 2.4 (48) | 104 → 13 | 7 | 65 | 2.0 | 5555 | 1185 |
| Conflux 1.1.0 | 378 | 1.0 (1) | 102 → 105 | 30 | 7220 | 7.0 | 5130 | 5245 |
| Morphology Evolved | 671 | 3.5 (167) | 586 → 29 | 21 | 187 | 4.6 | 1086 | 1187 |
| Pacific Ensemble Strings | 58 | 2.5 (4) | 11 → 5 | 4 | 84 | 0.6 | 109 | 162 |
| Performance Samples Vista | 48 | 2.5 (4) | 10 → 5 | 3 | 88 | 0.5 | 644 | 705 |
| Shreddage 3.5 E-Upright 1.0.1 | 1502 | 5.5 (108) | 492 → 71 | 32 | 330 | 12.3 | 1665 | 1082 |
| Una Corda Library | 118 | 2.2 (6) | 13 → 8 | 18 | 16 | 1.3 | 1351 | 546 |

The editor's own frame bench (`ui::frame_cost_bench`, 1180×780, median of
120 frames, ms): idle 0.18 build + 0.49 layout + 1.34 encode + 1.11 raster;
hover/drag/wheel ≈ 0.13 + 0.37 + 0.49 + 0.44 (≈1.0 ms CPU, 6% of a core at
60 fps); resize 1.08 layout (1.7 ms); HiDPI 2× 1.21 raster, 3× 2.29 raster.

v2 already draws its first frame from the tree in 2–6 ms and fills
pictures in as they decode. The settle tail was not decoding: the worker
handed results back through a one-slot channel, so one picture arrived per
UI frame (Morphology: 25 ms of decode, 572 ms to settle). An unbounded
results channel (a batch bounds it) cuts the tail 2–20×. ANALOG is now
decode-bound: 175 frames cut from 49 files on one thread, 302 ms. Next, the
`kontra-ui` picture store decodes each file once by content, on a small
pool, visible page first.

The v3 Original view (`kontra-ui`, #936), measured with `tools/ui-shots`
on the same instruments; v2 → **v3**. *Idle widgets built* counts the
widget memos an idle frame rebuilt; *knob moved* is one shown knob set
to 1.

| Library | widgets | first frame ms (pictures pending): v2 → v3 | settled ms: v2 → v2 unbounded → v3 | paint ms | idle µs | idle widgets built | knob moved: widgets built | images MB | peak RSS MB (whole process) |
|---|---|---|---|---|---|---|---|---|---|
| Afflatus Chapter II Brass | 161 | 3.0 (75) → **0.6 (24)** | 198 → 16 → **4** | 17 → **2** | 87 → **126** | **0** | 0 | 2.0 → **0.1** | 3229 → **2907** |
| ANALOG STRINGS | 934 | 4.8 (175) → **0.6 (58)** | 847 → 313 → **134** | 16 → **3** | 247 → **141** | **0** | 4 | 4.6 → **4.5** | 1837 → **344** |
| Areia 1.2.0 | 847 | 2.8 (53) → **0.8 (28)** | 123 → 14 → **7** | 8 → **2** | 64 → **149** | **0** | 8 | 2.1 → **1.9** | 1931 → **1886** |
| Audio Imperia CHORUS | 572 | 3.0 (59) → **0.7 (25)** | 131 → 13 → **7** | 7 → **2** | 57 → **119** | **0** | 10 | 2.1 → **1.9** | 1489 → **886** |
| Audio Imperia Dolce | 690 | 2.4 (48) → **0.6 (20)** | 104 → 13 → **8** | 7 → **2** | 65 → **49** | **0** | 8 | 2.0 → **1.8** | 1185 → **993** |
| Conflux 1.1.0 | 378 | 1.0 (1) → **2.0 (8)** | 102 → 105 → **4** | 30 → **5** | 7220 → **488** | **0** | 0 | 7.0 → **0.0** | 5245 → **88** |
| Morphology Evolved | 671 | 3.5 (167) → **0.4 (31)** | 586 → 29 → **19** | 21 → **7** | 187 → **114** | **0** | 2 | 4.6 → **11.9** | 1187 → **76** |
| Pacific Ensemble Strings | 58 | 2.5 (4) → **1.0 (4)** | 11 → 5 → **2** | 4 → **2** | 84 → **29** | **0** | 0 | 0.6 → **1.2** | 162 → **80** |
| Performance Samples Vista | 48 | 2.5 (4) → **0.9 (4)** | 10 → 5 → **2** | 3 → **1** | 88 → **33** | **0** | 0 | 0.5 → **0.6** | 705 → **255** |
| Shreddage 3.5 E-Upright 1.0.1 | 1502 | 5.5 (108) → **0.6 (30)** | 492 → 71 → **25** | 32 → **12** | 330 → **104** | **0** | 3 | 12.3 → **12.1** | 1082 → **770** |
| Una Corda Library | 118 | 2.2 (6) → **0.2 (6)** | 13 → 8 → **6** | 18 → **2** | 16 → **12** | **0** | 0 | 1.3 → **1.5** | 546 → **265** |

Idle frames still pay mui's resolve (step (b) skips them), v3 decodes at
source size (Morphology above v2), and Conflux's stock-drawn controls are
not drawn yet.

| v1 had it | v2 | v3 tree |
|---|---|---|
| Una Corda's pedal light: `on controller` sets `PICTURE_STATE` on `$T2_lblPedal` when CC64 goes down and back on release (the "moving part"; its 50 ms timer listener drives an audio LFO, not the UI) | not measured yet (v1 redrew on a 33 ms clock while sounding) | pedal down and up each rebuild only `$T2_lblPedal` (`examples/ui_tree`); the renderer must repaint that one widget |

### 5.3 The editor (lane D, 2026-10-29)

v2's look (graphite neutrals, square edges, hairlines, part colours a
golden angle apart), rebuilt from first principles: one primary action per
view, fewer controls on screen, details on demand. `kontra-ui::app` draws
it from a plain `App` the host fills and hands back `Action`s; every piece
comes from `kit` (button, icon button, tab, latch, knob, fader, field,
list row, menu, popup, sheet, meter) and `keyboard`. Hover, press, value
and layout changes ease through mui springs; once they settle the editor
asks for no frames (`tests/app.rs`), so idle costs nothing.

| v2 | v3 |
|---|---|
| Rack · Mixer · Report tabs | the rack alone; Report is a panel opened from the header's warning or the menu; Mixer is dropped until a view needs more than each strip's level, pan, solo and mute |
| header: CPU, voices, RAM, disk, master, keyboard, MIDI thru, Panic, menu | CPU alone (the rest is its tooltip), master, Panic, a warning when something did not load, menu (Settings, Report, About); MIDI thru moves to Settings |
| browser: two lists with two searches, A–Z, filter, add, collapse, Favorites and Recent rows | one search, Kontakt/UVI, a star filter and one list that drills from libraries into presets; adding a folder moves to Settings, collapsing to the header |
| part strip: name, preset step, MIDI, output, pan, volume, tune, S, M, more, close | name (library in its tooltip), preset step, channel (a 4×4 grid), volume and pan knobs with their values as tooltips, S, M, more (Duplicate, Remove) |
| part tabs: Articulations, Mapping, Sound, Info, Scripts, the current articulation, velocity Auto/64/127 | Interface and Articulations, each shown only when the part has one; Info is the name's tooltip; Scripts goes to the Report; Mapping and Sound come back as they are ported |
| keyboard: range, a hint line, octave, collapse | range (the hint is its tooltip), octave, collapse; each part's span drawn over its keys |

Measured with `examples/app_shot` (specimen, release, CPU): first frame
0.3–0.7 ms, a frame when one is due 0.07–0.33 ms, and 0–46 frames of motion
after an action. Lines: v2's theme, menu, keyboard, header, browser, rack,
logs and editor are 12,240; v3's theme, kit, keyboard and app are 1,760,
tests included.

## 6. Auto-align (`app/align`, feature `align`)

Target state, implemented by P3.3. Auto-align is per-patch latency
compensation and stays optional. Many
libraries start their audible attack late on purpose (by the user's ear: Areia ~80 ms,
some Performance Samples libraries ~120 ms; the load-time measurement is
what counts) because the lead-in makes the attack sound natural. At load
each patch's attack delay is measured. The slowest loaded patch sets the
reported plugin latency, and every other patch's notes are held back by
the difference, so layered patches start together. The host's delay
compensation then plays the MIDI early by that latency, and the attacks
land on the grid.

- Holding is the same live and on playback. Jamming with two patches lines
  them up too. Live, the felt latency is the slowest patch's, never less.
- A delay never goes negative. A sample's lead-in is never trimmed to make
  a slow patch faster.
- One patch per instance: that instance reports its own delay, and the
  host aligns instances across tracks.
