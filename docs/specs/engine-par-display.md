---
status: in-progress
project: kontra
owner: agent (KSP engine-parameter lane)
created: 2026-11-25
updated: 2026-11-25
proof_scope: internal
evidence_freshness_days: 180
---

# `$ENGINE_PAR_*` display text from Kontakt

## Problem & Goal

- **Problem:** `get_engine_par_disp` answered nothing for most module
  controls, because their display text lives in each module's own display
  code in Kontakt, which has not been ported (`docs/re/engine/k-ksp-engine-params.md`).
  Libraries that show those values got empty or unanswered text. In the parity
  census, Conflux snapshots read Raum, Replika, Psyche Delay and Choral knobs
  through `get_engine_par_disp_ext`.
- **Goal:** every `$ENGINE_PAR_*` text that KONTRA shows is the text Kontakt 8.13.1
  printed, checked row by row against an oracle measurement. A name counts as
  `tested` in `docs/ksp/MANUAL-COVERAGE.md` only when that check covers it.
- **Plan reference:** 13-IR-VNEXT (KSP parity, scripting first).

## Primary-Source & Version Proof

N/A — internal refactor: the evidence is Kontakt 8.13.1 itself, run under the Wine
oracle (`tools/kontakt-oracle/disp_sweep.py`), plus the KSP manual's parameter names.

## Design

- `tools/kontakt-oracle/disp_sweep.py` loads a fresh unit of each module type
  into an empty instrument's insert slots, from a timer listener inside a
  render session. With no audio, the listener never fires. The script then sets
  every parameter to fixed points (or `--points N` even points) and saves
  `get_engine_par_disp` with `save_array_str`. The rows go to
  `docs/re/kontakt-bindings/engine-par-disp.tsv` (name, value, text).
- `readouts.rs` gives each measured name a law, a print form and end texts.
  A module control with no processor is held by its slot (`engine_pars`
  derives an `Opaque` row for it), so it still shows its text.
- Raum's predelay and high-cut laws move to `kontra-law` as
  `FormulaId::RaumPredelayMs` / `RaumHighCutHz`. The DSP and the readout use
  the same law.
- A readout's line is shown as `lo + (hi − lo)·x` and its curve as
  `e^(ln lo + x·(ln hi − ln lo))`, both in f32, as measured. The laws' own forms
  round differently at ties. The print formatter keeps ties to even: Morphology's
  route intensity 6.25 reads "  6.2" (oracle 2026-10-29). Ring Mod's 31.25 Hz
  reads " 31.3" because the f32 curve gives 31.250002, not because ties round
  away from zero. No value has an end text for rounding.

## Acceptance criteria

1. `kontra-host` `every_measured_engine_par_text_reads_as_kontakt_shows_it`
   shows every row of the table, except the names listed on its `# pending:`
   line, exactly as Kontakt printed it. It fails as soon as a pending name reads
   right, so the list only shrinks.
2. `tools/ksp-manual-coverage.py` counts a measured, non-pending name as `tested`.
3. Raum's reverb is unchanged (same f32 arithmetic; `kt03_raum_laws_*` passes).

## Measured

| | tested | done | missing |
|---|---:|---:|---:|
| `$ENGINE_PAR_*` before (main 22f3e8ce99, with corpus) | 48 | 319 | 300 |
| after #203 | 115 | 263 | 289 |
| after the amp/delay/ring batch (main 2597cd0d20) | 196 | 263 | 208 |
| after the menu, multiple and rate forms (main fa56d6b5b2) | 197 → 200 | 263 → 260 | 207 |

## Verification Matrix

| Check | Tier | Target / OS | Exact Command | Pass Criterion | Status / Observed Evidence |
|---|---|---|---|---|---|
| Spec proof gate | Doc | any | `python3 tools/check-spec-proof.py plugins/kontra/docs/specs/engine-par-display.md` | Exit 0 | PASS (2026-11-25, linux-x86_64) |
| Oracle measurement | Validator/Runtime | linux-x86_64 (Wine, Kontakt 8.13.1) | `tools/kontakt-oracle/disp_sweep.py` | 77 names, 1156 rows; two runs agree at shared points. Batch 2: 98 names, 980 rows (Bass Pro, Dirt, Kolor, V5, SF100, Twin Delay, Ring Mod) | PASS (2026-11-25, linux-x86_64) |
| Display test | Compile/ABI | linux-x86_64 | `matari fast kontra -p kontra-host -- -E 'test(every_measured_engine_par_text)'` | Every non-pending row equal | PASS (2026-11-25, linux-x86_64) |
| Scoped package tests | Compile/ABI | linux-x86_64 | `matari fast kontra -p kontra-law / kontra-plan / kontra-format-kontakt / kontra-host`, `-p kontra-engine -- -E 'binary(arch_guards)'` | Exit 0 | PASS (2026-11-25, linux-x86_64) |
| Full PR verification gate | Validator/Runtime | linux-x86_64 | CI `verify` | green | PENDING |

## Unverified Boundaries & Promotion Gate

None — all verification tiers execute on the current host.

## Non-goals and next steps

- The pending names: Raum's decay and rate laws and Choral's rate law, which need
  the module display code or a fitted closed form. The 201-point Raum sweep is in
  the table. Also menu names (Replika quality and BBD type), Choral delay's "x"
  prefix, Psyche Delay's stepped pitch and its detune rounding.
- Settled by 201-point runs (2026-11-25): Replika quality and BBD type are
  menus (entry `floor(4x)`), Choral delay is "x " and `0.1 + 5x`, and Ring Mod's
  LFO rate is `exp(1/32, 32)` with two decimals under 1. A curve readout is
  `e^(ln lo + x·(ln hi − ln lo))` in f64 on f32 logs: 1/32..32 Hz at 0.2 reads
  0.13, which pure f32 rounds to 0.12.
- Still pending, each needing the module's display code from the binary and not a
  fit: Psyche Delay's pitch (25 steps, but the edges at 0.300 and 0.520 fit no
  single rounding of one line), Twin Delay's time (`(1 − x)² + 1999x²` misses
  109 of 201), Bass Pro's gain (`(e^(kx) − 1)/(e^k − 1)`, k ≈ 3.228, misses 1 of
  201; the binary reads a table), and its master, mid and GEQ curves.
- Batch 2's pending names (before the 201-point runs): Bass Pro's gain, master, mid, GEQ bands and GEQ
  volume (curves: gain reads like 20·log10((e^(kx) − 1)/(e^k − 1)) with k
  near 3.23, not pinned by one-decimal text), Twin Delay's times (near
  2000·x², off at the low end) and Ring Mod's LFO rate (two decimals below 1,
  one above: no form yet).
- V5 and SF100 had no owner family, so their controls were not held and read
  nothing. They are now held by `$EFFECT_TYPE_VAN51` / `$EFFECT_TYPE_SUPERFAST100`.
- The manual and `crates/script/ksp/data/vendor_constants.txt` spell
  `$ENGINE_PAR_SF100_OVEDRIVE`. Kontakt 8.13.1's binary has only `…_OVERDRIVE`
  (the only other manual name it lacks is `$ENGINE_PAR_HP_FREQ`). That is for the KSP
  language lane.
- The other modules' parameters (Bite, Freak, Flair, Phasis, Bass Invader,
  Super GT, the guitar pedals and others): the same sweep, one PR per batch.
- DSP for the held-only controls (Jump, Replika held, Raum freeze/sparse) is not
  part of this lane.
