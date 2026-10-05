# Coordinated native take selection

This implements sequential round robin and retained take decisions within V2-08.
It is native behavior, not a Kontakt/Falcon/SFZ interpretation. Random policies,
release-trigger mapping, articulation state and persisted sequence snapshots remain
open; this does not close the complete variation or selection gate.

## Authoring and compilation

`Prepared::with_variation(sequences, region_takes, max_states)` attaches one optional
`Take { sequence, index }` to each region in its original authoring order. A
`Sequence { takes, scope, capacity }` declares a positive take count and a bound on
distinct scope owners. Sequence and take indices are local to that prepared plan.
Unknown sequence/take references, wrong mapping lengths, zero counts/capacities,
state-budget overflow and invalid allocation sizes fail on control.

Each key's existing candidate list is grouped by sequence. Within a group,
authoring order is preserved. Unconditioned layers form one family; each eligible
sequence forms a separate coordinated family containing its selected layers. Four
microphones tagged with the same take therefore make one decision, not four.
The compiled mixing order is unconditioned layers, then ascending sequence index,
then original region order within each family. Unvaried instruments preserve their
previous region order. No key × channel × take × microphone Cartesian product is
materialized, and plans with no sequences skip group lookup.

## Scope and advancement

| Scope | Counter owner |
| --- | --- |
| Global | One owner within this sequence and plan generation |
| Key | Logical selection key; absolute pitch uses its integer part |
| Channel | Original protocol, port, UMP group and channel |
| ChannelKey | Original channel address plus logical selection key |

Generated notes retain their original input address and select with their own
logical pitch. Sequence declarations remain independent even if their scopes match.
Global storage is capped at one cell and key storage at 128; channel scopes reserve
the declared capacity. The sum must fit `max_states`. Cells are claimed only on
successful selection and remain assigned for the generation's lifetime. A new owner
at full scope capacity fails explicitly; musical history is never silently evicted.

The native advancement rule is **once per successfully admitted eligible logical
note**. Key and velocity eligibility is evaluated across all takes in the group.
The current take is selected and the counter advances modulo the declared count.
A deliberately unmapped take produces a no-source note with a retained decision
and still advances. A completely ineligible gesture does not claim a scope or advance.
Bound note programs suppress root selection as before; their generated notes use
the same selection service. This rule is not asserted for imported profiles.

Every group's selected source rates and the aggregate voice, family and decision
budgets are preflighted before admitting the note. Failed scope, source-rate,
family/voice/decision capacity, input identity or note/expression admission does not
advance any sequence or publish a partial microphone set. Counter rollover is safe
even at the largest `u32` take count.

## Decision and generation ownership

`Limits::decisions` is a separate runtime budget. Each eligible sequence consumes
one record owned by the logical note. Its sounding family borrows that record;
family retirement does not erase the decision. Physical key retention, descendants,
callbacks, manual pins and rejected terminal notifications continue to retain the
note and its decisions. Completed internal notes can reclaim decision capacity under
pressure without consuming external terminal notifications.

`note_take(note, sequence)` reads the retained choice. `note_families(note)` borrows
the live family set, and `family_take(family)` identifies its coordinated decision;
manual/unconditioned families return `None`. These APIs validate public generational
handles and expose no internal slot addresses. Clients must interpret sequence indices
using the note's retained `note_plan`, not the current active plan.

Immutable prepared metadata and mutable sequence counters are separate. Initial
runtime construction and `PlanControl::submit` allocate the sequence-state box on
control. The box moves with its generation through pending/adopted/retired ownership,
including lossless retirement rollback. Audio never allocates, resizes or destroys
it. A replacement starts fresh counters; retained old notes and their later children
continue using the old generation's counters and regions. There is no implicit
cross-generation state migration. Snapshot/restore and stable serialized sequence
identities remain future persistence work.

Scope lookup currently scans that sequence's bounded cells. Selection visits only
the chosen key's candidates, but evaluates candidate groups during preflight and
admission. Decision history uses the existing fixed generational arena and private
note-owned links. These are explicit cost bounds, not constant-time claims at
arbitrary instrument sizes.

## Executable evidence

`cargo test --locked -p sampler-core --test variation` exercises:

- An independent linear evaluator over 400 deterministic gestures (LCG seed 991),
  four scopes, mixed protocols/ports/groups/channels, logical keys, velocity
  predicates and four tagged microphones per take. It compares selected decisions,
  source/family counts and exact stereo PCM across varying block partitions.
- Decision retention through source EOF and terminal rejection, stale handles,
  separate decision/scope/family/voice pressure and aggregate failure rollback.
- Invalid preparation and allocation bounds, unmapped versus ineligible takes,
  absolute-pitch logical keys and selected-only live-pitch preflight.
- Program-generated decisions under repeated capacity pressure, with host terminals
  retained and exact generated-note audio.
- New-generation reset and an old delayed child choosing the old generation's next
  take after adoption. Mutable state returns for control-side destruction.

The final-counter boundary has a separate unit test. The existing actual two-thread,
64-plan transfer test now also allocates, adopts and retires sequence-state boxes.
Runtime selection/render/reclamation paths are guarded against both allocation and
deallocation. All four native crates pass debug/release and Rust 1.92 checks, strict
all-target Clippy, and the root historical boundary tests pass separately.

This directly exercises `VARIATION_AND_RELEASE_SELECTION_01`'s coherent-microphone
contract and the global/key/channel portion of `_03`. Articulation state in `_03`,
independent release selection (`_02`), restored snapshots (`_04`), random/shuffle
policies (`_05`) and vendor rules/probes (`_06`–`_08`) remain open. The supplied
scenario catalogue stays byte-for-byte unchanged; no vendor observation is inferred.

The admission workload accepts `--variation` for three global takes with four
microphones each, optionally combined with `--ids`. It preserves the same admitted
voice counts while measuring the selection/record cost separately from release and
retirement. See [local workload evidence](RENDER_WORKLOADS.md#coordinated-variation-cost).
