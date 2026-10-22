# Kontakt 8 CPU architecture: what keeps the sampler cheap

Research note. Source: the Kontakt 8 decompile export (`source/kontakt8`, marked READY), read by grep only. No binary was opened and nothing was built. Addresses are image addresses (base `0x140000000`). `file:line` is the export file that holds the address (one file per 64 KiB). The export has 175,503 functions, most still named `FUN_*`, so the identifiers below come from demangled C++ signatures, RTTI-backed vtable constructors, config-key strings, UI strings and source-path asserts.

Confidence tags: **[H]** direct evidence in the export, **[M]** strong inference, **[G]** gap, not established.

## Bottom line

1. Kontakt's packed-SIMD kernels that the export shows are spectral (complex-multiply/FFT, AVX and AVX-512 forms). The per-block voice render chain has no VEX/EVEX instructions in the export.
2. Kontakt's levers are structural: sub-divide host buffers into blocks of at most 1024 frames, run one time-sorted event pass per block, set FTZ/DAZ on render and worker threads, keep structural work off the audio thread under suspend, share prepared sample objects, and gate voice kills on a measured load.
3. The "3x" target cannot be settled from the decompile. Per-voice cycles need the Wine oracle (one session, per the CPU rule). Several things KONTRA already does match Kontakt's structure (voice stealing modes, quiet-voice end, bus effects).

## Method and limits

- Ghidra prints AVX/EVEX instructions as named userops (for example `vfmadd231ps_avx512vl(`), so ISA and width are countable. Legacy SSE packed ops print as generic arithmetic here. "No VEX/EVEX" therefore does not exclude SSE2 packed code in a function. **[G]**
- Counts are over the whole export unless a function is named. Function-level counts were taken from the `// ====` headers.

## What Kontakt does, by question

### 1. Block size and per-block vs per-sample work

- Host buffer `n` is split into `ceil(n/1024)` sub-blocks. Each sub-block is `ceil(n/nsub)` frames, capped at `0x400` (1024). `0140d30000.c:6905-6935`, function `0x140d38620`. **[H]**
- When `n <= 1024` there is one sub-block and no split. `0140d30000.c:6905-6913`. **[H]**
- The instance wrapper `0x140d38fc0` (`0140d30000.c:7099`) calls the block routine with the instance and its sub-object. **[H]**
- Per block, before the render routine runs, two passes run: `0x140952580` (`0140950000.c:490`, walks a time-ordered linked list of events) and `0x14094fe20` (`0140940000.c:11479`, 7 KB, duties not recovered). Call sites `0140d30000.c:6910-6911`. **[H]** for the calls, **[G]** for the second pass's duties.
- Event times look like 1/1024-frame ticks: the list head is compared against `frames * 0x400` (`0140950000.c:515`). **[M]**
- The render routine `0x140906000` (`0140900000.c:4971`) takes the sub-block length, stores it at instance `+0x2538`, and then calls a virtual method. Call sites show only two arguments (Ghidra prototype artefact). **[H]**
- Cost meter: after each block, `(elapsed - t0) / window` is computed and the peak is kept in a global (`0140900000.c:6561-6582`, function `0x140908b30`). The UI prints it as "DFD load". `01406b0000.c:6142`. **[M]**

### 2. SIMD and ISA

- Userop totals across the export: `_avx` 59,974; `avx2` 15,457; `avx512vl` 24,265; `avx512f` 22,312; `avx512dq` 386; `avx512bw` 30. **[H]**
- Hot packed-float kernels: `0x140284e60` (19,352 B, about 1,500 packed-float ops, 646 FMA), `0x1402e8be0` (19,579 B, near-identical twin), `0x140289a00`, `0x1402ed860`, `0x14028d2e0`. Their pattern is complex multiply and FFT: `vaddsubps` with `vmovsldup`/`vmovshdup`. Files `0140280000.c`, `01402e0000.c`. **[H]** for ops, **[M]** for "FFT/complex".
- 512-bit (ZMM) code is concentrated in `0x14039840d` (114 ZMM refs, `0140390000.c`) and `0x14038a30d` (88 refs, `0140380000.c`). **[H]**
- Spectral code is not uniformly vectorized. The zplane FFT butterfly `0x1419c4c70` (`01419c0000.c`) has no vector ops. **[H]**
- Convolution: `NI::DSP::Convolver_FFTW` (`0140a80000.c:4123`), zplane FFT classes (`0x1419c2xxx`), config key `convolutionBlockSize` (`0140920000.c:3792`), and "Asynchronous mode is always used for Convolution engine parameters" (`0140780000.c:5778`). **[H]**
- Phase vocoder: `CTBuffer<CPhaseVocoderV3::ZplTransientState_t>` appears as a template argument. **[H]**
- The voice render chain has no vector ops: `0x140d38620`, `0x140952580`, `0x14094fe20`, `0x140906000` all show zero AVX/AVX2/AVX-512 userops. They are integer-pointer-heavy (about 650 integer-pointer ops in `0x140906000` against about 100 scalar float ops). **[H]** for "no VEX/EVEX", **[G]** for SSE2 packed.
- Runtime dispatch: a CPUID-based feature mask is built once (`0x14002f560`, `0140020000.c:14241-14379`). It reads leaves 1 and 7, gates AVX on OS support, and sets AVX-512 class bits. A has-all-features test `0x14002f960` (`0140020000.c:14393`) has two call sites (`0140010000.c:44981`, `0140010000.c:45287`), and their arguments are not recovered. **[H]** for the mask, **[G]** for the kernel-selection sites.

### 3. Resampling, interpolation and baked tables

- UI quality menu: Lo-Fi, Medium, High, Best (`0140630000.c:1883-1886`, module not identified). Config keys `interpQuality`, `interpQualityOffline`, `hiQuality` (`0140910000.c:11901`, `0140920000.c:7690`). Script parameter "High Quality Interpolation" accepts 0..2 (`0140780000.c:6138`). **[H]**
- Real-time interpolation kernel, tap count and table layout: not located. **[G]**
- Baked nonlinearity tables: per-stage `WaveShapingTables::TableBase` classes for about ten amp and distortion models (VAN51, HOTSOLO, JUMP, ACBOX, DSTORTION, CAT, BASSPRO, FuzzFace, BigFuzz), each with per-stage subclasses. **[H]**
- Window generator: `ni::dsp::window::hann_gen` (vtable name list). **[H]**

### 4. Control-rate vs audio-rate

- Modulation is organised as per-block objects: LFO families (`BLfoSine`, `BLfoTriangle`, `BLfoMulti`, ... each with a `WithTargets` variant), `BGlide`, envelopes `BParEnv_AHDSR`, `BParEnv_DBD`, `BParEnv_FM7`. **[H]** (class names)
- Smoothing parameter range 0..20 (`0140580000.c:8922`). **[H]**
- The per-block pass runs before the render call (`0140d30000.c:6910-6911`), so modulation is most likely evaluated per sub-block, not per sample. **[M]**. Exact law: **[G]**.

### 5. Per-group vs per-voice effects

- Effects sit on insert buses and group outputs: `BInsertBus`, `BGroupOutputRoutingBase` (33 vtable refs), `BEffectSends`, `BParFXSendLevels`, `BDsp::BGroupReminder` arrays, and `BEngine::UpdateFXChannelConfiguration`. **[H]** (class names)
- No per-voice effect object was found. **[M]**

### 6. Voice stealing, idle detection, denormals

- Five kill modes: Any, Oldest, Newest, Highest, Lowest (`0140580000.c:4114-4118`, `0140930000.c:91-114`). **[H]**
- Overload gating: "Relaxed (start killing voices if CPU load is very high)", "Medium (... high)", "Strict (... somewhat high)" (`0140580000.c:6236-6238`). Config key `overloadVoiceKillMode` (`0140730000.c:6305`). Per-instance stat "Voices killed (CPU / DFD overload)" (`01406b0000.c:5934`). **[H]** for the modes; the numeric thresholds are not in the export. **[G]**
- Voice limits: `BVoiceLimit` (vtable), `maxNumVoices` (`0140930000.c:134`). **[H]**
- Kill priority range 1..1000 (`0140790000.c:8906-8912`). **[H]**
- Steal fade time: "voice stealing fadeout time", bounded at 1000 (`01410a0000.c:3523-3525`). Unit not stated in the export; likely ms. **[M]**
- Idle or silent voice skip: no routine identified. **[G]**
- Denormals: the render routine sets FTZ and DAZ (`MXCSR |= 0x8040`, `0140900000.c:5092`, function `0x140906000`). A per-worker wait routine does the same (`0140820000.c:4367`, function `0x140825aa0`). **[H]**
- 79 guard sites in 14 files normalize MXCSR to `0x1f80` (FTZ=0, DAZ=0, all exceptions masked), for example `0140030000.c:514-516`. These are math helpers that want IEEE behaviour. **[H]**

### 7. Streaming (DFD = Direct From Disk)

- Options: `numStreamingVoices`, `streamingBufferSize` (`0140910000.c:7645-7650`), `dfdChannelPreloadSize` (`0140920000.c:8258`), `overrideInstPreloadSize` and `overrideInstDFD` (`0140730000.c:6385-6386`), `loadSamplesInBackground` (`0140730000.c:6376`), `simulateK2DFDVolModulation` (`0140920000.c:6348`). **[H]**
- Stats: "DFD load %", "DFD memory: MB", "DFD requests" (`01406b0000.c:6142-6209`). **[H]**
- Request accounting: a routine keeps a request counter, a timestamp and a pending-sample count (`0140900000.c:6561-6582`, function `0x140908b30`). **[M]**
- DFD expansion as "Direct From Disk" is taken from `docs/architecture-v2/DFD_READINESS_EVIDENCE.md:8`. **[H]**
- Ring and buffer-reuse details: **[G]**.

### 8. Threads

- Structural edits run inside `SuspendEngine(bool,bool)` and `ResumeEngine` brackets. **[H]** (method names)
- Structural jobs take a `SyncedThreadingJobManager`: `BDoubleChainedArray<BVoice>::ReAlloc` (also `BDsp::BGroupReminder`), `BSamplePool::AttachSample`, `BProgram::AddGroup` and `AddZoneImpl`, `BInsertBus::setMuteOutputDelaySynced`. **[H]**
- Voice storage is a chained array (grows in blocks), so voice start does not reallocate. **[M]** (name only)
- Meters and settings: "CPU Load (N cores used)" (`01406b0000.c:1491-1494`), "%i cores" (`0140580000.c:6070`), `numCPUs`, `numCPUs_AU`, `numCPUs_RTAS` (`0140720000.c:3828`). **[H]**
- A Win32 thread pool exists (`0x142a86b40`, `0x142a87610`, `0142a80000.c:7785`), but whether the engine uses it is not established. **[G]**
- Voice-parallel rendering: **not evidenced**. The job manager covers structural work only. **[G]**

### 9. Caching prepared sample and zone data

- Sample pool items shared across zones: `BSamplePool`, `BSamplePoolItem`, `BPoolMem`. **[H]**
- Zone prepare: `BZone::PrepareSlices`. **[H]**
- Zone level envelopes stored as precomputed segments: `BZoneLevelEnv`, `BZoneLevelEnvSegment`, `BEngine::InsertZLERange`. **[H]**
- Modulation tables and curves: `BModTable`, `BModCurve`. **[H]**
- "Updating sample data cache..." (`0140960000.c:3126`). Options `disableMemMap`, `disableOSDiskCache`, `LibrariesCache`. **[H]**
- Compressed sample readers: `BFileCompressed`, `BFileCompressedMem`. **[H]**

## Techniques KONTRA should adopt

Each entry gives the Kontakt evidence, the KONTRA file (under `crates/sampler-core/src/` unless stated) and the action.

**T1. Amortize per-block work over up to 1024 frames.**
Kontakt: sub-blocks of at most 1024 frames, balanced (`0140d30000.c:6905-6935`).
KONTRA: `dsp.rs:1240` `BLOCK = 64`, `v1_voice_controls.rs:8` `MAX_BLOCK = 128`, `render.rs`.
Action: keep the 64-frame granularity where voice stealing or modulation needs it, but run the per-voice control setup once per up-to-1024-frame sub-block. Measure the cost of the 64-frame setup first.

**T2. One time-sorted event list per block, with fixed-point sub-frame ticks.**
Kontakt: list walked per block, ticks at 1/1024 frame (`0140950000.c:490`, `0140940000.c:12470`).
KONTRA: `schedule.rs`.
Action: store event times as integer ticks (1/1024 frame) and consume the list head once per sub-block. Avoid per-sample event checks.

**T3. FTZ and DAZ on audio and worker threads, not per-sample offsets.**
Kontakt: `MXCSR |= 0x8040` on render and worker entry (`0140900000.c:5092`, `0140820000.c:4367`).
KONTRA: `dsp/lanes.rs:455-456` flushes histories per block; `dsp/color/distortion.rs:58` adds `ANTI_DENORMAL = 1e-20` per sample.
Action: set FTZ/DAZ at audio-callback entry and on pool worker start (`sampler-pool`, `parallel.rs`). Then drop the per-sample offset where FTZ covers it. This changes numbers, so regenerate golden vectors and keep the bit-exact tests on a fixed mode.

**T4. ISA dispatch from one CPUID mask, with AVX-512 variants only where they pay.**
Kontakt: one CPUID mask (`0140020000.c:14241-14379`), twin kernels, 512-bit used in a few kernels only (`0x14039840d`).
KONTRA: `crates/sampler-simd` `dispatch` is used by `dsp/eq_family/kernel.rs:123`, `dsp/space/convolver.rs:204`, `source.rs:1456`, `parallel.rs:665`.
Action: keep one dispatch point. Benchmark AVX2 vs AVX-512 on the voice mix before adding AVX-512 variants.

**T5. Keep convolution spectral and asynchronous, with a block-size setting.**
Kontakt: convolution always async (`0140780000.c:5778`), `convolutionBlockSize` (`0140920000.c:3792`), FFTW-based convolver (`0140a80000.c:4123`).
KONTRA: `dsp/space/convolver.rs`.
Action: partitioned convolution on the worker pool, partition size exposed as a setting, output latency reported.

**T6. Structural work off the audio thread under suspend.**
Kontakt: `SuspendEngine`/`ResumeEngine` around edits; `SyncedThreadingJobManager` for voice-array realloc, sample attach, zone and group add (see section 8).
KONTRA: `allocation.rs`, `grow.rs`, `prepare.rs`, `sample_revision.rs`.
Action: voice storage as chunked arena grown only from control context. Sample attach and zone prepare run as jobs; the audio thread only swaps published revisions.

**T7. Voice stealing with three CPU-gated overload levels.**
Kontakt: five kill modes; Relaxed, Medium, Strict thresholds (`0140580000.c:6236-6238`); `overloadVoiceKillMode` (`0140730000.c:6305`); priority 1..1000 (`0140790000.c:8906`); steal fade bounded at 1000 (`01410a0000.c:3523`).
KONTRA: `steal.rs` (`steal_voices` at line 154, `enforce_voice_limits` at line 176), `digital_fade.rs`, `release.rs`, `docs/re/engine/k-voice-engine.md` (kill enum 0..4 already mapped).
Action: add an overload mode (Relaxed/Medium/Strict) that gates kills on the T8 load meter. Keep the kill enum and fade clamp at 0..1000 ms.

**T8. Peak load meter from per-block wall-clock time.**
Kontakt: peak of `(elapsed - t0)/window` per block (`0140900000.c:6561-6582`), shown as "DFD load" and used by the overload kill (`01406b0000.c:6142`).
KONTRA: `performance.rs` (669 lines), `head_budget.rs`.
Action: one cheap timer per block, peak ratio over a window, feeding T7. Report it in the diagnostics like Kontakt's stats.

**T9. Streaming budget as explicit knobs with request counters.**
Kontakt: `numStreamingVoices`, `streamingBufferSize`, `dfdChannelPreloadSize`, per-instance preload override, request and memory counters (`0140910000.c:7645`, `0140920000.c:8258`, `0140730000.c:6385`, `01406b0000.c:6142-6209`).
KONTRA: `stream.rs` (`PAGE_FRAMES = 4096`), `residency.rs`, `residency/startup.rs`, `head_budget.rs`.
Action: expose the same four knobs per instance and count requests and pending samples. Reuse the page size; the Kontakt page unit is not recovered.

**T10. Shared prepared sample objects, precomputed per zone.**
Kontakt: shared pool items (`BSamplePoolItem`), `BZone::PrepareSlices`, precomputed level-envelope segments, cached sample data (`0140960000.c:3126`).
KONTRA: `resident_data.rs`, `prepare.rs`, `prepare/selection.rs`, `source.rs`.
Action: key pool items by content and revision, precompute slices and envelope segments in prepare, and keep the audio thread free of IO and decode.

**T11. Baked per-stage nonlinearity tables.**
Kontakt: `WaveShapingTables::TableBase` per amp stage (about ten models) (section 3).
KONTRA: `dsp/color/distortion.rs`, `blocks/step_table.rs`, `blocks/tables/`.
Action: build stage transfer tables at prepare time and look them up with linear interpolation. Measure table size against the cost of computing the curve.

**T12. Reuse phase-coefficient tiles per block in the resampler.**
Kontakt: four quality levels and a high-quality mode (section 3). Kernel not recovered.
KONTRA: `resample.rs`, `resample/cubic_block.rs` (4-lane cubic, scalar bit-exact test), `resample/coefficient_reuse.rs` (`FRAMES = 64` tile).
Action: keep the coefficient-tile reuse (it is the baked-table idea applied to the resampler). Map KONTRA's quality levels onto Kontakt's names only after an oracle measurement.

**T13. Modulation once per sub-block, not per sample.**
Kontakt: per-block modulation objects and a pre-render pass (`0140d30000.c:6910-6911`); smoothing 0..20 (`0140580000.c:8922`).
KONTRA: `blocks/lfo.rs`, `blocks/glide.rs`, `blocks/envelope.rs`, `modulation.rs`, `voice_mod/laws.rs`.
Action: advance LFOs, glides and envelopes once per sub-block and ramp the result linearly across it. Confirm the Kontakt law with the oracle before changing the laws.

**T14. Effects on buses, not on voices.**
Kontakt: insert buses, group output routing, send levels, channel-configuration updates (section 5).
KONTRA: `bus.rs`, `groups.rs`.
Action: confirm that voices only sum into group buses and that effects run per bus. Keep it; this is already the Kontakt shape.

**T15. Quiet-voice end.**
Kontakt: no idle or silence routine identified (section 6). **[G]**
KONTRA: `render.rs:160-182` ends a releasing voice after `QUIET_FRAMES = 512` frames below `INAUDIBLE = 1e-6`.
Action: keep it. Measure its cost against Kontakt on the oracle (section 9 below).

## Gaps and next measurements

- **Voice render inner loop and interpolation kernel**: not located. Recovering them needs the vtable slot map for the voice class, which the export does not give. **[G]**
- **Idle and silence skip**: not located in Kontakt. **[G]**
- **Voice-parallel rendering**: not evidenced. **[G]**
- **Legacy SSE2 packed code in the voice path**: cannot be counted from this export. **[G]**
- **Numbers**: overload thresholds, streaming defaults, and the unit of the steal fade are not in the export. **[G]**
- **3x target**: needs per-voice cycles from the Wine oracle, one session. Suggested cases: one sustained note, 64 notes of one sample, one streamed sample with preload, and one convolution instance. Compare against KONTRA's `MULTICORE.md` figures (1024 voices x 4 SVF at +7 st, 64-frame blocks: 1264 us wall at 1 thread, 317 us at 8).

## Verification

Docs only. No build, test or binary run. Evidence is grep output from the export plus repo file references listed above.
