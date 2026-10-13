# Native legacy Kontakt one-band EQ

Implemented on the canonical main checkout based on `da17ace2`; no commit,
build, Cargo invocation, native-library execution or audio render was run by
this lane. Final Rust and native checks belong to the coordinator.

The production live legacy EQ used `PeakingEq`, a v1 TPT bell whose normalized
cutoff midpoint is approximately 632.4555 Hz. Original Kontakt instruction
checks specify 903.2728881835938 Hz at the same native cutoff. The saved path
used a generic RBJ filter and discarded the native float-bit conversions and
four-history scalar recurrence. V1 agreement tests could not establish native
fidelity for either path.

The new `KontaktLegacyEq` IR processor and `KontaktLegacyEqSettings` core
processor carry native frequency/width knobs and the existing displayed dB
gain lane. Saved filter subtype 22 and its actual physical-slot live controls
now reach this processor. Subtypes 23/24 keep their existing multiband fallback
and record its unverified native status. Generic UVI/TPT bell behavior is
unchanged. `eq_controls`, `source_eq_values`, the EQ target resolver, lowering,
scalar dispatch and tracing consume the new owner; the trace label is
`kontakt_legacy_eq`.

Evidence is the committed [DSP laws](../architecture-v2/KONTAKT_DSP_LAWS.md)
and [instruction vectors](../architecture-v2/KONTAKT_DSP_LAWS.vectors.json),
whose source image is the Kontakt 8.13.1 standalone EXE with SHA-256
`0fe6356e0879d058b6e5b73507c54c5e345cea451b35287c974e438291d4dae8`.
Those laws distinguish standalone instructions from the unverified plugin
payload and declare substituted CRT functions. No new host recordings or
whole-library null measurements are claimed.

Read-only `r2` disassembly of `0x1409590a0` and PE constant extraction in this
lane confirmed gain conversion `(db - -18) * f32(1/18) - 1`, clamped to
[-1,1], in that operation order. The multiplier is bits `0x3d638e39` at
`0x144f3869c`; -18 is at `0x144f38e10`. A saved +6 dB becomes
0.3333333730697632, rather than the result of plain division by 18.

The kernel preserves the instruction-checked float-bit frequency/exp
approximations, coefficient storage order and f32 recurrence with separate
input/output history. A persistent 32-frame countdown retains the verified
frozen-target wrapper behavior: partial chunks update too, empty calls do
nothing, and an inherited zero countdown updates before the next samples.
It deliberately preserves the verified dependence on call partition.
Updated masks clamp only their selected fields and preserve other parameters.
Histories, coefficients and clocks occupy existing prepared processor state;
the render path performs no allocation.

One evidence defect was found before integration: every previously committed
`eq_sample_kernel` vector had zero coefficients/output. Its harness left
`EXTRA[0x1409591c0]` installed, so coefficient preparation is stubbed. These
records could not prove nonzero native scalar audio. After the coordinator
removed setter interception, the original instructions exposed another defect
at sample 63 of the 129-frame test: the old law grouped feedforward terms
incorrectly. Both agents independently read the actual `addss` sequence:
`0x140aeafcb` adds b1*x1 to b0*x0, then `0x140aeafcf` adds that grouped
sum to `(a1*y1+a2*y2)+b2*x2`; the scalar remainder does the same at
`0x140aeb1d1` / `0x140aeb1d7`. The new kernel and tests use this exact
grouping. The coordinator owns fresh vector generation and original-instruction
validation; this lane does not certify the old zero-output records. The
coordinator re-ran all 1,222 original-instruction records successfully and
replaced only the 32 `eq_sample_kernel` rows with nonzero coefficient preparation;
all 28 positive-frame cases produce nonzero PCM. The four zero-frame cases
exercise history and reset behavior without claiming generated audio.
The fresh canonical vector SHA-256 is
`d69f5e51e1f5d0335eb42d7087cf970ffb172fc2305964c84999e9e67c02b1ce`.
The Rust test now independently reconstructs the coefficients, starts with the
original nonzero histories, and checks both the direct recurrence and actual
processor storage/wrapper path against these mono/stereo PCM/history vectors.
Its voice reuse check starts cleared state and requires no retained tail.
Native local reset admission is asserted separately from full host lifecycle.

Added focused tests, not executed by this lane:

- `dsp::native_eq::tests::saved_conversion_matches_original_instruction_vectors`
  (160 saved conversions and the native midpoint witness).
- `dsp::native_eq::tests::coefficients_match_original_instruction_vectors_at_all_rates`
  (225 coefficient vectors at 8, 48 and 192 kHz).
- `dsp::native_eq::tests::parameter_masks_clamp_only_updated_fields_and_retain_coefficients`
  (24 mask vectors).
- `dsp::native_eq::tests::frozen_target_wrapper_cadence_matches_native_block_partitions`
  (nine native call sequences, including empty and boundary calls).
- `dsp::native_eq::tests::nonzero_scalar_recurrence_preserves_native_history_and_order`.
- `dsp::native_eq::tests::nonzero_scalar_pcm_and_histories_match_original_instruction_vectors`
  (requires nonzero native preparation and consumes all 32 corrected mono/stereo
  sample/history cases, including the 129-frame rounding witness).
- `library::tests::native_one_band_eq_saved_and_live_controls_reach_production_audio`
  (authored binary FX payload, real adapter/live bindings/lowering/runtime,
  correct physical slot, three native aliases, and held-note sine response).

The existing init-only EQ gain translation fixture now expects the dedicated
one-band processor while retaining its multiband assertions. A pure Python
binary32 arithmetic cross-check matched all 225 committed coefficient vectors.
`rustfmt` parsed/formatted only the new Rust file, and `git diff --check` passed.
Neither substitutes for coordinator-owned Rust execution.

Limits remain explicit: optimized native sample dispatch, multiband kernels,
modulation-buffer cadence, initial dirty-state conversion, complete native
reset/lifecycle, script integer conversion last bits, standalone/plugin
identity, host latency/tail and calibrated host audio parity are unverified.
Prepared saved controls begin settled; this is an initialization choice, not a
claim about the native dirty-state path. The adapter retains a native lifecycle
UnknownLaw diagnostic for one-band slots. A generic fallback is never renamed
or counted as a complete native EQ processor.
