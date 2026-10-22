# Engine unit: Ladder, Monark, PRO-53 (Kontakt 8)

Status: the Kontakt 8 decompile is in (`source/kontakt8/`, READY). The constants and laws below
were checked against it and against the binary's own data (`Kontakt8.exe` .rdata read with
pefile; the exponential table is filled at load by FUN_1404d14d0). One row stays open: which
record byte drives the Ladder alternative cutoff path (0x49).

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
- Gain (confirmed, decompile): the typed apply calls the control setter FUN_140b05c70 index 0,
  which FUN_140b05320 turns into the table index `1200 + 119.58940887 g` (constant pair
  0x144fc4b50 / 0x144fc5450); the stored gain is the linear factor `2^(index/60 - 20)`. This is
  12 dB per unit, and the kernel's `exponential(1200 + 119.589_41 gain)` is the same law. The
  target lands in lane 0 (0x4fe0, input gain). The modulated path clamps it to 0..1 (FUN_140b05c70
  call sites in FUN_1408f0000). Table: native loop FUN_1404d14d0, `pow(2, i/60 - 20)` in f64,
  stored f32, i = 1..2400; entry 0 is 0x35800000 (= our `exponential(0)`).
- Cutoff (confirmed): index `575 v + 1481.8816` (0x144fcfe64, 0x144fcfe80), 0 .. 2399 clamp,
  linear interpolation between table entries; pole polynomial `((40.80263 w^2 + 10.335426) w^2
  + pi) w`, clamped at 8 (0x1447149f8). Clip: clamp +-24 (0x144f38e18/0x144f38c70), then
  `v (1 - |v| / 48)` (0x144f38674 = 0x3caaaaab). Feedback scale 4.16 (0x144fcfdd0).
- HQ: the saved flag (first flag byte of the record: v0x92 `unknown_flag`, v0x95 first flag)
  selects the native 2x path. Implemented as the shared Kaiser 2x oversampler (`dsp/kaiser.rs`,
  the same stage Monark and Simple/Dual SKF use) around the kernel run at twice the internal
  rate, one host frame at a time. Its registers live in the chain's cells. The static
  composition matches all 282 native HQ cells bit-exact (`ladder_original_hq_static_composition`).
  Moving controls in HQ retune once per host frame (native cadence unproved).
- Alternative cutoff path, flag `0x49`: decoded in `ladder-cutoff-byte.md`. v0x90..0x94 set the
  flag to 1 (the reader's tail), v0x95 reads it as its first flag byte, and HQ is its second.
  Flag 1 snaps the cutoff lane to each target on the 32-frame tick (zero step rate from the core
  init). The kernel reads it from `LadderLP4::cutoff_path`, not from the record version.

### Monark (saved 140..143)
- Oversampling: x3 below 66 kHz, x2 below 100 kHz through the shared Kaiser stage.
- Tables `monark.f32le` (cutoff to note 48, resonance to feedback 21, two 4032-entry tables).
- Noise LCG per channel; stereo channel interleave unprobed (vectors are mono).

### Pro-53 (saved 16)
- Setter FUN_14095d940 (confirmed): note `100 v + 20`, index `5 note + 1381.88`, table value;
  stage frequency `f = hz * (6.283 / rate)` (native constant 0x144f38bdc, applied as
  `hz * (K/rate)` in FUN_1408de8e0's scale 0x364). Resonance: `k = 0.48 r + 0.46` (r >= .5) else
  `1.16 r + 0.12`; second stage `k2 = 0.8 k`; damping `2 - 2k`; limit `(0.293 r)(r + 1.3075) +
  0.7352` (all three constants checked). Process FUN_1409099a0: two Chamberlin stages, second fed
  the feedback NL `(a - c|s|)s`, input clamped to +-4.
- Output selector Lp 4 pole, `Variant::Pro52`.

## Evidence

- Ladder core checks: `NATIVE_LADDER_EVIDENCE.md`, `ladder_native_vectors.rs`,
  `ladder_native_hq_vectors.rs` (static HQ-off composition; PCM over 564 original captures,
  HQ on and off, 44.1/48/96 kHz, gain -1/0/+1; HQ-on captures are not reproduced).
- Monark and KONTAKTFX: bit-exact vs the native classes under `kemu` (per
  `sampler-kontakt/src/effects/filter.rs` KFX note).
- Pro-53 and Legacy Ladder: `FUN_14095d940` / `FUN_1409099a0` checked under `kemu` to 2e-5.

## Gap table (after the decompile)

Rows marked confirmed were checked against the decompile and the binary's constants.

| Item | Shared engine has | Status |
| --- | --- | --- |
| Ladder gain law | `exponential(1200 + 119.589 g)` | confirmed (FUN_140b05320, FUN_1404d14d0) |
| Ladder cutoff, resonance, clip, feedback constants | kernel | confirmed (binary constants) |
| Ladder HQ (2x) | shared Kaiser 2x, flag > 0.5 | confirmed (FUN_140b06180 factor, FUN_140ae22e0 16-tap); HQ-on moving-control cadence still unproved |
| Ladder flag 0x49 (alt cutoff) | version proxy `0x90..=0x92` | **open**: mechanism confirmed, source byte not decoded for v0x95; needs a native capture with the byte set |
| Ladder moving controls | 32-frame ramp, 2 ms | partly confirmed (FUN_140b036a0, FUN_140b06180 ramp count); HQ cadence open |
| Monark | Exact (kemu runs native code) | not re-read in the decompile this pass; stereo interleave unprobed |
| Pro-53 | Exact (constants confirmed) | confirmed; per-stage feedback constants (0.13335, 0.0166875) verified by vectors, not re-read |


## Next steps

1. Done: the flag is explicit (`cutoff_path`). Confirm flag 1 with a native capture (open in
   `ladder-cutoff-byte.md`).
2. Re-read Monark's setter and process (FUN_140b05740 / FUN_140afd9c0) against the decompile.
