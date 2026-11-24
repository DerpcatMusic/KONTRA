---
status: in-progress
project: kontra
owner: agent (tables lane)
created: 2026-11-24
updated: 2026-11-24
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

## Non-goals (this PR) and next steps

- Behaviour changes (`KT-05` Replika, `KT-06` Compressor law, `KT-07` Chorus
  and Flanger depth) land as separate commits/PRs so this proof holds.
- `KT-17` held_fields.tsv / held.rs dedupe, `KT-18` engine-par/readout
  table, `KT-19` shared laws into `kontra-law`, and `map_mods!`/`map_zone!`
  for modulators and zone fields follow.
