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
