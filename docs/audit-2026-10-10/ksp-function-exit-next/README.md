# KSP function-scoped exit — following-batch source handoff

Status: **SOURCE READY; Rust tests NOT_RUN_NOT_COMPILED.** This is not an append to integration7. Only the root integration owner may include these commits in a future combined build/test batch. No per-lane validation was requested.

## Ordered commits and checkout

Base: `2a243bcfa2262abb789265aa0951fd2a85854023`.

1. Source: `360b14f698051d5300de7a8da96bbba746cd4eb3` — `eval.rs` nearest Call consumes function exit after restoring depth; `lower.rs` emits existing Return for function exit.
2. Tests: `c8c0f610ebf8d49cab921d5b235094f032aaa995` — seven added regressions in the existing `sampler-ksp` / `functions` target; one corrected old assertion.
3. Audit: this separate documentation commit. Its exact SHA/tree and final clean-worktree seal are in the root handoff `ksp-function-exit-next.{md,json}`.

Own new worktree: `/home/derpcat/.t3/worktrees/KONTAKTO/ksp-function-exit-next`, branch `pi/ksp-function-exit-next`. Existing source workers, integration checkouts, frozen checkouts and receipts were not edited. Git commits bypassed hooks explicitly; no build-capable hooks ran.

## Primary requirement, not a native execution receipt

NI authoritative URL: <https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/general-commands#exit>.

> If used within a function, exit only exits the function, but not the entire callback.

Cached complete primary HTML SHA256: `9d71af86f192abff29804487e0c00d7b7b84b9fa191d04d4477f38b893cff0a2`.

Exact DOM excerpt SHA256: `b8395df4c13071247dcf0c80d8b644181f9ed246a0e6b77d0f1dbb82f60474e4`.

`source-checks.json` pins the archive and excerpt paths, extraction method, authority, retrieval timestamp, applicability, and original round15 requirement hashes. The audit independently verified those bytes and re-extracted the exact section from the archive. The archive is unversioned public documentation. It supports the function-vs-callback contract only. Native recursion, wait, reentrancy, errors and frame-limit behavior remain **UNKNOWN**. No fresh native or protected-library run occurred.

Frozen3665 scout evidence was used as a lead, not projected onto current line numbers. Current source and callers were retraced at the exact base. There is no checkout-local Graft graph; `rg` fallback preceded edits. No graph build/refresh was performed.

## Small implementation seam

The initializer already has `Flow` and a nearest Call boundary. It still evaluates the callee block, restores depth even when the result is an error, then propagates the error with `?`. Successful function exit is consumed there. Direct callback exit still reaches the callback boundary. Nested block control flow inside a function is unchanged.

The lowerer already has callback Context, per-context function ranges, call patching and natural Return. None identifies whether the block currently being emitted is a callback body or a function body. One private `Gen::in_function` boolean supplies that distinction. Both existing constructors initialize it false. The existing function-emission loop sets it true only around the function block and restores it afterward. Explicit function Exit emits existing `Op::Return`, with no Forward or End. Direct callback Exit retains its exact Forward + End behavior.

No new Instruction, VM framework, frame representation, dependency or allocator was added. Callback Context and wait forwarding remain unchanged. Return resumes one existing caller PC and is not a callback wait/end/fault deferred-flush barrier. Parser/sema/lowering still visit all source statements after an unreachable exit; no dead-code workaround was added.

## Reserved shared spans and preservation

Inclusive base-source spans were reported before edits:

- `crates/sampler-ksp/src/eval.rs:438–445`.
- `crates/sampler-ksp/src/lower.rs:188–204, 218–225, 379–395, 477–490, 1545–1550` (reported edit interiors were narrower).
- `crates/sampler-ksp/tests/functions.rs:173`; additions after the original line260.

The only old test change is `note_cell(a, 0): Ok(23) → Ok(123)`. The old value explicitly asserted the conflicting behavior: a function Exit stopped the caller before its `+100` tail. The fixture, test name, all other assertions and the other original tests are preserved. That changed assertion has no inherited execution credit.

The audit reconstructs both complete original source files by reversing exactly these changes. It also checks the entire original test prefix with only the stated expectation correction. All other base source bytes are unchanged. Adjacent real Sgn, waveform operands/typed lowering, RPN lowering/binder, core ops/frames/fuel/wait, timer/logging, random/arrays and catalog paths are excluded.

## Relevant callers and actual consumers

`source-checks.json` contains49 immutable SHA/blob/file/span/SHA256 citations. Spans use inclusive1-based UTF8 Git lines with their final LF, without trimming.

- `initialize_inner → eval::run → Eval::block/stmt`, including init, synchronous persistence and async-completion block callers; recursive If/While/Select/Call consumers.
- `compile_initialized_inner → Unit::program`; both `Gen` constructors (`program` and `listener_driver`); function-emission/call-patching loop; recursive statement blocks; builtin call sites from statements, expressions and menu helpers.
- Existing core bounded Call/Return interpreters, continuation program/PC/frames/owner, original-plan resolution, scheduled wait and preemption, and wait/end/fault deferred flush.
- Actual stage guards: note projection forwarded + attack Pending; selection Pending; release Pending→Forwarded; controller pending cleared before downstream delivery. These are read-only dependencies, not new code.
- Public multi-module bind associates programs with original script instances. Tests observe original-plan scalar cells and actual downstream callbacks, not just enum or instruction registration.

## Qualified test filters for the root's future combined batch

Every item below is package `sampler-ksp`, target `functions`, top-level test name. **All are NOT_RUN_NOT_COMPILED.** No command was run or requested.

- `function_exit_init_resumes_nested_and_repeated_callers_but_callback_exit_stops`
- `function_exit_runtime_resumes_callers_without_forwarding_intermediate_pitch`
- `function_and_callback_exit_deliver_note_release_controller_once_and_stop_plan_tail`
- `function_exit_after_wait_preserves_wait_time_stage_delivery_without_duplicates`
- `function_exit_returns_are_not_deferred_note_flush_barriers`
- `function_exit_frames_survive_each_pc_fuel_boundary_wait_and_plan_switch_by_instance`
- `function_exit_never_hides_invalid_unreachable_source`

Preserved/changed existing filters in the same package/target:

- `shared_function_waits_inherit_each_callback_and_return_without_an_extra_forward`
- `nested_function_branches_loops_and_exit_keep_polyphonic_state_and_caller_targets` (corrected assertion)
- `invalid_functions_and_expansion_pressure_fail_during_compilation`

Coverage intent: init/persistence caller tails and skipped function tails; lexical While/Select exits; nested leaf→parent→callback and repeated calls; caller-final pitch and synthetic PCM; top-level Note/Release/Controller/Plan termination; actual downstream counters and attack/release audio; wait-before-exit retains intentional wait-time forwarding without duplicate stage delivery; Return does not flush generated notes before caller edits; one-instruction-per-block fuel preemption through Call/Return and waits preserves original note/program/plan/instance across active-plan replacement. Runtime assertions reuse the existing `support::without_heap` guard, with compilation, binding, plan submission and construction outside it.

The tiny-fuel fixture observes identity through public owner, note-plan, two per-instance scalar banks and unchanged new-plan sentinel cells. It does not expose private frame storage; private frames are pinned and reviewed as source. Exactly-once assertions observe downstream delivery and PCM, not counts of emitted Forward instructions. Wait can forward, and callback end can forward again; existing reached-stage guards provide idempotence.

## No-build checks performed

- `git diff --check` passed.
- Node WASM tree-sitter Rust grammar parsed all three changed Rust files with zero ERROR/missing nodes. Grammar SHA256 and full file hashes are in `syntax.json`. This is syntax checking only, not Rust typechecking or KSP compilation.
- Python source/spec/model audit passed:49 pinned spans; original-file reconstruction; exact old-test preservation; exhaustive two-constructor scope initialization; preserved callback Forward+End; function Return without forwarding; full block walk; exact primary archive/DOM evidence.
- Eight hand-authored abstract continuation cases (fuel1/2/3/1024 × wait/no-wait) checked return-PC retention, one caller frame at a time, repeated calls, owner identity and delivery order. These are **ABSTRACT_MODEL_ONLY**, not emitted bytecode, product fixtures or native execution.
- Eleven in-memory negative controls rejected old eval callback-stop, old lower callback-stop, missing scope initializer/entry/reset, function End, function Forward, missing callback Forward, and three abstract-model control-flow/flush mutations. These checks cannot certify the Rust test expectations.

Audit source: `check_source.py`; syntax parser: `syntax.cjs`. Reproduction uses only already-installed source parsing tools. No product installation or dependency changes occurred. The no-build audit output is `source-checks.json`.

## Limits and stop condition

No Cargo, rustc, clippy, build, Rust test executable, host, native, Wine, official reader, protected library, scheduler, server, install, publication, nested agent or self-loop work occurred. Neither CARGO_TARGET_DIR nor RUSTC_WRAPPER was set.

Independent source review and the sole root-owned future combined build/test remain pending. Syntax/source/model checks do not establish compilation, runtime success, heap behavior or native parity. Complete product polish and significantly lower **CPU AND RAM than BOTH v1 and Kontakt remain UNACHIEVED**. Native runtime remains **UNKNOWN**.

Source work stops at this readiness handoff. No integration7 append or next task is started.
