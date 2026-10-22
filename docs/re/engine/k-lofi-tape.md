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
- Live lanes (2026-10-23): the ten continuous `ENGINE_PAR_BITE_*` rows (identity law, `BITE_LIVE` in `sampler-ir/src/fx/color.rs`) are script-addressable. Each block rebuilds the engine from the block-start values (`Curve::LiveBite`). DC quant (bool) and HPF (enum) stay stored, as Freak's switches do.

#### Native control callbacks (decompile, `0141930000.c`, Kontakt 8)

The Bite constructor `FUN_141935890` registers twelve `std::function` control callbacks. Their vtable slot 2 bodies (`FUN_141936200`..`FUN_141936730`, vtables `0x1476973c0`..`0x1476975f0`) map each control to a core field (`lVar1` = core object):

| Field | Control (from callback) | Native write |
|---|---|---|
| `+0x3a4` | Bits | `FUN_141936610`: `exp2(1 - nb)`, `nb = 2 - int(x·-15)` clamped 2..16, same as `Engine::new` |
| `+0x3ac` | DC quant | `FUN_141936710`: bool `x >= 0.5` |
| `+0x3b8` | HPF | `FUN_1419363d0`: 5 / 100 / 200 Hz by `int(3x)` clamped 0..2; writes the HP smoother at `+0x980` |
| `+0x54` | Saturate | `FUN_1419364e0`: soft `0.2x + 0.05`, trim table at `1200 − 79.73x` |
| `+0x64` | Crunch | `FUN_141936600` → `FUN_1419390d0`: drive in/out (`+0x740/+0x744`, `24·saturate` term) |
| (pitch) | Frequency | `FUN_1419365d0` → `FUN_141936c40` (not read; presumed to set pitch `+0x728`, which the pre/post curve uses) |
| `+0x60` | Dither | `FUN_1419365e0`: `+0x734 = x²` |
| `+0x6c` | Jitter | `FUN_1419366f0`: `+0x730 = x²` |
| `+0x74` | Expand | `FUN_141936730`: `c = 2^(8 − 7.9x) − 1` (`+0x758..0x760`) |
| `+0x80` | Mix | `FUN_141936490`: wet/dry at `+0x74c/+0x750` |
| `+0x58`, `+0x78` | Pre / post filter | `FUN_141936870` (pitch-dependent log curve, pitch at `+0x728`) → `FUN_141938dc0` |

`FUN_141938dc0` is the TPT coefficient setter for one SVF pair: `g = tan3(π·f·(1/sr))` (the `tan3` of `bite.rs`, constant `π` at `0x1447149d4`), `a1 = 1/((g + k)·g + 1)` with damping `k` at `+0x58`/`+0x5c` (0.7654, 1.8478), `a2 = a1·g`. It writes the coefficients **immediately**. It also sets the target (`+0x48`, `+0x50`) and the current value (`+0x4c`, `+0x54`) to the same number, so no ramp is started here.

Open: the **20 ms ramp** is not in these callbacks. It must be in the process `0x141937fb0` or a helper that moves `+0x4c` toward `+0x48`. Not traced, so the Rust live path rebuilds the engine per block with no ramp. Kontakt output vectors are still missing.

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
| Bite | covered, model-checked; live lanes wired (10 continuous) | mapped | 20 ms ramp (not in the control callbacks; process `0x141937fb0` untraced), DC/HPF live switches, Kontakt output vectors |
| Tape Saturator | covered, table recovered | mapped | Kontakt output vectors |
| Tape live slew | tick law verified under KEMU, Rust `Slew` (not wired) | tick cadence decoded (every 32 host frames, `FUN_1408fc360`), per-frame lane use decoded | host warmth → per-tick flag/array path (`FUN_14199dd80`), wrapper KEMU run, live Rust lane |
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
- Init `FUN_140ae0fc0(obj, T, sr)` (decompiled, constants read from `.rdata`): `+0x2170 = T`, `+0x2174 = 1/T`, `+0x2178 = sr`. `T` is compared with 50 kHz and 100 kHz (`DAT_144f38d5c`, `DAT_1447377cc`) to pick the oversampling (`+0x2160`, `+0x2164`), so **T is the processing rate (host rate times oversampling), not a ramp length**.
- Step count: `+0x2258 = max(1, int(0.03·T/sr + 0.5))` (`DAT_144fcdc04 = 0.03`, `DAT_14470bad0 = 0.5`). For any real T/sr below about 16.7 this is 1, so the warmth ramp is one tick.
- Slew rate: `+0x225c = 1/(N·sr)` with `N = +0x2258` (`DAT_14470bad4 = 1.0`). The earlier note `0.025/(N·sr)` was wrong. The constant 0.025 (`DAT_144f38678`) feeds the `+0x1458` field, not the slew.
- **Tick (`FUN_140ae1b40`, group B) vectors, KEMU (`tools/re/tape_slew_kemu.py`):** a pending tick sets `inc = (tgt − cur)·rate` per lane, snaps lanes with `inc² < 1e-15`, reloads the counter `+0x2254` from `+0x2258`; a non-pending tick decrements the counter and snaps `cur = tgt`, `inc = 0` at zero. Verified in `tape.rs::Slew` (`slew_tick_matches_the_native_lanes`, sampler-core).
- Per-frame advance and read (`FUN_140ae1190`, lines ~1210–1245, **decompiled, not yet run under KEMU**): the lane advance `cur += inc` runs once per **base-rate frame** (inner loop over the chunk's `uVar34` frames, not per oversampled sample; the oversampled loop feeds the resampler, bounded by `+0x2168`). Output is `out_L = cur0 · b_L + a_L`, `out_R = cur0 · b_R + a_R`, with `cur0 = +0x2220` read after the increment. This is the Rust `self.warmth * b[2] + a[2]` term, so the live lane replaces `warmth` with `cur0` per frame.
- **Tick cadence (decoded 2026-10-23).** The tape wrapper is `BEffectTape` (ctor `FUN_1407c8890`, vtable `0x144f14e40`). Its block process is **vtable slot 23 = `FUN_1408fc360`** (`01408f0000.c`). It keeps a control counter at wrapper `+0x1bc`:
  - When the counter is `< 1`, it sets it to **32** (host frames), then for params 0..4 calls the core setter (`core->vt[0x60]`, slot 12 = `FUN_140ae1e10`) only where the per-param flag `+0xb5b0 + i` is set, with the value from the per-tick array `+0xb5c0 + 4·tick` (clamped to `[0, 1]`), then calls **`core->vt[0x40]` = slot 8 = the slew tick `FUN_140ae1b40`**.
  - Then it runs `core->vt[0x38]` = slot 7 = `FUN_140ae1190` on `min(counter, remaining)` frames and subtracts that from the counter.
  - So the tick fires **once per 32 host frames** (not per sample and not per block), and the first tick happens at the first frame. The setter's per-param flags are the only way a value reaches the core from this loop.
- **Derived consequence (not yet run through the wrapper):** with `N = max(1, int(0.03·T/sr + 0.5)) = 1` for every realistic `T` (`T ≤ 4·sr`), a pending tick sets `inc = (tgt − cur)/sr` and reloads the counter to 1. The next tick snaps `cur = tgt`. So one warmth move is a **32-frame linear segment** of slope `(tgt − cur)/sr`, then a snap at the next tick. That is a very small move per segment, and it will be checked against the wrapper before any Rust lane depends on it.
- **Still open (the live-lane blocker):** how a host warmth change becomes the per-tick flag and array above. The host path is the generic store `FUN_140ad1c50(wrapper, id, value, force)` (writes `+0x351c[id]`, then `FUN_14199dd80(wrapper+0x1c0, value, clamp)`). `FUN_14199dd80` writes a per-param value array (`+0x388`, or the smoothing arrays `+0x340/+0x358/+0x370/+0x3a0` when `+0x2f8` is set), sets the dirty flag `+0x3d4` (or `+0x3d0`), and, for param 9, a flag `+0x3288`. Which of these becomes the `+0xb5b1` flag and `+0xb5c8` array of `FUN_1408fc360` is **not traced yet**. Without that link, the 32-frame cadence is confirmed, but the value curve a host sees is not.
- A KEMU end-to-end run (ctor, init, setParam, per-block) needs that link too. A harness that only sets the setter directly would skip the wrapper path, so it is not a native vector for the live curve.

**Blocker for the live Rust lane:** the host warmth → `FUN_14199dd80` → `+0xb5b1`/`+0xb5c8` → `FUN_1408fc360` chain. Next step: trace who reads `+0x3d4`/`+0x3d0` in the wrapper and what writes `+0xb5b1`, then run the wrapper under KEMU with a moving warmth to produce the vectors.

No Rust processor lane or parity row is claimed. The Tape `live control not modelled` gap stays open.

Bite (0x66) and the Bus Comp and Raum trailing items were not started in this pass.
