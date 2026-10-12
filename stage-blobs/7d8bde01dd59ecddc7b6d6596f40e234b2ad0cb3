# Conflux: retain saved zero-delay Digital Multi sine sources

Base: `61158054` on `v2/w6-conflux-modfx-381`. The previous ranked audit identified type 6 LFO/source-module modulation as the next audible class after saved wavetable playback. This change closes the measured zero-delay sine subset, not every source-module parameter or every Digital Multi clock.

## Intent and root cause

The installed Conflux NKI has 182 type 6 LFO records (version 0x73), all retriggered, normalized, with one nonzero sine component whose magnitude is at most 1. Of these, 124 have zero native delay/fade; 58 have a nonzero native fade. The additional v73 flag is false. The saved records and scripts are read in memory; receipts retain aggregate counts rather than decrypted payloads or private identifiers.

The translator rejects waveform 6 before creating an IR modulator. Aliasing it to ordinary sine would introduce two errors: native Digital Multi negates sine, and its normalization leaves a subunit weight intact. A native waveform must remain bipolar: a 0.3 sine at its negative peak has unipolar value 0.35, not 0 and not 0.15.

The existing source registry, route transforms and physical group/internal-slot identities remain the shared path. `LfoShape::SineScaled(level)` supplies the signed peak through IR validation, core lowering and the existing source evaluator. Both IR and direct core preparation reject nonfinite/out-of-range levels. No engine-parameter mirror, parallel modulation compiler, new dependency or callback storage is introduced. Live source writes remain outside this saved-source admission; W5 owns the shared addressed service and is working separately on live wavetable controls.

## Native evidence and subset

Pinned native engine SHA256: `0fe6356e0879d058b6e5b73507c54c5e345cea451b35287c974e438291d4dae8`.

The retained RTTI/vtable index identifies `BLfoMultiDigitalWithTargets<1>`; its waveform entry is `0x140b078d0`. Factory `0x140605430` has distinct case 5/case 6 paths, with case 6 installing vtable `0x144729c08`. Packed layout equality is not the reason for admission.

`tools/dsp/verify_digital_lfo.py` executes original waveform instructions in Unicorn against hash-checked PE bytes, with authored inputs and no substituted helpers. 700 sample values cover positive/negative/subunit/greater-than-unit weights, normalization on/off, 33 phases and 1/3/17/32/127/256-frame blocks. The native result agrees with falling sine and division by `max(1, abs(weight))` within `8.970593361468104e-7`. This is a waveform-core check, not a native host-clock or setter check. No Kontakt/Wine process is launched.

Admission is intentionally limited to type 6/v73, sine-only finite weights in -1..1, zero native delay/fade, unsynchronized delay record and the false extra flag. The sine weight is negated without being discarded or normalized to unity. The existing Hertz/beat timing and retriggered/free-running IR source clock are preserved. Nonzero native fades, other components and live waveform/timing edits remain diagnosed. Native control timing, sync/fade setter conversions and whole-instrument audio remain unverified.

Pinned v1 `0cb7a8a0:src/modulation.rs` admits only a strict version 71/type 5 saved sine subset. It rejects these type 6 records; there is no correctly admitted v1 importer/kernel route to copy for this class. The change is own code from the bounded measured waveform law and reuses v2's common evaluator.

## Validation

Receipts: `/mnt/Windows11/DEV_WORKSPACE/kontra-runs/w6-conflux-digital-lfo/`.

- Installed translator RED: 182 waveform source losses (`red.log`, exit101); the narrowed pre-fix census also retains 182 losses (`red2.log`).
- Numeric native waveform check: 700 generated values pass; Rust waveform checkpoints agree within 1e-6.
- IR→core→PCM fixture: all four quarter-cycle phases, five host partitions and two voice-reuse cycles retain the authored 0.3 peak, correct bipolar attenuation and zero callback heap calls. NaN rejects at IR/lowering and invalid direct core levels reject before rendering.
- Translator fixture covers phase, Hertz/beat projection, both normalization flags, and explicit diagnostics for fade/mixed/nonfinite/extra-flag states.
- Installed GREEN (`green2.log`, exit 0): 124 LFOs admitted with all 124 physical internal-slot identities retained; waveform losses fall from 182 to 58. One nonzero pan route is recovered. Both installed Conflux intent tests pass.
- Core waveform and lowering/heap tests pass (`green.log`); the translator test passes after correcting its zero-time unit assertion (`green2.log`).
- `cargo test --locked -p sampler-core -p sampler-kontakt -p sampler-ir -p sampler-uvi -p sampler-ksp --profile ci --no-run` passes through `kontakto-heavy` (`green2.log`).

No timed CPU/RSS gate, native Kontakt host render, all-corpus scanner or whole workspace test suite is included. Those remain UNKNOWN/HOLD and belong to W0's batch gate. Source-module intensity/frequency modulation and RandomBipolar remain separate open classes.

NEXT: verified physical source-module intensity law, followed by native nonzero LFO fades and shared live source controls.
