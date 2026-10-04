# Bounded static BiquadFilter source evidence

Reviewed 2026-10-04. This source path has scoped original-native/Rust comparisons
pending package verification. It does not establish whole-host lifecycle,
complete program audio parity or performance.

| Admission dimension | Measured gate |
| --- | --- |
| Mode | High-pass 0; low-pass 1 |
| Frequency / Q | 150–20,000 Hz; Q 0–0.1. Low-pass additionally requires frequency ≥1,000 Hz and Q = 0. |
| Rate / width | 32,000, 44,100 or 48,000 Hz; 1–12 channels |
| State and controls | KeyTracking = 0; initially enabled; no connected controls. Only equal finite physical numeric/boolean setters are admitted. Changed values, including onInit/saved overrides, reject before graph/clock/state mutation. |
| Attributes | Unknown attributes reject, except Name. |

Original retained native code executes the cold core constructor, fresh
empty-history voice clone, prepare, named setters and audio processing with
authored descriptors and explicit allocation adapters. It uses original PE
data and the native SSE trigonometric branch. No copied math, patched activation
or substituted dispatch is used. The outer processor/UI constructor and
initialized runtime-data state are not exercised. Fresh reconstruction clears
history; this is not evidence for live reset/bypass behavior.

The 36 native matrix cases match the reconstructed coefficient recurrence,
fresh split processing and equal-setter behavior bit for bit. **36 actual
Rust/native cases** have maximum observed audio residual 4.657e-7 and coefficient
residual 1.789e-7 (113 ULP maximum). **69 actual Rust/native tone cases**, each
32,768 frames, have maximum observed audio residual 0.000244588 within the
declared 0.0003 comparison budget. Rust fresh split processing matches its own
bulk output exactly. The runtime diagnostic retains these numerical differences;
the observed budget is not a proven universal bound.

Five focused leaf/Renderer checks, an authored routed-sample native impulse
comparison and first-error regression pass. The actual Alto regression preserves
six PCM/event/state hashes and counts. Among seven observed BiquadFilter nodes
in five external Starter presets, **two nodes pass the gate and zero whole
presets become playable**. Those two scripts initialize without changing their
filter settings; remaining graph gates still reject the complete presets.

High Q, band-pass/notch, key tracking, initial/live bypass, connected or changed
controls and unmeasured rates remain gated. Inline histories add no persistent
heap buffer/cache. No speed, deadline, additional-program or universal numerical
parity claim follows from these checks.
