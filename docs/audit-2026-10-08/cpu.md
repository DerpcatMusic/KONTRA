# CPU and streaming audit — 2026-10-08

Audit baseline: v2 `7e82b152b46b31e8b9fd85c2ac01d01dad669ddd` (`origin/integrate/core-v2`); v1 `0cb7a8a0` in the read-only `gpt-kontakt-ui-v1-bench` worktree. Research only: no engine fixes. Probes render in memory; no decrypted library data, samples, scripts or keys are written to disk.

## Verdict

**V2 is worse for audio-thread CPU on the measured piano, layered-string and FX-rich instrument workloads.** At 64 frames, steady-state p50 is **52.951 vs 6.920 µs** for Una Corda (7.65×), and **69.092 vs 3.500 µs** for Vista (19.74×). ANALOG STRINGS is **112.422 vs 45.571 µs** (2.47×). These are the same requested MIDI schedules, not equal-PCM/equal-graph kernel comparisons: selected voices and unsupported features differ. V2's empty-note-arena scanning independently establishes a CPU regression: 16,384 empty slots take 24.770 µs per flush versus 0.080 µs for 64 slots.

**Streaming is present and used by the plugin.** V2 improves sample-head footprint; v1 has no measured underruns; v2 has two in one ANALOG STRINGS/256 unforced-cache run and none in the cold repeats. That does not establish a disk deadline SLA. V2 has regressions/gaps in callback retirement, offline policy, failed-page recovery and matching stream admission to its much larger voice pool. Detailed matrix, cold-cache evidence and profiles are below. Empty native CLAP process is better in v2 (0.900 vs 3.620 µs at 64 frames); parameter flush is effectively equal.

## Method and limits

- AMD Ryzen 7 7800X3D, 16 logical CPUs, Linux, Rust 1.99.0, perf 7.2.9-1. `/mnt/MAIN_STORAGE` is `/dev/nvme0n1p1`, `ntfs3`. No system tracing/cache settings changed. Other agents/builds ran concurrently; scheduler outliers and p99/max are not controlled worst-case estimates.
- `examples/cpu_audit.rs` uses the **same `V2Loader` / `V2Core` sound seam as `src/plugin.rs`**, including streaming, scripts, node mixing, callback fuel cap and note-end handling. The v1 adapter uses `import::read`, script initialization, `Bank::load_counting(Streaming::Auto)`, `effects`, `Engine::set_script` and `Engine::render`. It is the v1 engine path, not the complete v1 rack/plugin shell or background residency manager.
- 48 kHz, 32/64/256-frame host blocks, four seconds of paced rendering. Piano: keys 48/52/55/60/64/67/72/76; strings/FX: twelve keys 48–59. CC1=110, CC11=127, sustain down at onset, note-offs at one second, sustain up at three seconds. Events quantize to block starts identically in both adapters. Both split 256 into two 128-frame sound passes, matching the plugin's maximum render chunk.
- `steady` covers 0.25–1.0 s, before note-offs. `all` includes attacks, release and pedal-up. Per-voice numbers divide each timed block by its live voice count; they include streaming, scripts, bus processing and mix/notification costs and **are not isolated per-voice DSP costs**. Timing includes a matching output peak scan in both adapters. Peak/count/error checks identify silence and refused work; sonic parity was not established.
- Both source adapters use release optimization / thin LTO with symbols retained (v2 `corpus` inherits `release`, v1 isolated manifest overrides symbol stripping); compiler/hardware is shared. Neither adapter is built with an extra target-cpu override.
- Counting allocator covers event delivery and rendering, including frees. Timing arrays, JSON and pacing are outside the counted section. Probes write aggregate metadata only. Perf samples only instruction pointers, **not copied user stacks**.
- V1's parsed-instrument cache can serialize decrypted scripts/IRs. `XDG_CACHE_HOME=/dev/null` makes cache writes fail with ENOTDIR without changing v1. V2 has no corresponding parsed-instrument disk cache on this path. Load times therefore do not compare production v1 cache-hit startup.
- Cold tests use `posix_fadvise(DONTNEED)` on only the selected library's original NKI/NKR/NKX/NCW/WAV files, verify pages with `mincore`, and immediately load/render inside the same heavy slot. This tests cold **Linux file pages**, not a flushed NVMe controller cache or rotational disk. Head loading necessarily warms the onset pages. Other processes can still rewarm shared files.
- Mounted Kontakt libraries include Una Corda, Vista and ANALOG STRINGS. The supplied corpus has **no drum kit**, and the UVI mount contains orchestral/wind banks. A sequence/pulse preset is not a substitute for a drum kit. Drum-kit measurements remain unavailable.
- Every build and real-library run uses `~/.cache/kontakto-heavy`, one own job at a time; each cell releases its slot. V1 source and the integration worktree were not edited.

## Render, residency and cold-cache measurements

Raw per-cell results are in [cpu-evidence](cpu-evidence/). Times are microseconds unless stated otherwise. “Unforced” means the cache was not deliberately evicted; the first 32-frame load of each preset may have been cold. These are one serial measurement per cell, with the scheduler limitations above. Deadline-miss counts cover all four seconds, including attack/release events; they are elapsed callback time exceeding block/48k, not necessarily recorded audio xruns.

### Unforced-cache rendering

| Preset | Engine | Frames | Steady p50 | Steady p99 | p50 / active voice | Loaded idle p50 | Peak voices |
|---|---|---:|---:|---:|---:|---:|---:|
| Una Corda | v1 | 32 | 4.581 | 13.890 | 0.437 | 0.150 | 19 |
| Una Corda | v2 | 32 | 44.671 | 67.172 | 5.583 | 25.220 | 12 |
| Una Corda | v1 | 64 | 6.920 | 21.111 | 0.677 | 0.360 | 19 |
| Una Corda | v2 | 64 | 52.951 | 76.652 | 6.618 | 26.060 | 12 |
| Una Corda | v1 | 256 | 16.660 | 40.701 | 1.571 | 0.540 | 19 |
| Una Corda | v2 | 256 | 144.523 | 398.179 | 18.065 | 29.671 | 192 |
| Vista | v1 | 32 | 2.730 | 7.430 | 0.197 | 0.130 | 16 |
| Vista | v2 | 32 | 56.352 | 73.021 | 3.522 | 26.381 | 80 |
| Vista | v1 | 64 | 3.500 | 8.400 | 0.238 | 0.160 | 16 |
| Vista | v2 | 64 | 69.092 | 108.333 | 4.318 | 26.821 | 80 |
| Vista | v1 | 256 | 10.701 | 18.180 | 0.787 | 0.460 | 16 |
| Vista | v2 | 256 | 171.634 | 236.745 | 10.727 | 28.841 | 68 |
| ANALOG STRINGS | v1 | 32 | 20.801 | 55.331 | 0.945 | 0.230 | 22 |
| ANALOG STRINGS | v2 | 32 | 73.921 | 120.162 | 3.080 | 33.731 | 24 |
| ANALOG STRINGS | v1 | 64 | 45.571 | 84.912 | 2.071 | 8.990 | 22 |
| ANALOG STRINGS | v2 | 64 | 112.422 | 233.953 | 4.684 | 47.611 | 24 |
| ANALOG STRINGS | v1 | 256 | 172.513 | 290.346 | 7.188 | 38.051 | 24 |
| ANALOG STRINGS | v2 | 256 | 478.908 | 2125.584 | 19.954 | 161.912 | 24 |

The p50/voice column includes the idle arena floor and shared graph costs; v1/v2 graph and voice selection differ. At 64 frames, p50 block budget use (one core) is Una Corda **0.52% → 3.97%**, Vista **0.26% → 5.18%**, ANALOG STRINGS **3.42% → 8.43%**. Multiply neither these medians nor per-voice figures into a worst-case capacity promise. V2 ANALOG STRINGS at 256 has p50 478.908 µs, but its cold repeat falls to 209.023 µs; uncontrolled machine contention makes that individual cell unsuitable for a fixed performance ratio.

Every measured event/render section in both engines reports **zero Rust allocator alloc/free calls**. All v1 runs report zero stream underruns/dropped commands. V2 reports **two underruns** only in unforced ANALOG STRINGS/256; all its other unforced/cold cells report zero underruns, capacity drops, refused starts, script overruns and nonfinite faults. The observed all-block deadline-miss counts are v1 unforced 1 (FX/32), v2 unforced 5 (piano/32:2, piano/256:1, strings/32:1, FX/32:1); v1 cold 4, v2 cold 12. These are not controlled starvation tests.

Voice census is reproducibly block dependent: v2 Una Corda peaks **12 voices at 32/64, 192 at 256**, including its cold repeat; Vista **80 at 32/64, 68 at 256**. V1 piano stays 19, Vista stays 16. V2 missing-feature report counts are not translated into an exact number of missing audible features. V2 silent-note selections occur even with nonzero audio. No same-audio kernel speed claim follows from these timings.

### Load and memory accounting at 64 frames, unforced cache

| Preset | v1 load s | v2 load s | v1 loaded RSS MiB | v2 loaded RSS MiB | v1 Bank::bytes MiB* | v2 packed heads MiB | v2 decoded pool MiB |
|---|---:|---:|---:|---:|---:|---:|---:|
| Una Corda | 0.157 | 0.288 | 146.41 | 144.75 | 163.77 | 16.90 | 24.00 |
| Vista | 0.193 | 0.280 | 167.12 | 165.95 | 193.62 | 32.47 | 24.00 |
| ANALOG STRINGS | 1.204 | 2.890 | 661.61 | 835.91 | 490.22 | 135.59 | 24.00 |

*Bank::bytes includes virtual rings and is not an RSS/head equivalence. V1 preload is 4,096 frames for piano/Vista and 4,048 for ANALOG STRINGS; v2 warm heads are 546/545/546 output frames, then zone-scaled. V2 has smaller packed sample heads, essentially equal loaded piano/Vista RSS, but **26.3% higher** loaded ANALOG STRINGS RSS. V2 warm load is **1.83× / 1.45× / 2.40×** v1 in these cache-disabled source adapters. Load speed ownership belongs to the load audit; these figures do not measure v1 production parsed-cache hits.

### Verified cold Linux file pages

All 18 cold runs reached **pages_after=0**. Each eviction covers 9 source files / 2,611,376 pages for Una Corda, 21,074 files / 2,449,724 pages for Vista, 13 files / 4,790,818 pages for ANALOG STRINGS. Metadata has per-run before/after counts; no file contents were copied. Actual reads below are `/proc/self/io` **process-wide read_bytes during playback only**, after head loading and idle warmup; they include decoder/reloader threads and exclude load I/O.

| Preset | Frames | Load s v1 / v2 | Steady p50 v1 / v2 | Playback disk MiB v1 / v2 | Deadline misses v1 / v2 | Underruns v1 / v2 |
|---|---:|---:|---:|---:|---:|---:|
| Una Corda | 32 | 2.292 / 1.171 | 10.471 / 44.570 | 7.72 / 7.57 | 2 / 6 | 0 / 0 |
| Una Corda | 64 | 6.981 / 3.018 | 12.170 / 88.771 | 7.52 / 7.57 | 0 / 1 | 0 / 0 |
| Una Corda | 256 | 1.874 / 1.670 | 29.920 / 210.923 | 8.04 / 22.62 | 0 / 1 | 0 / 0 |
| Vista | 32 | 0.921 / 1.349 | 6.300 / 58.580 | 2.03 / 9.39 | 0 / 3 | 0 / 0 |
| Vista | 64 | 0.613 / 1.170 | 3.530 / 69.520 | 2.40 / 9.59 | 0 / 0 | 0 / 0 |
| Vista | 256 | 3.927 / 1.144 | 22.250 / 167.132 | 2.53 / 9.53 | 0 / 0 | 0 / 0 |
| ANALOG STRINGS | 32 | 4.401 / 9.428 | 15.380 / 75.081 | 19.52 / 23.14 | 1 / 1 | 0 / 0 |
| ANALOG STRINGS | 64 | 19.103 / 17.270 | 68.431 / 163.253 | 19.39 / 22.92 | 1 / 0 | 0 / 0 |
| ANALOG STRINGS | 256 | 4.856 / 27.123 | 176.713 / 209.023 | 21.28 / 22.75 | 0 / 0 | 0 / 0 |

Cold load speed is not consistently better in either engine. Both play these paced low-polyphony schedules with zero cold-run underruns, but v2 reads more for Vista alongside its larger voice counts; matched-output tests must separate voice-selection differences from prefetch overhead. No claim of native sonic parity, high-polyphony resilience, transient-error recovery or spinning-disk suitability is supported by this table.

### Exported CLAP process and parameter flush

10,000 measured calls per cell after 1,000 warmups, empty rack, no GUI. Installed v1 descriptor is **0.3.152** (different from source v1 0.3.148); owned v2 artifact is **0.3.208**. 64 events are repeated Volume values at offset zero, not 64 distinct params or a realistic GUI burst.

| Frames | Volume events | Process p50 v1 / v2 | Flush p50 v1 / v2 | Process p99 v1 / v2 |
|---|---:|---:|---:|---:|
| 32 | 0 | 3.270 / 0.690 | 0.020 / 0.030 | 6.610 / 1.300 |
| 32 | 1 | 3.320 / 0.730 | 0.040 / 0.040 | 5.670 / 0.880 |
| 32 | 64 | 6.330 / 3.880 | 0.900 / 0.900 | 9.980 / 4.590 |
| 64 | 0 | 3.620 / 0.900 | 0.020 / 0.030 | 6.530 / 1.050 |
| 64 | 1 | 3.620 / 0.950 | 0.040 / 0.040 | 5.330 / 1.101 |
| 64 | 64 | 6.630 / 4.040 | 0.890 / 0.890 | 13.130 / 4.900 |
| 256 | 0 | 6.620 / 2.090 | 0.020 / 0.030 | 9.740 / 2.370 |
| 256 | 1 | 6.711 / 2.140 | 0.040 / 0.040 | 9.460 / 2.600 |
| 256 | 64 | 9.720 / 5.240 | 0.900 / 0.890 | 12.800 / 6.720 |

**Empty CLAP process is better in v2**: 64-frame p50 0.900 vs 3.620 µs. Parameter flush is effectively equal; 64 events cost about 0.89 µs in both. These numbers include plugin/output/smoothing work, not only an ABI thunk. They do not measure a loaded native plugin or prove whole-callback allocation freedom. The audio seam and exported-host results are separate experiments.

## Perf hot paths

Six clean 64-frame profiles attach after library loading/idle warmup, sample hardware `cycles:u` at 499 Hz, and stop **before JSON generation and instrument destruction**. Main-thread reports filter by its exact TID and renormalize percentages to that thread; separate whole-process reports retain decoder/reloader work. No user-stack memory, registers or sample payloads were recorded. Initial profiles accidentally included unload work (particularly v1 ANALOG STRINGS); those are excluded from the committed evidence and conclusions.

| Preset | v1 main-thread hotspots | v2 main-thread hotspots |
|---|---|---|
| Una Corda | Engine::render 23.74%; Player::render_block 21.96%; filter lane end/dots/kernels about 32.46% | **V2Core::end_block 46.40%**; scalar DSP 18.42%; V2Core::render 14.59%; libc memory helper 10.80%; resample sample_window 3.96% |
| Vista | Player::render_block 27.44%; KSP begin_audio_block 18.77%; AVX512 mix 14.12%; envelope 13.59% | **V2Core::end_block 40.67%**; Runtime::render_split 24.94%; render_voice 15.40%; BusState::render 15.08% |
| ANALOG STRINGS | KSP VM 41.99%; FX processor 26.17%; clock_gettime 26.11% | **V2Core::end_block 37.79%**; drain_behavior 13.33%; scalar DSP 8.70%; bus 4.68%; fused AVX2 source dispatch 3.41% |

Leaf IPs are not inclusive call stacks. The low main-thread sample counts (v1 89–100, v2 102–323) and a large v1 clock share make fine percentage ordering noisy; report these as localization, not nanosecond attribution. End-block assembly sampling concentrates in the inlined reserved-note-slot loop; the separate empty-arena probe verifies its scaling. Clean whole-process profiles also show v1's worker polling/cache loop and v2's cold-head reloader/worker completion; never assign those worker shares to the audio callback.

SIMD is demonstrably executing: both v1 AVX2/AVX512 filter/mix and v2 fused AVX2 source/convolution symbols appear. This rules out “SIMD branch is unused” as the general explanation. V2 scalar bus DSP and voice modulation remain real costs. Neither allocator counts nor these profiles establish a blocking audio mutex wait.

Precision candidates: `dsp.rs:649` uses f64 planar samples, `dsp/lanes.rs:16` f64 lanes, and delay storage is f64. Buses convert f32→f64→f32 (`bus.rs:333–365`), while reverb/convolution convert back into f32 inside the scalar chain (`dsp.rs:771–803`). Simple gain/matrix/mix transport could plausibly use f32; resample positions, filter recurrences and stability-sensitive coefficients require separate accuracy justification. Benchmark conversion/copy elimination and f32 stateless stages against the DSP reference tolerances before changing precision. No “f32 suffices everywhere” claim or quantified speedup was established. Stage matching is outside the sample loop; scalar sinc per-tap addressing above step 2 is the remaining dispatch/traversal candidate in finding 6.

## Ranked findings

Unqualified core filenames below refer to `crates/sampler-core/src/`; `v1/` means the prescribed read-only v1 worktree. Source spans refer to the audit baselines above. Pinned moose-utils/moose-core state citations are from dependency revision `bffa467` in Cargo.lock.

### 1. P1 — Full reserved-note scans dominate loaded idle CPU (M)

**Evidence:** `crates/sampler-core/src/ownership.rs:521–526` scans `self.notes.slots.len()` on every `flush_ended`; `src/sound/v2.rs:686–713` calls it for every loaded part at every host block end. `src/sound/v2.rs:857–879` sizes the arena for a future growth ceiling, commonly 16,384 notes. Repro: `cpu_audit --check`, zero live notes: 64 reserved slots p50 0.080 µs; 16,384 slots p50 24.770 µs (309.6×). Loaded Una Corda/Vista idle p50 is approximately 25–30 µs, before any MIDI notes are sent. A second empty-arena run gives 0.070 µs / 24.960 µs, confirming the floor. At 32 frames that floor alone consumes about 3.7% of a core per loaded part.

The held-note mapping also uses linear position/count scans at `sound/v2.rs:689–706`, making large release bursts potentially quadratic; it is separate from the proven idle-capacity floor.

**Root cause:** terminal notification discovery depends on configured capacity rather than pending terminal owners. The existing voice activity bitset avoids the analogous full voice scan (`render.rs:245`); the note arena does not.

**Fix:** `ownership.rs`, note/child completion sites and runtime storage in `lib.rs`: maintain a bounded ready-to-retire index/bitset; preserve parent traversal and sink-refusal retry. Initial conservative arena sizing helps memory but does not fix capacity-dependent work. Check idle 16-part racks and NOTE_END backpressure at 32 frames.

### 2. P0 — Plugin does not flush completed script callbacks (S/M)

**Evidence:** `src/sound/v2.rs:574–678` renders/takes the latest fault, and `:686–717` flushes ended notes, but neither calls `Runtime::flush_behaviors`. The perf harness **does** (`crates/sampler-perf/src/main.rs:333`), so its lifecycle differs from the plugin. `behavior.rs:984–1005` decrements note work/plan callbacks only when accepting completed behaviors; `ownership.rs:554–579` refuses note retirement while `work != 0`. The runnable probe triggers an authored `End` callback, releases the note and renders: **0 end notifications and 1 retained note before flushing callbacks; 1 end notification and 0 notes after**.

**Root cause:** the sound adapter consumes the single fault breadcrumb but omits the callback outcome/lifetime service. Successful outcomes can be reclaimed under behavior-arena pressure (`behavior.rs:1014–1046`); faults/cancellations cannot. That pressure reclamation is not prompt NOTE_END delivery.

**Fix:** `src/sound/v2.rs`: accept completed outcomes after each render/before `flush_ended`, retaining/reporting failures. Start with the existing `flush_behaviors_at` API; if its complete arena scan adds material CPU, use a bounded completion index rather than adding a second permanent full scan. Regress repeated scripted note/release cycles, plan-owned callbacks and faults through the actual sound seam.

### 3. P1 — Stream capacity and DSP polyphony have different admission budgets (M)

**Evidence:** Kontakt plugin load uses `Default::default()` stream policy (`src/sound/v2.rs:1074`); `crates/sampler-kontakt/src/stream.rs:271–285` sets 256 streaming voices, three 4,096-frame pages each: **768 pages / 24 MiB**. The same part's initial DSP pool can be **16,384 voices**, growing by up to 8× (`src/sound/v2.rs:857–879`). `service_streaming` errors are discarded (`:612`). `sampler-perf` overrides stream voices to 2,048 (`main.rs:199`), so its results do not certify the plugin's default capacity.

**Root cause:** storage setup precedes final runtime admission and never follows DSP pool growth. Page sharing helps identical assets, but diverse assets, crossfades and high pitch/read-ahead can require more than three pages per voice. A cache whose current demand is fully protected correctly reports Capacity instead of evicting live data.

**Fix:** `sound/v2.rs`, `sampler-kontakt/src/stream.rs`, core cache growth/admission: derive one measured memory/page-demand budget, grow page storage off audio or limit/steal streaming voices when it cannot admit their horizon. Do not blindly reserve 16,384×3 pages (1.5 GiB). Surface capacity/disconnect/failure separately from audible underruns. Test diverse long assets beyond 256 voices and high-ratio crossfade demand.

### 4. P0 — V2 offline render flag has no storage-readiness policy (M)

**Evidence:** sound contract `src/sound/mod.rs:114–116` says offline renders wait for disk; plugin supplies `cx.process_mode.is_offline()` (`src/plugin.rs:1571–1574`). `V2Core::begin_block` is empty (`src/sound/v2.rs:555`), and rendering takes the same immediate page path/fade policy. V1 plugin explicitly sets `Engine::blocking_streams = offline` (`v1/src/plugin.rs:3481–3483`), separately from the script budget passed to `begin_audio_block`, and supports bounded window waits up to five seconds (`v1/src/engine/voice.rs:30`, `:1644–1654`).

**Root cause:** offline intent is lost at the adapter; a faster-than-realtime bounce can outrun page decoders and substitute the live starvation fade/silence.

**Fix:** `sound/v2.rs`, core readiness boundary, host offline tests: implement explicit offline preparation/wait/error reporting off the realtime contract. Verify a deliberately delayed decoder renders the same PCM as resident preparation offline, while live mode remains nonblocking. This is source-proven; no native-host offline bounce was captured here.

### 5. P1 — A failed decoded page is sticky; plugin has no retry consumer (M)

**Evidence:** `stream.rs:282–303` returns the existing page status; `:365` provides explicit invalidation. `tests/stream_cache.rs:169–191` establishes failure then explicit invalidation/retry. Decoder maps unavailable/read errors to failed pages (`sampler-kontakt/src/stream.rs:620–636`); the plugin discards service results and never calls `invalidate` in `src/sound` or its stream frontends. The reloader consumes `take_cold()` before opening/reading, and its parked loop ignores reload errors (`crates/sampler-kontakt/src/stream.rs:500–507`, `:561–572`).

**Root cause:** the core intentionally leaves retry policy to its client, which implements cold-head wakeup but not failed-page/backoff/reloader-error handling. Failed pages can remain silent while their demanded entries are protected; eventual eviction may incidentally remove a failure but is not a recovery policy.

**Fix:** streamer/client bounded retry records with asset/request generation, backoff and permanent-failure reporting; invalidate only eligible failed pages. Keep a failed cold head marked pending for retry. Never retry corrupt immutable sample data forever. Inject one transient open/read failure and permanent corruption; verify live recovery and offline failure reporting.

### 6. P1 — Resident octave acceleration does not cover the normal streamed path (M/L)

**Evidence:** `render.rs:333–359`, `:585–609` calls `want_levels`/`try_levels` only for fully resident PCM. Streamed PCM uses `PagedFrames` with no levels. `resample.rs:315–320` limits the AVX2 polyphase bank to `1 < step <= 2`; above 2 the realtime fallback is scalar short sinc (`:349–360`). `Table::sample` (`:73–100`) loops over a step-dependent support with f64 coefficient interpolation/accumulation; at step 16, the short radius is 192, i.e. **385 source taps per output sample**. Streamed instruments normally use this path for high transposition/bend.

**Root cause:** the resident lazy-decimation optimization is merged but conditional on full PCM residence. Streaming/memory savings expose the unaccelerated high-ratio source path.

**Fix:** source/resample/stream decode boundary: prepare/cached decimated pages with immutable level+asset identity and correct loop/crossfade guards, or benchmark a wider SIMD polyphase path first. Retain double precision for positions and stability-sensitive coefficients; do not blindly turn all DSP state into f32. Test alias rejection, block independence and reverse/crossfade traversal as well as CPU. Real-library pitch-sweep attribution remains unmeasured; the claimed tap count is source-derived, not a measured speedup estimate.

### 7. P2 — Page lookup is logarithmic, but page admission moves a sorted vector (M)

**Evidence:** `crates/sampler-core/src/stream.rs:227–230` uses binary search; request replacement removes/inserts from `index` (`:343–346`, `:359–361`). At every cache miss/replacement these are O(cache-pages) moves on audio. A saturated clock sweep can also scan the full cache (`:307–323`). Worker `next_job` scans its pending array for earliest deadline (`:498–505`), serialized under the decode-worker mutex (`sampler-kontakt/src/stream.rs:595–613`); the latter is off audio.

**Root cause:** bounded allocation-free storage is still linear work during demand churn. Increasing page capacity to fix finding 3 amplifies both maintenance and worker admission costs.

**Fix:** first profile diverse-asset churn at the intended page budget. If material, use a preallocated lookup/index with bounded updates and an indexed deadline heap on workers; keep the existing immutable asset/request identity semantics. No need to replace the 16-reader linear LRU without profile evidence. This is a complexity finding, not a measured user deadline failure.

### 8. P2 — Audio-side residency uses try-locks, not a lock-free snapshot (M)

**Evidence:** `prepare.rs:156–157` uses `RwLock::try_read`; `:197–198` uses `Mutex::try_lock` for resident octave levels. Streaming protection and requests each acquire a head guard per active voice (`stream.rs:604–638`), and rendering acquires it again (`render.rs:343`, `:595`). An unsuccessful guard is interpreted as no resident head, so control-side `set_ranges` can momentarily turn resident reads into page misses.

**Root cause:** immutable ranges are published through a shared lock. The calls do not wait on a mutex, but they add atomic/synchronization cost and lock-contention fallback changes readiness. A successful read-guard release can wake a waiting writer.

**Fix:** only if profiles/contention tests justify it, publish immutable range/level generations through the existing off-audio retirement model, pinning the generation for a render/service epoch. Reuse one asset snapshot across deduplicated demands. Do not remove guard safety or move Arc destruction onto audio. Test concurrent trim/reload with sustained voices and count misses attributable to publication contention.

### 9. P2 — V2 page buffers remain allocated/touched when the instrument is idle (M)

**Evidence:** `StreamCache::new` (`stream.rs:150–194`) allocates 768 decoded stereo pages in the plugin default; `Stream::trim` (`src/sound/mod.rs:205–207`) can purge heads but never the 24 MiB pool. Sixteen such parts have a **384 MiB page-pool floor**, before source heads, plans or DSP arenas. V1 stream rings are lazily touched and return pages after five seconds idle via madvise (`v1/src/engine/stream.rs:28–44`, `:138–150`, `:561–577`); its worker clears cached readers/blocks when idle. V2 parks decoder threads but retains page buffers/readers until retirement.

**Root cause:** fixed decoded-page ownership gives excellent bounded realtime transfer but lacks a control-side idle shrinking/reclaim policy. A rack memory budget cannot reclaim that floor.

**Fix:** off-audio cache resizing or safe idle physical-page discard after ownership returns to control; rebuild/reset readiness before reuse. Share process-wide decode resources if measured multi-instance thread/memory cost warrants it. Reuse the current worker-owned buffer transfer protocol; never free pages in the callback. Measure post-idle RSS for multiple loaded parts, not just sample-head accounting.

### 10. P2 — Host wrappers free restored state on audio before their heap guard (S/M)

**Evidence:** CLAP pops owned pending state, applies it and drops it at `vendor/moose-clap/src/lib.rs:2644–2647`, then enters `RtSection` at `:2654`; VST3 has the same order (`vendor/moose-vst3/src/lib.rs:1282–1292`). Pinned moose-utils `src/state.rs:57–64` defines owned `Vec` fields for params, extra and persist. Their destruction can free allocations on audio. Pinned moose-core `state.rs:132–146` correctly excludes persist parsing/locking from `apply_state`; this audit does **not** claim that rack deserialization happens in the process callback.

**Root cause:** the deferred ownership handoff retires the state blob on the consumer thread, and the guard deliberately starts afterward. The steady render/event allocator result of zero does not cover state recall or C++ allocations.

**Fix:** wrapper retirement queue for consumed state and any custom replacement data, destroyed by host/control worker; extend allocation/free instrumentation around the entire valid process callback. Keep host-thread persist validation and realtime parameter coherence. Reproduce active preset recall with a large persisted rack. This is shared wrapper debt; it is not proven unique to v2 versus the old installed binary.

## Host-facing checks

The plugin process path (`src/plugin.rs:1511–1730`) uses preallocated scratch, bounded ArrayQueues and off-audio retirement for loaded parts/mix/growth. Every 100 ms it refreshes controls under a nonblocking try-lock (`:1541–1545`, `:355–363`) and every block updates node meters with try-locks (`:1705`). `control_edits` and `keyboard` each have capacity 256 (`:534–538`) but process drains until empty (`:1584–1599`), without a fixed per-block drain count if producers keep refilling. Neither creates per-event strings/vectors in the normal path.

Volume smoothing is intentionally sampled per output frame, but `db_to_linear(p.volume.read())` (`:1648–1650`) also repeats dB conversion per frame. Benchmark a linear-gain smoother or stable-target fast path before changing its automation law. SIMD dispatch is per batch/run; there is no CPU feature probe per audio sample. Stage dispatch in `dsp.rs:704` and `dsp/lanes.rs:57` is per stage/block; source fallback per-tap address resolution remains a candidate hot path.

CLAP (`vendor/moose-clap/src/lib.rs:2663–2682`) converts into reusable events and sorts by offset. Its params flush skips sorting (`:3734–3760`) and stores latest values in a preallocated deferred vector (`:2186–2191`). KONTRA exposes one ordinary automatable Volume param; an O(P²) deferred-parameter concern is not a demonstrated cost for this plugin. GUI changes are bounded queued values/gestures, drained until empty (`:2548`). VST3 shim process uses stack `paramChanges[512]` (`shim/vst3_shim.cpp:1428`); extra automation points can be truncated. Its bus negotiation vectors (`:1323–1324`) and omitted-bus malloc/calloc (`:1389–1401`) are in non-process setup, **not per-block allocations**. Rust VST3 reuses `RawBufferScratch` (`lib.rs:1313`).

An actual exported CLAP host probe measures empty process and parameter flush separately for 0/1/64 repeated Volume events at all requested sizes, with no GUI. It uses SDK 1.2.2 at `27f20f81dec40b930d79ef429fd35dcc2d45db5b`. V1 here is the explicitly requested installed backup; its version/sha can differ from source baseline `0cb7a8a0`. Native VST3 timing, f64 host-wire conversion and live UI producers are still unmeasured; source inspection is not a wrapper timing substitute.

## Streaming architecture comparison

`docs/architecture-v2/STREAMING.md` correctly keeps storage-latency tolerance and distinct offline preparation open; its early “not yet streamed playback” evidence sentences predate later implementation sections. Executable plugin reachability and real-library measurements, rather than stale wording, determine this audit’s status.

| Property | v1 at 0cb7a8a0 | v2 plugin at audit baseline |
|---|---|---|
| Default preload | 4,096 frames, shrink toward 1,024 (last-resort 256) under budget; offsets around script-selected starts | p95 measured open+first-page latency + 10 ms slack + block; scaled per zone with pitch/guards; commonly 545–561 output frames on warm SSD |
| Read-ahead | 8,192 virtual playback-order frame ring per voice; urgent below 4,096; chunks of 2,048 | demand horizon max(head,4,096)+128 output frames; all voices protected then requested; 4,096-frame physical asset pages |
| Worker model | four process-wide striped workers; worker-local decoded-block/readers cache | four decoder threads plus cold-head reloader per streamed part, plus voice grower; mutex only on worker endpoint, reads overlap outside it |
| Data sharing | shared source/head identity, worker decoded block cache; per-voice virtual rings | immutable asset/page cache shared among voices; packed heads, f32 stereo decoded pages |
| OS cache | regular file/random-access reads; Linux file page cache; idle ring madvise | regular file/random-access reads, 16 KiB buffered reader; Linux file page cache; no O_DIRECT or OS-cache bypass |
| Underrun | missing frames zero-filled; new cold onset can hold up to 50 ms; later windows advance; offline bounded wait | 1 ms fade out, cursor advances silently, 1 ms fade in when current window ready; cold starts enabled by plugin; failed-page retry requires caller |
| Residency | heat/adaptive head tiers; idle worker caches/ring page reclaim | trim/purge least-recently-played heads and cold reload; fixed decoded pool remains |
| Audio synchronization | seqlock/atomics for stream rings, no blocking stream mutex | SPSC page exchange plus nonblocking per-asset range/level try-locks |

The v1 raw probe key `head_bytes` is **Bank::bytes**, which includes resident samples **and virtual stream buffers** (`v1/src/engine/bank.rs:440–443`, `:671`), not just sample heads. Its default streamed bank ring reservation is 64 MiB (1,024×8,192×8 bytes), lazily physically committed. V2 raw `stream.head_bytes` is packed sample-head bytes, with its separate pool accounting. Compare RSS and definitions, not these fields as though they were interchangeable.

## Merge/use verification and prior work

- `origin/v2/simd-voice-dsp@95fd47b1efda449e03dde6489ae9ac2a75f91d72` **is an ancestor** of the audit baseline. `sampler-core/Cargo.toml:13` links sampler-simd; `render.rs:376/396` dispatches lane stages and `source.rs:767` dispatches fused resample runs. `sampler-simd/src/lib.rs:61–74` caches AVX2/FMA detection. Plugin reaches these through the ordinary core path.
- `origin/v2/stream-storage@bd3b92efc682c363c2a55017e533036a5ed3ad18` **is an ancestor**. Actual plugin loader calls streamed loading (`sound/v2.rs:1074`), attaches cache (`:1243`) and calls streaming every sound pass (`:610–613`). There is no evidence that storage streaming exists only on a side branch.
- `origin/v2/block-voice-dsp@e1568f704aa44e583891b829f6260529adce1197` **is not an ancestor**, but its essential work was integrated by another commit/squash: `901a3a8023fc655ec80b4c19fd42159158a8e3b9` implements block stages, and current source contains the realtime quality ladder, starvation recovery and lazy octave levels. The original `9d9faabe` ancestry test alone would incorrectly label block DSP missing. Core chunks are 64 frames and stage dispatch sits outside the sample loops. **Do not merge this stale whole branch just to get already-present code.**
- Already present: clock sweep/cache demand fast paths, packed heads, cold reload, runtime AVX2/FMA, idle bus skipping and resident lazy octaves. Branch tips are not evidence of further missing fixes.
- `origin/v2/dsp-kernels@d32a2ba5d01a24a302349fa1b26581f13e98931a` adds DSP coverage/work-order documentation; compared core code is older than integration, not a replacement CPU implementation. The measured precision/kernel choices should follow that DSP coverage/reference work.
- Unmerged `origin/perf/v1-block-voice-dsp@7210cb968bd3d6e8b2a0ce5b79b035d4f71cb4b6` adds a high-polyphony WIP benchmark; it is not a v2 CPU fix. `test/v1-audio-thread-heap-guard@1f8dcabb` provides a realistic-polyphony heap guard on v1, also not a production v2 fix.
- Prior KSP agent report `v2/gpt-ksp-audit@a1446ec6`, `docs/architecture-v2/KSP_AUDIT.md`, establishes script/engine consumer and purge gaps. Its purge finding is attenuation versus residency, so it must not be treated as proof that UI/KSP purge frees storage. Those correctness issues confound native-library CPU equivalence. No fresh engine fix from a named unmerged branch was established for findings 1–10; route concrete integration fixes instead of duplicate DSP/streaming work.

## Remaining measurements

1. Supply an installed actual drum kit; repeat both source paths and native host at 32/64/256 with human drum hits/round-robin/chokes, rather than an orchestral pulse preset.
2. Repeat at least three times on an idle machine; record pinned audio-thread CPU cycles and per-thread worker CPU, onset/steady/release histograms, and concurrent UI/automation. Current p99/max includes build/scheduler noise.
3. Repeat 256-frame Una Corda/Vista voice census and validate native Kontakt audio/group selection. Current voice counts change with block size; the exact KSP/pedal cause is not diagnosed here and must not be invented from CPU timing.
4. Diverse-asset high-polyphony cache saturation, pitch ratios above 2/large MPE bends, loop/crossfade boundaries, and injected decoder latency/failures. Compare the plugin default 768-page pool against perf harness 6,144 pages.
5. Cold controller/storage cache and slower physical disk require a suitable disk; DONTNEED on this NVMe mount cannot emulate them.
6. Offline native CLAP/VST3 bounces with delayed storage, plus state recalls and complete wrapper allocation/free instrumentation (including C++), not only core render counts.
7. UVI CPU/stream-residency comparison needs a selected playable preset and matching v1/v2 program path; the measured matrix is Kontakt only.
8. Multi-part RSS/idle cost after >5 s and >30 s, trim/reload while sounding, and N-plugin instances. Avoid counting virtual rings as RSS.

## Validation and evidence

- **36 render cells** succeeded with nonzero aggregate peak: three installed Kontakt instruments × two engines × three sizes × unforced/cold.
- **18 file-cache evictions** verified zero resident source pages before loading.
- **Six clean perf profiles** saved as both whole-process and audio-TID reports; instrument load and teardown excluded. Binary hashes and the source/version distinction are in `cpu-evidence/provenance.json`; raw `.perf.data` and binaries stay in the own cache, not the repository.
- **36 exported CLAP host cells** succeeded (two binaries × three sizes × three event counts × process/flush).
- Callback-lifetime/empty-arena `--check` passed twice; stream-cache integration tests **6 passed**.
- `~/.cache/kontakto-heavy cargo test --no-run --profile corpus` passed after the final Rust probe edit. Existing dropping-copy warning is unchanged. This compiles the default CLAP/VST3/root tests; it does not mean all root tests were executed.
- Python runners parse successfully; CLAP probe compiled with `-Wall -Wextra -Werror`. No dev servers were started or left running.

## Reproduction

Build the sound seam probe with symbols and v1 adapter, serially through the heavy gate:

```sh
~/.cache/kontakto-heavy cargo build --profile corpus --lib --example cpu_audit --no-default-features --features clap,library-access
~/.cache/kontakto-heavy cargo build --release --manifest-path tools/cpu-audit-v1/Cargo.toml
# The v1 manifest points to the prescribed read-only sibling worktree.
# Use the per-worktree target chosen by kontakto-heavy; do not export CARGO_TARGET_DIR.
<target>/corpus/examples/cpu_audit --check
python3 tools/cpu-audit-run.py v1 <target>/release/cpu-audit-v1 <own-cache>
python3 tools/cpu-audit-run.py v2 <target>/corpus/examples/cpu_audit <own-cache>
# Add --cold or --profile. Each cell gets its own heavy slot and timeout.
# To repeat a cell, use a fresh own output dir (the runner is resumable).
~/.cache/kontakto-heavy cargo test --profile corpus -p sampler-core --test stream_cache
~/.cache/kontakto-heavy cargo test --no-run --profile corpus
# Empty native CLAP host (SDK is free-audio/clap tag 1.2.2):
g++ -O2 -std=c++17 -Wall -Wextra -Werror -I <sdk>/include tools/cpu-audit-clap.cpp -ldl -o <own-cache>/clap-host
~/.cache/kontakto-heavy env XDG_CACHE_HOME=/dev/null KONTRA_DISABLE_NETWORK=1 KONTRA_REPORT_DIR=<own-cache>/reports <own-cache>/clap-host <clap-artifact>
```

No dev servers were started. The saved evidence contains metadata, hashes, source references and perf symbols only.
