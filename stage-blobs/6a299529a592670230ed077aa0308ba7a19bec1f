# RPN/NRPN stage-lifecycle successor — following batch only

**SOURCE_READY_FOR_FRESH_REVIEW, not integration-ready and not GREEN.** This is a bounded response to independent review F1/P2, not an independent closure of that finding. All twenty authored Rust tests are **NOT_RUN**; source is **NOT_COMPILED / NOT_TYPECHECKED**. The fresh successor review and separately authorized central combined validation are still required. Native **UNKNOWN**. CPU AND RAM below BOTH v1 and Kontakt remain **UNACHIEVED**.

## Immutable source custody

- Original integration parent: `2a243bcfa2262abb789265aa0951fd2a85854023`, tree `c9504d7133090d35117048895189f50fdfa8fb6a`. Its terminal RED cycle remains separate and preserved.
- Original RPN source: `3cc0f94ea625715028897d4611c0d477ccfed724`, tree `f8e7a7457ddacd63e1c79e8f00d056abf343ce7a`, REVIEW_BLOCKED.
- Exact fresh checkout parent: docs-only `b557602b540ab2e2b45875d4ec1fec1e38e8e587`, tree `0cf69d1d3fb1a56047ed99e9567a2b42ac393890`. This preserves original code and its explicit blocker acceptance.
- This successor's source/test commit: **`e6710e4e04c2404123a730d5b7977afa6a68c679`**, tree **`0a8599bd90e56f71202bbc1ae5319d012ac9a474`**. Its immediate parent is exactly b557.
- New isolated branch: `pi/ksp-rpn-lifecycle-fix`; checkout `/home/derpcat/.t3/worktrees/KONTAKTO/ksp-rpn-lifecycle-fix`. Producer, reviewer, main, active integration and frozen refs were not edited.

This source commit changes exactly five files, +458/-18: core `stages.rs`, `plan_programs.rs`, `prepare.rs`, `controller_event.rs`, and KSP `tests/rpn.rs`. The controller implementation is unchanged; its documentation now states the existing delegate's lifecycle restriction. No VM dispatcher, runtime queue, fuel policy/default, PGS implementation, UI/plugin, lowerer, callback constants, dependency, option, per-voice field, or frozen fade/mod/remap/UVI logic was changed by this followup.

## Explicit bounded routing/binding policy

1. With a nonempty parameter receiver table installed, `Prepared::with_stages` rejects **any different exact Stage array** with `Error::InvalidInput`. Same length is insufficient. Reinstalling the identical array preserves the table. This handles reorder, substitution, growth, shrink and callback-kind replacement without guessing a new module identity.
2. To change routing, explicitly call `with_parameter_programs(vec![])`, replace the stage schema, then rebind receivers against it. The complete `with_programs` reset still clears the table before resetting stages.
3. `with_release_program` now delegates the new proposed stage array to `with_stages`; it cannot bypass this guard. Identical release binding remains valid. `with_controller_programs` already delegates and retains that behavior; identical controller schema remains valid.
4. `with_parameter_programs` retains range/startability/instance/duplicate-kind checks and additionally requires the receiver instance to match **all ordinary note/release/controller callbacks on its chosen stage** and **all parameter peers on that stage**. An unrelated in-range ordinary route or mixed-owner RPN/NRPN peer pair is rejected.
5. An empty ordinary Stage has no inherent script identity. It may be **explicitly caller-assigned** to a receiving instance. Receivers there must agree with each other. Neither a dense instance number nor a physical source-slot ID identifies that routing position. Swapping indistinguishable empty Stage values does not convey a semantic remapping; any intended receiver reassignment must be explicit in the parameter table. This low-level policy is not a native module-identity assertion and does not redesign independent existing control/listener/PGS route APIs.

The public compiler/binder initially installs each source module's ordinary stages and parameter table coherently. The runtime still uses the sender's retained plan owner, own source instance, actual routed stage and captured physical context. Private continuation payloads, bounded arena admission, callback/wait ownership, internal callback discriminants, later-stage/no-self routing, numeric PGS and MIDI/NKA completion identities remain original source invariants, not observed runtime results.

## Callers, ownership, exports and reservations

Root received the reserved five-file spans before edits and explicitly accepted this reject/clear/rebind policy. Read-only Graft query returned no matching nodes; bounded `rg` was then used. `owners-and-callers.json` maps production paths and the mutation contract. `source-trace.json` provides exact Git blobs, full-file hashes, source spans and current parent/successor line shifts.

Public affected methods: `Prepared::with_stages`, `with_parameter_programs`, `with_release_program`, and the existing `with_controller_program[s]` delegate. `with_programs` reset is traced and unchanged. `ParameterProgram` remains a three-field public struct with clarified routing documentation; no new exports were added. `sampler_ksp::bind_modules` installs stages, control/listener/signal callbacks, and parameter receivers on the control thread. `Script::bind` and `bind_controller_chain` delegate to it. The untouched production Kontakt loader calls the public initialized compiler and binder inside its lower hook; `src/sound/v2.rs` installs that Prepared in production Runtime. This is a source-connected production seam, not an observed proprietary-loader run.

## Twenty precise future test filters

Package **sampler-ksp**, integration target **rpn**, file `crates/sampler-ksp/tests/rpn.rs`. `tests.json` is the exact package/target/harness-filter and inventory-qualified-name mapping, with source SHA, full test span/hash and literal/standalone/generated fixture mapping. Inventory prefix `sampler-ksp::rpn::` is not a Rust harness module prefix. Retained fourteen names are unchanged; thirteen complete bodies are byte-identical, and the original lifecycle test gains a wrong in-range stage assertion. Six focused tests are new:

- `compiled_three_module_rotation_requires_clear_and_exact_receiver_rebind`
- `receiver_only_stage_setters_preserve_identical_schema_or_require_explicit_rebind`
- `parameter_binding_rejects_unrelated_ordinary_and_mixed_receiver_owners`
- `preempted_sender_defers_receivers_fifo_and_preserves_numeric_pgs_controller_order`
- `deferred_fanout_pressure_faults_atomically_without_heap_or_partial_second_message`
- `sparse_physical_slots_nonzero_sender_and_separate_notes_keep_private_payloads`

The three-module rotation test compiles/binds A/B/C, rejects C/A/B with installed receivers, rejects stale B stage1 after an explicit schema change, and rebinds B to stage2. It authors exact payload and generated-note PCM assertions under both the unchanged and rebound schemas. The receiver-only test covers identical and changed controller/release setters, explicit empty-stage reassignment and full program reset. Owner tests distinguish caller-selected empty routing from ordinary/peer mismatches.

Deferred fixtures exhaust the public block allowance1 before the send, queue younger same-plan continuations, then replenish the next block's public allowance8192. Production queues/fuel are untouched. They author `preemptions>0`, both newly admitted receivers at pc0/not waiting, multi-receiver FIFO numeric log12345 versus neighbor-only345, and capacities3/5 with explicit first/second-message atomic refusal. These assertions are intended to distinguish the actual deferred=true branch from immediate LIFO; **fresh review and execution must verify reachability**, not accept yield alone as proof. The Python FIFO sketch is not a Rust scheduler execution receipt.

The sparse-slot test binds physical slots[4,0,2], sender instance1, and two independently triggered note callbacks. Their waits send separate payloads to receiver instance2. Host slots2/3 are overwritten between waits; exact private values, no earlier/self delivery, generated-note physical slot2, PCM and physical-domain hard stop are authored assertions. Setup/compilation/PCM allocation stays outside existing `support::without_heap`; production dispatch/render/failure paths are inside. The guards are **NOT_RUN**.

## Static and primary/native evidence

`build-evidence.py` retrieved immutable Git objects, independently checked test retention and lifecycle/capacity scalar arithmetic, and re-extracted seven exact sections from four immutable official NI archives. `verify-evidence.py` separately checks those manifests/digests and rejects five deliberately corrupted custody cases. These are static metadata checks, not product tests. Own snapshot `/home/derpcat/.t3/scratch/2026-10-10-stop-this-thread-2336e1f0-2f34-7f6415ff/handoff/ksp-rpn-lifecycle-fix-snapshot` contains parent and successor source bytes, test map, original handoffs and owned archive copies. Parent copies were retrieved from immutable Git objects after edits; no mutable producer source was adopted as a baseline.

Counts: **20 selected source files / 40 versioned Git blobs, 27 source-symbol/caller records / 54 versioned spans, 20 complete test spans, 4 NI archives / 7 re-extracted sections, 7 scalar checks, 5 rejected synthetic custody corruptions**. Original scout 16/44/40/10 and producer/reviewer counts remain historical, not additional execution credit. Rustfmt parsed/formatted the five Rust files, then `--check` and `git diff --check` succeeded. No Cargo, rustc, clippy, test binary, application, host, PCM generation, native process/reader/database import, UVI reader, install, server, timer, nested agent, push, publication or merge ran. Source-only Git commit bypassed hooks to prevent automatic builds.

`primary-authority.json` retains precise URLs, excerpt text/digests and original/own archive paths. Archive hashes: general commands `9d71af86f192abff29804487e0c00d7b7b84b9fa191d04d4477f38b893cff0a2`; callbacks `f91a2a1411c18e84e15f4e89e74499c3ae74dec75f230bb8c99df65e316267ba`; variables `9a5ab14188022767684a1dea13a0ca0e060e0838a788073b7a26b50b0030247e`; classic Script Editor `846a09243ad7d17d8daa92bf5105418501ebb25cd358ba8d09108ffb5d4ae7c4`. The round10 requirements JSON matches `613adb5b09356b7616794e0db90b3b38a4d7ba4f54636f8e805c658c465009c7`.

These authorities establish raw integers0..16383, distinct callbacks/payload names, symbolic callback-type names and simple inter-script communication, not command-specific slot/echo/init/channel/wait timing or numeric callback ABI5/6. Later-stage/no-self and lifecycle rejection are explicit KONTRA policy. Init remains warning/no-op and unsupported for runtime replay; adjacent set_controller's init prohibition is not projected onto RPN. No external MIDI-output/modulation feature is claimed. Fresh read-only REA `current_document` returned `target_unavailable`; `rea-current-document.json` preserves it. **Native UNKNOWN**; unrelated AR/DFD/v1 receipts are not RPN proof. Comparative CPU/RAM remains unmeasured and the user goal UNACHIEVED.

## Next owner and ordered handback

Root has launched a NEW independent review of exact e671 source. Stop source work here. Do not append this lane to the current frozen RED batch. A source-clear independent successor review is still needed, including F1 mutators/owner contract and deferred branch/heap/PCM fixture gates. Only after that review and a **new explicit authorization**, root's central validation owner can compose the following one combined Cargo selection, offline/release `-j1`, shared target and existing sccache. This lane supplies all twenty exact filters in `tests.json`; frozen/neighbor package selections remain root-owned. No standalone build, extra matrix, target/wrapper override or native execution is authorized by this handback.

From the original2a source baseline, ordered source/evidence layers are original3cc, producer docs b557, source fix e671, then this separate docs/evidence commit. From b557, only e671 and the new docs commit are required. Final docs SHA and clean branch custody are recorded in the durable `handoff/ksp-rpn-lifecycle-fix.{md,json}` after commit; the original blocked producer handoff is deliberately preserved, not rewritten to suggest it was GREEN.
