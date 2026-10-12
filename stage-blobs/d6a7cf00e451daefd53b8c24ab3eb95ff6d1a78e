# UVI initialization wall phases

Branch v2/fix-uvi; base 0c69feea. No Lua runtime change: eager Luau native
compilation remains enabled. The rejected vendored-mlua/native-policy experiment
is parked outside the repository and is not part of this change.

## Measurement and correction

Numeric KONTRA_AUDIT_LOAD spans now separate XML/VM/host installation, graph,
root/module compilation, script body, restoration, onInit and UI publication.
Scan-only host API counters sum actual wall intervals without per-call output.
Nested rows are subsets of parent phases and must not be added to them.

The earlier 50 ms GDB sample attributed about 40% to codegen; wall markers
supersede that inference. Coline/Diamond module compilation is only 74/67 ms,
while full descriptor construction takes 3513/3585 ms. Native/interpreter A/B
already showed little init gain from disabling native but about 3–5% callback
CPU benefit on Diamond/Antartide, so eager native is retained.

These are optimized targeted production translate/attach/stream probes, not a
new corpus collector or frozen-scanner census. Each case runs in a fresh process;
OS page cache and concurrent activity are uncontrolled. Before markers already
include the approved coherent ready snapshot, isolating the parameter change.
The historical untouched frozen Coline scanner load was 8533 ms, Lua-init 7419 ms;
its clock/path is different and is not treated as a paired speedup below.

| Preset | Admission/load ms before → after | Lua init + ready ms before → after |
|---|---:|---:|
| coline | 8136 → 5259 | 6790.2 → 4071.0 |
| diamond | 8010 → 5299 | 6815.4 → 4142.0 |
| antartide | 16762 → 5905 | 14164.0 → 4192.6 |
| alto-flute | 1635 → 1203 | 205.0 → 194.7 |

| Phase ms before → after | Coline | Diamond | Antartide | Alto Flute 2 |
|---|---:|---:|---:|---:|
| XML parse | 45.9 → 46.3 | 46.8 → 67.3 | 96.3 → 46.7 | 9.6 → 9.3 |
| VM | 0.4 → 0.4 | 0.4 → 0.5 | 0.5 → 0.4 | 0.4 → 0.4 |
| Host/prelude installation | 5.0 → 5.3 | 5.0 → 6.9 | 8.2 → 5.1 | 5.2 → 5.5 |
| Graph construction | 1178.6 → 1165.1 | 1217.3 → 1251.5 | 2425.5 → 1223.4 | 96.9 → 96.9 |
| Script body (includes module compile/API) | 4861.5 → 2116.1 | 4899.8 → 2155.2 | 10723.6 → 2167.3 | 73.4 → 63.1 |
| Module compile (nested in body) | 74.0 → 67.4 | 67.0 → 68.3 | 153.0 → 67.4 | 58.4 → 57.3 |
| Full parameter catalogs (nested in body) | 3513.0 → 819.3 | 3585.1 → 868.4 | 7771.1 → 838.9 | 8.9 → 0.0 |
| Restore | 43.8 → 47.5 | 48.3 → 55.3 | 37.6 → 34.9 | 1.3 → 1.2 |
| onInit | 5.9 → 5.8 | 7.7 → 8.7 | 8.6 → 5.9 | 0.0 → 0.0 |
| UI save | 23.9 → 22.3 | 18.5 → 18.5 | 39.6 → 23.2 | 0.7 → 0.6 |
| One UI snapshot | 577.2 → 604.4 | 523.0 → 526.6 | 760.6 → 629.1 | 16.2 → 16.4 |

Antartide's before pass experienced roughly twice the normal graph/body wall
time. The other AO before passes and all after passes had graph near 1.2 s;
its large total delta must not be extrapolated as a stable percentage.

## Source port and preserved behavior

Port from v1 4bffbb18:src/uvi/host.rs:1213–1288: resolve named parameters and
validate their scalar types directly from retained state. Adapt this to v2's
existing typed catalog, ranges, defaults and engine bindings. Named reads,
existence tests, writes, parameter counts and numeric ID resolution no longer
force construction of an entire catalog. Explicit parameterDefinitions still
returns the complete ordered typed catalog, including retained XML fields.
Already exposed descriptor tables retain their existing semantics.

Adapt v1 host.rs's Lua-managed inventory to hold private scalar descriptor
schemas. Native table.clone produces independent public descriptor tables;
field mutation in one element cannot affect another. Retained XML metadata is
cloned from private per-name/type templates without inventing ranges/defaults.
Full catalog calls fall from 82962 to 58482 for Coline and 84514 to 60034 for
Diamond/Antartide; unavoidable explicit requests become much cheaper.

Port v1 4bffbb18:src/uvi/worker.rs:1880–1905: capture one initialized interface
and reuse it for ready publication. Control values come from that same typed
IR. Save callbacks run before the capture; automatic widget values are saved
after the callback so a mutating onSave cannot expose stale controls or state.
Initial Lua interface captures fall from three to one, live publication from
two to one. The failing-first regression observed bridge 0.25 vs ready report
0.75; both and persisted widget state now agree at 0.75.

## Frozen v1 successful-load comparison

Used the checked, unmodified frozen v1-uvi-onset-probe based on product4bffbb18.
Alto Flute 2 is admitted, observed ready 5355 ms and first output5382 ms in this
pass; its preliminary read/program work took2643 ms. Worker trace phases:

| v1 worker phase | ms |
|---|---:|
| Bank reopen |21.2|
| Program decode |42.1|
| Graph diagnosis/preflight |104.1|
| Resource/full PCM preparation |2241.6|
| Module resources |0.7|
| Combined Lua init + initialized UI |80.9|
| Renderer setup |122.8|

The v1 frozen trace combines VM/host graph/API/script/init/snapshot work in one
Lua-init span; individual subphases are unavailable, not zero. V2's matching
aggregate is205 ms before and195 ms after (rounded), still worse than v1's81 ms.
The v2 streamlined resource/head path and v1 full-PCM path are different; their
whole-load clocks cannot establish every-metric parity. The three AO presets
are rejected by v1 before equivalent Lua work, so no successful AO A/B exists.

## Checks and limits

Both targeted regressions failed before their fixes. Parameter tests cover
range clamping, rejected scalar mismatches, named and numeric IDs, invalid IDs,
retained string fields, complete catalogs and independent descriptor mutation.
The UI tests cover edits, saved state and coherent ready values.

Production bank XML/module bytes are read before the Lua deadline is armed.
A targeted Bartók test delays completed resource preload by21 s, then requires
successful script attachment within the20 s Lua budget with over5000 widgets.
This simulates the slow-disk boundary; it is not a flushed-OS-cache benchmark,
and arbitrary blocking custom Files implementations are not covered.

No full suite,660 sweep, native-host fidelity certification, product release
or install was run. W0's next batch owns the shared scanner/regression gate.
Graph construction remains the next large Lua cost; v1's faster successful
Flute Lua-init aggregate is still an open performance gap.

## Follow-up: v1 leaf collection construction

Ported `4bffbb18:src/uvi/host.rs`'s omission of unused leaf collections and
shared metatable handles into the Luau graph builder. Requested empty lists
are materialized once, per element; public indexing, parent links, canonical
child order and `mods` identity remain intact. Raw enumeration no longer finds
an unused list before its first access. Authored collections remain eager.
Aggregation uses the already constructed collections instead of nine Lua
lookups per element. Synthesis lists are eager on synthesis owners and lazy
on other leaves; unusual authored synthesis children remain present.

Same optimized profiling artifact configuration, Alto Flute 2, consecutive
OS-warm reads (not cache-flushed cold reads), milliseconds:

| Phase | baccabfc | v1 graph port |
| --- | ---: | ---: |
| XML | 9.59 | 9.51 |
| VM + host install | 5.80 | 5.63 |
| Graph | 97.05 | 42.66 |
| Scripts, including nested compilation | 63.82 | 64.54 |
| Nested module compile (included above) | 56.96 | 57.53 |
| Name discovery (included in module compile) | not separated | 1.08 |
| Ready interface | 15.72 | 15.89 |
| Combined Lua + ready | 193.46 | 139.58 |

V1's frozen successful Flute combined Lua stage remains 80.89 ms. This port
reduces but does not close that gap. Eager native Luau compilation remains
unchanged, as instructed; v1's Lua 5.1 interpreter is not ported. The new name
discovery subphase rules out host name scanning as the main compilation cost.
The synthetic regression failed before the port, and all 13 host parameter
checks pass afterward. The sampler-uvi scan-feature area compile passes.
