# Attack preload and first-tail streaming evidence

Three owned Analog Strings snapshots now produce nonzero audio through the real V2 loader, script activation/effect host and renderer with bounded resident heads. This does **not** establish dropout-free playback or preset-switch equivalence: Harmonics retained eight first-tail underruns in one run and one in a separate fresh-owner worker-audit run. The 300-voice / 1% KONTRA-meter target remains open; this investigation is distinct from [CPU_EVIDENCE.md](CPU_EVIDENCE.md).

## Why the reported 7.8 GB request retained no heads

The request was optional sample-start modulation coverage, not mandatory full-resident audio. Actual pre-fix V2 Auto geometry for all three snapshots had 23,595 physical assets, 95,624 zones, no aliases, no mandatory-full assets and no pinned wavetable assets. Fully decoded sources would occupy 50,156,846,256 bytes; the independent streamed page pool was 100,663,296 bytes.

| Snapshot | Requested output horizon | Optional requested source frames | Offset-free source frames | Saved exact-zero start-route occurrences |
| --- | ---: | ---: | ---: | ---: |
| Analog Gates | 631 | 3,897,329,953 | 33,177,853 | 58,520 |
| 9001 Punch | 621 | 3,897,021,648 | 32,641,011 | 72,571 |
| Analog Harmonics | 1,400 | 3,907,660,176 | 68,729,057 | 67,042 |

These are merged per-asset request geometry before decode. Two bytes per source frame is a lower packing bound, not actual resident cost. The full offset union therefore required at least 7.79–7.82 GB even before decoding. The previous typed-capacity fallback retained mandatory sources only; these snapshots had none, so the accepted fallback had zero heads. It did not allocate 7.8 GB.

The offset-free Gates request fits even at eight bytes/frame (265,422,824 < 268,435,456 bytes), as does Punch (261,128,088). Storage-latency-derived horizons vary between loads, however: a later Gates load requested 1,763 frames and again exceeded the quota even without offsets. Arbitrary script offsets and all modulation offsets cannot be assumed warm.

`573f7799c` adds opt-in numeric `stream_preload_geometry` records; `9363f43cb` tries complete authored base attacks after a typed offset-coverage quota failure. Required-full and pinned wavetable sources retain their requirements. Original offset requests remain authoritative for full-coverage reporting and explicit reload.

`c417f76fc` additionally selects a shorter uniform attack horizon when complete base attacks exceed the quota. The binary search evaluates only metadata and a conservative eight bytes/frame bound against available shared physical quota, preserving interpolation guards, reverse endpoints and disjoint starts. It performs one final preparation after the search. Packing/publication still pass exact shared-quota admission; mandatory or minimum-anchor failure stays a typed error with no partial publication. `StreamStartup.selected_anchor_head_frames` and anchor coverage are separate from the original requested onset coverage. The quota remains 256 MiB.

## Actual sounding receipt

Frozen optimized `ci` helper SHA256 `e2e5cc527189d3bfc7f180e93548ab800ca7a5f8c6f9f72e071f43ec2bee2d70`, 48 kHz, 256-frame host blocks rendered in 128-frame chunks, actual `V2Loader` default Auto. Each snapshot used a fresh process and owner, bounded real read-only UI-effect activation, then eight exact CLAP note IDs at keys 60–67. Audio remained in RAM.

| Snapshot | Original requested horizon | Published attack coverage | Packed payload bytes | First-burst physical voices at end | Peak | First nonzero output offset | Cold starts | Underruns |
| --- | ---: | --- | ---: | ---: | ---: | ---: | ---: | ---: |
| Analog Gates | 2,065 | 39,420 / 39,420 shorter anchors, 639-frame horizon | 168,685,347 | 5 | 0.721928 | 2,048 frames | 5 | 0 |
| 9001 Punch | 890 | 39,420 / 39,420 original base attacks | 225,125,291 | 16 | 2.382340 | 0 | 8 | 0 |
| Analog Harmonics | 612 | 39,420 / 39,420 original base attacks | 163,987,559 | 14 | 0.226114 | 0 | 0 | 8 |

All three first bursts recorded zero callback allocation/deallocation calls, non-finite frames, voice-slot refusals, streaming-credit refusals and aggregate capacity drops. This does not explain the user's earlier 26/3 aggregate drops: those were not reproduced by this bounded chord.

Activation observed the exact main script's persistence callback finish, along with PGS/listener work. The main source hash matched exactly one physical-slot-2 view / dense instance 0; all 47 emitted typed guard entries had valid compiled homes and no read errors. Guard values were unchanged across activation. The preceding zero-voice receipt omitted UI-effect service and is **rejected as an audio witness**, rather than attributed to DFD or snapshot-value restoration.

These are functional measurements under compiler/external workload contention and an instrumented observer. They are not whole-plugin meter measurements or isolated timing acceptance. The observer's hash queries can affect wall pacing. Gates' first-signal delay includes authored callback/gating behavior and cold starts; it is not a measured disk-only latency.

The helper's second bursts retained existing authored voices after its host choke/warm-up: baseline voices were 5/16/14 and the held-key flag remained set. Therefore second-burst energy is overlapping audio, not proof of isolated fresh attacks, complete note retirement or a repeated preset transaction. New second-burst cold-start deltas were 0/7/0, with no additional underruns in this particular run. Parent lifecycle witnesses own actual preset replacement and state preservation.

## Retained first-tail fault and worker attribution

In the sounding three-snapshot run, Harmonics' eight warm-admitted voices from source zones 46474–46481 first starved during block 1. Their current reads were around absolute source frames 1,616/1,657, page 0 remained `Pending`, and those ranges were no longer covered by resident heads. Block-1 callback time was about 698 microseconds versus a 5,333-microsecond audio span. By block 2 all 14 current demands were ready. This identifies source starvation, not an observed callback overrun.

A separately approved fresh-owner worker-audit repeat also sounded from output offset zero, but retained **one** underrun. It loaded all 39,420 original base attacks in 217,963,092 bytes. The missed voice was source zone 46481, page 0, at block 4 / source position 2,276. Its job was queued at engine frame 96,512; queue wait was 26 microseconds, open took 9,362 microseconds wall / 36 microseconds thread CPU, and read/decode took 907 microseconds wall / 22 microseconds CPU.

The same asset's page-1 and page-2 jobs launched on other workers and each reopened it (9,342/9,323 microseconds wall). Across four workers: 309 jobs, 46 opens, zero decode failures, zero reply backpressure, zero reader evictions and zero omitted probes; retained-reader peaks were only 325–350 KB per 2 MiB worker quota. Queue maxima were 5.7–6.24 ms and the longest job 21.24 ms. Aggregate worker CPU was about 9.65 ms across the entire receipt. This points to off-CPU/storage/scheduling delays and duplicate cross-worker opens, not expensive decoding alone. The repeat overlapped external compilation and a separate UI load; it is not a controlled before/after dropout comparison.

Two hypotheses remain unpromoted:

- Linear demand assigns the outer page fragment's deadline before removing resident head ranges. Partial page-0 tails can therefore receive the same immediate priority as genuinely cold page-0 reads. Direction-correct first-use at the actual uncovered edge could preserve urgency ordering.
- Per-worker reader caches can open one asset independently for concurrent pages despite ample free cache. Bounded source locality may remove repeated opens, but it cannot guarantee the first open meets a short runway. Near-edge start offsets may need an explicit minimum runway/priming rule rather than immediate warm admission.

No production scheduling or onset-holding change is justified solely by these two contended receipts. Exact source-start/pitch metadata, worker ordering oracles, independent fresh-owner repeats and boundary tests are needed before promotion.

## Effective onset and codec metadata follow-up

Source-bound, initialization-free metadata for the exact eight Harmonics zones identifies a Constant SampleStart route with depth 0.0041315858 and start range 180,479 frames. The current voice-start law truncates its offset to 745 source frames. Group tuning is approximately one octave, which consumes the resident remainder faster than unity playback. This is distinct from the helper's zero script-offset-microseconds field. It explains why covering the authored base onset alone does not prove an effective-onset runway; the exact live cursor observations remain the stronger runtime evidence.

`Prepared::constant_start_offset(region)` now exposes a control-only proof through the same route accumulation consumed by actual voice starts. Any dependent SampleStart source or depth scale returns `None`, even at saved zero depth. Unrelated routes do not defeat the proof. This preserves the current runtime law; it does not establish new Kontakt-native SampleStart calibration or admit previously rejected native intensity laws.

The opt-in geometry record adds `constant_onset_hints`: non-loop effective windows, their merged union with original base attacks, physical-asset deduplication, dependent/omitted-loop counts and two/eight-byte payload bounds. Bounds are clipped to the authored source view; offsets past its end are silent. Looped and wavetable hint geometry is explicitly omitted rather than replaced by a linear guess. The query/audit checkpoint changes neither preload selection nor onset timing; no hint-readiness or fault-elimination claim follows from it.

A single later frozen helper load (SHA256 `1f5f622bfaa797de3b7a79e7e52cf0674468ab41192ce258435ab2a25d769f7f`, source cohort `5772c4f1`) measured the actual Harmonics geometry at a requested 1,700-output-frame horizon. All 95,624 zone offsets proved Constant in the current prepared plan; 70,961 evaluated to zero, 22,052 positive non-loop windows were eligible, and 2,611 positive looped windows were explicitly omitted. Unique base coverage was 82,496,595 source frames. The 14,177 hint ranges over 11,086 physical assets added 11,144,303 unique source frames, giving a base-plus-hint union of 93,640,898. Payload bounds are **187,281,796–749,127,184 bytes**, straddling the 268,435,456-byte quota; the lower bound does not prove admission. Exact packing, or a shorter selected horizon, is still required.

This read-only load used a fresh private dev-drive metadata cache (header-cache miss), completed bounded script activation, and supplied no musical input. It observed zero active voices; its zero runtime faults are not an audio witness. No compiler was observed, but unrelated `pure_va` test processes were active, so loading wall time is not a performance result. The raw numeric receipt and derived classification note are `harmonics-constant-hints-audit.json` and `harmonics-constant-hints-analysis.json`; no source, UI report or PNG payload was retained.

Optimized `ci` validation passed three new actual-start/PCM query oracles, 24 existing modulation cases (one ignored), and two geometry/cancellation cases. Coverage includes host partitions 1/7/64/137, dependent zero-depth sources/scales, fractional truncation, current native metadata law, reverse/high-step bounds, distinct asset revisions and shared aliases. Failed first fixtures (zero allocated voice slots and an extra non-required reverse guard) remain in private logs with their corrections. `constant-start-query-test-manifest-final.json` records source and executed-binary hashes.

The existing validated numeric header cache classifies all 23,595 Analog sources as member ranges in ten NKX archives, with retained file handles. Consequently the worker's measured `open_us` includes snapshot validation, buffered sniff/refill, codec/header/index parsing and scheduling; it is not an operating-system `File::open` measurement. The library volume is on an NVMe SSD, but the contended wall/CPU difference does not isolate physical I/O.

A full NCW-index cache is not free: the numeric frame counts imply 12,255,356 512-frame blocks, or 49,021,424 bytes of tight u32 offsets, before headers, ownership and cache capacity. Codec identity is absent from that numeric cache, so this is an **all-NCW hypothesis envelope**, not a measured NCW total. The existing parser validates and discards a terminal sentinel. Revision-bound immutable layout/index reuse, or smaller validated index windows around attacks, remains a proposal requiring a declared metadata quota, independent mutable decoder state, unchanged identity checks, exact decode oracles and measured benefit.

One approved compiler-free fresh-owner attempt aborted **before loading** because a Cargo/rustc process was present. It created no cache and produced no new audio receipt. Future probes explicitly use a dev-drive `XDG_CACHE_HOME`; earlier receipts retain their original cache provenance and the user's cache is untouched.

## Counter and verification scope

`05d7aa846` adds O(1) branch-local voice-slot refusals and waiting-voice replacements while preserving the aggregate drop counter. Streaming-credit refusal can increment the aggregate without exhausting physical voice slots. Three admission oracles passed with a heap guard, including failed stream admission preserving an existing waiting voice.

The final Kontakt stream suite passed 32 tests, one ignored. Coverage includes one/four workers, largest guaranteed-fit horizon, high fractional pitch in both directions, zero-output-horizon first-read guards, full/pinned preservation, disjoint attacks, exact minimum-anchor quota fit versus one-byte-short failure, atomic error/cancellation behavior and failed reload preserving existing heads/quota.

Saved exact-zero route occurrence counts do not authorize blanket pruning: KontaktIntensity flag 0x04 can make base zero nonzero, and live bindings can replace saved zero. A proved static zero/Mul-only subset remains a separate geometry optimization.

Raw source-free artifacts are private, not committed: `/mnt/Windows11/DEV_WORKSPACE/PluginArtifacts/kontra-cpu-20261014/analog-adaptive-activated-readiness.json`, its `-manifest.json`, `harmonics-tail-worker-audit.json`, rejected `analog-onset-readiness.json`, and `adaptive-anchors-stream-suite.log`. Initial no-note geometry is in `/mnt/Windows11/DEV_WORKSPACE/PluginArtifacts/kontra-diagnostic-fixes/kontakt-ui-parity-current-evidence.json` (inspector SHA256 `e977ef50f519ad0f8b3a9603ab3584ef504ac178cf91e849a4cbab02a03d0606`). No vendor source, PCM payload or native executable is included in these commits.
