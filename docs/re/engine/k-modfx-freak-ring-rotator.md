# Engine unit: Kontakt Freak, Ring Modulator, Rotator

Status: **waiting on READY**. `/data/dev/workspace/Research/KONTRA/source/kontakt8/` is empty and has no `READY` marker, so no decompiled Kontakt 8 code was available. Ring Modulator and Rotator core laws are not recovered and nothing below is fabricated. `docs/ENGINE.md` named in the swarm brief and AGENTS.md is not present in this repo; the engine map is taken from `crates/sampler-core/src/dsp/` and `crates/sampler-ir/src/fx/` directly.

Evidence used: `docs/re/kontakt-bindings/fx_bite_dirt_freak_wowflutter_twindelay.tsv` (Freak), `docs/re/kontakt-bindings/fx_other.tsv` (Ring Modulator, Rotator), the in-repo code comments in `crates/sampler-kontakt/src/effects/{color,timemod,mod}.rs`, and `crates/sampler-core/src/dsp/color/freak.rs`.

## Freak (serialization id 0x68)

| Item | Detail |
|---|---|
| Core | `crates/sampler-core/src/dsp/color/freak.rs`, a port of the native core layout (offsets 0x940..0x11e0): envelope-gated drive, quadrature or side-chain mix, Hilbert pair, implicit feedback, ADAA shaper, fuzz chain in mode 0. |
| Config | `sampler_ir::fx::color::Freak { knobs: [f32; 19] }` (`crates/sampler-ir/src/fx/color.rs:211`). |
| Mapping | `crates/sampler-kontakt/src/effects/color.rs:159` (`typed_core`, module 0x68). Saved field to core index: 0→3 (TYPE→core 3), 1→4 (HARMONICS), 2→11 (FEEDBACK), 3→18 (MIX), 4→13 (TUNING), 5→12 (CARRIER), 6→14 (WIDTH), 7→1 (FREQUENCY), 8→0 (ANTIFOLD), 9→10 (STEREO), 10→5 (CONTOUR), 11→7 (BP_FREQ), 12→6 (RELEASE), 13→15 (DEMOD), 14→17 (GATE), 15→2 (WIDE_RANGE), 16→8 (BP_FILTER), 17→9 (no binding row), 18→16 (MODE, table). |
| Tests | `dsp::color::freak::tests::matches_the_native_core` passes (`build-slot cargo test -p sampler-core freak`), comparing against native fxemu vectors `tables/kontakt_freak_vectors.json` at 48 kHz. |

Gap table (Freak):

| Gap | Impact | Action |
|---|---|---|
| Hilbert coefficient designer (0x141968890) not ported; 44.1/48/88.2/96 kHz use dumped tables, other rates use the nearest | Other sample rates are not bit-exact | Ceiling noted in `freak.rs` (`ponytail:`). Needs the decompiled designer. |
| Saved field 17 has no row in the bindings TSV; code maps it to core 9 (STEREO) | Unverified mapping for one field | Verify against a native record dump when READY. |
| Binding rows give laws as `identity` except BP_FREQ/CARRIER etc. where only the identity mapping is measured | Parameter scaling is taken as identity | Re-check against the core setters once the source is exported. |
| `docs/architecture-v2/DSP_COVERAGE.md:173` lists Freak as "missing, unmeasured, 0 slots" | Stale | Update the coverage row once this doc is merged. |

Freak is already covered by the shared engine. No DSP change is needed in this unit beyond the rate-designer gap.

## Ring Modulator (serialization id 0x64)

| Item | Detail |
|---|---|
| Core | None in the engine. `MODULES` names it (`crates/sampler-kontakt/src/effects/mod.rs:67`), but there is no `typed_core`, no `unproved` note and no `Curve` variant. |
| Bindings (version 0x50, `fx_other.tsv`) | RINGMOD_FAST_MODE field 0 (bool), RINGMOD_RING field 1 (f32, offset 12), RINGMOD_FM field 2 (offset 16), RINGMOD_FREQUENCY field 3 (offset 20), RINGMOD_EDGE field 4 (offset 24), RINGMOD_LFO_RATE field 6 (offset 32), RINGMOD_LFO_AMOUNT field 7 (offset 36). Shared: EFFECT_BYPASS field 3, INSERT_EFFECT_OUTPUT_GAIN field 4 (pow a=16 p=3). All identity laws except as noted. |
| Core laws | Not extracted. Need the carrier oscillator, FM, edge shaping, LFO and the fast-mode switch from the decompiled module. |

Gap table (Ring Modulator): every row is missing. The module is unsupported and reported as such by the format crate. No DSP was added: a ring modulator written from the manual alone would not be bit-exact and would be a guess. Implement after READY, as a config-driven variant of the shared modulation family, not as a per-product copy.

## Rotator (serialization id 0x22)

| Item | Detail |
|---|---|
| Core | None. Notes: summed-input core 0x140aae920 with four-state recurrences and ring buffers; horn/drum law located at control 0x140a97d30, setter case 0xe at 0x140962630; lane vectors at 0x149e6c400..0x149e6c45c not verified (`timemod.rs:241`, `color.rs:196`). |
| Bindings (version 0x50) | RT_SPEED field 0 (bool, threshold 0.6), RT_BALANCE field 1, RT_ACCEL_HI field 2, RT_ACCEL_LO field 3, RT_DISTANCE field 4, RT_MIX field 5 (pow a=1 p=3). Shared: EFFECT_BYPASS field 3, INSERT_EFFECT_OUTPUT_GAIN field 4. |
| Core laws | Rotation, acceleration and distance laws not extracted. |

Gap table (Rotator): every row is missing. Remains in `unproved` with the existing notes. Blocked on READY.

## Next steps (after READY)

1. Read the Ring Modulator core (carrier, FM, edge, LFO, fast mode) and the Rotator core (speed threshold, balance, acceleration, distance, mix) from `source/kontakt8/` with `rg`.
2. Add a `Curve` variant per module in `sampler-ir/src/fx/`, the DSP core under `sampler-core/src/dsp/`, and the field mapping in `sampler-kontakt/src/effects/`.
3. Tests: one recorded-vector or null test per module.

## Decompile evidence (Kontakt 8 export, READY)

Confirmed from `source/kontakt8/` by name only. No DSP law is taken from it yet.

- Ring Modulator: effect class `BEffectRingModulator` (`0140a80000.c:3752`), UI name "Ring Modulator" (`0140a70000.c:3106`), preset folder `RingModulator`. Module classes `RingModulatorModuleBase` / `Inst` / `Group` (`0140640000.c:6519`, `0140650000.c:11`, `0140650000.c:1925`). Setter for the LFO waveform (par id 0x34d) rejects values >= 2 (`0140780000.c:6103`), so the LFO has two waveforms.
- Rotator: effect class `BEffectRotator` (`01407c0000.c:4673`), parameter class `BParFXRotator` (`0140700000.c:1283`), modules `RotModuleBase` / `RotModuleInst` / `RotModuleGroup` (`0140640000.c:6552`, `0140650000.c:47`, `0140650050`).
- Freak: DSP core `ni::effektrig::dsp::freak::Freak` (`0141960000.c:2872`), already ported in `dsp/color/freak.rs`.

Gap: the vtable slot tables are not in the export, so the process method behind each class cannot be named from the `.c` text. The Ring and Rotator DSP bodies are therefore not yet located. Next step: a vtable dump for these classes (or a read-only xref query on a copy of the Ghidra project), then the laws can be written here.

## Ring Modulator: measured laws (Wine oracle, 2026-10-22)

Method: `tools/kontakt-oracle/stimrender.py` with a 1.5 s constant 0.5 input, so the output is the carrier itself. The harness was fixed to send stored values as raw `ENGINE_PAR` integers (x 1e6); earlier runs had truncated fractional values to 0 or 1.

| Measurement | Result | Law |
| --- | --- | --- |
| Carrier rate, FREQUENCY x in steps of 0.05 (slow) | 1 Hz at 0.0-0.10, 2.0 at 0.25, 7.5 at 0.5, 24 at 0.75, 80 at 1.0 | `0.625 * 128^x` Hz (fit within 1 zero-crossing per second) |
| Carrier rate, fast mode, x = 0.25 / 0.5 / 0.75 / 1 | 105 / 353.5 / 1189.5 / 4000 Hz | slow law times 50 |
| Wet level vs dry, RING = 1 | wet peak / dry level = 1.6000 | wet gain 1.6 |
| Crossfade, RING = 0.5 | half the wet amplitude and half the dry mean | linear `(1-r) dry + r wet` |
| Carrier shape, EDGE 0 | 3rd harmonic 3.7 %, 5th 0.8 %, 7th 0.3 % | sine plus a small 3rd (not yet explained) |
| EDGE 0.5 / 1 | 3rd 0.6 % / 11 %, 5th 1.5 % / 4 % | EDGE reshapes the carrier; law not yet fit |
| LFO amount 0.5 / 1 (DC input) | carrier rate shifts (about 90 Hz / 53 Hz zero-crossing count) | LFO frequency modulation; LFO rate and wave not yet measured |

Implemented in this PR: `sampler_ir::fx::ringmod` (carrier Hz, mix, wet gain) and `sampler_core::dsp::ringmod` (sine carrier, linear crossfade, phase kept in the stage's delay storage). Tests: `carrier_rate_and_wet_gain_follow_the_measured_laws`, `dry_mix_is_bit_exact_and_phase_persists`.

Not yet implemented, diagnosed as unsupported in the mapping: EDGE shaping, FM (input-driven), LFO (amount, rate, wave). Format mapping: `crates/sampler-kontakt/src/effects/ringmod.rs` reads the saved fields 0..7 (bindings order, typed by the NI schema) into `Processor::RingMod`; non-default edge, FM, LFO wave and amount are reported as unmeasured and the module is not marked modelled. Tests: `carrier_frequency_follows_the_measured_points` (slow and fast points) and `unmeasured_parameters_are_reported_not_modelled`.
