# Kontakt 8 amplifier effects: Jump, Reverb Delight, Super Fast 100, Twang, Van51

Status: **waiting on READY.** `/data/dev/workspace/Research/KONTRA/source/kontakt8/` is empty and has no `READY` marker, so no decompiled function has been read. Nothing below is a recovered core law. Items are either repo evidence (cited) or open questions. Twang is the only one of the five with a native core already in the engine, and its evidence predates this unit.

## Evidence used

- Module IDs and names: `crates/sampler-kontakt/src/effects/mod.rs:42-50` (`0x23 Twang`, `0x4d Jump`, `0x52 Van51`).
- Object classes, reader schemas, object addresses: `docs/re/kontakt-catalog/ser_classes.tsv` (`BParFXJump` 0x4D `0x140d01e10`, `BParFXVan51` 0x52 `0x140d033e0`, `BParFXSuperFast100` 0x76 `0x140ac61a0`, `BParFXReverbDelight` 0x7A `0x140ac5e50`), `reader_schemas.tsv:172,182,222,226`.
- Parameter bindings: `docs/re/kontakt-bindings/fx_dynamics_dist_tape.tsv:25-33` (Jump), `fx_other.tsv:177-188` (Van51), `fx_other.tsv:117-118` and `fx_raum_delight_psyche_vibrato_phasis_replika.tsv:101-120` (Reverb Delight). No binding rows exist for Super Fast 100.
- Twang core: `crates/sampler-core/src/dsp/color/amp/twang.rs` (header lists the addresses: init `0x1408de330`, apply `0x140adf660`, kernel `0x140ade440`), config mapping `crates/sampler-kontakt/src/effects/color.rs:95-96`, `AmpModel::Twang` at `crates/sampler-ir/src/fx/color.rs:131-132`, vector test `matches_the_native_twang` against `kontakt_twang_vectors.f32le`.
- Current diagnostics: `crates/sampler-kontakt/src/effects/color.rs:195-201` (`unproved` for `0x4d | 0x52 | 0x76`, amp/cabinet/pedal cores not extracted); `crates/sampler-kontakt/src/effects/space.rs:246` (Reverb Delight: field binding measured, MONO unproved because field 11 bytes are unstable, amp, tremolo and spring-reverb cores not extracted).
- Inventory: `docs/re/DSP_SYSTEM_INVENTORY.md:147` lists all five under Amplifiers. `docs/diagnostics/20261019-installed-gap-inventory.md:78` names the Jump amplifier models as the open gap.

## Per module

### Twang (0x23): implemented, Kontakt 8 evidence from before this unit

- Params (core order): volume, treble, mid, bass, bright, mono. Knobs 0..3 are clamped to 0..1; bright and mono are booleans.
- Engine: `AmpModel::Twang` with the four-lane tube stage, tone stack, push-pull power stage with ripple sag, and 4x oversampling. Shared shape with AC Box, own tables.
- Gap: the four static tables (`0x144f3d900..0x144f3f900`) and the reset operating point are dumped under kemu, not re-derived. The reset runs before knobs are applied in the native code. Both are noted as `ponytail:` debt in `twang.rs`.
- Action here: none. Keep the existing vector test.

### Jump (0x4d): not modelled

- Params: preamp (`JMP_PREAMP`, field 0), master (`JMP_MASTER`, 1), presence (2), mid (4), treble (3), bass (5), mono (7, bool). Stored types are float 0..1 identity except mono.
- Missing: the preamp, tone stack and power/cabinet cores and their tables. Topology and ordering are unknown, so no `AmpModel` variant is added. Adding one would be fabrication.
- Diagnostic: already reported as unproved (`effects/color.rs:197`). The gap table entry is the unit's main output.

### Van51 (0x52): not modelled

- Params: pregain rhythm (`V5_PREGAINRHYTHM`, field 0), pregain lead (1), bass (2), mid (3), treble (4), postgain (5), resonance (6), presence (7), lead channel (8, bool), high gain (9, bool). Floats are identity 0..1.
- Missing: the two-channel (rhythm / lead) preamp, channel switch behaviour, resonance and presence stages, and the power stage. None extracted.
- Diagnostic: already reported as unproved.

### Super Fast 100 (0x76): not modelled, binding not measured

- Reader: `Fx8Bx4` (8 bytes plus 4 bytes), object at `0x140ac61a0`.
- Missing: no `$ENGINE_PAR_*` binding rows, so parameter names, indices and units are unknown. The core is also unknown. Diagnostic lives in `effects/color.rs:197`.
- Next step once READY exists: read the binding function and the apply/kernel functions; measure field ordering from the native reader.

### Reverb Delight (0x7a): binding measured, cores missing

- Measured params: volume, bass, treble, bright (bool), tremolo on/off (bool), tremolo speed (with unit table: 5:4, 6:2.6667, 7:2, 8:1.3333, 9:1, 10:0.6667, 12:0.5, 13:0.333 as recorded in `timemod-reads.tsv:4`), tremolo depth, tremolo sag, reverb on (bool), verb type (bool), verb volume, mono (unstable field 11, unproved). Output gain uses the standard `pow a=16 p=3` law.
- Missing: the amplitude/tremolo core, the spring-reverb core, and the MONO field's stable encoding. The reverb core belongs in the shared reverb family, not a new copy.
- Diagnostic: `effects/space.rs:246`.

## Gap table

| Module | Native id | Engine core (`crates/sampler-core/src/dsp/`) | Format mapping | Bindings | Missing | Test |
|---|---|---|---|---|---|---|
| Twang | 0x23 | covered (`color/amp/twang.rs`) | covered (`effects/color.rs:96`) | measured | tables + reset point dumped, not re-derived | `matches_the_native_twang` |
| Jump | 0x4d | none | unmapped (diagnosed) | measured | preamp, tone stack, power/cabinet cores | none |
| Van51 | 0x52 | none | unmapped (diagnosed) | measured | two-channel preamp, switch, resonance/presence, power | none |
| Super Fast 100 | 0x76 | none | unmapped (diagnosed) | not measured | bindings, core | none |
| Reverb Delight | 0x7a | none (shared reverb family would host it) | partial, MONO unproved | measured (except MONO) | tremolo core, spring reverb, MONO | none |

## Open questions

- Confirm Jump and Van51 knob ordering against the native reader once the export is ready.
- Confirm whether Jump and Van51 share the same preamp/power topology (expected in one shared amp family, not two copies).
- MONO field 11 of Reverb Delight: which encoding is stable across saves.

No code change in this unit. Adding any of the four unmodelled modules needs the native cores first; the existing diagnostics already report them as unproved rather than ignoring them.

## Wine-oracle measurement (this unit, 2026-10-21)

Harness: `tools/kontakt-oracle/stimrender.py` with specs from `tools/kontakt-oracle/ampspecs.py` (ramp stimulus, 200 Hz 0 -> 1.0, one knob at 0 / mid / max per step, Kontakt 8 under Wine, output to a PipeWire monitor). Renders live in `~/.cache/kontra-amps-b/` (not committed).

- Reverb Delight: all 31 steps rendered. Default-knob peak 0.567 on a 0.766 bypass peak. The knob sweeps change the tail only (RMS 0.20-0.21 across steps), so the sweep cannot yet separate the tremolo/spring cores. Not fitted.
- Jump: default peak 0.194 (bypass 0.757). PREAMP 0 gives peak 0.021, PREAMP mid and max change the level. Steps after `JMP_PREAMP` returned no audio above threshold, so BASS, MID, TREBLE, PRESENCE, MASTER and MONO are not measured yet.
- Van51: default peak 0.471 (bypass 0.766). PREGAINRHYTHM 0 gives near silence (RMS 0.0018). Steps after `V5_PREGAINRHYTHM` have no audio above threshold; cause not yet known.
- Super Fast 100: not rendered (no bindings).

Open harness issue: a missing-audio step is recorded as NaN by `stimrender.py`. Jump and Van51 lose audio after their first few knob steps, but Reverb Delight does not. The cause (silence from the knob value, a threshold miss, or the MIDI/step timing) is unconfirmed, so the NaN gaps are not evidence of a law.

Status: no shared-engine change in this unit yet. Measurement is partial and the universal amp chain (pre-filter, transfer, post-filter, tone stack) is not built.
