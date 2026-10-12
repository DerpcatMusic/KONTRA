# Kontakt KSP persistence format

Original analysis, 2026-10-08. Reader baseline: `origin/integrate/core-v2` at
`699758ab`. Scope: script saved-variable records in instrument, multi, bank and
snapshot containers. This document specifies source values; it does not authorize
or implement a runtime restore policy.

Evidence labels used below:

- **Corpus**: in-memory inspection of installed library files. No decrypted
  payloads, script exports, samples or keys were saved. Counts include every path
  reached by the recursive reader, including recovery saves under Pacific
  Ensemble Strings/KONTRA project recovery and library symlinks. The master
  corpus excludes recovery saves; its denominator is reported separately.
- **Engine**: inspection of the existing read-only Kontakt 8.13.1 analysis,
  engine SHA-256
  `0fe6356e0879d058b6e5b73507c54c5e345cea451b35287c974e438291d4dae8`.
  Addresses identify functions in that build, not portable identifiers. Static
  analysis is not a reference-host playback test.
- **Manual**: Native Instruments' documented KSP behavior, linked at the point
  of use. It describes host semantics, not the undocumented file layout.
- **Fixture**: original authored bytes exercised by the Rust reader tests.

## Outer framing and script public state

All binary numbers below are little endian. A chunk is `u16 id; u32 body_bytes;
body[body_bytes]`. A structured-object body begins with `u8 structured; u16
version`. For flag 1, it continues with length-prefixed private, public and child
byte regions (`u32 size` each). For flag 0, the remainder of the chunk is public
state. Saved-variable strings live in **public**, not private, state.

This extends the script `0x06` description in
[`DSP_FORMAT_SPECIFICATION.md`, “Kontakt chunks and structured objects”](../../../t3code-80fe786b/docs/DSP_FORMAT_SPECIFICATION.md).
The original writer's empty table did not establish the populated-entry grammar.
Its fourth nullable string field is the linked script filename field, not a
saved-variable type discriminator or an NKA reference.

| Order in script public bytes | Encoding | Meaning |
|---|---|---|
| 1 | nullable string | KSP source |
| 2 | u8 | source editor open |
| 3 | u8 | touched but not applied |
| 4 | u8 | bypass |
| 5, v0x50 | nullable string | legacy password field |
| 5, v0x60 | u32 length; bytes | opaque password/hash bytes |
| 6 | nullable string | title/description |
| 7 | nullable string | linked script filename |
| 8 | u32 entry count | saved-variable table |
| 9 | count × (u32 byte length; bytes) | saved-variable entries |

A nullable string uses `u32 byte_count` and the specified bytes; `0xffffffff`
means absent. Counts are byte counts, not Unicode character counts. The
saved-entry length has no implicit terminal NUL. Endianness does not apply to
numbers inside the entry: those numbers are text.

Engine: script reader `0x140d03920` accepts versions 0x50 and 0x60, switches only
the password field's representation, and reads the same saved-string vector
following both. Writer `0x140d12740` emits the v0x60 fields and vector. The
borrowed reader also preserves old chunk-bounded records ending before a table
as `None`, distinct from a present zero-count table; no such omission was found
in this corpus. Unknown extensions remain visible. All measured script records
had zero extension bytes.

`0x06` is used both by program scripts and bank/multi scripts. A name is scoped
by its owner **and script slot**, not globally by the string name. Embedded
program scripts in multis retain their own tables. File extension does not
change the entry grammar. No `.nkb` was present in the installed corpus; bank
layout was traversed through `.nkm` files.

## Entry grammar and typed values

Every measured entry begins with the full variable name, including its sigil,
then one ASCII space (`0x20`). The rest is the payload. There is no additional
binary type byte, UI class tag, stored array dimension, persistence-scope flag,
or schema version per entry. The sigil is the wire-level type indicator.
The owning script declaration supplies the UI class and array dimension.

| Sigil | Bytes | Typed source interpretation |
|---|---|---|
| `$` (0x24) | `$name SPACE signed_decimal` | integer, or UI integer state; **menu index** if `ui_menu` |
| `~` (0x7e) | `~name SPACE real_text` | IEEE binary64 value parsed from decimal/exponent text |
| `%` (0x25) | `%name SPACE integer_tokens` | integer array; ordinary arrays have repeated-tail compression |
| `?` (0x3f) | `?name SPACE real_tokens` | real array; ordinary arrays have repeated-tail compression |
| `@` (0x40) | `@name SPACE arbitrary_string_bytes` | scalar string or `ui_text_edit` text |
| `!` (0x21) | `!name SPACE cell_0 LF cell_1 LF ...` | string array; every cell is followed by LF |

Scalar integers are signed 32-bit values. Reals use binary64 in the KSP storage
and are serialized as text, not as an eight-byte binary field. The modern
serializer uses ordinary general floating-point formatting; a reader must
accept exponent notation. Exact source decimal text remains in `raw`; parsing
cannot reconstruct bits that were already rounded by the writer. The corpus
had no populated `~` scalar or ordinary `?` array, so those paths are supported
by engine analysis and authored fixtures, not native-file measurements.

Numeric-array tokens are separated by ASCII spaces. The writer appends a space
after the last token. Scalar strings must be split **once** at the name/payload
separator: subsequent leading/trailing spaces, newlines, CRs and quotes belong
to the string. No quoting, backslash unescaping or whitespace trimming applies
to strings. Measured entries were valid UTF-8; the reader nevertheless retains
string payload bytes instead of silently replacing invalid Unicode.

### Repeated tails and string-array boundaries

Engine writer `0x140781d20`, cases 4 and 5: start at the last ordinary numeric
array cell, walk backward while adjacent trailing values are equal, and emit
through the first cell in the final repeated run. At least one value is emitted
for a nonempty ordinary array. Reader `0x140782eb0`, cases 4 and 5: write the
encoded prefix into the declared array, then fill its remaining cells with the
last decoded value. Thus `%a 7 -3 0 ` with declaration `%a[6]` means
`[7, -3, 0, 0, 0, 0]`. Treating encoded token count as declared array length is
incorrect. The dimension is absent from the saved record.

UI tables and XY controls have separate serializer cases: **all cells** are
emitted, even if their tails repeat. Their reader cases consume those cells
without the ordinary-array repeated-tail fill. `SavedValue` exposes an
`ArrayTail` marker so a consumer can preserve that distinction without running
restore logic in the format reader.

String arrays have no element-length subrecords. Engine case 7 emits each cell
verbatim and then `0x0a`, including empty cells. A final LF terminates the final
cell; it is not another empty cell. `!names red blue\n\nlast\n` is three
cells: `"red blue"`, `""`, `"last"`. Do not use whitespace tokenization or
`str::lines()` (which treats CRLF specially) for this encoding. There is no
escape for an LF embedded inside a cell; the wire representation cannot
unambiguously recover the original cell boundaries in that case. The engine
restorer splits at LF and stops at the declaration's dimension. No CR or
embedded-boundary ambiguity was observed in the measured string arrays.

### Authored byte examples

These examples were written for this spec and the tests, not copied from a
library. Offsets are relative to the entry's u32 length prefix.

| Offset | `$curve 2` | Meaning |
|---|---|---|
| 0..3 | `08 00 00 00` | eight entry bytes |
| 4..11 | `24 63 75 72 76 65 20 32` | `$curve`, space, `2` |

For a three-item `ui_menu` with item values `[0, 20, 80]`, this means selection
index 2 and script value 80. For a plain integer declaration with the same name,
it means integer value 2. The bytes alone cannot distinguish these meanings.

A complete two-entry table is:

```text
02 00 00 00                         entry_count = 2
08 00 00 00 24 63 75 72 76 65 20 32 "$curve 2"
0a 00 00 00 25 61 20 37 20 2d 33 20 30 20 "%a 7 -3 0 "
```

The second entry has 10 bytes and requires `%a`'s declaration to recover the
logical dimension and to distinguish an ordinary array from a UI table.

## UI control state

UI type tags in this table are **Kontakt 8.13.1 in-memory declaration tags**
from `0x1407a1a10`. They are not extra bytes in the saved record. Cases in the
saved-value serializer and restorer were matched to the corresponding UI
storage and getter paths.

| UI declaration | Internal tag | Entry sigil | Stored state and restore interpretation |
|---|---:|---|---|
| `ui_button` | 0x0a | `$` | integer 0/1; restorer clamps to 0..1 |
| `ui_waveform` | 0x0b | `$` | control's integer base state, clamped to 0..1; **not** waveform audio, zone, slice or display-property serialization |
| `ui_wavetable` | 0x0c | `$` | integer base state, clamped to 0..1; not wavetable sample data |
| `ui_menu` | 0x0d | `$` | selected zero-based **position** in the item vector |
| `ui_knob` | 0x0e | `$` | integer control value, clamped to declared bounds |
| `ui_value_edit` | 0x0f | `$` | integer control value, clamped to declared bounds |
| `ui_table` | 0x10 | `%` | all integer cells, not repeated-tail compressed |
| `ui_slider` | 0x11 | `$` | integer control value, clamped to declared bounds |
| `ui_switch` | 0x12 | `$` | integer 0/1; restorer clamps to 0..1 |
| `ui_text_edit` | 0x13 | `@` | current text bytes |
| `ui_file_selector` | 0x14 | — | no saved-value serializer case; persist a separate string/path or array in script code |
| `ui_xy` | 0x15 | `?` | all binary64 coordinate cells as text; X/Y pairs in declaration order |
| `ui_label` | 0x19 | — | no saved-value serializer case; rebuild label text/properties from script variables |
| `ui_level_meter` | 0x1a | — | no saved-value serializer case; live meter attachment/display is not saved-variable data |

The same serializer also has no saved-variable case for `ui_panel` (0x17) or
`ui_mouse_area` (0x16). No entry means no value in this table; it does not prove
that other Kontakt native/UI records contain no properties for that control.

Engine menu serializer uses UI field `+0x70`; restorer writes the same index
field. Getter `0x14078fc50`, case 0x0d, indexes the item vector (32-byte elements)
and returns the selected item's integer value. This establishes the essential
translation:

```text
saved decimal -> menu selection index -> ordered item vector -> KSP integer value
```

Menu identity/order is established while executing init and `add_menu_item`.
A host-facing menu index and KSP `$menu` value are therefore separate quantities.
The typed reader reports `MenuIndex`, without performing menu construction,
clamping or runtime assignment. A consumer must supply `Some(sampler_ksp::model::WidgetKind::Menu)`.

## Instrument state, snapshots, init and NKA

`make_persistent` and `make_instr_persistent` do not create distinct entry tags.
Engine `0x140792460` sets declaration persistence mode 1 or 2. Collector
`0x14078f820` feeds the serializer a snapshot-selection flag. Serializer
`0x140781d20` excludes mode 0 always and excludes mode 1 when collecting a
snapshot. Instrument saves include both persistent modes; snapshots exclude
instrument-only mode. Recovering the mode requires the script, not its entry.
[NI variables manual](https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/variables#make_instr_persistent--).

A snapshot is saved state for an existing instrument. It does not supply its own
script declarations or init defaults. Native chunk 0x4f has the following
public layout, as decoded by the existing `ni-file::Snapshot` reader and verified
across all measured snapshots:

| Version | Prefix before the script tables | Script tables |
|---|---|---|
| 1 | chunk 0x33 compact groups; two instrument FX arrays; 16 bus objects (v0x11) | exactly five tables, one per script slot |
| 2 (engine only; absent from corpus) | chunk 0x33; two FX arrays; 16 buses | five tables, **then** the main FX array |
| 3, flags 0 | u32 flags; chunk 0x33 compact groups; two FX arrays; 16 bus objects (v0x12); main FX array | exactly five tables |
| 3, flags 3 | u32 flags only (script-only snapshot) | exactly five tables |

Each slot table is `u32 count; count × (u32 entry_bytes; entry[entry_bytes])`.
The grammar is the same as in script 0x06. Empty slots remain present; slot
indices must not be compacted. Other v3 flags remain unsupported by the existing
reader and were not observed. Engine reader `0x140d04600` also accepts v2,
which places its main FX array **after** the five tables. In v3 the engine
reads native state for flags below 2 and omits it for flags 2 or greater;
writer `0x140d13070` follows the same boundary. Flags 1/2 and v2 lack corpus
validation and are not admitted by the current snapshot container reader. The flags express which native state is present,
not whether a particular variable is instrument-persistent. Snapshot metadata
0x51 provides names used to locate the base instrument; resolution belongs to
the owning loader.

The required lifecycle is:

1. Keep the selected instrument/snapshot saved table separate from current
   working variables. Execute declarations and init assignments to obtain
   defaults and construct menus and other controls.
2. `read_persistent_var(v)` can retrieve a saved value at that point in init.
   In the inspected engine, the matching name is restored immediately and the
   entry is removed from the pending table, so a later init assignment to that
   variable is not overwritten again by that same entry at init completion.
   Engine read-persistent path calls `0x140782eb0` and erases the matched entry;
   the normal init completion iterates the entries still pending.
3. At init completion, restore remaining saved entries, then run
   `on persistence_changed`. Missing saved entries leave the initialized value.
   The callback can update groups, engine parameters and UI derived from state.

The end-of-init ordering and the immediate-read API are documented by NI;
the pending-entry consumption detail comes from static engine analysis and
still needs a live reference-host differential test.
[NI variables manual](https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/variables#read_persistent_var--),
[NI callbacks manual](https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/callbacks#on-persistence_changed).

`set_snapshot_type` acts across all five slots:

| KSP snapshot type | Run init on recall? | Save native engine state? | Then |
|---:|---|---|---|
| 0 (default) | yes | yes | restore saved KSP state; persistence callback |
| 1 | no | yes | restore saved KSP state; persistence callback |
| 2 | yes | no | restore saved KSP state; persistence callback |
| 3 | no | no | restore saved KSP state; persistence callback |

Types 1 and 3 preserve the running script/control setup and instrument-persistent
values. A consumer must retain the four-valued recall policy as well as whether the
snapshot contains native state: policies 0/1 save native state and differ in
whether init runs; policies 2/3 omit native state and also differ in init.
The measured flags 0/3 alone do not exercise the other two policies. NI documents an audio-engine reset on every snapshot load.
[NI general commands manual](https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/general-commands#set_snapshot_type--).

NKA is an external array file, not a special saved-entry tag or automatic
pointer followed by the reader. Its documented form is the full array name on
the first line, then one cell per line. A persisted path is an ordinary `@`
string; the script's `load_array[_str]` call and resource/path lookup establish
the relationship. `load_array` in init implicitly makes the array persistent,
so pending saved state can overwrite freshly loaded NKA data at init completion.
NI recommends reading persistence before loading in that situation.
`load_array_str` does not implicitly mark persistence. Calls outside init are
asynchronous; mode 1/2 `load_array` and `load_array_str` in init are synchronous.
Resource-container NKA files require a final newline. The typed saved-entry
reader never opens a path or applies NKA data.
[NI load/save manual](https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/load-save-commands).

## Reader API and admission boundary

The public API is in `sampler-kontakt`:

```rust
SavedEntry::parse(entry_bytes, widget: Option<sampler_ksp::model::WidgetKind>, limits: Limits)
    -> Result<SavedEntry<'_>, Error>
SavedEntry::from_bytes(raw: Bytes<'_>, widget, limits)
    -> Result<SavedEntry<'_>, Error>
```

`from_bytes` accepts entries from `Script::persistent.iter()` and retains their
absolute source offset. `parse` accepts the entry bytes from an owned snapshot
string's `as_bytes()`; its offset starts at zero. `None` denotes a plain variable; `Some(kind)` reuses the existing KSP widget type.
Both return the original name,
raw bytes and a `SavedValue`: `Int`, `Real`, `MenuIndex`, `Text`, `Ints`, `Reals`
or `Texts`. Numeric lists expose typed `iter()` and `len()`, with `ArrayTail`;
string lists expose byte-slice `iter()` and `len()`. Parsing and iteration
allocate nothing. Source integers/reals remain unclamped; admission belongs to
the runtime owner. Unknown sigils, invalid numbers, NULs, malformed names,
incompatible declarations and exceeded limits return errors without logging
saved text. Unsaved UI classes return `UnsupportedLayout` if presented with a
manufactured entry.

The source decoder does not infer declarations, look up menu item values,
expand arrays to script dimensions, select snapshot precedence, resolve files,
run callbacks or modify playback state. The existing runtime `library::saved`
and snapshot application path were deliberately not changed by this work.
The KSP agent must replace their assumptions using declaration-aware decoding,
menu index resolution and array-tail handling, and implement the lifecycle above.

## Corpus validation

Initial full-tree scan, **including recovery saves**: 2,741 `.nki`, 94 `.nkm`, 1,103 `.nksn`, no `.nkb`;
3,938 file paths, 15,540 script records, 801 v1 and 302 v3 snapshots. No container
or saved-table framing failures. Every script was v0x60 with no trailing
extension. Populated entries: 732,145 `$`, 69,173 `%`, 102 `?`, 6,414 `@`, 2,707
`!`; total 810,541. All were UTF-8. Every `$` payload was one integer token;
all `%` payloads were numeric token lists; every `?` was a two-cell real list.
String arrays used LF separators with empty cells preserved.

The master census excludes project recovery: **1,937 paths = 781 NKI +
53 NKM + 1,103 NKSN**, with 834 instrument/multi paths. The 810,541 entries
above belong to the 3,938-path scan and must not be divided by the master
834-path instrument/multi denominator.

The declaration-aware second-pass table and focused library measurements are
recorded below. A “0 present” row is a coverage gap, not proof that the format
cannot contain that type.

<!-- MEASUREMENTS -->

### Reproduction

The census exports aggregate shapes, counts, completed paths and errors, never decrypted source
or a raw saved-table dump. `--probe` is limited to derived integer velocity/mute
state and menu item-value lists for the targeted instruments. Increment `--start`
by 250 between calls, retain each shard's output in the agent cache, and sum
its `COUNT`, `FILES` and `LIBRARY` rows. `ITEM_DONE` records make completed paths
reviewable; `ITEM_COUNT` stores per-path counter deltas for filtering against
the master path set without mixing denominators. `--files` selects a
`path\tstatus` TSV (such as the master census's `files.tsv`); omitting it
selects the full tree including recovery saves. The declaration-aware second
pass uses the master list. Snapshot shards inspect a matched base NKI for declaration context
without adding that base's records to the shard totals. The wrapper's target
override is removed for Cargo to use this machine's shared target directory.

```sh
~/.cache/kontakto-heavy env -u CARGO_TARGET_DIR \
  cargo build --locked -p sampler-kontakt --example persistence_census
~/.cache/kontakto-heavy env -u CARGO_TARGET_DIR \
  /mnt/Windows11/DEV_WORKSPACE/Toolchains/User/cargo-target/debug/examples/persistence_census \
  /mnt/MAIN_STORAGE/Libraries/Kontakt \
  --files ~/.cache/kontakto-gpt-format-gaps/census/files.tsv --start 0 --count 250
~/.cache/kontakto-heavy env -u CARGO_TARGET_DIR \
  cargo test --locked -p sampler-kontakt --test persistence
```

Fixtures cover menu index/value separation, all serialized UI scalar types,
ordinary/UI array-tail markers, real values and exponent notation, strings and
empty string-array cells, malformed/unknown/oversized records, v0x50 password
layout, absolute source offsets and no-allocation parsing/iteration.
