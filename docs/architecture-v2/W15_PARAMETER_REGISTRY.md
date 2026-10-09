# Parameter registry plumbing — W15

Implementation step on `v2/w15-modulation`; full slice READY requires the frozen
5fc362f3 22-item PCM gate and quiet CPU acceptance. Full acceptance remains HOLD.

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
failed at unsupported Target::Control lowering. The implementation passes 80 core
unit tests, 26 lowering tests and 7 registry contracts, plus core area no-run.
The gain fixture covers Control and Processor targets, 128 voices, 1/2/4 threads,
1/7/64/137 frame partitions, actual worker execution, shared-base edits and audio
allocation/deallocation guards. The projection test checks clamping after the base
ramp and saved-order reduction. No native PCM or CPU equivalence claim yet.

Audit provenance: frozen product source 5fc362f3. UVI seed42 is a common audit-only
overlay on both sides: W10 deterministic-work/coroutine dependencies 469013bc and
78d159df, seed/barrier 0be9aa7b, and the numeric hash harness. This avoids silently
ignoring the seed in the unmodified 5fc source. Product patches are excluded from
that common overlay; hashes and raw numeric results live in the W15 run receipt.

Polyphonic filter regression: a generic projected cutoff/Q must not enter the
shared coefficient cache. The failing-first 128-voice fixture exposed sibling
cutoff reuse. Projected filters now prepare a bounded stack cache, keeping both
per-voice history and sample-clock base ramps; unprojected siblings retain the
shared base cache. The fixture requires exact ordered isolated-voice sums at
1/2/4 threads, including unmodulated siblings and RT heap guards.
Validation after the cache fix: 134 targeted core/lowering/registry/control-DSP/
SVF/multicore/trace tests pass. Quiet CPU acceptance remains open.

Final frozen candidate: b2bd75994b311bbb3195dab2cf4f17deb1a3b344.
The 22-item comparison now has all 23 program pairs bit-identical with equal
reported diagnostic counters (22 audible, one identically silent). W8 completed
exactly one fresh 400-second run per side for rows 11 and 19, with the unchanged
frozen binaries, plans, manifest and seed42/common overlay. Both full PCM records
match. Earlier 140-second timeout receipts remain intact; no cause is attributed.
PCM preservation passes. Timings were contended; quiet CPU remains UNKNOWN.

Numeric receipts and frozen binary identities are under
`/mnt/Windows11/DEV_WORKSPACE/kontra-runs/w15-registry-5fc362f3/`:
`acceptance.json`, `comparison.json`, `provenance.json` and the three-line
`PARK.txt`. Both builds asserted the same audit overlay SHA256
`9d73c3db8594b4699a0d07e3b26e9067cf612c42c700850a313f7e6074390ef3`.
PCM was hashed in RAM; no library WAV or decrypted payload was persisted.
`rows-11-19-w8.json` closes the PCM hold; W6 owns remaining quiet CPU acceptance.
The next W15 slice is the real EQ gain owner and physical band routing.
