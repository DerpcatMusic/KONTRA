# Third-party components

Project-authored code is offered under Apache-2.0 (see LICENSE and NOTICE).
Vendored and other third-party components retain their own terms. The
crate inventory was produced with cargo-deny --all-features list
(cargo-deny 0.20.2) against Cargo.lock.

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

## Format references (not vendored)

The NKX/NKR archive layout was implemented with
[nkxtract](https://github.com/maxton/nkxtract) (`ca40dbf`) and
[unnks](https://github.com/JimiHFord/unnks) (`eb59538`) as references.
Both are **GPL-3.0**, and no files from them are included. The
encoded-offset constant in `vendor/ni-file/src/nkr/archive.rs` is the
same as nkxtract's.
<!-- private:start -->
The keystream in `src/access.rs` (the `library-access` feature) uses the
same algorithm and constants as nkxtract's `Nks.cs`.
<!-- private:end -->
These documented overlaps do not by themselves conclude that the
implementation is a derivative work. A broader line-by-line source
comparison has not been recorded.

## Vendored and patched

| Component | Where | License | Notes |
|---|---|---|---|
| MOOSE (fork of truce) | git dependency `Matari-Audio/moose` rev `bffa467`; `vendor/moose-*` | Truce License 1.0 (`LicenseRef-TruceLicense-1.0`): MIT or Apache-2.0 plus a framework rider | See below. Patches: `vendor/MOOSE-PATCHES.md`. |
| mui-baseview | `vendor/mui-baseview` | MIT | Copyright Matari Audio. |
| MUI | git dependency `Matari-Audio/MUI` | MIT | |
| vello 0.10.0 | `vendor/vello` | Apache-2.0 OR MIT | linebender/vello; patches in `vendor/vello/PATCHES.md`. |
| ncw 0.4.0 | `vendor/ncw` | MIT OR Apache-2.0 | monomadic/ncw `75af0c0`. Test data in `vendor/ncw/tests/data` is part of that crate. |
| Noto Sans | `assets/NotoSans.ttf` | SIL Open Font License 1.1 | Full text in `assets/OFL.txt`. |

### The Truce License rider

The Truce License is MIT or Apache-2.0 at your option, except that
offering truce (or a derivative) **as a commercial plug-in framework or
framework service to other developers** needs written permission
(section 2.1). Section 2.2 names audio plug-ins and plug-in suites as
not covered, so KONTRA, an audio plug-in, has the plain MIT/Apache-2.0
grant. The rider would matter only if the vendored `moose-*` crates were
split out and sold as a framework. The name "truce" may not be used for
KONTRA (section 3), and it is not.

## Crates from crates.io

| License | Crates |
|---|---|
| Apache-2.0 OR MIT (some also Zlib, BSD, 0BSD, LLVM-exception, Unicode-3.0 or LGPL-2.1-or-later alternatives) | 336 crates |
| MIT | 84 crates |
| LicenseRef-TruceLicense-1.0 | 17 `moose-*` crates (above) |
| Apache-2.0 | cpal, hound, moose-font, gethostname, gl_generator, glutin_wgl_sys, khronos_api, spirv, codespan-reporting, unicode-linebreak |
| MPL-2.0 | symphonia, symphonia-core, symphonia-codec-pcm, symphonia-format-riff, symphonia-metadata, option-ext |
| MIT OR Unlicense | memchr, same-file, walkdir, winapi-util |
| Zlib | foldhash, slotmap |
| BSD-3-Clause | tiny-skia, tiny-skia-path |
| BSL-1.0 | clipboard-win, error-code |
| BSD-2-Clause | arrayref |
| ISC | libloading |
| CC0-1.0 | hexf-parse |

MPL-2.0 is file-level copyleft. `vendor/symphonia-format-riff` contains
Symphonia 0.5.5 sources with a PCM WAVE format-boundary patch; the modified
files retain MPL-2.0 notices and are published with this repository.
Provenance and patch scope: `vendor/symphonia-format-riff/PATCHES.md`.
Other Symphonia crates are used unmodified from crates.io. Regenerate this list with
`cargo deny --all-features list`.
