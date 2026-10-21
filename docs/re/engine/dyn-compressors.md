# Engine unit: dynamics compressors (Kontakt 8)

Scope: Kontakt Compressor (0x19), Feedback Compressor (0x4c), Solid Bus Comp
(0x46), Supercharger GT (0x60). Kontakt 8 only; UVI follows later.

Status at writing: the Kontakt 8 decompile export is **not ready**
(`source/kontakt8/` is empty and has no `READY` marker). Nothing below was
re-verified against that export. Every address and constant is taken from
the existing repo evidence named per row. Items marked *unverified* must be
checked against the export before they are treated as parity.

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
| 0 Classic | `KontaktClassic`: trigger, hold/attack/release shaping a fixed gain `thr^(1-ratio)` | `laws.rs` chain row "Kontakt Classic"; `docs/re/kontakt-compressor.md` calls it a proxy *(unverified)* |
| 1 Enhanced | `KontaktEnhanced`: signed-mean detector, piecewise curve, attack recurrence, peak hold | `docs/re/kontakt-compressor.md` (constants from radare2, all 32 listed); test `enhanced_compressor_uses_native_detector_and_integer_selector` (computed f32 vectors, not native recordings) |
| 2 Pro | `KontaktPro`: channel max, 33-frame window, dB knee, RBJ low-pass release, 32-frame look-ahead | `laws.rs` `Pro` constants (`0x144f385a8`, `0x144f38be0`, ...); golden-bit test only |

Parameter laws (Enhanced, from the note; shared by Pro through `Pro::new`):
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
| Classic core | config and law present | proxy status; no verified constants |
| Saved defaults, UI units, smoothing across parameter change | none | export: default block, param update path |
| Invalid-value handling | yes (`compressor()` rejects non-finite, ratio outside 0..=1, `attack<=0` for Enhanced) | native behaviour on those inputs |

## Feedback Compressor (0x4c, version 0x10 only)

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
| 6 float | no measured binding | **unmodelled, not diagnosed** *(see below)* |
| byte 7 | high quality: 4 sub-steps instead of 2 | mapped |
| byte 8 | link (`Max` or `Off`) | *inferred from layout* |
| byte 9 | side-chain | *inferred*; true → `NotModeled` |

Engine: `KontaktFeedback { native, high_quality }`, detector `HighPassed`,
loop `FeedbackLoop<2|4>`, output `FeedbackOut`. Table and coefficients tested
against native values (`tests.rs`, `feedback` cases).

Gap: field 6 has no binding and is not reported. Link and side-chain bytes are
inferred, not traced. Treat link as *unverified* until the export confirms.

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

## Supercharger GT (0x60, version 0x51)

Saved fields are known from the binding table (`fx_other.tsv`): trim, saturation,
compress, character, attack, mix, release (`ENGINE_PAR_SUPERGT_*`), and
the struct `BParFXSupercharger` (`ser_classes.tsv`). The per-field laws are
**not** known. `KONTRAKT_DSP_LAWS.md` lists Supercharger as "no verified law".

Current behaviour: no processor. The effect raises a `NotModeled` note
(`effects/mod.rs` `!modelled` branch), now with the explicit reason from
`dynamics::unproved(0x60)`, so it is never silently passed through.

### Gap table

| item | covered | missing |
|---|---|---|
| parameter names and saved order | yes (bindings) | variant identity (what 0x51 selects) |
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
