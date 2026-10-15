# UVI scalar presentation invalidation

Identical scalar presentation writes no longer request a complete UI export.
The generic widget property setter previously advanced the UI revision for
every write, even when a label, visibility, enabled state or other scalar was
unchanged. The actual threaded owner uses that revision to decide whether to
rebuild the complete interface; comparing the resulting face afterward cannot
recover the cost of exporting it.

The change only skips an identical immutable scalar on a plain backing table.
Resource-path normalization still runs first. Values and selections retain
their existing control/setParameter/callback path. Geometry setters retain
their explicit sizing flags. Mutable tables, functions and userdata still
invalidate when reassigned; a script can use reassignment to expose an in-place
edit. Backing tables with authored metatables preserve every write and its
side effects. Numeric checks retain distinct zero signs and leave nonfinite
writes observable. No widget, page, unsupported property or asset is pruned.

The source-free regression first failed against the original implementation:
8,192 identical scalar writes left the full projected face unchanged but
advanced its revision. The signed-zero/type/nil test separately exposed
redundant invalidation; two other tests already passed. With the fix, all five
focused checks pass, including a real unseeded ScriptThread. Eight acknowledged
controller events preserve the same Arc-backed face and perform zero additional
owner UI exports. A ninth event making a real text change performs exactly one
export and publishes the changed text. Mutable menu items and authored backing
metamethods remain observable.

The complete scan-enabled UVI unit suite passed: 182 passed, 12 ignored, zero
failed, optimized `ci`, x86-64-v3, locked and offline. Source and HEAD were
unchanged during both the targeted green and complete suite. Private commands,
source hashes and genuine red results are in
`PluginArtifacts/kontra-uvi-ui-invalidation-20261015/{red,green,fullscan}.json`.

This proves removal of redundant exports for identical scalar writes, not a
universal library speedup. The earlier Augmented Orchestra owner profile
observed 21 genuinely different faces; those measurements do not establish
that this change removes those exports. A fresh checked helper is required for
post-change real-bank performance evidence. Live changing interfaces still
need full exports; incremental projection is separate future work.
