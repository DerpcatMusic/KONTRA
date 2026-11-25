---
status: in-progress
project: kontra
owner: agent (tables lane)
created: 2026-11-24
updated: 2026-11-25
proof_scope: internal
evidence_freshness_days: 180
---

# Kontakt mapping tables

## Problem & Goal

- **Problem:** Kontakt effect identity and dispatch live in several places
  (audit `KT-16`/`KT-17`, kontakt-translation-audit-2026-11-24.md): a
  `Meta` table plus a 230-line `match effect.id` in `effects.rs`, a second
  per-id dispatch in `drive::effect` that re-parsed six modelled modules
  (Skreamer, Tape, DStortion, Supercharger GT, Bite, Freak) the `Module`
  tables of #155 already decode. The program-level `ignored_fields` still
  reflected `{:#?}` text (13-IR-VNEXT Phase 0).
- **Goal:** 06-FORMATS.md §2: one row per vendor module (id → name, script
  type, mix, map), modelled modules built by their `Module`; no second
  dispatch; typed program fields. Behaviour unchanged.
- **Plan reference:** 06-FORMATS.md §2–3; 13-IR-VNEXT.md Phase 0.

## Primary-Source & Version Proof

N/A — internal refactor: no external API; the vendor layouts are unchanged.

## Design

`effects.rs` `MODULES: &[Fx]`, one row per `BParFX*` chunk id:
`fx(id, name, script_type, parallel, Map)` or, for a modelled module,
`model(&MODULE, name, parallel, read)` whose script type is the module's.
`Map::Read(fn(&mut Slot) -> Option<EffectKind>)` reads a hand-mapped record;
`Map::Model(fn(&mut Slot) -> Option<KnobModel>)` reads the knob positions and
the module's `build` makes the effect (the same function a script's live
move runs). `Slot` carries the record, the diagnostics and the slot's
knobs/model outputs; `Slot::lost` is the one bypass-aware "not carried"
report. Slot output/dry laws (legacy EQ, Wow/Flutter mix, Supercharger dry)
and rack emission are untouched (the AudioGraph lane owns racks).

## Acceptance criteria

1. `match effect.id` and `drive::effect` are gone; `arch_guards.rs`
   `architecture_metric_ratchets_hold` §4 fails if either returns.
2. `translate.rs` production code has no `.ignored_fields(` call; §5 of the
   same ratchet. `program_ignored_fields_fast_path_matches_debug_reflection`
   proves the typed path equals the reflection.
3. IR identical: `tools/migrations/kontakt-ir-dump.sh` (full `{:#?}` IR plus
   every law application) on Areia, Conflux and ANALOG STRINGS before and
   after, `diff -r` empty.
4. `matari fast kontra` green.

## Measured (production lines, before `mod tests`)

| | before (21a6979) | after |
|---|---|---|
| `format/kontakt/src` | 10,764 | 10,768 |
| `effects.rs` | 878 | 924 |
| `drive.rs` | 226 | 157 |
| `script_knobs.rs` | 659 | 648 |
| `translate.rs` | 1,357 | 1,395 |
| `format/kontakt-effects/src` | 3,464 | 3,464 |

The move is size-neutral; it removes a dispatch, not lines. The ~8k target
needs the follow-ups below.

Measured split (`tools/migrations/kontakt-sizes.sh`; the cfg(test) rigs
audit.rs and fit.rs are not production): on main after #191 and #196
(b19c21ca37), readers 2,711 and mapping 7,904 in `format/kontakt`, plus
3,464 in `kontakt-effects`: 11,368 mapping against ~8k. With #203 (display readouts) merged in: mapping 7,957, total 11,421.

| Step | Deletes | mapping after |
|---|---|---|
| KT-18 knob rows (script_knobs.tsv and its string parser become typed rows; `tools/migrations/script-knobs-table.py`) | TSV 107 lines, parser 35 net .rs lines | 11,333 |
| KT-17 held rows and single parses (109 of 140 held_fields.tsv rows named controls `engine_pars` binds to a processor, never read; `tools/migrations/held-dead-rows.py`; Wow/Flutter and Raum MIX and Supercharger GT dry read once by their rows; `supergt::mix_dry` deleted) | 109 TSV rows; .rs net 0 | 11,333 (11,524 on main e4dbdc6625 with #203 and #208 merged in) |

## Verification Matrix

| Check | Tier | Target / OS | Exact Command | Pass Criterion | Status / Observed Evidence |
|---|---|---|---|---|---|
| Spec proof gate | Doc | any | `python3 tools/check-spec-proof.py plugins/kontra/docs/specs/kontakt-mapping-tables.md` | Exit 0, zero proof errors | PASS (2026-11-25, linux-x86_64) |
| Fast unit/lib tests | Compile/ABI | linux-x86_64 | `matari fast kontra` | Exit 0, zero failures | PASS (2026-11-24, linux-x86_64) |
| IR identical on real libraries | Validator/Runtime | linux-x86_64 | `plugins/kontra/tools/migrations/kontakt-ir-dump.sh` before/after, `diff -r` | Empty diff (held-row removals only for KT-17) | PASS (2026-11-24, linux-x86_64) |
| Full PR verification gate | Validator/Runtime | linux-x86_64 | `matari check origin/main --project kontra` | CI `verify` green | PENDING |

## Unverified Boundaries & Promotion Gate

None — all verification tiers execute on the current host.

## Non-goals (this PR) and next steps

- Behaviour changes (`KT-05` Replika, `KT-06` Compressor law, `KT-07` Chorus
  and Flanger depth) land as separate commits/PRs so this proof holds.
- `KT-17` held_fields.tsv / held.rs dedupe, `KT-18` engine-par/readout
  table, `KT-19` shared laws into `kontra-law`, and `map_mods!`/`map_zone!`
  for modulators and zone fields follow.
