# Headless controls and ownership

Implemented in `sampler-core::control`, independently of the old application and
any renderer. This is partial V2-13 evidence, not complete plugin state support.

`ControlId(u128)` is assigned persistently by the source frontend/composition root.
It is neither a widget ordinal nor a prepared-table index. Each definition has an
exact integer, finite real or toggle domain and a typed default. Preparation sorts
IDs and rejects duplicates, invalid ranges/defaults and incompatible program reads.
The core does not normalize, coerce or clamp values; vendor units and edit policies
belong to the source profile. Strings, arrays, read-only meters and dynamic schemas
remain separate obligations, not numeric encodings hidden in this API.

The prepared plan owns immutable definitions. Each retained generation owns a
separate value array and revision. These arrays are allocated with the plan on
control, moved into audio ownership, and returned with retired plans for off-audio
destruction. A waiting callback reads its originating generation. Replacement does
not silently share controls with the previous instrument; recall by stable identity
is explicit. UI closure or presentation replacement has no role in this lifetime.

## Mutation and capture

`edit_controls` validates the whole ordered, unique-ID batch before changing any
value. Exact type/range checks, unknown IDs, stale generation and optional expected
revision failures reject the complete batch. A nonempty accepted batch advances one
revision, including repeated values; an empty batch only validates the revision.
Revision exhaustion fails before mutation. `recall_controls` additionally requires
every declared ID exactly once. Arbitrary definition order compiles into the same
sorted identity schema, so a captured state survives table reordering.

`capture_controls` writes into caller-provided storage and returns the count and
one coherent revision. Insufficient capacity leaves the output untouched. These
methods are audio-owner operations, not permission for UI access to `Runtime`.
Immediate edits first execute already-due musical work; a callback at that boundary
can legitimately invalidate an expected revision. Capture through the queued path
also settles due work before reading. Direct capture observes the currently applied
state and does not itself advance the timeline.

Integer `ReadControl`/`WriteControl` instructions use these same values. References
and types are validated before activation; bounded callback fuel and outcome
retention remain in force. Native real/toggle state is available to control clients;
the existing integer behavior registers do not pretend to execute real arithmetic.

## Control/UI handoff

`with_control_updates(queued, max_values)` creates a bounded SPSC pair off audio.
One `ControlClient` serializes UI, state and other non-audio producers. Each request
contains its exact plan generation, optional expected revision and owned edit,
recall or capture storage. Submission limits both queue depth and payload length.
Rejected submission returns the caller's payload without publishing it.

`poll_control_update` processes at most one request at the current sample boundary.
It requires response capacity before taking a request. Every accepted request
returns its original storage, request number and success or explicit error. A full
response queue leaves later requests unchanged; values are not silently coalesced.
The control side consumes replies and reuses or destroys their storage. Even an
unexpected response push failure retains the reply without rerunning the command.
A disconnected client stops processing, retaining pending storage until off-audio
shutdown. Endpoint/runtime destruction belongs off audio after processing stops.

Cost is bounded by the configured payload length and binary ID lookup into the
prepared schema. No locks, allocation/free, string lookup or formatting occur in
processing. This channel applies transactions at the boundary where it is polled;
future-timestamp automation and host gesture notification are still adapter work.
Capture copies values, not the whole instrument/script/asset state. Complete product
state serialization and schema migration remain open.

## Source integration and checks

The new KSP compiler consumes explicit variable-name-to-persistent-ID bindings for
knob, slider, button and switch declarations. Presentation metadata stays on control;
integer state and callback access use the native service. See [KSP_FRONTEND.md](KSP_FRONTEND.md).

`sampler-core/tests/controls.rs` checks typed extremes, NaN/infinity/range rejection,
transaction rollback, coherent capture, strict recall, declaration reordering,
retained callbacks, replacement/stale generation, ordered due work, bounded queues,
reply backpressure, conflict acknowledgement and disconnected ownership. The capture
buffer returns with the same allocation address. Audio-side checks use the existing
allocation/deallocation guard. `sampler-ksp/tests/controls.rs` adds independent exact
PCM checks with the presentation dropped and block partitions 1/7/64, mixed note and
control state, release access, initialization and source rejection cases.

Validation: all 187 tests across the four native crates pass in debug, release and
Rust 1.92; strict all-target Clippy passes. Logs: `artifacts/controls-*`.

## Instrument-owned UI callbacks

A prepared `with_control_programs` table binds control identities to native programs.
`invoke_control` admits the value write and callback together: lack of continuation
capacity rejects before mutation. A handler fault after admission is a retained
outcome, not a rollback of already executed operations. The queued `Invoke` operation
returns the admitted `BehaviorId` with its acknowledgement. Plain edits, recall and
script assignments do not recursively dispatch UI handlers.

`BehaviorOwner` distinguishes `Note` from `Plan`. A plan-owned callback consumes the
same fixed continuation/local/fuel/command budgets, but no note or expression owner.
It explicitly retains the generation through waits and outcome backpressure. Note
operands and gate-lifetime programs are rejected before admission. Channel sound-off
does not cancel an unrelated UI handler; explicit abort and global panic cancel
its pending work. Plan-handler faults do not release unrelated musical notes.
`flush_behaviors` now returns this explicit owner, and release of a completed handler
permits off-audio plan retirement. Script-instance subdivision and non-note generated
musical events remain open; this is not fabricated-note dispatch.

Additional native/source checks cover overlapping plan callbacks, exact local
retention, edits rejected under callback saturation, MIDI cleanup isolation,
replacement without any note pin, retained outcomes, stale slot reuse, fault/fuel
cleanup, queued dispatch identity and KSP UI-driven playback. These use the heap guard.

This callback extension passes all 190 native tests in debug, release and Rust 1.92,
strict all-target Clippy and both root workspace boundary tests. Evidence is under
`artifacts/ui-callbacks-*`; it does not establish rendered UI or vendor fidelity.


## Shared values and native DSP

Prepared `GainControl` bindings now project scalar values into voice processing.
The existing atomic edit path updates both raw values and generation-owned smoothing
trajectories after validation; failed/revision-conflicting batches change neither.
Scripts, queued UI operations and full scalar recall all use this path. Stable IDs
remain valid when definitions are reordered, and missing bound IDs prevent preparing
a replacement schema. Multiple processors can bind one control with independent
amplitude endpoints/ramp lengths while sharing its single raw value owner.

Gain ramps use absolute sample time and are shared by the generation's voices.
New voices join the running trajectory; unchanged writes do not restart it. Rendering
does not mutate control values. The implemented mapping is native linear amplitude;
vendor units/curves and broader destination types remain explicit future work.
[VOICE_DSP.md](VOICE_DSP.md#shared-controls-driving-gain) records semantics and tests,
including a waiting KSP UI callback controlling an already-running native voice.
This does not mean the production window or host automation adapter has been ported.
