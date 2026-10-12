# MUI popup wheel ownership patch

Source: installed MUI 0.4.0 at dcf0796082feec053af1418e3a38a302ee61da0a, MIT. Only the mui crate is vendored; sibling crates stay pinned by KONTRA Cargo.lock. Cargo metadata is expanded from upstream workspace inheritance, unused examples omitted, and doctest include paths point at the included unchanged documentation.

Ui::get now honors the topmost captures_wheel boundary and its descendants, using one cached owner lookup per resolved wheel frame. capture_popup_wheel(existing_id) additionally registers an existing popup surface as an input scope without a layout wrapper. Automatic land_wheel stays in the same subtree, so popup lists scroll (including a popup that is itself a scroll node) and consume at either end. Registration survives retained subtrees and retires on unmount. No painting or audio changes.

Witnesses: src/ui/popup_tests.rs and the native popover input guard in src/ui/native_ui.rs, under the product root test harness. Original red used the exact upstream dcf0796 implementation: popup and exhausted popup both exposed wheel to the background. Existing generated anchors and normal nested-scroll handoff are preserved.
