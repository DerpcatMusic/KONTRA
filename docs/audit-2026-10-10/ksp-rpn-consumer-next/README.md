# Bounded runtime RPN/NRPN consumer — following batch only

**SOURCE_AUTHORED / REVIEW_BLOCKED — not SOURCE_READY for integration and not GREEN.** Source and test commit `3cc0f94ea625715028897d4611c0d477ccfed724`, tree `f8e7a7457ddacd63e1c79e8f00d056abf343ce7a`. Exact parent `2a243bcfa2262abb789265aa0951fd2a85854023`, tree `c9504d7133090d35117048895189f50fdfa8fb6a`. Branch `pi/ksp-rpn-consumer-next`; isolated checkout `/home/derpcat/.t3/worktrees/KONTAKTO/ksp-rpn-consumer-next`.

No Cargo, rustc, clippy, test binary, application, host, native target, or PCM generation was run. All **14 authored Rust tests are NOT_RUN**, not compiled or type-checked. Rustfmt parsed the changed Rust files; Git whitespace and independent Python source/scalar/archive checks are separate static evidence. The first archive-check attempt encountered an un-hashed manifest row; the filtered retry verified all seven sections. This was not a product failure or execution. Root's current frozen batch is separate. This lane does not append to it or authorize any validation.

## Unresolved independent-review objection

Root relayed a preliminary source finding at immutable `3cc0f94e`: parameter-table validation checks only that `binding.stage < stages.len()`. `with_stages` permits same-length reorder/replacement without remapping or rejecting parameter-stage identities; `with_parameter_programs` permits a receiver instance at an unrelated in-range stage. A swapped stage can suppress an original receiver or execute with the wrong projection context. Severity and a concrete reproduction are pending the review's terminal report. No runtime reproduction was run here.

**This is unresolved and blocks future integration.** The source commit is preserved unchanged for review. The authored out-of-range/shrink/duplicate-binding test does not cover same-length stage swaps or wrong in-range instance/stage associations. The next root-assigned fresh followup must resolve exact stage/script identity and setter lifecycle, and add the corresponding regressions, before composing the following validation batch. This handback neither dismisses the objection nor silently claims that the range guard establishes full lifecycle safety. No speculative broad refactor or rushed fix was made.

## Source path and ownership under the compiler's ordinary module binding

Production loader `sampler-kontakt/src/load.rs` initializes source, calls public `sampler_ksp::compile_initialized`, and passes compiled modules to `bind_modules` inside the production lower hook. `src/sound/v2.rs` installs that Prepared in the production Runtime. These untouched callers are pinned in `source-trace.json`; this is source inspection, not an observed loader run.

The changed path is:

1. `set_rpn`/`set_nrpn` lower into typed `Instruction::SendParameter`, not generic Host effects.
2. `bind_modules` binds distinct compiled `EntryKind::Rpn` and `EntryKind::Nrpn` callbacks to a prepared, sorted `ParameterProgram` table. Programs retain their script instance and physical source-slot metadata.
3. `Runtime::send_parameter` gets the **sender's owner PlanId**, source script instance, current source route stage, and captured physical input/performance context. It never substitutes `active_plan` and never traverses the plugin/UI effect queue.
4. It validates both raw integer operands in `0..=16383`, preflights callback-arena capacity and callback-ID arithmetic for the whole immediate fanout, and admits receivers in the same retained plan. Each receiver owns its immutable payload in its existing Continuation. There is no new message heap queue, global payload slot, or per-voice field.
5. `Instruction::ReadParameter` reads address/value from that Continuation, including after waits, other messages, and nested named functions. Outside a parameter callback it returns zero; host slots 2/3 cannot overwrite this payload.
6. `$NI_CB_TYPE_RPN` and `$NI_CB_TYPE_NRPN` are valued constants matched to the existing **internal** callback discriminants. NI documents the names, not numeric ABI values. Internal 5/6 are not a Kontakt ABI claim.

The public core additions are `ParameterKind`, `ParameterProgram`, `Prepared::with_parameter_programs`, and the two Instruction variants. Replacing the program table clears the binding; shrinking the stage table rejects orphaned parameter stages. No new library dependency, option, generic service framework, UI/plugin change, or PGS-string implementation was introduced.

## Explicit KONTRA routing policy, not native parity

- Runtime messages deliver to matching **later compiled module stages only**, in ascending stage admission/start order. The sender's script instance is excluded even if a native caller binds that instance again at a later stage. Earlier stages and self do not receive. A missing later receiver is a successful no-op; there is no newly documented host MIDI output.
- The existing explicit ready stack handles immediate nested work; its reverse enqueue order starts earlier receivers first. If older preempted work exists, the existing deferred FIFO gets ascending enqueue order. Waits/preemption can interleave execution and completion; they do not serialize all receivers behind the first receiver's completion. This is not a native synchronous/asynchronous clock law.
- A receiver that sends a new message starts a new fanout strictly after its own receiving stage. Direct RPN/NRPN forwarding is therefore a directed acyclic route, not self-echo recursion. Mixed authored feedback through other services remains under existing bounded arena/fuel policies.
- Valid input is raw integer address and value, **not** normalized, cents, semitones, pitch sensitivity, or external modulation. Invalid operands fault the sender with `Error::InvalidInput`; no immediate receivers are admitted. Insufficient callback capacity/ID range faults with `Error::Capacity`, also before any immediate receiver admission. Side effects of earlier successful messages are not rolled back if a later message fails.
- Callback memory and ready/deferred queues use existing `Limits.behaviors` budgets. Wait command capacity and behavior-fuel limits remain existing policies. Payload storage adds a small fixed callback field and the Prepared adds an off-audio table; neither CPU nor RAM improvement has been measured.
- A message retains its originating runtime/plan generation. An old sender or receiver waiting through replacement deliberately resumes in **the old retained plan**, never the replacement's script bank or another part. Retirement makes the PlanId stale through existing arena generation checks. There is no external RPN completion token or unfenced UI/host roundtrip to transplant into a new epoch. Plugin slot/epoch fences, MIDI/NKA completion IDs, numeric PGS, and all frozen fade/mod/remap/UVI logic are unchanged.
- Emitting requires a routed performance context (note/controller/control/listener context). Bare plan emission is rejected by the instruction's admission requirement. Initialization still warns/no-ops: **no replay and no initialization parity claim**. The adjacent `set_controller` prohibition was not projected onto RPN; command-specific initialization legality is UNKNOWN.

## Authored regressions and future validation handoff

Package **`sampler-ksp`**, integration target **`rpn`**, file `crates/sampler-ksp/tests/rpn.rs`. `tests.json` gives every exact qualified name, source span/hash, fixture mapping, and required future result. All 14 are **NOT_RUN**.

The synthetic source fixtures go through public source compilation, actual Prepared binding, and the production Runtime. One test has an actual note/PCM assertion authored against the receiver-generated note; another uses real controller routing and generated-note physical-origin checks. These are **future witnesses**, not observed PCM or native receipts. No Kontakt-format file-loader/proprietary instrument fixture was opened, and no new cyclic dev dependency was added for a format-loader test.

Coverage: both raw 14-bit endpoint operands, distinct callback kinds and symbolic equality/inequality, nested named constants/functions, callback payload stability across different-kind and same-kind overlapping waits, ascending later-stage routing, no self/earlier delivery and bounded rebroadcast, invalid operands, atomic capacity refusal, repeated-message waiting-arena pressure, initialization gate, ordinary CC/numeric PGS neighbors, MIDI/NKA job identities/completion waits, retained sender/receiver generation and independent-part isolation, stale retired handles, physical origin/performance, and prepared-table replacement validation. Existing test-only heap guards are reused around production Runtime calls, waits/rendering, failures, replacement, and service completions. Their assertions have not run.

Next central validation owner: **do not integrate this source yet**. First obtain the terminal review and root's fresh followup for the stage-identity/lifecycle blocker. After that correction and separate authorization, compose the **following batch** after the current frozen batch. Include package/target `sampler-ksp`/`rpn` and all fourteen exact names from `tests.json` in the sole combined offline/release `-j1` validation. Use the shared target and existing sccache configuration. No CARGO_TARGET_DIR/RUSTC_WRAPPER override, standalone build, broad extra matrix, or native test is authorized by this report. Existing frozen validation packages/names remain root-owned, not replaced by this lane.

## Primary/native evidence and limits

`primary-authority.json` retains the exact seven section excerpts and digests from the immutable handoff. Static verification matched four complete NI HTML archives and freshly re-extracted every section using the archived heading/nearest section. Precise URLs, archive paths, retrieval timestamps, and hashes are retained there. These establish raw operands, distinct callbacks, payload names and symbolic callback-type names, **not command-specific slot/echo/init/wait timing**.

- set_rpn/set_nrpn archive: `9d71af86f192abff29804487e0c00d7b7b84b9fa191d04d4477f38b893cff0a2`.
- callbacks archive: `f91a2a1411c18e84e15f4e89e74499c3ae74dec75f230bb8c99df65e316267ba`.
- built-in variables archive: `9a5ab14188022767684a1dea13a0ca0e060e0838a788073b7a26b50b0030247e`.
- classic Script Editor archive: `846a09243ad7d17d8daa92bf5105418501ebb25cd358ba8d09108ffb5d4ae7c4`.

REA `current_document` was called read-only and returned `target_unavailable` / no app open. `rea-current-document.json` retains the response. No binary was opened, database imported, protected script/sample/key/PCM exported, process launched, server installed, or UVI reader invoked. **Native UNKNOWN.** AR/DFD/coefficient receipts are not RPN evidence. Historical round10's 16 blobs/44 citations/40 spans and 10 corruption checks remain historical 3665 audit custody, not this implementation's execution credit. `source-trace.json` independently re-traces relevant current parent/successor symbol locations and includes exact Git blobs, full-file SHA256s, span SHA256s and line shifts. This lane records **20 source files, 39 symbol-span records / 67 versioned spans, 13 exact historical RPN-service snippets re-traced into the actual parent, 14 NOT_RUN tests, 4 archives / 7 sections, and 10 rejected synthetic custody-corruption cases**. These static checks do not resolve the independent stage-identity objection.

**Polished feature/native parity and CPU AND RAM below BOTH v1 and Kontakt remain UNACHIEVED.** Source authorship/static custody is not integration readiness, test success, native proof, a merge/publication request, or authorization to touch main/active integration/frozen refs.
