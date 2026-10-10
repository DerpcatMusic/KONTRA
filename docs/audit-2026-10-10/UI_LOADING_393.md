# Authored UI loading and script-call coverage

Runtime source: `f199980d` plus W3 picture reduction slice `4a05318d` and authored
control renderer `9d08a583`.
Receipt directory: `/mnt/Windows11/DEV_WORKSPACE/kontra-runs/w3-ui-load-parity-393`.

The target is the library's authored KSP or UVI UI. Full editor pixel equality is
not the acceptance criterion. The script's assets, frames, text, placement,
ordering, visibility and selected page remain authoritative.

## Shared loading difference

V1 `0cb7a8a0:src/artwork.rs::resample` integrates fractional source-pixel coverage
when shrinking. V2 selected one source pixel per output pixel. Alternating detail
therefore disappeared or aliased, and transparent edges lost their coverage.
The W3 port applies v1's premultiplied-alpha coverage calculation to all shared
picture codecs. PNG retains scanline processing and one accumulator row; frame
selection, crop windows, cancellation, output limits and no-upscale behavior stay
in the existing decoder.

The failing-first fixture checks the exact v1 bytes for an 8-to-7 stripe reduction
and a translucent edge, on both axes and through streamed PNG and decoded RGBA.
Existing tests also exercise selected filmstrip frames, crop windows and codecs.
The corrected per-worktree RED ran and failed the pixel assertion. Earlier
shared-target build failures are transport failures and are excluded.

## Script-call seams

| Authored intent | Shipping source path | Finding / owner |
| --- | --- | --- |
| Pictures, strips, frame selectors, bitmap fonts | KSP emitter assets → resource resolver → picture worker → renderer | Shared reduction fixed in this slice; per-library current paint still requires observation. |
| Native package request | KSP `load_native_ui` request → `Interface.native_ui` → Native frontend | A Native request is distinct from `make_perfview`; it must remain selectable even without legacy widgets. |
| `make_perfview`, `load_performance_view` | Evaluator sets performance intent; emitter drops it before default face selection | W1 owns explicit IR propagation and authored default selection, including empty-widget performance views. |
| Position, parent panels | Emitter → `Interface::page_rect` → authored renderer | Source path exists; current gesture/native-host parity is not certified by this review. |
| Width and height calls | Evaluator → page dimensions → renderer | W11 owns literal v1 exclusion of controls wholly outside the authored page, retaining partial intersections. |
| Visibility and z-order | IR visibility and ordered children → renderer | Preserve script order and hidden state; a source reference count is not an executed-call receipt. |
| Text and font state | Text properties and state styles → renderer | Paths exist; no blanket typography acceptance claim. |
| Plain `ui_slider` | Authored kind → renderer | Follow-up removes the dimension-based dial choice. Authored `MOUSE_BEHAVIOUR` axis changes slider pixels; `ui_knob` retains its dial pixels. W2 separately owns input semantics. |
| Menu selected index and hidden selected item | KSP getter → menu IR → caption/popup | W5 owns the getter/RMW contracts. W3 captured a real-KSP caption pixel RED: a hidden selected item displayed the first visible choice. The v1 caption-selection port passes GREEN 1/1 and area no-run. |

The detailed `KSP-UI-CALL-CHECKLIST.json` includes all eleven Kontakt libraries
and 835 observed instruments/multis from the complete historical census at
`5fc362f3`. It groups source references by assets, position, visibility/order,
text, performance page and interaction. Counts describe source observations,
not successful execution. Every library retains an explicit pending runtime
acceptance status. `CORPUS-FIDELITY-CHECKLIST.json` additionally records the four
historical UVI library groups; new frozen-v1 UVI runs requiring the third-party
reader are prohibited.

Historical Kontakt observations were 752 Original OK, 53 missing-images and 30
Conflux script-error results. Pacific and Vista missing-art cases were previously
searched across the library folder and containers and also failed frozen v1.
These are historical findings, not fresh shipping-source or native-host verdicts.

| Kontakt library | Historical observations | Original OK | Missing images | Script errors | Current call fidelity |
| --- | ---: | ---: | ---: | ---: | --- |
| ANALOG STRINGS | 1 | 1 | 0 | 0 | Pending |
| Afflatus Chapter II Brass | 348 | 348 | 0 | 0 | Pending |
| Areia | 155 | 155 | 0 | 0 | Pending |
| CHORUS | 42 | 42 | 0 | 0 | Pending |
| Dolce | 77 | 77 | 0 | 0 | Pending |
| Conflux | 51 | 21 | 0 | 30 | Pending |
| Morphology Evolved | 1 | 1 | 0 | 0 | Pending |
| Pacific Ensemble Strings | 50 | 4 | 46 | 0 | Pending |
| Vista | 7 | 0 | 7 | 0 | Pending |
| Solo | 100 | 100 | 0 | 0 | Pending |
| Una Corda | 3 | 3 | 0 | 0 | Pending |

W10's current metadata-only checklist covers 26 UVI banks / 660 programs:
`/mnt/Windows11/DEV_WORKSPACE/kontra-runs/w10-uvi-scan-344/scripted-ui-bank-checklist.csv`
and `scripted-ui-checklist-PROVENANCE.json`. Protected runtime cells are
UNKNOWN/PARKED; frozen v1 cells needing the official reader are unavailable.
W10 delivered READY `bcf09ee2` after corrected behavioral RED3+9 and targeted
GREEN22/22, area no-run, renderer build and a shared asset-worker pixel test.
Its v1 `4bffbb18` port covers `setHeight`, `Button.push`, authored constructor
positions, embedded module semantics, strip orientation and publication.
Receipt: `w10-uvi-scan-344/ui-modules-READY.json`. These trusted fixtures do not
certify the 26 installed banks or 660 protected programs.

W2 delivered authored input-axis READY `ec53b382` with fixture `7aaa2d98`:
24 widget tests and no-run pass. Its fresh shipping-source Native footer witness
also passes: all six fields retain seven characters, equal the saved string,
and fit a 44px frame/viewport with 44px advance and zero insets.
Receipt: `w2-footer-current.log`. No new font workaround is required.
W2 also establishes the four legacy aliases as skin proxies: the Native package
has zero Alias tokens and all four original controls exist. The Edit-selector
witness observes 22 graph reads from published KSP slot 2. Receipts:
`w2-alias-native-394/READY.json` and `w2-authored-axis-394/READY.json`.
These RAM-only checks do not establish host pointer, sound or corpus acceptance.

The [NI control-parameter reference](https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/control-parameters)
defines the selected menu index as a getter. The
[NI UI-command reference](https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/user-interface-commands)
specifies menu `get_control_par(VALUE)` as an index and keeps a selected hidden
item displayed until deselected. W5 confirms pending `f2c59e2c` covers the getter
and hidden-entry lookup. W3 owns the caption fix: v1 `perf_view::caption_of`
selects from all authored entries. The real-KSP pixel regression now captures RED: hiding the selected entry
changes its hash to the other visible entry. The renderer port selects from all authored entries. GREEN restores identical
selected/hidden-selected hashes (`ed4f36e0…`); deselection remains distinct
(`6e935bce…`). GREEN 1/1 and area no-run pass. Receipt:
`w3-menu-caption-395/READY.json`. This fixture does not
establish a measured failure in any particular installed library.

## Pacific and Vista resource handoff

The corrected current probe and fresh frozen-v1 pair use the same Pacific
10 Cellos Trills and Vista 3 Violins FFF Overlay NKIs. Both request
`Resources/pictures/pic.png` on a visible switch and
`Resources/pictures/admin_bg.png` on a hidden label. Searches across the entire
Kontakt folder find zero exact, basename, stem or double-stem matches in 20
indexed containers and loose assets. One Vista NICNT is rejected at parse time;
its resource completeness remains unresolved.

V2 finds and decodes all other requested visible pictures (Pacific 4/4, Vista
3/3). Frozen v1 reports two missing picture references per NKI; it counts the
hidden label as well. V1 JSON does not export the failed names, so this establishes
same-NKI missing-image parity rather than an independent v1 filename trace.
The lookup evidence and exact requests were handed to W12; no renderer workaround
is added. Receipt: `w3-authored-control-kind-394/MISSING-IMAGE-CAUSE.json`.
The metadata witness passes 1/1 and root UI area no-run passes. Product resolution
and native Kontakt comparison remain open.

## Authored control type follow-up

The CPU pixel regression compiles square, same-size KSP controls with title/value
hidden: a horizontal slider, a vertical slider, and a knob. In RED all three
rendered the same dial (`a1eb117a…`). Removing seven renderer lines makes the
slider hashes distinct (`90e33a6b…` horizontal, `d2ab697b…` vertical), while the
knob hash stays `a1eb117a…`. The focused test, area no-run and scanner build pass.
Receipt: `/mnt/Windows11/DEV_WORKSPACE/kontra-runs/w3-authored-control-kind-394`.

V1's `perf_view::knob_like` also guessed from picture names and dimensions; that
heuristic is intentionally not ported under the current authored-call directive.
This change leaves page bounds, hit geometry and input axis/sensitivity to their
owners. It selects the visual control type supplied by the script.

## Fresh paired representative paint

All eleven representative NKIs loaded in both frozen v1 and v2. V1 reports
seven Original OK and four missing-images; frozen v2 at `9d08a583` reports nine
Original OK and two missing-images. Afflatus and Conflux improve to Original OK;
no v1 Original OK representative regresses. Pacific and Vista remain the resource
lookup handoff above. This is one NKI per library, not the full 835-item census.

Pixel receipts retain every painted view. V1 captures the full editor, whereas
v2 captures the authored page, so their whole-image hashes are not equality
scores. The shared decoder's exact v1 byte oracle supplies the reduction check.
Receipt: `w3-authored-control-kind-394/PAIRED-GALLERY-RECEIPT.json`.
Font metrics, filmstrip selection in real libraries, callbacks, host pointers,
authored default-page acceptance and timing/RSS still require their own gates.

## Acceptance limits

The decoder's exact pixel fixture establishes reduction parity, not full-library
fidelity. Scanner Original OK certifies authored paint and picture resolution,
not every callback, gesture, font or page transition. Timing and RSS under shared
workloads remain unknown. No allocator experiment, release build or installation
belongs to this slice.
