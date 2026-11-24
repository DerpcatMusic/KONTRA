---
status: in-progress
project: plugins/kontra
owner: claude-opus-5-5
created: 2026-11-24
updated: 2026-11-24
proof_scope: internal
evidence_freshness_days: 180
---

# Phase 3: one effect encoding

## Problem & Goal

- **Problem:** 13-IR-VNEXT Phase 3 said "`EffectFamily`/`EffectSpec` are
  read only by the `.kontra` schema; the engine still runs the 31-variant
  `EffectKind`". Measured on e0436491e, that is wrong twice over. Nothing
  reads them: the `.kontra` schema has its own `GraphEffectSpec`, and the
  25 references in `crates/` all sit in `model/ir/src/generator.rs` and its
  unit test. And the step's premise, folding kinds into families, was
  measured by the null report (#155,
  [effect-family-null-report.md](../architecture-v3/effect-family-null-report.md)):
  every kind pair is a distinct recurrence except degenerate settings
  (`Comb(fb = 0)`, unit-delta `Convolution`, the utility matrices already
  unified in `matrix::utility_matrix`). `Strip` is the one conditional fold,
  and it cannot split: it is one Kontakt slot (Supercharger GT), and two
  slots would shift every later KSP slot index.
- **Goal:** one effect encoding. Delete the unread taxonomy (`EffectFamily`,
  `EffectKind::family`, `EffectSpec`) with a committed, re-runnable codemod,
  and add a ratchet so a parallel taxonomy cannot come back. The families
  stay as documentation of the measurement, not as a second type.
- **Plan Reference:** [13-IR-VNEXT.md](../architecture-v3/13-IR-VNEXT.md),
  row "3: graph, generators, effect families".

## Primary-Source & Version Proof

N/A — internal refactor: deletes unread IR types; no external API, format
or protocol is touched (the `.kontra` schema uses `GraphEffectSpec`, which is
unchanged).

## Pinned Crate & SDK Ground Truth

| Dependency / Tool | Pinned Version | Lockfile / Header Source | Verified Symbols / Features |
|---|---|---|---|
| `@ast-grep/cli` | 0.51.3 | `bunx @ast-grep/cli --version` | `scan -r <rule.yml> -U`, `fix.expandStart` (takes attributes, not `///` lines: the driver drops the orphaned doc runs) |

## Invariants & Non-Goals

### Hard Invariants
- **Audio RT-Safety:** no audio-thread code changes; the deleted items were
  load-time types with no readers.
- **Determinism & Compatibility:** `.kontra` files keep their schema
  (`GraphEffectSpec` untouched); every render is unchanged because no code
  path read the deleted types.
- **Mirror Boundaries:** the codemod lives in `plugins/kontra/tools/migrations`
  and names only paths inside the mirror.
- **Additional Domain Invariants:**
  - Each distinct DSP recurrence stays a typed `EffectKind` variant or mode
    (VISION, ENGINE.md KISS caveat).

### Non-Goals
- Merging any `EffectKind` variants. The null report forbids it except for
  degenerate settings, where the benefit is zero.
- Interning rack slots as `EffectId` (blueprint §6.3). The engine mutates
  each group's rack at runtime (`param_addr.rs`, `params.rs`), so plan groups
  keep their own copies whatever the IR does; `Effect` is 232 B because of
  `Strip` (216 B payload, `Delay` 176 B next), so boxing it saves ~40 B per
  slot. Not worth a PR.
- Running every instrument through `AudioGraph` instead of the per-group
  racks. That is the real structural leftover of Phase 3 and needs its own
  spec with A/B renders.

## API / Data / File Contract

- **Removed:** `kontra_ir::EffectFamily`, `kontra_ir::EffectSpec`,
  `EffectKind::family()`. No replacement; nothing called them.
- **Files:** none. **CLI:** `tools/migrations/effect-family-subtract.sh`
  (idempotent; prints references in `crates/` before → after).

## Verification Matrix

| Check | Tier | Target / OS | Exact Command | Pass Criterion | Status / Observed Evidence |
|---|---|---|---|---|---|
| Spec proof gate | Doc | any | `python3 tools/check-spec-proof.py plugins/kontra/docs/specs/effect-family-subtract.md` | Exit 0 | PASS (2026-11-24, linux-x86_64, da070c7ad) |
| Codemod idempotent | Doc | linux-x86_64 | `plugins/kontra/tools/migrations/effect-family-subtract.sh` (twice) | `25 -> 0`, then `0 -> 0` | PASS (2026-11-24, linux-x86_64, da070c7ad) |
| Ratchet catches a regression | Compile/ABI | linux-x86_64 | append `pub enum EffectFamily { A }` to `generator.rs`; `build-slot cargo test -p kontra-engine --test arch_guards architecture_metric_ratchets_hold` | test fails naming the file | PASS (2026-11-24, linux-x86_64, da070c7ad) |
| Arch guards | Compile/ABI | linux-x86_64 | `build-slot cargo test -p kontra-engine --test arch_guards` | Exit 0 | PASS (2026-11-24, linux-x86_64, da070c7ad) |
| IR tests | Compile/ABI | linux-x86_64 | `build-slot cargo test -p kontra-ir` | Exit 0 | PASS (2026-11-24, linux-x86_64, da070c7ad) |
| Null harness still runs | Validator/Runtime | linux-x86_64 | `build-slot cargo test -p kontra-effects --test effect_family_null` | Exit 0 | PASS (2026-11-24, linux-x86_64, da070c7ad) |
| Fast unit/lib tests | Compile/ABI | linux-x86_64 | `matari fast kontra` | Exit 0 | PASS (2026-11-24, linux-x86_64, da070c7ad) |
| Real-library load | Validator/Runtime | linux-x86_64 | `build-slot cargo test -p kontra-host --test instrument` (Areia 16 Violins) | Exit 0, Areia test ran (not skipped) | PASS (2026-11-24, linux-x86_64, da070c7ad) |

## Unverified Boundaries & Promotion Gate

None — all verification tiers execute on the current host.

**Hard Promotion Rules:**
1. This spec **MUST NOT** move to `status: implemented` while any required row in `## Verification Matrix` is `PENDING`, `PENDING_OS_CI`, or `BLOCKED`, or while any unchecked `- [ ]` acceptance criterion remains.
2. An unverified OS backend or platform path **MUST NOT** be flipped to default until its `PENDING_OS_CI` row becomes `PASS` on real OS CI or hardware.

## Acceptance criteria

- [x] `git grep -cE '\b(EffectFamily|EffectSpec)\b|\.family\(\)' -- plugins/kontra/crates` is 0.
- [x] `architecture_metric_ratchets_hold` fails when either type is redefined.
- [x] 13-IR-VNEXT's Phase 3 row names `AudioGraph` vs racks as what is left.

## Task Decomposition

| # | Subtask | Owned Paths | Depends On | Verification |
|---|---|---|---|---|
| 1 | Codemod and run | `plugins/kontra/tools/migrations/effect-family-subtract.*`, `crates/model/ir/src/{generator,lib}.rs` | — | codemod output |
| 2 | Ratchet | `crates/engine/engine/tests/arch_guards.rs` | 1 | arch guards |
| 3 | Plan row | `docs/architecture-v3/13-IR-VNEXT.md` | 1 | doc review |

## Rollout & Affected Dependents

- **Affected Projects:** `plugins/kontra` only (`matari affected origin/main`).
- **Mirror Impact:** subtree mirror (`kontra`); no private paths.
- **Versioning & Changelog:** no user-facing change; no bump.
