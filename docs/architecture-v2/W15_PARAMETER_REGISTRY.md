# Parameter registry plumbing — W15

Implementation step on `v2/w15-modulation`; full slice READY requires the frozen
5fc362f3 22-item PCM gate and quiet CPU acceptance. Neither is claimed here.

`ModRoute::target` is now an open `ParameterAddress { scope, node, parameter }`.
`ModTarget` remains a compatibility alias with legacy constructors. Preparation
compiles legacy operations once, preserving saved reduction order, native enabled
bits, attenuation products, decibel sums and current control clocks. Generic
registered control routes resolve to actual DSP binding lanes. Voice routes cannot
overwrite the shared base or reach a summed bus through an invented reduction.

`Prepared::parameter_registry()` publishes immutable registration-order metadata:
`ParameterDescriptor { address, control, name, unit, range, default, law, display }`.
Scope is in the address; display contains `group: String` and `order: u32`.
Units include Linear, Normalized, Percent, Decibels, Hertz, Seconds, Semitones,
Octaves and Frames. Native aliases resolve to the same lane/ControlId and are
checked against the real engine binding before publication.

Range/default and direct ControlId edits use the existing owner's DSP lane domain.
`Native(EngineParameterLaw)` maps native service position to/from that domain.
A native filter knob can still be normalized or signed normalized: it is not Hz
or dB just because it controls cutoff or gain. A physical response graph needs
that adapter's native frequency/gain curve. Percent preserves its authored range.
Currently lowering publishes continuous IR processor-control bindings; parameter
publication for unbound processors, sources and route meta-parameters remains next.

Generic projection copies only the active chain's parameter span into preallocated
worker-local scratch, applies each route's midpoint before saved-order addition,
and clamps the base sample-clock ramp plus the summed offsets. Base edits remain
shared while held offsets remain independent. No registry lookup occurs in render.
Legacy-only plans allocate no projection scratch. ControlRamp carries optional
projection metadata; its memory/CPU impact remains a measurement gate.

Validation: registry API imports initially failed; the real gain contract initially
failed at unsupported Target::Control lowering. The implementation passes 79 core
unit tests, 25 lowering tests and 7 registry contracts, plus core area no-run.
The gain fixture covers Control and Processor targets, 128 voices, 1/2/4 threads,
1/7/64/137 frame partitions, actual worker execution, shared-base edits and audio
allocation/deallocation guards. The projection test checks clamping after the base
ramp and saved-order reduction. No native PCM or CPU equivalence claim yet.

Audit provenance: frozen product source 5fc362f3. UVI seed42 is a common audit-only
overlay on both sides: W10 deterministic-work/coroutine dependencies 469013bc and
78d159df, seed/barrier 0be9aa7b, and the numeric hash harness. This avoids silently
ignoring the seed in the unmodified 5fc source. Product patches are excluded from
that common overlay; hashes and raw numeric results live in the W15 run receipt.
