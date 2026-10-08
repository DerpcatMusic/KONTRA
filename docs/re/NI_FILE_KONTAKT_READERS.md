# ni-file Kontakt reader limits

## Base identity and scope

Implementation target: `/home/derpcat/.t3/worktrees/KONTAKTO/decipher-readers-v2`,
base HEAD `2fb8c926dd39bb7ac26a84d4806f42de14b6630e`, initially clean.
Report workspace: `/home/derpcat/.t3/worktrees/KONTAKTO/t3code-80fe786b`,
base HEAD `0cb7a8a0b4d43086596a64c77320caa1b26d6d98`.
The report workspace already contained untracked `docs/DSP_FORMAT_SPECIFICATION.md`
and `docs/DSP_SYSTEM_INVENTORY.md`; neither was changed.

Changed files:

- V2 `vendor/ni-file/src/kontakt/objects/header.rs`
- V2 `vendor/ni-file/src/kontakt/objects/voice_groups.rs`
- V2 `vendor/ni-file/src/kontakt/objects/voice_group.rs`
- V2 `vendor/ni-file/src/kontakt/objects/voice_limit.rs`
- V2 `vendor/ni-file/src/kontakt/objects/program.rs`
- Report workspace `docs/NI_FILE_KONTAKT_READERS.md`

The follow-up changes only the three voice modules and this report. The earlier
header/program changes remain untouched. Other agents' changes in the shared V2
checkout were left untouched. No dependency, IR, schema, authentication, license,
or key-handling changes were made.

## Evidence and reader behavior

Source references below use paths relative to V2. Unchanged-source references are
anchored to the base revision; changed-source references describe this patch.

| Area | Evidence | Implemented behavior |
| --- | --- | --- |
| Application version | `header.rs:340` defines major and three distinct minor components; its Debug implementation at `header.rs:347` already prints all four. The V42 reader at `header.rs:245` reads the wire bytes in reverse component order. | Display now prints `major.minor_1.minor_2.minor_3`, matching Debug, without allocating an intermediate formatted String. |
| Patch descriptions | `header.rs:394` maps IDs 0–5 to NKM, NKI, NKB, NKP, NKG, NKZ, retaining any other `u16` in `Unknown`. No NKZ semantic layout is documented by the examined sources. | Existing five descriptions remain; NKZ returns `NKZ (layout unknown)`. Unknown IDs return, for example, `Unknown patch type (0xabcd)`. Neither branch panics. |
| VoiceGroups | The pinned baseline read only eight mask bytes/eight bits and returned an empty group vector. [Binary record research](NI_FILE_BINARY_RECORDS.md#1-voicegroups-0x32-concrete-0x60-layout) now corroborates the complete unstructured inline framing in the existing V2 importer. | The v0x60 reader returns the instrument limit plus exactly 128 indexed optional overrides. It reads sixteen mask bytes LSB-first and consumes one inline limit per populated bit in ascending order. Clear bits remain None; no native defaults are fabricated. |
| Shared inline limits | `objects/voice_limit.rs:29` validates the instrument/group record flag and version, then calls the existing field reader. `objects/voice_group.rs:18` exposes the resulting override as `voice_limit`. The separate importer at `crates/sampler-kontakt/src/library.rs:376,440` follows the same framing/mask order. | Structure flags must be 0; exact version must be 0x60. Names and signed kill/voice/fade/exclusion scalars are exposed unchanged. Instrument/group errors preserve version details and identify the containing instrument/override. The importer is unchanged. |
| Program private parameters | `doc/presets/Kontakt/BProgram.md:5` describes length-framed private/public data and children, and its V80 section documents public fields only. The previous private reader discarded unnamed fields, skipped unresolved nested objects/arrays and returned an empty struct; it also asserted inner versions and panicked on filename references. | `ProgramDataPrivateParams::read` now returns an unsupported error containing the supplied version for every version, including 0x80. It does not consume or validate the private body, assume an inner layout, interpret filename sentinels, print diagnostics or return a fake empty success. |
| Raw preservation | `kontakt/structured_object.rs:10` exposes `private_data`, `public_data`, and child `Chunk`s. `objects/program.rs:115` exposes children. `kontakt/chunk.rs:23` writes the raw ID, length, and body; typed conversion borrows the chunk. | Raw private bytes remain in `Program.0.private_data`; VoiceGroups bodies remain in `Program.children()` / `Program.0.children`. Typed decoding errors do not discard or change either representation. |

Table path shorthand: bare object filenames and `objects/` paths refer to
`vendor/ni-file/src/kontakt/objects/`; `kontakt/` paths refer to
`vendor/ni-file/src/kontakt/`; `doc/` paths refer to `vendor/ni-file/doc/`.
Paths starting `crates/` are relative to the V2 checkout root.

## Recovered VoiceGroups wire evidence

The follow-up uses the completed [binary record report](NI_FILE_BINARY_RECORDS.md)
and its [static byte-check source](../artifacts/ni-file-records-2026-10-08/check_records.py). The parent reports that the frozen check
passed again with eleven original-byte anchors and 162 dispatch entries; this
agent did not execute either proprietary image or rerun that artifact-producing
check. It pins the standalone image to SHA256
`0fe6356e0879d058b6e5b73507c54c5e345cea451b35287c974e438291d4dae8`.

Each inline record is `[0, 0x60, 0]`, a u32 UTF-16 code-unit count, those units,
then i16 kill mode, u8 preference, and i32 voices/fade/exclusion. The full chunk
is `u16 0x32 / u32 body_length / instrument_inline / mask[16] / selected_inline*`.
There is no second header, inner 0x2b tag, record length, count or index field.
An empty-name instrument with no overrides has a 38-byte body (44-byte chunk).

Native reader `0x140d1e350` visits all 128 bits; writer `0x140d145f0` emits the
same ascending inline sequence. The original-byte check corroborates sixteen
mask bytes, byte index `group >> 3`, bit index `group & 7`, and the 128-slot loop.
The vendored prose's claim that these group records are StructuredObjects is
superseded by this evidence; that documentation file is outside this follow-up's
write scope.

`VoiceLimit::read_inline` is the only ni-file framing helper, reused by both
instrument limits and `VoiceGroup::read`. `VoiceLimit::read` remains the field-body
API. Its existing UTF-16/read-bytes helper checks declared name bytes against
remaining input before allocation and rejects invalid UTF-16. The fixed mask
bounds the number of records to 128; scalar and missing-record reads propagate
errors. `VoiceGroups::try_from(&Chunk)` and `VoiceGroup::try_from(&Chunk)` reject
trailing bytes; their generic `read` methods consume one record and leave enclosing
stream bounds to the caller. Unsupported versions, invalid structure flags and
malformed input leave the borrowed raw Chunk available.

The preference field needs an explicit distinction: native reader
`0x140d049b0`, original pseudocode lines 4720814–4720815, copies the raw byte to
member `+0x30`; writer `0x140d13370`, line 4729536, passes that byte back to the
byte writer. The omission test at lines 4733813–4733818 tests zero/nonzero and
requires exclusion **-1**, rather than the old comment's 0. The existing typed
`prefer_released: bool` now normalizes byte 0 to false and every nonzero byte to
true. This bool cannot distinguish byte 1 from 2 or 255. Original `Chunk.data`
and raw `Chunk::write` preserve those distinctions independently. No lossless
typed serialization or semantic writer is claimed or added. Names and signed
scalars are not clamped, and absence stays None rather than synthesized defaults.

## Upstream research

Examined the upstream HexFiend templates at commit
`c6f309bae04a03967b94f54d81dc2050f827a1e8`:

- [VoiceGroups.tcl](https://github.com/monomadic/hexfiend-templates/blob/c6f309bae04a03967b94f54d81dc2050f827a1e8/Kontakt/VoiceGroups.tcl)
  reads the structure flag, version, a voice-limit field sequence for 0x60 and
  sixteen individually unnamed bytes. It does not describe populated group records.
- [ProgramPrivateData.tcl](https://github.com/monomadic/hexfiend-templates/blob/c6f309bae04a03967b94f54d81dc2050f827a1e8/Kontakt/ProgramPrivateData.tcl)
  reads a version and unnamed scalar fields. It supplies no complete nested-object
  or filename semantics.
- [BProgram.tcl](https://github.com/monomadic/hexfiend-templates/blob/c6f309bae04a03967b94f54d81dc2050f827a1e8/Kontakt/BProgram.tcl)
  retains private, public and child sections as length-framed bytes for version 0x80.
- [BPatchHeaderV42.tcl](https://github.com/monomadic/hexfiend-templates/blob/c6f309bae04a03967b94f54d81dc2050f827a1e8/NKS/BPatchHeaderV42.tcl)
  names the wire version components MinorC, MinorB, MinorA, Major.

These research templates support the prefix/framing observations, not complete
semantic support. The vendored `tests/data` directory and the two old fixture
paths for VoiceGroups/private parameters do not exist in this checkout.

## Regression coverage and validation

Four local `reader_regression_*` tests cover this agent's cumulative changes,
using synthetic records and existing Cursor/Chunk/Program/error APIs. The two
voice tests replace the earlier blanket-unsupported regression:

- Distinct application-version components, all mapped patch labels and an unknown ID.
- Instrument and group limits with nonempty UTF-16 names (including a surrogate-pair
  instrument name), distinct signed scalars and override indices 0, 7, 8, 63, 64,
  127. Clear bits remain None, a zero mask yields 128 missing slots, and a full mask
  decodes all 128 populated slots. Every truncation of the multi-limit body fails.
- Both instrument/group headers reject nonzero structure flags, lower/higher/unknown
  versions, oversized name lengths and unpaired UTF-16. Preference bytes 0/1/2/255
  decode according to zero/nonzero semantics while preserving original raw bytes.
  Bounded chunk decoding rejects tails; generic stream decoding stops at the exact
  record boundary. Standalone 0x2b limits share the reader and reject every
  truncation, wrong IDs and trailing bytes. Existing raw Chunk serialization is
  checked independently of typed decoding.
- Program private-data unsupported errors and raw parent/private/child retention,
  as implemented in the initial task. The old asserted leading-u32 value 2 and
  filename-reference offset 61 remain historical regression triggers, not new
  format claims. The final binary report identifies that leading value as a
  discriminator and notes that 2 is native-valid; the entire private layout remains
  opaque in this implementation, so it never accepts or partially interprets it.

The two original fixture-dependent VoiceGroups/private-reader smoke tests were
replaced with synthetic regressions. Existing header fixture tests remain
unchanged and are excluded by the targeted command below.

The original parent integration passed eleven focused reader tests and 32
compatibility tests before this follow-up. During final review on 2026-10-08,
the parent reran the focused reader check including the new decoder: **12 passed,
0 failed**. This includes byte-reader tests, Kontakt reader regressions,
and NIS/schema reader tests. The compatibility suite also passed again after
the follow-up: **32 passed, 0 failed**, with the `serde` feature enabled.
The research agent ran formatting/whitespace
checks only; the Rust check was serialized by the parent using the shared cargo
configuration.

Completed parent reader check:

```sh
cargo test --offline --manifest-path /home/derpcat/.t3/worktrees/KONTAKTO/decipher-readers-v2/vendor/ni-file/Cargo.toml --lib reader
```

## Remaining gaps

NKZ's label conveys recognition only. VoiceGroups/VoiceGroup typed support is
restricted to little-endian unstructured v0x60 records. Other versions/flags and
unexpected tails remain errors with the original raw chunks accessible. The bool
preference view normalizes nonzero bytes; byte-exact preservation belongs to raw
Chunk APIs. No semantic writer was added.

Program private parameters remain opaque for all versions; meaningful typed
support requires the complete versioned body, including filenames and nested
arrays. Program framing remains readable, while the existing public-parameter
reader is still partial: it ignores the supplied version and leaves
filename-related fields unset. Program/header source was unchanged in this
follow-up.

No unprotected real preset fixtures, live application acceptance, or matching
VST3 serializer evidence were established by these synthetic tests. The raw chunk
APIs preserve bytes; neither recognition nor serializer framing establishes
complete preset support, application acceptance of edits, or plugin audio
equivalence.
