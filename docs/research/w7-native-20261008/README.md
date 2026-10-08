# Kontakt 8.13.1 native evidence for W7

All addresses are VAs in the standalone image at base `0x140000000`, SHA256
`0fe6356e0879d058b6e5b73507c54c5e345cea451b35287c974e438291d4dae8`.
Do not apply these addresses to the differently based VST3 payload.

`static.asm` contains original-byte disassembly. `static.c` contains selected
decompiler output; register types and reconstructed signatures can be wrong.
`provenance.json` pins each original byte range. Run:

```sh
python3 docs/research/w7-native-20261008/verify.py
```

This executes the original criteria, loop, automation, and preceding-array
public readers in Unicorn with explicitly stubbed scalar/string I/O, tag
lookup, and the security-cookie check. All 12 synthetic reader vectors pass.
`reader_vectors.tsv` is serialization evidence. It does not measure playback.
It also executes the original address gate and Program/group host-address
remap on 16 boundary vectors in `host_address_vectors.tsv`.

## Criteria join constants

| Serialized join | Native name | Evidence |
|---:|---|---|
| 0 | AND | Reader `0x140d04400`, writer `0x140d12ec0`, UI `0x14061c033`, diagnostic `0x140878025` |
| 1 | AND NOT | Same independent consumers |
| 2 | OR | Same independent consumers |

The reader consumes the second i32 and preserves its numeric ID in object
`+0x28`. Diagnostic join load is `0x140878054`. String VAs are `0x144f18208`
(and), `0x144f18218` (and not), and `0x144f18228` (or).
`criteria_ids.tsv` records these mappings.

KSP registration IDs `0x256/0x257/0x258` at `0x14045ab91` and its following
registrations identify symbols. They are not serialized join values.
The string ledger's file offsets need PE section translation before they are
used as RVAs. For example AND_NEXT is file offset `0x4e7b988`, RVA
`0x4e7d788`, VA `0x144e7d788`.

Evaluation precedence, short circuiting, and handling of empty intermediate
criteria remain unverified. The diagnostic traversal prints descriptions.

## BAutomationObject source and target

V0x71 public reader `0x140cfec70`, writer `0x140d0f570`:

| Public offset, after raw/version header | Wire type | Native object offset | Verified meaning |
|---:|---|---:|---|
| 0 | i32 enum | 0x28 | 0 unknown, 1 midi_ctrl, 2 host_par |
| 4 | u8 | 0x31 | softTakeOver |
| 5 | u16 | 0x32 | source address: MIDI controller or host parameter, according to mode |
| 7 | i32 | 0x40 | objIdx |
| 11 | i32 | 0x44 | additional target selector; text reader derives FX-chain selector for older formats |
| 15 | f32 | 0x50 | rangeFrom |
| 19 | f32 | 0x54 | rangeTo |
| 23 | u32 N then N bytes | 0x34 | ParTag lookup of byte-string name |

The raw flag and version are 3 bytes before public offset0, not included in
the table. A 22-byte tag yields `3+23+4+22 = 52` bytes per complete record.

These names come from the native text reader `0x14091f1b0`, not guessed field
labels. Strings: `0x144f0f618` midi_ctrl, `0x144f0f628` host_par,
`0x144f0f638` softTakeOver, `0x144f0f670` adress, `0x144f0f678` parTag,
`0x144f0f680` objIdx, `0x144f0f688` rangeFrom, `0x144f0f698` rangeTo.
Mode1 store is `0x14091f366`; mode2 store `0x14091f389`; softTakeOver store
`0x14091f3de`. Mode helper `0x140cf0360` accepts only 0,1,2.

Runtime index rebuild `0x14096e6b0` inserts mode1 into source list
`0x35+address`, mode2 into `0xb5+address`. The reverse notification path
`0x140958d50` only invokes the host callback for mode2. Do not reinterpret
a mode1 source address as a host automation slot.

### Host source address bounds

The v0x71 reader calls u16 I/O at `0x140cfecb7` and stores AX unchanged at
`0x140cfecbf`, object `+0x32`. An emulated reader input of65535 is preserved.
The wire field has no reader clamp to2047 or2048.

Native `NewAutomationObject` checks its source address independently of the
source mode: `0x1408f3479` loads EAX=`0x801`; `0x1408f347e` loads the u16
address into R11D; `0x1408f3486` compares R11W,AX; `0x1408f348a` branches
to the no-creation path `0x1408f3da3` for unsigned address>=2049.
This address gate permits0..2048 inclusive; other creation checks still apply.

The reverse host notification path loads the global limit from
`0x144fd4c80` at `0x140958f0f`. For mode2, it loads the address at
`0x140958f5d`, compares against that limit at `0x140958f61`, and skips the
notification with JGE at `0x140958f63`. The PE's initial u32 limit is2049.

Host address remap `0x1409ab730` computes delta=`Program+0x1cf30` minus
`Program+0x1cf34`. For mode2 records whose object `+8` is0, it clamps
`address+delta` to `[0,global_limit-1]`: Program instructions
`0x1409ab7bc..0x1409ab7d6`, group instructions
`0x1409ab88c..0x1409ab8aa`. With the PE's initial limit this is0..2048.
The emulated vectors check both Program and group records, including2047,
2048,2049,65535, negative shifts, and excluded mode/flag cases.

These bounds are verified for this standalone image. The VST3 payload's
published host capacity, possible runtime changes to the global limit, and
the meaning of address2048 remain unverified. The source-list index alone
establishes no bound. Keep full-u16 wire metadata separate from host capacity.

V0x70 reader order differs: i32 mode, u8 softTakeOver, extra u8, u16 address,
i32 objIdx, f32 rangeFrom, f32 rangeTo, byte-string tag. It derives object
`+0x44`: when group selector `+0x3c >= 0`, 0; otherwise tag0x16..0x211 gives1,
tag0x214..0x292 gives the extra u8, and other tags give -1.

The coordinator supplied Areia's measured source addresses: slider0_40 CC1,
slider0_42 CC11, slider0_43 CC21, slider0_41 CC16, slider0_44 CC17. All five
softTakeOver values are0. Those input values are coordinator observations;
this work independently establishes how the native fields interpret them.

## Script slider identity

`pts_script_slider_0_40` is a slider-only ordinal in script slot0.
Native tag-table construction `0x1409b695c` loads enum `0x17db`, then
`0x1409b696f` loads string VA `0x144e9eff8`, and `0x1409b697e` calls the pair
constructor `0x1407bccd0`. Tag converter `0x14096af20` looks the string up
in a native ordered map.

Slider decoder `0x140993fc0` subtracts slot0 base `0x17b3`, yielding40.
The parameter read `0x140898570` calls `0x1408ab280` with control category9
and that decoded ordinal. `0x1407a3ba0`, category9, indexes
`parser+0x85e8 + ordinal*0x218`; RTTI identifies this vector's elements as
`BScriptParser::UISliderControl`.

`0x1407a5760`, case9, sets slider value at element `+0x70`, clamps using
`+0x1b0/+0x1b4`, and reads the distinct control identifier at `+8` for
callback dispatch. Match the zero-based slider declaration ordinal within
the requested script slot to the frontend widget. Do not use
`32768+tag_ordinal` as its UI ID. Native get_ui_id arithmetic is unverified.

## Program private framing

Private reader `0x140d0a2c0` consumes a 61-byte fixed prefix. The precise
v0xae widths and destinations are in `program_private_prefix_ae.tsv`.
Some fields are read and discarded for newer versions; they still occupy
their historical wire positions.

| Accepted Program versions | Following reader sequence |
|---|---|
| 0x80,0x82,0x90 | Additional legacy variable fields, legacy automation, arrayA, arrayB |
| 0x91,0x92,0xa0..0xa8 | arrayA, arrayB, legacy automation |
| 0xa9..0xb5 | arrayA, arrayB, modern automation |

0x93..0x9f and other versions are rejected by this native private reader.
The older 0x80/0x82/0x90 extra variable fields have not been fully decoded
here and should be reported unsupported by a bounded new decoder.

Both preceding arrays begin with u32 count, then count raw-u8/version-u16
headers and public records without object-length prefixes. Raw must be0.
Element readers only accept v0x50 even though their outer version checks
permit lower values through to those rejecting readers.

| Array | Outer helper | Element public reader | Public record fields |
|---|---|---|---|
| A | 0x140cf0ad0 | 0x140cfedf0 | i32; UTF16 string; three f32; u8; u16 |
| B | 0x140cf0dc0 | 0x140cfeea0 | i32 enum1or2; i32; UTF16 string; u32 K; K i32 values |

UTF16 string helper `0x140cfbaf0` calls `0x142a784b0`, which reads u32
code-unit count N and exactly `2*N` bytes. ArrayA public length is `23+2*N`.
ArrayB public length is `16+2*N+4*K`. Add3 for each raw/version header.
Native arrayB stops at64 i32 values; reject K>64 rather than letting
unconsumed bytes become subsequent framing.

Modern helper `0x140cf14f0` reads u32 outer count/presence. If nonzero,
`0x140cf08b0` reads a second u32 count and consumes that many BAO records.
Legacy `0x140cefcf0` reads outer capacity, allocates it, and, if nonzero,
reads inner count and consumes up to min(capacity,count) BAO records.
Writer `0x140cf25a0` writes the vector size and then the record count, which
are equal for normal saved data. Mismatched counts need explicit treatment.

When arrayA and arrayB are both empty: countA is at61, countB at65,
automation outer count at69, inner count at73, first raw/version header
at77. These are conditional offsets. Nonempty preceding arrays move the
automation section, so production must parse their lengths.

## Loop serialization and pending measurements

Loop reader `0x140cff510` accepts v0x60. Serialized modes0,1,2,3 map to
native internal modes0,3,4,1. Writer `0x140d0faa0` reverses that map.
Start, length, count, alternate, tuning, and crossfade are read in that
order. Their object offsets are8,12,24,28,36,40. These fields are preserved
by the original reader in `reader_vectors.tsv`.

Counted wrap, pingpong counts, tuning onset/rate, pingpong crossfade and
multi-slot traversal remain unverified. `cases.json` and `scenario.txt`
describe 52 authored synthetic cases awaiting native recordings. Their
contents are test inputs, not expected playback vectors.
`prepare_cases.py` preserves the rejected fixture experiment for reproduction;
it is not a working NKI writer or part of the native reference harness.

Native reference calibration passed before the batch (left -0.091dB,
right -0.023dB, residuals -148.9/-151.9dB). The edited synthetic NKI was
rejected before MIDI playback, so no waveform-derived vectors are claimed.
An unchanged literal-run FastLZ re-encoding loads. Even a one-byte public
transpose edit is rejected. A native-saved control proves BPatchHeaderV42
offset182 equals CRC32 of inner Kontakt chunks for both compared controls,
but updating that field alone does not make edited fixtures load. Further
serialization/integrity work remains. Preparation and UI checks took place
outside the quiet lock. The actual quiet window ended on the load failure.

No library instrument or library sample was used to author fixtures.
Authored fixtures and source noise stay in tmpfs. Native writes target a
tmpfs FUSE overlay; the original Wine prefix stays read-only. WAV captures
are deleted after metric extraction.
The owned native session, sink, Xvfb and FUSE process were stopped; the overlay
was unmounted and owned tmpfs fixtures removed. Original prefix registry
mtimes remained unchanged. No quiet lock remains held by this work.

## Global preferences and old loop proof

Read-only overlay `Portapotty/UserData/Settings.cfg`, [Kontakt Application]:

```ini
midiChannelAssignmentK76 = dw:0
keepK1MIDIChannel = dw:0
unwindAutomationIDs = dw:1
AB2 MidiDevice0 Name = sz:Midi Through Port-0
AB2 MidiDevice0 UID = sz:WinMidi Input Midi Through Port-0
AB2 MidiDevice0 PortID = dw:0
AB2 MidiDevice0 Type = dw:0
```

No explicit CC-to-host/widget binding appeared in the relevant-key
inventory or matching Kontakt registry sections. This negative inventory
does not exclude inline instrument automation records or other opaque state.

The ignored `artifacts/combined-80dbb6f` probe source was not found in current
worktrees, the main checkout, or bounded searches of relevant cache trees.
Commit80dbb6fb only contains CHANGELOG/docs/export-script changes. The
documented production loop comparison is KONTRA versus independently
unrolled PCM, not a Kontakt reference. It cannot verify native endpoint or
count semantics. The import test at src/import.rs1354 is a metadata witness.
