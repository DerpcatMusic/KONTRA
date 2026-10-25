---
name: v3-task
description: >-
  Do one KONTRA v3 refactor task end to end: read the issue brief, branch from v3, stay in the
  owned paths, commit with a Task trailer, keep a draft PR as the resume point, run the gate,
  answer review comments, and report. Use whenever you are given a v3 task id (P0.1, P5.3a, ...)
  or a GitHub issue labelled v3, or you resume someone else's v3 branch or draft PR.
---

# Doing a v3 task

The brief is the GitHub issue (`gh issue view <n>`), generated from
`docs/architecture-v3/tasks.toml`. It wins over anything here except the README
rules. The full manual is `docs/architecture-v3/12-WORKFLOW.md`.

## 0. Resume or start

- First look for an open PR of this task (`--head` matches the exact name, not a prefix):
  `gh pr list --base v3 --limit 500 --json number,headRefName,isDraft --jq '.[] | select(.headRefName | startswith("v3-<id>-"))'`.
  If one exists, you are resuming: read its `## Progress` list and commits, check out its branch, and
  carry on from "next". Do not start over.
- Otherwise `git fetch origin v3 && git switch -c <branch from the brief> origin/v3`.
  (T3/Claude worktrees start from an unrelated commit; always branch from `origin/v3`.)

## 1. Before writing code

- Read the brief's "Read first" docs and the code you will touch. With graft:
  `graft ask "<question>" --source`, `graft grep "<literal>"`, `graft skeleton <file>`,
  `graft callers <symbol>` before changing a symbol. Otherwise `rg`.
- Load the `beautiful-rust` skill for any Rust. Precedence: README rules and
  real-time rules > KISS/YAGNI (ENGINE.md) > beautiful-rust.
- List the files you expect to change and compare them with the brief's Scope.
  A file outside scope: stop and comment `Scope request: <paths> because <reason>`
  on the issue, then report. Never edit it.

## 2. While working

- Commit early: Conventional Commits, `Task: <id>` trailer, and your
  `Co-Authored-By:` line. After the first commit push and open a **draft** PR:
  `gh pr create --draft --base v3 --title "<id>: <title>"`, body from the template
  plus a `## Progress` list (done / next / numbers so far). Update it at every push.
  Sessions end without warning; the draft PR is how the next agent picks up.
- Cargo only through `build-slot cargo ...` on the maintainer's machine (contributors:
  plain `cargo`). Never set `CARGO_TARGET_DIR`. Crate-scoped commands only:
  no workspace builds, corpus runs or plugin bundles, except commands the brief's
  gate lists explicitly (for example `cargo check --workspace` in P4.0).
- Formatting: `rustfmt --edition 2024 <files you changed>`. Never `cargo fmt` on a
  whole crate; it rewrites unrelated files. Clippy: `cargo clippy -p <crate>
  --all-targets --no-deps`; the bar is no new warnings in files you changed.
  Once `cargo xtask ci` exists (task P0.7), run it instead; it does all of this.
- `Cargo.lock`: keep only additions your change needs.
- New crates declare `license = "Apache-2.0"` (repository policy). Separately, a crate the
  plugin links with no license declared fails the Website build's notice bundler
  (tools/licenses.py) with "Missing license text".

## 3. Proving it

- Run every gate command from the brief and paste the real output. Check that each
  test filter actually ran tests: `running 0 tests` is a failed gate, not a pass.
- Measure what the done criteria name, under the conditions they name. A
  "rebuild after a DSP edit" is timed after editing a DSP constant, not a comment.
- If you skip a brief step, say so and why. Never write "per the brief" about a
  step the brief did not tell you to skip.
- `python3 tools/plan/plan.py scope <id> --base origin/v3` must print ok.

## 4. Finishing

- Rebase on `origin/v3`, fill in the template (gate output, scope check, net
  production lines, deviations), `gh pr ready`.
- CodeRabbit reviews the PR. Fix each real finding in a new commit, or reply
  with the reason it does not apply (a measurement, a doc rule). Do not leave
  threads unanswered.
- Report to whoever dispatched you: PR URL, head SHA, gate output, the numbers
  the done criteria ask for, and every deviation.
