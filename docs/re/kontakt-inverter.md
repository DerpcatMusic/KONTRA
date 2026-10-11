# Inverter: flag-order evidence still incomplete

Status: implementation blocked at the [MCP evidence gate](kontakt-fx-2-evidence.md).

Docs first: Kontakt 8 [Effect Reference, Inverter](https://docs.native-instruments.com/ni-tech-manuals/kontakt-manual/en/effect-reference),
Phase Invert and L/R Swap; KSP [Engine Parameters](https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/engine-parameters).

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
