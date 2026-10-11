# Reverb: setter laws do not establish the algorithm

Status: implementation blocked at the [MCP evidence gate](kontakt-fx-2-evidence.md).

Docs first: Kontakt 8 [Effect Reference, Reverb](https://docs.native-instruments.com/ni-tech-manuals/kontakt-manual/en/effect-reference),
Room/Hall and their controls; KSP [Engine Parameters](https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/engine-parameters).

Historical binary leads: saved reader `0x140d019f0`, wrapper
`0x1408fc160`, NI::DSP::GaloisReverb setter `0x141aea370`,
processing dispatch `0x141ae2c60`, update routine `0x141ae5fc0`.
Legacy Reverb audio `0x140aa3d10` is a different module.

The first ten setter mappings already recorded in KONTAKT_DSP_LAWS are
.5·exp(3.7x), 3x+1, x^(1/4), 10x, .5x, 11025x,
19000(1−x)+2000, −12x, x, x. They describe stored parameter values,
not a verified feedback-delay network or Room/Hall algorithm.
The processing dispatch depends on several flags and a jump table at
`0x141ae2cf4`; the cached decompiler explicitly did not recover it.

Confirmation: wrapper and dispatch inspected in the historical cache.
No complete delay topology, table, impulse vector or native recording
was obtained; existing approximation and diagnostics remain unchanged.

## Survey follow-up: recovered dispatch and network leads

The Oct-7 same-SHA cache plus radare2 readonly copy are available; the older
MCP gate is no longer the blocker. radare2 at 0x141ae2c60 confirms the index
bits: bit 2 when either byte +0x6e15/+0x6ed5 is set; bit 1 for two input
channels (+0x6f10); bit 0 for byte +0x6d40. Do not infer Room/Hall from those
bits without tracing their setters. The 32-byte RVA table at 0x141ae2cf4 is:

| Index | Stub | Processing target |
|---|---|---|
| 0 | 0x141ae2cb2 | 0x141ad7b70 |
| 1 | 0x141ae2cba | 0x141acda50 |
| 2 | 0x141ae2cc2 | 0x141acf210 |
| 3 | 0x141ae2cca | 0x141ad0590 |
| 4 | 0x141ae2cd2 | 0x141ad18e0 |
| 5 | 0x141ae2cda | 0x141ad3230 |
| 6 | 0x141ae2ce2 | 0x141ad4f20 |
| 7 | 0x141ae2cea | 0x141ad6370 |

In cached FUN_141acda50, stereo input passes two fractional circular delays
at +0x2c10, with base and differential delay controls from the parameter
buffer +0x6d80 offsets +0x14/+0x18. Non-stereo input is averaged before
this pair. A complex oscillator at +0x6ca8/+0x6cb0 rotates each sample and
modulates alternating delay lengths using parameter-buffer +0xc.
The delayed pair feeds 32 weighted components (coefficient arrays +0x40
and +0xc0). The path calls FUN_14192cf80 with count 0x20 and
FUN_141ad96d0 before the fractional-delay network. These are network
leads, not a complete verified implementation: recurrence order, the full
mixing transform, coefficient initialization and update ramps must still
be extracted. FUN_141ae5fc0 includes countdown states and buffered ramp
updates, so copying the static setter curves alone loses audible state.

Legacy FUN_140aa3d10 separately invokes a virtual processor at wrapper
+0x1100 before its own delay/filter chain. It must not be substituted with
one of these Galois paths just because both are called Reverb.

No code law changed and no native impulse acceptance vector was obtained.
Same binary SHA: 0fe6356e0879d058b6e5b73507c54c5e345cea451b35287c974e438291d4dae8.
