# Retained native release context

Logical notes now retain their admission time, first key release and first effective
gate closure. This provides native release-policy input within V2-06/08; automatic
release-region mapping, release-voice reserves and articulation/controller snapshots
are still open. Source completion alone is not a key or gate transition.

## One authoritative lifecycle

`Runtime::release_context(note)` returns a copy of the note-owned `ReleaseContext`:

- `admitted_at`: logical admission on the monotonic engine sample clock, including
  generated notes. It does not pretend to be a delayed source's audible onset.
- `key: Option<KeyRelease>`: the first consumed key transition, with sample time,
  optional normalized release velocity and cause. `None` means the key remains down.
- `gate: Option<GateRelease>`: the first effective closure, with sample time and cause.
  `None` means the gate remains open.

The records' cause markers replace the private key/gate booleans. Pairing, pedal capture, source
admission, child release, callback cancellation and terminal retirement derive their
state from the records; there is no separately writable duplicate state.

`note_off(input, velocity)`, `key_up(note, velocity)` and
`Event::KeyUp(note, velocity)` accept `Option<f64>`. Present values must be finite
and in `[0, 1]`; absent velocity is distinct from an explicit zero. All new-core
callers use this contract directly. A key release records data when it executes,
not when a future event is queued. Equal timestamps retain submission order;
consuming the first key-up cancels subsequent key-up jobs for that note. An immediate
release needs no spare queue entry. Invalid velocity or a rejected future event
cannot consume a key or install a release record.

Repeated explicit closure preserves the first key and gate records. Anonymous input
note-off still pairs FIFO over the complete original input identity. Direct repeated
`key_up` on a retained handle is idempotent; a queued key-up requires a still-held key.
Old or foreign handles cannot access a replacement note's context.

## Closure causes and ownership

| Cause | Native transition |
| --- | --- |
| `KeyUp` | Direct, scheduled or adapter-delivered key release; closes the gate if no pedal hold remains |
| `Pedal` | A held release becomes effective after sustain/sostenuto allows it |
| `Explicit` | Native explicit or scheduled release, including generated duration expiry |
| `BehaviorCancelled` | Explicit abort of a pending native behavior |
| `BehaviorFault` | Behavior execution fault or fuel exhaustion closes its origin |
| `Parent` | Linked descendant closes with its parent |
| `AllNotesOff` | Channel-mode key release; pedals still apply |
| `AllSoundOff` | Hard silence of the addressed domain |
| `Panic` | Global hard cleanup |

Synthetic key consumption has no invented velocity. A physical note silenced by
All Sound Off retains its key for matching the real note-off; therefore its gate
record can precede its key record. Independent descendants keep their own clocks
and gates. Later panic/hard cleanup does not rewrite an earlier release event.
These are records of first transitions, not a complete history of every later
command or a universal vendor release-trigger policy.

Cause markers live in the logical note arena. Timestamps and optional velocity use
one control-preallocated array indexed by the owning note slot. This keeps cold
payload out of input-pairing and ownership scans without duplicating key/gate state.
Successful admission resets the cold slot before publishing the note; failed
admission leaves live contexts alone. Queries validate the complete generational
handle before reading its payload. The payload contains no owned allocation and is
destroyed with the stopped runtime on control. Context survives source EOF, family
choke, retained callbacks/children/pins and rejected host terminal delivery. Prepared
replacement leaves held notes and their context attached to their original generation.
Existing note velocity, pitch, plan and take queries provide the other already-owned
selection facts. No queue, callback allocation, reference-count update or new owner is
needed to retain release facts. Retirement invalidates access with the note; reuse resets the payload for its new owner.

## MIDI and evidence

Ordinary UMP ingress and fixed-zone MPE pass decoded optional release velocity into
the core. MIDI 1 Note Off retains its seven-bit value; MIDI 2 retains its sixteen-bit
value through normalized `f64` projection. MIDI 1 velocity-zero Note On remains an
absent release velocity, as represented by the decoder. The raw `Applied::Released`
velocity/attribute result remains available; unknown Note Off attributes continue
to release the paired note. No new attribute interpretation is claimed.

`sampler-core/tests/release.rs` checks physical FIFO pairing after attack EOF,
retained attack decision/plan after replacement, exact key and pedal times across
whole/split/empty blocks, same-time duplicate key-up, invalid velocity, full queues,
missing versus zero velocity, stale slot reuse, terminal rejection, linked and
independent descendants, explicit closure, panic and channel modes. Runtime operations
run under the allocation/deallocation guard. Existing behavior tests check cancellation
and deferred fault causes. MIDI ingress/MPE tests check precise values, both MPE zones,
unknown release attributes and velocity-zero Note On.

Four native crates pass debug/release tests, Rust 1.92 and strict all-target Clippy;
the root historical boundary tests pass separately. Workload costs are recorded in
[the local measurements](RENDER_WORKLOADS.md#retained-release-context-cost).

Next release-selection work must select attack versus current context explicitly,
permit independent release sequences, budget coherent release families under full
polyphony, and ensure panic/fault cleanup cannot accidentally spawn release audio.
Articulation and controller snapshots remain separate missing facts. This checkpoint
does not claim those selection/reservation contracts or vendor equivalence.
