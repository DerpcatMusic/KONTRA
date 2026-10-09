# Falcon/UVI Lua script runtime

Status: design and first milestone, 2026-10-06. Owner: UVI translator.

## Why

A UVI program may carry a `ScriptProcessor` whose Lua decides which oscillators
sound. Without it a translated program plays every oscillator of every keygroup
(V Strings Bartok peaks at 3.27 instead of about 1.08) or none (VWinds).

## Survey (660 programs, banks opened in memory, nothing written to disk)

- 620 programs: `require '_Main/Main'`, the Augmented Orchestra framework
  (Main 69 KB, Layer 115 KB, Part 78 KB, Arpeggiator, Harmonizer, LFO and step
  editors, data tables). 40 programs (VWinds): `require("hornScript3")`
  (6.3 MB, mostly tables) plus `hornScriptMain` and preset lists.
- The scripts live in the bank (`*.lua` members), not in the program XML.
- Most-used API, by occurrences: `spawn` 1422, `Program.auxs` 1158, `wait` 918,
  `playNote` 626 (many with `oscIndex`), `Program.modulations` 575,
  `Program.layers` 345, `:setParameter` 2395 / `:getParameter` 329 on
  Program, layers, keygroups, oscillators, inserts and auxs,
  `sendScriptModulation` 1752, `getTime` 173, `.oscillators`/`.inserts`
  of a keygroup, `setSampleOffset` 208, `onNote`, `onRelease`, `onController`.
- UI (`Panel`, `Knob`, `Slider`, `Menu`, `OnOffButton`, `Button`, `Table`,
  `Image`, `Label`, `NumBox`, `AudioMeter`; `.value`, `.changed`, `:setValue`)
  is the bulk of the code but not of the sound.
- Lua dialect: 5.1 (`math.pow`, `table.insert`, `string.find/sub`, `pairs`,
  `ipairs`, `tostring`; no goto, no integers). UVI documents "built on Lua 5.1".

## Runtime choice

Cargo.lock and vendor/ hold no Lua. Options:

| Option | Verdict |
|---|---|
| Own Lua interpreter | Rejected: string patterns, coroutines, 5.1 semantics are weeks of work and a conformance risk. |
| `piccolo` (pure Rust) | Rejected: 5.4-style, incomplete stdlib (no `math.pow`, partial `string`). |
| `mlua` with `luau` + `luau-jit` | Chosen (switched from `lua51`): Luau is a Lua 5.1 derivative; all 660 corpus scripts compile. Coroutines, interrupt-based time budget, memory limit. Built from source; no system Lua. Not pure Rust. |

Cargo.lock changes are additive only (new packages, no version bumps).

## Sandbox and budgets

- Standard libraries loaded: base (minus `dofile`, `loadfile`, `load*`,
  `require` replaced), `table`, `string`, `math`. No `io`, `os`, `package`,
  `debug`.
- `require(name)` resolves `<name>.lua` against the bank's script members
  (relative to the program's `Scripts` folder, then bank root). No other path.
- Per callback (`onInit`, `onNote`, each resumed `spawn` thread): an instruction
  budget enforced by the VM hook (default 20 M); exhaustion aborts that
  callback and is reported. Memory limit per host (default 768 MB; the 6 MB
  VWinds tables need most of it).
- Unknown API is never silent. UI constructors and unknown elements return
  inert stubs that absorb any field or method; the first use of each distinct
  name becomes one `Unsupported` entry (`lua <name>`, NotModeled).

## Threading

Lua does not run on the audio thread. `ScriptHost` is a control-thread object
that is fed note, release and controller events with a sample timestamp and
returns timed commands (play, release). The embedding drains these into the
runtime through a bounded queue; one block of latency is accepted and stated.
`wait`/`spawn` are coroutines resumed by the host's clock (`advance(frames)`),
so timing is sample-accurate against the commands' timestamps, not wall time.
The offline driver in the tests applies commands synchronously.

## Effects

`playNote(note, vel, duration, layer, channel, input, vol, pan, tune, slice,
oscIndex)` becomes a core child note whose selection mask allows only the zones
of the named layer and oscillator: the translator emits one IR group per
(layer, oscillator index), each carrying the layer gain/pan, and the load
tags group ids so the host can map (layer, oscIndex) to a set of groups.
`duration >= 0` schedules the release (`Runtime::release_at`), `-1` follows the
originating note, `vol`/`pan`/`tune` map to the expression inputs of
`note_on_pitched`. `onNote` present means the original attack is suppressed
(UVI semantics: the script re-emits what it wants).

Not modeled in milestone 1 and reported when touched: `postEvent` other than
note events, `setParameter`/`getParameter`/`sendScriptModulation` (accepted,
stored, no audio effect), `setSampleOffset`, `waitBeat`, `onTransport`,
`onSave`/`onLoad`, all UI.

## Milestones

1. `ScriptHost`: sandbox, `require`, `onInit`, `onNote`/`onRelease`,
   `playNote` with layer/oscIndex, `spawn`/`wait`/`getTime`, stubs and report.
   Bartok renders about 1.08, VWinds sounds.
2. Parameter model: `setParameter`/`getParameter` and `sendScriptModulation`
   onto the IR where a law exists.
3. UI widgets into `sampler-ui-ir::Interface`, as the KSP frontend does.

### Deterministic evaluation bounds (2026-10-09)

UVI admission and callbacks consume Luau call/backedge checkpoints and deferred
coroutine resumes. Initialization shares 33,554,432 units across graph/script
phases; live callbacks receive 1,048,576. Graph construction permits 262,144
engine elements and depth 192, separately from the Original UI widget budget.
The existing 1.5 GiB Lua memory limit remains. Config's elapsed load/callback
thresholds are observations only: scheduler contention cannot reject finite
work. Work exhaustion keeps the existing fault category and unsupported-init
classification; infinite Lua loops and repeated deferred spawning still stop.

The five former load-deadline leaves complete with 86,868 elements/depth 6 and
12,877,619–13,366,624 VM checkpoints. The work-bound candidate produces the same
checkpoint counts; receipts are in the W10 `w10-uvi-deadlines-20261009` run.
Finite initialization with an already-expired elapsed threshold fails before
and passes after this change. No quiet CPU or whole-census acceptance follows
from these diagnostic timings.


### Seeded audit protocol (2026-10-09)

Only scan/shot builds expose `Config.audit_seed` and
`KONTRA_UVI_AUDIT_SEED`/scanner `--audit-seed`. An explicit seed initializes Luau
math and runtime random/native-cycle state per load. The threaded audit driver
acknowledges after prior events and due coroutines at the virtual clock have
completed and their commands have drained. It closes the event/request queue
race and handles queue backpressure. Unseeded rendering keeps asynchronous
processing and normal math seeding; no user setting or persistence field exists.
Take-sequence seeds and modulation hashes already start deterministically from
the loaded plan and virtual note/frame sequence.

Four-second production-loader renders at block 64, key 60/velocity 64 and seed 42
repeat bit-for-bit on two loads: Clarinet `97ad41a1…`, Flute `e8e09e24…`.
Both remain varied with the seed unset on the same binary. All eight renders are
nonzero with zero problem counters. Numeric-only receipts live in
`~/.cache/kontakto-w10/audit-seed/`; no authored PCM is retained. This is A/A
repeatability, not acceptance of W8's chain-sharing A/B or a CPU/RSS comparison.

### Audit publication boundary (2026-10-09)

The seeded barrier must acknowledge the owner only after publishing control
values, the authored interface, runtime findings and scanner faults. Before this
correction, the command reply preceded those publications: a synthetic note
callback returned its Play command while the bridge still showed velocity 0
instead of 64. The regression retains all 4,097 synthetic widgets and checks
their text, scalar readback and one callback fault at the same completed barrier.
It uses a clear inline preset and no library reader. This affects only the scan
barrier; ordinary asynchronous playback keeps its existing publication path.

Readback has its own load-sized work allowance after initialization. Metatable
reads needed to project a large UI previously spent the remaining live callback
work: after fixing reply ordering alone, the same synthetic panel exposed empty
Panel placeholders starting at source widget 1,411. The inspection guard restores
the exact previous work remainder, exhaustion flag and elapsed observation on
exit. Initialization still shares one allowance and cannot refill it through
inspection. A separate regression checks both near-empty and exhausted live
allowances, and an inspection that exceeds its own bound reports a budget fault.

The historical 11/21 timeouts remain incomplete observations. This UI-publication
race does not establish their cause or a PCM difference. W8's frozen matching
0be9 witnesses remain valid for their reported PCM comparisons; counter snapshots
from those witnesses do not imply a fully published owner boundary.
