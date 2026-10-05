# Resident render workload and sustain optimization

This supplies partial V2-01/V2-19 evidence. It measures the new resident source
renderer, not file import, host dispatch, streaming, resampling, effects, or a
comparison with other samplers.

## Reproduction

```sh
CARGO_TARGET_DIR="$PWD/target-core" cargo run --locked --release -p sampler-core --example render_workloads
```

The example prepares four independent 4,096-frame stereo PCM layers per logical
note with continuous loops. It measures 64, 256 and 1,024 active voices with matching
reserved capacity, plus 64 active voices in a 4,096-slot pool. Cases cover 48/96 kHz,
64/256-frame blocks, unity envelopes and settled AHDSR sustain at 0.5. Each case
warms 64 callbacks, then times 512 callbacks and reports median, p99, maximum,
p99/deadline percentage and median nanoseconds per voice-frame.

Preparation, note admission, output validation, percentile sorting and retirement
are untimed. Every callback's stereo output is independently checked using exact
binary fractions. This constant-data workload has predictable memory locality and
no event traffic; it cannot substitute for future multisample/cache, modulation,
streaming-stall, dense-onset, host jitter or effect-chain workloads. Reported deadline
ratios are observations, not worst-case guarantees.

## Measured change

Previously, non-unity envelopes called `next` on every frame even after reaching
sustain. The renderer now recognizes a constant held level once per contiguous
source span. Ramps and release still advance per sample. Arithmetic order and
ascending voice-slot summation are unchanged. The existing unity path remains
explicit so the general constant-level path does not add a multiplication there.
No SIMD intrinsics, unsafe code, fast-math settings or new DSP dependency were added.

On a Ryzen 7 7800X3D, x86_64 Linux, Rust 1.99 release with thin LTO, three paired
before/after runs were pinned to CPU 2. Builds and scans finished before timing.
The table is the median of the three per-run medians, in microseconds, with 1,024
voices and matching capacity:

| Envelope | Rate | Frames | Before | After | Speedup |
| --- | ---: | ---: | ---: | ---: | ---: |
| Sustain 0.5 | 48 kHz | 64 | 62.281 | 27.371 | 2.28× |
| Sustain 0.5 | 48 kHz | 256 | 224.404 | 77.422 | 2.90× |
| Sustain 0.5 | 96 kHz | 64 | 65.461 | 27.840 | 2.35× |
| Sustain 0.5 | 96 kHz | 256 | 222.555 | 77.772 | 2.86× |
| Unity | 48 kHz | 64 | 26.650 | 27.230 | 0.98× |
| Unity | 48 kHz | 256 | 75.042 | 73.681 | 1.02× |
| Unity | 96 kHz | 64 | 27.021 | 26.460 | 1.02× |
| Unity | 96 kHz | 256 | 72.771 | 72.502 | 1.00× |

Across all 16 sustain configurations, the median speedup was 2.41×. Across unity
configurations it was 1.00×, with ratios between 0.968 and 1.033. CPU frequency,
background work and scheduler outliers remain uncontrolled. These are local results;
no competitor or production-latency claim follows from them.

Existing envelope partition tests cover phase crossings, captured release levels,
zero-duration phases, pedals and source end. Native process tests independently
check complete AHDSR waveforms at 44.1/48/96 kHz. Benchmark output remains bit-exact.
The first general-constant version added a multiply to unity rendering; that measured
regression was removed before this final comparison.

Raw CSVs, executable hashes, source hashes and platform metadata are under ignored
`artifacts/architecture-v2/render-*`. The before executable used core commit
`0358598`; only the benchmark's debug-mode guard changed before the final paired
runs, with no release-mode workload change. The current new-core scan remains 90
with all rules retained; complete gates accompany the implementation checkpoint.
