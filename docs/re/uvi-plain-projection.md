# Fresh UVI scalar-widget projection

Production UI export now projects a plain scalar widget from one fresh raw-table
pass when its data table has no metatable, its element is nil, and its kind is in
an explicit scalar allowlist. Projection happens at the widget's declaration
position. Bound values, data metatables, nested sequences, cross-widget lookups,
image-size and meter services, and unknown kinds use the canonical lowering path
with its existing getter order. Constructor data tables are plain; the ordinary
widget metatable belongs to the proxy. Scripts can add a data metatable, so this
constructor fact does not establish eligibility for an existing instrument.

There is no retained value cache, revision-only gate, hidden-widget skip or
library-name condition. Raw/nested edits are observed afresh. The scalar route
executes no Lua getter or file service between its property reads. A malformed
inventory key preserves the canonical documented-property cutoff while modeled
field collection continues, since canonical field reads are independent of that
cutoff. Existing export revision capture is unchanged.

The 2026-10-15 exploratory benchmark used one optimized `ci` test executable,
eight alternating reference/candidate pairs per authored 10,145-widget fixture,
and Linux `ThreadCPUTime`. Timed intervals contain complete exports; complete
model comparisons and serialization run afterward. Reference is the previously
selected raw `for_each` inventory path. Ratios are candidate/reference.

| Authored fixture | Eligible widgets | Median paired caller CPU ratio | Candidate wins |
| --- | ---: | ---: | ---: |
| Ordinary fields | 10,145 | 0.45067 | 8 / 8 |
| Sparse documented fields | 10,145 | 0.45840 | 8 / 8 |
| All documented fields | 10,145 | 0.68176 | 8 / 8 |
| Mixed scalar/menu/metatable fallback | 5,072 | 0.78026 | 8 / 8 |
| All metatable fallback | 0 | 0.99940 | 4 / 8 |

**This cohort was contended, not a quiet acceptance run.** Compiler processes
were present initially and in all 68 process samples; one external process
reached 1,522.08% CPU. Thread CPU excludes time while the caller is descheduled,
but frequency, cache and memory-pressure effects remain. The large, consistent
paired reductions and preserved fallback behavior justified a reversible
production promotion. Quiet confirmation remains pending. Absolute latency and
a real-instrument speedup are not established; actual instrument metatable and
kind-route prevalence is unknown. Opt-in owner-audit route counters can measure
that prevalence without exporting script values or artwork.

All 80 timed complete Interfaces matched. The authored reference and candidate
transport files were byte-identical (SHA-256
`e5057e012374c70d4238c9fbe82d25aab3dce29957dc8d61ab20cf7b6d4bf409`).
The root renderer bridge passed complete 10,145-widget model and resolved-geometry
comparison plus exact Bitmap and Vector PNG comparison. The promoted UVI `scan`
lib suite passed 177 tests, with 12 ignored and zero failures, using
`--profile ci --locked --offline -j2 --test-threads=1`. Authored tests also cover
all scalar kinds, documented value types, asset/style/identity ordering, raw and
nested mutations around live getters, commands, saved state, revision behavior,
malformed keys, work-budget faults and allowance restoration. These checks do not
establish native-host fidelity or arbitrary userdata/finalizer timing.

Private numeric/authored evidence is under
`PluginArtifacts/kontra-diagnostic-fixes/uvi-projection-20261015/`:
`contended-summary.json`, `contended-process-receipt.json`,
`contended-authored-oracle/timings.json`, `contended-pixel-bridge.log`,
`promoted-full-scan.log` and `promoted-freeze-receipt.json`. The quiet wrapper was
not run because its passive gate observed external compiler processes.

Follow-up timing cohorts are homogeneous bound ParamKnobs with ordinary live
parameter getters, and homogeneous plain Menus with two-item tables. These would
isolate the classification cost before those canonical fallback paths; neither
has a measured homogeneous-route overhead claim yet.
