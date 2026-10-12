# UVI library browser presentation

Source changes following the 2026-10-04 screenshots are reviewed statically.
They are not compiled, visually checked or installed under the CPU restriction.

Selecting a UVI bank with an empty search now shows expandable folders from
its actual member paths. Leaves retain the original bank path, UUID and member
identity. Folder rows cannot load instruments or become filesystem targets.
Pointer and keyboard navigation reuse the existing browser controls. Search,
Favorites and Recent remain flat; Unified / By player settings are preserved.
Oversized trees fall back to the complete flat listing. This follows the
expand/collapse behavior described in the official [Falcon File Browser
manual](https://s3.amazonaws.com/uvi/UVIFC/falcon_manual.pdf); it does not claim
complete parity with Falcon's separate searchable Library Browser.

Both cached-startup and fresh-scan workers now discover explicit bank-specific
cover sidecars: `Bank.ufs` tries `Bank.png`, then `Bank.jpg`, then `Bank.jpeg`.
Missing or invalid candidates fall through. The scanner never reads the UFS
body or uses a sibling bank's generic wallpaper or arbitrary instrument panel.
Regular-file validation, nonblocking Unix opens, encoded-byte limits and
decoded-pixel limits bound this automatic path. Images load off the UI thread;
the immediate cached listing can initially show generated covers.

User-selected covers already worked and retain priority. The inspected owned
bank folders contain no exact sidecars, and an authoritative bundled product
cover identity has not been established. Those banks can therefore still show
generated initials. Native product-cover discovery remains unfinished; this
change must not be reported as recovering their missing product images.

## Separate native browser metadata

Official [Falcon browser support guidance](https://support.uvi.net/hc/en-us/articles/22417230234013-Falcon-3-1-Browser-Edition-New-Features-and-Troubleshooting)
requires a separately installed Tag Library. It identifies `TagLibrary.ufs` as
a preset-preview container, not a playable soundbank to mount in the browser.
The public release-thread account **UVI Doctor**, post 23 dated 2025-03-21,
also attributes product images and preset tags to that file in the
[Workstation 4 release discussion](https://vi-control.net/community/threads/uvi-releases-uvi-workstation-4-new-browser-tags-previews-uvi-starter-soundpack-free.161264/page-2).
That representative commentary is a location lead, not a decoded-schema receipt.

The file was absent from three explicitly checked known locations and from the
existing 26-bank catalog. This does not establish machine-wide absence.
No Tag Library download, container decode or image extraction was performed.
Its product/bank identity join, cover member schema and ownership bindings
remain unimplemented; the metadata container must not become a playable preset
library merely because it uses the `.ufs` extension.

Prepared functional checks cover folder navigation and preset identity, exact
sidecar selection, fallback, bank isolation, resource bounds and FIFO rejection.
All are unrun. No index schema change, bank census, asset decoding or binary
replacement was performed for this work.
