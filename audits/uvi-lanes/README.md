# UVI lanes — CURRENT13 inventory and completion seam (design only)

Base/source: `13c044d6f9f80576507521a2dc6521e9aad7d9c5`, tree
`4af7c9babd93c49cdb0a026a63d25e6c433498b0`.
Only this audit directory is authored. No product, setter, loadData, target,
wrapper, manifest, official-docs/sections.json or old-ledger changes.
Rust characterization: **NOT_RUN / NOT_COMPILED**. Native: **UNKNOWN**.
CPU **AND** RAM versus both Kontakt and Falcon: **UNACHIEVED**.
`evidence.json` pins CURRENT13 spans and rehashed primary vendor excerpts.
Historical source3665 handoffs were leads only, not current evidence.

## Current cause, exact callers

1. `Translation::inserts` (`sampler-uvi/src/inserts.rs:71–307`) records a
   bypassed insert with count 0. OnePole key tracking creates a static
   middle-key approximation and finding, not a raw live cutoff lane.
2. `translate_full` (`lib.rs:220–312`) calls `engine_parameters::register`
   after translation. `register` (`engine_parameters.rs:68–126`) requires
   count 1, zero OnePole KeyTracking, and a finite in-range authored default.
   Only OnePole.Freq and Gain.Volume are supported; do not expand that set.
3. `native.setParam` (`script.rs:1395–1478`) calls `binding` using XML-node
   identity and element type/name, without the register checks or a Prepared.
   `Shared::command` (`656–666`) returns true on local 65536-command admission.
   `element.setParameter` (`script_prelude.lua:365–403`) then writes `__set`;
   `getParameter` (`345–357`) reads it before authored XML. `insert_overrides`
   (`script.rs:2354–2381`) exports the same overlay. None is a DSP completion.
4. Resident `assemble` (`lib.rs:1680–1712`) and streamed `assemble_streamed`
   (`1402–1447`) derive `bindings(instrument)` from registered control keys,
   call Kontakt finish/stream preparation, then attach them to the resulting
   plan with `with_engine_parameters`. These are the final-plan inventory
   publication points, after sample/key filtering and any group-tap rewrite.
5. Kontakt `finish`/`stream_instrument` enter `prepare_inner`
   (`sampler-kontakt/src/load.rs:915–1018`), which invokes core `lower_with`
   (945 or 1005). Core `lower` is the simple entry to the same function.
   `Lowering::lower_list` (`lower.rs:1685–1755`) binds each consumed processor
   through `bind_processor` (`1588–1635`), or fails if a requested lane is
   unavailable. Gain becomes ControlGain; OnePole cutoff becomes a control
   parameter on a StateVariable filter. No alternate normalized DSP law.
6. `Driver::input`, `note_on`, release and `wake` eventually reach
   `apply`/`apply_all` (`scripted.rs:349–382`). The EngineParameter arm calls
   `Runtime::set_engine_parameter` directly, retains only a deduplicated
   `insert parameter without a DSP lane` category on failure, and sends no
   result to the script owner. `Player::render` wakes this driver each block.
   The product caller is `src/sound/v2.rs::uvi` (3123–3184): attach_script runs
   first, `nest` rewrites the IR mixer, then assemble_translated_streamed lowers
   it and returns the driver with the plan. `V2Core::render_chunk` (1753–1754)
   calls that driver's wake before rendering; this is not just an offline path.
   `Translated::attach_script_with_ui_state` (`lib.rs:1086–1131`) instead
   creates a ScriptThread and Driver. The thread (`scripted/thread.rs:188–280,
   439–453`) publishes the initial report before commands, uses a forward
   command ring, and has no per-insert reverse completion message.
7. Runtime (`sampler-core/src/engine_parameters.rs:676–803`) resolves the
   active plan, exact four-field address, and binding law, then calls
   `edit_controls_now` (`control.rs:546–567`). A missing UVI insert binding
   returns InvalidInput. `DspState::edit_control` (`dsp/control.rs:251–292`)
   updates only matching voice/bus consumer ranges. Its empty range is not
   an error. The direct UVI call does not emit core behavior-op outcomes.

### Two extra traps from this retrace

* `Prepared::with_engine_parameters` validates duplicate addresses, known
  parameter ID, law and control existence; it does **not** require an actual
  DSP consumer. `retain_zones` can drop every zone but preserve controls and
  processor-control declarations. Core lower consumes voice chains only in
  its zone loop (`371–399`), while it inventories all IR controls (`433–439`).
  An orphan voice-chain control can therefore accept writes and base readback
  despite having no consumed DSP lane. A binding-only inventory is too weak.
* `Lowering::parameter_registry` builds descriptors from IR processor controls
  (`1077–1151`), not an independent consumer witness. Its native aliases are
  created before UVI attaches final bindings; `resolve_native` alone also
  misses valid late UVI bindings. Neither registry presence nor absence is
  the proposed inventory predicate. Do not replace unrelated core bindings
  during a future refactor; current with_engine_parameters replaces the slice.

## Smallest proposed interface (NOT IMPLEMENTED)

The seam lives between final Prepared assembly and the Lua owner/Driver.
Keep the inventory insert-only, not a new full native catalog.

* Build once, off the audio thread, **after successful final lowering and
  final UVI binding installation**. Select existing UVI bindings for which
  `control` has a validated entry in either `Prepared.dsp_controls` with
  `dsp_bindings`, or `Prepared.buses.controls` with `buses.parameters`.
  Check consumer lane index, matching ControlRange.control and the Real
  control domain/default; reject invalid metadata, deduplicate by the entire
  EngineParameterAddress. Multiple real consumers for one control are valid.
  The consumer lists are core-private today: the narrow future core interface
  is `Prepared::has_processor_control_consumer(ControlId) -> bool`, not a
  registry parser or hash-only/name-only identity. No envelope/slot/bypass
  expansion. Pass a sorted immutable slice of existing bindings (address,
  control, law), plus an explicit nonreused owner epoch and active PlanId.
* Publish/install on the Lua owner before **live** callbacks. A modeled insert
  write needs an exact inventory hit, the inventory law's normalized_value,
  a current owner epoch and command/pending capacity before queue admission.
  An absent lane rejects without an overlay or insert override. Type/unknown
  name handling and retained-XML-only writes stay separate and unchanged.
* Add only an insert request token `(epoch, sequence)` to the command, and a
  bounded reverse record `{epoch, sequence, address, result}`. On the runtime
  owner, check that the command's plan/epoch is still active; reserve reverse
  capacity **before** applying a write. Then use the same runtime setter and
  immediate base readback to report Result<normalized i32, Error>. Success
  means the base/control lane accepted the command, not audible parity or a
  rendered frame. Keep the submitted authored scalar in owner pending state;
  do not roundtrip it through normalization to invent a new Lua value law.
* Preserve synchronous read-your-write semantics as **staged** state only
  after proven-lane admission. Track committed state separately. The owner
  resolves pending writes in sequence order; success advances committed state,
  failure removes just that pending write, exposing the newest remaining
  pending value or last committed value. Never let an older completion erase
  a newer staged write. Reject duplicate/foreign/out-of-order completions,
  and do not permit sequence/epoch wrap. A staged getParameter is explicitly
  not proof of completion. Returning committed-only values would break
  existing immediate controller/widget assertions; that is a separate choice.
* Backpressure is part of the interface. Bound outstanding insert requests
  across host queue, thread backlog, command ring, Driver and reverse ring
  with one credit budget; reserve a credit before admission and release only
  after owner consumption. A full reverse ring delays application without
  consuming/dropping the request. No wait, lock, allocation, Lua access or
  destructor-heavy inventory replacement on the audio thread. Owner wakeup
  must service completions even when no callback/clock advance is due.
  Cancellation drains/rejects old-epoch requests and releases credits before
  replacing the inventory; old handles cannot address the new XML node space.

Do not use the existing 64-record EngineParameterOutcome queue as an ACK:
`ops.rs:853–886` records behavior programs, lacks UVI request identity and
increments a dropped-outcome counter when full. UVI's direct call bypasses it.

### Initialization is an explicit blocker, not solved by a live inventory

`ScriptHost::new_with_ui_state` installs/builds/executes script body/onInit
before any final plan exists (`script.rs:1002–1007`). Scripted library loaders
then patch XML using `insert_overrides` **before** translate/lower
(`lib.rs:1515–1527`, `1610–1619`); attach_script instead changes registered
control defaults with `initialize` after the thread's initial report.
Rejected-at-register defaults can even be repaired by initial XML patching.
A live-only inventory cannot be passed retroactively to these script branches.

For a later complete fix, declare initial overlays provisional, stage their
final writes, lower final state once, reconcile every initial request against
actual consumers, and publish initial readiness only after that reconciliation.
If the requirement demands onInit itself observe only committed DSP values,
loading must become two-phase (or a provisional plan must be prepared first),
with an explicit decision on key-tracked/bypassed/invalid-default behavior.
Do not rerun Lua to repair state: callbacks/files/UI effects can duplicate.
No claim that initialization is fixed by the proposed live seam. Product
loading/setter behavior is deliberately untouched for the current release.

## Authored characterization and future custody

`characterization.rs` is detached source intended as a cfg(test) child module
of `sampler-uvi::engine_parameters`; it is not wired into a target. Four exact
synthetic tests, all NOT_RUN / NOT_COMPILED:

* `queue_admitted_readback_precedes_missing_registered_lane`: actual Lua
  setter/readback/overrides/queue against bypassed OnePole/Gain, key-tracked
  OnePole, and invalid authored cutoff/gain; exact runtime rejection.
* `driver_reports_missing_lane_without_setter_acknowledgement`: actual
  Driver input/wake, repeated finding deduplication, no behavior-op outcome.
* `supported_inserts_keep_exact_bindings_but_late_native_aliases_are_absent`:
  positive OnePole/Gain controls and exact current setter/base readback.
* `dropped_voice_chain_keeps_descriptor_and_accepts_an_unused_control`:
  post-retention orphan control/descriptor is accepted; absence of DSP
  consumption is established by the pinned lower/prepare source, not PCM.

The setup uses production translate/private binding functions and core lower,
then the same final binding attachment as assemble. It bypasses all loaders,
clears behaviors only because actual Lua runs in ScriptHost, supplies generated
zero PCM, and never renders or accesses synthetic.wav. It does not validate
streaming, thread ordering, init reconciliation, the proposed consumer method,
ACK Rust integration or audio parity. Python checks exercise only an independent
minimal inventory/ACK model plus exact source/hash checks; rustfmt checks syntax.

Future integration owner must wire and compile these tests in a separately
frozen candidate, then add actual voice/bus consumer, threaded reverse-pressure,
repeated writes/failure rollback, idle ACK wake, stale epoch/reload, no-wrap,
init reconciliation and positive PCM tests. No commands/tests/build were run
here beyond Python, Git inspection and installed rustfmt. No automatic merge.
