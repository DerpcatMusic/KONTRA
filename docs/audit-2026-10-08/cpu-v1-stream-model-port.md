# v1 source-time streaming port — HOLD

Base: `52438eb49b3d9daa45e88eeead48f610c9dfaa95`. Compare against this exact source and the frozen, equivalence-checked v1 CPU adapter. The earlier d75 reference is excluded: intervening accepted runtime reductions must not be credited to this port.

## Source port and adaptation

- `0cb7a8a0:src/engine/stream.rs`: zeroed 8192-frame per-voice rings, urgent 4096-frame lead before speculative whole 2048-frame chunks, 64 cached readers and 128 shared physical decode blocks per worker. Untouched idle ring pages remain uncommitted.
- `0cb7a8a0:src/engine/bank.rs`: resident heads of 4048 source frames, merged sample/start-offset and short-loop spans, budget fitting and controller-reachable start ranges. RAM-only keeps the existing smallest-sample-first policy.
- `0cb7a8a0:src/engine/params.rs`: control-side start-offset reach queries, including initialized CCs and conservative unknown/script sources.

The pool boundary owns unsafe ring storage and checked borrowed runs. Core remains `forbid(unsafe_code)`. Core traverses virtual source frames while retaining reverse playback, crossfade guards, loop tuning and all eight native loop slots. Admission counts only streamed layers and reserves available setup queue space before publishing the selection. Typed decode failures keep bounded retries; terminal faults retire held voices of the affected source. Fault notifications survive queue pressure and stale generations cannot publish into reused slots. Workers remain parked while idle and wake at a failed decode's retry deadline.

Kontakt assembly uses the ring transport and the preload planner. The host Auto policy eagerly loads budgeted onset spans with v1's 1 GiB bank budget; storage reads remain off audio. Existing cold-onset lifecycle semantics are inherited from the accepted base.

## Targeted validation

Validation at `d826303f`: **56 targeted tests PASS**, plus root `cargo test --no-run` PASS.

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

The first attempt stopped on external cargo/rustc contention at 21:04:58 UTC and removed its request. Exact 52438 A/A Areia32 cold rows were QUIET: 24 underruns each, 932/933 capacity errors, loaded RSS 379.30/379.02 MiB. The candidate row had zero underruns/capacity errors but was CONTENDED: loaded RSS 688.01 MiB, and its unit recorded 553 MiB swap. This is a memory failure warning, not an accepted timing comparison. Attempt receipts remain under `~/.cache/kontakto-fix-cpu/stream-model-matrix`. No streaming SHA is READY for integration. Release/install remains held.


## Complete v1 head memory model — instrument acceptance pending

The `d826303f` head owner reports 800,308,908 bytes (763.23 MiB). Rings reserve 67,108,864 bytes virtually (64 MiB; idle pages remain untouched), matching v1's f32 ring representation. Four decoder caches can hold 8 MiB of physical frames; the 64 readers per worker have 4 MiB total byte buffers, plus codec metadata. These cache figures are maximum capacities, not attributed resident measurements. Frozen v1 Full Ensemble scanner RSS was 307.10 MiB cold and 305.12 MiB os-warm in the d75 gate receipt.

The follow-up ports `0cb7a8a0:src/audio.rs` predictive 64-frame blocks, with safe scalar reads at the core boundary; short or whole resident loops keep raw native PCM. Native 4/6/8-byte planning and cache metadata replace the eight-byte estimate. The 1 GiB v1 budget includes the ring reservation before the preload/offset-coverage fallback. The existing numeric header cache version advances because the old entries did not retain native width. Warm valid entries still skip codec opens.

Numeric `stream_head_owners` diagnostics count distinct assets, head assets/spans, planned source frames/native bytes, predictive spans/frames and actual stored bytes. RSS/swap and original-instrument timing are pending. A smooth stereo i24 fixture failed before the codec port (`head-packed-red.log`, status101); 48 targeted checks PASS: packing3, compressed ring render3, Kontakt stream/preload12, header cache1, AIFF1, cold-chain10, cold-offset2, paged render16. Root `cargo test --no-run` PASS. Logs are `head-model-check-3.log` and `head-model-neighbors.log`, both status0. The previous exact raw-size assertion now verifies both compression-disabled raw sizing and the smaller predictive representation; a noisy fixture that correctly fell back to native PCM was replaced with a smooth nonzero-residual fixture to exercise the predictor. Next is ordinary-wrapper ownership/RSS/swap measurement, followed by quiet A/B only if RSS fits the exact 52438 baseline.

The v1 full-bank UI probe (`0cb7a8a0:src/ui/audit.rs:737`) uses 512 MiB, while the frozen CPU adapter uses the product `MEMORY_LIMIT` of 1 GiB. Scanner and CPU-adapter RSS must retain that provenance; the candidate still must meet the exact 52438 RSS ceiling and zero swap. The upcoming ownership probe records the CPU adapter’s actual head bytes, sample count and chosen preload alongside v2’s detailed counts.

## 368d2654 ownership verdict — RSS FAIL, no quiet window

Normal-wrapper, OS-warm ownership probes (timing UNKNOWN; other work recorded) used the frozen candidate, exact 52438 and frozen v1 CPU adapter. Receipt: `/mnt/Windows11/DEV_WORKSPACE/kontra-runs/w9-head-owners/summary.json`. Source 368d2654; frozen binary SHA256 `b3239c67f8ae7e10debf1217eaa13929cd9eb164141f11adc08acd9684bdcf1b`.

| Metric | 52438 | 368d2654 | Frozen v1 CPU adapter |
|---|---:|---:|---:|
| Loaded RSS, MiB | 380.16 | 819.92 | 765.15 |
| Final RSS, MiB | 387.86 | 875.13 | 812.38 |
| Probe-process peak VmSwap, MiB | 0 | 0 | 0 |
| Storage underruns, this unscored run | 24 | 0 | 0 |
| Stream capacity errors | 912 | 0 | UNKNOWN (not exposed) |
| Actual head payload, bytes | 0 | 498,620,047 | 498,608,156 |
| Preload, frames | 616 (lazy old path) | 3670 | 3664 |

The candidate plans **41,334 distinct assets and head spans**, **166,617,354 source frames**, **999,704,124 native bytes**. All spans/frames use predictive packing, holding **498,620,047 bytes** (49.88% less than the native plan). Every source plans at six bytes per stereo frame. Rings remain 67,108,864 virtual bytes. The v1 adapter’s `head_bytes=565,717,020` includes its 64 MiB ring reservation; subtracting that gives the head payload above. Full-bank payload parity is within 11,891 bytes. Compared with d826’s 800,308,908-byte raw heads, stored heads fall by 287.71 MiB, but the old swapped RSS row is not a valid physical-memory comparison.

The probe descendants each recorded zero VmSwap. The ordinary wrapper unit separately reached 7.1 MiB swap, which includes persistent helper processes; zero-swap release acceptance still requires the quiet gate. No new quiet request was made because candidate RSS fails the required exact-52438 ceiling.

**Reference-stage correction:** the 307.10 MiB gate scanner row explicitly reports an “initial streaming bank”. Its pinned `src/ui/scan.rs:250` calls `Bank::load_bare`, not the full-bank UI benchmark’s separate 512 MiB path. Product v1 first publishes that no-head bank, then fills resident heads on a worker. The CPU adapter instead finishes the full 1 GiB load. Both provenance and numbers remain explicit; the required RSS ceiling is not silently changed. The optional heap-retention probe was stopped before completion: the packed payload alone exceeds the total RSS ceiling, so trimming cannot change the verdict. Next: resolve initial-vs-completed-bank residency policy with the coordinator; prepare the deferred held-note slow-attack fixture in source-only time. Streaming remains HOLD.
