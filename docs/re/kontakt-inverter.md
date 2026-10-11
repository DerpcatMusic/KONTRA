# Inverter: saved flag assignment proved

Status (round 3): saved flag order proved end to end; existing mapping confirmed.

Docs first: Kontakt 8 [Effect Reference, Inverter](https://docs.native-instruments.com/ni-tech-manuals/kontakt-manual/en/effect-reference),
Phase Invert and L/R Swap; KSP [Engine Parameters](https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/engine-parameters).

## Historical round-2 observations (superseded below)

Historical binary leads: version-0x60 reader `0x140d01db0` stores the
first byte at BPar+8 and the second at BPar+9. Audio `0x140908e60`
uses wrapper+0x1c1 to swap exactly two channels and wrapper+0x1c0
to invert the sign bit. The outer callback is `0x140904830`.
Copy function `0x140837510` copies the wrapper flags without establishing
which BPar flag populated them.

The audio law is a channel permutation followed by an optional sign
inversion. Those operations commute for stereo. This does not settle
the serialized flag order: the BPar→wrapper assignment is still missing.
For hypothetical flags (invert=true, swap=false), (.25,-.5) becomes
(-.25,.5); with only swap it becomes (-.5,.25). These expected tuples
have not been confirmed against the native parameter reader/assignment.

Confirmation: cached reader and audio function inspected; no end-to-end
flag-order vector, MCP function output, or production regression test.
The existing flag-order diagnostic remains because the missing assignment
would make its removal an unsupported claim.

## Round 3: assignment resolved

Same-SHA cache `complete-export/pseudocode.c`: `FUN_1409619b0`, case 4,
uses `FUN_1407cb4f0` to obtain BParFXInverter and assigns BPar+8 to
wrapper+0x1c0 and BPar+9 to wrapper+0x1c1. radare2 MCP disassembly
`0x140961f91..0x140961fac` confirms both source/destination pairs.
Reader `0x140d01db0..0x140d01de7` confirms first/second saved byte.
Audio `FUN_140908e60` tests +0x1c1 for swap (only if two channels),
+0x1c0 for sign inversion; radare2 confirms the branch and sign mask
`0x144714ba0` = four copies of `0x80000000`.

The importer already used this order; only its now-proven false
UnknownLaw diagnostic was removed. `inverter_saved_flags_match_native_assignment_and_audio`
failed first on that diagnostic, then passed for the three nonidentity flag
combinations, using input (.25,-.5). Expected outputs: (-.25,.5),
(-.5,.25), (.5,-.25). Existing chain-boundary expectation updated.
No Kontakt reference recording or timing/performance claim.
