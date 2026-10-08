# v1 source-time streaming port — HOLD

Base: `52438eb49b3d9daa45e88eeead48f610c9dfaa95`. Compare against this exact source and the frozen, equivalence-checked v1 CPU adapter. The earlier d75 reference is excluded: intervening accepted runtime reductions must not be credited to this port.

## Source port and adaptation

- `0cb7a8a0:src/engine/stream.rs`: zeroed 8192-frame per-voice rings, urgent 4096-frame lead before speculative whole 2048-frame chunks, 64 cached readers and 128 shared physical decode blocks per worker. Untouched idle ring pages remain uncommitted.
- `0cb7a8a0:src/engine/bank.rs`: resident heads of 4048 source frames, merged sample/start-offset and short-loop spans, budget fitting and controller-reachable start ranges. RAM-only keeps the existing smallest-sample-first policy.
- `0cb7a8a0:src/engine/params.rs`: control-side start-offset reach queries, including initialized CCs and conservative unknown/script sources.

The pool boundary owns unsafe ring storage and checked borrowed runs. Core remains `forbid(unsafe_code)`. Core traverses virtual source frames while retaining reverse playback, crossfade guards, loop tuning and all eight native loop slots. Admission counts only streamed layers and reserves available setup queue space before publishing the selection. Typed decode failures keep bounded retries; terminal faults retire held voices of the affected source. Fault notifications survive queue pressure and stale generations cannot publish into reused slots. Workers remain parked while idle and wake at a failed decode's retry deadline.

Kontakt assembly uses the ring transport and the preload planner. The host Auto policy eagerly loads budgeted onset spans with v1's 1 GiB bank budget; storage reads remain off audio. Existing cold-onset lifecycle semantics are inherited from the accepted base.

## Targeted validation

Latest validation: **56 targeted tests PASS**, plus root `cargo test --no-run` PASS.

| Area | Tests |
|---|---:|
| Worker fairness, EOF tail, stale completion/fault, fault pressure, queued admission, retry/idle parking and resident-prefix purge backfill | 7 |
| Ring/resident rates/directions/native loops/release/actual 64-voice parallel path, mixed admission, terminal held-source fault | 3 |
| Cold lifecycle | 10 |
| Late source offsets and pre-onset note-off, both directions and both backends | 2 |
| Existing paged render regressions | 16 |
| Ring storage: wrap, stale generation, concurrent publication, head gap, untouched physical idle pages | 5 |
| Kontakt decoder, retry, preload planning, RAM policy, lazy budget compatibility, random reads and complete-source offline factory render | 11 |
| Initialized-controller/nonmonotonic/conservative offset reach | 2 |

The first draft's parallel fixture used an undersized candidate bound, and the first offline fixture omitted its IR asset entry. Both setup mistakes were corrected before the above green runs. Logs: `~/.cache/kontakto-fix-cpu/v1-ring-wired-check2.log` and `v1-ring-wired-check3.log`; latest status is zero. A separate failing-first resident-head purge regression reproduced an unfilled virtual prefix; ring reuse now requires that prefix to remain resident. `ring-purge-red.log` records the failure and `ring-purge-green.log` records the correction plus neighboring ring render checks and root compile validation. Audio event/render/adoption checks allocate and free no heap memory. Per-part workers retain the existing v2 ownership boundary; process-wide shared-bank scheduling is still pending a multi-part probe.

## Original-instrument acceptance — pending

Areia Full Ensemble and ANALOG STRINGS at 32/64/256 frames, cold and warm. Three rotations compare candidate, exact 52438 twice (A/A), and frozen v1. Record median/p99 per block, deadlines, storage underruns, capacity errors, loaded/final RSS and per-run unit/cgroup activity. Only QUIET observations count. Candidate must have zero underruns and capacity errors; RSS must not exceed 52438 beyond its measured A/A noise floor. CPU/deadline changes and v1 gaps remain explicit.

No original-instrument numbers have been collected for this wiring. No streaming SHA is READY for integration. Release/install remains held.
