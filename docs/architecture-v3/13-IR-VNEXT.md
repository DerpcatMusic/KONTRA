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

## Status on main (cabee674c6, 2026-11-22)

The phase PRs below all merged, but most of them added the new types without
moving callers onto them. The numbers come from `git grep -c` on main.

| Phase | PRs | Landed | Still to do |
|---|---|---|---|
| 0: hygiene, vendor leaks | #102 | `ByteTransform` keystream, `Cow` tables, `ir::validate`, compact regions, typed fast path for zone `ignored_fields` | group `ignored_fields` still formats Debug text; only the `.kontra` loader runs `validate`; the translators and prepare don't |
| 1: one modulation and envelope core | #109, #110 | separate decay/release curves, equal-power and CC crossfades, `lorand`/`hirand`, `PerNoteCc`, `NoteExpr`, `RrScope::Key`, and `ParamAddr`/`ModRoute` declared | UVI `PROBE_GRID` (23 rebuilds per attribute, L2). Done in #128 (L1): every route is a `ModRoute`, `ModKind` is deleted, the `WARPS = 8` cap is lifted, brightness reads only `NoteExpr`, and UVI macros are `Source::Macro` |
| 2: compact zones, SDK, native | #96, #111, #116 | `ZoneMatch`/`ZonePlay` SoA used by `ZoneIndex`, format SDK (DecentSampler translator, every documented effect), `.kontra` native format | `plan.zone_facts` still duplicates every zone (`ZoneFacts`, `loop_pars` per zone); no Areia RAM measurement of the result |
| 3: graph, generators, effect families | #112, #113, #114 | `AudioGraph` with sidechain and multi-stem output, `Generator` (PCM, oscillator, granular), `Engine::set_param(ParamAddr)` | `EffectFamily`/`EffectSpec` are read only by the `.kontra` schema; the engine still runs the 31-variant `EffectKind` |

## Next wave: lanes

Lanes own disjoint paths. Each lane finishes by deleting what it replaces, and
proves the result with scoped tests plus one real-library load (Areia, Conflux
or VWinds).

| Lane | Goal | Owns | Number to move |
|---|---|---|---|
| L1 modulation core | Every translator emits `ModRoute`; plan and engine read it. Delete `ModKind`. Lift the `WARPS = 8` cap. Brightness becomes `NoteExpr`, and UVI macros become `Macro` | `model/ir`, `engine/plan/src/modulation.rs`, `engine/engine/src/{modulators,voice_gain,start}.rs`, the modulation emitters in each `format/*` | `ModKind::` uses: 458 → 0 ✅ #128 |
| L2 UVI parameter names | Replace `PROBE_GRID` with a declared name → `ParamAddr` + law table | `format/uvi/src/program/routing.rs` | ✅ #123 — rebuilds per attribute at load: 23 → 0 (`PROBE_GRID` & `values::probing` deleted; verified on VWinds Bb Clarinet) |
| L3 one zone table | Drop the `zone_facts` copy; scripts read zones from the SoA tables; `loop_pars` only for user zones | `engine/plan/src/zones.rs`, the zone parts of `engine/prepare`, `engine/engine/src/zone_commands.rs` | ✅ #123 — Areia (49,152 saved zones): ~8.11 MiB & 55,380+ heap allocs → 2.93 MiB (`3,076,368` B) & 0 per-zone allocs (`ZoneFacts`/`zone_facts` deleted; 0 B `user_zones`) |
| L4 Kontakt parity | Continue the #115 audit: each translator law is checked against the oracle or the binary, not assumed | `format/kontakt` (not its modulation emitter, which L1 owns), `vendor/ni-file` | translator laws with no evidence → 0 |

Deferred: consolidating `EffectKind` into families. VISION and the KISS caveat
keep a separate DSP recurrence wherever a measurement shows it is audible, so
this needs a null test per effect first. `rkyv` waits until a cached load is measured too slow.

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
| B P2-4 | `BParGroupDynamics` (0x4A) is not read | parsed in ni-file, not translated |
