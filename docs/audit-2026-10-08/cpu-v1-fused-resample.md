# v1 fused realtime resampling port

Branch `v2/w9-v1-fused-resample-5fc`, based on `49401fce` (CPU42 source
`5fc362f3` plus prepared-bank and frozen-host probe changes).

The user directed W9 to take v1's interpolation instead of further sinc-bank
optimization. `0cb7a8a0:src/engine/voice.rs::hermite`, `mix_body` and
`mix_avx2` supply the f32 polynomial, upper-24-bit interpolation phase and
fused lane arithmetic. Safe f32 column arrays use v2's existing SIMD dispatch
instead of adding unsafe architecture intrinsics to sampler-core. The cursor
retains v2's f64 position advancement, native serial-loop address law and
amplitude product order. No env/mod, DSP-chain or slot-mixing edits are included.

Realtime uses v1 interpolation through three pitched-up octaves (8x). This is
the v2 ordinary/extreme-ratio cutoff; v1's source window supported up to 32x.
The existing prepared bank remains above 8x through v2's supported 16x maximum. High quality keeps its long
sinc. Whole-position whole-step voices retain exact centre taps, including
signed zeros and finite extreme samples, without evaluating an overflowing
polynomial. This intentionally restores v1's realtime interpolation spectrum;
it does not claim the former short-sinc alias rejection below 8x.

Native interiors now reuse one bounded source window. The bulk/scalar regression
caught a short-span admission bug in the previously source-only candidate:
when there was less than one kernel margin left, the run could cross a native
pitch-tuning transition. Such spans now fall back to the scalar traversal.
Crossfades, reflected direction, release, tuning, missing frames, starvation
and envelope continuation retain the existing checks. Storage windows are
borrowed or gathered on the stack, and no callback allocation was added.

## Checks and evidence

- `4506e4d5`: failing-first v1 Hermite regression executed on the baseline;
  its first fractional comparison failed by one ULP before reaching pitched-up
  kernel-selection checks.
- `7584228e` / `c44a9e8a`: prepared native-window regression and source port.
- `0e38851e`: fused f32 Hermite + mix adaptation and runtime null fixture.
- `22e44927`: short-span boundary fix, exact whole-step taps and updated
  realtime spectral policy fixtures.
- Core library checks: 82 passed in the broad focused run, with the native
  boundary regression subsequently fixed and passing separately.
- Integration checks: resample 19, source 10, paged_render 16 — all passed.
  The v1 null fixture covers 9 ratios × 4 callback partitions × 256 stereo
  frames (18,432 scalar samples): bit-identical, zero measured heap calls.
- Root compile and quiet A/B: pending completion.

The runtime null fixture compares a generated stereo waveform with an independent
literal v1 scalar Hermite oracle at dyadic ratios 0.25–8x and callback partitions
1/7/64/256. Allocation/free counting covers trigger and rendering. These ratios
have equal v1 fixed-point and v2 traversal positions. This is an interpolation
reference check, not a native Kontakt or full-instrument null claim.

The numeric run ledger is
`/mnt/Windows11/DEV_WORKSPACE/kontra-runs/w9-v1-fused-resample-20261009`.
The planned quiet matrix reuses the audit's original Morph256, Horns256 and
Horns64 schedules, frozen v1 adapter and source5fc baseline, with cold and warm
source-file cache conditions and reversed side order between cold and warm.
One paired matrix is sufficient unless it leaves the verdict unclear. Timed runs must
follow W13's handback; profiler-attribution estimates are not scored timings.
No PCM or decrypted library payload is persisted.

READY and full CPU/native parity are not claimed until the required checks
and measurements are complete. Native Morph/Horns capture paths have been
requested from the coordinator; the available reference folder lacks these
programs.

NEXT: finish targeted/root checks, then take the first-ready quiet slot after W13.
