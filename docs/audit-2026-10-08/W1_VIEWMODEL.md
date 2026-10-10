# W1 view model and publication implementation

Baseline: `7e82b152` (core-v2). Retained witnesses originate from UI_LOOP `46bf47ac`, with Original-default gates from `732037d5`.

A generation retains its immutable Interface Arc. Runtime publications carry cumulative sparse overrides in the existing UI IR; only changed scripts regenerate. Applying them keeps Face, script/page, values, presentation and picture source/cache. Repeated property/key writes do not publish. Widget identities include part, source epoch and script.

One resolver chooses the serialized rack override, saved instrument preference, or global preference. Factory global preference is Original regardless of unsupported metadata. Both body and header picture menu offer Original, Vector and generated KONTRA. Generated sections and strips consume existing IR widget references and bindings. Fit uses both axes; explicit 1x, 1.5x and 2x scales are exact. Requested unsupported native views produce a load diagnostic and visible warning.

The scalar bridge now uses the native ControlClient queue and admission replies, rather than publishing unadmitted atomic values. Pending presentation is separate from authoritative readback. Source epochs guard submission, readback and effect replay. Changed native scalar revisions refresh after rendering; the 100 ms idle Watch includes mirror revisions. The typed WidgetEdit extension from W5 follows independently.

Runtime UI aliases, including set_knob_label/set_text, movement, hide/unit/default, menu mutation, skin offset and UI colour, reach the existing KSP model and IR. Alias first arguments lower to UI identity rather than the current variable value. Equal effects are no-ops.

## Measurements

Local Conflux, sample key subset 0, optimized CI profile; baseline CPU timings are not GPU or input-to-photon measurements. Full-editor harness uses 1180x900, 8 warmup + 24 measured frames and retained painter resources. Direct UI-loop harness uses 1000x600 and creates fresh painter resources. Compare each only with the same harness.

| Baseline full editor | Build median | Total median |
|---|---:|---:|
| Original | 0.305 ms | 7.202 ms |
| Vector | 0.295 ms | 6.694 ms |

Baseline full publication median: approximately 0.376 ms (8 samples). Load: 4830.653 ms, 390 controls, three interfaces. Callback: three effects emitted, two applied, set_knob_label discarded. Main-page input: 0/1 sampled drags in both modes; secondary page: 1/1. Widget gesture fixes belong to W2.

Before logs/JSON: `~/.cache/kontakto-fix-viewmodel/{before.log,loop-before.log,loop-before/conflux.json,failing.log}`. No decrypted source or assets are stored. After measurements and typed transport validation will be appended after the follow-up gate.

### Independent Original-default checkpoint

`42cba1ba` implements the view/publication fixes; `f2f1a979` also merges integration `be4c5c21` and fixes the picker completion race. Both are pushed. Later branch commits incorporate W5 and W2 and are a separate handoff: W0 can land `f2f1a979` independently of those services.

The picker worker stores its answer before clearing `open`. A reader could consume completion in that interval and immediately observe the operation as still busy. Both shared consumers now require `open == false`; a deterministic witness holds the intermediate state explicitly. The async test plus that witness passed in five isolated runs, without changing its timeout.

After no-run, all seven loop checks passed, including the opt-in Conflux run. The real callback emits three effects and applies all three; the label alias is no longer discarded. Changed sparse publication median is 0.01779 ms; identical publication median is 0.000065 ms. The initial uncontrolled full-editor run measured Original 11.596 ms and Vector 11.966 ms mean, versus 7.136/6.723 ms before. That result triggered the interleaved checks below rather than a performance claim.

### Equal-pixel interleaved frame check

Baseline source `7e82b152` plus the identical benchmark-only harness lives in the explicitly approved detached `fix-viewmodel-base` worktree. Frozen CI test binaries are `~/.cache/kontakto-w1/bin/{before,after}`. SHA256: before `54e13f01e99af244bbaacf9f164f2db7f2a8690cfe3ec55425fa433b12cde333`, after `5c6a6e9aa215664d61c4286e54f819fe0bb6d0641b621e8777c977bd9e06b2cc` (`f2f1a979`).

Both output 1180×900 pixels. The old view ignores explicit zoom and fits its canvas to 888 pixels wide. Giving the after harness an explicit `888/970` scale produces the same 888×541.955 authored canvas; the measured pixel dimensions are logged. Each invocation retains the same 8 warmup/24 measured frames and measures each pass separately.

| Three alternating pairs, equal canvas | Original before → after mean ms | Vector before → after mean ms |
|---|---:|---:|
| 1 | 7.008 → 25.709 (after outlier) | 6.716 → 26.421 (after outlier) |
| 2 | 7.085 → 7.811 | 7.383 → 6.761 |
| 3 | 7.623 → 7.508 | 7.757 → 6.898 |

| Repeat, both processes pinned to CPU 15 | Original before → after mean ms | Vector before → after mean ms |
|---|---:|---:|
| 1 | 6.797 → 6.838 | 6.741 → 6.815 |
| 2 | 7.234 → 6.823 | 8.165 → 6.951 |
| 3 | 27.229 (before outlier) → 6.879 | 24.870 (before outlier) → 6.903 |

The large excursions occur in either binary and across every pass. Stable frame timings are approximately 6.8 ms; there is no consistent pass regression at equal pixels. These are shared-machine CPU measurements, not proof of native artwork fidelity, GPU performance or input latency. Every raw run is retained, including outliers: `~/.cache/kontakto-w1/{ab-*,ab-equal-*,ab-pinned-*}.log`. The first `ab-*` series uses different authored canvas sizes and is not the equal-pixel comparison.

### Scanner provenance

The first own-build quick15 used the older shared driver and matched integration: 15 loads, 7 Original OK, 8 missing-images. That receipt is historical following scanner Addendum 4. The new shared extension `9dcf05e5` is merged; its fresh own-build quick15 receipt follows separately. Scanner Original OK does not certify gestures or callbacks.

## Retained checks

Four synthetic loop witnesses were flipped from assertions of defective behavior to expected behavior. Before production edits, those plus the Original-default and label-alias checks failed (six failures). Checks now also cover sparse/no-op publication, restored mode precedence, header three-mode selection/editor reopen, script/page/value continuity, stale effect rejection and both-axis zoom.

### Integrated typed readback and host automation

W1 `630e82f1` adds the existing-channel typed capture adapter: callback changes to arrays/text/XY reach persistent Face.input, rejected previews roll back and wake the view, and capture payloads recycle off the audio thread. The retained production two-part probe independently exercises focus, wheel and drag for the same widget ordinal across namespaced faces. Both pass. W3 `eb8dade0`, W2 `00c88076` and W7 final `0951f197` are merged; the persistent input state, per-source native updates, cursor/event/modifier grouping, UVI input and one pictures revision hash are preserved.

CLAP/VST3 ParamChange events now map stable normalized plugin parameter IDs to saved native host addresses and invoke W7's callback route at the existing sample boundary. The main-thread HostParameter operation uses the same ControlClient admission/reply channel and generation-aware runtime API. Volume keeps its existing ID. Registered host addresses are 0..=2048 (2049 slots), following the verified Kontakt 8.13.1 standalone creation limit; native VST3's published capacity is unverified. Saved addresses above 2048 remain unchanged and generate an unsupported load finding. Program/Group remap delta wire fields are not decoded, so no delta or reader clamp is invented.

Root `cargo test --features shots --no-run` passed. Integrated UI suite: 85 passed, 10 opt-in ignored; plugin loop suite: 5 passed; typed capture and host callback/epoch/revision/noheap checks passed. Stable slot/static metadata registration, address 2048, nonfinite input, and unchanged master parameter identity are retained checks. Raw logs: `~/.cache/kontakto-w1/{capture-final,host-check2}.log`. Native Conflux startup remains W3's explicit diagnostic/fidelity work; these checks do not claim that gate.

### OS gestures and meter provider

W2 `4c4da3a0` (including W5 `91d58944`) is merged. OS drag/drop targets the actual painted, clipped, winning MouseArea in the part/source namespace. Payload conversion preserves type, bounded UTF-8 paths, cursor/event/modifiers/mouse-over and one atomic gesture. Leaving a target emits its native leave metadata. Invalid or stale drops onto a MouseArea are vetoed rather than falling through to WAV rack creation. DropPath is transient and never copied into authoritative widget snapshots.

KSP attached meters use the IR group/slot/channel/bus address and `Runtime::engine_meter`, through the existing 100 ms nonblocking atomic mirror. The current IR bounds/prunes the registered addresses; source epochs guard registration and refresh, changed values wake Watch, and Face.input.meters retains normalized amplitude. Unsupported service addresses read zero, with no substitution of UVI output peaks. W5's default group/master-route lookup is being verified by its owner. Native-zone waveform mapping/worker peaks remain the W7/W3 seam and are not claimed wired.

Root shots no-run passed; OS hover/leave/drop, atomic 33-path veto/stale epoch, meter native bus/channel/noheap/epoch, and W2 exact-hit/namespace/path-limit producer checks pass. Raw log: `~/.cache/kontakto-w1/drop-final3.log`. W3 reports real Conflux native startup now succeeds; its gated renderer checkpoint is still pending integration.

### Renderer and gesture integration checkpoint

W2 `4f3db733`/`42cdc50a` and W3 `410ba8d3` are merged. Authored XY modes/active cursor, interpolated table batches, pre-validation OS target detection, axis defaults, wallpaper frame/origin, state fonts and bounded NativeUI graphs are retained alongside W1 transport. Combined root shots no-run passed; UI suite 90 passed / 10 ignored, plugin ingress loop suite 6 passed. Raw log: `~/.cache/kontakto-w1/render-integration.log`. The real Conflux rerun follows; no latest native-artwork acceptance is inferred from synthetic checks. Waveform provider awaits the gated source-zone/Pcm API and remains explicitly unwired at this checkpoint.

### Source waveforms and resumed host/drop gates

W7 `e7f7bca1` and W3 `f3d71907` are merged. The retained native positive source-zone ID resolves through Prepared to a shared Pcm and existing streamed AssetSource; no ordinal fallback or PCM export is introduced. One cancelable worker per part computes finite min/max envelopes with 4096-frame scratch. Requested display bins are resolved width × view zoom × device scale, clamped 1..4096. The per-plan cache shares Arc envelopes for a zone/bin count, keeps at most 64 entries (2 MiB of peak data), and stores sample duration alongside peaks in Face.input. Source epochs and matching control-plan generation guard results, and the existing scalar revision wakes Watch. Resident/custom-streamed equality, missing reader, cancellation, sparse ID/hole, plan/epoch, duration and shared-Arc checks pass; raw red/green logs `~/.cache/kontakto-w1/wave-{red2,green,final2}.log`. Integrated UI suite now passes 93 tests (10 opt-in ignored), ingress loops 7, and waveform check 1; root shots no-run passed. Raw `wave-render.log`.

Host automation retains 2049 normalized slots. A production Sampler::process regression sends a note at sample 0 and address 2048 at sample 7; supported ControlGain audio stays zero for samples 0..6, changes on sample 7, runs the authored callback echo, and makes zero heap calls. Native OS DragLeft now forwards an empty file gesture before clearing the pointer; editor cancellation also emits leave through the existing FileDrag admission route. This closes a real host-boundary gap that the direct producer tests did not cover. The retained native red→green check is `native_drag_leave_delivers_an_empty_file_gesture_to_the_owner`; raw `drag-leave-red.log` and `host-drop-final2.log`. Root shots no-run, exact audio offset, host callback/epoch/revision/noheap, production OS hover/leave/drop/33-path veto/stale source, and producer winning-hit checks all pass.

The latest opt-in Conflux gesture rerun stopped in preparation because its fixture clears zones/assets while retaining a physical source-zone map. W2/W7 own the fixture and malformed-map guard; raw failure retained at `conflux-integration.log`. This is not reported as a new gesture or native-artwork acceptance result. An exploratory UI play_note fixture also hit a pinned-core InvalidInput fault and was sent to W5; the retained host transport test uses the supported DSP/callback route above.

### Authored performance intent (2026-10-10)

Ports v1 `0cb7a8a0:src/ksp/ui.rs`'s explicit `Interface.performance` into the shared UI IR. KSP `make_perfview` and `load_performance_view` copy the existing model flag; Lua `makePerformanceView` copies `__ui.performance`. `load_native_ui` retains its separate frontend request and does not synthesize performance intent. Sparse publication carries `InterfacePatch.performance`, including authored-value reversion and identical-publication no-ops.

Default source selection is explicit performance, then authored native frontend, then the first nonempty source, with declaration order within each group. Wallpaper/background-only performance and empty-widget native sources remain eligible. Live intent changes preserve the active source tab, page, input, assets and saved presentation mode; reopening reads the current cumulative patch intent.

The retained full-editor regression failed before production edits because auxiliary widget count won over `make_perfview`. IR 6/6, KSP UI 9/9, UVI UI 7/7, full-editor intent/state 2/2 and part state/source/zoom 4/4 pass on the restored per-worktree optimized CI target. Root `cargo test --locked --profile ci --features shots --no-run` also passed. Receipts are in `/mnt/Windows11/DEV_WORKSPACE/kontra-runs/w1-perfview-20261010`. The first background-only fixture incorrectly expected a tab bar on reopen with only one eligible source; its corrected rendered-source assertion passes. No new Conflux frame/publication timing or full-corpus acceptance is claimed for this metadata/default-selection change.
