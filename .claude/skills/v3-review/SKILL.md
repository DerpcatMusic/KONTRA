---
name: v3-review
description: >-
  Review a KONTRA v3 PR or an agent's task report before it merges or before anything is built on
  it: re-run claims, debunk wrong ones, check scope, rules and gates, triage CodeRabbit, and push
  corrections back into the plan. Use when a worker reports back, when a v3 PR is ready, or when
  asked to check a spike verdict or plan proposal.
---

# Reviewing v3 work

Reports are claims. Treat every number, "tests pass", "skipped per the brief" and
"no change needed" as unverified until you have checked it. The checklist in
`docs/architecture-v3/12-WORKFLOW.md` §9 is the minimum.

## 1. Re-run at least one claim

- Pick the claim the merge depends on most: the gate output, a timing, a contract
  verdict. Runtime claims (tests, timings, verdicts) are re-run and their output
  read; reading the code is enough only for claims about code.
- Known failure shapes (all happened):
  - a test filter that matches no tests (`running 0 tests` reported as green);
  - a timing on a no-op edit (comment) instead of the edit the target names;
  - "skipped per the brief" for a step the brief required;
  - a contract that passes on a silent reference;
  - a stacked PR still carrying its squash-merged parent's commits.
- Spike or plan verdicts: check the sample size (how many instruments or hosts),
  whether the machine was shared during timing, and whether the conclusion
  follows from the numbers or only from the narrative.

## 1b. Before dispatching a brief

- Check every step against the code it names: does the type exist, is it
  public to the crate that must use it (P1.1b: `ScriptBank` was `pub(crate)`),
  is the value still available at that stage (a lowered constant is a register)?
- A step that adds a field to a struct needs every struct literal in scope:
  `graft grep "<Struct> {"` and add each file (P1.1b round 1 stopped on two).
- A gate's test filter must match the module path (`--lib <module>`), or it runs 0 tests.

## 2. Check the diff

- `gh pr diff <n> --name-only` against the brief's Scope; `plan.py scope` is
  enforced by CI but read the files anyway.
- README rules: no new `#[path]`, unexplained `#[allow]`, `cfg(test)` hooks in
  production functions, `Mutex` in real-time crates, vendor names below `format-*`.
- KISS/YAGNI (ENGINE.md): a second implementation, a one-impl trait, or config
  for a constant needs a cited measurement.
- Moves are pure (`git diff -M --stat` shows renames); behaviour changes are separate.
- Net production lines: negative unless the task adds a crate.

## 3. CodeRabbit

- Read every CodeRabbit comment. Real findings go back to the worker (the
  `v3-reviewer` agent never edits task code; a coordinator may fix a small one
  itself). Wrong ones: reply with the reason (a rule, a measurement) and resolve.
- CodeRabbit does not know the plan. A *proposed fix* that contradicts tasks.toml
  or the architecture docs is answered by the doc. *Code* that contradicts them is
  a real finding: fix the code, or update the doc in the same PR if the code is right.

## 4. Close the loop

- Every correction goes back into the plan the same day: tasks.toml steps,
  gates, done criteria, the brief template in `tools/plan/plan.py`, or this skill.
  The next worker must not be able to repeat the mistake.
- Partial delivery: merge what is correct and add a follow-up task (for example
  P1.1 → P1.1b) rather than stretching the PR's scope.
- Only when the verdict is `merge` and every finding and review thread is
  resolved: `gh pr merge <n> --auto --squash`. After the PR shows `MERGED`,
  `git fetch origin && git switch --detach origin/v3` (clean tree), then
  `python3 tools/plan/plan.py sync` and `ready`.
- After each merge batch, on that clean `origin/v3` checkout, run
  `cargo xtask debt --check` and `size --check`. Required checks are not strict, so two PRs can each pass and still
  break the ratchet together (P1.1b merged just before the size baseline did).
  Fix forward at once: a PR that carries the `size-growth` label (add it before
  pushing) and the baseline from `size --update`.
