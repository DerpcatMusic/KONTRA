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

## Recovered whole-manifest receipt, 2026-10-09

All 1494 worker receipts survived the restart. Final aggregation establishes **834 complete Kontakt inventories and 660 UNKNOWN UVI inventories**, not 1494 successful inventories. Every UVI worker exited 1 with typed `native-slot-census-failed`; all 26 referenced bank files exist. The failure is at our reader/worker boundary; no missing-user-data verdict or precise cause is established. The provenance status is corrected to `partial-inventory`. No protected-reader, Wine or native-host retry was run.

The recovered report has 163 typed groups and 3,706,447 unique references in the full 32 MiB compressed sidecar. A bounded-memory, 50-item FIFO aggregation replaces the interrupted eager aggregation; its self-check matches the common collector exactly for counts, ranking, capped locations and full unique references. Shards paused when quiet requests appeared. Sidecar totals equal group totals; numeric item identities are unique. Source remains `5172a9ef`, through W15 `42062adc`; these are metadata admission receipts, not the latest release or native PCM certification.

Measured Kontakt loss counts, enabled/bypassed: FX slots **459/1037**, filter slots **1799/545**, modulator source slots **158209/2685**, target routes **222757/18054**. These four quantities are different units and must not be added. The native target losses form 55 groups. The separate 22-item gate ledger retains its 80 target-loss groups, including its measured UVI baseline.

| Native target reason / parameter | Enabled | Bypassed | Libraries |
|---|---:|---:|---:|
| TargetsDropped / intensity | 64974 | 0 | 1 |
| TargetsDropped / frequency | 37128 | 0 | 1 |
| SourceNotExecuted / filterCutoff | 14047 | 0 | 2 |
| SourceNotExecuted / filterQ | 14047 | 0 | 2 |
| SourceNotExecuted / pan | 14047 | 0 | 2 |
| SourceNotExecuted / frequency | 13923 | 0 | 1 |
| SourceNotExecuted / pitch | 13923 | 0 | 1 |
| TargetsDropped / eqGain1 | 10396 | 0 | 1 |
| SourceNotExecuted / volume | 9406 | 0 | 2 |
| TargetsDropped / eqGain2 | 7157 | 0 | 5 |
| SourceNotExecuted / intensity | 4641 | 0 | 1 |
| TargetsDropped / filterCutoff | 4612 | 0 | 7 |
| TargetsDropped / filterQ | 2370 | 0 | 2 |
| TargetsDropped / startPhase | 1920 | 0 | 1 |
| TargetsDropped / formantTalk | 1509 | 0 | 1 |
| TargetsDropped / eqGain3 | 1265 | 0 | 5 |
| TargetsDropped / ahdsr_attack | 1069 | 0 | 4 |
| TargetsDropped / shaper | 647 | 0 | 2 |
| TargetsDropped / bitdepth | 572 | 0 | 1 |
| TargetsDropped / downsample | 572 | 0 | 1 |

W15 received this final queue, superseding the provisional 123-item prefix. Full artifacts beside the earlier receipts: `grouped-corpus-native.json`, `grouped-corpus-native-locations.json.gz`, `grouped-top20-corpus-native.md`, `grouped-top20-all-corpus-native.md`, and `grouped-corpus-native-resume.log`. The older plain sidecar belongs to the partial prefix; only the compressed sidecar named by the final report is authoritative. `corpus-ranked-stream.py` and its resumable runner retain the analysis and assertions in the receipt directory.

NEXT: recount admission-changing W15 EQ/source routes; prioritise the NI family protocol in `w12-priority-family.md`. UVI whole-manifest inventory remains UNKNOWN pending an admitted reader path.
