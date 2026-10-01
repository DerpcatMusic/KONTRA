# Contributing

KONTRA is under active development. Keep changes small, describe the behavior they
change, and report the checks actually run. Do not commit proprietary sample banks,
local logs, support reports, build output or machine-specific configuration.

## Local workflow

The declared minimum is Rust 1.92, matching the pinned plugin framework. CI and contributors
use Rust 1.99.0, pinned in `rust-toolchain.toml` and verified in hosted CI. Local focused
checks have also passed on Rust 1.98.1; the minimum has not been independently tested.

Start from the current development branch, create a focused branch, and commit a
coherent change after its relevant checks pass. Keep unrelated work separate.
Use Conventional Commits, for example `fix(engine): preserve release ownership` or
`feat(diagnostics): export build identity`. The prefix helps review; it does not
trigger an automatic version bump. Mark incompatible changes with `!` or a
`BREAKING CHANGE:` footer and explain the user-visible migration.
[Conventional Commits](https://www.conventionalcommits.org/en/v1.0.0/) describes the syntax.

`bacon` watches the library typecheck by default. Its `parity` job runs the existing
small targeted KSP suite; explicit clippy and nextest jobs are also available. Do not
run the complete sample-library suite on every edit. For a focused regression:

```sh
cargo nextest run --release -E 'test(regression_name)'
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
separate version literal or a `moose.toml` override. The next planned release is
`0.2.0`; its changelog is explicitly unreleased until publication.

Follow [Semantic Versioning](https://semver.org/spec/v2.0.0.html). During `0.y.z`
development, use a deliberate minor bump for incompatible behavior or substantial
new capabilities and a patch bump for compatible fixes. After `1.0.0`, incompatible
public API/behavior changes require a major bump, compatible additions a minor bump,
and compatible fixes a patch bump. A commit does not automatically increment the
package version. The release maintainer chooses the next version after reviewing
the accumulated changes, updates the changelog, and includes both in the release
preparation commit:

```sh
python3 tools/version.py set 0.2.0 --write
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

Nightlies are prereleases such as `0.2.0-nightly.20261002.g012345abcdef`. The date
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
nightly is not a new stable release and does not require a fabricated daily changelog.
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
