# Native loader and script-service takeover

Coordinator: `916e49fe-045f-4c95-b109-6a4d7f6415ff`.
Status: SOURCE COMMITTED. Integration validation is pending.
Scope: native UVI/Falcon/Kontakt readers, providers, assets, and script services.
DSP kernels and shared UI rendering remain outside this takeover.

## Safety and original requirements

- Use owned native code to open libraries on Windows and Linux.
- Do not require UVI Workstation, an official reader process, or Wine.
- Do not run third-party openers for validation.
- Do not install, restore, or replace any local plugin or application.
- Do not restart archived workers, create subagents, or enable schedules.
- Do not set or export `CARGO_TARGET_DIR` or `RUSTC_WRAPPER`.
- Only the parent's integration worker may run compilation or tests.
- Do not interpret older reader permissions or install requests as current authorization.

The recovered W10 initial brief permitted read-only official-reader access.
Later no-third-party directives explicitly superseded that permission.
The latest takeover brief prohibits all local installs.
The recovered W0 directive identifies the native provider as already integrated.
Neither directive authorizes repeating the old release or installation jobs.

## Durable recovery

Read the W10 and W5 durable messages, including later continuation directives.
Read the relevant old coordinator messages.
Queried both workers' held queues. Both queries returned no held messages.
Neither archived worker received a message from this takeover.

Evidence files in the coordinator's scratch `handoff/` directory:

- `native-uvi-messages.json` and `native-uvi-messages-first.json`
- `native-ksp-messages.json` and `native-ksp-messages-first.json`
- `native-coordinator-latest.json` and `native-coordinator-last.json`

Worker thread identifiers:

- W10: `thread:delegated-task:command%3Amcp%3A353f3f16-ae7e-4a0a-a81d-47e0d2747c72%3Adelegate-task%3Aphase2-w10-uvi`
- W5: `thread:delegated-task:command%3Amcp%3A353f3f16-ae7e-4a0a-a81d-47e0d2747c72%3Adelegate-task%3Aphase2-w5-ksp-service`

Last W10 report: message position 3572.
It reports a clean tree, drained jobs, and geometry/order READY work.
It reports 37 UVI checks, 10 shared-IR checks, three renderer checks, and both no-run gates.
These are historical receipts, not takeover validation.
Its next items are callback modifier metadata and in-place children-table publication.
Bank/native fidelity and the callback deadline root cause remain open.
Its 26-bank/660-program native acceptance cells remain UNKNOWN/PARKED.

Last W5 report: message position 3133.
It approves the narrow lowerer arm in W6 commit `448cfaa59`.
It does not certify shared64/native32 host timing.
Its next item is the `_pgs_changed` alias.
Earlier W5 report `23c9c59d` records completed global UI callback dispatch.
That report claims five callback contracts and 65 selected tests passed.
Those results also remain historical receipts.

## Worktree preservation and ownership

Repository: `/mnt/Windows11/DEV_PROJECTS/Repos/KONTAKTO`.

The main checkout contains extensive staged, modified, and untracked work.
This takeover did not change, reset, stash, clean, or stage any main-checkout file.

Recovered W10 checkout:
`/home/derpcat/.t3/worktrees/KONTAKTO/fix-uvi-scan-344`.
Branch: `v2/fix-uvi-scan-344`.
Recovered clean HEAD: `6df342507c67c24a3c3de94e7881cb4bf3fc2315`.
This takeover leaves that checkout unchanged.

Recovered W5 checkout:
`/home/derpcat/.t3/worktrees/KONTAKTO/fix-ksp-service`.
Recovered clean HEAD: `23c9c59dff109fea84b76a1a3cd15f8b684d2112`.
Exclusive takeover branch: `takeover/native-ksp-20261010`.
The prior W5 branch reference remains at its recovered HEAD.
Only this takeover branch contains the new alias changes.

Neither selected worker checkout contains `graft/`.
No graph query was available there.
Read the exact resolver, parser, lowering, shared-callback caller, and test-helper spans before editing.

## Existing work and integration dependencies

At inspection, local `integrate/core-v2` was `5498e631`.
Do not assume that stale local reference represents the parent's current integration checkout.

The raw native-provider commit `8f4c3c7c` is not an ancestor of that local reference.
Release history uses squashed/cherry-picked integration.
Native provider implementation is already present in the actual access source.
Access history includes `27fdb0e1`, the native Falcon opening release.
Do not replay the provider commit merely because its original SHA fails an ancestry check.

Unintegrated W5 global-callback sequence, according to the inspected local reference:

1. `d293ade0`: callback contract tests.
2. `2ea4dc1c`: global dispatch product change.
3. `23c9c59d`: historical receipt.
4. Prerequisite: prior evaluated-setter/RMW slice `4c9b0e62`.

Unintegrated W10 final projection sequence, according to the inspected local reference:

1. Shared geometry/order fields: `67fed2df`.
2. Shared validation/traversal: `e320712b`.
3. Shared renderer/producer adaptation: `1f940ea1`.
4. Remaining integer-producer adaptations and normalization guards: `40028b11`, `0a69d042`, `9ef8ba4a`.
5. Sparse cumulative publication: `36e6cc25`.
6. UVI projection: `99dfa1e1eae28c099cbc7227fcef0db430704da8`.
7. Projection regression tests: `6fc20f22`, `9d864f11`.
8. Invalid geometry admission: `c1241d0a68163ab1b5982718af0e1229c9a4cdf0`.
9. Historical receipt: `6df34250`.

The parent must reconcile these dependencies with the shared-UI takeover.
Do not integrate only the UVI projection against integer shared geometry.
Do not merge either worker's entire historical branch.
Earlier W10 module/class/constructor/unit/value slices require separate receipt and ancestry reconciliation.

## New implementation

Test-only commit:
`2288c42aa61918732ecad43d0dc8bb47d168af33`.

Product commit:
`c7e43d27fc1d8bc0c9c36a401d484cf336ca49fd`.

Legacy evidence: `0cb7a8a0:src/ksp/compile.rs:385` accepts
`"pgs_changed" | "_pgs_changed"`.

The native semantic resolver previously accepted only `pgs_changed`.
The product change adds the alias to the same existing `CallbackKind::PgsChanged` arm.
It requires no callback argument.
Existing lowering maps that kind to the existing PGS_CHANGED callback ABI.
Existing duplicate detection rejects both spellings as the same callback.
No runtime, loader, DSP, renderer, or bytecode-layout change was necessary.

Changed files:

- `crates/sampler-ksp/src/sema.rs`
- `crates/sampler-ksp/tests/compile.rs`
- `crates/sampler-ksp/tests/params.rs`

New coverage:

- A writer script changes shared PGS state.
- A different slot's underscored callback observes the write and produces its authored note.
- The existing canonical callback retains the same runtime contract.
- Canonical/alias duplicates fail in both orders.
- Repeated canonical and repeated alias callbacks fail.
- An alias with a UI-control argument fails.
- Duplicate and unsupported-argument diagnostics retain source-line checks.

Static check: `git diff --check 23c9c59d..c7e43d27` passed.
No compilation, test, or clippy command ran in this takeover.
No GREEN or release-readiness claim applies to these new commits yet.

## Integration validation request

Use the parent's serialized build transport.
Do not use the obsolete wrapper that exports target-directory variables.

For RED, use the test-only commit `2288c42a`:

```sh
cargo test -p sampler-ksp --test params pgs_writes_reach_every_slot_and_run_underscored_pgs_changed -- --exact
cargo test -p sampler-ksp --test compile pgs_callback_alias_and_canonical_name_share_duplicate_detection -- --exact
```

Expected RED causes:

- Runtime alias test: semantic resolver reports unsupported callback.
- Duplicate test: resolver reports unsupported callback instead of duplicate callback.

For GREEN, use the product commit `c7e43d27`:

```sh
cargo test -p sampler-ksp --test params pgs_writes
cargo test -p sampler-ksp --test compile pgs_callback_alias
cargo test -p sampler-ksp --test compile --test params --test ui_callbacks
cargo test -p sampler-core -p sampler-ksp -p sampler-kontakt --no-run
```

Run the root/plugin area gate if the integration checkout permits it.
Record the exact revision, commands, exit codes, and logs.
Report failures without changing unrelated DSP or UI code.

For integration, cherry-pick only the two new commits and this handoff documentation.
Reconcile older W5 prerequisites separately.
The alias itself does not require the global UI dispatcher.

## Remaining risks

- New alias tests and area gates remain unrun.
- Old READY slices are not automatically integrated or revalidated.
- The provider's raw SHA differs from integrated release history.
- Full native bank/program acceptance on Windows and Linux remains incomplete.
- UVI modifier metadata and in-place children-table publication remain unfinished.
- UVI callback deadline root cause remains unresolved.
- W5 dynamic selectors, GET-only enforcement, runtime fonts, and unit text remain open.
- Shared64/native32 timing remains uncertified.
- Main-checkout uncommitted work remains preserved and outside this takeover.

## Round 2 continuation (source only; combined-batch policy)

Current exclusive branch remains takeover/native-ksp-20261010.
Alias source stack is preserved. Parent reports alias RED receipts from the
integration test-only candidate; no local Rust command ran here.

Additional commits:

1. a39b07f5fc9d37a9a06719a7ab6238a1e7e9429c — dynamic integer menu
   NUM_ITEMS/SELECTED_ITEM_IDX callback dispatch and two authored regressions.
   This is independent of the fade runtime dependency and was sent for the
   current combined batch.
2. 9fe764e4dcb3e876f82a1d19e2656cf8fe02ffe6 — typed FadeCurve enum,
   optional Instruction operand, builtin names/arity, lowering, direct test
   caller adaptation, and authored compiler/runtime regressions.
   MUST pair with DSP-owned script_params.rs signature/runtime change before
   any compilation. Event ALL_EVENTS/by_marks admission is unchanged.
3. 27dfb4272544a72aee9442daf057a38c961e14d3 — precise native/script
   evidence report, corrected Luau-vs-Lua5.1.4 manifest comment, and three
   inline Lua compatibility probes. No VM/dependency/runtime replacement.
4. 9da42c031f86e610734db7386e32ad79c4552ed8 — within-cell exponential
   fade witness (frame16 of384; blocks1/17/128; exact/time-mirror tolerance1e-7).
   Pair after the fade seam and DSP runtime. Quarter-grid tests alone cannot
   certify nonlinear behavior within the engine's32-frame gain cells.

No-build checks: git diff --check and rustfmt passed; Python source invariants,
immutable official-document digests, independently evaluated fade fixture values,
and the within-cell endpoint-interpolation discriminant passed.
These checks do not execute Rust. All new Rust regressions remain UNRUN.

Traceable source/docs/native limits:
docs/audit-2026-10-10/native-script-round2.md.
Authoritative Kontakt/UVI documentation was checked at exact hashes.
REA opened Kontakt executable SHAfe12b6a7b652cfd026b5bec62ae6c096b2dfddcc0e17249ba518cb3ef0f0c8e0;
one focused string search timed out330s, was not retried, and close_binary
completed. Supplied fade-name file offsets establish names only.
Documented semantics are checked; native rendered/numerical parity is UNKNOWN.
Unknown fade-selector rejection is our safety policy, not proven native reaction.

All exact SHA/dependency/filter/reference reports were sent to parent, integration,
DSP, and roadmap scout. No archived worker, schedule, or nested task was started.
No compiler, reference process, reader, server, install, publication, or frozen-v1
change occurred. No proprietary script/sample/key dump was created.

NEXT: receive combined-batch menu/alias receipts; pair fade9fe + DSP runtime +
within-cell9da before the next frozen batch; run Lua probes in an integration
batch. Compare actual failures before changing VM semantics. The end-goal of
lower CPU AND RAM than both frozenv1 and Kontakt remains UNACHIEVED/UNMEASURED.
