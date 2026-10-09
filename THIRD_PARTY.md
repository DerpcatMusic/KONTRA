# Third-party components

Project-authored code is offered under Apache-2.0 (see LICENSE and NOTICE).
Vendored and other third-party components retain their own terms. The
crate inventory was checked on 2026-10-02 with
`cargo deny --all-features list --format json` (cargo-deny 0.20.2) against
Cargo.lock at source snapshot `e531505`: 470 licensed package/version entries (including KONTRA) and
one unlicensed entry, `ni-file`. This includes development dependencies
and multiple platforms; it is not a per-binary bill of materials or
confirmation that every distribution obligation has been satisfied.

## Upstream metadata: vendor/ni-file

> **No explicit redistribution license found.** This vendored crate comes
> from [Ma5onic/ni-file](https://github.com/Ma5onic/ni-file), pinned at
> [commit 1b7a518243125857fddec8217167b47a35cb58fa](https://github.com/Ma5onic/ni-file/commit/1b7a518243125857fddec8217167b47a35cb58fa),
> a fork of monomadic/ni-file. The pinned [source tree](https://github.com/Ma5onic/ni-file/tree/1b7a518243125857fddec8217167b47a35cb58fa)
> contains no LICENSE, COPYING, or NOTICE file, and its
> [Cargo.toml](https://github.com/Ma5onic/ni-file/blob/1b7a518243125857fddec8217167b47a35cb58fa/Cargo.toml)
> has no license field. This records the upstream metadata; it does not
> assign Apache-2.0 to upstream code or claim that any instrument library
> is included in KONTRA.
> Local patches add modulation, archive, and resource decoding; they do
> not resolve the upstream licensing question.

**Redistribution permission remains unresolved.** A public repository is
not itself a redistribution license. Obtain a grant covering the upstream
authors' code and relevant fork contributions, or replace the dependency
with code whose redistribution rights are established. Neither this notice
nor KONTRA's Apache-2.0 license supplies that permission.
[GitHub's licensing guidance](https://docs.github.com/en/repositories/managing-your-repositorys-settings-and-features/customizing-your-repository/licensing-a-repository)
distinguishes public viewing/forking from a software license.

Native Instruments' [2024-04-04 takedown notice](https://github.com/github/dmca/blob/master/2024/04/2024-04-04-native-instruments.md)
targets `monomadic/ni-file`. On 2026-10-02, GitHub's API reported that
upstream as blocked for DMCA reasons. This records an allegation and a
hosting action, not a judicial determination. The fork's availability does
not resolve that dispute. See [the legal review](docs/LEGAL.md).

## Format references (not vendored)

The NKX/NKR archive layout was implemented with
[nkxtract](https://github.com/maxton/nkxtract) (`ca40dbf`) and
[unnks](https://github.com/JimiHFord/unnks) (`eb59538`) as references.
Both are **GPL-3.0**, and no files from them are included. The
encoded-offset constant in `vendor/ni-file/src/nkr/archive.rs` is the
same as nkxtract's.
The keystream in `crates/sampler-kontakt/src/access.rs` (the `library-access` feature) uses the
same algorithm and constants as nkxtract's `Nks.cs`.
The 2026-10-02 focused comparison of `Nks.cs::FileDecryptStream` and
`src/access.rs::Keystream::new` confirms the 64 KiB stream, LCG recurrence,
AES-encrypted counter and XOR construction. The Rust code combines the
operations per block and uses a wrapping big-endian integer counter;
the C# reference uses separate passes and a byte-array counter.
This is a limited algorithm comparison, not an authorship or clean-room
finding. A complete expression/provenance review remains outstanding.

Resolve the origin of any copied protectable expression before treating
these paths as Apache-2.0-only. If GPL-covered code was incorporated,
its applicable licensing and corresponding-source obligations must be
addressed; attribution alone is insufficient. This is a provenance
question, not a conclusion that matching format facts require GPL.
[GPL-3.0 text, sections 5 and 6](https://github.com/maxton/nkxtract/blob/ca40dbf/COPYING).

## Vendored and patched

| Component | Where | License | Notes |
|---|---|---|---|
| MOOSE (fork of truce) | git dependency `Matari-Audio/moose` rev `bffa467`; `vendor/moose-*` | Truce License 1.0 (`LicenseRef-TruceLicense-1.0`): MIT or Apache-2.0 plus a framework rider | See below. Patches: `vendor/MOOSE-PATCHES.md`. |
| mui-baseview | `vendor/mui-baseview` | MIT | Copyright Matari Audio. |
| MUI | git dependency `Matari-Audio/MUI` | MIT | |
| vello 0.10.0 | `vendor/vello` | Apache-2.0 OR MIT | linebender/vello; patches in `vendor/vello/PATCHES.md`. |
| ncw 0.4.0 | `vendor/ncw` | MIT OR Apache-2.0 | monomadic/ncw `75af0c0`. Test data in `vendor/ncw/tests/data` is part of that crate. |
| Noto Sans | `assets/NotoSans.ttf` | SIL Open Font License 1.1 | Full text in `assets/OFL.txt`. |
| BUFFR support integration | `src/support.rs`, `src/support/` | ISC | Adapted from owner-maintained BUFFR commit `fd2fdba92f3f71cee24c0c72a39aa190fc9ee414`; notice and integration scope in `src/support/LICENSE-BUFFR` and `src/support/SOURCE.md`. |
| buffr-durable-file, derpcat-flight-recorder | `vendor/buffr-durable-file`, `vendor/derpcat-flight-recorder` | ISC | Same BUFFR source checkpoint; each crate retains its own license. |

### The Truce License rider

The Truce License is MIT or Apache-2.0 at your option, except that
offering truce (or a derivative) **as a commercial plug-in framework or
framework service to other developers** needs written permission
(section 2.1). Section 2.2 names audio plug-ins and plug-in suites as
not covered, so KONTRA, an audio plug-in, has the plain MIT/Apache-2.0
grant. The rider would matter only if the vendored `moose-*` crates were
split out and sold as a framework. The name "truce" may not be used for
KONTRA (section 3), and it is not.

### Plug-in interfaces

CLAP support uses `clap-sys` (MIT OR Apache-2.0) through MOOSE. MOOSE's
VST3 implementation includes its own native shim with SDK interface/layout
references; a Cargo-only inventory cannot independently verify the origin
of that native expression. Steinberg currently offers VST3 SDK 3.8 onward
under MIT, with copyright/notice conditions; this is not an automatic
license override for code taken from older SDK versions. Preserve origin
and version records for future SDK/interface changes and review the shim
as part of the native-component audit.
[Steinberg's licensing guidance](https://steinbergmedia.github.io/vst3_dev_portal/pages/FAQ/Licensing.html).
The project does not build a VST2 variant.

## Dependency inventory (all sources)

The following groups exclude KONTRA itself; multiple versions count as
separate entries. Source-specific components are described above.

| License | Crates |
|---|---|
| MIT/Apache-2.0 license-ID groups (including mixed expressions; see cumulative terms below) | 338 package/version entries |
| MIT | 84 crates |
| LicenseRef-TruceLicense-1.0 | 17 `moose-*` crates (above) |
| Apache-2.0 | cpal, hound, moose-font, gethostname, gl_generator, glutin_wgl_sys, khronos_api, spirv, codespan-reporting, unicode-linebreak |
| MPL-2.0 | symphonia, symphonia-core, symphonia-codec-pcm, symphonia-format-riff, symphonia-metadata, option-ext |
| MIT OR Unlicense | memchr, same-file, walkdir, winapi-util |
| Zlib | foldhash, slotmap |
| BSD-3-Clause | tiny-skia, tiny-skia-path |
| BSL-1.0 | clipboard-win, error-code |
| BSD-2-Clause | arrayref |
| ISC | libloading, buffr-durable-file, derpcat-flight-recorder, rustls-webpki, untrusted |
| CDLA-Permissive-2.0 | webpki-roots 1.0.9 trust-anchor data; complete agreement is included in generated dependency notices |
| CC0-1.0 | hexf-parse |

The discovery table groups license IDs and overlaps; it does **not**
preserve complete SPDX expressions. In particular, these are cumulative
obligations, not additional alternatives:

| Package | Declared expression |
|---|---|
| dpi 0.1.2 | Apache-2.0 AND MIT |
| encoding_rs 0.8.42 | (Apache-2.0 OR MIT) AND BSD-3-Clause |
| unicode-ident 1.0.26 | (MIT OR Apache-2.0) AND Unicode-3.0 |
| ring 0.17.14 | Apache-2.0 AND ISC |

Use the generated package inventory and texts, not this grouped table,
for distribution. The separate cargo-about scan at that snapshot includes 482 package
entries across all features/platforms, including build/development crates;
its graph differs from cargo-deny's grouped inventory above.

MPL-2.0 is file-level copyleft. `vendor/symphonia-format-riff` contains
Symphonia 0.5.5 sources with a PCM WAVE format-boundary patch; modified
files retain MPL-2.0 notices and are published with this repository.
Provenance and patch scope: `vendor/symphonia-format-riff/PATCHES.md`.
Other Symphonia crates are used unmodified from crates.io. Binary distribution
also requires informing recipients how to obtain the covered source;
using unmodified crates does not remove that obligation.
[MPL-2.0, sections 3.1 and 3.2](https://www.mozilla.org/en-US/MPL/2.0/).
Regenerate this list with
`cargo deny --all-features list`.

## Distribution notices and assets

Keep LICENSE, NOTICE, this file and the applicable third-party copyright
and license texts with redistributed material. Linux and Windows nightlies
consolidate the complete root notices, legal review, dependency notices, MOOSE
rider and NOTICE, MUI and BUFFR texts, and font licenses in `LICENSES.txt`.
The original paths identify sections of this file rather than separate files.
The release asset `KONTRA-nightly-covered-source.zip` supplies the MPL dependency
source archives; `release-manifest.json` records their checksums. macOS retains
its license bundle and covered source inside the universal installer. The patched
`symphonia-format-riff` archive contains the current vendored source;
other MPL archives are copied unchanged from the Cargo registry.
The `.crate` files are gzip-compressed tar archives containing the
preferred source; extract them with a tar-compatible archive tool.
Distribute the modified covered source for local MPL patches instead
of only an upstream archive.

Generate the same bundle for a manual release:

```sh
cargo install --locked --version 0.9.2 --features cli cargo-about
python3 tools/licenses.py --self-test
python3 tools/licenses.py --output license-bundle
```

The nightly staging script consolidates `license-bundle/` and the root notices
into `LICENSES.txt`, and publishes its covered sources separately. Manual
packages may retain the original `licenses/` layout alongside all root notices,
font licenses and legal review. `about.toml` chooses among
permitted alternatives without dropping `AND` obligations. No override
assigns a license to `ni-file`: that known gap remains explicit, while
other dependencies with missing license text fail bundling. Generating
notices is not permission or a complete legal/provenance audit. Review
embedded/native dependencies and changes to Cargo.lock before publishing.

The NCW/WAV files under `vendor/ncw/tests/data` are upstream codec
fixtures, not a supplied Kontakt instrument library. The crate advertises
MIT/Apache-2.0 licensing; this review has not independently established
the authorship and redistribution rights of each audio recording.
Preserve their upstream provenance and verify it for source redistribution.
