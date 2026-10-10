# W10 UVI class userdata parity — READY

Product: `f3b53c93db2c9a018c65bb0d08442c8999a024ec`. Reference: `4bffbb18:src/uvi/host.rs:1692-1790`. The v1 userdata implementation now runs on the existing v2 Luau owner. Class declarations publish a userdata in the main environment and return a base-builder; instances retain authored fields/methods. Inheritance copies existing members except `__init`, so a derived constructor requires its own initializer. Invalid names and non-class bases fail.

Three authored contracts failed on the previous table stand-in. The direct port passed inheritance and validation but exposed a VM difference: Lua 5.1 checks raw userdata identity before its equality metamethod; Luau invokes the metamethod first. The adapter preserves self-identity for classes and instances while keeping v1's error on equality between distinct objects and on tostring. The local vendored VM sources establish the dispatch difference (`lua-5.1.5/lvm.c:263` and `luau/VM/src/lvmutils.cpp:469`); mlua raw-handle equality uses `lua_rawequal`.

## Verification

Corrected per-worktree target, ordinary wrapper FIFO:

- Behavioral RED: 0 passed / 3 failed.
- Initial direct-port candidate: 2 passed / 1 failed; retained unchanged.
- Adapted candidate: 25 passed / 0 failed across class, real modules, UI and assets contracts.
- `cargo test --profile ci -p sampler-uvi --no-run`: PASS.
- `git diff --check`: PASS. The verification unit drained inactive/MainPID 0.

Receipt: `/mnt/Windows11/DEV_WORKSPACE/kontra-runs/w10-uvi-scan-344/ui-class-READY.json`, including exact source/log hashes, product SHA, build base, target and wrapper digest. Logs `ui-class-RED.log`, `ui-class-GREEN.log`, `ui-class-GREEN-retry.log` and `ui-class-area-no-run.log` preserve every outcome.

Installed-bank fidelity remains UNKNOWN/PARKED for 26 banks / 660 programs, and native-v1 cells requiring an official reader remain unavailable. No full-corpus, native Falcon or timing acceptance is admitted by this slice. The single corrected callback diagnostic is documented separately in [W10_UVI_CALLBACK32.md](W10_UVI_CALLBACK32.md).

NEXT: named geometry precedence and parent/children construction, with authored shared-renderer pixel receipts.
