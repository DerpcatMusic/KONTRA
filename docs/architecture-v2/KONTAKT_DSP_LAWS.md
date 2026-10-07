# Kontakt DSP laws

Research branch: `v2/gpt-decipher-dsp`. Parser baseline: `699758ab`.
Evidence date: 2026-10-08. This is a factual compatibility specification,
not an implementation. A verified local law does not establish a complete module.

## Evidence and reproduction

The original x64 instruction checks execute the Kontakt 8.13.1 standalone EXE
with SHA-256 `0fe6356e0879d058b6e5b73507c54c5e345cea451b35287c974e438291d4dae8`.
They do **not** execute the Kontakt 8 plugin payload. RTTI/payload target identities
are leads; parity between these two images remains unverified.

The read-only reference is `t3code-80fe786b/docs/DSP_SYSTEM_INVENTORY.md`,
sections “Original machine-code DSP checks” and “Kontakt source engines and
signal flow”, and `DSP_FORMAT_SPECIFICATION.md`, “Effect parameter payloads”.
Its artifacts root is
`/home/derpcat/.t3/worktrees/KONTAKTO/t3code-80fe786b/artifacts/engine-analysis-2026-10-07`.

Run `python tools/dsp-research/verify_laws.py ARTIFACT_ROOT OUTPUT_JSON`.
Already-installed Unicorn and pefile map the hash-checked original image;
each call has a two-second/two-million-instruction bound. No host launches,
activation changes, library dumps, decoded presets or samples are involved.
[Committed vectors](KONTAKT_DSP_LAWS.vectors.json) contain 1,157 result records,
including 480 individual spline evaluations in 24 table records.

Every arithmetic operation marked F rounds to IEEE binary32, ties to even.
D means binary64. MXCSR is `0x1f80`; flush-to-zero behavior is not established.
The check substitutes Python math for CRT exp/expf/pow/powf/sincosf and memory
helpers. Exact comparisons establish arithmetic around those substituted
results, not Windows CRT last-bit parity. Mix preparation/finalization, base
copy/reset, selected core dispatch, and Reverb topology are stubbed as declared
in the harness. Constructor allocation, full preset/voice lifecycle, host
resampling and complete effect rendering remain outside those checks.

## Corpus census and ranking

`crates/sampler-kontakt/examples/dsp_census.rs` counts saved enabled/bypassed
slots as well as lowered IR processors, modulators, source modes, gain units,
pan laws, envelope shapes, velocity and key-tracking routes. Saved identities
are essential: the current IR drops or substitutes some authored effects.

The corpus is `~/.cache/kontakto-corpus/items.tsv`; per-item aggregate caches
are under `~/.cache/kontakto-gpt-decipher-dsp/items/`. Each shard has a 240-second
soft budget and an outer 300-second timeout, runs through `kontakto-heavy`,
and releases its heavy slot before the next shard. Cache output contains only
identities/counts and hashed source metadata. Ranking is pending the running
shards; no count or usage priority is inferred from the user's candidate list.

Muted groups are counted but their detailed modules are skipped. “Enabled”
internal modulation means flag byte 1 is zero and targets are nonempty, a
parser interpretation rather than host execution proof. Snapshot/script
changes after parsing are not represented. Signed modulation depth predates
the FX/mod agent's correction; reconcile with `v2/gpt-format-fxmod` before
using route signs. Parse/IR errors must be reported with the census denominator.

## AHDSR timing, attack curve and control kernel

**Verified:** control conversion `0x140ae46d0` (125 cases), uninterrupted
control kernel `0x140ae40f0` (54 uninterrupted cases), retrigger/setup
`0x140ae5820` and stage-transition helper `0x140ae5e40` (90 lifecycle cases).
**Open:** external note-off scheduling, voice reset/allocation, one-shot flags,
curve-only update invalidation, resampling cadence and host latency. The object rate is a **control rate** R;
these checks do not prove R = sample rate / 32.

For clamped normalized time x, attack/hold milliseconds are
`F(F(exp(F(F(x × 8.922792434692383) + F(ln 2)))) − 2)`;
decay/release replace the multiplier by `9.433565139770508`.
The stored length is `trunc(F(F(milliseconds × F(0.001)) × F(R)))`.
Sustain stores `F(F(F(x × x) × x) + F(0.075))`.

For attack curve c in [-1,1], let
`b = F(exp(D(F(1 − abs(F(c)))) × ln(500000) − ln(20000)))`.
For c > 0, the start is `F(b + 1)` and ratio `F(b / start)`;
otherwise the start is b and ratio `F(F(b + 1) / b)`.
An attack length N > 0 stores `F(ratio^(1/N))`; decay and release
store `F((3/43)^(1/N))`. Updating a zero length on the zero-filled test
object leaves its multiplier zero; that result is not a zero-time stage law.

At each uninterrupted tick the output is
`F(F(F(state − F(0.075)) × scale) + bias)` using the **old** state;
then `state = F(state × multiplier)`. Both tested counters decrease by one.
The test covers stages 0 through 5 and lengths 0,1,3,4,5,31,32,33,129,
with counters initially 1,000 so no stage boundary is crossed.

At R = 1,500 and c = 0:

| x | attack/hold ticks | decay/release ticks | sustain storage | attack multiplier | decay/release multiplier |
| ---: | ---: | ---: | ---: | ---: | ---: |
| 0 | 0 | 0 | 0.07500000298023224 | 0 | 0 |
| .25 | 24 | 28 | .09062500298023224 | 1.0016355514526367 | .9092888832092285 |
| .5 | 256 | 332 | .20000000298023224 | 1.0001531839370728 | .9920122027397156 |
| .75 | 2415 | 3543 | .49687498807907104 | 1.000016212463379 | .9992488026618958 |
| 1 | 22500 | 37500 | 1.0750000476837158 | 1.0000017881393433 | .9999290108680725 |

Curve 0 has start 25 and ratio `1.0399999618530273`. This geometric
state and affine output require preserving the native floor; substituting
a generic normalized exponential envelope is not supported by the evidence.


### Stage initialization and release termination

The six tested stages are attack, hold, decay, sustain, release and inactive.
Durations occupy six consecutive signed counters. Sustain and inactive use
`INT_MAX`; stage multipliers for hold/sustain/inactive are 1. Stage starts are
attack b (or b+1), hold/decay/release `F(1.075)`, sustain storage, and inactive
`F(.075)`. The floor is `F(.075)`. Stage entry snaps state to its start and
counter to its duration. Zero-length stages are skipped.

Retrigger selects the first nonzero-duration stage. Attack affine output has
scale +1 and bias `F(floor−start)` for c <= 0, or scale −1 and bias
`F(start−floor)` for c > 0. Hold and sustain have scale 1/bias 0. Decay has
scale `F(1−sustain_level)` and bias sustain_level, where sustain_level is
`F(sustain_storage−floor)`; the special flag at +0x118 is zero in these checks.
Release captures the preceding affine output as its new scale, sets bias 0,
and restarts geometric state at `F(1.075)`. Inactive sets scale/bias to zero.

A separate release countdown is decremented on control ticks. Expiry moves
any stage before release into release, before producing the next tick.
The tested countdown is installed directly, so no MIDI-to-countdown law is
claimed. Exactly N release ticks are produced for a positive release length N;
the following ticks are zero. A zero release length skips to inactive.
Ordinary stage transitions and release expiry use the persistent state and
counters across blocks. For these checked paths, whole-block and split-block
output agree exactly, including empty blocks.

The lifecycle vectors cover curves −1,0,+1; attack/hold/decay/release lengths
(7,3,11,13), (0,3,11,13), (0,0,11,13), (0,0,0,13), (7,0,0,0);
release countdowns 0,5,25; and partitions [70] versus [0,1,3,4,5,31,26].
Each vector includes all 70 output ticks and final stage/state. Sustain is
.125. All finish inactive with exact zero output. These finite synthetic
counter cases establish state transitions, not the host's audio tail metadata.

## Legacy parametric EQ

**Verified:** saved conversion `0x1409590a0` (160 cases) and peaking
coefficient generator `0x1409591c0`, selector byte 7 (225 cases).
**Open:** other selector modes, processing recurrence/history, coefficient
smoothing/cadence, reset, host latency and tail. This is not Solid or Pro EQ.

The normalized frequency conversion uses a float-bit logarithm approximation.
Define u = clamp(F(F(hz − 20) × F(0.000050050050049321726)),0,1),
t = F(F(u × 468.7109375) + 1), and
z = F(F(signed_bits32(t) × 2^-23) − 127).
For fraction q = F(z − floor(z)), the normalized frequency is
`F(F(z + F(F(q − F(q × q)) × F(.346607))) × F(.11257956176996231))`.
Width is `F(F(octaves − F(1/3)) × F(.375))`; gain is approximately dB / 18,
with the original conversion's rounding retained in the vectors.

For coefficient generation define E(z) by q = F(z − floor(z)) and
`E(z) = float_from_bits(trunc(F(F(F(z + 127) − F(F(q − F(q × q)) × F(.33971))) × 8388608)))`.
This bit approximation, rather than a true exponential, is part of the law.
For normalized frequency f, bandwidth w, signed gain g:

- u = F(F(E(F(f × F(8.882606506347656))) − 1) × F(.0021335112396627665)).
- hz = F(F(clamp(u,0,1) × 19980) + 20).
- omega = F(min(F(hz/Fs),F(.49)) × 2π); s = F(sin omega), c = F(cos omega).
- A = E(F(F(F(F(g × .5) + .5) × 6) − 3)).
- B = 2^D(F(F(w × F(2.6666667461395264)) + F(1/3))).
- alpha = s × (.5 / (1 / (B − 1/B))), p = alpha/A, q = alpha×A, d = 1/(1+p), all D.
- Stored coefficient order is F((q+1)d), F(-2cd), F((1-q)d), F(2cd), F((p-1)d).

At Fs = 48,000, w = .5, g = .5:

| f | hz | coefficients in stored order |
| ---: | ---: | --- |
| 0 | 20 | 1.0092595816, -1.9973512888, .9880983829, 1.9973512888, -.9973580241 |
| .5 | 903.2728881835938 | 1.3942785263, -1.8743262291, .4932261407, 1.8743262291, -.8875046968 |
| 1 | 20000 | 2.4136760235, 1.3827401400, -.8170252442, -1.3827401400, -.5966507792 |

Saved 1,000 Hz / 1 octave / +6 dB converts to
`.516291081905365, .2499999850988388, .3333333730697632`.
All exact float values and Fs = 8,000/48,000/192,000 cases are in the vectors.

## Stereo Modeller

**Verified:** settled stereo matrix, pseudo-stereo delay and local reset
(150 cases), moving width/pan and output samples (96 cases).
Entries are `0x140aa5ac0` and reset `0x14094cd90`.
**Open:** channel layouts other than two channels, cross-.5 width transitions,
pseudo-mode automation, base reset, host latency reporting and full bypass/mix.

For settled width w >= .5, spread = 2w−1, with native F order;
L' = (1+spread)L−spread R, R' = (1+spread)R−spread L.
For w < .5, a = .5−w;
L' = L+a(R−L), R' = R+a(L−R).
Thus w = .5 is identity; w = 0 is mono average, w = 1 is (2L−R,2R−L).
Pan p in [-1,1] multiplies L' by 1−max(0,p) and R' by 1+min(0,p).
This is a linear stereo balance law. Separate expression ordering applies to
moving versus settled width; exact sample comparisons are in the harness.

Width state updates once per sample with k = F(1/180):
`w_next = F(w + F(F(target − w) × k))`. Output uses w before advancing it.
Pan uses k = F(1/1800) and also outputs the prior state. In each four-sample
SIMD group, advances 1,2,3 use their respective one-pole deltas, but advance 4
**reuses advance 3's delta**. A scalar remainder uses a fresh delta each sample.
SIMD groups restart at each process call; block partitioning can therefore
change the pan trajectory. Moving-width tests stay on one side of .5.

Pseudo mode keeps L immediate and delays R by
`min(1023,trunc(F(F(F(F(Fs × F(.01)) × w) × w) × w)))` frames
in the checked settled path, followed by balance. Its 1,024-float ring buffer
is initially zero. At 48 kHz, w = .5 gives 60 frames, w = 1 gives 480;
at 192 kHz, w = 1 saturates at 1023. This is an asymmetric delay;
no whole-module latency is inferred. The ring contents imply a finite
remaining right-channel tail in the fixed-delay path, not a measured host tail.

Local reset clears 4,096 ring bytes and its cursor and snaps width/pan state
to their targets. Its base-reset call is stubbed. Width .6→.9,
pan −.8→−.2, four frames ends at width `.6066113710403442`,
pan `−.7986676692962646`. Exact vectors cover 1,3,4,5,31,32,33,1024 frames.

## Modern wrapper control cadence

**Verified:** outer wrappers for Replika `0x1408f98d0` (53 controls),
Reverb `0x1408fc160` (11), Solid EQ `0x1408fbd60` (16),
Solid Bus Comp `0x1408fbb60` (11), 48 block observations altogether.
**Open:** core audio processing and smoothing, other channel layouts,
true parameter-buffer advancement and lifecycle initialization/reset.

A persistent frame countdown splits audio into chunks of at most 32 samples.
When countdown < 1, it is set to 32, enabled parameter pointers are read in
index order and clamped to [0,1], and the core update callback runs. Update
runs even when all parameter-enable bytes are zero. Processing decrements
the countdown and advances channel pointers by the number of frames.
A zero-frame block performs no update. The countdown persists across calls.
These statements describe outer scheduling; the stubbed core process/update
callbacks cannot establish inner smoothing or audio behavior.

With initial countdown 5 and 7 frames, process 5 first, then parameter/update,
then process 2; final countdown is 30. A following 65-frame block processes
30, then 32, then 3, with two updates; final countdown is 29.

## Modern Reverb / Galois parameter storage

**Verified:** ten setters `0x141aea370`, 50 cases.
**Open:** parameter names/units and mapping of the wrapper's eleventh control,
size-dependent topology, all delay/filter coefficients, smoothing, state,
reset, latency, tail, and identity/parity with Raum.

These are internal storage values, not a promise of UI units:

| core index | law, retaining F arithmetic | x=.5 storage |
| ---: | --- | ---: |
| 0 | F(F(exp(F(x × F(3.7)))) × .5) | 3.1799099445343018 |
| 1 | F(F(x × 3) + 1) | 2.5 |
| 2 | F(x^.25) | .8408964276313782 |
| 3 | F(x × 10) | 5 |
| 4 | F(x × .5) | .25 |
| 5 | F(x × 11025) | 5512.5 |
| 6 | F(F(F(1−x) × 19000) + 2000) | 11500 |
| 7 | F(x × −12) | −6 |
| 8 | x | .5 |
| 9 | x | .5 |

The topology helper for index 1 is stubbed. The quarter-power conversion
must not be replaced by linear damping. Tests cover x = 0,.25,.5,.75,1.

## Solid EQ parameter splines

**Verified:** original eleven-knot tables, initializer `0x141a9a510` and
evaluator `0x141a98340`, 22 EQ tables plus two Bus Comp tables, 480 evaluations.
**Open:** bank selection and saved-to-core index permutation, nonlinear
coefficient formulas and processing, smoothing, reset, latency and tail.
The label is the **core descriptor index**, not the saved FX field index.

The grid is D(F(i × F(.1))) for i = 0..10. It is not exact decimal tenths.
Each table uses a cubic spline clamped to its grid endpoints. For segment i,
h = x[i+1]−x[i], a = (x[i+1]−u)/h, b = (u−x[i])/h, the value is
`a*y[i] + b*y[i+1] + ((a³−a)*m[i] + (b³−b)*m[i+1])*h²/6`, all D.
The checked boundary controls 11 and 1e32 select **zero slope at the left**
and **zero second derivative at the right**. The value 11 is a boundary-mode
control; it is not an endpoint derivative. Interior second derivatives obey
`h_left*m[i−1] + 2*(h_left+h_right)*m[i] + h_right*m[i+1] = 6*(slope_right−slope_left)`.
All knots, second derivatives and evaluations are committed in the vectors.

Bus Comp table 0's apparently linear knots produce −17.32959641360375 at
u=.05, rather than linear −16.74, because of the left boundary. EQ bank0
and bank16 tables differ at indices 3 and 6; selecting one universal gain
curve would lose those differences. Constructor-to-bank binding is still open.

| core table | knots at grid indices 0..10 |
| --- | --- |
| BusComp:0 | -18.6, -14.88, -11.16, -7.44, -3.72, 0.0, 3.72, 7.44, 11.16, 14.88, 18.6 |
| BusComp:1 | -5.25, -2.625, 0.0, 2.625, 5.25, 7.875, 10.5, 13.125, 15.75, 18.375, 21.0 |
| SolidEQ:bank0:0 | -20.5, -18.0, -11.0, -6.0, -2.0, 0.0, 2.0, 6.0, 11.0, 18.0, 20.5 |
| SolidEQ:bank16:0 | -20.5, -18.0, -11.0, -6.0, -2.0, 0.0, 2.0, 6.0, 11.0, 18.0, 20.5 |
| SolidEQ:bank0:1 | 1539.0, 1710.0, 2400.0, 3980.0, 5460.0, 6820.0, 9160.0, 13100.0, 17813.0, 21000.0, 21800.0 |
| SolidEQ:bank16:1 | 1539.0, 1710.0, 2400.0, 3980.0, 5460.0, 6820.0, 9160.0, 13100.0, 17813.0, 21000.0, 21800.0 |
| SolidEQ:bank0:3 | -20.0, -17.5, -11.0, -6.0, -2.0, 0.0, 2.0, 6.0, 11.0, 17.5, 20.0 |
| SolidEQ:bank16:3 | -20.0, -16.8, -9.0, -4.5, -1.8, 0.0, 1.8, 4.5, 9.0, 16.8, 20.0 |
| SolidEQ:bank0:4 | 692.48, 730.72, 1022.48, 1614.67, 2112.54, 2619.29, 3291.53, 4544.29, 5634.35, 6444.74, 6892.65 |
| SolidEQ:bank16:4 | 692.48, 730.72, 1022.48, 1614.67, 2112.54, 2619.29, 3291.53, 4544.29, 5634.35, 6444.74, 6892.65 |
| SolidEQ:bank0:5 | 0.65, 0.62, 0.58, 0.5, 0.45, 0.4, 0.37, 0.31, 0.26, 0.22, 0.22 |
| SolidEQ:bank16:5 | 0.65, 0.62, 0.58, 0.5, 0.45, 0.4, 0.37, 0.31, 0.26, 0.22, 0.22 |
| SolidEQ:bank0:6 | -21.5, -18.0, -11.0, -6.0, -2.0, 0.0, 2.0, 6.0, 11.0, 18.0, 21.5 |
| SolidEQ:bank16:6 | -21.5, -18.0, -9.2, -5.0, -1.8, 0.0, 1.8, 5.0, 9.2, 18.0, 21.5 |
| SolidEQ:bank0:7 | 223.0, 239.0, 323.0, 494.0, 668.0, 809.0, 1050.0, 1479.0, 1841.0, 2083.0, 2200.0 |
| SolidEQ:bank16:7 | 223.0, 239.0, 323.0, 494.0, 668.0, 809.0, 1050.0, 1479.0, 1841.0, 2083.0, 2200.0 |
| SolidEQ:bank0:8 | 0.65, 0.62, 0.58, 0.5, 0.45, 0.4, 0.37, 0.3, 0.25, 0.21, 0.2 |
| SolidEQ:bank16:8 | 0.65, 0.62, 0.58, 0.5, 0.45, 0.4, 0.37, 0.3, 0.25, 0.21, 0.2 |
| SolidEQ:bank0:9 | -19.5, -16.0, -10.0, -5.3, -2.0, 0.0, 2.0, 5.3, 10.0, 16.0, 19.5 |
| SolidEQ:bank16:9 | -19.5, -16.0, -10.0, -5.3, -2.0, 0.0, 2.0, 5.3, 10.0, 16.0, 19.5 |
| SolidEQ:bank0:10 | 40.43, 44.64, 63.44, 101.36, 140.21, 183.73, 245.15, 351.55, 513.3, 614.68, 697.34 |
| SolidEQ:bank16:10 | 40.43, 44.64, 63.44, 101.36, 140.21, 183.73, 245.15, 351.55, 513.3, 614.68, 697.34 |
| SolidEQ:12 | 14.5, 16.0, 23.0, 38.0, 57.0, 74.0, 100.0, 162.0, 280.0, 380.0, 460.0 |
| SolidEQ:13 | 31000.0, 29000.0, 25200.0, 19400.0, 15500.0, 12300.0, 10500.0, 8100.0, 4800.0, 3450.0, 3300.0 |

## Solid Bus Comp timing

**Verified:** indices 2 (attack) and 3 (release), setter `0x141ac69e0`,
coefficient helper `0x141acc420`, 120 cases across five sample rates and
all 16 stored channel lanes. **Open:** full descriptor initialization,
threshold/makeup index binding, ratio law, detector, linking, automatic
release state machine, gain smoothing, reset, latency and tail.

On a descriptor spanning 0..5, enum = floor(D(F(x))×5+.5).
Attack table milliseconds are [.1,.3,1,3,10,30]. Release table values
are [.2,.3,.6,.8,1.6,1.2], multiplied by 62.5, giving
[12.5,18.75,37.5,50,100,75] milliseconds. Release enum 5 sets the auto flag.
These internal values do not establish the advertised UI release times or
auto-release behavior; extra stages may reinterpret them.

Each stored coefficient is `min(1,exp(ln(.8)/(milliseconds×.001×Fs)))`, D.
At Fs=48,000 and x=.5, enum=3: attack 3 ms gives
`.998451592027297`; release 50 ms gives `.9999070278424647`.
At x=1 release is 75 ms, `.9999380176011787`, with auto flag set.
Input boundaries .099/.1/.101 and .899/.9/.901 are checked as float32 inputs.

## External performance modulator timing

**Verified:** `BExtMod` queue renderer `0x140ceaf60`, 15 byte-check cases.
**Open:** event-to-control-tick normalization, velocity/key value conversion,
queue insertion/overflow, shapers/lag/depth and destination gain/pitch laws.
The block unit is the caller's control ticks, not a verified audio-frame clock.

The renderer holds its prior float32 value until a queued event tick, then
holds the event's supplied value. No interpolation, clamp or smoothing occurs
in this local kernel. Signed values pass through. Equal-tick events apply
in queue order, so the last value at that tick wins. Events with tick < block
length are consumed; events at exactly block length remain queued with tick
zero. Remaining event ticks decrease by block length; the final emitted value
persists across calls. A zero-length call does not consume a tick-zero event.

Initial value .25 and events (0,−1), (2,.5), (5,1), (8,−.25) produce
`[−1,−1,.5,.5,.5,1,1,1,−.25,−.25,−.25,−.25]` over 12 ticks.
An event (12,.75) remains queued as (0,.75) after that block, whose output
remains .25. The checks compare [12], [0,1,2,3,4,2] and [2,0,2,4,4],
including buffer-end sentinels, exact queued records and persistent state.

## Velocity-to-volume: prior host observations

The [reference protocol](REFERENCE_PROTOCOL.md), “Findings from the calibration
(2026-10-07)”, records the following relative gains for a bare mono noise
instrument with default Velocity-to-Volume, CC7=127 and velocity 127 at 0 dB.
These are prior host measurements, not new byte checks or a universal
instrument law. The saved depth and target/shaper state must be known before
a formula can be specified. The protocol's rough fitted amplitude
`0.22 + 0.78*(v/127)^1.4` is an approximation and is **not** an exact DSP law.

| velocity | relative dB |
| ---: | ---: |
| 1 | −12.9 |
| 8 | −12.0 |
| 16 | −11.0 |
| 32 | −9.1 |
| 48 | −7.4 |
| 64 | −5.7 |
| 80 | −4.1 |
| 100 | −2.30 |
| 110 | −1.42 |
| 127 | 0 |

That protocol separately measures the instrument CC7 amplitude law as
`(CC7/127)^3`, with unsent CC7 after restart at .5 amplitude. This is a
controller observation and must not be substituted for velocity response.
The census's `ir_velocity=None` also does not mean velocity is inaudible:
authored velocity modulation can remain in the separate modulation graph.

## Other required modules and performance laws

| module/law | established here | remaining evidence needed |
| --- | --- | --- |
| Replika | 53-control wrapper cadence | names, conversions, core coefficients/smoothing, delay state/reset, latency/tail |
| Raum | none; Reverb is not equated to Raum | positive identity, complete parameter/audio/lifecycle checks |
| Transient Master | saved layout in reference spec | all parameter and detector/smoother laws, reset/latency/tail |
| Supercharger / GT | no verified law | variant identity, nonlinear transfer, detector/timing, reset/latency/tail |
| Pro EQ | no verified law | distinction from legacy/Solid EQ, coefficients/smoothing/state |
| velocity → volume | prior host vectors and external queue timing; saved/IR census | shaper, signed depth, combination with base gain, event normalization and effective amplitude law |
| key tracking | saved flag / IR census path only | pitch/volume route conversion, shaping and event cadence |
| amplifier / group pan | IR enum and unit census only | native mapping and placement relative to modulation/effect chains |

Gainer, Daft and compressor variant-0 signed channel averaging remain the
prior verified local laws in the reference inventory; none is promoted to a
complete host-compatible processor by this document. The DSP agent should
consume the verified domains above and retain explicit gaps elsewhere.
