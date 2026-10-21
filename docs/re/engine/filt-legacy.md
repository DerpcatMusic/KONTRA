# Kontakt 8 legacy filters: spec and gap table (`filt-legacy`)

Scope: Kontakt 8 filter types LP1/2/4/6, HP1/2/4, BP2/4, BR4, Legacy Ladder (Pro-53 and Vowel
share its stage core), and Simple LP/HP. Engine core: `crates/sampler-core/src/dsp/filter/`
(`cascade.rs`, `ladder_kernel.rs`, `kontaktfx.rs`). Mapping: `crates/sampler-kontakt/src/effects/filter.rs`.

Status: laws come from repo evidence (`docs/re/kontakt-bindings/filter-laws.tsv`, recorded native
vectors, kemu runs) and from **Kontakt 8 measured through Wine** (`tools/kontakt-oracle/eqrender.py`,
`eqspecs.py filtlegacy`; raw output in `Research/KONTRA/oracle-filt-legacy/`, not in the repo). The
decompiled export was not used (user decision: do not wait for READY); refine with it if it lands.

## Kontakt measurements (Wine oracle, 2026-10-21)

Left-channel impulse responses, 48 kHz, reference = bypass, probes 30 Hz to 15 kHz. Test
`legacy_responses_match_the_kontakt_measurements` (`cascade.rs`, data `cascade_kontakt_oracle.json`)
compares the compiled cascade unit's 16384-frame impulse response at the same probes:

- Types: LP2, HP2, BP2, HP4, BP4, BR4 (new measurements) and Legacy Ladder (type 17); LP1/LP4/LP6 were
  measured earlier (`filter-laws.tsv`).
- 48 cases x 10 probes, every probe above -60 dB, match within **0.1 dB** (largest error ~0.06 dB,
  at the 30 Hz end of HP2/HP4/BP4/BR4; Kontakt values are rounded to 0.01 dB).
- Legacy Ladder: knob 0 with resonance 0 does **not** match (Kontakt ~6 dB/oct shallow tail,
  -12.3 dB at 30 Hz, -46.1 dB at 1 kHz; ours -14.7 dB at 30 Hz, -126.9 dB at 1 kHz). Excluded from
  the golden. Knobs .25 to 1 and resonance .5/.9 match. Cause not found; the knob-0 clamp of the stage
  cutoff law is the suspect.
- Simple LP/HP (120): inconclusive. CUTOFF and RESONANCE do not change the measured response (flat
  -0.25 dB, identical at every setting), so the mapping `lp = CUTOFF, hp = RESONANCE` is not confirmed.
  Its knobs need another engine-parameter name, which is not in the repo.
- Stage 16 (Pro-53), 14/15 (Vowel) and 19 (Versatile) were not measured in Kontakt yet.

## Saved type ids (measured)

| Kontakt name | Saved id | Core | Config fidelity |
| --- | --- | --- | --- |
| Legacy LP1 / HP1 | 2 / 3 | cascade, 1 pole | Exact |
| Legacy BP2 | 4 | cascade, 2 pole | Exact |
| Legacy LP2 / HP2 | 6 / 7 | cascade, 2 pole | Exact |
| Legacy LP4 / HP4 / BP4 / BR4 | 8 / 9 / 10 / 11 | cascade, 4 pole | Exact |
| Legacy LP6 | 12 | cascade, 6 pole | Exact |
| Pro-53, Vowel A/B, Legacy Ladder, 3x2 Versatile | 16, 14, 15, 17, 19 | stage core | Exact |
| Simple LP/HP | 120 | KONTAKTFX (`kontaktfx.rs`) | **Partial** (changed, see below) |

Source: `docs/re/kontakt-bindings/filter-type-ids.tsv`, `crates/sampler-kontakt/src/effects/filter.rs`.

## Per module

Parameters are the saved knobs (`CUTOFF`, `RESONANCE`), both 0..1 on the engine's native clock
(32-frame control block, 2 ms ramps).

**Legacy 1-pole LP1 / HP1 (2, 3)**
- Cutoff law (measured, `filter-laws.tsv` rows "legacy LP1POLE / HP1POLE"): LP1 -3 dB at 249 / 1189 /
  5355 Hz for knob .25 / .5 / .75 (x4.6 per quarter); HP1 208 / 990 / 4389 Hz. Knob 0 bottoms at
  38-59 Hz.
- Resonance has no effect. Order 1, 5.3-5.6 dB/oct.
- Native prepare: one-pole prototype `b0 = b1 = 0.26276`, pole `0.29341`, store `44011.976 / rate`.

**Legacy 2-pole LP2 / HP2 / BP2 (6, 7, 4)** and **4-pole LP4 / HP4 / BP4 / BR4 (8, 9, 10, 11)**, **6-pole LP6 (12)**
- One design (`cascade.rs` header): Butterworth prototype per section, `Q_k = 1 / (2 cos((2k+1) pi / 2N))`,
  bilinear at `tan(w/2) = 0.5463`, warp by the stored knob through an 8-term exp series, feedback taps
  scaled by `1 + f r'` with `r' = 1 - (1 - res)^2`, first-order allpass substitution for the move.
- LP4 / BP4 knee clamp on section 1 output (`0.6`), HP4 on its input; LP6 on section 2 (`0.7`); BR4
  sums two sections in parallel, double coefficients.
- Measured (`filter-laws.tsv`, LP4/LP6): at resonance 0 the knee is sharp and flat, -3 dB at the same
  corners as LP1 (246 / 1192 / 5364 Hz), 24-25 dB/oct (LP4) and 36-37 dB/oct (LP6): maximally flat
  pole distribution. Resonance .5 / .9 on LP4: +20.9 / +37.3 dB peak at ~1.7 kHz (knob .5), passband
  -5.2 / -9.9 dB. LP6: +30.5 / +52.3 dB, passband -11.2 / -22.9 dB.
- Stereo: no L-to-R coupling (right output exactly 0 for left-only input).

**Legacy Ladder (17), Pro-53 (16), Vowel A/B (14, 15), Versatile (19)**
- Shared stage core (`cascade.rs` `Legacy::Ladder|Pro52|VowelA|VowelB|Versatile`). Setter and process
  (Pro-53 FUN_14095d940 / FUN_1409099a0; Legacy Ladder FUN_14095d630 / FUN_140aef200; Vowel
  FUN_14095dd00 / FUN_14095e250 / FUN_140914a20) match the native code under kemu to 2e-5.
- Scalar summation order only; the SSE path for >= 32-frame calls differs in rounding.
- Kontakt measurement of these four types is **not** in `filter-laws.tsv`; the Exact claim rests on
  kemu against the native code.

**Simple LP/HP (120)**
- KONTAKTFX core: FilterDJExtRange mode 2 high pass into FilterLadderExtRange mode 0, 2 knobs (LP, HP).
- Core is bit-exact in mono against the native class under kemu at 48 kHz (`kontaktfx.rs`, `kontaktfx_vectors.json`).
- Stereo: runs mono cores per channel with shared controls. The native channel interleave is unprobed.
- Alternate smoother `u8`, rate `1/(p2 1500 + 150)` as recorded in `kontaktfx.rs`: unprobed; not implemented.

## Control clock (shared by the legacy cascade and stage cores)

- Native control clock `0x140ae9f90` reproduced at 32-frame blocks.
- Its alternative branch (global `0x144fd4c54`) and offset slew (`+0x1f8`, written `+0x1f4`) are
  unwritten in the binary as far as recovered; the engine takes them as unused
  (`cascade.rs:1880`).
- Coefficient update cadence of LP1/HP1: not measurable with static impulse steps (OPEN).

## Gap table

| Item | Engine covers | Missing / unproved | Action |
| --- | --- | --- | --- |
| LP1 / HP1 law | cascade 1-pole, cutoff law measured | update cadence (OPEN) | Needs dynamic-cutoff capture or decompiled clock; no change |
| LP2/HP2/BP2/LP4/HP4/BP4/BR4/LP6 law | cascade design; Kontakt-measured to 0.1 dB at all probes above -60 dB (golden test) | clock alternative branch (only unused-branch evidence) | none for the response; re-check the branch when decompile lands |
| Legacy Ladder (17) | stage core; Kontakt-matched at knobs .25-1 and res .5/.9 (golden) | knob 0 (res 0) shape; see measurements above | Investigate the stage cutoff law's knob-0 clamp against Kontakt |
| Pro-53 (16), Vowel A/B (14/15), Versatile (19) | stage core, kemu match 2e-5 | no Kontakt measurement | Measure with the same oracle spec (add subtypes to `LEGACY_UNMEASURED`) |
| Simple LP/HP (120) | mono core, bit-exact vs kemu | Kontakt knob names (CUTOFF/RESONANCE do nothing); stereo interleave; alternate smoother `u8` | Find the real engine-parameter names, then re-measure; mapping kept `Partial` |
| Stereo (legacy family) | no L-to-R coupling (Kontakt-measured) | none | none |

## Change in this unit

- `crates/sampler-kontakt/src/effects/filter.rs`: Simple LP/HP (120) fidelity `Exact` -> `Partial`, with
  a dedicated caveat, so the import reports it instead of silently claiming full parity. No DSP change.

## Shared core (filt-sv API)

Built on `engine/filt-sv` (`dsp/filter/linear.rs` `Zdf`, `dsp/filter/shaper.rs` `Knee`, `pade`).

- **Knee.** The legacy BFilter clip (LP4/BP4 on section 1, HP4 on its input, LP6 on section 2) was an
  inline closure `(v - c) s + c` with `c = clamp(v, -1, 1)`. It is now `Knee::excess(s)`, a third
  `Slope` shape in `shaper.rs`, and the cascade `Design` stores `clip: Option<(usize, Knee)>`. The
  operation order is the same, so the native vectors (`legacy_classes_match_the_native_vectors`, 2e-5)
  and the Kontakt golden are unchanged. The packed cell layout is unchanged.
- **Not moved onto `Zdf`.** The legacy 2/4/6-pole sections are direct-form-I biquads with
  coefficients from the Butterworth warp, not zero-delay SVF stages. A `Zdf` realisation has the same
  transfer function but different rounding, and no vector proves it bit-identical, so the DF1 kernel
  stays. The one-pole LP1/HP1 and BR4 use the same DF1 tick.
- **Duplicates still open** (other lanes): the DF1 section tick also appears in `eq_family/legacy.rs`,
  `space/shape.rs`, `space/network.rs`, `timemod/dual_x.rs`, `color/tonestack.rs`. Sharing one runner
  there needs a bit-identity vector per caller and is not done here.

## Pending

1. Legacy Ladder knob-0 shape: find the stage cutoff clamp by measuring a finer knob sweep near 0 in
   Kontakt (oracle), then match it in the stage core.
2. Simple LP/HP: find the engine-parameter names for its two knobs (decompiled `FilterDJExtRange`
   setParam at 0x140b058a0 when available, or a KSP `$ENGINE_PAR_*` probe), then re-measure.
3. Stage types 14/15/16/19: add to the oracle spec and extend the golden test.
4. When `source/kontakt8/READY` appears: confirm the clock alternative branch and the smoother rate.
