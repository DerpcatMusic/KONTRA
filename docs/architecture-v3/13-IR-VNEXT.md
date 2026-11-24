# IR vNext: the engine overhaul plan

The v3 board (TASKS.md, P0–P8) built the fresh engine. This plan reshapes the
IR underneath it so that every format maps onto a small set of orthogonal
pieces instead of a Kontakt-shaped one. Source: the "KONTRA IR Architectural
Audit: Ground Truth & vNext Blueprint" (2026-11-22, orchestrated by Gemini),
kept verbatim in [ir-vnext-blueprint-2026-11-22.md](ir-vnext-blueprint-2026-11-22.md)
(§6.3 lists the phases).
Every step must still pass the [VISION.md](VISION.md) design test, and engine
laws still need a probe render against Kontakt (docs/re/NATIVE_ORACLE.md).

## When a step is done

A step is done only when all of these hold:
1. its PR is merged on main;
2. the old structure it replaces is deleted (adding a new type next to the
   old one is only the first half);
3. its number has reached the target in the table below, checked on main
   with the same `git grep -c` or measurement;
4. its row here says ✅ with the PR number, changed in that same PR.

Anything less is "in progress", whatever a PR title says. The coordinator
checks 2 and 3 on main before marking a step ✅.

## Status on main (e0436491e, 2026-11-24)

The phase PRs below all merged, but most of them added the new types without
moving callers onto them; the lanes and waves below did most of the moving.
The numbers come from `git grep -c` on main.

| Phase | PRs | Landed | Still to do |
|---|---|---|---|
| 0: hygiene, vendor leaks | #102, #146 | `ByteTransform` keystream, `Cow` tables, `ir::validate`, compact regions, typed fast paths for zone and group `ignored_fields`; every translator (Kontakt, SFZ, SF2, DecentSampler, UVI, native) and `prepare` run `ir::validate` ✅ #132, #146 | the Kontakt program-level `ignored_fields` still formats Debug text (`format/kontakt/src/translate.rs`, once per load, not per zone) |
| 1: one modulation and envelope core | #109, #110, #123, #128 | separate decay/release curves, equal-power and CC crossfades, `lorand`/`hirand`, `PerNoteCc`, `NoteExpr`, `RrScope::Key`; every route is a `ModRoute`, `ModKind` deleted, `WARPS = 8` lifted, brightness reads `NoteExpr`, UVI macros are `Source::Macro` ✅ #128; `PROBE_GRID` 23 → 0 ✅ #123 | UVI mono-to-stereo pan laws (B P1-8) |
| 2: compact zones, SDK, native | #96, #111, #116, #123 | `ZoneMatch`/`ZonePlay` SoA used by `ZoneIndex`, format SDK, `.kontra` native format; `ZoneFacts`/`zone_facts` deleted, Areia 8.11 → 2.93 MiB ✅ #123 | nothing |
| 3: graph, generators, effect families | #112, #113, #114, #155, #186 | `AudioGraph` with sidechain and multi-stem output, `Generator` (PCM, oscillator, granular), `Engine::set_param(ParamAddr)`; per-pair null test of every rack kind ([report](effect-family-null-report.md)) ✅ #155; one effect encoding: the unread `EffectFamily`/`EffectSpec` deleted, references 25 → 0, ratchet in `arch_guards.rs` ([spec](../specs/effect-family-subtract.md)) | every non-`.kontra` instrument still runs the per-group racks (`inserts`/`sends`/`main`/buses); `AudioGraph` runs only `.kontra`. Moving the translators onto it, with A/B renders, needs its own spec |

## Next wave: lanes

Lanes own disjoint paths. Each lane finishes by deleting what it replaces, and
proves the result with scoped tests plus one real-library load (Areia, Conflux
or VWinds).

| Lane | Goal | Owns | Number to move |
|---|---|---|---|
| L1 modulation core | Every translator emits `ModRoute`; plan and engine read it. Delete `ModKind`. Lift the `WARPS = 8` cap. Brightness becomes `NoteExpr`, and UVI macros become `Macro` | `model/ir`, `engine/plan/src/modulation.rs`, `engine/engine/src/{modulators,voice_gain,start}.rs`, the modulation emitters in each `format/*` | `ModKind::` uses: 458 → 0 ✅ #128 |
| L2 UVI parameter names | Replace `PROBE_GRID` with a declared name → `ParamAddr` + law table | `format/uvi/src/program/routing.rs` | ✅ #123 — rebuilds per attribute at load: 23 → 0 (`PROBE_GRID` & `values::probing` deleted; verified on VWinds Bb Clarinet) |
| L3 one zone table | Drop the `zone_facts` copy; scripts read zones from the SoA tables; `loop_pars` only for user zones | `engine/plan/src/zones.rs`, the zone parts of `engine/prepare`, `engine/engine/src/zone_commands.rs` | ✅ #123 — Areia (49,152 saved zones): ~8.11 MiB & 55,380+ heap allocs → 2.93 MiB (`3,076,368` B) & 0 per-zone allocs (`ZoneFacts`/`zone_facts` deleted; 0 B `user_zones`) |
| L4 Kontakt parity | Continue the #115 audit: each translator law is checked against the oracle or the binary, not assumed | `format/kontakt` (not its modulation emitter, which L1 owns), `vendor/ni-file` | #144 (StepMod, GroupDynamics) and #158 moved rows; `tools/coverage/engine.tsv` on e0436491e: 10 exact, 45 mapped (law not independently proved), 7 approx, 7 reported, 2 absent. Translator laws with no evidence: 45 + 7 → 0 |
| Wave 2A single-encoding & IR hygiene ✅ #155 | Replace `KnobModel.build: fn(...)` with serializable `ModelId(u16)` + wait-free `EffectModel` table; declarative `Module` / `UviModule` tables (all 16 Kontakt + 13 UVI coupled effects); delete 600+ LOC `controls_for_chunk`; collapse `script_knobs.rs` to TSV; fold `ui-model` `ParamMap` math into `ir::Law`; exact UVI rising-LFO mapper depth scaling without resampling; `AudioGraph` cycle errors surfaced in `prepare` & `Engine`; KSP `EnginePar` decoupling & `RackScheme`; mechanical arch ratchet tests | `model/{ir,ui-model}`, `engine/{plan,prepare,engine,vm}`, `script/lua`, `format/{kontakt-effects,kontakt,uvi}` | `fn_addr_eq` = 0, `fn(` in `model/ir` = 0, `Instrument: PartialEq`, vendor-word ratchets (`engine` ≤ 291, `dsp` ≤ 17, `model` ≤ 176); RT spike: 0 allocs across Kontakt `Tape` (215.9 ns/knob) and UVI `ThreeBandShelves` (377.2 ns/knob vs 608.6 ns for 3x `EffectField`, 1.61× faster) |
| Wave 3 Kontakt parity ([spec](../specs/wave3-parity.md)) | Close L4 gaps with evidence in hand; make the coverage table name current code | `engine/behaviour`, `format/kontakt`, `tools/coverage/engine.tsv` | start joins left-folded → `and` binds tighter than `or` ✅ #158; stale coverage rows corrected: 10 |
| Wave 1A engine safety ✅ #146 | Modulation/recall safety, IR validation in every translator and prepare, null report, inspect lints | `engine/*`, `format/*`, `tools/inspect` | `ir::validate` callers: `.kontra` only → 6 translators + `prepare` (#132, #146) |
| Wave 1B Kontakt translation ✅ #144 | StepMod and GroupDynamics (0x4A) translated, ni-file hardening | `format/kontakt`, `vendor/ni-file` | B P2-4 closed |
| Wave 2B host dispatch ✅ #152 | Host Registry dispatch, Conflux fast snapshots, block resize, Report UX | `app/host` | — |
| Wave 2C Conflux Native UI ✅ #151 | Kontakt 8 `native_ui` pages at parity (v3 task P8.4b, as `app/face/nui`), wheel step, wavetable routes, Kontakt DSP parity | `app/face`, `script/lua`, `format/kontakt` | — |
| App fixes and features ✅ | #160 Conflux double-open and silent Analog Strings; #163 automatic library discovery and scan diagnostics; #166 classic panner, wheels, master volume; #174 persistent event-driven library index with relocation; #175 preset browser redesign; #179 About, settings and theming, standalone app, Kontakto retired | `app/*` | — |

What is really left, in order: L4's 52 unproved Kontakt laws, the Phase 3
rack → `AudioGraph` move, UVI mono-to-stereo pan laws, and the Kontakt program-level
Debug reflection. The v3 board (TASKS.md) has no open task: each one there is
`landed` or `retired` in tasks.toml.

Consolidating `EffectKind` into families: VISION and the KISS caveat keep a
separate DSP recurrence wherever a measurement shows it is audible. The null
test per effect is done ([report](effect-family-null-report.md), #155); only
the pairs it shows as bit-exact or conditional may merge. `rkyv` waits until a cached load is measured too slow.

## Fundamentals audit: what is still open

Gemini's two core audits from the same day, kept verbatim:
[fundamentals-audit-a-2026-11-22.md](fundamentals-audit-a-2026-11-22.md)
(envelopes, playback, voice lifecycle) and
[fundamentals-audit-b-2026-11-22.md](fundamentals-audit-b-2026-11-22.md)
(modulation, gain/pan, silent drops). #94 fixed most of A, #95 fixed B P0-1…P0-8,
P1-1, P1-2, P1-4 and P1-6, and #97 fixed B P2-1 and the 256-group cap in P2-3.
These items are still open on main:

| Item | What | Evidence on main |
|---|---|---|
| A P1.9 | Aliasing on upward pitch shift, sample-rate-dependent `GAIN_SMOOTH`, `HEAD_WAIT` stall | ✅ #123 — `HEAD_WAIT` deleted (0-frame live stall), `GAIN_SMOOTH_MS = 1.0` derived from sample rate, 4-lobe Kaiser-windowed sinc (`step > 1.0`, >50 dB alias suppression) |
| B P0-9 | ✅ (#120) `fx_idx_amp_split_point`: group inserts run on their true side of the amp envelope | `amp_split: u8` on `ir::Group`/`plan::Group`, `Voice::render_through_stages`, `Kind::Matrix` around drives, `group_racks::split`; `ignored("group.fx_idx_amp_split_point")` uses: 1 → 0 (verified on ANALOG STRINGS `amp_split = 6`) |
| B P1-5 | Several lagged routes on volume or pitch collapse into one smoother | ✅ #128: one lag per route (`LAGS = 4` per voice, later ones folded in), live `LfoParam::Rate` routes integrated into the warp |
| B P1-7 | Block-rate modulation stepping (`MAX_SPAN = 256` frames) | ✅ #123 — `MAX_SPAN` lowered 256 → 64 (750 Hz / 1.33 ms at 48 kHz; `Dsp::Wet` chunks > `wet.len()`) |
| B P1-8 | Pan law is linear balance for every format | ✅ #128: voices balance by `Output.balance` (Kontakt linear, UVI cosine); UVI's mono-to-stereo laws are still open |
| B P2-2 | Kontakt vendor leaks in ir and plan | this plan's Phases 0–1 |
| B P2-3 | Per-note vs per-channel separation for pitch bend and CC (MPE) | ✅ #128: `Source::PitchBend` adds the note's own bend (48-semitone wheel) on non-pitch targets |
| B P2-4 | `BParGroupDynamics` (0x4A) is not read | ✅ #144: translated in `format/kontakt/src/translate.rs` |
