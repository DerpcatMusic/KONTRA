# Load audit — 2026-10-08

V1: `0cb7a8a0`. V2: `origin/integrate/core-v2`, frozen at `7e82b152`. Audit-only probes; no performance fixes.

## Verdict

**Load responsiveness: worse. Memory: mixed. UVI support: better than the requested v1 commit, with expensive startup in some banks.** In the final survey v2 reaches first sound later on **10/10** playable Kontakt presets against v1 with existing caches, and **9/10** against v1 with caches disabled. Conflux: **91.6 ms cached v1 / 109.6 ms fresh v1 / 4682.9 ms v2**. Areia Full Ensemble: **2820.9 / 3406.2 / 22815.5 ms**. V2 settled RSS is lower on **6/10** playable Kontakt presets against fresh v1; it does not satisfy “better in every aspect.”

Fourteen real presets from thirteen products were measured: ten playable Kontakt products, three UVI products and Conflux Big Screen multi program 0 as a supplemental controller-only case. All thirteen playable v2 presets produced nonzero C4 output. The multi controller has zero samples/zones and produces no note; it is **not** a benchmark of the full multi. V1 at `0cb7a8a0` has no UVI loader and immediately fails those paths; N/A below means unsupported, never a fast success.

## Method and interpretation

This uses optimized `cargo test --profile ci` executables and the production plugin `Load.run`, frontends, KSP/Lua initialization, streaming workers, prepared runtime and native editor scene/layout harness. Browser selection writes part sources through `src/ui/browser.rs:733` and `src/ui/mod.rs:529`; the host process schedules the loader roughly every 0.1 seconds of processed audio (`src/plugin.rs:1546`). The benchmark starts at worker dispatch, installs the real handoff and sends C4 velocity 100 plus CC1/CC11=127. It renders paced 64-frame blocks at 48 kHz and detects the first sample above 1e-7. Publication, first sound, full worker completion and UI build are separate milestones. These probes do not include browser event dispatch, the initial 0–100 ms processing-dependent task delay, native DAW/audio-device scheduling, or GPU presentation. A stopped host can delay task dispatch indefinitely; full click-to-speaker timing remains open.

Each row is a fresh process on the mounted NVMe/ntfs3 library volume with an uncontrolled OS page cache. Most presets had prior pilot passes; repeats are warm, while Flute’s first pass was not established warm. No system-cache flushing, Wine, yabridge or Bitwig changes were made. This shared machine has other audits competing for CPU/I/O; wall-clock variance is substantial. Short shards run through `kontakto-heavy`, one owned heavy job at a time. All timings include the audit probe overhead (numeric RSS reads between stages). The final headline uses v2 round 4 and historical v1 round 5. Round 4 corrected the instrument-cache identity; round 5 additionally retains a preload bank deferred by script-driven zone edits. Historical cache hits are inferred from a completed cache lookup with no NI-file-read stage; the cache mode permits existing reads and does not assert every preset hits.

**Memory:** MiB = 1,048,576 bytes. Peak is the maximum of process VmHWM and sampled VmRSS (approximate Linux counters can differ slightly); settled is VmRSS after four wall-clock seconds with the editor harness still alive. It includes engine/script/IR/sample/UI allocations and allocator retention. `rss0_mb` and pre-editor RSS remain in the numeric artifact. V1 retains a deferred preload bank rather than incorrectly dropping it; `extra.preload_deferred` records that condition. The probe does not perform the native plugin’s later zone-preload reconciliation or smart-head management loop, so these are worker/handoff/UI residency measurements, not a sustained host-memory benchmark. The trim experiment closes the editor and calls glibc `malloc_trim(0)` **after** all reported measurements; it distinguishes reclaimable allocations and is not open-editor RSS. Four wall seconds do not exercise v2’s 30 audio-clock seconds of eviction.

The editor follows the baseline presentation decision, which can choose Vector; this is not an ORIGINAL-only/UI-fidelity certification. V1/v2 audio peaks and articulation states differ. Nonzero sound is a milestone, not native sonic parity. The shared census results were not available; this fixed manifest is a supplemental stage probe, not a new corpus scanner.

## Final whole-path measurements

First sound in milliseconds; complete means the loader worker has finished its work, including v1’s later artwork/preload. The editor was built after worker completion; a host can build it earlier/overlap stages, so do not sum UI time into first audio or treat this sum as captured click-to-present latency.

| Preset | V1 cache first sound | V1 fresh first sound | V2 first sound | V1 cache complete | V1 fresh complete | V2 complete |
|---|---:|---:|---:|---:|---:|---:|
| Conflux | 91.6 | 109.6 | 4682.9 | 111.6 | 170.3 | 4682.6 |
| Areia-FullEns | 2820.9 | 3406.2 | 22815.5 | 3188.5 | 4777.1 | 22803.9 |
| Dolce-Vln1 | 444.5 | 1167.2 | 1973.9 | 602.0 | 1624.5 | 1969.2 |
| Barbarian | 108.1 | 474.7 | 842.1 | 466.6 | 1267.9 | 841.4 |
| UnaCorda-Felt | 67.7 | 241.5 | 325.1 | 2046.7 | 468.8 | 325.0 |
| AnalogStrings | 1991.2 | 8871.3 | 3254.9 | 43557.3 | 20737.5 | 3253.1 |
| Multi-ConfluxBigScreen | — | — | — | 17.4 | 17.8 | 32.9 |
| UVI-AugOrch | N/A | N/A | 1822.9 | N/A | N/A | 1822.5 |
| Morphology | 248.4 | 186.1 | 490.1 | 684.8 | 233.4 | 489.7 |
| Vista-Harp | 36.2 | 68.2 | 119.6 | 209.1 | 100.1 | 119.1 |
| Solo-Violin | 279.0 | 471.7 | 1085.8 | 4245.4 | 791.1 | 1081.1 |
| Pacific-Cellos | 39.0 | 98.7 | 267.4 | 502.3 | 155.4 | 265.2 |
| VWinds-Clarinet | N/A | N/A | 609.7 | N/A | N/A | 609.7 |
| VWinds-Flute | N/A | N/A | 1527.5 | N/A | N/A | 1527.4 |

All rows have approximately 12–13 MiB initial process RSS. Failed v1 UVI process RSS is excluded from the comparison.

| Preset | V1 cache peak / settled MiB | V1 fresh peak / settled MiB | V2 peak / settled MiB | V1 fresh UI ms | V2 UI ms |
|---|---:|---:|---:|---:|---:|
| Conflux | 115.1 / 114.9 | 115.0 / 114.9 | 234.2 / 199.2 | 20.6 | 21.0 |
| Areia-FullEns | 922.3 / 911.5 | 948.6 / 920.3 | 777.4 / 777.3 | 29.3 | 19.9 |
| Dolce-Vln1 | 536.1 / 536.0 | 530.8 / 530.7 | 435.2 / 435.2 | 27.5 | 18.2 |
| Barbarian | 313.2 / 313.0 | 315.0 / 314.8 | 199.4 / 199.2 | 16.3 | 19.6 |
| UnaCorda-Felt | 201.9 / 201.7 | 203.1 / 203.0 | 178.1 / 178.0 | 10.5 | 32.1 |
| AnalogStrings | 1057.0 / 1052.6 | 1112.5 / 1064.1 | 1245.6 / 1231.8 | 17.1 | 415.4 |
| Multi-ConfluxBigScreen | 44.8 / 44.6 | 45.3 / 45.2 | 95.9 / 95.7 | 16.6 | 14.6 |
| UVI-AugOrch | N/A | N/A | 248.2 / 248.2 | N/A | 14.7 |
| Morphology | 141.9 / 141.8 | 142.0 / 141.9 | 173.9 / 173.9 | 20.9 | 46.0 |
| Vista-Harp | 98.2 / 98.2 | 98.2 / 98.2 | 122.6 / 122.6 | 12.6 | 12.3 |
| Solo-Violin | 808.8 / 808.7 | 806.6 / 806.4 | 314.7 / 314.6 | 15.5 | 16.7 |
| Pacific-Cellos | 175.5 / 175.5 | 178.8 / 178.6 | 162.0 / 161.8 | 12.5 | 11.9 |
| VWinds-Clarinet | N/A | N/A | 158.9 / 158.8 | N/A | 35.5 |
| VWinds-Flute | N/A | N/A | 165.2 / 165.1 | N/A | 29.6 |

## Stage timing

Milliseconds. NI parse combines chunk and object parsing; translation includes the **two early KSP passes**, while final KSP means resource-aware compilation/init. Nested `ksp_frontend`/`ksp_on_init` and translation subspans must not be added again. Sample header opening includes the first-32-source latency probe. Lower/runtime are shown together. Unlabelled residual work (e.g. report/tree assembly, cloning) explains why columns do not always sum to worker total.

| Kontakt preset | File read | Decrypt/expand | NI parse | Translate/resolve IR | Source resolve | Headers | Final KSP | Preload | Lower/runtime | UI metadata |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| Conflux | 0.3 | 3.1 | 1.8 | 4424.9 | 9.6 | 35.8 | 87.4 | 35.2 | 18.0 | 0.7 |
| Areia-FullEns | 2.7 | 22.5 | 43.3 | 1464.0 | 209.9 | 19428.5 | 551.1 | 894.5 | 131.6 | 2.1 |
| Dolce-Vln1 | 1.9 | 14.3 | 18.9 | 903.7 | 72.4 | 164.3 | 459.8 | 242.3 | 66.6 | 1.8 |
| Barbarian | 0.3 | 3.7 | 5.2 | 87.3 | 33.1 | 443.8 | 4.1 | 213.2 | 35.6 | 0.6 |
| UnaCorda-Felt | 0.4 | 3.5 | 3.1 | 69.3 | 20.1 | 81.7 | 18.7 | 85.3 | 22.6 | 1.3 |
| AnalogStrings | 2.1 | 21.9 | 84.3 | 954.5 | 165.6 | 483.7 | 464.0 | 522.0 | 149.5 | 9.7 |
| Multi-ConfluxBigScreen | 0.3 | 2.0 | 3.3 | 7.5 | 0.0 | 0.0 | 3.4 | 0.0 | 9.7 | 0.1 |
| Morphology | 0.3 | 3.4 | 3.8 | 185.4 | 1.3 | 4.7 | 137.7 | 21.3 | 16.2 | 6.7 |
| Vista-Harp | 0.1 | 1.0 | 1.1 | 35.1 | 5.1 | 17.0 | 0.9 | 34.9 | 11.9 | 0.1 |
| Solo-Violin | 1.0 | 8.0 | 13.0 | 442.2 | 56.3 | 88.4 | 162.7 | 258.3 | 34.9 | 1.2 |
| Pacific-Cellos | 0.4 | 3.5 | 3.5 | 52.5 | 31.4 | 37.9 | 1.6 | 86.5 | 20.7 | 0.2 |

V1 imports directly into its engine structures and has no separate v2 IR stage. Fresh NI object import below includes parse, mapping and sample resolution. Script initialization and bare-bank header opening run concurrently, so **do not sum them**. Cached presets bypass the fresh read/decrypt/chunk/import stages; final-round cache hits are recorded in the first column. Artwork and full preload happen after first publication.

| Kontakt preset | Cache hit | Cache lookup ms | Fresh file / decrypt / chunk ms | Fresh object import/resolve ms | Fresh KSP ms | Fresh bare-bank ms | Fresh worker artwork / preload ms |
|---|---|---:|---:|---:|---:|---:|---:|
| Conflux | yes | 8.0 | 0.2 / 2.4 / 0.4 | 16.3 | 63.2 | 12.4 | 1.3 / 59.6 |
| Areia-FullEns | yes | 75.9 | 2.1 / 22.6 / 5.3 | 387.4 | 275.7 | 2905.2 | 19.2 / 1357.2 |
| Dolce-Vln1 | yes | 33.4 | 3.1 / 26.0 / 7.0 | 309.8 | 366.1 | 722.2 | 31.4 / 427.5 |
| Barbarian | yes | 18.7 | 0.7 / 2.9 / 0.9 | 212.1 | 17.2 | 241.1 | 9.8 / 785.0 |
| UnaCorda-Felt | yes | 16.5 | 0.7 / 3.0 / 0.6 | 93.7 | 14.7 | 123.9 | 31.1 / 198.2 |
| AnalogStrings | yes | 103.7 | 15.7 / 23.5 / 5.2 | 1875.3 | 176.0 | 6874.7 | 969.5 / 10907.9 |
| Multi-ConfluxBigScreen | no | 0.0 | 0.3 / 2.0 / 0.4 | 4.6 | 6.7 | 0.0 | 1.0 / 0.0 |
| Morphology | no | 58.3 | 0.3 / 3.3 / 1.1 | 51.5 | 105.2 | 6.7 | 33.4 / 16.4 |
| Vista-Harp | yes | 23.4 | 0.2 / 1.0 / 0.2 | 44.3 | 7.8 | 14.2 | 1.7 / 32.0 |
| Solo-Violin | yes | 32.8 | 2.2 / 10.3 / 2.6 | 106.8 | 128.3 | 324.0 | 12.1 / 309.4 |
| Pacific-Cellos | yes | 11.7 | 0.9 / 3.2 / 0.7 | 55.9 | 10.1 | 30.0 | 1.3 / 57.9 |

UVI bank-opening substages are nested, not additive to bank-open total. File read and decrypt of the protected program are one stage; NI-file/KSP do not apply.

| UVI preset | Bank open | Header | Directory/reader | Content setup | Program read/decrypt | XML → IR | Scripts/resources | Lua init | Headers | Preload | Lower/runtime |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| UVI-AugOrch | 610.7 | 0.0 | 408.2 | 198.4 | 56.0 | 94.7 | 5.0 | 276.0 | 22.7 | 114.8 | 400.0 |
| VWinds-Clarinet | 141.0 | 1.2 | 124.3 | 15.4 | 16.0 | 32.2 | 0.9 | 71.2 | 6.0 | 18.7 | 289.5 |
| VWinds-Flute | 983.8 | 0.2 | 391.4 | 591.8 | 21.9 | 31.0 | 1.0 | 83.5 | 11.4 | 25.1 | 217.2 |

## Sample residency and preload

V2 full GiB is the decoded stereo-f32 **equivalent if all samples were loaded**, not measured disk bytes/RSS. Heads are actual packed range bytes; the page pool is actual initialized decoded-page storage. V1 fresh resident is process-counted packed sample backing, excluding virtual streaming-ring reservations and including a retained deferred bank if present. The v1 preload report’s `resident_bytes` also adds 64 MiB virtual ring reservation, so it is not interchangeable with these actual sample bytes or RSS.

| Preset | V2 samples / zones | Full equivalent GiB | V1 fresh sample MiB | V2 packed heads MiB | V2 pool MiB | Base head frames | V1 preload deferred |
|---|---:|---:|---:|---:|---:|---:|---|
| Conflux | 1962 / 1985 | 6.0 | 19.8 | 5.8 | 24.0 | 548 | true |
| Areia-FullEns | 41334 / 82668 | 42.9 | 475.5 | 327.0 | 24.0 | 550 | false |
| Dolce-Vln1 | 14784 / 30768 | 9.6 | 270.9 | 80.8 | 24.0 | 552 | false |
| Barbarian | 7044 / 8408 | 7.8 | 209.4 | 36.9 | 24.0 | 545 | false |
| UnaCorda-Felt | 3943 / 4402 | 15.7 | 102.6 | 16.9 | 24.0 | 547 | false |
| AnalogStrings | 23595 / 95624 | 46.7 | 426.2 | 136.9 | 24.0 | 552 | false |
| Multi-ConfluxBigScreen | 0 / 0 | 0.0 | 0.0 | 0.0 | 24.0 | 544 | false |
| UVI-AugOrch | 2177 / 6984 | 0.9 | N/A | 4.3 | 24.0 | 547 | N/A |
| Morphology | 458 / 3495 | 4.5 | 7.3 | 4.1 | 24.0 | 548 | false |
| Vista-Harp | 2000 / 2000 | 4.2 | 42.7 | 8.8 | 24.0 | 546 | false |
| Solo-Violin | 11858 / 23982 | 16.9 | 634.1 | 96.0 | 24.0 | 545 | false |
| Pacific-Cellos | 4470 / 6136 | 3.4 | 106.1 | 26.5 | 24.0 | 545 | false |
| VWinds-Clarinet | 188 / 188 | 0.2 | N/A | 0.4 | 24.0 | 552 | N/A |
| VWinds-Flute | 285 / 285 | 0.2 | N/A | 0.7 | 24.0 | 551 | N/A |

## Repeatability

V2 first-audio range across the earlier successful rounds 2/3 and final round 4. The earlier rounds used the same production load path but settled after editor close, so they are retained for **timing variation only**, not headline steady-state memory. Historical round 3 cache-mode runs had a probe-induced importer-hash miss and are excluded.

| Preset | V2 first-audio min–max ms |
|---|---:|
| Conflux | 4682.9–4872.1 |
| Areia-FullEns | 5905.2–22815.5 |
| Dolce-Vln1 | 1973.9–2093.1 |
| Barbarian | 799.2–842.1 |
| UnaCorda-Felt | 292.5–410.4 |
| AnalogStrings | 2978.6–4196.2 |
| Multi-ConfluxBigScreen | — (controller) |
| UVI-AugOrch | 1746.1–3224.6 |
| Morphology | 448.0–490.1 |
| Vista-Harp | 118.2–363.7 |
| Solo-Violin | 1082.9–1091.8 |
| Pacific-Cellos | 264.3–523.5 |
| VWinds-Clarinet | 609.7–1634.7 |
| VWinds-Flute | 1527.5–15514.5 |

## Ranked findings

### 1. P0 — Conflux runs expensive initialization twice with a different control environment (M)

**Evidence:** `crates/sampler-kontakt/src/library.rs:264` calls `init_engine_pars` for engine writes, then `library.rs:284` calls `compile_with` just to discover dynamic effect-slot writes. Both pass an empty performance view (`library.rs:273`, `:294`). The eventual `crates/sampler-kontakt/src/load.rs:587` reads/parses the authored NCKP before compiling again. `crates/sampler-ksp/src/lib.rs:512` and `:556` each lex/resolve/evaluate initialization; the second early pass also lowers callbacks. In survey round 3, Conflux spends **2,141 + 2,286 ms** in those two early passes, versus **88 ms** for all final compilation/resource setup; the main final `on init` alone takes **1.63 ms**, versus **2,094 ms** during the early compile. These successful early passes consume 91% of the 4,872 ms worker load. No numeric `translate_init_error` was emitted: this is not evidence of an initialization-fuel failure.

**Cause:** repeated compilation and initialization, with missing authored control definitions in the early environment. The guessed knobs/sliders have 0–1,000,000 ranges (`sampler-ksp/src/model.rs:62`); final initialization gets the actual NCKP. The particular library loop responsible for the large environment sensitivity remains unproven; do not invent a fuel exhaustion or pretend a script-level profile was collected. Different group names/state can also matter; normalize the whole environment, not only a guessed control limit.

**Fix:** in `library.rs`, `load.rs` and the KSP frontend, resolve resource-backed controls before any initializer; compile/evaluate once and retain the initialized model, engine writes and dynamic-effect information for IR lowering and final runtime binding. A frontend/static effect-write query can replace a full dynamic-discovery initializer. Keep independent mutable state per instance. Recheck engine-FX defaults and Conflux authored control values, then benchmark unchanged C4/audio/UI behavior. Do not merely reduce the interpreter budget.

### 2. P0 — Authored artwork decode blocks the editor and produces a large RAM spike (M)

**Evidence:** `src/ui/part.rs:26` builds a new resource index; `:30` immediately calls `Face::sync`, and `:56` explicitly documents decode on the UI thread. `src/ui/pictures.rs:20` reads and decodes the full image, then `:25`/`:32` crops/copies animation frames. Round 3 ANALOG STRINGS takes **414 ms** to build/layout the editor and grows RSS from **864.7 to 1,246.6 MiB** while it is alive: **381.9 MiB**. V1's corresponding build is **17 ms**. After closing the harness, `malloc_trim(0)` reduces v2 RSS to **853.5 MiB**; the spike is therefore largely reclaimable image/scene/decoder allocation, not 393 MiB of additional live samples. That experiment closes the UI and is not an open-editor steady-state measurement.

**Cause:** heavy PNG/animation work lives in the first editor frame and repeats resource lookup after loader metadata work. `crates/sampler-kontakt/src/resources.rs:115` already reads the entire encoded image to obtain metadata, and the editor subsequently reads it again. There is no background handoff of prepared pictures analogous to v1's loader artwork publication.

**Fix:** prepare visible authored resources off the UI thread, publish immutable decoded images/frame views, cancel stale work, and retain one resource index with the prepared part. Use shared frame views when the renderer supports them instead of copying all frame pixels. Bound the image cache and measure UI-open, UI-close and allocator-retained memory separately. Preserve the ORIGINAL default required by the user's addendum: baseline `part.rs:59` chooses Vector when `unsupported` is empty, so optimizing only that cheaper presentation is not parity. The UI agent owns that P0 presentation correction; merge its resource fixes before extending decode.

### 3. P1 — All sample headers are opened serially before the part can play (M)

**Evidence:** `crates/sampler-kontakt/src/stream.rs:403` loops through every source and calls `source.open()` synchronously; only the first 32 get a latency read. Round 3 Areia opens **41,334 sources in 6,031 ms**, the dominant stage of its 9,378 ms load. Barbarian headers take 407 ms; Vista 260 ms despite only 2,000 samples. These vary with contention/page-cache state: Areia's earlier complete v2 run was 5.89 s total, so 6.03 s is a measured run, not a guaranteed device latency.

**Cause:** v2's four decode threads start only after all header work and preload (`stream.rs:480`). They do not parallelize opening or startup. V1 resolves archives once, opens member headers in bounded parallel workers and can use a validated numerical header cache (`v1 src/audio.rs:816`, `src/engine/bank.rs:730`, `src/cache.rs:337`).

**Fix:** add a bounded header-opening stage using existing worker primitives; preserve asset order, cancellation and deterministic error attribution. Reuse validated source/header metadata keyed by canonical container identity and revision. Avoid opening the same source again in successive IR/source/header passes. Measure 1/2/4 workers and storage saturation; do not spawn one thread per sample.

### 4. P1 — UVI bank setup repeats expensive content preparation (M)

**Evidence:** round 3 VWinds Flute spends **14,110 ms** in bank opening, **57 ms** in program read/decrypt, **66 ms** in XML translation, **212 ms** in Lua initialization and **46 ms** in sample preload. The 15.5 s wait is not caused by loading the full 220 MiB equivalent sample set. That was the first Flute pass; OS cache warmth was not established. The repeated round-4 load falls to **1,527 ms**, with **391 ms** in directory/reader setup and **592 ms** in content preparation. The older 14.1 s bank-open span cannot retrospectively distinguish cold directory I/O from content recovery or scheduling. The UVI table and raw numeric results preserve that distinction.

**Cause:** `crates/sampler-uvi/src/bank.rs:53` reopens the bank, decodes its directory, loads/verifies reader namespaces and prepares content access for every new load. `access.rs:153` calls bounded content-state recovery; `crypto.rs:96` allows up to 2^32 candidates on eight workers. Content recovery is library-dependent CPU work; directory decoding also walks the bank record chain using many small reads (`ufs.rs:261`, `:302`). Both happen before first sound; the slow first-pass attribution is still incomplete. This is not a disk mmap operation. `access.rs:21` also rereads/hashes the official reader for each namespace request. There is no process-level weak bank/namespace cache in this baseline. The content state stays only in memory, which is correct; preserve that property.

**Fix:** share a weak/Arc prepared bank per immutable file identity (including header snapshot), and the hash-verified reader namespaces per reader revision. Invalidate on replacement/modification, propagate failure safely and retain recovery validation. Never persist recovered content state or decrypted resources. Measure a second program in the same bank with the first part retained, and a fresh-process load, as separate cases. V1 at the requested commit has no UVI frontend, so its immediate failures are **N/A**, not a speed win.

### 5. P1 — V2 waits for every sample preload before publishing the first playable part (M)

**Evidence:** Kontakt preparation calls streamed assembly before `V2Loader` returns (`src/sound/v2.rs:1071`); `Streamer::start` serially reads every start range (`stream.rs:468`) before starting workers. `src/plugin.rs:1203` publishes only afterward. Round 3 Areia's preload adds **845 ms**; ANALOG STRINGS **542 ms**, Solo Violin **264 ms**, Dolce **241 ms**. V1 publishes a bare streaming bank and script runtime first (`v1 src/plugin.rs:2414`), then decodes artwork and prepares the full resident bank while notes can already play. Conflux’s final verified cached v1 pass sounds at **92 ms** and completes at **112 ms**, whereas v2 sounds at **4,683 ms** and completes at **4,683 ms**. The completed preload is held even when script zone edits defer its engine installation; that native reconciliation remains an explicit harness limit.

**Cause:** the loader contract currently makes complete head residency part of first publication. V1's later `Handoff::Bank`/`upgrade_bank` preserves playing voices, so its user-perceived load excludes work which v2 puts on the critical path.

**Fix:** prepare bounded first-note-safe heads, start stream workers, publish the ready script/runtime, then fill wider start ranges in a cancelable worker. Preserve sample identity while upgrading, and protect starts until disk pages are valid (v1 uses START_HOLD). Do not simply publish zero heads into the current runtime and accept dropouts. Test notes during loading, source replacement and cancellation.

### 6. P1 — V1's preset/compiled-program/sample sharing has no v2 equivalent (M)

**Evidence:** v1 `src/import.rs:583` shares a live imported instrument through a revision-checked weak registry; `src/cache.rs:89` can skip parse/translation entirely; `src/ksp/runtime.rs:24` shares immutable compiled programs; `src/engine/bank.rs:225` shares packed resident spans/sources by path and revision, including another part/instance. V2 creates a fresh sample registry (`sampler-kontakt/src/samples.rs`), `Pcm::streamed` IDs and per-part heads on each read (`stream.rs:419`, `:470`). Its 16-reader cache per decoder only helps subsequent playback inside that part (`stream.rs:592`); it is not a preset cache.

**Cause:** immutable imported state, compilation results, source metadata and resident spans are rebuilt at each load. Both browser implementations already focus an identical loaded part instead of reloading it; that shortcut is not an exclusive v1 improvement. These cross-instance sharing paths are code-confirmed; a retained-two-instance RSS/timing experiment was not collected, so no invented cache-hit percentage or memory saving is claimed.

**Fix:** start with process-memory weak sharing of immutable parsed/compiled state and packed heads keyed by source/dependency identity. Use independent mutable script/runtime state. A numerical header/index cache can persist metadata; v1's instrument cache contains script plaintext, so copying that format to v2 would violate the no-decrypted-data rule. Record cache hits/misses and invalidation reasons numerically.

### 7. P1 — The memory setting cannot constrain initial load or total plugin RSS (M)

**Evidence:** `src/plugin.rs:189` defaults the budget to zero (keep heads), `:1030` trims before the new part loads, and `:916` only considers heads idle for 30 **audio-clock** seconds. A freshly loaded part consequently retains all planned heads. `src/sound/mod.rs:205` subtracts an unevictable page pool and trims only heads. The setting excludes scripts, IR, note/voice pools, images and decoder buffers. V1 combines a selected per-bank limit with a process-wide free-RAM-aware budget (`v1 src/engine/bank.rs:307`, `src/plugin.rs:2465`).

**Cause:** this is a post-load head-eviction preference, not the global startup budget required by `docs/architecture-v2/PRODUCT.md:75`. Idle clock behavior also means a stopped host does not advance eviction. Four wall-clock seconds of settling do not test it.

**Fix:** pass the budget into startup planning and enforce a process-level admission budget across retained parts/instances, reserving safe first-note data and mandatory wavetable cycles. Show separate measured categories and explicit unavoidable minimums rather than imply that the knob limits RSS. Test budgets below/above the packed-head total and idle/active/stopped-host behavior.

### 8. P1 — Fixed page and runtime reservations cost RAM even for tiny/controller parts (M)

**Evidence:** default `StreamPolicy` reserves 256 voices × 3 pages (`stream.rs:270`, `:287`); `sampler-core/src/stream.rs:167` fills all 768 × 4,096 stereo-f32 frames: **24 MiB physically initialized per part**. Big Screen multi **program 0 is a controller with zero samples/zones**, yet reserves exactly 24 MiB and reaches 96 MiB settled process RSS in round 3. The full multi was not measured. Theoretical 16 streamed parts alone reserve 384 MiB of page storage; that is arithmetic, not measured 16-part RSS. `src/sound/v2.rs:859` additionally sizes runtime state with a 256 MiB voice-state budget, 512–16,384 initial voices and much larger note/family/decision ceilings; these reservations are not the streaming budget. Conflux runtime allocation increases stage RSS by about 31 MiB.

**Cause:** startup pool sizes ignore active streaming source count and realistic concurrent voices, and an empty stream plan still allocates its default pool. V1's 1,024 × 8,192 rings reserve 64 MiB virtually, but `alloc_zeroed` leaves untouched pages lazy and idle pages can be reclaimed (`v1 stream.rs:350`, `:147`); four decoder workers are process-shared. There is no whole-library sample mmap in either loader: v1 uses positional file reads and v2 buffered reads. `madvise` here concerns ring pages, not mapped preset/sample loading.

**Fix:** bypass streaming allocations for zero-source parts; size a smaller initial page pool from concurrent streaming demand and grow off the audio thread. Measure actual note/voice bookkeeping before reducing limits; retain the existing safe growth/refusal accounting. Share control-side decoder resources where appropriate rather than create four workers for every part regardless of demand.

### 9. P1 — Displayed sample residency overcounts packed head storage (S)

**Evidence:** `sampler-core/src/prepare.rs:213` counts `head_frames × sizeof(stereo f32)` even when head storage is packed 16/24-bit or mono. `:143` exposes actual `head_bytes`, and eviction already uses that actual quantity (`sampler-kontakt/src/stream.rs:538`). `src/sound/mod.rs:197` sums the inflated counter for the plugin. Round 3 Conflux reports **36.27 MiB** versus **29.82 MiB** actual packed heads + pool. Areia reports **468.12 MiB** versus **357.09 MiB**, an overstatement of **111.03 MiB**. These are counter differences, not additional RSS allocations.

**Cause:** a frame-count estimator is used as retained-byte accounting, so the displayed amount and eviction's budget math refer to different representations.

**Fix:** use packed `head_bytes` plus actual resident f32/mipmap storage and the page pool; count shared backing storage once for a global budget. Add one mixed mono/stereo 16/24/f32 check that proves displayed bytes equal storage and remain correct after purge/reload. Do not multiply packed bytes by eight again.

### 10. P1 — Startup preload assumptions differ from the renderer's admitted consumption (M)

**Evidence:** the plugin passes default policy at `src/sound/v2.rs:1071`/`:1106`, which assumes **64-frame blocks, 2 semitones extra pitch and step ≤4** (`stream.rs:270`). The plugin renders up to **128** frames (`src/sound/mod.rs:33`) and the resampler admits step **16** (`sampler-core/src/resample.rs:5`). The policy sizes from p95 of the first 32 warm source reads (`stream.rs:438`). Recorded base head horizons are about 544–620 output frames (11–13 ms); actual per-zone packed ranges depend on pitch/start offsets and need not equal that base. V1's ordinary target is **4,096** frames (128 × maximum step 32), falling to **1,024**, then **256** under budget (`v1 bank.rs:37`). This largely explains why v2's large-library sample RAM can be lower.

**Cause:** startup uses a generic default rather than actual host block/pitch/concurrency limits. Warm first-32 p95 does not certify a safe cold-drive/high-polyphony tail. One earlier ANALOG STRINGS run recorded a streaming underrun; later runs did not. This mismatch is confirmed, but that isolated underrun is not proven to arise from it.

**Fix:** pass the actual maximum service block and pitch/step contract into policy; align the horizon with the renderer and measure representative source families/cold latency under polyphony. Keep adaptive small heads where safe; increasing every preset to v1's 4,096 frames indiscriminately would erase measured RAM wins. Test 128-frame in-block starts, maximum admitted transposition, sustain/release, cold reload after trim and more simultaneous streamed voices than the fixed 256-page-policy assumption.

## Other established observations

- The production load report has essentially one `prepare` timer (`src/plugin.rs:1145`), hiding the difference between decrypt, script init, source opening, sample residency and editor artwork. Keep lightweight numeric milestones and actual first-note/streaming counters for future regression measurement (P2, S); publication is not first audio.
- Allocator retention matters, but is not the main Conflux explanation: round 3 trim releases only 9.3 MiB there. It releases 393.2 MiB after ANALOG STRINGS' editor closes. V1 explicitly calls `audio::trim_heap` after several compile/preload/residency operations (`v1 audio.rs:38`, `engine/script.rs:128`, `bank.rs:714`); v2 has no corresponding production call. Measure live allocations before considering control-thread trimming; never add allocator work to audio callbacks.
- Native fidelity is not certified: the presets emit nonzero audio, but v1/v2 peaks/articulation state differ; Augmented Orchestra reaches peak 4.39 in one run. Baseline v2 reports many unsupported/missing-feature entries, and a `decoded.missing` vector is not automatically a missing-audio-file count. Do not call lower memory/shorter loads equivalent if decoding/runtime coverage differs.

## Unknowns and the next measurements

1. **Native click-to-speaker/UI-present timing:** replay the manifest in a controlled CLAP host or the authorized native reference recorder with timestamped browser input, loader dispatch, publication/install, first audible device block and first GPU-presented scene. Include a stopped host. This audit exercises real production loading/core code through a test harness, not a captured DAW session.
2. **The entire Conflux Big Screen multi:** program 0 here is only its controller. The browser synchronously calls `read_multi` in `src/ui/mod.rs:635` and inserts every program at `:558`; measure that parse/UI stall plus the full sequential rack and scripts, first audible part, all parts ready and peak RSS. Do not treat its silent 18/203 ms program-0 run as full-multi parity.
3. **Cold storage and realistic polyphony:** OS caches were deliberately not flushed on a shared machine. Use an authorized isolated cold reference run and the CPU/streaming agent's sustained MIDI render workload, with p99/p999 latency, streaming failures and per-source decoded bytes. Neither this C4 probe nor four idle wall seconds proves sustained streaming quality.
4. **Repeated retained instances:** measure one/two/four instances of the same preset, then another program in the same UVI bank, while prior instances remain alive. Confirm exact cache-hit identities, sharing and invalidation after library replacement. A new process on each row cannot exercise weak in-process sharing.
5. **Conflux script hot loop:** profile numerical interpreter opcode/function/loop counts in memory under empty and actual NCKP environments; persist only aggregates. Confirm the exact authored-query mismatch before a KSP correctness fix. No source dump is needed.
6. **UI resource boundaries and lifetime:** repeat the fully ORIGINAL view after prior UI branches merge; measure actual decoded image bytes, intermediate decode/crop peak, GPU upload memory and UI-close trim behavior. The current CPU scene harness does not include GPU/driver allocations and follows baseline presentation selection.
7. **Automatic memory control:** render at least 30 seconds of audio clock with active/idle parts, exercise explicit budgets and two plugin instances, and sample both packed counters and process RSS. Measure real eviction/reload behavior and any subsequent first-note delay.

## Prior work

- `origin/v2/load-speed` is absent from fetched remote refs on this date. Local `v2/load-speed` / `inv-load-speed` is at `fb456b41` and has only an uncommitted 51-line `src/plugin.rs` probe. The original probe's `first_playable_ms` watches a report/tree publication; it never sends a note or builds the editor. There is no implementation commit to merge. This audit records publication and actual audio separately.
- `origin/v2/stream-storage@bd3b92efc682c363c2a55017e533036a5ed3ad18` is already an ancestor of frozen `7e82b152`; its disk-streaming implementation is present. It is incorrect to say v2 lacks streaming, or that merging this branch again will fix startup.
- `origin/v2/gpt-kontakt-ui@0be9ed3f174d8925f1d8a142d3f709e5e26b840a` has four unmerged commits relative to this baseline. `UI_V1_PARITY.md` covers resource lookup boundaries/NICNT routing, scalar recall/redraw and passive rendering, plus unavailable Komplete UI detection. It explicitly lists synchronous first asset decode as still worse and Conflux measurements as pending. Merge its established fixes, then extend artwork loading; it is not a completed asynchronous-load optimization.
- `origin/v2/gpt-uvi-ui@24c70a25031243d002e24c661c1afbb1408653aa` has two unmerged commits: bank-local images/fonts, widget/control services and retained UI state. Merge the existing UVI resource adapter rather than write another one. Its optimized census/matched performance measurements remain pending; it does not establish a load-time win.
- The shared census directory had a v2 scanner but no published `results/{v1,v2}.tsv` when inspected during this audit. This fixed 14-preset numeric plugin probe is a supplemental stage/RSS benchmark, not a competing corpus collector. Reader/format fixes in the brief remain coordinator-owned integration work; this branch changes no decoding behavior.

## Reproduction and validation

Build this branch with `~/.cache/kontakto-heavy cargo test --profile ci --no-run`. Apply `load-v1-probe.patch` to an **owned** checkout of `0cb7a8a0`, then build with `cargo test --profile ci --lib --no-run` through the same wrapper. The historical patch pins the importer identity to the untouched baseline hash **e5923baf64b09b21**; adding timers otherwise invalidates the existing cache and produces a misleading cache-mode comparison. The wrapper chooses target directories; do not export target/wrapper variables yourself.

`tools/audit-load.py MANIFEST OUT_DIR TEST_BINARY MODE START END REPEAT` runs a resumable shard; wrap every invocation in `kontakto-heavy`. Use `load-libraries.tsv`, an owned output directory, and `v1user`, `v1fresh`, or `v2`. Set `KONTRA_UVI_READER=/home/derpcat/.codex/cache/kontakto-uvi-official-reader/app/UVIWorkstationx64.exe`. Use `.ufs/Presets/...uvip` plugin paths; corpus `.ufs::Presets/...` notation must be converted, otherwise the production plugin just fails to open the artificial path. Do not exceed roughly five minutes per shard.

The runner uses a read-only bubblewrap root/cache, an inaccessible `TMPDIR`/log path and a volatile `/tmp`; raw plugin stdout is captured in memory and only whitelisted numeric metadata is saved. V1 caches are read-only. No decrypted script, XML, sample, artwork or recovered content state is included in this repository. Only the metadata manifest, sanitized measurements and probe patch are delivered. Earlier pilot rounds allowed transient tmpfs logs; none survived the isolated processes. Round 4 prevents those logs as well.

Validation: optimized root `cargo test --profile ci --no-run`; historical probe build; existing synthetic WAV loader/handoff integration unit; `python tools/audit-load.py --self-check`; and `git diff --check`. No performance fixes or new dependencies were added. Failed historical pilot probes and the importer-hash-invalid cache-mode rounds are excluded from the headline comparison.

