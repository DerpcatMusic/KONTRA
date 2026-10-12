# Retained native release context

Logical notes now retain their admission time, first key release and first effective
gate closure. This provides native release-policy input within V2-06/08.
[Release selection](RELEASE_SELECTION.md) now uses these facts with note-owned capacity
reservations; articulation/controller snapshots remain open. Source completion alone is not a key or gate transition.

## One authoritative lifecycle

`Runtime::release_context(note)` returns a copy of the note-owned `ReleaseContext`:

- `admitted_at`: logical admission on the monotonic engine sample clock, including
  generated notes. It does not pretend to be a delayed source's audible onset.
- `key: Option<KeyRelease>`: the first consumed key transition, with sample time,
  optional normalized release velocity and cause. `None` means the downstream logical
  key remains down; raw external key ownership is queried separately with `input_held`.
- `gate: Option<GateRelease>`: the first effective closure, with sample time and cause.
  `None` means the gate remains open.

The records own logical key/gate state. A separate `input_down` bit owns the raw
external key pairing; generated notes never acquire that bit. Script note-off and
callback faults can end the downstream event without consuming its host input.
FIFO matching, physical sostenuto capture and MPE member tracking use the raw
projection. Indefinite module-linked generated notes use their logical key for
sostenuto and retained channel for pedals, without a fabricated input. Source admission, child release and script `$NOTE_HELD` use the logical
projection. Retirement requires both external input consumption and completed logical
ownership; these are distinct lifecycle facts, not competing copies of one state.

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
`key_up` on a retained handle is idempotent; a queued physical key-up accepts either
a pending external input or an open logical key.
Old or foreign handles cannot access a replacement note's context.

## Closure causes and ownership

| Cause | Native transition |
| --- | --- |
| `KeyUp` | Direct, scheduled or adapter-delivered physical key release; consumes the raw input and closes the gate if no hold remains |
| `Script` | Downstream scripted note end/forwarding; retains the raw external key pairing |
| `Pedal` | A held release becomes effective after sustain/sostenuto allows it |
| `Explicit` | Native explicit or scheduled gate release; generated duration expiry now uses scripted key-up routing |
| `BehaviorCancelled` | Explicit abort of a pending native behavior |
| `BehaviorFault` | Behavior execution fault or fuel exhaustion closes its origin |
| `Parent` | Linked descendant closes with its parent |
| `AllNotesOff` | Channel-mode key release; pedals still apply |
| `AllSoundOff` | Hard silence of the addressed domain |
| `Panic` | Global hard cleanup |

Synthetic key consumption has no invented velocity. Script end, callback fault and
cancellation retain raw key pairing until its actual input note-off (or an explicit
owner abort/panic). They do not cause that later note-off to target a newer same-key
input. A physical note silenced by
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

The subsequent [release-selection checkpoint](RELEASE_SELECTION.md) adds explicit
phase/velocity policies, independent release sequences, coherent family reservations
and cleanup suppression. Articulation and controller snapshots remain separate missing
facts; neither checkpoint claims universal vendor release behavior.


`Event::ScriptKeyUp` and `replace_script_key_up_at` preserve queued physical key-ups
when replacing script deadlines. A script deadline survives a physical key-up whose
release callback suppresses forwarding; execution then resumes the held release once.
Physical note-off after an already ended script event consumes input ownership without
rewriting the original logical release context or dispatching a duplicate callback.
The existing explicit native `release`/`Event::Release` operation deliberately aborts
the input owner as well; source note-off and callback faults do not use that policy.

## Monophonic release-trigger groups

Source: KONTAKT_Manual.pdf p.205 (Group Editor, Release Trigger "Monophonic"):
repeated release samples of the same note cut the previous ones.
`ir::Group.monophonic_release` carries it; core cuts a new voice's same-key,
other-note earlier voices in that group (`cut_monophonic_release`, steal.rs).
The 10 ms cut fade is an assumption, not in the manual.
