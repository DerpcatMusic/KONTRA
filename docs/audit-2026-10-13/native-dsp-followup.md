# Kontakt scalar DSP follow-up

The coordinator ran four bounded original-machine-code captures against Kontakt 8.13.1 standalone SHA256 `0fe6356e0879d058b6e5b73507c54c5e345cea451b35287c974e438291d4dae8`. Captures and receipts live under `/mnt/MAIN_STORAGE/kontra-runs/kontakt-parity-performance-20261013/native/`. This DSP lane performed only source/static review and Python stdlib arithmetic comparisons; no builds or native execution.

The committed numeric fixture `KONTAKT_NATIVE_DSP_PCM.vectors.json` contains 1,362 rows (396 moving stereo, 216 all-active multiband EQ, 120 mixed flat/active multiband EQ, 630 one-band near-flat gain/pointer cases). SHA256: `642dd2f982b88941e3988e857c2419c196230984509b55a5673c4716126395b8`. Inputs are authored synthetic PCM; the fixture copies actual native output and state arrays, with no third-party code. Zero-frame rows test retained state and do not claim generated audio. Coefficient preparation executes original instructions, SIMD EQ dispatch is disabled, and declared CRT/memory/base-reset helper substitutions remain.

## Changes and evidence

Moving nonpseudo Stereo Modeller selects narrow moving arithmetic at the call boundary. For b=.5-width, the moving narrow outputs are `(1-b)*L+b*R` and `b*L+(1-b)*R`, retaining binary32 operations. The settled difference form had caused exact PCM differences in 180 of 396 captures. The corrected moving law matches all 396 captured PCM and final width/pan states in independent stdlib analysis. Scalar and voice-lane paths now share the corrected production primitive; both consume the independent numeric PCM in tests. Native pan fourth-lane delta reuse, call partition dependence, and width/pan persistence are retained. The captures cover constant, impulse and nonconstant signed PCM, empty/boundary/129-frame calls, and two split sequences. Same-side width motion is verified; crossing .5, near-settlement branch admission, intra-call moving targets, pseudo automation and exact settled-high arithmetic remain unverified.

The native scalar one-band EQ bypasses strictly between normalized gains -2^-20 and +2^-20 when all three native modulation pointers are null. It copies binary32 input and preserves all four histories. At either endpoint the recurrence runs. Production now applies that fast path and retains history across 32-frame control boundaries. All 630 original PCM/history cases match independent stdlib analysis, including threshold neighbors and pointer-presence masks. Pointer-presence records verify the bypass predicate only; production native modulation-buffer advancement remains unimplemented/unverified. Production bypass admission also checks each prepared frequency/width/gain lane through `PreparedParameter::projected`: `ControlRamp.modulation` is present for accepted voice projections even when the delta is zero or routes cancel. Such feeds conservatively keep the recurrence. Ordinary scalar control aliases and their automation ramps have no projection marker and retain the unmodulated fast path. This does not equate the supported projected-control cadence to native audio-buffer stepping.

BEQ2Band/BEQ3Band scalar entries are `140aeb240`/`140aeb8e0`, wrappers `140904780`/`140904790` (thunks to `140918930`/`140918e30`). Their scalar processing is a cascade of the existing native per-band f32 recurrence with per-band flat bypass. Exact PCM and every band's histories match all 216 all-active and 120 mixed captures. Always processing flat bands fails 96 mixed rows; a whole-module-only bypass fails 64. Rates are 8/48/192 kHz for the active captures, mono/stereo, distinct nonzero coefficients, nonzero initial histories, impulse/signed PCM, and 0/1/3/4/5/31/32/33/129 frames. Per-band local reset clears 16 channel histories and snaps smoothed frequency/gain; native base reset remains substituted.

Saved Filter subtypes 22/23/24 now select the same dedicated native per-band IR owner and float-bit frequency/width conversions. Live physical band owners retain flat bands; source metadata and ENGINE_PAR_GAIN/FREQ/BW aliases retain actual band and slot indexes. The end-to-end authored adapter/lowering/runtime test covers all three subtypes, wrong-slot rejection, all band aliases and held-note gain edits. Generic non-Kontakt filters and TPT fallback laws are unchanged.

Multiband production reuses the existing one-band control scheduling per band. The adapter explicitly reports UnknownLaw for native multiband wrapper timing and SIMD; this batch does not claim complete multiband processor or host equivalence. Initial dirty state, native modulation buffers, full lifecycle, latency/tail, standalone/plugin identity and host audio parity remain open. No new exact-module status is assigned.

## Parent-owned validation

Syntax/format parsing passed for the modified Rust sources and `git diff --check` passed. Focused Rust tests are authored but were not executed by this lane:

- `dsp::stereo::tests::moving_stereo_matches_original_pcm_partitions_and_state`
- `dsp::lanes::tests::moving_stereo_lanes_match_original_pcm_and_state`
- `dsp::native_eq::tests::scalar_multiband_and_flat_bypass_match_original_pcm_and_history`
- `dsp::native_eq::tests::flat_gain_preserves_production_history_across_control_boundaries`
- `dsp::native_eq::tests::projected_zero_delta_suppresses_flat_bypass_but_scalar_aliases_do_not`
- `lowered_zero_and_cancelled_eq_routes_suppress_flat_bypass` in sampler-core integration target `native_eq_modulation` (all three EQ fields, registered processor and direct control routes, zero depth and exact cancellation)
- `modulation::native_legacy_eq_saved_and_live_controls_reach_production_audio` in sampler-kontakt

Run sampler-core DSP tests and sampler-kontakt library tests through the coordinator's heavy wrapper before accepting the code.
