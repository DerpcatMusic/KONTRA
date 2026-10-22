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
- Law: three parallel second-order bandpasses, coefficients refreshed every four frames within a call.
- Ramp (decompiled, now ported): the setters (`FUN_140b05ac0`) write twelve targets (frequency, radius,
  amplitude; lane 3 is zero) and the shared dirty flag `+0x530`. The 32-frame tick `FUN_140b03690` calls
  `FUN_140acbcb0` on the block at `+0x4a0`: a dirty tick sets `delta = (target - cur) / (N · 32)`,
  N = round(15 ms · rate / 32) (`FUN_140afd4f0`), and arms a countdown of N; a clean tick counts down and
  snaps to the target. The audio loop `FUN_140b00750` advances `cur += delta` every frame.
- Engine: `formant::process_ramp` (`crates/sampler-core/src/dsp/filter/formant.rs`). `Processor::LiveFormant`
  writes its three lanes every 32-frame chunk (dirty each chunk); `Processor::Formant` writes once.
- Evidence: `tools/re/formant_kemu_vectors.py` drives the unchanged core under kemu; the Rust test
  `native_kemu_vectors_through_the_live_path` matches nine cases (static, sweeps, a hold and 44.1 kHz)
  within 3.3e-6 of peak.
- Open: the host start-up order (prepare and setter sequence) is not recovered. The vectors use the
  prepared state (controls written, second prepare). A single prepare with no targets set ramps the
  frequency lanes in from zero; the Kontakt measurements show no start-up ramp, so that is not used.

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
- Engine: static `Processor::FormantII` through `FilterBank(Topology::FormantII)`; live lanes through
  `Processor::LiveFormant { second: true }`, the same ramp as Formant I.
- Gap / confidence: the kemu vectors cover Formant II (flag 1 cases). No Kontakt audio render of a moving
  Formant II control exists; the lane routing is verified against the native core, not against Kontakt output.

### Phaser filter (type 13)

- Parameters: Cutoff, Resonance (normalized 0..1).
- Law: native four-stage first-order allpass cascade, `Processor::Phaser`, with a static coefficient from
  normalized cutoff. Evidence: test `phaser_filter_native_four_stage_impulse` (impulse at cutoff 0 with
  `a = (1-.02)/(1+.02)`, dry/wet half mix).
- Gap: live Cutoff/Resonance lanes are not wired (`Processor::Phaser` is static). Resonance feedback and the
  cutoff-to-coefficient law above cutoff 0 are not recorded as vectors. Kontakt's internal feedback path is
  not verified. Note: `docs/FEATURES.md` describes an older notch-only model; that is stale.
- Native (2026-10-22): the object, vtable `0x144f0fa38`, driver (slot 23), kernel (slot 33) and coefficient
  (slot 36) are located. The live clock is ported (`dsp/filter/phaser.rs`), checked by kemu vectors (6.6e-7 of
  peak), and routed in the runtime (`Processor::LivePhaser`); the host lane writer is open. See `docs/re/engine/filt-phaser.md`.

### Vowel A / Vowel B (types 14 and 15)

- Parameters: Cutoff and Resonance only. The setters `FUN_14095dd00` (A) and `FUN_14095e250` (B) take
  `(this, cutoff, resonance)`, and the process `FUN_140914a20` has no morph input, so there is no morph lane.
- Law: `Topology::Cascade` with `Variant::VowelA` / `VowelB`; core `SectionClass::VowelA/VowelB` in `cascade.rs`:
  two parallel warped second-order sections summed (prepare on the 4-pole prototypes, scalar process).
- Live lanes, the clock of the shared wrapper `0x140ae9f90` (vtable slot 23): a host call writes its goals at
  `obj+0x1dc` (cutoff) and `obj+0x1e0` (resonance); the 32-frame countdown (`obj+0x1bc`) carries across calls. A moving
  call steps the cutoff once per segment, including the empty segment that opens a call starting on a boundary, and
  moves the resonance by 0.015 of the distance over 1e-4. The raw cutoff is kept unclamped; the setter clamps.
- Evidence: `tools/re/vowel_kemu_vectors.py` (kemu, real vtables, `KONTAKT8_EXE OUT_JSON`) writes
  `crates/sampler-core/src/dsp/filter/vowel_kemu_vectors.json`: 10 host-call schedules for A and B at 44.1, 48 and
  96 kHz (static, cutoff and resonance sweeps, mixed call sizes, over-range goals). Test
  `vowel_live_lanes_match_native_process_vectors`: worst case 6.5e-6 of peak (tolerance 1e-5). Static output is
  unchanged: 2e-6 (A) and 6e-6 (B) of peak against the same native run. The Kontakt 8 static measurement
  (`vowel_filters_match_kontakt_measurements`, 0.1 dB) still passes.
- Not verified: (1) host start-up order and how the host feeds live lanes (the vectors write goals per call and set
  the dirty flag only on the first call); (2) any Kontakt audio render of a moving control; (3) the modulation-buffer
  path (`obj+0x1c8` / `obj+0x1d0`, one value per 32-frame chunk), where resonance snaps per chunk instead of
  ramping, not covered by any vector; (4) the SIMD kernel `FUN_140914f20` (gated by `DAT_14a5ed92c`) and the stereo
  group path; vectors are mono and use the scalar kernel; (5) the raw goal `obj+0x1f8` stays 0 in the vectors (its
  host write is unknown).
- RE note: `FUN_140a9a300` stores the rate from xmm1 (`movss [rcx+0x14], xmm1`), so prepare takes the rate as its
  float argument; the decompile shows it without arguments.

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
| Formant I | 90 | static + live lanes (Talk/Sharp/Size) with the native 15 ms ramp | host start-up order | kemu vectors, FUN_140acbcb0, FUN_140b00750 | parity (kemu oracle) |
| Formant II | 91 | static table 2 + live lanes with the native ramp | flag constant for type 91; no Kontakt audio of a moving control | kemu vectors (flag 1) | parity (kemu oracle) |
| Phaser | 13 | static four-stage allpass | live Cutoff/Resonance lanes; feedback vector | timemod.rs test | Partial |
| Vowel A | 14 | cascade VowelA, two parallel sections; live cutoff/resonance clock | host routing of live lanes; Kontakt render of a moving control | vowel_kemu_vectors.json (kemu, 6.5e-6 of peak); Kontakt 8 static measurement | Partial (native live cadence verified) |
| Vowel B | 15 | cascade VowelB, two parallel sections; live cutoff/resonance clock | host routing of live lanes; Kontakt render of a moving control | vowel_kemu_vectors.json (kemu, 6.5e-6 of peak); Kontakt 8 static measurement | Partial (native live cadence verified) |
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
- Phaser live lanes not wired. Vowel A/B live cutoff and resonance lanes are covered in the Vowel section.

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

Still open: live lanes for Phaser and Vowel, AET Filter (not implemented). Formant live ramps are ported and
vector-checked (see Formant I above).
