# BProgram public suffixes: bounded standalone evidence

Research date: 2026-10-08. This adds the missing public-tail grammar to [NI_FILE_BINARY_RECORDS.md, section 2](NI_FILE_BINARY_RECORDS.md#2-program-0x28-proven-dispatch-bounded-fields). It establishes byte-consumption candidates for the exact native dispatch set **`{0x80,0x82,0x90,0x91,0x92} ∪ {0xa0..0xb5}`**, including both nested serializers and the `b3` byte string. It does not establish private-section layouts, application acceptance, historical writers, or standalone/VST3 equivalence. No executable, plugin, audio path, sample, license/authentication operation, or Rust build/test was run.

The main corrections to carry into implementation are:

- `a8..b5` always contain the second counted UTF-16 string. Context selects its destination, never its presence.
- `a6` contains **three** counted UTF-16 strings: two discarded legacy values, then a retained value. `a7` contains one; `a8..b5` contain two.
- Both nested serializers are `SER::BNISoundData`, unstructured inline **flag 0/version 1**, with no SerType or byte-length wrapper. Their present bodies have explicit counted lists and an optional 16-byte value.
- `b3` adds **u32 byte count + exactly that many bytes**, not UTF-16.
- A second, distinct trailing filename reference exists for **`a2..b5`**, including `a2..a5` and `a6`. The earlier report's case table did not enumerate this post-switch read. Keep its wire name neutral; the current `wallpaper_filename` name is not proved by the historical template labels.

## Exact identities and reproducibility

| Input | Pinned identity |
| --- | --- |
| Root report workspace | `/home/derpcat/.t3/worktrees/KONTAKTO/t3code-80fe786b`, HEAD `0cb7a8a0b4d43086596a64c77320caa1b26d6d98` |
| V2 baseline | `/home/derpcat/.t3/worktrees/KONTAKTO/decipher-readers-v2`, committed HEAD `2fb8c926dd39bb7ac26a84d4806f42de14b6630e` |
| Standalone PE | `/home/derpcat/.wine/drive_c/Program Files/Common Files/VST3/Portapotty/Kontakt 8/x64/Kontakt 8.exe`; PE version `8.13.1`; image base `0x140000000`; SHA256 `0fe6356e0879d058b6e5b73507c54c5e345cea451b35287c974e438291d4dae8` |
| Static decompilation | `artifacts/engine-analysis-2026-10-07/kontakt-engine/complete-export/{decompilation.tsv,pseudocode.c}`; frozen bounded function spans in the new evidence directory |
| Prior pinned evidence | `artifacts/ni-file-records-2026-10-08/{sources.json,check-results.json,pseudocode,reference-v2}`; reused without changes |
| New evidence | [sources.json](../artifacts/ni-file-program-public-tails-2026-10-08/sources.json), [pseudocode index](../artifacts/ni-file-program-public-tails-2026-10-08/pseudocode/index.json), [original-image disassembly](../artifacts/ni-file-program-public-tails-2026-10-08/disassembly.txt), [check results](../artifacts/ni-file-program-public-tails-2026-10-08/check-results.json) |

All VAs below belong to this standalone image. `P:L` denotes exact line `L` in the complete export; each new snippet retains those original line numbers and a SHA256 in its index. The manifest hashes 59 evidence inputs, and records 43 original-image ranges with SHA256 and end VA. Most ranges come from PE unwind metadata; four tiny leaf getters use bytes through their first `ret`. These hashes identify the evidence, not recovered source code. Decompiled types and variable names remain hypotheses.

The root graph was queried before source discovery. It indexes production code, not these ignored PE artifacts. V2 is unindexed. The prior directory's `program.rs` is the frozen committed baseline. New [reference-v2](../artifacts/ni-file-program-public-tails-2026-10-08/reference-v2/) copies of `read_bytes.rs` and `structured_object.rs` are explicitly hashed **working-tree observations**, including already completed reader safety changes. The parent's ongoing FX/library edits were not read or changed.

## Complete public tail order

Use the common public prefix `P` from the prior report. Its length is `70 + 2*(Nname + Ncredits + Nauthor + Nurl)` bytes. All multi-byte scalars here are little-endian in the studied file path. The native primitive helpers also have an alternate stream-endian mode; that does not imply a new file variant.

Notation:

| Token | Exact wire shape | Native destination, relative to BProgram serializable receiver |
| --- | --- | --- |
| `F0` | signed i32 filename-table reference | `+0x10ed0`; historical resource-container label |
| `D0`,`D1` | `W`: u32 UTF-16 code-unit count `N`, then `2*N` bytes | temporary values, discarded by `a6` |
| `W0` | `W` | `+0x11eb8`; historical snapshot-factory-subdirectory label |
| `W1` | `W` | `+0x11ed8` or a temporary value, selected by read context |
| `U0` | u32, neutral scalar | `+0x128` |
| `S0` | inline BNISoundData, below | `+0x21b90` or temporary |
| `Q0` | u8, neutral scalar | written temporarily to `+0x158`, then original native value restored |
| `S1` | inline BNISoundData, same grammar as `S0` | `+0x160` or temporary |
| `U1` | u32, neutral scalar | `+0x21dc0` |
| `B0` | u32 byte count `N`, then `N` bytes; no wire terminator | temporary byte string, conditionally converted into the 16-byte member at `+0x21dc8` |
| `U2` | u32, neutral scalar | `+0x21dc4` |
| `F1` | signed i32 filename-table reference | `+8` |
| `F2` | signed i32 filename-table reference | `+0x11c38` |

The RTTI complete-object pointer precedes the serializable receiver by `0x10`: add 16 to obtain complete-object member offsets. Filename references are table indices, not literal paths; negative values mean empty. Preserve their exact signed values in the wire representation rather than normalizing every negative value to `-1`.

Every row below is the **entire suffix after P**, including reads after the public switch. No row relies on cumulative shorthand.

| Program version | Case entry VA | Complete suffix | Minimum suffix bytes, empty strings/absent S |
| --- | --- | --- | --- |
| `80,82,90` | `0x140d0d5b8` | empty | 0 |
| `91,92,a0,a1` | `0x140d0d71b` | `F0` | 4 |
| `a2,a3,a4,a5` | same | `F0 F2` | 8 |
| `a6` | `0x140d0d890` | `F0 D0 D1 W0 F1 F2` | 24 |
| `a7` | `0x140d0da9b` | `F0 W0 F1 F2` | 16 |
| `a8,a9,aa,ab,ac,ad,ae` | `0x140d0dc22` | `F0 W0 W1 F1 F2` | 20 |
| `af` | `0x140d0ddb7` | `F0 W0 W1 U0 F1 F2` | 24 |
| `b0` | `0x140d0df5a` | `F0 W0 W1 U0 S0 F1 F2` | 28 |
| `b1` | `0x140d0e10b` | `F0 W0 W1 U0 S0 Q0 S1 F1 F2` | 33 |
| `b2` | `0x140d0e2d8` | `F0 W0 W1 U0 S0 Q0 S1 U1 F1 F2` | 37 |
| `b3` | `0x140d0e4b3` | `F0 W0 W1 U0 S0 Q0 S1 U1 B0 F1 F2` | 41 |
| `b4,b5` | `0x140d0e69d` | `F0 W0 W1 U0 S0 Q0 S1 U1 B0 U2 F1 F2` | 45 |

Each W adds `2*N` bytes to that minimum; B0 adds `N`; present S bodies add the sizes below. These are lower bounds for valid reader paths, not a substitute for parsing lengths. Public case grouping does not imply identical private sections.

The exact dispatcher is the two-level original-byte table at RVAs `0xd0ed98`/`0xd0ed68`, default VA `0x140d0ed4b`. Holes such as `81`, `83..8f`, and `93..9f` are not supported. The check compares all 54 public table entries and all eleven case-local wire-call sequences after the existing 24 prefix calls.

### a6/a7/a8 and contextual retention

At `P:4740617–4740621`, `a6` reads F0, two W values into the **same temporary string** (the second replaces the first), W0 into the Program, then F1 through `0x140cfb6d0`. None of D0/D1/W0 has a presence flag, and no discarded string can be skipped without consuming its length and content.

At `P:4740681–4740683`, `a7` reads only F0/W0 in its case. At `P:4740733–4740736`, `a8..ae` reads F0/W0/W1. Later cases repeat those reads. The prologue chooses W1's destination at `P:4740456–4740459`: context byte `+0x3d == 0` means temporary storage; nonzero means receiver `+0x11ed8`. **Both destinations execute the same unconditional W call.** Calling W1 a “conditionally present full path” would corrupt cursor alignment. Its exact native semantic name is not recovered here; retain a neutral wire name such as `tail_string_1` even if existing UI/template context suggests a path.

The same context chooses whether S0/S1 populate Program members or a scratch BNISoundData instance (`P:4740440–4740451`). This also changes only retention, never wire presence. The writer uses context byte `+0x3d` as each S body's presence byte (`P:4725837–4725839`); that is distinct from the reader's choice of destination.

## S0/S1: SER::BNISoundData inline serializer

### Native ownership and framing

The RTTI name is **`.?AVBNISoundData@SER@@`**, vtable RVA `0x4fd1018`, object displacement 0. The check compares its ten actual PE pointers. Relevant slots:

| Slot | VA | Role |
| --- | --- | --- |
| 1 | `0x140d18fc0` | SerType getter returns `0x6d`, `P:4738317–4738320` |
| 2 | `0x14051b380` | version getter returns `1`, `P:2611359–2611362` |
| 3 | `0x14050f330` | structured flag getter returns `0` |
| 8 | `0x140cff5f0` | body reader, `P:4714623–4714672` |
| 9 | `0x140d0fb60` | body writer, `P:4725829–4725851` |

BProgram constructor `0x1407f8f20` calls BNISoundData constructor `0x140cf3a90` at complete-object offsets `0x170` and `0x21ba0` (`P:3370192,3370437`). Those are precisely public receiver offsets `+0x160` and `+0x21b90`. Constructor `0x140cf3a90` installs `0x144fd1018` (`P:4705912`). The public scratch instance installs that same vtable (`P:4740440`). This establishes **both owners**, not just similarity of helper calls.

Generic inline reader `0x14093d6c0` consumes u8 structured flag and u16 version and dispatches vtable slot 8 (`P:3686846–3686859`). Generic writer `0x14097d650` emits the same header and slot-9 body (`P:3744908–3744913`). There is **no u16 `0x6d` tag, no outer chunk header, no u32 total length, and no private/public/children section framing** before these bodies. The SerType getter is an owner identity, not an inline wire field.

### Exact body grammar

Define `W` as above. `u8 presence` and `u8 value_present` are tested as **nonzero**, not equality to 1. Preserve the raw bytes if byte-for-byte retention is required.

```text
S:
  u8 structured = 0
  u16 version = 1
  u8 presence
  if presence != 0:
    Metadata2
    Groups1
    u8 value_present
    if value_present != 0:
      u32 value_word_0
      u16 value_word_1
      u16 value_word_2
      u8 value_bytes[8]
```

The S body reader explicitly accepts only version 1 (`P:4714635,4714667–4714668`). An absent S is exactly **four bytes** (`00 01 00 00`). A `presence != 0` body calls metadata reader `0x142514fc0`, group-list reader `0x142514390`, then reads the optional-value presence and helper `0x142514b80` (`P:4714636–4714662`). The optional value is exactly 16 bytes; it is not sixteen u32 values. Do not call it a UUID without additional semantic evidence. Its writer `0x14251b680` emits u32/u16/u16/eight bytes (`P:11592248–11592255`), matching the reader at `P:11585715–11585736`.

`Metadata2` is this **positional** record, not an NIS tagged container or XML document:

```text
Metadata2:
  u32 metadata_version = 2
  u32 metadata_word_0
  u32 metadata_word_1
  W metadata_strings[5]
  u32 metadata_words[7]
  u32 string_group_count
  repeat string_group_count times:
    u32 string_count
    W strings[string_count]
  u32 string_list_count
  W strings[string_list_count]
  u32 pair_list_0_count
  repeat pair_list_0_count times: W first; W second
  u32 pair_list_1_count
  repeat pair_list_1_count times: W first; W second
```

Wrapper `0x142514fc0` invokes `0x142515050` and converts the temporary metadata only if its status is `<2` (`P:11585879–11585883`). The latter reads exactly version 2, two u32 values, five W strings, seven u32 values, then the counted collections (`P:11587369–11587479,11587516–11587538,11587584–11587688`). `string_group_count == 0` omits all group-count/contents words. The native reader retains at most the first three strings of the first group, but **consumes every string in every group** (`P:11587487–11587537`). The one-string list and pair lists are separately counted. Preserve their full wire order and values instead of reproducing native normalization or set/map deduplication.

Unknown `metadata_version` sets status 6 after consuming its discriminator (`P:11587690–11587691`); this gives no general length or unknown-body grammar. A candidate typed decoder must reject that path and retain the **whole public section/suffix**, rather than guess where Groups1 begins. The reciprocal writer `0x14251bbe0` emits version 2 and the same fields (`P:11593557–11593573,11593585–11593671`); it writes exactly one string group. The reader additionally accepts zero/multiple groups, so a loss-preserving reader cannot hard-code the writer's one-group case.

```text
Groups1:
  u32 groups_version = 1
  u32 group_count
  repeat group_count times:
    u32 group_version = 1
    u32 item_count
    W group_string
    repeat item_count times:
      u32 item_version = 1
      W item_string
      f32 item_float_0
      f32 item_float_1
      u32 item_word_0
      u32 item_word_1
```

`0x142514390` reads Groups1's version/count and loops over `0x1425140c0` (`P:11585137–11585159,11585180–11585185`). The group reader consumes its own version/count/W and each item's version/W/two f32/two u32 (`P:11585286–11585291,11585328–11585329,11585353,11585399–11585406`). Writer `0x14251b180` independently emits exactly that order (`P:11592008–11592035`). Native wrong-version return/break behavior does not establish a recoverable length: reject unknown list/group/item versions in the typed candidate.

Metadata2's minimum is **76 bytes** (zero groups and zero remaining lists, empty fixed strings). Groups1's minimum is **8 bytes**. Consequently a reader-valid present S with no optional value is at least **89 bytes including its four-byte inline header/presence**, versus four bytes absent. The current writer's mandatory single metadata string group adds at least four bytes, making its present minimum 93. Each empty group adds 12 bytes, each empty item 24, and an optional value adds 16. These formulas distinguish bounded reader acceptance from the current writer's canonical layout.

## B0 in b3: counted bytes, not counted UTF-16

Public `b3` invokes `0x14093ff90` at `P:4740998`; `b4,b5` repeat it at `P:4741055`. That helper calls `0x142a78330`, then copies the resulting byte string (`P:3690951–3690952`). The primitive reads **four length bytes**, allocates N bytes, reads **N bytes**, then appends a zero in memory (`P:13244118–13244137`). The memory terminator is not consumed from the stream. At VA `0x142a783ca`, original `mov r8,rsi` supplies N unchanged to the stream read; there is no multiplication by 2.

Current public writer `0x140d16da0` converts the member at `+0x21dc8` through `0x142c63020` and writes B0 using `0x142a7de40` (`P:4736762,4736804`). That primitive emits u32 N and exactly N bytes (`P:13252681–13252691`). The reader conditionally converts nonempty bytes through `0x142c63430` into two eight-byte native member pieces, only when an engine byte at `+0x303a8` is nonzero (`P:4741059–4741064`). This condition affects native interpretation, **not B0's wire presence**.

This pass proves byte framing and a 16-byte native destination. The converters' text conventions and field meaning were not recovered; `B0` should remain a raw byte vector. An existing `read_sized_utf8()` would consume the correct width but impose UTF-8 validation that these native byte-read calls do not prove. Use existing bounded `read_u32_le()`/`read_bytes()` and expose a UTF-8 view only as a separate interpretation.

## F1/F2 are distinct; F2's semantic name remains bounded

F1 goes to receiver `+8` (complete object `+0x18`). `a6` reads it inside its case via `0x140cfb6d0`. Later versions read it through `0x140cfb780`, under exact `version > 0xa6` at `P:4741066–4741068`. Original-byte anchor `0x140d0e8c7` verifies this threshold. The helpers are both four-byte signed references, but differ in table translation: the older `0x140cfb6d0` indexes directly; `0x140cfb780` adds the read context's integer at `+4` for nonnegative indices and retrieves a table-entry metadata word at `+0x254` (`P:4709666–4709679,4709735–4709748`). Preserve raw indices unless the owning filename context is available.

F2 goes to receiver `+0x11c38` (complete object **`+0x11c48`**), under exact `version > 0xa1` at `P:4741175–4741179`. Original-byte anchor `0x140d0ebbe` verifies this independent threshold. It is always the last wire field on those accepted versions. Its path is subsequently resolved when another native filename is nonempty and the filename-table entry metadata equals 1 (`P:4741180–4741189`). Writer `P:4736808–4736819` uses the same destination and selects entry metadata from path-resolution context.

The owner-relative path and UI evidence establishes more than a template label but less than an exact member name:

| Evidence | What it establishes |
| --- | --- |
| `0x140994e70`, `P:3772755–3772756` | Tests a separate complete-object filename at `+0x10dc0`; this is not F2's destination |
| `0x14135e360`, `P:6237671,6237719–6237726` | Compares a candidate path against literal `artwork`; uses complete-object `+0x11c48` as a fallback passed to `0x140998b40` |
| `0x14058d430`, `P:2736185–2736188,2736231–2736252` | Clears this member or populates it through a file chooser and refreshes the path display |
| `0x140581980`, `P:2725967–2725971,2726041` | Formats that path and sends its text to a UI control |
| `0x14135f9b0`, `P:6238426–6238443` | Uses the same member as the initial chooser path and writes the selected filename back |

An **artwork/image-related path** is supported as an inference from these uses. No recovered native getter name, direct property-name association, or controlled same-version preset delta proves that its exact meaning is wallpaper, instrument artwork, or a path shared by both. Preserve a neutral name such as `terminal_filename_ref` with native destination metadata. Treat `wallpaper_filename` as a legacy API/template hint rather than proven wire semantics. Do not map F1 into that field or omit F2 on `a2..a6`.

## Candidate decoder boundary and parent recommendations

Reuse `Program(StructuredObject)` and its already bounded `public_data`/`private_data` sections. The current `ProgramPublicParams::read`/`Program::params` at the committed V2 baseline reads only P and ignores its version; it must not imply full tail coverage. Parsing the public tail needs only the existing `Cursor`, `ReadBytesExt` primitives, and the positional grammar above. Do not introduce a third generic chunk format or substitute NIS object framing.

`StructuredObject::read` is appropriate for the outer Program. **It is not an inline S parser**: when its flag is false, it reads the entire remaining input into `public_data`. In a public suffix that would swallow Q0/S1/U1/B0/U2/F1/F2. Read the three-byte inline S header and the exact BNISoundData body on the existing section cursor. No inner byte length is available to isolate an unknown body after the fact.

Recommended implementation order:

1. Use the exact accepted-version set and the complete table above. Add F2 at `a2`, including `a6`; preserve D0/D1 and all filename indices. Consume W1 unconditionally for `a8..b5`. This gives a complete public-only decoder candidate through `af`, without changing private/authentication paths.
2. For `b0..b5`, accept only S flag 0/version 1; handle both nonzero-presence branches with Metadata2 and Groups1, all counts, and the optional 16-byte value. Unknown nested versions must fail the typed path while keeping the existing raw public bytes. Do not “skip S” by inventing an outer length.
3. Retain Q0/U0/U1/U2 without semantic labels or normalization. Q0 is consumed but restored in native state (`P:4740460,4741065`), so native loading is not a loss-preserving wire model. Read B0 as bytes, then F1/F2, and require exact section exhaustion.

Check every declared string/collection against remaining **public-section** bytes before allocation or iteration. Useful minimum per-element bounds are W = 4 bytes, W pair = 8, Groups1 group = 12, and group item = 24. Combine nested counts with the bounded cursor; do not allocate from a hostile count first. For loss preservation, retain UTF-16 units or make invalid-UTF-16 behavior explicit rather than silently replacing units. Use raw u8 for the S presence flags; current `read_bool()` tests `==1` and would treat another native nonzero value as absent.

The parent can choose a smaller release boundary: expose the proven P plus raw suffix first, or implement complete public tails through `af` and keep later raw. It should not report complete `b0..b5` decoding after supporting only absent S values. The nonempty Metadata2/Groups1 grammar above now supplies a concrete implementation candidate, but has not been exercised against real Program records in this pass.

## Runnable evidence check and unresolved constraints

Run from the root workspace:

```bash
python artifacts/ni-file-program-public-tails-2026-10-08/check_public_tails.py
```

The completed run reports **PASS** for 59 hashed inputs, 43 PE ranges, BNISoundData RTTI/getters, all eleven suffix cases, nested primitive-call order, both Program post-switch version thresholds, absent-body branching, byte-string length/read width, and constructor/member anchors. [check_public_tails.py](../artifacts/ni-file-program-public-tails-2026-10-08/check_public_tails.py) uses only existing `pefile`/`capstone` plus the standard library. It checks original bytes independently of decompiler call names; the source spans supply loop/framing explanations. It is a static evidence regression, not a decoder round-trip test or application integration test. It writes only the new directory's check result.

| Remaining constraint | Exact next evidence needed |
| --- | --- |
| Real-file boundary/application acceptance | Unprotected same-version public sections with nonempty a6 legacy strings, a8 W1, present S0/S1, nonempty groups/items and B0, plus valid filename context; check exact consumption separately from loss-preserving reserialization |
| F2's exact semantic name | Native named getter/property association or controlled preset delta tied specifically to complete-object `+0x11c48`; artwork/UI use alone does not settle wallpaper terminology |
| Neutral scalars and metadata field meanings | Named getter/property ownership or controlled per-field deltas; positional widths do not establish categories, units, or enum semantics |
| Unknown S/metadata/group/item versions | Their actual readers and reliable framing; a generic version `<= current` check alone does not establish old-version bodies |
| B0 encoding and optional 16-byte semantics | Trace `0x142c63020`/`0x142c63430` and the metadata-value owner; do not infer UTF-8/UUID policy from byte framing |
| Historical writer equivalence | Version-selected historical writer or real-file round trips; current `b5` writer order only corroborates the current layout |
| VST3 match | Independently trace the pinned `0x180000000` payload image's serializer ownership/code; standalone addresses and equal version strings do not prove equivalence |

Only this document and `artifacts/ni-file-program-public-tails-2026-10-08/*` were authored. Existing guides, production files, parent changes, pinned prior evidence, and samples were preserved.
