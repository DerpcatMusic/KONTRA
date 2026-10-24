# 02 Engine: the audio-thread runtime

`kontra-engine` is the only code that runs per block. It owns everything
it touches, allocates nothing after prepare, and has one loop.

## 1. The `Engine` struct

Today's `Runtime` has 109 fields. The v3 engine is a handful of owners,
each a module with its own invariants:

```rust
pub struct Engine {
    clock: Clock,                 // now: SampleTime, rate, transport, tempo map
    queue: EventQueue,            // every future thing, ordered by (time, seq)
    ledger: NoteLedger,           // host note ↔ script event ↔ voices; expressions
    voices: VoicePool,            // kontra-voice::Voice slots + allocator + stealing
    scripts: ScriptHost,          // kontra-vm instances + coroutine scheduler
    mixer: Mixer,                 // groups → buses → outputs, compiled chains
    plans: PlanSlot,              // current Arc<Plan>, pending swap, retired
    store: StreamClient,          // read side of kontra-stream (page table, request ring)
    out: Publisher,               // protocol publications (meters, values, taps)
    stats: Stats,                 // #[derive(Stats)], one struct
}
```

Each field is `pub(crate)` and owned by its file:
`clock.rs`, `queue.rs`, `ledger.rs`, `voices.rs` (wraps kontra-voice),
`scripts.rs` (wraps kontra-vm), `mixer.rs`, `plan.rs`, `publish.rs`,
`stats.rs`, `render.rs` (the loop). Nothing else.

## 2. One event type, one queue

```rust
pub struct Timed<T> { pub at: SampleTime, pub seq: u32, pub body: T }

pub enum Work {
    Input(kontra_events::Event),        // host notes/CC/expression/transport, already sample-stamped
    Resume(CoroutineId),                // a script wait expiring
    EngineEdit(EngineEdit),             // set_engine_par etc. from scripts/host, applied at `at`
    NoteStart(VoiceStart),              // delayed play_note / NoteData.delay
    NoteStop(NoteRef, StopKind),        // durations, note_off(delay), fades ending
    Listener(ListenerId),               // timers, beat listeners
}
```

- `EventQueue` is a fixed-capacity binary heap (capacity from the plan;
  overflow is a stated, counted refusal at admission, never silent reorder).
- `seq` breaks ties in arrival order, so same-sample work is deterministic.
- Host events enter with `at = block_start + sample_offset`. The plugin shell
  no longer splits blocks; the engine does, at every queue boundary.

## 3. The render loop

```rust
pub fn process(&mut self, block: &mut Block, input: &[Timed<Event>]) {
    self.queue.extend(input);                       // stamped by the shell
    let end = self.clock.now + block.len();
    while self.clock.now < end {
        // 1. everything due now, in order; scripts run to completion or wait
        while let Some(w) = self.queue.pop_due(self.clock.now) {
            self.dispatch(w);                        // may push more Work at >= now
        }
        // 2. render up to the next due item (or the control grid, or the block end)
        let until = self.queue.next_at().min(end).min(self.clock.next_control_point());
        self.render_span(block.slice(self.clock.now, until));
        self.clock.now = until;
    }
    self.out.publish(&self.stats, &self.mixer, &self.ledger);
}
```

- Same inputs at any block size give the same output: the loop's
  boundaries are the queue and the control grid, never the host block.
- `render_span` = for each active voice batch: source → per-voice chain →
  group bus; then buses → outputs. One path (SIMD-batched by kernel kind, 03-DSP.md).
- No fuel, no deferral queue, no "yielded" list. Scripts that `wait` push a
  `Resume`; scripts that loop are bounded by Kontakt's `while` limit only.

## 4. The note ledger

One table replaces `held`, `consumed_hosts`, native physical sources, the
aligner's host table, families-as-cohorts and `closed_notes`:

```rust
pub struct NoteLedger {
    notes: Arena<Note>,                 // generational handles: NoteRef
    by_host: FixedMap<HostKey, NoteRef>,// (port, channel, key, host id) → note
}
pub struct Note {
    origin: Origin,                     // Host(HostKey) | Script { parent: NoteRef, event: EventId }
    key: u8, velocity: f32, pitch: Pitch,
    expression: ExpressionRef,          // per-note expression (MPE/MIDI2/CLAP), may follow parent
    state: NoteState,                   // Pending → Started → Released → Ended
    original: OriginalAttack,           // Pending | Forwarded | Ignored (KSP ignore_event)
    voices: SmallList<VoiceRef>,
    script: Option<ScriptEventId>,      // the KSP/Lua event id handed to scripts
}
```

- **Host note-on**: create `Note`, run the script `on note` callback(s) at its
  sample. When the callback ends or first waits, the original attack is
  decided: `Ignored` if the script called `ignore_event`, else `Forwarded`
  (voices selected and started *at the note's sample*, not later).
- **`play_note`** creates a child `Note` (`Origin::Script`); its voices start
  at the same sample as the callback instant. All voices of one instant are
  started in one `start_batch`, so every mic and layer aligns by construction.
- **Release/choke/expression** look up the note by `HostKey` or `NoteRef`; the
  ledger fans out to children by `origin` and to voices by `voices`.
- **Ending**: a note ends when its voices and scripts are done; the host gets
  its CLAP note-end then.

## 5. Voices

`kontra-voice::Voice` is plain data plus a cursor:

```rust
pub struct Voice {
    note: NoteRef, zone: ZoneRef, start: SampleTime,
    cursor: Cursor,              // position, step, loop state, direction (from today's source.rs)
    reader: SourceReader,        // preload slice + stream buffers (05-STREAM.md)
    amp: EnvelopeState, mods: ModState,
    chain: ChainState,           // per-voice DSP state, layout fixed by the plan
    gain: [f32; 2], bus: BusRef,
}
```

- Allocation: fixed pool sized by the plan; stealing law = released oldest,
  then quietest, then oldest (stated in the plan, counted in `Stats`).
- A voice never waits for storage (05-STREAM.md). The state machine has no
  `cold`, `holding_onset`, `start_runway`, `starvation`-as-hold.
- Culling: releasing voices below −120 dB for 512 frames end (today's rule, kept).

## 6. Plans and swaps

- The loader builds an immutable `Plan` (zones, compiled chains, script
  bytecode, law tables, preload map) from the IR and hands it over as `Arc<Plan>`
  through `protocol`.
- The engine swaps at a block boundary; notes on the old plan finish on it
  (today's tails idea, kept); the old `Arc` goes back on the retirement
  channel and is dropped off-audio.
- Growth (more voices, more notes) is a new plan with bigger pools, not a
  separate growth protocol.

## 7. What the engine does not do

No file I/O, no decoding, no allocation, no locks, no UI formatting, no
format knowledge, no `message()` string building (the UI store keeps an index
and arguments; the UI formats).
