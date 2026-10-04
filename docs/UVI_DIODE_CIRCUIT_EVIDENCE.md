# Diode Clipper circuit and fixed DC boundary

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
helper `0x141258040`, a separate fixed DC blocker, and OutputGain. The original
Rust leaf implements the circuit and DC boundary before OutputGain. It has at
most 12 independent channel states and admits only the six measured rates:
8/32/44.1/48/96/192 kHz. It is not registered with Program playback.

The channel circuit retains three doubles: capacitor voltage, previous circuit
output and conditioned input. The original mathematical model conditions input
with a tanh limiter, integrates the coupled capacitor/diode system, and solves
the scalar implicit equation with Newton iterations. The solver retains its
prior output as the initial estimate, limits each correction to +/-0.5 and
circuit voltage to +/-1.5, and stops after ten iterations or a correction smaller
than 0.001. Tone and HighPass enter the circuit through frequency times a
float32-rounded tau divided by rate.

Scaling the implicit equation by its reverse exponential preserves the native
finite-iteration trajectory in stiff asymmetric transitions. A simpler,
algebraically equivalent residual matched moderate fixtures but differed by
about 1.5e-7 in the first failing high-amplitude Asymmetry=1 case. Passive
observation located that mismatch in the circuit Newton trajectory, before DC
or gain. The independently authored factored model and production Rust now
match that failing case and the wider matrices exactly. The candidate therefore
accepts the official Asymmetry range; sampled values are 0, 0.5 and 1. These
comparisons establish sampled behavior rather than exhaustively testing every
possible floating-point control/input combination.

Circuit measurements use 432 authored cold/warm cases spanning six rates,
1/2/6/12 channels, seeded nonlinear input, buffer lengths 17/32/65 and direct
control edits. Sampled controls include Tone 100/5000/20000, HighPass
1/200/20000 and Drive 0/3/12/30. A further 24 signed high-amplitude cases
(up to +/-32 input), including Asymmetry=1, matched native and the independent
model exactly. Actual native pow/exp/tanh and solver instructions execute
unchanged, with no replacement arithmetic hooks enabled.

The fixed DC coefficient is evaluated from a float32 sample-period reciprocal
as exp(-tau * period), then rounded to float32. Non-stereo channels use a scalar
one-pole subtraction. Exactly two channels use a two-step recurrence evaluated
in four-frame groups, with four float32 history values per channel and a scalar
tail. The distinct operation ordering and stereo phase history are preserved;
a generic one-pole substitution does not reproduce every native output bit.

The unchanged native DC stage and actual compiled Rust match in 624 authored
cases: six rates, 1/2/6/12 channels, both isolated DC and circuit-plus-DC stages,
partial counts 1/2/3/4/5/7/17/31/32/33/65/129/1024, retained warm state and direct
control edits. For the composed stage, the fixture executes the native
processor and passively stops at its first gain-multiplication entry before
any gain arithmetic. The coefficient matches at all six measured rates.

A further 576 composed pre-gain lifecycle cases cover Asymmetry 0/0.5/1,
six rates, 1/2/6/12 channels, counts 1/3/17/32/65/129/1024/4096, high-amplitude
transitions, a long zero tail, initially bypassed and warm-bypassed sequences,
and direct control changes. Active phases match native output bit for bit.
Bypassed phases execute outer wrapper `0x140ecc2d0` on authored caller-owned
effect/context/signal metadata and establish dry passthrough with byte-for-byte
freeze of all measured circuit and DC state, followed by exact active resume.
Three focused Rust tests retain authored native impulse, asymmetric stiff
transition, stereo partial-buffer, bypass, reset, channel independence and
unsupported-control fixtures.

OutputGain calls native SIMD dispatch `0x141686ae0` through runtime slot
`0x14259f060`; its implementation is unavailable in the unchanged loaded-text
snapshot. No replacement multiply or approximated arithmetic hook is used.
Consequently OutputGain, the complete callback, hosted property smoothing,
voice clone/cache behavior and whole-program audio remain unverified. This
leaf exposes the measured circuit/DC helper boundary only. Program admission
remains blocked and its fidelity diagnostic retains these limitations.

The owned Bartok program was decoded only in memory. DiodeClipper node 29
starts bypassed, with Drive=3, Tone=20000, HighPass=1, Asymmetry=0,
OutputGain=-3 and no owned connections. Initial bypass does not waive the
missing gain stage or later activation. No vendor preset, script, bank audio,
source, activation patch, or account state is included.
