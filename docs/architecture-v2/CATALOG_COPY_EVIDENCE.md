# Catalog-copy semantics, timings and initialization work

The scalar catalog copier in `sampler-uvi/src/script.rs` enters one private Lua
function per `native.definitions` request. Previously Rust entered Lua once for
each scalar definition. Both paths return fresh mutable definition tables: the
numeric entry and its name alias identify the same table, while another element
gets an independent copy. The private helper captures the original `table.clone`
and `table.create` functions before authored scripts execute. Retained XML fields
continue through the existing path and keep their provenance.

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

## Current Augmented Orchestra profile

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
wide-fixture delta, not an explicit scalar-row counter. The unchanged retained-XML
field path still clones one template per field through Rust→Lua calls. It is the
next concrete batching candidate; the wide scalar fixture alone cannot establish
its cost or justify caching mutable public descriptors by module kind.

If a real authored initializer approaches the allowance because of this private
copying loop, the appropriate follow-up is scoped private-helper accounting:
charge a bounded catalog operation, retain authored call/backedge charging and
restore accounting with an RAII guard on every success/error path. Authored
callbacks must never execute inside such an exemption. That requires explicit
work-budget/error-path regressions before implementation; increasing the global
allowance would obscure the regression.
