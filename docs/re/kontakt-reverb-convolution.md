# Reverb and convolution, round 3

Docs: NI Kontakt 8 [Effect Reference](https://docs.native-instruments.com/ni-tech-manuals/kontakt-manual/en/effect-reference), Reverbs / Reverb and Convolution, especially Auto Gain and early/late controls.
Binary SHA: `0fe6356e0879d058b6e5b73507c54c5e345cea451b35287c974e438291d4dae8`.
Algorithms: same-SHA Ghidra cache `kontakt-engine/complete-export/pseudocode.c`.
Confirmation: radare2 MCP, read-only `re-binaries/Kontakt8.exe`.

## Convolution Auto Gain implemented

`FUN_1419b2bc0`, instructions `0x1419b31f0..0x1419b326e`: square each sample in f32, sum in f64 per channel, select the largest channel energy. At energy <= f64(f32(0.001)) gain is one; otherwise round energy to f32, take sqrt(f32(0.5 / energy)), cap at two. MCP disassembly confirmed the accumulation, comparison and narrowing. MCP dumps confirmed threshold `0x14769abe8` = 0.0010000000474974513, numerator `0x14470bad0` = .5 and ceiling `0x144714994` = 2.

The previous f32 sum lost small tail energy. Focused failing-first test `effects::tests::convolution_auto_gain_keeps_native_double_energy_sum` uses one unit sample followed by 10,000 samples of .0001. Extracted-law gain is f32 bits `0x3f3502a2` (.7070714235305786); old gain bits `0x3f3504f3` (.7071067690849304). Test passed after the correction. No Kontakt reference recording.

This is one preparation sub-law, not complete convolution parity. `FUN_140abd100` assigns early/late/flags/envelope to `FUN_140ab5e00`; `FUN_1419b2bc0` invokes `1419b3450` for resizing, `1419b27a0` for filtering, `1419b1e30` for early/late composition, and `1419b2320` for envelope application. Their complete parameter assignment, resampling and envelope laws are not yet implemented. Existing size/filter/decimation diagnostics remain. Surround channel routing also remains outside the stereo kernel.

## Modern Reverb dispatch recovered; no replacement kernel

Cached `FUN_141aea370` contains its ten-control setter; `FUN_141ae2c60` computes three selector bits but Ghidra failed to recover its jump table. MCP hexdump `0x141ae2cf4..0x141ae2d13` and disassembly `0x141ae2cb2..0x141ae2cf2` resolved all eight targets:

- `141ad7b70`, `141acda50`, `141acf210`, `141ad0590`
- `141ad18e0`, `141ad3230`, `141ad4f20`, `141ad6370`

The functions are present; this is incomplete extraction, not a missing-function or tool blocker. A correct replacement still requires tracing delay initialization, feedback/diffusion matrices, parameter interpolation and all selector modes. Installing setter formulas over the existing unrelated network would not establish native parity. The existing algorithm warning remains.
