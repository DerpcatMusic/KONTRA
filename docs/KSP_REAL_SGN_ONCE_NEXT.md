# KSP real `sgn`: evaluate its operand once

Status: SOURCE_READY for a fresh review and a **following** root-owned combined batch.
Not part of `integration7-source-draft`. All authored Rust tests are
**NOT_RUN_NOT_COMPILED**. Native runtime parity is **UNKNOWN**. The polished
product with CPU **and** RAM below **both** v1 and Kontakt remains **UNACHIEVED**.

## Narrow change

Base: `2a243bcfa2262abb789265aa0951fd2a85854023`, tree
`c9504d7133090d35117048895189f50fdfa8fb6a`.
Only the real `Sgn` branch in `crates/sampler-ksp/src/lower.rs` changes product
code. The old branch evaluated its argument twice and returned
`(first > 0) - (second < 0)`. The new branch evaluates it once and returns
`(x > 0) - (0 > x)`.

The base has no general full-bit register-move instruction. No new move seam is
necessary: the existing `Op::CompareReal` writes only its left-hand register.
After evaluating into `dst`, initialize `t` and `u` to real zero. First compare
`u > dst` into `u`, leaving the full IEEE64 operand in `dst` untouched. Then
compare `dst > t` into `dst` and subtract integer `u`. Both comparisons consume
the same evaluated operand, without a float arithmetic copy, hidden script
cell, text scratch, narrowing conversion, or `Binary32` copy.

The existing `reg(dst, 2)` check remains. Operand evaluation still uses only
`dst` and registers above it; both zero scratch registers are initialized
**after** that evaluation. The branch uses the same three register positions;
removing the evaluation at `dst + 2` cannot increase expression temporary
requirements. This is source reasoning, not a measured program allocation.

Integer `sgn`/`signbit`, initialization, random lowering, RNG seed/algorithm,
other arithmetic, callback admission, fuel machinery and native constants are
unchanged. Greater-than comparisons against zero retain the existing own
NaN and signed-zero result of zero. No vendor numeric-policy claim is added.
No files from another worktree were copied or edited. Waveform dispatch/helper
changes in the current batch are outside this patch's base-line reservation.

## Immutable primary requirement, not native execution

Cached official NI KSP manual, **Basic Operators**:
<https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/arithmetic-commands---operators#basic-operators>.
Retrieved `2026-10-10T03:05:30.511268+00:00`.

> The following operators work on both integers and real numbers.
> sgn(x) Signum function (returns -1 if the number is negative, 0 if it's zero,
> 1 if it's positive).

The immutable cached document `handoff/official-docs/ksp-arithmetic.html` has
SHA256 `3bef630209e32d0a099168af024bded981619209b48908b4cc00743617a3df8a`.
The exact Basic Operators section text has SHA256
`71ea38cee3883030dc52f54b403d463496b9536ce313c428953d3de69352a191`.
The cache's `random()` and `real()` sections support the operand's integer RNG
and explicit conversion shape, not a native draw sequence or seed.

This uses the permitted immutable-primary equivalent evidence route, not a
fresh REA session or native receipt. Prior native/REA limitations are inherited,
not re-tested here. The unversioned manual does not establish native evaluation
order, native seed/distribution/state sharing, NaN policy, callback timing,
error behavior, or comparative performance. Evaluating this operand once is
an own-language correctness fix, consistent with the existing initializer,
not a claim of measured Kontakt parity.

Frozen audit `366515c96b583747d1bd77c881b1f6909e86cf0d` documents the old
branch at `lower.rs:1675–1719`. Its 70-span audit and four old own PASS
invocations are not new test receipts. No frozen source, audit status or
receipt is changed or projected onto this patch.

## Source trace before editing

Graft was queried first in the exclusive worktree. Qualified `Compiler::`
queries did not resolve: this file uses `Gen`. Graft supplied ambiguous-name
caller edges; exact `rg` searches and Git source spans supplied the complete
local use lists. Graft auto-refreshed its ignored graph in this new worktree;
no frozen graph refresh was requested.

The relevant paths are `stmt/assign/arg/value → Gen::builtin → real Sgn`,
`Script::bind → bind_modules → Program::new`, and actual callback
`Instruction::Op → Runtime::op_step → CompareReal`. The core reads IEEE64
registers and writes only the comparison's LHS. `Op::locals` and `Program::new`
account for both registers. `Gen::scratch` is **text** scratch, unrelated to
this numeric change. Every local `scratch`/`copy_i32` use was inspected and
recorded in the handoff; `copy_i32` uses zero plus `Binary32::Add` and is not
safe for real operand bits. No such helper is used or changed here.

The handoff JSON pins the exact commit, whole Git blob, line span, full text
and span SHA256 for the before/after branch, tests, public compile/bind,
real-bit helpers, core comparison/register handling, type registration,
initializer, RNG, runtime callback dispatch and existing heap fixture.

## Authored future regression filters

Crate: `sampler-ksp`. Existing target: `compile`.

- `real_sgn_note_evaluates_operand_once_and_preserves_following_rng_draw`
- `real_sgn_note_reuses_full_operand_bits_and_preserves_comparison_policy`

Both are **NOT_RUN_NOT_COMPILED**. No Rust parser/typechecker, compiler, Cargo,
clippy, test executable or native host was invoked.

The first uses public `sampler_ksp::compile`, `Script::bind`, `Runtime::new`,
`Runtime::seed_random`, `Runtime::trigger`, and actual on-note assignments. It
checks result cells and the **following** RNG draw, callback completion and
retirement using the already included `support::without_heap`. Variants cover
a direct operand, nested sign/conversion in a nonzero destination, and a KSP
function containing a real arithmetic operand. No new allocator is defined.

Pinned own seed: `0x9e3779b97f4a7c15`. Source-derived xorshift draws in `[-1,1]`
begin `[-1,1,0,0]`. For the direct fixture, old source predicts sign `0` and
following draw over `[0,2147483647]` of `7999096` (third draw); new source
predicts sign `-1` and following draw `1912511847` (second draw). These are
**SOURCE_ONLY model predictions**, not executed-script results. The wider
following range prevents two equal small-range draws from hiding consumption.

The second loads a real script variable before nested `sgn`, preventing literal
folding from bypassing the runtime branch. It covers positive/negative finite
values, both zeros, large values, smallest subnormal literals and existing own
unordered comparison behavior via `sqrt(-1.0)`. The existing lexer accepts
finite decimal/exponent literals by source inspection; runtime execution is
still unknown until the sole root batch runs the authored tests.

## Boundaries

No build/test/native/performance execution, protected library, official reader,
Wine, server, scheduler, nested agent, installation or publication work.
No `CARGO_TARGET_DIR` or `RUSTC_WRAPPER` changes. Array equality/search,
initializer RNG and indexed inc/dec remain separate unresolved gates.
Source checks prove patch confinement and model consistency only. They do not
prove Rust compilation, fixture success, no-allocation execution, native parity
or CPU/RAM superiority. Stop source work after handing this source-ready patch
to a fresh reviewer; only the root integration owner selects a future batch.
