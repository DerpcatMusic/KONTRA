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
