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

Native evidence used here: the Wine-oracle capture in `docs/re/kontakt-bindings/filter-responses-raw.tsv`
(copied to `crates/sampler-core/src/dsp/filter/sv_response_vectors.tsv`: impulse responses of Kontakt's
SV effects, dB at ten probes, 48 kHz, 16384 bins), the PE constants and the decompile.
Guard test: `sv_responses_match_the_kontakt_capture` (1 dB; rows listed in `KNOWN_OPEN` are the open
gaps below, and every res .9 row of the classes other than SV1/HP1 is open under G4).

| # | Item | Status | Evidence / reason |
|---|------|--------|-------------------|
| G1 | Cutoff law, u .25 to .75 | **closed** | Capture rows at res 0 and res .5 match within 1 dB (u = 1 for BP2 is G12) |
| G2 | Cutoff law, u <= .25 (SV2 class) | **closed** | BP2 and HP2 match the capture at u = 0, .25, .5, .75 (res 0). LP2 at u = 0 matches once the capture's window is applied (G10). The g law is the setter `0x140b04ee0`: `note = min(115 u + 20, 160)`, `f = table(5 note + 1381.8816)`, `g = tan(pi f T)` (PE constants 115, 20, 160, 5, 1381.88) |
| G3 | Resonance law res <= .5 | **closed** | Native vectors (1e-6) and the capture rows at res .5 (within 1 dB) |
| G4 | Resonance law res .9 | **algebra closed; capture chain open** | Native-exact (kemu oracle): `sv_res9_match_the_native_vectors` (`tools/re/sv_native_vectors.py`: kemu runs of 0x140b02400, 0x140b02790 and 0x140b02cf0 at u .5, res .9, 512 frames; SV6 through the native setters and `TABLE_INIT`) agrees with the Rust stages to 1e-5 of peak for LP2/BP2/HP2, LP4/BP4/HP4/Notch4 and LP6/BP6/HP6/Notch6. The capture rows at res .9 still differ by 2 to 11 dB (BP and HP worst, 30 Hz skirt included). The capture window is now reproduced; what remains is the capture chain (the knob ramp from the previous step is the first suspect, not tested) |
| G5 | Self-oscillation stability | **covered** | Test `sv_classes_stay_bounded_at_full_resonance` (all five classes, u 0 and 1, res 1, +-4 square): finite and peak < 1e4. Not a level match (G4) |
| G6 | Knee constants | **closed** | `0.25`, `2L`, the SV2 and SV4 operation orders confirmed from `0x140b02790` and the setters |
| G7 | MultiSV bandwidth | **identified; record path partly traced** | Core ENGINE_PAR index 2 (+0x28) is read by `0x140b03b50` as `spread = 31.5 b + 0.5` (constants confirmed, centre `180 u - 20` confirmed). Mode is index 3 (`0x140b048a0`, `round(2 v)` clamped 0..2). The reader `0x140d00db0` writes the value at +0x24, but the copy to core index 2 is not traced. Vector-verified at bandwidth 0.3 only |
| G8 | State limit `l`: SV2, SV4, SV6 and Multi | **closed** | SV2 `l = 600 - 576 n` (setter `0x140b04ee0` writes slot +0x2ac; PE constants 600, 576, 0.00746, 160). Multi `l = 400 - 376 n`, `n = clamp(note, 0, 134) / 134` (setter `0x140b03b50`). SV4 `l = 1600 - (2346 - 782 abs(n)) n` and SV6 confirmed earlier |
| G9 | Multi gains and mix | **closed** | Parallel gain `0.7 - 0.49 R^5` (PE 0.7, 0.49). Serial: `R'' = (0.01 min(spread, 6) / 6 + 0.99) R` and gain `(1 - 0.5 R''^5)^2` (PE 0.99, 0.01, 6, 0.5). Mix `(2 bp2 + 2 bp1) w + (hp2 + o1)` with `w = 0.5 - 0.1667 min(spread, 3)` (PE 0.5, 3, 0.1667), process `0x140b01d00`. Split knee order confirmed there too |
| G10 | Outputs of LP2, LP4, LP6 and BP6 at u = 0 (res 0) | **closed (measurement)** | Not a filter fault. The capture windows 16384 samples from 256 before the response peak; at u = 0 the LP responses peak near sample 294, so the capture drops their first samples and a 6 dB/oct tail appears. Emulated native outputs (kemu, process 0x140b02400) reproduce the capture to 0.01 dB with the same window. The guard test now uses that window and reads dB by interpolating the two bins around each probe |
| G12 | BP2 at u = 1 (res 0) | **open (low end only)** | Native emulation under the capture window (kemu, 0x140b02400) matches the capture to 0.8 dB at 100 Hz and above, and differs at 30 Hz by 4.2 dB and 60 Hz by 1.8 dB. The LF skirt is sensitive to the window and to the capture's reference normalisation, neither of which is modelled in the emulation; not closed |
| G13 | Defaults of each knob | open | Needs the group/insert default table (`KONTRA_VOICE_GROUP_DEFAULTS.md`) |
| G15 | SV2 input gain (parameter index 0) | open | Setter `0x140b05460` writes the input gain target as a table value `W(119.589 u + 1200)` (2^(1.993 u), 1 to 4). Our Two stage hard-codes `gin = 1`. The captures were taken at the default, so this only matters once the knob is modelled |
| G14 | Config type `response` for Multi | cosmetic | Par BP/BP and Ser LP/HP carry `Bp`; lowering ignores it for `MultiSv`. Left as is |

Captured: SV_LP2, SV_HP2, SV_BP6, SV_HP6, SV_NOTCH6 (`eqspecs.py svmore`, Wine oracle). Not captured: the three
MultiSV types (63..65), which need a bandwidth/mode spec.

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

1. G4: the kemu driver is in `tools/re/sv_native_vectors.py` (SV2, SV4, SV6 at res .9). The remaining
   step is the measured peaks against the recorded level, not a vector.
2. G10 and G12: trace the SV2/Six process transform (`0x140b02400`, `0x140b02cf0`) at small `g`.
3. G7 and MultiSV capture: find the bandwidth engine-parameter name (the probe was blocked by another oracle session holding the guard), then capture types 63..65 at bandwidths 0, .3, 1.
   G8 (SV2 `l`): trace the SV2 process (`0x140b02400`).
4. G4 and G10: capture the res .9 SV4/SV6 path with a documented input level, and the first-step
   behaviour after a subtype change.
