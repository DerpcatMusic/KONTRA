---
status: in-progress
project: plugins/kontra
owner: claude-opus-5-5
created: 2026-11-24
updated: 2026-11-24
proof_scope: internal
evidence_freshness_days: 180
---

# UVI effects map through one table each

## Problem & Goal

- **Problem:** [06-FORMATS.md](../architecture-v3/06-FORMATS.md) §2 wants
  one declarative table line per vendor effect, and §3 targets `format/uvi`
  at readers plus ~6k lines of mapping. #155 declared a `UviModule` per
  script-addressable UVI insert (13 of them), but it added them beside the
  old code. Measured on 21a6979b2 (and unchanged on b19c21ca3 but for one line), each of the nine effects below is still
  mapped twice:
  - a hand-written load builder (`dual_delay`, `spark_verb`, `flanger`,
    `phasor`, `wave_shaper`, `diode_clipper`, `drive`, `shelves`,
    `maximizer`) reads the XML;
  - `UviModule::build_effect` rebuilds the same effect when a script moves a
    knob.

  On top of that, `decode_model` lists every attribute name and default a
  third time. `format/uvi/src` has 8,456 production lines on b19c21ca3 (6,965 not blank
  or comment), 1,311 of them readers (`bank.rs`, `keys.rs`, `lib.rs`).
- **Goal:** each of those effects is declared once, as its `UviModule`. Its
  `attrs` are `Attr` rows (name, default, flag, scripted), and the load path,
  script knobs and moved-parameter tables all build through it. The
  duplicates are deleted by a committed codemod, a ratchet keeps them
  deleted, and the translated IR does not change.
- **Plan Reference:** [06-FORMATS.md](../architecture-v3/06-FORMATS.md)
  §2–§3, `format/uvi` row.

## Primary-Source & Version Proof

N/A — internal refactor: the translator's output must not change; no vendor
format, API or law is reinterpreted.

## Invariants & Non-Goals

### Hard Invariants
- **Same IR:** every program's `ir::Instrument` (`{:#?}`) is byte-identical
  before and after on the installed banks: VWinds Bb Clarinet and C Flute
  (full text), and Augmented Orchestra (per-program hash). AO is the one
  that holds the DualDelay and SparkVerb corpus settings.
- **Same diagnostics:** each builder's load-only report moves into its
  module's `report` function with the same text, condition and order.
- **Audio thread:** `build_effect` is unchanged, and still does not allocate.
- **Ownership:** rack, insert and send emission (`Translator::lay`,
  `slot`, `voice_slots`) keep their structure. Another lane owns the move to
  `AudioGraph`.

### Non-Goals
- `map_mods!` / `map_zone!` tables for modulators and zone fields (§2's
  second half). That is the larger share of the ~6k gap and needs its own
  spec.
- DigitalEq and the bus XpanderFilter. Both keep their own load paths:
  DigitalEq because it spans 16 bands over several slots with GainScale
  routes, XpanderFilter because it shares the keygroup filter's builder and
  provenance. Their parity test (`load vs model`) stays.
- Convolver and SampledReverb, which need the impulse loader at load.

## API / Data / File Contract

- `models::Attr { name, default, flag, scripted }` with `Attr::num(name,
  default)`, `Attr::flag(name)` and `.held()` (no script name).
- `UviModule { id, tag, attrs: &[Attr], report, build_effect }`:
  - `UviModule::saved(node)` gives the saved values in knob order;
  - `UviModule::load(node, notes)` reports, then builds; `Translator::built`
    and `moves` call it.
- The builder files move under `program/models/` (`delay.rs`, `reverb.rs`,
  `sweep.rs`, `shaper.rs`). Each module's static sits where its builder was,
  under the same binary-address notes.

## Tasks

1. Codemod `tools/migrations/uvi-mapping-tables.py`: `attrs` become `Attr`
   rows (defaults taken from `decode_model`), those `decode_model` arms are
   deleted, the builder files are `git mv`'d with their statics, the builder
   fns are deleted, and test call sites are rewritten.
2. Hand-fix the residue the compiler lists: `saved`/`load`/`report`, the
   `built` branch order, `moves` over the saved vector, and imports.
3. Add the ratchet `uvi_effects_load_through_their_module_tables` in
   `engine/engine/tests/arch_guards.rs`: 0 builder fns, and code lines
   (not blank or comment, so RE notes stay free) ≤ the measured 6,613.
4. Proof: `matari fast kontra`, and before/after IR dumps
   (`crates/format/uvi/tests/ir_dump.rs`, env-gated).
5. Update the 06-FORMATS.md §3 size table with the measured numbers.

## Verification Matrix

| Check | Tier | Target / OS | Exact Command | Pass Criterion | Status / Observed Evidence |
|---|---|---|---|---|---|
| Spec proof gate | Doc | any | `python3 tools/check-spec-proof.py plugins/kontra/docs/specs/uvi-mapping-tables.md` | Exit 0, zero proof errors | PASS (2026-11-25, linux-x86_64) |
| UVI tests | Compile/ABI | linux-x86_64 | `matari fast kontra -p kontra-format-uvi` | Exit 0, zero failures | PASS (2026-11-25, linux-x86_64) |
| Ratchet | Compile/ABI | linux-x86_64 | `matari fast kontra -p kontra-engine -- -E 'test(uvi_effects_load_through_their_module_tables)'` | Exit 0 | PASS (2026-11-25, linux-x86_64) |
| VWinds IR identical | Validator/Runtime | linux-x86_64 | `KONTRA_UVI_DUMP=… KONTRA_UVI_DUMP_OUT=… cargo nextest run -p kontra-format-uvi --test ir_dump`, `cmp` before/after | Byte-identical dumps | PASS (2026-11-25, linux-x86_64) |
| AO effect IR | Validator/Runtime | linux-x86_64 | same with `KONTRA_UVI_DUMP_EFFECTS=1 --release`, numeric diff before/after | Only f32-ulp differences | PENDING |
| Full PR verification gate | Validator/Runtime | linux-x86_64 | CI `verify` | Green | PENDING |

## Unverified Boundaries & Promotion Gate

- AO effect dumps: 340 of 620 programs are identical; 280 differ only in
  SparkVerb floats by f32 ulps (worst relative 1.59e-7) and 0 differ
  otherwise. This is parity evidence, not proof: it stays PENDING until the
  cause (reordered f32 arithmetic in the table path) is pinned or the dumps
  match exactly.
