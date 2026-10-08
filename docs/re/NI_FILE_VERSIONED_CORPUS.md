# ni-file versioned corpus: in-place metadata ledger

Research snapshot: **2026-10-08**. The [manifest](../artifacts/ni-file-versioned-corpus-2026-10-08/manifest.json) pins **176 files**: every one of the 170 files already present under the upstream fork's `tests/data`, plus six specifically named installed defaults/templates. It stores paths, sizes, SHA256 hashes, outer framing, directly accessible authoring/fixed-header fields, and bounded raw Program/group/zone serializer framing. Assets were read in place; no preset, sample, or expanded payload was copied into this repository. No build, Rust test, installation, server, proprietary plugin execution, access-key derivation, or sample modification occurred.

**The two real FileContainer fixtures are preset-only containers.** Both have exactly one TOC member, at stored index **1**, named `patch.nki` or `patch.nkm`. Both name fields support UTF-16LE, despite the pinned template's ASCII label. Neither fixture supplies embedded-sample mapping evidence. A subsequent parent Rust probe decoded their clear compressed chunk streams and complete Program public sections; its separate results are below. Complete semantic preset decoding and DSP/host acceptance remain unproved.

## Reproduce and detect drift

From the report checkout:

```sh
python3 artifacts/ni-file-versioned-corpus-2026-10-08/check_corpus.py
```

The [read-only check](../artifacts/ni-file-versioned-corpus-2026-10-08/check_corpus.py) recomputes the entire metadata ledger and compares it to the pinned manifest. It exits nonzero on missing inputs, changed fixture bytes/inventory, changed source bytes, changed recorded repository HEADs, or changed extracted metadata. `--emit` prints recomputed metadata to stdout without writing files; use the normal command for verification. The command requires the existing local clone, template checkout, installed paths and V2 checkout at the manifest's exact roots. It downloads nothing. A missing `/tmp` checkout is an unavailable pinned input, not permission to substitute an arbitrary newer corpus. Read-only Python metadata checking is distinct from the prohibited Rust build/test work.

The parser follows bounded NIS items/data layers/child tables and uncompressed subtrees; it does **not** expand compressed subtrees or encrypted content. It follows strictly bounded raw chunk/StructuredObject/group-list/zone-list framing in already-exported fragments. A `raw chunk candidate` is a profile hypothesis based on corpus context; its recorded error must be retained. This helper is an evidence extractor, not a replacement production decoder. Unsupported layouts and partial observations remain explicit in each file's `errors` and `preset_decode_status`.

### Pinned identities

| Evidence | Identity |
| --- | --- |
| Root report checkout | `/home/derpcat/.t3/worktrees/KONTAKTO/t3code-80fe786b`; HEAD `0cb7a8a0b4d43086596a64c77320caa1b26d6d98` |
| V2 implementation reference | `/home/derpcat/.t3/worktrees/KONTAKTO/decipher-readers-v2`; committed HEAD `2fb8c926dd39bb7ac26a84d4806f42de14b6630e`; inspected working-tree files are separately hashed |
| Existing upstream fork | `/tmp/ni-audit-upstream.PIqOQo`; HEAD `1b7a518243125857fddec8217167b47a35cb58fa` |
| Existing templates | `/tmp/ni-audit-templates.POu0in`; HEAD `c6f309bae04a03967b94f54d81dc2050f827a1e8` |
| Installed root (`I` below) | `/home/derpcat/.wine/drive_c/Program Files/Common Files/VST3/Portapotty/Kontakt 8` |

The manifest pins 31 source/evidence files, including the checker itself. The V2 source hashes refer to inspected bytes, not a claim that its working tree equals its HEAD. `program.rs`, `effects.rs`, and `library.rs` were not edited or frozen from concurrent work. Root graph queries preceded source research; the graph's initial ranked answer did not describe this vendor profile, and V2/upstream were unindexed. No applicable on-disk `AGENTS.md` was found at the checked roots/ancestors; the supplied instructions governed this task.

### Source/function identity index

The manifest gives each file's full SHA256; line identities below refer to those frozen-by-hash bytes. Upstream paths are relative to the pinned fork, V2 paths to its reference checkout. These are source functions, so they have line identities rather than native virtual addresses.

| Source/function | Exact entry identity | Evidence used |
| --- | --- | --- |
| Upstream `BPatchHeader::read_le` / V2 / V42 readers | `src/kontakt/objects/header.rs:23`, `:142`, `:232` | Header-family dispatch and fixed field order |
| Upstream `ItemContainer::read` / `read_children` | `src/nis/container/container.rs:16`, `:74` | NIS framing and ordered child descriptors |
| Upstream `Preset::read` | `src/nis/properties/preset.rs:70` | Separate app ID and authoring version string |
| Upstream `BNISoundHeader::read` | `src/nis/properties/bni_sound_header.rs:15` | Header property tied to a NIS layer, not an arbitrary magic-byte search |
| Upstream `StructuredObject::read` | `src/kontakt/structured_object.rs:17` | Structured flag, serializer u16 and block lengths |
| V2 `NIFileContainer::read_member` / `read` | `vendor/ni-file/src/file_container/mod.rs:24`, `:46` | Stored-index access, TOC filenames and member ranges |
| V2 `read_chunks` / `nis_payload` | `crates/sampler-kontakt/src/container.rs:89`, `:113` | High-level admission and transparent wrapper path |
| V2 `Samples::resolve` / `archive_member` | `crates/sampler-kontakt/src/samples.rs:57`, `:274` | Root-bounded names and supported archive ancestors |

## Actual FileContainer TOC and member framing

All offsets below are decimal byte offsets in the outer file unless explicitly member-relative. The metadata marker is the exact 16 bytes `/\ NI FC MTD  /\`; the TOC marker is `/\ NI FC TOC  /\`. In both files: opaque metadata block `16..272`, count at 272, aggregate member length at 280, first TOC marker at 288, opaque 600-byte TOC block at 304, and the only 640-byte entry at 904. Its index is at 904, opaque 16 bytes at 912, **600 filename bytes at 928**, unknown u64 at 1528, and cumulative end at 1536. End marker at 1544 is `0xf1f1f1f1f1f1f1f1`; 16 padding bytes precede the second TOC marker at 1568 and opaque 592 bytes at 1584. The file section starts at **2176**.

| Outer fixture (`tests/data/Containers/FileContainer/files/`) | Outer bytes / SHA256 | TOC order / stored index / name | Member range and SHA256 |
| --- | --- | --- | --- |
| `000-default.nki` | 4473 / `ba8b3cbebb919e6de7243e961eefbe7e6f4b148887fc06ecbade9fe2be5988e5` | 0 / 1 / `patch.nki` | relative `[0,2297)`; absolute `[2176,4473)`; `7c61aef810a81ff80ce4ab93c57ee8bfd29af5761f4370dc56ee0da7bad61209` |
| `001-multi.nkm` | 5900 / `f2bddd5a9e221bb46119867621ae4f7025d7f602a2bd4dee66e59861f51f78ab` | 0 / 1 / `patch.nkm` | relative `[0,3724)`; absolute `[2176,5900)`; `7defb0a5bc0d977595b0c0a611764351046159258b6b9c091a3146f460753ec0` |

The member aggregates exactly fill the outer files; neither has trailing bytes. Stored index **1 is not ordinal 0**. Neither member hash matches a whole-file hash elsewhere in the frozen 176-file ledger; differently named exported presets must not be substituted as identical payloads.

### Filename encoding discrepancy

The exact first 20 bytes of the fixed filename field are:

```text
000-default.nki @928: 70 00 61 00 74 00 63 00 68 00 2e 00 6e 00 6b 00 69 00 00 00
001-multi.nkm   @928: 70 00 61 00 74 00 63 00 68 00 2e 00 6e 00 6b 00 6d 00 00 00
```

UTF-16LE interpretation yields `patch.nki` / `patch.nkm`; null-terminated ASCII interpretation yields only `p`. The manifest records the SHA256 of the **entire 600-byte field**, its short filename prefix, and both interpretations. V2 `vendor/ni-file/src/file_container/mod.rs:80–81` calls `StringReader::read_nullterminated_utf16`, matching these fixtures. Pinned `Monolith.tcl:16` says `ascii 600 "fileName"`; template SHA256 is `840c4a2a7b22e22e32e6d7f21a06311b665a30d6d10965bb09da2e0b268ae758`. This is a demonstrated discrepancy, not proof that every possible FileContainer filename encoding is UTF-16LE. Non-ASCII names and other variants remain untested. The template's `fileSize` label at line 18 likewise must not replace the reader's cumulative-end interpretation; these single-member fixtures alone cannot distinguish cumulative end from individual size.

### Correct preset members and accessible version fields

The sole member in each fixture starts with a complete NIS item: length 2297 / 3724, item framing revision 1, domain marker `hsin`. Its filename and fixed header type identify the candidate preset; there is no sample member or competing candidate.

| Member | NIS authoring metadata (member-relative offset) | Fixed header | Counts in header |
| --- | --- | --- | --- |
| `patch.nki` | Preset at 342: app ID 2, `7.1.3.0` | @2067 (outer 4243), word `0x110`, type 1, `Kon7`, patch `7.1.3.255`, minimum `7.1.0.0`, SVN 0; author `Kontakt` | zones 0, groups 1, instruments 1 |
| `patch.nkm` | Preset at 535 and AppSpecific at 3690: app ID 2, `7.1.3.0` | @3460 (outer 5636), word `0x110`, type 0, `Kon7`, patch `7.1.3.255`, minimum `7.1.0.0`, SVN 0; author empty | zones 0, groups 2, instruments 2 |

Both **inner** fixed headers have monolith flag 0; that does not negate the observed **outer** FileContainer. The NKM has a transparent uncompressed AppSpecific subtree beginning at member offset 319, followed far enough to inspect its NIS layers and fixed header. The actual preset subtree is compressed: property at 1055 for the NKI and 1260 for the NKM. Program, group and zone serializer versions and filename tables inside those payloads were unknown in the metadata snapshot; the separate parent probe below establishes selected decoded fields. Header counts alone are reported fields, not decoded slot/group validation.

## Per-file application/container ledger

`U:` paths below are relative to upstream `tests/data/`; `I:` paths are relative to the installed root. Patch version, minimum version, application signature, authoring version and object serializer version are independent axes. `—` means unavailable at this inspection stage, not zero, unsupported by the real vendor, or absent from a compressed body. Fixed header offsets are absolute within the named file; FileContainer member offsets are given separately above. The manifest contains full hashes, author strings, SVN words, errors and raw serializer observations for **every** file, including fragments omitted from this readable table.

| File | Outer profile | Header @ / signature / patch / minimum | NIS authoring version |
| --- | --- | --- | --- |
| `U:Containers/FileContainer/files/000-default.nki` | FileContainer | member: see above | — |
| `U:Containers/FileContainer/files/001-multi.nkm` | FileContainer | member: see above | — |
| `U:Containers/NIS/files/AppSpecific/default-7.nkb` | NIS | 1571 / Kon7 / 7.1.3.255 / 7.1.0.0 | 7.1.3.0 |
| `U:Containers/NIS/files/AppSpecific/fx-001.nkm` | NIS | 3397 / Kon7 / 7.1.3.255 / 7.1.0.0 | 7.1.3.0 |
| `U:Containers/NIS/files/BNISoundPreset/fx-001.nki` | NIS | 2185 / Kon7 / 7.1.3.255 / 7.1.0.0 | 7.1.3.0 |
| `U:Containers/NIS/files/BNISoundPreset/insert-fx.nki` | NIS | 2233 / Kon7 / 7.1.3.255 / 7.1.0.0 | 7.1.3.0 |
| `U:Containers/NIS/files/BNISoundPreset/musical_saw.nki` | NIS | 2656 / Kon5 / 5.0.2.255 / 4.9.0.255 | 5.0.2.5641 |
| `U:Containers/NIS/files/BNISoundPreset/script.nki` | NIS | 1958 / Kon7 / 7.1.3.255 / 7.1.0.0 | 7.1.3.0 |
| `U:Containers/NIS/files/BNISoundPreset/two-voice-groups.nki` | NIS | 2009 / Kon7 / 7.1.3.255 / 7.1.0.0 | 7.1.3.0 |
| `U:Containers/NIS/files/KontaktEnvelope/default.nkp` | NIS | 1663 / Kon7 / 7.1.3.255 / 7.1.0.0 | 7.1.3.0 |
| `U:Containers/NIS/files/fm8/1.2.0.1010/001-fm7.nfm8` | NIS | — | 1.2.0.1010 |
| `U:Containers/NIS/files/fm8/1.2.0.1010/002-fm7.nfm8` | NIS | — | 1.2.0.1010 |
| `U:Containers/NIS/files/fm8/1.2.0.1010/003-fm8.nfm8` | NIS | — | 1.2.0.1010 |
| `U:Containers/NIS/files/fm8/1.2.0.1010/004-fm8fx.nfm8` | NIS | — | 1.2.0.1010 |
| `U:Containers/NIS/files/fm8/1.2.0.1010/005-attacks.nfm8` | NIS | — | 1.2.0.1010 |
| `U:Containers/NIS/files/group/group.nkg` | NIS | 2290 / Kon7 / 7.1.3.255 / 7.1.0.0 | 7.1.3.0 |
| `U:Containers/NIS/files/kontakt/5.0.2/musical_saw.nki` | NIS | 2656 / Kon5 / 5.0.2.255 / 4.9.0.255 | 5.0.2.5641 |
| `U:Containers/NIS/files/kontakt/5.3.0.6464/000-did.nki` | NIS | 28361 / Kon5 / 5.3.0.255 / 5.2.8.255 | 5.3.0.6464 |
| `U:Containers/NIS/files/kontakt/5.4.3.307/000.nki` | NIS | 7420 / Kon5 / 5.4.3.255 / 5.4.1.0 | 5.4.3.307 |
| `U:Containers/NIS/files/kontakt/5.8.1.43/5.8.1.43-ncw.nki` | NIS | 15724 / Kon5 / 5.8.1.255 / 5.8.0.0 | 5.8.1.43 |
| `U:Containers/NIS/files/kontakt/6.2.2.51/002-tib.nki` | NIS | 2327 / Kon6 / 6.2.2.255 / 6.2.2.0 | 6.2.2.51 |
| `U:Containers/NIS/files/kontakt/7.1.3.0/000-default.nki` | NIS | 1915 / Kon7 / 7.1.3.255 / 7.1.0.0 | 7.1.3.0 |
| `U:Containers/NIS/files/kontakt/7.1.3.0/001-single-sample.nki` | NIS | 2474 / Kon7 / 7.1.3.255 / 7.1.0.0 | 7.1.3.0 |
| `U:Containers/NIS/files/kontakt/7.1.3.0/002-single-sample-2.nki` | NIS | 2499 / Kon7 / 7.1.3.255 / 7.1.0.0 | 7.1.3.0 |
| `U:Containers/NIS/files/kontakt/7.1.3.0/003-multi.nkm` | NIS | 3366 / Kon7 / 7.1.3.255 / 7.1.0.0 | 7.1.3.0 |
| `U:Containers/NIS/files/kontakt/7.1.3.0/004-triple-multi.nkm` | NIS | 3638 / Kon7 / 7.1.3.255 / 7.1.0.0 | 7.1.3.0 |
| `U:Containers/NIS/files/maschine/2.0.0.0/001-standard.mxfx` | NIS | — | 2.0.0.0 |
| `U:Containers/NIS/files/massive/1.0.0.0/000-new.nmsv` | NIS | — | 1.0.0.0 |
| `U:Containers/NKM/000.nkm` | KontaktMultiV1 | — | — |
| `U:Containers/NKR/000.nkr` | KontaktResource | — | — |
| `U:Containers/NKS/KontaktV1/000-kontaktv1-nki.nki` | NKSv1LE | 0 / — / — / — | — |
| `U:Containers/NKS/KontaktV2/KontaktV2-000-empty.nki` | NKSv2LE | 0 / Kon4 / 4.0.0.255 / 3.9.9.255 | — |
| `U:Containers/NKS/KontaktV2/NKSv2-NKG-Kon3.nkg` | NKSv2LE | 0 / Kon3 / 3.9.9.255 / 3.9.9.2 | — |
| `U:Containers/NKS/KontaktV2/cs-kicks.nki` | NKSv2LE | 0 / Kon4 / 4.0.1.6 / 4.0.0.255 | — |
| `U:Containers/NKS/KontaktV2/kokiriko_all_fx.nki` | NKSv2LE | 0 / Kon3 / 3.5.0.25 / 3.0.9.14 | — |
| `U:Containers/NKS/KontaktV2/kokiriko_all_stac_reverse.nki` | NKSv2LE | 0 / Kon3 / 3.5.0.25 / 3.0.9.14 | — |
| `U:Containers/NKS/KontaktV2/kokiriko_all_stac_vokiverb_4.nki` | NKSv2LE | 0 / Kon3 / 3.5.0.25 / 3.0.9.14 | — |
| `U:Containers/NKS/KontaktV42/4.2.2.4504-000.nki` | NKSv2LE | 0 / Kon4 / 4.2.2.255 / 4.2.0.255 | — |
| `U:Containers/NKS/KontaktV42/4.2.4.5316-000.nki` | NKSv2LE | 0 / Kon4 / 4.2.4.255 / 4.2.3.255 | — |
| `U:Containers/NKS/KontaktV42/KontaktV42-000.nki` | NKSv2LE | 0 / Kon4 / 4.2.4.255 / 4.2.3.255 | — |
| `U:Containers/NKS/MonolithV2/000-phv2_monolith_kon2_nki.nki` | NKSv2LE | 0 / Kon2 / 2.0.1.2 / 0.0.0.0 | — |
| `U:Containers/NKS/MonolithV2/2.1.0.001-000.nki` | NKSv2LE | 0 / Kon2 / 2.1.0.1 / 2.0.9.9 | — |
| `U:Containers/NKS/MonolithV2/KontaktV2-001.nki` | NKSv2LE | 0 / Kon4 / 4.0.5.255 / 4.0.4.255 | — |
| `I:default/kontakt_def.nki` | NKSv2LE | 0 / Kon3 / 3.9.9.255 / 3.9.9.2 | — |
| `I:default/kontakt_def.nkm` | NIS | 2144 / Kon6 / 6.7.1.255 / 6.7.0.0 | 6.7.1.0 |
| `I:Templates/Empty.nki` | NIS | 2133 / Kon8 / 8.6.1.255 / 8.6.0.0 | 8.6.1.0 |
| `I:Templates/Kontakt Controls.nki` | NIS | 2996 / Kon8 / 8.7.0.255 / 8.5.1.0 | 8.7.0.0 |
| `I:Templates/.template_data/Empty/Empty.nki` | NIS | 2091 / Kon8 / 9.9.9.255 / 8.5.1.0 | 9.9.9.0 |
| `I:Templates/.template_data/Kontakt Controls/Kontakt Controls.nki` | NIS | 2931 / Kon8 / 9.9.9.255 / 8.5.1.0 | 9.9.9.0 |

The installed `default/kontakt_def.nki` is a V2-family `Kon3` header with patch `3.9.9.255`; installing Kontakt 8 does not reauthor every bundled file. Installed `Empty.nki` and `Kontakt Controls.nki` report **8.6.1.0** and **8.7.0.0**. Their `.template_data` counterparts report **9.9.9.0**, with `Kon8` and minimum **8.5.1.0**. These are literal metadata values; no Kontakt 9 release or serializer profile is inferred. The metadata pass did not expand their Program bodies; the parent independently measured them below.

The installed six-file scope was confined to existing `default/` and `Templates/` under the already-identified installation. It is not a new whole-library inventory. The metadata check reads their clear outer framing only and never opens sample assets or an access-key source.

## Direct serializer evidence in exported/raw fixtures

The rows below require successful bounded chunk framing in this Python check. The original application/build strings in their directory/file names are **corpus labels**, not measured save provenance. No row proves the full file was decoded, imported into IR, roundtripped semantically, or accepted by a native plugin. Object versions are directly read from the flag/u16 prefix, and manifest offsets identify every occurrence. `none observed` does not assign a serializer version to an empty list.

| File under upstream `tests/data/` | Program versions | Group versions | Zone versions |
| --- | --- | --- | --- |
| `Containers/NIS/chunks/inner-file/kontakt/000-default` | 0xaf | 0x95 | none observed |
| `Containers/NIS/files/kontakt/6.2.2.51/kontakt.nki.kon` | 0xac | 0x95 | none observed |
| `Objects/Kontakt/0x28-Program/ProgramV80/ProgramV80-000.kon` | 0x80 | 0x90 | 0x93 |
| `Objects/Kontakt/0x28-Program/ProgramVA5/ProgramVA5-000.kon` | 0xa5 | 0x95 | 0x98 |
| `Objects/Kontakt/0x28-Program/ProgramVAC/ProgramVAC-000.kon` | 0xac | 0x95 | none observed |
| `Objects/Kontakt/0x28-Program/ProgramVAF/ProgramVAF-000-group-fx.kon` | 0xaf | 0x95 | none observed |
| `Objects/Kontakt/0x28-Program/ProgramVAF/ProgramVAF-001-insert-fx.kon` | 0xaf | 0x95 | none observed |
| `Objects/Kontakt/0x33-GroupList/GroupList-000.kon` | none observed | 0x90 | none observed |
| `Objects/Kontakt/0x33-GroupList/GroupList-001.kon` | none observed | 0x95 | none observed |
| `Objects/Kontakt/0x33-GroupList/GroupList-002.kon` | none observed | 0x95 | none observed |
| `Objects/Kontakt/0x34-ZoneList/ZoneList-000.kon` | none observed | none observed | 0x93 |
| `Objects/Kontakt/InternalPatchData/5.4.3.307-000` | 0xa8 | 0x95 | 0x98 |
| `Objects/Kontakt/InternalPatchData/7.3.0.30-000` | 0xaf | 0x95 | none observed |
| `Objects/Kontakt/InternalPatchData/internal_patch_data/4.2.2.4504/000` | 0x80 | 0x90 | 0x93 |
| `Objects/Kontakt/InternalPatchData/internal_patch_data/5.3.0.6464/000` | 0xa5 | 0x95 | 0x98 |
| `Presets/Kon4/4.2.2.4504/000` | 0x80 | 0x90 | 0x93 |
| `Presets/Kon5/5.3.0.6464/000-did` | 0xa5 | 0x95 | 0x98 |
| `Presets/Kon6/6.2.2.51/002-tib` | 0xac | 0x95 | none observed |
| `Presets/Kon7/7.1.3.0/000-default` | 0xaf | 0x95 | none observed |
| `Presets/Kon7/7.1.3.0/001-single-sample` | 0xaf | 0x95 | 0x9a |
| `Presets/Kon7/7.1.3.0/002-single-sample-2` | 0xaf | 0x95 | 0x9a |
| `Presets/Kontakt/NKI/Kon5/5.0.0.255-01 (Kontakt 5.1.0.6066).nki.kon` | 0xa2 | 0x94 | 0x97 |
| `Presets/Kontakt/NKI/Kon5/5.1.1.0-01 (Kontakt 5.3.1.37).nki.kon` | 0xa5 | 0x95 | 0x98 |

This establishes observed Program `0x80,0xa2,0xa5,0xa8,0xac,0xaf`, group `0x90,0x94,0x95`, and zone `0x93,0x97,0x98,0x9a` in these accessible fragments. It does not assign one tuple to all releases of a plugin major. In particular, compressed installed Kon8 files must not be assigned Program `0xb5` from static native dispatch coverage alone.

The existing [Program public-tail report](NI_FILE_PROGRAM_PUBLIC_TAILS.md) retains the stronger native framing evidence and its separate fixture-support limits. Its native image is `Kontakt 8.exe`, PE version 8.13.1, image base `0x140000000`, SHA256 `0fe6356e0879d058b6e5b73507c54c5e345cea451b35287c974e438291d4dae8`. The previously pinned plugin payload has base `0x180000000`, SHA256 `8ed90c4b9dd2bb2c5cc45b64dea144f6cca245c706457de4aa72c643a8acc09d`. Existing function identities include BProgram read-public dispatcher **0x140d0d4b0**, private dispatcher **0x140d0a2c0**, child dispatcher **0x140d063a0**, and VoiceGroups reader **0x140d1e350**. Their exact original function ranges/hashes remain in the pinned [record results](../artifacts/ni-file-records-2026-10-08/check-results.json); those results/source manifests are hashed by this ledger. **No executable was opened or run again in this task**, and this corpus check does not repeat their machine-code validation.

## Filename-table evidence and the sample-path gap

Direct raw `FNTableImpl` v2 records were read using the pinned segment layout. The manifest records segment types and text, separate special/sample/other indices, raw sample timestamp and unknown-u32 words, and trailing metadata **length/hash**. Trailing metadata is not rejected or assigned a meaning: V2 `FNTableRecord` already preserves it. A filename-table index is local to its table, not automatically a FileContainer stored index. Unknown versions/segment kinds fail this narrow metadata path explicitly.

| Export under `tests/data/Presets/Kon7/7.1.3.0/` | Sample table | Other table | What is established |
| --- | --- | --- | --- |
| `000-default` | empty | index 0: `kontakt7-empty.nki` | Raw exported table; not the FileContainer NKI member's decoded table |
| `001-single-sample` | index 0: `001-single-sample Samples/beep.wav` | index 0: `001-single-sample.nki` | Directory segment type 2 followed by filename type 4 |
| `002-single-sample-2` | index 0: `002-single-sample-2 Samples/beep.wav` | index 0: `002-single-sample-2.nki` | Same path grammar; no matching WAV asset in upstream tests |
| `003-multi.chunk` | empty | index 0: `../../../monolith/kontakt/001-multi.nkm/000-default.nki` | Three parent segments type 3, directory segments type 2, **multi-file segment type 9**, filename type 4 |

All four have two trailing metadata bytes, recorded by hash without inferred semantics. The multi-file segment confirms that a filename grammar can treat an `.nkm` as a path component. It does **not** establish that this exported table belongs to the priority FileContainer, that its last component names the stored `patch.nkm`, or that basename-only matching is valid. The two actual FileContainers contain no sample bytes, so there is no observed filename-table-to-embedded-sample-member mapping to certify. Such a mapping requires a real multi-member fixture with its exact internal table decoded by the parent.

## Concrete integration constraints

1. **Admission:** V2 `crates/sampler-kontakt/src/container.rs:89–110`, `read_chunks`, admits NKS and NIS only; FileContainer reaches `not an instrument container`. Its outer 128 MiB cap also applies before any proposed member selection. A large sample-bearing monolith can exceed that cap even when its preset is small. A parent implementation must choose and document a bounded outer/member policy rather than accidentally treating sample aggregate size as preset allocation size.
2. **Selection:** use actual TOC order, stored index and decoded name, then redetect the bounded member bytes. For these two fixtures the unique candidate is index 1; do not read ordinal/index 0, require the outer basename (`000-default`/`001-multi`) to match `patch`, or hard-code 1 for all containers. Ambiguous preset candidates, duplicate stored indices, invalid ranges, unsupported/nested containers and oversized members must produce explicit errors. Preserve the container's origin and member identity for subsequent path resolution.
3. **Bounds:** V2 `vendor/ni-file/src/file_container/mod.rs:24–43`, `read_member`, already rejects missing/duplicate indices, caller-limit excess and checked offset overflow. Its `read:46–126` validates markers, count/ranges, cumulative total and available file bytes. Reuse these boundaries. Real multi-member monotonic offsets, filename variants and ambiguous selections still need conformance fixtures. Current real data has only one entry each; the authored multi-member test is a separate evidence tier.
4. **Inner preset:** feed selected member bytes through the existing NIS/NKS detection and clear-preset path. The NKM's AppSpecific wrapper must be followed before finding the bank/program chunk stream. Retain type 0 multi slot/routing relationships; do not coerce it to one NKI. Existing `nis_payload` at `container.rs:113–163` handles AppSpecific recursion, with its existing wrapper limit and unchanged access boundary; no new license/auth/key behavior is proposed here.
5. **Sample locations:** `samples.rs:57`, `Samples::resolve`, handles bounded loose files and NKX/NKR paths. Its `archive_member:274–287` recognizes only `.nkx`/`.nkr` ancestors. A virtual `<file.nki|file.nkm>/<member>` currently falls through to a loose-file read. Existing `Samples::decode:123`, `source:149`, and `frames:192` must share the same explicit FileContainer member identity/range if admission is added; otherwise initial decoding might succeed while streaming or header reads fail. Preserve library-root bounds and duplicate-name ambiguity; do not solve this by writing extracted samples, guessing TOC index = zone filename ID, or accepting arbitrary path traversal.
6. **Mapping remains unproved:** obtain actual inner filename tables and zone filename references first. Match against decoded TOC names under a documented normalization policy that preserves meaningful segment types. The type-9 multi-file grammar is relevant evidence, but these fixtures cannot validate an embedded-sample policy. Nonempty sample-bearing FileContainer, duplicate basenames, multiple instruments, unsupported sample encoding, size limits and stream bounds belong in the parent's authored or legally available fixture checks.

These are source/evidence findings, not production changes. No general new abstraction is required by this research.

## Read-only probe recipe and remaining validation

The parent can use the existing Rust readers in a read-only fixture probe: `NIFileContainer::read(File::open(path)?)`, assert the metadata/TOC facts above, then `read_member` by the actual stored index with a preset-specific byte limit and verify the member SHA256. Pass a cursor over that in-memory member through `NIFile::read`; use the existing NIS/AppSpecific/preset-subtree path without altering authorization handling. After decompression, enumerate chunk IDs and exact consumption, collect StructuredObject Program/group/zone versions, and decode `FNTableRecord`/`FNTableImpl` plus multi bank/slot/program relations. Record unsupported errors rather than converting them into release guesses. For these two sample-free members, explicitly report an empty embedded-sample member set independently of whatever external filenames the inner table contains.

A Rust probe must **not** substitute the separate exported presets as the selected member or claim that metadata success means native acceptance. A multi-member sample fixture is additionally needed to establish actual table-to-TOC matching, WAV/NCW header access, streaming range selection and explicit unsupported samples. Builds/tests are the parent's responsibility and were not started here. No native DSP, host, serializer roundtrip, or playback acceptance is claimed.

## Evidence limits and results

The [recorded check results](../artifacts/ni-file-versioned-corpus-2026-10-08/check-results.json) pass for **176 files / 31 source hashes**. Separate temporary manifest alterations to a pinned source hash and fixture hash both produced exit code 1; the temporary files were removed. No pinned source or asset was changed to exercise rejection. Seventy-one per-file records contain unsupported-profile or framing errors; many are standalone property/old-public-private fragments intentionally not modeled by the narrow extractor. They are **not** 71 demonstrated failures of the Rust vendor. Even a metadata row without an error has `preset_decode_status = not run; static metadata only`. Every source/fixture hash is in the manifest, including duplicate fixtures; byte-identical labels do not add independent conformance coverage.

The metadata pass includes no BE preset fixture, embedded-sample FileContainer, modern installed Program expansion, or native-host verification. The separate parent follow-up below adds clear raw chunk/public-record decoding without upgrading semantic playback or native acceptance. Unknown layout stays unknown. The research child left existing guides, production source, shared changes, sample assets and access handling untouched.

## Parent clear-fixture decode follow-up

The parent ran the existing V2 vendor APIs against **26 pinned inputs**: two FileContainers, six installed defaults/templates and 18 accessible Program-bearing raw exports. The temporary Cargo example was removed after each run; no production source, access handling or sample asset was changed. `NIFile::inner_preset()` was called without a key provider. Expanded payloads remained in memory; retained artifacts contain only metadata, source and check results.

| Selected real input | Decoded raw stream | Complete Program public view | Group/zone observations |
| --- | --- | --- | --- |
| FileContainer `000-default.nki`, actual member index 1 | 4048 bytes; chunks `28,47,4b` (hex); filename lookup empty | one `af`, public 130 bytes | one Group `95`; no zones |
| FileContainer `001-multi.nkm`, actual member index 1 | 11583 bytes; Bank `03` and filename table `4b`; lookup empty | `af` in original slots 0 and 1, public 140/142 bytes | one Group `95` per program; no zones |
| Installed `Templates/Empty.nki` | 4279 bytes | `b5`, public 175 bytes | one Group `96`; no zones |
| Installed `Templates/Kontakt Controls.nki` | 6263 bytes | `b5`, public 221 bytes | one Group `96`; no zones |
| Hidden `Empty/Empty.nki` | 4231 bytes | `b5`, public 175 bytes | one Group `96`; no zones |
| Hidden `Kontakt Controls/Kontakt Controls.nki` | 6143 bytes | `b5`, public 205 bytes | one Group `96`; no zones |

Across all inputs, **25 complete public records decoded, zero public-record errors**. Observed versions are `80,a2,a5,a8,ac,af,b5`; duplicates remain duplicate evidence. All four installed b5 bodies have S0/S1 absent and a 36-byte B0. Real present-S bodies remain a corpus gap despite authored tests proving their grammar. The NKM slot identity comes from the existing Bank/SlotList/ProgramList APIs; this is not a routing or native-host conformance test.

The whole-file result is **25 OK, 1 recorded profile error**: the installed Kon3 default expands to 12539 bytes of legacy XML, which the deliberately chunk-only probe cannot decode. It is not a damaged Kontakt preset. The installed Kon6 default NKM has a decoded Bank with no observed programs. Filename lookup entries may include resources/preset paths; their count must not be labeled a count of audio samples.

The [probe output](../artifacts/ni-file-round2-2026-10-08/fixture-probe-output.txt), [structured summary](../artifacts/ni-file-round2-2026-10-08/fixture-probe-summary.json), [input hashes](../artifacts/ni-file-round2-2026-10-08/fixture-probe-inputs.json) and [source hashes](../artifacts/ni-file-round2-2026-10-08/fixture-probe-sources.json) are separate from the frozen metadata ledger. Reproduce with the shared Rust slot idle:

```sh
python artifacts/ni-file-round2-2026-10-08/check_fixture_probe.py
```

This verifies pinned inputs/sources, runs one offline Cargo example, compares exact metadata output, and removes the example. The parent recorded and independently reran it successfully. Input files are capped at 16 MiB and the known preset members at 1 MiB; these are probe limits, not general monolith admission policy. Generic vendor extraction/public views are separate from `KontaktPreset` schema admission and V2 semantic translation. Both real FileContainers still lack sample members and now also show empty decoded filename lookups, so embedded-sample mapping/streaming remains unproved.
