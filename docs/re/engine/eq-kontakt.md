# EQ family: Kontakt 8 EQ modules vs the shared `eq_family` engine

Unit: `eq-kontakt`. Scope: Kontakt 8 only. Target: `Kontakt8.exe` (see
`docs/re/kontakt-solid-geq-live-control.md`).

**Status: partial. The decompiled Kontakt 8 export is not ready.**
`/data/dev/workspace/Research/KONTRA/source/kontakt8/READY` did not exist when this
was written, and the directory was empty. Everything below comes from recorded
evidence already in the repo (oracle renders, RE spline tables, saved-record
readers, `filters_eq.tsv`), not from new decompilation. The two open rows that
need the native code (coefficient smoothing, LF/HF shelf law) are not closed here.

## 1. Modules

Kontakt 8 exposes three EQ modules that matter here, plus one that has no law yet.

| Module | Saved identity | Shared law | Status |
|---|---|---|---|
| Filters/EQ, 1/2/3-band legacy (`Filter` kinds 22, 23, 24) | `BParFXFilter`, `ENGINE_PAR_FREQ1..3`, `BW1..3`, `GAIN1..3` | `Law::KontaktLegacy` (peak only), one config per band, serial | Proved, see 1.1 |
| Filters/Solid G-EQ (`BEffectSSLGEQ`, module `0x44`) | `BParFXSSLGEQ`, `ENGINE_PAR_SEQ_*` (16 lanes) | `Law::Solid(SolidSection)`, one config per saved band | Partly proved, see 1.2 |
| Pro EQ | not in Kontakt 8 | none | No Pro EQ strings in the decompile, no Pro EQ preset under `Filters/` (section 6) |
| Output gain (`ENGINE_PAR_INSERT_EFFECT_OUTPUT_GAIN`, `pow a=16 p=3`) | `BParFX` field 4 | generic effect output gain, `effects/mod.rs:225` | Mapped for all effects, not EQ-specific |

### 1.1 Legacy 1/2/3-band EQ (Filter kinds 22, 23, 24)

- **Parameters.** `FREQn` (`exp a=20 r=1000`, Hz 20..20 kHz), `BWn` (`6 .. 120`
  stored, linear, octaves), `GAINn` (`linear a=-18 b=36`, displayed dB -18..+18),
  `EFFECT_BYPASS` (bool, stored=1 when value>=1).
- **Band shape (PROVED).** Each band is an RBJ peaking biquad with peak gain
  `2 × stored GAIN dB` and bandwidth `2 × stored BW octaves`. Q is
  `1 / (2 sinh(ln2/2 × 2·BWoct))`, no prewarp. Evidence: stored +12 dB / 1 oct
  fitted to G 24.08 dB, Q 0.6667 at 504/1006/2009 Hz, rms 0.000 dB
  (`eq-topology.tsv`, "band shape"). The engine's `KontaktLegacy` band
  (`legacy.rs`, `b = 2^(w·2.6667 + 1/3)` octave width mapping, coefficient generator
  `0x1409591c0`) is the native law for one band; the cascade is driven by
  `Config::count`.
- **Combination (PROVED).** Kinds 23 and 24 are a serial cascade in dB
  (band1 +12 with band2 −12 gives 12.12 dB at 500 Hz and −11.94 dB at 1 kHz, as
  dB sums). Kinds 22, 23, 24 give identical numbers for the same bands, so kind
  selects only the number of slots, not a separate core.
- **Edge cases.** Stereo is independent (no L→R coupling, right output exactly 0
  for left-only input). Three kind-22 slots at +24 dB ring into the next step.
  That result was taken from a multi-slot write that did not fully apply in one
  KSP callback, so it is not a topology measurement.

### 1.2 Solid G-EQ

- **Saved record.** Module `0x44`, v0x10 record `FFBFx8BFFBB`, v0x11 adds HP
  freq (42), LP freq (46), LP on (50), HP on (51). Full offsets are in
  `kontakt-solid-geq-live-control.md` §1.
- **Knobs.** All twelve continuous knobs are normalized `0..=1` (`identity` law).
  Four switches (LF bell, HF bell, LP on, HP on) are `bool: stored=1 when value>=1`.
- **Spline tables (PROVED, `KONTAKT_DSP_LAWS.md` "Solid EQ parameter splines").**
  Eleven-knot cubic splines on grid `D(F(i·F(0.1)))`, zero slope at the left, zero
  second derivative at the right. Bank 0 applies, bank 16 does not
  (`eq-topology.tsv`, "all"). Bank 0 and 16 differ at HMF idx 2 (−11.0 vs −9.2)
  and HF idx 2 (−11.0 vs −9.0). The gain tables and Q tables are in
  `solid.rs`.
- **Topology per section (from `eq-topology.tsv`, `solid.rs`).**

  | Section | Shape | Status |
  |---|---|---|
  | LMF | RBJ peak, f0 = spline(LMF_FREQ) within 0.0–1.3 %, Q = table Q (RBJ alpha = sin w0/2Q) | PROVED |
  | HMF | RBJ peak, f0 within 0.1–2.7 %, same Q rule | PROVED |
  | LF bell | RBJ peak, f0 = spline(LF_FREQ) exactly; Q 1.09 at ±18.7 dB, 0.70 at ±7.1 dB | PROVED |
  | HF bell | RBJ peak | PROVED |
  | LF shelf | not an RBJ S=1 shelf; fit rms 0.2–1.0 dB, residual overshoot bump | PARTIAL |
  | HF shelf | corner mostly above the audible band; RBJ fit rms up to 1.1 dB | PARTIAL |
  | HP | 3rd-order (18 dB/oct), −3 dB at spline(HP_FREQ) within 1–10 % | PROVED |
  | LP | 2nd-order (12 dB/oct), −3 dB at spline(LP_FREQ) within 1–3 % | PROVED |

- **Proportional Q (PROVED).** Effective RBJ Q = table Q × k(|G|), with
  k = 0.97/0.84/0.63/0.56/0.54 at |G| = 20/17/10/5.6/1.9 dB (LMF); HMF k is
  0.90/0.79/0.65/0.59/0.58. Boost and cut mirror each other.
- **Stereo.** No L→R coupling.
- **Coefficient smoothing (IMPLEMENTED, unconfirmed law).** Kontakt builds the
  core with smoothing on (20 ms, per the RE notes in `solid.rs`). The engine now
  glides each redesigned section's TPT coefficients toward the new design with a
  one-pole step per block, time constant 20 ms (`EqBand::ramp`, set by
  `eq_family::live_band` for `Law::Solid`). Every step was measured 300 ms after
  its write and was history-independent, so settle time is ≤300 ms. This is
  consistent with the measurement but does not confirm the native stepping. The
  setter law is still not located; confirm it when the decompile is ready.
  **Decompile check (2026-10-22, not closed):** the process loop `0x141a595d0`
  advances its 22 current registers (`+0x13db0..+0x13dec`) by their increments
  once per frame, i.e. a linear per-frame ramp, not a one-pole per block. The
  placeholder in `solid.rs` therefore differs in shape. The increment writer is
  still not located (`0x141aa1580` init sets `+0x13e00` and `+0x13e04/08`, but
  not the per-register increments). The Kontakt mapping reports this as
  UnknownLaw (`effects/eq.rs`, "Solid G-EQ smoothing").
- **Live switches (OPEN in the engine).** A bell or pass switch changes the
  band's `Kind` (shelf↔peak) or adds a section. `Kind` is compile-time in the
  stage, so a live switch write has no binding and reports
  `EngineParameterOutcome::Err`. Kontakt's runtime behaviour on a switch flip
  (crossfade or hard switch) is not measured.

## 2. Gap table

| # | Item | Shared engine today | Missing | Evidence needed |
|---|---|---|---|---|
| G1 | Legacy 1/2/3-band peak cascade | `KontaktLegacy`, per-band config, serial | none found | `eq-topology.tsv` band shape / combination rows |
| G2 | Solid LMF, HMF, LF bell, HF bell | `Law::Solid`, spline tables, proportional Q | none found | `solid-geq-fits.tsv` |
| G3 | Solid HP, LP | `solid.rs` HP/LP sections | none found | `eq-topology.tsv` |
| G4 | Solid LF shelf | RBJ S 0.5 shelf + two Q 0.5 peaks (`solid.rs`) | residual 0.2–1.0 dB; native shelf formula not recovered | native shelf coefficient code (decompile READY) |
| G5 | Solid HF shelf | RBJ S 0.55 shelf + Q 0.7 peak (`solid.rs`) | residual up to 1.1 dB | same as G4 |
| G6 | Solid coefficient smoothing (20 ms) | `EqBand::ramp`, one-pole glide per block (`generic.rs`) | exact native stepping (block vs sample) unconfirmed | setter from decompile, or sample-accurate oracle steps |
| G7 | Solid live switches (LF/HF bell, LP/HP on) | saved and init writes only (`apply_writes`), live lane absent | runtime kind/section change | Kontakt step test on switch flips, and decompile of the setter |
| G8 | Solid bank selection | bank 0 applied (matches measurement) | none for Kontakt 8 | — |
| G9 | Pro EQ | n/a | not a Kontakt 8 module (section 6) | none |
| G10 | Output gain law | generic effect output gain (`effects/mod.rs:225`) | none found for EQ | the mapping `16·x³` linear is recorded in the TSV; confirm it in a test |
| G11 | Settle time / sub-300 ms steps | not applicable | measurement needs sample-accurate CC timing | oracle rendering with sample-accurate events |

G1–G3 and G8 are covered by the shared `eq_family` cores. G4–G7 and G9 need
the native code or new measurement. Nothing here required changing shared DSP.

## 3. Verification

Ran on `engine/eq-kontakt` (from `origin/main` 6955c4e8b) with crate-scoped tests only:

```sh
build-slot cargo test -p sampler-ir --lib fx::eq -j 4        # 1 passed
build-slot cargo test -p sampler-core --lib eq_family -j 4   # 24 passed, 1 ignored
```

Full kernel and mapping tests in `sampler-kontakt` were not run in this unit (the
scope asked for crate-scoped tests and the PR runs CI).

## 4. Next step when `READY` exists

1. `rg -n "Solid|G_EQ|0x141ac|0x141ab4d70|smooth" <kontakt8 dir>` to find the
   setter for the 12 knobs and the process loop. Look for the 20 ms ramp.
2. Locate the LF/HF shelf coefficient routine and replace the RBJ S=0.5/0.55 guess
   with the native formula if it differs (G4, G5).
3. Add one golden-coefficient test for the shelf and one step-ramp test for
   smoothing (G6), then update the gap table.

## 5. Decompile evidence (Kontakt 8 export, READY, 2026-10-21)

Confirmed in the decompile (addresses are function starts in `kontakt8/*.c`):

- **Knob storage.** The setter `0x141ac70f0` stores the twelve Solid knobs at
  `module+0x13e30 .. +0x13e5c` (core ids 0..10, 12, 13). Id `0x20` is the bank
  select, and it writes 16 bank values through the setter. Those stored values
  are the knob targets.
- **Recompute.** `0x141a89350` is the Solid coefficient update, one call per
  knob id: it writes the knob as a double into the band object (stride `0x240`)
  and rebuilds that section (spline evaluators `0x141a98340`/`0x141a95be0`, peak
  design `0x141ac4cf0`). `0x141a89140` re-runs it for all knobs.
- **Process.** `0x141a595d0` is the per-block Solid processor. It keeps a
  current value per knob at `+0x13db0 + 4i`, with a per-sample increment at
  `+0x13df0 + 4i`, and calls the recompute for any knob whose increment is
  non-zero. Its per-sample loop is `param_4` long. Its pending-update path
  calls `0x141a89140` and clears the flag at `+0x13da1`.
- **Copy/snap.** The state-copy function `0x141a90...` (near `0x141a917b0`)
  snaps current to target when the module is active, and otherwise marks a
  pending update.

**Not yet found:** the code that writes the increment at `+0x13df0`. Nothing
writes it except zeroing in the reset path, so the glide length (20 ms in the
RE note) and its shape (linear per sample vs my one-pole per block) are still
unconfirmed. The current code's one-pole glide should be treated as a
placeholder until that writer is found. Shelf law (G4/G5), live switches (G7),
and Pro EQ (G9) are not closed in this pass.

## 6. Scope check: Pro EQ and the Kontakt 8 EQ modules (2026-10-21)

- The Kontakt 8 preset library's `Filters/` folder holds `EQ` (1/2/3-band
  legacy, presets `EQ1 - - INIT -`, `EQ1 - Notch`, ...) and `Solid G-EQ`
  (`- INIT Solid G-EQ -`). No Pro EQ preset exists.
- The Kontakt 8 decompile has no `Pro EQ` / `ProEQ` string. The only EQ module
  names are `Solid G-EQ` (`0140a70000.c:3154`) and the `Solid Bus Comp`.
- Conclusion: Pro EQ is not a Kontakt 8 module, so G9 is out of scope for this
  product. The matrix marks it `n/a`.
- Remaining work in this family: G4/G5 (shelf law), G6 (glide writer), G7
  (live switches), plus the multi-slot legacy case. For G6, a sample-accurate
  measurement is needed: MIDI timing jitter (tens of ms) is too coarse for a
  20 ms glide, so the plan is a KSP-timed step against a continuous sine.

## 7. Native probe-response check (2026-10-22)

`solid::tests::sections_match_native_probe_responses` runs the composition of
each section against the Wine-oracle probe rows in
`docs/re/kontakt-bindings/solid-geq-responses.tsv` (LL pair, dB at 30..15000 Hz,
48 kHz).

| Section | Rows | Result |
| --- | --- | --- |
| LF shelf (shelf + two peaks) | geqA 1-9, 20 | matches, worst 0.033 dB (asserted) |
| LF bell | geqA 10-19 | matches, worst 0.045 dB (asserted) |
| HF bell, knob <= 0.5 | geqA 66-75 | matches within 0.1 dB (asserted) |
| HF bell, knob >= 0.75 | geqA 72-75 | OPEN: native has a low-frequency term (about -25.7 dB at 30 Hz for knob 0.75 gain max, +3 dB for knob 1), not in our peak |
| HF shelf | geqA 57-65, 76 | OPEN: gain=min rows match, gain=max rows do not (native +13.8 dB flat at knob 0, ours 0 to +19.9 dB) |
| LMF | geqA 35-38 | partial: knob 0.75 q 0 matches (worst 0.06 dB); knob 0.25 q 0 is off by 2.9-13.6 dB; q 1 is off by up to 2.4 dB at 30 Hz |
| HMF | geqA 53-56 | OPEN: worst 4.0 dB at 15 kHz, q 1 |

Interpretation (not yet proven):
- The mismatched rows are the ones where the bell centre is near the top of
  its range, or where a section is near Nyquist. The native peaks are Orfanidis
  designs with a prescribed Nyquist gain (`0x141ac4cf0`, `solid.rs` header). Our
  `peak()` does not apply that Nyquist term, so the DC/low-frequency offset is
  missing.
- Decompile constants confirmed from the image (not guessed): `pi` =
  4*atan2(1,1) at `0x14a6520b8`, shelf slope S = 0.55 (default) and 0.49 (other
  bank) at `0x1479ab370` / `0x1479ab358`, peak Q 0.7 at `0x1479ab3b0`. The
  shelf design `0x141abee60` has the same algebra as the RBJ high shelf with
  that S. Its HF call matches the existing `high_shelf` with S 0.55, but the
  LF shelf path in the decompile passes S 0.55/0.49, while the instruction-level
  test (`coefficients_match_the_native_setter`) matches S 0.5. That conflict is
  unresolved: the instruction-level test is the stronger evidence, so S 0.5 is
  kept.

Still open (no code in this pass): the glide writer for the per-knob increment
(section 5), live switch semantics beyond the recompute path, bank 16, the HF
shelf gain=max rows, the HF bell knob >= 0.75 rows, and the LMF/HMF rows above.
