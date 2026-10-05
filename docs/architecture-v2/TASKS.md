# 2.0 task list

Status at 2026-10-05: first experimental slice implemented; see
[implementation evidence](IMPLEMENTATION.md). V2-01 through V2-05 have partial
progress; none of their full gates is closed. Later tasks are not started.
Priority is execution order, not a delivery-date estimate.
Task ownership means subsystem responsibility; no individuals or permanent file
locks are assigned. [PLAN.md](PLAN.md) defines the contracts and milestone gates.

## Completed planning work

- [x] Confirm isolated `docs/plan-v2-architecture` worktree and source baseline.
- [x] Preserve all four attachments with byte hashes.
- [x] Map the main import/load, host/event, KSP, voice, resource, and UI/state boundaries.
- [x] Record proposed native decisions, migration/rollback gates, and implementation tasks.
- [x] Allocate every supplied conformance scenario to a task; leave execution unclaimed.

## M0 — establish the baseline and decisions

### V2-01 — Capture current behavior and reproduce ownership risks

- **Progress:** focused regressions pass; both flagged defects reproduced. Exact
  reproducers are retained in `probes/legacy.rs`. Production fixes and workload
  timing/memory baselines remain required before closing this task.
- [ ] **P0; depends on:** no implementation task. **Boundary:** conformance/legacy.
- Run the focused host ownership, MPE reuse, stale completion, async KSP and
  persistence regressions identified in [CURRENT_STATE.md](CURRENT_STATE.md).
  Capture build/profile/fixture identity and baseline timing/memory for tiny fixtures.
- Add focused reproductions for rejected NOTE_END on no-owner paths, continuous
  mutation during multi-budget snapshot capture, and replacement under queue pressure.
- **Done when:** actual results are recorded, baseline failures are separated from
  migration regressions, and the two flagged risks are either reproduced or closed
  with tests. Test presence alone is insufficient. Fix confirmed ownership loss at
  its shared source before using that behavior as a migration oracle.
- **Existing seam:** plugin `process`/`finish_host_notes`, runtime snapshot refresh;
  primary catalogue coverage remains assigned to V2-13/15.

### V2-02 — Ratify identities, native policies, and resource budgets

- **Progress:** prototype contract and fixture capacities recorded in IMPLEMENTATION;
  all-domain queue inventory, production budgets and full profile decisions remain open.
- [ ] **P0; depends on:** V2-01. **Boundary:** core contract.
- Specify the native defaults proposed in PLAN, tagged/signed external IDs,
  generational handle behavior, raw/projected state, unit domains and lifecycle states.
- Inventory every queue and plan/job epoch used by the selected slice, with producer,
  consumer, cap, full-queue action and destructor. Choose numeric capacities from the
  admitted fixture/workload set; include terminal, release and retirement reserves.
- **Done when:** the contract has explicit answers for repeated keys, child lifetime,
  MPE channel reuse, release selection, same-time ordering, clock overflow, failure
  cleanup and plan transitions; unresolved vendor behavior remains a named probe.
- **Deletion target:** undocumented coupling of capacities to KSP and pointer identity
  as semantic identity, as their respective call paths migrate.

## M1 — executable semantic slice

### V2-03 — Introduce the bounded logical-note kernel

- **Progress:** independent crate, note/voice generations, provenance, children,
  continuation pins, FIFO fallback and terminal retention implemented/tested. Explicit
  family identity, expression inheritance and richer lifecycle states remain open.
- [ ] **P0; depends on:** V2-02. **Boundary:** `sampler-core`, first functional commit.
- Add typed input/note/family/voice handles, original/transformed addresses, ownership
  admission, parent links, expression policy and cleanup. Test slot reuse and near-wrap
  arithmetic; do not silently recycle a still-observable identity.
- **Done when:** independent logical/source limits, one-shot completion, linked versus
  detached children, family targeting and stale commands have executable assertions.
  A bounded mock host sink rejects terminal output and later accepts exactly one
  retry without losing or reusing the original identity, including no-source notes.
  Core compiles without root/plugin/parser/KSP dependencies.
- **Reuse:** host-note arena invariants and existing authored fixtures. **Replace:**
  ownership spread across host/KSP/key tables only as each caller is adapted.

### V2-04 — Build the deterministic event/PCM harness

- **Progress:** resident unity-rate stereo PCM, delayed start/release and cancellation
  pass regular/irregular/zero partition checks at 44.1/48 kHz. Unified external-event
  and continuation scheduling, trace tooling and projected CC state remain open.
- [ ] **P0; depends on:** V2-03. **Boundary:** core scheduling/headless tests.
- Reuse/extract the smallest pure PCM playback primitives needed for an impulse fixture.
  Merge external, scheduled and continuation boundaries with a bounded equal-time loop.
  Preserve projected state and transport-clock separation.
- **Done when:** traces and onset positions agree across the PLAN partition matrix;
  zero blocks, end-boundary events, same-time CC/expression, delayed cancellation and
  recursive zero-time generation terminate correctly. Pin numerical tolerances explicitly.
- **Deletion target:** separate scheduling rules for migrated paths; existing alignment
  remains an explicit processor until its semantics are represented and tested.

### V2-05 — Put the first real KSP behavior on the kernel

- **Progress:** authored KSP suppression/children/wait/release fixture uses the new
  kernel with no measured callback heap operations. Bridge is test-only and handles
  one root; production mapping, fault cleanup, expression and readback remain open.
- [ ] **P0; depends on:** V2-03/04. **Boundary:** KSP service adapter.
- Adapt existing KSP operations for suppress/create/wait/release, polyphonic variables,
  callback identity and child expression to shared services. Keep the existing parser/VM.
  Exercise consumed CC visibility and command/query readback.
- **Done when:** one authored KSP fixture suppresses input, creates linked and detached
  notes, resumes after a wait, and cleans up after fault/steal/cancel through the same
  kernel as native notes. Unsupported services return precise diagnostics.
- **Deletion target:** the corresponding direct `Player`/vendor-specific ownership path
  after all selected callers are migrated; no copied KSP runtime.

### V2-06 — Add gate/pedal and release reserves

- [ ] **P0; depends on:** V2-03/04/05. **Boundary:** note lifecycle.
- Separate physical key, effective gate, sostenuto capture and source release; define
  sustain/retrigger, panic, half-pedal and repedal policies as explicit supported features.
  Budget pedal-up bursts and cleanup independently from new release-source admission.
- **Done when:** saturated note traffic cannot orphan cleanup; native pedal policies
  have traces, including source-ended-before-key-up. Vendor release eligibility is
  left unverified until probed. M1 may defer half-pedal/repedal execution only by
  declaring them unsupported; it cannot claim all pedal scenarios passed.

## M2 — source separation and prepared execution

### V2-07 — Add source-to-semantic lowering and capability reporting

- [ ] **P1; depends on:** V2-02/05. **Boundary:** compiler/frontends.
- Preserve Kontakt source IDs/order/unknown fields, lower a concrete supported subset,
  and add strict/best-effort reports with feature/profile/source location/effect.
  Add one small authored SFZ or Decent subset to expose Kontakt assumptions.
- **Done when:** both frontends feed the same semantic model; unsupported start
  conditions/scripts/source modes cannot silently become successful imports. Bound
  parser structure, declared sizes and asset paths. No claim of full format coverage.
- **Deletion target:** runtime reliance on `ni_file` object types for the migrated subset.

### V2-08 — Compile selection, articulation, and variation decisions

- [ ] **P1; depends on:** V2-04/06/07. **Boundary:** selection/compiler.
- Reuse key candidate indices; add scoped selection counters, family take decisions,
  release policies and phrase relationships. Keep a simple reference evaluator in tests.
- **Done when:** optimized/reference selection agrees on generated small maps; coherent
  multimic takes, independent release RR, consumed switches and MPE/channel separation
  have traces. Reference-probe scenarios stay unverified without target observations.
- **Deletion target:** duplicated vendor/route-specific selectors for migrated features.

### V2-09 — Separate source/DSP scope and expression units

- [ ] **P1; depends on:** V2-04/07/08. **Boundary:** prepared DSP/modulation.
- Extract existing loop/resampling/envelope/filter kernels as required; describe
  voice/family/bus state, units, rates, latency, tails and source demand. Make stealing
  and fade reserves explicit. Preserve high-resolution expression until adapter conversion.
- **Done when:** PCM boundaries and reverse/loop guards pass; per-voice nonlinear
  processing differs correctly from bus processing; expression cannot leak on channel
  reuse; muted-layer reactivation has a declared state-continuity policy. No silent
  block-rate downgrade or algorithm substitution under load.
- **Deletion target:** duplicate migrated DSP and untyped parameter-conversion paths.

## M3 — production resource lifetimes

### V2-10 — Extract transactional preparation/adoption/retirement

- [ ] **P0; depends on:** V2-02/07/09. **Boundary:** application/worker exchange.
- Extract the relevant `Load`, `Handoff`, `Retired` responsibilities. Tag work by stable
  part/plan/script/request identity; distinguish source replacement from residency upgrade.
  Reserve retirement capacity before adoption and bound live plan generations.
- **Done when:** held notes follow the selected finish/choke policy; failed loads keep
  active state; stale async products retire off audio; repeated replacement under
  saturation has bounded memory and exactly-once destruction.
- **Deletion target:** duplicated generation comparisons and migrated loader/callback
  orchestration in `plugin.rs`, not all plugin code in one move.

### V2-11 — Adapt existing streaming to explicit asset demand

- [ ] **P1; depends on:** V2-09/10. **Boundary:** asset workers.
- Reuse source decoding, residency, worker pool and rings where they satisfy measured
  needs. Expose asset/view identity and demand windows for rate, reverse, offsets,
  interpolation and loop crossfades. Measure before replacing the cache/ring design.
- **Done when:** storage stalls, cold onsets and admitted extreme pitch changes never
  block live rendering; starvation policy and counters are visible; shared PCM does
  not share voice history. Keep offline preparation under a separate contract.
- **Deletion target:** source-specific scheduling duplicates only after equivalent demand
  and lifetime tests pass.

### V2-12 — Audit realtime bounds and fault containment

- [ ] **P0; depends on:** V2-05/06/09/10/11. **Boundary:** execution safety.
- Instrument allocations/deallocations for render, note bursts, faults, swaps and
  disposal. Review blocking/I/O, native builtin cost, malformed values and zero-time work.
- **Done when:** the admitted stress matrix has no forbidden callback operations or
  orphaned cleanup; every queue has an exercised overflow policy. This is an ongoing
  gate for later changes, not a one-time promise based on normal-path tests.

## M4 — controls, compatibility, and application cutover

### V2-13 — Extract headless controls and coherent state migration

- [ ] **P0; depends on:** V2-05/07/10. **Boundary:** state/control service.
- Define stable control/automation IDs, consistent snapshot capture and versioned
  legacy recall conversion; preserve original saved data. Bind existing MUI to views/commands.
- **Done when:** correlated-state snapshot stress is coherent, editor open/closed runs
  agree, automation survives reordered prepared tables, missing assets are handled
  off-thread and profile changes require explicit migration.
- **Deletion target:** musical state requiring plugin UI structures for ownership.

### V2-14 — Broaden KSP through versioned compatibility profiles

- [ ] **P1; depends on:** V2-05/08/09/13. **Boundary:** Kontakt adapter.
- Inventory required builtins from supported fixtures and port services by behavior,
  including engine parameters, persistence, async completion and scoped controls.
  Pin reference versions for slot propagation, numeric edges and restore ordering.
- **Done when:** each supported feature has native/service and profile-specific evidence;
  unknowns fail strict mode or carry explicit approximation/unverified status.
- **Deletion target:** KSP-specific fields and dispatch in the neutral core; retain
  justified dialect behavior in KSP itself.

### V2-15 — Migrate protocol and host lifecycle adapters

- [ ] **P0; depends on:** V2-03/04/09/10/13. **Boundary:** CLAP/VST3/MIDI/MPE.
- Preserve signed IDs/wildcards/precision, protocol-specific zero velocity, tuning,
  output backpressure, reset/suspend, sample-rate rebuilds and multi-output rules.
  Reuse vendored exact-event support; inventory patches before changing it.
- **Done when:** terminal retries cover admitted and no-owner cases, including same-key
  retriggers; variable/zero blocks and host lifecycle tests pass. MIDI 2/UMP remains a
  separate future adapter until implemented; reserving precision is not support.
- **Deletion target:** duplicated migrated normalization/terminal logic in the plugin.

### V2-16 — Integrate the vertical slice and preserve legacy sessions

- [ ] **P0; depends on:** V2-12/13/14/15. **Boundary:** application composition.
- Select legacy/v2 at preparation, expose precise support status, and migrate CLI,
  plugin and applicable audit entry points. Exercise existing saved multis and host state.
- **Done when:** a reviewed supported subset works headlessly and in chosen CLAP/VST3/
  standalone hosts; failed activation rolls back intact; relevant legacy and new
  regressions plus required CI pass with build evidence. No blanket v2 default until
  the supported scope and state migration are accepted.

## M5 — prove a different semantic frontend

### V2-17 — Preserve UVI hierarchy and asset capability boundaries

- [ ] **P2; depends on:** V2-07/10/11. **Boundary:** UVI source frontend.
- Extend existing metadata groundwork with authored clear hierarchy/mapping fixtures;
  preserve program/layer/keygroup/oscillator identities and explicit mapping dimensions.
- **Done when:** lowering retains source addressing and reports unavailable assets or
  generators. UFS header inspection is never reported as playable bank support.
- **External gate:** an authorized readable target fixture is required for claims
  about real target banks; synthetic work can proceed without it.

### V2-18 — Prove bounded UVI callback semantics

- [ ] **P2; depends on:** V2-05/12/13/17. **Boundary:** UVI behavior frontend.
- Choose a Lua/runtime approach from concrete API needs; prove dispatch precedence,
  forwarding, coroutine state, widget contexts, async readiness and budget cleanup.
- **Done when:** native service tests and pinned UVI probes are separate; no unsupported
  callback context is silently treated as yieldable; allocation/native-helper/GC limits
  are demonstrated for the admitted slice. Do not infer UVI compatibility from Lua syntax.
- **Deletion target:** none until this replaces a real path; never add a UVI-only allocator.

## M6 — simplify after cutover

### V2-19 — Delete superseded paths and measure matched workloads

- [ ] **P1; depends on:** V2-16; V2-18 additionally for UVI-specific retirement.
- Query callers, migrate remaining entry points, delete duplicate old paths and their
  temporary switches. Retain historical behavioral fixtures; update source maps/docs.
- **Done when:** each deletion has no remaining caller and required tests pass; compare
  callback distributions, misses and memory using matched quality/workloads. Keep old
  state readers as long as the supported migration contract needs them.
- Optimize only measured costs; private audio pools/SIMD redesign are separate justified
  changes. Rollback remains available until the migrated scope is accepted.

### V2-20 — Expand only against concrete capability gaps

- [ ] **P2; depends on:** V2-19 and an identified supported-fixture requirement.
- Consider wider open formats, SampleTank/SINE, advanced stretch/morph/source modes,
  export workflows, or an independently distributed module API as separate tasks.
- **Done when:** each accepted extension names source access, semantics, implementation,
  reference evidence, state compatibility and measured cost. No external ABI freeze
  until materially different in-tree adapters have exercised the contract.

## How to close tasks

Attach changed entry points, exact test commands/results, source/build identity,
fixture/profile hashes, known limitations and legacy deletion status. Reference
probes require observations from a pinned reference; a native assertion cannot
close them. The [conformance allocation](CONFORMANCE_MAP.md) covers all 128 supplied
IDs, but task completion also includes the task-specific checks above.

The first implementation change should close a narrow runnable portion of V2-01;
the first core change should pair V2-03 with a minimal V2-04 check. Do not start with
mass file moves, renamed mega-structs, or empty subsystem crates.
