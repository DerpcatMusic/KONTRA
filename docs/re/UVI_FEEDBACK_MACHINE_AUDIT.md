# Feedback Machine: graph ownership and calibration boundary

The translator currently drops `FeedbackMachine`, including its nested effect
chains, and retains unsupported diagnostics. No simple-delay substitute or DSP
parity claim is justified. The official module is an Effect Rack with delayed
output feedback into its chain input, plus a global mix. Public controls are
DelayTime (0.002–1 second, default 0.005), Feedback (0–1, default 0.5), and Mix
(0–1, default 0.5). [UVI Effects](https://manual.uvi.net/falcon/en/effects/),
[UVI parameter definitions](https://lua.uvi.net/_elements.html)

This audit read the existing Workstation409 project and prior focused local
extractions for executable SHA256
`78729e96b752aea746280275072ad24cb4399a053739c49a161ff1fcfbf85721`.
It stores addresses, numeric constants and findings only. No native renderer
was executed, and the analyzed snapshot is not interchangeable with the packed
on-disk executable at every DSP address. Static decompiler argument types need
instruction/runtime confirmation before implementation.

| Observed native ownership | Address / offset |
| --- | --- |
| Factory and allocation | `0x140e85c90`, 0x248 bytes |
| Constructor, inheriting rack constructor `0x1412441a0` | `0x141246b70` |
| Owner vtable | `0x141e6ced8` |
| Owner process slot | +0x1f8 → `0x141247480` |
| Child collection | owner +0x218 through +0x220 |
| Three owned audio-buffer objects | owner +0x230, +0x238, +0x240 |
| Channel-layout update / buffer preparation | `0x1412473e0` / `0x141247350` |
| DelayTime scalar setter / getter | `0x141247f60` / `0x141247f90`, context +0x1d8 |
| Feedback scalar setter / getter | `0x141247f70` / `0x141247fa0`, context +0x1d0 |
| Mix scalar setter / getter | `0x141247f80` / `0x141247fb0`, context +0x1d4 |

The process walks nested children through their +0x1c0 wrapper. It works in
chunks of at most 64 frames, prepares channel-local delay state, processes the
children, and writes the feedback storage after the child pass. Consequently,
flattening children into the outer serial insert chain would lose the feedback
ownership and processing boundary. Exact branch accumulation and the three
buffer roles still require the carrier/helper trace; a pointer list alone is
not proof of serial or parallel summation.

The static process computes square roots of Mix and 1–Mix for the wet/dry
targets. Native numeric reads confirm constants 1, 64, +2, −2 and 0.1. The
corresponding paths indicate a minimum 64-frame delay target, 100 ms control
smoothing, and a ±2 clamp before feedback storage is written. These are bounded
static observations, not a whole-module equation or measured audio vectors.
Delay interpolation, smoothing edge behavior, bypass/tail behavior and correlated
wet/dry levels remain uncalibrated.

The smallest useful original-code calibration starts with the scalar controls,
the smoothing helpers `0x141389f90` / `0x141389d80`, delay read
`0x1416485a0`, delay write/advance `0x141648470` / `0x141648450`, and buffer carrier
`0x140f85b60`, then exercises owner process `0x141247480` with authored empty
and known-gain child racks. It must retain the original code and reject unresolved
runtime dispatch rather than substitute neutral helper stubs.

Required vectors include independent mono/stereo impulses and signed DC, empty
and two-branch gain racks, Mix endpoints and midpoint, Feedback 0/0.5/1, exact and
fractional delay targets, 44.1/48/96 kHz, and blocks 0/1/63/64/65/256. Consecutive
blocks must check persistent delay/ramp state, per-channel isolation, live control
steps, bypass/tails, clipping boundaries and guard bytes. Only after these laws
match should a bounded nested graph processor be added to IR/core and wired to
the existing XML insert identities. Warnings remain until that consumer exists.

Numeric-only local receipts are `feedback-numeric-native-map.json` and
`feedback-control-native-map.json` in the private asset-audit artifacts. The prior
survey's 620-program exposure is a historical census, not a new runtime count.
