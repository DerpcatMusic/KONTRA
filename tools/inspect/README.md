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
unavailable data. Translator law IDs and provenance, script callback inventory,
original-to-runtime script slot mapping, and asset existence are not yet exposed by this path and are explicitly listed
as unavailable. Script inventory slots are original file slots; UI slots are runtime slots, which can differ after a compile failure. Slot laws and route curves that are already in the IR are shown.

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
