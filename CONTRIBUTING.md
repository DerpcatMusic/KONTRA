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

Use Conventional Commits, for example `fix(engine): preserve release ownership` or
`feat(diagnostics): export build identity`. The prefix helps review; it does not
trigger an automatic version bump. Mark incompatible changes with `!` or a
`BREAKING CHANGE:` footer and explain the user-visible migration.
[Conventional Commits](https://www.conventionalcommits.org/en/v1.0.0/) describes the syntax.

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

## Version and changelog policy

The root `[package].version` in `Cargo.toml` is authoritative. The root package in
`Cargo.lock`, plugin descriptors and bundle metadata must agree. Do not add a
separate version literal or a `moose.toml` override. Published snapshots keep their
original identity; the reviewed fix ledger starts after the shipped `0.3.0` baseline.

Follow [Semantic Versioning](https://semver.org/spec/v2.0.0.html). During `0.y.z`
development, use a deliberate minor bump for incompatible behavior or substantial
new capabilities and a patch bump for compatible fixes. After `1.0.0`, incompatible
public API/behavior changes require a major bump, compatible additions a minor bump,
and compatible fixes a patch bump. A commit does not automatically increment the
package version. `release-fixes.json` records stable logical defect IDs, source
checkpoints, validation and explicit maintainer acceptance. Each accepted ID advances
the baseline patch once: ten accepted fixes after `0.3.0` produce `0.3.10`.
Features, repeated commits, test follow-ups and pending fixes do not increase that
count. Review and accept the shipped fix IDs, update the changelog, then prepare
the authoritative Cargo and lockfile version:

```sh
python3 tools/version.py fixes --write
python3 tools/version.py check
```

Stable releases use immutable `vX.Y.Z` tags on the reviewed release commit. Before
publishing, change the unreleased heading to its UTC release date, check the exact
version and tag, and verify packaged binaries/manifests. Never move a published
stable tag or replace its assets with different code. Start a fresh unreleased
changelog section for subsequent development. Changelog entries describe meaningful
changes and known limits; do not manufacture historical release entries from every
commit or imply complete Kontakt compatibility.

## Nightly builds and source identity

Nightlies are prereleases such as `0.3.10-nightly.20261002.g012345abcdef`. The date
comes from the source epoch in UTC and the `g` prefix keeps a hexadecimal revision
from becoming an invalid numeric SemVer identifier. An identical revision and epoch
produce the same version. CI prepares it in its disposable checkout:

```sh
export SOURCE_DATE_EPOCH="$(git show -s --format=%ct HEAD)"
python3 tools/version.py nightly --write
python3 tools/version.py check --tag "v$(python3 tools/version.py check)"
```

The helper rewrites only the root package version and matching lockfile entry. This
makes the build's tracked source state `modified`, which the identity reports
truthfully. Ignore/untracked build artifacts do not mark the source modified. A
nightly includes reviewed Added, Changed, Fixed and Known limits deltas from the
previous published source, the complete shipped public commit messages and merged
PR descriptions, and a comparison link. The versioned changelog is also retained
inside `release-manifest.json`. Keep every accepted fix described in CHANGELOG.md;
never describe a pending candidate as shipped. An unchanged-source rerun preserves
the published notes and assets.
Nightly packaging retains its documented bounded snapshot history; stable tags and
assets follow the stable release policy.

`build.rs` emits `kontra-build.json` under Cargo's `OUT_DIR` and embeds the same
identity in the binary. Both the CLI and standalone support `--version` (readable,
including the full revision) and `--build-info` (JSON, before audio/UI startup).
About and support exports consume the same API. Package the manifest from the
actual plugin build immediately after that build; a standalone build can have
additional features and therefore a different identity. Validate a manifest with:

```sh
python3 tools/version.py manifest --file path/to/kontra-build.json
```

`SOURCE_DATE_EPOCH` fixes the UTC timestamp for reproducible build identity; without
it, the timestamp records the build time. This does not by itself guarantee identical
binary bytes across toolchains or machines.

For release validation, retain Cargo's `--message-format=json` compiler output,
a manifest of tracked source-file hashes (including local path dependencies),
the toolchain/build arguments and hashes of the exact tested executables alongside
the test results. After suspected cache contamination, rebuild in a fresh target
directory and rerun the required gates; earlier results remain evidence of their
observed behavior, not proof of the final source/dependency cohort.

[The reproducible-builds specification](https://reproducible-builds.org/docs/source-date-epoch/)
describes the convention. The manifest records the chosen epoch explicitly.

`revision` always identifies the actual checkout. A public export can have its own
Git history: set `KONTRA_SOURCE_REVISION` to a full upstream revision only when that
provenance is known; it stays a separate field and does not replace `revision`.
For a source archive without Git, `KONTRA_BUILD_REVISION` may provide its known full
revision. It must match HEAD when Git is present. Unknown revision/status is reported
as unknown, not inferred. Dirty status considers tracked changes only.

Use the standard library and existing tools before adding dependencies. The helper
requires Python 3.11+ and is intentionally manual; no bot makes release decisions.
Its focused check is `python3 tools/version.py self-test`; Cargo includes
`embedded_build_identity_matches_cargo_and_manifest` to check the manifest and plugin
version against the package version.

Mac publication requires `APPLE_APPLICATION_CERTIFICATE_P12_BASE64`,
`APPLE_CERTIFICATE_PASSWORD`, `APPLE_DEVELOPER_ID_APPLICATION`, `APPLE_ID`,
`APPLE_APP_SPECIFIC_PASSWORD` and `APPLE_TEAM_ID` as Actions secrets available to
this repository. Both hosted Mac architectures sign with Developer ID, submit a
DMG to Apple, and require Accepted status, validated stapling and Gatekeeper
assessment before publishing. Missing credentials or validation failures leave
the existing release intact; there is no unsigned fallback. ZIPs retain their
existing native products and include the notarized DMG and hash-bound receipt.
Open the DMG in Finder so Gatekeeper can ingest its ticket before copying the
products; direct ZIP extraction is not an offline-ticket installation path.
See [Apple's distribution guidance](https://developer.apple.com/documentation/xcode/packaging-mac-software-for-distribution).
