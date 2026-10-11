# Legacy Phaser: four-stage cached topology

Status: implementation blocked at the [MCP evidence gate](kontakt-fx-2-evidence.md).

Docs first: Kontakt 8 [Effect Reference, Legacy Phaser](https://docs.native-instruments.com/ni-tech-manuals/kontakt-manual/en/effect-reference),
Depth, Speed, Phase, Feedback and Dry/Wet; KSP
[Engine Parameters](https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/engine-parameters), phaser controls.

Historical binary leads: BPar reader `0x140d01fa0` supports versions
0x50/0x51; audio `0x140aa3830`, initialization `0x1408ddb60`,
reset `0x14094c6d0`, routing `0x14095fde0`; shared mix preparation
`0x140ac28f0` and finalization `0x140ac21f0`.

The cached audio function uses four allpass stages per channel. Each
stage computes y = a(x + previous_y) − previous_x and advances its
two histories. Input feedback uses the preceding final-stage output.
An oscillator advances sine first by subtracting cosine·step, then
cosine by adding the new sine·step. Depth scales that oscillator before
the frequency mapping; the allpass coefficient is a rational function
of the mapped frequency. This is separate from saved filter type 13.

The v1 six-stage proxy therefore cannot establish this native topology.
The speed/free/sync mapping, oscillator initialization and stereo phase,
feedback clamp, frequency constants and dry/wet law remain untraced.
The reader layout alone is insufficient to assign named controls.

Confirmation: cached topology inspected, but no complete coefficient/LFO
vector and no production test. This slot remains unimplemented.
