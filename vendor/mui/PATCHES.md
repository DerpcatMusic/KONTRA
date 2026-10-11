# MUI popup wheel ownership patch

Source: installed MUI 0.4.0 at dcf0796082feec053af1418e3a38a302ee61da0a, MIT. Only the mui crate is vendored; sibling crates stay pinned by KONTRA Cargo.lock. Cargo metadata is expanded from upstream workspace inheritance, unused examples omitted, and doctest include paths point at the included unchanged documentation.

Ui::get now honors the topmost captures_wheel boundary and its descendants, using one cached owner lookup per resolved wheel frame. capture_popup_wheel(existing_id) additionally registers an existing popup surface as an input scope without a layout wrapper. Automatic land_wheel stays in the same subtree, so popup lists scroll (including a popup that is itself a scroll node) and consume at either end. Registration survives retained subtrees and retires on unmount. No painting or audio changes.

Witnesses: src/ui/popup_tests.rs and the native popover input guard in src/ui/native_ui.rs, under the product root test harness. Original red used the exact upstream dcf0796 implementation: popup and exhausted popup both exposed wheel to the background. Existing generated anchors and normal nested-scroll handoff are preserved.

# MUI per-Ui frame node limit

Upstream refuses any frame whose tree exceeds `mui_layout::Limits::default()` (4096 nodes) with `BudgetExceeded`, hard-coded in `Ui::frame`, the popup/tip validations, memo expansion and `SceneSpec::new`. A real instrument face (thousands of controls, each several nodes) blanked the whole editor.

`Ui` now carries `node_limit` (default 65536, `Ui::set_node_limit`) and uses `Ui::limits()` at every one of those sites. The limit guards linear work and buffers (layout allocates in proportion to the nodes present) and the depth limit (64) still bounds recursion, so only the node count changed. Witness: `src/ui/chrome_tests.rs` `a_five_thousand_widget_face_never_blanks_the_editor`.
