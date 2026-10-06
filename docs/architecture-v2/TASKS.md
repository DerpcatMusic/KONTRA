# 2.0 task list — clean-sheet implementation

Revised 2026-10-05 following explicit user direction: implement a new product with
no KONTRA 1.x backward compatibility requirement. [PLAN.md](PLAN.md) is the current
contract. Existing task IDs remain stable for the [scenario allocation](CONFORMANCE_MAP.md),
but legacy extraction/migration requirements are superseded here.

The existing kernel is an experiment with partial V2-03/04 evidence. Its old-VM KSP
bridge is historical boundary evidence, not progress toward a new scripting runtime.
Family/expression ownership and segmented rendering now have
[additional executable evidence](OWNERSHIP_SLICE.md). No full implementation gate is closed. The four supplied references remain intact.

## Required completion scope

The user reiterated on 2026-10-06 that completion means the entire new core, fully
wired production UI, an upgraded sample section, and full Kontakt KSP parity.
Intermediate commits and native subsets are progress only. Do not substitute a
smaller supported subset for the full KSP requirement or count legacy bridge tests
as its implementation. Core/plugin completion still requires removal of the old
production engine paths and no compatibility shims.

Every subsystem must be checked against the architecture and pinned open-source
sampler references, following [REFERENCE_REVIEW.md](REFERENCE_REVIEW.md). Convert
relevant basic and advanced DSP/musical edge cases into tests; explicitly track
unimplemented cases. Source inspection alone is not passing conformance evidence.

## Active priority — Kontakt and Falcon first

User direction on 2026-10-06 explicitly defers SFZ and all other format frontends.
Finish native DSP/audio, MIDI, core ownership, production UI and full Kontakt/Falcon
capabilities first. Other formats may resume only after complete parity and measured
performance materially above both reference platforms. These are acceptance targets,
not current claims: require versioned vendor fixtures and matched-quality workloads.
No SFZ frontend has been implemented. Shared primitives remain format independent.

The independent [Kontakt source decoder](KONTAKT_SOURCE.md) now inspects expanded
chunk payloads and retains source records, raw bytes and saved script state. An
authored saved script executes through the native KSP/audio path. This does not
close container decoding, whole-instrument lowering, persistence or playback gates.
NKS 4.2 and clear NIS preset profiles now have bounded native decoding, with raw
metadata retained and no dependency on the old importer. Remaining wrappers,
source semantics and checksum/authentication validation are still required.

## Completed groundwork

- [x] Isolate worktree, preserve all attachments and record their hashes.
- [x] Map legacy ownership and reproduce terminal-loss and torn-snapshot failures.
- [x] Implement/test the first independent note/PCM prototype.
- [x] Replace the migration mandate with clean-sheet implementation requirements.

- [x] Run Rust Doctor and configure the pinned CI/DSP policy; see [scan evidence](RUST_DSP_POLICY.md).
- [x] Meet and enforce user-selected new-core score >=90 plus no new workspace errors (90, authoritative, complete; no rules disabled).
- [x] Review callback ownership/index invariants before expanding DSP; [evidence and remaining review](ENVELOPES.md).
- [x] Implement native linear AHDSR and retained release tails with independent timing/heap tests.
- [x] Add immediate/scheduled family chokes with per-source level capture, bounded
  non-extending fades, delayed-start cancellation and generational targeting;
  [evidence](ENVELOPES.md#timed-family-choking). Automatic stealing remains open.
- [x] Implement independent source ranges, forward/reverse playback and continuous/until-release loops; [evidence](SOURCE_VIEWS.md).
- [x] Replace release ownership scans with direct child/family/source lists and a
  prepared closure stack; validate slot reuse, linked/independent descendants, tails
  and terminal retries. Full stealing/overload policy remains open.

- [x] Pin UMP v1.1.2 and implement allocation-free framing/channel-voice decoding plus native note/pedal ingress; [partial scope and evidence](MIDI_INGRESS.md).

- [x] Add native fractional source traversal, asset/output rate conversion and static
  transposition with ratio-dependent filtering; [evidence and open quality work](RESAMPLING.md).
- [x] Compile native root-key tracking and apply live note-scoped pitch, with queued
  expression/source admission invariants and initial expression installed before
  bound programs/source selection. Pitch ramps remain open.
- [x] Compile native per-key tuning with explicit future-root adoption, retained
  descendant tuning and no render-time table lookup; external protocols and live
  retuning remain open. See [native tuning](RESAMPLING.md#prepared-native-tuning).
- [x] Separate inherent absolute pitch from key tuning and live expression; consume
  MIDI 2 Pitch 7.9 attributes with physical pairing intact and retain fractional
  pitch through generated-note transposition. Per-note controllers remain open.
- [x] Add bounded fixed-zone MPE note/pitch/pressure/timbre projection, physical key-up snapshots,
  runtime identity, whole-semitone RPN sensitivity, zone pedals and atomic gestures; [partial receiver scope](MIDI_INGRESS.md).

- [x] Prepare note-scoped event-rate pressure/timbre routes to gain, balance and
  pitch, with retained-plan ownership and transactional source constraints;
  [scope, evidence and remaining modulation work](MODULATION.md).

## M0 — contracts and measurable targets

### V2-01 — Establish independent conformance and workload evidence

- **Partial evidence:** [resident render workloads](RENDER_WORKLOADS.md) now cover
  layered polyphony, block size, sample rate, envelope state and sparse reservation,
  with independent exact output and recorded local timing. Streaming, modulation,
  host jitter and comparative production workloads remain open.


- [ ] **P0; dependencies:** none.
- Define authored correctness fixtures and workloads: dense same-key overlap,
  layered/multimic instruments, long release tails, modulation, streaming stalls,
  scripting bursts and plan replacement. Record hardware/build/quality/seed inputs.
- Preserve legacy failure probes as lessons. Fixing legacy code is not required.
- **Done when:** reproducible trace/audio/resource measurements and expected native
  outcomes exist; no old engine output is silently used as a correctness oracle.

### V2-02 — Specify identities, MIDI 2.0 events and resource contracts

- [ ] **P0; dependencies:** V2-01 workload definitions.
- Define input/note/family/voice/expression/plan/asset identities, raw versus projected
  controller state, timestamp domains, gate policy and explicit command outcomes.
- Pin MIDI Association specifications/revisions and supported MIDI 2.0/UMP/MIDI-CI
  capabilities. Preserve group/channel/per-note context and original precision.
- Inventory each domain/queue: writer, reader, capacity, admission, full behavior,
  cancellation, epoch and destructor. Set numerical budgets against target hardware.
- **Done when:** lifecycle, precision, ordering, overload and future v2 schema evolution
  have executable contract cases. Prototype API changes remain permitted.

## M1 — musical runtime

### V2-03 — Build the bounded ownership kernel

- [ ] **P0; dependencies:** V2-02.
- Extend or replace the prototype with separate logical notes, families, voices and
  expression owners; generational handles, provenance and linked/detached lifetimes.
- **Done when:** admission is transactional; stale/cross-runtime handles fail; steals,
  child cancellation, source completion and continuation pins cannot orphan ownership;
  terminal delivery retries exactly once to acceptance without premature ID reuse.
- Current evidence covers note/family/voice/expression generations, separate capacities,
  child inheritance/detachment and terminal retention. Full gate/pedal/continuation
  semantics remain open. No legacy type dependencies allowed.

### V2-04 — Build unified sample-time execution and PCM conformance

- [ ] **P0; dependencies:** V2-02/03.
- Implement new event scheduling that merges external input, continuations and source
  boundaries with defined causal ordering and bounded zero-time generation.
- **Done when:** event traces/PCM agree across regular, irregular, zero and boundary
  partitions; controller consumption, expression precision and transport epochs work.
- Current unity-rate PCM/start/release tests are partial evidence only. Extend beyond
  44.1/48 kHz to the declared production rate matrix, with explicit tolerances.

### V2-05 — Implement a new bounded behavior runtime

- **Partial evidence:** [native instructions](BEHAVIOR.md) now execute generated
  notes, suppression and waits through the shared queue, with fuel, cancellation
  and retained completion/fault outcomes. A [new KSP source subset](KSP_FRONTEND.md)
  now compiles into these services. Completed generated notes reclaim internal slots
  under pressure without consuming host terminals; repeated-note audio is independent
  of block size and terminal backpressure. Bounded note-owned integer cells now
  survive callbacks, release and plan replacement separately from callback locals.
  Physical-release callbacks reserve continuation capacity at admission, survive
  source EOF and suppress safely under hard cleanup.
  The new KSP frontend now lowers polyphonic declarations, scalar assignment and
  note/release callbacks onto those services. Physical held-state reads, nested
  scalar conditionals, while/continue and callback exit now execute with bounded code/registers
  and heap-checked overlapping-owner fixtures. Separate script-instance integer banks
  now execute ordinary KSP globals across note/release/UI callbacks and retain old
  generations independently from new initial values. Polyphonic ranges now isolate
  those instances on each note, with preparation-time offsets and summed capacity;
  ordered note/controller/release routing is implemented below; full profile semantics remain open. Signed-32 arithmetic, bitwise
  expressions and scalar inc/dec now execute through explicit native instructions
  with bounded expression depth and register requirements. Integer constants, bounded
  script-instance arrays, indexed inc/dec and num_elements now share those services.
  Nested select/case, signed hexadecimal literals, combined Boolean conditions and
  short-circuit array guards now lower onto the same native IR. Broader typed arithmetic
  and language/state/controller work stay open. Declared-before-use user functions
  now expand into bounded native instructions with caller-specific forwarding,
  shared state and existing waits; nested branch/loop targets relocate at each call.
  Init calls and broader language contexts remain open. Native per-note group masks now filter
  layer selection, snapshot into children and retain generation ownership. KSP
  allow/disallow and first-yield release-group commits use the same services.
  Stored source IDs now support individual note_off and optional deadline overrides
  through shared, retained key-up scheduling, including plan-owned UI callbacks.
  Release callbacks can suppress and later forward the same note, preserving physical
  key-up context, group selection, pedal holds and owned release-layer reservations.
  Raw external key pairing now survives downstream script note-off and callback
  fault/cancellation; logical held-state and MPE's physical input projection are
  separate, including same-key FIFO and independent host/script deadlines.
  Nested release callbacks now use preallocated dispatch frames instead of recursive
  interpreter calls, preserving per-callback fuel, wait ordering and release completion.
  Controller callbacks now capture their input and original plan while separating
  raw CC banks from downstream selection state. KSP CC consumption/remapping and
  ordinary MIDI/MPE manager ingress share the same full-resolution/pedal services.
  Note/release CC reads and writes now reuse the retained note performance/channel
  context across waits; UI CC context and virtual controllers remain open. Controller-only stages now retain independent incoming CC projections
  and downstream continuation reservations; KSP controller modules combine through
  the same runtime with isolated state and relocated UI callbacks. Note stages now
  copy per-module properties/groups and reserve downstream callbacks. Generated
  notes/CCs enter the following module, retaining creator views and old plans.
  Reached-stage release callbacks now reserve owned capacity, retain separate group
  drafts and expose local logical held state. Generated parent-follow links now
  follow the creating module's release with independent downstream holds, pedal
  handling and bounded native dispatch/cleanup. Stage-scoped scripted stops remain open. Controller callbacks now generate parentless notes in their retained
  plan/domain, including source offsets and stored-ID stops; no fake host input is used.
  Marked/all-event targets, broader mutation/status services and ordered script slots
  remain open; individual-ID fixtures are not full vendor-command parity.

- [ ] **P0; dependencies:** V2-03/04.
- Build command/query services, continuations, callback contexts, cancellation and
  fault cleanup from scratch. Native behavior and new language frontends share them.
- **Done when:** generated notes, suppression, waits, polyphonic state, consumed CC,
  immediate readback and fault/steal/cancel resolve through the same kernel; instruction
  and native-helper costs are bounded. No dependency on the existing parser/VM.
- Recreate authored KSP fixtures through the new frontend; old-VM bridge tests do not
  establish this gate or vendor compatibility.

### V2-06 — Implement gates, pedals and release reserves

- **Partial evidence:** binary sustain/sostenuto and physical/effective key separation
  now execute through the shared timeline; see [scheduling evidence](SCHEDULING_SLICE.md).
  [Retained release context](RELEASE_CONTEXT.md) preserves sample times, optional
  velocity and distinct musical/cleanup causes through source EOF, pedal hold,
  plan replacement and terminal rejection. [Native release selection](RELEASE_SELECTION.md)
  now reserves voices/families/decisions/commands at admission and preserves coherent
  key/gate layers under full capacity. Half-pedal and repedal remain open.

- [ ] **P0; dependencies:** V2-03/04/05.
- Separate physical key, gate, sostenuto capture and source release. Specify and
  implement sustain, retrigger, panic, half-pedal and repedal semantics.
- **Done when:** pressure/pedal-up bursts preserve cleanup, release context and
  source-ended notes; no omitted positive scenario is counted as passing.

## M2 — instrument compiler and rendering

### V2-07 — Build new source, semantic and prepared representations

- **Partial evidence:** owned prepared PCM, bounded native region compilation and
  indexed selection now run through the [independent native entry](NATIVE_ENTRY.md).
  Vendor frontends and the broader semantic model remain open.

- [ ] **P1; dependencies:** V2-02/05.
- Native authoring plus new Kontakt and Falcon frontends lower into neutral
  semantic data; preserve source identity, unknown data and explicit capability reports.
- **Done when:** bounds/asset paths are validated; unsupported required behavior fails
  preparation; no parser/vendor objects cross into musical or render execution.

### V2-08 — Compile selection, articulation and variation

- **Partial evidence:** [coordinated native takes](VARIATION.md) now support
  global/key/channel/channel-key scope, retained note decisions, transactional
  advancement and original-generation state, including seeded random/no-repeat/shuffle
  policies with bounded draws and control-prepared bags. Independent multimic PCM and
  failure/retirement checks pass. Independent key/gate release phases now use native
  selection and retained decisions with admission-owned quotas. [Articulation routing](ARTICULATION.md)
  adds independent performance domains, silent latched switches and onset/current
  release snapshots. Controller snapshots now retain bounded shared versions with full-resolution MIDI
  updates and compiled controller conjunctions with onset/current release filtering.
  Control-time interval projections tighten release source reserves for exclusive
  CC groups while preserving conservative bounds for multidimensional correlations.
  Controller-triggered notes, other switch policies, phrase
  state and persisted snapshot/restore remain open.

- [ ] **P1; dependencies:** V2-04/06/07.
- Implement indexed selection, scoped counters, family take decisions, phrase and
  release context, with a straightforward independent reference evaluator.
- **Done when:** compiled/reference results agree, multimic takes remain coordinated,
  release sequences stay independently expressible and consumed switches do not leak.

### V2-09 — Implement source/DSP kernels with explicit state scope

- **Partial evidence:** native [source views](SOURCE_VIEWS.md) now include explicit
  wrap/ping-pong topology, fractional reflected guards and outward release exits.
  Independent unrolled PCM, analytic released interpolation, muted phase and
  heap-checked retirement cover both initial directions and short/high-rate loops.
  Finite pass counts now share traversal/guard/tail boundaries and allow natural-EOF
  release loops without duration commands. Native DAHDSR delay, independent stage
  curves and one-shot AHD now share source ownership and bounded rendering. Vendor
  curve/clock profiles remain unverified. [Native voice chains](VOICE_DSP.md) now run
  independent stereo biquads and gain stages before/after the envelope, including
  shared-control gain ramps on the sample clock, with retained
  DSP tails, whole-chain choke and generation-owned state. Low/high shelving EQ now
  has response, resonance and inverse-pair evidence. Linear crossfaded wraps now
  preserve guard phase, finite exits and forward/reverse source views through the
  common resampler. [Bus DSP](BUS_DSP.md) now uses a prepared stereo DAG with summed
  histories, sends, sample-clock control ramps and generation-owned tails independent
  of host note retirement. Reflected/other crossfade profiles, nonlinear effects,
  automated filters and remaining routing/scopes remain open.

- [ ] **P1; dependencies:** V2-04/07/08.
- New playback/resampling/loop/envelope/filter/modulation execution; declare units,
  rate, voice/family/bus scope, latency, tails, demand windows and quality modes.
- **Done when:** boundary guards, reverse/loops, nonlinear scope, expression isolation,
  steals/fades and muted-layer continuity pass numerical and audio-quality tests.
  Record alias rejection, stability and cost at declared quality settings.
- No silent quality reduction or block-rate substitution under overload.

## M3 — resource service and realtime guarantees

### V2-10 — Implement preparation, adoption and retirement

- **Partial evidence:** [prepared-plan adoption](PLAN_ADOPTION.md) retains original
  PCM/program generations through notes, descendants, callbacks and terminal
  backpressure. Bounded SPSC transfer returns unused plans for control-side destruction;
  validated immutable PCM handles now share buffers across plans without rescanning
  contents or changing audio-thread destruction ownership.
  Mixed-generation audio and a separate control-thread check pass. Streaming and
  host lifecycle integration remain open.


- [ ] **P0; dependencies:** V2-02/07/09.
- New worker/control/audio exchanges with immutable plans, tagged requests, bounded
  retained generations and reserved retirement capacity before adoption.
- **Done when:** failed loads preserve the active v2 plan; stale completions cannot
  publish; finish/choke transitions and exactly-once off-audio destruction are tested.

### V2-11 — Implement asset storage and streaming

- **Partial evidence:** [immutable asset identity and source demand](STREAMING.md)
  now preserve shared decoded revisions, exact traversal/guard ranges and first-use
  deadlines without mutating playback. A bounded decoded-page cache now transfers
  worker buffers, protects current demand and rejects stale completions without
  audio-thread destruction. Paged/resident rendering now shares source and DSP
  kernels, rejects missing onset guards atomically and reports live misses while
  draining owned DSP tails. Decoder integration, demand scheduling, preventive
  fades/recovery and offline readiness remain open.

- [ ] **P1; dependencies:** V2-09/10.
- New immutable asset/view identities, decoding, residency, cache and demand service.
  Select queue/cache layout through measurements, independently of old implementation.
- **Done when:** pitch/reverse/loop demand, cold onsets and storage stalls obey bounded
  live-render policy; starvation is visible; offline preparation has a separate contract;
  shared samples never share mutable voice history.

### V2-12 — Enforce realtime bounds and fault containment

- [ ] **P0; dependencies:** V2-05/06/09/10/11; enforce incrementally from M1.
- Audit allocation/deallocation, locking, I/O, destructor paths, native helper work,
  malformed data and recursion. Exercise every capacity and failure outcome.
- **Done when:** stress workloads preserve cleanup, memory bounds and declared callback
  budgets; sanitizer/fuzz and overload evidence accompany supported configurations.

## M4 — independent application

### V2-13 — Build headless controls and coherent v2 state

- **Partial evidence:** [typed headless scalar controls](CONTROL_STATE.md), stable
  IDs, atomic edits/full scalar recall, revision conflicts, bounded acknowledged
  UI handoff and coherent capture are implemented. Shared scalar values now drive
  native gain ramps through the same atomic edit/recall path. Timestamped native
  control events reserve the target generation/revision and share musical ordering. KSP scalar widget declarations
  and plan-owned UI handlers use the shared state without a window. Complete instrument serialization,
  script-instance state, non-scalar controls and host automation remain open.

- [ ] **P0; dependencies:** V2-05/07/10.
- New stable automation/control IDs, coherent bounded capture, state schema and UI
  command/view model. Plan v2 schema evolution; no 1.x conversion requirement.
- **Done when:** correlated snapshots stay coherent during mutation; UI-closed results
  agree; reordered compiled tables preserve automation identity; recalls are transactional.

### V2-14 — Implement new KSP frontend and external behavior profiles

- [ ] **P0; dependencies:** V2-05/08/09/13.
- Evaluated integer `wait`/`play_note` arguments and onset velocity reads now execute
  through native bounded services; gate-linked and whole-source duration policies
  now share native ownership/retirement. Generation-scoped source aliases and
  generated-note return IDs now survive scalar/array storage without pinning notes.
  Resident source-time offsets now preserve per-asset fractional positions and
  explicit loop/end behavior; DFD limits and vendor profile fidelity remain open.
  Full event-targeted commands and
  ordered multi-slot/controller forwarding remain open. Single-stage suppressed
  releases now resume through native pedal-aware services. Original note
  forwarding now commits once on the same identity before KSP wait/exit/completion.
  Per-note allow/disallow groups now select attacks, generated children and
  automatic release layers with a once-only release-group commit. Names, purge,
  affected-group queries and complete source hierarchy/import remain open.
  Current-event pitch/velocity edits use separate admission, script-visible and
  committed audio properties; late edits preserve running/release audio. See [KSP_FRONTEND.md](KSP_FRONTEND.md).
- Implement full Kontakt KSP language and service semantics against the new runtime:
  callbacks, builtins, asynchronous work, persistence, UI controls and engine services.
  Pin the target Kontakt/KSP version and inventory the complete reference surface.
  [Kontakt 8.12 surface inventory](KSP_PARITY.md) now covers all functional manual
  chapters; operand/context/alias review and actual parity evidence remain open.
- **Done when:** the full pinned KSP surface has implementation and appropriate service/
  reference evidence. A supported subset is not completion; approximate, unsupported
  or unverified required semantics remain open obligations.
- External KSP semantics are a product capability, not old KONTRA compatibility.

### V2-15 — Implement new host and MIDI 2.0 adapters

- [ ] **P0; dependencies:** V2-02/03/04/09/10/13; protocol cases start at M0/M1.
- New CLAP/VST3/standalone integration and MIDI 1/MPE/MIDI 2 UMP adapters. Implement
  declared high-resolution/per-note behavior, validation, routing, translation, time
  mapping, MIDI-CI control-plane scope and explicit unsupported transport reporting.
- **Done when:** pinned protocol conformance fixtures cover malformed packets, precision,
  expression ownership, note management and ordering; host terminal pressure, signed
  IDs/wildcards, rate/reset/suspend/reactivation and multi-output cases pass.
- UMP passthrough or reserved numeric precision alone does not close MIDI 2.0 support.

### V2-16 — Ship an independent v2 composition root and application

- **Required UI scope:** [UI_FRONTENDS.md](UI_FRONTENDS.md) covers generic native,
  stock/bitmap KSP, GUI Designer `.nckp`/`.nckc`, Komplete Script/Komplete UI,
  and Falcon Lua over shared controls, assets and native services. No family is
  complete. Plan-owned scalar UI handlers now execute; remaining callback services
  and rendering/sample editing are open.

- **Partial evidence:** `sampler-native` renders supported WAV through the new core
  without legacy dependencies. Live audio, host adapters and UI remain open.

- [ ] **P0; dependencies:** V2-12/13/14/15.
- New executables/plugin targets, product/state identity and complete UI bound to headless services.
  Upgrade the sample section and verify its real editing/playback flows against the new core.
  No legacy runtime switch, old session loader or old engine in production dependencies.
- **Done when:** declared native/import scope works in standalone and chosen DAWs;
  independent build and platform/host matrix pass; failed activation leaves valid v2
  state. Audit the dependency graph and packaged artifacts for legacy coupling.

## M5 — second semantic frontend

### V2-17 — Implement UVI source hierarchy and capability boundaries

- [ ] **P2; dependencies:** V2-07/10/11.
- New program/layer/keygroup/oscillator representation and lowering with authored fixtures.
- **Done when:** source addressing survives; inaccessible assets/generators are reported.
  Claims about real banks require authorized readable fixtures; metadata is not playback.

### V2-18 — Implement bounded UVI behavior semantics

- **2026-10-06 direction:** evaluate Luau against UVI's Lua 5.1/API and realtime
  requirements; preserve the [shared IR/service boundary](SHARED_IR.md). No per-format
  note engine and no forced script execution for declarative SFZ/DSP graphs.

- [ ] **P2; dependencies:** V2-05/12/13/17.
- Select a language implementation against concrete dispatch/coroutine/widget/async
  requirements and prove its realtime strategy through the shared services.
- **Done when:** supported contexts, cancellation, helper/GC costs and fault cleanup
  have evidence; new UVI-only note ownership is prohibited. Reference claims need probes.

## M6 — measured excellence and expansion

### V2-19 — Meet quality/performance targets and audit independence

- [ ] **P1; dependencies:** V2-16; measure incrementally from M1.
- Record callback percentiles/worst observed time, misses, memory, streaming and
  worker pressure on declared hardware. Compare layouts/SIMD at matching audio quality.
- **Done when:** numerical targets from V2-02 hold for admitted workloads; dependency
  and artifact audits show no legacy engine/VM/state requirement; reproducible reports
  support performance claims. Remove obsolete prototype code after checking callers.
- Parity with the old KONTRA engine is not a release gate. Full Kontakt KSP parity
  is an explicit product requirement under V2-14; new-core/UI integration is required
  under V2-16.

### V2-20 — Deliver broader source and product capabilities

- [ ] **P2; dependencies:** relevant core contracts and V2-19 evidence for release.
- Track advanced stretch/morph/source modes, wider formats, export and independently
  distributed modules as concrete follow-on features from the supplied specifications.
- **Done when:** each feature has semantics, access, lifetime/cost budgets, quality tests
  and capability evidence. No speculative public ABI or unsupported parity claims.

## Closing work

Record changed entry points, test commands/results, source/build and fixture identity,
quality/performance settings and remaining limits. Keep all 128 supplied scenarios
allocated; their original bytes/statuses remain unchanged. MIDI 2.0 protocol fixtures
must supplement that catalogue. No task closes on document wording alone.

## Legacy reference extraction (2026-10-06 steering)

- [x] Locate the separate Falcon/UVI implementation and review selected v1 Kontakt
  import/runtime/tests and UVI graph/dispatch/ownership/evidence paths. The source
  pins, useful fixtures and explicit differences are in
  [REFERENCE_REVIEW.md](REFERENCE_REVIEW.md#existing-kontra-implementations-as-migration-references).
- [ ] Re-author controller consumption, captured callback context, script-stage
  forwarding, generated handles/group eligibility and hard-cleanup state fixtures
  on the new runtime (V2-05/14). Old passing tests alone do not close these gates.
- [ ] Preserve reviewed source identity/decoding, UVI graph hierarchy, DSP default/
  control-clock and streaming edge cases in new import/render fixtures (V2-07–13).
  No old-runtime fallback or wholesale branch merge.
