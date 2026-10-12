# Kontakt binary records: bounded serializer research

Research date: 2026-10-08. The initial pass established two implementation candidates: the complete wire shape of **VoiceGroups `0x32` / VoiceLimit `0x2b`, version `0x60`**, and **Program `0x28` version dispatch plus its common public prefix**. The public suffix table below now incorporates the [second-pass complete framing evidence](NI_FILE_PROGRAM_PUBLIC_TAILS.md), which has a separate pinned source/check ledger. Neither pass establishes decoded write support, VST3 serializer equivalence, or DSP/audio parity. No proprietary executable, constructor, host, or audio path was run.

## Identities and evidence

| Input | Exact identity | Use here |
| --- | --- | --- |
| Root workspace | `/home/derpcat/.t3/worktrees/KONTAKTO/t3code-80fe786b`, HEAD `0cb7a8a0b4d43086596a64c77320caa1b26d6d98` | Existing standalone export and binary metadata; only this document and the new artifact directory were authored |
| Reference V2 | `/home/derpcat/.t3/worktrees/KONTAKTO/decipher-readers-v2`, committed HEAD `2fb8c926dd39bb7ac26a84d4806f42de14b6630e` | Pinned reader/docs baseline; concurrent safety edits are outside this report's baseline |
| Kontakt standalone | `Kontakt 8.exe`, PE version `8.13.1`, image base `0x140000000`, SHA256 `0fe6356e0879d058b6e5b73507c54c5e345cea451b35287c974e438291d4dae8` | All serializer virtual addresses below |
| Actual VST3 payload | `Kontakt 8.vst3plugin`, PE version `8.13.1`, image base `0x180000000`, SHA256 `8ed90c4b9dd2bb2c5cc45b64dea144f6cca245c706457de4aa72c643a8acc09d` | Identity checked only; standalone serializer addresses are **not** payload addresses |
| Public templates | [monomadic/hexfiend-templates, commit `c6f309bae04a03967b94f54d81dc2050f827a1e8`](https://github.com/monomadic/hexfiend-templates/tree/c6f309bae04a03967b94f54d81dc2050f827a1e8/Kontakt) | Fetched and read; relevant files retained locally |
| Public ni-file repository | `https://github.com/monomadic/ni-file` | **Not fetched**. The task reports HTTP 451; the actual local vendored docs/source were read instead |

The binaries live under `/home/derpcat/.wine/drive_c/Program Files/Common Files/VST3/Portapotty/Kontakt 8/x64/`. The outer `Portable.vst3` wrapper was not used. Existing helpers `artifacts/engine-analysis-2026-10-07/trace_plugin_topology.py` and `deep-dsp/verify_machine_code.py` supplied the PE identity/disassembly pattern; importing the latter would initialize an emulator, so the new check uses only their existing `pefile`/`capstone` dependencies.

The root graph was queried before root-source discovery (`graft map`); it indexes production code rather than this ignored binary evidence. V2 had no graph. Relevant committed V2 source/docs were frozen in [reference-v2](../artifacts/ni-file-records-2026-10-08/reference-v2/), because other agents are actively editing that worktree. [sources.json](../artifacts/ni-file-records-2026-10-08/sources.json) pins the identities and hashes these inputs. Public source snapshots are in [hexfiend-templates/Kontakt](../artifacts/ni-file-records-2026-10-08/hexfiend-templates/Kontakt/).

**Citation convention:** `P:L` means exact line `L` of `artifacts/engine-analysis-2026-10-07/kontakt-engine/complete-export/pseudocode.c`. The selected functions are copied into [pseudocode](../artifacts/ni-file-records-2026-10-08/pseudocode/) with original line numbers retained; [index.json](../artifacts/ni-file-records-2026-10-08/pseudocode/index.json) maps their spans. `V2:path:line` refers to the committed source snapshot above. Large-export warning status is preserved in [decompilation-status.json](../artifacts/ni-file-records-2026-10-08/decompilation-status.json). A decompiler's inferred types and names are not source recovery; the check independently reads original PE instructions, pointers, and branch tables.

## 1. VoiceGroups `0x32`: concrete `0x60` layout

### Why this is a real gap

At the pinned V2 base, `V2:vendor/ni-file/src/kontakt/objects/voice_groups.rs:27–54` reads an unstructured flag/version, a bare VoiceLimit body, **eight** mask bytes, then loops over only **eight** bit positions and returns an empty groups vector. `voice_group.rs:15–17` consumes no bytes. The local document `doc/presets/Kontakt/VoiceGroups.md:3–5` gives the ID/count but calls the groups StructuredObjects. This description is too broad: these entries are unstructured inline serializable records.

There is already a substantially better path in `V2:crates/sampler-kontakt/src/library.rs:376–416,440–470`: `voice_limit()` requires header `[0,0x60,0]`, and `Translation::voice_groups()` reads a 16-byte mask followed by one inline limit per set bit and rejects trailing bytes. Its callers are the Program translation at `library.rs:179–184`; assigned group limits are looked up at `library.rs:526`. Reuse the wire reader rather than create an incompatible third parser. That translator discards names and clamps voice/fade values for its IR; a loss-preserving ni-file decoder should retain the original fields.

### Exact native ownership and call path

The standalone RTTI record `.?AVBVoiceLimit@@` identifies a 10-slot vtable at RVA `0x4725438`, receiver displacement 0. Its slot 1 getter `0x140611850` returns SerType `0x2b` (`P:2879464`); slot 2 `0x1406118f0` returns `0x60` (`P:2879487`); slot 3 `0x14050f330` returns structured flag 0 (`P:2597166`). Slot 8 points to reader `0x140d049b0`; slot 9 points to writer `0x140d13370`. The check compares these RTTI pointers with the actual PE vtable bytes and inspects the getter instructions.

Program's child reader `0x140d063a0` calls `0x140d1e350` (`P:4723540`). That function:

1. Reads/checks outer SerType `0x32`, then consumes its u32 length (`P:4746521–4746529`).
2. Reads the instrument-wide inline BVoiceLimit through generic reader `0x14093d6c0`, at native Program offset `0x1fb30` (`P:4746530`).
3. Reads **16 u8 mask bytes** (`P:4746548–4746553`).
4. Visits `g=0..127`, testing `mask[g >> 3] & (1 << (g & 7))` (`P:4746554–4746557,4746596–4746597`). Set bits consume the structured flag, version, and limit body; clear bits consume no record (`P:4746558–4746577`).

The inline reader `0x14093d6c0` checks the flag against vtable slot 3 and the incoming version against slot 2 (`P:3686846–3686859`). BVoiceLimit's own body reader accepts **only** `0x60`, despite the generic reader's “version ≤ current” comparison (`P:4720810–4720825`). A lower version is not automatically supported.

### Wire schema

All multi-byte values below are little-endian in the studied ni-file path. Each inline limit is:

| Byte position | Width | Field | Native BVoiceLimit offset / evidence |
| --- | --- | --- | --- |
| 0 | 1 | structured flag = 0 | Getter `0x14050f330`; generic framing |
| 1 | 2 | version = `0x60` | Getter `0x1406118f0` |
| 3 | 4 | name length `N`, UTF-16 code units | Reader `0x140cfbaf0 → 0x142a784b0`; `P:4710223–4710224,13245593–13245602` |
| 7 | `2*N` | UTF-16 name, no extra terminator field | Native string at `+8` |
| `7+2*N` | 2 | kill mode, signed i16 in local decoder | `+0x2c`; `P:4720812–4720813` |
| `9+2*N` | 1 | prefer released | `+0x30`; `P:4720814–4720815` |
| `10+2*N` | 4 | maximum voices, signed i32 in local decoder | `+0x28`; `P:4720816–4720817` |
| `14+2*N` | 4 | stolen-voice fade milliseconds, signed i32 | `+0x34`; `P:4720818–4720819` |
| `18+2*N` | 4 | exclusion group, signed i32 | `+0x38`; `P:4720820–4720821` |

An inline record is exactly `22+2*N` bytes. Field meanings and signed interpretations come from `V2:vendor/ni-file/src/kontakt/objects/voice_limit.rs:5–35` and the existing sampler translator, while widths/order/native destinations are corroborated by the binary. The reciprocal writer `0x140d13370`, `P:4729534–4729545`, writes the same string/u16/u8/three-u32 sequence. Integer primitives read exactly 1, 2, or 4 bytes (`P:13243704,13244041,13244096`) and swap on the alternate stream-endian mode.

The **outer chunk** is:

```text
u16 SerType = 0x32
u32 body_length
inline limit for the whole instrument (flag/version/name/fields)
u8 presence_mask[16]
inline limit for each set bit g, in ascending g order
```

There is **no second VoiceGroups header before the instrument limit**, no u16 `0x2b` tag or u32 length before each group limit, and no stored group count/index. The outer body begins with the instrument limit's own `[0,0x60,0]`. For mask population `K`, body size is `22+2*Ninstrument + 16 + Σ(22+2*Ng)` over the K present groups. Empty names and no present groups produce a 38-byte body and a 44-byte outer chunk.

The writer `0x140d145f0` opens chunk `0x32`, writes the instrument limit using generic writer `0x14097d650` (`P:4733793–4733794`), builds all 128 mask bits (`P:4733795–4733897`), writes 16 bytes (`P:4733898–4733902`), emits selected inline records in ascending order (`P:4733903–4733938`), and closes the chunk (`P:4733939`). Generic writer framing is at `P:3744908–3744913`; chunk opening/length patching at `P:4713925–4713932,4713736–4713741` shows that the length excludes the six-byte outer header.

Native omission defaults are empty name, kill mode 1, maximum voices 1, prefer-released nonzero, fade 10, **exclusion -1** (`P:4733813–4733818`). Therefore a clear bit means a default-valued native group rather than a serialized empty record. The local VoiceLimit comment's “off (0)” at `voice_limit.rs:21–22` is not the native omission sentinel. Do not silently convert -1 to 0 in a wire decoder; exact UI numbering remains separate evidence.

The pinned [public VoiceGroups template](https://github.com/monomadic/hexfiend-templates/blob/c6f309bae04a03967b94f54d81dc2050f827a1e8/Kontakt/VoiceGroups.tcl) corroborates the instrument fields and sixteen trailing bytes (`:5–24,27–35`), but leaves those bytes unnamed and stops before mask-selected group records. It cannot by itself establish the complete shape above.

**Implementation candidate:** a version-`0x60` decoder retaining one instrument limit plus 128 indexed optional overrides, using the existing translator's framing/mask behavior and the existing VoiceLimit field reader. Validate flags/version, names against remaining bytes, complete set-bit records, and exact body consumption. Preserve raw chunks for other versions. A typed writer could later emit this exact order while preserving scalar/name values; application acceptance is still unverified.

## 2. Program `0x28`: proven dispatch, bounded fields

### Ownership, framing, and versions

The standalone `.?AVBProgram@@` serializable vtable is at RVA `0x4e98350`, object displacement **16**. Its ID/version/flag getters return `0x28`, **`0xb5`**, and 1 (`0x140993f20`, `0x1409940d0`, `0x14051b400`; `P:3772034,3772151,2611389`). This is wider than the vendored comment's `0x80..0xaf` at `V2:vendor/ni-file/src/kontakt/objects/program.rs:13–16`.

| Vtable slot | VA | Proven role |
| --- | --- | --- |
| 8 | `0x140756e50` | Reads length/private, length/public, length/children by calls through offsets `0x58`, `0x50`, `0x60`; `P:3221977–3221984` |
| 9 | `0x140756ef0` | Writes/paches those three section lengths; `P:3222252–3222313` |
| 10 / 13 | `0x140d0d4b0` / `0x140d16da0` | Public reader / writer |
| 11 / 14 | `0x140d0a2c0` / `0x140d161c0` | Private reader / writer |
| 12 / 15 | `0x140d063a0` / `0x140d145f0` | Children reader / writer |

The local `StructuredObject` framing at `structured_object.rs:17–56` therefore has direct native support: outer chunk `u16 id/u32 body_size`, then flag=1/u16 Program version, then private/public/children each with u32 section length. Native `0x140756e50` merely consumes section lengths before dispatch; its pseudocode does not prove hard section-bounded subreaders. ni-file should retain its own bounded-section parsing.

Three native two-level jump tables use `version-0x80`, compare against `0x35`, and reject holes. Original instruction anchors are `0x140d0d592`, `0x140d0a31b`, `0x140d063c9`; table RVAs/targets are recorded in [check-results.json](../artifacts/ni-file-records-2026-10-08/check-results.json). All three accept the exact set **`{0x80,0x82,0x90,0x91,0x92} ∪ {0xa0..0xb5}`**. A range guard `0x80..=0xb5` would wrongly accept holes. Native acceptance is evidence for dispatch topology, not proof ni-file can decode every accepted layout.

### Public prefix and suffix dispatch

`V2:program.rs:50–81` ignores `_version`, reads one historical prefix, and always returns `None` for its two filename fields. `Program::params()` feeds that reader at `program.rs:94–97`; `V2:crates/sampler-kontakt/src/library.rs:160–170` consumes the result. Binary `0x140d0d4b0` has eleven public case bodies. The static check confirms the same **24 direct helper calls** implementing the common 23-field prefix in all eleven bodies, including the extra loading-flags helper.

The prefix, starting at public-section offset 0, is:

| Ordered fields | Wire widths | Corresponding native receiver offsets |
| --- | --- | --- |
| name | u32 UTF-16 count + `2*N` | `+0x12088` |
| total sample bytes | f64 | `+0x21b68` |
| transpose | i8 | `+0x21b7c` |
| volume, pan, tune | three f32 | `+0x1fb04,+0x1fb08,+0x1fb0c` |
| low/high velocity, low/high key | four u8 | `+0x21b80..+0x21b83` |
| default keyswitch | i16 | `+0x21b84` |
| DFD preload, library ID, fingerprint, loading flags | four 32-bit words | `+0x21b60,+0x1bee0,+0x1c008,+0x1c00c`; loading-flags helper `0x140d0ee85` clears runtime bit 0 after reading the wire word |
| group solo | u8 | `+0x21b7d` |
| category icon index | i32 | `+0x120a8` |
| credits, author, URL | three counted UTF-16 strings | `+0x120b0,+0x120d0,+0x120f0` |
| category 1, 2, 3 | three i16 | `+0x12110,+0x12112,+0x12114` |

Names/signedness here follow the local `doc/presets/Kontakt/BProgram.md:20–46` and `program.rs:21–74`; reads/destinations are at `P:4740469–4740510`, reciprocally written at `P:4736763–4736785`. These offsets are relative to the serializable receiver; add 16 to obtain offsets from the RTTI complete-object pointer. The prefix's fixed part is 54 bytes plus four u32 string lengths and their UTF-16 contents: **`70 + 2*(Nname+Ncredits+Nauthor+Nurl)`** bytes.

Use `P` for that prefix, `W` for a counted UTF-16 string and `F` for a **signed i32 filename-table reference**. Negative references represent an empty filename; preserve the exact negative sentinel rather than normalize it. Nonnegative references require the context's table. The [second public-tail investigation](NI_FILE_PROGRAM_PUBLIC_TAILS.md) includes the post-switch reads omitted from the initial case-local table and verifies the following complete suffix order against original machine code.

`F0` is the first reference; `F1` targets receiver `+8`; `F2` targets receiver `+0x11c38`. Their wire identities are distinct. `D0,D1` are consumed legacy strings, `W0,W1` retained strings, `U0..U2` neutral u32 values and `Q0` a neutral byte. `S0,S1` are inline BNISoundData records. `B0` is u32 byte count followed by exactly that many raw bytes.

| Version(s) | Native case entry VA | Complete wire additions after P | Minimum bytes |
| --- | --- | --- | --- |
| `80,82,90` | `0x140d0d5b8` | none | 0 |
| `91,92,a0,a1` | `0x140d0d71b` | F0 | 4 |
| `a2..a5` | `0x140d0d71b` | F0 F2 | 8 |
| `a6` | `0x140d0d890` | F0 D0 D1 W0 F1 F2 | 24 |
| `a7` | `0x140d0da9b` | F0 W0 F1 F2 | 16 |
| `a8..ae` | `0x140d0dc22` | F0 W0 W1 F1 F2 | 20 |
| `af` | `0x140d0ddb7` | F0 W0 W1 U0 F1 F2 | 24 |
| `b0` | `0x140d0df5a` | F0 W0 W1 U0 S0 F1 F2 | 28 |
| `b1` | `0x140d0e10b` | F0 W0 W1 U0 S0 Q0 S1 F1 F2 | 33 |
| `b2` | `0x140d0e2d8` | F0 W0 W1 U0 S0 Q0 S1 U1 F1 F2 | 37 |
| `b3` | `0x140d0e4b3` | F0 W0 W1 U0 S0 Q0 S1 U1 B0 F1 F2 | 41 |
| `b4,b5` | `0x140d0e69d` | F0 W0 W1 U0 S0 Q0 S1 U1 B0 U2 F1 F2 | 45 |

Minima assume empty strings/bytes and absent S bodies. `F2` is read after the switch for **every accepted version greater than a1**, including a2–a5. `F1` starts at a6; a6 reads it directly, later versions use the common path. `W1` is always consumed from a8 onward; context affects its native destination, not wire presence. Exact case/post-switch/helper spans, reciprocal current-writer evidence and the bounded Metadata2/Groups1 body grammar are in the [public-tail report](NI_FILE_PROGRAM_PUBLIC_TAILS.md). The current writer does not prove a historical-version writer.

Both S records have flag 0, inline version 1 and a presence byte; they have no SerType or total length. Generic StructuredObject parsing would consume the remaining suffix and is unsuitable here. Present bodies contain positional Metadata2 version 2, Groups1 version 1 and an optional 16-byte value. Unknown nested versions have no proven skip grammar. `B0` byte framing is verified; encoding and semantic meaning remain unresolved.

The pinned templates corroborate parts of the historical prefix and suffix evolution ([V92](https://github.com/monomadic/hexfiend-templates/blob/c6f309bae04a03967b94f54d81dc2050f827a1e8/Kontakt/ProgramV92PublicParams.tcl), [VA6](https://github.com/monomadic/hexfiend-templates/blob/c6f309bae04a03967b94f54d81dc2050f827a1e8/Kontakt/ProgramVA6PublicParams.tcl), [VA8](https://github.com/monomadic/hexfiend-templates/blob/c6f309bae04a03967b94f54d81dc2050f827a1e8/Kontakt/ProgramVA8PublicParams.tcl)). Their extra VA5/VA6 words are now accounted for by F2. Historical labels such as wallpaper still do not establish F1/F2 member semantics; retain neutral identities.

### Private dispatch: what is established and what remains opaque

The private reader `0x140d0a2c0`, `P:4731779–4732905`, dispatches the same accepted version set into thirteen case groups:

```text
80 | 82,90 | 91 | 92,a0 | a1,a2,a3 | a4 | a5 | a6,a7,a8
a9,ab | aa | ac | ad,ae,af,b0,b1,b2,b3,b4 | b5
```

The exact entry VAs are in `check-results.json:program_dispatch.private.groups`. The leading u32 is **not proven to be another serialization version**: helper `0x140cf03c0` maps wire 0→native 0, 1→1, 2→3 and rejects other values (`P:4702762–4702778`). Current private writer `0x140d161c0` reverses this mapping (`P:4736445–4736460`). Consequently the baseline `ProgramDataPrivateParams::read`, `program.rs:149–154`, calling it “version” and asserting `<2` misses valid wire discriminator 2. Its scalar prefix is followed by translated filenames and nested serializers/arrays (`P:4731846–4731854` for `0x80`). Those nested schemas are not established by this task; do not claim full private-field semantics from anonymous memory offsets.

`b5` adds a string to receiver `+0x11e78` and shares a two-u32 continuation at `+0x11c14,+0x11c18` (`P:4732899–4732904`). Earlier `aa` already jumps to this continuation (`P:4732513–4732602`); it is not a blanket “introduced in b5” assertion. The exact helper calls and version branches remain the evidence until member semantics are identified.

**Implementation candidates:** retain Program's three raw bounded sections for every unknown version; expose the common public prefix only for explicitly allowed versions, with an opaque suffix rather than silently dropping it. A concrete `a8..ae` extension can decode F/W/W/F with raw names/indices retained, once a real preset checks section exhaustion. Keep `b0..b5` suffixes opaque until their S records have a verified length/framing schema. Keep private sections opaque beyond individually verified fields/discriminator; there is no complete private editor here.

## Targeted check, result, and remaining evidence

Run from the root workspace:

```bash
python artifacts/ni-file-records-2026-10-08/check_records.py
```

[check_records.py](../artifacts/ni-file-records-2026-10-08/check_records.py) uses existing dependencies and performs only static reads. The completed check verifies both image hashes/bases, all frozen source hashes, two RTTI vtables against original PE pointers, four ID/version getters and two flag getters, eleven original instruction anchors, the six VoiceLimit field-read calls, the common public-prefix calls in all eleven public case bodies, and **162 original jump-table entries** across Program's three dispatchers. It records original function-range hashes using the existing unwind metadata. [check-results.json](../artifacts/ni-file-records-2026-10-08/check-results.json) contains the successful result and exact targets.

This check will fail if the mask width, group iteration/indexing, recognized version set, public case grouping, prefix call order, vtable ownership, selected binary bytes, or pinned source evidence changes. It is a serializer **evidence regression**, not execution of a decoder against real presets and not live Kontakt integration. No Rust build/test/clippy was run.

| Unknown / blocker | Precise next evidence |
| --- | --- |
| Real-file acceptance of reconstructed VoiceGroups and Program suffixes | Small unprotected raw record fixtures at `0x60` with named groups 0, 7, 8, 63, 64, 127, plus Program `a6/a7/a8/b5` private/public sections with nonempty strings and valid filename-table context. Test exact consumption and byte-preserving serialization separately from IR normalization |
| Native semantic names for new Program scalars / S children | RTTI/vtable ownership and reader/writer tracing for each nested record, plus controlled same-version preset changes identifying the affected member |
| Standalone-to-VST3 serializer match | Payload RTTI/vtable and machine-code call-graph match in the separately pinned `0x180000000` image. Equal product versions and relative addresses are insufficient |
| PatchType 5 / NKZ and Kontakt 8 application signature | Not selected in this bounded task; no complete NKZ layout or new signature mapping claimed. Need real type-5 fixtures and native dispatch/xrefs or explicit signature-bearing headers |
| DSP/format/plugin parity | Separate numerical and lifecycle evidence. Reconstructing serializer framing does not reconstruct the DSP functions stored by those records |

Public templates establish useful historical observations; the local reader/docs identify practical gaps; the original bytes establish ownership, widths, and dispatch boundaries. These two record families are concrete schema candidates, with the remaining unknowns preserved above.
