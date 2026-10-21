# Slot intent, ticket and instrument snapshot: design and increments

Follows `lifecycle-and-core-20261021.md` (Part 3, migration steps 3 and 5). Status of each
increment is at the bottom; nothing here changes audio-thread rules (no allocation, no locks,
bit-exact DSP untouched).

## Problem

Per slot, one commit writes about a dozen separately locked cells (`PartShared`: `controls`,
`scripts`, `stream`, `engine_bindings`, `node_meters`, `effect_controls`, ...) plus `PartView`
inside the `view` lock. Readers can interleave between two writes; the only protection is
convention (generation checks, comment-documented lock order). Intent (what the user asked for),
observation (what is installed) and persistence are one `Selection`/`PartView` blob reconciled by
the `attempted`/`active_target` tuples.

## Model

```
Intent   { source, program, snapshot, rate, mpe, mpe_upper, dynamics, streaming, force }   // written by header/host
Ticket   u64, per slot, bumped when a load starts                                          // PartView::ticket (done)
Active   Arc<InstrumentSnapshot>                                                          // what is installed + visible
Phase    Empty | Active | Active+Loading(ticket) | Active+Failed(err)
```

Rules:

1. A load starts only if `intent != active.identity` or `force` (host recall, rate change).
   Increment 1 of this rule is `PartView::active_target` (done, PR 1/5).
2. Starting a load takes a ticket. A result commits only if its ticket is still the slot's latest
   (`PartView::ticket`, checked at commit; done here). Cancellation keeps its current checks
   (selection, recall revision, generation, rate) as an early-out.
3. Commit builds an `InstrumentSnapshot` entirely off-lock, then publishes it with one store.
4. Failure and panic leave `Active` untouched and set `Failed` (panic containment: `LoadGuard`, done).
5. Retiring a snapshot hands it to the reaper; no join/free under a shared lock.

## InstrumentSnapshot

Everything the commit assigns today, as immutable fields:

| Field | Today |
| --- | --- |
| `generation` | `PartShared::generation`, `PartView::generation` |
| `identity` (source key, `LoadTarget`) | `PartView::active_source`, `active_target` |
| `view` (interfaces, tree, instrument, mapping index, keys, report, trace, host_bindings) | `PartView` |
| `controls`, `engine_bindings`, `node_meters`, `effect_controls`, `engine_meters` | `Mutex<...>` cells |
| `scripts` (UVI thread, KSP views, sources, resources) | `PartShared::scripts` |
| `stream`, resident/full bytes | `PartShared::stream` + atomics |
| `waveforms` | `PartShared::waveforms` |

Mutable-by-design values stay atomics inside the snapshot (`ControlCell`, meters), so the audio
thread's refresh and the UI read never take a lock.

## Publish

`Rack { slots: Vec<SlotCell> }`, `SlotCell { current: Mutex<Arc<InstrumentSnapshot>> }` where the
mutex guards only the pointer swap (clone-out for readers, never held across any other lock or
drop). No new dependency is needed; `arc-swap` can replace the mutex later without API change.
Readers (`display_values`, `control_values`, UI frame) clone the `Arc` once. The old snapshot is
sent to the reaper (`Shared::discard`-style queue drained by the control tick), so `ScriptThread`
and `Streamer` joins never run on the UI, audio or commit path.

## Increments

| # | Increment | Status |
| --- | --- | --- |
| a | `active_target` dedup; recall forces | done (PR 1/5) |
| b | `ticket` per slot; stale ticket cannot commit | done |
| c | `LoadGuard`: panic leaves slot retryable | done |
| d | Unload resets every per-part cell, joins before `view` | done |
| e | Commit drops replaced script thread/streamer outside `view` | done (PR 4/5) |
| f | `SlotPhase` enum replacing `attempted`/`loading`/status prefix, shown in the UI | not started |
| g | `InstrumentSnapshot` struct; commit and unload build it and publish with one store; cell accessors become `snapshot()` reads | not started |
| h | Reaper for retired snapshots (also covers `Shared::drop`, F-16) | not started |
| i | Seeded random-ops model test (switch, cancel, recall, rate change, unload, save/restore) | not started |

(g) is the large step: ~120 call sites read the cells through `PartShared`. The safe order is to
introduce `snapshot()` accessors that still read the old cells, move readers over per file, and
only then flip the commit to build and swap the struct. The existing transaction tests
(`plugin::tests::*transaction*`, `lifecycle_audit_tests`) are the characterization suite for each
step.
