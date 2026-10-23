# DSP bake-off (`sampler-dspbench`)

Head-to-head harness: each pair runs our block and a candidate through one `Block` trait on the same
signals, and scores three axes:

1. **CPU**: ns/frame (min of 5 runs, 1 s white noise) and instructions/frame (`perf stat -e instructions:u`
   on a child that processes 1 s of noise, minus the same child without processing).
2. **Quality**: alias, THD+N, noise floor, subnormal outputs, stability under 37 Hz modulation, overshoot,
   latency, plus pair-specific metrics (passband ripple, stop-band, static curve, attack/release).
3. **Kontakt fidelity**: magnitude shape error after a fitted parameter mapping (Nelder-Mead), only where a
   Kontakt reference exists. Phase and time-constant bands are wired in `band.rs` but no pair supplies
   Kontakt phase or time-constant data yet.

Verdicts (`band.rs`): **faithful** (inside the fidelity band), **faithful + cleaner** (inside the band and
at least 3 dB better on alias/noise, or stable where the other is not), **differs from Kontakt (character
change)** (outside the band), **no Kontakt reference** (no capture; quality only). Default bands:
magnitude shape +-0.5 dB, phase +-5 deg, time constants +-10 %.

## Reproduce

    build-slot cargo run -p sampler-dspbench --features candidates --release -- --out docs/research/dspbench

Without `--features candidates` only our side runs (the shipping build never pulls the candidates).
`--no-perf` skips the instruction counts. The engine hooks live in `sampler-core` behind feature
`dspbench` (`crates/sampler-core/src/dspbench.rs`).

## Verdicts (48 kHz, 64-frame blocks; all rates and block sizes in `grid.csv` and `results.json`)

Fidelity is the worst magnitude shape error after the fitted mapping, against the band of +-0.5 dB. Only
the filter (LP2), Solid G-EQ and Ladder pairs have Kontakt data; every other row is quality only.

| Pair | Ours | Candidate | Fidelity worst shape (dB) | ns/frame (ours / cand) | instr/frame (ours / cand) | Verdict (ours / cand) |
|---|---|---|---|---|---|---|
| Resampler, 48 -> 44.1 kHz | sampler-core `resample` (High), stop-band -64.4 dB | rubato sinc (async), stop-band -57.7 dB | n/a | 475 / 275 | n/a | no Kontakt reference |
| Filter, Kontakt LP2 class | LP2 cascade | biquad crate LP (RBJ-style), fitted hz and Q | 0.0 / 8.7 | 8.1 / 1.9 | 129 / 22 | faithful / differs (character change) |
| Limiter | `DbLimiter`, +1.3 to +2.3 dB true-peak overshoot at +6 dBFS (2.3 at 48 kHz) | look-ahead brick-wall, 0 dB overshoot, 241 frames latency | n/a | 15.3 / 8.6 | 168 / 110 | no Kontakt reference |
| Solid G-EQ bell and shelf | Solid G-EQ designer | Signalsmith `BiquadStatic` (Vicanek) | p90 over 228 rows: 7.3 / 7.0 (worst row 19.0 / 19.2) | 2.1 / 3.9 | 25 / 33 | differs (character change) |
| 2x oversampling, tanh drive | `hiir` half-band (20 kHz, 78 dB) | Signalsmith `Oversampler2xFIR` | n/a | 17.6 / 62.1 | 215 / 705 | no Kontakt reference |
| Compressor (Giannoulis gain computer) | generic dynamics kernel | Giannoulis/Massberg/Reiss gain computer | n/a | 32.8 / 31.3 | 260 / 238 | no Kontakt reference (see caveat) |
| Ladder, Kontakt class 17 | Ladder cascade (legacy class 17) | Zavalishin TPT ladder, one-sample feedback | 0.0 / 6.0 | 64.3 / 8.9 | 315 / 35 | faithful + cleaner (see caveat) / differs (character change) |

## Caveats on these verdicts

- **Ladder "faithful + cleaner" is not a quality win.** Ours is inside the fidelity band (0.0 dB). The
  "cleaner" flag comes only from the subnormal rule in `Quality::cleaner_than` (ours 0 subnormal outputs,
  the candidate 1774). The README already says the subnormal count does not discriminate on these tests, so
  read this row as "faithful", not "cleaner". The candidate has 8 dB less alias (-20.3 dB against -12.3 dB),
  but it sits outside the fidelity band, so its verdict is "differs" whatever its quality.
- **The compressor candidate matches ours to 0.00 dB** in static curve, attack and release. Both sides run
  the same textbook gain computer and one-pole ballistics, so this pair compares an implementation with
  itself. It shows the kernel is correct against the paper's equations, not that it beats any alternative.
  It is not evidence for or against adopting anything.
- **The Ladder ns/frame** for ours (64 ns, 315 instructions) is about 7x the candidate's, because the
  Kontakt ladder runs its own clocked control cadence. That is the cost of matching the class. The candidate
  is cheaper because it has no clocking, and its resonance is approximate (one-sample feedback delay).
- **Limiter has no Kontakt capture**, so the 1.3 to 2.3 dB overshoot is a measured property of our code, not a
  confirmed Kontakt behaviour.
- **Instruction counts** come from `perf stat -e instructions:u` on the perf child, minus the same child
  without processing. The counts above are from a run with perf enabled.

## Licences of the candidates

| Candidate | Licence | Used as | Status |
|---|---|---|---|
| Signalsmith DSP (`BiquadStatic`, `Oversampler2xFIR`) | MIT, Copyright 2021 Geraint Luff / Signalsmith Audio Ltd (`candidates/signalsmith-dsp/LICENSE.txt`) | Vendored headers and C++ shim, built only with `--features candidates` | Bench only. Not shipped; attribution kept with the vendored copy. |
| `biquad` crate 0.6 | MIT OR Apache-2.0 (checked in its `Cargo.toml`) | Filter candidate | Bench only, optional dependency. |
| `rubato` crate 5 | MIT OR Apache-2.0 (checked in its `Cargo.toml`) | Resampler candidate | Bench only, optional dependency. Needs a NOTICE line if ever shipped. |
| Giannoulis, Massberg, Reiss (JAES 2012) | Published equations; no code copied, written from the paper | Compressor candidate | Bench only. |
| Zavalishin, *The Art of VA Filter Design* | Published equations; no code copied, written from the TPT equations | Ladder candidate | Bench only. The book is not vendored. |

No candidate is adopted by this run. Adoption would need a Kontakt capture where the band is at stake, and
a separate change to the engine.

## Brief coverage

| Brief item | Status |
|---|---|
| 1. Signalsmith `BiquadDesign` bell/shelf vs `eq_family` | Run (Solid G-EQ pair, 228 Kontakt rows). Signalsmith `BiquadStatic` is used, not `BiquadDesign`. |
| 2. rubato sinc vs our resampler | Run. |
| 3. Signalsmith `Oversampler2xFIR` vs `hiir` | Run (quality only, no Kontakt oversampler capture). |
| 4. Giannoulis/Reiss compressor vs `dynamics` | Run, but the candidate is identical to ours (see caveat). |
| 5. Signalsmith Stretch vs time-stretch | **Not run.** Needs a C++ build of Stretch and a `timemod` hook. Still open. |
| 6. Simper TPT SVF and ladder reference vs ours | Run as a Zavalishin TPT ladder (Simper's SVF is not built). Fidelity fails at 6.0 dB. |
| Extra: `DbLimiter` vs look-ahead reference | Run. |

## Recommendations (evidence only, nothing adopted)

1. **Filter (Kontakt LP2 class)**: keep the shipping cascade. The RBJ-style biquad misses the Kontakt shape by 8.7 dB and runs about 6x cheaper, so it is a character change, not a parity swap.
2. **Solid G-EQ**: keep the designer. Signalsmith's Vicanek shelf/bell is no closer to Kontakt at the p90 (7.0 dB vs 7.3 dB), and its CPU is about 2x ours.
3. **2x oversampling**: keep `hiir`. Signalsmith's FIR is about 3.5x the cost, with no quality win on these tests.
4. **Ladder**: keep the shipping ladder, which is inside the fidelity band. The TPT candidate is cheaper but a character change (6.0 dB), so it is not a parity swap.
5. **Limiter**: capture a Kontakt limiter response (step, and a +6 dBFS sine) before changing `DbLimiter`. Its 1.3 to 2.3 dB overshoot may be Kontakt parity (the +-2.5 clamp is a Kontakt law) or a bug. The look-ahead brick-wall (241 frames, 0 dB overshoot) is a latency-changing option that must be opt-in.
6. **Resampler**: ours has 6.7 dB more stop-band rejection, rubato has 4x less pass-band ripple. No engine change from this run. rubato is an option for an offline render path only.
7. **Compressor**: no verdict. Needs a real alternative and a Kontakt compressor capture before this pair means anything.

## Open items

- Signalsmith Stretch (brief item 5) is not run.
- Kontakt captures needed for a fidelity verdict: limiter, resampler, oversampler, compressor.
- Phase and time-constant bands are wired in `band.rs` but no pair supplies Kontakt phase or time-constant data.
