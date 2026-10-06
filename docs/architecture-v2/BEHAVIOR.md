# Native behavior execution

This is a clean-sheet, bounded musical instruction path. It does not use the old
parser, VM or compatibility bridge. A [new KSP source subset](KSP_FRONTEND.md) now
compiles into it. This remains partial V2-05 evidence, not full KSP/Lua support or
completion of the behavior milestone.

## Executed contract

Control-side `Program::new` validates immutable instructions. A prepared plan owns
its program table and optional note binding, validated together by `with_programs`.
A bound `Runtime::trigger` admits an input owner and runs the program instead of
automatic region playback. An empty program therefore suppresses playback while
preserving physical note pairing and terminal delivery. Generated children select
native regions directly; they do not recursively invoke the bound program.

The current instructions are deliberately concrete:

- `Play`: transpose the logical key, scale velocity, select the complete native
  layer set and create a generated child with a sample-duration release.
- `Wait`: suspend until an absolute engine sample boundary; zero executes inline.
- `End`: finish the callback. Reaching the instruction array's end also finishes.
- `SetLocal` / `AddLocal`: callback-local signed 64-bit integers with checked addition.
- `ReadKey`: read the originating note's logical key into a local.
- `ReadKeyDown`: read that owner's downstream logical key state, independently of its gate.
- `CompareLocal`: compare two signed locals and replace the left with 0/1; all six
  equality/order relations work at i64 extrema without subtracting or wrapping.
- `ReadNoteCell` / `WriteNoteCell`: transfer a local to/from note-owned integer state.
- `Jump` / `JumpIfZero`: validated instruction targets; all branches consume fuel.

Transposition changes logical pitch and region selection; resident sources use the
prepared tuning and bounded [resampler](RESAMPLING.md). Fixed-pitch regions remain
independent of key pitch.

Generated children select linked, snapshot or independent expression inheritance,
and fixed or scaled velocity, separately from their explicit duration:
`Gate` follows the originating effective gate, `Frames` is independent, and
`FramesOrGate` ends on whichever condition happens first; `UntilSilent` has an
independent gate and can retire after all owned sound/work finishes. The default wait lifetime
is gate-bound; `WaitLifetime::Callback` retains a callback across input release.
Sustain can keep an effective gate open after physical key-up. Faults and explicit
abort force root release and cancel pending gate-bound work. Already-admitted
independent notes keep their scheduled release and retain the input ancestor until
finished. Existing envelope tails finish normally; panic hard-stops all voices and
cancels every pending callback regardless of its wait lifetime. These are explicit
native policies, not claims about vendor cancellation behavior.

## Ownership and bounds

Each callback occupies one generational continuation slot and privately pins its
originating note. `Limits.behaviors` bounds slots and `behavior_fuel` bounds
instructions per synchronous run/resume. Programs, PCM and all arenas are prepared
off audio. No callback allocates, frees, locks, sleeps or performs file IO.

A callback record remains allocated after `Finished`, `Cancelled`, `FuelExhausted`
or `Fault(Error)`. `flush_behaviors` releases its private pin only when the sink
accepts that outcome. Rejection stops retry for that call. This prevents both lost
faults and premature host terminal notifications. Public `unpin` cannot consume
these private pins. A full continuation pool rejects a new bound input before
publishing any note. Every resource failure is observable.

The existing stable command queue now also holds resume actions. Waits enqueue
strictly future boundaries; zero waits consume fuel inline. Native service calls
inside a queued resume do not recursively drain later equal-time work. This keeps
callback execution ordered and its synchronous changes immediately visible.
Previously submitted same-frame events still execute in submission order, and
exclusive block-end work waits for the next render call, including an empty call.

Native helper costs remain bounded by prepared region candidates and arena/queue
capacities. Fuel is an instruction bound, not a time guarantee. A frame can execute
up to the admitted callbacks' fuel plus previously queued musical work; future
production budgets need workload measurements on target hardware.

## Invariant review

Remaining checked-index/unwrap diagnostics are retained, not suppressed. Internal
resume IDs originate only in the continuation arena; records cannot retire before
acceptance, and cancellation removes their queued resumes. Records privately pin
notes, so their owner lookups remain valid. Program indices are validated before
admission and the program table cannot mutate while the runtime owns it. Instruction
fetch checks its bound. Generated layer admission shares the existing prepared
selector and whole-family preflight. No untrusted bytecode pointer enters rendering.

## Evidence and runnable example

Behavior checks cover blocks 1–32, generated-note audio, suppression, duration,
exclusive-end and empty rendering, stable equal-time order, invalid programs/keys,
clock overflow, queue/voice/continuation saturation, fuel exhaustion, deferred
faults, panic, explicit abort, rejected completion and cross-runtime stale handles.
Allocation/free counters wrap actual program execution, waits and retirement.

The independent executable now offers:

```sh
CARGO_TARGET_DIR="$PWD/target-core" cargo run --locked --release -p sampler-native -- echo /path/to/new-echo.wav
```

The echo starts through native MIDI 2.0 ingress and the shared block processor.
It generates two 125 ms notes at frames 0 and 12,000, each with the region's 50 ms
release. The rendered 48 kHz file contains identical 8,400-frame voice waveforms,
an exact silent gap and an exact silent suffix after frame 20,400. It accepts one
original-input terminal. Existing output files are refused.

Rust 1.99 release/all-target Clippy, MSRV 1.92 and the historical v2 boundary checks
pass. The full-workspace comparison against `2643eb9` has no new errors.
The current authoritative, complete new-core Doctor scan
meets **90**, zero errors, 115 warnings, with all rules retained. Local test, scan
and audio evidence is under ignored `artifacts/architecture-v2/behavior-*`.

## Still open

Broader language parsing, persistent state, real/string values and further array operations, note-handle
locals, controller callbacks, consumed controller projections, rich engine queries,
beat/transport waits, async services and compatibility profiles are not implemented.
Their implementations must use these ownership/time services and extend the
executable contracts. No old-VM fallback is present.

## Callback-local state and bounded branches

Preparation derives each program's local count from validated 16-bit operands.
Runtime construction reserves a fixed stride from `Limits.behavior_cells` divided
by continuation capacity and checks each program width and allocation layout before
allocating. [Plan replacement](PLAN_ADOPTION.md) validates new widths against that
same reserved stride; old callbacks keep their original program and local bounds. A callback clears its own local range when
admitted. Values survive waits and completion backpressure; reused slots start at
zero. `behavior_local` validates both the generational handle and program-local
bound, so a caller cannot inspect another callback's cells.

Branch targets are validated before activation. A yielding loop can continue while
its originating note is held, with at most one queued resume per callback. A loop
without a positive wait exhausts instruction fuel and follows the native abort
policy. Integer overflow reports `Fault(ArithmeticOverflow)` instead of wrapping.
These are native arithmetic semantics, not a vendor-language emulation rule.

Tests run overlapping callbacks with different key-derived counters across blocks
1–16, and check exact independently expected audio. Additional checks cover reused
slots, reads at the exclusive wait boundary, stale handles, oversized local tables,
invalid branches, positive/negative integer overflow, zero-time loops, and release
of indefinitely yielding loops. Actual operations remain allocation/free checked.
The native echo now uses a local counter and loop instead of two written-out plays;
its rendered audio is byte-identical to the earlier straight-line program.

The first expanded scan scored 89. Shared checked local access and fault propagation
removed unchecked accesses without disabling rules; the authoritative scan is back
at 90. Local evidence uses the `artifacts/architecture-v2/locals-*` prefix.

## Independent callback and generated-note lifetimes

Continuation retention, generated duration and expression inheritance are separate
ownership decisions. `with_wait_lifetime(Callback)` permits waiting work on a still-
owned closed note, allowing later release-side processing. It cannot resurrect an
already retired handle. Completion still requires acceptance before the private
input pin is released. An input-linked generated note on an already-closed gate is
rejected; fixed-duration generation can proceed and retain the closed ancestor.

New checks compare independent and gate-bound waits across blocks 1–16, explicit
duration modes, closed-note callback admission, panic and explicit abort. A separate
fault case verifies that an independent child's scheduled release remains owned
and executable after its callback faults; the original input cannot retire early.
All paths are allocation/free checked. The core score remains 90. Evidence is under
`artifacts/architecture-v2/lifetime-*`.

Frontend research is pinned to the KSP manual showing **Kontakt 8.12**, consulted
2026-10-05: [manual version](https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/welcome-to-ksp),
[generated-note duration](https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/general-commands),
[wait timing](https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/time-related-commands),
and [release callbacks](https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/callbacks).
These references motivate distinct ownership policies. No Kontakt binary comparison
or complete language implementation is implied. Source-duration zero is now covered
by the explicit source-owned lifetime below; vendor rounding/fidelity remain unverified.

## Generated-note reclamation under pressure

Before a behavior admits another generated note, exhausted note or expression
capacity triggers a bounded internal-owner sweep. Closed notes retire only after
all families, children, public pins and private work pins are gone. External input
owners remain allocated until `flush_ended` accepts their terminal; a waiting or
rejected external terminal does not block reclamation of unrelated internal notes.
Code retaining an internal note handle across later generation must pin it until
finished with it. Reclamation allocates/frees no heap memory and shares the same
iterative retirement path as explicit terminal draining.

The regression first failed with `Fault(Capacity)`. It now renders 100 short notes
using three note slots (two retained inputs and one reusable child), and separately
with three expression slots. Blocks 1, 7, 64 and 256 produce identical expected
pulses while the terminal sink rejects delivery. Completion and both original
terminals are accepted exactly once afterward. This removes dependence on host
block size or terminal-drain frequency for completed generated-note reuse; genuinely
simultaneous live owners still require sufficient declared capacity.

Evidence: `artifacts/architecture-v2/reclaim-*`. The authoritative new-core scan is
90, complete, zero errors and 118 warnings, with no rule suppression.

## Note-owned integer state

`Limits.note_cells` reserves a separate, cold integer slab, evenly divided across
logical note slots. Preparation derives each program's referenced note-cell width;
the plan owns the maximum width across its programs. Runtime construction and
plan submission reject widths exceeding the reserved stride before activation.
No additional reference count, allocation or hot-note field is required.

Each successfully admitted note clears its plan's logical cell range. Failed
admission cannot overwrite an existing owner's cells. All callbacks on the same
note share this state, including callback-lifetime execution after key/gate release.
Completing or accepting a callback does not erase it. Public pins, descendants and
terminal backpressure retain the note and therefore its cells. Logical retirement
invalidates the generational handle; slot reuse clears the new plan's declared range.
Generated children start with zero state independently of expression inheritance.

`note_cell` checks runtime identity, generation and the original plan's logical bound.
Held notes and children keep their original layout across plan replacement; an
active narrower layout cannot expose leftover physical cells. `ReadNoteCell` and
`WriteNoteCell` use the same owner checks and instruction fuel as other operations.

Two heap-guarded regressions cover repeated same-key notes, completion and terminal
rejection, post-release waits, pins, saturation, foreign/stale handles, slot reuse,
child isolation, old/new layouts and rejected replacement ownership. Oversized cell
indices and allocation layouts fail before allocation. Native debug/release/MSRV
and strict all-target Clippy evidence uses `artifacts/note-state-*`.

These cells are native signed 64-bit storage. The KSP frontend now validates and
lowers polyphonic declarations and 32-bit scalar assignments; full arithmetic and
variable scopes remain open. Native release dispatch is described below. The
[KSP parity map](KSP_PARITY.md) retains those separate obligations.

## Reserved physical-release callbacks

`Prepared::with_release_program` binds a callback-lifetime program to each external
input admitted through `trigger*`. Low-level `note_on*`, generated children and
consumed keyswitches do not implicitly enter these bindings. With no note program,
ordinary attack selection still occurs. Replacing the program table clears its
release binding; invalid indices and gate-bound release programs are rejected.

Admission preflights the note callback slot plus one future release callback slot,
then reserves the latter in the existing continuation arena. Manual callbacks and
other inputs cannot consume that capacity. The pending flag lives in cold release
context; the note's original prepared generation owns the program identity. Source
EOF does not discard the reservation. No extra per-note program copy is stored.

First physical key-up (including native all-notes-off) records release context,
consumes the callback reservation exactly once, runs the callback, and then selects
key-release layers. Pedal/gate handling follows; the callback may wait beyond both.
This is explicit native ordering, not verified Kontakt event-stage equivalence.
Hard silence, panic, explicit abort and fault cleanup suppress pending callbacks
and relinquish the reservation. A fault cannot recursively start another release
callback. Executed callbacks retain their ordinary completion/fault ownership.

Only callback admission is reserved: code still obeys command, generated-note and
fuel budgets. A failed wait or generated-note command reports its retained fault
without rolling back physical release or stranding the note. Release program bodies
are not assumed to have statically predictable resource use.

Heap-guarded tests cover saturated continuation and command pools, repeated key-up,
pedal hold/up, callback/terminal backpressure, release-side waits, hard-silence
suppression, EOF, original-plan replacement and generated-child non-reentry. An
independent PCM check sums old/new generation release-generated audio exactly.
Evidence uses `artifacts/release-behavior-*`; the [KSP frontend](KSP_FRONTEND.md) now
executes note/release source callbacks and polyphonic scalar assignments on this path.

## Non-note callback ownership

`BehaviorOwner::Plan` retains the originating instrument generation independently
of any musical note. These callbacks share continuation slots, locals, fuel,
sample-time waits and retained outcomes with note callbacks. Context requirements
are derived from all program instructions and checked before admission: note reads,
polyphonic cells, note-relative playback and gate-lifetime waits require a note.
A plan callback's fault/abort does not release an unrelated key. Global panic cancels
both owner types; channel sound-off only cancels matching note-owned callbacks.
Completion delivery now names the explicit `BehaviorOwner`, with no legacy wrapper.
See [control interaction admission](CONTROL_STATE.md#instrument-owned-ui-callbacks).

## Script-instance integer state

`Prepared::with_script_instances` defines initialized integer banks, each identified
by a dense `ScriptInstanceId` scoped to that plan. `Program::with_script_instance`
binds a callback program to exactly one bank. `ReadScriptCell`/`WriteScriptCell`
operands can address cells only in that binding; they cannot switch instances.
Preparation validates even dead-code references against the declared layout, checks
both instance/cell address ranges, and revalidates when programs or banks are replaced.
Programs without script operations do not require an instance.

Mutable banks are cloned off audio during runtime construction or plan submission.
All callbacks for that instance share the audio-owner bank; note cells and callback
locals remain separate. Note and plan callback pins retain their original generation.
Completion, cancellation and callback slot reuse do not reinitialize shared state.
A fault retains writes already executed; this is not a callback-wide transaction.
Prepared replacement initializes new banks and does not change waiting old callbacks.
All banks accompany retired plans for off-audio destruction, including queue failure
restoration. `Runtime::script_cell` is a checked audio-owner query, not UI access to
live runtime memory or a complete language-state snapshot.

Heap-audited native checks cover independent instances, overlapping notes/UI callbacks,
waits, generation replacement, terminal/outcome backpressure, stale handles, cancellation,
arithmetic faults, callback slot reuse and schema/local-capacity rejection. KSP ordinary
integer variables and arrays now lower into this service. Instance IDs are 16-bit;
cell addresses are 32-bit. `ScriptArray` is a nonempty bounded view in the same bank.
`ReadScriptArray`/`WriteScriptArray` validate evaluated indices before any destination
mutation; preparation checks the entire view even in unreachable code. Indexed reads
may reuse the index register as their destination. No runtime allocation or separate
array owner is introduced. String/real/object storage,
script-module registration, persistence and ordered source event stages remain open.

Script-state validation: all 203 native tests pass in debug, release and Rust 1.92;
strict all-target Clippy and both root boundary tests pass. Logs use
`artifacts/script-state-{debug,release,msrv,clippy,boundary}.log`. The regenerated
KSP inventory retains all 25 chapters, 288 sections and 1,605 identifiers; the
ordinary integer-variable section is now partial, not complete or vendor-verified.

## Explicit signed-32 arithmetic

`Binary32` and `Unary32` perform typed scalar integer work inside the same bounded
callback interpreter. They do not change the checked-i64 `AddLocal` used by native
counters. Operands must fit i32, including when reached through native script/control
state; invalid operands fault without a partial destination write. The declared
wrapping/division/bitwise policy is described in [KSP_FRONTEND.md](KSP_FRONTEND.md#integer-expressions).

Program validation includes every referenced register, including aliased operands
and dead code. `Prepared::behavior_local_count` exposes the largest callback width
for control-side arena sizing; runtime construction and replacement still reject
insufficient capacity. Execution uses no temporary heap stack or dynamic dispatch.


## Evaluated time and MIDI arguments

`ReadVelocity7` provides an explicit nearest-integer view of script-visible velocity without
mutating high-resolution note state. `MicrosToFrames` converts nonnegative register
microseconds to u32 sample frames with checked integer ceil division. `WaitLocal`
uses the same scheduler as `Wait`. `PlayMidi` reads separate key, seven-bit velocity
and either a positive frame-duration register or an explicit fixed duration policy.
It calls the same admission implementation as
native `Play`. Invalid arguments publish no child/timer; wider register values are
checked, not truncated. KSP uses these operations; no language VM runs on audio.


## Commit or suppress an existing attack

`ForwardAttack` and `SuppressAttack` require a note context. The note records pending,
forwarded or suppressed mapping independently of its physical/effective gate and
source/family lifetime. Forwarding commits its original plan and captured onset
selection onto the same note, with the same expression owner and full-resolution
velocity. Direct native calls return whether they changed the pending state; repeated
calls are no-ops after forwarding/suppression. Pending released keys cannot forward, including pedal-held gates: a deferred
attack cannot reserve a key-release phase that has already passed.
Consumed keyswitches are already suppressed. Explicitly started manual families are
separate from this mapped-attack state.

Normal selection and deferred forwarding share checked preflight and release quota
reservation. Failure cannot partially advance selection or publish mapped voices.
Direct callers can retry a pending capacity failure; instruction failure uses the
existing callback fault cleanup and retained outcome. Native programs do not gain
implicit forwarding: their translator places these instructions at its own semantic
commit boundaries. The KSP frontend now does so before waits/exit/completion, while
ordinary suppression-only native programs retain their explicit behavior.


## Admission, event view and committed sound

`initial_note_properties(note)` preserves the full-resolution pitch/velocity at
admission; generated notes have their own initial properties. `note_event(note)`
returns the script-visible view. `edit_note_event` validates the entire replacement
before changing that view, independently of physical input and expression ownership.
`ForwardAttack` preflights the view, then commits pitch/velocity exactly once to the
existing note. `note`/`note_pitch` expose those committed properties (admission values
until forwarding). Late or suppressed edits remain visible to scripts without
retargeting existing voices or release mappings. Invalid preflight leaves the view
available for a corrected retry and leaves committed values/reservations unchanged.

`ReadKey` and `ReadVelocity7` read the current event view; `WriteEventKey` and
`WriteEventVelocity7` replace its key/velocity from checked integer registers. Their
native domains are 0–127 and 1–127 respectively. These instructions require a note
and contribute register bounds even in unreachable code. The cold view/admission
bank is allocated once, reset by shared admission and protected by note-generation
checks; no script edit allocates, locks, or creates a replacement musical identity.


## Source-owned generated-note lifetime

`Duration::UntilSilent` is independent of the parent gate and schedules no synthetic
release. Only generated notes can use this retirement policy. Existing families,
voices/effects tails, scheduled work, pins and children remain counted owners.
Retirement can proceed after all of them finish even without a musical gate close;
it discards unused release quotas instead of selecting release audio. This keeps
source completion distinct from key-up and avoids inventing a release timestamp.
Unbounded loops and muted live sources remain owned until explicitly stopped.

`DurationValue::Fixed` and `DurationValue::Frames(register)` let frontend IR choose
native policy separately from integer time values. Gate/whole-source generation
needs no timer slot. The shared reclamation fast path now checks all requested
selection quotas, so completed internal notes can return release reservations under
voice/family/command pressure as well as note/expression/decision pressure. External
terminal acceptance remains exclusive to the caller's `flush_ended` sink.


## Native region-group selection

`Prepared::with_groups` assigns optional dense group IDs to authored regions. IDs
belong to a prepared generation; empty groups are valid. Ungrouped regions remain
eligible. This selection axis is independent of articulations, controller conditions,
variation sequences and DSP chains. It does not flatten source object hierarchies.

Each generation prepares two bitsets per runtime note slot: an editable selection
and its committed snapshot. Memory is `notes * 2 * ceil(groups / 64) * 8` bytes;
checked multiplication/allocation occurs during construction/submission, never on
audio. With no groups there are no bitset cells. Notes retain generation ownership,
and all group storage returns through the existing off-audio retirement path.
Input admission enables all groups; child admission snapshots its parent's editable
selection independently of expression inheritance. Slot reuse resets the masks.

`set_note_group` edits the draft with checked identity/index access. Attack preflight
filters eligible candidates before committing voices or take state, and successful
forwarding captures the draft. Mask edits do not remap existing voices. Automatic
releases retain that committed snapshot unless a source release callback explicitly
uses `forward_release_groups` at its first forwarding boundary. This operation
requires physical release and commits only once, including while sustain holds the
musical gate open. Later draft edits affect future children, not pending release
selection. `WriteGroup` and `ReadGroupCount` share these services; preparation derives
register/note requirements even from unreachable instructions.

Release reserves remain conservative across all group choices, so a release callback
cannot increase demand beyond owned quotas. Pitch/sequence validation likewise
covers pending release choices. This can reserve more than a particular mask needs;
compact group-dependent bounds should only replace it with equivalent proof.
The all-groups operation and child/forward snapshot copies are O(ceil(groups/64));
there is no hardcoded Kontakt group maximum in the native mask layout. Other vendor
hierarchies and source event stages still require explicit translators/profiles.


## Source-visible event identities

`source_event_id(NoteId)` exports a positive signed-32 alias. `resolve_source_event`
requires the original `PlanId`, validates its runtime/generation, then checks the full
native note handle. Unknown/retired integers resolve to no note; a stale plan errors.
Aliases contain no borrowed memory or ownership pin. Host note IDs are unchanged.
`ReadEventId` and optional `PlayMidi.result` use this same service. Return registers
participate in dead-code layout validation and can alias argument registers because
all arguments are captured before the result write.

The alias index is a standard vector reserved to note capacity during construction.
Binary search is O(log N); monotonically issued IDs normally append. Dead entries
remain safe through their full generational note handles and compact only when the
index fills. Compaction is O(N) with no allocation/destruction of owned resources.
This admission-time cost is explicitly marked for replacement by a bounded hash index
if churn benchmarks make it material; no competitive throughput claim is made.
One alias per live note guarantees room after compaction. A guarded insert cannot
silently allocate even if that invariant regresses.

The runtime-wide counter is monotonic across panic and prepared replacements.
`Prepared::with_source_event_limit` lets a frontend reserve integer selector space;
the default maximum is i32::MAX. Exhaustion returns Capacity, never wraps/reuses an ID,
and does not constrain native full-width handle admission. Result-bearing generation
reserves an ID before child publication; a later admission failure can leave a numeric
gap. A note keeps its assigned alias until retirement and repeated reads consume no
additional IDs. A fresh runtime resets its own namespace; plan validation prevents
cross-runtime API lookup. Persistent source integers are not native ownership handles.


## Targeted key-up and deadline replacement

`replace_key_up_at(note, at, velocity)` atomically replaces that full handle's queued
`KeyUp`/`Release` commands. It validates time, velocity, physical key state and net
queue/work capacity before removing deadlines. A replacement can reuse an occupied
slot; immediate key-up requires no slot. Existing equal-time commands run first.
All future note-end commands now own private work pins, just like expression commands,
so whole-source note retirement cannot invalidate a queued target. Cancellation and
execution return each pin once; manual unpin cannot consume it.

`KeyUpEvent` accepts a source ID and optional frame-delay register. Lookup is scoped
to its behavior's plan, including plan-owned UI callbacks. No delay preserves a
generated fixed-duration policy; an explicit delay overrides queued ends. Fixed
policy is admission metadata, separate from host scheduling and source completion.
Register layout is prepared off audio. Key-up reuses existing pedal, release callback
and release-region behavior; it does not invent a second release implementation.


## Polyphonic state across script instances

Prepared programs now derive one note-state range per `ScriptInstanceId`, plus an
unbound native range. Callbacks in one instance share the maximum cell requirement
of their programs; distinct instances add their requirements. Preparation checks the
sum and assigns each program a fixed base. Read/write instructions use that base and
their own declared bounds, with no instance lookup or allocation during execution.

`Prepared::note_cell_count()` reports the total per-note requirement. Runtime and
replacement-plan budgets use this total, and the native CLI sizes its note pool from
the bound plan. `program_note_cell` inspects a program's namespace in the queried
note's original generation. `note_cell` remains raw flattened-layout inspection.
Children and reused slots start with zero state; retained callbacks preserve old
ranges after a new plan changes program order, namespace sizes or instance bindings.

Two native fixtures cover concurrent scripts on overlapping notes, same-instance
sharing, unbound-native isolation, post-release waits, old/new layouts, empty child
state, invalid program/cell/context, summed capacity rejection and all u16 cells in
each of two independent namespaces. This establishes state isolation, not ordered
Kontakt/Falcon script execution. Validation: 263 native tests in debug/release/MSRV,
strict Clippy and both root boundary tests (`artifacts/polyphonic-instances-*`).


## Deferred release ownership

The note's cold release state now separates physical key-up from an explicitly held
release. `suppress_release` applies before the release-group forwarding boundary;
`resume_release` commits groups, resolves pending key-release selection once and
reuses the pedal-aware gate closure. Physical context remains unchanged. Existing
pedal updates and all-notes-off respect the hold; forced/hard closure clears it and
returns every pending release reservation. A group forwarding barrier alone cannot
undo an explicit hold. Faulted callbacks use the existing hard-close path.

`Event::ForwardRelease` targets the full retained note handle. Scheduling validates
the held gate, owns a work pin and cancels when that hold ends. Both deadline-replacement
APIs share one capacity/clock/ownership transaction. Replacing, consuming or cancelling
commands returns pins exactly once. Native callback and source tests cover this path
without allocation, including automatic key-release and gate-release layers together.


## Input pairing survives downstream script completion

The native note keeps raw `input_down` separately from its logical key/gate records.
`input_held` queries that raw fact; `key_down`/`ReadKeyDown` query downstream event
state. Script key-up, callback faults and cancellation never consume the raw input.
FIFO matching and terminal retirement therefore cannot transfer a late input note-off
to a replacement same-key event. Physical key-up/all-notes-off, explicit native owner
abort and panic consume the input bit even if the logical key was already closed.

`ScriptKeyUp` records `ReleaseCause::Script`, using the same pedal/release services,
and is separately scheduled from physical `KeyUp`. Script deadline replacement leaves
physical commands intact; both retain their complete target handles. A pending script
key-up can also resume a subsequently suppressed release without another callback.
All-sound-off retains raw keys as before. MPE member expression now consults the same
raw projection, with no parallel adapter-side ownership table.


## Bounded callback dispatch

Nested native release callbacks run on an explicit LIFO work stack prepared with
two entries per continuation slot: its instruction frame and possible key-release
completion. Dispatch never recursively invokes the interpreter. Suspended callers
keep their remaining fuel; waits retain the existing fresh budget on resume. A
completion privately pins its note while callback side effects determine release
groups and the final pedal-aware gate state. Explicit forwarding inside a suppressed
release still closes its gate before the next instruction observes it.

A 4,096-note release chain runs on a 128 KiB thread stack without runtime heap
activity. Source fixtures cover nested side-effect ordering, wait boundaries, gate
closure before subsequent generated notes, raw input retention and arena reuse.
This preserves the tested native synchronous policy; it does not establish vendor
callback ordering or implement ordered script slots.

Validation: 272 native tests pass in debug, release and Rust 1.92; strict all-target
Clippy and both root boundary tests pass (`artifacts/callback-dispatch-*`).

A scripted stop before pending attack forwarding now suppresses that attack before
release dispatch. Implicit forwarding therefore completes harmlessly, without
weakening the native rejection of a physically closed deferred attack. The source
fixture covers both immediate note_off forms, one release callback, silence and
retained raw input until host key-up. Validation: 273 native debug/release tests and
strict Clippy pass (`artifacts/pending-attack-*`).
