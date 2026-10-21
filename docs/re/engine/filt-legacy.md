# Kontakt 8 legacy filters: spec and gap table (`filt-legacy`)

Scope: Kontakt 8 filter types LP1/2/4/6, HP1/2/4, BP2/4, BR4, Legacy Ladder (Pro-53 and Vowel
share its stage core), and Simple LP/HP. Engine core: `crates/sampler-core/src/dsp/filter/`
(`cascade.rs`, `ladder_kernel.rs`, `kontaktfx.rs`). Mapping: `crates/sampler-kontakt/src/effects/filter.rs`.

Status: **provisional**. The decompiled Kontakt 8 export
(`/data/dev/workspace/Research/KONTRA/source/kontakt8/READY`) was absent when this was written, so
every law below comes from the existing repo evidence (`docs/re/kontakt-bindings/filter-laws.tsv`,
recorded native vectors, kemu runs). Each row must be re-confirmed against the decompiled functions
once READY exists.

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
| LP2/HP2/BP2/LP4/HP4/BP4/BR4/LP6 law | cascade design, measured LP4/LP6 | BP2/BP4/BR4/HP2 Kontakt measurements absent from `filter-laws.tsv`; clock alternative branch | Re-check against decompiled `14095*`/`14090*` setters once READY |
| Legacy Ladder / Pro-53 / Vowel / Versatile | stage core, kemu match 2e-5 | no Kontakt host measurement recorded | Same as above |
| Simple LP/HP | mono core, bit-exact vs kemu | stereo interleave; alternate smoother `u8` | Mapping downgraded to `Partial` with caveat; implement when the smoother is recovered |
| Stereo (legacy family) | no L-to-R coupling | none recorded | none |

## Change in this unit

- `crates/sampler-kontakt/src/effects/filter.rs`: Simple LP/HP (120) fidelity `Exact` -> `Partial`, with
  a dedicated caveat, so the import reports it instead of silently claiming full parity. No DSP change.

## Pending

Waiting on `/data/dev/workspace/Research/KONTRA/source/kontakt8/READY`. Then: grep the decompiled
export for the setter, prepare and process addresses listed above; confirm the smoother `u8` rate and
the stereo interleave for Simple LP/HP; confirm the clock alternative branch; and remove any caveat the
decompile disproves.
