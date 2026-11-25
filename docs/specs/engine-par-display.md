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
  in the reference (0x40 bytes from [ref], by internal id: default, name,
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
  - Freak frequency ([ref], mode 1): `f·range·f·f` with `f = 2x − 1`. The
    range is 5000 when wide range is on (the fresh unit) and 200 otherwise; only the
    5000 form is encoded, since only that one is measured.
  - Rotator mix: [ref] stores `clamp(x³)`, and [ref] prints `-inf`
    under 1e-9, else `ln v · 6/ln 2` as `%3.1f`.
  - Rotator speed: "fast" iff `x > 0.5`. A menu now picks
    `floor(n·v)` on the law's value, so this is a formula under `Menu`; the line
    menus read the same.
  - The shared formatter [ref] (Freak frequency, Phasis rate and center)
    prints its argument as Padded with no law of its own, and its k form tests
    `|v| ≥ 1000`. KONTRA's Padded now masks the sign too.
- Phasis rate and center (now encoded): with a value given, `get_engine_par_disp`'s
  [ref] sends ids 0x16f/0x170 of a type-0x24 module through
  [ref] before the shared formatter. Both are a quartic
  exp2 times 440: rate `2^((1.6667 + 10)·x − 15.75)` (note −120..20; Ultra, flag
  +0x2c, `(9.25 + 10)·x`, to 111; synced, flag +0x40, the value as given), centre
  note `30 + 90·x` (Ultra −30..190), `·1/12 − 69/12`. These are the ranges
  phasis.rs already plays. A fresh unit is Ultra off, so only that form is
  encoded; the 201-point sweep reads it to the printed digit.
- Bus Compressor (2026-11-25, 101 points): ratio, attack and release are six-entry
  menus picked by `floor(6x)` (1.5 2 3 4 5 10; 0.1 0.3 1 3 10 30; 100 200 400 800
  1600 Auto). Threshold and makeup go through a parameter object ([ref]
  writes x, [ref] returns `lo + (hi − lo)·x` in f64, or log10 between them
  when its flag is set); [ref] fills threshold −20..20 and makeup −5..20,
  both linear. The f64 step on an f32 x is why makeup prints "1.00" at .24.
- Solid G-EQ (2026-11-25, from code): frequencies and Qs go through the same object
  kind ([ref], base [ref]), which [ref] fills: HF 1500..22000,
  HMF 600..7000, LMF 199.96..2500 and LF 40.02..600, all log; HMF and LMF Q
  0.7..2.5, linear (ends as f32 widened to f64). Printed by the `k` formatter
  [ref] (`%5.1fk` at 1000 and up, `%5.1f` at 1 and up, else `%5.2f`).
- Filter (from code): Gain `60·x − 30`, Shift `80·x`
 and Type `x`, all `%3.1f`. EQ bandwidth
  `0.33333334 + 2.6666667·clamp(x)` is printed through [ref].
  Bypass, resonance and Formant talk/sharp/size are percents. GAIN and BANDWIDTH on
  a one-band EQ read empty, so the reference has no such control there.
- Batch 4 (2026-11-25): names read with `get_engine_par_disp_ext(name, x, -1, -1, -1)`
  and no module (`disp_sweep.py EXT`), as the KONTRA test reads them.
  - Formatting needs no unit, so the IRC controls read directly: sizes 50..150 % in
    one decimal, cut-offs as the knob laws.
  - The `--async` sweep (one set a 200 ms tick, read on the next) agrees wherever
    the value had landed. The applied value often lags more than a tick, and
    `on async_complete` fired 2 times for 60 sets, so that route is not a sweep.
  - Flair pitch is `trunc(97x)` clamped to 0..96 and printed as a note name, C-2 at
    0. Phasis notches are `2 − trunc(−11·clamp(x))`.
  - Setting them (in range, `--values`) reads the same texts. Their earlier floors
    were the setter refusing out-of-range values.
  - Attack curve truncates: `(2x − 1)` in f32,
    `·100`, then `%d`. So 900000 prints 79.
  - An LFO's waveform mixes are bipolar percents. Wavetable phase random is a
    percent.
  - An LFO's delay is the Fade In descriptor: `e^(8.5175934·x +
    ln 2) − 2`. Its constant is ln 5002, not ln 5001, so the top is 10 002 ms, as the
    saved-value probe read it. `common::STAGE_10S` now uses 5002, which also moves
    the fade time by up to 0.02 %.
  - The text is empty for the switches and menus (`INTMOD_BYPASS`,
    `SEND_EFFECT_BYPASS`/`TYPE`, `WT_INHARMONIC_MODE`, `WT_MOD_TYPE`), and for a
    one-band EQ's `GAIN` and `BANDWIDTH` (`FILTER/EQ1BAND`).
- Missing names, batch 1 (2026-11-25, `EXT`): Beat Masher, Beat Slicer, Reverse Grain
  (`RG_`, now owned by `$EFFECT_TYPE_REVGRAIN`), Transpose Stretch (`TS_`, now owned by
  `$EFFECT_TYPE_TRANS_STRETCH`) and Stereo Tune. The laws, from their formatters:
  - Stereo Tune: split `(9x + 1)·100` and drift `0.99·100·x + 1`
.
  - Transpose Stretch key: `120x − 60` below 0.5 and `24·(x − 0.5)` from it, `%2.2f`
.
  - Beat Masher gate: `(x − 0.5)·200`.
  - The rest are percents or empty switches and menus. These modules have no
    processor, so a script's set is held and shown only.
- Missing names, batch 2 (2026-11-25, `EXT`):
  - New owners: Cabinet type, EP Preamps, Cat (`CT_`), Chainsaw (`CS_`), Lo-Fi's
    noise level and colour, Distortion's type and Shaper's type.
  - Distortion's and Shaper's types are provisional owners. The manual lists
    `$ENGINE_PAR_DISTORTION_TYPE` and `$ENGINE_PAR_SHAPE_TYPE` only in its 5.7.0
    new-features notes, under no effect. Change them if the oracle shows another
    module takes them.
  - The EP tremolo rate is `19.99·x² + 0.01` Hz.
  - Lo-Fi's noise is `ln(clamp(x)³)·8.65617` dB (`18·log2 x`; [ref], then
    [ref]), and reads "-inf" below 1e−9.
  - Cabinet treble/bass and Chainsaw low/high are bipolar percents. The rest are
    percents, or empty switches and menus.
- Missing names, batch 3 (2026-11-25, `EXT`), the remaining effects:
  - Transparent Limiter: threshold `40x − 40`, ceiling `39.7x − 40`, release `500^x`.
  - Surround Panner: LFE `72x − 36` dB, size `0.9x + 0.1`, azimuth −180..180.
  - Plate Reverb ([ref], one case per control): predelay `250x`, decay
    `6400x + 556`, low shelf `5.625 − 34.375x`, damping `10851 − 4851x`.
  - Solid G-EQ: HP `10^(1.30103x + 1.4771212)`, LP `10^(4.69897 − 1.2218485x)`.
  - The simple filter's LP/HP cut-offs read the exponential table Freak uses, at
    `799.085x + 1281.88`.
  - The rest are percents, or empty switches and menus.
  - New owners, from the manual's sections: `TRANSLIM_`, `SP_`, `PR_`,
    `EXP_FILTER_` (AET Filter), `PHASE_INVERT`/`LR_SWAP` (Inverter) and
    `STEREO_PSEUDO` (Stereo). The ladder HQ switch, the simple filter's cut-offs and
    the S1200 mode join the uncarried filter controls.
- V5 and SF100 had no owner family, so their controls were not held and read
  nothing. They are now held by `$EFFECT_TYPE_VAN51` / `$EFFECT_TYPE_SUPERFAST100`.
- The manual and `crates/script/ksp/data/vendor_constants.txt` spell
  `$ENGINE_PAR_SF100_OVEDRIVE`. Kontakt 8.13.1's binary has only `…_OVERDRIVE`
  (the only other manual name it lacks is `$ENGINE_PAR_HP_FREQ`). That is for the KSP
  language lane.
- DSP gap (after scripting and UI): Reverb2's modulation (`$ENGINE_PAR_RV2_MOD`) has
  its text, so a script's set is held and shown, but the reverb plays the saved
  modulation only; a set does not move it.
- These laws drive text only. Whether a held module's DSP should use them is
  for the module's own lane.
- Missing names, group class C (2026-11-25): the group's start criteria,
  `VOICE_GROUP` and `POST_FX_SLOT`, measured with `disp_sweep.py` `GET/<g>/<c>/-1` and
  `AT/...` against groups that Kontakt's own `set_group_start_options` and
  `set_voice_groups` configured (`--setup`, probes/start_criteria_pars.lua, saved as
  `tests/data/start-criteria.nki`):
  - A missing or wrong-kind modulator index and a missing group read 0, with
    empty text, and the script runs on. This is the existing `Engine::absent` path.
  - Start criteria are addressed (group, criterion 0..=3, −1), are read only (a
    set is dropped, as the manual's "only work with get_engine_par()" says), and read
    the saved fields as stored: mode 0 none, 1 key, 2 controller, 3 round robin,
    4 random; next 0 and, 1 and not, 2 or; cycle class the round-robin position. A
    criterion never set reads mode 0, keys 24..24, controller 1, 0..64, zone and
    slice −1. Index 4 read `1065353216` (float 1.0's bits) in the reference: a
    read past its four criteria, not reproduced; KONTRA reads 0 there.
  - `VOICE_GROUP` reads the saved voice-group index (−1 none) and keeps any set,
    even past the instrument's voice groups. It is held, not driven: the IR's
    voice groups are only the ones groups name at load, so a set to another one has
    nothing to point at.
  - `POST_FX_SLOT` reads 0 and drops every set, with or without group effects
    loaded, at every address tried.
  - KONTRA keeps these in `ir::Group::held` (`EnginePar::GroupHeld`), separate from
    the effect-slot store. None of them shows a text.
  - The set → read rows (values −5 to 2³¹−1, sweeps/c-set.tsv) are the `Group` rows of
    `docs/re/kontakt-bindings/engine_par_clamp.tsv`. `slot` there is the index, and
    `preset` is what the group held before the set. kontra-host's
    `a_groups_controls_take_sets_as_kontakt_takes_them` checks each row.
