# Owned Conflux interface CPU-phase profile

Root alone builds and runs this procedure. No release/install/version change is required.
Use an isolated candidate built from `fe86caf4256121d0b5a0ef3eff5964d2aa61aa07`
plus the profiler patch and root's selected release fixes. Record exact source commit,
artifact SHA-256, owned Conflux NKI SHA-256, host/backend, window size, scale and actions.
Do not change shared target/config or use an official reader as part of this procedure.

## Live interface profile

1. Set `KONTRA_NATIVE_UI_TIMING=1` **before creating the plugin instance**. This reuses
   the existing Activity switch; unset is disabled, including all new clocks.
2. Load the owned Conflux NKI. Select Interface and Original. Keep the window size
   and zoom fixed. Wait for artwork/native resources to finish. Record any native
   failure/fallback: an IR fallback is not an authored NativeUI measurement.
3. Take baseline A via the new host export below, on the host main thread. Alternatively,
   Reports → Export… → a fresh local support-report folder captures `ui_cpu_phases`
   in the existing diagnostic context. Export does not reset aggregates.
4. In separate fresh instances, repeat the same actions: idle 10 seconds; drag the
   same actually visible control 20 times; hold/release the same owned MIDI chord;
   switch the same page 10 times; save a multi once to exercise control capture.
   Record targets and counts. Use fixed-duration A/B windows for each action class.
5. At rest, take snapshot B. Repeat each action class three times. Also record a
   disabled run to expose timing overhead. Avoid opening Reports during the action
   window: report UI itself adds build frames. Export both snapshots after the window
   when using reports (export UI overhead must be treated as part of that capture).
6. Report count delta, total_ns delta and mean = total delta / count delta (only if
   count delta > 0). `max_ns` is a lifetime maximum, **not subtractable**. Use a fresh
   instance to obtain an action-window maximum; otherwise label it lifetime maximum.

### ABI / diagnostic schema

`__kontra_clap_ui_cpu_phases(plugin, version, uint64_t *out, size_t count) -> bool`
requires version **1** and exactly **27** writable aligned u64 values, on the host
main thread before instance destruction. Each row is `(count, total_ns, max_ns)`.
Disabled, bad version/length, null pointers or panic returns false without writing.
The existing `__kontra_clap_ui_activity` version-1 20-field export and `Perf` ABI
are unchanged. Existing presented-host tooling does not automatically read this
new symbol; root must bind it or use support export.

| Row | Name | Caller/thread role | Scope |
| --- | --- | --- | --- |
| 0 | ui_build_frame | editor_ui | Entire app build closure returning El; excludes later MUI resolve, renderer and present |
| 1 | ui_snapshot_readback | editor_ui | Selection/view snapshot, ensure/resize and before-copy |
| 2 | ui_control_readback | editor_ui | Display scalar/typed/meter/waveform capture in the shown face |
| 3 | ui_control_capture | editor_ui | Save-multi UI control capture only, not arbitrary host state serialization |
| 4 | ui_projection_sync | editor_ui | Face/model reconciliation, native patch materialization/readback/model update |
| 5 | ui_authored_layout_submission | editor_ui | Authored-size/native draw-to-El, or IR/generated view-to-El; not later global MUI layout |
| 6 | ui_native_script | editor_ui | Native module/VM initialization, graph render, event calls and deferred Canvas Lua calls |
| 7 | ui_native_canvas_submission | editor_ui_deferred_canvas | Deferred Canvas callback through CPU command/path conversion into Draw items |
| 8 | worker_script_effects | serialized_load_worker | Background Load::run apply_effects; includes lock waits and publication |

`ui_cpu_phases` is null when disabled. When enabled it contains `version`, `clock`,
`scope`, and `phases` rows with `name`, `thread_role`, `count`, `total_ns`, `max_ns`.
Thread roles describe connected call sites, not OS thread IDs. A stack-only !Send
span prevents its start/end moving across threads. No audio-thread timing was added;
existing audio readback/publication counters are unchanged.

All times use Instant elapsed wall time around CPU work: they include preemption
and waits, and are **not on-core CPU time**. Spans are inclusive and overlap:
script execution inside deferred Canvas is included in Canvas submission; UI build
includes its child phases. Do not sum phases as a frame budget. Not every editor
cost has a subphase (asset preparation, shell/rack building and admission remain in
build total). No GPU present, renderer resolve, compositor or DAW responsiveness
claim follows from these measurements. Fixed atomic storage, no histogram, no trace,
no per-frame filesystem writes. Snapshots are independent relaxed loads; worker
activity can advance between fields. Capture at rest and flag overlapping worker work.

## Existing headless IR benchmark (root only)

With root's private libtest executable in `$LIBTEST`, use a new local cache each run:

```sh
KONTRA_LOOP_PATCH='/mnt/MAIN_STORAGE/Libraries/Kontakt/Conflux 1.1.0 [Native Instruments]/Instruments/Conflux.nki' \
KONTRA_LOOP_CACHE="$OWNED_RUN/cache" \
"$LIBTEST" --exact ui::loop_audit::loop_audit_conflux --ignored --nocapture --test-threads=1
```

This benchmark measures the real IR view plus MUI headless frame resolve and CPU
software paint. It does **not** invoke the full plugin editor or authored NativeUI
session, so it cannot collect the live profiler phases or prove GPU-present lag.
The old `build_ms` includes IR build **and** frame resolve; `paint_ms` is software
paint. Runtime values now seed the caller-owned UI map. Only a runtime-bound,
finite/nonzero-range, visible, enabled, pointer-hit-winning rendered knob/slider
can be dragged. Skipped candidates have reason counts. Only changed drags enter
callback measurement; callback admission, outcome and effects are reported separately.

`conflux.json` checkpoints after renders and after callbacks, before the existing
strict effect/publication assertions. A later assertion still fails the test; the
checkpoint's `stage` proves which evidence completed, not that the benchmark passed.
No retained product assertions were weakened. Root's original RED log:
`/mnt/Windows11/DEV_WORKSPACE/kontra-runs/conflux-ui-diagnostic-20261010T142240Z/profile.log`.

Source-only checks to run at root: `phase_spans_are_opt_in_bounded_and_aggregate_count_total_max`,
`ui_phase_export_preserves_output_on_rejection_and_copies_schema_order`,
`native_profiler_counts_connected_script_and_deferred_paint_calls`, and
`loop_drag_target_skips_unavailable_controls_and_uses_runtime_values`.
Child status for all checks: **NOT_RUN**.


## Measured headless Conflux result, 2026-10-10

The full-editor `ui::tests::audit_ui_real_frames` measurement loaded the owned
Conflux1.1.0 instrument, admitted its Original NativeUI surface, warmed eight
frames, and sampled24 frames at1180×900. It removes audio zones before preparation;
there is no audio worker, DAW or GPU present. Test-only source-path instrumentation
also adds overhead that installed production code does not have.

The initial baseline at `a3aa5a93` measured56.8ms/frame construction in Original
and4.1ms in Vector. Original spent22.8ms/frame in authored layout submission and
19.9ms/frame in native script calls. These are overlapping elapsed-wall phases.
Snapshot/control readback was below0.01ms/frame in this static fixture.

`e31d0e8e` removes two temporary Lua tables per primitive while preserving child
frames, outer modifiers, graph validation and existing flexibility caching.
Two sequential baseline/change pairs with privately retained release test binaries
produced the following means (48 frames per source):

| Work | Baseline | Allocation change |
|---|---:|---:|
| Original frame construction |51.03ms|47.88ms|
| Original authored layout phase |20.79ms|16.87ms|
| Original total software rendering |80.49ms|82.09ms|
| Vector frame construction |3.99ms|3.80ms|

The local layout phase fell about19%; frame construction fell about6%.
Total software rendering did not improve consistently. Original remains slow.
The baseline/change Original PNGs were byte-identical. All eight selected native
layout/geometry/budget tests and four profiler checks passed; four native-library
unit tests stayed ignored. The full-editor fixture and four paired repeats passed.

This identifies expensive Original frontend construction in this fixture; it does
not prove live host input latency, GPU performance, callback contention or native
Kontakt parity. Original and Vector also have different authored canvas sizes.

Root receipts: `kontra-runs/conflux-ui-layout-20261010T145549Z/PROFILE-RESULT.json`
and `PAIRED-LAYOUT-COMPARISON.json`; previous baseline:
`kontra-runs/conflux-ui-phases-20261010T144404Z/PROFILE-RESULT.json`.
