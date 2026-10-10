# Shared authored geometry and traversal

Source work in progress; not READY until root checks and area no-run pass.

v1 `4bffbb18:src/uvi/host.rs:273` carries `UiBounds` as f64. Its snapshot traversal at lines 806–838 keeps widget IDs in declaration order, derives the current parent tree, retains authored child order for children still belonging to each parent, and appends missing current children. `src/ui/uvi_instrument.rs:557` renders `bounds.x * scale` and `bounds.y * scale` without integer projection.

One actual ScriptHost-to-renderer RED on `43f8ee7d` authored bounds [10.25,20.5,60.75,30.125] but produced surface [10,20,60,30]; exit 101. Receipt: `/mnt/Windows11/DEV_WORKSPACE/kontra-runs/w3-ui-projection-396/RED-CONTRACT.json`.

The shared Rect/Size now carry f64. The const integer Rect::new constructor preserves exact Kontakt declarations. The shared validation boundary rejects nonfinite or negative dimensions, nonfinite resolved positions/edges, and duplicate/missing traversal references. Image headers and filmstrip sizes retain integer pixel semantics; allocation targets use ceil after scaling.

Interface.paint_order is optional. None retains the existing source z/declaration traversal; Some is an authoritative stable-ID traversal filtered to the requested page; an explicit empty list paints no widgets. Widgets and bindings stay in declaration order. W1 owns the cumulative patch field Option<Option<Vec<WidgetRef>>> and live/reopen publication checks. W10 owns UVI producer geometry and child-order lowering.

W11 accepted page bounds `cf0d2295` are preserved: positive-size controls with any page intersection remain, controls wholly outside are excluded, authored height stays unchanged, and existing Falcon viewport ancestor clipping remains.

The current shared IR check passes 10/10. Root fractional rendering, actual traversal pixels, existing KSP pixel hashes, producer checks and area no-run are pending. No real-host, native-bank, corpus-wide, timing or RSS verdict follows from synthetic checks.

NEXT: UVI producer handoff, root GREEN, unchanged KSP hashes and area no-run, then READY.
