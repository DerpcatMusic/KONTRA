# Kontakt 8 spatial unit: Stereo Modeller, Stereo Tune, Surround Panner, pan laws

Status: **partial.** Kontakt 8 decompile is complete (`source/kontakt8/READY`).
This pass confirmed the Stereo Tune reader; the Stereo Tune and Surround Panner
DSP laws still need their process routines. Items not re-read in the decompile
come from existing repo evidence (`docs/architecture-v2/KONTAKT_DSP_LAWS.md`,
`docs/re/kontakt-bus-pan.md`, `docs/re/kontakt-catalog/*`,
`docs/re/kontakt-bindings/fx_other.tsv`) and the current engine code. Items
marked **open** are not claimed.

Docs first (NI Kontakt 8 manual, Effect Reference / Bus Insert Effects, and the
KSP engine-parameters "Spatial" section) were not re-fetched in this pass; the
parameter names below come from the KSP symbol tables already in the repo.

## Stereo Modeller (module 0x1f, version 0x70)

Parameters: `ENGINE_PAR_STEREO` (spread, stored as 0..1 with .5 identity),
`ENGINE_PAR_STEREO_PAN` (-1..1), pseudo flag (bool). Reader 0x140d02d10
(`effects/color.rs::stereo_modeller`).

Laws (from `KONTAKT_DSP_LAWS.md` s.Stereo Modeller, verified in the native
harness):

- Width w, settled: w >= .5 uses spread s = 2w-1, `L' = (1+s)L - sR`,
  `R' = (1+s)R - sL`. w < .5 uses a = .5-w, `L' = L + a(R-L)`, `R' = R + a(L-R)`.
  w = .5 identity, w = 0 mono average, w = 1 gives (2L-R, 2R-L).
- Balance p: L' x (1 - max(0,p)), R' x (1 + min(0,p)). Linear, no crossfeed.
- Width one-pole k = F(1/180) per sample; pan k = F(1/1800). Four-sample SIMD
  groups reuse the third delta on the fourth sample; scalar remainder uses a
  fresh delta. Groups restart per process call.
- Pseudo: L immediate, R delayed by `min(1023, trunc(sr*.01*w^3))` frames
  through a 1024-float zero-initialised ring.

Already in the shared engine: `dsp/color/stereo.rs` (matrix, balance, both
smoothers, pseudo ring), `sampler-ir/src/fx/color.rs::Stereo`, and the mapping
in `sampler-kontakt/src/effects/color.rs`. Tests: settled matrix and pseudo
impulse, four-frame recurrence, and the frozen-kernel comparison.

**Open:** channel layouts other than stereo; width transitions across .5;
pseudo-mode automation; base reset; host latency; full bypass/mix behaviour.

## Stereo Tune (module 0x70, version 0x10)

Catalog: `BParFXStereoTune`, sertype 0x70, version 0x10, reader
0x140ac6120, schema `Fx4` (four saved floats, 16 bytes). KSP names:
`$ENGINE_PAR_STEREOTUNE_SPLIT`, `_DRIFT`, `_SPREAD`, `_MIX`.

Decompile confirmed (`0140ac0000.c`, `FUN_140ac6120`, 113 bytes): the reader
accepts only a 16-byte payload (else throws) and stores four consecutive floats
read in order into object offsets +0x8, +0xc, +0x10, +0x14. Object class
`StereoTuneModuleInst`, 0x458 bytes, constructor `FUN_140650620` (base
`FUN_14064b740`; vftable global not yet resolved). The four float-to-KSP-name
mapping (Split, Drift, Spread, Mix) and the process routine are **not yet
recovered**: the process method sits in the class vtable, and resolving that is
the next step. No DSP law is claimed.

RTTI scan of `re-binaries/Kontakt8.exe` (static, `pefile`): the
`StereoTuneModuleInst` vtable is at 0x14474b670 (82 slots). Slot 81 is
0x14066e470, a parameter-query thunk in a second base vtable at 0x14474b8f8,
not the DSP. Candidate slots checked and rejected as DSP: 0x140651900,
0x140651970, 0x140651ac0, 0x14065cf20, 0x14065d0b0, 0x14065d240, 0x140658a60
(generic module plumbing: parameter get/set and state queries). The process
slot is therefore still unidentified.

Follow-up (static scan): the StereoTuneModuleBase vtable is 0x14474b140. The
instance (0x14474b670) overrides only slot 16 (0x140668220, parameter-object
wiring at +0x430..+0x450 and +0x400/+0x408, the same generic pattern used by
other modules) and slot 69 (returns -1). Every other slot is inherited from the
base, so the audio work is not a virtual override of this class. The DSP is
probably a child core object at instance +0x410 (`FUN_141f33da0`,
`FUN_141f362a0`, vtable slot 0x58). Its class is not identified yet. The
modeller's process is `FUN_140aa5ac0` on `BEffectStereo` vtable 0x144e961d0
slot 23, and it has no Stereo Tune branch.

Before this change the module had no name in the module table and showed as
`unknown effect 0x70`. It now has a name and a `NotModeled` note saying the
slot is passed through unprocessed. No DSP is invented.

## Surround Panner (module 0x1d, version 0x70)

Catalog: `BParFXSurroundPanner`, sertype 0x1d, version 0x70, 112-byte payload,
reader 0x140d02f60, schema with 0x70 layout (typed). Its speaker lists are
variable and it embeds a resampling-style panner (0x140ab0070, 0x140ab4eb0).
Measured bindings in `kontakt-bindings/fx_other.tsv` (identity or linear
mapping, not the pan law):

| Parameter | Mapping (observed) |
| --- | --- |
| `ENGINE_PAR_SP_DIVERGENCE` | identity 0..1 |
| `ENGINE_PAR_SP_SIZE` | linear 0.1..0.9 (a=0.1, b=0.9 on the unit range) |
| `ENGINE_PAR_SP_LFE_VOLUME` | linear -36..+36 dB (a=-36, b=72) |
| `ENGINE_PAR_SP_OFFSET_AZIMUTH/X/Y` | linear -1..1 (a=-1, b=2) |
| `ENGINE_PAR_SP_OFFSET_DISTANCE` | identity 0..1 |
| `ENGINE_PAR_INSERT_EFFECT_OUTPUT_GAIN` | `pow a=16 p=3` |

The speaker matrix, azimuth/distance panning law and LFE routing are **not
extracted**. The module is already diagnosed as NotModeled (version 0x70 on
module 0x1d is excluded from the Saturation mapping and named in
`color::unproved`).

## Bus pan (instrument bus, `$NI_BUS_OFFSET` + n)

Evidence: `docs/re/kontakt-bus-pan.md` (`FUN_140824910` audio, `FUN_140823d70`
per channel, left `1 - clamp(p,0,1)`, right `clamp(p+1,0,1)`, unity at centre,
no crossfeed). Engine: `crates/sampler-kontakt/src/effects/mod.rs::instrument_buses`
routes it through the shared `Processor::Pan` stage with `PanLaw::Balance` (identical gains; position clamped to ±1 first, matching the native clamps); the test
`instrument_bus_pan_only_uses_native_stereo_balance` checks it.

Decompile confirmation of `FUN_140824910` (bus audio, `0140820000.c`):

- Order: effects are upstream; then L/R swap if `+0x2d` and the channel count
  is 2; then phase invert if `+0x2c` (XOR of the sign bit, mask 0x80000000,
  on every sample); then fader; then the stereo balance; then
  `FUN_140715380`, which is a peak meter (ring of 32 floats with peak hold),
  not an effect.
- Fader: current `+0x60`, target `+0x24`. If the ratio of
  `|cur + 1e-12| / |target + 1e-12|` is above 1.001 or below 0.999, the gain is
  ramped per sample: each sample multiplies by `g`, then
  `g += (target - g) * (1/180)`, starting from `cur` on every channel and
  storing the final `g` into `+0x60`. Otherwise one constant multiply by target.
- Pan: current `+0x64`, target `+0x28`. The step is `10/sr` per side, so
  rising `20/sr` and falling `-20/sr`. If `|cur - target| > 20/sr`, the
  per-sample pan trajectory is emitted (`cur`, then `cur` advances toward target,
  clamped at target) and stored into `+0x64`. Otherwise the static balance
  uses target and `+0x64` is left unchanged.
- The stored `+0x2e` first-call flag copies the saved values into the current
  fields before the first block.

Still open: the saved byte locations of the invert and swap flags. The
`InsertBus` record reader in `vendor/ni-file` exposes only name, volume, pan,
output and an unknown tail, and the BInsertBus reader was not yet found in the
decompile. Until then the engine keeps both flags off. Live pan wiring (a bus
pan control slot next to `BUS_VOLUME_SLOT`, and the per-sample pan trajectory
above) is not built yet.

## Group and instrument pan

`sampler-core/src/ownership.rs::Pan::gains` applies the same balance law to
group pan: `gain*(1-max(0,p))` left, `gain*(1+min(0,p))` right, centre unity.
`docs/re/kontakt-bindings/limiter-dynamics.tsv` instead says a "group pan law
0.7677" applies before the insert in the limiter oracle. That value is a
centre attenuation and conflicts with centre unity. Left **unresolved**: it
needs a measurement at centre pan on a group, and the reader for group pan was
not read in this pass.

## Gap table

| Item | Shared engine state | Evidence | Status |
| --- | --- | --- | --- |
| Stereo Modeller width/pan law | implemented, tested | KONTRA_DSP_LAWS | matched (settled) |
| Stereo Modeller pseudo delay | implemented, tested | KONTRA_DSP_LAWS | matched (settled path) |
| Stereo Modeller channel layouts, base reset, latency | missing | none | open |
| Stereo Tune (0x70) | named + diagnosed; no DSP | catalog reader schema | missing (DSP law) |
| Surround Panner (0x1d v0x70) | diagnosed; no DSP | fx_other.tsv bindings | missing (matrix, pan law, LFE) |
| Instrument bus static pan | implemented on shared Pan/Balance stage, tested | kontakt-bus-pan.md | matched (static) |
| Bus live pan slew | law confirmed (±20/sr, threshold), not wired | decomp `FUN_140824910`, `FUN_140823d70` | missing (control plumbing) |
| Bus fader ramp (1/180, ratio 1.001/0.999) | law confirmed | decomp `FUN_140824910` | law confirmed; engine check pending |
| Bus phase invert / L-R swap | audio order and laws confirmed; saved byte offsets not located | decomp `FUN_140824910` | partial |
| Group pan centre law (0.7677 vs centre unity) | conflict unresolved | limiter-dynamics.tsv, ownership.rs | unverified |

## Verification

- `build-slot cargo test -p sampler-kontakt --lib unproved_spatial` passes.
  It checks that Stereo Tune and Surround Panner surface a named NotModeled
  note and produce no processors.
- Existing Stereo Modeller and bus-pan tests were not rerun in this pass.

## Next

1. Stereo Tune: the vtable is located (0x14474b670); identify the process slot
   and derive the float-to-name mapping and the law, then implement it in the
   shared `color` family.
2. Surround Panner: extract the speaker list reader and the 0x140ab0070 panner.
   Decide whether a generic multichannel stage belongs in `sampler-core` (see
   `docs/architecture-v2/KONTAKT_TOPOLOGY.md` P1 T12) before adding one.
3. Bus live pan: add a bus pan control next to `BUS_VOLUME_SLOT` and the
   native ±20/sample-rate trajectory above. Keep it additive in
   `engine_parameters.rs`.
4. Bus invert/swap: locate the BInsertBus saved bytes for the two flags, then
   set them in `effects/bus.rs::bus_plans`.
