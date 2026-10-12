# Kontakt 8.12 fade curves — paired runtime source, next-batch HOLD

This slice depends on native lane `9fe764e4dcb3e876f82a1d19e2656cf8fe02ffe6`:
`FadeCurve`, checked optional operand local, KSP constants/signatures/lowering,
behavior dispatch and the direct parallel test caller. Pair both commits before
any build. Neither source slice is independently compile-ready. No changes to
the current frozen AR batch are requested.

## Authoritative intent and limits

NI KSP Manual, retrieved 2026-10-10:
<https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/event-commands#fade_in-->
and `#fade_out--`; optional curve introduced in Kontakt 8.12. Archived HTML
SHA-256 `57f8f69b4deec41cc0eb98fd1cc5196f3c02b72e3d222faff7b91fe8551626ce`
under coordinator `handoff/official-docs/ksp-events.html`, section text lines
64..95 of `ksp-events.txt`.

The five fade-in functions of normalized time t are t, sin(pi*t/2),
(1-cos(pi*t))/2, t*t and 1-(1-t)*(1-t). Fade-out is the TIME-MIRROR of fade-in,
not 1-shape(t). Omitted curves remain Linear. Selectors 0..4 are our internal
enum values, not vendor ABI ordinals. Invalid selector rejection is our safety
policy; native diagnostic strings include LINEAR fallback, so native invalid
selector reaction is UNKNOWN.

Approved Kontakt 8.13.1 static strings at file offsets 0x4e84898..0x4e848f0
establish the NI_FADE names only. No exact measured native shape coefficients,
clock, PCM or CPU/RAM parity is claimed. The corresponding immutable static
binary specification was inspected instead of executing a Kontakt host.

## Runtime code and scope

`script_params.rs::Fade::at` applies the documented functions, preserving the
old Linear arithmetic order exactly. Public `fade_note`/`fade_note_group` stay
Linear; only checked script event curves select the new paths. Interrupted
fade-out starts from the current level, and zero-duration/endpoint behavior
remains explicit. Stop flags and existing retirement cadence are unchanged.

The old render path only evaluated fades at a CELL control grid (64 frames in
this checkout) and interpolated endpoint gains. That is not the nonlinear
shape inside a cell: a quadratic fade at frame 16 of 128 would be four times
the documented level under a 64-frame straight interpolation. Active nonlinear
note fades now ride the ephemeral `voice_mod::Ramp` and apply at the physical
amplifier's per-sample clock. Both chained and unchained paths consume the same
Fade; `without_gains` clears it after the chain amplifier so it cannot multiply
twice. Completed curves use a constant endpoint and do not retain nonlinear
sample work. The no-fade/legacy Linear gain loops retain their old arithmetic.

The existing amplifier clock evaluates `at + sample_index + 1`, both in
`dsp.rs` and unchained `VoiceModState::mix`. Direct fade setup at now=0 therefore
uses elapsed16 at audio index15. The prepared runtime witness follows that
existing clock. Round3 `70005b680de9fffa2e218a9d6edc4e448b1925e8` corrects the
native lane's KSP fixture to index15, and its quarter-time fixture to index1199.
A new synchronous-callback/sample-end/default-Linear control checks origins0/128.
The direct runtime helper now reserves one voice rather than zero. These are
test-only corrections, not a changed amplifier clock or measured native phase.

No heap ownership is added during render. Persistent Fade stays 40 bytes (test
assertion); only the temporary returned Ramp carries the optional fade. The
existing modulation state arrays are unchanged. New sine/cosine work occurs
only for active nonlinear fades, O(active fade frames); actual performance must
be measured by the performance owner before CPU/RAM acceptance.

## Prepared regression checks (UNRUN Rust)

- Five independent quarter-time constants and time-mirror values.
- Zero/one/short durations, monotonic bounded gains, exact endpoints and stop
  completion flags.
- Equal-power crossfade sum-of-squares and interrupted current-level continuity.
- Legacy Linear f64 bits and persistent Fade size.
- Synthetic runtime frame 16/128 witness for all five shapes, both directions,
  exact block 1/17/128 PCM equality, chained vs unchained no-double-amplitude,
  and post-endpoint constant output. Round3 adds an UNRUN external allocator
  regression in `tests/fade_curves.rs` using existing `tests/support/mod.rs`.
  The core library forbids unsafe code; no allocator wrapper was included there.
- Native lane prepares KSP variable/array selector, dynamic stop, invalid value,
  optional-default and operand-register tests.

Combined NEXT-batch requests: sampler-core lib `kontakt_812_`,
`legacy_linear_fade_keeps_bit_order_and_state_size`, native lane `fade_curve_`,
plus sampler-ksp compile/params `fade_curve_` and complete params regressions.
No Rust job ran in the DSP worker. Own-code tests, when run, prove documented
intent and runtime wiring, NOT measured native equivalence. ALL_EVENTS/by_marks
admission is unchanged and remains separately blocked.
