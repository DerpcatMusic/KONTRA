# Kontakt 8 amplifier effects: gap table (k-amps-a)

Scope: ACBox (0x53), Bass Invader (0x54, version 0x52), Bass Pro (0x61), Cabinet (0x24), EP Preamps (0x6C), HotSolo (0x54, version 0x10).

Status: **shared-kernel refactor shipped; the four missing models are measured but not yet modelled.** The Kontakt 8 source export is not complete (`source/kontakt8/` is empty, no `READY`), so the laws come from black-box Wine-oracle renders (below), not decompilation. No DSP law has been fitted yet, so nothing below is a spec.

## Done in this unit

- `amp/kernel.rs`: the four-lane ops, the 4x upsampler phase matrices, the decimator taps and the shaper table lookup were copied into six amp kernels (Fuzz, Distortion, Cat, Twang, AC Box, Big Fuzz). They now come from one module. Big Fuzz keeps its own phase matrix. The decimator taps differ only in the sign of zero in two places; that changes no value. All eight amp golden-vector tests pass.
- `tools/kontakt-oracle/ampspecs_a.py`: stimrender specs for Bass Invader, Bass Pro, HotSolo and EP Preamps.

## Oracle measurements (2026-10-21, Kontakt 8 under Wine, `stimrender.py`, `ramp` stimulus)

- Bass Invader: 29 steps (10 knobs), all rendered. Default-settings 200 Hz ramp: H2 -34 dB, H3 -24 dB against a bypass floor near -134 dB, so it is strongly nonlinear. Output clips at 1.0.
- Bass Pro: 36 steps, all rendered, with DRIVE and MASTER run last. Master/gain at 0 mutes (peak 0.02 to 0.03). At DRIVE 1.0 the peak drops (0.59 against 0.83 at default), so drive is not a simple gain increase.
- HotSolo: 30 steps, all rendered, with MASTER run last. Master at 0 mutes (0.03). Depth and bass at 0 both drop the peak to 0.48.
- EP Preamps: **not measurable on this install.** `Presets/Effects` has no EP Preamps folder, so `render.py` cannot load a preset. Needs a library or preset that contains it.
- Run order matters for the harness: a level-0 master knob stays in the session, and the stimrender detection marks later steps missing when they are silent. The specs therefore run such knobs last.

## Next

1. Fit each model's transfer curve (memoryless part) from the ramp, with filters off where possible.
2. Identify the pre/post filters and tone stack from the knob sweeps.
3. Express each model as a config of the general amp chain (pre-filter, transfer, post-filter, tone stack) and check it against the renders.

## Evidence used

- Public manual: Kontakt 8 effect reference (`docs.native-instruments.com/ni-tech-manuals/kontakt-manual/en/effect-reference`) lists control names for Bass Invader, Bass Pro, EP Preamps and HotSolo. It gives no numeric laws.
- Repo bindings: `docs/re/kontakt-bindings/fx_other.tsv` (field index, slot, identity/bool law for ACBox, Bass Invader, Bass Pro, Cabinet, HotSolo). No rows for EP Preamps.
- Reader schemas: `docs/re/kontakt-catalog/reader_schemas.tsv` (Bass Invader 0x51/0x52, HotSolo 0x10 `Fx8BB`, Bass Pro 0x10 `Fx17Bx5`, EP Preamps 0x10/0x11 `Fx11BBIx4`, `Fx11BBIx4Bx4`).
- Existing engine: `crates/sampler-core/src/dsp/color/amp/` (Cabinet, AC Box, Twang, Fuzz, Cat, DStortion, Cry Wah, Big Fuzz) with kemu vectors `kontakt_*_vectors.f32le`. Mapping in `crates/sampler-kontakt/src/effects/color.rs` (`amp()`, `unproved()`).

## Module notes

### ACBox (0x53, Fx7B) — covered
Eight stored fields (normal volume, brilliant volume, bass, treble, tone cut, tremolo speed, tremolo depth, mono). Mapped to `AmpModel::AcBox` with native vectors (`kontakt_acbox_vectors.f32le`). Engine: `amp/acbox.rs`.

### Cabinet (0x24, Fx4I) — covered
Size, air, treble and bass (bass and treble stored -1..1), type clamped 0..=10. Mapped to `AmpModel::Cabinet` with vectors. Engine: `amp/cabinet.rs`.

`docs/architecture-v2/DSP_COVERAGE.md` still lists ACBox and Cabinet as `missing / unmeasured`. That row is stale and should be updated.

### Bass Invader (0x54, version 0x52) — missing
- Fields (fx_other.tsv, version 0x52): volume (field 0, slot 8), treble (1), hi-mid (2), lo-mid (3), bass (4), boost (5), master (6), lo-cut (7, bool), high-boost (9, bool), mid-contour (8, bool).
- Manual adds a Mono control (Kontakt 8.1, `$ENGINE_PAR_BASSINVADER_MONO`). It is not in the bindings table or the reader schema, so it is unverified in the stored format.
- Behaviour per manual: Volume adds saturation and distortion. No law, topology or table recovered.
- Gap: transfer stage, tone stack, EQ/contour topology, switch behaviour, master/output law. Unsupported: needs evidence.

### Bass Pro (0x61, Fx17Bx5) — missing
- Fields (version 0x10): gain (0), bass (1), mid (2), mid freq (3), treble (4), drive (5), master (6), graphic-EQ bands at 40, 90, 180, 300, 500 Hz, 1, 2, 4, 10 kHz (7–15), GEQ volume (16), bright (19), ultra-lo (17), ultra-hi (18), GEQ on (20), mono (21), field stored in slots 8…80.
- Manual: GEQ bands ±12 dB. Mid freq 200–3200 Hz.
- Gap: nine-band graphic EQ law (Kontakt GEQ may match the existing `eq_family` Solid G-EQ fit, but this is not proved for Bass Pro), drive curve, master law. Unsupported.

### EP Preamps (0x6C, version 0x10/0x11) — missing
- Reader schemas only (`Fx11BBIx4`, `Fx11BBIx4Bx4` for 0x11). No binding rows, so field indices and laws are unknown.
- Manual: Drive, Bass (becomes passive high-pass cutoff in Passive EQ mode), Mid (Electric Grand only), Treble, Rate, Amount, Width, Mono, Output. Electric C mode added in 8.8.
- Gap: all of it. Needs a probe run (`tools/kontakt-oracle/probe_fx.py scan`) to get bindings.

### HotSolo (0x54, version 0x10) — located, not modelled
- Fields (reader 0x140d01b50, `Fx8BB`): eight floats at record +0x08..+0x24 (prenormal, preoverdrive, master, presence, depth, treble, mid, bass) and two bools at +0x28 (overdrive channel) and +0x29 (mono). Eight rows are in `fx_other.tsv`; the two bools have no rows yet. The earlier "output" field is not in the reader.
- DSP: class vtable 0x144fc0900; block process is slot 23 (0x140aa2ae0), kernel 0x140aab1e0. Details and measurements: [k-amps-a-hotsolo.md](k-amps-a-hotsolo.md).
- Measured: overdrive and mono reroute the output, master and prenormal mute the left channel, and the right channel stays close to the bypass. The bypass render carries a 0.76766 scale and a 51-sample delay.
- Gap: coefficient laws (0x140ad5440, 0x140ad44f0), kernel topology, knob-cache source. Unsupported.

## Gap table

| Module | Engine coverage | Bindings | Reader | Laws/topology | Status |
| --- | --- | --- | --- | --- | --- |
| ACBox | `AmpModel::AcBox`, vectors | yes (9) | yes | recovered (vectors) | covered |
| Cabinet | `AmpModel::Cabinet`, vectors | yes (7) | yes | recovered (vectors) | covered |
| Bass Invader | none | yes (12) | yes | none | missing |
| Bass Pro | none | yes (24) | yes | none | missing |
| EP Preamps | none | no | yes | none | missing |
| HotSolo | none | yes (8; overdrive and mono bools unbound) | yes | vtable and block process located; laws not recovered | missing |

Shared-engine pieces that might serve the missing models (not proved for them): `color::Curve` waveshapers, `eq_family` (Solid G-EQ fit), `filter` biquads, `voice_mod` tone path. Using any of them would need evidence for each model.

## What is needed to finish

1. Kontakt 8 decompile (`source/kontakt8/READY`), or the Wine oracle: `tools/kontakt-oracle/probe_fx.py scan "Effects/HotSolo"` (and the other three) to collect bindings, then `render.py` and `eqfit.py`-style fits for the transfer and EQ laws. The oracle needs `/mnt/Windows11` for the NKP decoder, which is not mounted here.
2. Once laws are known: one general amp topology (pre-filter, transfer, post-filter, tone stack) in `sampler-core/src/dsp/color/amp/`, with per-model config, and golden vectors.

## Confidence

- Parameter names and field indices for Bass Invader, Bass Pro, HotSolo: high (bindings and reader agree).
- EP Preamps field layout: low (reader only).
- Manual control lists: medium (secondary search summary, not checked against the PDF).
- No DSP behaviour claims are made for the four missing models.
