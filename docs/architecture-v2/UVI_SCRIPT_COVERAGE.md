# UVI Lua script API coverage

Source of counts: `census_symbols` over all 660 programs of the local UVI libraries
(`crates/sampler-uvi/src/lib.rs`, ignored survey test, `KONTRA_SYMS`). "Programs" is
the number of programs whose script text mentions the symbol, "uses" the total
occurrences. Names are checked against the UVI Lua reference (lua.uvi.net). The
runtime is Luau (mlua, `luau-jit`) with an interrupt budget; `io`, `os`, `package`,
`debug` and `loadstring` are not available.

Status: **implemented** = real effect on the IR runtime; **stub** = defined (or caught
by the inert fallback), returns a neutral value and is reported as a finding
(`unsupported`) with the exact name and value; **missing** = nothing yet.

All tests below live in `crates/sampler-uvi/src/script.rs` unless noted.

## Callbacks

| Symbol | Programs | Uses | Status | Test |
|---|---:|---:|---|---|
| onInit, onLoad | 660 | 5000 / 3760 | implemented (called at load) | `note_on_plays_the_oscillator_the_script_picks` |
| onNote | 660 | 9380 | implemented | same |
| onRelease | 660 | 10620 | implemented | `wait_for_release_resumes_on_note_off_and_release_is_called` |
| onController | 660 | 6900 | implemented | `other_host_events_reach_their_handlers_and_on_event_takes_precedence` |
| onPitchBend, onAfterTouch, onPolyAfterTouch, onProgramChange, onTransport | 40 / 40 / 25 / 0 / 40 | 40 / 40 / 25 / 0 / 40 | implemented (host input from the v2 packet path) | same |
| onEvent | 0 | 0 | implemented, takes precedence over onNote | same |
| onSave | 660 | 4380 | stub (called; saved state is not persisted) | none |

## Voices and events

| Symbol | Programs | Uses | Status | Test |
|---|---:|---:|---|---|
| playNote, postEvent | 660 | 3971 / 2005 | implemented (children use Inheritance::Expression) | `classes_tables_and_playnote_tables_work`, `voice_manipulation_and_generated_midi_become_commands` |
| releaseVoice | 620 | 1860 | implemented | `wait_for_release_resumes_on_note_off_and_release_is_called` |
| changeTune, changeVolume (changeVolumedB, changePan) | 620 | 2480 / 2480 | implemented (`set_note_param`) | `voice_manipulation_and_generated_midi_become_commands` |
| fadein, fadeout | 660 | 1280 / 2183 | implemented (`fade_note`) | same |
| fade, fade2 | 40 | 354 | implemented | same |
| sendScriptModulation | 660 | 8673 | implemented (ramp default 20 ms) | `send_script_modulation_becomes_a_command` |
| sendScriptModulation2 | 40 | 589 | implemented | same |
| setSampleOffset | 40 | 296 | stub | none |
| controlChange | 40 | 498 | implemented (MIDI out into the part) | `voice_manipulation_and_generated_midi_become_commands` |
| programChange, pitchBend, afterTouch, polyAfterTouch, postMidiEvent | 0 | 0 | implemented | same |

## Time and context

| Symbol | Programs | Uses | Status | Test |
|---|---:|---:|---|---|
| wait, spawn, run | 660 / 660 / 620 | 5495 / 7639 / 620 | implemented (host clock) | `wait_and_spawn_follow_the_hosts_clock` |
| waitBeat | 620 | 1860 | implemented | same |
| getTime | 660 | 1513 | implemented | same |
| getTempo, getRunningBeatTime, beat2ms | 620 | 620 / 1240 / 1240 | implemented (tempo from host input) | `context_conversions_and_key_state` |
| isKeyDown | 660 | 1996 | implemented | same |
| getBeatTime, getBeatDuration, getBarDuration, getTimeSignature, getSamplingRate, getNoteDuration, getCC, isOctaveKeyDown, isNoteHeld, ms2beat, ms2samples, samples2ms | 0 | 0 | implemented | same |

## Parameters and engine objects

| Symbol | Programs | Uses | Status | Test |
|---|---:|---:|---|---|
| setParameter | 660 | 95909 | stub: accepted, nothing in the IR changes (Program.Polyphony, OnePole.Freq, Layer.Gain and every FX module included) | `unknown_api_and_ui_are_inert_and_reported_once` |
| getParameter | 660 | 5167 | stub: returns the authored XML value | same |
| getParameterConnections | 660 | 4774 | stub | same |
| findLayer | 40 | 190 | stub | none |
| Program, Unit | 660 | 47967 / 57044 | stub objects (parameter access above) | none |
| Layer, Part, Synth, Mapper, Event | 620 | 32860 / 16120 / 25420 / 19220 / 620 | stub objects (Mapper is undefined and inert) | none |
| class | 620 | 9300 | implemented | `classes_tables_and_playnote_tables_work` |
| require | 660 | 20580 | implemented for the bundled script modules; `uvi.ChordRec` missing | none |

## Loading and persistence

| Symbol | Programs | Uses | Status | Test |
|---|---:|---:|---|---|
| loadSample, loadImpulse, loadData | 660 / 660 / 653 | 1360 / 980 / 1298 | stub | none |
| saveState, loadState, browseForFile | 40 | 40 / 80 / 80 | stub | none |

## UI

Widgets all export to the UI IR (0 unsupported on the 660 programs).

| Symbol | Programs | Uses | Status | Test |
|---|---:|---:|---|---|
| Panel, Button, OnOffButton, Knob, Slider, NumBox, Menu, Label, Image, Table, Viewport, AudioMeter, XY, WaveView | 660 (AudioMeter 620, XY 40, WaveView 25) | 19805 / 28578 / 27621 / 39949 / 16175 / 6705 / 11751 / 6400 / 10481 / 2800 / 1900 / 1240 / 160 / 100 | implemented | `widgets_export_to_the_ui_ir` |
| setSize | 660 | 660 | implemented | same |
| setBackground | 40 | 40 | implemented | same |
| setBackgroundColour, setKeyColour, makePerformanceView | 620 / 660 / 660 | 620 / 2020 / 660 | stub (no IR field yet) | none |

## Lua basics

`table`, `string`, `math` (660 programs) are Luau standard libraries. The sandbox
test is `the_sandbox_has_no_io_os_or_package_access`; runaway scripts are covered by
`runaway_scripts_are_aborted_and_reported`.

## Open items

- setParameter/getParameter must drive IR parameters (Program.Polyphony, OnePole.Freq,
  Layer.Gain/Pan). This needs a translator binding table plus a public group-parameter
  setter in sampler-core; the 1-pole filter needs DSP (runtime chain filters are 2-pole).
- 13 programs fail in Main at the arpeggiator header (`HeaderArpOnOffs[2]` is nil when
  the ArpeggiatorOnOffs callback fires); the hornScript3 programs hit nil arithmetic
  on their volume tables. Both are layout assumptions of the scripts, not API gaps.
