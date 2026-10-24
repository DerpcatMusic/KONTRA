# Supercharger GT (Kontakt 8, class 0x60): transfer, attack, release (2026-10-24)

Verdict: **not measured. No verdict on identity with a KONTRA compressor law, and no mapping added.** The live rig
does not give repeatable Supercharger output: the same setting rendered twice gives different curves, and the
bypass-vs-reference floor is too high for the ±1 dB comparisons this needs. Nothing below is a Kontakt transfer,
attack or release number.

## What is known without a measurement

From the bindings (`docs/re/kontakt-bindings/fx_other.tsv`, `decoder_validation.tsv`):

| Kontakt parameter | saved field | notes |
|---|---|---|
| `SUPERGT_TRIM` | 0 | input/trim, 0..1 |
| `SUPERGT_SATURATION` | 1 | mode 0..2 (validator `0x140789c30`) |
| `SUPERGT_COMPRESS` | 2 | the single compression amount |
| `SUPERGT_CHARACTER` | 3 | mode 0..2 |
| `SUPERGT_MIX` | 4 | |
| `SUPERGT_ATTACK` | 5 | |
| `SUPERGT_RELEASE` | 6 | |
| output gain | 4 (`ENGINE_PAR_INSERT_EFFECT_OUTPUT_GAIN`) | pow, a=16 |

Supercharger GT has **no threshold and no ratio parameter**. The Classic, Enhanced and Pro laws in
`crates/sampler-core/src/dsp/dynamics/laws.rs` take threshold and ratio (the Enhanced law: `[db, ratio, attack, release]`).
So even a perfect match on attack and release could not make them identical: the Supercharger's gain computer is
driven by `COMPRESS`, and its mapping onto threshold and ratio is one of the things that is not recovered
(`dyn-compressors.md`, "CompXT core parameter map": the Supercharger uses the `Rockruepel_CompXT` core; the gain
computer, window coefficients and saved-field→index map are not recovered). This is a parameter-set difference, not a
measurement result.

## What was run

All renders through `oracle-slot`, Kontakt 8 under Wine, insert `Effects/Supercharger GT` from the INIT preset
(`Presets/Effects/Supercharger GT/- INIT Supercharger GT -.nkp`). Specs in this directory:

| spec | rig | steps | raw |
|---|---|---|---|
| `supercharger-gt-levels.spec.json` | `fxrig.py levels` (1 kHz, −40..0 dBFS, 1 s per level, bypassed reference in the session) | COMPRESS 0, .25, .5, .75, 1.0; TRIM .5, SATURATION 0, CHARACTER 0, MIX 1, ATTACK .5, RELEASE .5 | `supercharger-gt-levels.tsv` (run 1), `supercharger-gt-levels-rerun.tsv` (run 2, same spec) |
| `supercharger-gt-burst.spec.json` | `fxrig.py burst` (−12 dBFS tone, then −50 dBFS quiet tone) | COMPRESS 1.0; (ATTACK, RELEASE) = (0,.5), (.5,.5), (1,.5), (.5,0), (.5,1) | `supercharger-gt-burst.tsv` |
| `supercharger-gt-ramp.spec.json` | `stimrender.py` ramp (200 Hz, 0→1 over 0.6 s) | COMPRESS 0, .25, .5, .75, 1.0 | `supercharger-gt-ramp.tsv` (analysed) |
| `supercharger-gt-release.spec.json` | `stimrender.py` release (0.2 s DC at 0.6, then 0.7 s 2 kHz probe at 0.02) | COMPRESS 1.0; (ATTACK, RELEASE) five combinations | `supercharger-gt-release.tsv` (analysed) |

Analysis: `supercharger-gt-analyze.py RAMP.npz RELEASE.npz OUT_DIR`. The ramp gain is the window-peak ratio of the
effect channel to the latency-free reference channel at 0.2..1.0 input (the method of `supercharger-static.tsv`). The
release gain is the 1 ms RMS envelope difference, with attack at 63% of the steady gain and release at 63% recovery.

## Why no number is usable

1. **Not repeatable (levels).** Same spec, two runs. Run 1 and run 2 disagree on almost every cell. Run 1 has active
   segments at −101 to −240 dBFS (digital silence or near it) inside tone segments, e.g. `comp0` at −30 and −18 dBFS
   (−134.6, −133.4), and `comp750` at every level (−117 to −240). Run 2 has the same cells at −23 to −35. A setting
   cannot have a sign-changing transfer from run to run.
2. **Bypass-like settings do not match the reference (levels).** `comp0` should be the reference. Run 2 gives
   −40.6 vs −22.5 at −12 dBFS and −23.2 vs −11.3 at 0 dBFS (errors 11–18 dB). Run 2 `comp1000` is flat at −24 to −29 dB
   for input from −40 to 0 dBFS, which is not a compressor transfer.
3. **Burst is unusable.** Four of five variants are silent for the first ~1 s (gain floor −96.67 dB at 0.5 s and 1.0 s),
   then return to about 0 dB. The attack and release numbers the script derives from that (0 ms, 0 ms) come from the
   silent floor, not from a 63% crossing. The only non-silent variant (`att50_rel100`) shows no reduction at all
   (+0.4, −1.0, +0.2 dB) at COMPRESS 1.0 and −12 dBFS.
4. **Ramp shows no compression.** All five COMPRESS settings are within 0.01 dB of each other at every input
   (`supercharger-gt-ramp.tsv`). The effect's output differs from the bypass by ~1e-3 full scale (max 0.018, at COMPRESS 0.25). The earlier
   INIT-preset vector (`supercharger-static.tsv`, 2026-10-21) shows −0.04 dB at COMPRESS 0.5 and no change at
   COMPRESS 1.0 at 0 dBFS, so the knob has little audible effect in this range.
5. **Reference floor.** With the effect bypassed, the effect channel and the reference channel differ by up to
   0.08 full scale (RMS 0.017). Gain comparisons of ±1 dB therefore sit inside the rig's own error.
6. **Release stimulus, bypass channel.** Even the bypass channel's DC input decays from 0.44 to 0.003 over 190 ms while
   the reference holds 0.46. This is not a settings effect (the bypass row is identical to every step), and the release
   analysis (`steady_dB` −94.7 for bypass) is invalid as a result. Not explained; not checked against the bypass
   path of another effect.
7. **Parameter path.** `fxrig.py` sets parameters at `on init` (verified to work for Compressor per
   `comp-levels-pilot.md`). `stimrender.py` sets them from `on controller` on CC20. For the release and ramp runs the
   steps rendered within 0.02 full scale of the bypass (typically 1e-3). Whether CC20 is applied for this effect was not separately verified.

The 2026-10-21 static vector (`docs/re/engine/vectors/supercharger-static.tsv`) remains the only Supercharger
transfer data, with the same ±0.5 dB limits. The comp-grid and comp-burst methods (threshold/ratio grid, attack/release
at knob values) do not apply directly, since this effect has no threshold or ratio.

## Verdict and what would settle it

- Identity with Classic, Enhanced or Pro: **not decided**. Parameter sets differ (no threshold or ratio here), so the
  KONTRA laws cannot be identical to this effect as they stand. No mapping was added. `dynamics::unproved(0x60)` and the
  `NotModeled` diagnostic stay.
- To settle it, in order: (a) find out why run 1 and run 2 differ. A reproducible rig would first show the same
  output for the same spec. Check the `fxrig` timing and MIDI assignment per instance for this effect, and verify
  the parameter path (init vs CC20) with the output stage bypassed. (b) Repeat the levels grid after that. (c) Only
  then the burst. (d) Recover the COMPRESS → gain-computer map from the CompXT core (`0x141ac7450` setter,
  `0x141a95f00` per-frame core) rather than fitting it to the rig.

## Files

`supercharger-gt-levels.tsv`, `supercharger-gt-levels-rerun.tsv`, `supercharger-gt-burst.tsv`,
`supercharger-gt-ramp.tsv`, `supercharger-gt-release.tsv` (raw tables, columns as printed by the rig and analysis);
specs `supercharger-gt-*.spec.json`; `supercharger-gt-analyze.py`. Raw WAV and `.npz` captures were deleted from scratch
after analysis; they reproduce from the specs with `oracle-slot python3 tools/kontakt-oracle/fxrig.py levels|burst SPEC OUT`
and `oracle-slot python3 tools/kontakt-oracle/stimrender.py SPEC OUT_PREFIX`.
