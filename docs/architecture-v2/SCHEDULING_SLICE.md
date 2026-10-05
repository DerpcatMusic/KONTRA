# Native gates and one musical timeline

2026-10-05. Continues the clean-sheet runtime at `c510d79`. The user has explicitly
requested sustained implementation and eventual replacement/deletion of old core
paths. Integration will use new services directly, not a compatibility shim or a
legacy/v2 engine switch. Old behavior is not an acceptance oracle.

## Implemented contracts

- Physical key state and effective gate are independent. Anonymous same-key note-offs
  pair FIFO among physical keys, including when older notes remain pedal-held.
- Registered controller domains include protocol, port, group and channel. Native
  sustain holds physical releases; sostenuto captures only physically held notes at
  its rising edge. Repeated down does not capture new notes. The two holds combine.
- Channel registration is idempotent, bounded by `Limits.channels`, and stable for
  the runtime lifetime. Panic clears pedal values and scheduled changes, not IDs.
  Notes on unregistered domains have no pedal holds. Adapters register before sending
  controls; missing registration is not silently allocated on a control event.
- Source starts, physical key-up, explicit release, expression and pedal changes use
  the same stable sample-time queue. Equal timestamps retain submission order. New
  immediate musical mutations drain already-due work first. Events at an exclusive
  render end remain queued until another processing/immediate operation at that time.
- Future scheduling rejects full queues. Immediate key/pedal/explicit cleanup needs
  no queue entry. Expression jobs retain private note work pins; public continuation
  `unpin` cannot consume them. Release/panic cancels jobs and relinquishes those pins.
- A scheduled expression targets the note's owner at execution, including an explicit
  detach before it runs. Original owner handles remain separate from note-targeted jobs.

The implementation is in [gate.rs](../../crates/sampler-core/src/gate.rs) and
[schedule.rs](../../crates/sampler-core/src/schedule.rs). Scheduling logic was moved
out of the core file rather than adding another independent controller queue.

## Evidence and limits

15 core unit tests and two core-only realtime tests cover the prior ownership
contracts plus pedal interplay, channel isolation, mixed-event partition equality,
same-time ordering, queue-full pedal-up, job cancellation, detachment and panic.
The new realtime test repeats controller/expression scheduling and cleanup 100 times
without measured allocations or frees. Core debug tests and warnings-denied clippy
pass. Release/MSRV and historical integration results are recorded with this change.

Native binary sustain/sostenuto is implemented. Half-pedal, repedal, envelope release
tails, consumed/raw CC projections, transport clocks, external note-on admission,
script continuations and arbitrary controller/modulation routing are not complete.
Explicit release still stops the resident PCM fixture immediately. These limits keep
V2-04/05/06 open; no complete catalogue or MIDI 2.0 support claim follows.

## Next integration work

Replace fixture-only asset borrowing with owned immutable prepared data that can
live in an independently constructed application/runtime. Add the native composition
root, prepared instrument selection and source execution, then connect the new host
and behavior services. Preserve typed lifetimes and off-audio construction/destruction
through that integration; do not wrap the old Engine or VM to fill missing behavior.
