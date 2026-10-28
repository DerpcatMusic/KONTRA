# Engine unit: dynamics compressors (Kontakt 8)

Scope: Kontakt Compressor (0x19), Feedback Compressor (0x4c), Solid Bus Comp
(0x46), Supercharger GT (0x60). Kontakt 8 only; UVI follows later.

Status: the Kontakt 8 export is complete (`source/kontakt8/READY`, 1,113
bucket files). Constants below were re-read from the binary
(`/data/dev/workspace/re-binaries/Kontakt8.exe`, f32/f64 at the cited VA) and
the setter/core C in the export. Items marked *unverified* are still open.

Shared engine: `crates/sampler-core/src/dsp/dynamics/` (laws in `laws.rs`),
config in `crates/sampler-ir/src/fx/dynamics.rs`, format mapping in
`crates/sampler-kontakt/src/effects/dynamics.rs`.

## Kontakt Compressor (0x19)

Saved fields (`BParFXCompressor`): integer mode, threshold dB, saved ratio
(0..=1), attack ms, release ms, stereo-link byte. Dispatcher `0x1409047a0`
(selector at wrapper+0x1cc). Mode 0 → core `0x140ab2250` (Classic), mode 1 →
`0x140ab2c80` (Enhanced), mode 2 → `0x140ab35d0` (Pro), setter `0x140ab9220`.

| mode | law in engine | evidence |
|---|---|---|
| 0 Classic | trigger, hold/attack/release shaping a fixed gain `thr^(1-r)` | **verified in binary**, v3 `Detector::Trigger`. Setter: thr `2^(dB/6)` (+0x1c4), depth `2^(ln(thr)·−8.6562·(1−r)·−1/6)` (+0x2e0), attack `0.625^(1/n)` (+0x1d0), release `(1/3)^(1/n)` (+0x1d8), n = round(ms·rate/1000) ≥ 1. Core per channel: x = \|linked mean\|; if x ≤ thr or hold ≥ 0.985 the hold falls by release, else it is 1; the envelope drops to the hold at once, else rises by attack, capped at the hold; if hold < 0.97 and (env+1)/(hold+1) < 0.985 the hold restarts at env·1.0152284+0.0152284; gain `1−(1−depth)·env`. Constants 0.985@144fc3fe0, 0.97@144fc3fd8, 1.0152284@144fc4000, 0.0152284@144fc3df8. ANALOG insert 1 alone: ours 0.794 peak vs Kontakt 0.854 (oracle `analog_compressor_stage.ksp`). |
| 1 Enhanced | `KontaktEnhanced`: signed-mean detector, piecewise curve, attack recurrence, peak hold | **constants verified in binary**: `.0390524`@144fc3330, `.0004`@144fc3088, `2513.2`@144fc4bd4, `.2895294507215381`(f64)@144fc3f58, `35.566`@144fc47f4, `1778.3`@144fc4bb4, `.13`@144fc3608, `.884`@144fc3ce8, `.04`@1447148bc, `1.8`@144714980; all 8 piecewise slope/intercept endpoints present (`.46`…`.198195`, `.08962`…`.832671`) and equal to `computer.rs` table. Test `enhanced_compressor_uses_native_detector_and_integer_selector` uses computed f32 vectors, not native recordings. |
| 2 Pro | `KontaktPro`: channel max, 33-frame window, dB knee, RBJ low-pass release, 32-frame look-ahead | `laws.rs` `Pro`: floor `1e-12`@144f385a8 and 2π@144f38be0 confirmed in binary; golden-bit test only (self-generated) |

Parameter laws (Enhanced; shared constants with Pro through `Pro::new`):
threshold = `2^(dB/6)`, floor `.0004` only on the excess denominator; attack
`k = 2513.2/(rate·max(attack_ms,1))`; ratio: saved <.04 → 5, else
`1/(r+.13) − .884`; release multiplier `1 − 35.566/rate` below 50 ms, else
`1 − 1778.3/(rate·release_ms)`.

Link: Classic and Enhanced use the signed channel mean when linked; Pro
always uses the channel maximum and ignores the byte (`dynamics.rs`).

### Gap table

| item | covered by engine | missing / open |
|---|---|---|
| Enhanced detector, curve, ballistics | yes (f32 vectors) | native recording comparison; full branch constants on export |
| Pro core | yes (constants reproduced) | no native recording; window scheduling and knee confirmed only by the note |
| Classic core | law from the binary; one oracle stage peak | full-signal native recording comparison |
| Saved defaults, UI units, smoothing across parameter change | none | export: default block, param update path |
| Invalid-value handling | yes (`compressor()` rejects non-finite, ratio outside 0..=1, `attack<=0` for Enhanced) | native behaviour on those inputs |

## Feedback Compressor (0x4c, version 0x10 only)

Core setter `0x141ac56d0` (export, `0141ac0000.c:3249`) takes core indices 0–8:
0 input (`10^(2.5x−.5)`, base 10 @144714a00, 2.5@144fc40e0), 1 makeup
(`10^(2.5x−2)`, 2@144714994), 2 attack and 3 release (both `1−table(1200 −
sub/(t·fs)·86.56)`, the same `ar_kernel` table exp as `table_exp` in
`computer.rs`), 4 ratio (`x·19+1`@144fc46dc), 5 mix, and **6, 7, 8 are bool
setters** (`0x64`, `0x65`, `0x66`). Attack is `x·0.0498966+0.0001034`
(@1479ab198, @1479ab0d0) and release `x·1.15+.05` (@1479ab2b4, @1447148c0),
both matching `effects/dynamics.rs::feedback`. Table size 0x960 (2400), base
index 1200 (@144fc4b50), `86.56` (@1479ab5b0): all match `laws.rs`.

Saved fields: seven floats then three bytes (`BParFXFBComp`, read in
`crates/sampler-kontakt/src/effects/mod.rs`). Core `NI::DSP::FBComp`, process
`0x141aa80d0`, setter `0x141ac56d0`, no clamp on the effect setter
`0x1409a9980`.

| field | meaning | state |
|---|---|---|
| 0 input | drive `10^(2.5x−.5)` | mapped (`fx_dynamics_dist_tape.tsv`) |
| 1 makeup | output gain `10^(2.5x−2)` | mapped |
| 2 attack | `x·0.04990 + 0.000103` seconds (constants `0x3d4c6061`, `0x38d8d874`) | mapped |
| 3 release | `x·1.15 + 0.05` seconds (constant `0x3f933334`) | mapped |
| 4 ratio | `1 + 19x` | mapped |
| 5 mix | wet share | mapped |
| 6 float | no measured binding; core index 6 is a bool, so the float has no core input | **unmodelled, not diagnosed** *(see below)* |
| byte 7 | high quality: 4 sub-steps instead of 2 | mapped |
| byte 8 | link (`Max` or `Off`) | *inferred from layout* |
| byte 9 | side-chain | *inferred*; true → `NotModeled` |

Engine: `KontaktFeedback { native, high_quality }`, detector `HighPassed`,
loop `FeedbackLoop<2|4>`, output `FeedbackOut`. Table and coefficients tested
against native values (`tests.rs`, `feedback` cases).

Gap: field 6 has no core input (core 6 is a bool) and is not reported; this
is still open. Link and side-chain bytes remain inferred: the effect setter
`0x1409a9980` only forwards `(index, value)` and its callers are reached
through the vtable, so the byte→core-index map is not traced yet.

## Solid Bus Comp (0x46, versions 0x10–0x12)

Saved fields: six floats (threshold, makeup, attack, release, ratio, mix),
link byte, side-chain byte (v≥0x11), trailing float (v0x12). Reader in
`effects/mod.rs`. Core `G_BusComp` (`0x141aaa1e0`); attack/release
coefficients via setter `0x141ac69e0` and helper `0x141acc420`.

Verified (repo evidence, `docs/architecture-v2/KONTAKT_DSP_LAWS.md`):
- attack enum to ms: [.1, .3, 1, 3, 10, 30]; release enum to ms: values
  [.2, .3, .6, .8, 1.6, 1.2]·62.5 = [12.5, 18.75, 37.5, 50, 100, 75]; enum 5
  sets the auto-release flag.
- coefficient `min(1, exp(ln(.8)/(ms·.001·Fs)))`, checked at 5 rates.
- Solid Bus Comp table knots (`BusComp:0`, `BusComp:1`) for the spline in
  `dsp/eq_family/solid`.

Status this pass: core entry `0x141aaa1e0` (`0141aa0000.c:5701`) located; its
block loop was read but the threshold/ratio/envelope formulas were **not**
re-verified. Still *unverified*.

Engine: `KontaktBusComp { threshold, makeup, attack, release, ratio, link }`,
`Bus::new`, `BusRatio`, `BusEnvelope` (with auto release), `BusOut`. The
mapping of saved floats to core control ids is **inferred** from core ids,
not traced.

### Gap table

| item | covered | missing |
|---|---|---|
| attack/release timing | yes (coefficient law) | none measured against native |
| threshold, makeup index binding | inferred order | trace binding in export |
| ratio law | `BusRatio` | verify against export |
| auto release state machine | `BusEnvelope<true>` | verify against export |
| link and side-chain | link yes | side-chain → `NotModeled` (`Err`) |
| v0x12 trailing float | parsed | **now diagnosed** (note `solid bus comp trailing field`); processor still runs |
| smoothing, reset, latency, tail | none documented | export |

## Solid Bus Comp trailing field: evidence and blocker (2026-10-23)

- **Record offset.** The v0x12 reader `FUN_140d02850` (`0140d00000.c:1726`) stores the trailing float at record `+0x24`, after the link and side-chain bytes at `+0x20`/`+0x21`. Earlier versions stop at `+0x21` (v0x11) or `+0x1c` (v0x10).
- **Setter.** The effect setter `FUN_1409a9d40` (`01409a0000.c:7961`, 60 bytes) calls the core's dirty-mark `FUN_141ac69e0(this+0x1c0)`, then stores the raw float at `this + 0x2a18 + idx*4`. It is a generic per-control store, not a Bus Comp field. The generic getter `0140990000.c:1496` reads the same array.
- **Consumer not found.** The core `FUN_141aaa1e0` (`0141aa0000.c:5701`) reads its own fields (`+0x19e1` link byte, `+0x0c`/`+0x10` channel counts), not `this+0x2a18`. No direct caller of `FUN_1409a9d40` exists. It is reached only through an effect vtable.
- **Blocker.** The trailing field's `idx` is set by the apply code that calls the vtable slot. That code is not located. Two whole-program searches timed out at 550-600 s (`RefsTo` on `0x1409a9d40`, `DispScan` on `0x2a18`). A targeted search needs the vtable of `BEffectSSLGBusComp` (its constructor is near `01407f0000.c:2856`) and a `DumpVtable` of that vtable to find the slot that targets `0x1409a9d40`. Then the apply that writes slot `idx` from `record+0x24` is the next read.
- **Status.** Still `NotModeled`. The processor runs without it. No law is added.

## Supercharger GT (0x60, version 0x51)

Saved fields are known from the binding table (`fx_other.tsv`): trim, saturation,
compress, character, attack, mix, release (`ENGINE_PAR_SUPERGT_*`), and
the struct `BParFXSupercharger` (`ser_classes.tsv`).

**New (export):** the effect object `BEffectSupercharger` constructor
`0x1407f3d70` (`01407f0000.c:2861`) owns a `Rockruepel_CompXTWrapper` at
+0x38 whose core is `NI::DSP::CSL::Rockruepel_CompXT` (constructor
`0x141a820e0`, `0141a80000.c:893`, ~577 lines). So the Supercharger's
dynamics is the CompXT compressor core, not a separate law. It also sets
core parameters 0x2a, 0x2b, 0x7, 0x8, 0x9, 0x2e, 0x30, 0x33, 0x36, 0x16 at
construction. `set_engine_par` validates the Supercharger modes: saturation
(internal id 0x3a9), character (0x3aa) and channel link (0x3ab), each 0..=2
(`0140780000.c:6079`–`6096`, validator `0x140789c30`).

Native static vectors (Wine oracle, `docs/re/engine/vectors/supercharger-static.tsv`,
200 Hz ramp, 14 steps from INIT): the default state applies a constant
×1.0716 gain (+0.6 dB) at every input level 0.2–1.0. SATURATION=1.0 gives
level-dependent reduction above ~0.6 (×1.0045 at 0.6, ×1.0315 at 1.0), and
COMPRESS=0.5 and CHARACTER=0.5/1.0 reduce only the top level (×1.067–1.063).
TRIM 0/0.5/1 had no measurable effect. MIX=0 and MIX=1 both give ×1.0 (dry
level) at all levels, which does not fit a plain wet/dry blend of the default
state; the MIX result is unexplained.

The per-field transfer, the CompXT detector/ballistics and the mapping from
saved fields to CompXT indices are **not** yet recovered. `KONTRAKT_DSP_LAWS.md`
lists Supercharger as "no verified law"; this still holds.

Current behaviour: no processor. The effect raises a `NotModeled` note
(`effects/mod.rs` `!modelled` branch), now with the explicit reason from
`dynamics::unproved(0x60)`, so it is never silently passed through.

### Gap table

| item | covered | missing |
|---|---|---|
| parameter names and saved order | yes (bindings) | variant identity (what 0x51 selects) |
| core | CompXT located (`0x141a820e0`, setter not yet read) | CompXT process, detector and gain law; saved-field→core-index map |
| saturation / character / link modes | validator ranges 0..=2 (`0x140789c30`) | meaning of each mode |
| transfer, detector, timing | none | the whole processing law |
| reset, latency, tail | none | export |

## Engine changes in this unit

- `dynamics::unproved(0x60)` gives the Supercharger GT a specific diagnostic.
  Test: `effects::tests::supercharger_is_diagnosed_not_silently_passed`.
- Solid Bus Comp v0x12 trailing float: a `DynamicsNoted` note is raised
  while the processor still runs. Test: `limiter_tests` in `effects/dynamics.rs`.
- No DSP law was added. Nothing in this unit is verified against the export yet.

## Next steps (when `READY` exists)

1. Trace Feedback field 6 and bytes 8/9 in `FUN_140d...` reader and the
   `0x1409a9980` setter. Add the binding or keep the diagnostic.
2. Confirm Solid Bus Comp control indices, ratio law, auto release and
   smoothing from `0x141aaa1e0`.
3. Recover the Supercharger variant selector and transfer from the 0x60
   object and its core. No law without it.
4. Check Classic against the native recording path.

## CompXT core parameter map (Supercharger GT, 2026-10-22)

Source: `NI::DSP::CSL::Rockruepel_CompXT` parameter setter `0x141ac7450`
(`0141ac0000.c:4308`, 4,714 bytes, index guard `> 0x58` returns). Each case
stores the value in a `double` slot at `core+8+index*8` and recomputes the
derived state. Only the dispatch is recorded here; the derived formulas
for each case and the process function are not yet recovered.

| index | conversion (as in the setter) | derived state |
|---|---|---|
| 0 | `FUN_141a98340` (spline evaluator, same as Solid EQ/Bus tables) | `+0x6f8` (and `+0x770` when flag `+0x680`) |
| 1 | spline × `DAT_1479ab4b0` | `+0x708` |
| 2 | `pow(DAT_1447149c8, spline × DAT_144f388e0)` | `+0x710` |
| 3, 4 | block-level float index (`×DAT_1476989a0`) into table `DAT_1478fa900` | interpolated |
| 5, 6 | spline into `+0x920` loop | — |
| 7, 8, 9, 0xa, 0x16 | bool switches (`x > 0.0`) | `+0x2f2`–`+0x2f6`, `+0x7f0`–`+0x7f7` |
| 0xb, 0xc, 0xf | additive spline, cast to int | — |
| 0xd, 0xe, 0x13, 0x14, 0x15, 0x19, 0x1a–0x1d, 0x29 | `pow(2^…)`-style dB/time conversions | `+0x718`, `+0x720`, `+0x6c8`, `+0x6d0`, `+0x700`, `+0x728`–`+0x17f0` |
| 0x10, 0x11, 0x12 | spline; 0x12 is `1 − x` | `+0x740`, `+0x748` |
| 0x20, 0x23, 0x26, 0x28 | three-value spline groups | `+0x500`, `+0x560`, `+0x5c0`, `+0x620` |
| 0x2a, 0x2b | spline | `+0xdd8`/`+0x1e98`, `+0xdd0`/`+0x1e90` |
| 0x2c, 0x31, 0x34 | scaled spline | `+0x52e0`, `+0x52f0`, `+0x52f8` |
| 0x2e, 0x30, 0x33, 0x36 | bool (`x ≥ 0.5`) → `FUN_141ab9560`/`9cd0`/`9140` | `+0x2f3`–`+0x2f6` |

Unrecovered: the process function (the CompXT vtable entry is not yet
located), each derived-state formula, the saved-field→index map for
Supercharger, and the meaning of each index. Supercharger GT therefore stays
`missing` in the parity matrix.

## CompXT signal path (partial, 2026-10-22)

Recovered structure (addresses are the Kontakt 8.13 binary; C in the export):

- Block process: vtable slot 7 of the CompXT wrapper, `0x141aaa9e0`
  (`0141aa0000.c:5955`, 685 bytes). Per frame it: reads the two channel
  inputs, optionally replaces channel 0 by the mean of the last two
  channels when the link flag `+0x2f1` is set (×0.5 at `DAT_14470bad0`),
  sets trigger flags `+0x6d/+0x74/+0xa60` (and `+0x7b/+0x82/+0x5301`) when
  the level exceeds `DAT_1479ab288`, calls the per-frame core
  `0x141a95f00` through vtable slot 18, and writes the two outputs back.
- Per-frame core `0x141a95f00` (`0141a90000.c:3161`, 1,431 bytes): a
  peak/power detector with two one-pole followers per path. Each follower is
  `s = (s − x)·c + x`, where `c` is the attack coefficient when `x` exceeds
  the state and the release coefficient otherwise (state pairs at
  `+0x340/+0x358/+0x360`, `+0x378/+0x390/+0x398`, `+0x3b0/+0x3c8/+0x3d0`,
  `+0x3e8…`, `+0x420/+0x438/+0x440`, `+0x458/+0x470/+0x478`,
  `+0x490/+0x4a8/+0x4b0`). Coefficients are set by the prepare routine
  `0x141aa2400` (`exp(−1/(t·Fs))`, `DAT_14470bb18`, `DAT_144714908`).
- Look-ahead window: `FUN_141a964a0` (`0141a90000.c:3381`, 3,392 bytes),
  a sliding window with a decay state per channel (`+0x500`, `+0x15c0`,
  coefficients `+0x2d0`, `+0x2d8`). Same family as the Kontakt Pro "33
  Windowed" stage; not yet compared sample-for-sample.
- Gain smoother: a one-pole on the detected level at `+0x4c0` with attack
  `+0x4d8` and release `+0x4e0`.
- Parameter setter `0x141ac7450` (index guard `> 0x58`); the per-index
  table in the section above.

Still unrecovered: the gain-computer law (the mapping from detected level
to gain, which the one-pole smoother acts on), `FUN_141a964a0`'s
coefficient and reset rules, how the effect's saved fields and the
engine-parameter ids (`0x3a8`…`0x3ab` saturation/character/link modes) reach
the setter indices, and the saturation and character transfer curves.
No Rust port has been written for CompXT yet.

## CompXT per-frame core and shared detector family (2026-10-22, export re-read)

Read from the export only (no binary session, no build). Status: structure
recovered, constants and the gain computer still open.

- Per-frame core `0x141a95f00` (`0141a90000.c:3161`, 1,431 bytes). Input is the
  two channels converted to mid and side (sum and difference, each scaled by
  the constant at `DAT_14470bae8`). Each path runs a squared-level follower
  whose attack/release pair is picked by comparing the new square with the
  state (states `+0x340/+0x358/+0x360`, `+0x378/+0x390/+0x398`,
  `+0x3b0/+0x3c8/+0x3d0`, `+0x3e8/+0x400/+0x408`). The mid and side signals
  then pass two look-ahead windows, `FUN_141a964a0` on `+0x500` and on
  `+0x15c0` (coefficients `+0x2d0` and `+0x2d8`). The outputs recombine as
  `left = wetMid + wetSide`, `right = wetMid − wetSide`. A second pair of
  followers (`+0x420/+0x438/+0x440`, `+0x458/+0x470/+0x478`) and the output
  one-pole on `+0x490/+0x4a8/+0x4b0` read the recombined levels.
- The `+0x2680 != 0` or `+0x2f0 != 0` branch of the same function instead runs
  the squared-level followers on the pre-gain `+0x6d8` input and calls the
  objects at `+0x2f8` and `+0x318` (vtable slot `0x18`). Those objects are not
  traced. Mode meaning: open.
- Shared detector `0x141a95cb0` (`0141a90000.c:3049`, 578 bytes) is called from
  the Solid Bus Comp core `0x141aaa1e0` (`0141aa0000.c:5701`, two call sites
  in that file). The export shows no other caller. So Solid Bus Comp's detector
  is this CompXT-family routine, not a separate law. Its state init writes
  `100.0` at `+0x18d8`. The CompXT block process `0x141aaa9e0` (`0141aa0000.c:5955`)
  does not call it.
- Solid Bus Comp block loop (`0x141aaa1e0`): when the link byte at `+0x19e1`
  is clear, the detector gets the per-channel samples. When set, it gets the
  scaled sum of the last two channels (scale `DAT_14470bad0`, which is 0.5).
  The gain-computer and envelope formulas called after the detector were not
  read in this pass.
- Still unrecovered: gain computer, `FUN_141a964a0` coefficients and reset,
  the `+0x2f8`/`+0x318` objects, saturation and character curves, and the
  setter-to-core-index map for saved Supercharger fields.

### Look-ahead window `FUN_141a964a0` (export, `0141a90000.c:3381`, 3,392 bytes)

Read in this pass (text only):
- Signature: `(state, sample, coefficient) → sample`. It is called on the
  mid and side samples from the per-frame core. Its output is the windowed
  signal that the core recombines.
- Bank of about 11 first-order followers on the state block (`+0x4a` …
  `+0x5d` in double units). Each updates as `y = y·a + x·b`, where the pair
  (a, b) is chosen by a fixed constant pair (`DAT_1479ab3d8`/`DAT_144f38840`
  for the first group). Any result below `DAT_144fc3270` that is also above
  `DAT_1479ab668` is flushed to 0 (denormal guard).
- Second group (`+0x43`, `+0x44`, `+0x3f`, `+0x41`, `+0x39`, and the output
  states `+0x4e`, `+0x50`, `+0x51`, `+0x54`, `+0x55`): each picks one of two
  coefficient pairs by comparing the new value with the state (attack when
  rising, release when falling). A flag byte at `+0x30` (bypass or reset)
  copies the input straight into the state, with no smoothing. Same flush
  rule as above.
- Constants (one-shot pefile read of the image, f64):
  `DAT_1479ab3d8` = 0.995, `DAT_144f38840` = 0.005 (the pair for the fast
  group: falling state `= input·0.005 + state·0.995`); `DAT_1479ab3e0` =
  0.9995 and `DAT_144fc3b88` = 0.0005 (rising state `= state·0.9995 +
  input·0.0005`); `DAT_144fc3ff0` = 0.9999 and `DAT_144737738` = 0.0001 (the
  slowest pair, used by the output states); `DAT_144fc3270` = +1e-15 and
  `DAT_1479ab668` = −1e-15 (a state with |y| < 1e-15 is set to 0); `DAT_144714988`
  = 1.0.
- Coefficient pairs are as read in the second group: a value at or below the
  state takes the release pair, a value above it takes the attack pair.
- Not yet read: the tail of the function (after `LAB_141a96ad4`, the output
  tap combination), and so the window length and the exact gain-computer law
  it implies. This is the open part of the CompXT gain computer.
