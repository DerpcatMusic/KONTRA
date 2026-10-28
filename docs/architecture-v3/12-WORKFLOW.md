# 12 Workflow: how agents and people work on v3 in parallel

This is the operating manual for the refactor. 00–11 say *what* to build;
this says *how many hands build it at once without stepping on each other*.
It is enforced by tools (`tools/plan/plan.py`, the `v3 plan` CI check, the
`v3` ruleset), not by memory.

## 1. Sources of truth

| What | Where | Who edits |
|---|---|---|
| Why, and what every change must keep possible | [`VISION.md`](VISION.md) (its opening quote is copied into every brief) | coordinator only |
| Design | `docs/architecture-v3/00–11` | any PR that changes reality updates the doc in the same PR |
| Tasks: ids, deps, owned paths, steps, gates | [`tasks.toml`](tasks.toml) | coordinator only (`v3-plan-*` branches) |
| Task index + dependency graph | [`TASKS.md`](TASKS.md) | generated: `plan.py index` |
| Live status | GitHub issues (`v3` label), project **KONTRA v3** | `plan.py sync` / `refresh`, the merge hook, claims |
| Gate results | `docs/architecture-v3/gates/G<n>.md` | coordinator |

Issue bodies are generated from `tasks.toml`. Editing an issue body on GitHub
is pointless: the next `sync` overwrites it. Discussion goes in comments.

## 2. Roles

- **Coordinator** (one at a time: the main session or the maintainer). Keeps
  `tasks.toml` true, dispatches ready batches, reviews and merges PRs into `v3`,
  runs the expensive things once per merge batch (workspace check, probe on
  fixtures, real-library runs at gates), merges `main` into `v3`.
- **Worker** (agent or person). Takes one task, works only inside its owned
  paths, runs only its gate commands, opens one PR, reports. Default model for
  agent workers: see the coordinator's standing instructions (currently Claude
  Haiku 5.5 at xhigh via T3 `delegate_task`, self-contained brief).
- **Escalation (paused 2026-10-25: DeepSeek capacity reached; Haiku only, split large tasks smaller).** When a task is large (engine rewrites, big DSP families,
  KSP init-on-VM) or Haiku needs a third review round, the coordinator gives
  it to DeepSeek V4.1 Flash Free on OpenCode at max thinking
  (`opencode/deepseek-v4.1-flash-free`, variant `max`). It is not Claude and
  does not auto-load `.claude/skills`, so its brief says to read
  `.claude/skills/beautiful-rust/SKILL.md` and repeats the repo rules.
- **Spike** (agent or person). Answers a question with a written verdict and
  evidence (`docs/architecture-v3/spikes/*.md`); code only as a throwaway probe.

## 2b. Rust style

Every Rust change on v3 follows the vendored
[BeautifulRust](https://github.com/morganlinton/BeautifulRust) skill at
`.claude/skills/beautiful-rust/` (Claude agents in any v3 worktree load it as
`beautiful-rust`; people can read `SKILL.md`). Precedence when they disagree:
README rules and real-time constraints (no allocation, locks or I/O on the
audio thread), then KISS/YAGNI (AGENTS.md, ENGINE.md), then the skill.

## 3. Branches

```
main ──────────────●──────────────●────────────▶  ships nightlies; v3 work never lands here directly
                    \ merge main   \ merge main
v3   ──●──●──●──●────●──●──●──●─────●──●──●────▶  integration branch, protected, always green
        \  \  \
         v3-P1.2-preload-law, v3-P5.1-dsp-family-dynamics, ...   one branch per task
         v3-plan-<slug>      plan edits (tasks.toml, docs, tools/plan)
         v3-sync-main-<date> main → v3 merges
```

- `v3` is created from the commit that holds this folder. It merges back into
  `main` only at a gate the user approves (G3 is the first candidate: the
  playback fixes are user-visible; G8 is the end).
- Task branches: `v3-<task id>-<slug>`. CI reads the task id from the branch
  name, so the name is not cosmetic. `plan.py brief <id>` prints the exact name.
- Squash-merge task and plan PRs (merge commits only for main syncs). One task = one squash commit on `v3`, titled
  `<id>: <title>`.
- `main` → `v3`: the coordinator opens `v3-sync-main-YYYYMMDD` with a merge
  commit (never rebase `v3`) at least weekly and before every gate.

## 4. Task lifecycle

```
tasks.toml ──sync──▶ issue [status:blocked] ──deps merged──▶ [status:ready]
   ──coordinator runs `plan.py ready`, dispatches a conflict-free batch──▶ assigned / [status:claimed]
   ──worker: branch, commits, gate──▶ PR into v3 ──`CI required` + `v3 plan` green, review──▶ squash merge
   ──merge hook──▶ issue closed, dependents relabelled [status:ready] ──▶ next batch
```

1. **Ready.** All `deps` are merged into `v3` (issue closed). `draft-brief`
   tasks also need the coordinator to re-check `owns` and `steps` against the
   tree, update `tasks.toml`, and drop `draft = true` first.
2. **Dispatch.** `python3 tools/plan/plan.py ready` prints the largest batch
   of ready tasks whose owned paths do not overlap each other or anything
   already claimed. Dispatch exactly that batch (it also respects the build
   slots: at most ~6–8 workers, each only running crate-scoped tests).
3. **Claim.** Assign the issue (or add `status:claimed`) *before* the worker
   starts. Unassign if the worker gives up, with a comment saying where it stopped.
4. **Work.** Follow the brief. Commit early. After the first commit open a
   **draft** PR and keep a `## Progress` list in its body (done, next,
   numbers so far). Sessions end without warning (rate limits, context,
   quota); the next agent resumes from the draft PR, not from anyone's memory.
   Rebase on `origin/v3` before marking it ready.
5. **PR.** `gh pr create --base v3 --title "<id>: <title>"`; the template
   asks for the gate output, scope check, net lines and deviations.
6. **Merge.** Coordinator squash-merges. The `v3 plan / merged` job closes the
   issue (closing keywords do not work for non-default branches) and refreshes
   labels. Then the coordinator runs `plan.py sync` if the plan changed.

## 5. Scope: why parallel work does not conflict

Every task lists `owns`: the only paths it may change. Two rules make
parallel work safe:

- **Dispatch rule.** `plan.py ready` never hands out two tasks whose owned
  paths overlap (existing files are matched exactly; new paths by glob prefix).
- **PR rule.** The `v3 plan` check runs `plan.py scope <id>`: any changed file
  outside `owns` fails the PR. If the task genuinely needs more, the worker
  stops and comments `Scope request: <paths> because <reason>`; the coordinator
  either widens `owns` (plan PR) or labels the PR `scope-override`.

Hot shared files are named **locks** in `tasks.toml` and referenced as `@name`:

| Lock | Paths | Mode |
|---|---|---|
| `@manifests` | root `Cargo.toml`, `.cargo/**`, toolchain | exclusive |
| `@lockfile` | `Cargo.lock` | regenerate: never hand-merged (§6); any task may change it, so it never serializes tasks |
| `@ci` | `.github/workflows/**`, `.github/scripts/**` | exclusive |
| `@v2rs` | `src/sound/v2.rs` | exclusive |
| `@plugin` | `src/plugin.rs`, `src/plugin/**`, `src/lib.rs` | exclusive |
| `@corelib` | `sampler-core` `lib.rs` + `Cargo.toml` | exclusive |
| `@registry` | `crates/dsp/registry` | append: several tasks add one line each |
| `@plan` | `tasks.toml`, `TASKS.md` | exclusive (coordinator) |

Design choices that keep the lock set small:

- **P4.0 scaffolds every v3 crate up front** (members, path deps, workspace
  dependencies), so later tasks edit only inside their own crate folder.
- **P5.0 adds a bridge** (`Processor::Module(AnyModule)`, one lookup in
  `lower.rs`), so the DSP family tasks never touch `dsp.rs`/`lower.rs`; they
  add one registry line each (append lock). The old arms die in P5.9.
- **Moves are separate tasks** from behaviour changes (`git mv` + `package =`
  aliases + `pub use` re-exports), so a move touches few files and is
  reviewable as a rename.

`python3 tools/plan/plan.py validate` lists every pair of tasks that could run
in parallel by the DAG but share an exclusive path ("serialized: …"). Those
pairs simply run one after the other; that list is the honest parallel width.

## 6. Conflicts and rebases

- Rebase the task branch on `origin/v3` (`git fetch && git rebase origin/v3`).
  Never rebase or force-push `v3`.
- **`Cargo.lock`:** during a rebase take `v3`'s copy, then let cargo update it:
  `git checkout --ours Cargo.lock && build-slot cargo metadata --format-version 1 >/dev/null && git add Cargo.lock`
  (in a rebase, `--ours` is the branch being rebased onto).
- **Unrelated lockfile drift:** cargo may re-resolve packages you did not touch
  (seen in P0.1: `gpu-allocator` moved to `windows 0.56`, breaking CI's DX12
  check). Your `Cargo.lock` diff must contain only additions your change needs.
- **Stacked tasks:** a task may start on its parent's PR branch while the parent
  waits for CI. After the parent squash-merges:
  `git rebase --onto origin/v3 <old parent head>`.
- **Append lock (registry list):** keep both sides' lines, sorted.
- **Anything else:** a conflict outside your `owns` means the plan was wrong.
  Stop and tell the coordinator; do not resolve it by editing their paths.
- Before any destructive git command on a dirty tree: `git wip` (global rule).

## 7. Testing and building

- Workers run **only the gate commands** in their brief, through `build-slot`
  (4 machine-wide slots; it sets per-worktree target dirs). No
  `cargo build --workspace`, no corpus, no surveys, no plugin bundles.
- The coordinator, once per merge batch on `v3` head: `cargo check --workspace`,
  `cargo xtask debt` (must not grow), `kontra-probe` on all fixtures.
  At phase gates: real libraries (Areia, Vista, Conflux, ANALOG), CPU, a DAW
  listen test, `gates/G<n>.md`.
- GitHub runs full CI on every PR (`CI required`) and on every push to `v3`.
- A red `v3` stops merging until it is fixed or the offending PR is reverted.

## 8. Dispatching an agent

The issue body *is* the brief (`plan.py brief <id>` prints the same text).
For a T3 delegated worker:

```text
delegate_task(
  task = <output of `python3 tools/plan/plan.py brief P5.2`>
         + "Issue: #<n>. Work in your worktree; first run `git fetch origin && git switch -c v3-P5.2-<slug> origin/v3`.",
  target = { model: <the coordinator's standing worker model> },
  isolation = "worktree",
  clientRequestId = "v3-P5.2-r1")
```

A `delegate_task` worktree starts from the coordinator's local commit, which is
why the brief tells the worker to branch from `origin/v3` itself. Review
rounds are new `delegate_task` calls with the brief, the review findings and
the worker's replies (`-r2`, `-r3` …).

Claude Code users (maintainers and contributors) get the same setup from the
repo: the `v3-worker` agent (`.claude/agents/v3-worker.md`) runs one task with
the `v3-task` skill; the `v3-reviewer` agent runs the `v3-review` skill on a
report or PR before anything is merged or built on it.

## 9. Review checklist (coordinator)

- [ ] `v3 plan` and `CI required` green; scope check clean or `scope-override` justified.
- [ ] Gate output pasted and plausible; the done criteria hold. **Re-run at
      least one claim yourself** (a gate command, a number, a file the report
      says exists). Debunked so far: a gate filter that matched 0 tests (P1.1),
      a "rebuild after edit" timed on a comment-only edit (P0.4), a brief step
      reported as "skipped per the brief" when the brief required it (P0.4).
- [ ] Corrections go back into the plan the same day: `tasks.toml` steps,
      gates and the brief template, so the next worker does not repeat it.
- [ ] Net production lines reported; negative unless the task adds a crate.
- [ ] No new `#[allow]`, `#[path]`, `cfg(test)` hooks in production functions,
      `Mutex` in RT crates, vendor names below `format-*` (README rules).
- [ ] Rust follows the `beautiful-rust` skill (`.claude/skills/beautiful-rust`): fmt and clippy clean on the touched crates,
      no `unwrap` on fallible paths, `// SAFETY:` on every `unsafe`, types over bool flags. The architecture docs win on conflict.
- [ ] Docs in this folder updated if the code diverged from them.
- [ ] Moves are pure moves (`git diff -M --stat` shows renames).

## 10. Changing the plan

- Only on `v3-plan-<slug>` branches, by the coordinator: edit `tasks.toml`,
  run `plan.py validate && plan.py index`, PR into `v3`, then `plan.py sync`.
- New task ids append (`P5.11`, `P1.4b`); never renumber merged tasks.
- A task that turns out bigger than one PR is split into lettered tasks with
  deps, before work continues.

## 11. GitHub setup (applied once by the coordinator)

| Thing | Setting |
|---|---|
| Branch `v3` | ruleset "v3 integration": PR required, required checks `CI required` + `v3 plan` + `CodeRabbit`, squash (merge commits only for `v3-sync-main-*`), no force push, no deletion, admins may bypass in a PR |
| Repo | auto-merge allowed; delete head branches on merge |
| Labels | `v3`, `v3-epic`, `phase:0–8`, `lane:*`, `size:S/M/L`, `agent:*`, `status:ready/blocked/claimed`, `draft-brief`, `scope-override`, `size-growth` (`plan.py sync`) |
| Milestones | `v3 · P<n> <phase>` per phase |
| Issues | one per task (type Task), one epic per phase (type Feature) with tasks as sub-issues, task deps as "blocked by" relations |
| Project | **KONTRA v3** (user project, linked to the repo): fields Phase, Lane, Size, Agent, Task; suggested views: Board by Status, Table grouped by Phase, Roadmap by milestone (views are created in the UI; the API cannot create them) |
| Templates | `.github/pull_request_template.md`, issue forms for bugs and v3 task proposals |
| Review | CodeRabbit (`.coderabbit.yaml`) reviews ready PRs into `v3` (drafts skipped) with the README rules and real-time checks as path instructions; the ruleset requires every review thread resolved, so each finding is fixed or answered with a reason before auto-merge |
| CI | `.github/workflows/v3.yml`: `v3 plan` (validate, index check, unit tests, scope) on PRs into `v3`; issue close on merge; full CI on push to `v3` |

Why `v3-` and not `v3/`: git stores refs as files, so a branch named `v3`
makes every `v3/...` branch impossible (`directory file conflict`).
