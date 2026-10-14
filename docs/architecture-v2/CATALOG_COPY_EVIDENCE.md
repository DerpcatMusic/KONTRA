# Catalog-copy semantics, timings and initialization work

The catalog copier in `sampler-uvi/src/script.rs` enters one private Lua function
per `native.definitions` request, copying scalar declarations and current retained
XML field templates. Previously Rust entered Lua once for each retained field;
the earlier scalar change removed that boundary for scalar declarations.
Both paths return fresh mutable definition tables: the
numeric entry and its name alias identify the same table, while another element
gets an independent copy. The private helper captures the original `table.clone`
and `table.create` functions before authored scripts execute. Retained XML fields
keep their provenance, order, numeric IDs and name aliases. Current field lists,
inferred field types and values are read again for every catalog request; no
public descriptor or element inventory is shared by module kind.

The `catalog_copies_keep_aliases_inventory_and_element_local_mutations` regression
and the complete `host_parameters` suite passed 22/22. This covers mutations,
numeric/name alias identity, independent elements, inventory and prelude behavior;
it is implementation regression evidence, not native-host equivalence.

## Three before / three after measurements

The local `catalog-1000-wide.uvip` fixture authors 1,000 `DigitalEq` inserts and
reads each 134-entry scalar catalog once, asserting the inventory and `Bypass`
alias. It reads no sample payloads and runs no audio, gestures or DAW. Each side
uses a fresh optimized inspector process with ordinary initialization, no line
coverage and the explicit source-free profiler. The frozen pre-helper executable
and current executable run the same fixture. This is an ordered, warm-machine
microbenchmark; there is no OS-cache flush or confidence-interval claim.

The local numeric receipt is
`PluginArtifacts/kontra-diagnostic-fixes/catalog-wide-paired-evidence.json`.
All six records report successful initialization, the same 1,000 definition API
calls and 1,005 graph nodes. Inclusive nested clocks must not be summed.

| Measured scope | Before median (range), ms | After median (range), ms |
| --- | ---: | ---: |
| Definition API | 146.685 (144.621–147.719) | 50.405 (50.040–51.980) |
| Host constructor | 167.077 (165.126–170.076) | 71.750 (71.607–72.918) |
| Script body and requires | 148.133 (146.123–149.264) | 52.067 (51.690–53.718) |
| Translation and initialization | 225.566 (222.075–233.059) | 129.889 (129.851–130.155) |
| Inspector wall time | 537.879 (530.589–545.487) | 430.488 (429.946–434.058) |

The measured definition API median decreased 65.6%; the whole inspector median
decreased 20.0%. These figures describe this metadata fixture, not Augmented
Orchestra startup, plugin first-frame time or the KONTRA audio CPU meter.
Stack-sampling overhead was 0.054–0.061 ms before and 0.058–0.064 ms after.
Reported Lua memory was 107.6–109.9 MB before and 91.3–91.4 MB after; that is
the retained measurement point, not peak allocation.

## Work accounting

The same fixture uses 7,158 VM checkpoints before and 277,159 after: an increase
of 270,001, despite faster measured execution. Moving the loop into Lua exposes
the private helper's calls and backedges to the existing VM interrupt counter.
Checkpoint counts therefore measure a deterministic work policy, not CPU cost.

The default initialization allowance is `1 << 25` (33,554,432), shared across
script bodies, restore, callbacks and spawned work. `Shared::arm` does not replenish
it while initialization is active. The after-fixture checkpoints alone are 0.826%
of that allowance. Explicit deferred/coroutine work charges also use the ledger,
so checkpoints alone do not prove the remaining budget. Graph nodes and depth
have separate limits of 262,144 and 192; they are not the VM work counter.

The prior ordinary Augmented Orchestra Pizzicato 2 profile reported 12,972,346
VM checkpoints and 86,871 graph nodes before this private copier. Its API and
wall-clock observations came from different workload/cache conditions than the
microbenchmark; they cannot establish a real-bank speed ratio. A current
cache-read-only, ordinary-constructor profile below records the exact numeric
`uvi_lua_work` ledger, final UI-property counts and asset/render outcomes.
No budget has been raised or private-helper work exempted on this evidence.

## Scalar-copy Augmented Orchestra profile

One frozen optimized process inspected `V Strings Pizzicato 2.uvip` with
`--ui-only --profile-init --ephemeral-render`, without line coverage, a plugin,
audio Runtime or DAW. Source and PNGs stayed in RAM. The mount sandbox remained
read-only and `KONTRA_SCAN_ACTIVE=1` stayed enabled; the explicit
`KONTRA_UFS_CACHE_READ_ONLY=1` override allowed only validated directory-cache
lookup. Both `TMPDIR` and the seeded `XDG_CACHE_HOME` were on the dev drive.
No resource payloads were cached. The compact local numeric receipt is
`PluginArtifacts/kontra-diagnostic-fixes/catalog-final-ao-evidence.json`; it omits
script bodies, authored Lua stack labels, artwork and PNGs.

The frozen binary's SHA-256 is
`a02296d73dfd94c649a06e3a77be7a5c52ae5279a2619b25b51c0014c4a62b1f`.
Its loadSample implementation is the prior unsupported service: audio and impulse
decoders were not attached, samples were not bound, and dynamic replacement or
every audio-dependent callback path is not established by this UI profile.

The initializer completed with no retained initialization faults, no work-budget
exhaustion and no elapsed-deadline expiry. Its exact ledger charged 13,093,011
units and retained 20,461,421 of 33,554,432 units (61.0%). VM checkpoints were
13,093,007, up 120,661 from the previous ordinary receipt, not the wide fixture's
39-fold increase. Graph size remained 86,871 at depth 6. Lua memory was 410.0 MB
at the initializer ledger and 439.9 MB at the later profile readback, below the
1,536 MiB host allowance; neither is a peak-allocation measurement. There is no
observed need to change the work allowance or exempt the private helper here.

| Current measured phase | Inclusive wall time, ms |
| --- | ---: |
| Validated metadata cache lookup | 116.0 |
| Container, program and scripts | 875.7 |
| Translation and initialization | 8,298.8 |
| Host constructor (nested) | 5,834.3 |
| Program object graph (nested) | 1,072.2 |
| Script body and requires (nested) | 4,529.2 |
| Definition API, 60,034 calls (nested) | 1,894.2 |
| Module compilation, 19 modules (nested) | 153.8 |
| Post-init overlays and IR translation (nested) | 2,095.4 |
| UI model export | 292.8 |
| Production UI render and witnesses | 567.6 |
| Inspector process | 10,792.7 |

The profiler retained 1,964 safepoint observations, measured 15.28 ms of stack
sampling overhead and dropped no frame records. Machine/compiler work could
overlap this run. The prior ordinary receipt had a cold directory path and
different contention; the numbers above are not a controlled real-bank speedup
ratio or plugin first-frame timing.

Both 720×480 initial-state CPU witnesses completed. All 89 requested assets were
found: 84 image decodes and five font loads succeeded, with no pending, missing,
ambiguous, read/decode, preparation-budget or dropped-trace outcomes. There were
no named-font fallbacks. Initial bitmap preparation took 386.7 ms. Each
paint/encoding pass took about 80 ms: PNG encoding was 48.3–48.9 ms, CPU raster
22.1–22.5 ms, view construction 3.6–4.2 ms and two-frame layout 1.13–1.15 ms.
PNG encoding is an inspector-witness cost, not product UI painting.

The actual initialized model retained 10,145 widgets, 101 initially visible,
541 declared assets and 4,282 styles. Recorded unsupported UI properties decreased
from 305 to 81, and initially visible records from 29 to 20:

| Remaining property gap | Total | Initially visible |
| --- | ---: | ---: |
| AudioMeter element ownership | 8 | 8 |
| AudioMeter.displayAsMono | 8 | 8 |
| AudioMeter.0dBColour | 4 | 4 |
| Table.fillStyle | 8 | 0 |
| Menu.hierarchical | 53 | 0 |

The overall report remains partial for these gaps and other conversion findings.
Found assets do not prove native behavior, faithful meter taps, later page states
or Kontakt/Falcon pixel equivalence. No authored overlapping/offscreen controls
were moved.

The checkpoint increase is consistent with two private-loop checkpoints per
definition request and only 296 total scalar rows across those requests:
`1 + 2 × 60,034 + 2 × 296 = 120,661`. This is an inference from the helper and
wide-fixture delta, not an explicit scalar-row counter. At that frozen checkpoint,
the retained-XML field path still cloned one template per field through Rust→Lua
calls. The next measurements address that specific boundary; the wide scalar
fixture alone cannot establish its cost or justify caching mutable public
descriptors by module kind.

If a real authored initializer approaches the allowance because of this private
copying loop, the appropriate follow-up is scoped private-helper accounting:
charge a bounded catalog operation, retain authored call/backedge charging and
restore accounting with an RAII guard on every success/error path. Authored
callbacks must never execute inside such an exemption. That requires explicit
work-budget/error-path regressions before implementation; increasing the global
allowance would obscure the regression.

## Retained XML field batching

`script/catalog_copy.rs` now copies the scalar schema and a freshly gathered list
of retained field templates in the same private Lua entry. Scalar-name aliases in
the private schema avoid scanning the scalar catalog for each current attribute.
Only fixed metadata templates are reused, keyed by field name and inferred type;
they contain no current value, inferred bounds or fabricated default. The helper
assigns IDs to the copies, never the private templates. It preserves numeric/name
alias identity and suppresses duplicate scalar names as before.

Five focused helper tests and all 22 `host_parameters` tests pass. They cover
current field addition/removal/type changes, live reads and scalar defaults,
independent element mutations, captured original cloning functions, numeric IDs
and aliases, interrupt propagation and memory-allocation failure. A native-copy
test exhausts the existing live host allowance, verifies that no partial public
catalog was cached, then uses a new callback allowance to copy successfully.
There is no private-helper exemption or global budget increase.

The local authored `catalog-1000-retained.uvip` fixture has 1,000 `OnePole` inserts,
each with four scalar fields and 32 retained fields of alternating numeric and
string types. Every element checks the 36-entry inventory, aliases, types,
provenance, live `Freq`/retained values and independent mutations. Its SHA-256 is
`f87148fca881ea5de71892165be1557edd231f18f2ad126c51e459c3534a4968`.
Three interleaved before/after pairs use fresh `dsp-dev` processes with ordinary
initialization, no line coverage, the same explicit profiler and read-only mount
sandbox. Each run reports 1,000 definition calls, 2,000 parameter reads, 1,005 graph
nodes, successful initialization and no diagnostics. Team heavy work was held
during this fixture window; external contention and OS caches were not controlled.

The compact numeric receipt is
`PluginArtifacts/kontra-diagnostic-fixes/catalog-retained-paired-evidence.json`.
The before binary is the frozen scalar-copy binary above. The after binary is
`74e9f596e65920ad70011c255ae46f6235b5777798a71d7073623d589b5bee8b`.

| Measured scope | Before median (range), ms | After median (range), ms |
| --- | ---: | ---: |
| Definition API | 60.845 (59.347–63.266) | 34.057 (33.028–34.446) |
| Host constructor | 88.038 (87.004–90.003) | 63.291 (61.497–63.432) |
| Script body and requires | 67.776 (66.240–70.631) | 42.706 (41.665–43.244) |
| Translation and initialization | 102.427 (101.264–103.526) | 76.231 (75.329–77.499) |
| Inspector process | 114.491 (114.033–117.248) | 88.089 (86.774–88.435) |

This fixture's definition API median decreased 44.0%, and inspector median
23.1%. Work/checkpoints increased from 115,159 to 179,159: exactly 64,000 extra
units for the 32,000 retained fields. All after runs kept 33,375,273 of 33,554,432
units (99.47%); no deadline expired. Lua memory at the initializer ledger was
23.1–23.2 MB before and 20.7–20.9 MB after, not peak memory. Sampling overhead was
0.038–0.044 ms before and 0.032–0.037 ms after. Inclusive phases overlap.

XML-heavy catalog requests benefit from fewer Rust→Lua entries. Tiny or
scalar-only catalogs offer less batching work; the fresh retained-template list
is an additional temporary allocation when retained fields exist. The allocation
failure test protects that path, but the reported memory points do not establish
peak allocation or every metadata workload. Very large field inventories can
still exhaust the existing deterministic allowance; faster wall time does not
remove that policy. Shared mutable descriptors, stale per-kind element snapshots
and unbounded template inventories are not substitutes for these semantics.

## Retained-copy Augmented Orchestra validation

A final frozen after-binary inspection used the same Pizzicato 2 member and
`--ui-only --profile-init --ephemeral-render`, read-only cache override, seeded
dev-drive cache and owned dev-drive `TMPDIR`. Its compact receipt is
`PluginArtifacts/kontra-diagnostic-fixes/catalog-retained-ao-evidence.json`.
The first process completed but a summary-projection key error lost its numeric
receipt; one repeat retained the evidence. Parent metadata-only `cargo check`
could overlap the retained run. These are single real-bank observations, not a
controlled before/after speed ratio. Source, authored stack labels and artwork
were excluded from the persisted receipt.

Initialization completed without faults, budget exhaustion or elapsed expiry.
The exact ledger charged 14,053,219 units, leaving 19,501,213 (58.1%) of the
unchanged 33,554,432 allowance. VM checkpoints were 14,053,215, an increase of
960,208 from the scalar-copy receipt. That delta is consistent with two extra
checkpoints per 480,104 copied retained rows; the row count is inferred from the
loop and delta, not a separate manifest measurement. Graph size/depth stayed
86,871/6. Lua memory was 381.0 MB at the ledger and 381.4 MB at profile readback;
neither measures peak memory. There is no observed work-budget regression here.

| Final measured phase | Inclusive wall time, ms |
| --- | ---: |
| Validated metadata cache lookup | 114.3 |
| Container, program and scripts | 446.1 |
| Host constructor (nested) | 2,891.3 |
| Script body and requires (nested) | 2,192.4 |
| Definition API, 60,034 calls (nested) | 640.0 |
| Program object graph (nested) | 567.5 |
| Post-init overlays and IR translation (nested) | 1,954.8 |
| Translation and initialization | 4,988.8 |
| UI model export | 284.3 |
| Production UI render and witnesses | 625.7 |
| Inspector process | 6,919.8 |

Sampling retained 1,068 observations with 2.99 ms measured overhead and zero
dropped frame records. Definition calls and parameter-read/write counts match the
earlier scalar-copy receipt. The final model still has 10,145 widgets, 101 visible,
541 declared assets, 4,282 styles and 81 unsupported properties, 20 visible; the
property categories are unchanged from the table above. Both 720×480 witnesses
completed, all 89 requested assets succeeded (84 image decodes and five fonts),
and there were no pending assets, missing/decode failures, preparation limits,
named-font fallbacks or dropped asset traces.

Bitmap preparation took 446.3 ms. Each paint/encoding pass took about 80 ms,
including 48.7–49.3 ms PNG encoding and 22.1–22.2 ms CPU rasterization. The cache
lookup is nested in the container clock; helper/API clocks are nested in script
initialization. This identifies remaining translation/export/preparation work,
without attributing witness PNG encoding to the shipping UI. The frozen sample
service still did not decode or bind dynamic samples; native parity, later
gestures, audio-dependent callbacks and the KONTRA audio meter remain unverified.
