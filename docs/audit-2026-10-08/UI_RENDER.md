# Rendering audit — 2026-10-08

Baseline: `origin/integrate/core-v2@7e82b152b46b31e8b9fd85c2ac01d01dad669ddd`.
Branch: `audit/ui-render-20261008`. This phase changes probes and documentation only.

## Verdict and evidence boundary

**Worse than the pinned v1 rendering implementation; incomplete Kontakt support.**
The user reports that v1 rendered most original library UIs correctly, including
Conflux. Treat that as the product baseline. For every failing mechanism, inspect
and reuse the render path at `0cb7a8a0` first; then extend only the missing source
semantics. The per-mechanism v1 map below makes that order explicit.
V2 retains typed presentation data, but drops bitmap fonts, text layout, sliced
resizing, several image states and actual table/meter/waveform painting. Conflux's
authored frontend is absent. Passing a CPU render means that our fallback drew;
it does not establish the authored UI, working callbacks or native fidelity.

The requested Kontakt census denominator is 834 installed paths; the shared paired
sweep also covers 660 UVI IDs, for 1,494 total. The scanner extension is installed;
affected/unlocked counts remain pending until its coverage is complete. A multi counts
once as a file; every program/script within it is inspected separately. Counts overlap and must not be added as independent
instruments unlocked. An affected-file count is potential reach of a mechanism,
not a promise that fixing it alone completes that file's UI.

`src/ui/render_audit.rs` follows production source decoding and initialization:
`sampler_kontakt::read/read_program` → the same saved scalars/numeric arrays and
group/slot environment as `load.rs:523` → `.nckp` resource resolution →
`compile_with` → `Script::ui` → `ir_view::resolved` → `pictures::Source` →
the production MUI/Vello CPU painter. No sample bank is decoded by this witness.
It paints each compiled script's initial page after init/persistence; later pages
requiring callbacks, listeners or NativeUI are not magically included. Frames
are bounded to 1200×900; declared dimensions are recorded separately. This is
an initial-state rendering witness, not an exhaustive reachable-state census.

Only counts, timings, known API names, geometry, colour values and hashes are
saved; no decrypted script, image, font, sample or key is written. The few PNGs
are small screenshots of OUR renderer, below 50 MB total. Historical small-corpus
numbers in `KSP_SEMANTICS.md` are not substituted for this 834-path corpus.

**Process correction:** I mistakenly started an independent render-only sweep
before rechecking the shared-scanner addendum. It was stopped at 800 cached paths,
and its list-driven collector was removed. Those partial supplemental metrics
remain only in `~/.cache/kontakto-audit-ui-render/corpus/`; they are not the
canonical full census or load/UI verdicts. The committed opt-in probe takes one
instrument. Use `~/.cache/kontra-scan/bin/kontra-scan-v2` and its README for corpus
collection. The earlier KSP audit's token list omits recognized APIs; it cannot
supply complete source-reference counts. Matrix counts remain pending rather
than treating absent tokens as zero uses.

## Conflux: complete cause of the pale blank

Witness: `Conflux 1.1.0 [Native Instruments]/Instruments/Conflux.nki`;
SHA-256 `dbe61912507d2ab46404f68a468a5194c039f1e7ca17864af92f2b6f0634d676`.
No wallpaper override, guessed alias or substitute image was used.

| Measurement | Result |
|---|---:|
| Compiled scripts / emitted interfaces | 3 / 3, original slots 2, 3, 4 |
| Main Creator Tools interface | 411 widgets; 134 visible |
| Other interfaces | 22 widgets / 5 visible; 1 widget / 0 visible |
| Main visible controls | 78 knobs, 8 value edits, 8 switches, 6 menus, 2 buttons |
| Main declared size / renderer's expanded size | 970×499 / 970×592 |
| Visible widgets below the declared page bottom | 19 |
| Main background | RGBA `(240,239,228,255)` |
| Main pixels within ±2 of that background | **94.94%** |
| Main pixels ≥245 in all RGB channels | 0%; the user's “white” is pale cream |
| Main classic assets / successful decodes | 4 / 3 |
| Classic wallpaper resolved | **No** |
| Bitmap pixels resident / Vector pixels, existing baseline test | 11,112 bytes / 5,040 bytes |
| Main native UI load requests at init | **1** |
| Own NKR members / native_ui namespace members | **322 / 282** |
| Readable `.nui` files in own NKR | **134 / 134**; 727,378 total bytes in memory |
| Classic KSP custom fonts requested by main script | 0 |

The reproduction is visible in
`~/.cache/kontakto-audit-ui-render/real/Conflux-{bitmap,vector}.png` and the
saved-value render in `conflux-detail/part-0-slot-2-bitmap.png`. JSON retains the
pixel hash and numeric geometry. The existing real-instrument memory test passed
in 36.18 seconds in debug; the optimized metadata/render witness took 4.79 seconds
before archive enumeration. The archive witness and synthetic checks also passed.
These are different probes, not a controlled load-speed or GPU frame-rate comparison.

The shared scanner independently reports `loads=yes`, `ui=missing-images`,
`controls_bound=107/113` across the visible views, and `plays_note=yes` for its
bounded audition. Its main-view pixel hash is identical to this witness:
`1de618412e4997cd4436556a4f9f77da5dead3c7b130d0a00c6c41a24dd13011`.
The final shared witness records **93.90%** declared cream background with its
one-unit pixel tolerance; the supplemental **94.94%** above uses ±2. These are
different measurement definitions, not a changed screenshot. The extension
`abf248cd` witness reports 5,212.61 ms load admission and 229.69 MiB peak worker RSS.
These whole-worker figures include playback/asset work absent from the render-only
probe. That witness's v2 binary SHA-256:
`d4534838916e008d32a6e0763541a8bd9285651d77f130bad18fe926d0975f5a`;
record `~/.cache/kontra-scan/results/v2/cache/06cfd71d81310ddba20c5a1640431d9909985050fc4ec519c5f8a3629886189d.json`.
Timing differences between scanner revisions do not establish a controlled
performance improvement. This scanner still measures the frozen integration
baseline's classic fallback, not W3's new NativeUI graph.
Audible output and bound scalar controls do not prove gesture, callback or native
UI fidelity.

The extension observes five raw wire slots: two empty and three inline active,
with no bypassed, linked-only or decode-failed slots. All three final runtime
compilations are admitted and clean; three init callbacks and two actual
`persistence_changed` callbacks complete, with no load-fault records. Slot 4 has
no persistence callback, rather than a failed completion. Import-harvest and
dynamic-rack attempts are kept separate from these final counts. Raw saved
sigils are `$:223`, `%:15`, `@:6`, `!:13`; production-admitted state excludes the
13 `!` entries. Admission alone does not establish declaration-aware restoration.
The main page has one strip, nine frames and one margin-bearing picture. Asset
failures distinguish one lookup miss from six unavailable font-service uses;
custom bitmap-font uses remain zero. Its audition uses MIDI 60 / velocity 64
from zone coverage, is marked `matched-note-plan`, and explicitly records
`fallback_note=0`. It observes 1,985 surviving sample zones and six visible typed
targets in `bound_typed`, separately from the 107/113 scalar binding criterion.
`phantom_free_controls` is unknown; do not subtract guessed library-specific
controls or treat typed readback as a successful text/array gesture.

The final pinned-v1 pair is
`~/.cache/kontra-scan/results/v1/cache/aff9c78de29075f5c197a8f7a8d0daefc8dff64ce75f1a85ff1bc9e1ae69683b.json`.
Both records complete and audition the same MIDI 60 / velocity 64 without
fallback. They retain the same prior render hashes and differing classic views;
this is a paired scanner observation, not a native-graph fidelity or same-content
render-speed acceptance result.

| Final Conflux measurement | Pinned v1 `0cb7a8a0` | V2 baseline `7e82b152` |
|---|---:|---:|
| Raw slots: inline / empty / decode failed | 3 / 2 / 0 | 3 / 2 / 0 |
| UI verdict | missing-images | missing-images |
| Scalar bindings / typed targets | 78/78 / 5 | 107/113 / 6 |
| Init / persistence callbacks completed | 3 / 1 | 3 / 2 |
| Raw saved-entry histogram | unknown | decoded: `$223`, `%15`, `@6`, `!13` |
| Production-admitted saved entries | `$223`, `%15`, `@6`, `!13` | `$223`, `%15`, `@6` |
| First finite nonzero audio / first CPU paint | 146.11 / 354.09 ms | 5,257.57 / 5,262.49 ms |
| Product cache state / peak worker RSS | cold / 88.96 MiB | cold / 229.69 MiB |
| Retained sample mappings | 1,985 | 1,985 |

V1 wire slot 2 maps to runtime slot 0 and its persistence callback is still
`waiting` / `not_reached` at observation, not a reported fault. V1's raw saved-table
format is unknown, independently of its correctly decoded source-slot partition;
incomplete raw sigil totals must stay unknown. Neither version exposes
`phantom_free_controls` here. V1 whole-editor background coverage is unknown;
its 1180×900 image cannot serve as a native Conflux page golden.

The causal chain is:

1. **The instrument asks for an authored native frontend.** `load_native_ui` is
   admitted in `sampler-ksp/src/builtins.rs:182` and retained only as a request in
   `eval.rs:1260`. `load.rs:641` publishes `script.ui()` without consuming that
   request. There is no NativeUI package/session/render path in this integration
   tree. The main interface's `.nckp` is successfully loaded; this is not a failed
   NKI decrypt or failed `.nckp` lookup.
2. **The editor chooses the exposed KSP fallback.** `part.rs:46` selects the
   interface with the most widgets. It is the 411-widget classic/Creator Tools
   view, not the authored native component tree. `.nckp` sets the pale background
   (`nckp.rs:44`, `eval.rs:636`); `ui.rs:270` carries it into the page;
   `ir_view.rs:178` paints it. All 78 visible fallback knobs have 85×52 stock
   rectangles. Their presence is not evidence that the native interface loaded.
3. **The classic wallpaper is not drawn.** Its requested classic asset fails
   lookup; `pictures.rs:20` returns `None`, `Assets::sync` caches that failure,
   and `ir_view.rs:181` silently draws no image. The own NKR/NICNT indexes have no
   classic wallpaper member; their native assets use a separate namespace.
   Changing Original/Vector cannot construct a missing frontend. Browser artwork in `.nicnt` is a different
   namespace from a performance wallpaper and cannot be used as an alias.
4. **The contrast fallback misses the actual background.** `light_under` samples
   parent pictures or a loaded wallpaper, then returns false at
   `ir_view.rs:111`; it never evaluates `page.background.color`. Thus the dark
   plate policy at line 367 does not activate over this pale colour. The PNG's
   common fallback ink RGB `(231,233,235)` has only **1.05:1** contrast against
   `(240,239,228)`; factory white `(254,254,254)` would be **1.15:1**. NativeUI
   artwork, layout and fonts are absent, and the exposed fallback becomes faint.
5. **Fallback visual meaning is additionally lost.** Four visible level meters
   are hard-coded silent (`ir_view.rs:347`), six text edits display the caption
   rather than current string state (line 356), alignment and text offsets are
   ignored, and picture margins are stretched wholesale. Those are systemic
   omissions; they are not a reason to invent Conflux-specific geometry or art.

The native package is the legacy Lua/`.nui` family described in the read-only
`codex/conflux-native-ui@4da70640` investigation, **not automatically modern typed
Komplete Script**. The archive witness in
`~/.cache/kontakto-audit-ui-render/conflux-archive/witness.json` finds its own
`Samples/Conflux.nkr` readable, with 322 members: 282 under `native_ui`, 6 under
`pictures`, 1 under `performance_view`, and 33 other. All 134 `.nui` members read
successfully in memory (727,378 bytes). This counts every `.nui` archive member;
the earlier investigation's 128-module count is a different measure. The own
NICNT has 17 other members and no `.nui`. Both own indexes lack the classic
`Resources/pictures/wallpaper.png` member. The package is present; routing it
into an authored frontend is missing. The prior native candidate renders module
graphs,
loads WebP/SVG/PNG/JPEG and TTF/OTF, and bridges exposed KSP controls; it targets the
older composition root and needs deliberate v2 integration and validation.

The separate “knobs cannot move” complaint belongs to widgets/loop. This render
probe shows 102 visible scalar-bound widgets in the main fallback, but does not
claim that audio admission, callbacks or UI publications preserve a drag. The
actual visible main knobs are `Kind::Knob`, whose render path requests vertical
drag; attributing ALL Conflux knobs to the generic horizontal-slider bug would be
incorrect. Missing native gestures, overlapping fallback faces, shared widget IDs
and publication resets need those owners' end-to-end input traces.

V1 qualification: pinned `0cb7a8a0` has font/slicing/text/wallpaper machinery,
but the prior measured Conflux capture itself had 118 visible controls, zero
continuous controls and no wallpaper. Its 6.333 ms vs v2 36.430 ms warm CPU means
are **unequal content**, not a same-quality speed ratio. That narrow probe does not overturn the user's observation that installed v1
rendered most original UIs correctly. Capture the installed v1 page to explain
the difference between that working build and this specific pinned-fixture probe. See the read-only
`audit-ui/docs/audit-2026-10-08/ui.md:39` and `ui-frames.json`.

## Systemic mechanisms, fixes and acceptance tests

All fixes below apply to a frontend or shared rendering mechanism. No product fix
was made in this phase. Reach and unlock counts remain pending in the matrices;
no fix is claimed to complete an instrument before differential validation.

| ID / priority / effort | Root cause and concrete target | Test that proves the fix |
|---|---|---|
| R0 / P0 / S | `part.rs:55` defaults clean interfaces to Vector, contradicting the user’s Original-by-default requirement. Start in Bitmap regardless of unsupported metadata; preserve user selection independently of `Arc` replacement and use a separate semantic generated view. Coordinate selection lifetime with loop. | Clean, unsupported and missing-art interfaces all initially select Original. Value/page changes and fresh interface publications do not change the user’s selection. |
| R1 / P0 / L | Authored frontend requests stop at the KSP request model. Preserve frontend family and package/version identity; route legacy NativeUI to a bounded off-audio session/component painter and shared control bindings. Keep modern Komplete Script as a distinct implementation. Targets: `load.rs`, source/frontend descriptor, `part.rs`, native package/session/painter integration. Reuse the existing candidate, not library aliases. | A tiny authored `.nui` package with nested stacks, conditional children, local font, image and exposed KSP knob must produce the specified geometry and callback trace. Then every Conflux native page, same values and size as Kontakt; no KSP fallback classified as success. |
| R2 / P0 / S | Contrast sampling ignores a page's solid colour and non-parent overlays. `ir_view.rs:85,111,367`. For generated fallback faces evaluate the effective background, including solid colour and composited art; show an explicit unsupported-frontend state when authored meaning is absent. | One colour-only light page, one dark page, translucent parent and opaque sibling overlay; same fallback face readable in each. Conflux fallback remains readable while still diagnosed unavailable. |
| R3 / P0 / M | `Resources::of` indexes shallow loose folders and NKR only, walks up into sibling libraries, and `read` erases errors. `resources.rs:40,43,51,77`; a bounded NKR/NICNT reader already exists separately. Route through it with an explicit library boundary, relative-path normalization, recursive local indexing, unambiguous precedence and structured resolution/decode diagnostics. | Synthetic loose/NKR/NICNT equivalents, mixed case/separators, nested subfolders, duplicate names, absent/corrupt members and a neighbouring library with the same basename. No foreign asset accepted. Actual missing members remain missing. |
| R4 / P0 / M | Custom font IDs start at 0 (`eval.rs:1049`) and collide with factory IDs. `ui.rs:174` creates `Font::Stock` for every ID; font names never become assets; `pictures.rs:19` rejects `BitmapFont`. Reserve the correct source ID space, map IDs to fonts, prepare glyph atlas/advances off audio and render source glyphs. | Stock ID 0 and the first two distinct custom fonts retain distinct identity; duplicate `get_font_id` stable; synthetic 256-marker font renders `Ai`, CP1252, fallback glyph and multiline spacing at 1×/2×. Missing/malformed font reported; Vector deliberately releases bitmap fonts. |
| R5 / P1 / M | Style alignment, source font family/weight, text offsets, multiline/scroll semantics and state fonts do not reach paint. `ui.rs:174,444,512`; `ir_view.rs:228,238,346`. Draw text with source metrics/alignment/state, respecting title/value/unit/background parts and typed string state. Factory fonts need a measured versioned profile; avoid claiming exact metrics from colour samples. | Left/centre/right pixel bounding boxes differ correctly; text/value offsets independent; two-line labels retain both lines and scrolling; off/on/pressed/hover state fonts with base inheritance; value edit hide-value; bitmap button with non-baked script caption. |
| R6 / P1 / M | `ImageMeta.margins` retained but every picture uses `Fit::Fill` (`ir_view.rs:79`); fixed edges distort. Apply nine-slice or the source's resize rule on stretchable axes; retain actual PNG alpha. | Synthetic four coloured corners/edges retain their pixel thickness at 2 widths and 2 heights; one-axis stretch unchanged on the other axis; fractional device scale; transparent pixels composite correctly. Native check for sidecar `Has Alpha Channel` semantics. |
| R7 / P1 / M | Image-state dispatch is incomplete: switch only 0/1; menu hard-codes frame 0; ValueEdit/meter ignore their strip; Handle never painted. `ir_view.rs:35,317,327,347`; wallpaper always frame 0 at line 181; page IR lacks wallpaper state. Preserve image role and state through source → IR → painter; state metadata is per source. | Six distinguishable switch frames across off/on×idle/pressed/hover; menu fixed state; value edit background; moving slider/XY handle; animated wallpaper changes independently of skin offset; values min/mid/max/out-of-range on horizontal and vertical strips. Confirm native midpoint rounding, not just v1 code. |
| R8 / P1 / M | Geometry collapses independent sizing into one `auto_size` flag (`ui.rs:429`, `ir_view.rs:148`), ignores declaration label/table dimensions, extends pages to lowest visible widget (`ir_view.rs:199`) and fits width only with 0.5–1 clamp (`part.rs:111`). Preserve per-axis authored/default sizes and source page/viewport rules; apply player zoom and width AND height fitting. | Only width set retains it; only height set retains it; zero-size panels/explicit zero vs default separated; declaration dimension fixtures; 970×499 page contract and deliberate overflow; zoom 0.5/1/2; fit in tall/narrow/short panes; device scales 1/1.25/1.5/2 with snapped edges/hit boxes. |
| R9 / P1 / M | Classic wallpaper never removes v1's 68 header rows and cannot select wallpaper state; very large strips are fully decoded/cut. `ir_view.rs:181`; v1 `perf_view.rs:31,458`; v1 atlas/window machinery. Apply the confirmed source/version wallpaper origin, animation state, skin offset and a bounded visible atlas window. Do not apply classic crop rules blindly to NativeUI. | Stripe-coded wallpaper with a separately coloured header, state and offset; inspect first/last visible pixel. Native Kontakt profile decides the header rule. Huge atlas does not exceed texture limits; repeated redraw retains pixels; cross-check the existing Vello image-cache patch. |
| R10 / P0–P1 / L | Tables retain `cells/range/steps_shown` but draw constant one-pixel stubs; meters return `[0.;2]`; XY/wave/wavetable/file/text are placeholders. `ir_view.rs:347–357`. Implement actual presentation from typed values and owned waveform/meter/file data; coordinate binding/runtime support with their owners. | Changing table cells changes pixels and bipolar zero line; meter signal/colours/orientation; XY cursors; waveform sample peaks, slice markers/cursor and opacity; 2D/3D wavetable with gradient/parallax; selected file path and current editable string. Attachments must be real, not dummy signals. |
| R11 / P1 / M | Widget colours are retained (`ui.rs:450`) but not consumed; alpha/gradient params often remain unsupported. `ir_view.rs:widget` uses theme roles and a constant meter; hide parts inconsistently applied. Route per-kind source colours/alpha and parts into painting without interpreting a theme substitution as Original parity. | Distinct BG/ON/OFF/BAR/PEAK/OVERLOAD/ZERO/WAVE/CURSOR/SLICE colours, opacity 0/127/255, gradient endpoints/inheritance and combined hide masks. Keep scalar colour and independent alpha separate. |
| R12 / P1 / S–M | Missing/failed art silently returns `Option::None`; `Assets::sync` remembers it; `needed_assets` retains hidden-widget art and an icon the page painter never uses. `pictures.rs:19`, `ir_view.rs:45`, `ui-ir/lib.rs:520`. Prepare bounded assets off audio/UI, report categorized failures, draw the icon in the right host location, and cache by resource identity. | Corrupt vs missing vs oversized vs unsupported-format errors distinct; image retry on changed resource generation; hidden images not eagerly decoded unless required for an impending state; Original/Vector switches preserve background and baked labels, release replaceable strips/fonts; resource/decode/decoded-pixel limits reject oversized PNG/NKR inputs cleanly. |

**Spec/layout status caution:** ancestor visibility, relative placement and stable
per-parent `(z,declaration)` ordering are implemented in `sampler-ui-ir/lib.rs:554,
569,583`. Native panel clipping, cross-parent z semantics, exact stock size/font
metrics and fractional pixel placement still need reference-host fixtures. They
are partial/unverified, not counted as known wrong solely because they differ
from another UI framework. `Widget.enabled` is currently not used by this painter.

## V1-first reference for every failing mechanism

All paths in this table refer to read-only commit
`0cb7a8a0b4d43086596a64c77320caa1b26d6d98`, worktree
`/home/derpcat/.t3/worktrees/KONTAKTO/gpt-kontakt-ui-v1-bench`.
Use its executable checks and behavior as the first implementation reference.
Where the pinned path is also incomplete, preserve its working parts and name
that limit; a new frontend or native fixture closes the remaining obligation.
Do not replace the working authored presentation with a generic theme.

| Mechanism | First v1 path/check to use | What v2 must recover; remaining limit |
|---|---|---|
| R0 default / retained selection | `src/library.rs:99` defaults `ViewMode` to Original; `src/ui/perf_view.rs:268` `mode`, `:282` `code`, `:1556` selection persistence check | Preserve Original / Vectorized / Kontra as independent per-part choices; do not infer a default from unsupported metadata or reset it on UI publication. |
| R1 frontend routing | `src/ksp/performance_view.rs:18` imports exported source properties; `src/ui/perf_view.rs:443` paints the resulting authored view; `src/ksp/compile.rs:1554` explicitly diagnoses unavailable native execution | First preserve the working exported-view route. This pinned renderer does **not** execute `load_native_ui` either; adapt the separate legacy NativeUI candidate through v2 services for the readable package rather than fabricate classic artwork. The installed-v1 Conflux capture resolves the build/page discrepancy. |
| R2 effective background / contrast | `src/ui/perf_view.rs:492` computes solid-background luma, `:505` uses it when pictured layers provide none; `:863` `luma_under` walks opaque drawn layers in reverse; `:854` selects contrasting fallback ink | Restore the solid-colour fallback and actual draw-order context that v2 dropped. Keep explicit source font colour; translucent compositing beyond the pinned threshold still needs a native fixture. |
| R3 resources / lookup diagnostics | `src/resources.rs:77` source discovery and `:134` `Result<Option<bytes>,error>`; `src/artwork.rs:122` named wallpaper, `:155` accepted names/extensions, `:259` picture failure report | Recover named-resource selection and distinct absence/access/decode errors. Pinned v1 discovery also remains shallow and NKR-based; use the pushed bounded NKR/NICNT/root-boundary work for those shared gaps. |
| R4 custom fonts | `src/ksp/calls.rs:1555` reserves IDs `26 + index`; `src/artwork.rs:218` font resource key, `:230` 256 glyph markers, `:247` character mapping; `src/ui/perf_view.rs:769` bitmap glyph painter; `:1615` glyph/ID test | Restore distinct custom identity, glyph pixels, variable advances and alpha. Validate native encoding/profile rather than assert all Unicode or factory metrics are exact. |
| R5 text / fonts / offsets / hide parts | `src/ui/perf_view.rs:591` `caption_of`, `:634` responsive `font_id`, `:666` factory weight/colour approximation, `:741` aligned/multiline `words`, `:1019` caption layering; checks `:1582`, `:1655` | Recover state-font inheritance, alignment, text offset, current TextEdit string, file caption, title/value masks and separate script captions over pictures. Exact factory glyphs and missing value-offset/scroll semantics still require source fixtures. |
| R6 sliced resizing / alpha | `src/ui/perf_view.rs:821` `sliced`; `src/ui/perf_view.rs:1472` edge-preservation check; `src/artwork.rs:458` alpha-weighted resampling and `:798` fractional-edge check | Recover preserved ends and device-aware resized pieces. V1 cuts symmetric ends around the centre; it does not parse arbitrary Fixed Top/Bottom/Left/Right margins. Extend to explicit sidecar margins without regressing working v1 skins. |
| R7 picture/state dispatch | `src/artwork.rs:322` metadata parser / `:349` frame cutting; `src/ui/perf_view.rs:163` value frame, `:805` per-kind/fixed `picture_frame`; `:1320` endpoint/bipolar/reversed-range checks | Restore fixed frames for menus/value backgrounds and source-role dispatch. V1 switches also use only off/on frames; full hover/pressed strips and independent cursor handles remain an extension, not an already-proven v1 feature. |
| R8 geometry / z / scaling | `src/ui/perf_view.rs:190` both-axis `scale_to_fit`, `:202` parent-relative placement/hide, `:224` layout using each extent, `:456` device-pixel placement and `:539` declared page clip; checks `:1486`, `:1496`, `:1522` | Recover independent extents, declared viewport, both-axis fit, user zoom and consistent device snapping. Native stock defaults, panel clipping and cross-parent z laws still need confirmation; v1 global stable z sorting differs from v2's tree ordering. |
| R9 wallpaper origin / atlas window | `src/artwork.rs:183` state plus independent pixel offset; `src/ui/perf_view.rs:31` header origin, `:458` visible-window preparation; `src/ui/fitted.rs:116` cached window; `src/artwork.rs:753` atlas-state check | Recover v1 classic-header origin, state and bounded visible-window use. Keep NativeUI geometry separate and validate version-specific origin rules. |
| R10 table / service-backed widgets | `src/ui/perf_view.rs:1100` typed table values, `:1201` real bipolar bars, `:1221` waveform peaks/cursor, `:1263` zone attachment; `:606,610` current text/file caption and `:958` file picker route | Port the working table, waveform and caption behavior first. Pinned v1 meters remain silent; XY/wavetable and broader file/string interaction are not fully implemented there, so acceptance requires real services rather than a cosmetic port. |
| R11 source colours / alpha / hide | `src/ui/perf_view.rs:1113` hide-background / ARGB handling, `:1125` meter background/off and wave/cursor colours; `:597` value hide and `:1045` knob title/value masks | Restore the source colours/parts that v1 actually paints. Bar/peak/overload/zero-line/gradient and independent alpha coverage remain incomplete in the pinned path and require the parameter/native fixtures. |
| R12 preparation / diagnostics / cache | `src/ui/perf_view.rs:371` off-frame picture fetch with generation check, `:418` per-control dependency cache; `src/artwork.rs:259` categorized failures; `src/ui/fitted.rs:73,79,103,116` owner generation / resize / cut / window caches | Restore worker preparation, explicit errors and reusable render assets. Do not copy v1's cache-count ceiling as a decoded-byte budget; strengthen bounds/retry/resource identity through the shared v2 asset layer. |

The historical `7e82b152` shared scanner extension is published at
`tools/kontra-scan@01178ba443e2b409c23282509f57d35a60753c36`. Its recorded v2
digest is `d4534838916e008d32a6e0763541a8bd9285651d77f130bad18fe926d0975f5a`.
That witness used
`~/.cache/kontra-scan/bin/kontra-scan-v1` (pinned v1) and
`kontra-scan-v2` (integration baseline), with their adjacent README and frozen
Kontakt manifest. Use existing census outputs and requested extra columns;
do not build another collector. Preserve Original in both comparisons and
record the scanner digests, installed-file identity, values and render dimensions.
The matching Conflux v2 hash above comes from that published scanner.
Pinned Kontakt v1 remains `0cb7a8a0`; adapter source is `44d03cec`, and its
installed scanner digest is
`870cea2140b5c9db5361831664966848302a2f57e82fcb6c7ed1b3534545ce5e`.
The UVI sidecar is a distinct later-v1 baseline, source
`026bdbb49f29a5ad752b3470a5f6f64a20a8957d` on product
base `4bffbb18`, digest
`34a61e82ca6f09afc0eab692a1bf0b6765edbd7d063fccca94d95e699a93b9c1`.
Its section J timing extension is installed. Canonical `kontra-scan-v1-uvi`
and alias `kontra-scan-v1uvi` hashes were checked and are identical. Historical
`1c198e60` / `d565661a` is retained separately under `~/.cache/kontra-scan/frozen`.
The common driver/README update leaves verified `abf248cd` Rust binaries unchanged;
the latest pinned-v1 driver snapshot is `59c6cbbb`, with Rust adapter `44d03cec`.
The current frozen census uses product checkpoint
`9993db691a5f69d31980357694a678e358785e5e`, scanner Rust source
`tools/kontra-scan-native-9993-20261008@ef88dcb6bd5e50ff12ad8317bdb73da92fcd415f`,
and v2 binary digest
`19e2f2c76771ceeb1f5db47956d404a290e16ef61dabb6e34909176b362b6751`.
Exporter source is `7452da10bc4989a4e93f131bf9492e0e2fca1e27`, adjacent driver
digest `a979c6a56359f717e1e5cba39b403dde8a1b3d8c26e2ee06ecdd51e938936cd5`.
Pinned Kontakt v1 and the section J UVI sidecar digests above remain unchanged.
Current Native consumer/paint attempts and decoded package font-file counts are
observed separately; legacy bridge widget/geometry/strip counts do not describe
the Native scene tree. Native fallback/loading captions do not establish authored
paint and are excluded from authored first-frame timing.

### Shared schema and coverage update

The adjacent `bin/kontra_scan.py:COLUMNS` defines **72** columns; its first nine
remain stable. Published summaries are `~/.cache/kontra-scan/results/{v1,v2}.tsv`,
with per-ID evidence in `{v1,v2}/cache`. The current frozen `9993db69` snapshot has
**678/1,494 rows in each version**. The historical `7e82b152` Conflux pair above
remains tied to its recorded digests; current checkpoint counts are not substituted
as baseline failure counts. The owner continues serial 235-second shards and
will assert complete ID/signature coverage after the paired Kontakt/UVI passes.
The full 1,494-ID paired sweep is pending. This partial snapshot cannot supply
whole-corpus affected counts, regression totals or a paired Conflux rendering comparison.

| Matrix evidence | Shared column / detailed JSON | Interpretation |
|---|---|---|
| Widget declarations | `widget_kind_counts`; views' `kinds`, `widgets`, `visible` | Kind histogram includes hidden declarations; it is not a visible-kind count. |
| UI API and control-parameter source references | `ui_api_refs`; raw slots' `symbols` | `$CONTROL_PAR_*` keys share this dictionary; there is no separate `control_par_refs` TSV column. Union matching IDs across programs/slots for affected-file counts; do not sum token occurrences as files. |
| Bypassed references and slot ownership | `bypassed_ui_api_refs`, `slots_*`; metadata slots' owner/program/wire/runtime identity | Exclude bypassed uses from active reach while retaining their separate evidence. Raw and compacted runtime slots are distinct. |
| Actual execution and saved state | `compiled_script_slots`, `clean_compiled_slots`, `disabled_block_errors`, `init_callbacks_completed`, `persistence_changed_completed`, `load_fault_records`, raw/admitted saved sigils | Use phase records for completion/faults; successful admission is not successful persistence. Disabled callback errors do not establish preprocessor-inactive errors. |
| Typed and origin-aware bindings | `bound_typed`, `phantom_free_controls` | Typed text/array validation is separate from scalar bindings; phantom-free counts stay unknown when origin/readback accessors are absent. |
| Fonts, strips and slicing | `custom_font_uses`, `font_declared`, `font_success`, `picture_strips`, `picture_frames`, `picture_margins` | Preserve lookup/decode/font-service distinctions and declared versus resolved evidence. |
| Asset failures and paint | `asset_lookup_*`, `asset_decode_*`, `resource_failure_reasons`, `paint_ok`, `paint_error` | A successful paint is not authored frontend or native-host fidelity. |
| Pale or empty authored page | `page_background_rgba`, `plain_background_fraction`; renders' background method | Declared colour uses one-unit tolerance; modal colour is explicitly inferred. V1 page coverage remains unknown. |
| Load onset and first paint | `first_audio_ms`, `ui_first_frame_ms`, `cache_state` | Actual output/CPU-paint completion on one production-load clock; unknown when unobserved. Product cache condition is separate from OS or metrics caches. |

`unknown` stays unknown, never zero. Lua init/runtime faults and safe categories,
load path, sample residency/underruns and common note-plan evidence remain
separate from renderer fidelity. No authored diagnostic text, names or payloads
are needed for these matrix counts. No new collector or independent sweep is used.

The generated whitelist now covers 246 known identifiers, including 29 omitted
by the previous scanner revision. Consume the owner's
`results/v2/symbol-aggregates.tsv`: it records identifier incidence by ID and
program-owner scope, active/bypassed occurrences, NKI/NKM splits, initialized
widget incidence, saved sigils, explicit coverage and scanner digest. Its 254
rows are identifiers/sigils, not 254 measured instruments. UI regression output
is `~/.cache/kontra-scan/v1ok-v2missing.tsv`; an empty partial file does not prove
no regressions. Older cache records must not enter current-revision totals.

`sample_zone_count` and `zero_zone_reason` describe surviving load-time mappings;
reserved sample bytes are not proof of usable PCM. `fallback_note` and
`keyswitch_picked` preserve the actual audition choice. No safe key means
`plays_note=no` with a reason; fallback-note auditions and mismatched note plans
are excluded from audio parity. The superseded null-pick audio observations are
discarded for parity. Rendering witnesses above remain tied to their exact pixel
hashes and scanner revision.

Section J starts a monotonic item clock at the first production program import;
metadata prepass and worker spawn are outside it, and later multi programs share
it. `ui_first_frame_ms` ends at actual CPU paint completion before hashing/PNG
writing, concurrently with audition. `first_audio_ms` ends at the first finite,
exactly nonzero output block; it is distinct from the audible `1e-5` criterion.
Absent observations remain unknown. `load_ms` retains its previous admission
meaning and is not first sound. These are scanner timings, not native plugin
scheduling benchmarks. Frozen product caches are cold; OS page cache is
uncontrolled. A newer product cache must expose its actual state before warm/cold
comparisons can use `cache_state`. Section J's strict native saved-table parser
is connected; its safe raw/admitted sigils and actual phase records remain the
source for restoration coverage.
Source-slot decoding and saved-table integrity are independent: only actual
record/parameter failures enter `decode_failed`; an unknown saved format retains
its observed bypass/inline/link/empty source category. Incomplete raw sigil
aggregates export unknown rather than a misleading partial total.

All three adapters now expose section J. The UVI sidecar starts its clock at
production `Worker::start` after the metadata/assets prepass, includes required
pre-audition native snapshots, and observes audio concurrently with CPU paint.
Its `load_ms` retains the legacy origin, distinct from first sound. The shared
driver forces `KONTRA_UVI_STATIC_PCM_CACHE=0`; `cache_state=cold` refers to this
disabled persistent PCM cache, not the OS cache. The owner's finite-onset,
missing-onset/null and common MIDI 60/64 Clarinet smoke proofs passed; these
selected checks do not establish full UVI corpus coverage or native UI fidelity.

### Pacific Ensemble Strings: current partial witness

The two common TSVs contain the same **49** Pacific IDs. Every row was matched
to its cache using the shared driver's `signature()` and installed binary digest;
all 98 JSON records agree with their TSV UI/load/audio/binding/note fields.
This reads existing shared evidence only; no renderer or collector was run.

| Measured Pacific subset | Pinned v1 | V2 checkpoint `9993db69` |
|---|---:|---:|
| Loads admitted | 49/49 | 49/49 |
| Missing-images / original-ok | 49 / 0 | 45 / 4 |
| Audible / silent | 45 / 4 | 45 / 4 |
| Non-fallback matched note plans | 49/49 | 49/49 |
| Lookup requested / resolved | unknown | 253 / 196 |
| Decode requested / successful | unknown | 196 / 196 |
| Fixed lookup misses / decode failures / font-service failures | unknown | 57 / 0 / 0 |

All 49 IDs have equal per-ID notes and equal audio statuses between versions;
the four silent pairs do not establish a v2 sound regression. Both versions bind
every visible scalar target (18–23 per instrument); typed bindings are zero.
V2 records `source_presentation=legacy-authored` for every Pacific view, not a
Native graph. Its 33 instruments with one miss and 12 with two misses account
for all 57 unresolved image references; four other instruments have no misses.
The four `original-ok` rows are the Cluster Risers instruments for 10 Cellos,
12 Violas, 16 Violins and 8 Basses. Original-ok is render/resource admission,
not proof of native typography, gestures or complete UI fidelity.

V2 observes one strip per instrument, 73–75 declared picture frames, no margins
and no custom bitmap-font uses. All declared font uses report success. Retained
widget kinds include tables and the classic controls, but visible/declaration
projections differ from v1; do not subtract histogram totals as missing controls.
V1's detailed lookup/decode/font/strip columns remain unknown rather than zero.
The remaining v2 failures are at lookup, not image decode; physical resource
absence versus resolver namespace/path failure still needs the library's own
resource index. No alias or replacement artwork is justified by this census.

Signature-matched evidence examples:
`results/v1/cache/c0df35e8d71d5e2b91c780dfef5ff323d498b8b3e042f3744a4835f15f4dfdfd.json`,
`results/v2/cache/83ccbee49c56c44df5030dfedfaa1528880d25f9b7ad692ef5a590c788933978.json`
(lookup miss), and
`results/v2/cache/e772d0fa37d03dbe305660979a15440fc0f24a23599f86272ef78847a9a4fb70.json`
(original-ok), relative to `~/.cache/kontra-scan`. The 678-row paired snapshot and
this 49-ID subset are partial; whole-corpus matrix counts remain pending.

## Prior work to reuse, not reimplement

| Branch / commit | Relevant available work | Merge boundary |
|---|---|---|
| `v2/gpt-kontakt-ui@bd7447db`, `7591f35e`; tip `0be9ed3f` | NKR/NICNT resource routing, library-root boundary, resumable census, explicit unavailable modern Komplete frontend | Not ancestor of baseline. Resource fixes need local ancestry/path tests; they do not provide a legacy `.nui` runtime or prove Conflux's missing classic wallpaper exists. |
| `v2/gpt-kontakt-ui@cda4ce23`, `47f14b59` | Header/presentation persistence, namespaced IR widget IDs, redraw/control/recall and passive-menu fixes | Read `UI_V1_PARITY.md`; loop/widgets owners adapt to current integration. Rendering gaps above remain in that branch. |
| `codex/conflux-native-ui@4da70640` | Legacy `.nui` Lua module/session/component painter, images/fonts, hierarchy, control bridge, off-audio preparation | Older v1 composition root; port the general frontend and native services deliberately. Its report expressly lacks native/full-host compatibility proof. Do not cherry-pick its DSP work into a render audit. |
| `v2/gpt-uvi-ui@dcbc07c1`, tip `24c70a25` | Authored UVI resources/fonts, state/opacity/clipping, table/XY/picture rendering and mapper defaults | UVI-specific host semantics stay separate from Kontakt. Shared painter/resource preparation patterns may be reused; not a KSP or Conflux fix. |
| V1 `0cb7a8a0` | `src/artwork.rs:228` font glyph/advance preparation; `perf_view.rs:634` state font selection; `:769` bitmap words; `:821` slicing; `:190` both-axis fit; `:458` wallpaper origin | Behavior/reference code, not evidence that every stock metric is Kontakt-exact. V1 also lacked several widgets and used off/on switch frames only. |
| RE worktree `t3code-80fe786b` | `UI_NATIVE_PRESERVATION.md`: X11 reparent/visibility and shared accessibility/IME bridge; `FALCON_RUNTIME_UI_GROUNDWORK.md`: source/runtime distinctions | Host visibility success does not establish KSP artwork or pixels. No native window/DAW/macOS claim is made by this CPU audit. |

## Acceptance gates and unknowns

1. **Authored frontend fidelity:** record the owner's installed Kontakt pages at
   the same dimensions/values; identify legacy `.nui` vs modern Komplete version.
   Compare structure, clipping, labels, fonts and every visual state. CPU fallback
   screenshots cannot certify these.
2. **Asset attribution:** for each production failed lookup enumerate only that
   library's actual container/loose index, preserving absence vs access/corrupt/
   extension/nesting/namespace failure. This census's `present=false` means the
   current resolver returned no bytes, not necessarily physical absence. Conflux
   is independently checked against its own NKR/NICNT names. Count actual unlocks
   only after differential rendering with a systemic resolver change.
3. **Font fidelity:** capture the 26 stock fonts in native Kontakt, including
   weights/advances/baselines and responsive state inheritance. Custom glyph atlas
   metadata and Unicode mapping need explicit reference checks beyond v1 parity.
4. **Animation and source laws:** verify midpoint frame rounding, strip/button
   state ordering and wallpaper header/origin per version. A seek-safe value/frame
   function and synthetic atlas verify our consistency; they do not establish the
   vendor's exact law without a native capture.
5. **Host/device path:** compare CPU and actual GPU readback at several device
   scales; exercise partial redraw, scroll, resize, hide/show/reparent/reopen.
   The Vello image-retention patch and RE host tests are valuable, separate gates.
6. **Complete reachable-state census:** run actual page/visibility callbacks,
   listener updates, persistence changes, UI values and native component states.
   Keep semantic state independent of presentation and avoid republishing/copying
   an entire interface for one control. The loop owner defines that architecture.

## Other scopes

- **Widgets:** Conflux's actual main fallback knobs are vertical-path knobs,
  while KSP sliders default horizontal and `wd.drag` is ignored. Missing native
  gesture routing, hit overlap, fine/reset, wheel arbitration and stale/global
  `ir-N` identities need physical-input traces. No renderer-only drag success is
  claimed as a musical edit.
- **Params:** complete typed get/set/array semantics, ID lookup, persistence,
  `.nckp` control numbering and callback ordering belong to that owner. The symbol
  matrix below evaluates the rendering consumer and flags cross-scope meanings.
- **Loop:** stale `Arc` identity recreates `Face`, discarding presentation/page
  and asset/value state (`part.rs:55`). Font, asset and geometry changes need an
  immutable, versioned publication strategy; queue admission/callback latency
  and CPU warm-frame work belong to the loop report.
- **Census:** share this initial-state metrics set and Conflux colour criterion.
  A white-pixel threshold alone misses the pale blank (0% pure white; 94.94%
  cream). An image alone cannot classify missing native frontend or unbound UI.

## Reproduction and verification

```sh
cd /home/derpcat/.t3/worktrees/KONTAKTO/audit-ui-render
~/.cache/kontakto-heavy cargo test --profile ci --no-run
~/.cache/kontakto-heavy cargo test --profile ci --lib ui::render_audit::baseline_rendering_omissions_are_observable -- --exact
# Single witness only. Whole-corpus collection belongs to the shared scanner.
KONTRA_RENDER_PATH='/mnt/MAIN_STORAGE/Libraries/Kontakt/Conflux 1.1.0 [Native Instruments]/Instruments/Conflux.nki' \
KONTRA_RENDER_OUT=$HOME/.cache/kontakto-audit-ui-render/new-witness KONTRA_RENDER_SHOTS=1 \
  ~/.cache/kontakto-heavy timeout 235s cargo test --profile ci --lib \
  ui::render_audit::installed_render_witness -- --exact --ignored --nocapture
```

Validation before this push: `kontakto-heavy cargo test --profile ci --no-run`
passed for the root library and integration-test executables. Both synthetic
characterization/token checks and the ignored installed Conflux witness passed
through the heavy wrapper. Existing compiler warnings were left unchanged.
`git diff --check` passed. No product rendering behavior was changed.

The characterization test deliberately asserts observed baseline omissions:
custom font becomes Stock(0) with no asset; alignment/text offset changes leave
pixels identical; table value changes leave pixels identical; wallpaper draws
frame zero from its top. These are **passing defect witnesses**, not compatibility
tests. Reverse/replace them with the acceptance criteria when implementing fixes.


## Exhaustive rendering-consumer matrices (implementation handoff)

All 16 widgets, all 92 entries in the pinned PDF symbol index and all 38 commands
in its UI-command chapter are listed. Additional typed variants, frontend, keyboard
and callback surfaces follow. `correct` means the stated initial rendering
consumer exists; it does not certify the runtime/input/host contract. `partial`
includes explicitly unverified native semantics. Four PDF prefix fragments are
kept visible rather than presented as invented APIs.

**Counts:** `Refs/834` and `Census` are **pending** for source references,
whole-corpus active widgets/assets, affected rendering and fixes that unlock
authored interfaces. The shared columns now exist; complete paired coverage is
pending as described above. Source references must be unioned across programs and scripts per file;
Creator Tools can supply widgets/properties without a script token. Do not
substitute reference counts for unlocked files.

Evidence shorthand: **K** = `crates/sampler-ksp/src/ui.rs`; **E** =
`crates/sampler-ksp/src/eval.rs`; **P** = `src/ui/ir_view.rs`; **I** =
`crates/sampler-ui-ir/src/lib.rs`. Mechanism IDs refer to the fixes/tests above.
Expected obligations follow the pinned `KSP_SURFACE.json` control-parameter,
UI-command and widget sections and `KSP_SYMBOLS.md`; source/version details
marked uncertain require Kontakt captures. This is a renderer matrix; the params
owner supplies exact accessor/type/callback matrices.

### Widgets

| Widget | Expected pixels | V2 status / evidence | Systemic fix/test | Refs/834 | Census |
|---|---|---|---|---:|---|
| `ui_button` | Button states, picture and script caption. | **partial**; P:283 | R5/R7 | pending | pending |
| `ui_file_selector` | Filtered file list, selected path and column sizing. | **missing**; P:356: placeholder | R10; file service owner | pending | pending |
| `ui_knob` | Bound scalar, authored strip/name/value/unit. | **partial**; P:242–281 | R4–R8/R11 | pending | pending |
| `ui_label` | Multiline/scrolling source text, alignment, font and background. | **partial**; P:346; K:479 | R4–R6 | pending | pending |
| `ui_level_meter` | Attached signal level/peak, orientation and source colours. | **wrong**; P:347: constant zero | R10/R11 | pending | pending |
| `ui_menu` | Selected visible text; authored background and popup items. | **partial**; P:303 | R5–R7; popup input owner | pending | pending |
| `ui_mouse_area` | Transparent authored hit region and optional picture. | **partial**; P:359; K:508 | widgets owns events; R6/R8 | pending | pending |
| `ui_panel` | Parent-relative layout, background and inherited visibility. | **partial**; K:530; I:554,569,583; P:359 | R6/R8; clipping/z native test | pending | pending |
| `ui_slider` | Scalar position with source strip/handle and geometry. | **partial**; P:242–281 | R6–R8; input owner | pending | pending |
| `ui_switch` | On/off with pressed/hover state art/fonts. | **partial**; P:295 | R5/R7 | pending | pending |
| `ui_table` | Real column values/range/zero line and shown steps. | **wrong**; P:348; passing cell-pixel witness | R10/R11 | pending | pending |
| `ui_text_edit` | Current editable string, source selection/caret/layout. | **missing**; P:356: caption placeholder | R5/R10; string/input owner | pending | pending |
| `ui_value_edit` | Editable numeric display/unit, title, arrows and picture. | **partial**; P:327 | R5/R7/R10 | pending | pending |
| `ui_waveform` | Attached source peaks/slices/selection/cursor and colour. | **missing**; P:356: placeholder | R10/R11 | pending | pending |
| `ui_wavetable` | Attached zone table with 2D/3D/gradient/parallax. | **missing**; P:356: placeholder | R10/R11 | pending | pending |
| `ui_xy` | Typed cursor coordinates with independent cursor art. | **missing**; P:356: placeholder | R7/R10 | pending | pending |

### Control parameters

| Parameter | Expected rendering or service obligation | V2 status / evidence | Fix/test | Refs/834 | Census |
|---|---|---|---|---:|---|
| `$CONTROL_PAR_` | PDF index prefix, not a callable parameter. | **partial**; KSP_SYMBOLS.md:3 | Clean index; no guessed API | pending | pending |
| `$CONTROL_PAR_ACTIVE_INDEX` | Selected/active XY cursor index. | **missing**; K:8,537; P:356 | R10 + widgets | pending | pending |
| `$CONTROL_PAR_ALLOW_AUTOMATION` | Automation eligibility and source host parameter mapping. | **partial**; K:501 | params/host; render is not automation proof | pending | pending |
| `$CONTROL_PAR_AUTOMATION_ID` | Automation eligibility and source host parameter mapping. | **partial**; K:501 | params/host; render is not automation proof | pending | pending |
| `$CONTROL_PAR_AUTOMATION_NAME` | Full/short automation display labels. | **partial**; K:499; P:373 uses full a11y name | params/host; source short-label consumer pending | pending | pending |
| `$CONTROL_PAR_BAR_COLOR` | Independent per-kind 0xRRGGBB source colour, with supported opacity rules. | **wrong**; K:54,450; P:347–357 | R11 | pending | pending |
| `$CONTROL_PAR_BASEPATH` | File selector base/selected path, filter and pixel column width. | **missing**; K:408–416; P:356 | R10; file service owner | pending | pending |
| `$CONTROL_PAR_BG_ALPHA` | Independent source alpha 0–255, per-kind composition. | **missing**; K:8,537; P:356 | R11 | pending | pending |
| `$CONTROL_PAR_BG_COLOR` | Independent per-kind 0xRRGGBB source colour, with supported opacity rules. | **wrong**; K:54,450; P:347–357 | R11 | pending | pending |
| `$CONTROL_PAR_COLUMN_WIDTH` | File selector base/selected path, filter and pixel column width. | **missing**; K:408–416; P:356 | R10; file service owner | pending | pending |
| `$CONTROL_PAR_CURSOR_PICTURE` | Cursor/handle art; indexed cursor state where supported. | **missing**; K:525,549; P:widget ignores Handle | R7/R10 | pending | pending |
| `$CONTROL_PAR_CUSTOM_ID` | Authored control identity distinct from runtime/dense/UI IDs. | **partial**; K:8,537 | R1 + params; full identity mapping | pending | pending |
| `$CONTROL_PAR_DEFAULT_VALUE` | Integer scalar bounds/default; displayed value/frame normalized to range. | **partial**; K:307; P:22,223,247 | R7; widgets/params prove source laws | pending | pending |
| `$CONTROL_PAR_DISABLE_TEXT_SHIFTING` | Control pressed-state text shift policy. | **missing**; K:8,537; P:288 | R5/R7 | pending | pending |
| `$CONTROL_PAR_DND_ACCEPT_ARRAY` | Source drag/drop accepted types, event routing and export-region index. | **missing**; K:8,537; P:356 | widgets/host; visual drag affordances | pending | pending |
| `$CONTROL_PAR_DND_ACCEPT_AUDIO` | Source drag/drop accepted types, event routing and export-region index. | **missing**; K:8,537; P:356 | widgets/host; visual drag affordances | pending | pending |
| `$CONTROL_PAR_DND_ACCEPT_MIDI` | Source drag/drop accepted types, event routing and export-region index. | **missing**; K:8,537; P:356 | widgets/host; visual drag affordances | pending | pending |
| `$CONTROL_PAR_DND_BEHAVIOUR` | Source drag/drop accepted types, event routing and export-region index. | **missing**; K:8,537; P:356 | widgets/host; visual drag affordances | pending | pending |
| `$CONTROL_PAR_FILEPATH` | File selector base/selected path, filter and pixel column width. | **missing**; K:408–416; P:356 | R10; file service owner | pending | pending |
| `$CONTROL_PAR_FILE_TYPE` | File selector base/selected path, filter and pixel column width. | **missing**; K:408–416; P:356 | R10; file service owner | pending | pending |
| `$CONTROL_PAR_FONT_TYPE` | Factory/custom font identity, metrics, glyphs and colour. | **wrong**; E:1049; K:174; P:228 | R4/R5; passing font-ID witness | pending | pending |
| `$CONTROL_PAR_FONT_TYPE_OFF_HOVER` | Font per on/off × hover/pressed state; inherit base where specified. | **missing**; K:8,537; P:228 | R5; native inheritance fixture | pending | pending |
| `$CONTROL_PAR_FONT_TYPE_OFF_PRESSED` | Font per on/off × hover/pressed state; inherit base where specified. | **missing**; K:8,537; P:228 | R5; native inheritance fixture | pending | pending |
| `$CONTROL_PAR_FONT_TYPE_ON` | Font per on/off × hover/pressed state; inherit base where specified. | **missing**; K:8,537; P:228 | R5; native inheritance fixture | pending | pending |
| `$CONTROL_PAR_FONT_TYPE_ON_HOVER` | Font per on/off × hover/pressed state; inherit base where specified. | **missing**; K:8,537; P:228 | R5; native inheritance fixture | pending | pending |
| `$CONTROL_PAR_FONT_TYPE_ON_PRESSED` | Font per on/off × hover/pressed state; inherit base where specified. | **missing**; K:8,537; P:228 | R5; native inheritance fixture | pending | pending |
| `$CONTROL_PAR_GRID_HEIGHT` | Source grid coordinate/extent; verify accepted per-kind units. | **missing**; K:8 (only synthetic grid_x/grid_y mapped) | R8 + params | pending | pending |
| `$CONTROL_PAR_GRID_WIDTH` | Source grid coordinate/extent; verify accepted per-kind units. | **missing**; K:8 (only synthetic grid_x/grid_y mapped) | R8 + params | pending | pending |
| `$CONTROL_PAR_GRID_X` | Source grid coordinate/extent; verify accepted per-kind units. | **missing**; K:8 (only synthetic grid_x/grid_y mapped) | R8 + params | pending | pending |
| `$CONTROL_PAR_GRID_Y` | Source grid coordinate/extent; verify accepted per-kind units. | **missing**; K:8 (only synthetic grid_x/grid_y mapped) | R8 + params | pending | pending |
| `$CONTROL_PAR_HEIGHT` | Independent integer pixel extent; preserve authored/default axis. | **wrong**; K:429; P:148 | R8 | pending | pending |
| `$CONTROL_PAR_HELP` | Tooltip string on the control. | **correct**; K:497; P:374 | Retain; params validates runtime update | pending | pending |
| `$CONTROL_PAR_HIDE` | Source hide bitmask: whole, background, title, value, modulation light. | **partial**; K:433,457,537; P:268,334,377 | R5/R11; ValueEdit hide-value ignored | pending | pending |
| `$CONTROL_PAR_IDENTIFIER` | Authored control identity distinct from runtime/dense/UI IDs. | **partial**; K:8,537 | R1 + params; full identity mapping | pending | pending |
| `$CONTROL_PAR_KEY` | PDF-extracted prefix/fragment; not independently specified in the pinned chapter. | **partial**; KSP_SYMBOLS.md:3; KSP_SURFACE.json | Verify real symbol; do not fabricate semantics | pending | pending |
| `$CONTROL_PAR_KEY_ALT` | Modifier state for the source input/callback contract. | **partial**; K:8,537 | widgets/params; no scalar paint equivalent | pending | pending |
| `$CONTROL_PAR_KEY_CONTROL` | Modifier state for the source input/callback contract. | **partial**; K:8,537 | widgets/params; no scalar paint equivalent | pending | pending |
| `$CONTROL_PAR_KEY_SHIFT` | Modifier state for the source input/callback contract. | **partial**; K:8,537 | widgets/params; no scalar paint equivalent | pending | pending |
| `$CONTROL_PAR_LABEL` | Explicit value-display string; independent from title. | **partial**; K:447; P:260,337 | R5; empty override/state test | pending | pending |
| `$CONTROL_PAR_MAX_VALUE` | Integer scalar bounds/default; displayed value/frame normalized to range. | **partial**; K:307; P:22,223,247 | R7; widgets/params prove source laws | pending | pending |
| `$CONTROL_PAR_MIDI_EXPORT_AREA_IDX` | Source drag/drop accepted types, event routing and export-region index. | **missing**; K:8,537; P:356 | widgets/host; visual drag affordances | pending | pending |
| `$CONTROL_PAR_MIN_VALUE` | Integer scalar bounds/default; displayed value/frame normalized to range. | **partial**; K:307; P:22,223,247 | R7; widgets/params prove source laws | pending | pending |
| `$CONTROL_PAR_MOUSE_BEHAVIOUR` | Source drag axis/sensitivity or XY behavior; visual cursor follows typed values. | **partial**; K:388,448; P:247,356 | widgets + R10; renderer ignores drag metadata | pending | pending |
| `$CONTROL_PAR_MOUSE_BEHAVIOUR_X` | Source drag axis/sensitivity or XY behavior; visual cursor follows typed values. | **partial**; K:388,448; P:247,356 | widgets + R10; renderer ignores drag metadata | pending | pending |
| `$CONTROL_PAR_MOUSE_BEHAVIOUR_Y` | Source drag axis/sensitivity or XY behavior; visual cursor follows typed values. | **partial**; K:388,448; P:247,356 | widgets + R10; renderer ignores drag metadata | pending | pending |
| `$CONTROL_PAR_MOUSE_MODE` | Source drag axis/sensitivity or XY behavior; visual cursor follows typed values. | **partial**; K:388,448; P:247,356 | widgets + R10; renderer ignores drag metadata | pending | pending |
| `$CONTROL_PAR_NONE` | Read-only/source sentinel control metadata. | **partial**; K:8,537 | params; no independent paint obligation | pending | pending |
| `$CONTROL_PAR_NUM_ITEMS` | Menu count and selected visible item index semantics. | **partial**; K:339; P:303 | widgets/params; source getter fixture | pending | pending |
| `$CONTROL_PAR_OFF_COLOR` | Independent per-kind 0xRRGGBB source colour, with supported opacity rules. | **wrong**; K:54,450; P:347–357 | R11 | pending | pending |
| `$CONTROL_PAR_ON_COLOR` | Independent per-kind 0xRRGGBB source colour, with supported opacity rules. | **wrong**; K:54,450; P:347–357 | R11 | pending | pending |
| `$CONTROL_PAR_OVERLOAD_COLOR` | Independent per-kind 0xRRGGBB source colour, with supported opacity rules. | **wrong**; K:54,450; P:347–357 | R11 | pending | pending |
| `$CONTROL_PAR_PARALLAX_X` | Wave/wavetable visualization mode, zone attachment and source parallax. | **missing**; K:394; WT_ZONE unsupported; P:356 | R10 | pending | pending |
| `$CONTROL_PAR_PARALLAX_Y` | Wave/wavetable visualization mode, zone attachment and source parallax. | **missing**; K:394; WT_ZONE unsupported; P:356 | R10 | pending | pending |
| `$CONTROL_PAR_PARENT_PANEL` | Parent UI ID; relative geometry and inherited visibility. | **partial**; K:530; I:554,569 | R8; native clipping/z tests | pending | pending |
| `$CONTROL_PAR_PEAK_COLOR` | Independent per-kind 0xRRGGBB source colour, with supported opacity rules. | **wrong**; K:54,450; P:347–357 | R11 | pending | pending |
| `$CONTROL_PAR_PICTURE` | Picture basename/path with resource metadata and per-kind image role. | **partial**; K:82,508; pictures.rs:19; P:79 | R3/R6/R7/R12 | pending | pending |
| `$CONTROL_PAR_PICTURE_STATE` | Explicit image-frame selection; wallpaper/control source rules differ. | **partial**; K:263,522; P:181,216,317 | R7/R9; menu/wallpaper omitted | pending | pending |
| `$CONTROL_PAR_POS_X` | Integer source-pixel coordinate, relative to parent panel. | **partial**; K:423; I:569; P:187 | R8; native fractional/origin tests | pending | pending |
| `$CONTROL_PAR_POS_Y` | Integer source-pixel coordinate, relative to parent panel. | **partial**; K:423; I:569; P:187 | R8; native fractional/origin tests | pending | pending |
| `$CONTROL_PAR_RANGE_MAX` | Per-kind display/range bounds; accepted kinds and units need native fixture. | **missing**; K:8,537 | R10 + params | pending | pending |
| `$CONTROL_PAR_RANGE_MIN` | Per-kind display/range bounds; accepted kinds and units need native fixture. | **missing**; K:8,537 | R10 + params | pending | pending |
| `$CONTROL_PAR_RECEIVE_DRAG_EVENTS` | Source drag/drop accepted types, event routing and export-region index. | **missing**; K:8,537; P:356 | widgets/host; visual drag affordances | pending | pending |
| `$CONTROL_PAR_SELECTED_ITEM_IDX` | Menu count and selected visible item index semantics. | **partial**; K:339; P:303 | widgets/params; source getter fixture | pending | pending |
| `$CONTROL_PAR_SHORT_NAME` | Full/short automation display labels. | **partial**; K:499; P:373 uses full a11y name | params/host; source short-label consumer pending | pending | pending |
| `$CONTROL_PAR_SHOW_ARROWS` | Value-edit increment/decrement affordances. | **missing**; K:353; P:327 ignores arrows | R10 + widgets | pending | pending |
| `$CONTROL_PAR_SLICEMARKERS_COLOR` | Independent per-kind 0xRRGGBB source colour, with supported opacity rules. | **wrong**; K:54,450; P:347–357 | R11 | pending | pending |
| `$CONTROL_PAR_TEXT` | Scalar/indexed strings; caption or multiline label content. | **partial**; K:479; P:238,346,356 | R5/R10 | pending | pending |
| `$CONTROL_PAR_TEXTLINE` | Scalar/indexed strings; caption or multiline label content. | **partial**; K:479; P:238,346,356 | R5/R10 | pending | pending |
| `$CONTROL_PAR_TEXTPOS_Y` | Separate title/value vertical offsets in source pixels. | **wrong**; K:446; VALUEPOS_Y not mapped; P:widget | R5 | pending | pending |
| `$CONTROL_PAR_TEXT_ALIGNMENT` | Left/centre/right source text alignment. | **wrong**; K:505; P:238,346 | R5; passing pixel witness | pending | pending |
| `$CONTROL_PAR_TYPE` | Read-only/source sentinel control metadata. | **partial**; K:8,537 | params; no independent paint obligation | pending | pending |
| `$CONTROL_PAR_UNIT` | Display unit NONE/DB/HZ/PERCENT/MS/OCT/ST, source ratio formatting. | **partial**; K:319; P:239 | R5; native numeric formatting test | pending | pending |
| `$CONTROL_PAR_VALUE` | Typed scalar or indexed table/XY data; pixels reflect current state. | **partial**; K:364,461; P:228,347,356 | R10; scalar works, arrays ignored | pending | pending |
| `$CONTROL_PAR_VALUEPOS_Y` | Separate title/value vertical offsets in source pixels. | **wrong**; K:446; VALUEPOS_Y not mapped; P:widget | R5 | pending | pending |
| `$CONTROL_PAR_VERTICAL` | Meter/appropriate widget orientation. | **wrong**; K:401; P:347 always meter_v | R10 | pending | pending |
| `$CONTROL_PAR_WAVETABLE` | PDF-extracted prefix/fragment; not independently specified in the pinned chapter. | **partial**; KSP_SYMBOLS.md:3; KSP_SURFACE.json | Verify real symbol; do not fabricate semantics | pending | pending |
| `$CONTROL_PAR_WAVETABLE_ALPHA` | Independent source alpha 0–255, per-kind composition. | **missing**; K:8,537; P:356 | R11 | pending | pending |
| `$CONTROL_PAR_WAVETABLE_COLOR` | Source waveform/wavetable colour or gradient endpoint. | **missing**; K:8,537; P:356 | R10/R11 | pending | pending |
| `$CONTROL_PAR_WAVETABLE_END_ALPHA` | Independent source alpha 0–255, per-kind composition. | **missing**; K:8,537; P:356 | R11 | pending | pending |
| `$CONTROL_PAR_WAVETABLE_END_COLOR` | Source waveform/wavetable colour or gradient endpoint. | **missing**; K:8,537; P:356 | R10/R11 | pending | pending |
| `$CONTROL_PAR_WAVE_ALPHA` | Independent source alpha 0–255, per-kind composition. | **missing**; K:8,537; P:356 | R11 | pending | pending |
| `$CONTROL_PAR_WAVE_COLOR` | Independent per-kind 0xRRGGBB source colour, with supported opacity rules. | **wrong**; K:54,450; P:347–357 | R11 | pending | pending |
| `$CONTROL_PAR_WAVE_CURSOR_COLOR` | Independent per-kind 0xRRGGBB source colour, with supported opacity rules. | **wrong**; K:54,450; P:347–357 | R11 | pending | pending |
| `$CONTROL_PAR_WAVE_END_` | PDF-extracted prefix/fragment; not independently specified in the pinned chapter. | **partial**; KSP_SYMBOLS.md:3; KSP_SURFACE.json | Verify real symbol; do not fabricate semantics | pending | pending |
| `$CONTROL_PAR_WAVE_END_ALPHA` | Independent source alpha 0–255, per-kind composition. | **missing**; K:8,537; P:356 | R11 | pending | pending |
| `$CONTROL_PAR_WAVE_END_COLOR` | Source waveform/wavetable colour or gradient endpoint. | **missing**; K:8,537; P:356 | R10/R11 | pending | pending |
| `$CONTROL_PAR_WF_VIS_MODE` | Wave/wavetable visualization mode, zone attachment and source parallax. | **missing**; K:394; WT_ZONE unsupported; P:356 | R10 | pending | pending |
| `$CONTROL_PAR_WIDTH` | Independent integer pixel extent; preserve authored/default axis. | **wrong**; K:429; P:148 | R8 | pending | pending |
| `$CONTROL_PAR_WT_VIS_MODE` | Wave/wavetable visualization mode, zone attachment and source parallax. | **missing**; K:394; WT_ZONE unsupported; P:356 | R10 | pending | pending |
| `$CONTROL_PAR_WT_ZONE` | Wave/wavetable visualization mode, zone attachment and source parallax. | **missing**; K:394; WT_ZONE unsupported; P:356 | R10 | pending | pending |
| `$CONTROL_PAR_ZERO_LINE_COLOR` | Independent per-kind 0xRRGGBB source colour, with supported opacity rules. | **wrong**; K:54,450; P:347–357 | R11 | pending | pending |
| `$CONTROL_PAR_Z_LAYER` | Integer source z layer and declared-order tie break. | **partial**; K:432; I:583 | R8; native cross-parent z test | pending | pending |

### UI builtins and adjacent display services

| Builtin / callback | Expected result | V2 status / evidence | Fix/test | Refs/834 | Census |
|---|---|---|---|---:|---|
| `add_menu_item` | Preserve item text/value/visibility in menu and selected display. | **partial**; E:973–1016; P:303 | R5; widgets/loop prove dynamic publication | pending | pending |
| `add_text_line` | Replace/append source caption, multiline text or value label. | **partial**; E:929; K:479; P:238,346 | R5/R10 | pending | pending |
| `attach_level_meter` | Bind meter to requested engine/output signal. | **missing**; E:1259; K:meter; P:347 | R10 + core signal owner | pending | pending |
| `attach_zone` | Attach waveform zone and read/write display/cursor/slice properties. | **missing**; E:1253,1259; P:356 | R10 + waveform service owner | pending | pending |
| `expose_controls` | Expose source controls to the native authored frontend. | **missing**; builtins.rs:225; no NativeUI consumer | R1 + params identity bridge | pending | pending |
| `fs_get_filename` | Return/navigate selected source file path. | **missing**; E:1250,1260; P:356 | R10 + file service owner | pending | pending |
| `fs_navigate` | Return/navigate selected source file path. | **missing**; E:1250,1260; P:356 | R10 + file service owner | pending | pending |
| `get_control_par` | Get the same typed property/default/indexed state set by source. | **partial**; E:913; lower.rs:1988,2097 | params; getter itself has no paint equivalent | pending | pending |
| `get_control_par_arr` | Get the same typed property/default/indexed state set by source. | **partial**; E:913; lower.rs:1988,2097 | params; getter itself has no paint equivalent | pending | pending |
| `get_font_id` | Load/deduplicate custom font and return a distinct source font ID. | **wrong**; E:1049; K:174; passing font witness | R4 | pending | pending |
| `get_menu_item_str` | Return source menu metadata without altering the display. | **partial**; E:1000; lower.rs:2097 | params/loop; callback-time getters differ from init | pending | pending |
| `get_menu_item_value` | Return source menu metadata without altering the display. | **partial**; E:1000; lower.rs:2097 | params/loop; callback-time getters differ from init | pending | pending |
| `get_menu_item_visibility` | Return source menu metadata without altering the display. | **partial**; E:1000; lower.rs:2097 | params/loop; callback-time getters differ from init | pending | pending |
| `get_ui_id` | Stable source UI ID distinct from semantic control/dense index. | **partial**; E:892; lower.rs:1978; K:431 | params/loop; namespaced editor identity | pending | pending |
| `get_ui_wf_property` | Attach waveform zone and read/write display/cursor/slice properties. | **missing**; E:1253,1259; P:356 | R10 + waveform service owner | pending | pending |
| `hide_part` | Combine source whole/part hide flags per widget. | **partial**; E:949; K:433,457; P:334 | R5/R11 | pending | pending |
| `load_performance_view` | Load/merge Creator Tools .nckp geometry/control/resource references. | **partial**; load.rs:581; E:1040; nckp.rs:44 | R3–R8; params validates IDs/typing | pending | pending |
| `make_perfview` | Declare the script performance view. | **partial**; E:1045; K:page; part.rs:46 | R1/R8; source view admission fixture | pending | pending |
| `move_control` | Grid or pixel placement; grid (0,0) hides. | **partial**; E:961; K:434; P:116,142 | R8; grid units native fixture | pending | pending |
| `move_control_px` | Grid or pixel placement; grid (0,0) hides. | **partial**; E:961; K:434; P:116,142 | R8; grid units native fixture | pending | pending |
| `set_control_help` | Preserve control tooltip string. | **partial**; E:929; P:374 | Retain initial paint; loop tests later update | pending | pending |
| `set_control_par` | Typed scalar/indexed presentation/value properties. | **partial**; E:893; K:537; lower.rs:1985 | Per-parameter table + params/loop | pending | pending |
| `set_control_par_arr` | Typed scalar/indexed presentation/value properties. | **partial**; E:893; K:537; lower.rs:1985 | Per-parameter table + params/loop | pending | pending |
| `set_knob_defval` | Source display unit/default and reset/format law. | **partial**; E:949; K:307,319; P:239 | R5/R7; widgets reset | pending | pending |
| `set_knob_label` | Replace/append source caption, multiline text or value label. | **partial**; E:929; K:479; P:238,346 | R5/R10 | pending | pending |
| `set_knob_unit` | Source display unit/default and reset/format law. | **partial**; E:949; K:307,319; P:239 | R5/R7; widgets reset | pending | pending |
| `set_menu_item_str` | Preserve item text/value/visibility in menu and selected display. | **partial**; E:973–1016; P:303 | R5; widgets/loop prove dynamic publication | pending | pending |
| `set_menu_item_value` | Preserve item text/value/visibility in menu and selected display. | **partial**; E:973–1016; P:303 | R5; widgets/loop prove dynamic publication | pending | pending |
| `set_menu_item_visibility` | Preserve item text/value/visibility in menu and selected display. | **partial**; E:973–1016; P:303 | R5; widgets/loop prove dynamic publication | pending | pending |
| `set_script_title` | Source script/performance view title. | **partial**; E:1035; K:288; part.rs:72 | loop; editor tabs use Script n | pending | pending |
| `set_skin_offset` | Classic wallpaper vertical offset in source pixels. | **partial**; E:1019; K:247; P:181 | R9; header/profile origin fixture | pending | pending |
| `set_table_steps_shown` | Number of table columns displayed; valid source range. | **wrong**; E:949; K:378; P:348 | R10 | pending | pending |
| `set_text` | Replace/append source caption, multiline text or value label. | **partial**; E:929; K:479; P:238,346 | R5/R10 | pending | pending |
| `set_ui_color` | Source solid page background colour. | **partial**; E:1125; K:256; P:178,111 | R2; painted but contrast sampling ignores it | pending | pending |
| `set_ui_height` | Declared view height grid rows/pixels and width pixels. | **partial**; E:1023; K:282; P:136,199; part.rs:111 | R8 | pending | pending |
| `set_ui_height_px` | Declared view height grid rows/pixels and width pixels. | **partial**; E:1023; K:282; P:136,199; part.rs:111 | R8 | pending | pending |
| `set_ui_wf_property` | Attach waveform zone and read/write display/cursor/slice properties. | **missing**; E:1253,1259; P:356 | R10 + waveform service owner | pending | pending |
| `set_ui_width_px` | Declared view height grid rows/pixels and width pixels. | **partial**; E:1023; K:282; P:136,199; part.rs:111 | R8 | pending | pending |
| `get_control_par_real` | Get the same typed property/default/indexed state set by source. | **partial**; E:913; lower.rs:1988,2097 | params; getter itself has no paint equivalent | pending | pending |
| `get_control_par_real_arr` | Get the same typed property/default/indexed state set by source. | **partial**; E:913; lower.rs:1988,2097 | params; getter itself has no paint equivalent | pending | pending |
| `get_control_par_str` | Get the same typed property/default/indexed state set by source. | **partial**; E:913; lower.rs:1988,2097 | params; getter itself has no paint equivalent | pending | pending |
| `get_control_par_str_arr` | Get the same typed property/default/indexed state set by source. | **partial**; E:913; lower.rs:1988,2097 | params; getter itself has no paint equivalent | pending | pending |
| `get_key_color` | Read source keyboard display/event state. | **partial**; E:1077; lower.rs runtime fallback | params/widgets; no independent paint | pending | pending |
| `get_key_name` | Read source keyboard display/event state. | **partial**; E:1077; lower.rs runtime fallback | params/widgets; no independent paint | pending | pending |
| `get_key_triggerstate` | Read source keyboard display/event state. | **partial**; E:1077; lower.rs runtime fallback | params/widgets; no independent paint | pending | pending |
| `get_key_type` | Read source keyboard display/event state. | **partial**; E:1077; lower.rs runtime fallback | params/widgets; no independent paint | pending | pending |
| `get_num_menu_items` | Return source menu metadata without altering the display. | **partial**; E:1000; lower.rs:2097 | params/loop; callback-time getters differ from init | pending | pending |
| `listener` | Source callbacks update values and presentation at their defined event boundary. | **partial**; CONTROL_STATE.md; lower.rs:1978,2048; part.rs:55 | widgets/params/loop; painter cannot certify dispatch | pending | pending |
| `load_komplete_ui` | Modern Komplete UI typed/reactive frontend request (versioned). | **missing**; UI_FRONTENDS.md; no baseline builtin/runtime | R1; distinguish from legacy NativeUI | pending | pending |
| `load_native_ui` | Load and execute the legacy versioned .nui frontend package. | **missing**; E:1260; load.rs:641; no consumer | R1 | pending | pending |
| `make_instr_persistent` | Retain/restore source values that select captions/art/pages. | **partial**; load.rs:523; E:performance init | params/loop; render after actual recall | pending | pending |
| `make_persistent` | Retain/restore source values that select captions/art/pages. | **partial**; load.rs:523; E:performance init | params/loop; render after actual recall | pending | pending |
| `persistence_changed` | Source callbacks update values and presentation at their defined event boundary. | **partial**; CONTROL_STATE.md; lower.rs:1978,2048; part.rs:55 | widgets/params/loop; painter cannot certify dispatch | pending | pending |
| `read_persistent_var` | Retain/restore source values that select captions/art/pages. | **partial**; load.rs:523; E:performance init | params/loop; render after actual recall | pending | pending |
| `remove_keyrange` | Source named key-range bands and removal. | **missing**; E:1097; keyboard.rs:277 | params/loop; source band consumer | pending | pending |
| `set_control_par_real` | Typed scalar/indexed presentation/value properties. | **partial**; E:893; K:537; lower.rs:1985 | Per-parameter table + params/loop | pending | pending |
| `set_control_par_real_arr` | Typed scalar/indexed presentation/value properties. | **partial**; E:893; K:537; lower.rs:1985 | Per-parameter table + params/loop | pending | pending |
| `set_control_par_str` | Source text/image/font/path or indexed string properties. | **partial**; E:893; lower.rs:2048; K:479,508 | R3–R7/R10 + loop | pending | pending |
| `set_control_par_str_arr` | Source text/image/font/path or indexed string properties. | **partial**; E:893; lower.rs:2048; K:479,508 | R3–R7/R10 + loop | pending | pending |
| `set_key_color` | Authored keyboard colour/label/type/pressed state and enabling policy. | **missing**; E:1060; keyboard.rs:392 uses mapping/articulations | params/loop; route source KeyLook into keyboard painter | pending | pending |
| `set_key_name` | Authored keyboard colour/label/type/pressed state and enabling policy. | **missing**; E:1060; keyboard.rs:392 uses mapping/articulations | params/loop; route source KeyLook into keyboard painter | pending | pending |
| `set_key_pressed` | Authored keyboard colour/label/type/pressed state and enabling policy. | **missing**; E:1060; keyboard.rs:392 uses mapping/articulations | params/loop; route source KeyLook into keyboard painter | pending | pending |
| `set_key_pressed_support` | Authored keyboard colour/label/type/pressed state and enabling policy. | **missing**; E:1060; keyboard.rs:392 uses mapping/articulations | params/loop; route source KeyLook into keyboard painter | pending | pending |
| `set_key_type` | Authored keyboard colour/label/type/pressed state and enabling policy. | **missing**; E:1060; keyboard.rs:392 uses mapping/articulations | params/loop; route source KeyLook into keyboard painter | pending | pending |
| `set_keyrange` | Source named key-range bands and removal. | **missing**; E:1097; keyboard.rs:277 | params/loop; source band consumer | pending | pending |
| `ui_control` | Source callbacks update values and presentation at their defined event boundary. | **partial**; CONTROL_STATE.md; lower.rs:1978,2048; part.rs:55 | widgets/params/loop; painter cannot certify dispatch | pending | pending |
| `ui_controls` | Source callbacks update values and presentation at their defined event boundary. | **partial**; CONTROL_STATE.md; lower.rs:1978,2048; part.rs:55 | widgets/params/loop; painter cannot certify dispatch | pending | pending |
| `ui_update` | Source callbacks update values and presentation at their defined event boundary. | **partial**; CONTROL_STATE.md; lower.rs:1978,2048; part.rs:55 | widgets/params/loop; painter cannot certify dispatch | pending | pending |

### Fonts, strips, hierarchy and display modes

| Mechanism | Expected behavior | V2 status / evidence | Fix/test | Reach |
|---|---|---|---|---|
| Factory font IDs 0–25 (every ID) | Source family/weight/glyph metrics, colour and baseline; IDs 1,5,7,16,17,20 are larger in current approximation | **partial**: K:174 samples 26 colours and two sizes; P:228 uses theme font, ignores identity/weight | R5; capture all 26 native fonts | pending |
| Custom bitmap fonts, loose/NKR | Separate font IDs, glyph atlas, encoded character mapping and advances | **wrong/missing**: E:1049 collision; no font asset; pictures.rs:19 rejects font | R4; distinct-ID/glyph test | pending; get_font_id refs above |
| Picture .txt frames and horizontal/vertical axis | Cut N nonempty equal frames on selected axis | **partial**: K:86–143; pictures.rs:21–29, no remainder/oversize diagnostic | R7/R12; two axes, malformed frame counts | pending |
| Continuous value → frame | Clamp source range, select source-correct frame including midpoint/bipolar endpoints | **partial**: P:22 rounds normalized (N−1); native law unverified | R7; native min/mid/max plus out-of-range witness | pending |
| State strips and fixed frames | Per-kind off/on/hover/pressed; explicit state wins where specified | **partial**: P:35 only off/on; P:317 menu fixed frame ignored | R7 | pending |
| Resize margins and one-axis resizing | Fixed corners/edges, stretch designated centre axes | **wrong**: K metadata retained; P:79 stretches full image | R6; coloured-corner fixture | pending |
| PNG opacity / Has Alpha Channel | Actual alpha compositing; source sidecar policy where applicable | **partial**: decoded alpha preserved; sidecar flag unused | R6/R11; native sidecar fixture | pending |
| Classic wallpaper, animation, skin offset | Correct source viewport/header origin and current wallpaper frame | **partial/wrong**: P:181 first frame; no classic header crop; native rule unverified | R9; versioned stripe/state fixture | pending |
| Instrument icon | Source icon in correct host header location with hide policy | **missing consumer**: K:265 retains; needed_assets loads; P never draws | R12; host header test | pending |
| Loose/NKR/NICNT resource resolution | Same library, recursive normalized names, explicit failures/precedence | **partial**: resources.rs:40,51,77; separate ResourceContainer unused | R3; no sibling assets | pending |
| Panel parenting / inherited hide / local z | Relative rects, parent visibility, source stacking/clipping | **partial**: I:554,569,583 implement tree; native clip/cross-parent laws unverified | R8; nested fixture | pending |
| Whole/part hide flags | Skip hidden subtree; mask background/title/value/mod-light per kind | **partial**: whole works; value-edit value and baked parts not masked consistently | R5/R11; combined-mask fixture | pending |
| Page colour / colour alpha | Solid colour and authored pixels composed with readable generated fallback | **wrong fallback**: P:178 paints; light_under P:111 ignores it | R2; Conflux 94.94% pale ground | Conflux proved; corpus pending |
| HiDPI / zoom / viewport fit | Authored coordinate scale and hit boxes agree on both axes | **partial**: part.rs:111 width-only 0.5–1; native/fractional GPU tests absent | R8; native/device scale grid | pending |
| Initial Original presentation | Original by default, independent of unsupported metadata | **wrong/P0**: part.rs:55 chooses Vector when unsupported list empty | R0; assert Bitmap for clean/unsupported/missing-art fixtures | pending |
| Original / Vector / generated choices | Explicit independent choices; keep authored backgrounds/baked labels; retain selection on publication | **partial**: two choices; stale Arc resets Face; generated semantic stage separate | R0/R12 + loop; state lifecycle test | pending |
| Asset preparation/cache/windowing | Bounded off-audio/UI decode, stable identity, visible atlas subset, GPU reuse | **partial**: part.rs:55 creates Source and sync loads on UI thread; failed Option cached | R9/R12 + loop; oversize/retry/redraw test | pending |

## Ranked implementation start and pending corpus reach

W3 can start R0, R2, R4–R7 and R11 against small synthetic fixtures now. R1 is
a frontend integration, not a wallpaper workaround. R3 should reuse the already
pushed bounded resource/root-boundary work. R8/R9 require source-version geometry
fixtures before declaring native parity; R10 requires real service-backed data.

| Order | Mechanism / priority / effort | Measured witness | Authored instruments unlocked |
|---:|---|---|---|
| 1 | R0 Original default / P0 / S | Direct branch in part.rs:55 | pending census + differential fix validation |
| 2 | R1 authored frontend / P0 / L | Conflux native request has no consumer | pending; Conflux is affected, single fix does not prove completion |
| 3 | R2 effective-background contrast / P0 / S | Conflux fallback ink 1.05:1 | pending; readability is not native fidelity |
| 4 | R4 font identity/atlas / P0 / M | Passing Stock(0)-collision witness | pending; source refs pending |
| 5 | R3 bounded resource routing / P0 / M | Conflux wallpaper fails; own archive checked separately | pending; physically absent assets cannot be unlocked by lookup |
| 6 | R10 real table/meter/service widgets / P0–P1 / L | Table cell change identical pixels; meter hard-coded silent | pending |
| 7 | R5 source text metrics/layout/state / P1 / M | Alignment/text_y change identical pixels | pending |
| 8 | R6 slicing / P1 / M | Metadata present, all pictures Fit::Fill | pending |
| 9 | R7 image roles/states / P1 / M | Menu frame 0, switch off/on only, handle not painted | pending |
| 10 | R8 geometry/fit / P1 / M | Conflux 970×499 becomes 970×592 | pending; overflow is candidate until source rule verified |
| 11 | R11 per-widget colours/alpha / P1 / M | Colour slots retained, not consumed | pending |
| 12 | R9 wallpaper origin/window / P1 / M | Baseline draws first-frame top; v1 had header/window machinery | pending + native profile |
| 13 | R12 preparation/diagnostics / P1 / S–M | Failed resource is silent cached None | pending |

**Measurement update contract:** shared scanner provides the active/declaration
widget counts, known property/API uses, fonts, margins/strip axes, resource/decode
failures and background/ground fraction. Add counts here with manifest denominator
and scanner digest. The extension schema is installed, but the current smoke
coverage does not fill the corpus-count cells. Keep later dynamic-state/native-host
validation open.
