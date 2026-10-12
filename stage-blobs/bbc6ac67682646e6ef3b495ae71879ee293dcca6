# ni-file provenance and format coverage audit

Research snapshot: 2026-10-08 local date; final checkout inspection at 2026-10-07 22:49 UTC. This is a source audit, not a successful-build or playback certification. No cargo, rustc, tests, clippy, proprietary plugins, key discovery, or encrypted-content extraction were executed. Only this report was authored in the task workspace.

## Scope, identities, and evidence conventions

| Location | Inspected identity | State |
| --- | --- | --- |
| `R`: `/home/derpcat/.t3/worktrees/KONTAKTO/t3code-80fe786b` | `0cb7a8a0b4d43086596a64c77320caa1b26d6d98` | Initially two unrelated untracked DSP reports; neither was edited |
| `V`: `/home/derpcat/.t3/worktrees/KONTAKTO/decipher-readers-v2` | `2fb8c926dd39bb7ac26a84d4806f42de14b6630e` | Clean initially; concurrent reader edits appeared during research, detailed below |
| Recorded source export marker in V | `SOURCE_COMMIT`: `1f1836a156f96ee8e18652e771c49c2d6744cd4c` | A provenance marker, not V's current HEAD |
| Actual ni-file upstream fork fetched read-only into `/tmp/ni-audit-upstream.PIqOQo` | `1b7a518243125857fddec8217167b47a35cb58fa` | `Ma5onic/ni-file`; shallow clone HEAD matches the recorded pin |
| Templates fetched into `/tmp/ni-audit-templates.POu0in` | `c6f309bae04a03967b94f54d81dc2050f827a1e8` | `monomadic/hexfiend-templates`; pinned before comparisons |

`R:path:line` and `V:path:line` below identify different checkouts. V evidence refers to the **base commit** above unless explicitly called live work. Recover stable cited spans with `git -C <V> show 2fb8c926:<path>` because concurrent edits can move line numbers. Paths beginning `vendor/ni-file/src/` belong to the vendored library; paths beginning `crates/sampler-` belong to KONTRA V2.

Root graph was queried before source reads: `graft ask "Where are ni-file vendor provenance, schema container readers, and import coverage defined?" --source`, then `graft skeleton vendor/ni-file/src/lib.rs`. The latter reports no indexed definitions, so vendor reads used normal searches. V has no graft index. The graph's ranked answer concerned the creator rather than vendor coverage and was not treated as vendor evidence.

## Exact upstream provenance

This is **not** an unmodified checkout of the original `monomadic/ni-file` repository. `R:THIRD_PARTY.md:12–24` records the immediate source as [Ma5onic/ni-file at 1b7a518](https://github.com/Ma5onic/ni-file/tree/1b7a518243125857fddec8217167b47a35cb58fa), a fork of monomadic's repository. A successful read-only clone independently confirmed that exact HEAD, author `monomadic`, commit subject `update:`, and commit date 2023-12-20. The original monomadic repository was previously reported HTTP 451; **it was not fetched in this audit**. A web open of the fork tree failed, but its [pinned Cargo metadata](https://github.com/Ma5onic/ni-file/blob/1b7a518243125857fddec8217167b47a35cb58fa/Cargo.toml) and git clone succeeded.

The original fork Cargo manifest names `ni-file` version `0.0.1`, author `monomadic`, edition 2021, and NCW dependency `0.1.2`. Local `R:vendor/ni-file/Cargo.toml:1–29` retains that package identity, replaces NCW with `../ncw`, adds `encoding_rs` and optional serde, and has no repository/revision/license field. The root Cargo dependency uses a local path; Cargo's package version is therefore **not** a source commit pin or a measured support version. The source pin lives in project provenance documentation, not Cargo resolution.

Local git history provides the rest of the chain: `e0378815192bb153efd827922950afff98ec9947` imports the vendored directory in the public project's initial commit; older local histories also contain the vendor in baseline `5d2abd6f8996fa4b9207e85fac1e2bcc5c8c7285`. It is a tracked tree, not a git submodule. Later project commits add modulation, archive decoding, writing, and compatibility fixes. Root contains 164 Rust/Markdown vendor files, of which **112 are byte-identical to files at the upstream pin**. The remaining files include local changes/new code; identical crate versions do not establish identical readers.

The fetched pin has no explicit LICENSE/COPYING/NOTICE or Cargo license field; this matches the existing project notice. This is a provenance observation, not a new license grant or an authorization change. Do not copy upstream fixture assets into distributed project tests merely because the fork is publicly readable.

## Version axes: what the code actually distinguishes

Plugin major version, authoring application metadata, app signature, minimum compatible version, container revision, object serializer revision, and reader support are separate axes. Build numbers form another field. They must not be collapsed into one `Kontakt version` column.

| Axis | Evidence and exact behavior | What cannot be inferred |
| --- | --- | --- |
| Installed plugin/application release | No proprietary executable or plugin was inspected or run | File metadata does not establish the installed plugin release or its implementation |
| NIS authoring application/version | `V:vendor/ni-file/src/nis/properties/preset.rs:25,69,97`: app ID `2` = Kontakt; UTF-16 version string read separately | `Kon5` alone does not supply a complete authoring version string |
| Patch/application version bytes | `V:vendor/ni-file/src/kontakt/objects/header.rs:153,245`: four byte components; comment explicitly warns patch version can exceed creating app version | A final component `255` is not the release build number |
| App signature | `header.rs:160,252` reverses four stored bytes; base schema dispatch `schemas/preset.rs:36` selects `Kon4`, `Kon5`, `Kon6`, `Kon7` for NKI | A signature is a schema discriminator, not proof of coverage for every release with that major number |
| Minimum compatible version | `header.rs:176,268`; `KontaktPatch::preset` forwards it, but `schemas/preset.rs:32` names the argument `_version` and ignores it | Current typed schema selection does not enforce minimum-version compatibility |
| Build/SVN revision | `header.rs:201–203,300–304`: separate `svn_revision`, checksum, and expanded-size fields | Do not derive the full build from the four-byte patch version or a fixture filename |
| NKS outer header family | `header.rs:23–30`: format words `0..=255` → V1; `256..=271` → V2; `>=272` → V42 | `NKSv2` does not mean Kontakt application major 2; the broad `>=272` branch is not evidence for every future revision |
| NIS framing/repository version | Item header, data layer, child table must each have revision **1** (`nis/container/header.rs:45`, `data/item_data_header.rs:23`, `container.rs:151`); RepositoryRoot separately decodes packed major/minor/patch (`properties/repository_root.rs:27,58`) | `NISound 1.3.0` is not Kontakt 1.3, nor is it the item framing revision |
| Object serializer version | `StructuredObject.version: u16`; each decoder has its own supported set | Program `0xae`, zone `0x98`, LFO `0x73`, and source `0x106` need not change together |
| ni-file reader support | Local package `0.0.1` plus vendored source changes and two different local revisions | README percentages or crate version do not measure playable compatibility |

### Exact observed cross-axis examples

These examples were read from **clear fixed-size headers only** in the fetched fork's fixture files using Python `struct`, not by executing plugins or unpacking encrypted subtrees. Header magic was checked, and monolith values were validated as 0/1. Offsets within a full NKS-style header were format word +8, patch version +16, reversed signature +20, monolith +42, minimum version +46, SVN revision +178 for V42. Embedded NIS header offsets are included so these observations are reproducible. Filenames are corpus labels; the table states actual byte values.

| Upstream fixture under `tests/data/` | Header offset / word | Signature / patch version bytes | Minimum compatible bytes | Additional evidence |
| --- | --- | --- | --- | --- |
| `Containers/NKS/MonolithV2/2.1.0.001-000.nki` | 0 / `0x100` | `Kon2` / `2.1.0.1` | `2.0.9.9` | Monolith flag 1; reader rejects legacy monolith body |
| `Containers/NKS/KontaktV2/kokiriko_all_fx.nki` | 0 / `0x100` | `Kon3` / `3.5.0.25` | `3.0.9.14` | Monolith flag 0; V2 **header family** with app signature Kon3 |
| `Containers/NKS/KontaktV2/KontaktV2-000-empty.nki` | 0 / `0x100` | `Kon4` / `4.0.0.255` | `3.9.9.255` | Monolith flag 0; directory name does not identify app major |
| `Containers/NKS/KontaktV42/4.2.2.4504-000.nki` | 0 / `0x110` | `Kon4` / `4.2.2.255` | `4.2.0.255` | Binary-era header |
| `Containers/NKS/KontaktV42/4.2.4.5316-000.nki` | 0 / `0x110` | `Kon4` / `4.2.4.255` | `4.2.3.255` | Separate SVN field = **5316** |
| `Containers/NIS/files/kontakt/5.0.2/musical_saw.nki` | 2656 / `0x110` | `Kon5` / `5.0.2.255` | `4.9.0.255` | NIS wrapper at this corpus path; not evidence that all 5.0 files are NIS |
| `Containers/NIS/files/kontakt/5.3.0.6464/000-did.nki` | 28361 / `0x110` | `Kon5` / `5.3.0.255` | `5.2.8.255` | Separate SVN field = **6464** |
| `Containers/NIS/files/kontakt/5.8.1.43/5.8.1.43-ncw.nki` | 15724 / `0x110` | `Kon5` / `5.8.1.255` | `5.8.0.0` | Patch and minimum versions differ |
| `Containers/NIS/files/kontakt/6.2.2.51/002-tib.nki` | 2327 / `0x110` | `Kon6` / `6.2.2.255` | `6.2.2.0` | Separate SVN field = **51** |
| `Containers/NIS/files/kontakt/7.1.3.0/000-default.nki` | 1915 / `0x110` | `Kon7` / `7.1.3.255` | `7.1.0.0` | Minimum version is not the patch version |

All rows are pinned by the [upstream fixture tree](https://github.com/Ma5onic/ni-file/tree/1b7a518243125857fddec8217167b47a35cb58fa/tests/data/Containers); the parser layouts are in [upstream header.rs](https://github.com/Ma5onic/ni-file/blob/1b7a518243125857fddec8217167b47a35cb58fa/src/kontakt/objects/header.rs). These are observations about specific files, not release-wide guarantees. No Kon8 fixture/header correspondence was established from this upstream corpus. The later [in-place corpus ledger](../artifacts/ni-file-versioned-corpus-2026-10-08/manifest.json) does establish **installed** Kon8 NIS templates: `Empty.nki` header `8.6.1.255`/minimum `8.6.0.0`, and `Kontakt Controls.nki` header `8.7.0.255`/minimum `8.5.1.0`. Compressed inner objects were not expanded in that census. These headers do not establish a typed Kon8 schema or release-wide playback support.

## Coverage matrix

`Partial` means specific fields/layouts are decoded; `raw` means framing preserves bytes without establishing meaning. The rendering column concerns `ni-file` itself; V2's conditional playback is described separately. No row means complete plugin equivalence.

| Format/profile | Recognition | Outer framing / extraction | Typed metadata/preset | Sample access | Raw roundtrip writing | Semantic editing | Audio/DSP rendering |
| --- | --- | --- | --- | --- | --- | --- | --- |
| LE NKS V1 / legacy XML | Magic recognized | V1 header; zlib extraction | Header fields + XML **string**, no XML semantic model | No XML-zone-to-PCM importer in ni-file | No NKS writer | No decoded legacy instrument editor | None |
| LE NKS V2, non-monolith, including Kon2/3/early Kon4 | Magic recognized | V2 header; zlib extraction | Header + XML string; invalid UTF-8 can panic at base | External sample layout remains inside XML; no integrated sample loader | No NKS writer | None | None |
| Legacy NKS monolith | Outer signatures/header recognized | `NKSContainer::read` explicitly errors when V2/V42 monolith flag is set | Body not decoded | No embedded sample mapping | None | None | None |
| LE NKS V42 binary preset | Recognized | V42 header, FastLZ expanded-size checks, metadata footer | NKI signatures Kon4–7 dispatched; Program/file tables/groups/zones partial | File references only; NCW handled by sibling crate | Chunk-stream writer exists; **outer NKS writer absent** | Selected record writers, not complete NKI regeneration | None |
| NIS Kontakt single preset, Kon5/6/7 | Validates generic NIS body | Item/layer/child framing revision 1; clear compressed/uncompressed subtree extraction; caller-provided access interface when required | Partial header, authoring metadata, Program and records; base generic typed dispatcher only Kon4–7 NKI | Sample references; no automatic PCM loading in ni-file | Yes: raw ItemContainer with UUID, reserved word, descriptors, unknown layers/properties and trailing bytes | Narrow codecs only; no automatic sound-header/checksum rebuild | None |
| Modern object layouts / installed Kon8 templates | Generic NIS/chunks recognized; Kon8 is observed in installed header signatures, with no distinct outer magic | Same raw framing; filename-table v3 and newer array/source/snapshot records implemented | Partial modern records; base Kon8 dispatch panics, live change preserves Unsupported; no full major-version profile proven | Sample references plus source-mode identity; no wavetable/stretch engine in ni-file | Raw NIS/chunks preserve opaque data | v3 filename table has no lossless semantic writer; some modern source/snapshot codecs exist | None |
| NKM / banks / groups / preset chunks | Patch types 0–5 recognized; NIS AppSpecific can expose raw body | Bank/slot/program containers and some direct multi reader APIs exist | Base `KontaktPreset::read` does **not** dispatch non-NKI; V2 bypasses it for multis | V2 can derive sample names from binary multi programs | Raw NIS/chunks only | No universal NKM/NKB/NKG/NKP editor | None |
| Modern FileContainer monolith | Recognized | TOC + member ranges; **V already adds bounded read_member and framing validation** | Member names/sizes/indices; skipped fixed headers remain uninterpreted | V can return member bytes; V2 importer still rejects this NIFile variant | No outer writer; skipped header bytes cannot be reconstructed from this model | None | None |
| NKX/NKR resource/archive | NIFile detection can return a tag or reject a standalone archive; use `nkr::Archive` directly | Directory/member versions `0x110` and `0x111`; member signatures select 22/27/31-byte headers; lazy validation | Directory filenames, member offsets/sizes, flags; not resource contents' semantics | Clear member bytes available; encoded member support is conditional on caller-provided access data; unsupported legacy cipher errors | No archive writer | None | None |
| NCW | NIFile returns an empty marker variant | Actual codec is `vendor/ncw`, not that NIFile variant | Header/channel/bit-depth/block metadata in sibling codec | PCM/float-bit decoding and block access in NCW crate | PCM16/24 writer and template-preserving rewrite in sibling codec; float writer absent | Audio replacement within codec's supported constraints | Codec decoding only; not Kontakt synthesis |
| FM8 raw `FM8E` / NIS-wrapped FM8 | Magic or generic NIS recognized | NIFile FM8 variant is a marker; separate exploratory FM8 reader exists | Reader prints fields and returns unit `FM8Preset`; versions below `0xd0` explored, newer versions panic | Not an FM synthesis implementation | Generic NIS wrapper only; no FM8 body writer | None | None |
| Reaktor / Maschine / Battery / other NI NIS applications | Some domains/app IDs recognized; unknown domain/item IDs retained | Generic NIS framing only where revision 1 and valid inheritance chain apply | Reaktor enum variants, authoring-app labels and generic properties do not implement product schemas | No general app-specific sample graph | Raw generic NIS writing where framing is accepted | None | None |
| RIFF NKSF / Kore / caches | NKSF module is skeleton; Kore enum unselected; NICache recognized as marker | No active NKSF parser; no cache body reader | None | None | None | None | None |
| Big-endian historical files | Several BE magics recognized by detection | NKSContainer accepts only listed LE forms; other readers use LE field reads | No demonstrated BE semantic decoding | None | None | None | None |

Matrix evidence: `R:vendor/ni-file/src/nifile/mod.rs:11–93`; `V:vendor/ni-file/src/nks/container.rs:25–160`; `V:vendor/ni-file/src/kontakt/schemas/kon1.rs:11`, `kon2.rs:12`; `V:vendor/ni-file/src/file_container/mod.rs:21–125`; `V:vendor/ni-file/src/nkr/archive.rs:29–93,119–129,225–249`; `V:vendor/ni-file/src/nksf/mod.rs:1`, `nksf/riff.rs:1–48`; `V:vendor/ni-file/src/fm8/mod.rs:16–35,919`; `R:vendor/ncw/README.md:13–63` and `reader.rs:97–123`. Raw NIS writing contracts are at `V:vendor/ni-file/src/nis/container/container.rs:58–99` and `data/mod.rs:18–46`.

## Exact object support and opaque boundaries

| Record | Evidenced reader versions / limits | Preserved but not fully interpreted |
| --- | --- | --- |
| Program `0x28` | Common prefix decoded without a supported-version whitelist; comment calls versions `0x80..0xaf` known. Base Program private decoder only attempts `0x80`; live replacement correctly returns unsupported | All raw private/public buffers and child chunks; public suffix after category fields; resource/wallpaper/snapshot path fields not decoded |
| Group / zone | V2 borrowed Group view only `0x95`; borrowed Zone view `0x95`, `0x98`, `0x9a`; vendored prefix readers are broader assumptions. `0x9a` adds six bytes before filename ID | Group private state; zone metadata/extra prefix bytes; unknown child chunks |
| Filename table `0x4b` | FNTableImpl versions **2 and 3**; lossless editable FNTableRecord only **2** | v2 timestamp bits, u32 unknown records, segment encodings and tail retained by record codec; v3 convenience view discards per-entry 8-byte/20-byte metadata |
| Legacy filenames `0x3d` | Separate FileNameListPreK51 reader, no modern version marker | Joined path/calendar-date view is not a native lossless editable record |
| Slot arrays | Generic modulation array code accepts unstructured `0x10`, `0x12`, `0x13`; v13 carries explicit count; external slots support 32/64 | Child slots retain raw chunks. FX header comment lists `0x11`, but implementation rejects it: documentation is not a support contract |
| AHDSR `0x3f` | Unstructured `0x11`; at least 52 trailing bytes | Unknown tail retained; flag is used by V2 as one-shot/AHD mode |
| Flex envelope `0x40` | Local codecs support versioned `0x11`/`0x12` records; max 32 points | Additional opaque envelope metadata retained; not every law established merely by a record codec |
| LFO `0x08` | `0x71..0x73`; waveform-specific fields; unknown supported-reader result can remain raw | Packed sync/flag fields; unnamed waveform 6; numeric serialization alone is not validated DSP law |
| Internal/external modulation | Internal object `0x80`/`0x81`; external `0x100..0x104`; target parsing maps only known meanings | Unknown sources/targets/tails can remain raw or become unsupported reports |
| Source identity | Unstructured `0x102`, `0x103`, `0x104`, `0x106`, seven-byte identity only; existing source-state method strictly `0x102`; wavetable codec is specific `0x106`/mode 9 | Other source parameters/record boundaries; source identity is not stretch/wavetable playback |
| Snapshots | Snapshot object versions **1 and 3**; metadata version **1**; compact group versions **2 and 4**; v4 source `0x106` mode3 length32/mode9 length99 | Other modes/version/layouts rejected; supported source bytes can still be opaque |
| Ladder filter record | Versions **0x90, 0x91, 0x92**, selected serialization types | v92 flag retained without inferred semantics; other effect modules require their own profiles |
| VoiceGroups `0x32` | Base vendor `0x60` decoder returns an empty groups vector after incomplete bitmap work; **V2 has a separate complete 128-bit bitmap reader** | Live vendor change explicitly refuses incomplete typed decoding rather than claiming success |

Evidence: `V:crates/sampler-kontakt/src/mapping.rs:29–63,91–121`; `V:vendor/ni-file/src/kontakt/objects/program.rs:14,50–80,143`; `filename_table.rs:53–99,197–225`; `envelope.rs:9–46,138–143`; `lfo.rs:16–41`; `internal_mod.rs:61`; `modulation.rs:174,340–357`; `group.rs:137–211`; `snapshot.rs:62–151`; `snapshot_group.rs:53–90`; `bparfx.rs:104–123`; `voice_groups.rs:27–54`; `V:crates/sampler-kontakt/src/library.rs:440–469`.

Many `KontaktObject` variants are labels, not decoded payloads: base `chunk.rs:154–263` maps BGroup, BLoop, BParEnv/LFO/Arp, many FX, BSample, BZone, snapshots, etc. to unit enum variants. Dedicated codecs for some of these exist elsewhere and are used directly by V2. Unknown IDs become `Unsupported(u16)` in that enum, but the original `Chunk.data` remains available only if the caller retains the raw chunk. Conversion to a unit variant does not carry its bytes. Do not report the count of named variants as the count of decoded objects.

NIS is similarly uneven. `nis/container/data/item_type.rs:49–94` names NISD/NIK4/RKTR IDs and preserves unknown domain/ID values. `nis/schema/nis_object.rs:14–27` maps just AppSpecific, BNISoundPreset, Preset, PresetChunkItem, RepositoryRoot; other types infer `Unknown`. Files for AudioSampleItem, AutomationParameters, BankContainer, BinaryChunkItem, InternalResourceReferenceItem, Module, ModuleBank, PictureItem and PresetContainer properties are **empty**. ControllerAssignments contains research pseudocode, not a Rust decoder. Generic NIS preservation therefore exceeds semantic product coverage substantially.

## Template comparisons: useful evidence, not a complete specification

All template links below pin [c6f309b](https://github.com/monomadic/hexfiend-templates/tree/c6f309bae04a03967b94f54d81dc2050f827a1e8). These are exploratory layouts with omissions and mistakes; promote fields only when lawful fixture evidence corroborates them.

| Template | Comparison to local code | Consequence |
| --- | --- | --- |
| [NIS/NISD.tcl](https://github.com/monomadic/hexfiend-templates/blob/c6f309bae04a03967b94f54d81dc2050f827a1e8/NIS/NISD.tcl#L51) | 40-byte item header, inherited 20-byte data layers, child descriptors of index/domain/ID match local framing. Template calls the two post-magic words unknown/flags whereas local naming differs | Preserve the words and descriptors; do not assign new semantics from their names |
| [Kontakt/StructuredObject.tcl](https://github.com/monomadic/hexfiend-templates/blob/c6f309bae04a03967b94f54d81dc2050f827a1e8/Kontakt/StructuredObject.tcl#L74) | Structured flag, u16 version, private/public/child lengths match. Template's unstructured branch stores length−1 raw bytes; local generic StructuredObject reads a u16 version before unstructured data | A format convention discrepancy requiring record-specific fixtures, not a reason to rewrite all unstructured parsing blindly |
| [Kontakt/ProgramV92PublicParams.tcl](https://github.com/monomadic/hexfiend-templates/blob/c6f309bae04a03967b94f54d81dc2050f827a1e8/Kontakt/ProgramV92PublicParams.tcl#L35), [ProgramVA8PublicParams.tcl](https://github.com/monomadic/hexfiend-templates/blob/c6f309bae04a03967b94f54d81dc2050f827a1e8/Kontakt/ProgramVA8PublicParams.tcl#L37) | Templates append resource-container/snapshot-folder/full-path/wallpaper u32 fields; local Program params stops at categories and returns resource/wallpaper None | Exact missing public-tail metadata candidates; u32 names alone do not prove index semantics |
| [Kontakt/FNTableImpl.tcl](https://github.com/monomadic/hexfiend-templates/blob/c6f309bae04a03967b94f54d81dc2050f827a1e8/Kontakt/FNTableImpl.tcl#L55) | v2, three logical filename groups and 8+4 bytes of sample metadata agree in broad structure; template omits active other-table parsing and describes timestamps as 32+32 | Local lossless v2 codec is more complete; preserve actual u64 timestamp bits, not guessed date precision |
| [Kontakt/ZoneDataV98Pub.tcl](https://github.com/monomadic/hexfiend-templates/blob/c6f309bae04a03967b94f54d81dc2050f827a1e8/Kontakt/ZoneDataV98Pub.tcl) | Common sample/velocity/key/gain/pan/tune/reference fields agree; no modern six-byte `0x9a` prefix | Local v0x9a handling is already present; template cannot establish its semantics |
| [Monolith.tcl](https://github.com/monomadic/hexfiend-templates/blob/c6f309bae04a03967b94f54d81dc2050f827a1e8/Monolith.tcl) | Fixed TOC sizes match; template labels filename ASCII and final member field fileSize, local reads UTF-16 and cumulative ends | Need real member-range fixtures before expanding variants; V's bounded helper already exists |
| [NKS/NKS.tcl](https://github.com/monomadic/hexfiend-templates/blob/c6f309bae04a03967b94f54d81dc2050f827a1e8/NKS/NKS.tcl), [NKS/NKSv42.tcl](https://github.com/monomadic/hexfiend-templates/blob/c6f309bae04a03967b94f54d81dc2050f827a1e8/NKS/NKSv42.tcl) | Header-family thresholds broadly agree; a BE magic is mentioned; v42 compressed body is called zlib in templates/docs, while local reader actually uses FastLZ | Docs' statement that all NKS compression is zlib is stale. Match decoder to header family |
| [NIS/AudioSampleItem.tcl](https://github.com/monomadic/hexfiend-templates/blob/c6f309bae04a03967b94f54d81dc2050f827a1e8/NIS/AudioSampleItem.tcl) | Contains nested version1 checks and a reference to AudioBlock loading, but no full payload layout; local property module empty | Neither source is sufficient for playable NIS sample coverage |
| [FM8/FM8.tcl](https://github.com/monomadic/hexfiend-templates/blob/c6f309bae04a03967b94f54d81dc2050f827a1e8/FM8/FM8.tcl), [NCW.tcl](https://github.com/monomadic/hexfiend-templates/blob/c6f309bae04a03967b94f54d81dc2050f827a1e8/NCW.tcl) | FM8 shows matrices/effects/morph fields without a complete semantic reader; NCW header agrees but sample blocks are illustrative fixed reads | FM8 decoding/DSP remains separate work. The existing NCW crate is the actual codec |

Also read NKX/NKR directory templates, BProgram, VoiceGroups, and the corresponding vendored docs. The VoiceGroups template itself has placeholder bytes; it is not evidence that the vendor's empty-vector result is complete.

## V2 import boundary and implications for Linux

The active import flow is:

1. `V:crates/sampler-kontakt/src/container.rs:89–110`: `read_chunks` opens a path, rejects files over 128 MiB, calls **vendored NIFile::read**, expands NKS or unwraps NIS, and reads KontaktChunks. It rejects FileContainer/NCW/resource/FM8 marker variants as “not an instrument container.” Legacy XML extraction consequently does not become an IR instrument: the next stage expects binary chunks.
2. `container.rs:113–162`: NIS AppSpecific wrappers are followed with a wrapper-depth limit; BNISoundPreset is selected using Repository schema inference; required access data is obtained only via the existing caller/feature boundary. No authorization modification is needed for clear files.
3. `library.rs:52–80`: locate Program `0x28` and file table `0x4b` or legacy `0x3d` **by ID**, not by the generic Kon4–7 typed schema dispatcher. Therefore an unrecognized app signature in that generic dispatcher is not automatically a V2 playback blocker. `read_multi` and `read_program` explicitly decode Bank/SlotList/ProgramList (`container.rs:18–85`, `library.rs:85–133`).
4. `library.rs:136` translates groups/zones/modulation/effects/scripts to `sampler_ir::Instrument`, retaining only modeled semantics. `SourceFormat::Kontakt { version: program.version() }` at `library.rs:167` stores the **Program serializer version**, not Kontakt plugin major. `V:crates/sampler-ir/src/lib.rs:93–99` explicitly documents that meaning. Unsupported entries carry location/feature/value/reason (`lib.rs:1119–1139`); the IR is not a lossless container representation.
5. `samples.rs:57–124` resolves slash-normalized loose paths and NKX/NKR member paths, checks roots and ambiguous duplicate filenames. `samples.rs:329–347` decodes WAV or NCW; labeling an asset AIFF in IR (`library.rs:995`) does not give this Kontakt loader an AIFF decoder.
6. `load.rs:156–204` loads selected samples; `finish`/`prepare` compiles and binds KSP and lowers the IR. `V:crates/sampler-core/src/lower.rs:214–230` validates IR and asset count before preparing playback; further branches reject unsupported runtime features. The runtime consumes prepared semantics and PCM, not ni-file's raw byte tree.

Thus Linux interoperability consists of several independently testable stages. Parsing an NKI, preserving a chunk, locating sample bytes, and producing approximate sampler audio are different outcomes. V2 explicitly reports nonzero source modes as “played as a sampler” and mode9 as “wavetable source” (`library.rs:541–553`); there is no source-engine equivalence established here. Additional/count/tune loop behaviors are reported (`library.rs:1019–1040`). Effects and KSP support require their own law/runtime audits; this report does not certify them.

At the audit baseline, `effects.rs:87–109` dropped failed BParFX/state conversions via `.ok()?` inside filter_map; `effects.rs:127–138` omitted malformed racks/buses; `library.rs:564` ignored group insert-rack errors via `if let Ok`. The parent has since fixed these paths using located entries in the existing Unsupported list; see [FX decode reporting](NI_FILE_FX_DECODE_REPORTING.md). An empty unsupported list still does not establish complete record coverage or audible equivalence.

The borrowed source APIs in `sampler-kontakt` already offer bounded/on-demand NIS layers (`nis.rs:29–75`) and a strict `Nks42` profile accepting outer word `0x0110` (`nks.rs:15–42`). They are not the same path as `container::read_chunks`, which still enters the vendored recursive decoder. Do not build a second parser merely because the live importer has not adopted these existing APIs.

## Fixtures and validation debt

The **local** ni-file directory has 170 files: 136 Rust, 28 Markdown, two TOML, one lockfile and three extensionless files. It contains **zero checked-in NKI/NKM/NKR/NKX/NKSN/NCW/FM8 fixture files** in either checkout. Literal File::open/std::fs::read scanning found 62 fixture-path references, including examples/doctests, with zero matching files. Several tests still refer to old `tests/filetype`, `tests/patchdata`, or `test-data` trees.

At the root base there are 110 `#[test]` annotations in the ni-file tree; V base has 112. Only one literal `#[ignore]` appears in each base vendor tree. These are annotation counts, not test execution results. In particular, historical file-backed tests cannot demonstrate successful compatibility on a checkout that omits their files. Ignore-status changes on other git refs are not assumed to be present at V HEAD.

The fetched **upstream fork** contains 345 files, including 140 Rust files, 29 Markdown files, and 170 non-Rust files under tests. Inventory suffixes include 26 `.nki`, five `.nkm`, one `.nkr`, two `.nkg`, one `.nkp`, one `.nkb`, five `.nfm8`, 57 `.kon`, two `.xml`, four `.fm8e`, and property/data fragments. Kontakt fixture directory labels include V1/V2/V42, 5.0.2, 5.3.0.6464, 5.4.3.307, 5.8.1.43, 6.2.2.51, and 7.1.3.0. No `.nkx`, `.ncw`, `.nksn`, `.wav`, or `.nksf` files occur in this upstream tests inventory. Presence, directory labels, and source assertions do not establish a passing full-file suite. Header observations above are narrower evidence.

Local deterministic coverage is nevertheless real source code: `vendor/ni-file/tests/compatibility.rs` has authored NIS/chunk roundtrips, corrupt-length checks, clear archive + bad-sibling behavior, synthetic keystream tests, versioned filename tables, envelopes, source identity and explicit slot-count checks. Examples: lines 11,55,79,146,313,384,769,856,1009,1037,1078. These tests were **read, not run**.

`V:crates/sampler-kontakt/tests` has ten Rust files plus two FastLZ binary vectors and their JSON metadata. Real-library tests find user-installed libraries via path list/settings and return when absent (`real_libraries.rs:1–43`); the broad surveys are ignored (`:249–251`, `:358–360`) and skip read failures. A passing run in an empty environment or a survey of only successful imports cannot establish version-wide support. `vendor/ncw/tests/data` separately has eight NCW and seven WAV files; there is no paired WAV for the listed 24-bit-stereo NCW. Those codec fixtures do not validate an NKI's zones, FX, scripts, or plugin sound.

Missing reproducible coverage includes actual major/release-to-header/object profiles, BE files, legacy monolith member layouts, XML semantic imports, modern FileContainer-to-IR sample resolution, filename v3 semantic edits, NIS AudioSampleItem payloads, non-Kontakt application bodies, NKSF, modern snapshot/source combinations beyond modeled modes, and unknown/future serializer behavior. Prefer legally distributable authored fixtures and manifests that say which stage each fixture exercises. Keep private-library conformance evidence separate from distributable corpus assets.

## Panic/TODO status and concurrent fixes

At the **V base**, these concrete incomplete APIs remain: generic preset dispatch `schemas/preset.rs:42,44` panics for unknown signatures/non-NKI; VoiceGroups `objects/voice_groups.rs:35` panics for other versions and returns incomplete groups for v0x60; exploratory Program private reader `objects/program.rs:145–207` panics; XML UTF-8 handling `schemas/xml.rs:23`, `kon1.rs:17`, `kon2.rs:18` uses expect; BNISoundHeader `nis/properties/bni_sound_header.rs:21–22`, Preset `preset.rs:71,76`, RepositoryRoot `repository_root.rs:60,66` assert on untrusted fields; FM8 `fm8/mod.rs:21,919` asserts/panics. Historical NIS extraction wrappers in `nis/items/preset.rs:19–33` and `nis/schemas/kontakt.rs:42–104` unwrap/panic/TODO. NKZ's description TODO is at `objects/header.rs:386`.

**Do not reimplement the concurrently assigned fixes.** V remained at HEAD `2fb8c926`, but the last observed working diff modifies exactly six files: `objects/header.rs`, `objects/program.rs`, `objects/voice_groups.rs`, `schemas/preset.rs`, `nis/items/preset.rs`, `nis/schemas/kontakt.rs`. It fixes the version Display bug (minor_2 previously printed twice), replaces NKZ description TODO, preserves unknown signatures/non-NKI as Unsupported raw chunks, refuses incomplete private/VoiceGroups typed decoding, and propagates extraction errors. This was inspected as an **uncommitted source diff**, not built or tested by this audit. The underlying missing semantic layouts remain missing even when panic behavior is corrected.

Already in the clean V base, compared with root: bounded FileContainer member reads and release-mode validation; bounded nested NIFile extraction and exact subtree consumption; Zone::filename_id; ID-based Kon4–7 schema access; corrected slot-count/dense-slot handling; native filename timestamp preservation; raw NIS/chunk roundtrips; filename v3 reads; modern source identities; narrow snapshot and wavetable record codecs. These are not new implementation tasks.

### Parent integration after this audit snapshot

The parent enabled `nis/items` and only `nis/schemas/kontakt`, corrected the
missing-inner properties unwrap, Preset property-prefix assertions and
BNISoundHeader magic/version assertions, and added runtime ItemWrapError guards.
Details are in [NI_FILE_NIS_READERS.md](NI_FILE_NIS_READERS.md). The focused
`--lib reader` check passed 12 tests after the VoiceGroups decoder; the existing `--features serde --test
compatibility` suite passed 32. This supersedes the source-only status for those
specific fixes, without changing the base-revision coverage observations above.

[NI_FILE_BINARY_RECORDS.md](NI_FILE_BINARY_RECORDS.md) now establishes the
VoiceGroups 0x60 framing and exact Program reader version dispatch from pinned
standalone machine code. The bounded VoiceGroups 0x60 reader is implemented and
covered by the 12-test focused run; [reader behavior and limits](NI_FILE_KONTAKT_READERS.md)
describe raw retention and version rejection. Native file acceptance and DSP
parity remain unverified.

The next parent round preserves malformed Program/group FX rack and occupied-slot
failures in the existing IR diagnostics while retaining valid siblings and source
coordinates. `cargo test --offline -p sampler-kontakt --lib` passed **30 tests**, with
**3 survey tests ignored**, including two new regression tests. See
[FX decode reporting](NI_FILE_FX_DECODE_REPORTING.md) and its frozen source receipt.
The [public-tail evidence](NI_FILE_PROGRAM_PUBLIC_TAILS.md) also establishes the
previously omitted F2 reference from a2, unconditional W1 from a8, both inline
BNISoundData body grammars and b3's counted bytes. The complete read-only public
record view is implemented as `Program::public_record()` and independently
reviewed. The expanded `--lib reader` run passed **15 tests**, including three new
Program regressions. Existing `params()` remains a prefix-only view; private
Program semantics remain opaque. See [complete public reader](NI_FILE_PROGRAM_PUBLIC_READER.md).

The [versioned corpus](NI_FILE_VERSIONED_CORPUS.md) pins **176 in-place files and
31 source hashes**, with metadata-stage unsupported/error records preserved.
Parent vendor-API follow-up separately decoded **25 complete Program public
records across 26 inputs**, including the actual two FileContainer members and
four installed Kon8 templates. Those templates measure Program b5/Group 96;
the FileContainer NKM retains two af programs in slots 0 and 1. Both containers
have empty decoded filename lookups and no sample members. The one whole-file
profile error is legacy Kon3 XML in the chunk-only probe. These stages do not
prove semantic import, embedded-sample resolution or native host/DSP acceptance.

## Finite implementation backlog, ranked

| Rank | Concrete remaining work | Exact evidence / reference | Completion evidence |
| --- | --- | --- | --- |
| 1 — partly completed | A pinned per-file metadata ledger and separate decoder probe are complete; authored full-container fixture expansion, absent fixture reporting and real-library failure totals remain | [176-file corpus and 26-input probe](NI_FILE_VERSIONED_CORPUS.md); `V:crates/sampler-kontakt/tests/real_libraries.rs:35–43,259–261,367–369` | Source/fixture drift checks pass and stages/errors remain distinct. Sample access, semantic roundtrip, rendering and survey failure totals still require independent evidence |
| 2 — completed | Preserve decode failures in V2's effects/rack translation reports using the existing Unsupported list | [FX reader/report change](NI_FILE_FX_DECODE_REPORTING.md); current tested source snapshots in its provenance ledger | 30 library tests passed, 3 surveys ignored; malformed occupied slots/racks/buses produce located notes and valid siblings survive. Raw source bytes remain accessible through container APIs |
| 3 | Close remaining parser trust-boundary gaps: NIS recursive depth/allocation budget on the actual importer path; errors instead of metadata/XML/FM8 assertions; bounded zlib expansion | `V:vendor/ni-file/src/nis/container/data/mod.rs:89–97`, `container/container.rs:44–48`; `nks/container.rs:103–107`; metadata/XML/FM8 spans above; existing `V:crates/sampler-kontakt/src/nis.rs:29–75`, `nks.rs:69` | Deep NIS and small compressed bodies with extreme expansion fail predictably; malformed public metadata returns errors. Reuse existing bounded readers where compatible. Exclude the six concurrent fixes above |
| 4 | Wire modern FileContainer into V2 import/sample resolution using the **existing** bounded member reader; keep legacy NKS monoliths separately unsupported until their layouts are established | `V:vendor/ni-file/src/file_container/mod.rs:22–43`; `V:crates/sampler-kontakt/src/container.rs:98–108`; `samples.rs:57`; [Monolith.tcl](https://github.com/monomadic/hexfiend-templates/blob/c6f309bae04a03967b94f54d81dc2050f827a1e8/Monolith.tcl) | Authored monolith fixture enters the IR and resolves its internal samples without pretending modern TOC rules cover legacy monoliths |
| 5 — partly completed | Complete Program public-record views are implemented; lossless filename v3 semantic editing and real-profile/native acceptance remain | [Program public reader](NI_FILE_PROGRAM_PUBLIC_READER.md); `filename_table.rs:64,197–225` | 15 focused tests passed, including all accepted Program versions and present nested bodies. Filename context relationships, historical writers and byte-preserving typed edits still need separate evidence |
| 6 | Implement legacy XML **semantic** profiles and prioritize requested Linux sample codecs such as AIFF; BE and legacy monoliths each need independent bounded profiles | `V:vendor/ni-file/src/kontakt/schemas/kon1.rs:11`, `kon2.rs:12`; `V:crates/sampler-kontakt/src/container.rs:110`, `samples.rs:329`, `library.rs:995`; [upstream XML/schema source](https://github.com/Ma5onic/ni-file/tree/1b7a518243125857fddec8217167b47a35cb58fa/src/kontakt/schemas); [NKS templates](https://github.com/monomadic/hexfiend-templates/tree/c6f309bae04a03967b94f54d81dc2050f827a1e8/NKS) | Actual XML zones/file refs reach IR and valid PCM; the matrix changes only for proven profiles. Existing zlib string extraction is reused |
| 7 | Decode NIS AudioSampleItem/resources/controller/automation properties only for concrete requested application profiles; RIFF NKSF is a separate framed reader | `V:vendor/ni-file/src/nis/properties/mod.rs:6,11,22,31`; empty modules inventoried above; `nksf/riff.rs:1–48`; [AudioSampleItem.tcl](https://github.com/monomadic/hexfiend-templates/blob/c6f309bae04a03967b94f54d81dc2050f827a1e8/NIS/AudioSampleItem.tcl) | Authored app-specific fixtures prove semantic bindings and samples. A named app enum or parsed wrapper never upgrades the playback column |
| 8 | If native file editing/export is required, add outer NKS/FileContainer writers and versioned header/checksum regeneration; otherwise leave the documented raw writer boundary intact | `V:vendor/ni-file/src/nifile/mod.rs:88–94`; `objects/header.rs:299–307`; [V42 template](https://github.com/monomadic/hexfiend-templates/blob/c6f309bae04a03967b94f54d81dc2050f827a1e8/NKS/NKSv42.tcl#L48) | Independent parse/readback and lawful application acceptance evidence for the exact edited profile; raw chunk roundtrip alone is insufficient |

Actual wavetable, time-stretch, synthesis, FX and script equivalence belongs to the separate DSP/runtime backlog. This audit supplies the source-mode and record-coverage evidence, not a promise of full Kontakt/FM8/Reaktor/Massive/Battery plugin emulation. Work on those engines only follows a demonstrated record mapping and a separately validated sound law.

## Source read inventory

Read the complete local ni-file README and Cargo manifest; root THIRD_PARTY/NOTICE provenance spans and relevant git history; the fetched fork README/Cargo and primary public APIs; all main doc topics (README, MAGIC, TERMINOLOGY, Applications, NIS/NKS/FileContainer/Monolith/AppSpecific, Kontakt/FM7/FM8/NCW), the Kontakt object-document subdirectory, and mdbook overview/summary material. Read public lib/NIFile/detection, NKS container/decompression, Kontakt preset/schema dispatch for Kon1/2/4/5/6/7 and multi, chunk/chunk-set/StructuredObject APIs, NIS header/data/container/ID/schema/property and subtree/encryption APIs, FileContainer, archive/member code, NKSF skeleton, exploratory FM8 reader regions, and supported record codecs for Program/group/zone/file names/arrays/envelopes/LFO/modulation/source/snapshots/filter. Read compatibility-test bodies and fixture inventories; sampled NCW reader/README/header and its fixture filenames.

For V2 specifically, inspected `crates/sampler-kontakt/src/{lib,container,library,load,mapping,nis,nks,samples,effects}.rs`, real-library test setup/surveys and checked-in fixtures; `crates/sampler-ir/src/lib.rs` Instrument/SourceFormat/Unsupported contracts; `crates/sampler-core/src/lower.rs` validation/asset/runtime-feature boundary. This is substantial source review, not a claim to have read every line or proven every layout. The detailed per-plugin architecture guide is owned separately in `docs/PLUGIN_VERSION_ARCHITECTURE.md`.

Report artifact: `/home/derpcat/.t3/worktrees/KONTAKTO/t3code-80fe786b/docs/NI_FILE_COVERAGE_AUDIT.md`.
