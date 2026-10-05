# Native articulation routing and snapshots

The native selector now has explicit musical performance domains, silent latched
switches, sparse articulation filtering and per-release onset/current policies.
This is partial V2-08 evidence. Controller predicates/snapshots, momentary/additive
switches, next-note overrides, phrase selection and persisted recall remain open.

## Domains and note identity

`Limits::performances` budgets a fixed number of domains at runtime construction;
it must be at least one. Each starts at articulation `0`. `performance(index)`
returns a runtime-bound `PerformanceId`; foreign IDs cannot address a domain in another runtime. Domains
exist for the runtime's lifetime and cannot be recycled under a retained note or
queued update. Their values use a control-allocated array; no new arena, lock,
reference count or per-event allocation is needed.

`trigger_in`, `note_on_pitched_in` and `note_off_in` select/pair in an explicit
domain. Convenience APIs select domain zero. Identical external input IDs or
anonymous tuples in different domains have distinct owners; pairing is FIFO within
the complete physical tuple **and** domain. The physical channel address stays
unchanged for MIDI expression, pedals and channel modes. A performance domain is
an articulation/routing scope, not an expressive MPE member channel and not a new
implicit pedal scope.

Ordinary `Ingress::apply_in` accepts a domain; `Mpe::new_in` binds an expressive
zone's notes to one domain. Their default entry points use domain zero. MPE member
pitch/pressure/timbre still retain independent expression owners; manager/member
pedal rules are unchanged. Physical channel-mode controls continue to address their
physical domain, including notes routed to multiple musical domains.

Every successful logical admission captures its domain and current articulation in
a cold, preallocated note-slot record. `note_selection(note)` returns the snapshot
and whether the input was a consumed switch. Full generational-handle validation
prevents stale slot access. The record survives source EOF, child/callback retention
and rejected terminal delivery, and resets only on successful slot reuse. Existing
hot note records do not grow. Children inherit their parent's domain and original
prepared generation, but capture articulation at their own logical admission time.
Expression inheritance remains a separate choice.

`set_articulation` and `Event::Articulation` use the shared timeline. Equal-time
updates/releases obey submission order and exclusive-end/empty-block rules. A
failed future queue submission cannot mutate state. Panic cancels queued work and
cleans notes; it does not silently reset an instrument's latched articulation.

## Prepared filtering and release policies

`Prepared::with_articulations(region_tags, switches, key_policy, gate_policy)`
accepts one optional `u32` tag per region in original authoring order. `None` means
an unconditional layer, and every `u32` label is valid, including sparse labels.
The builder composes with variation and release compilation in any order.

Candidates remain indexed by key and phase, then sequence, articulation and original
region order. Within each sequence, binary partitioning yields two sparse ranges:
unconditional candidates and candidates for the selected articulation. Preflight
and commit each reuse those ranges for eligibility, take choice and source selection;
no key × articulation × take × microphone table is materialized. Inactive groups
neither draw nor record a take. Existing sequence scopes retain their declared
meaning; a global sequence remains global, not implicitly per performance or per
articulation. Additional variation scopes remain separate work.

For each release phase, `SelectionPolicy::Onset` (default) uses the owning note's
snapshot; `Current` reads that same performance domain at the phase's actual
transition. Velocity policy, release timing, expression and mapping generation
remain independent facts. A held old-generation note uses its original mapping and
policy after replacement. A delayed child can use current domain state with the
original mapping, while retaining its own new onset snapshot.

The control-time release-bound sweep now combines unconditional overlaps with the
largest exclusive articulation overlap for each take, then takes the maximum over
takes. It sums independent sequence groups. This avoids reserving all exclusive
articulations' microphones simultaneously. Bounds remain conservative across
sequence groups, velocities and possible future articulation; it is not a solver
for every correlated predicate combination. Resource ownership and cleanup retain
the [release-selection contract](RELEASE_SELECTION.md).

Pending onset-policy pitch checks use only the snapshot's sparse candidate ranges;
current-policy checks cover all reachable articulation tags. Known release velocity
also filters those checks. This prevents accepted expression changes from invalidating
later releases. Scope-owner preclaim skips groups unreachable under known onset
articulation/velocity; current or unknown state reserves every potentially needed
owner without advancing its sequence.

## Silent latched switches

`Keyswitch { key, articulation }` maps a **physical input key** to a latched value.
Out-of-range or duplicate keys fail preparation. A mapped input first admits a
normal bounded logical owner, then updates its domain and marks the input consumed.
Failure to admit changes neither articulation nor ownership. Consumed inputs start
no source, release phase or bound note program; generated children do not reinterpret
keyswitch mappings.

The silent owner preserves exact input pairing and terminal retry, so a repeated
switch's note-off cannot accidentally close a musical note. It consumes ordinary
note/expression capacity. Sustain and sostenuto do not keep consumed switches alive
after key-up or All Notes Off. Their release does not undo the latch. APIs do not
pretend that this implements momentary, additive or next-note-only switch policies.

## Evidence

`sampler-core/tests/articulation.rs` runs runtime operations under allocation/free
guards and checks:

- Onset/current key and gate release PCM, independent phase reevaluation, attack EOF
  and retained snapshots through terminal rejection.
- Identical tuples in separate domains, expressive-channel changes inside a domain,
  foreign IDs, construction bounds and failed switch admission.
- Silent switches under sustain/sostenuto, physical versus transformed keys, repeated
  anonymous input pairing, and suppression before bound programs.
- Equal-time event order across whole/split/empty blocks.
- An independent linear evaluator over 120 gestures, two phases, sparse articulation
  labels, velocity layers, coordinated microphones and sequence decisions.
- Dormant pitch and scope preclaim, delayed children, original-generation mapping,
  current admission snapshots and off-audio plan retirement. Mixed silent/musical
  input pairing after replacement and reused switch slots preserve note-off/pedal behavior.

MIDI ingress tests check explicit domain pairing. An MPE test selects articulation
from one expressive member and plays it on others in a nondefault domain, while
keeping expression owners separate and the default domain unchanged.

The four native crates pass debug/release tests, Rust 1.92 and strict all-target
Clippy; root historical boundary tests pass separately. Local admission/indexing
measurements are in [RENDER_WORKLOADS.md](RENDER_WORKLOADS.md#articulation-selection-cost).

This does not claim full Kontakt articulation/KSP behavior or vendor equivalence.
The user completion target remains the entire core, production UI integration,
upgraded sample section and full Kontakt KSP parity; these native tests are
intermediate evidence, not that completion gate.
