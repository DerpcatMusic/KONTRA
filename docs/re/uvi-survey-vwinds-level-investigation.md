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
1699-1740 check those numbers. [Falcon Effects, Convolver/IReverb]
(https://manual.uvi.net/falcon/en/effects/) and the
[UVI IReverb parameter reference](https://lua.uvi.net/_elements.html)
document NormalizePower and linear Dry/Wet but do not specify the 0.125
reference gain. The cached native constructor is `0x14103aa90`; process
delegates through `0x14103ff00` to `0x141039d50`. The approved SHA-identical
executable remains packed at those addresses (radare2 confirmation), so raw
disassembly does not independently validate the unpacked constants.
No patch gain compensation is justified by these old-vs-new peaks.

Native full-patch parity, frozen-v1 patch A/B and Alto Flute scanner onset are
still under investigation. The separate multichannel GainMatrix gap remains,
but did not first appear between these two survey builds.
