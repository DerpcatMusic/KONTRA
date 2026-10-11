# VWinds level regression investigation (survey item 1)

Survey measurements (release CLAP host, 48 kHz / 128 frames):

| Patch | e9d918e9 peak | 3962616c peak | This branch after peak |
|---|---:|---:|---|
| Clarinet A | 0.292 | 0.0540381409 | not measured |
| Bassoon | 0.518 | 0.0771422163 | not measured |
| Alto Flute | 0.465 | 0.149914414 | not measured |

No gain compensation or DSP change was made. These numbers do not establish the
correct native level. Alto Flute's separate 0.5-s scanner silence remains open.

The three preset scripts were initialized in memory through the existing bank
reader. A temporary setter observer collected only target types/names and boolean
values; no script text, samples or access material was written. Every VWinds
preset writes boolean false to `Gain.Volume`. The scripts also write booleans to
`SignalConnection.Bypass`, whose retained numeric XML field is currently inferred
as float because this element has no catalog schema. The latter is a separate
schema/live-routing gap; it does not justify coercing every float write.

The [UVI Element API](https://lua.uvi.net/class_element.html) requires the value
type specified by the parameter definition. In cached Ghidra extraction of
UVIWorkstationx64.exe SHA256
`78729e96b752aea746280275072ad24cb4399a053739c49a161ff1fcfbf85721`, setter
`0x140f3d310` (`complete-export/pseudocode.c:6051786`) dispatches boolean input
through the boolean-descriptor lookup, vtable offset `0xe8`, and returns if that
descriptor is absent. Numeric input instead checks float `0xd8`, then integer
`0xf8`. Thus a boolean is not coerced to 0/1 for a float parameter. v1 UVI
`4bffbb18:src/uvi/host.rs` also ignores mismatched types, with a focused test
preserving numeric gain when a boolean is supplied. The global ignore behavior
was therefore retained. The approved original executable is packed at the cached
DSP addresses; radare2 did not independently validate the unpacked instructions.

Saved program inspection found shelves at zero dB, largely bypassed; that is no
evidence of a shelf attenuation law causing this loss. Each patch has enabled
DigitalEq bands of types 5/6; the body EQ's initial GainScale is zero. Scripted
bus EQs also retain disabled bands for later live writes. GainMatrix coefficients
route inputs 3–6 into output 1 in addition to inputs 1/2. Translation currently
has 188/189/272 oscillator groups, respectively, and 2/2/3 buses.

Native GainMatrix process `0x141338530` and Gain process `0x141337360` are described
in the cached `gain-routing-topology.json`: input-major parameter handles,
output scratch before copyback, and native channel count at owner+`0x1a8`.
The existing stereo coefficient orientation agrees with that topology. The cache
explicitly marks the complete gain pipeline unverified and its runtime dispatch
pointers unavailable. An uncalibrated v1 peak would not resolve that missing
native gain-pipeline evidence; no v1 audio A/B was run and no level correction
was guessed to match the old numbers.

Next evidence needed: a native/reference gain-path trace for the same CC/note
state, including multi-channel source routing, plus a full CLAP callback trace
for the scanner/host onset difference. Do not use a blanket bool-to-float
conversion or an arbitrary level multiplier as a substitute.

## Round 2: paired CLAP attribution (2026-10-11)

One fresh six-second run per patch/build, 48 kHz / 128 frames, survey note
plan, CC1=100, CC11=127, five half-second notes. Both use the frozen survey
CLAPs and the same frozen CLI to export state; private settings start empty.
Signal tracing is enabled, so timing is contended/instrumented, not acceptance.

| Patch | e9d918e9 peak | 3962616c peak | load average old / new (1 minute) |
|---|---:|---:|---:|
| Clarinet A | 0.301440090 | 0.067735381 | 10.65 / 17.68 |
| Bassoon | 0.513810456 | 0.075200513 | 12.12 / 16.86 |
| Alto Flute | 0.465053141 | 0.149420917 | 13.17 / 15.74 |

**The large drop starts in convolution.** The 2x2 GainMatrix implementation
and its live controls are identical at the two revisions; `git diff
e9d918e9..3962616c -- crates/sampler-uvi/src/inserts.rs` excludes the original
prime suspect. Clarinet's matrix input is 0.068953156 -> 0.065463036, output
0.048267213 -> 0.045824129: a small upstream/timing variation, not the
4.45x output loss. The first active convolution (trace node 158, impulse 7)
has input 0.048267200 -> 0.045824117 but output 0.058621570 -> 0.007201866.
Node 183 (impulse 9) changes 0.135900199 -> 0.017369986. Bassoon node 158
changes 0.095940135 -> 0.014828422; Alto Flute node 158 changes
0.070618145 -> 0.011468436. Subsequent convolutions amplify the difference.

Commit `de2334ce31fe9ff8ae4b08ce1229354f7307490f` introduced impulse shaping
(Time/damping/Width/NormalizePower, and the SampledReverb reference gains);
`402b3fb0` added live rebuilding of those kernels. The old build used raw
responses. This identifies the changed DSP stage, rather than merely listing
suspects. Traces contain approximately one million records per patch; the
plugin shutdown can leave a partial JSON footer, so summaries consume complete
record lines and make no claim that the final tail is complete.

The old peak is not a native target. V1 `4bffbb18:src/uvi/effects.rs:789-805`
normalizes mean-channel energy, uses 1.25 for normalized SampledReverb, and
0.125 for unnormalized SampledReverb. Its authored native basis tests at
1699-1740 check those numbers. [Falcon Effects, Convolver/IReverb](https://manual.uvi.net/falcon/en/effects/) and the
[UVI IReverb parameter reference](https://lua.uvi.net/_elements.html)
document NormalizePower and linear Dry/Wet but do not specify the 0.125
reference gain. The cached native constructor is `0x14103aa90`; process
delegates through `0x14103ff00` to `0x141039d50`. The approved SHA-identical
executable remains packed at those addresses (radare2 confirmation), so raw
disassembly does not independently validate the unpacked constants.
No patch gain compensation is justified by these old-vs-new peaks.

### Reference checks and final branch measurements

In-memory saved-value inspection confirms every patch has enabled, wholly wet
SampledReverb inserts with NormalizePower=0, PreDelay=0, Width=0 and both damping
controls=0. Time is 1 for Bassoon/Alto Flute; Clarinet also uses 0.58765006.
Their last Convolver has NormalizePower=1. These are authored response-shaping
chains, not transparent pass-through inserts. No recovered script or sample was
written. `reverb::tests::unnormalized_reverb_is_an_eighth` passed, one focused
test (83 filtered), preserving the measured 0.125 kernel gain. No DSP law changed
in this round; the older raw-response level is not restored.

All frozen-v1 checksums passed before using its prebuilt UVI scanner. Each
patch was run once with the same note-plan C4/velocity 64 and CC1=100/CC11=127:

| Frozen v1 UVI 4bffbb18 | Clarinet A | Bassoon | Alto Flute |
|---|---:|---:|---:|
| 512-ms probe peak | 0.064820878 | 0.121118680 | 0.041017056 |
| Nonfinite / runtime errors | 0 / 0 | 0 / 0 | 0 / 0 |

The v1 worker reads the approved static, SHA-identical reference executable as
data to locate the bank reader; it does not launch UVI/Wine. Its gate is 384 ms,
96 blocks of 256 frames, full resident PCM, unlike the six-second CLAP's streaming
and five 500-ms gates. These peaks therefore corroborate that the old high survey
numbers are not a native target; they do not establish sample-for-sample full-patch
parity or an exact v1/v2 peak ratio.

One fresh untraced host run per patch on the final release branch measured:

| Patch | Old traced peak | New traced peak | Final branch untraced peak | Final load avg 1/5/15 min |
|---|---:|---:|---:|---|
| Clarinet A | 0.301440090 | 0.067735381 | 0.055657297 | 21.13 / 20.63 / 19.00 |
| Bassoon | 0.513810456 | 0.075200513 | 0.082673766 | 20.43 / 20.49 / 18.97 |
| Alto Flute | 0.465053141 | 0.149420917 | 0.149784595 | 18.97 / 20.19 / 18.90 |

The only code change in the final branch is meter-publication cost. Differences
between the traced and untraced new peaks are not a level correction: script wall
time, random selection and instrumentation differ. All final host runs dispatched
12/12 planned events, had zero nonfinite values and zero callback deadline misses.
The numeric receipt preserves artifact SHA256s, timings and load averages.

### Alto Flute: onset admission, not convolution attenuation

The paired trace's first C4 sample-source record and first master output above
1e-5 occur in the **same block**: frame 8832 (184 ms) in the old build, frame
65536 (1365.3 ms) in the newer build. New master output is exactly zero during
the first second. C4 source data is absent in that interval; the init script's
key-0 voices are not an audition onset. Thus an eightfold wet attenuation cannot
explain this silence: it precedes the source, with no additional traced DSP onset
delay. Script-generated velocities and selected zones differ across this
instrumented pair, so these timestamps are evidence of admission delay, not
calibrated native latency measurements.

The final scanner run also reports Alto Flute silent in its 480-ms/180-block
window (`src/ui/scan.rs:944-975`), with 22/22 controls, zero Lua init/runtime
faults, 18/18 asset decodes and no underruns. Frozen v1's 512-ms worker does
produce audio. The exact Lua/streaming admission cause remains open; the trace
does not distinguish worker scheduling from script-requested delayed playback.

Native full-patch parity remains open. Multichannel GainMatrix routing is still
stereo-limited but did not first appear between these survey builds. V1 also has
SampledReverb wet-channel diffusion (0.8 same + 0.2 opposite) not yet modelled in
the v2 convolution path; this was not added from a peak comparison. A calibrated
native patch recording plus stage/voice admission evidence is required before
claiming an exact correct absolute level. Do not compensate these gaps with a
patch-wide gain.
