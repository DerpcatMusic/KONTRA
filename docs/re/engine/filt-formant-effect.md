# Engine unit: effect filters (Formant I, Formant II, Phaser, Vowel A, Vowel B, AET Filter)

Scope: Kontakt 8 only. Status date 2026-10-21.

Evidence state: the decompiled Kontakt 8 export (`source/kontakt8/READY`) was not present when this unit was
worked, so no constant or law below was re-checked against decompiled C. Evidence is the existing repo
record (`docs/re/kontakt-formant.md`, `docs/re/kontakt-bindings/filter-type-ids.tsv`, `crates/sampler-core/src/dsp/filter/formant.rs`,
`crates/sampler-kontakt/src/effects/filter.rs`) plus the in-repo tests. Re-verify against the decompiled export
when it lands.

## Shared engine structure these modules use

- Saved type ids: `docs/re/kontakt-bindings/filter-type-ids.tsv` (Phaser 13, Vowel A 14, Vowel B 15, Formant I 90,
  Formant II 91, AET Filter is an effect type, not a filter type).
- Mapping table: `crates/sampler-kontakt/src/effects/filter.rs` (`build()` per saved type; `Exact` / `Partial` /
  `Elsewhere` fidelity).
- Filter config: `crates/sampler-ir/src/fx/filter.rs` (`Topology::Formant`, `Topology::FormantII`,
  `Variant::VowelA/VowelB` on `Topology::Cascade`).
- Formant core: `crates/sampler-core/src/dsp/filter/formant.rs` (`Formant::compile(knobs, second, rate)`).
- Phaser: `crates/sampler-ir/src/fx/timemod.rs` (`phaser_filter`), processed as `Processor::Phaser`.

## Module notes

### Formant I (type 90)

- Parameters: Talk, Sharp, Size (normalized 0..1). Rows of 5 frequency/radius/amplitude sets; Talk selects
  4·Talk with linear interpolation; Sharp and Size select the core index (see `docs/re/kontakt-formant.md`).
- Law: three parallel second-order bandpasses, coefficients updated every four samples, 32-sample control
  clock. Evidence: tables 0x144fcea10..0x144fcebc0, setter 0x140b05ac0, radare2 hexdumps.
- Engine: `Processor::Formant` (static) and `Processor::LiveFormant` (event-rate lanes, KSP `ENGINE_PAR_FORMANT_*`).
- Gap: live control ramps are not wired. Decompile (`FUN_140b00750`, audio): each band's state
  accumulates a per-sample coefficient increment (`state += delta` at `+0x30..`), and the deltas are
  recomputed every 4 frames (`(frame & 3) == 0`). `FUN_140b05ac0` writes the targets and deltas at
  `+0x500..+0x540` and sets a dirty flag at `+0x530`. The 15 ms ramp length is not yet read as a constant.
  The engine uses `Formant::compile` per fragment (no ramp), so live Formant timing is approximate.
  Static saved controls are exact to the recorded impulse vector.

### Formant II (type 91)

- Parameters: same three knobs as Formant I.
- Law: same core, second table (rows 5..=9). Confirmed in the decompile (Kontakt 8 export):
  - `FUN_140b05ac0` (control setter): control index 3 stores `0.5 < value` into the flag at `+0x60`.
  - `FUN_140b054e0` (Talk position): base offset `DAT_1447149f0` (5.0) when the flag is 1, else 0; then
    `+ 4·Talk`, clamped to the last row. So the flag adds 5 rows.
  - `FUN_1408faa30` (wrapper): four control streams (0..3), each clamped to [0,1], refreshed every 32 frames
    (`+0x1bc` counter).
  - Type 91 reaches the assignment in `FUN_1409619b0` at `case 0x5b`, which calls `FUN_140aefc60` (the
    four-control dispatch through vtable `+0x1b0`). The value passed for control 3 comes from the caller's
    arguments, which Ghidra did not recover, so the static flag value for type 91 is **confirmed as a
    control-3 selector, not yet read as a constant**.
  Tests: `second_table_starts_five_rows_in`, `second_table_row_five_matches_libm_coefficients`.
- Engine: static `Processor::FormantII` through `FilterBank(Topology::FormantII)`.
- Change in this unit: script/modulation lanes now drive Formant II through `Processor::LiveFormant { second: true }`
  instead of being dropped with a `NotModeled` note. The output for a given knob setting is identical to the
  static path by construction (same `Formant::compile`).
- Gap / confidence: no measured Kontakt vector for Formant II output exists yet. Routing Formant II lanes
  through the same setter is now supported by the decompile (control 3 selector above). Lane routing is still
  **unverified** against a Kontakt measurement.

### Phaser filter (type 13)

- Parameters: Cutoff, Resonance (normalized 0..1).
- Law: native four-stage first-order allpass cascade, `Processor::Phaser`, with a static coefficient from
  normalized cutoff. Evidence: test `phaser_filter_native_four_stage_impulse` (impulse at cutoff 0 with
  `a = (1-.02)/(1+.02)`, dry/wet half mix).
- Gap: live Cutoff/Resonance lanes are not wired (`Processor::Phaser` is static). Resonance feedback and the
  cutoff-to-coefficient law above cutoff 0 are not recorded as vectors. Kontakt's internal feedback path is
  not verified. Note: `docs/FEATURES.md` describes an older notch-only model; that is stale.

### Vowel A / Vowel B (types 14 and 15)

- Parameters: stored cutoff/resonance; the Kontakt vowel-pair morph is in the cascade core.
- Law: `Topology::Cascade` with `Variant::VowelA` / `VowelB`, four band-pass stages (`variant(Bp, 4, ...)`).
  `filter.rs` records the core as matching the native code to 2e-5 (scalar order). That claim is cited to the
  `kemu` tool and was not re-run in this unit.
- Gap: no live control lanes for vowel morph. No recorded vector in `docs/re/` for these two types was found.

### AET Filter (effect type `$EFFECT_TYPE_AET_FILTER`)

- Not implemented. The KSP name is recognised but not lowered (`docs/architecture-v2/KSP_COVERAGE.md`,
  parity ledger "Approximate, opaque").
- Behaviour, from the RE reference model `tools/re/kontakt_dyx.py` (BEffectDYX<N>, Kontakt 8.13.1, not a
  product implementation): a 44.1 kHz block spectral morph. 3200-point FFT, 128-sample hop, per-layer
  spectral responses multiplied in the frequency domain, breakpoint window `W(node, pos)` and a per-block
  position/amount ramp. It needs AET analysis data (per-layer spectral fingerprints and morph node
  breakpoints) that no format reader in `crates/` currently provides.
- Gap: whole family missing: reader for the AET analysis data, the spectral morph core, the saved type id and
  its fields. This is a spectral-morph family, not a member of the filter family, so it should land as its
  own core, not inside `Config` for filters.
- Confidence: low for the model details (python reference written from a decompile; not checked against
  the Kontakt native output here).

## Gap table

| Module | Kontakt type | Engine now | Missing | Evidence | Status |
| --- | --- | --- | --- | --- | --- |
| Formant I | 90 | static + live lanes (Talk/Sharp/Size) | per-sample coefficient ramps (decompiled, not ported), 15 ms length unread | kontakt-formant.md, FUN_140b00750, FUN_140b05ac0 | Exact (static), Partial (live timing) |
| Formant II | 91 | static table 2 + live lanes (this PR) | measured output vector; lane routing confirmed on Kontakt | formant.rs tests, core inference | Partial, unverified routing |
| Phaser | 13 | static four-stage allpass | live Cutoff/Resonance lanes; feedback vector | timemod.rs test | Partial |
| Vowel A | 14 | cascade VowelA, BP4 | live morph lanes; recorded vector | filter.rs (kemu) | Claimed exact, not re-run |
| Vowel B | 15 | cascade VowelB, BP4 | live morph lanes; recorded vector | filter.rs (kemu) | Claimed exact, not re-run |
| AET Filter | effect | none | reader, spectral morph core, saved id | tools/re/kontakt_dyx.py | Missing |

## Tests

- `crates/sampler-core` formant filter tests: `second_table_starts_five_rows_in`,
  `second_table_row_five_matches_libm_coefficients` (pass).
- `crates/sampler-kontakt` formant tests: `formant_native_parallel_impulse_and_saved_sharp_size_mapping`,
  `v1_formant_slot_is_an_executable_processor_in_both_scopes` (pass).
- No new test added for the Formant II live route; it reuses the core path verified above.
- No tests removed.

## Not done in this unit

- Decompiled Kontakt C was not used (export not READY when worked). Constants above are from repo evidence.
- No Wine oracle measurement (`tools/kontakt-oracle/`) was run for Formant II, Phaser, Vowel A/B or AET.
- AET Filter core not written.
- Phaser live lanes and Vowel morph lanes not wired.

## Shared filter core check (filt-sv, PR #116)

Checked against `dsp/filter/linear.rs` (`Zdf`) and `dsp/filter/shaper.rs` (`Knee`, `pade`):

- **Formant I / II**: three parallel second-order bandpasses in radius/frequency form, with the native
  sine/cosine polynomials and a 4-sample coefficient clock. Not a zero-delay SVF, and the output is bit-checked
  against the recorded impulse. Expressing it as `Zdf` would change the numbers, so it stays in `formant.rs`.
- **Vowel A / B**: two parallel warped biquad sections (`cascade.rs` `SectionClass::VowelA/VowelB`), no state
  nonlinearity, so `Knee` has no place in it. Stays in the cascade core.
- **Phaser**: a cascade of first-order allpass stages, no state saturation. Stays in `timemod.rs`.
- **AET Filter**: spectral morph, outside the linear/shaper family.

Result: none of the six modules is a `Zdf` or `Knee` configuration, so this unit adds no shared-stage code.
Mapping them would break the bit-exact vectors for no gain. Revisit if the Kontakt decompile shows a
state knee in Formant or Phaser.

## Kontakt measurements (Wine oracle, 2026-10-21)

Rendered with `tools/kontakt-oracle/eqrender.py` (48 kHz, impulse through L, bypass reference, probes at
30..15000 Hz). Specs: Formant I/II (`FORMANT_TALK/SHARP/SIZE` grid, 18 steps); Vowel A/B and Phaser
(`CUTOFF/RESONANCE` grid, 24 steps, `Filters/Effect` presets with `EFFECT_SUBTYPE` 13/14/15).

| Module | Steps | Engine test | Result |
| --- | --- | --- | --- |
| Formant I (90) and II (91) | 17 (step 1 excluded: first write lost, reads back as the Talk 0.5 response) | `formant_responses_match_kontakt_measurements` | max error < 0.1 dB at every probe |
| Vowel A (14), Vowel B (15) | 16 | `vowel_filters_match_kontakt_measurements` (cascade `design`, parallel bank) | < 0.1 dB |
| Phaser (13) | 8 (cutoff grid at res 0, res .5/.9/1 at cutoff .5) | `phaser_filter_matches_kontakt_measurements` | < 0.1 dB, including resonance feedback `0.85 r` |

Still open: Formant live ramps, live lanes for Phaser and Vowel, AET Filter (not implemented).
