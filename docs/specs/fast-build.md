---
status: in-progress
project: plugins/kontra
owner: claude-opus-5-5
created: 2026-11-25
updated: 2026-11-25
proof_scope: internal
evidence_freshness_days: 30
---

# Fast build: KONTRA compile and test time

Cross-cutting plan: `docs/specs/2026-11-24-unified-event-driven-ci.md`, task 4b
("RT pass 2 runs only RT tests; optimised DSP test profile; kontra job ≤ 2 min").
Neighbours, not duplicated here: PR #246 (one warm target per project shared by the CI
runners, `tools/ci_cache.py`) and branch `ci-3min` (test-result cache, slow lane).

## Acceptance criteria

1. A warm incremental `test` build after a one-line change in a leaf crate is under 20 s.
2. The kontra CI test job is at most 2 min warm.
3. rt-paranoid pass 2 runs only the tests that assert real-time behaviour.
4. Every kontra test still passes; none is deleted or ignored to buy speed.

## Non-goals

- The slow lane itself (`ci-3min`); this spec only names its kontra candidates.
- Sharing CI targets across runners (#246).
- Rewriting the slowest tests' bodies (`plugin/src/tests.rs` is under active parity work).

## Method

All numbers are from `nice -n 19 build-slot cargo …` on the shared build host at load
115-155 (the CI runners share it), dsp-dev profile (`opt-level = 1`, kontra crates
`debug = "line-tables-only"`). Wall times at that load vary by up to 2× run to run, so
each decision below rests on an A/B run back to back, and on unit counts, which do not
vary. Raw logs and timings are kept outside the repo.

## What the profile showed

- Cold test build: 5 m 29 s wall, 1,544 s CPU, 708 units. The critical path is the
  `mlua-sys` build script compiling Luau's C++ (243 s, starting at 36 s). The largest
  Rust units are naga (80 s), read-fonts (72 s), kontra-dsp-reverb (72 s), kontra-engine
  (67 s), kontra-format-uvi (63 s lib, 67 s lib test). Cold-only; a warm target keeps all
  of it.
- **moose and its dependents were never fresh.** `moose-loader/build.rs` declared
  `rerun-if-changed=../../rust-toolchain.toml`, a file that exists in the standalone
  moose repo but not under `frameworks/moose` here. Cargo reruns a build script whose
  watched file is missing, so every build recompiled moose-loader, moose, kontra-host,
  kontra-plugin and the three tools.
- **`-p` on one crate rebuilt shared dependencies.** Per-package feature resolution gave
  syn, serde, toml, thiserror and kontra-macros different feature sets than
  `--workspace`: one sfz edit plus `nextest run -p kontra-format-sfz` compiled 15 units.
- **A test wrote into the source tree.** `library_root_requires_one_unambiguous_nicnt`
  made and removed a folder in its cwd, the crate dir. kontra-plugin's build script
  watches `../crates`, so every test run made the next build recompile kontra-plugin and
  kontra-window. In CI that is the job after every job.
- **16 unused intra-workspace dependency edges** (cargo's unused-dependency warnings),
  mostly `kontra-params` on eight DSP crates. They serialise the graph and widen what a
  change rebuilds.
- **Examples are 28% of an incremental test build's CPU.** After a reverb edit, 19
  example binaries took 37 of 131 unit-seconds and run no tests. The lint job's
  `clippy --all-targets` compiles them anyway.
- **rt-paranoid pass 2 reran the whole kontra-plugin suite** (118 s here, 48 s in CI)
  to check one test: `reset_is_realtime_after_prepare`. Every other test only logs
  allocations (`moose::rt` `Count` mode).
- **The suite's wall was its tail.** `auto_align_reports_the_latest_part_and_holds_the_other`
  (22 s CPU, 55-100 s wall under load) started a third of the way in and finished alone.

## Changes

| Change | Where |
|---|---|
| `rerun-if-changed=build.rs` (Cargo already rebuilds on a rustc change) | `frameworks/moose/crates/moose-loader/build.rs` |
| `[resolver] feature-unification = "workspace"` (nightly, tree pins nightly) | `.cargo/config.toml` |
| Drop 16 unused dependency edges | 12 `crates/*/*/Cargo.toml` |
| Test scratch in `temp_dir`, not the crate dir | `crates/format/kontakt/src/resources.rs` |
| `test_targets = ["--lib", "--bins", "--tests"]` for both test passes | `tools/projects.toml`, `tools/matari.py` |
| `rt_paranoid_filter = "test(/realtime/)"` for pass 2 | `tools/projects.toml`, `tools/matari.py` |
| nextest `priority = 100` for the nine tests over ~15 s | `.config/nextest.toml` |
| `slow_tests` names the two ~20 s-CPU tests for the `ci-3min` slow lane | `tools/projects.toml` |

Real-time tests must keep `realtime` in their name to stay in pass 2.

## Results

| Measure | Before | After |
|---|---|---|
| No-op `test --workspace --no-run` | recompiles moose-loader, moose, kontra-host, kontra-plugin, 3 tools | 0 units, 0.9 s |
| sfz edit, `nextest run -p kontra-format-sfz` | 15 units, 32.1 s | 1 unit, 1.6 s |
| reverb (DSP leaf) edit, workspace test build | 18 units, 27.3 s | 16 units, 13-21 s (median 16 s) |
| kontra-ir (core, 30 dependents) edit, workspace test build | n/m | 31 units, 11-26 s (median 19 s) |
| rt-paranoid pass 2 run | whole kontra-plugin suite, 118 s (48 s in CI) | 1 test, 0.02 s |
| Full suite (1,638 tests then, 1,643 after merging main) | 164 s wall at load 134 | 104 s at load 134 (examples skipped, before priorities); 37-73 s with priorities |
| CI test job, warm, nothing changed (`matari.py check origin/main --project kontra --stage test`, second of two runs) | 182 s at load 111: 7 units rebuilt (20 s), suite 88 s, pass 2 compile 13 s + run 57 s (60 tests) | 39 s at load 81, 78 s at load 139: 0 units, suite 37-73 s, pass 2 0.02 s (1 test) |
| CI test job, warm, first run after switching between the two configurations | n/m | 56 s |

`n/m`: not measured before.

Rejected: `debug = false` for dsp-dev. In an interleaved A/B, incremental times were
indistinguishable from `line-tables-only` (reverb 13.3/20.8 s against 15.8/16.6 s; ir
20.8/14.0 s against 34.7/10.9 s). An earlier sequential run that looked 2× faster had
dropped from load 155 to 120 between arms. Line tables stay for test backtraces.

Not tried: Cranelift. Kontra's tests need `opt-level = 1` (143 s → 28 s of CPU for the
five slowest, 2026-11-25-blaze-ci.md), and with the fixes above the incremental critical
path is kontra-plugin's own lib and lib test (8-9 s), which also runs the slowest tests.

## Verification Matrix

| Check | Tier | Target / OS | Exact Command | Pass Criterion | Status / Observed Evidence |
|---|---|---|---|---|---|
| Spec proof gate | Doc | any | `python3 tools/check-spec-proof.py plugins/kontra/docs/specs/fast-build.md` | Exit 0, zero proof errors | PASS (2026-11-25, derpcat-linux) |
| Incremental sfz test build | Compile/ABI | linux-x86_64 | `nextest run -p kontra-format-sfz` after a one-line sfz edit | 1 unit rebuilt, under 20 s | PASS (2026-11-25, derpcat-linux) |
| Warm CI test job | Validator/Runtime | linux-x86_64 | `matari check origin/main --project kontra --stage test` | at most 2 min warm | PASS (2026-11-25, derpcat-linux) |
| rt-paranoid pass 2 without a second build | Validator/Runtime | linux-x86_64 | `matari check origin/main --project kontra --stage test` | pass 2 compiles no crate twice | BLOCKED (needs a run-time RT checker in moose) |

## Unverified Boundaries & Promotion Gate

- The CI job time is measured on the local self-hosted runner under load, not on every runner.
- See "What is left above target" for the open items; the spec stays `in-progress` until they close.

## What is left above target

- **Pass 2 compile.** rt-paranoid's features (`moose/rt-paranoid`, `kontra-lua/system-allocator`)
  give 13 crates a second build: moose-core and its 8 dependents, kontra-lua, -face,
  -host and -plugin. That is 75 s the first time and incremental after it (15 s once warm).
  Removing it needs the checker selectable at run time instead of by feature, in moose.
- **Test wall under load.** CPU starvation, not test cost, drives the tail: the two
  slowest tests use about 20 s of CPU each but take 55-100 s of wall at load 120+ with
  nice 19. The slow lane (`slow_tests`) takes both off the PR path once `ci-3min` lands.
- **Cold builds.** The Luau C++ build (243 s) bounds a cold build; #246's shared warm
  targets keep CI from paying it.
- **Upstream failure, not from this change.** On origin/main at 889b7909db,
  `kontra-engine::arch_guards uvi_effects_load_through_their_module_tables` fails its
  line ratchet ("format/uvi code lines rose to 6619; ratchet is 6,609"). This branch does
  not touch `format/uvi`.
- **Wall-clock flakes under load (existing, not from this change).** kontra-lua runs
  script callbacks against a 500 µs wall-clock slice, and three of its tests assert that
  a callback finishes (`Done`) inside one slice. At load 130+ with nice 19 a run can be
  preempted past the slice, the test sees `Yield`, and it fails. Each failed once in
  these runs and passed 3/3 alone:
  `set_value_runs_changed_only_on_a_change_but_a_users_move_always`,
  `an_endless_comparator_under_sort_ends_with_a_runaway_fault` (once it was in the
  start-first group, since removed), and
  `a_script_write_of_the_routed_value_is_the_next_routes_base`. The fix belongs in
  kontra-lua: a slice measured in instructions, or a test-only slice override. It is
  not done here.
