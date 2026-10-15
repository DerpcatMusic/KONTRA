# UVI property-inventory experiments

Full UI export remains costly after ready commands can reach audio before
publication. The earlier rotating real-instrument sample attributed 34.08% of
sampled widget CPU to documented-property inventory; that is a subset with probe
overhead, not an exact fraction of total export CPU.

The raw iterator candidate is selected for production after the regression and
authored pixel oracles below passed. Canonical iteration and the
rejected classifier remain private, compile-time test specializations. There
is no runtime setting, UI revision cache, skipped hidden control or cached live
parameter value.

## Exact classifier: not accepted

The first candidate recognizes the 21 documented property names before scanning
the 63 modeled names. It retains the existing consumed-property checks, modeled
guard, raw iterator, error cutoff and unsupported-row order.

One optimized `ci` executable compared eight alternating reference/candidate
pairs for each authored 10,145-widget fixture. Owner profiling was disabled.
Each timed interval contains the full export; exact model comparison and
serialization occur afterward. Linux caller-thread CPU uses `ThreadCPUTime`.

| Authored property mix | Median paired CPU ratio | Candidate wins | Pair range |
| --- | ---: | ---: | ---: |
| Ordinary modeled/private fields | 0.96966 | 5 / 8 | 0.764–1.089 |
| One documented field on every fourth widget | 0.99582 | 5 / 8 | 0.828–1.055 |
| All 21 documented fields | 0.94650 | 6 / 8 | 0.860–1.045 |

Ratios are candidate/reference. The sparse case showed no material median
improvement, and every case included regressions. The run observed no compiler,
but an external workload reached 602.5% CPU. Separate unpaired medians suggested
larger gains and are unsuitable for acceptance under this drift. This receipt
does not establish a useful general win; the classifier was not promoted.

The frozen executable SHA-256 was
`f1678a4778d6db8439117e90dabf1475c6370d28155d74a43009c5d53c71de04`.
Private numeric evidence lives under
`PluginArtifacts/kontra-diagnostic-fixes/uvi-inventory-20261015/` in
`paired-summary.json`, `process-receipt.json` and `authored-oracle/timings.json`.
All 48 timed exports matched the complete reference Interface. The authored
transport files were byte-identical, SHA-256
`e5057e012374c70d4238c9fbe82d25aab3dce29957dc8d61ab20cf7b6d4bf409`.
These are synthetic models, not installed-library source or artwork.

## Independent iterator candidate

The next test specialization changes `pairs::<String, Value>().flatten()` to
`for_each::<String, Value>`, retaining the canonical membership rules. In pinned
mlua 0.12.2 with Luau, both use the same integer-indexed `lua_rawiter`. The latter
keeps one stack guard instead of constructing and cloning each continuation key
through the iterator. The candidate retains `Value` conversion; it does not
discard references through a unit-value shortcut.

The terminal conversion error is deliberately ignored, preserving the original
first-malformed-key cutoff. This includes non-UTF8 keys, noncoercible key types
and numeric-to-string coercion. Neither path calls `__pairs`. No statement about
other mlua backends follows from this Luau-only experiment.

The independent paired run used executable SHA-256
`9e0dcb4837051c4845e375a57f8ffa869d253b8c409e055bab37ba5cec99d46b`.
It again compared eight alternating pairs per fixture, with profiling disabled
and exact complete-model equality after every timed export.

| Authored property mix | Median paired CPU ratio | Candidate wins | Pair range |
| --- | ---: | ---: | ---: |
| Ordinary modeled/private fields | 0.80516 | 8 / 8 | 0.768–0.825 |
| One documented field on every fourth widget | 0.82223 | 8 / 8 | 0.791–0.838 |
| All 21 documented fields | 0.72493 | 7 / 8 | 0.535–1.082 |

The first two cases show an 18–20% reduction in paired full-export caller CPU
for these synthetic inputs. The warning-heavy stress case retains one slower
pair and substantial variance. No compiler was observed, but unrelated active
processes exceeded a combined 903.8% CPU in one monitoring interval. That is a
lower bound from processes above the monitor's 20% threshold. Absolute times,
tail latency and a real-instrument improvement are not established by this run.
Do not compare its absolute medians with the earlier classifier cohort.

All 48 timed interfaces matched. The reconstructed authored transport retained
the same complete-model hash as the classifier run. Private evidence is in
`foreach-paired-summary.json`, `foreach-process-receipt.json` and
`foreach-authored-oracle/timings.json` under the same DEV-drive directory.
Full UVI suites and the root pixel hook provide the acceptance boundary below;
the paired synthetic result alone is not native or all-library parity.

With the iterator selected in the ordinary production entry point, the final
serialized optimized `ci` library suites passed: default features 163 passed,
10 ignored; `scan` 173 passed, 11 ignored. Both had zero failures and used
`--locked --offline -j2 --test-threads=1`. This includes the ready-command
delivery, full-queue/acknowledgement and all authored export oracles. Their
private logs are `foreach-final-default-lib.log` and
`foreach-final-scan-lib.log`. The final 0.3.477 root/`shots` cohort passed
785 tests with 53 ignored and zero failures. The separately invoked authored
pixel hook passed: complete 10,145-widget Interface, resolved geometry and exact
PNG bytes in both Bitmap and Vector presentations matched. The default CLI
parser check also passed. A new real-bank performance receipt remains pending;
no source/renderer parity beyond these checks is claimed.

## Semantic and render oracles

Authored comparisons retain complete Interface equality across all documented
keys, widget kinds and value types; live bound getter ordering; raw and nested
edits; exact commands, control identities, saved state and findings; malformed
key traversal; and work-budget faults/restoration. No authored budget is raised.
The direct reentry fixture also runs Lua and mutates the iterated raw table
between visits. These tests do not establish arbitrary userdata/finalizer timing.

The UI IR has no serialization dependency. A test-only transport represents only
this benchmark's authored Label/style/page shape and must reconstruct the
complete Interface exactly before writing. The root test rejects unknown fields
and uses the existing production preview renderer for resolved geometry and exact
Bitmap/Vector PNG comparison. This limited synthetic pixel oracle does not prove
native Falcon fidelity, asset rendering, live menu behavior or real-bank timing.

The benchmark and pixel hook are ignored by default. Both require an explicit
private DEV-drive fixture directory for artifact exchange; ordinary plugin and
inspector paths write no artifacts from this harness.
