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
