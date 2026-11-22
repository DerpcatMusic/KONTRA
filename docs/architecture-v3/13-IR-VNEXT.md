# IR vNext: the engine overhaul plan

The v3 board (TASKS.md, P0–P8) built the fresh engine. This plan reshapes the
IR underneath it so that every format maps onto a small set of orthogonal
pieces instead of a Kontakt-shaped one. Source: the "KONTRA IR Architectural
Audit: Ground Truth & vNext Blueprint" (2026-11-22, orchestrated by Gemini).
Every step must still pass the [VISION.md](VISION.md) design test, and engine
laws still need a probe render against Kontakt (docs/re/NATIVE_ORACLE.md).

**A phase is done when the old structure it replaces is deleted.** Adding a
new type next to the old one is only the first half of a step.

## Status on main (cabee674c6, 2026-11-22)

The phase PRs below all merged, but most of them added the new types without
moving callers onto them. The numbers come from `git grep -c` on main.

| Phase | PRs | Landed | Still to do |
|---|---|---|---|
| 0: hygiene, vendor leaks | #102 | `ByteTransform` keystream, `Cow` tables, `ir::validate`, compact regions, typed fast path for zone `ignored_fields` | group `ignored_fields` still formats Debug text; only the `.kontra` loader runs `validate`; the translators and prepare don't |
| 1: one modulation and envelope core | #109, #110 | separate decay/release curves, equal-power and CC crossfades, `lorand`/`hirand`, `PerNoteCc`, `NoteExpr`, `RrScope::Key`, and `ParamAddr`/`ModRoute` declared | **`ModRoute` has 0 users; `ModKind::` has 458.** `WARPS = 8` cap (36 uses), the CC74 brightness hack in `voice_gain.rs`, UVI macros as `Script(0x8000+…)`, UVI `PROBE_GRID` (23 rebuilds per attribute) |
| 2: compact zones, SDK, native | #96, #111, #116 | `ZoneMatch`/`ZonePlay` SoA used by `ZoneIndex`, format SDK (DecentSampler translator, every documented effect), `.kontra` native format | `plan.zone_facts` still duplicates every zone (`ZoneFacts`, `loop_pars` per zone); no Areia RAM measurement of the result |
| 3: graph, generators, effect families | #112, #113, #114 | `AudioGraph` with sidechain and multi-stem output, `Generator` (PCM, oscillator, granular), `Engine::set_param(ParamAddr)` | `EffectFamily`/`EffectSpec` are read only by the `.kontra` schema; the engine still runs the 31-variant `EffectKind` |

## Next wave: lanes

There is one lane per row. Lanes own disjoint paths. Each lane finishes by
deleting what it replaces, and proves the result with scoped tests plus one
real-library load (Areia, Conflux or VWinds).

| Lane | Goal | Owns | Number to move |
|---|---|---|---|
| L1 modulation core | Every translator emits `ModRoute`; plan and engine read it. Delete `ModKind`. Lift the `WARPS = 8` cap. Brightness becomes `NoteExpr`, and UVI macros become `Macro` | `model/ir`, `engine/plan/src/modulation.rs`, `engine/engine/src/{modulators,voice_gain,start}.rs`, the modulation emitters in each `format/*` | `ModKind::` uses: 458 → 0 |
| L2 UVI parameter names | Replace `PROBE_GRID` with a declared name → `ParamAddr` + law table | `format/uvi/src/program/routing.rs` | rebuilds per attribute at load: 23 → 0 |
| L3 one zone table | Drop the `zone_facts` copy; scripts read zones from the SoA tables; `loop_pars` only for user zones | `engine/plan/src/zones.rs`, the zone parts of `engine/prepare`, `engine/engine/src/zone_commands.rs` | Areia metadata RAM, measured before and after |
| L4 Kontakt parity | Continue the #115 audit: each translator law is checked against the oracle or the binary, not assumed | `format/kontakt` (not its modulation emitter, which L1 owns), `vendor/ni-file` | translator laws with no evidence → 0 |

Deferred: consolidating `EffectKind` into families. VISION and the KISS caveat
keep a separate DSP recurrence wherever a measurement shows it is audible, so
this needs a null test per effect first. `rkyv` waits until a cached load is measured too slow.
