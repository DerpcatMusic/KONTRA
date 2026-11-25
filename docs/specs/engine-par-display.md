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
| after the display-code laws and the module batch (main 63fcf6ef62, fresh `ksp_status`) | 364 | 140 | 163 |
| after Freak frequency and the Rotator mix and speed readouts (fresh `ksp_status`) | 367 | 137 | 163 |

## Verification Matrix

| Check | Tier | Target / OS | Exact Command | Pass Criterion | Status / Observed Evidence |
|---|---|---|---|---|---|
| Spec proof gate | Doc | any | `python3 tools/check-spec-proof.py plugins/kontra/docs/specs/engine-par-display.md` | Exit 0 | PASS (2026-11-25, linux-x86_64) |
| Oracle measurement | Validator/Runtime | linux-x86_64 (Wine, Kontakt 8.13.1) | `tools/kontakt-oracle/disp_sweep.py` | 77 names, 1156 rows; two runs agree at shared points. Batch 2: 98 names, 980 rows (Bass Pro, Dirt, Kolor, V5, SF100, Twin Delay, Ring Mod). Batch 3: 150 names (Bite, Freak, Flair, Phasis, Bass Invader, Super GT, the pedals, Rotator, Transient Master, Vibrato Chorus), 201-point runs of 11 names and a 199-step Psyche detune run | PASS (2026-11-25, linux-x86_64) |
| Display test | Compile/ABI | linux-x86_64 | `matari fast kontra -p kontra-host -- -E 'test(every_measured_engine_par_text)'` | Every non-pending row equal | PASS (2026-11-25, linux-x86_64) |
| Display code replay | Validator/Runtime | linux-x86_64 | each formatter re-run in f32 numpy from its decompiled operations and constants (`BuildScratch/oracle-probe/simdisp.py`) against the table (local scratch; the display test is the committed proof) | 0 differences over 20 names, 3,600 points | PASS (2026-11-25, linux-x86_64) |
| Scoped package tests | Compile/ABI | linux-x86_64 | `matari fast kontra -p kontra-law / kontra-plan / kontra-format-kontakt / kontra-host`, `-p kontra-engine -- -E 'binary(arch_guards)'` | Exit 0 | PASS (2026-11-25, linux-x86_64) |
| Full PR verification gate | Validator/Runtime | linux-x86_64 | CI `verify` | green | PENDING |

## Unverified Boundaries & Promotion Gate

None — all verification tiers execute on the current host.

## Non-goals and next steps

- Read from the display code (2026-11-25). Each module parameter has a descriptor
  in the reference (0x40 bytes from `0x149e58388`, by internal id: default, name,
  unit, formatter). The formatters these names call are encoded as
  `kontra_law::FormulaId` in their f32 operation order:
  - Bass Pro gain and master: Kontakt's exp table (`2^((i − 1200)/60)`,
    interpolated) at `a·x + b`, less its value at 0, scaled to 1 at 1, as dB. A
    level under 1e-5 prints `-inf`, and so does any readout law that returns −∞.
  - Bass Pro mid, the GEQ bands and GEQ volume: cubics and a quadratic.
  - Psyche Delay detune (`101x − 50`) and pitch (`25x − 12`): rounded half away
    from zero below 0, truncated above, clamped. This is why the 0.005 grid fitted
    two roundings and the off-grid run fitted neither.
  - Raum decay and rate, Choral rate (`7.4x³ + 0.25`) and Twin Delay time
    (`2002x² − 3x + 1`).
  - Bite rate, Freak carrier and band-pass: the exp table at `a·x + b`. A pure exp
    misses one display digit at about 5 of 201 points.
  - Bite bits: `2 − trunc(−15x)`, clamped to 2..16.
  - Flair rate: `440·2^(11.5x − 16.25)` through a quartic exp2.
  - Freak frequency (`0x141965690`, mode 1): `f·range·f·f` with `f = 2x − 1`. The
    range is 5000 when wide range is on (the fresh unit) and 200 otherwise; only the
    5000 form is encoded, since only that one is measured.
  - Rotator mix: `0x1407b9fa0` stores `clamp(x³)`, and `0x1407ba280` prints `-inf`
    under 1e-9, else `ln v · 6/ln 2` as `%3.1f`.
  - Rotator speed (`0x1407bb180`): "fast" iff `x > 0.5`. A menu now picks
    `floor(n·v)` on the law's value, so this is a formula under `Menu`; the line
    menus read the same.
  - The shared formatter `0x1407baa90` (Freak frequency, Phasis rate and center)
    prints its argument as Padded with no law of its own, and its k form tests
    `|v| ≥ 1000`. KONTRA's Padded now masks the sign too.
- Phasis rate and center (now encoded): with a value given, `get_engine_par_disp`'s
  `0x1408a6400` sends ids 0x16f/0x170 of a type-0x24 module through
  `0x140a75560` / `0x140a752e0` before the shared formatter. Both are a quartic
  exp2 times 440: rate `2^((1.6667 + 10)·x − 15.75)` (note −120..20; Ultra, flag
  +0x2c, `(9.25 + 10)·x`, to 111; synced, flag +0x40, the value as given), centre
  note `30 + 90·x` (Ultra −30..190), `·1/12 − 69/12`. These are the ranges
  phasis.rs already plays. A fresh unit is Ultra off, so only that form is
  encoded; the 201-point sweep reads it to the printed digit.
- Still pending:
  - Phasis notches (`0x1407baee0`: `2 − trunc(−11·clamp(x))`, 2..12) and Flair
    pitch (`0x140adfb60`: `i = trunc(97x)`, 0..96, note `i % 12`, octave
    `i/12 − 2`) read their floor (2, C-2) at every set value. Both descriptors are
    identity and their defaults are 1.0, so the value handed to the formatter is 0,
    not the set or default value. It is not a read-before-set artefact: read one
    200 ms audio tick after each set (the same 10 points), both still show 2 and
    C-2, while Phasis rate in the same run reads as in the direct sweep.
    Read so far, and not the cause: both descriptors (0x174, 0x17b) have identity
    w2/w3 and no bipolar flag (+0x38); the name map `0x140780c80` gives Phasis
    0x16f..0x177 and Flair 0x17a..0x17c in order; and `0x1408a6400`'s generic
    path (`0x1408a6c4a` → `0x140514930` mode 2 → w3 → w4 at `0x1408a7250`)
    hands the formatter `xmm0 = x`, as it does for the knobs that read right.
    Only their unit word (`0x14470787b`) sets them apart. The probe of
    `get_engine_par` after each set (same 10 points, one tick later) settles it:
    Flair pitch reads 0 and Phasis notches 2 at every set value, the fresh unit's
    stored values (notches unscaled), while rate and centre read back what was
    set. So `set_engine_par` leaves these two unchanged, and the display prints
    the stored value. Why the setter skips them is not read yet; KONTRA should not
    copy it until it is.
- V5 and SF100 had no owner family, so their controls were not held and read
  nothing. They are now held by `$EFFECT_TYPE_VAN51` / `$EFFECT_TYPE_SUPERFAST100`.
- The manual and `crates/script/ksp/data/vendor_constants.txt` spell
  `$ENGINE_PAR_SF100_OVEDRIVE`. Kontakt 8.13.1's binary has only `…_OVERDRIVE`
  (the only other manual name it lacks is `$ENGINE_PAR_HP_FREQ`). That is for the KSP
  language lane.
- These laws drive text only. Whether a held module's DSP should use them is
  for the module's own lane.
