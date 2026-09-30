# Third-party components

KONTRA is Apache-2.0 (see `LICENSE` and `NOTICE`). It builds on the
components below. Crate licenses were listed with
`cargo deny --all-features list` (cargo-deny 0.20.2) against `Cargo.lock`.

## Unresolved: `vendor/ni-file`

> **No license.** `vendor/ni-file` has no license file and no `license`
> field. It was vendored from [Ma5onic/ni-file](https://github.com/Ma5onic/ni-file)
> (revision `1b7a518243125857fddec8217167b47a35cb58fa`), a fork of
> `monomadic/ni-file`, which also has no license. The original repository
> is blocked on GitHub after a DMCA takedown notice from Native
> Instruments: [github/dmca 2024-04-04-native-instruments](https://github.com/github/dmca/blob/master/2024/04/2024-04-04-native-instruments.md).
> Without a license grant, the upstream code is all rights reserved by
> its author(s), and the takedown shows that Native Instruments objects to
> it. **Resolve this before any public release:** get a written license
> from the author(s), or replace the crate with a clean-room parser.
> Local changes to it (modulation, archive and resource decoding) are
> ours.

## Format references (not vendored)

The NKX/NKR archive layout and the resource keystream were implemented
from the descriptions in [nkxtract](https://github.com/maxton/nkxtract)
(`ca40dbf`) and [unnks](https://github.com/JimiHFord/unnks) (`eb59538`).
Both are **GPL-3.0**. No source files were copied. The encoded-offset
constant in `vendor/ni-file/src/nkr/archive.rs` and the keystream in
`src/access.rs` implement the same algorithm, so a lawyer should confirm
that they are not derivative works before a public release.

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

MPL-2.0 is file-level copyleft: the symphonia sources are used
unmodified from crates.io; if they are ever modified, those files must
stay MPL-2.0 and be published. Regenerate this list with
`cargo deny --all-features list`.
