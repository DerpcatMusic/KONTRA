# Authored UI loading and script-call coverage

Runtime source: `f199980d` plus W3 picture reduction slice `4a05318d`.
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
| Plain `ui_slider` | Authored kind → renderer | Square dimensions currently select a dial without authored type intent. Pending renderer semantics review; no change in this slice. |

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
Trusted source `b34c08e6` has three authored RED findings: missing `setHeight`,
missing `Button.push`, and an invented constructor grid. W10 owns their v1
`4bffbb18` port, module/resource semantics, publication and production pixel
receipts. A prepared port or a recognized call does not establish runtime support;
its GREEN/no-run/READY results are pending and must be cited separately.

## Acceptance limits

The decoder's exact pixel fixture establishes reduction parity, not full-library
fidelity. Scanner Original OK certifies authored paint and picture resolution,
not every callback, gesture, font or page transition. Timing and RSS under shared
workloads remain unknown. No allocator experiment, release build or installation
belongs to this slice.
