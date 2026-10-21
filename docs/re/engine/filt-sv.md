# Engine unit `filt-sv`: Kontakt 8 SV filters

Scope: saved filter types 50..=65 (SV LP1/HP1, LP2/BP2/HP2, LP4/BP4/HP4/Notch 4, LP6/BP6/HP6/Notch 6,
Par. LP/HP, Par. BP/BP, Ser. LP/HP). Kernel: `crates/sampler-core/src/dsp/filter/cascade.rs`
(`Sv`, `SvRamp`, `sv_stage`, `Cascade::sv`). Mapping: `crates/sampler-kontakt/src/effects/filter.rs`
(`build`, `map`). Shared config: `crates/sampler-ir/src/fx/filter.rs` (`Variant::Sv`,
`Variant::MultiSv`). Lowering: `crates/sampler-core/src/lower.rs` (`cascade_class`).

Status of the decompiled source: **waiting on READY** (`source/kontakt8/READY` absent, the export
directory is empty at the time of writing). Everything below that cites a function address is taken
from the existing repo evidence (`cascade.rs` comments, `kontakt-bindings/filter-laws.tsv`,
`filter-responses.tsv`, `cascade_sv_vectors.json`), not from a fresh read of the decompile. Once
READY exists, the rows marked *confirm* must be checked against the C.

## Modules, parameters and laws

| Type | Name | Topology (code) | Stored knobs | Ramp |
|------|------|-----------------|--------------|------|
| 50 / 51 | SV LP1 / HP1 | `Sv::One` one TPT pole | cutoff | 2 ms |
| 52 / 53 / 54 | SV LP2 / BP2 / HP2 | `Sv::Two`, one ZDF stage | cutoff, resonance, gain-in, gain-out | 2 ms |
| 55 / 56 / 57 / 58 | SV LP4 / BP4 / HP4 / Notch 4 | `Sv::Four`, two stages | cutoff, resonance | 8 ms |
| 59 / 60 / 61 / 62 | SV LP6 / BP6 / HP6 / Notch 6 | `Sv::Six`, three stages | cutoff, resonance | 2 ms |
| 63 / 64 / 65 | SV Par LP/HP / Par BP/BP / Ser LP/HP | `Sv::Multi`, two stages | cutoff, resonance, bandwidth (record lead value, clamped 0..1) | 2 ms |

Units and laws:

- **Cutoff** (knob `u` in 0..1): `note = min(115u + 20, 160)`; the stage coefficient is
  `g = tan(pi f / rate)` with `f = exp(note * 5 + 1381.88)` in the shared exponential table. Multi
  uses `centre = 180u - 20`, with the two notes at `centre -/+ spread`. Measured corners
  (`filter-laws.tsv`, `filter-responses.tsv`): 26.4 / 137.7 / 717.8 / 3785 / 18280 Hz at u = 0 / .25 /
  .5 / .75 / 1. Our law gives 25.96 / 136.6 / 718.9 / 3783.5 / 18274 Hz (see gap G1).
- **Resonance** (knob `r` in 0..1): `R = 1 - (1 - r)^3` for SV2, SV4, Multi; `R = 1 - (1 - 0.9 r)^3`
  for SV6 (its stored 0.9 scale). SV1 ignores resonance (measured: 0, .5, .9 identical).
  `k = 2 (1 - R)` sets the damping of the stage (SV4 second section `k2 = 2 (1 - 0.666 R)`).
  Multi derives `gain` from `R`, and in serial mode a spread-dependent scale on `R`.
- **Output gain** (applied to the selected tap): SV2 `1 - 0.5 R^5`; SV4 `1 - R c_mode` with
  `c = 0.77 / 0.9334 / 0.77 / 0.88` for LP/BP/HP/Notch; SV6 `1 - s` with `s = (1.2125 - 0.2425 |R|) R`
  for LP/HP, `s = (2 - R) R 0.991` for BP, `s = (1.482 - 0.494 |R|) R` for Notch; Multi `0.7 - 0.49 R^5`
  (parallel) or `o^2` with `o = 1 - 0.5 R^5` (serial).
- **Saturation**: band and low states pass `sv_sat`: `c = clamp(v, -2L, 2L)`, stored as
  `c (1 - 0.25 |c| / L)` (L = the stage's `l` law). Only the state is clipped, not the output.
- **Ramps**: each 32-frame block, a changed coefficient target ramps linearly over
  `round(rate * ramp / 32)` blocks (8 ms for SV4, 2 ms otherwise); steps with square < 1e-15 snap.
- **Stereo**: channels are independent (measured: no L to R coupling).

Measured dB facts used as checks (`filter-laws.tsv`): SV LP1 1st order 5.3-5.8 dB/oct; SV BP2 peak
0 dB at the cutoff, skirts 6 dB/oct; BP4 12 dB/oct; NOTCH4 depth -72..-121 dB at res 0; LP4 at u .5
corner 313 Hz (res 0); LP6 corner 252 Hz (res 0).

## Gap table

| # | Item | Shared engine now | Status |
|---|------|-------------------|--------|
| G1 | Cutoff law, u >= .5 | 718.9 / 3783.5 / 18274 Hz vs measured 717.8 / 3785 / 18280 (<= 0.2 %) | covered (native vectors and measurement agree) |
| G2 | Cutoff law, u <= .25 | 26.0 / 136.6 Hz vs 26.4 / 137.7 Hz (1.7 / 0.8 %) | **open**: our own tables disagree (`filter-responses.tsv` gives fc 46.9 Hz at u = 0, 140.6 Hz at .25; fit floor at low frequency). Needs a clean low-frequency sweep before changing the law |
| G3 | Resonance law res <= .5 | native vectors (`cascade_sv_vectors.json`), tolerance 1e-6 | covered |
| G4 | Resonance law res .9 | the kernel runs; no comparison to the measured peaks | **open**: measured peaks LP4 +53.7 dB, LP6 +48.2 dB, BP4 +41.8 dB, BP2 +49.9 dB at res .9 are level-dependent and not reproduced |
| G5 | Self-oscillation stability | state clamp `sv_sat`; new test `sv_classes_stay_bounded_at_full_resonance` (all five classes, cutoff 0 and 1, res 1, +-4 square, 48000 frames): finite and peak < 1e4 | covered as a bound check; not a level match (G4) |
| G6 | Saturation constants (`sv_sat` slope .25, clamp 2L) | from the native code (cascade.rs comments) | *confirm* against the decompile once READY |
| G7 | MultiSV bandwidth | mapped from the record's leading value, clamped to 0..1; `spread = 31.5 b + 0.5` | **open**: its ENGINE_PAR id is unprobed (the record lead is the only source) |
| G8 | MultiSV response label | `Par BP/BP` maps to `Bp`, `Ser LP/HP` maps to `Bp` in the config; the lowering ignores `response` for `MultiSv` | cosmetic inconsistency: the config `response` is not meaningful for Multi. Left as is (lower path is correct); clean up when the config type is split |
| G9 | Mode order SV6 BP6 / HP6 / Notch 6 | mode 1 uses `((2 - r) r) 0.991`, mode 2 uses the LP/HP law; notch takes `x - 2 bp` | *confirm* the per-mode constants against the decompile |
| G10 | Defaults of each knob | not recorded | open, needs the group/insert default table (`KONTAKT_VOICE_GROUP_DEFAULTS.md`) |

## Tests

- `sv_classes_match_the_native_vectors` (existing): native vectors under kemu, 1e-6 relative.
- `sv_classes_stay_bounded_at_full_resonance` (new): stability at the self-oscillating corner (G5).

No new golden vectors were added: the measured res .9 data cannot be matched until the level and
test signal of `filter-responses` are reproduced (G4).

## Next steps

1. Wait for `source/kontakt8/READY`. Then confirm G6, G9 and the per-mode constants, and the
   Multi bandwidth ENGINE_PAR id (G7) by `rg` on the export (`FilterSV1/2/4/6`, `FilterMultiSV`,
   addresses in the `cascade.rs` comments).
2. Settle G2 with a low-frequency sweep on Kontakt (not a new decompile).
3. Settle G4 with a level-matched res .9 measurement.

## Universal filter design (shared linear core API)

The filter family is one linear core plus separate transfer-function stages. Every filter lane
(`filt-*`) plugs into these; a product module is a configuration of them, never a copy.

- **Linear core, zero-delay state variable** (`dsp/filter/linear.rs`, `Zdf`): `new(g, k)` derives
  the `k`-dependent part; `tick` / `tick_divided` run one sample on `[h, b, l]` with the live `g`.
  The two methods are the two native dialects of the same recurrence: SV4 and SV6 multiply by the
  reciprocal `1 / (gk + 1)`, SV2 and SV Multi divide. A core keeps the dialect its native loop uses,
  so the vectors stay exact to the native rounding (a single dialect moved SV2 by 2e-6 and broke the
  1e-6 vector gate).
- **Transfer-function stages** (`dsp/filter/shaper.rs`): `Knee` (clamp then `(1 - w|c|) c`, with the
  `Fused` or `Split` slope the family uses) and `pade` (`v(v²+27)/(9v²+27)`). A core places a shaper
  on a state, a feedback path or an input; the shaper owns no state.
- **Plug-in points for the other lanes**: a new SVF-type or cascaded section calls `Zdf`/`Knee`;
  a biquad or SOS section stays in `biquad.rs`; a nonlinear feedback loop (ladder, AR, Daft) keeps its
  own recurrence and only reuses `pade`/`Knee` where the native loop has the same curve.

Duplicates removed in this unit: `sv_sat`/`sv_stage` and the inline SV4/SV6 loop (now `Zdf` + `Knee`),
`kontaktfx::sat3` and `cascade::xp_sat::<0>` (now `shaper::pade`; the same operation order, so
bit-identical). Not yet migrated (other lanes): `svf.rs` generic TPT recurrence, `cascade::soft`
(Pro-53 / Versatile `(a - c|s|)s`), `ar_kernel::saturate`, `daft::soft` (f64, clamped at 3), and the
ladder's inline Padé forms. Each differs in precision or clamp, so each needs its own vector check.

Test added: `sv_classes_stay_bounded_at_full_resonance`. Existing vector tests unchanged and passing.
