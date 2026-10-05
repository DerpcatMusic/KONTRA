# Native articulation routing and snapshots

The native selector now has explicit musical performance domains, silent latched
switches, sparse articulation filtering and per-release onset/current policies.
This is partial V2-08 evidence. Controller-triggered notes, momentary/additive
switches, next-note overrides, phrase selection and persisted recall remain open.

## Domains and note identity

`Limits::performances` budgets a fixed number of domains at runtime construction;
it must be at least one. Each starts at articulation `0`. `performance(index)`
returns a runtime-bound `PerformanceId`; foreign IDs cannot address a domain in another runtime. Domains
exist for the runtime's lifetime and cannot be recycled under a retained note or
queued update. Their articulation/controller values use construction-bounded shared
versions; no lock, atomic reference count or per-event allocation is needed.

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

## Effective controllers and shared state versions

`controller`, `set_controller`, `Event::Controller` and `note_controller` use native
unsigned 32-bit full-scale values for 128 CC slots. Each starts at zero. These are
**effective downstream** values, committed after an event-processing stage accepts
an event; they are not raw MIDI input history or per-note MPE expression. Full
script interception remains separate pending work.

Each musical domain owns its current state version; each successfully admitted
note retains its onset version. A private pool has exactly `notes + performances`
slots. A version contains articulation, all 128 CC values and a non-atomic owner
count. Updating an exclusively owned version changes it in place. Updating a
shared version copies its fixed primitive payload into a free slot first. A shared
version proves that the number of distinct live versions is less than the number
of owners, so a free slot exists even when every note slot is occupied. This does
not introduce a fallible controller-update budget. Identical updates do nothing.

No private version index escapes. No public generational arena is repurposed, and
no `Arc` destruction, heap allocation/free or lock occurs during capture, update
or retirement. The last owner returns the slot to a construction-reserved free
stack. The snapshot is released at actual logical retirement, not source EOF,
key-up, gate closure or rejected terminal delivery. Failed admissions do not retain
an owner. Children capture current domain state at their own admission independently
of expression inheritance. A consumed switch records the resulting articulation
and the existing controller state. Panic retains current musical state and cancels
queued updates.

The cost is bounded but real: on 64-bit targets, each state uses 528 bytes and the
pool reserves its worst-case distinct-note snapshots at construction. Admission
retains one private index without copying 128 CCs; the first changed update while
notes share a domain version copies that payload once. Dense changes without a new
snapshot mutate the current version in place. This is not sample-rate modulation,
a persistence format or an unlimited controller namespace.

Ordinary MIDI ingress commits CC0–119 to its explicitly routed domain; CC64/66
also apply their physical pedal scope. Native full-scale projection of MIDI 1 uses
integer `value * u32::MAX / 127`; MIDI 2's 32-bit value is unchanged. This is not a
claim of protocol-level MIDI 1-to-UMP bit replication. Channel-mode messages are
not ordinary CC bank updates: supported modes retain their cleanup semantics and
unsupported reset/configuration remains explicit.

`set_pedal_controller` preflights scope capacity before committing the value, then
updates gates. Current release selection therefore observes the new value, while
failed pedal-down changes neither bank nor gate. Pedal-up requires no free channel,
command or snapshot slot. Direct `sustain`/`sostenuto` APIs change gates only, and
`Event::Controller` changes selection state only; explicit adapters join them.
Physical scope may cover notes in several domains; the accepted CC updates only
the adapter's selected musical domain.

MPE accepts ordinary selection CCs from its manager. Member CC74 remains expression,
member pedals remain ignored, RPN selectors retain their existing interpreter, and
manager CC74 retains its expression behavior. Unsupported member controllers or
channel modes are not silently promoted to global musical state.

`tests/controllers.rs` exercises 100 full-pool cycles, all 128 slots, sustained
updates, distinct domains, terminal retry, failed admission, child/pin retention,
one-bit precision, timeline boundaries and invalid targets under heap guards.
MIDI tests cover both resolutions, MPE isolation, ignored/unsupported events and
failed zone-pedal publication. Logs: `artifacts/controller-{debug,release,msrv,clippy,boundary}.log`.

## Prepared filtering and release policies

`Prepared::with_articulations(region_tags, switches, key_policy, gate_policy)`
accepts one optional `u32` tag per region in original authoring order. `None` means
an unconditional layer, and every `u32` label is valid, including sparse labels.
The builder composes with variation and release compilation in any order.

Candidates remain indexed by key and phase, then sequence, articulation, interned
controller condition set and original region order. Within each sequence, binary partitioning yields two sparse ranges:
unconditional candidates and candidates for the selected articulation. Preflight
and commit each reuse those ranges for eligibility, take choice and source selection;
no key × articulation × take × microphone table is materialized. Inactive groups
neither draw nor record a take. Existing sequence scopes retain their declared
meaning; a global sequence remains global, not implicitly per performance or per
articulation. Additional variation scopes remain separate work.

For each release phase, `SelectionPolicy::Onset` (default) uses the owning note's
articulation/controller snapshot; `Current` reads that same performance domain at the phase's actual
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

## Compiled controller conditions

`with_controllers` accepts one conjunction of inclusive `ControllerCondition`
ranges per region in original authoring order. Its explicit condition budget checks
the total input count before preparation. It rejects invalid controller indices,
inverted bounds and contradictory conditions on the same controller. Duplicate
conditions are intersected, full-range tests removed and equivalent canonical sets
interned. Empty conjunctions are unconditional. All comparisons preserve exact
32-bit boundaries, including adjacent values around the midpoint and endpoints.

Selection first narrows candidates by key, phase, sequence and articulation. A
small cursor reads control-compiled run boundaries, evaluates each contiguous
condition run once per selection pass and skips a rejected run; matching microphones reuse the result. It owns only indices,
so committing voices needs neither candidate copies nor mutable prepared data.
Condition sets repeated in separate sequence/articulation runs are reevaluated;
this explicit ceiling should drive any future sparse-cache optimization. Run-end
indices cost one `usize` per candidate only in plans with nontrivial conditions. No
key × controller × articulation × take table is created.

Take choice sees the first eligible candidate before any ownership changes.
Wholly ineligible gestures do not advance or claim a sequence; an eligible gesture
whose selected take has no mapped region still records/advances that take. Preflight
and commit traverse the same immutable context. Controller updates alone generate
no note, source, family or take decision.

`with_release_selection(key_policy, gate_policy)` chooses onset/current independently
for both release phases, including controller-only instruments. These policies
apply to the coherent articulation/controller version together; velocity, mapping
generation and expression retain their separate contracts. Onset dormant-pitch and
sequence-scope checks skip unreachable controller conditions. Current policy checks
all possible future controller conditions so later cleanup cannot introduce an
invalid playback rate. Generated delayed children retain the original mapping but
capture their own current domain version after replacement.

Release preparation now bounds controller overlap by projecting each independent
sequence group onto every CC it uses. Each projection ignores the other conditions,
so its maximum simultaneous overlap is a safe upper bound. The minimum of these
bounds is still safe; summing independent groups and taking the minimum with the
existing velocity/articulation sweep tightens the final source quota. It does not
reduce family/decision/command bounds without separate evidence. Missing CC
conditions cover the full range; a `u64` exclusive end represents `u32::MAX + 1`
without wrapping. Each take is swept separately, with shared inclusive endpoints
counted as overlapping.

The 64 mutually exclusive CC groups in the admission workload now reserve only
one group's microphones. General multi-controller correlations may still make the
bound conservative: this is a set of one-dimensional projections, not a Cartesian
solver or per-note callback calculation. All allocation/sorting stays in control
preparation; the runtime reservation/admission contract is unchanged.

Two additional guarded tests exercise 64 exclusive groups plus an unconditional
mic at exact source capacity, including `u32::MAX`, and an independent grid over
two CCs, articulation, velocity, independent sequences and alternating takes.

[Admission measurements](RENDER_WORKLOADS.md#controller-snapshots-and-predicate-selection-cost)
record both added cost and the conservative reservation geometry.

`tests/controllers.rs` compares audible attack/key/gate selection with an independent
linear evaluator over 128 gestures for each policy, coordinated takes and multiple
microphones. It also checks inclusive bounds, canonical intersection, rejected
preparation, unmatched gestures, unmapped takes, and direct/batch/scheduled dormant
pitch changes, plus audible equal-time controller/release order across whole, split
and empty blocks. The articulation replacement test now combines CC and articulation
conditions across old delayed children/new mappings and off-audio retirement.

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
