# CI and snapshot checks

## The normal loop

Open a draft PR early, make small commits, and review the failing job's log before
rerunning it. Every PR, merge-group candidate and `main` push gets a stable
**CI required** result. A small change-classification job skips native/Rust jobs
only for known documentation/audit-only changes. Unknown paths, missing history,
workflow changes and dependency changes run the full gate. Failed or cancelled
checks cannot turn the aggregate green.

| Check | What it establishes | Local command |
| --- | --- | --- |
| Workflow checks | YAML, expressions, dependency graph, documentation filtering and simulated release safety | `bash .github/scripts/check_workflows.sh` (Linux); `python3 -m unittest discover -s .github/scripts -p 'test_*.py'`; `python3 .github/workflows/check_nightly.py` |
| rustfmt | Formatting differences, currently advisory because the tree has existing debt | `cargo fmt --all --check` |
| clippy | Compilation plus Rust lint diagnostics; existing warning policy is unchanged | `cargo clippy --locked --profile ci --all-targets` |
| Linux tests | The entire existing default-feature unit/integration/doc-test suite | `cargo test --locked --profile ci` |
| Windows/macOS check | Native API, platform configuration and standalone compilation; this does not execute tests | `cargo check --locked --profile ci --all-targets --features standalone` |
| CI required | Every applicable job succeeded, or all expensive jobs were deliberately skipped for docs | GitHub Actions job |

`library-access` is already a default feature. The full test command includes its
four `access::*` unit tests, so rerunning them with the same enabled feature was
redundant. No assertion or test was removed, and existing ignored/local-library
or timing-sensitive tests were not newly disabled.

`ci` inherits `release`: optimization level 3, release assertion/overflow behavior
and unwind semantics remain. It turns off **cross-crate ThinLTO**, reducing linker
work. It does not prove that the shipping ThinLTO binary behaves identically.
That is why every snapshot must rerun the full suite with `--profile release`.
The shipping release profile itself is unchanged.

Rust 1.99.0 matches the compiler in the successful baseline run linked below.
Actions use immutable commit pins; Cargo commands use the committed lockfile.
Caches are separated by OS/target, profile and feature set. The cache action also
keys the compiler, Cargo manifests/lockfile and compiler-related environment.
Caches are accelerators, not test evidence or release artifacts.

## Nightly and deeper verification

Nightly runs at **03:23 UTC daily** on public `main` when build-relevant content
changed since the published `nightly` tag. GitHub may delay scheduled runs.
Manual **Actions → Nightly → Run workflow → main** intentionally rechecks and
rebuilds even an unchanged revision. A missing old tag triggers a first build.

The graph is:

1. Resolve the event's exact candidate SHA and classify changes
2. Call this commit's CI workflow with `release_validation: true`
3. Require release-profile Linux tests and Windows/macOS compilation
4. Build CLAP, VST3 and standalone for Linux x64, Windows x64, macOS arm64 and macOS x64
5. Check all four ZIPs and SHA-256 manifests, then publish together only if that
   SHA is still `main`; each ZIP records its source in `SOURCE_COMMIT.txt`

No PR workflow publishes. Downloads come only from the same Nightly run, not a
cache or a different PR. Obsolete verification/build jobs can cancel; the publish
critical section cannot. Existing read/write permission boundaries are preserved.
These checks are automated regression coverage, not DAW or Kontakt certification.

To test shipping optimization without publishing, use **Actions → CI → Run
workflow**, leave **release_validation** enabled, and choose your branch. Locally:
`cargo test --locked --release`. Changes to DSP, unsafe code, CPU assumptions,
compiler settings or dependencies particularly deserve this deeper check.

## Before/after and what to measure

Before: each code PR/main push ran Linux release clippy + full release tests +
a second access-only invocation, plus Windows compile. Every main push, including
docs, independently ran four packaging jobs and publication.

After: each code PR/main push runs one optimized Linux test pass, Windows and
macOS compile checks, plus inexpensive workflow/aggregate jobs. Docs get the stable
cheap gate. Four-platform release builds are daily changed-code/manual work behind
exact-commit shipping-profile verification.

An observed baseline Linux run spent about **11m52s compiling for the full tests**
and **8s executing them**, then about **4m** on the redundant invocation. Removing
that invocation avoids its work, but these are historical observations, not a
promised speedup. The extra native macOS check has a cost. Compare cold/warm job
durations and total runner-minutes on real runs before claiming realized savings.

For branch protection, select the stable **CI required** check after it has run
successfully. This PR does not change repository protection/settings. Existing
required check names must be reviewed by a maintainer before changing them.

## Why this setup / primary references

- [GitHub: required checks and skipped workflows](https://docs.github.com/en/pull-requests/how-tos/merge-and-close-pull-requests/troubleshooting-required-status-checks)
- [GitHub: reusable workflows](https://docs.github.com/en/actions/how-tos/reuse-automations/reuse-workflows)
- [GitHub: scheduled-event behavior](https://docs.github.com/en/actions/reference/workflows-and-actions/events-that-trigger-workflows#schedule)
- [Cargo: custom profiles and LTO](https://doc.rust-lang.org/cargo/reference/profiles.html)
- [rust-cache: automatic key inputs](https://github.com/Swatinem/rust-cache#cache-details)
- [Observed successful compiler/runtime baseline](https://github.com/DerpcatMusic/KONTRA/actions/runs/36933781255/job/110609129410)
