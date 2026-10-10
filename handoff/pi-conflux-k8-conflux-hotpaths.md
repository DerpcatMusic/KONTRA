# Conflux UI lag: source-only hot-path review

Base: `fe86caf4256121d0b5a0ef3eff5964d2aa61aa07` (experimental 0.3.430).
Branch: `pi/conflux-k8-conflux-hotpaths-fe86caf4`.
Checkout: `/home/derpcat/.t3/worktrees/KONTAKTO/pi-conflux-k8-conflux-hotpaths`.

**Result:** ranked measurement candidates, not an established cause. Root subsequently reported that its loop_audit attempt failed at `values[&id]` before metrics; the profiler owner is correcting that fixture. This is root-reported failure, not a measured lag cause or a locally reproduced failure. No build, test, profiler, host, native reader, Wine, or nested agent was run by this reviewer. Reviewer tests: **NOT_RUN**. Production and test source are unchanged. The only commit is this report and its JSON companion. Root owns all measurement execution. No decoder admission or compatibility claim is made.

Read the root repository's `AGENTS.md` and `/home/derpcat/ponytail/skills/ponytail/SKILL.md`. This exact checkout has neither AGENTS nor graft; used source searches, not the main checkout's graph. The parent scratch remains the thread binding; the new git worktree is an inspection checkout. No shared target/config, ledger, frozen manifest, or existing release-fix span was edited.

## Actual path and the Lua distinction

1. NKI: `crates/sampler-kontakt/src/load.rs:142-176` calls `read_with_controls`; `library.rs:75-100` uses the cache or `read_chunks` and ni-file Program decoding. `library.rs:394-418` turns script text and saved state into **KSP** behaviors with physical slots. `load.rs:817-910,916-1020` initializes/compiles scripts, produces UI IR and ScriptViews, then binds modules into the prepared plan. This is load work, not repeated editor-frame work.
2. Shell: `src/sound/v2.rs:210-300` builds the runtime, scalar definitions, typed widget snapshots, capture batches and ControlClient. `src/plugin.rs:2119-2137` runs the serialized Load worker, settles effects/readback, and refreshes ingress. Its regular audio-triggered cadence is about 100 ms (`plugin.rs:2969-2997`); stopped-host editor builds also enqueue Load (`ui/mod.rs:1205-1212`). Coalescing Load tasks does not coalesce admitted control operations.
3. Gesture: `ui/part.rs:220-530` uses the retained Face; gestures enter Shared's epoch-fenced setters and `ControlIngress::submit_widgets` (`sound/v2.rs:731-779`). The runtime drains up to 256 requests before each render chunk (`v2.rs:1727-1737`). Script callback effects leave audio through Shared.effects (`plugin.rs:3216-3221`). The worker applies them to ScriptView, regenerates changed script interfaces, and publishes cumulative patches (`plugin.rs:1715-1801`; `sound/mod.rs:269-316`).
4. Frame: `ui/mod.rs:1142-1238` builds the full editor from cloned Selection/View. `ui/part.rs:240-262` retains Face, resolving only a new base or changed patch. It copies scalar/typed/meter/waveform presentation values (`part.rs:435-456`), then chooses native Original or IR rendering.
5. **Lua is not exclusively UVI in this code.** KSP `load_native_ui` is projected to `Interface.native_ui` (`sampler-ksp/src/ui.rs:272-284`). Face creates `native_ui::State` from that entry (`part.rs:38-53`); Original/Bitmap dispatches to it (`part.rs:465-500`). `native_runtime::Package` reads Kontakt `resources/native_ui`/`native_ui` `.nui` members through `pictures::Source` (`native_runtime.rs:121-176`; `pictures.rs:47-91`). `Session` is an editor-owned mlua VM (`native_runtime.rs:640-655`), configured with Luau (`Cargo.toml:35`), and renders the legacy graph. KSP remains the Kontakt behavior engine; this VM is the **Kontakt NativeUI presentation frontend**, separate from Falcon/UVI script execution.
   The existing Conflux footer fixture explicitly selects KSP slot 2 and constructs a Session from its `native_ui` entry (`ui/v2_tests.rs:3790-3859`). That is a source consumer, not a newly executed receipt. The actual current instrument's selected mode/entry must still be recorded by root.

## Ranked candidates and exact taps

Ranking is by diagnostic value for lag, not measured CPU share. W = widgets, C = scalar controls, R = retained KSP requests, T = non-scalar captured cells/texts.

### 1. Original NativeUI frame work: graph reconstruction and repeated cross-source projection

- `ui/part.rs:467-498`: every Original native build traverses all source interfaces. Every nonempty cumulative patch materializes a fresh source clone and applies the patch, even without a new ui_revision. Each source also calls widget_values and widget_meters, then native.update_view. `State::view` updates the selected face again (`native_ui.rs:403-406`). This is distinct from retained Face.update, which already skips equal patches.
- `native_runtime.rs:814-844`: Session.update_view locks its bridge and scans all bridge controls for each source. `native_ui.rs:405-435`: every build dispatches events against the prior graph, invokes Session.render, derives authored size and redraws a new El tree. `native_runtime.rs:860-873` calls `__render`; the previous graph is retained for event hit testing, not reused as an unchanged rendering result.
- `native_ui.rs:679-744,868-879,989-1017`: iterative graph traversal, fresh text elements, and deferred canvas callbacks remain frame work. Do **not** interpret this as all text shaping or all image decoding missing their caches. `native_runtime.rs:615-638` already avoids copying unchanged authored widget metadata; images/resources have caches.
- Existing taps: `native_materializations`, `native_widgets`, `typed_calls`, `meter_calls`, `face_updates`, `face_noops`; native `advance_ns` and `new_scenes`. Internal read-only inspection points for root's separately owned instrumentation: State::view around `events`, `session.render`, `authored_size`, `draw`; `Session::graph_work` nodes/fuel (available under tests/shots); Session.update_view call count, controls visited, lock-wait/hold time. No such instrumentation was added here.
- Prediction: if this dominates, Original's build/advance cost exceeds Vector at the same physical canvas; costs increase with cumulative patched sources and graph work even when publication_changes stays flat. A fast native advance falsifies this as the main CPU cause.

### 2. Full typed-array round trips, presentation cloning, and Shared locks

- `sound/v2.rs:246-270`: enumerates **every** non-scalar widget cell/text into reusable batches of at most 4096, not just visible/dirty cells. `refresh` (`781-803`) submits one batch when none is in flight; completion recycles it (`907-916`). There is no control-revision/visibility gate before recapturing unchanged values. A nominal whole sweep needs ceil(T/4096) refresh opportunities, about 100 ms apart, plus worker/audio scheduling; this is a cadence model, **not measured Conflux latency**.
- `sampler-core/src/widget.rs:241-296`: capture validates the entire batch with widget_value, then reads it again into the output. Each widget_value uses binary search. Queue payloads recycle off audio; no claim that this path allocates on audio.
- `sound/v2.rs:977-1012`: values scans face widgets and clones complete stored typed arrays/text before overlaying pending edits. `part.rs:440-456,483-493` consumes these snapshots each build; NativeUI repeats them for each source. IR table/XY branches additionally clone array values (`ir_view.rs:1257,1398`). Count actual arrays/lengths in this Conflux before calling them large.
- Scalar readback is also full C, but **not blindly every frame**: `PartShared::refresh_controls` uses try_lock and changes-only revision (`plugin.rs:629-653`), called on the 100 ms poll and after native revision changes (`2969-2997,3207-3215`). Display reads collect the whole C vector under controls lock (`620-627`), then settle/overlay under ingress (`655-661`). Core scalar lookup is binary search, not an alleged quadratic core scan (`sampler-core/src/control.rs:268-271,415-419`).
- GUI/worker blocking locks: ingress during settle+typed values; engine_meters during retain/nested widget/address scans (`plugin.rs:664-714`); scripts during effect apply/regeneration; Shared.view during fingerprint/copy/publication. Shared.part/with_parts use the registry mutex (`1393-1400`). Audio mirrors generally try_lock and may skip readback rather than block. Native adapter's `shared_lock_wait_ns` measures a **different** lock (mui::host::Shared), not these product locks (`mui-baseview/src/lib.rs:627-646`).
- Existing taps: readback_calls/busy/cells/changes, typed_calls, meter_calls, worker_ticks. Proposed root taps: per-batch cells/bytes, capture submit→reply time, whole-sweep time, values-cloned bytes, ingress/controls/meters/view/scripts lock wait and hold times, registered meter count. Separate capture cost on audio from snapshot cost on UI.
- Prediction: typed costs scale with T and snapshot multiplicity even at no change; lock tails line up with worker activity. Small T, negligible lock waits and low snapshot time would deprioritize this candidate.

### 3. Sparse publication still rebuilds a whole KSP interface; Request lookup is linear

- `ScriptUi::interface` invokes ScriptView.ui → `sampler-ksp::ui::interface` for a changed instance (`sound/mod.rs:298-303`; `sampler-ksp/src/lib.rs:300-305`); this reconstructs its UI IR before PartView computes `InterfacePatch::between` (`plugin.rs:1022-1046`). The resulting publication is sparse; the upstream regeneration is not widget-delta-only.
- Request is `model::Request { command, args }` in an ordered Vec (`sampler-ksp/src/model.rs:189-197,277`). Interface generation reverse-scans for native_ui/color and meter attachments; waveform reconstruction finds the widget then walks requests (`sampler-ksp/src/ui.rs:272-297,682-730`). Runtime property application finds widgets linearly (`lib.rs:325-329,387-388`). No evidence that R grows unbounded on this patch: color updates replace a matching request (`lib.rs:366-385`), and waveform projection replaces same-address entries/reset attachments (`waveform.rs:18-36`). Preserve ordering; do not blindly deduplicate commands.
- Control transport is separately bounded FIFO: `ControlClient::submit`, `Runtime::poll_control_update` explicitly acknowledge every accepted request and do **not** coalesce (`sampler-core/src/control/transfer.rs:87-122,152-235`). Ingress does linear definition/widget searches when submitting (`sound/v2.rs:688-710,736-738`); pending preview replacement is not cancellation of prior accepted callbacks.
- Existing taps: loop_audit publication_ms, changed_publication_ms/noop_publication_ms; publication_calls/changes. Proposed root taps: R/W counts per regenerated instance, ScriptUi.interface time separate from publish_interface, effect count/services, queue submit→apply→reply and rejection counts. No new payload/ordinal/coalescing law is proposed.
- Prediction: callback-heavy lag correlates with interface regeneration/request scans, not merely static frames. Low publication rates/times rule this out for static dragging that does not emit UI metadata effects.

### 4. Model cadence and event/present scheduling can dominate perceived lag

- `Watch::changed` samples scalar/native revisions inside its **100 ms** readout block (`ui/mod.rs:282-336`), whereas UI revision/pointer/loading state has other wake routes. Animation gates are 33 ms (`253-259,387-399`). Local previews can update on input; this is not proof of a universal 100 ms knob delay. Worker effect publication adds another scheduling boundary.
- `mui/src/host.rs:511-529` coalesces equal-modifier hover samples; drags retain their trail and combine contiguous equal-button/modifier samples at advance (`611-630`). Other queued edges can cause multiple resolve/builds per native tick (`631-646`). `resolve` times ViewBuild separately from Resolve and records layout work/cache counters (`654-710`). Idle/inert moves can skip all building (`553-590`).
- `mui-baseview/src/lib.rs:617-688` holds host Shared over advance+scene snapshot, **releases it before present**. Present can include acquisition/vsync, rendering/upload/submission (`707-795`). Slow present is not automatically a script problem. Native callback scheduling is host/platform-owned; headless 16 ms step is synthetic (`855-863`).
- Existing native timing: callback_interval_ns/total_ns, shared_lock_wait_ns, advance_ns, scene_clone_ns, resize_ns, present_call_ns; outcome counters, pointer/drag/reentrant counts, new_scenes and geometry. `KONTRA_NATIVE_UI_TIMING=1` enables a fixed 4096-sample, ten-second primary-drag capture at construction (`diagnostics.rs:410-473`; `mui-baseview/src/timing.rs:7-85`). Summary is off-thread; **present submission is not GPU completion, compositor FPS or input-to-photon**.
- Existing Driver profiler API has ViewBuild/Resolve, dispatch/coalescing/idle/throttle counts and validated/measured/arranged/rebound/cache work; it is not automatically enabled by the native timing env flag. Root would need its own controlled hookup to use those subdivisions.
- Prediction: large callback gaps/present tails with small advance indicate scheduling/presentation rather than UI CPU. Capture authoritative callback→publication→Watch observation timestamps separately to establish readback latency.

## Smallest existing root-owned repeat

Root's existing loop_audit invocation **currently has a concrete blocker**: root reports failure at `values[&id]` before metrics. In the fixture, `let was = values[&id]` (`loop_audit.rs:201`) assumes the selected control has entered the rendered Values map. Selection walks visible knobs/sliders, whereas rendering additionally rejects zero-sized/off-page widgets; source inspection alone does not establish which mismatch caused this failure. The profiler owner is correcting it; do not overlap that edit. The command below is the minimal existing invocation, **not a presently successful repeatable baseline**. Re-run only after root validates its fixture repair, recording that repair's commit and the resulting artifact identity.

Use root's **already validated** shots-enabled kontakto libtest binary for this source plus separately approved release/fixture fixes. Do not reuse the historical W1 before/after binary as a 0.3.430 witness. No build command is required by this handoff. Example (NOT_RUN here):

```sh
PATCH='/mnt/MAIN_STORAGE/Libraries/Kontakt/Conflux 1.1.0 [Native Instruments]/Instruments/Conflux.nki'
OUT='/root-selected/private-run-directory/conflux-hotpaths/run-1'
mkdir -p "$OUT"
KONTRA_LOOP_PATCH="$PATCH" KONTRA_LOOP_CACHE="$OUT" \
  "$ROOT_VALIDATED_KONTAKTO_TEST_BIN" \
  --exact ui::loop_audit::loop_audit_conflux --ignored --nocapture --test-threads=1 \
  >"$OUT/loop.log" 2>&1
```

After the profiler owner's repair succeeds, repeat serially into run-2/run-3, with the same binary/patch, geometry and cache policy; retain raw outliers. Save binary/source/fixture hashes and feature/profile identity; do not mutate release manifests. Existing consumer: `src/ui/loop_audit.rs:94-352`. JSON: `$OUT/conflux.json`; per-face Bitmap/Vector build_ms and paint_ms (8 warmup + 24 measured, 1000×600), asset decode_ms/missing/bytes, full/changed/no-op publication, admitted callbacks/effects/outcomes. Its first stage loads only key 0. It directly builds **IR**, even for Bitmap faces with native_ui; assets.sync is synchronous and `tests::pixels` creates fresh CPU painter resources. These numbers do not certify native Original performance, production asset-worker cost, ControlIngress capture/locks, Watch wake latency, GPU or host presentation. The callback section directly invokes runtime and renders 512 frames; it bypasses shell admission/worker scheduling.

### Existing full-editor fixture: concrete measurement blocker

`ui::viewmodel_tests::audit_ui_real_frames` (`viewmodel_tests.rs:69-237`) is the existing 1180×900 headless fixture, with 8 warmup + 24 measured per mode and retained CPU painter resources. It exposes build/layout/scene_clone/cpu_paint/cpu_raster/total. However, at this exact base it clears `source.zones` directly rather than `retain_zones` (`85-88`), so the physical source-zone map can be invalid; it also unconditionally unwraps an **IR** canvas surface after each mode (`203`), which NativeUI Original does not guarantee. It only warms a fixed number of frames, without asserting native resource readiness. Therefore an unchanged invocation is a diagnostic attempt, **not a repeatable successful native baseline**:

```sh
KONTRA_AUDIT_UI_PATCH="$PATCH" \
  "$ROOT_VALIDATED_KONTAKTO_TEST_BIN" \
  --exact ui::viewmodel_tests::audit_ui_real_frames --ignored --nocapture --test-threads=1
```

Root alone may repair this measurement fixture: preserve source-map invariants when removing zones; identify the actual selected native/IR canvas; wait for bounded resource/session readiness and reject fallback/loading/error panels. Reservation excludes this fixture and all profiler/production edits. `widget_conflux_native_gestures_and_readback` already uses retain_zones but renders IR too; it is not a substitute native frame benchmark. `widget_conflux_footer_text_reaches_typed_publication_without_truncation` does instantiate the native Session but is a text/graph correctness check, not a latency profiler.

For the real-window follow-up, reuse root's current native timing host/harness with `KONTRA_NATIVE_UI_TIMING=1` set before plugin/editor construction. Read cumulative counters through existing `__kontra_clap_ui_activity(plugin, 1, out, 20)` on the host main thread (`plugin/host_probe.rs:72-114`), taking deltas around the same drag. Do not call that export on an arbitrary pointer or wrong artifact. Separate Original/Vector, silence/sounding, local input/remote callback changes, and equal **physical** canvas sizes. Native materializations >0 at flat publication_changes is a useful first discriminator, not proof of causation.

## Limits / handoff

- No new timing observations; no cause, native fidelity, Kontakt 8 importer coverage or regression-free claim. Historical W1 measurements are not current receipts.
- Current Conflux counts, array lengths, request counts, selected native entry, resource readiness and frame geometry remain to be measured. Code constants/fixture assertions are not an executed census.
- Unknown serialized laws/native behavior were not admitted, replaced or guessed. Existing b21a424a / 5d9ea1e / eff5198d fix spans remain untouched.
- Root's measurement lane should first distinguish native vs IR, then advance vs present; only add narrower taps if existing counters do not resolve attribution. No broad audit or speculative production optimization is needed first.
