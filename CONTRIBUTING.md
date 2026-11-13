# Contributing

KONTRA is under active development. Keep changes small, describe the behavior they
change, and report the checks actually run. Do not commit proprietary sample banks,
local logs, support reports, build output or machine-specific configuration.

## Rights and provenance

Submit original work that you are entitled to contribute. Intentional
contributions to project-authored code follow Apache-2.0 section 5;
third-party code retains its own license. A public GitHub repository is
not sufficient evidence of redistribution rights. Preserve copyright,
license and NOTICE files and clearly identify modifications to vendored code.

For a dependency, copied implementation or interoperability change, record
its origin: repository/document URL, exact revision, author, applicable
license or written permission, and the information actually used. Distinguish
format observations and mathematical facts from copied source expression.
Record access to reference implementations accurately; do not describe
existing work as clean-room work without supporting development records.
Keep lawful-acquisition and permission records privately; never publish
receipts, account information or library access values as proof.

Do not submit Kontakt executable code, leaked SDKs, patches to remove
activation checks, account credentials, serial numbers, library-key lists,
commercial instruments/samples, extracted scripts, impulse responses,
artwork or manuals without appropriate rights. Use synthetic fixtures or
material with an explicit redistribution grant. A library purchase or a
personal music-production license does not establish fixture-sharing rights.
Keep licensing questions visible; do not remove provenance to pass a scan.

Bug reports should contain reproduction steps and the minimum reviewed
diagnostics needed. Commercial presets, `.nicnt` access metadata, caches,
screenshots of library artwork and converted instruments may contain
protected or private material. Do not attach them by default. Local
testing does not authorize uploading its inputs or outputs to an issue.
The maintainer's lawful-interoperability purpose is not permission to
publish content or facilitate unauthorized use. Do not add piracy links,
shared credentials, or instructions for acquiring unlicensed libraries.

Before distributing binaries, regenerate the [license/source bundle](THIRD_PARTY.md#distribution-notices-and-assets)
from the locked dependency graph and review new licenses, native components
and vendored changes. The required `ni-file` permission and focused GPL
reference review remain unresolved; a successful build is not clearance.

## Publishing measured progress

The website's Progress page is built from `website/progress.json`. After a sound
scoreboard run, `tools/progress.py sound <score.json> --revision <commit>` rewrites it in
plain words; commit the file with the change that moved the numbers.

## Where the work is planned

The engine's design, its rules and the reasoning behind them are in
[docs/architecture-v3](docs/architecture-v3/README.md); start with
[VISION.md](docs/architecture-v3/VISION.md). All work lands on `main` through
pull requests.

## Local workflow

The declared minimum is Rust 1.92, matching the pinned plugin framework. CI and contributors
use Rust 1.99.0, pinned in `rust-toolchain.toml` and verified in hosted CI. Local focused
checks have also passed on Rust 1.98.1; the minimum has not been independently tested.

Start from the current development branch, create a focused branch, and commit a
coherent change after its relevant checks pass. Keep unrelated work separate.

Use Cargo's default `target/` directory for each worktree, or set an explicit
`CARGO_TARGET_DIR` unique to that worktree. Do not share it between worktrees
with different local path-dependency sources: cached dependency metadata can
make a successful build or test run insufficient proof of the intended source.

Pull request titles follow Conventional Commits; see
[Pull requests, commits and releases](#pull-requests-commits-and-releases).

For local listening and probe runs use the `fast` profile: release-like code without
cross-crate LTO and with incremental compilation. Shipping (`release`), `ci`, `maxperf`
and `corpus` are unchanged, and CPU numbers for the gate come from `release`.

```sh
build-slot cargo build --profile fast --lib
build-slot cargo --config .cargo/fast-linux.toml build --profile fast --lib
```

`.cargo/fast-linux.toml` links with clang and mold, which must be on `PATH`. Without it,
the default linker is used.

`bacon` watches the library typecheck by default. Its `parity` job runs the existing
small targeted KSP suite; explicit clippy and nextest jobs are also available. Do not
run the complete sample-library suite on every edit. For a focused regression:

```sh
CARGO_TARGET_DIR="$PWD/target" cargo nextest run --release -E 'test(regression_name)'
```

Nextest runs at most two tests concurrently and does not retry failures. Before
merging, run the required [CI checks](docs/CI.md), including the full default-feature
unit/integration/doctest suite with `cargo test --locked --profile ci`. Every nightly
additionally runs this suite with the shipping `release` profile. Report failures
and any checks that could not run. A parser success,
render or synthetic regression establishes only the behavior it actually exercises.
Update the compatibility notes when supported behavior or a known limit changes.

## Pull requests, commits and releases

Someone scrolling the commit list, the pull requests or the releases should
understand what changed without having done the work. Three pieces of text
carry that, and CI checks them (`tools/release/pr.py`):

**The title** becomes the commit on `main` and a line in the release. Write
`type(scope): what changed`, within 72 characters, in plain words:

```text
fix(ui): long preset menus scroll
feat(kontakt): snapshots load their effects
perf(reverb): dense string patches use less CPU
```

Types: `feat` (something new), `fix`, `perf`, `refactor`, `docs`, `test`,
`build`, `ci`, `chore`. Say what was missing or wrong and what is true now.
Leave out how you found it: no memory addresses, decompiler names, task ids
or lane names. Those belong in the description.

**The description** becomes the commit's body. Open with what changed and why
in plain words, then how it was checked. Measurements, sources read and other
detail a reviewer needs come after that.

**The release note** is the line `Release note: ...` in the description: one
sentence a musician understands, or `none` for work they will not notice. With
a note, add `Area:` (`sound`, `interface`, `libraries`, `performance` or
`fix`), which picks its section in the release.

Pull requests are squash-merged, so one pull request is one commit on `main`,
titled with the pull request's title and number and credited to its authors.

### Versions and releases

Versions are semver, `0.MINOR.PATCH` until 1.0. Nobody picks them by hand:
`tools/version.py next` reads the titles merged since the last release. A
`feat` raises MINOR; anything else raises PATCH.

The Release workflow runs when the third pull request since the last release
merges on `main`, and on demand (Actions > Release > Run workflow). It builds
and tests every platform, signs and notarizes the macOS installer, tags the
commit `vX.Y.Z` and publishes a release. Its notes are the merged pull
requests' release notes, each linked to its pull request and author
(`tools/release/notes.py`); `CHANGELOG.md` on every release lists every pull
request and commit since the project began (`tools/release/changelog.py`).
So the changelog writes itself from the pull request title and its
`Release note:` line: write those for a musician. Never move a published tag
or replace its downloads.

Mac publication needs the `APPLE_*` signing secrets listed in
`.github/workflows/release.yml`; without them the release fails and the
previous one stays the latest. There is no unsigned fallback.

## One engine

DSP is shared, not per product. New emulation work extends the engine in `sampler-core` and maps parameters in the format crate. Read [docs/ENGINE.md](docs/ENGINE.md) first.
