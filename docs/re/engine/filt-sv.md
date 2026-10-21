# Engine unit `filt-sv`: Kontakt 8 SV filters

Scope: saved filter types 50..=65 (SV LP1/HP1, LP2/BP2/HP2, LP4/BP4/HP4/Notch 4, LP6/BP6/HP6/Notch 6,
Par. LP/HP, Par. BP/BP, Ser. LP/HP). Kernel: `crates/sampler-core/src/dsp/filter/cascade.rs`
(`Sv`, `SvRamp`, `Cascade::sv`, `sv_g`, `sv_g_six`). Shared linear core: `dsp/filter/linear.rs`
(`Zdf`); transfer-function stages: `dsp/filter/shaper.rs` (`Knee`, `pade`). Mapping:
`crates/sampler-kontakt/src/effects/filter.rs`. Config: `crates/sampler-ir/src/fx/filter.rs`.

Evidence: the Kontakt 8 decompile (`source/kontakt8/`, READY) and the PE image
(`/data/dev/workspace/re-binaries/Kontakt8.exe`). Constants below were read from the PE image with
`tools/re/pe_consts.py`; addresses are the setter/process functions in bucket `0140b00000.c`.
Process functions: SV2 `0x140b02400`, SV4 `0x140b02790`, SV6 `0x140b02cf0`, Multi `0x140b01d00`.
Setters: SV1 `0x140b05d80`, SV2 `0x140b05ee0` (cutoff helper `0x140b05460`), SV4 `0x140b05fa0`
(cutoff `0x140b05030`), SV6 `0x140b06070` (cutoff `0x140b05160`), Multi `0x140b05d50` (shared
helper `0x140b03b50`). Object classes: `FilterSV1/2/4/6`, `FilterMultiSV` (`01407c0000.c`).

## Modules, parameters and laws

| Type | Name | Native core | Stored knobs (ENGINE_PAR index) |
|------|------|-------------|---------------------------------|
| 50 / 51 | SV LP1 / HP1 | one TPT pole, mode from param 2 | cutoff (0), mode (2); resonance has no effect |
| 52 / 53 / 54 | SV LP2 / BP2 / HP2 | one stage, mode 0 LP, 1 BP, 2 HP | cutoff (0), resonance (1) |
| 55 / 56 / 57 / 58 | SV LP4 / BP4 / HP4 / Notch 4 | two stages, mode from the type | cutoff (0), resonance (1), mode (2) |
| 59 / 60 / 61 / 62 | SV LP6 / BP6 / HP6 / Notch 6 | three stages, mode from the type | cutoff (0), resonance (1) |
| 63 / 64 / 65 | SV Par LP/HP / Par BP/BP / Ser LP/HP | two stages, mode 0 / 1 / 2 | cutoff (0), resonance (1), bandwidth (2), mode (3) |

Cutoff, shared law (`u` in 0..1): `note = u*115 + 20` (`min 160` where the setter clamps it), hertz
from the native exponential table `2^(i/60 - 20)` at index `note*5 + 1381.8816`. The table is
`ladder_kernel::exponential`. At `u = 0` this is 25.96 Hz. The stage coefficient is
`g = tan(pi f T)` by the native 4-term series (`sv_g`). Multi: `centre = u*180 - 20`, the two notes
are `centre -/+ spread`, each clamped to 160, and each `g` floored at 10 Hz.

Resonance (`r` in 0..1): `R = 1 - (1 - r)^3`; SV6 uses `R = 1 - (1 - 0.9 r)^3`. SV1 ignores it.
Damping `k = 2(1 - R)`. SV4 second section `k2 = 2(1 - 0.666 R)`.

Output gain on the selected tap:
- SV2: `1 - 0.5 R^5`.
- SV4: `1 - R c_mode`, `c = 0.77 / 0.9334 / 0.77 / 0.88` for LP / BP / HP / Notch (setter mode
  index 0..3 at +0x48).
- SV6: `1 - s`, with `s = (1.2125 - 0.2425 |R|) R` for LP and HP, `s = (2 - R) R 0.991` for BP,
  `s = (1.482 - 0.494 |R|) R` for Notch.
- Multi: serial `(1 - 0.5 R^5)^2`; parallel `0.7 - 0.49 R^5` (constants not yet read, see G9).

Stage state: `[h, b, l]`. Band and low pass through a knee: clamp to `+-2L`, then
`c (1 - 0.25 |c| / L)`. SV4 and SV6 use the form `(1 - |c| (0.25 / L)) c`; SV2 and Multi use
`(1 - (|c| 0.25) (1 / L)) c` (native order, confirmed by the 0x1447148e4 = 0.25 read and the
`0x140b02790` stores of `0.25/l` and `2l`).

State limit `l` (the stored `l` per stage): SV2 `600 - 576 n`, SV4 `1600 - (2346 - 782 |n|) n` with
`n = min(note, 134) / 134`, SV6 `1600 - (2 - n) n 1576` with `n = min(note, 128) / 128`. Multi
`400 - 376 n` with `n = clamp(note, 0, 134) / 134`. Only SV4 and SV6 values are confirmed from the
binary; SV2 and Multi `l` are from the existing kernel (see G8).

Ramps: a changed target ramps linearly over `round(rate * ramp / 32)` blocks (8 ms for SV4,
2 ms otherwise). A step whose square is below 1e-15 snaps.

Constants read from the PE image (all confirmed): 115, 20, 160, 134, 128, 180, 5, 1381.8816,
2346, 782, 1600, 1576, 0.00746268639, 0.0078125, 0.9, 0.666, 0.25, 0.5, 2, 3, 0.77, 0.9334, 0.88,
0.494, 1.482, 0.2425, 1.2125, 0.991, 31.5, 0.5, 12.987881, 3.2898684, 1, pi.

Stereo: channels are independent (measured: no L to R coupling).

## Gap table

| # | Item | Status | Evidence / reason |
|---|------|--------|-------------------|
| G1 | Cutoff law, u >= .5 | **closed** | Native law reproduces the measured corners to 0.2 % and the vectors at u = .5 and .9 to 1e-6 |
| G2 | Cutoff law, u <= .25 | **closed for SV1, SV4, SV6, Multi; open for SV2** | The PE constants give exactly 25.96 Hz at u = 0. The earlier 26.4 Hz and 46.9 Hz values were fit artefacts of `filter-laws.tsv` / `filter-responses.tsv`. SV2 stores a different index (`1200 + 119.589 u` into `2^(i/60 - 20)` at `0x140b05460`) and its process transform (`0x140b02400`, 892 bytes) is not traced. The vectors at u = .5 and .9 agree with our law, but u <= .25 is unverified for SV2 |
| G3 | Resonance law res <= .5 | **closed** | Native vectors (1e-6) and the decompiled constants above |
| G4 | Resonance law res .9 | **open** (kernel confirmed, level not matched) | The SV2 res .9 vectors (u .9) match the native kernel to 1e-6. SV4 and SV6 at res .9 use the same confirmed constants but have no native vectors. The measured peaks (LP4 +53.7 dB, LP6 +48.2 dB, BP4 +41.8 dB, BP2 +49.9 dB) depend on the test signal level, which `filter-responses.tsv` does not record. Next step: a kemu SV driver (none in the repo) to dump res .9 SV4/SV6 vectors |
| G5 | Self-oscillation stability | **covered** | Test `sv_classes_stay_bounded_at_full_resonance`: all five classes, cutoff 0 and 1, res 1, +-4 square, 48000 frames stay finite with peak < 1e4 |
| G6 | Saturation (knee) constants | **closed** | `0.25`, `2L` and the SV2 / SV4 operation orders confirmed from `0x140b02790`, `0x1447148e4` and the setter stores |
| G7 | MultiSV bandwidth | **identified; record path partly traced** | Core ENGINE_PAR index 2 (at +0x28) is read by the shared helper `0x140b03b50` as `spread = 31.5 b + 0.5` (constants 31.5, 0.5, and the centre 180, 20 confirmed). Mode is index 3, set by `0x140b048a0` from `round(2 v)` clamped 0..2. The record reader `0x140d00db0` writes the value at reader offset +0x24 for these types, but the copy from that record field to core index 2 is not traced. Our mapping (record lead value, clamped 0..1) is vector-verified at bandwidth 0.3 only |
| G8 | SV2 and Multi state limit `l` | **open** | Not confirmed from the binary (only SV4 and SV6 `l` are) |
| G9 | Multi serial/parallel gain constants (0.7, 0.49) and the Multi `w = 0.5 - 0.1667 min(spread, 3)` mix | **open** | The Multi process was only partly traced (`0x140b01d00`) |
| G10 | Defaults of each knob | open | Needs the group/insert default table (`KONTAKT_VOICE_GROUP_DEFAULTS.md`) |
| G11 | Config type `response` for Multi | cosmetic | Par BP/BP and Ser LP/HP carry `Bp`; lowering ignores it for `MultiSv`. Left as is |

## Tests

- `sv_classes_match_the_native_vectors`: native vectors under kemu, 1e-6 relative (both SV2 dialects pass).
- `sv_classes_stay_bounded_at_full_resonance`: stability at the self-oscillating corner (G5).

## Universal filter design (shared linear core API)

The filter family is one linear core plus separate transfer-function stages. Every filter lane
(`filt-*`) plugs into these; a product module is a configuration of them, never a copy.

- **Linear core, zero-delay state variable** (`dsp/filter/linear.rs`, `Zdf`): `new(g, k)` derives
  the `k`-dependent part; `tick` / `tick_divided` run one sample on `[h, b, l]` with the live `g`.
  The two methods are the two native dialects of the same recurrence: SV4 and SV6 multiply by the
  reciprocal `1 / (gk + 1)`, SV2 and SV Multi divide. A single dialect moved SV2 by 2e-6 and broke
  the 1e-6 vector gate, so each core keeps the dialect its native loop uses.
- **Transfer-function stages** (`dsp/filter/shaper.rs`): `Knee` (clamp then `(1 - w|c|) c`, with the
  `Fused` or `Split` slope the family uses) and `pade` (`v(v^2+27)/(9v^2+27)`). A core places a
  shaper on a state, a feedback path or an input; the shaper owns no state.
- **Plug-in points for the other lanes**: a new SVF-type or cascaded section calls `Zdf`/`Knee`;
  a biquad or SOS section stays in `biquad.rs`; a nonlinear feedback loop (ladder, AR, Daft) keeps its
  own recurrence and reuses `pade`/`Knee` only where the native loop has the same curve.

Duplicates removed in this unit: `sv_sat`/`sv_stage` and the inline SV4/SV6 loop (now `Zdf` + `Knee`),
`kontaktfx::sat3` and `cascade::xp_sat::<0>` (now `shaper::pade`, same operation order). The SV6
setters use the series order `x f f t t`, which is now its own `sv_g_six`; the SV1, SV4 and Multi
setters use `x f t f t` (`sv_g`).

Not yet migrated (other lanes): `svf.rs` generic TPT recurrence, `cascade::soft` (Pro-53 / Versatile
`(a - c|s|)s`), `ar_kernel::saturate`, `daft::soft` (f64, clamped at 3), and the ladder's inline
Padé forms. Each differs in precision or clamp, so each needs its own vector check.

## Next steps

1. G4: a kemu driver for the SV classes (the vector generator is not in this repo) to dump SV4/SV6
   res .9 vectors, then compare the measured peaks with the level recorded.
2. G2 (SV2) and G8: trace the SV2 process transform (`0x140b02400`) and the Multi `l` law (`0x140b03b50`).
3. G9: read the Multi gain constants (0.7, 0.49) and the serial mix in `0x140b01d00`.
