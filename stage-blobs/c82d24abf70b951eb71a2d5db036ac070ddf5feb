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

## Implemented fields and behavior

FileContainer uses the existing TOC decoder: u64 member index, 600-byte UTF-16
filename slot, cumulative member end offsets and the validated file-section
base. The instrument reader now opens a unique NKI/NKM/NKB member with a
128 MiB preset bound, instead of rejecting the entire container or limiting
its sample section to 128 MiB. A member must itself be a supported NI preset
container; arbitrary byte scanning is not used to guess preset boundaries.
Multiple preset members fail explicitly because no active-member selector is
established. Recursive preset wrappers are limited to four levels.

Embedded samples are resolved to bounded physical `(container, offset, length)`
sources. Authored `|` and backslash separators normalize to `/`. A saved external
path can resolve by an exact member suffix or a unique basename. Ambiguous,
duplicate and escaping member names fail explicitly. Resident decode, frame
counts and random-access streaming use the same source; no files are extracted.
Only the outer FileContainer sample directory is indexed; nested sample-bearing
containers need an explicit nesting/identity model before claiming support.

The shared sample reader now accepts AIFF and AIFC PCM alongside WAV and NCW.
It decodes FORM/COMM/SSND sizes, channels, frame count, left-justified signed
integer PCM (1–32 bits), the IEEE extended sample-rate field, SSND byte offset,
chunk padding and either COMM/SSND order. `NONE`/`twos`, little-endian `sowt`
and big-endian `fl32`/`FL32` are accepted. Other AIFC codecs, including float64,
are refused. Metadata traversal is limited to 10,000 chunks. The random-access
reader skips sample bodies to reach a later COMM. AIFF's own MARK/INST loops
remain unmodeled; Kontakt's preset zone loops remain the playback authority.
Evidence: Apple's [AIFF specification](https://ich.music.mcgill.ca/classes/synth/AudioIFF1.2.1/AudioIFF1.2.1.html),
**Common Chunk**, **Sound Data Chunk**, **Sample Points** and **File Structure**.
AIFC codec identifiers also match the already-vendored Symphonia AIFF reader.

Legacy NKS extraction now honors the V1 absolute zlib offset, including padding
between header and stream, and bounds expanded v1/v2 XML and v4.22+ binary
presets to 128 MiB by default. An explicit `decompressed_preset_bounded(limit)`
API permits a smaller worker budget. Malformed XML UTF-8 returns an error
instead of panicking. This is extraction support, **not** an XML-to-IR translator.
Synthetic XML fixtures test byte extraction only; they are not claimed to be
Kontakt-authored semantic fixtures. The existing old NKS-monolith rejection
remains, since the patch-member envelope is not described well enough in the
available resource-tree template and no installed witness exists.

## Runnable checks and measurement commands

All cargo commands use `~/.cache/kontakto-heavy` without slot overrides.
The baseline binary is preserved privately before reader changes. The
coordinator authorized the already-running baseline outside the wrapper.
Future corpus jobs use the wrapper in resumable shards lasting at most five
minutes; each shard releases its slot before the next begins.

- `cargo test -p sampler-kontakt --test monolith`: authored NKS binary presets
  wrapped in FileContainers, WAV/AIFF/AIFC/NCW resident and random-access audio,
  audible runtime render, sparse multi slots, large outer containers, missing
  or ambiguous preset members, ambiguous samples and malformed AIFF.
- `cargo test --manifest-path vendor/ni-file/Cargo.toml --test legacy`:
  V1 padded zlib offset, V2 zlib, exact expansion budgets and malformed UTF-8.
- `legacy_health /mnt/MAIN_STORAGE/Libraries/Kontakt`: per-file authoring
  version from the NIS sound header; snapshot state parse; complete named
  resource reads for NKR/NICNT; NKX directory indexing. Its TSV contains only
  filenames, header versions, counts and errors. It writes no payloads.
- `corpus-health run BEFORE.jsonl --tier parse --workers 4 /mnt/MAIN_STORAGE/Libraries/Kontakt`
  and the same command for AFTER: explicit roots bypass cached items and avoid
  modifying the shared corpus cache. They test every installed NKI and NKM,
  with dummy sample audio, script compile and IR lowering.

Snapshot/resource results are separate from instrument load counts: a snapshot
is state for a parent instrument, a NICNT/NKR is resources and an NKX is sample
storage. Supplemental probe success is not a promise of independent playback.
