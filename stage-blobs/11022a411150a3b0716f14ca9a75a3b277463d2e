# Bounded RPN deferred-fixture correction

**SOURCE_READY_FOR_FRESH_REVIEW. Not integration-ready, source-review-clear, GREEN, or execution authorization.** All 21 Rust tests are **NOT_RUN_NOT_COMPILED**; types and borrowing are not checked. Heap and PCM results are UNOBSERVED. Native scheduler/order/echo/timing remain UNKNOWN. Complete polished KONTAKTO with lower CPU AND RAM than BOTH v1 and Kontakt remains UNACHIEVED.

## Exact custody and scope

- New exclusive checkout: `/home/derpcat/.t3/worktrees/KONTAKTO/ksp-rpn-deferred-fixture-fix`, branch `pi/ksp-rpn-deferred-fixture-fix`.
- Exact checkout parent: docs-only `273504189d92656e4444552e1c5d8ff548f4c2c2`, tree `fd89663e613282411f9c612b6773d88221bd2b0c`. This carries source e671 and preserves the lifecycle producer's terminal handback.
- This source/test commit: `c6cf5a3da69ed8c72541e5402376430711891501`, tree `11ed4942b0e4194bf73fd059ba903b003e379bcb`, immediate parent exactly 27350418.
- Source diff: ONLY `crates/sampler-ksp/tests/rpn.rs`, +64/-1. This is a bounded F2 fixture response, not a review restart or production scheduler change.
- Before edits, reserved parent line795 and insertion after line813/before815. Existing test name/body range722–813 was the only existing test touched. Corrected exact assertion is now799; the separate new test occupies819–876. All other test bodies remain unchanged.
- No core queue, fuel, PGS, product, UI/plugin, waveform, KSP evaluator/lowerer/binder, option, dependency, or production lifecycle file changed. Main, old producers, reviewers, root/frozen integration checkouts and receipts were not edited.

Required input review JSON rehashed to `885876bba032b2a2cd0dc04fe7379ebefcb74d98b282c4814c48c9ff23e336ac`. Root's `root-ksp-rpn-lifecycle-successor-review-check.json` agrees. `checks.json` pins the read input files and own immutable copies. Round10 requirements JSON matches `613adb5b09356b7616794e0db90b3b38a4d7ba4f54636f8e805c658c465009c7`.

Prior F1 remains **SOURCE-resolved in exact e671, runtime closure pending**. This task changes none of its implementation or tests. Original3cc REQUEST_CHANGES and e671 F2 REQUEST_CHANGES remain preserved historical verdicts; this implementation does not overwrite or independently close either review.

## Why one additional test is necessary

The existing mixed fixture makes RPN receivers write LOG, which emits PGS notifications. It cannot isolate uninterrupted receiver FIFO while retaining that notification behavior and the no-send neighbor comparator. The smallest correction is to keep the mixed fixture and its exact assertions, correct its derived LOG to13425, and add one separate test without PGS handlers or a controller. No arbitrary-order assertion or tolerance was added.

All20 existing harness names remain.19 complete parent bodies and13 complete original3cc bodies are byte-identical. Only the mixed test's expectation/comment changed. The exact package is **sampler-ksp**, target **rpn**. Actual changed/new harness names are:

- `preempted_sender_defers_receivers_fifo_and_preserves_numeric_pgs_controller_order` (retained name, now explicitly a notification-interleaving witness).
- `preempted_sender_defers_receivers_fifo_without_notification_interleaving` (new, essential pure FIFO witness).

`tests.json` records all21 exact harness names, full body line spans/digests, embedded fixture strings/digests, source commit/blob, heap guards and PCM assertion presence. The label `sampler-ksp::rpn::<name>` is an inventory label, **not** a Rust harness module prefix. It must not be used as an exact qualified runtime filter. The unchanged external service fixture is separately pinned in `source-custody.json`.

## Complete deferred-path and phase ownership trace

These are selected source deductions, NOT executed behavior:

1. Public fixture setup uses `compile_with → bind_modules → Prepared → Runtime`. The helper resolves public entry programs, not dense slot guesses. Binder installs later-stage parameter programs and PGS signal programs independently (`lib.rs:614–646,738–808`). The single RPN callback type has its own initially empty code group and entrypc0 (`lower.rs:159–245`).
2. Public `set_behavior_block_fuel(1)` changes configured and remaining fuel (`render.rs:42–45`). Note60 has a32-iteration loop before send, so it yields. Note61 and, in the mixed test, controller work queue behind it. The fixture restores8192 before rendering. `render_inner` resumes a snapshot before due work (`render.rs:171–211`).
3. The sender is popped with younger same-plan work still in `yielded`. `SendParameter` dispatch calls `send_parameter` (`behavior.rs:2480–2503`). Its live same-plan predicate makes **deferred=true**. Admissions are ascending, payload-owning Plan continuations, all admitted before execution (`plan_programs.rs:143–241`, `behavior.rs:1126–1173`). `queue_behavior` appends behind same-plan yielded work, rather than starting receivers through the ready LIFO (`behavior.rs:1356–1462`).
4. **Mixed sender phase:** sender emits RPN1/RPN2, then writes GO1. GO's write emits Signal and admits PGS1/PGS2. The original snapshot's younger/controller work sees new same-plan tail work and requeues. Sender checkpoint queue is `[RPN1,RPN2,PGS1#1,PGS2#1,younger,controller]`. Both receiver progress records remain `(pc0, waiting=false, outcome=None)` and both call counts are0.
5. **Mixed receiver/notification phase:** RPN1 records payload, appends LOG1, emits two NEW PGS notifications and intentionally waits100000us. `resume_yielded` recomputes `len - (pending - done - 1)` and blocks remaining original callbacks behind that new same-plan tail (`behavior.rs:1652–1705`). New PGS1#2 and PGS2#2 see GO1 and unset local `$logged`, append3/4, set those flags, and finish. PGS callback LOG writes do **not** re-signal (`lower.rs:2695–2706`). RPN2 then appends2, emits PGS#3 and waits; these notifications add no digits. The older GO notifications also add none. Younger note61 finishes without sending; controller appends5, publishes its own `$log` read, and emits final PGS notifications, which add no digits. Final exact controller cell is **13425**. Receiver execution order is still1→2, but notifications intervene. This is not a newly diagnosed production bug.
6. **No-send neighbor phase:** no parameter receivers are admitted. GO schedules PGS1/2, which append3/4 once, then younger/controller work runs and controller appends5. Exact unchanged comparator is **345**. Existing logged flags, call-count, raw payload, fault and cleanup assertions remain.
7. **Pure FIFO phase:** only sender and younger work are initially queued. No source has `on pgs_changed` or `on controller`; binder installs no notification handlers. Sender emits RPN1/RPN2 and sets its checkpoint cell. Queue at checkpoint is `[RPN1,RPN2,younger]`; both receiver entries arepc0 with no outcome/wait and zero calls. RPN1 writes LOG1 and waits. Its PGS Signal has zero receivers and therefore appends no callbacks. RPN2 writes LOG2 and immediately reads exact `$log=12`, then waits. Younger work finishes. The exact assertion12 fails on reverse order: a reversed second receiver would read2 (the eventual shared LOG would become21). Payload60/16383, exactly one call per receiver, no fault, heap guard and panic cleanup are also asserted.
8. All observation loops are bounded. The4-frame×256 observation window is1024 samples, below the100000us/4800-sample intentional receiver waits at48kHz. These tests author retained waiting receivers and then cleanup, not wait-completion or native timing closure.

The callback writes, new notifications, once-only flags, waiting ownership, younger work, controller cell read and final no-op notifications are all included. The source-derived requeue model does not assume that only fuel exhaustion can create a same-plan requeued tail.

## Meaningful no-build checks and limits

`check-source.py` reads exact Git objects and writes only this task's docs/owned handoff evidence. It performs:

- Exact parent/tree/single-file diff and unchanged body checks; source bytes verified against the frozen commit.
-10 selected complete source/test files and18 connected spans with SHA/blob/full-file/span digests. Full tests use separately stated body digest rules; selected line-span digests preserve original newlines. Custody does not imply a renewed review of all file contents.
- Exact fixture digit/handler extraction and source-predicate assertions. The bounded Python model derives no-send345, mixed13425, pure FIFO12, plus complete queue snapshots/execution phases. It tracks the pure second receiver's own LOG read.
- Three scalar negative controls: reversed FIFO contradicts12, removed tail precedence contradicts13425, historical12345 is rejected. Three custody negatives reject mutated source/excerpt and wrong parent. These are model/custody checks, not Rust tests.
- Direct `rustfmt --edition2024 --emit stdout` syntax parse, exit0, with output digest and source-unchanged check. No formatting edits, type check, borrow check, Rust/KSP compilation or executable test run.
- Four owned immutable official NI archive hashes and seven precise primary sections freshly re-extracted using existing BeautifulSoup; excerpt bytes and digests match. No install/network/server/native reader was used.
- Exact14 prior neighbor names and eight unique neighbor files rehashed against this source. `future-selection.json` carries them plus the21 RPN names as selection inputs only; no per-lane command or new matrix.

Model limitations: it transcribes selected snapshot/tail/notification predicates, assumes finite handlers fit replenished fuel, models live same-plan callbacks only, and omits the complete VM/arena/audio engine. It is not an allocation, PCM, Runtime, native, or performance receipt. Compilation and actual dispatch/render assertions can still fail for reasons beyond this bounded source trace.

## Primary requirements versus UNKNOWN native behavior

`primary-authority.json` preserves exact URLs, section excerpts and own archived paths. Verified full archive SHA256:

- General commands: `9d71af86f192abff29804487e0c00d7b7b84b9fa191d04d4477f38b893cff0a2`.
- Callbacks: `f91a2a1411c18e84e15f4e89e74499c3ae74dec75f230bb8c99df65e316267ba`.
- Variables/constants: `9a5ab14188022767684a1dea13a0ca0e060e0838a788073b7a26b50b0030247e`.
- Classic Script Editor: `846a09243ad7d17d8daa92bf5105418501ebb25cd358ba8d09108ffb5d4ae7c4`.

These establish raw0..16383 payloads, separate callbacks, shared payload names, symbolic callback constants and simple inter-script communication. General left-to-right changed-event chaining is context, not command-specific RPN scheduler/echo/wait/channel proof. Neither12345 nor13425 is a vendor order claim. Internal callback discriminants5/6 are not a Kontakt ABI claim. Init warning/no-op remains unsupported, not native parity; the adjacent controller-init prohibition is not projected onto RPN. No external modulation, host MIDI output, stringPGS, Falcon parity or full runtime equivalence claim is added.

No fresh native/REA operation was performed in this fixture-only round. The predecessor's exact read-only `target_unavailable` receipt is copied with provenance and digest in `native-limit.json`; it is inherited unavailability, not a fresh observation. No binary, official reader, host, Wine, protected bank/script/sample or proprietary PCM was opened. Unrelated AR/DFD/v1 receipts are not RPN proof.

## Hand back and stop

Root must assign a **NEW independent corrected-source review**. Only after that and separately granted authorization may root compose its one new combined validation cycle including21 RPN tests and selected neighbors. Do not append to the preserved current/terminal RED batch. No build permission, native permission, integration acceptance or GREEN is supplied here.

Source order from27350418 is this test commit, then a separate docs-only commit recorded by durable handoff. Source work stops at **SOURCE_READY_FOR_FRESH_REVIEW**. CPU and RAM have not been measured, and the overall user goal remains UNACHIEVED.
