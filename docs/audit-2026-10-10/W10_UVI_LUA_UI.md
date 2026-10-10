# W10 authored UVI Lua/UI parity — READY

Product: `44eacd756364fb3c92228d6eac0e00527ec0ffbe`. Reference: `4bffbb18:src/uvi/host.rs`. Baseline: `b34c08e6` above diagnostic `4912e581`. This ports actual v1 behavior into the existing v2 Lua owner and shared renderer. It removes the invented constructor grid.

## Executed contracts

| Authored call or behavior | Retained proof |
| --- | --- |
| `setHeight`, `Button.push`, origin coordinates and relative child positions | 3 corrected-target UI RED failures → GREEN |
| Real `uvi.ChordRec`, real `uvi.AsyncUpdater`, embedded precedence, false reload, cache, cycles, validated module names and identical-source aliases | 6 module RED failures → GREEN |
| Horizontal/vertical strip metadata, root changes and callback-created widget publication | 3 asset/publication RED failures → GREEN |
| Existing UI input, persistence, logical types and script-thread publication | Targeted suite: 22 passed, 0 failed |
| Actual asset-worker loading, declared page/positions and callback-selected strip frames | Shared renderer pixel test: 1 passed, 0 failed |
| Affected area and root UI compile | `sampler-uvi --no-run` and root `--lib --features shots --no-run`: PASS |

The pixel fixture creates its own clear PNGs and XML/Lua. On the 320×160 page the nested knob and slider occupy `(30,20,32,32)` and `(90,20,32,32)`. Their centers are RGBA `(255,0,0,255)` initially and `(255,255,0,255)` after the real Button callback. The callback uses ChordRec and changes the label from Ready to M. Both screenshots were visually inspected. These are authored-contract checks, not native pixel-matching claims.

## Receipts

Directory: `/mnt/Windows11/DEV_WORKSPACE/kontra-runs/w10-uvi-scan-344`.

- `ui-modules-READY.json`: exact product SHA, source/log/PNG digests, corrected target, renderer provenance.
- `ui-v1-corrected-RED.log`, `ui-modules-assets-RED.log`: behavioral REDs.
- `ui-modules-assets-GREEN.log`: preserved initial port compile failure (missing import), followed by successful `ui-modules-assets-GREEN-retry.log`.
- `ui-modules-area-no-run.log`, `ui-modules-renderer-build.log`, `ui-modules-authored-pixels.log`, `ui-modules-root-ui-no-run.log`: all successful.
- `scripted-ui-shots/uvi-scripted-before.png` and `uvi-scripted-after.png`: real renderer output.

## Bank coverage and remaining work

[Per-bank checklist](W10_UVI_BANK_CHECKLIST.csv): 26 banks / 660 presets, based only on retained catalog metadata (manifest SHA256 `023d53cb37e043f50dc257bd88380ab13e5dc2c281504bb7b7ba2a1e128b1ed9`). Every installed-bank execution, artwork/font/filmstrip, position, ordering/visibility/tab, persistence and interaction cell remains UNKNOWN/PARKED. Protected payloads were not opened. V1 cells needing the official reader remain UNAVAILABLE-NO-THIRD-PARTY.

Class userdata/inheritance is separately READY in [W10_UVI_CLASS.md](W10_UVI_CLASS.md). Remaining source-observed parity work: named geometry precedence, real Unit/Mapper enum IDs, named-parent relationships and `setKeySwitches` publication. Those behaviors are not admitted by this slice. Callback deadline acceptance remains UNKNOWN; the ordered single frozen diagnostic recorded zero misses and captured no attribution events (see [W10_UVI_CALLBACK32.md](W10_UVI_CALLBACK32.md)). No timing or corpus improvement is claimed here.

NEXT: named geometry and parent/children construction with targeted contracts and shared-renderer pixels.
