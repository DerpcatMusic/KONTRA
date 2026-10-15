# Incremental UI export: bounded next experiment

This is a read-only design following the measured [owner attribution](UVI_OWNER_TIMING.md).
No production export cache, frame throttle or UI-value cache is implemented.
The shipping candidate remains the behavioral baseline.

## Measure the export itself first

The current observation brackets all of `ScriptHost::interface`. It does not
separate these actual code paths in `script/ui.rs`:

1. Registry collection and `widget_tree`: identity map, parent/child validation,
   cycle detection and authored paint traversal.
2. Per-widget field reads and IR conversion, including live Param* getters,
   table/menu state, mapper/unit text, assets/fonts/styles, meters and XY links.
3. Documented unsupported-property inventory, including property iteration and
   validity checks for genuinely consumed styles.
4. Page extents and final `Interface::validate`.

First add a few opt-in coarse spans around collection/tree, the full conversion
loop, extents and validation. If the loop dominates, sample a bounded fraction of
widgets for conversion versus unsupported-inventory time and report that sampling
scope/overhead explicitly. Do not add clocks to every property or assume a
linear list/field lookup is expensive merely because it executes often.

## Smallest conservative cache boundary

If topology work is material, reuse only the validated topology result behind an
**exact** signature of current registry identities/order, every parent identity
and each actual child-list key/value sequence. Registry append/reorder, duplicate
identity, reparent, child-order edit or malformed sequence invalidates it.
Compare content rather than trusting a hash or a revision hint. The same canonical
validator handles misses and errors, retaining the current visible diagnostic and
empty paint order for invalid graphs. All widget properties and bound values are
still read fresh. This avoids inventing an invalidation scheme for arbitrary
property tables and measures one concrete reusable substage.

If field conversion is material, a later per-widget cache must key an exact,
typed snapshot of every exporter input, including absence/type distinctions and
nested table contents. This can reuse converted IR fragments and per-widget
diagnostics without treating a Lua table address as immutable. Dirty flags may
avoid work only where every mutation is controlled; otherwise they are hints and
the exact input comparison or canonical export remains mandatory. Its extraction
cost may erase the gain: reject it if that happens.

`__data`, item/value/child tables and host restore paths can be edited directly.
Lua `__newindex` does not observe writes to existing raw table keys. A coarse or
per-widget revision alone therefore cannot establish an unchanged input. Merely
adding `value_revision` and `structure_revision` would miss these cases. Never
return a cached explicit `ScriptHost::interface()` snapshot solely because no
known setter ran.

## Dependencies that must remain fresh

| Input/change | Required handling |
| --- | --- |
| Scalar value | Initial value, unit-derived value text, caption rules and a default derived from the current value when no explicit default exists |
| Param* widget | Re-read the live element getter and current parameter policy; no cached parameter value or default |
| Table values/length/range | Cells, components, component control IDs and bipolar/default semantics |
| Menu content/hierarchy | Exact authored item order/value, selected text, ambiguity errors and original leaf indices |
| XY names/registry | Re-resolve aliases when names or registry identities change |
| Parent/children/geometry | Canonical authored traversal, visible/enabled ancestry and page extent |
| Images/fonts/strip metadata | Canonical asset/style indices and role metadata; late resource dimensions or authority changes invalidate relevant geometry |
| Appended controls | Rebuild structural metadata and extend the immutable UI control-cell registry off audio; no existing binding may be renumbered |
| Unsupported fields/faults | Same unsupported rows/order and findings/fault revisions, including malformed properties and validation failures |

Asset/style references are currently assigned during full declaration-order
export. Cached fragments need symbolic local references and a canonical remap,
or a full rebuild on any relevant asset/style change. Stable-but-different global
indices would fail the complete-model oracle even if a screenshot happened to
look the same. The full authored graph remains available, including hidden nodes.

## Acceptance and rejection

Keep the canonical uncached exporter as the oracle during the experiment. At the
same observed logical boundary compare complete `Interface`, geometry, bindings,
unsupported diagnostics and production CPU-rendered pixels in both presentations.
Use authored hostile-mutation fixtures, real AO initial/LayerNext states, current
Morphology settled UI and ordinary no-audit runs. Dynamic tests must include
direct raw table edits, table/menu changes, parameter writes, resource changes,
append/reparent/invalid graphs and callback faults.

The real gesture also retains 1,746 task conservation, bounded independent PCM
comparison, fault/queue behavior and full final UI. Run paired optimized frozen
binaries with equivalent clocks, inputs and cache observations; keep profiling
off for acceptance timings and measure its overhead separately. Reject gains
that skip hidden controls, stale live defaults, omit diagnostics, alter aliasing
or move work into an unmeasured publication stage. Frame throttling cannot replace
identifying and reducing the expensive export work.
