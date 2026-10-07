# Kontakt legacy formats and containers

Status: implementation and measurements in progress, 2026-10-08.
Branch: `v2/gpt-format-legacy`, baseline `fb456b41`.

## Corpus boundary

A fresh signature census recursively visited `/mnt/MAIN_STORAGE/Libraries/Kontakt`,
following directory links once by canonical path, including folders absent from
`~/.cache/kontakto-corpus/items.tsv`. The recursive `Pacific Ensemble Strings/KONTRA
project recovery` directory was excluded, matching corpus-health's library boundary.
No library content, decoded presets, samples or access values were written.
Private measurement outputs live in `~/.cache/kontakto-gpt-format-legacy/`.

| Format / version | Files present | Load before | Load after | Remaining decoding |
| --- | ---: | ---: | ---: | --- |
| NKI / NIS | 781 | pending | pending | Per-record translation gaps measured separately |
| NKM / NIS | 53 | pending | pending | Per-program translation gaps measured separately |
| NKSN / NIS | 1,103 | pending | pending | Requires its parent NKI; snapshots are state, not instruments |
| NKR / directory archive | 12 | pending | pending | Resource access, not instrument playback |
| NICNT / metadata plus FileContainer | 9 | pending | pending | Resource access, not instrument playback |
| NKX / sample directory archive | 270 | pending | pending | Sample access, not instrument playback |
| NKI/NKM / NKS v1 XML | 0 | n/a | n/a | XML-to-IR translation has no installed reference fixture |
| NKI/NKM / NKS v2 XML | 0 | n/a | n/a | XML-to-IR translation and old monolith layout unverified |
| NKI/NKM / NKS 4.22+ binary | 0 | n/a | n/a | No installed standalone NKS fixture |
| NKI/NKM / NKS monolith | 0 | n/a | n/a | No installed monolith fixture |
| NKI/NKM / FileContainer monolith | 0 | n/a | n/a | Integration tested with authored fixtures |
| NKB / all versions | 0 | n/a | n/a | Bank semantics owned by format-gaps lead |
| NKP / all versions | 0 | n/a | n/a | No installed preset fixture; not necessarily an instrument |
| Big-endian presets / all versions | 0 | n/a | n/a | No installed fixture |

The existing cached list already contains every installed NKI/NKM. It omits the
snapshots, resources and sample containers above. Extensions NKG and NKZ were
also searched; none are present. Loose sample census: 495 WAV, 21,066 NCW and
1,673 OGG; no AIFF/AIF/AIFC.

## Evidence and ownership

- Vendored `vendor/ni-file/doc/containers/NKS.md`, `Monolith.md`, `FileContainer.md`
  and source readers are the available ni-file implementation evidence. Upstream
  ni-file is DMCA-blocked; all claims about its current behavior are **unverified**.
- [monomadic's hex templates](https://github.com/monomadic/hexfiend-templates):
  `Monolith.tcl` describes the FileContainer TOC; `NKS/ResourceBlock.tcl` describes
  the legacy resource directory. Template availability does not certify playback.
- Read-only v1 importer (`decipher-readers-v1/src/import.rs` and vendored readers):
  also rejects legacy monoliths and does not translate Kontakt v1/v2 XML.
- Read-only `t3code-80fe786b/docs/DSP_FORMAT_SPECIFICATION.md`, sections **NKS wrapper
  versions**, **Kontakt chunks and structured objects**, and the format-family
  inventory: compression dispatch, selected public fields and FileContainer
  distinction. Historical endian and monolith variants explicitly need verification.
- The gap-map lead owns linked script resources, save settings, quick browse,
  metadata and banks. This branch does not change those areas or
  `KONTAKT_FORMAT_GAPS.md`.

Recognition, raw extraction, translated/lowered load and native-host playback
parity are separate measurements. A parse-tier pass uses placeholder audio;
it does not establish embedded-sample access or sound parity.
