# MS20 scalar helper boundary

Measured 2026-10-04 against the unchanged official UVI Workstation 4.0.9 x64
executable (SHA256 `78729e96b752aea746280275072ad24cb4399a053739c49a161ff1fcfbf85721`)
and its earlier loaded-text snapshot (SHA256
`684a5557efed9e426a62cbba75c15cc727d764680e1660164b7c5ae88698fda1`).

[Official element parameters](https://lua.uvi.net/_elements.html#vcf-20) and the
[Falcon manual, VCF-20](https://uvi.s3.us-east-1.amazonaws.com/UVIFC/falcon_2026_manual.pdf)
identify MS20 as the revision-2 VCF with HP/LP morph, resonance trim, reference
voltage, cutoff, resonance and key tracking. This is a distinct circuit model.

Native registration reaches factory `0x140e86c30`, constructor `0x1412af0a0`,
coefficient update `0x1412ae600`, scalar callback `0x1412afb10` and nonlinear
solver `0x1412b0b60`. Channel state is independent, with two integrators, prior
inputs and a retained Newton estimate. The original implementation solves the
implicit midpoint circuit, uses two internal steps and averages midpoint
outputs. Measured fixed Trim=0.5 coefficients are 2.065561 and 1.749079.

The compiled Rust leaf and a separate original mathematical model matched
float32 output exactly in 33 authored helper comparisons: nonlinear cold
signals at 8/32/44.1/48/96/192 kHz, cutoff 20/1000/19000 Hz, Q 0/0.5/0.75/1,
morph 0/0.37/1 and voltage 0.5/1/5; warm direct field transitions at
32/48/96 kHz. Additional 2/6/12-channel native comparisons confirmed independent
state. A direct Rust check covers an authored native impulse, split buffers,
reset, independent channels, invalid controls and an unmeasured-rate rejection.

The isolated Unicorn oracle executes native coefficient, callback and solver
instructions. Scratch allocation and float32 multiply primitives are authored
hooks because SIMD dispatch globals are absent from the snapshot. This does
not establish connected-control timing, other trim/key-tracking values, SIMD
parity, or whole-program audio fidelity. The separate outer-boundary measurements
below establish only explicit physical-point dispatch and bypass behavior.
The leaf is exported for explicit scalar use but is **not registered with
Program playback**. Its diagnostic and rate gate retain that distinction.

The owned Augmented Orchestra Bartok program was decoded only in memory.
Its 388 MS20 inserts all start bypassed; each has Freq=1008.4754, Q=0.5,
Morph=1, ResonanceTrim=0.5, ReferenceVoltage=1 and KeyTracking=0.
There are 2716 Freq connections and 2328 Q connections. Those inserts remain
unsupported: initial bypass cannot waive connected controls or later activation.
No vendor preset, script, bank audio, source, activation patch or account state
is included in this change.

## Explicit physical-point dispatcher (still unadmitted)

The next authored fixture executes native outer wrapper `0x140ecc2d0`, parameter
manager `0x14134a480`, MS20 control dispatcher `0x141532cf0`, native Freq/Q
callbacks and the scalar kernel. Caller-owned effect/context/parameter metadata
and memory allocations replace missing host setup; native stack checking runs
with an authored thread stack-limit record. The prior float32 multiply hooks
remain. This is an isolated native instruction comparison, not a DLL-hosted
whole-program render.

Native Freq mapper `0x14134fad0` maps normalized controls exponentially from
20 to 20000 Hz. Native parameter smoothing uses float32
`1 - pow(0.33f32, 100/rate)` per sample and
`1 - pow(0.33f32, 3200/rate)` per full interval. The initial point comes from the
native getter rather than a zero control. A partial final interval advances its
persistent smoother by the actual number of samples and generates a padded
endpoint separately. An independent float32 arithmetic fixture matched 32 native Q-clock cases
(1/17/31/32/33/65/129/256-frame buffers at the four rates), including both
persistent future and padded endpoint bits. Smoothing is not implemented by
this leaf.

MS20 consumes physical Freq/Q at each interval start and holds it for up to 32
frames. When bypassed, the wrapper selects the final interval's controls, passes
audio through, and freezes integrators, prior inputs and retained Newton state.
Resuming uses that retained state. `process_control_points` mirrors this explicit
boundary: it accepts already-prepared physical points and never generates,
smooths or combines connections. Its rate gate is 32/44.1/48/96 kHz.

The production dispatcher matched native float32 output exactly in 32 authored
cases across those four rates, initial Freq=1000/Q=0.5, edits to Freq=4000/Q=0.9,
warm sine input, buffer lengths 1/17/31/32/33/64/65/96/128/129/256, and middle-buffer
bypass/resume. Comparisons feed measured native physical points into compiled
Rust; they prove point dispatch and DSP, not independent control generation.
The dry buffer is separately checked against authored input and the native
state snapshot is checked for freeze.

The actual Bartok connections have not been exercised through the native
connection-combination manager. Their presence therefore still prevents
Program playback admission, including while initially bypassed.
