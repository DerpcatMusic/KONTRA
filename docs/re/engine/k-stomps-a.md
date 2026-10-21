# Kontakt 8 stomps (unit k-stomps-a): Big Fuzz, Cat, Chainsaw, Cry Wah, Dirt, Distortion, DStortion

Status: six of seven modules are already implemented on the shared engine with
evidence from earlier recovery. Chainsaw is not implemented. The Kontakt 8
decompile is now complete (`source/kontakt8/READY`), and it identifies Chainsaw's
parameters, record and object layout, but not yet its apply/kernel laws.

## Shared engine location

- Cores: `crates/sampler-core/src/dsp/color/` (`dirt.rs`, `distortion.rs`, `amp/{bigfuzz,cat,crywah,dstortion}.rs`).
- Config: `crates/sampler-ir/src/fx/color.rs` (`Curve::Dirt`, `Curve::Amp` with `AmpModel::{BigFuzz,Cat,CryWah,DStortion}`, `Curve::Tube`/`Transistor` for Distortion).
- Format mapping: `crates/sampler-kontakt/src/effects/color.rs` (`amp()`, `typed_core()` arm `0x67`, `unproved()`), `effects/mod.rs` (Distortion `Params`, `color::distortion`).

## Modules

| Module (id) | Reader | Parameters (stored, core order) | Engine state | Evidence |
| --- | --- | --- | --- | --- |
| Big Fuzz (0x6F) | `FUN_140ac51f0`, Fx4B | sustain, tone, bass, treble (0..1), mono | implemented: `amp/bigfuzz.rs` (2-lane table clipper, 4x oversampled one-pole network, tone blend) | ctor 0x140a85c00, apply 0x140ab9f40, laws 0x140acf2b0 / 0x140ad1650, kernel 0x140aa8fd0 (module header); kemu vectors `kontakt_bigfuzz_vectors.f32le` |
| Cat (0x55) | Fx7B | volume, distortion, filter, bass, balls, treble, tone (0..1), mono | implemented: `amp/cat.rs` | ctor 0x140a85cd0, apply 0x140ab9100, kernel 0x140aa9ae0 (module header); `kontakt_cat_vectors.f32le` |
| Chainsaw (0x77) | `FUN_140ac51f0` (record version 0x10): f32 at +0x8, +0xc, +0x10, +0x14 and u8 at +0x18 | `chainsaw_distortion`, `chainsaw_low`, `chainsaw_high`, `chainsaw_level` (decompile `FUN_140a815b0`, ids 0xd7..0xda); the u8 is unnamed | **not implemented** (diagnosed) | object ctor `FUN_1407c6e40` (0x2400 bytes, two sub-cores at +0x40 and +0x268), wrapper ctor `FUN_1407fde10`, `BEffectInfoChainsaw` at `01407d0000.c`; apply and kernel not yet located |
| Cry Wah (0x58) | FB | pedal (0..1), mono | implemented: `amp/crywah.rs` (four staggered bilinear lanes) | ctor 0x140a85de0, pedal law 0x140ad07f0, SR setup 0x140abef20, kernel 0x140aaa190; `kontakt_crywah_vectors.f32le` |
| Dirt (0x67) | Fx10BBIII | drive A/B, blend A/B, tone A/B, comp A/B, type A/B, routing, level | implemented: `dirt.rs` (effektrig x2, diode-capacitor Newton stage, routing) | ctor 0x14193ec90, process 0x141940da0, setters 0x14193f620/f930/f910/f920/f760/f5e0, prepare 0x1419404d0 / 0x141942450; mapping in `effects/color.rs` apply 0x140aba420 |
| Distortion (0x1e) | `FUN_140d00c60`, IFF | mode (0 Tube, 1 Transistor), drive, damping | implemented: `distortion.rs` + `effects/mod.rs` | `docs/re/kontakt-drive-parameters.md` (template copy 0x1408373c0, LP copy 0x140837e40, damping 0.4); recovered vectors in `effects/mod.rs` tests |
| DStortion (0x56) | Fx6B | volume, tone, drive, bass, mid, treble (0..1), mono | implemented: `amp/dstortion.rs` (staggered one-pole tone network around 4x table shaper) | ctor 0x140a85ea0, apply 0x140ab96f0 (+ bass 0x140acf440), kernel 0x140aaa6c0; `kontakt_dstortion_vectors.f32le` |

Bindings: `docs/re/kontakt-bindings/fx_other.tsv` covers Cat and DStortion; `fx_bite_dirt_freak_wowflutter_twindelay.tsv` covers Dirt. Big Fuzz, Cry Wah and Distortion binding tables are not in `docs/re`; their mapping rests on the reader schemas and code comments.

## Gap table

| Module | Shared engine covers it | Missing | Action |
| --- | --- | --- | --- |
| Big Fuzz | yes, `amp/bigfuzz.rs` + `0x6f` mapping | binding table not written to `docs/re` | none for DSP |
| Cat | yes, `amp/cat.rs` + `0x55` mapping | binding table exists; no gap | none |
| Chainsaw | no | apply (record -> knobs), kernel, the u8 field's meaning, law/ranges, defaults | diagnosed in `unproved(0x77)`; parameter names and record layout now known |
| Cry Wah | yes, `amp/crywah.rs` + `0x58` mapping | pedal automation 32-frame ramp (0x140acb7d0) treated as settled | none |
| Dirt | yes, `dirt.rs` + `0x67` mapping | knobs are constants; native routing crossfade and smoothers marked settled | none |
| Distortion | yes, Tube and Transistor curves, damping LP | internal-modulator 32-frame clocks, dynamic mode switching (see `kontakt-drive-parameters.md`) | none for this unit |
| DStortion | yes, `amp/dstortion.rs` + `0x56` mapping | none recorded | none |

## Evidence confidence

- Six modules: high for the mapping (reader layout and core order documented in code), medium-high for DSP (kemu vectors and native-address citations already in the tree; not re-verified in this unit).
- Chainsaw: none. Its record reader is shared with Big Fuzz, but sharing a reader does not establish a shared DSP, so no Big Fuzz knobs are reused.

## Public spec (Kontakt 8 manual, Effect Reference, Chainsaw)

Controls: Distortion (input gain; range limited, distortion present at the lowest setting), Low and High (peak filters, max +20 dB), Level (distortion output), Output (module output). No pedal-mode or mono control is documented. This fixes the parameter surface and the topology order (gain, nonlinear stage, peak EQ, level), but not any curve, range, default, or law. Those need measurement or decompiled code.

## Measurement blocker

- The Wine oracle (`tools/kontakt-oracle`) instantiates effects only via `kt.load_fx_preset` / `kt.insert_fx`. No Chainsaw preset exists in the installed factory library, so Chainsaw cannot be inserted without authoring a `.nkp`. No oracle probe exists for that yet.
- KSP has no `ENGINE_PAR_CHAINSAW_*` names, so the write surface is also unknown.

## Open items

- Locate the Chainsaw apply and kernel. `ModulatableDSPCoreWrapper<BEffectChainsawTraits,1>` and `BEffectChainsaw` are named in `01407c0000.c`/`01407f0000.c`; the apply is not among the candidates checked so far (`140ab9ad0`, `140ab9d00`, `140ab9f40` read other records).
- Then record the laws, add an `AmpModel::Chainsaw` core and mapping.
- `docs/architecture-v2/DSP_COVERAGE.md` still lists these modules as "missing"; that table predates the amp cores.
