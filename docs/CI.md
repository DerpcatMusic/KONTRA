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
| Workflow checks | YAML, expressions, dependency graph, documentation filtering and simulated release safety | `bash .github/scripts/check_workflows.sh` (Linux); `python3 -m unittest discover -s .github/scripts -p 'test_*.py'` |
| rustfmt | Formatting differences, currently advisory because the tree has existing debt | `cargo fmt --all --check` |
| clippy | Compilation plus Rust lint diagnostics; existing warning policy is unchanged | `cargo clippy --locked --profile ci --all-targets` |
| Linux tests | Default-feature unit/integration/doc-tests for the root and every first-party workspace package, including the parity ledger and corpus collector | `cargo test --locked --profile ci -p kontakto -p sampler-core -p sampler-ir -p sampler-simd -p sampler-pool -p sampler-ksp -p sampler-kontakt -p sampler-uvi -p sampler-midi -p sampler-native -p kontra-ui-model -p sampler-perf -p sampler-parity -p corpus-health` |
| Windows/macOS check | Native API, platform configuration and standalone compilation; this does not execute tests | `cargo check --locked --profile ci --all-targets --features standalone` |
| CI required | Every applicable job succeeded, or all expensive jobs were deliberately skipped for docs | GitHub Actions job |

`library-access` is already a default feature. The full test command includes its
four `access::*` unit tests, so rerunning them with the same enabled feature was
redundant. No assertion or test was removed, and existing ignored/local-library
or timing-sensitive tests were not newly disabled.

The root-only `cargo test` command does not select the other workspace packages.
CI names every first-party package explicitly; `sampler-parity` and
`corpus-health` are included so parity reporting and corpus-result admission
receive the same checks as the playback crates. Installed-library tests that
require local paths remain separate from synthetic CI coverage.

`ci` inherits `release`: optimization level 3, release assertion/overflow behavior
and unwind semantics remain. It turns off **cross-crate ThinLTO**, reducing linker
work. It does not prove that the shipping ThinLTO binary behaves identically.
That is why every snapshot must rerun the full suite with `--profile release`.
The shipping release profile itself is unchanged.

Rust 1.99.0 matches the compiler in the successful baseline run linked below.
Actions use immutable commit pins; Cargo commands use the committed lockfile.
Editor validation uses the locally retained MUI workflow from revision
`dcf0796082feec053af1418e3a38a302ee61da0a`. Its display/graphics setup gets two
three-minute attempts inside a seven-minute step deadline, with bounded network
waits and package recovery before retrying.
Caches are separated by OS/target, profile and feature set. The cache action also
keys the compiler, Cargo manifests/lockfile and compiler-related environment.
Caches are accelerators, not test evidence or release artifacts.

## Releases

`.github/workflows/release.yml` runs once a day and on demand. A scheduled
run that finds nothing new in what ships (everything outside `docs/`,
`website/`, `tools/` and Markdown) since the last release tag stops there.
Otherwise it:

1. runs the full verification (`v3-verify.yml`) on the commit;
2. builds the CLAP and VST3 bundles for Linux, Windows and both macOS
   architectures, stamped with the release's version;
3. composes, signs and notarizes the universal macOS installer;
4. tags the commit `vX.Y.Z` (`tools/version.py next`) and publishes the
   release with notes from the merged pull requests
   (`tools/release/notes.py`).

Releases stay published. Download names are stable, so
`releases/latest/download/<name>` always serves the newest. No pull request
workflow publishes. How versions and notes are derived:
[CONTRIBUTING.md](../CONTRIBUTING.md#pull-requests-commits-and-releases).

## Before/after and what to measure

Before: each code PR/main push ran Linux release clippy + full release tests +
a second access-only invocation, plus Windows compile. Every main push, including
docs, independently ran four packaging jobs and publication.

After: each code PR/main push runs one optimized Linux test pass, Windows and
macOS compile checks, plus inexpensive workflow/aggregate jobs. Docs get the stable
cheap gate. Four-platform release builds still run on every main push/manual
request, behind exact-commit shipping-profile verification.

An observed baseline Linux run spent about **11m52s compiling for the full tests**
and **8s executing them**, then about **4m** on the redundant invocation. Removing
that invocation avoids its work, but these are historical observations, not a
promised speedup. The extra native macOS check has a cost. Compare cold/warm job
durations and total runner-minutes on real runs before claiming realized savings.

For branch protection, select the stable **CI required** check after it has run
successfully. This PR does not change repository protection/settings. Existing
required check names must be reviewed by a maintainer before changing them.

The Linux dependency step keeps its exact downloaded `.deb` files in the plugin artifact.
Editor jobs verify the cache hashes and configure those packages offline, with five
bounded attempts and exponential backoff. A complete preinstalled Xvfb/graphics/tool
stack needs no installation. Vulkan/GL diagnostics, the screen recording, editor
reopen and MUI first-frame checks remain required.

## Why this setup / primary references

- [GitHub: required checks and skipped workflows](https://docs.github.com/en/pull-requests/how-tos/merge-and-close-pull-requests/troubleshooting-required-status-checks)
- [GitHub: reusable workflows](https://docs.github.com/en/actions/how-tos/reuse-automations/reuse-workflows)
- [Cargo: custom profiles and LTO](https://doc.rust-lang.org/cargo/reference/profiles.html)
- [rust-cache: automatic key inputs](https://github.com/Swatinem/rust-cache#cache-details)
- [Observed successful compiler/runtime baseline](https://github.com/DerpcatMusic/KONTRA/actions/runs/36933781255/job/110609129410)
