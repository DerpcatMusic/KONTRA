# Following-freeze: immutable next-batch source preparation

**SOURCE_READY_PREPARATION_ONLY_NOT_AUTHORIZED_NOT_MERGED.**

This packet selects no batch and requests no build or authentication. It freezes the already-reviewed three-fix stack plus its owned-Text fixture successor. The private preparation branch starts at exact CURRENT13 and changes documentation only; it does **not** integrate the candidate. No automatic merge.

## Exact custody

- Base: `13c044d6f9f80576507521a2dc6521e9aad7d9c5`; tree `4af7c9babd93c49cdb0a026a63d25e6c433498b0`.
- Candidate: `c40a41d87ec6f807e8db236fe9831f58132021fd`; tree `67c3d6bf29f767f7d0429583bcaac5796b6f576f`.
- Private worktree: `/home/derpcat/.t3/worktrees/KONTAKTO/pi-parity-release-following-freeze-01a125c0`.
- Branch: `pi/parity-release-following-freeze-01a125c0`.
- The named repository's main checkout was already dirty at HEAD `58c4c715639077bba633bcfe24a421c1f169f3d0`; it was not treated as CURRENT13 source. Source checks used pinned Git objects and the fresh clean CURRENT13 private worktree. No tracked root/release file was written.
- No graft directory exists in this exact private checkout, so navigation used rg. Repository-root AGENTS and installed Ponytail were read.

### Ordered immutable candidate stack (not applied here)

| Commit | Role |
|---|---|
| `7792bb8194134cf802a8a121d0bf9b93973cd335` | fix(ksp): evaluate real sgn operand once without narrowing |
| `7ac37b54014152ff58acd29b89232ad806e8c474` | fix(ksp): return from function exit without ending its callback |
| `1f0d1cfefa305c7958dd2d0bb8467ed9ae278f12` | test(ksp): specify scoped exits across stages waits and preemption |
| `fb283cf78aa12457422c94c45c96b4d8320396fd` | docs: pin function exit source spec and no-build audit evidence |
| `23683f0032a8cb2d29239919d24e931c601c1309` | fix(ksp): preserve display Ext value and address operands |
| `18c5e31217a8c960b24e68ce2bf0717f8881c774` | docs(ksp): pin following three-fix composition and source-only custody |
| `c40a41d87ec6f807e8db236fe9831f58132021fd` | test(ksp): retain owned display text before borrowing it |

All seven are single-parent direct descendants of the stated base. Every per-file patch was reconstructed in memory against exact parent Git bytes, including the no-final-newline primary-exit document. All five original/assembled added/removed byte streams match canonical Myers/no-indent deltas and the original manifest hashes. FunctionExit stable patch IDs differ because of context; **no blanket full-diff or patch-ID equality** is claimed.

Candidate delta: **16 paths**, only **lower.rs/eval.rs** are production changes; **2,353 base paths** are byte/mode-identical. Audit seal 18c5e312 adds only REPORT/source-manifest. c40a41 changes only strings.rs. Existing manifests, ledger, wrappers, targets, source identity, waveform/RPN and other adjacent changes are not overwritten here.

## Feature plan

1. **Real SGN:** reuse one runtime real operand without narrowing; keep existing zero/unordered-comparison policy. Two compile-target regressions specify own RNG continuity and full-bit cases. No native seed/order/domain/infinity claim.
2. **Function Exit:** init consumes successful exit at its nearest function call; runtime function Return resumes caller, while callback exit still forwards and ends. Seven new functions-target regressions plus one changed legacy expectation (23 → 123 after the reached +100 statement). Wait/stage/fuel/plan/frame/heap assertions remain unexecuted.
3. **Ext display + Text successor:** address args 0/2/3/4 and supplied value arg1; distinct registers and existing semantic admission. Three library regressions plus one strings fixture. Retain five owned Text values before borrowing; preserve bound read/write/outcome/wrong-address checks because supplied-value display alone does not validate binding.

The fresh prior review says **SOURCE_REVIEW_CLEAR_ONLY**, retiring **COMPOSED-EXT-TEST-001/F1 in source only**. Its full bytes are sealed as review-snapshot.md. This preparation verifies that review's custody, not a new compiler or native result. The prior blocked review's 401/incomplete JSON remains historical; it is not finalized or promoted.

## Exact test plan

Package **sampler-ksp**. Every row is **NOT_RUN_NOT_COMPILED**. Names are source-located, not obtained from --list or an executed harness. Library namespace is literal lower::display_ext_tests; integration names have no filename prefix. The candidate test spans, Git blobs and SHA256 hashes are in source-ready.json.

| Target | Exact filter | Inclusive candidate lines |
|---|---|---|
| `compile` | `real_sgn_note_evaluates_operand_once_and_preserves_following_rng_draw` | 1301–1356 |
| `compile` | `real_sgn_note_reuses_full_operand_bits_and_preserves_comparison_policy` | 1358–1403 |
| `functions` | `function_exit_init_resumes_nested_and_repeated_callers_but_callback_exit_stops` | 263–290 |
| `functions` | `function_exit_runtime_resumes_callers_without_forwarding_intermediate_pitch` | 292–327 |
| `functions` | `function_and_callback_exit_deliver_note_release_controller_once_and_stop_plan_tail` | 355–401 |
| `functions` | `function_exit_after_wait_preserves_wait_time_stage_delivery_without_duplicates` | 403–446 |
| `functions` | `function_exit_returns_are_not_deferred_note_flush_barriers` | 448–470 |
| `functions` | `function_exit_frames_survive_each_pc_fuel_boundary_wait_and_plan_switch_by_instance` | 472–523 |
| `functions` | `function_exit_never_hides_invalid_unreachable_source` | 525–536 |
| `lib` | `lower::display_ext_tests::display_ext_constant_operands_keep_value_group_slot_and_generic_distinct` | 96–138 |
| `lib` | `lower::display_ext_tests::display_ext_dynamic_operands_are_loaded_once_into_nonaliasing_registers` | 140–163 |
| `lib` | `lower::display_ext_tests::display_ext_semantic_admission_still_requires_all_five_integer_operands` | 165–174 |
| `strings` | `display_ext_supplied_constant_and_dynamic_values_use_existing_pan_volume_tune_formatter` | 742–818 |
| `functions` | `nested_function_branches_loops_and_exit_keep_polyphonic_state_and_caller_targets` | 147–188 |

Count: **13 new + 1 changed legacy**, no inherited PASS. Static comparison independently confirms the 13 added source test declarations and all 14 mapped names.

## Production callers retraced

All below are pinned at c40a41 with whole-file and span digests in source-ready.json and remain identical to CURRENT13:

- `crates/sampler-ksp/src/lib.rs:1033–1111`: initialize -> lexer/parser/sema -> eval::run.
- `crates/sampler-ksp/src/lib.rs:1128–1167`: compile_initialized consumes initialized state, validates rate/bindings.
- `crates/sampler-ksp/src/lib.rs:649–713`: Script::bind -> bind_modules, existing binder.
- `crates/sampler-kontakt/src/load.rs:770–803`: production script initialization.
- `crates/sampler-kontakt/src/load.rs:843–860`: production compile_initialized.
- `crates/sampler-kontakt/src/load.rs:995–1017`: lower_with closure -> bind_modules.
- `crates/sampler-core/src/ops.rs:1194–1212`: normal Call/Return continuation frames.
- `crates/sampler-core/src/behavior.rs:1527–1547`: run_straight Call/Return frames.
- `crates/sampler-core/src/ops.rs:1451–1490`: EngineDisplay Some(value) formats supplied value; None records bound read.
- `crates/sampler-core/src/ops.rs:916–927`: Runtime::script_text returns copied owned Text.
- `crates/sampler-core/src/ops.rs:528–534`: Text::as_str borrows retained bytes.

Changed lower/eval spans and the complete public strings fixture were inspected separately. This is a bounded preparation trace, not a whole-repository correctness review.

## Primary contracts and immutable evidence

Five **SPEC_ONLY** excerpts were re-extracted from hash-matched archived NI HTML using BeautifulSoup html.parser and exact fragment/nearest-section selection. Their exact UTF-8 text, source URL, retrieval timestamp, archive SHA256 and excerpt SHA256 are embedded in source-ready.json. No fetch/native observation. Later checks need no mutable shared cache, handoff or official-docs/sections.json.

- Basic Operators: real/integer signum -1/0/1.
- random()/real(): operand form, not native RNG sequence/order.
- exit: function-only versus whole callback exit.
- get_engine_par_disp_ext(): parameter, value, group, slot, generic.

Immutable dependencies are exact Git commit/blob IDs, this sealed review snapshot/original packet, and embedded primary excerpts. The original composed manifest remains at its original Git path and hash; it is not rewritten. Historical claims inside retained files stay at their original scope.

## Checks performed and retained gates

- Python collector: seven exact per-file patch replays; five canonical cherry delta comparisons; exact ancestry/tree/path preservation; 14 source test mappings; five verified archive/DOM/excerpt pairs; Git diff --check.
- Offline verify.py: all **120 review-table spans**, **55 original committed source/test spans**, candidate test digests, unchanged production caller digests, 13 source-added test declarations.
- Initial collector check stopped on its unsupported no-final-newline handling; the checker was corrected and rerun successfully. This was checker bookkeeping, not a Rust/source failure.
- ZERO Cargo/rustc/clippy/rustfmt/build/test executable/KSP runtime/native app/Wine/reader/bank/perf/host/auth/install/publish/target-wrapper/delegate/schedule operation.

Root alone may select a **later combined batch**, retrace any different-tree composition/conflict, and authorize its validation. Combined candidate-specific typecheck and all 14 regressions remain pending. CURRENT13 compile/list receipts do not transfer. This plan is not authorization, and no future job was started.

Init display remains empty. Broad parameter laws, array equality/search, initializer RNG, indexed inc/dec, math and version-specific native numeric/timing/error/text/localization/quantization behavior stay unresolved. Existing ignored/alias/syntax/model/list/SOURCE_ONLY/UNREAD_BODY statuses retain their original limits.

**Rust: NOT_RUN_NOT_COMPILED. Native: UNKNOWN. Complete polished-product CPU AND RAM below BOTH v1 and Kontakt: UNACHIEVED.**

Deliverable complete; stop after handback. Do not merge this documentation-only preparation branch as if it contained the candidate stack.
