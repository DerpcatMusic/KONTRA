# RPN two-RED followup — source-only candidate

- Base: `34869c0e4cc64d6c756a1954427da0271f5f379e`.
- Implementation commit: `08b226a23399285f1b2b15cf22b9ce8d187c8aec`.
- Branch: `pi/conflux-rpn-two-red-followup-20261010`.
- Isolated checkout: `/home/derpcat/.t3/worktrees/KONTAKTO/pi-conflux-rpn-two-red-followup-20261010`.
- ROOT alone owns Cargo and combined validation. No root/main/candidate source or installed binary edits. No Cargo, rustc, clippy, Rust test binary, native/host/reader execution, build, or nested agent. Commit hooks disabled to prevent incidental builds.
- Read `/home/derpcat/AGENTS.md`, main repository `AGENTS.md`, and Ponytail instructions. No graft/graph index exists in this isolated checkout; navigation used `rg`.

## Sealed RED, unchanged

Receipt directory: `/mnt/Windows11/DEV_WORKSPACE/kontra-runs/conflux-parity-fixes-20261010T142539Z`.

| File | SHA-256 |
| --- | --- |
| `RESULTS.json` | `3c921f957068b84364b1e1879f4a5c9296988630f90908ff06f2d6d32fcd898a` |
| `test-rpn.log` | `1ef3b2ab5d5bb61e03a75ea59e2fab0ea57edd66f2b93f2ef30065ad1f9864a3` |

The supplied receipt at the base SHA records 22 tests, 20 PASS / 2 FAIL, exit 101. It is historical evidence, not a result for this correction. At the pinned source, line 398 is `collect_retired_plans() == 1` (not a voice-count assertion), and line 482 unwraps `nka_effect` (not `midi.take_reply()`). No receipt was rewritten or relabeled.

## Connected consumers and corrections

1. **Retirement witness missed external NOTE_END acceptance.**
   `Runtime::panic` (`crates/sampler-core/src/lib.rs:1853`) closes notes and stops work, but does not accept terminal notifications. `flush_behaviors_inner` (`behavior.rs:1278`) releases callback ownership, not the external note. `flush_ended` / `retire_note_chain` (`ownership.rs:558-651`) accept the notification and decrement generation notes. `collect_retired_plans` (`plans.rs:397`) requires zero notes/callbacks/pending controls and inactive buses. The existing core test `old_callbacks_and_tails_keep_their_plan_across_sample_exact_adoption` uses this contract.
   Correction: retain the exact retirement == 1 and stale-handle assertions; first prove a refused NOTE_END still retains one note and prevents retirement, then accept exactly the original input's one notification. Completed callbacks must report `Finished`.

2. **NKA fixture used a forbidden callback.**
   The `LoadArrayStr | SaveArrayStr` lowerer (`crates/sampler-ksp/src/lower.rs:2380-2414`) admits runtime file jobs only in persistence/UI/PGS callbacks. In `on note`, it warns and emits `-1`; no file instruction/resource/effect exists. MIDI completes through `complete_midi` / `finish_async_callback` (`midi_object.rs:1107-1206`), so the note reaches NRPN, but its original NKA command can never emit the effect the test unwraps. `request_array_file` / `complete_array_file` (`array_file.rs:382-541`) implement the approved typed job route and shared MIDI ID counter; `Op::WaitMidi` (`ops.rs:1087-1124`) suspends on either kind. Control completion dispatch remains unchanged (`control/transfer.rs:180-188`). The existing `nka_unapproved_note_callback_is_rejected_without_file_effect` and archived NKA round3 contract document the callback restriction.
   Correction: keep note-owned RPN/MIDI/NRPN unchanged; signal an approved `on pgs_changed` callback for NKA and its wait. Use an absolute synthetic path, never opened. Preserve wrong-instance and duplicate-completion assertions, failed NKA completion, final state `(42, 2)`, receiver values `(16383, 0)`, and no-heap guards. Add positive MIDI admission, before/after wait checkpoints, exact NKA plan/ID readback, and advancing shared job IDs.

Exclusive implementation spans: only the two named tests in `crates/sampler-ksp/tests/rpn.rs` and `tests/fixtures/rpn-service-neighbors.ksp`. No production source changed; no tolerance, sleep, guessed render delay, or callback whitelist change.

## ROOT-only exact requests — NOT_RUN here

Run these against ROOT's later combined candidate, not the historical sealed artifacts. ROOT must record the exact combined SHA, commands, logs and outcomes in a new receipt. These are requests, not executions:

```sh
cargo test --release --locked -p sampler-ksp --test rpn sender_and_receiver_waits_stay_in_retained_generation_and_part -- --exact --test-threads=1 --color=never
cargo test --release --locked -p sampler-ksp --test rpn midi_and_nka_job_ids_wait_completion_and_instances_remain_independent -- --exact --test-threads=1 --color=never
cargo test --release --locked -p sampler-ksp --test rpn -- --test-threads=1 --color=never
cargo test --release --locked -p sampler-ksp --test array_file nka_unapproved_note_callback_is_rejected_without_file_effect -- --exact --test-threads=1 --color=never
cargo test --release --locked -p sampler-ksp --test midi_object wait_async_waits_for_midi_jobs_and_invalid_ids_continue_in_init_and_runtime -- --exact --test-threads=1 --color=never
cargo test --release --locked -p sampler-core --test plans old_callbacks_and_tails_keep_their_plan_across_sample_exact_adoption -- --exact --test-threads=1 --color=never
```

ROOT may build these targets together once and run the resulting test artifacts with the same exact filters. RPN still contains 22 tests. Both named REDs and the complete RPN target must be rerun; prior 20 PASS results cannot establish post-change safety.

## Validation and unknowns

Source inspection and `git diff --check` only; changed Rust has not been compiled, formatted by a Rust executable, or executed here. The corrections are supported by existing source contracts, but their runtime results remain **NOT_RUN** until ROOT validates. Compiler/runtime regressions, combined-candidate interactions and allocation guards remain unverified. No new Kontakt timing/ABI, native NKA, host integration, CPU/RAM, or parity claim. The historical RED remains RED.
