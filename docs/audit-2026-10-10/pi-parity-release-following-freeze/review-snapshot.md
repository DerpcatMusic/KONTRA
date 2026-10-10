# Independent source-only review: composed three fixes and Text successor

## Verdict

**SOURCE_REVIEW_CLEAR_ONLY. No blocking source finding identified in the bounded scope.**

**COMPOSED-EXT-TEST-001 / F1 is RETIRED_IN_SOURCE_ONLY.** This is a new independent review, not inherited standalone/composed clearance. It is not compilation, executed-test, runtime, native, performance, merge or integration acceptance.

All **13 new + 1 changed legacy** tests are **NOT_RUN_NOT_COMPILED**, with zero inherited PASS credit. Native is **UNKNOWN**. The complete polished product with CPU **AND** RAM below **BOTH** v1 and Kontakt is **UNACHIEVED**. Root alone may select a future following combined batch. No build or authorization is requested.

## Exact custody and isolation

| Reference | Commit | Tree |
|---|---|---|
| Reviewed successor | `c40a41d87ec6f807e8db236fe9831f58132021fd` | `67c3d6bf29f767f7d0429583bcaac5796b6f576f` |
| Exact parent/audit HEAD | `18c5e31217a8c960b24e68ce2bf0717f8881c774` | `a2c288172cf853106dfbd80fa92291366fcd1ef6` |
| Original composed source | `23683f0032a8cb2d29239919d24e931c601c1309` | `ffe9e45ddfd9d0affa27960577688fe5f9d94e2a` |
| current13 comparison base | `13c044d6f9f80576507521a2dc6521e9aad7d9c5` | `4af7c9babd93c49cdb0a026a63d25e6c433498b0` |

Producer: `/home/derpcat/.t3/worktrees/KONTAKTO/ksp-display-ext-text-fixture-fix`, branch `pi/ksp-display-ext-text-fixture-fix`. Its working tree remained clean. Review used a fresh **read-only Git-archive snapshot** at `/tmp/ksp-text-successor-review-c40-source`, not the producer's mutable source or inherited reviewer checkout. Every one of its 2369 tree entries was compared with the successor's exact Git bytes.

**Parent-tree discrepancy resolved:** Git reports `a2c288172cf853106dfbd80fa92291366fcd1ef6` for parent 18c5e312. The supplied `67c3d6bf29f767f7d0429583bcaac5796b6f576f` belongs to the successor, not the parent. This is a resolved custody note, not an unresolved source defect.

Successor versus parent changes only `crates/sampler-ksp/tests/strings.rs`. Exact old/new block replacement reproduces its entire new file; **2,368 other tree entries are identical**. Original composed source versus parent changes only committed REPORT/manifest audit files. **2,353 base paths** survive unchanged. No production source file is changed by this test-only successor. The ordered five original/assembled cherry deltas independently match canonical Myers/no-indent added/removed bytes. FunctionExit contextual patch IDs differ as recorded historically; no blanket patch-ID/full-diff equality or semantic adapter is claimed.

The committed original source manifest is `docs/audit-2026-10-10/ksp-following-reviewed-three-fixes/source-manifest.json`, SHA256 `ff900e62430d2f0fa4e29f2020b000c436fdd3b77b090d4b54e385af2a4d24bc`. Independently verified: **55 source/test spans, 15 historical Git file versions, 18 manifest input hashes, four successor packet spans, five archive/excerpt DOM pairs**. New successor evidence adds **65 connecting-source ranges** plus whole-file blobs/digests and 14 exact target/filter test records. Git whitespace check passed; this is not Rust syntax/type/format clearance.

Only new durable outputs: `handoff/ksp-three-fixes-text-successor-review.md` and `handoff/ksp-three-fixes-text-successor-review.json`. They were absent before creation. No old handoff, source, current13 checkout, frozen plan or shared target was edited.

## Retiring COMPOSED-EXT-TEST-001 / F1

Original `strings.rs:789–792` returned `unwrap().as_str()` from a closure. The actual API, `Runtime::script_text(...) -> Result<Text, Error>`, calls `.copied()` and returns an **owned Text**, not a borrowed view into runtime storage. `Text` owns its fixed-capacity bytes, and `Text::as_str(&self) -> &str` borrows those bytes. The old temporary could not outlive the closure return. This was manual lifetime reasoning, not an observed compiler failure.

The successor `strings.rs:788–799` maps indices 0–4 to owned Texts and retains the resulting array in the surrounding scope. Only subsequent assertions call `as_str()` on array elements. No reference escapes the map; the array remains alive for every assertion. The full test `742–818`, existing `run_shaped` helper, actual Text/script_text APIs and engine outcome/formatter callers were inspected.

The same five retrievals, nonempty assertion, constant/dynamic equality checks, stored-value inequality checks, cell checks, read/write outcome ordering, complete address identities, final binding value and wrong-address checks remain. No String allocation, production API, helper, shared allocator or dependency was added. Source retirement **does not establish that the target compiles or the assertions pass**.

The prior composed reviewer left a detailed **BLOCKED** Markdown, then failed **401 token_expired before JSON/final handoff**. That review is not terminal-complete or cleared. Its original JSON remains absent. These two new files neither overwrite nor finalize that earlier review.

## Fresh review of all three production fixes

### Real SGN — source clear only

Typed real branch allocates existing dst,t,u registers, evaluates arg0 once into dst, then initializes both zero scratches. CompareReal u>dst overwrites only u, preserving full IEEE64 dst. CompareReal dst>t overwrites dst with 0/1. Binary32 subtraction runs only on comparison integers. Source formula (x>0)-(0>x) reuses one evaluated operand without truncating/moving/re-evaluating it. Register overflow checks/Op::locals validation and compile-bind consumers remain existing. Scratch after operand evaluation avoids nested argument clobber; no extra hidden cell, Op or service. Signed zero and unordered comparisons retain own zero result. Authored tests pin own next RNG draw; no vendor seed/evaluation-order/NaN/infinity claim.

Reviewed full builtin/numeric lowering context, argument/value/assignment helpers, typed admission, bit helpers, operation locals and actual compile/bind/dispatch consumers. The two fixtures force runtime real operands and preserve next-draw/result assertions. Own hard-coded RNG draw and source models are not Kontakt observations. Authored cases do not establish native infinity/domain/NaN policy.

### Function Exit — source clear only

Full program construction begins with in_function=false in each Gen constructor; true only while emitting a function body and restored false before natural Return. Function Exit emits existing Op::Return without forward/End; callback Exit still forwards then End. Eval Call restores depth even on a returned error, then propagates errors while consuming successful Flow at nearest function boundary. Nested callers resume; direct callback Exit still propagates from blocks. Sema rejects invalid dead code and unsupported function shape independently of runtime Exit. Call/Return consumers preserve bounded return-PC frames in normal dispatch and run_straight; waits/yields retain the continuation/frame/owner, and Return is not a deferred-note flush boundary. Normal forwarding is guarded by reached attack/release/controller state, not by counting emitted forward instructions. Wait deliberately may forward earlier. No changes to core frame capacity, fuel, waits, deferred flushes, ownership, plan retention or stage machinery. Source-created production path and all test helpers inspected; no execution proof.

Reviewed both Gen constructors, full program/function emission, block/statement/builtin lowering, init block/Call flow and error restoration, Sema function/dead-code admission, Call/Return in both dispatch paths, continuation/frame ownership, intentional waits, fuel preemption, deferred-note flush boundaries and guarded stage forwarding. The legacy change from 23 to 123 is consistent with the caller's now-reached +100 statement, but its earlier PASS cannot carry forward to the changed test.

### Ext display routing — source clear only

Ext text lowering calls existing engine_address with write layout selecting args0/2/3/4, then loads supplied arg1 into free+4. All four address fields and value occupy distinct registers; each source argument is lowered once. Sequential address-first evaluation remains own ordering, not an asserted native left-to-right law. Ordinary display/read uses args0/1/2/3; set uses 0/2/3/4 with value1. Existing checked reg allocation/program-local accounting and contiguous address decoding remain unchanged. Core EngineDisplay Some(value) intentionally does not validate binding or record a read outcome: plausible text alone cannot prove address. Library tests assert emitted identity/nonaliasing/admission; public fixture also asserts real read/write outcomes, wrong addresses and binding state. Fixture retains own PAN/VOLUME/TUNE formatter comparisons, not native strings. Init empty display and broad parameter laws remain separate gaps.

Reviewed complete text append/lowering context, read/write address helper, register limits and actual operation decoder/formatter. Typed five-integer admission remains. The public fixture's supplied-value display is deliberately paired with actual read/write/address checks; the string comparison alone is not a binding-validation test. There is no new initializer/provider/catalog/full-parity claim.

### Composition and production callers

Actual public chain inspected: compile/initialize/compile_initialized → admission/init/lower → Script::bind/bind_modules → Program::new → Runtime::new → callback/op dispatch. Production `sampler-kontakt/src/load.rs` callers initialize scripts, compile initialized states, then bind modules through lower_with. Public script state/Text/engine controls and existing test limits/helpers were examined manually, not inferred solely from diffs.

Exact tree/cherry custody preserves adjacent RPN typed callbacks, waveform typed Store/init cache/local ranges, source identity/highlight/remap/Switch/UVI/NCKP/semitone endpoints, binder, VM, formatter and dependencies outside reviewed hunks. No whole old lower/eval file was substituted. **Preserved bytes are not proof of correctness or native compatibility.**

## Cached primary contracts — SPEC_ONLY

DOM sections were independently selected by exact fragment ID/nearest section and re-extracted with BeautifulSoup `html.parser`, `get_text(' ', strip=True)`; UTF-8 bytes match retained excerpt files and hashes. No fetch or REA/native call was made. These unversioned manual pages do not establish version-specific runtime behavior.

### Basic Operators

- Primary: https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/arithmetic-commands---operators#basic-operators
- Cache: `/home/derpcat/.t3/scratch/2026-10-10-stop-this-thread-2336e1f0-2f34-7f6415ff/handoff/official-docs/ksp-arithmetic.html`
- Archive SHA256: `3bef630209e32d0a099168af024bded981619209b48908b4cc00743617a3df8a`
- Retrieved: `2026-10-10T03:05:30.511268+00:00`
- Exact retained DOM text: `/home/derpcat/.t3/scratch/2026-10-10-stop-this-thread-2336e1f0-2f34-7f6415ff/handoff/round14-selected-sections/ksp-arithmetic-basic-operators.txt`
- DOM/excerpt SHA256: `71ea38cee3883030dc52f54b403d463496b9536ce313c428953d3de69352a191`

### random()

- Primary: https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/arithmetic-commands---operators#random--
- Cache: `/home/derpcat/.t3/scratch/2026-10-10-stop-this-thread-2336e1f0-2f34-7f6415ff/handoff/official-docs/ksp-arithmetic.html`
- Archive SHA256: `3bef630209e32d0a099168af024bded981619209b48908b4cc00743617a3df8a`
- Retrieved: `2026-10-10T03:05:30.511268+00:00`
- Exact retained DOM text: `/home/derpcat/.t3/scratch/2026-10-10-stop-this-thread-2336e1f0-2f34-7f6415ff/handoff/round14-selected-sections/ksp-arithmetic-random--.txt`
- DOM/excerpt SHA256: `4378bfee255e1a292bc94aaf3b7e33c9e61927ee87138ef4345eb8e0f96c4fee`

### real()

- Primary: https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/arithmetic-commands---operators#real--
- Cache: `/home/derpcat/.t3/scratch/2026-10-10-stop-this-thread-2336e1f0-2f34-7f6415ff/handoff/official-docs/ksp-arithmetic.html`
- Archive SHA256: `3bef630209e32d0a099168af024bded981619209b48908b4cc00743617a3df8a`
- Retrieved: `2026-10-10T03:05:30.511268+00:00`
- Exact retained DOM text: `/home/derpcat/.t3/scratch/2026-10-10-stop-this-thread-2336e1f0-2f34-7f6415ff/handoff/round14-selected-sections/ksp-arithmetic-real--.txt`
- DOM/excerpt SHA256: `57270c689a05537fc392290b45bcbe775b05bcffa5591bf936c949a86cbebdf5`

### exit

- Primary: https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/general-commands#exit
- Cache: `/home/derpcat/.t3/scratch/2026-10-10-stop-this-thread-2336e1f0-2f34-7f6415ff/handoff/official-docs/ksp-general-commands.html`
- Archive SHA256: `9d71af86f192abff29804487e0c00d7b7b84b9fa191d04d4477f38b893cff0a2`
- Retrieved: `2026-10-10T05:06:52.052195+00:00`
- Exact retained DOM text: `/home/derpcat/.t3/scratch/2026-10-10-stop-this-thread-2336e1f0-2f34-7f6415ff/handoff/round15-selected-sections/ksp-general-commands-exit.txt`
- DOM/excerpt SHA256: `b8395df4c13071247dcf0c80d8b644181f9ed246a0e6b77d0f1dbb82f60474e4`

### get_engine_par_disp_ext()

- Primary: https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/engine-parameter-commands#get_engine_par_disp_ext--
- Cache: `/home/derpcat/.t3/scratch/2026-10-10-stop-this-thread-2336e1f0-2f34-7f6415ff/handoff/official-docs/ksp-engine-commands.html`
- Archive SHA256: `df2fe7192f32df7c4a738c713663bafdfec3630c2821fd9afe1887c7d5ed006d`
- Retrieved: `2026-10-10T03:03:28.593743+00:00`
- Exact retained DOM text: `/home/derpcat/.t3/scratch/2026-10-10-stop-this-thread-2336e1f0-2f34-7f6415ff/handoff/round16-selected-sections/ksp-engine-commands-get_engine_par_disp_ext--.txt`
- DOM/excerpt SHA256: `d1ed157ba48c640e622c65bf3c4481934e60f6c3e584c9ee5db1ca007ea156a4`

Basic Operators supplies real/integer signum -1/0/1. random()/real() support the operand form, not a native seed/draw/evaluation-order claim. Exit explicitly distinguishes function exit from entire callback exit. Ext explicitly takes **parameter, value, group, slot, generic**. Exact cached excerpts, archive hashes and DOM extraction identity support these narrow source contracts only.

## Test accounting

**Every row is NOT_RUN_NOT_COMPILED.** Package `sampler-ksp`; exact harness names below. Integration targets use top-level names, not filename prefixes. Library tests use their real `lower::display_ext_tests::` namespace. No --list/test binary was invoked.

| Kind | Target | Exact filter | Successor source span |
|---|---|---|---|
| New | `compile` | `real_sgn_note_evaluates_operand_once_and_preserves_following_rng_draw` | `crates/sampler-ksp/tests/compile.rs:1301–1356` |
| New | `compile` | `real_sgn_note_reuses_full_operand_bits_and_preserves_comparison_policy` | `crates/sampler-ksp/tests/compile.rs:1358–1403` |
| New | `functions` | `function_exit_init_resumes_nested_and_repeated_callers_but_callback_exit_stops` | `crates/sampler-ksp/tests/functions.rs:263–290` |
| New | `functions` | `function_exit_runtime_resumes_callers_without_forwarding_intermediate_pitch` | `crates/sampler-ksp/tests/functions.rs:292–327` |
| New | `functions` | `function_and_callback_exit_deliver_note_release_controller_once_and_stop_plan_tail` | `crates/sampler-ksp/tests/functions.rs:355–401` |
| New | `functions` | `function_exit_after_wait_preserves_wait_time_stage_delivery_without_duplicates` | `crates/sampler-ksp/tests/functions.rs:403–446` |
| New | `functions` | `function_exit_returns_are_not_deferred_note_flush_barriers` | `crates/sampler-ksp/tests/functions.rs:448–470` |
| New | `functions` | `function_exit_frames_survive_each_pc_fuel_boundary_wait_and_plan_switch_by_instance` | `crates/sampler-ksp/tests/functions.rs:472–523` |
| New | `functions` | `function_exit_never_hides_invalid_unreachable_source` | `crates/sampler-ksp/tests/functions.rs:525–536` |
| New | `lib` | `lower::display_ext_tests::display_ext_constant_operands_keep_value_group_slot_and_generic_distinct` | `crates/sampler-ksp/src/lower_display_ext_tests.rs:96–138` |
| New | `lib` | `lower::display_ext_tests::display_ext_dynamic_operands_are_loaded_once_into_nonaliasing_registers` | `crates/sampler-ksp/src/lower_display_ext_tests.rs:140–163` |
| New | `lib` | `lower::display_ext_tests::display_ext_semantic_admission_still_requires_all_five_integer_operands` | `crates/sampler-ksp/src/lower_display_ext_tests.rs:165–174` |
| New | `strings` | `display_ext_supplied_constant_and_dynamic_values_use_existing_pan_volume_tune_formatter` | `crates/sampler-ksp/tests/strings.rs:742–818` |
| Changed legacy | `functions` | `nested_function_branches_loops_and_exit_keep_polyphonic_state_and_caller_targets` | `crates/sampler-ksp/tests/functions.rs:147–188` |

No fixture parse/typecheck, execution, no-allocation, audio, state/result, completion or retirement success is claimed. Existing support::without_heap is reused; its presence is not an executed heap receipt. Historical syntax/source models are neither Rust compilation nor emitted KSP/VM execution. Test SHA256/blob evidence is in the JSON.

## current13 exclusion and retained gates

The read-only current13 custody records report the already-authorized **one Cargo build exit 0**, then **53 successful test-list receipts**, index interruption by foreign compiler contention, and provider/auth failure. Lists are not behavior executions. These facts do **not** compile or test 23683, 18c5e312 or c40a41. No execution credit transfers; no batch append, authorization repair, heavy job or new build was requested.

- Root-controlled future composed compile/typecheck and exact 13 new + 1 changed legacy regression execution remain pending; no standalone build request.
- All authored heap, semantic-result, audio, stage, wait/fuel/plan/frame, completion/retirement assertions require actual own runtime validation.
- Native Kontakt/Falcon/UVI behavior and exact version-specific errors, seed/state/order, callback timing, NaN/zero/numeric boundaries, recursion/wait/reentrancy/frame laws and display localization/Unicode/quantization/ranges/switches/stepped laws remain UNKNOWN.
- Ext initialization remains empty and host-facts/display-provider/full parameter-law coverage remains unresolved.
- Array equality/search, initializer RNG, indexed inc/dec and broad math are separate unresolved gates. Broad UNREAD_BODY, SOURCE_ONLY, ignored/alias/syntax/model/list statuses remain at original scopes; this review does not promote them to PASS.
- Current13 receipts/index interruption/provider-auth failure and any transport/hash/frozen-plan objections remain at their own scope; no build/authorization recovery attempted.
- Polished complete product with CPU AND RAM below BOTH v1 and Kontakt remains UNACHIEVED.

## Evidence pins and read limits

All source spans below are **inclusive 1-based UTF-8 LF Git lines**. SHA256 includes the original final LF. Each JSON pin also carries commit, Git blob and whole-file SHA256. Spans are connected source evidence, not executable receipts. Four key complete Text/fixture/SGN excerpts are included in the JSON. Historical manifest pins and original objection records are retained separately at their original commits; no original line number is silently promoted to successor proof.

Some long tool reads were truncated. Relevant changed functions, types, APIs, helpers and connecting consumers were read in bounded chunks; no claim is made that every byte of every unrelated historical report was semantically reread. Exact hashes preserve complete historical input records. Broad UNREAD_BODY, ignored, alias, SOURCE_ONLY, syntax/model/list and native/performance gaps stay at their existing scopes. This is a bounded three-fix-plus-fixture review, not a codebase-wide audit.

### Successor source ranges (commit c40a41d87ec6f807e8db236fe9831f58132021fd)

| Path:inclusive lines | Git blob | Span SHA256 |
|---|---|---|
| `crates/sampler-ksp/src/lower.rs:153–284` | `d7e9f4ec95dec39d3c275234811e369390d840c9` | `ceeda790d30282f486dbd8751018803c8aaa8b790cddb98ff84bd420fce08e8b` |
| `crates/sampler-ksp/src/lower.rs:285–323` | `d7e9f4ec95dec39d3c275234811e369390d840c9` | `b6a63714adf99ba73b26d35eeafcd6eb56141e2dbf3afccfa03c32c5ad79a9b1` |
| `crates/sampler-ksp/src/lower.rs:381–450` | `d7e9f4ec95dec39d3c275234811e369390d840c9` | `7162ddecc6fb7021d24471c0ec43a3db039eb3e7a220391138c60850877f824e` |
| `crates/sampler-ksp/src/lower.rs:480–535` | `d7e9f4ec95dec39d3c275234811e369390d840c9` | `ba8e5e3e818eaecd6e0cd0ecf69b04caa89e1d0bbce0c225b0a5099c1d8caf82` |
| `crates/sampler-ksp/src/lower.rs:595–602` | `d7e9f4ec95dec39d3c275234811e369390d840c9` | `6546f5fb950a7d0ebb4ddfc6f7d054c510a36d87dbd9f755106c85b416840f26` |
| `crates/sampler-ksp/src/lower.rs:642–746` | `d7e9f4ec95dec39d3c275234811e369390d840c9` | `29a6aba1d47d718a8041970324e58ca098b349e93e2974ede6cbe9c8b7311a45` |
| `crates/sampler-ksp/src/lower.rs:747–1016` | `d7e9f4ec95dec39d3c275234811e369390d840c9` | `908e2eecc4f241d759348d639d47fa97dc2a6b8988caa9f650dae8028f658fe7` |
| `crates/sampler-ksp/src/lower.rs:1177–1382` | `d7e9f4ec95dec39d3c275234811e369390d840c9` | `659d34be226cb4360dd77d39787f16efdc064bfb6c22ab08d62f155febba6004` |
| `crates/sampler-ksp/src/lower.rs:1573–2836` | `d7e9f4ec95dec39d3c275234811e369390d840c9` | `c054c256862278c2cafe3cabc3816e5d64e0e994e3f80468e52159acbd821448` |
| `crates/sampler-ksp/src/lower.rs:3667–3683` | `d7e9f4ec95dec39d3c275234811e369390d840c9` | `924596b6cb07f9ea21806b6c1c29fc08c4d61022c17007874875418beb47919b` |
| `crates/sampler-ksp/src/lower.rs:4068–4071` | `d7e9f4ec95dec39d3c275234811e369390d840c9` | `3e738dde8d501d9aaee7aa3cb9818eac8d39134226b6599782ef063bf0a9825d` |
| `crates/sampler-ksp/src/eval.rs:244–358` | `8e5e5a6e1e9e245963426f3be1e8d3f68a2ee9b6` | `35a38c024ab76f395cc98680ad8e7e707940d161b9019d1eeaaa638588954f5f` |
| `crates/sampler-ksp/src/eval.rs:382–452` | `8e5e5a6e1e9e245963426f3be1e8d3f68a2ee9b6` | `6bc729d392ce3338873ec15492f2e1fde6795724417066c51f47adf758c0a2ea` |
| `crates/sampler-ksp/src/eval.rs:964–1044` | `8e5e5a6e1e9e245963426f3be1e8d3f68a2ee9b6` | `f8d7f5191fbb9285b78bc0183a16dc1b797f95531fc95df0ef663b41109f3321` |
| `crates/sampler-ksp/src/eval.rs:1585–1625` | `8e5e5a6e1e9e245963426f3be1e8d3f68a2ee9b6` | `83ba5656f1f9e853b78b70ec8306fd5165ff1048388297c56e80283a9b7b1d38` |
| `crates/sampler-ksp/src/eval.rs:1629–1674` | `8e5e5a6e1e9e245963426f3be1e8d3f68a2ee9b6` | `7e8d8521ea57e71156464951d877b9912c9356051bc7b0b7592d699da445b0ce` |
| `crates/sampler-core/src/ops.rs:1–12` | `5fd265bc6d4992014a070c42d66a119cf3419870` | `0901a99340bc2a215b04cadd999e8422f4a89138d3b303c9dd0b8e2a99f32594` |
| `crates/sampler-core/src/ops.rs:367–431` | `5fd265bc6d4992014a070c42d66a119cf3419870` | `a420f2fb0899837aec1c291e2a8892a957fa6af62b746bcb9a2a9bfa8fb79cce` |
| `crates/sampler-core/src/ops.rs:498–573` | `5fd265bc6d4992014a070c42d66a119cf3419870` | `c6ac4270d099e6f0cb668e9e7b8e4e5370fad3636c5785db5279b4fe1076ed74` |
| `crates/sampler-core/src/ops.rs:779–800` | `5fd265bc6d4992014a070c42d66a119cf3419870` | `c304abe01dffce2e3443a61c34358838e8956364de15ad416c4a6dde0dde761c` |
| `crates/sampler-core/src/ops.rs:916–976` | `5fd265bc6d4992014a070c42d66a119cf3419870` | `a9ac29e4b6b7e441fc3b81cad44bdb7cd6b1aef757a7423127143e2399019242` |
| `crates/sampler-core/src/ops.rs:995–1017` | `5fd265bc6d4992014a070c42d66a119cf3419870` | `356047487b5aac7ecbbc63c8cf9954294e15f98757576e35bf44a75815f6ef56` |
| `crates/sampler-core/src/ops.rs:1148–1219` | `5fd265bc6d4992014a070c42d66a119cf3419870` | `5354447837e676b7f1b9dda7adba84e7a461840d51fdab0eca6a45231ac775f1` |
| `crates/sampler-core/src/ops.rs:1408–1494` | `5fd265bc6d4992014a070c42d66a119cf3419870` | `fd28514e054ed2ccc4d2ba9a7a4b0894de68971778246a77175b5fd970bc7752` |
| `crates/sampler-core/src/behavior.rs:636–736` | `a1ff442ba50002d089859623cc2524ad818c48c5` | `28344250145c65f5508d49051a13db7b02a39c541f380237b36b2efa4a24135c` |
| `crates/sampler-core/src/behavior.rs:977–997` | `a1ff442ba50002d089859623cc2524ad818c48c5` | `3cf965c45447219ab79b158db4ed4b0e64616e4d617d1e6aa2b27cf635435103` |
| `crates/sampler-core/src/behavior.rs:1393–1452` | `a1ff442ba50002d089859623cc2524ad818c48c5` | `cc683fd44d66595f95f1cc076340c917de08fedb30245fde49575c0e9cefec15` |
| `crates/sampler-core/src/behavior.rs:1527–1576` | `a1ff442ba50002d089859623cc2524ad818c48c5` | `673a29affe237f59ea5a57df11004a69d2e4f8a86056b265679d266d01d41d30` |
| `crates/sampler-core/src/behavior.rs:1695–1730` | `a1ff442ba50002d089859623cc2524ad818c48c5` | `676cc4b9ec9ffab8794be72ebbaa6dfb4378325c9167479f9499c6bad770a39f` |
| `crates/sampler-core/src/behavior.rs:1808–1838` | `a1ff442ba50002d089859623cc2524ad818c48c5` | `4a84d54790177a0c2c568dad99f9e54ea50a2477a0564a1ad7d636506b197da1` |
| `crates/sampler-core/src/behavior.rs:1968–1985` | `a1ff442ba50002d089859623cc2524ad818c48c5` | `5e95be3f2bdc1bfa4ec46600a8389c554eab90f0d168332dadee8282b9df0fb0` |
| `crates/sampler-core/src/behavior.rs:2590–2629` | `a1ff442ba50002d089859623cc2524ad818c48c5` | `6f4432980c6cb2b5501d291230b87aa640596ce4c22eac85383cdb5955f4a0f2` |
| `crates/sampler-core/src/behavior.rs:2699–2746` | `a1ff442ba50002d089859623cc2524ad818c48c5` | `22e0356bd88b1a03446f15d0e0d92eb2bd8a921aed3089bd48a45ddbae91d60a` |
| `crates/sampler-core/src/behavior.rs:2791–2809` | `a1ff442ba50002d089859623cc2524ad818c48c5` | `65ca6ef410fcc8a2e4f5c2d3ddf5525eb69e4dd8126671e7097937d04d83809f` |
| `crates/sampler-core/src/stages.rs:100–144` | `7e361cd751d5ddd2e41beca9a4d444e80690a3b1` | `95a42040744c89ac3a4bf0b60234850beefa77d0e3ac6f95fdf7cac8099f59db` |
| `crates/sampler-core/src/release.rs:480–496` | `c62913b0f2ea09538252353f19e4035ec1c75b79` | `828ef97fa9d2cc252c47fc7dd5b414c2a01dcdbd05c1205b8309d23b239c17bb` |
| `crates/sampler-core/src/controller_event.rs:308–373` | `7c6bf111868890deb19f3a7d14a042a1ff5033e1` | `51a5e735297feb273d08fac14da26462b5a51f61721aa1ba65f5bb94ff03273c` |
| `crates/sampler-core/src/engine_parameters.rs:761–851` | `bd48614da4b9b437871ebd894016f42861d12bd2` | `b36d62e79f891042ae5c0f9865ff9e8ec491b8bfb433d998c97093df4e2e032a` |
| `crates/sampler-core/tests/support/mod.rs:1–78` | `556402ece1f85fba69f811b509431c51b9909b37` | `d4b3a59fb7f2e9b046e1feef38e1dbb3b4261efc8ff40bf5a89eac7d03100d46` |
| `crates/sampler-ksp/src/lib.rs:649–843` | `a5e57862e820c58880212998c22f6b6eee8f79dd` | `32fcf3a330cbb0098f9563df6cbe22050e939f2f43e5f0c9e46c304c32a3df90` |
| `crates/sampler-ksp/src/lib.rs:846–948` | `a5e57862e820c58880212998c22f6b6eee8f79dd` | `218b2900bd36a8a9a64a0bd0fbf87e815db8f669074602660c24e5500341a4a1` |
| `crates/sampler-ksp/src/lib.rs:1033–1167` | `a5e57862e820c58880212998c22f6b6eee8f79dd` | `e5ecafbdf185e602fe37d16e4edbf5b36d2f95c77bab87b877a5131fe0d0da8a` |
| `crates/sampler-ksp/src/lib.rs:1216–1318` | `a5e57862e820c58880212998c22f6b6eee8f79dd` | `829ce24e4e6bedb5bf4aa9ff240d040914def101a1949009eaed0d8cc196dca5` |
| `crates/sampler-ksp/src/lib.rs:1383–1525` | `a5e57862e820c58880212998c22f6b6eee8f79dd` | `92d14b0ed7e66bafd602b35b5cbb775c7564db81ffc82a5515d3b471119fd6e9` |
| `crates/sampler-ksp/src/sema.rs:37–224` | `11a867979633a57f72db5badeb62189a149b3e68` | `60a44ad11ca59ad8f69c2a6670129ec1cf254761b5efcf37f480efb2f26cc00e` |
| `crates/sampler-ksp/src/sema.rs:322–378` | `11a867979633a57f72db5badeb62189a149b3e68` | `1a27abc7bc31d62b2292172e65b0eb6cd1c11df0af82a057fcfd001445f662c4` |
| `crates/sampler-ksp/src/sema.rs:1139–1230` | `11a867979633a57f72db5badeb62189a149b3e68` | `e9f7cd87e9db14f7bffa3a84e6977f178afdfea67578f152dc958c412094a859` |
| `crates/sampler-ksp/src/sema.rs:1241–1360` | `11a867979633a57f72db5badeb62189a149b3e68` | `de0e212d02d97e3941635da96a7900e13ff6c52500e8536c9c7e6b0fac1065f4` |
| `crates/sampler-ksp/src/builtins.rs:70–115` | `314bd26e762252837b209753a3ff10be5910eaef` | `d11694e54741da700104b698fd4f253616888f7d35d0f4ed672027fa459f8a1e` |
| `crates/sampler-ksp/src/builtins.rs:214–216` | `314bd26e762252837b209753a3ff10be5910eaef` | `bf365bd2a06a60112407212c04f722386e10d344e49123b84d210b5946c47de3` |
| `crates/sampler-kontakt/src/load.rs:726–803` | `cefa61bc0f138d27e437722c5de21f588929195c` | `8cab2c801a3cda8c6708d7de2e7e9fd980e59b815f86183a5953c57028ec658b` |
| `crates/sampler-kontakt/src/load.rs:817–898` | `cefa61bc0f138d27e437722c5de21f588929195c` | `3ba673769423cc808b28656c2c59383cd2024e870f0fe326cd58f92de7b192fd` |
| `crates/sampler-kontakt/src/load.rs:1000–1015` | `cefa61bc0f138d27e437722c5de21f588929195c` | `7ddcc208ae58d1e1dc2fbe7ec46209130dda343f5cdffb680a0f1ae690fe0645` |
| `crates/sampler-core/src/lib.rs:425–444` | `9eddeebae4271a1560e4f0d6dc768092a09cdc62` | `03a43cf8a7875f8736374247ba53e1c3971927c1970d218d6bf8e981c6547611` |
| `crates/sampler-core/src/lib.rs:911–927` | `9eddeebae4271a1560e4f0d6dc768092a09cdc62` | `14b81191a8f42f91ca7da84166239004729eebecdc1167f2818e4c6f9b17ecd8` |
| `crates/sampler-ksp/tests/strings.rs:1–100` | `45bc514253ca5cedff12ea01b3f8341a4e6eb26c` | `5e6ea8630935aad80f728c26f7f748d92d92ed6bd685fd29a3149695d531e616` |
| `crates/sampler-ksp/tests/strings.rs:742–818` | `45bc514253ca5cedff12ea01b3f8341a4e6eb26c` | `5b6b2b266bf76bf7bc6407120c17b46b24d83790ec12a92b3e3cf0ed50886369` |
| `crates/sampler-ksp/tests/strings.rs:788–799` | `45bc514253ca5cedff12ea01b3f8341a4e6eb26c` | `f650e27648b40722d51091fc35e16bfa2beff569de3f24ccb049340fa24e11a4` |
| `crates/sampler-ksp/tests/compile.rs:1–22` | `08009ef41c2efc16857ab4088f4593b34c86d9ca` | `2e941245d5532831f32cf58d283d87ff5583c108f907c1d7b56361f84ca42c23` |
| `crates/sampler-ksp/tests/compile.rs:1266–1403` | `08009ef41c2efc16857ab4088f4593b34c86d9ca` | `c9514ddb8d34f190a322e885fdef3b860b57afa778b9929bc3b9024af7c61de2` |
| `crates/sampler-ksp/tests/functions.rs:1–80` | `21a7dac1481ec7c3da9fdb9d872e82ec41808d49` | `732c78cd74a8de6970a640a68dc5512f0cad98026995cfe1bf5c3ed2067efa18` |
| `crates/sampler-ksp/tests/functions.rs:147–188` | `21a7dac1481ec7c3da9fdb9d872e82ec41808d49` | `12582ea0285bdb1829d3d8648844d9d524bebeb34bdf012c0d23531428e39ee8` |
| `crates/sampler-ksp/tests/functions.rs:262–536` | `21a7dac1481ec7c3da9fdb9d872e82ec41808d49` | `50f6d51d5e0bd7ef941b50c3a1a0ca26cf4ec18e9d1f23fa9a1ca970fe5ae129` |
| `crates/sampler-ksp/src/lower_display_ext_tests.rs:1–174` | `7b0c92419f00b3810c3f29817033789b7494b7c4` | `8f86c187c26195d57400f95c5b357c2b786cda63ee14da0c94893ebe194aa46a` |
| `crates/sampler-ksp/Cargo.toml:1–21` | `49ede8d2afbe27953c0b10a3df7731fcd96ce88a` | `fa585189adb406695c2712f708e4550b0a4e1e205f4e62c40a97ab8acf6aa763` |

### Historical independently verified spans

Original/base commit pins below are custody evidence, not successor test results. All complete historical spans/full-file digests are retained in the JSON, with the committed source manifest as the full-text source.

| Commit | Path:inclusive lines | Git blob | Span SHA256 |
|---|---|---|---|
| `23683f0032a8cb2d29239919d24e931c601c1309` | `crates/sampler-ksp/src/lower.rs:180–234` | `d7e9f4ec95dec39d3c275234811e369390d840c9` | `b693865da54bc7244636ed5fd997bf0bdc46b1efc9f4c5d04ac1f93d51bd3578` |
| `23683f0032a8cb2d29239919d24e931c601c1309` | `crates/sampler-ksp/src/lower.rs:378–402` | `d7e9f4ec95dec39d3c275234811e369390d840c9` | `357071a181dfd5dd1eb6fed949fa996c8b6c14fe41204a2ef20d6b27a3933976` |
| `23683f0032a8cb2d29239919d24e931c601c1309` | `crates/sampler-ksp/src/lower.rs:480–506` | `d7e9f4ec95dec39d3c275234811e369390d840c9` | `40f2506d7732287889e5f30fee8b431727f86f02ef1d7111694ee3460760be60` |
| `23683f0032a8cb2d29239919d24e931c601c1309` | `crates/sampler-ksp/src/lower.rs:1299–1328` | `d7e9f4ec95dec39d3c275234811e369390d840c9` | `ce483146061fd03fb6c7bb9d0e67e42eb38532b9296129dbeb14e29ca406e26f` |
| `23683f0032a8cb2d29239919d24e931c601c1309` | `crates/sampler-ksp/src/lower.rs:1570–1602` | `d7e9f4ec95dec39d3c275234811e369390d840c9` | `be1edd33b041c8745474ee829789746f292944555430e903b3983d1f1734fc6d` |
| `23683f0032a8cb2d29239919d24e931c601c1309` | `crates/sampler-ksp/src/lower.rs:1713–1767` | `d7e9f4ec95dec39d3c275234811e369390d840c9` | `5259bea9e00e030d5c78e95fc1fa175afe09b1d2ad3159687a455404c7c88504` |
| `23683f0032a8cb2d29239919d24e931c601c1309` | `crates/sampler-ksp/src/lower.rs:2360–2379` | `d7e9f4ec95dec39d3c275234811e369390d840c9` | `ebdd67c9a8ec4b837d4a418ca729b81dbe54410ff5ebeff4463d33d9b4d6ff5c` |
| `23683f0032a8cb2d29239919d24e931c601c1309` | `crates/sampler-ksp/src/lower.rs:3667–3683` | `d7e9f4ec95dec39d3c275234811e369390d840c9` | `924596b6cb07f9ea21806b6c1c29fc08c4d61022c17007874875418beb47919b` |
| `23683f0032a8cb2d29239919d24e931c601c1309` | `crates/sampler-ksp/src/eval.rs:438–450` | `8e5e5a6e1e9e245963426f3be1e8d3f68a2ee9b6` | `744202c99e7cd4dba4a2e960d829e3087e82ad7609c0d1c3b29338147634d260` |
| `23683f0032a8cb2d29239919d24e931c601c1309` | `crates/sampler-core/src/ops.rs:1148–1219` | `5fd265bc6d4992014a070c42d66a119cf3419870` | `5354447837e676b7f1b9dda7adba84e7a461840d51fdab0eca6a45231ac775f1` |
| `23683f0032a8cb2d29239919d24e931c601c1309` | `crates/sampler-core/src/ops.rs:995–1017` | `5fd265bc6d4992014a070c42d66a119cf3419870` | `356047487b5aac7ecbbc63c8cf9954294e15f98757576e35bf44a75815f6ef56` |
| `23683f0032a8cb2d29239919d24e931c601c1309` | `crates/sampler-core/src/ops.rs:1451–1494` | `5fd265bc6d4992014a070c42d66a119cf3419870` | `30a79403172d4a85f6d698faf61d862872d9ab5a2fb29452e924168b05227b96` |
| `23683f0032a8cb2d29239919d24e931c601c1309` | `crates/sampler-core/src/behavior.rs:1527–1576` | `a1ff442ba50002d089859623cc2524ad818c48c5` | `673a29affe237f59ea5a57df11004a69d2e4f8a86056b265679d266d01d41d30` |
| `23683f0032a8cb2d29239919d24e931c601c1309` | `crates/sampler-core/src/behavior.rs:1808–1838` | `a1ff442ba50002d089859623cc2524ad818c48c5` | `4a84d54790177a0c2c568dad99f9e54ea50a2477a0564a1ad7d636506b197da1` |
| `23683f0032a8cb2d29239919d24e931c601c1309` | `crates/sampler-core/src/behavior.rs:1968–1985` | `a1ff442ba50002d089859623cc2524ad818c48c5` | `5e95be3f2bdc1bfa4ec46600a8389c554eab90f0d168332dadee8282b9df0fb0` |
| `23683f0032a8cb2d29239919d24e931c601c1309` | `crates/sampler-ksp/src/lib.rs:1047–1111` | `a5e57862e820c58880212998c22f6b6eee8f79dd` | `6cbe928936c75dafd4c26b7b06769d8d0cf5a374083388436e42f50b4e57b183` |
| `23683f0032a8cb2d29239919d24e931c601c1309` | `crates/sampler-ksp/src/lib.rs:1128–1167` | `a5e57862e820c58880212998c22f6b6eee8f79dd` | `730a79475d925407dd823595827860ffd012593482a5a8258f70b2f5f47c4910` |
| `23683f0032a8cb2d29239919d24e931c601c1309` | `crates/sampler-ksp/src/lib.rs:649–713` | `a5e57862e820c58880212998c22f6b6eee8f79dd` | `578be348b37b6ad83b1afc8ab4eef93bd2cfc6d7137bac963051590e573258db` |
| `23683f0032a8cb2d29239919d24e931c601c1309` | `crates/sampler-ksp/src/lib.rs:1255–1314` | `a5e57862e820c58880212998c22f6b6eee8f79dd` | `5ccfe03b86c1c87c6a840dbbb771609800bf84ccaad0c719165334ee0c0a996f` |
| `23683f0032a8cb2d29239919d24e931c601c1309` | `crates/sampler-ksp/src/lower.rs:595–602` | `d7e9f4ec95dec39d3c275234811e369390d840c9` | `6546f5fb950a7d0ebb4ddfc6f7d054c510a36d87dbd9f755106c85b416840f26` |
| `23683f0032a8cb2d29239919d24e931c601c1309` | `crates/sampler-ksp/src/lower.rs:4070–4071` | `d7e9f4ec95dec39d3c275234811e369390d840c9` | `bc9502479427a7a28e19994d6ce19620876baaa694aa1f3d474c0338dd5d0299` |
| `23683f0032a8cb2d29239919d24e931c601c1309` | `crates/sampler-ksp/src/lib.rs:800–841` | `a5e57862e820c58880212998c22f6b6eee8f79dd` | `9a997e40821d3b84a78d2b1f4727739df074b90b190cdff279f1fe0425490cbc` |
| `23683f0032a8cb2d29239919d24e931c601c1309` | `crates/sampler-core/src/engine_parameters.rs:815–851` | `bd48614da4b9b437871ebd894016f42861d12bd2` | `0dac3be2b9503df2287f63f53297159f320628e9582518c8768b158672ff2842` |
| `23683f0032a8cb2d29239919d24e931c601c1309` | `crates/sampler-ksp/src/eval.rs:1674–1674` | `8e5e5a6e1e9e245963426f3be1e8d3f68a2ee9b6` | `28cc43a81e879846be189523cffc1d3a5c8501a353626f35feb81a8e2e6f5bc8` |
| `23683f0032a8cb2d29239919d24e931c601c1309` | `crates/sampler-ksp/src/eval.rs:244–335` | `8e5e5a6e1e9e245963426f3be1e8d3f68a2ee9b6` | `0cccd7c8843a95a5e1d266a1c94260b8b45964a5b833dcf9996d5253b1cb0a1f` |
| `23683f0032a8cb2d29239919d24e931c601c1309` | `crates/sampler-ksp/src/eval.rs:1603–1625` | `8e5e5a6e1e9e245963426f3be1e8d3f68a2ee9b6` | `68f93b3358a87796c417543e60d8cec6de07714f20994205b5e660d5098419b3` |
| `23683f0032a8cb2d29239919d24e931c601c1309` | `crates/sampler-ksp/src/lower.rs:516–554` | `d7e9f4ec95dec39d3c275234811e369390d840c9` | `7631547c46bffa75f897b02a746e754130c68523db959db46f69c830050f47af` |
| `23683f0032a8cb2d29239919d24e931c601c1309` | `crates/sampler-ksp/src/lower.rs:1366–1383` | `d7e9f4ec95dec39d3c275234811e369390d840c9` | `b414f989d22b704d00d59f107e1bdf1e7f6cd49447eb4de710d1417ef97a93cc` |
| `23683f0032a8cb2d29239919d24e931c601c1309` | `crates/sampler-core/src/ops.rs:367–431` | `5fd265bc6d4992014a070c42d66a119cf3419870` | `a420f2fb0899837aec1c291e2a8892a957fa6af62b746bcb9a2a9bfa8fb79cce` |
| `23683f0032a8cb2d29239919d24e931c601c1309` | `crates/sampler-core/src/ops.rs:951–993` | `5fd265bc6d4992014a070c42d66a119cf3419870` | `4408c68be85d425c11ae61d104b23d17ea855eb4559307f802b8eb3d86160147` |
| `23683f0032a8cb2d29239919d24e931c601c1309` | `crates/sampler-core/src/behavior.rs:636–736` | `a1ff442ba50002d089859623cc2524ad818c48c5` | `28344250145c65f5508d49051a13db7b02a39c541f380237b36b2efa4a24135c` |
| `23683f0032a8cb2d29239919d24e931c601c1309` | `crates/sampler-kontakt/src/load.rs:726–772` | `cefa61bc0f138d27e437722c5de21f588929195c` | `5de89172beecef4154861181a4e6c03c3d32af7682e14bdc66c6a2f1dbb09f70` |
| `23683f0032a8cb2d29239919d24e931c601c1309` | `crates/sampler-kontakt/src/load.rs:835–876` | `cefa61bc0f138d27e437722c5de21f588929195c` | `0d8ecf8ac911743193ed92819a355dc9a8b842752d162c3cb5970983ca7a699f` |
| `23683f0032a8cb2d29239919d24e931c601c1309` | `crates/sampler-kontakt/src/load.rs:1000–1015` | `cefa61bc0f138d27e437722c5de21f588929195c` | `7ddcc208ae58d1e1dc2fbe7ec46209130dda343f5cdffb680a0f1ae690fe0645` |
| `23683f0032a8cb2d29239919d24e931c601c1309` | `crates/sampler-core/tests/support/mod.rs:1–60` | `556402ece1f85fba69f811b509431c51b9909b37` | `717328c4e7cb60d17e3fc06a49287c42e112b6513115a25a4004cd3a9c9248a1` |
| `23683f0032a8cb2d29239919d24e931c601c1309` | `crates/sampler-ksp/Cargo.toml:1–21` | `49ede8d2afbe27953c0b10a3df7731fcd96ce88a` | `fa585189adb406695c2712f708e4550b0a4e1e205f4e62c40a97ab8acf6aa763` |
| `13c044d6f9f80576507521a2dc6521e9aad7d9c5` | `crates/sampler-ksp/src/lower.rs:1727–1749` | `799d9dcf722c405fe9f6763f4c3e4b16107512e3` | `8a26b1b0b3576b8556bb3b307da0ccdc90bf5e4e934248bc0c55183d04ad25df` |
| `13c044d6f9f80576507521a2dc6521e9aad7d9c5` | `crates/sampler-ksp/src/lower.rs:1296–1319` | `799d9dcf722c405fe9f6763f4c3e4b16107512e3` | `df50748f38787702f416397deda395fac8c0151a8b2ac06995bd5e5e39a92af3` |
| `13c044d6f9f80576507521a2dc6521e9aad7d9c5` | `crates/sampler-ksp/src/lower.rs:188–227` | `799d9dcf722c405fe9f6763f4c3e4b16107512e3` | `e0aa3e1541c487a492a058ccc112a5a9ce0a1577a9968fb34815c49a0b82879c` |
| `13c044d6f9f80576507521a2dc6521e9aad7d9c5` | `crates/sampler-ksp/src/lower.rs:1566–1585` | `799d9dcf722c405fe9f6763f4c3e4b16107512e3` | `6db487947155c17e1918c0139cf3e0f95a8d76a36bd586236b96b3b97fd1cb42` |
| `13c044d6f9f80576507521a2dc6521e9aad7d9c5` | `crates/sampler-ksp/src/eval.rs:438–449` | `aa2cdfde48f3a5c064deabae70a130bad601c719` | `4e6355b5f0836da099e5d2abfb637f3a4c103d3574b678e6960f7422052b0234` |
| `23683f0032a8cb2d29239919d24e931c601c1309` | `crates/sampler-ksp/tests/compile.rs:1301–1356` | `08009ef41c2efc16857ab4088f4593b34c86d9ca` | `1f4ba0a45278b8ce40e3841c91dfefcb082aca9562aa7791aac38f836d6210b2` |
| `23683f0032a8cb2d29239919d24e931c601c1309` | `crates/sampler-ksp/tests/compile.rs:1358–1403` | `08009ef41c2efc16857ab4088f4593b34c86d9ca` | `e5328067d0dc96f49aa1d8c62f0b0d9fc739e7cf8fe759776f1ea1fa691801f9` |
| `23683f0032a8cb2d29239919d24e931c601c1309` | `crates/sampler-ksp/tests/functions.rs:263–290` | `21a7dac1481ec7c3da9fdb9d872e82ec41808d49` | `6e9d42364eb806880ec0f405f8412fac9479bc4b99fe4e13cd7f6495be211849` |
| `23683f0032a8cb2d29239919d24e931c601c1309` | `crates/sampler-ksp/tests/functions.rs:292–327` | `21a7dac1481ec7c3da9fdb9d872e82ec41808d49` | `fa22d08e22781918ea8c5fe7cabc2a9010ed81272d3e7439e6fe25c146252448` |
| `23683f0032a8cb2d29239919d24e931c601c1309` | `crates/sampler-ksp/tests/functions.rs:355–401` | `21a7dac1481ec7c3da9fdb9d872e82ec41808d49` | `89fddc2d069db853f6cc8103f430aa85422fd2403c7f5246e50c261aaaf37691` |
| `23683f0032a8cb2d29239919d24e931c601c1309` | `crates/sampler-ksp/tests/functions.rs:403–446` | `21a7dac1481ec7c3da9fdb9d872e82ec41808d49` | `5378f2cdd82550a0f1b4213a6cf5906a1f91004832d18a39627564598dbb3417` |
| `23683f0032a8cb2d29239919d24e931c601c1309` | `crates/sampler-ksp/tests/functions.rs:448–470` | `21a7dac1481ec7c3da9fdb9d872e82ec41808d49` | `86216a15129171ac89204f81f402278c8379b4cf3153183030515641de060c79` |
| `23683f0032a8cb2d29239919d24e931c601c1309` | `crates/sampler-ksp/tests/functions.rs:472–523` | `21a7dac1481ec7c3da9fdb9d872e82ec41808d49` | `ac29485fdf09e19dfe91439c12e97713db22fe72ca0ce5aef5856d80652cc3ca` |
| `23683f0032a8cb2d29239919d24e931c601c1309` | `crates/sampler-ksp/tests/functions.rs:525–536` | `21a7dac1481ec7c3da9fdb9d872e82ec41808d49` | `598268ec41d640f358ff0cac362cb32b1c3f5f708636f438f710b8ef7858dcc1` |
| `23683f0032a8cb2d29239919d24e931c601c1309` | `crates/sampler-ksp/src/lower_display_ext_tests.rs:96–138` | `7b0c92419f00b3810c3f29817033789b7494b7c4` | `ac4329b54a958bf440115463dba515ad9d3b4e8fb645826e2f5b403fc6acbdf2` |
| `23683f0032a8cb2d29239919d24e931c601c1309` | `crates/sampler-ksp/src/lower_display_ext_tests.rs:140–163` | `7b0c92419f00b3810c3f29817033789b7494b7c4` | `3a6f2dcff3ee370ade49f0eada0ba4ff22a629d1799c4bd554aec4fafcd5d4d8` |
| `23683f0032a8cb2d29239919d24e931c601c1309` | `crates/sampler-ksp/src/lower_display_ext_tests.rs:165–174` | `7b0c92419f00b3810c3f29817033789b7494b7c4` | `b48190664dfa5b908e728987f57454c8288533debc84aadc97e3c0f2fe8c7da8` |
| `23683f0032a8cb2d29239919d24e931c601c1309` | `crates/sampler-ksp/tests/strings.rs:742–818` | `75188d92aef2e1663d95ecb41fc0f50f80492778` | `d7c1f4b6d7f5ae25d018b96b404713df698494c33d87880fa33efeaec43c1bf9` |
| `23683f0032a8cb2d29239919d24e931c601c1309` | `crates/sampler-ksp/tests/functions.rs:147–188` | `21a7dac1481ec7c3da9fdb9d872e82ec41808d49` | `12582ea0285bdb1829d3d8648844d9d524bebeb34bdf012c0d23531428e39ee8` |

## Prohibitions observed and stop

Only read-only source/Git/search/hash/cached DOM inspection, a fresh read-only archive snapshot and these two new handoffs were used. Python custody/hash checks did not execute Rust, KSP bytecode or the VM. **ZERO** Cargo/rustc/rustfmt/clippy/Rust parser/build/test binaries/native/host/REA/import/official-reader/proprietary fixture; source/config/environment/target-wrapper changes; auth/build requests; current-batch append; nested/new tasks; scheduler/self-loop/server/install/publication/version/push/PR.

Review complete. **Stop after durable handoff. Ownership returns to root.**
