# Resampling / interpolation lane (perf research)

## Metric

CPU cost of one plain resident sample voice (`Cursor::render`, 64-frame block, 48 kHz,
no filter, default envelope, stereo out), measured by the ignored test
`crates/sampler-core/src/source/voice_receipt.rs`. Run:
`nice -n 19 build-slot cargo test --release -p sampler-core --lib voice_receipt -- --ignored --nocapture`.
Synthetic 4M-frame buffer, no library env needed. Best-of-9 per cell, single thread.

## Baseline (origin/main 8547a6de7), ns per voice-block, 1 voice / 256 voices

| scenario | 1 voice | 256 voices | ns/frame (1 voice) |
|---|---|---|---|
| unity copy (step 1.0, frac 0) | 29.9 | 28.3 | 0.47 |
| unity cubic (step 1.0, frac 0.5) | 356 | 370 | 5.6 |
| down cubic (step 0.5) | 362 | 380 | 5.7 |
| up polyphase (step 1.37) | 687 | 701 | 10.7 |
| up short sinc (step 3.0) | 7654 | 7758 | 119.6 |
| High long sinc (step 1.37) | 13906 | 13806 | 217.3 |

Whole-process `perf stat` over all cells: 3.62e12 instructions, 6.90e11 cycles
(per-scenario instruction split not yet taken).

Result: a plain linear/cubic/polyphase voice is ~0.36-0.69 us per 64-frame block,
already far under the 5 us target. The outliers are the step > 2 short sinc (7.7 us)
and the High quality long sinc (13.9 us); those are the work left in this lane.

## Sources

- Niemitalo, "Polynomial interpolators for high-quality resampling of oversampled audio", yehar.com/blog (deip.pdf). Coefficient tables; not yet adopted (4-point 3rd-order optimal not implemented).
- sfztools/sfizz SFZ `sample_quality` opcode docs, sfzformat.com/opcodes/sample_quality: 1 linear, 2 polynomial, 3-10 windowed sinc. Reference behaviour only (BSD-2 engine, not copied).
- KVR Audio sfizz threads (kvraudio.com, p=8090844, p=8354458): sfizz defaults to Hermite polynomial interpolation; Sinc 8-72 for higher quality.
- oddio issue 12, github.com/Ralith/oddio/issues/12: Lagrange 4-point beats linear, still rolls off before 20 kHz.
- Kontakt engine: docs/re notes only (resampler setup and dispatch addresses in `docs/re/engine/`); REA trace of the Kontakt8.exe read kernel NOT yet done.

## Not done (honest state)

- No prototype landed. The 20% gate is not evaluated; no PR.
- Kontakt's kernel not traced in REA; no claim about its interpolation law.
- Candidate for the next experiment: extend the 64-phase polyphase bank from steps 1-2 to 2-8
  (removes the 7.7 us short-sinc path), and a per-note fixed-ratio fast path.
