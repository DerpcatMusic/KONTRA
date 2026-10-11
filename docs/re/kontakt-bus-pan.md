# Instrument bus pan: no verified native law

Status: implementation blocked at the [MCP evidence gate](kontakt-fx-2-evidence.md).

Docs first: Kontakt 8 [Using Filters and Effects in Classic View, Bus Insert Effects](https://docs.native-instruments.com/ni-tech-manuals/kontakt-manual/en/using-filters-and-effects-in-classic-view),
instrument-bus amplifier controls; KSP
[Engine Parameters](https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/engine-parameters), bus addressing.

Historical binary leads: BInsertBus serialization vtable includes
`0x140d0d300`, `0x140d095e0`, `0x140d05b60`, `0x140d16cc0`
and `0x140d13e50`. These locate the bus record class, not the audio pan
function. No bus pan-law function address was established.

Neither equal-power mono panning, group pan nor Stereo Modeller balance
can be substituted as the instrument-bus law without evidence. The
left/right gain curve, center attenuation and stereo crossfeed therefore
remain unknown. There is no defensible law or pan vector to report.

Confirmation: vendor bus controls and cached class inventory checked.
The Dolce pan-only path still lacks modeled pan and remains diagnosed;
no source change or production test was made.
