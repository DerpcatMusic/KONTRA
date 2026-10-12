# Modulation fixture compile successor — source READY, validation pending

Base: `2a243bcfa2262abb789265aa0951fd2a85854023`.
Isolated branch: `pi/modulation-fixture-compile-fix`.
Worktree: `/home/derpcat/.t3/worktrees/KONTAKTO/modulation-fixture-compile-fix`.
No existing worker, root, or frozen integration checkout was edited.

## Diagnosis and scope

Frozen integration6 reported E0308 in `sampler-kontakt:production_modulation`.
`Runtime::edit_controls` returns `Result<u64, Error>` (unchanged
`crates/sampler-core/src/control.rs:479-487`). The included heap guard accepts
`impl FnOnce()` (unchanged `crates/sampler-core/tests/support/mod.rs:42`).
The final `.unwrap()` therefore returns a revision where the callback requires
`()`. A semicolon discards that revision without changing the edit or panic
check. Both calls remain inside the heap guard.

Reserved before edits: production fixture lines 647 and 710, then core fixture
lines 1138, 1143 and 1299 after bounded discovery. New files are only this audit
folder and the scratch handoff. No product source, tolerance, assertion, render
partition, PCM value, serialized bytes, dependencies, or manifest changed.

Ordered code commits:

1. `3839fb9355c9e11dcc214782be406ce5f125e8f7`: two semicolons at the actual
   frozen compiler diagnostic sites.
2. `967ea34cf33f4646d873616688311ab4ac9b1bd5`: three semicolons in identical
   `sampler-core:controls` heap-guard tails discovered by the bounded scan.
   These three are source/signature findings, **not** additional diagnostics
   emitted by the frozen compiler run.

Exact zero-context code delta (five added bytes):

```diff
--- a/crates/sampler-core/tests/controls.rs
+++ b/crates/sampler-core/tests/controls.rs
@@ -1138 +1138 @@
-                .unwrap()
+                .unwrap();
@@ -1143 +1143 @@
-                .unwrap()
+                .unwrap();
@@ -1299 +1299 @@
-            .unwrap()
+            .unwrap();
--- a/crates/sampler-kontakt/tests/production_modulation.rs
+++ b/crates/sampler-kontakt/tests/production_modulation.rs
@@ -647 +647 @@
-        .unwrap()
+        .unwrap();
@@ -710 +710 @@
-            .unwrap()
+            .unwrap();
```

## No-build evidence

`SOURCE-CHECK.json` SHA-256:
`b3a9c17eef9412d15758494fae3c50300ce4c3bb185c650f62eec19e4592ac79`.
The receipt pins code commit `967ea34cf33f4646d873616688311ab4ac9b1bd5`,
original/successor blobs and file digests, caller/helper spans and their digests,
fixture spans, exact patch, all 13 original serialized test names, inherited
requirement manifests, and unchanged frozen receipt digests.

No-build command used (installed parser; no install or new dependency):

```sh
node docs/audit-2026-10-10/modulation-fixture-compile-fix/check_source.mjs \
  /home/derpcat/.t3/worktrees/KONTAKTO/modulation-fixture-compile-fix \
  /home/derpcat/.npm-global/lib/node_modules/@nanonets/graft/node_modules
```

Results: byte-for-byte base reconstruction with only the five semicolons;
`git diff --check` PASS; Tree-sitter Rust AST has no ERROR/missing nodes;
non-unit revision tails become terminated expression statements. Bounded scan
of tracked `crates/**/tests/**/*.rs` containing both `support::without_heap`
and `.edit_controls(` inspected nine files and 20 matching callbacks. No
remaining revision-valued tail was found in that exact pattern. This is not a
general Rust typecheck and does not cover aliases, macro-generated callers,
or other APIs. The initial scan found the three core tails; it did not establish
runtime failures. Graft query/callers preceded the multi-file changes.

Production fixture successor blob: `148e7f0c2822b6ce9db08d273e352be84a303d3a`;
SHA-256 `eaba1eac1741f5e07a9365d1f5eeb651b18140573b81d168aebe3c8eec47d16f`.
Core fixture successor blob: `8ab576120c3aea07276ed88ceb2f85bcc93cb006`;
SHA-256 `4b95c4f8809e8ea4c5159f2a600b8ea6f55cb6ded9ef9f66f3d8d9c2a991635b`.

## Frozen failure remains immutable and RED

Run directory: `/mnt/Windows11/DEV_WORKSPACE/kontra-runs/pi-integration-round6-successor-v2`.

- `FROZEN-SOURCE.json`: `f4c465ff191d581684a56d4fee920ef2c22dbac0bd59077923702fbfbb19cf61`.
- `combined-no-run.json`: `c555adbcf95063157db2ab83c8bb0123a1b765fbb7e7cf5a40fe901a9682531c`.
- `combined-no-run.log`: `6ca7ef6889999b16d2929b9709e405f997dda657f2cf2214948dece53c4c5c0e`.
- `COMPILE-FIRST-RED.json`: `d6a7c51757dd3894593f644ca4381b8d7e9dfabbd629681aa4a3ea7f01993bc3`.
- `COMPILE-FIRST-RED.log`: `d6059778986b2b007a4c70e3e9767868f838200927e453f21e402bc5d8299136`.

All 13 serialized modulation tests are preserved and **NOT_RUN**: the failed
harness did not emit the target. No Rust tests ran in this lane. Root's current
available-test phase continues with aggregate **RED**; this successor does not
append to or repair the frozen batch. Fresh root-owned combined validation is
required. No per-lane build is requested.

Future selection: package `sampler-kontakt`, target `production_modulation`,
all 13 exact names in `SOURCE-CHECK.json` (not only the two affected fixtures).
Additional package `sampler-core`, target `controls`, exact filters:

- `modulation_inputs_follow_identity_after_reorder_append_and_real_default_replacement`
- `schema_replacement_preserves_native_envelope_and_dsp_identity_consumers`

Both additional tests are **NOT_RUN**.

## Inherited specification, not new native semantics

At the exact base, retain `docs/audit-2026-10-10/modulation-control-remap-fix/source-manifest.json`
(blob `6287e83cc3fa4c177f8e087a6fbb76baac86b0e5`, SHA-256
`0bd92f20894284cfb7e31e4f89d003e9616e8bc21650344565f92f8c312e93e7`)
and `docs/audit-2026-10-10/production-modulation-next/source-manifest.json`
(blob `70aa67e6c578e10cf073ee1be7f4fd9a5d8359e3`, SHA-256
`9cdc896d20757a4fe35f96518edf696af6c48f9f9e6ff2162daa5596d21e3fba`).
Their immutable official requirement archives were rehashed read-only:

- Kontakt modulation, AHDSR / LFO controls:
  `ca330e3207a333061ebca4ee28b0545b5e93c223624ff69ca608c7a85d897c9d`.
- KSP engine parameters, Modulation:
  `4a15e3b10a9da8993c90ddc7681fee9fd62a7eafc5f3685f47b0c5ab37d1d16d`.
- KSP engine commands, `get_mod_idx` / `get_target_idx` / `set_engine_par`:
  `df2fe7192f32df7c4a738c713663bafdfec3630c2821fd9afe1887c7d5ed006d`.

Exact archive paths, additional UI requirement digests and section references
remain in the inherited manifests and new receipt. These support the inherited
behavior requirements only; they cannot establish Rust callback return types.
No official reader, native host, Kontakt/Falcon runtime, or protected library
was executed. Native parameter/clock/PCM/gesture parity remains **UNKNOWN**.
The polished-product and lower-CPU-and-RAM goal versus both v1 and Kontakt
remains **UNACHIEVED**. This test-only change provides no performance result.
