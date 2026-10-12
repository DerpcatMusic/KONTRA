# Grouped diagnostics v1

Key: `kind`, typed `reason`, and `subject` (native parameter/module type or KSP builtin). Counts distinguish `enabled`, `bypassed`, and explicitly `unknown` where historical diagnostic metadata lacks bypass state. Impact ranks are 1 sounding, 2 bypassed parity loss, 3 unknown state. Sort by impact, descending enabled count, descending libraries, then bypassed count and key. Source, slot and target counts are separate quantities.

A group contains the first 16 locations and the total distinct location count. Locations contain a library file path and numeric program/group/slot/zone/target/script indices only. Full unique locations and occurrence counts are in a sidecar using the same key. Neither authored names, script text nor fault text appears in these records. UVI member names are removed from paths. A stable numeric item hash distinguishes member receipts; program is the selected native-reader program index. Census receipts also strip UVI member names before persistence and carry that numeric item ID; a failing-first check covers separate sanitized members. Numeric rack/native-slot/bus/node addresses distinguish insert, send, internal and external slot namespaces. Runtime callback indices are separate from script slots.

First evidence: `/mnt/Windows11/DEV_WORKSPACE/kontra-runs/w12-fidelity/`:
- `grouped-top20-gate.md`: target work queue, sent W15.
- `grouped-top20-all-gate.md`: ranked source/slot/target failures; quantities must not be summed across kinds.
- `grouped-gate.json` + `grouped-gate-locations.json`: all 22 gate items, four remeasured after W15 42062adc and 18 unchanged baseline receipts. Original worker hashes are retained by the native receipt ledger.
- `grouped-top20-corpus-cache.md`, `grouped-corpus-cache.json` + `grouped-corpus-cache-locations.json`: latest historical cached record per path, 1494 items, 11 typed builtin/Lua/init diagnostic groups. No cached item has a native slot inventory. Bypass state is UNKNOWN; mixed/stale revisions are not a current full-corpus DSP census or execution proof.

Morphology group 208 insert 0 fresh in-memory metadata: Filter 33, native version 149 (`0x95`), Gain 0, Cutoff 0.5892040133, Q 0.1704549938. The current native Ladder admission accepts only versions `0x90..0x92`; W15 has the witness. No payload or samples were written.

Runtime: each new group logs once, later occurrences update counts and references, and each load writes one journal summary plus a full metadata sidecar. Live sidecar/report counts update without repeating journal summaries. A failing-first test observed two summaries for two refreshes; the corrected test retains one summary, two occurrences and both callback references. Support exports include sidecars under the existing session retention budget. Native unsupported records without bypass metadata retain UNKNOWN; muted groups are bypassed, executed faults enabled. A preallocated 256-event fault inbox transfers typed callback outcomes off the audio thread; overflow is explicit and the RT producer allocates nothing.

Validation: failing-first duplicate test observed 1024 retained errors; fixed test observes one error, 4099 occurrences, 4098 unique locations, 16 inline locations and all locations in the exported sidecar. A second failing test exposed internal/external slot-zero collisions. Shared scanner/runtime fixture, scanner checks and gate checks PASS. Wrapper final rerun: 12 diagnostics tests PASS, one fault-transport test PASS (zero allocations and explicit overflow), plugin-enabled library test build PASS. Receipt: `grouped-green-final.log` in the directory above. No full plugin or native-audio fidelity claim is made by these checks.

NEXT: recount LFO5 plus filter Q/Gain on W15 42062adc over ec4a79bb and 1e14ef33, then refresh ranked tables. Full-corpus DSP inventory remains UNKNOWN until measured; historical builtin diagnostics are explicitly separate.

## W15 1e14ef33 + 42062adc recount

Exact isolated source `5172a9ef` includes the three original W15 ports, ec4a79bb, 1e14ef33 and 42062adc. Native reader SHA256 `896f8bf7e4bfca1eae2a786d43957fef981b5b9e895a5911c5c1eb6cddbf8a38`; build flags `--features scan,sampler-kontakt/scan`. Four affected items/five programs remeasured; 18 unchanged gate receipts retained. Authored identities and enable states match; zero new dropped slots or routes. The first helper omitted Kontakt's scan feature and produced UNKNOWN inventories; those results were excluded.

| Drops | Before enabled / bypassed | After enabled / bypassed |
|---|---:|---:|
| FX slots | 100 / 1315 | 100 / 1315 |
| Filter slots | 1818 / 4770 | 1818 / 4770 |
| Modulator slots | 18151 / 24478 | 14960 / 24478 |

3191 enabled source slots recovered: LFO5 2693, Constant 9, CC1 163, Script0 163, Velocity 163. Recovered routes: pan 894, pitch 900, volume 899, filterQ 665 (native Filter33 632, Filter70 25, Filter71 8). LFO5 execution losses fall from 3713 to 124; 896 recognized sources still have dropped targets, so LFO5 total remaining drops are 1118 enabled and 2180 bypassed. Accordingly `TargetsDropped/filterCutoff` increases by 896 as `SourceNotExecuted/filterCutoff` falls by 896; this is a reason change, not a new drop.

Gain: no authored Ladder/Daft Gain routes in these four items. The 989 enabled `Gain` losses across the 22 items address UVI Program/Layer/Keygroup/SamplePlayer objects and remain dropped. Native Q/Gain DSP execution has W15's separate witnesses; this metadata receipt certifies admission only. Every recovered slot/route remains approximated with typed `NativeLawUnverified`.

Receipts: `recount-42062adc-{summary,provenance}.json`, `target-summary-after-42062adc.json`, `after-42062adc/`, refreshed `grouped-top20-gate.md`, `grouped-top20-all-gate.md`, `grouped-gate{,-locations}.json` under the directory above. Historical corpus grouping now includes 11 typed groups (builtin, Lua and initialization categories); native DSP coverage there is still zero. Fresh full-corpus native census is in progress in `corpus-42062adc/`, with private member IDs, metadata only and bounded FIFO shards.

NEXT: finish the fresh 1494-item native census and publish the full-corpus ranked table for W15.
