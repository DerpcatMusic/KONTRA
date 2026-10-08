# v1 settings and feature parity

Baseline: v1 **0cb7a8a0 (GAS)**; v2 **67dafc61**, `origin/integrate/core-v2` at the start of W4's resumed task. This is a source inventory of reachable product controls and behavior, not a performance certification. **Present** means the control and its backing path exist; **missing** means absent; **worse** means only a subset or read-only replacement exists. Runtime correctness still requires the unified release gate. No release build or install is authorized before that gate.

The current implementation status is tracked below. Source references are relative to the named revision. CPU-relevant rows carry **W9**; this includes settings that affect load/RSS/disk work as well as render CPU. Authored library controls are data-driven and potentially unbounded: their entire supported widget/engine behavior is inventoried by kind, rather than treating one library's knobs as global settings.

## Global preferences and settings panel

| v1 setting / feature | v1 evidence | v2 start status and evidence | CPU tag |
|---|---|---|---|
| Add folder of libraries, add one library folder | ui/header.rs:202; library::Root | Present: ui/header.rs; library.rs | W9 scan/load |
| Add a path by typing; remove roots; root kind and per-root scan counts | ui/header.rs:174–221 | Present: ui/header.rs | W9 scan |
| Import Kontakt registered folders, automatic import on first run | library.rs; ui/header.rs:214 | Present: library/kontakt.rs | W9 scan |
| Rescan; cancel scanning; progress; rescanning discovers added presets | ui/header.rs:218; ui/menu.rs | Present: library.rs, ui/menu.rs | W9 scan |
| Interface scale 100/125/150/200%, reset, remembered window size | ui/header.rs:261; library.rs:80 | Present: ui/header.rs; library::Settings | W9 UI render |
| Default Original / Vectorized / KONTRA performance view | ui/header.rs:278 | Present: ui/header.rs; ui/part.rs | W9 UI assets/render |
| Performance view scale Fit / 1 / 1.5 / 2 | ui/header.rs:291 | Present: ui/header.rs | W9 UI render |
| New-part MIDI: next free / omni / fixed port A–D + channel 1–16 | ui/header.rs:308 | Present: ui/header.rs; ui/mod.rs:new_part | — |
| New-part output: automatic / fixed stereo bus 1–16 | ui/header.rs:333 | Present: ui/header.rs | — |
| Global samples: disk streaming Auto / load all into RAM | ui/menu.rs:554; plugin::Selection::streaming | Missing: v2 always streams Kontakt/UVI | W9 CPU/RSS/load/underruns |
| Global auto-align timing (experimental) | ui/menu.rs:561; timing.rs | Missing: v2 Core::latency returns zero | W9 CPU/load/latency |
| Alignment only while transport plays; reported latency readout | ui/menu.rs:565 | Missing | W9 render/latency |
| Browser and keyboard visibility toggles | ui/menu.rs:531 | Present: ui/menu.rs | W9 UI |
| Appearance Plain / library color / artwork | ui/menu.rs:573 | Present: ui/menu.rs | W9 UI assets |
| Artwork blur toggle; sticky part headers toggle | ui/menu.rs:585 | Present: ui/menu.rs | W9 UI |
| Master volume drag / wheel / type / double-click reset, output meter | ui/header.rs:45 | Present, changed default/reset −12 dB → 0 dB (intentional v2 gain policy; compare at matched gains) | W9 matched benchmark |
| MIDI Thru; QWERTY enabled; panic / all notes off | ui/header.rs:70 | Present: ui/header.rs; plugin.rs | — |
| Save multi by typed name and native dialog; load saved rack + every part setting | ui/header.rs:401; plugin::SavedMulti | Present: version-2 multis only. Missing settings cannot yet round-trip | — |
| Create library from sample folder, native dialog and CLI fallback | ui/menu.rs:282,795; creator.rs | Missing: creator and dialog removed | W9 load |
| About/build info; logs/support report entry | ui/menu.rs:533 | Present: ui/logs.rs (same UI) | — |

## Per-part / rack / MIDI / keyboard

| v1 setting / feature | v1 evidence | v2 start status and evidence | CPU tag |
|---|---|---|---|
| Load, replace, add, remove, duplicate, rename, reorder by drag or menu | ui/rack.rs; ui/mod.rs; ui/menu.rs | Present | W9 load |
| Previous/next preset in library; audition; selected part or all compatible parts | ui/rack.rs:430; ui/keyboard.rs | Present | — |
| Collapse to header; resize part; sticky headers; source/status/facts | ui/rack.rs:246,533 | Present; RAM and purged facts removed | W9 telemetry |
| Per-part gain, pan, tune, mute, solo | ui/rack.rs:639; plugin::Part | Present | — |
| Per-part input port A–D, channel 1–16 / Omni | ui/menu.rs:427 | Present | — |
| MPE Off / Lower zone / Upper zone | ui/menu.rs:351; articulate::Mpe | Worse: off/lower only in v2; sampler-midi already supports upper | — |
| MPE bend range ±2/12/24/48 | ui/menu.rs:358 | Present: ui/menu.rs; sound/v2.rs | — |
| Part sample mode follows rack / force disk / force RAM | ui/menu.rs:368; plugin::Part::streaming | Missing | W9 CPU/RSS/load/underruns |
| Timing per articulation and first/legato/velocity readouts | ui/menu.rs:600; timing::Delay | Missing | W9 latency |
| Timing earlier/later by 10 ms; measured reset; exclude; measure again | ui/menu.rs:615 | Missing | W9 load/latency |
| Load native NKSN onto explicit base NKI; snapshot selector, original instrument option, metadata names/categories | ui/rack.rs:507; ui/menu.rs:294; plugin::Part::snapshot | Worse: v2 opens an NKSN as a separate source and guesses base, no selector/explicit base | W9 load |
| Per-part Original / Vectorized / KONTRA; make current view global default | ui/menu.rs:321 | Present per-part; missing make-current-default menu action | W9 UI assets |
| Reveal file/folder; copy source path | ui/menu.rs | Present | — |
| Four-octave mouse keyboard; velocity from height; glissando; cleanup on release/editor close | ui/keyboard.rs:25,518 | Present | — |
| Keyboard show/hide; octave down/up/center; range bars for every shown part | ui/keyboard.rs | Present | W9 UI |
| Keyboard authored key colors/names/active switch; playable/no-samples key menu info | ui/keyboard.rs; ui/menu.rs:414 | Worse: colors restored by prior W4, but name and mapped-status menu info removed | — |
| Pitch wheel drag/wheel/keys/reset/spring; CC1 drag/wheel/keys/reset/MIDI follow | ui/keyboard.rs:187 | Present | — |
| QWERTY piano A–apostrophe / black keys; Z/X octave; C/V velocity; text-focus/modifier exclusion; physical key release | ui/computer.rs | Present: same implementation | — |
| Articulation typed trigger, MIDI learn, remap/clear/reset, original keys policy | ui/panel.rs:1607; ui/menu.rs:461 | Present: stable per-part overlay, prior W4 | — |
| Articulation Keys / Channel / Velocity drivers; trigger channels/ranges | ui/panel.rs:1487 | Present; v2 adds CC and Program | — |
| Enable/exclude articulation from channel/velocity modes; split velocities evenly | ui/menu.rs:464; panel.rs:1530 | Worse: clear trigger supported, missing split-evenly action | — |
| Readable articulation/control names, segmented controls, long-list scroll | ui/panel.rs | Present: ui/generated.rs, inside.rs; correctness owned by UI gate | W9 UI |

## Sound, mixer and performance views

| v1 setting / feature | v1 evidence | v2 start status and evidence | CPU tag |
|---|---|---|---|
| Sound editor group previous/next; auto-selected group; all-groups / one-group scope | ui/editor.rs:201 | Missing: Sound is read-only summary | W9 voice DSP |
| Sound editor compact/expanded; envelope/filter response graphs; value fields and mapped zones | ui/editor.rs:377,607 | Worse: mapping exists, Sound limited to four envelope/filter summaries | W9 UI |
| Editable amplitude attack, curve, hold, decay, sustain, release | engine/overrides.rs:Param; ui/editor.rs | Missing | W9 CPU/voice lifetime |
| Editable each group filter cutoff/resonance | engine/overrides.rs:Param | Missing | W9 DSP |
| Editable EQ band frequency/bandwidth/gain per slot | engine/overrides.rs:Param | Missing | W9 DSP |
| Live/typed/wheel graph editing with Shift fine; reset individual parameter/graph/part | ui/editor.rs:250,329 | Missing | — |
| Player offsets over script-set base; all + one-group offsets compose; saved part state | engine/overrides.rs:1–200 | Missing | — |
| Voice envelope playheads, script-base/current curves, live spectrum under sound graphs | ui/viz.rs; editor.rs | Missing | W9 UI/probe |
| Source/group modulation assignment names, depth/invert/lag and live wheel/key/velocity source | ui/chain.rs:110 | Worse: v2 shows only limited static modulation summary | W9 UI |
| Full group/instrument insert/send/main/bus FX list, bypass state, details, unsupported marker | ui/chain.rs:223 | Worse: mixer carries names, read-only Sound lacks full details | W9 FX DSP |
| Instrument / bus / master mixer strips with levels/pan/mute/solo and meters | ui/mixer.rs:163 | Present via nested tree; v2 master lives in header | — |
| Narrow/wide strip choice with insert names on wide strips | ui/mixer.rs:261 | Worse: one fixed width | W9 UI |
| Spectrum Off / selected part / master | ui/mixer.rs:269 | Worse: v2 on/off master only | W9 scope CPU |
| Mixer part/bus rename, reset, route menu | ui/mixer.rs:604; menu.rs:493 | Worse: part rename in rack; bus names/reset shortcuts absent in tree UI | — |
| Part aux send bus Off/st.1–16 and send level | ui/mixer.rs:425; menu.rs:515 | Worse: state/core support exists, no UI | — |
| Output bus host-port assignment | ui/mixer.rs:520; menu.rs:527 | Worse: routing auto modes work, bus-port field not exposed | — |
| Automatic routing All master / per instrument / per mic; own outputs; own channels; all omni; name outputs; reset | routing.rs; ui/menu.rs:515 | Present: routing.rs, menu.rs | — |
| Original authored controls with artwork, fonts, filmstrip frames, labels, hide/layout rules | ui/perf_view.rs | Present by UI IR; parity defects tracked by render/widget owners | W9 UI assets |
| Script view selection, pages, menu/button/switch/knob/slider/XY/table/text/list/waveform interaction | ui/perf_view.rs; panel.rs | Present by UI IR; full correctness requires release gate | W9 UI/script |
| Script persistent variables and supported engine changes survive host save/restore | plugin::Part::script_state,engine_state,delay_state,ir_settings | Worse: v2 widget state persistence owners in flight; settings audit does not certify it | W9 DSP |
| Info: decoded instrument/groups/zones/samples/mapping, load stage timings/issues | ui/instrument.rs | Present: Report tab, inside::info | W9 telemetry |
| Header CPU, audible/running/muted voices, unique sample RAM/freed, disk MB/s, audio dropouts | ui/header.rs:126 | Worse: CPU + active voices only; RAM/disk absent; audible equals running; dropout label only says notes | W9 all metrics |

## Browser and libraries

| v1 setting / feature | v1 evidence | v2 start status and evidence | CPU tag |
|---|---|---|---|
| Libraries/preset panes; drag divider widths/split; remembered selection/scroll place | ui/browser.rs; plugin::Selection | Present | W9 UI |
| Search library/vendor/preset names across all or selected library; clear; keyboard navigation/Enter load | ui/browser.rs | Present | W9 UI |
| Instruments / Multis selection | ui/browser.rs | Present | — |
| Favorites add/remove, favorites-only source, recent presets, dedup/bounded history | ui/browser.rs; menu.rs:Presets | Present | — |
| Natural-sorted hierarchical preset folders; persistent open/close states, counts, selected row | ui/browser.rs:tree/flatten | Present | W9 UI |
| Sort custom / A–Z / recent / vendor; pinned first; drag reorder; move up/down; reset order | library::Settings; ui/menu.rs:LibrarySort | Present | W9 UI |
| Library display rename; source identities unchanged; reset via empty text | library.rs:rename_library | Present | — |
| Local authored covers; generated cover; pick/drop custom cover; reset; artwork aspect ratio | ui/cover.rs; ui/art.rs; ui/menu.rs | Present | W9 UI/RSS |
| Preset load/new-slot/favorite/reveal/copy menu; library pin/reveal/copy menu | ui/menu.rs | Present | — |
| Scanned multi expansion including native program routing; saved multis | plugin.rs; library.rs | Present (v2 files only) | W9 load |

## Logs, diagnostics and support

| v1 setting / feature | v1 evidence | v2 start status and evidence | CPU tag |
|---|---|---|---|
| Refresh; background bounded live history; open log folder | ui/logs.rs:455 | Present: logs UI differs only in syntax cleanup | W9 logging |
| Severity selection, text/source/code/instance/load filters, reset, this-load shortcut | ui/logs.rs:740–1010 | Present | — |
| Full event details selection/copy; copy all diagnostics; copy build info | ui/logs.rs:506,564,993 | Present | — |
| Support report preview/back; user notes; structured-path redaction toggle | ui/logs.rs:481,620,718 | Present | — |
| Export exact private crash evidence + retained rotated history; progress/error; issue link copy | ui/logs.rs:627; diagnostics.rs | Present; preserve explicit user sharing action | W9 log I/O |
| Counts for dropped/evicted/truncated/retention failures; retention limits explanation | ui/logs.rs:820 | Present | W9 logging |
| Build/audio settings and per-load trace context | plugin::diagnostic_report | Present but differs with missing settings/metrics above | W9 run ledger |

## CPU-relevant authored values and engine policies for W9

These are **not v1 user settings**. They must still be matched when comparing engines. Do not invent a UI control and then call it v1 parity.

| Value / policy | v1 evidence | v2 obligation | W9 tag |
|---|---|---|---|
| Instrument/group voice limits and victim/release policy | import::Instrument::voice_limit; engine/voice.rs | Compare authored limits, allocation ceiling, actual voiced/muted counts and stealing | W9 polyphony |
| Authored interpolation/sampling quality and high-quality loops | import::Group::interp_quality; engine/hq.rs | Match library quality; compare output as well as CPU | W9 quality |
| Authored Kontakt preload override; adaptive heads/zone starts/loops | import::Instrument::kontakt_preload; engine/bank.rs,residency.rs | v2 StreamPolicy latency-sized heads differ; test cold/warm/RSS/underruns | W9 preload |
| Sample residency, idle purge, sharing and stream-ring reclamation | engine/residency.rs; header::RAM | v2 memory-budget control is additional; measure retained process RSS and unique storage | W9 RSS/disk |
| Muted scripted mic/layer fast path; active vs audible voices | engine/voice.rs; header.rs:126 | v2 audible==active at baseline; do not compare raw counts as audible parity | W9 muted layers |
| Source filter/FX quality, bypass, modulators, script callback fuel | import,fx,ksp; per-library authored widgets | Match source values and admitted work; no dropped FX/modulators allowed | W9 DSP/script |
| Default −12 dB v1 master versus v2 0 dB | plugin::SamplerParams | Match gains in audio and CPU witnesses | W9 benchmark |
| Thread count | v1: no user setting; v2: settings threads Auto/1/2/4/8 | Record effective setting and KONTRA_THREADS override | W9 thread budget |
| Original versus vector artwork; spectrum/probe visibility; closed editor | ui/perf_view.rs,spectrum.rs,editor.rs | Record UI mode and instrumentation state per benchmark | W9 UI overhead |

## Validation and remaining gate

Initial inventory is source-grounded; full native widget, DSP, KSP/UVI and performance correctness remains the coordinator's one logged gate. Per-feature failing-first results and implementation evidence are added here as work lands. No release/install is part of this branch.

### Playback and view-control port checkpoint

Direct ports from `0cb7a8a0`: `src/engine/bank.rs` (`Streaming`, `keep_whole`, RAM reserve), `src/plugin.rs` (streaming state and snapshot validation queue), `src/import.rs` (snapshot/base metadata identity), `src/library.rs` (snapshot catalog), `src/ui/menu.rs`, `src/ui/rack.rs`, `src/ui/picker.rs`, and `src/ui/mixer.rs` (width/spectrum controls). Adapters use the v2 IR, native translators, `Loaded` handoff and mixer tree. No old KONTRA file reader was added.

Restored: rack/per-part Auto/RAM sample modes; smallest-first full residency with streaming fallback when memory is insufficient; upper MPE zone; explicit-base snapshot picker/drop/selector and previous/next navigation; rejected/stale snapshots preserve the saved/playing part; make-current-view-default; authored keyboard name/playability menu info; mixer Narrow/Wide and spectrum Off/Part/Master. The baseline-status columns above remain the start-of-task comparison; the remaining gaps are still open.

Failing-first evidence: `red-state.log` loses the three restored playback fields at serialization; `red-mixer.log` fails on absent `mix-narrow`. After the ports `green-parity2.log` passes 3/3 (state round-trip, invalid/stale snapshot preservation, reachable mixer choices). Logs are under `~/.cache/kontakto-fix-settings-parity/`. Upper-MPE runtime and RAM-budget tests, full root tests and compile-only checks are pending this checkpoint. W9 should benchmark both RAM and Auto, including the overflow-to-streaming case; v2 still preallocates its stream cache in RAM mode, so do not claim v1 RSS/CPU parity from this UI port alone.
