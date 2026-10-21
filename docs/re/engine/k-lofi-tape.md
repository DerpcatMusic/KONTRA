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
- `effects::timemod::tests::wow_flutter_reads_its_stream_and_maps_generic_law` (sampler-kontakt): reader order, mix, and the omitted-law diagnostic.
