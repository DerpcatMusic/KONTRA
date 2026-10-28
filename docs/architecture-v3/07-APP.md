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
