# Kontakt / NI file-format scout

Research snapshot: 2026-10-27. Read-only. Nothing was built, run, or committed. Clones live outside the repo in `/data/dev/workspace/Research/scout/<owner>_<repo>`, treated as untrusted data and only read. Repo metadata came from `gh`; licenses are GitHub's SPDX field, not a read of each license file, except where noted.

Compared against: `vendor/ni-file` (from the `Ma5onic/ni-file` fork at `1b7a518`), `vendor/ncw` (monomadic 0.4.0 plus local edits), `docs/re/NI_FILE_*.md`, `docs/re/formats/`, `docs/LEGAL.md`, and `THIRD_PARTY.md`.

Search coverage: GitHub repo and code search (NKI/NKX/NKS/NICNT/NCW/FC-marker strings, rate-limited for part of the run), and web search (standard mode) for forum and reference write-ups. Not done: deep reads of ConvertWithMoss's maschine package, Audiocity's full NKI probe, and the two "Kontakt-Public" resource repos.

## 1. Ranked projects

Ranked by how much each adds beyond what `ni-file` and `ncw` already decode.

| # | Project | License | Lang | Last push | Formats covered | Adds beyond ni-file / ncw | Quality / trust |
|---|---|---|---|---|---|---|---|
| 1 | [git-moss/ConvertWithMoss](https://github.com/git-moss/ConvertWithMoss) | LGPL-3.0 | Java | 2026-10-11 (459★) | Kontakt 1, 2/1.5, 4.2, 5+ incl. monolith; Maschine 1/2; converter to/from SFZ, SF2, WAV, etc. | Full NIS chunk-type name table (24 named IDs, see G3); magic constants missing from ni-file (G5); Kontakt 5 `FileList` v2/v3 and `InternalModulator` 0x80/0x81 readers (G6) | High. Mature, widely used, active. Large codebase; per-format claims are the author's, but the converter output is exercised. |
| 2 | [larsenwald/kontakt-8-library-manager](https://github.com/larsenwald/kontakt-8-library-manager) | none (404) | Python | 2026-10-18 | `.nicnt` FC container, ProductHints XML, patch `DSIN 108` bank list, `komplete.db3` schema | Only write-up of the NICNT TOC with claimed byte-exact rebuild of 7 vendor `.nicnt` files (G1, G2); patch bank-rename with size-field fixups (G11) | Medium-high on what it claims (it describes its own validation, not independent). 0 stars, no license: learn only. |
| 3 | [monomadic/ncw](https://github.com/monomadic/ncw) (upstream of `vendor/ncw`) | MIT OR Apache-2.0 | Rust | 2026-09-24 | NCW PCM16/24 and float; block/flag semantics | Newer than our vendored copy: `ncw-convert` CLI, template-preserving writer, `WRITER_VALIDATION.md`, `FORMAT.md` unknown-field register. Our vendor adds a format-word reader that upstream lacks. | High for NCW; author probes against Kontakt 8.9.0. Caveats in its own docs (widths 0/1, zero-width blocks, float M/S unvalidated). |
| 4 | [maxton/nkxtract](https://github.com/maxton/nkxtract) | GPL-3.0 | C# | 2019-04-15 (31★) | NKX/NKR monolith directory and members | Second, independent NKX directory reader (v0x111, `set_id`, count, flags) to cross-check ni-file's NKR code | Medium. Small and clear; old (Kontakt 5 era); hard-coded to one instrument. **GPL: learn only** (see §4). |
| 5 | [JimiHFord/unnks](https://github.com/JimiHFord/unnks) | GPL-3.0 | C | 2023-03-10 | NKS and NKX archive unpacker (tar-like CLI) | Third reader for the same archive family; useful for cross-checking member/offset rules | Medium-low. Maintained only sporadically. **GPL: learn only.** |
| 6 | [perivar/PresetConverter](https://github.com/perivar/PresetConverter) (`PresetConverterProject/NIKontaktNKS/`) | MIT | C# | 2025-06-03 (7★) | NCW (C# port of Pascal), NICNT writer, NKI version string, NKS magic table, Native Access library DB | `nklibs_info.userdb` and `Settings.cfg` readers, SNPID CSV, NCW magic `0xD69EA801` (G9). Independent NCW decoder to cross-check ours. | Medium. Small user base; some Delphi/Pascal-era code; the NICNT writer's 512000-byte metadata convention matches larsenwald. |
| 7 | [monomadic/hexfiend-templates](https://github.com/monomadic/hexfiend-templates) | none | Tcl | 2023-11-23 | Hex templates: NIS, NKS, NKX/NKR, FNTable, Program public tails | Layouts for `NIS/AudioSampleItem.tcl`, `NIS/Authorization.tcl`, `NIS/Ensemble.tcl`, NKR directory (G3, G11); Program V92 / VA8 public-tail u32 fields | Medium as layout evidence. Author says they are exploratory with omissions. Consistent with ni-file's own audit. Learn only. |
| 8 | [alset333/NKS-Rewrite-Meta](https://github.com/alset333/NKS-Rewrite-Meta) (`nksf-file-format.txt`) | none | JS / Java / Python | 2019-08-02 | NKSF = RIFF `NIKS`, `NISI` metadata chunk | Field-by-field NISI TLV notes with byte dumps from real `.nksf` files (G8) | Medium-low. Hand-annotated, README is TODO. The chunk layout is useful; the rest is unverified. |
| 9 | [gitter-badger/ni-file](https://github.com/gitter-badger/ni-file) (`nki.ksy`) | none | Kaitai / (no lang) | 2020-10-01 | Kontakt `hsin`/`DSIN` nesting; claimed LZ77 variant | Independent description of the `hsin`/`DSIN` nested blocks | Low-medium. Early draft. The `.ksy` is internally inconsistent (e.g. header length as `u8`). Historical. |
| 10 | [thetheosopher/Audiocity](https://github.com/thetheosopher/Audiocity) (`src/engine/LegacyNkiProbe.cpp`) | MIT | C++ | 2026-10-18 | Legacy NKI probe, sample path resolution (`Samples/`, recursive search) | Sample-reference resolution rules for NKI files | Low-medium. Small; probe-level. |
| 11 | [zhangdoa/KoEd](https://github.com/zhangdoa/KoEd) | LGPL-3.0 | C++ | 2024-06-03 (4★) | XML-form NKI parser/editor (tinyxml2) | Kontakt 1 to 4 XML side | Low. Few users. LGPL. |
| 12 | [B0ss-M/MrHighMan](https://github.com/B0ss-M/MrHighMan) (`src/formats/ni_kontakt.cpp`) | none | C++ | 2026-10-15 (4★) | Claims Kontakt 1 to 8 and monoliths | Its header says it is a **translation of ConvertWithMoss's Kontakt and nicontainer packages** (LGPL-3.0) | Unknown. Derivative of LGPL code with no license notice. **Do not copy.** |
| 13 | [cloinse/klm](https://github.com/cloinse/klm) (Kontakt Library Manager) | none | Dart | 2026-09-27 | None decoded. Discovers `*.nicnt`, `*_info.nkx`, `.nki/.nkm/.nkc` by filename; reads Service Center XML, macOS plists, installed-products JSON, Windows registry | Registration/state file locations (G9) | Medium as inventory documentation. Learn only. |
| 14 | [r-koubou/KSPCompiler](https://github.com/r-koubou/KSPCompiler) and [r-koubou/vscode-ksp](https://github.com/r-koubou/vscode-ksp) | MIT | C# / TS | 2026-10-26 | KSP script grammar, semantics, language server (not binary) | A maintained KSP grammar to check our KSP parser against (G12) | Medium-high for the KSP grammar. Not a binary decoder. |
| 15 | [Papyrus7914/TrId2-file-type-identifier](https://github.com/Papyrus7914/TrId2-file-type-identifier) and digipres TrID defs | not checked | n/a | n/a | Detection signatures (`.nbkt`, `trak`) | `.nbkt` uses the same NI FC container as `.nicnt` (G10) | Good for detection; not a decoder. |

Repo searches that returned nothing relevant (checked, not clones): `KontaktDecoder`, `komplete kontrol nks parser`, `nisound container`, `kontakt preset parser`, `ncw kontakt decoder`, `nicnt`, `nkx kontakt`, `ni-file kontakt`. Most "kontakt" hits were unrelated (contacts, beacons, the user's own repos).

Not reviewed, relevant but out of scope: `DmitriyZaitsev/Kontakt-Public` (5★, no detected license) and `faxinadu/Kontakt-Public` (21★, JavaScript, NOASSERTION). Both describe themselves as Kontakt builder resources; I did not open them.

Not searched or cloned: `LinuxSampler nkitool` (Kontakt 1 to 4.2.1 XML only; source at linuxsampler.org, not a GitHub repo; its own page says the 4.2.2 format change is unsupported); `Native Instruments` official Kontakt Lua API PDF and Creator Tools manual (official scripting docs, not file formats: `docs.native-instruments.com/online-guides/creator-tools-manual/`).

### Excluded

- `Eastmuamplify/Kontakt-8`, `GladiatorGutter/Native-Instruments-Kontakt-Full`, `EdgeDeskRecognize/Heavyocity-Damage-2-Kontakt-Library` are described as pre-activated or "full" installers. Not inspected or downloaded. Treat as likely malware or piracy bait; do not download or run.
- `DerpcatMusic/ni-file` is the user's own fork and not an external source.

## 2. Forum and web write-ups

- VI-Control, "binary file format from 4.2.2 not able to be decoded" (thread 22525). Says NKI up to 4.2.2 decodes to XML plus zlib; the binary format after that was opaque. Historical, date unclear. Consistent with the 4.2.2 break in our `docs/re/NI_FILE_*`.
- LinuxSampler nkitool page: NKI to and from XML for Kontakt 1 to 4; not updated for 4.2.2 onward.
- Steinberg forum and filext-type pages: generic only.
- KVR, ConvertWithMoss threads: confirm ConvertWithMoss is the active converter; a KVR poster says unencrypted NKIs are readable by most converters. A Kontakt-to-EXS converter's notes say most NKS files contain encrypted samples (not decrypted).
- NI community: NKSF is a RIFF with NISI, NICA, PLID, PCHK chunks; NISI is MessagePack-like, PCHK holds plugin state. Second-hand, unverified.
- NI community and VI-Control on `.nicnt`: a `.nicnt` holds Native Access registration; libraries lose it on reinstall.

No write-up was found that documents a NISound or Reaktor `.ens` container independently. No public NKX decryption tool that is still maintained was found, and I have not documented how to obtain the keys.

## 3. What we can learn or port: gaps in ni-file and ncw

Each gap names the project files that fill it and a verification step. "Verify" means check against a fixture we already hold locally. Do not add NI content to the repo.

### G1. FileContainer TOC entry layout: three readings disagree (highest priority)

The 640-byte entry in `NI FC TOC` is read differently by each source:

- `vendor/ni-file/src/file_container/mod.rs`: index (u64), 16 opaque bytes, 600-byte UTF-16 name (null-terminated), unknown u64, cumulative end offset (u64). Start is the previous end.
- ConvertWithMoss `kontakt/type/kontakt5/Kontakt5MonolithFormat.java:198–216`: same order, with the name decoded as UTF-16LE and the last u64 called "end of file offset".
- larsenwald `docs/kontakt-8-internals.md` §4: start offset (u64) at +0x00, index at +0x08, UTF-16 name at +0x20, "rebuilds byte-for-byte" on 7 vendor `.nicnt` files.
- monomadic hexfiend `Monolith.tcl`: the name is 600 bytes of ASCII, not UTF-16.

The Kontakt 8 doc puts the name at +0x20, while the other two readings put it at +0x18. Both lay out 640 bytes. Verify: take a `.nicnt` or Battery `.nbkt` from the local install (not in repo), print the TOC entries under each layout, and check which one reproduces the stored member sizes and offsets.

### G2. `.nicnt` ProductHints section and metadata size

larsenwald §4 says `MTD#1` `+0x90`/`+0x94` holds the metadata size (512000 in vendor files), the ProductHints XML starts at `0x100`, and the second MTD header is at `startOffset + 256`. PresetConverter `NICNT.cs:231–311` writes the same layout (512000 metadata size, 256 zero bytes, second MTD, TOC). `ni-file` reads only the member table and ignores the XML section. KONTRA has `crates/sampler-kontakt/src/resources.rs` and `resource_container.rs`; I did not verify whether they read ProductHints. Verify with the same fixtures as G1.

### G3. NIS item-type names: 24 named IDs vs ni-file's sparse table

`docs/re/NI_FILE_COVERAGE_AUDIT.md` says that nine NIS property files in `vendor/ni-file` are empty: `AudioSampleItem`, `AutomationParameters`, `BankContainer`, `BinaryChunkItem`, `InternalResourceReferenceItem`, `Module`, `ModuleBank`, `PictureItem`, `PresetContainer`. ConvertWithMoss `format/ni/nicontainer/NIContainerChunkType.java` names a single numeric space: 1 TERMINATOR, 3 BNI_SOUND_PRESET, 4 BNI_SOUND_HEADER, 100 BANK, 101 AUTHORING_APPLICATION, 102 BANK_CONTAINER, 103 PRESET_CONTAINER, 104 BINARY_CHUNK_ITEM, 106 AUTHORIZATION, 108 SOUNDINFO_ITEM, 109 PRESET_CHUNK_ITEM, 110 EXTERNAL_FILE_REFERENCE, 111 RESOURCES, 112 AUDIO_SAMPLE_ITEM, 113 INTERNAL_RESOURCE_REFERENCE_ITEM, 114 PICTURE_ITEM, 115 SUB_TREE_ITEM, 116 ENCRYPTION_ITEM, 117 APP_SPECIFIC, 118 CONTAINER_ROOT, 120 AUTOMATION_PARAMETERS, 121 CONTROLLER_ASSIGNMENTS, 122 MODULE, 123 MODULE_BANK.

ni-file's `nis/container/data/item_type.rs` is keyed by domain (NISD, NIK4, RKTR). Verify whether the CWM IDs are per-domain or global before adopting them. Use the hexfiend `NIS/*.tcl` templates to cross-check field layouts for the nine empty types. Keep unknown IDs raw, as ni-file already does.

### G4. Kontakt chunk IDs: ConvertWithMoss adds one; names differ on three

Diffing the ID tables: ni-file's `vendor/ni-file/src/kontakt/chunk.rs` maps 97 IDs, ConvertWithMoss's `kontakt/type/kontakt5/KontaktPresetChunkID.java` names 30. ConvertWithMoss has one ID ni-file does not map: `0x48 MULTI_CONFIGURATION`. Name conflicts to resolve by fixture, not by adoption:

- `0x4B`: CWM `FILENAME_LIST_EX`, ni-file `FNTableImpl`.
- `0x3A` to `0x3C`: CWM `PARAMETER_ARRAY_8/16/32`, ni-file `BParameterArraySer{BParFX8, BParInternalMod16, BParExternalMod32}`. ni-file's names carry more information.
- `0x3D`: CWM `FILENAME_LIST`, ni-file `FileNameListPreK51`. Matches the audit's pre-K51 note.

### G5. Magic constants missing from ni-file (verified by byte-level grep)

Present in ni-file: `0x5EE56EB3`, `0xB36EE55E`, `0x5AE5D6A4`, `0x1290A87F`, `0x722A013E`, `0x7FA89012`, `0xEA37631A`, `0x2AE905FA`, `0x4916E63C`, `0x16CCF80A`, `0x2F5C204E`, `0xB00EE1AE`, `0x5E70AC54`.

Absent (grep found no hits in `vendor/ni-file/src` or `crates/sampler-kontakt/src`): `0x01EF85AB` (Kontakt 2 multi, LE), `0x3E012A72` (Kontakt 2 header, BE), `0x1A6337EA` (Kontakt 4.2 instrument header, LE), `0x0040179F` (Kontakt 2 NKR raw-sample ID). The first three and the fourth come from `Magic.java` in ConvertWithMoss. Verify each against a fixture before adding; these are IDs, not decoders.

### G6. Kontakt 5 modulators and file lists: reader details to cross-check

ConvertWithMoss `kontakt/type/kontakt5/InternalModulator.java:274–367`: a `mainVersion` byte (0 means re-read; must be 1); version 0x80 or 0x81; modulated-parameter list with 0xFFFF sentinels; curve points required for 0x81. ni-file's `internal_mod.rs` covers the 0x80/0x81 split; check the sentinel and curve-point rules. `FileList.java:46–50`: versions 2 and 3 only, which matches the audit's FNTable 2 and 3 note. Verify with Kontakt 5 fixtures.

### G7. Kontakt 5 monolith TOC reader (cross-check)

ConvertWithMoss `Kontakt5MonolithFormat.java` reads the same F0/F1 markers and 600-byte TOC sections as ni-file. Use it as a second reader for G1 rather than as a new feature.

### G8. NKSF (RIFF `NIKS`) NISI metadata

`alset333/nksf-file-format.txt` documents a `NISI` chunk with a MessagePack-like TLV: a 3-bit type and a 5-bit size per value, a map of key/value pairs, lists, and string values. Known keys: UUID, author, bankchain (3 slots), comment, deviceType (`INST`), modes, name, types (2-D list), uuid, vender. The notes show byte dumps from real Arturia `.nksf` files. ni-file's `nksf/mod.rs` is a skeleton (per the audit). Implement a read-only NISI reader; verify on a user-owned `.nksf` (not in repo). NICA, PLID, and PCHK are second-hand from NI community posts. Do not assume their layout.

### G9. Native Access and NKS library state (not decoded anywhere in ni-file)

- `perivar/PresetConverter` `NKS.cs`: reads `nklibs_info.userdb` and `Settings.cfg`, and a magic table: `0x5E70AC54` directory, `0x16CCF80A` encrypted file, `0x4916E63C` file, `0x2AE905FA` content file, `0x7FA89012` NKI, `0xD69EA801` NCW.
- `cloinse/klm` discovers libraries by filename (`library_candidate_scanner.dart`) and reads Service Center XML, installed-products JSON, plists, and the Windows registry.
- `larsenwald` §1 to §3: `komplete.db3` schema and `k_content_path` / `k_sound_info` tables.

These matter for KONTRA library discovery and inventory, not for audio decoding. Keep them out of the codec path. Do not handle `HU`/`JDX` authorization values.

### G10. Battery `.nbkt` shares the FC container

TrID `Papyrus7914/TrId2-file-type-identifier` (database entry `(nbkt) ... Native Instruments BATTERY kit.yaml`): detection patterns `/\ NI FC MTD  /\` at offset 0 and `/\ NI FC TOC  /\` at offset 288, plus strings `DEVICETYPEFLAGS`, `$ACTUAL_VEL_PERCENTAGQ`, `PATCH.NBKT`, `SOUNDTYPE`, `UI_LABEL $TRIGGER`, `%GROUPS_SELECTED`. The offset 288 agrees with ni-file's `read_table` (16-byte magic, 256-byte header, two u64 counts = 288), so the first-MTD layout is consistent. Action: teach `detect.rs` the `.nbkt` signature. A Battery decoder is not in any public project.

### G11. NKR directory fields: `set_id`, `referenceType`, padding

`maxton/nkxtract` `Nks.cs` reads version (`0x111`), id, two unknown u32s, entry count, one more unknown u32, then entries. `monomadic/hexfiend-templates` `NKX/NKR.tcl` names them `set_id`, `?`, `numItems`, `padding`, and per entry `itemLength` (u16), `offset` (u32), `referenceType` (u16), UTF-16 name of `itemLength - 8`. ni-file's audit says "directory filenames, member offsets/sizes, flags" and does not mention `set_id` or `referenceType`. Verify the meaning of `referenceType` on the local NKX fixtures before naming it.

### G12. KSP grammar cross-check

`r-koubou/KSPCompiler` (MIT) is a maintained KSP grammar and semantic checker. Use it to check our KSP parser (KSP is script text inside NKI, so it is not a binary decoder). `docs/re/KSP_SEMANTICS.md` and `KSP_GAPS.md` already list the gaps. Check them against the compiler's grammar first.

### Not gaps (already covered by ni-file or ncw)

- NCW block, flag, and format-word semantics (see `vendor/ncw/FORMAT.md`).
- Monolith F0/F1 markers, NKR `0x110`/`0x111`, FNTable v2/v3, ladder filter, snapshot versions, modulation objects, Kon2-to-Kon8 NKI signatures. All documented in `docs/re/NI_FILE_COVERAGE_AUDIT.md`.

## 4. License notes: what we may copy, and what we may only learn from

KONTRA is Apache-2.0 (`LICENSE`). `docs/LEGAL.md` and `THIRD_PARTY.md` already flag open questions. This section records licenses only; it is not legal advice. `docs/LEGAL.md` asks for counsel review, and that review still applies here.

| Posture | Projects | What it means for KONTRA |
|---|---|---|
| Permissive: may copy with attribution and notice | `monomadic/ncw` (MIT OR Apache-2.0, already vendored), `perivar/PresetConverter` (MIT), `thetheosopher/Audiocity` (MIT), `r-koubou/KSPCompiler` and `vscode-ksp` (MIT) | Copy code if we keep the notice. Our `vendor/ncw` already does. |
| Copyleft, library use: LGPL-3.0 | `git-moss/ConvertWithMoss`, `zhangdoa/KoEd` | Copying code would bring LGPL obligations into an Apache-2.0 tree. Prefer reimplementing from the documented facts (layouts, magic numbers) in our own code, with a citation in comments. |
| Copyleft, strong: GPL-3.0 | `maxton/nkxtract`, `JimiHFord/unnks` | Learn only. Do not copy code, and do not copy the close structure of the code either. `THIRD_PARTY.md` already flags `access.rs` as a provenance question. Do not add more GPL-derived code. |
| No license: all rights reserved by default | `cloinse/klm`, `monomadic/hexfiend-templates`, `alset333/NKS-Rewrite-Meta`, `gitter-badger/ni-file`, `larsenwald/kontakt-8-library-manager`, `B0ss-M/MrHighMan`, `Ma5onic/ni-file` (the pin we vendor) | Learn only. Do not copy code or templates. Layouts and magic numbers can be described in our own docs with a citation. Do not ship any fixture from these repos. |

Separate from code licenses:

- Format facts (field offsets, magic numbers, byte orders) are documented in our own words with a citation. This is how `docs/re/NI_FILE_*.md` already works.
- Vendor content (samples, artwork, scripts, `.nicnt` payloads) is not in the repo and should stay out of it. `docs/LEGAL.md` lists the rights questions for those.
- `vendor/ni-file` has no redistribution grant. `THIRD_PARTY.md` records a 2024 NI takedown against `monomadic/ni-file`. The original repo returns 403 now. Replacing the dependency with reimplemented code with clear provenance is the open path `docs/LEGAL.md` already recommends.
- NI's EULA (dated 2026-07-01, per `docs/LEGAL.md`) prohibits reverse engineering in §3.1 (from the LEGAL review). That is a contract term that counsel should weigh; this scout does not weigh it.
- Key material: `nkxtract`, `unnks`, and our `access.rs` all touch per-library keys. This scout does not document how keys are obtained, and KONTRA should not bundle or derive keys (`ni-file` README and `docs/LEGAL.md` agree).

## 5. Verification queue (suggested order)

1. G1 (TOC entry layout): one fixture, three readings. Highest payoff: it decides how the `.nicnt` and Battery `.nbkt` monoliths parse.
2. G5 (four missing magic constants): add only after G1 proves the layout.
3. G3 and G4 (item and chunk names): name the nine empty NIS types and the one missing chunk ID.
4. G8 (NKSF NISI reader): a small read-only reader, verified on a local `.nksf`.
5. G10 (`.nbkt` detection): a signature addition in `detect.rs`.
6. G12 (KSP grammar cross-check): compare against `KSPCompiler`'s grammar.

## 6. Limits

- Nothing was built or run. Clones were read only, with no code executed.
- Tests and fixtures were not checked against the NI files; the layouts above come from reading source and docs.
- License values are GitHub's SPDX fields. Several "none" results are repos with no `LICENSE` file, which GitHub reports as no license. The Battery TrID license was not checked.
- Several GitHub repo searches hit secondary rate limits (`KontaktDecoder`, `nkxtract`, `komplete kontrol nks parser`, and others). I reran the three still needed after the window reset; the rest ran once. A relevant repo that none of the searches surfaced could still exist.
