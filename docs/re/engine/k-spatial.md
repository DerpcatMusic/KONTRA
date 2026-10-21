# Kontakt 8 spatial unit: Stereo Modeller, Stereo Tune, Surround Panner, pan laws

Status: **waiting on READY.** `source/kontakt8/READY` did not exist when this was
written, so no new decompiled function bodies were read. Everything below comes
from existing repo evidence (`docs/architecture-v2/KONTAKT_DSP_LAWS.md`,
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

The saved field order and the mapping from the four floats to those names are
**not verified** (the reader body was not read in this pass). The DSP law,
including what Drift and Split do, is **not extracted**.

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
writes this as a static `StereoMatrix`; the test
`instrument_bus_pan_only_uses_native_stereo_balance` checks it.

**Open:** live bus pan is not wired. The native per-sample ramp (step
±20/sample-rate once the difference exceeds 20/sample-rate, from the same
evidence doc) needs a bus pan control slot, which does not exist yet next to
`BUS_VOLUME_SLOT` (`sampler-core/src/control.rs`). Phase invert and L/R swap bus
flags are also open: the bus struct has only volume and pan, and the flag offsets
were not extracted in this pass.

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
| Instrument bus static pan | implemented, tested | kontakt-bus-pan.md | matched (static) |
| Bus live pan slew | not wired | native ramp noted in evidence | missing |
| Bus phase invert / L-R swap | not represented | none extracted | missing |
| Group pan centre law (0.7677 vs centre unity) | conflict unresolved | limiter-dynamics.tsv, ownership.rs | unverified |

## Verification

- `build-slot cargo test -p sampler-kontakt --lib unproved_spatial` passes.
  It checks that Stereo Tune and Surround Panner surface a named NotModeled
  note and produce no processors.
- Existing Stereo Modeller and bus-pan tests were not rerun in this pass.

## Next once READY exists

1. Decompile the Stereo Tune reader (0x140ac6120) and its audio routine for the
   field order and law. Write the law here and implement it in the shared
   `color` family, not a copy.
2. Surround Panner: extract the speaker list reader and the 0x140ab0070 panner.
   Decide whether a generic multichannel stage belongs in `sampler-core` (see
   `docs/architecture-v2/KONTAKT_TOPOLOGY.md` P1 T12) before adding one.
3. Bus live pan: add a bus pan control next to `BUS_VOLUME_SLOT` and use the
   native ±20/sample-rate ramp. Keep it additive in `engine_parameters.rs`.
