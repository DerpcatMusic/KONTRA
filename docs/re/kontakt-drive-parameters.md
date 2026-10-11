# Live Shape, Drive and Damping: physical-slot updates

Round 3 supersedes the round-2 MCP blocker. The full same-SHA Ghidra cache and
radare2 on the read-only `re-binaries/Kontakt8.exe` copy provide the evidence.
SHA-256: `0fe6356e0879d058b6e5b73507c54c5e345cea451b35287c974e438291d4dae8`.

## Documentation and saved domains

NI Kontakt 8 [Effect Reference — Saturator, Shape; Distortion, Drive and Damping](https://docs.native-instruments.com/ni-tech-manuals/kontakt-manual/en/effect-reference),
and KSP [Engine Parameters — Stomps](https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/engine-parameters)
describe `$ENGINE_PAR_SHAPE`, `$ENGINE_PAR_DRIVE` and `$ENGINE_PAR_DAMPING`.
Saved Classic Shape is signed [-1,1]; the KSP 0..1,000,000 domain maps linearly
to that range. Drive and Damping map to [0,1]. This preserves the round-1
initialization laws and existing native transfer functions.

## Cache extraction and radare2 confirmation

Cache: `artifacts/engine-analysis-2026-10-07/kontakt-engine/complete-export/pseudocode.c`
in the read-only `t3code-80fe786b` worktree, exact VA/function headers only.

* `FUN_1409619b0`, saved effect assignment case 7, clamps Shape to [-1,1]
  and stores half of it in Shaper +0x1c0. Case 0xb stores Drive and converts
  Damping to the embedded lowpass cutoff `1 - 0.4*damping`.
* `FUN_1408378f0`, Shaper template copy, copies +0x1c0 directly when the
  internal-modulator pointer is null. Radare2 disassembled 18 instructions
  at this function and confirmed the null branch and direct field write.
  `FUN_140aa4d20` is its audio path. Radare2 constants at `0x1447149f8`
  and `0x1447149e4` confirm the stored half-Shape is expanded using 8 and 4.
  The separate internal-modulator branch has a 32-frame clock; it is not
  evidence for smoothing direct script writes.
* `FUN_1408373c0`, Distortion template copy, copies Drive +0x1c0 and mode
  +0x1c4 directly when internal modulation at +0x7a0 is absent, then invokes
  the embedded filter's copy method. Radare2 confirmed its 18-instruction
  branch/virtual-call sequence.
* `FUN_140837e40`, embedded one-pole LP template copy, writes source cutoff
  +0x1dc to both destination current +0x1f8 and target +0x1dc. Radare2
  confirmed this 15-instruction direct-copy sequence. Thus direct Damping
  edits do not inherit the separate modulation smoothing clock.
* `FUN_140aa1580` is Distortion audio; `FUN_140ae9f90` is the embedded LP
  audio. Radare2 hexdump `0x1447148f8` confirms the Damping scale 0.4.

## Implementation and confirmation

Addressed IR forms carry group, physical slot and generic rack. `lower.rs`
reuses native engine bindings and zero-ramp controls. Classic Shape evaluates
that control in scalar and batched paths; Distortion updates its coefficients
on an event boundary while retaining audio histories. Existing static API
forms remain available. No generic registry or modulation-router edits.

Focused failing-first test:
`effects::tests::live_shape_drive_and_damping_reach_only_the_physical_slot`.
Before wiring, the correct-address runtime write failed with `InvalidInput`.
After wiring, a wrong physical slot remains rejected and all three correct
writes affect an already-playing voice. With a zero warmup then x=0.25 at
48 kHz, extracted/v1-native constants give:

| KSP write | Expected next left sample |
| --- | --- |
| Shape 750000 (signed Shape +0.5) | 0.75 |
| Tube Drive 1000000, Damping 0 | 0.74903756 |
| Damping 1000000, Tube Drive 0 | 0.031603266 |

All pass within 2e-6. These are computed vectors, not new host recordings.

Remaining: internal-modulator 32-frame clocks, Enhanced/Drums Saturator,
and dynamic mode switching. Those paths retain diagnostics rather than
being claimed as supported by the direct-copy evidence.
