# Analog Strings prepared-capacity diagnosis — source-only blocker

## Result

**BLOCKED; no production fix or regression authored.** The exact runtime binder step/array is not present in the supplied receipts. The strongest source-supported candidate is the explicit-path NKA runtime snapshot reservation at `crates/sampler-core/src/array_file.rs:113–127`. A 4,000-cell text array passes individual typed-array validation but necessarily fails its eight-job aggregate reservation. Installed plaintext metadata contains a 4,000-row text NKA. This establishes a concrete incompatible shape, **not proof that the failing script registered that array**. Do not report the installed failure fixed.

Changing the 8 MiB ceiling, deleting the negative test, skipping the affected script, or falling back to scriptless playback is not a safe complete fix. A memory-layout/admission change needs a confirmed binding shape and bounded snapshot/ownership regression coverage. This pass leaves all such policies unchanged.

## Isolation and source identity

- Main repository, read-only: `/mnt/Windows11/DEV_PROJECTS/Repos/KONTAKTO`.
- Private worktree: `/home/derpcat/.t3/scratch/2026-10-10-stop-this-thread-2336e1f0-2f34-7f6415ff/analog-capacity-worktree`.
- Branch: `pi/conflux-analog-capacity-diagnosis-20261010`.
- Exact source base: `2799df958eac916cba41edecddac2c79cd9c807f`.
- Installed failing baseline: `ff60de20eeae95fdd736f3b064325d1ef9f6a84c`.
- NKA service introduction: `8eedf4c6ebb76e25da6ae0243e5c7f71233abdeb`, parent `9075da681fbca5019640619822a4540bfd2a57a1`. It is an ancestor of the failing installed baseline. This is source/history localization, not an executable bisect.
- No diff between installed baseline and source base in the inspected NKA/evaluator/lowerer/compiler/binder/loader files listed in the companion JSON.
- Main HEAD observed before and after inspection: `58c4c715639077bba633bcfe24a421c1f169f3d0`; no main/other-worktree working files edited. Worktree creation necessarily updated common Git branch/worktree metadata.
- Only these two handoff files are added. The code-source SHA remains the exact base; a documentation commit does not validate code.

Read the parent repository `AGENTS.md`, `/home/derpcat/ponytail/skills/ponytail/SKILL.md`, `CONTRIBUTING.md`, relevant architecture notes and ceiling comments. This base has neither tracked AGENTS nor graft; navigation used `rg`. Task-specific source-only restrictions override historical build instructions. Full source Git-blob IDs, source-file SHA-256 values and protected UI blob IDs are in JSON.

## Retained evidence

Evidence directory: `/mnt/Windows11/DEV_WORKSPACE/kontra-runs/conflux-retained-layer-20261010T155337Z`.

The directory has **no `UI-REGRESSIONS.log`**. `UI-REGRESSIONS.json` points to `ui-neighbors.log`, whose SHA-256 matches its receipt. It reports 284 passed, 3 failed, 25 ignored. Two failures are the Analog load failures; the third, `ui::native_ui::tests::inactive_native_graph_does_not_allocate_lua_event_payloads`, is outside this diagnosis and its source is protected.

Both installed-baseline logs report:

`[Prepare at crates/sampler-kontakt/src/load.rs:420] lowering: behavior KSP: a prepared capacity is exhausted`

- `ui::v2_tests::keyswitch_real_analog_strings_does_not_invent_preset_browser_articulations`: panic at `src/ui/v2_tests.rs:6714`, before the articulation assertion.
- `ui::v2_tests::mapping_analog_waveform_worker_probe_skips_only_when_library_is_missing`: panic at `src/ui/v2_tests.rs:5772`, before waveform assertions.

Both use `/mnt/MAIN_STORAGE/Libraries/Kontakt/ANALOG STRINGS/Instruments/ANALOG STRINGS.nki`. Its locally hashed identity is recorded in JSON. No binary artifact was executed during this pass. Historical successful Analog runs were also inspected, but they predate the NKA service and use different sources; they are not validation of this source or a fix.

## Actual source path and capacities

1. The UI fixtures call the production V2 loader (`src/ui/v2_tests.rs:5734–5775`, `:6694–6719`), not an isolated presentation model.
2. `sampler-kontakt/src/load.rs:237–274` reads the instrument and carries initialized states into streamed preparation. `initialize_scripts` at `:726–801` uses `sampler_ksp::Limits::LIBRARY`.
3. `sampler-ksp/src/lib.rs:1040–1108` parses, resolves and evaluates initialization. Its library ceilings are declared at `:42–68`: 80 MiB source, 40,000,000 instructions, 65,536 variables, 16,777,216 declared array cells. None is the 8 MiB NKA snapshot ceiling.
4. `sampler-ksp/src/eval.rs:1748–1810` handles explicit-path NKA initialization through `ArrayFileCompletion::synchronous`; `sampler-core/src/array_file.rs:209–263` validates a single array/path/file. Individual shape ceiling: 16,384 cells; file ceiling: 2 MiB. This does not call aggregate `ArrayFileState::dimensions`.
5. `sampler-kontakt/src/load.rs:817–903` compiles initialized states. Frontend failures become unsupported-script records, not the observed `LowerError::Behavior { module: "KSP" }`. Therefore the error wrapper is not direct evidence of a parser/init/compiler budget failure.
6. `sampler-ksp/src/lower.rs:2380–2413` individually validates each explicit-path array used in approved persistence/UI/PGS callbacks and registers it in `Unit::array_files`. Callback/shared-function lowering uses `Program::new` and instruction budgets. `sampler-ksp/src/lib.rs:1503–1520` constructs the runtime array definitions and resources. This performs individual checks but not the aggregate snapshot reservation.
7. `sampler-kontakt/src/load.rs:1003–1013` calls `bind_modules`, then saved automation binding, and flattens any core error to `behavior KSP: ...`. Saved automation preparation (`sampler-core/src/automation.rs:22–45`) returns InvalidInput, not Capacity. The reported Capacity is thus in `bind_modules`.
8. `sampler-ksp/src/lib.rs:649–843` assigns dense script instances, adds widget storage, and binds instances/controls/resources/programs/routes/widgets/shared state. `with_script_resources` (`sampler-core/src/script.rs:58–78`) calls `ScriptResources::apply` (`ops.rs:734–772`). Its first step is aggregate `ArrayFileState::dimensions`.
9. `array_file.rs:113–127` takes the **maximum numeric length and maximum text length**, not sums across all arrays. It computes `(numeric_max * size_of::<i64>() + text_max * size_of::<Text>()) * 8` and rejects above **8,388,608 bytes**. `:129–145` subsequently preallocates all eight job payloads with those maxima. Registering an array in any admitted callback is enough; no runtime file request needs to execute for preparation to fail.
10. `finish_kept` at `load.rs:419–420` only stamps the propagated preparation error. **Line 420 is not the exhaustion check.** `sampler-core/src/lib.rs:353` supplies the generic error text.

### Structural size evidence, not a measured runtime value

Read-only inspection of already-plaintext installed metadata (no commercial script/sample contents copied):

| File | Kind | Rows after header | File bytes |
| --- | --- | ---: | ---: |
| `Library Data/AS Meta Data/name.nka` | text (`!`) | 4,000 | 17,013 |
| `Library Data/AS Meta Data/rhythm_presets.user_data.nka` | integer (`%`) | 16,000 | 48,288 |

`Text` contains a 320-byte array plus length/truncation fields (`ops.rs:500–506`). Regardless of padding, a 4,000-cell text payload has an eight-job **lower bound** of `4,000 * 320 * 8 = 10,240,000` bytes, above 8 MiB. If a 16,000-cell numeric payload is registered in the same bank, the lower bound is `11,264,000` bytes. These numbers do not use a measured Rust layout. They prove rejection for that registered shape without running Rust.

They do **not** prove the authored callback array length, registration, source slot, reachability or numeric/text pairing. No matching Analog product-cache entry was found by the bounded read-only header inspection. No new library/script extraction or loader execution was attempted.

## Alternatives and exact missing evidence

Ranked hypotheses after tracing:

1. **NKA snapshot dimensions**: compatible with the phase/error and installed metadata shape. Root must record actual registered array kind/length and the failing binder step.
2. **Off-audio keyed-store allocation failure**: `ops.rs:600–608` can return Capacity from `try_reserve_exact`, reached by script resources or shared state. The generic receipt does not exclude this; no allocation failure was measured here.
3. **Packing/address overflow**: `bind_modules` u16 script-instance/u32 widget-offset conversions; `script.rs:37–42` instance/bank bounds; `prepare.rs:739–741` polyphonic bank sum. These are exact Capacity sites, but the known historical three-script fixture makes packing overflow a weaker explanation. Shape facts from a historical run are not a fresh observation of the failing run.

Control/program-script validators return InvalidInput for ordinary missing cells/texts/controls. Stage, widget, plan/signal/parameter callback validation and saved automation preparation do not impose the NKA byte ceiling. Core runtime note/voice/continuation capacities are later construction/execution concerns, not evidence for this loader-binding error.

**Pinned blocker:** the receipt collapses the binder step and offending runtime array. Without that datum, neither an allocation-layout change nor a regression can be certified as the minimal complete installed root-cause fix. Even if root confirms dimensions, replacing eight worst-case payloads with demand-sized/shared storage changes admission and snapshot ownership, not merely a constant. It must retain bounded bytes/jobs, atomic failures, request-time save snapshots, cancellation and generation/instance isolation. Do not drop unreachable authored-array validation or lower a script into omission to obtain a UI pass.

## Root-only next validation

No commands below were run. Root owns admission, target/artifact selection and validation; use its existing serialized/quiet validation wrapper, not an automatic hook or shared target shortcut.

First perform one focused instrument preparation with a root-owned diagnostic build that records **binder step, source slot, registered array kinds/lengths, numeric/text maxima, actual `size_of::<Text>()`, computed reservation bytes and configured bounds** at the rejection. Do not print/copy commercial script bodies, metadata contents, credentials or key material. If this is not `with_script_resources -> dimensions`, revise the hypothesis before editing. No diagnostic instrumentation is supplied or committed here.

Existing exact tests to run against the chosen candidate source/artifact:

- Root lib: `ui::v2_tests::keyswitch_real_analog_strings_does_not_invent_preset_browser_articulations`.
- Root lib: `ui::v2_tests::mapping_analog_waveform_worker_probe_skips_only_when_library_is_missing`.
- `sampler-ksp`, integration target `array_file` (run the entire target, then retain exact results for):
  - `nka_unreachable_invalid_typed_ranges_and_snapshot_memory_limit_are_rejected_at_preparation`.
  - `nka_job_queue_is_bounded_and_midi_ids_remain_distinct_and_complete_independently`.
  - `nka_async_save_snapshot_is_request_time_not_delayed_host_capture_time`.
  - `nka_real_and_string_runtime_arrays_keep_typed_ownership`.
  - `nka_wrong_instance_and_cancelled_job_cannot_install_or_complete_a_new_owner`.
  - `nka_completion_is_plan_scoped_across_adoption_and_never_edits_the_replacement`.
  - `nka_runtime_load_preserves_non_ui_identity_and_resumes_wait_after_one_completion_without_heap`.
  - `nka_sync_init_reads_integer_real_and_string_arrays_before_following_statements`.
  - `nka_external_init_is_not_cached_or_replayed_from_an_old_cache`.

Example root-owned selections: `cargo test --locked --profile ci --lib <full-ui-test-name> -- --exact --test-threads=1`; `cargo test --locked --profile ci -p sampler-ksp --test array_file`. These are future selections, not permission for this worker to run Cargo.

Required new original synthetic regression **not authored**: initialize and lower a script with a 4,000-cell text array and a 16,000-cell numeric array used in approved runtime callbacks; bind via the real `Script::bind -> bind_modules -> with_script_resources` path. Confirm baseline rejection and candidate admission only under an explicitly retained bounded storage contract. Cover cap-boundary and over-cap rejection, occupied-job saturation, mixed payload reuse, request-time text save snapshots, cancellation/reuse, cross-instance/generation rejection, and zero audio allocations/frees. A shallow init-only test misses the reported defect. The existing 16,384-text-cell negative test is a real policy constraint, not a tolerance to remove casually.

After focused gates, root should rerun the retained UI suite and protected-worker regression names in `UI-REGRESSIONS.json`. The unrelated native allocation regression remains independently open. UI loading does not establish audio/native parity or performance.

## Unmeasured gates and stop state

No Cargo, rustc, build, test executable, formatter, host, native/official reader, plugin install, render, runtime measurement or auto hook was invoked. No native reference/parity or library interoperability claim follows. No fix/regression is marked passing; snapshot memory, audio-allocation safety, real waveform/keyswitch behavior, cold/warm init/cache behavior, UI retention, audio output and native parity all remain root gates.

Protocol deviation: three Pi child delegations were mistakenly attempted despite the no-nested-agent restriction. They all failed at provider startup (`gpt-5.4` unsupported for the configured account), then were cancelled to dispose delivery. No child source work/result was used; no further delegation occurred. This is recorded rather than described as compliant no-delegation work.

Protected files `src/ui/{native_ui,native_runtime,ir_view,generated,part}.rs` and `src/ui/v2_tests.rs` are unchanged. No watcher, scheduler, polling loop or continuation is left. Terminal handoff; wait for root/user instructions rather than starting further work.
