# Instrument bus pan: native saved balance

Docs first: Kontakt 8 [Using Filters and Effects in Classic View / Bus Insert Effects](https://docs.native-instruments.com/ni-tech-manuals/kontakt-manual/en/using-filters-and-effects-in-classic-view), amplifier Volume/Pan controls.
Evidence: same-SHA Ghidra cache `kontakt-engine/complete-export/pseudocode.c`, and radare2 MCP on `re-binaries/Kontakt8.exe`, SHA `0fe6356e0879d058b6e5b73507c54c5e345cea451b35287c974e438291d4dae8`.

## Audio path, not serialization

`FUN_1407f8f20` constructs 16 BInsertBus instances at instrument +0x1cf78, stride 600. `FUN_1408ef2d0` calls actual bus audio `FUN_140824910` at that address. Its saved volume is +0x24, saved pan +0x28, current pan +0x64. The first-call flag +0x2e initializes current values from saved values.

For stereo, `FUN_140824910` calls `FUN_140823d70` twice, left flag 0 and right flag 1. Its held-pan gain law is left `1-clamp(pan,0,1)` and right `clamp(pan+1,0,1)`. Center has unity gain in both channels and no crossfeed. Native SIMD/alternative-dispatch branches implement the same law. MCP disassembly confirmed left clamp/subtract at `0x140824333..34b`, right add/clamp at `0x14082402d..04a`, actual audio calls at `0x140824f74..f89`, and unity constant `0x14470bad4`.

Translation now retains pan-only buses and appends this diagonal balance after effects/volume. The previous pan diagnostic disappears because saved pan is executed. Nonfinite pan is diagnosed as invalid.

Focused test `effects::tests::instrument_bus_pan_only_uses_native_stereo_balance` observed red (old unsupported diagnostic) then green. Stereo input [1,1] gives [1,0], [1,.5], [1,1], [.5,1], [0,1] at pan -1,-.5,0,.5,1. Uses synthetic PCM and extracted constants, no host recording and no library/corpus render. Dolce's retained routing intent is a shared summed bus with saved pan; production audio parity is not measured here.

Remaining: live bus pan slew is not wired. Native `FUN_140824910` builds a per-sample pan ramp when the difference exceeds 20/sample-rate, with step +/-20/sample-rate. This static matrix does not claim automation parity. Phase invert and L/R Swap bus flags also need separate representation.
