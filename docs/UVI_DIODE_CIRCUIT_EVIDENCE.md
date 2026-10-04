# Diode Clipper circuit helper boundary

Measured 2026-10-04 against the unchanged official UVI Workstation 4.0.9 x64
executable (SHA256 `78729e96b752aea746280275072ad24cb4399a053739c49a161ff1fcfbf85721`)
and its earlier loaded-text snapshot (SHA256
`684a5557efed9e426a62cbba75c15cc727d764680e1660164b7c5ae88698fda1`).

[Official element controls](https://lua.uvi.net/_elements.html#diode-clipper)
identify Drive 0..30 dB, Tone 100..20000 Hz, HighPass 1..20000 Hz,
Asymmetry 0..1 and OutputGain -40..10 dB. The
[official Falcon description](https://www.uvi.net/falcon) identifies a circuit
model with tone and high-pass controls.

Native factory `0x140e85410` reaches constructor `0x141257010`; audio callback
`0x1412578f0` invokes processor `0x1412584e0`, which runs the channel circuit
helper `0x141258040`. The channel retains three doubles: capacitor voltage,
previous circuit output and conditioned input. No unbounded delay allocation
is needed for this boundary. Each Rust instance has at most 12 independent
channel states.

The original model conditions input with a tanh limiter, integrates the coupled
capacitor/diode system, and solves the scalar implicit equation with Newton
iterations. The solver retains its prior output as the initial estimate, limits
each correction to +/-0.5 and circuit voltage to +/-1.5, and stops after ten
iterations or a correction smaller than 0.001. Tone and HighPass enter the
circuit through frequency times a float32-rounded tau divided by rate. It is a
distinct circuit rather than a generic clipping curve or generic SVF.

The production Rust and an independent mathematical model matched native
float32 circuit output exactly in 432 authored cases: 8/32/44.1/48/96/192 kHz,
1/2/6/12 channels, original seeded nonlinear cold/warm input, buffer lengths
17/32/65 and direct control edits. Sampled controls include Tone 100/5000/20000,
HighPass 1/200/20000 and Drive 0/3/12/30. A further 24 signed high-amplitude
stress cases (up to +/-32 input) matched exactly. Six sustained-input comparisons
(1024 frames followed by a 4096-frame zero tail) matched exactly. Native
pow/exp/tanh and solver instructions execute unchanged, with **no replacement
arithmetic hooks enabled**.

The candidate strictly requires **Asymmetry=0**. Moderate probes with other
asymmetries matched, but an extreme Asymmetry=1 probe produced float32 differences
around 1.5e-7 in its first failing case. That setting remains rejected rather
than described as exact. The observed failure is separate from the unavailable
native gain dispatch discussed below.

A further 48 authored lifecycle comparisons execute native outer wrapper
`0x140ecc2d0` on caller-owned effect/context/signal metadata while bypassed.
They establish dry passthrough and byte-for-byte freeze of the three circuit
state doubles, followed by exact Rust/native circuit resume. Both initially
bypassed and warm-bypassed sequences are covered; direct control fields are
updated while bypassed. These comparisons do not establish hosted property
smoothing or voice clone/cache behavior. A direct Rust check covers an authored
native impulse, split buffers, bypassed state retention, reset, independent
channels and unsupported-control rejection.

The full native audio callback also applies a separate fixed DC blocker and
OutputGain. This Rust **circuit helper excludes both stages**. The gain stage
calls native SIMD dispatch `0x141686ae0` through runtime slot `0x14259f060`;
its implementation is unavailable in the unchanged loaded-text snapshot.
The fixture does not approximate that arithmetic with a hook or claim full
callback fidelity. The DC-blocker callback and gain processing, hosted control
timing, other asymmetries, voice lifecycle and whole-program audio remain
unverified. The leaf is exported for explicit helper use but **not registered
with Program playback**; the diagnostic retains these boundaries.

The owned Bartok program was decoded only in memory. DiodeClipper node 29
starts bypassed, with Drive=3, Tone=20000, HighPass=1, Asymmetry=0,
OutputGain=-3 and no owned connections. Initial bypass does not waive the
missing callback stages or later activation. No vendor preset, script, bank
audio, source, activation patch, or account state is included.
