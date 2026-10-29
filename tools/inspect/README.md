# kontra-inspect

`kontra-inspect load <instrument> --json <report.json>` reads and prepares a
Kontakt instrument or a UVI program (`bank.ufs/path/program.uvip`, or the bank
for its first program) using the v3 host. It prints a concise summary and writes
a versioned JSON inventory when `--json` is supplied. Exit 2 means a usage,
read, store or output error.

The report contains the translated samples, groups, zones, modulation routes,
shapers, effects and scripts, the prepared-zone count and per-zone `prepared`
flag, initialized UI models, and translator/prepare diagnostics with impact.
A zone not prepared is not necessarily an error: for example, a saved purged
group can defer sample loading. See diagnostics and group `purged` state.
Effect and some model details use Rust debug text; they are descriptive, not a
stable machine-readable schema. The versioned envelope reserves `null` for
unavailable data. The `law_applications` list records IDs, locations, inputs and outputs for
Kontakt group envelopes, release counters, timed envelopes and LFOs, EQ routes,
volume source curves, route lag, LFO-rate routes, modulation shapers, saved
filter/EQ, compressors/limiters, route time floor, streaming and group/zone
gain, pan and tuning at translation time.
For UVI it records keygroup/zone gain, pan and tuning, amplitude and AHD
envelopes, LFO shapes, matrix cells, filters and key/velocity fades, identified
by XML byte offset and tag.
UVI records include the source node attributes and context; cross-node mapper
inputs are not fully captured. Coverage is
partial: all other translator law applications are explicitly unavailable. Referenced UI assets are read through the host
resource resolver: `resolved` includes their byte length; `not_resolved` does
not distinguish missing, inaccessible, or unsupported resources. This inventory
contains assets referenced by the initialized UI, not every file in the library. Script
inventory includes original and runtime slots, compilation status, and compiled
callback entries. Entries are bytecode offsets or foreign dispatcher IDs; the
foreign widget dispatcher is generic and does not prove a script defines every
widget callback. A null entry means no compiled entry, while zero is a valid
offset. If host slot data is inconsistent, the mapping and callback inventory
are reported unavailable. Slot parameter laws and compiled script knob and reverb laws are structured
curve data from the IR/plan; the debug slot-law text and route curves are also
shown. These are available law definitions, not a claim that every translator
application or law ID was recorded.

`kontra-inspect` does not run the existing v2 `kontra-probe` contracts.

`kontra-inspect play <instrument> --scenario user --json <report.json>` runs
legato, chords and repeats with slow and fast CC1/CC11 sweeps, then 20 seconds
past the last note-off. A file scenario is `{"events":[{"frame":0,
"type":"note_on","key":60},{"frame":28800,"type":"note_off","key":60}]}`;
`cc` events use `cc` and `value` fields. Frames are at 48 kHz and note-on
velocity is 100. Reports include inputs, block-boundary EQ route/voice snapshots,
peak, clipped samples, maximum adjacent sample jump and the stereo second-
difference energy in a ±5 ms window around each CC event. This energy is a
measurement, not by itself a click verdict. The per-voice peak is the last
voice render span before group effects; it is not the level of the final mix.
Unavailable stages are listed in JSON. The inspector enables a compile-time
voice trace feature; the normal feature-off voice renderer has no trace state
or per-frame instrumentation.

Unprepared zones carry `unprepared_reason` and are counted by reason. A sample
referenced only by purged groups is not requested during initial preparation;
`resident_sample_unavailable` means preparation requested that sample but it
did not make the plan. Consult diagnostics for the specific open or load error.
An invalid sample index is separate. `unknown` means the plan's limited zone
member index cannot establish the answer.

Offline playback requests and waits for every voice's actual interpolation
window, including loop crossfades. If a wait times out, a page request is
refused, a read fails, or frames are missing, `render_complete` is false and
`play` exits with an error after writing its JSON report. Missing blocks and
stream counters are included for diagnosis. The wait is enabled only by the
inspector's trace build and is never enabled for live playback.
The `audio_fnv1a64` field fingerprints the exact little-endian stereo f32
output bits, allowing repeat runs to be checked for differences.

Playback JSON `stage_events` is ordered: callback starts/resumes carry script
slot and callback ID, script actions carry that ID and a unique call ID, and
additional integer arguments and UTF-8 text chunks follow with the same call ID;
concatenate each text argument’s chunks by chunk index. Voice starts,
steals, cuts and ends include slot, zone, sample, pitch step and gain. Voice
ends are timed to the end of the render span, not the precise sample. The
bounded engine event buffer is drained between blocks; an overflow is counted
and makes the report incomplete. `all_routes` gives current source and routed
source for each active voice, while start-sensitive routes additionally emit
`route_onset` events with the source sampled at voice start. LFO phase and rate
routes can be re-read later using the then-current controller state; the onset
event does not imply that their value remains fixed. `bus_routes` gives
the last applied bus amount. Non-EQ applied per-voice target values are not yet available; `null`
sources are not zero. Octave-band RMS uses Hann-windowed stereo FFTs and is
reported for the full output and successive one-second intervals.
`voice_effect_targets` are the engine's recomputed target configurations at
the snapshot, not the instantaneous value of an interpolating effect.
`shared_effects` shows the current shared rack configurations with the last
bus route amounts applied. Effect configurations use descriptive debug text.

`kontra-inspect ui INSTRUMENT --json report.json` builds each script's current
host UI model and moves every knob, slider, value edit, switch, button and
visible menu choice through that model. Hidden and disabled controls are also
attempted and identified in the report. Models are refreshed after each control,
so newly introduced controls are visited; a model that keeps creating controls
past 10,000 is reported as incomplete. A control without a source ID or without
selectable menu values is reported with an error.

Each move records callbacks and actions observed after the move, changed
script cells, group envelope/effect settings, group and bus levels, shared
rack effects, and modulation settings. It also records newly counted faults or
unhandled actions. Foreign VM internal variables and the complete engine
parameter state are unavailable, rather than reported as unchanged. The audio
comparison retriggers C4 and measures successive 80 ms stereo RMS windows;
natural time evolution and other callbacks can change the output, so a change
is evidence for follow-up rather than proof that the moved control caused it.

`kontra-inspect play INSTRUMENT --scenario user --oracle --json report.json`
writes the same input events as MIDI at one tick per 48 kHz frame, renders v3,
and captures a Kontakt `.nki` through `oracle-slot` using its isolated Wine
prefix. It never uses the live Wine prefix. The comparison reuses
`tools/oracle/compare.py`, records RMS and peak differences, octave-band levels,
maximum sample jumps, clipping, and per-CC second-difference energy. Native
capture timing has millisecond uncertainty; envelope alignment is in 10 ms
steps, so the narrow CC windows are indicative rather than sample-aligned.
Native script/voice/route/FX internal values are explicitly unavailable.

Add `--fresh-state` to run each of the six user-scenario segments in its own
new engine and oracle session. Reports contain six nested `play` results. The
oracle detects each first onset from 10 ms envelope bins and compares two
seconds after each onset; detected onset times and unavailable or silent captures
are reported explicitly. Segment input events are rebased exactly; fast sweeps
therefore begin 0.5 seconds after their first notes. Separate starts change
round-robin and script state relative to the continuous scenario.
A native UVI capture path is not implemented. A failed or silent oracle writes
an error/uncomparable status and exits nonzero after writing JSON.

Artifacts (MIDI, v3 and native WAVs, logs) are written under
`/data/dev/workspace/PluginArtifacts/inspect-oracle`; override the root with
`KONTRA_INSPECT_ORACLE_DIR`. To locate the existing oracle scripts outside a
checkout, set `KONTRA_ORACLE_DIR`.

`route_events` records the actual contribution evaluated by the engine at a
note onset, a controller-triggered voice update, or the first timed span after
controller changes. Units identify gain multipliers, pan, semitones, filter and
EQ contributions, effect amounts, LFO rate/phase, sample/loop fractions and
resulting envelope frame counts. They are recorded before route combining,
smoothing and final clamping; the existing voice and effect snapshots show
resulting state. Bus routes report their last applied amount separately.

`direct_controller_source` means the route source reads that controller;
`controller_can_affect` also includes the engine's structural intensity and
LFO rate/phase dependencies. Script callback IDs link evaluations made while a
controller callback was executing. Arbitrary dependencies hidden in script
state and an individual route's instantaneous smoothed value remain unavailable.
For a timed update the controller identifies one received since the previous
span; multiple CCs may therefore appear for the same evaluated value. Buffer
capacity is fixed before processing; any dropped record makes the report
incomplete. With the trace feature off these hooks and their state are absent.

## Seven-library oracle report

From any directory, `tools/inspect/oracle-diff.sh` builds the current checkout
with `build-slot`, runs continuous and fresh-state user scenarios on the seven
registered Kontakt programs with up to three isolated `oracle-slot` sessions,
and regenerates `/data/dev/workspace/scratch/laneG/ORACLE-DIFF.md`. Each run
uses a timestamped artifact directory and records the source revision. RMS and
band flags are separate; unavailable comparisons stay unknown and make the
command fail after the report is written. `--jobs 1..3`, `--output PATH`, and
`--library-root PATH` override defaults. `--reuse RUN_DIR` regenerates a report
from a prior run without launching Wine or rebuilding.
