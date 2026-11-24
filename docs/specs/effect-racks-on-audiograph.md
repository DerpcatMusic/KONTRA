---
status: in-progress
project: plugins/kontra
owner: claude-opus-5-5
created: 2026-11-24
updated: 2026-11-24
proof_scope: internal
evidence_freshness_days: 180
---

# Phase 3: one effect-routing path (racks on `AudioGraph`)

## Problem & Goal

- **Problem:** the engine has two routing back ends after the voices. Every
  translated format (Kontakt, UVI, SFZ, SF2, DecentSampler) runs
  `Engine::mix_out`: a hand-written loop over the plan's buses (rack with
  routes, LFOs and live `follow`, 3.7 ms fader, 50 ms balance, pre/post sends
  with modulated levels, peak meter, idle skip), then the instrument's
  insert, send and main racks. Only `.kontra` files run `GraphRuntime`
  (kontra-racks), whose bus node is a static rack with a fixed gain and pan.
  Two paths means every routing feature is written twice or, as measured
  below, missing from one.
- **Goal:** one routing path. `prepare` lowers every plan to an
  `ir::AudioGraph`; the engine routes every instrument through
  `GraphRuntime`; `mix_out`'s bus loop is deleted and an arch guard keeps it
  from coming back. Renders stay identical (threshold below).
- **Plan Reference:** [13-IR-VNEXT.md](../architecture-v3/13-IR-VNEXT.md),
  row "3: graph, generators, effect families", "Still to do".

### Baseline measurement (2026-11-24, 21a6979b2 + harness)

`ab_routing` (below), rack path vs the plan's buses lowered to today's
graph nodes (`Bus` with copied effects), 4 keys held 1.5 s, 4 s:

| Library | Rack vs rack | Graph vs rack (peak, rel. rack peak) | Cause |
|---|---|---|---|
| Areia 16 Violins | −inf | −136.6 dB | rounding (`(x·g)·l` vs `x·(g·l)`) |
| Conflux | −inf | −135.9 dB | rounding |
| VWinds C Flute | −inf | −26.5 dB | "Aux 1" bus: 13 routes, not run by the graph's static rack |
| AO V Strings Bartok | −inf | −79.8 dB | buses A1–B2: 4 routes + 9 LFOs each (Delay, Phaser) |

The rack-vs-rack floor is bit-exact, so the harness is deterministic and
every residual is a routing difference.

## Primary-Source & Version Proof

N/A — internal refactor: no external API, format or protocol is touched.

## Design

The graph schedules and routes; a bus stays one processor.

1. **`ir::GraphNodeKind::InstrumentBus { bus: u16 }`**: instrument bus
   `bus` (`Instrument::buses[bus]`, or the implicit Main bus when the list is
   empty), as `GroupInput` names a group. Its rack, fader, pan, routes, LFOs
   and sends belong to the bus and are not copied into the graph, so slot
   numbers (what KSP `$ENGINE_PAR_*` and routes address) cannot shift.
2. **`BusState` moves to kontra-racks** unchanged. `GraphRuntime::process`
   takes the engine's buses and a `Live` and runs an `InstrumentBus` node as
   `mix_out` runs a bus: `follow`, rack (with instrument send taps), idle skip,
   pre-fader sends, fader, post-fader sends (levels scaled per frame where an
   oscillator moves them, per span otherwise), balance, peak. Its input is the
   bus's own `mix`, which voices and shared chains already fill, so group
   chain sharing is unchanged.
3. **`kontra_prepare::lower_graph(&Plan)`** emits one `InstrumentBus` node per
   plan bus, each send as an edge (only to a later bus, as `mix_out`: a send
   to an earlier bus is dropped), and each `to_main` bus into one `Output`.
   The instrument insert, send and main racks and the output stage stay after
   the graph on the main stem, as in graph mode today.
4. **(b)** `prepare` sets `plan.graph = lower_graph(&plan)` for every format
   without a graph of its own; `mix_out` keeps only the instrument racks; the
   bus loop goes.

### Departures from the brief, and why

- **No translator codemod.** Translators write slotted racks
  (`Group/Bus/Instrument::inserts`): that is the format's model and where KSP
  slot numbering lives. Lowering in `prepare` reaches every format with zero
  translator edits, so there are no rack-construction call sites to script.
  The scripted part is (b)'s engine deletions.
- **Group inserts stay engine-side.** A group's inserts run per voice where a
  route or drive needs it (per-voice modulation, VISION) and otherwise on a
  summed chain shared by every group with equal sections (LTI, one run per
  span). Both feed the bus `mix` that the `InstrumentBus` node reads. Making
  each group a graph node would lose the sharing; `.kontra` graphs keep
  `GroupInput` for formats that route groups themselves.

## Invariants & Non-Goals

### Hard Invariants
- **Audio RT-Safety:** `GraphRuntime::new` (plan swap, off the audio thread)
  allocates every buffer; `process` allocates nothing (`rt-paranoid`).
- **Determinism & Compatibility:** renders null against the rack path within
  the threshold; KSP slot numbering, `Strip` semantics, `.kontra` files and
  saved presets are unchanged.
- **Mirror Boundaries:** nothing outside `plugins/kontra`.
- **Additional Domain Invariants:**
  - A send only reaches a later bus (`mix_out`'s order).
  - `set_bus_gain`, `set_bus_enabled`, script/UI writes to a bus slot and
    `take_bus_peak` act on the same `BusState` the graph runs.
  - The engine never names a vendor.

### Non-Goals
- Moving group inserts or per-voice racks into graph nodes (see above).
- Multi-stem output for translated formats; sidechain edges from bus nodes.
- Changing any effect's DSP.

## API / Data / File Contract

- `ir::GraphNodeKind::InstrumentBus { bus: u16 }`; `AudioGraph::validate`
  accepts it. The `.kontra` schema does not save it: `prepare` makes it.
- `kontra_prepare::lower_graph(&Plan) -> ir::AudioGraph`.
- `kontra_racks::BusState` (moved from kontra-engine).
- Harness: `cargo run --release -p kontra-host --example ab_routing -- <file> [keys] [seconds]`;
  `KONTRA_AB_WAV=<dir>` writes both renders, `KONTRA_AB_BUSES=1` lists buses.

## Verification Matrix

**Null threshold:** graph vs rack peak residual ≤ −120 dB relative to the
rack render's peak, with a rack-vs-rack floor of −inf (bit-exact) on the
same run. Rounding measured today is −136 dB.

| Check | Tier | Target / OS | Exact Command | Pass Criterion | Status / Observed Evidence |
|---|---|---|---|---|---|
| Spec proof gate | Doc | any | `python3 tools/check-spec-proof.py plugins/kontra/docs/specs/effect-racks-on-audiograph.md` | Exit 0 | PASS (2026-11-24, linux-x86_64, e42e7354a) |
| A/B Areia | Validator/Runtime | linux-x86_64 | `ab_routing "<Areia>/01 Areia - 16 Violins - Core Techniques.nki"` | floor −inf, peak ≤ −120 dB | PASS (2026-11-24, linux-x86_64, e42e7354a) |
| A/B Conflux | Validator/Runtime | linux-x86_64 | `ab_routing "<Conflux>.nki"` | floor −inf, peak ≤ −120 dB | PASS (2026-11-24, linux-x86_64, e42e7354a) |
| A/B VWinds | Validator/Runtime | linux-x86_64 | `ab_routing "<VWinds C Flute>" 72,76,79,84` | floor −inf, peak ≤ −120 dB | PASS (2026-11-24, linux-x86_64, e42e7354a) |
| A/B UVI insert FX | Validator/Runtime | linux-x86_64 | `ab_routing "<V Strings Bartok>.uvip"` | floor −inf, peak ≤ −120 dB | PASS (2026-11-24, linux-x86_64, e42e7354a) |
| Lowering + runtime tests | Compile/ABI | linux-x86_64 | `build-slot cargo test -p kontra-prepare -p kontra-racks -p kontra-ir graph` | Exit 0 | PASS (2026-11-24, linux-x86_64, e42e7354a) |
| RT-safety | Validator/Runtime | linux-x86_64 | `matari check --project kontra` (rt-paranoid stage) | Zero RT violations | PASS (2026-11-24, linux-x86_64, e42e7354a) |
| Fast unit/lib tests | Compile/ABI | linux-x86_64 | `matari fast kontra` | Exit 0 | PASS (2026-11-24, linux-x86_64, e42e7354a) |
| Arch guard (b) | Compile/ABI | linux-x86_64 | `build-slot cargo test -p kontra-engine --test arch_guards` | old bus loop absent | PENDING |

## Unverified Boundaries & Promotion Gate

None — all verification tiers execute on the current host.

**Hard Promotion Rules:**
1. This spec **MUST NOT** move to `status: implemented` while any required row in `## Verification Matrix` is `PENDING`, `PENDING_OS_CI`, or `BLOCKED`, or while any unchecked `- [ ]` acceptance criterion remains.
2. An unverified OS backend or platform path **MUST NOT** be flipped to default until its `PENDING_OS_CI` row becomes `PASS` on real OS CI or hardware.

## Acceptance criteria

- [x] (a) The four A/B renders meet the null threshold through `InstrumentBus` nodes.
- [x] (a) `GraphRuntime::process` allocates nothing with instrument buses (rt-paranoid).
- [ ] (b) Every format's plan carries a graph; `mix_out` has no bus loop; arch guard added.
- [ ] (b) The A/B renders after the deletion match the last rack-path renders (WAVs kept in PluginArtifacts) within the threshold.

## Task Decomposition

| # | Subtask | Owned Paths | Depends On | Verification |
|---|---|---|---|---|
| 1 | A/B harness | `crates/app/host/examples/ab_routing.rs` | — | floor −inf |
| 2 | `InstrumentBus` node | `crates/model/ir/src/graph.rs`, `crates/format/native` | — | ir tests |
| 3 | `BusState` to kontra-racks; graph runs it | `crates/engine/racks/src/{bus,graph}.rs`, `crates/engine/engine/src/{buses,racks,filters,lib}.rs` | 2 | racks tests, A/B |
| 4 | `lower_graph` | `crates/engine/prepare/src/graph.rs` | 2 | prepare tests, A/B |
| 5 | (b) switch over, delete bus loop, guard | `crates/engine/prepare/src/lib.rs`, `crates/engine/engine/src/{racks,filters}.rs`, `crates/engine/engine/tests/arch_guards.rs` | 1–4 | A/B, arch guard |

## Rollout & Affected Dependents

- **Affected Projects:** `plugins/kontra` only.
- **Mirror Impact:** subtree mirror (`kontra`); no private paths.
- **Versioning & Changelog:** no user-facing change; no bump.
