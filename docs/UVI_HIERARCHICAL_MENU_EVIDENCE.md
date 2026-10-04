# Hierarchical preset-menu source evidence

Reviewed 2026-10-04. This is source UI and hosted-callback evidence pending
package verification; it does not establish native menu placement/style or a
new installed-plugin result.

The owned original/V2 general-preset menus contain 68/73 items, including 66/71
slash-separated paths, five/nine root groups and two root leaves each. Three
other menus in each bank set the hierarchy flag with only flat entries. UVI's
[Menu API](https://lua.uvi.net/class_menu.html) documents that flag and path
syntax. The source preserves authored order and original one-based leaf indices;
category activation sends no value. Invalid, empty or deeper-than-32 paths retain
complete flat selectable items.

Actual source MUI pointer selection passes after resizing the same open menu
720 → 360 → 1080 → 720. Popup panels remain inside each viewport; six reviewed
screenshots cover both banks at 360/720/1080 pixels. Each selects original index
8, distinct from its category-local row ordinal. Submission to the actual hosted
worker invokes the authored changed callback; captured value remains 8 with
zero rejected UI inputs. Initialization and callbacks settle across more than
one second of worker frames.

Seventeen focused checks cover host snapshot ownership/type/budget, bank-font
ownership, three-level pointer/keyboard selection, one-level Left, stale/reload
rejection, unchanged flat indices, tooltip behavior and raw numeric editing.
Actual render checks retain 60 numeric font/skin/geometry observations. These
use source helpers and cached dependencies; they are not a new broad Cargo or
installed-host run. Native visual fidelity, musical behavior and numeric
unit/rounding calibration are outside this proof.
