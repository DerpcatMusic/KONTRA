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

## Results (48 kHz, 64-frame blocks; full grid in `grid.csv`, all in `results.json`)

| Pair | Implementation | Fidelity: worst shape (dB) | Alias (dB) | THD+N (dB) | Noise (dBFS) | Latency (frames) | ns/frame | instr/frame | Verdict |
|---|---|---|---|---|---|---|---|---|---|
| Resampler, 48 -> 44.1 kHz | sampler-core `resample` (High) | n/a | -64.4 (stop-band) | -105.2 | silence | 0 | 224 | n/a | no Kontakt reference |
| Resampler | rubato sinc (async) | n/a | -57.7 (stop-band) | -105.2 | silence | 0 | 161 | n/a | no Kontakt reference |
| Filter, Kontakt LP2 class (7 oracle cases) | sampler-core LP2 cascade | 0.0 (fitted; identity 0.01) | -17.1 | -87.8 | silence | 4 | 7.8 | 129 | faithful |
| Filter | biquad crate LP (RBJ-style), best mapping | 8.7 | -15.7 | -87.8 | silence | 9 | 2.4 | 22 | differs from Kontakt (character change) |
| Limiter | sampler-core `DbLimiter` | n/a | -56.5 | -54.1 | silence | 0 | 13.2 | 168 | no Kontakt reference |
| Limiter | look-ahead reference (bench-written) | n/a | -111.2 | -87.8 | silence | 241 | 4.1 | 110 | no Kontakt reference |

Extra numbers: resampler passband ripple 0.17 dB (ours) vs 0.04 dB (rubato); the limiter true-peak
overshoot at +6 dBFS is +2.3 dB for `DbLimiter` vs 0.0 dB for the look-ahead reference.
Noise "silence" means exact digital zero (-200 dBFS floor), so it does not separate the sides.

## Metric definitions and caveats

- **Alias**: energy outside the tone's harmonics (k = 1..12) and their +-400 Hz neighbourhoods (modulation
  sidebands), relative to the fundamental. For the resampler the column is the stop-band leakage instead
  (tones between the output and input Nyquist), so lower-is-cleaner means the same thing on every row.
- **Subnormal**: count of subnormal outputs during a decay to 1e-38. The input itself decays through the
  subnormal range, so this is about the same on every row and does not discriminate. Kept for completeness.
- **ns/frame** is noisy on a shared host (see the spread across runs). Trust instructions/frame for
  comparison. The resampler's ns includes table construction per call on both sides.
- **Fidelity**: the LP2 cascade's identity error is 0.01 dB because the Kontakt oracle test already
  validates it. The biquad row is a fit: hz and Q per case, with 2 starts, Nelder-Mead.
- **Latency**: the impulse-response peak index. A centred kernel peaks at 0, so 0 means "no reported
  look-ahead", not "zero group delay". rubato's `process_all` trims its own delay.
- The limiter has **no Kontakt reference**. Its overshoot and alias numbers are absolute quality only.

## Brief coverage

| Brief item | Status |
|---|---|
| 1. Signalsmith `BiquadDesign` bell/shelf vs `eq_family` | Not run. Only an RBJ-style biquad (crate) vs the LP2 cascade ran, as a filter-class stand-in. Needs `filters.h` vendored (MIT) and a `cc` build. |
| 2. rubato sinc vs our resampler | Run (table above). |
| 3. Signalsmith `Oversampler2xFIR` vs `hiir` | Not run. C++ path plus a `hiir` hook. |
| 4. Giannoulis/Reiss compressor vs `dynamics` | Not run. Needs a dynamics-pipeline hook (state packed through `ProcessorState`). |
| 5. Signalsmith Stretch vs time-stretch | Not run. C++ plus a `timemod` hook. |
| 6. Simper TPT SVF and ladder reference vs ours | Not run. Pure-Rust reference from the equations, plus a `tpt_svf` hook. |

Extra pair, from the earlier brief: the dB limiter `DbLimiter` vs a look-ahead reference (run, table above).

## Follow-up recommendations (evidence only, nothing adopted)

1. **Filter (Kontakt LP2 class)**: keep the shipping cascade. The RBJ-style biquad misses the Kontakt shape
   by 8.7 dB and runs 6x cheaper, so it is a character change, not a parity swap. Candidate licence:
   `biquad` crate, MIT OR Apache-2.0 (checked in its Cargo.toml).
2. **Limiter**: capture a Kontakt limiter response (step, and a +6 dBFS sine) before changing
   `DbLimiter`. Its +2.3 dB overshoot may be Kontakt parity (the +-2.5 clamp is a Kontakt law) or a bug,
   and the look-ahead brick-wall (241 frames, 0 dB overshoot) is a latency-changing option that must be
   opt-in.
3. **Resampler**: our kernel has 6.7 dB more stop-band rejection, rubato has 4x less pass-band ripple.
   No change to the engine from this run. For an offline/render-only path, rubato is an option. Licence:
   `rubato` MIT OR Apache-2.0 (checked in its Cargo.toml), which needs attribution in NOTICE if vendored
   or shipped. It stays an optional dependency here. A Kontakt interpolation capture is needed before any
   fidelity verdict.
