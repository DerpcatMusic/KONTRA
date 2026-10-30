# MUI popup wheel ownership patch

Source: installed MUI 0.4.0 at dcf0796082feec053af1418e3a38a302ee61da0a, MIT. Only the mui crate is vendored; sibling crates stay pinned by KONTRA Cargo.lock. Cargo metadata is expanded from upstream workspace inheritance, unused examples omitted, and doctest include paths point at the included unchanged documentation.

The consolidation review ports two bounded stability corrections from upstream
`65ab5baa58143c2b78a734a380b5aad0b0bc34ef`: retained memo captures are destroyed
outside their thread-local table's mutable borrow, and the headless clock rejects
unrepresentable durations. `Ui::close` uses the same memo release path as `Drop`;
headless parking releases the old owner's thread-local trees before moving the UI.
Regressions check reentrant destruction on release/close/drop, isolation between
editors, rebuilding after release, and invalid clock inputs. This does not change
the MUI sibling pin or adopt upstream's native GPU/OS initialization migration.

Ui::get now honors the topmost captures_wheel boundary and its descendants, using one cached owner lookup per resolved wheel frame. capture_popup_wheel(existing_id) additionally registers an existing popup surface as an input scope without a layout wrapper. Automatic land_wheel stays in the same subtree, so popup lists scroll (including a popup that is itself a scroll node) and consume at either end. Registration survives retained subtrees and retires on unmount. No painting or audio changes.

Witnesses: src/ui/popup_tests.rs and the native popover input guard in src/ui/native_ui.rs, under the product root test harness. Original red used the exact upstream dcf0796 implementation: popup and exhausted popup both exposed wheel to the background. Existing generated anchors and normal nested-scroll handoff are preserved.

# MUI per-Ui frame node limit

Upstream refuses any frame whose tree exceeds `mui_layout::Limits::default()` (4096 nodes) with `BudgetExceeded`, hard-coded in `Ui::frame`, the popup/tip validations, memo expansion and `SceneSpec::new`. A real instrument face (thousands of controls, each several nodes) blanked the whole editor.

`Ui` now carries `node_limit` (default 65536, `Ui::set_node_limit`) and uses `Ui::limits()` at every one of those sites. The limit guards linear work and buffers (layout allocates in proportion to the nodes present) and the depth limit (64) still bounds recursion, so only the node count changed. Witness: `src/ui/chrome_tests.rs` `a_five_thousand_widget_face_never_blanks_the_editor`.

# Semantic requests invalidate their retained ancestors

Accepted `Ui::request_action` calls wake the target surface's retained memo
ancestors before the next build. Otherwise an idle cached slider/button skips
its input closure and the queued request expires without changing its model.
This retains targeted invalidation, semantic value clamping and event coalescing;
pointer capture and source-unit conversion remain unchanged. Witnesses:
`semantic_actions_wake_targeted_nested_memos_without_rebuilding_siblings` and
the application's exact `mapped_accessibility_set_value_uses_authored_units`.

# Frame time and caller deadlines

`Ui::time()` reads the frame clock (the sum of each frame's `dt`), and
`Ui::repaint_in(secs)` adds a caller deadline to `Ui::repaint_after`, the
earliest one winning. KONTRA's value-edit arrows repeat while held and its
level meters fall and hold on Kontakt's GUI tick; both need a frame when no
input arrives. No change to painting or input.

# The pointer is re-derived when the scene space changes

`Driver` recorded the pointer in **scene units** at event time, dividing by
the zoom in force then. The scene space is physical pixels over the window
scale and the zoom, so a window scale the host changes (`gui.set_scale`, a
compositor change) or a zoom the view changes (a fit-to-window editor
re-zooms on every resize) moves the scene under a pointer that did not move.
The recorded position then belongs to the old space, and since baseview's
button events carry no position of their own -- only the last move's -- the
press that follows is tested against rects the pointer is not over. The
error grows with distance from the origin, so controls near the corner keep
working while a floating menu's items do not.

`Driver` keeps the pointer in window points (`pointer_at`) and derives the
scene position from it, so a zoom change re-derives it in `advance` and the
native host re-derives it from the physical point in `Handler::resized`
(`Driver::reposition`). Painting and layout are unchanged: both already used
the scale in force. Witnesses: `host::tests::a_zoom_change_moves_the_pointer_with_the_scene`
and `kontra-native-host`'s `tests::a_press_after_a_scale_change_lands_where_the_pointer_is`;
both fail on the unpatched driver, landing the press at the old scene
position.

# Queued pointer edges move with the scene too

The patch above re-derives `self.pointer` when `advance` recomputes the zoom,
but the edges already sitting in `Driver::pending` had copied their position
out of the old scene space at event time. A press that arrives between a
fit-to-window resize and the next frame was therefore dispatched -- in that
same frame, against the new rects -- at its old scene position: the identical
miss, one frame later than the one `repose` fixes.

`advance` now rescales every queued pointer position by `old_zoom / new_zoom`
when the zoom changes (`Driver::reproject`), before any of them is dispatched.
The ratio is exact: a queued edge's recorded position is its window point over
the old zoom, and the new space wants that same point over the new zoom.
Presses still carry the position they were enqueued with, so a move queued
after them does not retroactively move them; only the space changes.
`Pending::Cancel` is remapped with the rest. Wheel deltas are not positions
and are left as enqueued. Painting and layout are unchanged.

Witness: `host::tests::a_queued_press_is_reprojected_when_the_zoom_changes` --
pointer at window point (100, 60), zoom 2, press queued at scene (50, 30),
window resized to half so the zoom becomes 1. Unpatched, the frame that runs
the press builds with the pointer still at (50, 30); patched, at (100, 60),
the point it was pressed on. The existing
`a_zoom_change_moves_the_pointer_with_the_scene` still covers the pointer
itself.
