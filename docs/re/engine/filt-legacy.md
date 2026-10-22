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

## Knob 0 across the order-4/6 family (2026-10-21, parity matrix)

Kontakt's knob-0 response differs from the engine for LP4 and LP6 (and the Legacy Ladder, LP4-like),
at resonance 0. Kontakt shows about 8 dB/oct above 100 Hz (LP4: -9.5 dB at 60 Hz, -20 at 100, -36 at
500); the engine's 4-pole curve falls 24 dB/oct. The knob-0 cases are excluded from the golden.
Ruled out: a prior-knob state carry-over (warming to knob .5 first gives the same shape), the
knob-to-warp mapping (`FUN_14095c9c0`: `(e-1) 0.00257`, `+0.002`, `* 44011.976/rate`, `pi/2 * 0.99`,
`t = 0.99878 w`, the two `sin(0.5 -+ t)` poles: all equal the engine's `knob_position`, `angle` and
`lp_pole`). LP2, HP2, HP4, BP2, BP4 and BR4 match at knob 0. Open: the order-4 second section, whose
HP pole sits at -1 when `t -> 0` (`FUN_14095c9c0` second `FUN_1408e4970` call), and the native
normalisation `dVar4` at that point.

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
- The alternative branch (global `0x144fd4c54`) is dead: the image holds 0 there and the export
  never writes it, so only the `else` path runs. The offset slew `+0x1f8` is not referenced in
  `FUN_140ae9f90`. Both are closed (`cascade.rs` process comment).
- Coefficient update cadence of LP1/HP1: not measurable with static impulse steps (OPEN).

## Gap table (closed against the decompile, 2026-10-21)

Evidence: the Kontakt 8 export at `source/kontakt8/` (`rg -n "^// ==== <addr> "` for the function),
and the image's read-only data read with `pefile` (the static value of a global, not a decompile).

| Item | Engine covers | Evidence | Status |
| --- | --- | --- | --- |
| Clock alternative branch (`DAT_144fd4c54`) | ignored (else path) | the global is 0 in the image's `.rdata`; no writer in the export; `FUN_140ae9f90` runs only its `else` path | **closed**: dead branch |
| Clock offset slew `+0x1f8` | not modelled | not referenced in `FUN_140ae9f90` | **closed** for the clock; the cascade's own slew stays unmodelled and unused |
| LP1 / HP1 update cadence | static steps only | the clock in `FUN_140ae9f90` runs the 32-frame block | **open**: needs a dynamic-cutoff capture (no oracle step yet) |
| LP2 .. LP6, BP2, BP4, BR4 response | Kontakt-measured to 0.1 dB above -60 dB (golden) | measurement | **closed** |
| Stereo (legacy family) | per-channel state, no L to R coupling | measured, and the process loop of `FUN_140afef00` keeps per-channel state with shared controls | **closed** |
| Legacy Ladder setter (17) | note `100 p + 20`, index `5 * note + 1381.88`, `g = table * pi / rate`, clamp `1.57`, `0.5`, `res * 4`, `+1e-12` | `FUN_14095d630` and `FUN_140aef200` constants read from the image | **closed**: every constant equals the engine's `note_hertz`, `legacy_ladder` and `Ladder2x` |
| Legacy Ladder stage nonlinearity | `clip`: `v (1 - 0.01 |v|)` for `|v| <= 50`, else `+-25` | `FUN_140aef200` uses `0.01`, `50`, `25`, `0.769230x` (`big_g`), `k = g - 1` | **closed**: identical |
| Legacy Ladder knob 0 (res 0) | engine matches the Kontakt shape at knobs .25 to 1 | the setter and process agree with the engine, so the linear structure is the same; the knob-0 tail still differs (Kontakt about 6 dB/oct, -12.3 dB at 30 Hz) | **open**. Reproduced alone (no carry-over from BR4). Next: a knob sweep 0 to 0.25 to find where it leaves the engine's curve, and the runtime-generated exponential table entries near index 1481 |
| Simple LP/HP (120) knobs | `lp = CUTOFF, hp = RESONANCE` (unconfirmed) | the module wires its parameters through its own `FilterDJExtRange` (`FUN_140b058a0`, setParam index 0 to 2) and `FilterLadderExtRange` (`FUN_140b05ce0`); the mapping from the saved record to those indices is not in the export | **open**: the measurement shows CUTOFF and RESONANCE do nothing; the parameter indices must be traced from the record reader |
| Simple LP/HP alternate smoother (`u8`, `1/(p2 1500 + 150)`) | not implemented | `FUN_140afef00` ramps `+0x2b8`, `+0x2e0`, `+0x308` each frame by their increments (`+0x700`, `+0x2e4`, `+0x30c`); the `1500 + 150` rate is not in the function | **open** |
| Pro-53 (16), Vowel A/B (14/15), Versatile (19) | stage core, kemu match 2e-5 | not measured | **open**: add to `LEGACY_UNMEASURED` |

Notes on the decompile: 428 functions in the export are marked `decompile failed`. Those are not
usable. None of the closures above depends on them.

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

## Knob 0 resolved: analysis window, not a law (2026-10-22)

The Wine oracle's `eqrender.py` analyses a 16384-frame window that starts 256 frames before the response's
loudest sample (`spectrum`: `part = x[start - 16 ...]`, `ev = peak - 240`). For LP4 and LP6 at knob 0 the
peak is at frame ~508 and ~778, so the window drops the early part of the impulse response and removes its
DC energy: the measured shape is a window artefact. Taking the same window in the engine reproduces the
Kontakt curve to 0.02 dB (LP4: -0.43, -9.51, -20.12, -28.06, -36.33, -42.38, -48.40 dB at 30..2000 Hz).
The golden test now uses that window for every case, and includes the knob-0 rows for LP4, LP6 and Ladder.
The earlier "knob-0 mechanism" notes above are superseded by this.

## Knob-0 status (superseded, kept for the record)

- The LP4 setter (`FUN_14095c9c0`) and the allpass substitution (`FUN_1408e4970`) were compared term by
  term with `cascade.rs` `knob_position`, `angle`, `lp_pole`, `substitute`, `warp` and the LP4 design.
  Every constant and operation matches, including the normalisation `(2 - sum c) / sum b`, `n = q 0.35 + 0.65`,
  and both sections using the same low-pass pole `a`. The formulas predict the Kontakt-measured shape
  at knobs .25 and above (golden), and cannot produce the knob-0 tail at any small knob (engine sweep 0 to 0.05).
- So the knob-0 difference is not in the section law. It must come from code the setter does not
  reach at knob 0 or from the process path. Not yet identified.
- A Kontakt fine sweep (0 to 0.25) to find the transition is the next measurement. It could not be
  run on 2026-10-22: the `kontra_ref` PipeWire sink is SUSPENDED and the known-good Ladder spec now
  measures silence on the monitor (see the oracle note in the parity matrix PR). Routing is the
  user's audio setup; it was not changed.
- Simple LP/HP: the BFilterSimpleLPHP constructor (`01407c0000.c:8080`) initialises two wrapper cores
  (`FilterDJExtRange`, `FilterLadderExtRange`) with fixed floats `-1.0`; the record-to-parameter mapping is still not traced.

## Pending

1. Legacy Ladder knob 0: knob sweep and table check (above).
2. Simple LP/HP: trace the record-to-parameter mapping from the BFilter reader, then re-measure.
3. LP1/HP1 update cadence: a dynamic-cutoff capture.
4. Stage types 14/15/16/19: add to the oracle spec and extend the golden test.
