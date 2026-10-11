# Zero-weight Multi dispatch after integration

Kontakt 8 Manual, Modulation → LFO controls → Waveform Mixer:
https://docs.native-instruments.com/online-guides/kontakt-manual/en/modulation
A zero mixer amount contributes no waveform; all five zero amounts therefore
retain a bipolar zero source (its unipolar view is 0.5), not a Constant source.

Read the SHA-identical Kontakt 8 Ghidra cache,
`artifacts/engine-analysis-2026-10-07/kontakt-engine/complete-export/pseudocode.c`,
`FUN_140b07290`: the kernel multiplies each component by its saved weight.
Confirmed with radare2 against `/mnt/Windows11/DEV_WORKSPACE/re-binaries/Kontakt8.exe`:
`0x140b07394`, `0x140b073bd`, `0x140b073e7`, `0x140b0741a`,
`0x140b07433` multiply the five weights at offsets 0x128 through 0x138.
All zero weights give zero independently of phase. See W15_ZERO_MULTI.md
for the existing 27 original-byte checks and transformed-route audio evidence.

The modulation merge a3be3c28 put sine-only Multi before the existing zero
case. Moving the zero case first preserves its representation and leaves
nonzero signed/subunit sine weights and phase unchanged. v1
`0cb7a8a0:src/modulation.rs:280-298` also retains zero sine weights.

Verification uses the existing zero and signed-sine translation tests, not
a new host recording. The coordinator's integ-test-1.log supplies failing-first
evidence for the zero case; this package does not change the DSP kernel.
