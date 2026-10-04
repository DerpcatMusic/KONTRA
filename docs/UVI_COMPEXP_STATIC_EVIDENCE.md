# Bounded static CompExp evidence

Reviewed 2026-10-04. These source-level comparisons establish a bounded DSP
leaf, not whole-host lifetime, complete preset audio parity or performance.
[Official controls](https://lua.uvi.net/_elements.html#compressor-expander)
supply the public defaults and physical domains.

| Admission dimension | Measured gate |
| --- | --- |
| Placement | Shared Program, Layer or AuxEffect processor; every Keygroup ancestor rejects. |
| Rate / width | 32,000, 44,100 or 48,000 Hz; 1, 2, 6 or 12 channels. |
| Fixed state | Bypass = 0, AutoMakeUp = 0, GateThreshold = -130 dB; no connected controls. |
| Compressor | Threshold -130..0 dB; ratio 1..30; makeup -30..30 dB; Mix 0..1. |
| Time controls | Attack 0 or 1..500 ms; release 0 or 2..5,000 ms, for both compressor and dormant gate. |
| Dormant gate | Ratio 0.1..30; its -130 dB threshold is below the native amplitude floor, so gate gain stays unity. |
| Writes / attributes | Only equal finite physical numeric/boolean setters; changed, unknown or nonnumeric writes reject before graph, clock, parameters or processor history mutate. Unknown attributes reject except Name. |

The original native DSP base/core constructors, cold physical callbacks, fresh
empty-history clone, preparation, complete audio wrapper and clear method run
with authored object/parent descriptors and allocation adapters. Original native
powf, expf and memset instructions execute unchanged. No math/copy substitute,
activation patch, instruction patch or global-data patch is used. The metadata
factory and initialized native host are not exercised. Retained PE data is
uninitialized host data; the readable MSVC math uses its raw-image SSE fallback.
Other runtime dispatch and floating-point environments remain unproved.

The actual Rust leaf matches **288 original-native cases / 3,870,720 scalars
bit for bit**: 144 cases cover all 12 observed owned control sets, and 144 cover
authored physical endpoints, across the measured rates and layouts. Native
fresh 32-frame splits and original clear are exact; Rust fresh/reset/17-frame
splits are exact. Zero observed residual is not a universal bit-exact bound.
The implementation preserves linked float32 channel RMS, native attack/release
and post smoothing, scalar makeup/Mix ordering and the delay ring. Lookahead is
31/43/47 frames at the three rates; **Mix = 0 still delays audio**.

Of 12 observed CompExp nodes in eight Starter presets, **11 shared nodes in
seven presets pass the leaf gate**: six Layer, three Program and two AuxEffect
nodes. Many Faces node 127 is inside a Keygroup and stays rejected because its
native note-off/voice-retirement tail policy is unproved. The shared-tail
Renderer check verifies existing routing only, not native voice lifetime.
All eight scripts initialize without CompExp writes. Every whole preset retains
other preflight blockers: **zero whole presets become playable**.

Six focused leaf/Renderer checks pass; the actual Alto regression retains all
six PCM/event/state hashes and counts. Delay storage is bounded at 2,304 bytes
per processor and accounted once; no persistent cache is added. Automatic
makeup, active gate, initial/live bypass, connected/changed controls, short
positive times and unmeasured rates/layouts remain gated. One .001 ms native
preparation reached an unresolved CRT/IAT underflow path; it was not substituted
or retried. Existing-object rate/layout changes and populated-state clone
lifetime are not established. No speed or deadline claim follows.
