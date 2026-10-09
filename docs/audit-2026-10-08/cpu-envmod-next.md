# W9 next env/mod port — source-only preparation

Status: LOCAL PREPARED, tests not executed, no timings or READY claim. Streaming stays HOLD.

Branch `v2/w9-v1-envmod-sparse-608`, parent `494bb088` (W6 shared kernels over frozen `608a20a1`). Regression commit `f85c388c`; fix `5d71710b`.

## Attribution and rate check

The historical neutral-stage piano 64 cold samples attribute 0.0593179 µs/block to v1 env/mod and 7.8299644 to v2. These are aggregate sampled stage estimates from older source, not current-branch timings. They do not establish equal graphs or exact per-voice counts. The largest sampled symbol is `VoiceModState::fill_filter_factors` (5.30053 ms sampled period in the recorded profile), followed by `evaluate` (1.40014 ms); the aggregate also includes mixing/levels and envelope work.

Current `render.rs::prepare_voice` calls `VoiceModState::advance` per voice segment. `advance` evaluates sources only at an absolute CELL boundary; CELL is DSP BLOCK. Partial segments reuse the result. Gain/pan interpolate per frame, and envelopes advance per frame; route/source evaluation is already at control rate. This port therefore does not change modulation clocks, interpolation, envelopes, shapes, controller observation, or lag.

The avoidable work is clearing and converting a factors array sized for the entire plan for each voice, even an unbound voice. `0cb7a8a0:src/engine/filter.rs` builds only group-local knob rows and traverses that group's routes. This is an adaptation of that locality, not a verbatim data-layout transplant.

## Change and fixtures

Preparation records sorted unique addressed filter indices per program. Each lane's FilterBank remembers the program previously projected into its already neutral scratch. Projection resets those indices, clears the current program's indices, sums routes in original order, and applies the original midpoint/unit conversion. Native enabled bits remain set even when contributions cancel or have zero depth. Unbound voices clear the prior program.

Work is proportional to previous/current addressed stages plus current program routes, with no runtime allocation. The staged regression has 1024 plan stages and one addressed stage: baseline source implies 1024 normalized-scope queries each call; proposed source implies 1 initially, 2 when reused, and 1 when cleared. These are source-derived operation counts, not measured execution results.

Fixtures cover bounded scope queries, disjoint programs, unbound voices, shared scratch reuse, native enabled flags, cancellations, exact factor bits, and the pre-existing held-controller transition test. Both serial rendering and parallel single-voice rendering already pass through the one FilterBank method; no caller forks were added.

## Validation and remaining gate

Rustfmt parsing without writing/compiling and `git diff --check` passed. Cargo red/green, root no-run, allocation guard, frozen PCM checks and CPU cells are NOT RUN under the source-only instruction. No branch was pushed or integrated. Execute the regression at `f85c388c` first, then targeted modulation/filter tests at the final tip, followed by root no-run through the shared heavy queue. Measure separately after approval; do not combine with slot mixing in its first comparison.

Risk: new per-lane scratch ownership must remain paired with its immutable plan. The current constructor and serial/parallel owners satisfy that contract. The historical env/mod stage includes other work that this patch does not remove, so its aggregate 130× gap is not a projected speedup.
