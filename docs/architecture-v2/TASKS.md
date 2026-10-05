# 2.0 task list — clean-sheet implementation

Revised 2026-10-05 following explicit user direction: implement a new product with
no KONTRA 1.x backward compatibility requirement. [PLAN.md](PLAN.md) is the current
contract. Existing task IDs remain stable for the [scenario allocation](CONFORMANCE_MAP.md),
but legacy extraction/migration requirements are superseded here.

The existing kernel is an experiment with partial V2-03/04 evidence. Its old-VM KSP
bridge is historical boundary evidence, not progress toward a new scripting runtime.
Family/expression ownership and segmented rendering now have
[additional executable evidence](OWNERSHIP_SLICE.md). No full implementation gate is closed. The four supplied references remain intact.

## Completed groundwork

- [x] Isolate worktree, preserve all attachments and record their hashes.
- [x] Map legacy ownership and reproduce terminal-loss and torn-snapshot failures.
- [x] Implement/test the first independent note/PCM prototype.
- [x] Replace the migration mandate with clean-sheet implementation requirements.

## M0 — contracts and measurable targets

### V2-01 — Establish independent conformance and workload evidence

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

- [ ] **P0; dependencies:** V2-03/04/05.
- Separate physical key, gate, sostenuto capture and source release. Specify and
  implement sustain, retrigger, panic, half-pedal and repedal semantics.
- **Done when:** pressure/pedal-up bursts preserve cleanup, release context and
  source-ended notes; no omitted positive scenario is counted as passing.

## M2 — instrument compiler and rendering

### V2-07 — Build new source, semantic and prepared representations

- [ ] **P1; dependencies:** V2-02/05.
- Native authoring plus new Kontakt and one open-format frontend lower into neutral
  semantic data; preserve source identity, unknown data and explicit capability reports.
- **Done when:** bounds/asset paths are validated; unsupported required behavior fails
  preparation; no parser/vendor objects cross into musical or render execution.

### V2-08 — Compile selection, articulation and variation

- [ ] **P1; dependencies:** V2-04/06/07.
- Implement indexed selection, scoped counters, family take decisions, phrase and
  release context, with a straightforward independent reference evaluator.
- **Done when:** compiled/reference results agree, multimic takes remain coordinated,
  release sequences stay independently expressible and consumed switches do not leak.

### V2-09 — Implement source/DSP kernels with explicit state scope

- [ ] **P1; dependencies:** V2-04/07/08.
- New playback/resampling/loop/envelope/filter/modulation execution; declare units,
  rate, voice/family/bus scope, latency, tails, demand windows and quality modes.
- **Done when:** boundary guards, reverse/loops, nonlinear scope, expression isolation,
  steals/fades and muted-layer continuity pass numerical and audio-quality tests.
  Record alias rejection, stability and cost at declared quality settings.
- No silent quality reduction or block-rate substitution under overload.

## M3 — resource service and realtime guarantees

### V2-10 — Implement preparation, adoption and retirement

- [ ] **P0; dependencies:** V2-02/07/09.
- New worker/control/audio exchanges with immutable plans, tagged requests, bounded
  retained generations and reserved retirement capacity before adoption.
- **Done when:** failed loads preserve the active v2 plan; stale completions cannot
  publish; finish/choke transitions and exactly-once off-audio destruction are tested.

### V2-11 — Implement asset storage and streaming

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

- [ ] **P0; dependencies:** V2-05/07/10.
- New stable automation/control IDs, coherent bounded capture, state schema and UI
  command/view model. Plan v2 schema evolution; no 1.x conversion requirement.
- **Done when:** correlated snapshots stay coherent during mutation; UI-closed results
  agree; reordered compiled tables preserve automation identity; recalls are transactional.

### V2-14 — Implement new KSP frontend and external behavior profiles

- [ ] **P1; dependencies:** V2-05/08/09/13.
- Implement parsing/lowering and documented services against the new behavior runtime.
  Inventory supported builtins, async calls, persistence and controls from real fixtures.
- **Done when:** every claimed feature has service tests and appropriate pinned reference
  evidence. Approximate/unsupported/unverified features remain clearly distinguished.
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

- [ ] **P0; dependencies:** V2-12/13/14/15.
- New executables/plugin targets, product/state identity and UI bound to headless services.
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
- Legacy repair, behavioral parity and migration are not release gates.

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
