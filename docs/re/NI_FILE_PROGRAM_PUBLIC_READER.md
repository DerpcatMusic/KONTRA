# Program complete public-section reader

Implementation and parent review, 2026-10-08. This adds a read-only decoder for the complete public grammar established in [NI_FILE_PROGRAM_PUBLIC_TAILS.md](NI_FILE_PROGRAM_PUBLIC_TAILS.md). Parent validation passed **15 focused reader tests and 32 compatibility tests**, plus 30 importer library tests. A separate real-fixture probe decoded 25 complete public records, including four installed Kon8/b5 templates. These checks do not establish private-layout support, complete semantic import, native loading/writing, audio equivalence, or VST3 equivalence. The child performed formatting, diff inspection and preservation checks; the parent compiled, reviewed and tested the result separately.

## Exact bases and owned changes

| Workspace | Committed base |
| --- | --- |
| V2 implementation: `/home/derpcat/.t3/worktrees/KONTAKTO/decipher-readers-v2` | `2fb8c926dd39bb7ac26a84d4806f42de14b6630e` |
| Root report: `/home/derpcat/.t3/worktrees/KONTAKTO/t3code-80fe786b` | `0cb7a8a0b4d43086596a64c77320caa1b26d6d98` |

Changed files owned by this task:

- V2 `vendor/ni-file/src/kontakt/objects/program.rs`.
- New root `docs/NI_FILE_PROGRAM_PUBLIC_READER.md`.
- New root `artifacts/ni-file-program-public-reader-2026-10-08/before-state.json` and `handoff.json`.

The V2 Program file already contained the earlier unsupported-private-reader fix. That implementation and its existing authored regression remain **byte-identical**, verified by reconstructing the earlier block around the newly inserted public regressions and comparing its initial SHA256. The original public-prefix reader differs only in rustfmt whitespace. Its behavior and `Program::params()` behavior remain unchanged. No modifications were made to effects/library translation, Header, VoiceGroups, VoiceLimit, NIS, module exports, dependencies, IR, authentication, licensing, or key handling. The initially dirty files outside Program retained their initial hashes; see [handoff.json](../artifacts/ni-file-program-public-reader-2026-10-08/handoff.json) and [before-state.json](../artifacts/ni-file-program-public-reader-2026-10-08/before-state.json).

The full tail report and both original [record check results](../artifacts/ni-file-records-2026-10-08/check-results.json) and [tail check results](../artifacts/ni-file-program-public-tails-2026-10-08/check-results.json) were read. These report passing static standalone-image evidence checks, not decoder tests. They were not rerun or modified here. V2 has no graph; normal source discovery traced `Program::params`, `ProgramPublicParams::read`, `Program::try_from`, `KontaktChunks::program`, the frontend translation call and its `zone` parameter dependency. No root production source needed opening.

## APIs and compatibility boundary

The existing wildcard `pub use program::*` exposes the new public types without a `mod.rs` edit.

```rust
Program::public_record(&self) -> Result<ProgramPublicRecord, Error>
ProgramPublicRecord::read(public_data: &[u8], version: u16)
    -> Result<ProgramPublicRecord, Error>
```

`Program::public_record()` borrows the existing bounded `Program.0.public_data`. It returns a complete typed view only after consuming the entire section. Its exact accepted public versions are **`{0x80,0x82,0x90,0x91,0x92} ∪ {0xa0..0xb5}`**. Unsupported versions are rejected before reading any body bytes. There is no version-range fallback across holes and no best-effort typed suffix.

`Program::params()` still calls `ProgramPublicParams::read` for the common prefix P. It continues to ignore the version and unread suffix, including accepting a well-formed P at an unknown public version. The method now explicitly documents that partial behavior. Existing frontend callers still receive the same fields, including `resource_container_filename == None` and `wallpaper_filename == None`; their call sites were untouched. Complete public support must be requested through the new API, not inferred from `params()` success.

`ProgramPublicRecord` contains the original `prefix`, `version`, exact `group_solo_byte`, and every version-selected tail field. The extra raw prefix byte preserves values such as 255 alongside the legacy `group_solo` interpretation (`read_bool() == 1`). Float scalars are read without arithmetic; regression assertions use `to_bits()` for NaN payloads, negative zero and infinity. Raw public bytes remain the loss-preserving authority.

| Wire token | New record field | First public version |
| --- | --- | --- |
| F0, signed i32 | `resource_container_filename_ref` | `91` |
| D0/D1, counted UTF-16 | `discarded_strings: Option<[String; 2]>` | only `a6` |
| W0, counted UTF-16 | `tail_string_0` | `a6` |
| W1, counted UTF-16 | `tail_string_1` | `a8`, unconditional |
| U0, u32 | `word_0` | `af` |
| S0, inline BNISoundData | `sound_data_0` | `b0` |
| Q0, u8 | `byte_0` | `b1` |
| S1, inline BNISoundData | `sound_data_1` | `b1` |
| U1, u32 | `word_1` | `b2` |
| B0, counted bytes | `bytes_0` | `b3` |
| U2, u32 | `word_2` | `b4` |
| F1, signed i32 | `filename_ref_1` | `a6` |
| F2, signed i32 | `terminal_filename_ref` | `a2` |

The wire order follows the report's full suffix rows: F1 follows the case-local fields, then F2. Thus `a2..a5` have F0/F2, `a6` has F0/D0/D1/W0/F1/F2, and `b5` has F0/W0/W1/U0/S0/Q0/S1/U1/B0/U2/F1/F2. F0/F1/F2 retain their exact signed indices, including every negative sentinel, zero and positive references. No filename-table translation or semantic wallpaper mapping is attempted. `None` means the field is absent from that version's grammar; it does not mean a present empty string, a negative reference, or an absent S body.

## Both inline sound bodies

`ProgramSoundData` is a concrete inline **flag 0/version 1** parser shared by S0 and S1. It does not call `StructuredObject::read`: that generic unstructured path would consume the rest of the public suffix. There is no SerType tag, total byte length, section framing, or guessed unknown-body boundary.

The public data types are:

- `ProgramSoundData`: exact `presence: u8`, plus optional `body`.
- `ProgramSoundDataBody`: `metadata`, ordered `groups`, exact `value_presence: u8`, and optional `[u8; 16]` value.
- `ProgramSoundMetadata`: Metadata2's two leading words, five fixed strings, seven words, all string groups, string list, and both ordered pair lists.
- `ProgramSoundGroup`: group string and ordered items.
- `ProgramSoundItem`: item string, two f32 values and two u32 words.

Both presence bytes use native nonzero semantics and retain raw values such as **2 and 255**. An absent S consumes exactly four bytes. A present body accepts Metadata2 version exactly 2 and Groups1/list, group and item versions exactly 1. Zero, one or multiple metadata string groups are handled; every string is consumed and retained, including strings beyond the native first-three retention rule. All lists retain duplicates and wire order. The optional value is exactly 16 bytes; its u32/u16/u16/eight-byte wire pattern is preserved without asserting UUID semantics. B0 is separately retained as raw bytes, including invalid UTF-8 and embedded zeros.

Any unknown inline flag/version, metadata version, group-list version, group version or item version fails the entire typed view. No unknown nested body is skipped. `Program.0.public_data`, `private_data` and raw child chunks remain accessible, and the original `Chunk` retains its complete Program body for existing raw writing. The typed view has no native writer.

## Bounds, string handling and exact consumption

All parsing operates on a cursor over the public-section slice. Every declared count is checked against remaining public bytes before its allocation or loop:

| Count | Minimum bytes per element |
| --- | --- |
| UTF-16 code units | 2 |
| UTF-16 strings and metadata string groups | 4 |
| UTF-16 pairs | 8 |
| Groups1 groups | 12 |
| Group items | 24 |
| B0 bytes | 1 |

The P preflight checks all four string lengths before reusing `ProgramPublicParams::read`, including protecting its UTF-16 `usize * 2` calculation on 32-bit targets. It checks the 48-byte fixed interval after the name and six terminal category bytes, preserving the raw group-solo byte at fixed offset 43. New strings similarly validate their length, then reuse the existing `read_widestring_utf16` helper; raw byte reads reuse the existing bounded `read_bytes`. Collection vectors grow only after count checks. The fixed optional 16-byte value is read directly into an array. There are no added generic object frameworks, assertions, panic recovery, dependencies, or speculative maximum-size constants in the decoder.

The existing UTF-16 helper rejects invalid UTF-16 via `String::from_utf16`; it does not replace malformed units. This is an explicit typed-view limitation. The raw parent still preserves all bytes on that error path. All scalars are little-endian. Exact end-of-section consumption is mandatory: even one extra trailing byte rejects the complete view.

## Authored regressions and parent validation

Three new runnable Rust regressions were added inside the existing Program test module:

| Test | Authored coverage |
| --- | --- |
| `reader_regression_public_versions_and_tail_order` | Every accepted public version with empty/rich tails; a6 discarded strings; nonempty a8 W1; F2 at a2 and a6; both S bodies present/absent; all prefix/tail scalar patterns; metadata zero/one/multiple group branches; ordered duplicates and fourth group string; nonempty/empty groups and items; 2/255 presence bytes; optional 16 bytes; invalid-UTF-8 B0; F0/F1/F2 negative/zero/positive indices; per-version trailing-byte rejection; unchanged prefix API. |
| `reader_regression_public_minimal_present_sound_body` | Reader-valid 89-byte present S0 with zero metadata collections/groups and absent optional value, embedded in a complete b0 record. |
| `reader_regression_public_rejects_truncation_versions_counts_and_preserves_raw` | Every truncation of the rich b5 fixture; every unsupported u16 public version rejected before body reads; corrupted nested versions, inline flags/versions, every tracked string/collection/byte count set to u32::MAX; invalid UTF-16 in P/W1; preservation of raw public/private data and existing raw Chunk write/read/write; legacy prefix success for unknown version/unread suffix. |

The earlier `reader_regression_private_params_remain_opaque` was preserved unchanged. Fixtures are authored with existing primitive byte-extension patterns and contain no copied vendor presets. These tests were unexecuted at the child handoff and subsequently passed the parent checks below.

Completed checks:

```text
rustfmt --edition 2021 --config skip_children=true --check vendor/ni-file/src/kontakt/objects/program.rs
  PASS
git diff --check -- vendor/ni-file/src/kontakt/objects/program.rs
  PASS
```

The child recommended this focused command, from the V2 workspace, with no other cargo/rustc build, test or clippy running across agents:

```bash
cargo test --manifest-path vendor/ni-file/Cargo.toml reader_regression -- --nocapture
```

Use the existing shared target/sccache configuration; do not set `CARGO_TARGET_DIR` or `RUSTC_WRAPPER`. No cargo, rustc, tests, clippy, dependency installation, dev server, child delegation, proprietary execution, or sample/license/key operation was performed here.

### Parent verification

The parent reviewed the complete reader and authored cases against the positional tail/body report, then ran these commands sequentially after verifying the shared Rust build slot was idle:

```bash
cargo test --offline --manifest-path vendor/ni-file/Cargo.toml --lib reader
# 15 passed, 0 failed, 0 ignored; includes all three new public-record regressions.
cargo test --offline --manifest-path vendor/ni-file/Cargo.toml --features serde --test compatibility
# 32 passed, 0 failed, 0 ignored.
cargo test --offline -p sampler-kontakt --lib
# 30 passed, 0 failed, 3 ignored surveys; downstream importer and FX regressions.
```

The reviewed/tested Program source SHA256 is `b95ebc29885b57483db5bb655f00f500631e5bfc51e075841bc53d468003ab84`. The [parent validation receipt](../artifacts/ni-file-program-public-reader-2026-10-08/parent-validation.json) records the source identity and frozen copy; the child's handoff remains an unmodified historical record. Both native static evidence checks were rerun successfully. The older check now uses the exact original task ledger recovered under its original recorded digest, separating frozen research input from later operational task-status updates. Existing ni-file deprecated API warnings remain.

### Real-fixture public decoding

The [parent clear-fixture probe](NI_FILE_VERSIONED_CORPUS.md#parent-clear-fixture-decode-follow-up) exercises 26 pinned inputs using existing vendor APIs. It decoded **25 complete public records with zero public-record errors**, at versions `80,a2,a5,a8,ac,af,b5`. All four installed Kon8 templates decode to Program b5 with Group 96; the real FileContainer members decode to Program af, including both NKM slots. B5 bodies have S0/S1 absent and 36-byte B0, so present-S real-file evidence remains missing. The separate Kon3 legacy XML result is outside this chunk-only probe, rather than a malformed public record. Reproducible source/input hashes and exact output are linked from the corpus report; no expanded preset/sample asset was retained.

## Remaining evidence boundary

The full positional standalone public grammar is implemented and tested with authored fixtures, including present S bodies through b5. Selected real public sections now also decode completely; native acceptance, present-S real-file coverage and historical/native writer equivalence remain unproved. Private records remain explicitly unsupported, and neutral scalar/string/filename semantics remain neutral until named ownership or controlled field deltas justify interpretation.
