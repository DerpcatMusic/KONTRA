# Conflux integrated-survey gaps

Source-only inspection of the installed Conflux NKI (pick 5) finds 92 groups.
The six wavetable destination names are `wavetablePosition`, `warpFactor`,
`warpFactor2`, `wavetableModFrequency`, `wavetableModAmount`, and
`wavetableInharmonic`. The saved routes shown by this patch's inspection have
zero depth; this does not justify removing their diagnostics, because the GUI
can change them. Source-module destinations include LFO `frequency` and
`intensity`, distinct from effect processor parameters.

The source descriptor retains nine wavetable live engine parameters. Current
IR Wavetable has static position/phase/form1/form2 and two form selectors;
there is no live source-parameter destination for these six routes. Native
warp/inharmonic/FM laws and their clocks must be traced before adding those
routes. RandomBipolar retains external source ID 11, but execution still lacks
a verified native signed generator/reset law. v1 UI labels it Random +/-;
that label is not a DSP implementation or a measured law. These gaps remain.

## NKM Original view

For pick 6 program 0, source initialization produces:

| Physical script slot | Name | Performance view | Widgets |
|---|---|---|---|
| 1 | MIDI Player | false | 206 |
| 2 | Chord Naming | false | 12 |
| 3 | Humanize | false | 6 |
| 4 | Main | true | 2 |

The MIDI Player script contains a make_perfview call, but its initialized flag
is false under saved state. The Chord Naming helper has no make_perfview call.
`src/ui/part.rs::main_face` prioritizes an initialized performance view, then
native UI, then available controls. These facts do not prove Kontakt shows
nothing: all flags must be compared after activation, and the actual selected
program/script slot in the aggregate survey receipt needs correlating with
that runtime state. Do not choose the largest widget list or manufacture a
performance panel. Native UI comparison remains open; this probe did not
render the plugin or native Kontakt.

Evidence: `survey_switch_and_conflux_intent` source-only probe; preserved
physical descriptors in source_parameters.rs/modulation_objects.rs;
`docs/architecture-v2/KONTAKT_FX_MOD_FORMAT.md`; current Wavetable IR.
