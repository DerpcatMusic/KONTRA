# Engine unit: Ladder, Monark, PRO-53 (Kontakt 8)

Status: **waiting on READY.** The Kontakt 8 decompile export
(`/data/dev/workspace/Research/KONTRA/source/kontakt8/READY`) does not exist yet and the
export directory is empty. Everything below comes from existing repo evidence. The two open
laws (Ladder stored gain, Ladder HQ 2x mode) need the decompiled functions and are not
implemented.

## Modules and mapping

Saved type ids are measured (`docs/re/kontakt-bindings/filter-type-ids.tsv`). Mapping lives in
`crates/sampler-kontakt/src/effects/filter.rs`; cores live in `crates/sampler-core/src/dsp/filter/`.

| Kontakt module | Saved id | Engine core | Fidelity today |
| --- | --- | --- | --- |
| Ladder LP1 / LP2 / LP3 / LP4 | 30 / 31 / 32 / 33 | `ladder.rs` + `ladder_kernel.rs`, taps 1..4 | Partial |
| Ladder HP1 / HP2 / HP3 / HP4 | 34 / 35 / 36 / 37 | same, taps 5..8 | Partial |
| Ladder BP2 / BP4 | 38 / 39 | same, taps 9..10 | Partial |
| Ladder Peak / Notch | 40 / 41 | same, taps 11..12 | Partial |
| Monark LP4 / LP2 / LP1 / BP | 142 / 141 / 140 / 143 | `kontaktfx.rs` (Monark) | Exact (KFX, kemu, 48 kHz) |
| Pro-53 | 16 | `cascade.rs` `Variant::Pro52` (Chamberlin) | Exact (kemu, 2e-5) |

Ladder taps follow the saved id: `taps = id - 29` (1..=12), internal selectors 22..33 map to
modes 0..11 (`docs/re/DSP_SYSTEM_INVENTORY.md`).

Monark record layout (reader in `sampler-kontakt/src/effects/mod.rs`): kind x2, i32 variant,
five knobs, kind x3. Mode per saved id: 0 LP4, 1 LP2, 2 LP1, 3 BP. Variant 0 clean, 1 noisy
drive, 2 drive. Knob order in the core: cutoff, resonance, input gain, modulation, feedback.

## Parameters and laws

### Ladder (saved 30..41)
- Cutoff: normalized, native table `2^(i/60 - 20)` with 2401 entries and 575x + 1481.8816
  indexing (`ladder_kernel.rs`, FUN_1404d14e4).
- Resonance: pole polynomial clamped at 8, feedback scale 4.16, zero-delay feedback estimate,
  +-24 clamp and 1/48 soft clip.
- Four trapezoidal stages with tap weights and compensation curves for all twelve selectors.
- Control clock: 32 host frames, four internal frames; 2 ms ramp in 32-frame ticks on prepare.
- Gain: stored in the IR as -1..=1, applied linearly by the kernel. The native stored-gain law
  is **unproved**: only nine literal gain-target values are bit-exact.
- HQ: saved flag (record offset 0x28, setter parameter 3) selects a 2x resampled path. The
  core does not implement it. The IR has no HQ field, so HQ-on files run single-rate with a
  warning (`docs/FEATURES.md`).
- Alternative cutoff path, flag `0x49` (record offset 0x29): not implemented.

### Monark (saved 140..143)
- Oversampling: x3 below 66 kHz, x2 below 100 kHz through the shared Kaiser stage.
- Tables `monark.f32le` (cutoff to note 48, resonance to feedback 21, two 4032-entry tables).
- Noise LCG per channel; stereo channel interleave unprobed (vectors are mono).

### Pro-53 (saved 16)
- Two Chamberlin stages, second fed the feedback NL `(a - c|s|)s` with input clamped to +-4.
- Output selector Lp 4 pole, `Variant::Pro52`.

## Evidence

- Ladder core checks: `NATIVE_LADDER_EVIDENCE.md`, `ladder_native_vectors.rs`,
  `ladder_native_hq_vectors.rs` (static HQ-off composition; PCM over 564 original captures,
  HQ on and off, 44.1/48/96 kHz, gain -1/0/+1; HQ-on captures are not reproduced).
- Monark and KONTAKTFX: bit-exact vs the native classes under `kemu` (per
  `sampler-kontakt/src/effects/filter.rs` KFX note).
- Pro-53 and Legacy Ladder: `FUN_14095d940` / `FUN_1409099a0` checked under `kemu` to 2e-5.

## Gap table

| Item | Shared engine has | Missing | Needed to close |
| --- | --- | --- | --- |
| Ladder LP/HP/BP/Peak/Notch (12 selectors) | Full cores, all tap weights and compensation | Stored-gain law beyond nine points | Decompile of the gain binding (FUN_140b00be2 caller path, typed 22..33 apply) |
| Ladder HQ (2x) | Nothing (saved flag ignored with warning) | 2x resampler, coefficient setup (`sin`/`exp` in native), per-entry state | Decompile of FUN_140b06180 HQ branch + resampler; HQ-on captures as oracle |
| Ladder cutoff flag 0x49 | Default path only | Alternative cutoff branch | Decompile of the 0x49 branch |
| Ladder moving controls | Ramped 32-frame clock | Native behaviour when a control moves mid-note is only partially compared | Recorded moving-control vectors |
| Monark LP4/LP2/LP1/BP | Full core, Exact | Stereo interleave; noisy-drive variant untested beyond vectors | Stereo capture |
| Pro-53 | Full core, Exact | None found in repo evidence | — |

## Next steps (when READY exists)

1. Locate the Ladder gain binding and HQ branch in `source/kontakt8/` with `rg` (addresses above);
   record laws here as our own spec. Do not paste pseudocode.
2. Implement the stored-gain law and the HQ 2x path in the shared Ladder core, config-driven
   (`LadderSettings` gains an HQ selector; the Kontakt mapper sets it from the saved flag).
   Remove the HQ-on warning only once the path matches the HQ-on captures.
3. Add one golden test per behaviour (gain law, HQ path). Keep the existing 9-case gain test.
