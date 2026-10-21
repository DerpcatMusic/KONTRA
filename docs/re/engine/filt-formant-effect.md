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
- Gap: 15 ms native control ramps and the 32-sample wrapper clock are not wired (carried over from
  `docs/re/kontakt-formant.md`). Static saved controls are exact to the recorded impulse vector.

### Formant II (type 91)

- Parameters: same three knobs as Formant I.
- Law: same core, second table (rows 5..=9). The flag `second` adds 5 rows to the Talk position
  (`FUN_140b054e0` as recorded in `formant.rs`). Tests: `second_table_starts_five_rows_in`,
  `second_table_row_five_matches_libm_coefficients`.
- Engine: static `Processor::FormantII` through `FilterBank(Topology::FormantII)`.
- Change in this unit: script/modulation lanes now drive Formant II through `Processor::LiveFormant { second: true }`
  instead of being dropped with a `NotModeled` note. The output for a given knob setting is identical to the
  static path by construction (same `Formant::compile`).
- Gap / confidence: no measured Kontakt vector for Formant II output exists yet. Routing Formant II lanes
  through the same setter is inferred from the shared core and is **unverified** against Kontakt.

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
| Formant I | 90 | static + live lanes (Talk/Sharp/Size) | 15 ms ramps, 32-sample clock | kontakt-formant.md, recorded impulse | Exact (static), Partial (live timing) |
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
