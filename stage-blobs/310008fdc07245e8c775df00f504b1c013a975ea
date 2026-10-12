# W0 native integration batch 0.3.355

This batch fixes blank first editor frames, UVI discovery and metadata-only cataloging, scan snapshot settling, supported processor modulation and EQ control routing, Mapping navigation, positive script waits, Articulations heading duplication and Nightly provenance. Eleven reviewed logical defects advance the existing ledger from 0.3.344 to 0.3.355. Browser publication tests and diagnostic documentation add no version count.

## Source and integration checks

Runtime-tested source: `01680642beed28690fc7bfdfbfb3286406bdbf1f`. The final release-source commit adds only this report and ledger validation. Owner source SHAs and narrower test limits are retained in `release-fixes.json` and their individual reports.

- Merged ci/shots workspace at `f109799e`: 1,868 passed, zero failed, 218 ignored, including doctests. That suite preceded the separately validated UVI directory catalog change.
- Final merged root ci/shots no-run passed; nine discovery/catalog regressions passed (one installed-filesystem case ignored), UVI area 57 passed/six ignored, feature-off UVI no-run passed.
- Actual Vulkan GPU output: both fresh hidden-subtree fixtures produced opaque first and resized frames; changed/full-redraw assertions passed. All 24 partial/full redraw comparisons had zero differing pixels. Native Windows DAW scanout remains unverified.
- Native quick: 72/72 loaded, 68 audible, zero nonfinite samples and zero reported audio-thread allocations. Seventy matched 0.3.344 Kontakt witnesses retained the same stage and audibility. Four silent covered-note cases and three stored long releases remain disclosed.
- Two authored clear loose UVI/WAV fixtures produced audible samples. The scripted loose fixture uses the corpus harness's non-scripted streamed route and reports `Lua has no frontend`; it is not Lua runtime evidence. The separate UVI area tests cover the scripted bank/runtime path. No new loose-program harness fix is included.

Installed UVI directory parsing cataloged 26/26 banks into four libraries and 660 entries with zero bank issues, without opening member payloads. Directory metadata success does not establish protected payload playback. No private UVI names, paths or content are committed.

## Provenance and release checks

Nightly version stamping explains the shipped 0.3.344 dirty flag. The source checker permits the exact deterministic stamp, compares tracked source and complete manifest content, handles CRLF, and records unexpected changes. Missing or dirty provenance warns and preserves the manifest instead of blocking Nightly downloads. Native Windows/macOS/Linux PR jobs exercise real Nightly stamping; their hosted results must be checked before publication.

Existing release-retention code already preserves every published snapshot and source tag. Historical pruning was removed before 0.3.344; this batch adds no duplicate fix and recreates no deleted release. Packaging retention/provenance fixtures passed.

Local CLAP, VST3 and CLI artifacts are frozen outside installed plugin directories, with clean source identity, exports/factory discovery, ABI checks and CLAP smoke checks recorded separately. Local ABI is not the shipping ABI: hosted Linux release assets must retain the glibc 2.35 baseline. W0 performs no installs.

Receipts: `~/.cache/kontakto-w0/registry-batch-20261009/`, backed by the workspace volume. `merged-gate.json` and `quick-verdict.json` retain exact scope; immutable artifact and publication receipts follow there. Timing/CPU/RSS/native parity remain unknown unless an owner report supplies an accepted quiet witness. Dolce callback slowness, poisoned-editor recovery and adaptive Kontakt envelope revisions are separate pending P0 lanes.
