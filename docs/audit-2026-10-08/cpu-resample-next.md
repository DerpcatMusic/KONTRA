# W9 next resampling port — source-only preparation

Status: LOCAL PREPARED, tests not executed, no timings or READY claim. Streaming stays HOLD.

Branch `v2/w9-v1-resample-run-608`, parent `494bb088` (W6 shared kernels over frozen `608a20a1`). Regression commit `7a0b8262`; fix `3dc82bc9`.

## Attribution and existing code

Historical neutral-stage piano 64 cold samples attribute 0.2372716 µs/block to v1 resampling and 6.5842883 to v2. These sampled stage aggregates belong to older source, not this branch; equal graphs and exact voice integrals were not established. Most sampled v2 work is `Cursor::render_filtered`, `render_run`, `Kernel::sample_window` and cubic interpolation.

`0cb7a8a0:src/engine/voice.rs::copy_run` gathers resident physical runs into a window before the interpolation/mix kernel, handling traversal separately from per-output interpolation. Current v2 already has the cubic-four kernel and a contiguous block path (including the accepted cubic port `4c26a197`). Native eight-slot cursors, however, return zero immediately from `render_run`, and `crossfaded` disables the scalar contiguous shortcut for every such cursor. Their interior frames resolve each tap through the serial loop traversal.

## Change and fixtures

Reuse the existing native `segment` address law to obtain a contiguous span limit. Permit the existing block run to handle that interior. Its physical endpoint/distance, crossfade, octave, full residency, envelope and fade checks remain in place; boundary, reverse and missing windows retain the scalar path. No new interpolation formula, coefficients, quality option, streaming storage model or allocation was introduced. Native unity/fraction-zero keeps the exact-frame path, preserving signed zeros.

The staged regression renders a 64-frame interior at step 0.8 with one native wrap slot and requires zero single-frame reads. Baseline source implies 256 cubic tap reads; the proposed source uses one resident span. Those are source-derived counts, not benchmark or executed-test results.

A second fixture compares the enabled bulk path against a reader that forces the established per-frame path. It checks bitwise PCM, complete debug cursor state, envelope remaining/state continuation and rendered count across wrap, ping-pong, linear/equal-power crossfades, forward/reverse traversal, two finite slots, tuning, release, variable block sizes, realtime/high quality, unity/upsampling/downsampling and a missing physical frame. Existing cubic-column and native traversal tests remain.

## Validation and remaining gate

Rustfmt parsing without writing/compiling and `git diff --check` passed. Cargo red/green, root no-run, allocation guards, frozen PCM and CPU cells are NOT RUN under the source-only instruction. No push or integration. Execute `native_loop_interior_resamples_one_window_per_block` on `7a0b8262` first, then the native/cubic/source tests at final tip, followed by root no-run through the shared queue. Any performance comparison is a separate later slot, not combined with slot mixing.

Risk: benefit depends on the target voices using native loops and on resident interior windows. Reverse, crossfade and octave cases can still fall back; the historical 28× aggregate gap is not a predicted speedup. Tests must establish exact cursor, tuning and storage-failure behavior before claiming this port ready.
