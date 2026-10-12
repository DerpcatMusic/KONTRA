# Shared script text sizing

Base: exact integration 3f49a9a109220c95546596e37204b5b2faa829b7; measured candidate bc316448cc48d7794a6e90bedea1fb475e241ba2. Cache/schema work is excluded. Both use the current iterative UI blob 7faa02c0d36a3826034d72e8bd8df361c1562457.

Prepared script instances now retain compact Arc<str> seeds. Each live generation constructs its own fixed-capacity mutable Text buffers on the control thread. This removes the duplicate full-width initial text payload without copy-on-write allocation during callbacks. Control-thread plan submission uses the same constructor.

| Preset | Load RSS before / after (MiB) | Editor RSS before / after (MiB) | Trimmed RSS before / after (MiB) | HWM before / after (MiB) |
|---|---:|---:|---:|---:|
| conflux | 263.48 / 254.67 | 338.82 / 330.36 | 292.24 / 285.86 | 337.81 / 329.82 |
| pacific | 217.38 / 219.14 | 240.36 / 241.43 | 233.41 / 234.41 | 241.10 / 242.04 |
| analog | 1054.20 / 1047.81 | 1086.40 / 1067.70 | 1071.20 / 1037.97 | 1086.90 / 1078.77 |

These are debug production-path RSS probes, not quiet load/onset or release acceptance. Allocator reclamation can shift editor RSS beyond the exact text payload saving. Each probe opens the editor and renders C4 audio. Stream heads remain lazy (0 bytes), and the stream pool remains 25,165,824 bytes.

Validation: sharing/independent-bank unit test; both ops integration tests, including allocation-free text/store/control callbacks; all six allocation-free plan-transfer tests; root lib --no-run; all six before/after preset probes. Numeric receipts are ~/.cache/kontakto-fix-load/rss-owners/3f49-{before,text}-rss.json; frozen binaries and BUILD.json hashes are under /mnt/Windows11/DEV_WORKSPACE/kontra-runs/w8-rss-20261008/. Flags: RUST_MIN_STACK=33554432, XDG_CACHE_HOME=/dev/null, KONTRA_AUDIT_LOAD=1, no PROBE_ALLOCS, core dumps disabled.

Cross-workstream diff: sampler-core immutable preparation and live-bank construction (ops, script, prepare, lib, plans); src/plugin.rs adds only a numeric source-size/instruction-layout audit. No engine-parameter/compiler encoding or streaming change.

Pacific initial editor RSS increased 1.07 MiB. A retained alternating frozen-binary repeat measured 238.68 → 230.75 MiB (−7.93); load RSS 214.64 → 200.21 MiB and HWM 239.43 → 231.71. Process/allocator variation prevents a universal per-run RSS claim. Both pairs are retained; the shared text payload saving is structural, and release RSS acceptance remains with the next gate. Repeat receipts: 3f49-pacific-{before,text}-repeat-rss.json.

Conflux encoding attribution: source sizes [286858, 86323, 1043] bytes (374224 total); sizeof(core Instruction) is 32 bytes. Lowering budgets [1294461, 5922, 121] total 1300504 instructions, implying 41616128 bytes (111.2 code bytes per source byte). The allocator retained 41513568 bytes in instruction boxes ≥4096 bytes, plus 46848 in the engine-start site; budget is not an exact retained count. v1 0cb7a8a0 uses an Arc<Program> and Vec<Op> with fused flat operations. No frozen numeric v1 count/sizeof adapter exists in W5 records, so numeric encoding parity is unmeasured and routed to W5. No encoding change is included here.
