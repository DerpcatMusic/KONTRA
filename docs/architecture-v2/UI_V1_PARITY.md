# V1 UI parity audit

Reference: `origin/main` commit **0cb7a8a0**, before v2. The supplied read-only
`decipher-readers-v2` checkout is now at a v2 commit; this audit reads the pinned
v1 tree with `git show`, rather than treating its current files as v1.

Status compares the implementation on `v2/gpt-kontakt-ui` with v1. **Present**
means the path exists, not that every installed instrument passes it. **Worse**
means a retained path loses behaviour or fidelity. **Missing** means no equivalent
working path. Corpus counts and timings are measurements only when explicitly
labelled measured. Order prioritizes the user's Conflux and header reports, then
shared paths affecting the greatest number of instruments.

## Authored interfaces and controls

| V1 feature or behaviour | V1 source | V2 status at audit | Current work / evidence |
| --- | --- | --- | --- |
| Conflux original authored UI, pages and KSP callbacks | `perf_view`, `ksp/runtime`, `artwork` | Worse; reported by user | Targeted real-library probe queued; no parity claim yet |
| Header landscape picture button selects presentation | `rack::header_at`, `menu::Target::View` | Missing | Restoring header picture toggle Original ↔ Vectorized |
| Default Original; per-part choice overrides global preference, survives state recall | `perf_view::mode/code/shows`, `plugin::Part::view`, `library::ViewMode` | Missing | Restoring v1 codes 0/1/3 and global default; code 2 still lacks generated panel |
| Separate generated KONTRA panel: readable names, sections, extracted lists and routing | `panel::sections/view`, `instrument::stage_page` | Missing | Separate from Vectorized; do not call authored-layout vector rendering equivalent |
| Original bitmap KSP widgets at authored positions and declaration/z-order | `perf_view::layout/view` | Present, partial | Shared UI IR renderer; corpus survey checks geometry/resources |
| Vectorized controls at the same authored positions | `vector::plan`, `perf_view::view` | Present, worse | Preserves geometry, but lacks v1 text planning and native marker treatment |
| Creator Tools `.nckp` hierarchy and declared variables | `ksp/performance_view`, runtime preparation | Present, partial | v2 `sampler_ksp::nckp`, Kontakt resource loader; literal view-name resolution |
| Komplete/native UI request | `ksp/compile` | Missing in both | V1 diagnostic/fallback is not a Komplete Script runtime; v2 survey marks unavailable frontend |
| PNG strips, companion TXT frame count/axis, dimensions | `artwork`, `perf_view::frame` | Present | Bounded NKR/NICNT routing added; loose resource precedence retained |
| Wallpaper removes Kontakt's 68px instrument header | `perf_view::HEADER/view` | Worse | V2 uses only skin offset; investigate before changing crop |
| Wallpaper state/skin offset, huge-strip visible-window cropping | `fitted::window`, `Picture::wallpaper` | Worse | V2 selects first wallpaper frame and does not crop a GPU-safe window |
| Stretch margins / nine-slice drawing | `perf_view::sliced` | Worse | V2 stretches full bitmap frame; IR keeps margins but renderer ignores them |
| Bitmap font glyphs and advances, state-dependent fonts | `bitmap_words`, `font_id`, `artwork::font_key` | Missing | V2 records font metadata but draws native captions |
| Native font IDs, sizes/weight/colour, text X/Y offsets, alignment, wrapping and multiline labels | `text_font`, `words`, `caption_of` | Worse | V2 native style subset; single-line renderer ignores some retained properties |
| Picture hover/pressed/off/on states | `switch_frame`, `picture_frame` | Worse | V2 selects off/on only |
| Knob/slider drag, wheel, keyboard, reset, saved default | `drive`, `travel`, `drags_vertically` | Present, worse | V2 fixed travel ignores source sensitivity/track distance |
| Number edit drag and double-click typed value | `type_in`, value-control branch | Worse | V2 drag exists; typed entry missing |
| Menu popup, visible/enabled entries, direct semantic value selection | `menu::Target::Script` | Worse | V2 cycles entries; passive rendering no longer changes unmatched values |
| Native Kontakt menu persistence restores saved position; host state restores semantic value | runtime persistence, plugin state | Present after fixes | Separate restore paths and callback checks committed |
| Buttons/switches move KSP variable and run `ui_control` | `Shared::edit_control`, runtime | Present | Queue generation, admission and callback checks added |
| Script-driven value/text/visibility changes wake editor | optimistic snapshots, `Watch` | Present after fixes | Scalar revision triggers redraw; immutable interface publication retained |
| Script-page tabs in footer, page selection retained across publications | `instrument::stage` | Worse | V2 script tabs exist above UI; interface republish may reset selected script |
| Tables display actual dense arrays at declared range | `perf_view::face` | Missing | V2 draws empty one-pixel bars; array values not sent to renderer |
| Waveform from attached sample, async peaks and script cursor | `wave::ask`, `wave_now`, `face` | Missing | V2 placeholder only |
| File selector opens picker and validates script epoch before callback | `picker::Ask::ScriptFile`, `perf_view::control` | Missing | V2 placeholder only |
| Text edit display | `caption_of`, `face` | Worse | V1 itself did not implement authored text mutation; v2 placeholder loses current text |
| Mouse-area / XY / unknown widgets | `Kind::Area/Other`, `face` | Partial in both | Do not count v1 outline/blank fallback as functional interaction |
| Level meter colours / inactive face | `perf_view::face` | Worse | Neither v1 path proves live instrument meter binding; v2 paints silent default |
| Automatic scale fits width **and height**, explicit 0.5/1/2 zoom and scrolling | `scale_to_fit`, `room_height` | Worse | V2 width-only 0.5–1 clamp ignores global zoom |
| Pixel-snapped authored geometry at fractional device scale | `perf_view::view` | Worse | V2 leaves source placement multiplied by scale |
| Async artwork/resource loading, cached controls, dependency memoization | `perf_view::fetch/control_deps`, `fitted`, `art` | Worse | V2 synchronous first asset decode; steady frames need measurement |
| Per-part/script control identity and queued edit isolation | shared/runtime | Present after fixes | V2 namespaced render IDs + stable 128-bit control IDs; stale edits rejected |

## Remaining editor features

These are player-facing v1 paths, included to avoid limiting the audit to two
visible buttons. The scope is parity of the editor, not a rewrite of the v2 engine.

| V1 feature or behaviour | V1 source | V2 status / limitation |
| --- | --- | --- |
| Library covers, own art/generated/custom art, themes, blur | `art`, `cover`, `theme`, browser/menu | Present |
| Browser library/folder split, search, favourites, recent, library sort/pin/rename | `browser`, `browser_geometry`, `menu` | Present |
| Snapshot browser, snapshot header selection/stepping and drops | browser, rack tests `snapshot_*` | Missing; v2 header steps preset files |
| Multi rack open/save, append/replace/duplicate/reorder, drag/drop | `rack`, `header`, `menu` | Present |
| Sticky/foldable/resizable headers, preset previous/next, part rename/remove | `rack` | Present |
| Header MIDI port/channel/MPE/bend, output routing, gain/pan/tune/solo/mute/meter | `rack`, `menu` | Present |
| Articulation lists, keyswitch/velocity/channel/CC/program remap, typing/MIDI learn/reset | `panel`, `instrument` | Present, worse; v2 articulation metadata view lacks v1 authored-list extraction and full inline learn/edit paths |
| Sample/group mapping selection and zone details | `instrument`, `viz` | Present |
| Sound tab envelope handle edits in override layer | `instrument`, `viz`, tests | Worse; v2 Sound view readout lacks v1 interactive override path |
| Mixer buses, strips, routing, solo/mute, live meters | `mixer`, `chain` | Present; v2 uses output tree mixer |
| Effect-chain visual editing and bypass | `chain`, `mixer` | Worse; v2 tree shows decoded processors but does not preserve all v1 editable effect controls |
| Onscreen piano, computer keys, glissando, selected-part/all-part play, panic | `keyboard`, `computer` | Present |
| Keyboard note colours, key ranges/keyswitches, live sound glow | keyboard, `panel::keyswitches`, tests | Present, partial; authored key publication remains source-dependent |
| Warnings/load failures, missing samples, loading progress, diagnostics/about/logs | `instrument::notices`, `logs`, `audit` | Present; v2 adds structured load reports |
| Memory/streaming/worker settings and output naming | app menus, mixer | Present, subset; engine behaviour belongs to its owners |
| Window resize, browser hiding, theme setting persistence | editor/header/menu | Present |
| Idle redraw gating and independently painting mixer meters | `Watch`, meter canvases | Present; scalar redraw regression repaired |
| Offscreen screenshots, real-instrument frame benchmark with staged CPU/GPU timings | `audit`, tests | Worse; v2 screenshots exist, adding timings to survey |

## Measurement protocol and current results

Use the exact installed Conflux NKI, same initial authored page and Original
presentation for comparisons. Separate file parse/frontend initialization, asset
decode, first frame, and warm build/layout/CPU paint/raster. Warm 8 frames, sample
24 idle and 24 changing frames; report median and p99. CPU offscreen timings are
not a DAW GPU frame-rate claim. V1 has an opt-in staged benchmark at
`ui::audit::fixtures::real_instrument_frame_benchmark`; no comparable v1 run has
yet completed in this resumed session. User-observed v1 lag is qualitative.

Conflux targeted load/render and measurements: **pending**, not zero and not a
passing result. The earlier 40-item partial corpus is not Conflux coverage and
must not be used as a denominator for the whole 834-item Kontakt corpus.
