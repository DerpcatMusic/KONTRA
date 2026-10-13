# Authored Kontakt table column count

Source baseline: `da17ace28897a9dd1e70e81ef145833b8a43abd4`.

The official NI [UI command manual](https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/user-interface-commands)
defines `set_table_steps_shown` as the number of displayed columns. Its
[widget example](https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/user-interface-controls)
declares a larger table array and changes its visible column count independently.
These primary sources were read on 2026-10-13.

The production UI instead passed this count to horizontal value-grid painting
and mapped pointer X across the full backing array. With an authored 32-cell
table displaying 16 columns, the right edge edited cell 31 rather than cell 15.
This is an authored layout and callback-index failure, not an appearance policy.

The fix paints only the leading visible columns across the widget width and
uses that same count for pointer/cursor indices. Readback retains all 32 cells,
so editing visible cells preserves hidden values. The invented horizontal grid
is removed. A reduced column count invalidates a drag's old hidden-column anchor.
Absent counts still show the entire table; the renderer bounds the count to
the available backing array.

Two authored regressions exercise production KSP compilation/projection and
the current UI: prefix pixels equal an independently declared 16-column table;
hidden values do not paint; absent/full counts agree; pointer edits carry the
expected visible cell and cursor indices while retaining the full array.

- `ksp_table_steps_show_a_column_prefix_without_value_grid_lines`
- `ksp_table_visible_columns_route_pointer_edits_to_the_displayed_prefix`

NCKP `steps.visible` remains explicitly omitted by the importer. The consulted
Creator Tools manual documents performance-view loading but not that serialized
JSON field's exact contract; this fix does not remove its diagnostic or claim
native exported-file parity.

This agent ran only Rustfmt parsing without writing reformatted sources and
`git diff --check`. Regression execution belongs to the coordinator. No build,
native Kontakt capture, native callback comparison, corpus census, or audio
parity result is claimed.
