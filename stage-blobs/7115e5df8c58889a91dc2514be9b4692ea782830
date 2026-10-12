# Next-batch performance admission and run plan

**Python source:** `21bcdde3b545c20571ed3475b5285e6575119fce`.
**Clean base:** `51e0b9e78d543acfed1392c623119a312ae8e5cb`.
**Owner/checkout:** `pi/perf-acceptance-next`, `/home/derpcat/.t3/worktrees/KONTAKTO/pi-perf-acceptance-next`.

Following-batch deliverable only. Do not add this to the frozen current batch. No engine/DSP/UI source was edited, no Rust/compiler/host/library process was run, and no build or extra runtime cycle is requested here. Parent reports current `429e7dff3ecb0f53aef32c43201ab796ccc8c28c` frozen and authorized; the combined `618` test receipt is **PENDING** from the parent. The source changes below do not imply its result.

## Actual defect and smallest correction

The existing `tools/cpu-audit-native.py` did not call the existing `live_host.artifact_receipt()` for v2. Arbitrary plugin/CLI/host combinations could reach the audition and produce a `MEASURED` receipt with no selected-candidate identity. It also retained `cpu-audit-cold.py` output without checking `pages_after`, so a failed source-cache eviction could be called cold.

At source `21bcdde3`:

- `tools/cpu-audit-native.py:43–50`, `cold_receipt`: require nonempty observations, exact nonnegative integer counters, bounded pages-before and zero pages-after; refuse malformed/residual source-cache evidence before audition.
- `:79`, `:95–102`: v2 requires `--source-sha`, reuses the existing artifact/path/profile/hash checks, matches the selected full SHA and verifies the built host source digest. A v1 cell cannot override frozen artifacts.
- `:123–134`: carry the checked cache/build receipts into metrics. Do not label the current checkout's C++ source hash as proof of an arbitrary v1 host's provenance; that field is now unavailable without a checked build receipt.
- `tools/check-cpu-audit-receipts.py`: six no-host fixtures exercise the actual main-driver boundary, including rejection before state export, changed plugin/CLI/host, missing/wrong candidate and host source, residual cold pages, malformed counters, and successful receipt preservation. All subprocess/host calls are mocked.

Existing `tools/kontra-gate/live_host.py:29–37` remains the artifact checker; this is not a second collector. Its binary checks run before state export/audition. SHA/path/profile receipts are trusted build attestations, not reproducible-build proof. They must originate with integration; do not fabricate a BUILD.json to satisfy a check. Python assertions require normal execution, **not** `python -O`.

Regression: the pre-fix missing-selected-source test reached a mocked `MEASURED` observation, then failed `AssertionError not raised` (exit 1). After the correction, six fixtures pass. Schedule self-check, existing live-host/contention fixtures, five editor-cycle fixtures, audit-load sanitization self-check and diff-check pass. These are Python contract tests, **not** engine execution or performance proof.

## Frozen references and corresponding native evidence

`pi-perf-acceptance-next-reference.json` records actual hashes and limitations. This round reverified all seven entries in `~/.cache/kontra-v1/SHA256SUMS` read-only, plus W9's two CPU adapter hashes. No reference directory was edited:

- v1 source: `0cb7a8a0b4d43086596a64c77320caa1b26d6d98`.
- Frozen CLAP SHA256: `20ff6b471069d6891d2847e72a4f863db5b50ce24265fda82d083b713b3496de` (plugin 0.3.152, distinct artifact lineage from scanner).
- CPU adapter `42a0ae91`: `b9998ca2ce2f2ed4f9f88bbfb11c5e884fa162a87cdf89f26ece6f1248fdc6ab`.
- Profile adapter `41a4a5d8`: `5537c8fbfa29bb2b23860bf789d5122b313e78c614419bf870fbd3146907569f`.

Corresponding Kontakt behavior was checked using the immutable package from `3d4c9659a4bec3c6c97126bffb5404a9147a2f50`, re-read in this round. The [NI Kontakt manual DFD Tab](https://docs.native-instruments.com/ni-tech-manuals/kontakt-manual/en/classic-view#dfd-tab) says DFD retains small sample sections in RAM, defines DFD Preload Buffer Size, connects increased preload to drop-outs, and warns that background loading may cause artifacts. The recorded HTML hash is `846a09243ad7d17d8daa92bf5105418501ebb25cd358ba8d09108ffb5d4ae7c4`.

Original native serializer receipts at `t3code-80fe786b/artifacts/ni-file-records-2026-10-08/{check-results,sources}.json` match hashes `f56987ad0ee1cf1783c0774ea91d503e93852341ebf0d0d23855491f7ac598e2` and `4cfbf652b7aca9e475ac9b2bb8386635dcd52cb1930ab76fdfb487fdf7f419cd`. They pin Kontakt standalone 8.13.1 binary `0fe6356e…`, reader VA `0x140d0d4b0`, DFD serializable word `+0x21b60`. This is equivalent existing static native evidence, not a new REA session. It verifies neither native reload locking nor native CPU/RAM. No official product executed. No script builtin or new engine behavior is implemented by this Python change.

Kontakt CPU/RAM comparison is **UNAVAILABLE/UNKNOWN under the present native-launch constraints**. The word “native” in the Python CLAP driver means KONTRA's exported native session/API, not a measured Kontakt process. Do not relabel our own CLAP tests as Kontakt parity.

## Admission before any later expensive job

Integration is the sole validation/runtime owner. Use its exact frozen source/artifact manifest and already-built executables; no lane-specific rebuild or target override. Do not set `CARGO_TARGET_DIR` or `RUSTC_WRAPPER`, start servers, install, publish, start old workers, or create schedules.

1. Obtain a new explicit run authorization and current quiet-owner grant from the parent. The old W9/W10/W8 owner strings, grant files and queue order are historical, not permission.
2. Verify plugin, CLI, host and observer/driver hashes against their exact receipts. For v2 the adjacent `BUILD.json` needs `source_sha`, `profile` (`ci`/`release`), `path`, `sha256`, `cli_sha256`, `host_sha256`, and `host_source_sha256` from the actual build. Record frozen artifact source separately from following-batch Python driver SHA.
3. Confirm all other expensive jobs drained; observe the whole cell with existing `Activity`. A preparation snapshot in this round found foreign cargo/rustc and was **CONTENDED**. It is not a timed quiet witness. The observer sees fixed known process kinds and samples once/second; it can miss subsecond or unclassified foreign activity. Load averages are context, not an invented quiet threshold.
4. Use private settings with `imported=true`, `uvi_imported=true`, roots empty. Unset **both** `KONTAKTO_UVI_READER` and `KONTRA_UVI_READER`, plus profiling/tracing/allocation diagnostic variables for scored cells. Native states, CLI output and raw plugin logs remain in private tmpfs, removed afterward. Retain only numeric receipts/hashes.
5. **Do not run generic `gate.py`, its `probes()`, or an all14/UVI pass under this constraint.** At base `51e0b9e7`, `tools/kontra-gate/gate.py:366–368` auto-populates an official UVI executable in `KONTRA_UVI_READER`. Direct Kontakt-only drivers are the permitted proposed inputs. UVI performance stays UNKNOWN; no workaround opener is proposed.
6. Stop and preserve the attempt on missing/changed artifacts, unavailable input, failed readiness, silence, nonfinite output, incomplete MIDI, missing readback, residual cold pages or contention. Do not replace missing metrics with zero. New output directories per attempt; never overwrite contended evidence.

## Exact CPU input and executable recipe (NOT RUN)

Reuse the existing `cpu-audit-native.py` rather than the automatic-building CPU adapter in `gate.py`. It exercises our actual exported CLAP and records callback **thread CPU**, callback wall time and deadlines separately. Inputs are its exact `SCENARIOS` at source `21bcdde3:19–22`: Una Corda **Pure**, Vista **5 Violins**, ANALOG STRINGS; program 0, one part, 48 kHz, blocks 32/64/256, 4-second audition. Piano uses eight fixed keys; strings/fx use keys 48–59. CC1=110, CC11=127, sustain on at frame 0, note-offs at 48000, sustain off at 144000. `audit_events:33–40` and schedule checks bind dispatch order. Do not substitute the stage manifest's Una Corda Felt for CPU Pure.

After integration authorization, set `HOST`, `V2_PLUGIN`, `V2_CLI`, `EXPECTED_SOURCE` and `OUT` from its frozen manifest, and `QUIET_OWNER` from its new exact grant. The following is the command template, not an instruction to run during the current build:

```bash
# Use normal Python, no -O. Do not create or renew grant files here.
cd /home/derpcat/.t3/worktrees/KONTAKTO/pi-perf-acceptance-next
( cd "$HOME/.cache/kontra-v1" && sha256sum -c SHA256SUMS )
# Example single source-cold pair; SCENARIO=piano|strings|fx, BLOCK=32|64|256.
env -u KONTAKTO_UVI_READER -u KONTRA_UVI_READER -u KONTRA_SIGNAL_TRACE \
    -u KONTRA_HOST_SCHED_DIAGNOSTIC -u PROBE_ALLOCS \
    KONTRA_QUIET_OWNER=1 KONTRA_GATE_REQUIRE_QUIET=1 KONTRA_GATE_ACTIVITY=1 \
    "$HOME/.cache/kontakto-heavy" python3 tools/cpu-audit-native.py \
    "$SCENARIO" "$BLOCK" "$OUT/v1-cold" --version v1 --host "$HOST" \
    --quiet-owner "$QUIET_OWNER" --cold
env -u KONTAKTO_UVI_READER -u KONTRA_UVI_READER -u KONTRA_SIGNAL_TRACE \
    -u KONTRA_HOST_SCHED_DIAGNOSTIC -u PROBE_ALLOCS \
    KONTRA_QUIET_OWNER=1 KONTRA_GATE_REQUIRE_QUIET=1 KONTRA_GATE_ACTIVITY=1 \
    "$HOME/.cache/kontakto-heavy" python3 tools/cpu-audit-native.py \
    "$SCENARIO" "$BLOCK" "$OUT/v2-cold" --version v2 --host "$HOST" \
    --plugin "$V2_PLUGIN" --cli "$V2_CLI" --source-sha "$EXPECTED_SOURCE" \
    --quiet-owner "$QUIET_OWNER" --cold
```

Each cold cell applies library-local `posix_fadvise` and checks mincore `pages_after=0`; it does not write source files or globally drop OS caches. Repeat pairs in reversed order with fresh output names; preserve every attempt and report the spread, not just a favorable median. Exact repeat count/budget belongs to integration's run authorization, not an acceptance threshold invented here.

For the next repeated/unforced-source pair omit `--cold` and use new names; keep product caches disabled in both engines. Call this **unforced-source-cache**, not verified warm. To call it warm, the receipt must identify the completed priming cell, same input/host/schedule and no eviction/intervening workload; actual OS residency is still uncontrolled. Do not pool it with source-cold or product-warm data.

Compare `cpu_audit.steady_thread_cpu.p50_us/p99_us`, `cpu_audit.steady.p50_us/p99_us`, callback deadline misses, wake misses and sampler underruns independently. The host uses `CLOCK_THREAD_CPUTIME_ID` at base `51e0b9e7:vendor/moose-clap/tests/live_performance.cpp:303–321,393–394`. Main-output peak scanning is included equally; this is not complete all-thread/plugin-process CPU. The fixed audit steady window is 12000–48000; retain its sample/block count and audible peak. Silence/voice omission is not efficiency. Match actual feature/routing/load coverage; RR output is not byte-identical by assumption. Profile/trace/scheduler-diagnostic numbers are attribution only even if a lower-level receipt says `MEASURED`.

## Load, onset, RSS and IO-held guard: exact known gaps

Existing frozen input manifests remain intact: `~/.cache/kontra-runs/w8-load-all14-prepared-20261009/kontakt11.tsv` SHA256 `55c4aa306b4737f6e108a2b91299b30918593e01c3d3dfef1f8b930a009f48b7`; Areia-FullEns and Dolce-Vln1 are rows 1/2 (zero-based). Use these first for the historical cold-onset failure, then the remaining Kontakt rows. All14/UVI manifest hashes are recorded for provenance, not permitted executions.

Reuse `tools/audit-load.py MANIFEST OUT BIN MODE START END REPEAT` only as a bounded diagnostic with an integration-identified exact probe binary. Example per-item shape: `audit-load.py "$KONTAKT11" "$OUT/stage" "$PROBE_BIN" v2 1 2 1`, wrapped by the same quiet-owner/activity policy. For v2 numeric header cache use a new private `/dev/shm` `PROBE_CACHE_HOME`; for frozen v1 pairing the existing runner's `PROBE_PRODUCT_CACHE_ROOT` must be a new `/dev/shm/kontra-gate-cache-*` root. Keep each engine/item isolated, retain its root for the second pass, and remove it afterward. First/second product-cache labels require real hit/miss/file-count witnesses; no global OS-cold claim follows.

**This probe is not currently a production-callback onset acceptance witness.** At base `51e0b9e7:src/plugin.rs:3809–3903`, initial MIDI occurs before `begin_block`; `examples/cpu_audit.rs`/`tools/cpu-audit-common.rs` similarly begin inside render after scheduled MIDI. Historical W8 `c591b435` receipt explicitly disqualifies these pre-boundary timings for its corrected callback policy. Integration must confirm the current binary's equivalent correction before admitting its onset numbers; an old `stage-probe-callback` name is not proof. No engine edit is made in this round. The probe also calls `malloc_trim` after its retained-UI measurement: do not report its trimmed RSS as normal retained product RAM.

For host-ready/onset/RSS reuse `live_host.observe(..., load_probe=True)` and `load_observation` (`live_host.py:212–232`) with identical exported states and exact notes. `load_host.py` is frozen to 0.3.326/608a and **must not** be used as current-candidate proof. The host load probe measures ready/first-audio wall/frame and before/ready/done RSS/HWM/swap, but includes a fixed 100ms post-ready warmup (and the audit mode's idle warmup if combined). This is not immediate cold-note onset. Do not combine it with callback cold-start counters or scanner load_ms. Immediate cold onset remains UNKNOWN until an admitted production-boundary witness is selected by integration.

IO-held guard proof at source base `51e0b9e7`:

- `crates/sampler-kontakt/src/stream.rs:745–756,789–850`: trim/reload share the mutex across IO/publication.
- `src/sound/mod.rs:238–245`: part `Stream::trim` calls that trim.
- `src/plugin.rs:1840–1858,2121–2140`: serialized background `Load::run` trims before draining discard and starting `load_multi`. Thus slow reload IO can delay background retirement/next-load handling and hold RAM longer. It is not an audio-thread lock or proof of a main-thread freeze.
- `stream.rs:1127–1188`: the blocked-reader and concurrent-reload tests prove control serialization if their current combined binaries pass. They do not time worker stalls. Keep the pending parent `618` verdict separate.

The existing `stream_report INPUT.nki SECONDS VOICES` example purges midway and reports service CPU/wall, pending pages, cold starts, underruns and RSS, but performs its purge on the simulation thread after the measured block and does not time trim. It cannot quantify background worker stall by itself. Therefore recommend the next decisive authorized run after the combined receipt as **actual CLAP CPU pair first**, followed by a separately admitted Areia/Dolce load/retirement/trim-observation diagnostic with overlapping cold reload. Record trim request/completion, pending reload, worker retirement/next-load completion and RSS/HWM alongside callback CPU/underruns. **That overlap/time witness is absent from current Python receipts: UNKNOWN, not PASS.** Do not claim lower RAM merely from faster accounting traversal.

For retained/editor RAM and total process CPU, existing `editor_rss.summarize_cycles:59–95` has process clock, audio-thread clock, RSS/HWM/swap and numeric activity deltas. Frozen v1 lacks UI20/perf exports, so numeric-cycle mode is not a symmetric v1 witness. Use matched legacy four-phase raw RSS only, with separate quiet evidence and identical host/window/audio/source lifecycle; it is raw measured memory, not an automatic acceptance gate. Do not substitute sample-head bytes for process RSS, sum HWM with live RSS, or count freed allocator arenas as live leaks. Native readiness/generation/presentation remains UI-owned.

## Historical W9 finding and target verdict

The immutable `w9-horns-frame-diff-20261010/comparison-quiet-retry1.json` has six quiet historical Horns256 rows for before `d8f1c623` and rejected SSE after `5340a425`. Before/v1 steady median was 2140.921/167.863 us cold and 2072.749/175.333 us repeated. These are **historical wall-time gaps**, not a current 12x CPU assertion. Attributed leading frames were polyphase 14.22%, Stereo 8.04%, VoiceModState 6.40%; PageReader span was 1.67%, service_streaming 1.39%. The leading paths are DSP-owned. Since that capture the current base changes core source/stream/SIMD by 916 additions/128 deletions; a fresh exact-source attribution is required before choosing another optimization. Current `PageReader::span` uses the fixed bucket lookup (`crates/sampler-core/src/stream.rs:604–655`); this inspection establishes no present defect or dominant remaining bottleneck. No speculative engine edit is justified, and no shared DSP span is touched.

The AVX correction `8c206006` remains an old unaccepted A/B candidate, not a measured win by association. Its isolated reduction witness and source existence do not certify the current product.

**CPU and RAM lower than both frozen v1 and Kontakt: UNACHIEVED.** A strictly smaller observed pair is useful evidence, not automatic proof of “significantly lower.” Retain matched inputs, repeated spread and limitations; there is no invented percentage threshold. Missing native comparison blocks the joint claim even if v1 checks later pass.

**NEXT:** parent forwards the frozen current combined `618` receipt; append its source/test-binary hashes and actual nonzero filter results without another build. Then integration chooses the authorized comparable CLAP pair above. Resolve the production-onset, IO-overlap and symmetric total-CPU/RAM gaps explicitly before any broader acceptance claim.
