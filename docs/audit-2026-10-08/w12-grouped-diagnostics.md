# Grouped diagnostics v1

Key: `kind`, typed `reason`, and `subject` (native parameter/module type or KSP builtin). Counts distinguish `enabled`, `bypassed`, and explicitly `unknown` where historical diagnostic metadata lacks bypass state. Impact ranks are 1 sounding, 2 bypassed parity loss, 3 unknown state. Sort by impact, descending enabled count, descending libraries, then bypassed count and key. Source, slot and target counts are separate quantities.

A group contains the first 16 locations and the total distinct location count. Locations contain a library file path and numeric program/group/slot/zone/target/script indices only. Full unique locations and occurrence counts are in a sidecar using the same key. Neither authored names, script text nor fault text appears in these records. UVI member names are removed from paths. A stable numeric item hash distinguishes member receipts; program is the selected native-reader program index. Numeric rack/native-slot/bus/node addresses distinguish insert, send, internal and external slot namespaces. Runtime callback indices are separate from script slots.

First evidence: `/mnt/Windows11/DEV_WORKSPACE/kontra-runs/w12-fidelity/`:
- `grouped-top20-gate.md`: target work queue, sent W15.
- `grouped-top20-all-gate.md`: ranked source/slot/target failures; quantities must not be summed across kinds.
- `grouped-gate.json` + `grouped-gate-locations.json`: all 22 gate items, four remeasured after W15 ec4a79bb and 18 unchanged baseline receipts. Original worker hashes are retained by the native receipt ledger.
- `grouped-top20-corpus-cache.md`, `grouped-corpus-cache.json` + `grouped-corpus-cache-locations.json`: latest historical cached record per path, 1494 items, nine typed builtin diagnostic groups. No cached item has a native slot inventory. Bypass state is UNKNOWN; mixed/stale revisions are not a current full-corpus DSP census or execution proof.

Morphology group 208 insert 0 fresh in-memory metadata: Filter 33, native version 149 (`0x95`), Gain 0, Cutoff 0.5892040133, Q 0.1704549938. The current native Ladder admission accepts only versions `0x90..0x92`; W15 has the witness. No payload or samples were written.

Runtime: each new group logs once, later occurrences update counts and references, and each load writes a summary plus a full metadata sidecar. Support exports include sidecars under the existing session retention budget. Native unsupported records without bypass metadata retain UNKNOWN; muted groups are bypassed, executed faults enabled. A preallocated 256-event fault inbox transfers typed callback outcomes off the audio thread; overflow is explicit and the RT producer allocates nothing.

Validation: failing-first duplicate test observed 1024 retained errors; fixed test observes one error, 4099 occurrences, 4098 unique locations, 16 inline locations and all locations in the exported sidecar. A second failing test exposed internal/external slot-zero collisions. Shared scanner/runtime fixture, scanner checks and gate checks PASS. Wrapper final rerun: 11 diagnostics tests PASS, one fault-transport test PASS (zero allocations and explicit overflow), plugin-enabled library test build PASS. Receipt: `grouped-green-final.log` in the directory above. No full plugin or native-audio fidelity claim is made by these checks.

NEXT: recount LFO5 plus filter Q/Gain on W15 42062adc over ec4a79bb and 1e14ef33, then refresh ranked tables. Full-corpus DSP inventory remains UNKNOWN until measured; historical builtin diagnostics are explicitly separate.
