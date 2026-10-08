# W13 current settings/features parity

Compared frozen v1 source **0cb7a8a0** with **3f49a9a1**, plus W13 editor port **a6a5166d** plus the single-owner fix (targeted green). Frozen binaries are never rebuilt. This updates the older W4 start-of-task inventory; Present means reachable control and backing path exist, not gate certification. Authored widgets/FX are grouped by kind because libraries define an unbounded set of values.

W1 **4f1abcc7** is already an ancestor of 3f49a9a1: no remainder. Reused W4 d05b1594/f335571d/468aad70/172ac0f4 for the editor; c4167fdf timing stays HOLD until native measurement is wired. Browser/cache/artwork belong to W14; no changes to its files or Settings schema.

| Area | User-facing setting/feature | Frozen v1 source | Current v2 status | CPU impact |
|---|---|---|---|---|
| Global preferences and settings panel | Add folder of libraries, add one library folder | 0cb7a8a0:src/ui/header.rs:202; library::Root | Present: ui/header.rs; library.rs | W9 scan/load |
| Global preferences and settings panel | Add a path by typing; remove roots; root kind and per-root scan counts | 0cb7a8a0:src/ui/header.rs:174–221 | Present: ui/header.rs | W9 scan |
| Global preferences and settings panel | Import Kontakt registered folders, automatic import on first run | 0cb7a8a0:src/library.rs; ui/header.rs:214 | Present: library/kontakt.rs | W9 scan |
| Global preferences and settings panel | Rescan; cancel scanning; progress; rescanning discovers added presets | 0cb7a8a0:src/ui/header.rs:218; ui/menu.rs | Present: library.rs, ui/menu.rs | W9 scan |
| Global preferences and settings panel | Interface scale 100/125/150/200%, reset, remembered window size | 0cb7a8a0:src/ui/header.rs:261; library.rs:80 | Present: ui/header.rs; library::Settings | W9 UI render |
| Global preferences and settings panel | Default Original / Vectorized / KONTRA performance view | 0cb7a8a0:src/ui/header.rs:278 | Present: ui/header.rs; ui/part.rs | W9 UI assets/render |
| Global preferences and settings panel | Performance view scale Fit / 1 / 1.5 / 2 | 0cb7a8a0:src/ui/header.rs:291 | Present: ui/header.rs | W9 UI render |
| Global preferences and settings panel | New-part MIDI: next free / omni / fixed port A–D + channel 1–16 | 0cb7a8a0:src/ui/header.rs:308 | Present: ui/header.rs; ui/mod.rs:new_part | — |
| Global preferences and settings panel | New-part output: automatic / fixed stereo bus 1–16 | 0cb7a8a0:src/ui/header.rs:333 | Present: ui/header.rs | — |
| Global preferences and settings panel | Global samples: disk streaming Auto / load all into RAM | 0cb7a8a0:src/ui/menu.rs:554; plugin::Selection::streaming | Present: rack Auto/RAM and reload policy in plugin.rs/sound/v2.rs; overflow remains streaming fallback | W9 CPU/RSS/load/underruns |
| Global preferences and settings panel | Global auto-align timing (experimental) | 0cb7a8a0:src/ui/menu.rs:561; timing.rs | Missing: Core::latency remains zero; W4 c4167fdf not connected to measurement worker | W9 CPU/load/latency |
| Global preferences and settings panel | Alignment only while transport plays; reported latency readout | 0cb7a8a0:src/ui/menu.rs:565 | Missing: transport-only scheduling/reporting requires calibrated timing integration | W9 render/latency |
| Global preferences and settings panel | Browser and keyboard visibility toggles | 0cb7a8a0:src/ui/menu.rs:531 | Present: ui/menu.rs | W9 UI |
| Global preferences and settings panel | Appearance Plain / library color / artwork | 0cb7a8a0:src/ui/menu.rs:573 | Present: ui/menu.rs | W9 UI assets |
| Global preferences and settings panel | Artwork blur toggle; sticky part headers toggle | 0cb7a8a0:src/ui/menu.rs:585 | Present: ui/menu.rs | W9 UI |
| Global preferences and settings panel | Master volume drag / wheel / type / double-click reset, output meter | 0cb7a8a0:src/ui/header.rs:45 | Present, changed default/reset −12 dB → 0 dB (intentional v2 gain policy; compare at matched gains) | W9 matched benchmark |
| Global preferences and settings panel | MIDI Thru; QWERTY enabled; panic / all notes off | 0cb7a8a0:src/ui/header.rs:70 | Present: ui/header.rs; plugin.rs | — |
| Global preferences and settings panel | Save multi by typed name and native dialog; load saved rack + every part setting | 0cb7a8a0:src/ui/header.rs:401; plugin::SavedMulti | Present: version-2 multis only. Missing settings cannot yet round-trip | — |
| Global preferences and settings panel | Create library from sample folder, native dialog and CLI fallback | 0cb7a8a0:src/ui/menu.rs:282,795; creator.rs | Present: ui/picker.rs; creator and native sample-folder picker restored | W9 load |
| Global preferences and settings panel | About/build info; logs/support report entry | 0cb7a8a0:src/ui/menu.rs:533 | Present: ui/logs.rs (same UI) | — |
| Per-part / rack / MIDI / keyboard | Load, replace, add, remove, duplicate, rename, reorder by drag or menu | 0cb7a8a0:src/ui/rack.rs; ui/mod.rs; ui/menu.rs | Present | W9 load |
| Per-part / rack / MIDI / keyboard | Previous/next preset in library; audition; selected part or all compatible parts | 0cb7a8a0:src/ui/rack.rs:430; ui/keyboard.rs | Present | — |
| Per-part / rack / MIDI / keyboard | Collapse to header; resize part; sticky headers; source/status/facts | 0cb7a8a0:src/ui/rack.rs:246,533 | Present: RAM/freed telemetry restored; native per-part row/readout correctness still needs UI gate | W9 telemetry |
| Per-part / rack / MIDI / keyboard | Per-part gain, pan, tune, mute, solo | 0cb7a8a0:src/ui/rack.rs:639; plugin::Part | Present | — |
| Per-part / rack / MIDI / keyboard | Per-part input port A–D, channel 1–16 / Omni | 0cb7a8a0:src/ui/menu.rs:427 | Present | — |
| Per-part / rack / MIDI / keyboard | MPE Off / Lower zone / Upper zone | 0cb7a8a0:src/ui/menu.rs:351; articulate::Mpe | Present: upper/lower/off in ui/menu.rs and native MIDI adapter | — |
| Per-part / rack / MIDI / keyboard | MPE bend range ±2/12/24/48 | 0cb7a8a0:src/ui/menu.rs:358 | Present: ui/menu.rs; sound/v2.rs | — |
| Per-part / rack / MIDI / keyboard | Part sample mode follows rack / force disk / force RAM | 0cb7a8a0:src/ui/menu.rs:368; plugin::Part::streaming | Present: follows rack/Auto/RAM; saved v2 Part.streaming | W9 CPU/RSS/load/underruns |
| Per-part / rack / MIDI / keyboard | Timing per articulation and first/legato/velocity readouts | 0cb7a8a0:src/ui/menu.rs:600; timing::Delay | Missing | W9 latency |
| Per-part / rack / MIDI / keyboard | Timing earlier/later by 10 ms; measured reset; exclude; measure again | 0cb7a8a0:src/ui/menu.rs:615 | Missing | W9 load/latency |
| Per-part / rack / MIDI / keyboard | Load native NKSN onto explicit base NKI; snapshot selector, original instrument option, metadata names/categories | 0cb7a8a0:src/ui/rack.rs:507; ui/menu.rs:294; plugin::Part::snapshot | Present: explicit base selector/snapshot menu and validation before replacement | W9 load |
| Per-part / rack / MIDI / keyboard | Per-part Original / Vectorized / KONTRA; make current view global default | 0cb7a8a0:src/ui/menu.rs:321 | Present: three per-part views and make-current-default action | W9 UI assets |
| Per-part / rack / MIDI / keyboard | Reveal file/folder; copy source path | 0cb7a8a0:src/ui/menu.rs | Present | — |
| Per-part / rack / MIDI / keyboard | Four-octave mouse keyboard; velocity from height; glissando; cleanup on release/editor close | 0cb7a8a0:src/ui/keyboard.rs:25,518 | Present | — |
| Per-part / rack / MIDI / keyboard | Keyboard show/hide; octave down/up/center; range bars for every shown part | 0cb7a8a0:src/ui/keyboard.rs | Present | W9 UI |
| Per-part / rack / MIDI / keyboard | Keyboard authored key colors/names/active switch; playable/no-samples key menu info | 0cb7a8a0:src/ui/keyboard.rs; ui/menu.rs:414 | Present: colors/name/mapped information in keyboard/menu | — |
| Per-part / rack / MIDI / keyboard | Pitch wheel drag/wheel/keys/reset/spring; CC1 drag/wheel/keys/reset/MIDI follow | 0cb7a8a0:src/ui/keyboard.rs:187 | Present | — |
| Per-part / rack / MIDI / keyboard | QWERTY piano A–apostrophe / black keys; Z/X octave; C/V velocity; text-focus/modifier exclusion; physical key release | 0cb7a8a0:src/ui/computer.rs | Present: same implementation | — |
| Per-part / rack / MIDI / keyboard | Articulation typed trigger, MIDI learn, remap/clear/reset, original keys policy | 0cb7a8a0:src/ui/panel.rs:1607; ui/menu.rs:461 | Present: stable per-part overlay, prior W4 | — |
| Per-part / rack / MIDI / keyboard | Articulation Keys / Channel / Velocity drivers; trigger channels/ranges | 0cb7a8a0:src/ui/panel.rs:1487 | Present; v2 adds CC and Program | — |
| Per-part / rack / MIDI / keyboard | Enable/exclude articulation from channel/velocity modes; split velocities evenly | 0cb7a8a0:src/ui/menu.rs:464; panel.rs:1530 | Present: Include and Split actions in inside.rs/menu.rs | — |
| Per-part / rack / MIDI / keyboard | Readable articulation/control names, segmented controls, long-list scroll | 0cb7a8a0:src/ui/panel.rs | Present: ui/generated.rs, inside.rs; correctness owned by UI gate | W9 UI |
| Sound, mixer and performance views | Sound editor group previous/next; auto-selected group; all-groups / one-group scope | 0cb7a8a0:src/ui/editor.rs:201 | Present in W13 port: editor.rs group selection/all/one; targeted editor checks passed | W9 voice DSP |
| Sound, mixer and performance views | Sound editor compact/expanded; envelope/filter response graphs; value fields and mapped zones | 0cb7a8a0:src/ui/editor.rs:377,607 | Present in W13 port: compact/expanded envelopes/filter graphs and mapping; targeted editor checks passed | W9 UI |
| Sound, mixer and performance views | Editable amplitude attack, curve, hold, decay, sustain, release | 0cb7a8a0:src/engine/overrides.rs:Param; ui/editor.rs | Present in W13 port: admitted native bindings and law conversion; signed curves need validation | W9 CPU/voice lifetime |
| Sound, mixer and performance views | Editable each group filter cutoff/resonance | 0cb7a8a0:src/engine/overrides.rs:Param | Present: admitted cutoff/resonance offsets; native LP4 cutoff Hz, typed inverse, drag scale and actual kernel response checked red→green | W9 DSP |
| Sound, mixer and performance views | Editable EQ band frequency/bandwidth/gain per slot | 0cb7a8a0:src/engine/overrides.rs:Param | Present: native EQ handles and serial Filter responses; combined EQ release proof remains UNKNOWN | W9 DSP |
| Sound, mixer and performance views | Live/typed/wheel graph editing with Shift fine; reset individual parameter/graph/part | 0cb7a8a0:src/ui/editor.rs:250,329 | Present in W13 port: v1 interactions/reset; targeted targeted editor checks passed | — |
| Sound, mixer and performance views | Player offsets over script-set base; all + one-group offsets compose; saved part state | 0cb7a8a0:src/engine/overrides.rs:1–200 | Present in W13 port: separate script base + additive native addressed offsets; reload/state tests pending | — |
| Sound, mixer and performance views | Voice envelope playheads, script-base/current curves, live spectrum under sound graphs | 0cb7a8a0:src/ui/viz.rs; editor.rs | Present in W13 port: epoch-gated bounded voice probes, live graphs/spectrum; targeted editor checks passed | W9 UI/probe |
| Sound, mixer and performance views | Source/group modulation assignment names, depth/invert/lag and live wheel/key/velocity source | 0cb7a8a0:src/ui/chain.rs:110 | Present in W13 port: chain.rs names/source/depth/invert/lag; v2 handles retained | W9 UI |
| Sound, mixer and performance views | Full group/instrument insert/send/main/bus FX list, bypass state, details, unsupported marker | 0cb7a8a0:src/ui/chain.rs:223 | Present in W13 port: chain.rs processor/bypass/details; unadmitted effects still require DSP gate | W9 FX DSP |
| Sound, mixer and performance views | Instrument / bus / master mixer strips with levels/pan/mute/solo and meters | 0cb7a8a0:src/ui/mixer.rs:163 | Present via nested tree; v2 master lives in header | — |
| Sound, mixer and performance views | Narrow/wide strip choice with insert names on wide strips | 0cb7a8a0:src/ui/mixer.rs:261 | Present: mixer strip-width setting and wide insert names | W9 UI |
| Sound, mixer and performance views | Spectrum Off / selected part / master | 0cb7a8a0:src/ui/mixer.rs:269 | Present: Off/Part/Master selector | W9 scope CPU |
| Sound, mixer and performance views | Mixer part/bus rename, reset, route menu | 0cb7a8a0:src/ui/mixer.rs:604; menu.rs:493 | Present: rename/reset/routes menu port | — |
| Sound, mixer and performance views | Part aux send bus Off/st.1–16 and send level | 0cb7a8a0:src/ui/mixer.rs:425; menu.rs:515 | Present: aux bus/gain UI and saved state | — |
| Sound, mixer and performance views | Output bus host-port assignment | 0cb7a8a0:src/ui/mixer.rs:520; menu.rs:527 | Present: BusPort menu | — |
| Sound, mixer and performance views | Automatic routing All master / per instrument / per mic; own outputs; own channels; all omni; name outputs; reset | 0cb7a8a0:src/routing.rs; ui/menu.rs:515 | Present: routing.rs, menu.rs | — |
| Sound, mixer and performance views | Original authored controls with artwork, fonts, filmstrip frames, labels, hide/layout rules | 0cb7a8a0:src/ui/perf_view.rs | Present by UI IR; parity defects tracked by render/widget owners | W9 UI assets |
| Sound, mixer and performance views | Script view selection, pages, menu/button/switch/knob/slider/XY/table/text/list/waveform interaction | 0cb7a8a0:src/ui/perf_view.rs; panel.rs | Present by UI IR; full correctness requires release gate | W9 UI/script |
| Sound, mixer and performance views | Script persistent variables and supported engine changes survive host save/restore | 0cb7a8a0:src/plugin::Part::script_state,engine_state,delay_state,ir_settings | Worse/UNKNOWN: state owners W5/W7; settings source inventory does not certify every script/engine restore | W9 DSP |
| Sound, mixer and performance views | Info: decoded instrument/groups/zones/samples/mapping, load stage timings/issues | 0cb7a8a0:src/ui/instrument.rs | Present: Report tab, inside::info | W9 telemetry |
| Sound, mixer and performance views | Header CPU, audible/running/muted voices, unique sample RAM/freed, disk MB/s, audio dropouts | 0cb7a8a0:src/ui/header.rs:126 | Present: CPU/running/audible/muted/RAM/freed/disk/dropouts; live-host proof pending | W9 all metrics |
| Browser and libraries | Libraries/preset panes; drag divider widths/split; remembered selection/scroll place | 0cb7a8a0:src/ui/browser.rs; plugin::Selection | Present | W9 UI |
| Browser and libraries | Search library/vendor/preset names across all or selected library; clear; keyboard navigation/Enter load | 0cb7a8a0:src/ui/browser.rs | Present | W9 UI |
| Browser and libraries | Instruments / Multis selection | 0cb7a8a0:src/ui/browser.rs | Present | — |
| Browser and libraries | Favorites add/remove, favorites-only source, recent presets, dedup/bounded history | 0cb7a8a0:src/ui/browser.rs; menu.rs:Presets | Present | — |
| Browser and libraries | Natural-sorted hierarchical preset folders; persistent open/close states, counts, selected row | 0cb7a8a0:src/ui/browser.rs:tree/flatten | Present | W9 UI |
| Browser and libraries | Sort custom / A–Z / recent / vendor; pinned first; drag reorder; move up/down; reset order | 0cb7a8a0:src/library::Settings; ui/menu.rs:LibrarySort | Present | W9 UI |
| Browser and libraries | Library display rename; source identities unchanged; reset via empty text | 0cb7a8a0:src/library.rs:rename_library | Present | — |
| Browser and libraries | Local authored covers; generated cover; pick/drop custom cover; reset; artwork aspect ratio | 0cb7a8a0:src/ui/cover.rs; ui/art.rs; ui/menu.rs | Present | W9 UI/RSS |
| Browser and libraries | Preset load/new-slot/favorite/reveal/copy menu; library pin/reveal/copy menu | 0cb7a8a0:src/ui/menu.rs | Present | — |
| Browser and libraries | Scanned multi expansion including native program routing; saved multis | 0cb7a8a0:src/plugin.rs; library.rs | Present (v2 files only) | W9 load |
| Logs, diagnostics and support | Refresh; background bounded live history; open log folder | 0cb7a8a0:src/ui/logs.rs:455 | Present: logs UI differs only in syntax cleanup | W9 logging |
| Logs, diagnostics and support | Severity selection, text/source/code/instance/load filters, reset, this-load shortcut | 0cb7a8a0:src/ui/logs.rs:740–1010 | Present | — |
| Logs, diagnostics and support | Full event details selection/copy; copy all diagnostics; copy build info | 0cb7a8a0:src/ui/logs.rs:506,564,993 | Present | — |
| Logs, diagnostics and support | Support report preview/back; user notes; structured-path redaction toggle | 0cb7a8a0:src/ui/logs.rs:481,620,718 | Present | — |
| Logs, diagnostics and support | Export exact private crash evidence + retained rotated history; progress/error; issue link copy | 0cb7a8a0:src/ui/logs.rs:627; diagnostics.rs | Present; preserve explicit user sharing action | W9 log I/O |
| Logs, diagnostics and support | Counts for dropped/evicted/truncated/retention failures; retention limits explanation | 0cb7a8a0:src/ui/logs.rs:820 | Present | W9 logging |
| Logs, diagnostics and support | Build/audio settings and per-load trace context | 0cb7a8a0:src/plugin::diagnostic_report | Present but differs with missing settings/metrics above | W9 run ledger |

Additional v2 user-facing controls/features (v1 0cb7a8a0 has no corresponding setting):

| v2 addition | Status/source |
|---|---|
| Voice-render threads Single/Auto/1/2/4/8 | Present: library::ThreadSetting, ui/header.rs; effective setting and env override must be recorded in CPU receipts |
| Instrument-specific remembered performance view/reset-to-default | Present: ui/menu.rs and Settings.instrument_views; W14 owns prefs edits |
| Smart memory budget choices | Present: Selection.memory_budget_mb, ui/menu.rs; performance/RSS gate still required |
| Articulation CC and Program triggers | Present: sound/articulation.rs, ui/inside.rs/menu.rs; stable source overlay IDs |
| Typed native UI IR widget/control paths | Present: ui/native_ui.rs/generated.rs; exact native usability belongs to UI release gate |
| Typed processor/control inspection and coverage reports | Present: ui/inside.rs Info, sound/Report; admission does not certify all source DSP |
| Host note IDs, exact expression ownership and sample-exact automation | Present: native wrappers/core adapter; W1 4f1abcc7 already integrated |
| Native UVI state/source identity retention | Present: plugin::Part.uvi_state/uvi_state_source; W10/W5 own full restore proof |

Path/cache audit: v1's Settings.path/config/data helpers persist roots, artwork and saved Multis; v2 retains these helpers. Browser cache/stat-diff/artwork are W14's active scope. Existing v2 Settings still reads legacy vector flags and flattens unknown v1 keys with a shared-file comment; coordinator notified for W14 ownership because old settings format compatibility is banned. This is a **worse** trust/version boundary until fixed, not a parity benefit.

User-impact queue: (1) functional Sound editor, (2) calibrated timing alignment, (3) remaining engine/script persistence under W5/W7, (4) browser/cache/artwork under W14. Per-feature release proof remains the single unified gate; no release or install.

Validation: baseline **7bf6fa36** fails `ui::v2_tests::v1_sound_editor_controls_are_reachable` on absent `edit-group-prev`. Port first ran 20/22; single-editor selection failure fixed at `inside::bar`, malformed native-state test fixture gained required `name`. Second run **22/22 passed** (typed/drag/wheel/fine/reset/scopes/groups/state/reload included). `cargo test --profile ci --no-run` passed. Receipts: `~/.cache/kontakto-w13/editor-red.log`, `editor-green.log`, `editor-green2.log`, `no-run.log`.

Native LP4 follow-up: the editor previously displayed normalized cutoff as Hz, clamped typed Hz to full cutoff, and omitted LP4 from its graph. Ported v1's native-cutoff/graph-octave conversion (`0cb7a8a0:src/ui/viz.rs`) onto the existing pinned playback kernel; base/live offset graphs and current native gain use the same service values. Regression covers Hz readout, typed 1 kHz, handle axis/drag law, small-signal rolloff, base/edited separation and gain lane. Arc retirement follow-up fba565c9 passed A→B equal-content ownership without revision bump plus invalid→valid retry.

Loaded host evidence is in [w13-live-host.md](w13-live-host.md): 28 QUIET cells, candidate CPU FAIL with explicit UNKNOWN gaps.

NEXT: combined-head settings validation; calibrated timing and browser/cache remain with their owners.
