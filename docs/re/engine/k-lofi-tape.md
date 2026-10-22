# Engine unit: Kontakt Lo-Fi, Bite, Tape Saturator, Wow / Flutter

Scope: Kontakt 8 only. Evidence levels: **documented** (public manual or repo
vectors), **measured** (golden or recorded vectors in this repo), **unconfirmed**
(no evidence yet; the code labels it). Decompiled C for Kontakt 8 was not
available (`source/kontakt8/READY` absent), and no Kontakt install exists in the
local Wine prefix (`~/.wine/drive_c/Program Files/Native Instruments` holds only
Native Access), so the Wine oracle could not measure anything for this unit.

## Universal structure

| Family | Module | Core | Stages used |
| --- | --- | --- | --- |
| `color` (`sampler-core/src/dsp/color`, IR `fx/color.rs`) | Lo-Fi | `lofi.rs` | sample hold → bit quantizer → LCG noise |
| `color` | Bite | `bite.rs` | TPT SVF pre/post, resampling clock, log quantizer, dither, HPF |
| `color` | Tape Saturator | `tape.rs` | oversampled table shaper (`tables/tape.f32le`), bell/shelf, roll-off, HPF/LPF |
| `timemod` (`dsp/timemod`, IR `fx/timemod.rs`) | Wow / Flutter | `wowflutter.rs` | modulated fractional read of a ring (sine wow + flutter) |

Lo-Fi, Bite and Tape are presets of the color superset `Config`; Wow / Flutter is a
`Native` line variant of the delay family. Lo-Fi, Bite and Tape do not yet share a
delay stage with Wow / Flutter, so a full "tape" chain (delay + filter + nonlinearity)
is not one config. Merging the two families is the next migration step, not done here.

## Modules

### Lo-Fi (0x20, `FFBFF`: Bits, Frequency, flag, Noise level, Noise color)
- Bits → `Q = 2^max(1, 16b)`, truncation toward zero. **measured** (`lofi.rs` tests).
- Frequency → hold period `max(1, Fs / max(50, Fs·r))`, capture when `phase >= period`. **measured** (unit test `hold_has_p_plus_one_cadence`); from the earlier wave-3 spec.
- Noise: shared LCG, gain `level·0.075`, one-pole colour `alpha = 1 - 0.9·color`, level law `pow(3)`. **measured** (golden sample test).
- Inactive thresholds `0.9999899864` (Frequency, Bits) and level `< 0.001`. **measured** (tests).
- Flag (`+0x1f4` P truncation): **unconfirmed**, not used by the mapping (the code says so).
- Reset seed: **unconfirmed**. The code uses a non-native default seed (`0x9E3779B9`) when no state exists.

### Bite (0x66)
- Core and constants from the earlier kcolor model; code claims max error 1e-6 vs the emulated core. **measured** against that model, not against Kontakt output.
- Knobs are fixed constants; the native 20 ms control ramps are not modelled. **unconfirmed** (documented in `bite.rs`).
- Saved-field binding (`scatter` in `effects/mod.rs`): **measured** by `bite_record_fields_land_on_their_core_indices`.

### Tape Saturator (0x42)
- Gain, warmth, roll-off, quality (×4 oversampling at Quality ≥ 0.5). Table of 4098 f32 entries recovered earlier. **measured** (`matches_the_native_core` in `tape.rs`), **unconfirmed** against Kontakt output.

### Wow / Flutter (0x6A, version 0x10)
Saved fields (bindings in `docs/re/kontakt-bindings/fx_bite_dirt_freak_wowflutter_twindelay.tsv`, **documented**): field 0 Wow, 1 Flutter, 2 Scrape, 3 Speed, 4 Stereo, 5 Age, 6 Saturation (f32, identity law), 7 Gate (bool), 8 Mix (f32, identity law). All are `0..1` in the bindings' sample points.

| Parameter | Status | Where |
| --- | --- | --- |
| Wow, Flutter, Speed, Stereo | Mapped to a **generic** law (`wowflutter.rs`), reported `UnknownLaw` | `effects/timemod.rs` |
| Mix | Mapped as the line's wet gain | `effects/timemod.rs` |
| Scrape, Age, Saturation | Not modelled; reported `UnknownLawOmitted` when non-zero | `effects/timemod.rs` |
| Gate | Not modelled; reported `UnknownLawOmitted` when on | `effects/timemod.rs` |

Generic law (**unconfirmed**, labelled in code): base delay 8 ms; wow peak pitch
deviation 0.5 % at 0.5–2 Hz (set by Speed); flutter peak 0.2 % at 6–12 Hz; excursion
`peak / (2π f)` seconds; right channel phase `Stereo / 2` cycles. The constants are not
Kontakt values. Confirming them needs the decompiled core or Kontakt vectors.

## Decompile check (Kontakt 8, `source/kontakt8`, READY)

- **Wow / Flutter reader** `FUN_140ac65f0` (version 0x10 only): reads seven f32 into object offsets 0x08..0x20 (wow, flutter, scrape, speed, stereo, age, saturation), then one byte to 0x28 (gate), then one f32 to 0x24 (mix). **Confirmed** field order and types; the mapping in `effects/timemod.rs` (seven floats, flag, mix) matches.
- **Parameter ids** `FUN_140a821c0`: `wowflutter_wow` 0x126, `_flutter` 0x127, `_scrape` 0x128, `_speed` 0x129, `_stereo` 0x12a, `_age` 299, `_saturation` 300, `_mix` 0x12d. **Confirmed.**
- **Core constructor** `FUN_140a842b0` (`ModulatableDSPCoreWrapper<BEffectWowFlutterTraits,0>`): six two-channel state objects (buffers at +0xa0..+0xaf) and ten control targets (`vtbl+0x60` over `this+0x1e4`). Generic base `BEffectBase` buffers `FUN_140a856c0` size by sample rate. **Confirmed structure, not yet the per-sample laws.**
- **Per-sample wow/flutter kernel: NOT located.** The laws in `wowflutter.rs` (8 ms base, 0.5 % wow, 0.2 % flutter, 0.5-2 Hz / 6-12 Hz, Stereo/2 phase) remain **unconfirmed**. They are not taken from this decompile.
- Scrape, age, saturation and gate laws: **unconfirmed** (no kernel located).
- **Wrapper vtables.** `ModulatableDSPCoreWrapper<BEffectWowFlutterTraits>` = `0x144fc2d38` (written by `FUN_140a842b0` and `FUN_140a87d20`). `BEffectWowFlutter` = `0x144fc2e48` (written by ctor `FUN_140a87420`, which calls the core ctor at offset 0). Its first 16 slots match the wrapper's. Slot targets are all small except `FUN_140a9f850` (1647 bytes), which is a per-block peak tracker (`[+0x3500]`, `[+0x3510]`), not the wow kernel. `FUN_140ac21f0` (1777 bytes) is a block loop with a slewed gain (`+0x1ac`, target `+0x1a4`) over 4096-byte-stride buffers; **not identified** as the wow kernel. Next step: the wow kernel is reached through `FUN_140ac21f0`'s callees or a per-sample trait method outside the 16 shared slots.
- **Vtable slots 16–32 ruled out (2026-10-23).** `0x144fc2e48` has 31 function slots (slot 32 is the first non-slot word, `0x140ad1c50`). Read from the decompile cache, none calls sin/cos or a tap read, so none is the wow kernel:
  - `0x14050f330` (slots 16, 17, 24, 30, 31): 3-byte stubs (ret).
  - `0x1408802a0` (18), `0x14051b400` (15), `0x140abd780` (25), `0x140abd6e0` (14), `0x140abd670` (13), `0x140abd610` (12), `0x140885be0` (10): 7–13-byte forwarders or setters.
  - `0x140abca00` (20, 123 B), `0x140abc920`-family: per-bypass/peak-hold `+0x3514`/`+0x3510` latch.
  - `0x140a98820` (21, 42 B), `0x140acc440` (22, 29 B): state/begin-latch (`+0x3518`).
  - `0x140a93ae0` (26), `0x140a94970` (27): 25-byte setters.
  - `0x140abc820` (29, 253 B): eight `+0x100` virtual set-parameter calls (`set_param(i, …)` for slots 0–8), not a process.
  - `0x140ad1c50` (32, 101 B): clamped control store into `+0x351c[id]` plus `FUN_14199dd80` (ramp target).
  - `0x1404f99a0` (19, 20) is `_guard_check_icall`.
  - Also excluded: `0x140a99b10` (1, 181 B: resets `+0x6a0/+0x6a2` and calls `+0x100` for slots 0–9), `0x140a94bf0` (3, 238 B: copies a param block and calls `+0x100`).
  Conclusion: the BEffectWowFlutter vtable has no per-sample process entry. The kernel is driven from a parent (BEffectBase or the block loop) through a non-vtable direct call. Next step: find the direct callers of `FUN_140ac21f0`, and of the `+0x100` setter with id 0x6A, rather than more slots.
- **Level bound (no gain in the law).** The shipped law is `out = wet * y + dry * x` with `dry = 1 - mix` and `y` a linear interpolation of earlier inputs. Its peak cannot exceed the input peak at any mix or depth (test `wowflutter::tests::wow_flutter_adds_no_gain_at_any_mix`). A +23 dB insert gain therefore comes from downstream stages, not the Wow law. For Conflux `Ray Line`, that stage is the Gainer (0x13) after it. It takes its gain from the saved footer knob (`make_persistent($Footer__Volume)`, then `set_engine_par($ENGINE_PAR_GN_GAIN, $Footer__Volume, …)`) and its output from script writes (`INSERT_EFFECT_OUTPUT_GAIN`). The gain law (output-gain cubic, unity 396851) is assumed, and the Gainer gain is unprobed (`KE070` in `audit-probes.tsv`).

## Gap table

| Item | Shared engine | Kontakt 8 | Gap |
| --- | --- | --- | --- |
| Lo-Fi hold, quantizer, noise | covered, measured | mapped | flag (P truncation), reset seed |
| Bite | covered, model-checked | mapped | 20 ms control ramps, Kontakt output vectors |
| Tape Saturator | covered, table recovered | mapped | Kontakt output vectors |
| Wow / Flutter parameters | bindings measured | mapped | scrape, age, saturation, gate laws |
| Wow / Flutter DSP | generic only | unrecovered | exact pitch law, Scrape/Age/Saturation/Gate |
| Wine oracle measurement | none | no Kontakt install in the Wine prefix | everything measured against Kontakt |

## Tests
- `wowflutter::tests::zero_depth_is_the_base_delay` (sampler-core): zero depth is a pure 384-frame delay at 48 kHz.
- `wowflutter::tests::wow_flutter_adds_no_gain_at_any_mix` (sampler-core): peak stays at or below the input peak for mix 0, 0.5 and 1; wet-only RMS stays within 2 % of the input.
- `effects::timemod::tests::wow_flutter_reads_its_stream_and_maps_generic_law` (sampler-kontakt): reader order, mix, and the omitted-law diagnostic.

- **Kernel located (2026-10-23):** see [k-wowflutter-kernel.md](k-wowflutter-kernel.md). Process = vtable slot 19 (`FUN_14199d080`) of `WowFlutter::vftable` (`0x1476a3400`), reached from slot 7 (`FUN_14099f000`). Port still blocked on the sub-object readers and constants.

## Live control: Tape Saturator (decoded 2026-10-23, not yet a Rust lane)

Source: `source/kontakt8/0140ae0000.c`. Object fields, from the setter and ramp code:

- Setter `FUN_140ae1e10(obj, param, value)`: param 0 gain → `FUN_140ae1ee0` (dB table lookup, not decoded); param 1 warmth → `obj+0x3c = 10^(0.05·(6w−3))`, ramp target `obj+0x2240`; param 2 roll-off → `obj+0x24`/`+0x40` (law not decoded); param 3 quality → bool `obj+0x2c = 0.5 < v`. Warmth and the flags set `+0x2250 = 1`, `+0x2260 = 1`.
- **Native vector (KEMU, `tools/re/tape_live_kemu.py`):** warmth 0, 0.25, 0.5, 1 gives gain 0.70794577, 0.84139514, 1.0, 1.41253757, exact match with the law (error 0). `tape.rs` already uses the same law at init.
- **Slew (`FUN_140ae1b40`, 713 B):** four lanes, current `+0x2220..0x222c`, target `+0x2240..0x224c`, per-step increment `+0x2230..0x223c`, step counter `+0x2254` reloaded from `+0x2258`, step rate `+0x225c`. A lane snaps when its square falls under the threshold.
- Ramp length (`FUN_140ae0fc0`, init): `+0x2258 = int(T·0.03 / sr + 0.5)` clamped to ≥1, with `T = +0x2170` (init argument) and `sr = +0x2178`; `+0x225c = 0.025 / (N·sr)`. Only the callers of `FUN_140ae0fc0` supply `T`, and those are not traced yet.

**Blocker for the live lane:** a moving-control vector needs `T` (the init argument's native caller) and the process loop that consumes the four slew lanes. Without them the ramp curve is a guess, so no Rust lane or parity row is claimed. Next step: trace the callers of `FUN_140ae0fc0`, then run `FUN_140ae1b40` under KEMU for a block schedule.

Bite (0x66) and the Bus Comp and Raum trailing items were not started in this pass.
