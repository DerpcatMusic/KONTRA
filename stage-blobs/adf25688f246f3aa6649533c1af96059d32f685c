# Native release selection and reserved ownership

Native prepared instruments can select independent attack, physical key-release and
effective gate-release layers. This extends V2-06/08; it does not complete articulation,
half-pedal/repedal, persisted state, vendor importing or the production plugin.

## Prepared contract

`Prepared::with_releases(triggers, key_options, gate_options)` assigns a `Trigger` to
each region in original authoring order. The default remains `Attack`. Compilation
indexes candidates by key, phase and sequence. It works before or after
`with_variation`; phase selection uses the same transactional native selector.

Each release phase has `ReleaseOptions`:

- `ReleaseVelocity::Onset` uses the note's retained admission velocity.
- `KeyUp { fallback }` uses the retained optional physical velocity, with an explicit
  fallback for absent/synthetic values. Zero is a present value. Selection and source
  gain use the resolved velocity. Invalid fallback values fail preparation.
- `duration: None` requires unlooped sources, which finish naturally.
- `Some(frames)` closes the selected family's own gate after that many sample frames,
  using each source's envelope release and loop-exit rules. Zero closes immediately.
  This is not a hard-stop timer; a release envelope may continue afterward. Looped
  release regions require this finite duration.

Different sequence indices give phases independent counters, random streams or bags.
Reusing one sequence index intentionally shares its progression across phases. Every
accepted eligible phase records its own take, including an unmapped selected take;
completely velocity-ineligible groups do not advance. `note_take(note, trigger,
sequence)` preserves all three decisions separately. `family_trigger(family)` and
`family_take(family)` identify a live family's role and decision.

Selection uses the original prepared generation, inherent pitch and current expression
owner. [Articulation policy](ARTICULATION.md) now explicitly chooses onset snapshots
or current performance-domain state per release phase; controller snapshots remain open.
`trigger` arms release phases; manual `note_on`/`child` admissions do not. A bound note
program owns its source policy: generated `Play` children use the selector and arm
releases, while the intercepted program root remains unarmed.

## Admission owns future capacity

Before publishing an attack, the selector checks its immediate resources plus both
release phases' worst-case voices, families, retained decisions and duration commands.
A control-time sweep over closed velocity intervals computes each phase's bounds:
within a sequence, reserve its largest simultaneously eligible take, combining
unconditional layers with the maximum exclusive articulation overlap; sum independently
selectable groups. Shared endpoints, including signed zero, overlap. Disjoint velocity
layers and alternative takes do not all consume concurrent voice reservations.

Bounds are per key and phase, conservative across all velocities and sequence positions.
They are not recalculated using mutable counters in the callback. Each pending phase
owns its compiled quota until it selects, fails or is suppressed. Arena and command
availability subtract outstanding quotas, so manual starts, other attacks, delayed
sources, behavior waits and future commands cannot consume them. Generated duration
commands reserve their own slot across child admission as well. Immediate physical
cleanup requires no new queue slot.

Release sequence scope owners are claimed at successful note admission without drawing
or advancing a take. Scope exhaustion rejects admission transactionally; a later note
cannot steal the owner slot promised to a pending release. These scoped histories remain
owned by their generation until retirement, as with attack variation.

A phase returns its entire quota immediately before committing its actual complete
selection. Selected resources then have ordinary note/family ownership. No new note or
expression owner is allocated for release playback. Unused quota returns immediately,
including missing-take or ineligible selections. Attack EOF, a pedal hold and prepared
replacement do not erase reservations. `release_reserve()` reports outstanding quotas,
separately from live resource counts.
`set_release_stealing(true)` relaxes this for dense instruments: an attack that only
outstanding reservations keep from fitting suppresses the pending release phases of
the oldest notes (returning their quotas) instead of failing `Capacity`, and only when
those reservations could cover the shortfall. It is off by default.

Pitch changes validate both active sources and pending release candidates. Known onset
velocity excludes unreachable velocity layers. Unknown physical release velocity checks
all possible release layers; after key consumption, a pending gate phase can use the
known velocity/fallback. Direct, batched and scheduled expression changes share this
validation, preserving transactional failure.

## Musical transitions and cleanup

| Transition | Key phase | Gate phase |
| --- | --- | --- |
| Physical key-up | Select once | Select when pedals permit closure |
| All Notes Off | Select with fallback if configured | Pedals still apply |
| Explicit/duration closure | Suppress synthetic key phase | Select once |
| Linked parent musical closure | Suppress synthetic key phase | Select once |
| Panic, All Sound Off, behavior cancellation/fault | Suppress pending phase | Suppress pending phase |

Hard/fault causes propagate to linked descendants instead of becoming a musical
parent closure. All Sound Off retains a host key for pairing; its later physical
key-up cannot resurrect suppressed audio. Existing first-transition release context
is not rewritten by later cleanup.

Ordinary note gate closure releases attack families. Key and gate release families
keep their own finite lifetimes, so pedal-up does not immediately kill a still-playing
key release. `release_family` and `Event::ReleaseFamily` explicitly close an individual
family gate; family choke remains independently available. Natural family retirement
removes its future release/choke jobs immediately, freeing command capacity and
preventing stale work from targeting a reused slot. Cancellation scans the bounded
command vector only when a family retires; no new scheduler or owned heap payload is
introduced.

`release_status(note, trigger)` returns `Unarmed`, `Pending`, `Selected`, `Suppressed`
or `Failed(error)`. `Selected` means the transaction ran, including a valid zero-source
selection; it does not mean audio is still playing. Physical cleanup is never rolled
back by selection failure. Clock overflow or a bounded random-draw failure consumes
the phase, returns its quota and records the error without partial sources or counter
advancement. Phases are separate transactions. Resource exhaustion after a successfully
reserved admission is an internal invariant violation, not an ordinary release policy.
Status and decisions survive source completion and rejected terminal delivery. Queries
validate the full generational note handle; retirement invalidates them.

## Executable evidence and limits

`sampler-core/tests/release_selection.rs` checks:

- Saturated two-note/four-microphone pedal bursts, attack EOF, independent phase
  histories and explicitly shared sequences against expected PCM.
- Missing takes, release-scope preclaim/rollback, present-zero versus absent velocity,
  synthetic fallback, finite loop duration and exclusive-end/empty-block boundaries.
  Manual family release/choke preserves siblings and cancels its own scheduled timer.
- Velocity bounds against an independent grid evaluator over 64 varied instruments,
  including 576 releases at exactly bounded capacities; shared endpoints and signed
  zero have a separate focused case.
- Dormant expression validation, original-generation release mapping after replacement,
  generated duration command ownership, cancellation/fuel-fault descendant cleanup,
  repeated hard cleanup, terminal retention and complete resource reclamation.

Runtime operations in those tests run under the allocation/deallocation guard. The
internal clock-overflow regression checks failed phase ownership and cleanup. The four
native crates pass debug/release tests, Rust 1.92 and strict all-target Clippy; the root
historical boundary tests pass separately. Existing
release-context, choke, variation, scheduling, MIDI/MPE, behavior and native entry suites
remain part of validation. Workload measurements are recorded in
[RENDER_WORKLOADS.md](RENDER_WORKLOADS.md#native-release-selection-cost).

This provides native evidence for independent release selection; it does not assert
any vendor's release rules, attack-variation/release-variation pairing policy or import
compatibility. Articulation/controller snapshots, full MIDI 2 expression, persistence,
streaming and production host/UI integration remain open. The supplied scenario JSON
remains unchanged; partial native evidence does not mark its full scenarios executed.
