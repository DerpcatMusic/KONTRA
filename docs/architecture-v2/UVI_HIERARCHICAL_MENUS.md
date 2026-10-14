# Falcon hierarchical menus

Falcon `Menu.hierarchical` now reaches a validated tree in the UI IR and the
same popup, pointer, keyboard and CPU painter used by production instrument
views. Grouping a menu preserves each leaf's original item index and value;
opening a category never writes a control value. The closed control retains
its authored full item text.

The [official UVI Menu reference](https://lua.uvi.net/class_menu.html) specifies
slash-delimited path entries, a one-based selected index and `getText(index)`
on the original item array. This implementation groups those documented paths,
retains first occurrence order within each branch, and rebuilds from current
items on each interface export. A `setItem` callback therefore changes the next
tree without reusing stale labels or renumbering its bindings.

## Boundaries

Only a boolean `hierarchical` on a Menu/ParamMenu is consumed. A false value
keeps the ordinary flat popup. MultiStateButton and malformed property types
remain diagnosed. Empty path components, duplicate full paths and an entry
that is both a selectable leaf and a category remain flat and explicitly
unsupported: the documentation does not establish how those native cases are
resolved. Hidden items retain their original indices and are absent from the
visible tree. Builders reject depth beyond 32 or more than 16,384 tree nodes
with a distinct renderer-limit diagnostic.

The tree builder uses keyed parent/label lookup rather than repeatedly scanning
siblings. Its work is proportional to the total number of path components;
materialization is bounded by the validated depth. Popup columns fit within the
view and open to the left when the right edge has insufficient space. Long
columns use the existing scroll facility. Pointer activation, right/left arrow
branch traversal, up/down sibling focus, Escape dismissal and leaf activation
share the existing input path. The inspector exposes the same tree and original
zero-based `item_index` values as additive per-widget metadata.

This proves Kontra's production consumer and binding behavior. Exact Falcon
popup typography, native flyout geometry and ambiguous-path behavior have no
same-fixture native-host pixel evidence here; they are not claimed equivalent.
Meter laws and Table fill styles are separate gaps and remain diagnosed.

The new Widget fields do not change a persisted binary model: `sampler-ui-ir`
interfaces/widgets have no serde implementation. Kontakt's product metadata
cache stores the audio IR and KSP initializer state, refuses scripted instruments,
and validates its importer signature. The UVI cache stores UFS directory metadata,
without script or UI state. Current interface exports reconstruct menu trees and
meter requests; no old binary UI record is accepted using defaulted new fields.

## Validation

Authored fixtures cover Unicode, hidden entries, original indices, dynamic
`setItem` updates, malformed/ambiguous paths and renderer bounds. Production
view tests exercise actual pointer and keyboard input with CPU-painted pixels,
including a child column outside the first popup's frame. Selecting the third
original item under the first category sends value 3 and the original text to
the real Lua `changed` callback. No vendor script, artwork or native-host capture
is included in these fixtures.

Focused results: IR tree fixtures 2/2; UVI UI, hierarchy, property and meter
fixtures 23/23; shared production popup interaction/CPU-pixel fixtures 3/3;
inspector tree-metadata fixture 1/1 in each of the binary and integration test
targets. The popup edge fixture also verifies left-opening placement at the
right viewport edge and keyboard selection of original item 3.
