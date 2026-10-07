# Kontakt program, group and zone object research

Work in progress on `v2/gpt-format-objects`. This family owns program `0x28`,
voice groups `0x32`, group list `0x33`, zone list `0x34`, loop array `0x39`,
source mode `0x0e` and group start criteria `0x38/0x0f`. It does not own the
master gap map, script resource resolution, save settings or quick browse.

## Evidence and limits

The vendored `vendor/ni-file` and its `doc/presets/Kontakt` directory are the
available implementation/spec source. Upstream ni-file is DMCA-blocked;
claims about its current state are **unverified**. The public
[hexfiend templates](https://github.com/monomadic/hexfiend-templates/tree/master/Kontakt)
provide historical layouts, not complete modern semantics.

The read-only v1 importer at `decipher-readers-v1/src/import.rs` supplies the
128-bit voice-group mask and one-based assignment mapping. The read-only
`DSP_FORMAT_SPECIFICATION.md`, section “Kontakt chunks and structured objects”,
supplies framing and authored program/group/zone layouts. Native reader/writer
inspection in `t3code-80fe786b/artifacts/engine-analysis-2026-10-07` supplies
serialization order; no decompiled implementation is copied here.

A decoded number is not proof of its display scale, selection semantics or
DSP law. Unknown fields remain explicitly unknown. Existing reference tables
are read from branch `v2/kontakt-reference`; no Wine UI is available to this
agent. Corpus measurements and reader validation will be added below.

## Framing

All scalars are little-endian and unaligned. `WString = u32 UTF-16-unit count`
followed by exactly twice that many bytes. A chunk is `u16 id; u32 body_bytes;
body`. A structured object is `u8 structured; u16 version`, then three counted
byte areas: private, public, children. Children are bounded chunks. An
unstructured object still has its version; its payload length comes from the
containing schema.

Group list starts with `u32 count` then structured group records. Zone list
starts with `u32 count` then `u32 owner_group; structured_zone` per entry.
Loop list starts with an eight-slot presence mask, retaining empty slots.
Voice groups start with an unstructured v0x60 program voice limit, followed
by a **16-byte (128-bit)** presence mask and one v0x60 limit per occupied bit.
The mask is not eight bytes. Group voice assignment is one-based, with zero
meaning unassigned; zone owner group is zero-based.

## Native enum namespaces

Source reader/writer entries `0x140d03aa0` / `0x140d12910` establish different
internal and serialized mode IDs. Serialized mode 9 is Wavetable; do not copy
internal enum IDs or KSP menu positions into the file decoder. Start criteria
reader/writer `0x140d04400` / `0x140d12ec0` accept mode IDs 0..5 and operator
IDs 0..2. The
[official manual](https://docs.native-instruments.com/ni-tech-manuals/kontakt-manual/en/classic-view#group-start-options)
lists six conditions: Always, Start on Key, Start on Controller, Cycle Round
Robin, Cycle Random and legacy Slice Trigger. The request's “20” must not be
turned into invented modes; all serialized row parameters and supported
engine-query constants must be accounted for individually.

## Reference observations requiring playback-agent action

At the fetched reference branch, `probe/areia_vln_sus.md` shows v2 selecting
Dyn3 instead of Kontakt's Dyn2, many unidentified takes and starts near
90,601 frames. `probe/barbarian.md` shows agreement at key 48 but wrong
articulation/mic at key 60. These tables establish audible differences,
not the exact byte responsible. KSP/runtime selection and authored source
settings must be traced separately; this agent changes readers only.

## Public byte layouts

Offsets are relative to the public payload, not the chunk header. `S` is the
byte after the name WString. No padding is inserted between scalars.

### Program 0x28

| Offset | Type | Field | Playback responsibility |
|---|---|---|---|
| 0 | WString | name | Identity/UI |
| S | f64 | num_bytes_samples_total | Saved memory figure, not decoded PCM length |
| S+8 | i8 | transpose | Shift mapped note/pitch according to instrument semantics |
| S+9 / +13 / +17 | f32 ×3 | volume, pan, tune | Linear gain, signed pan, linear frequency ratio |
| S+21 / +22 | u8 ×2 | low/high velocity | Program-wide event acceptance |
| S+23 / +24 | u8 ×2 | low/high key | Program-wide event acceptance |
| S+25 | i16 | default_key_switch | Initial native keyswitch state; scripts may replace it |
| S+27 | i32 | dfd_channel_preload_size | Per-instrument preload override; zero uses global policy |
| S+31 | i32 | library_id | Resource/library identity |
| S+35 | u32 | fingerprint | Retained identifier; no audio law assigned |
| S+39 | u32 | loading_flags | Retained bits; bit semantics unresolved |
| S+43 | u8 | group_solo | Native group solo selection state |
| S+44 | i32 | cat_icon_idx | Metadata icon |
| S+48 | WString ×3 | credits, author, URL | Metadata |
| after three WStrings | i16 ×3 | category 1/2/3 | Metadata |
| remainder | version-dependent | resource references and extension | Metadata lead owns extension work |

Historical templates describe a common base at v0x80, a resource reference by
v0x92/v0xa0, wallpaper by v0xa5, two extra compatibility references at v0xa6,
and four references at v0xa8. These are **historical spec claims**, not proof
that every intermediate version is interchangeable. Corpus versions and
actual parse results are recorded in the measurement table.

### Group records inside 0x33

| Offset | Type | Field | Playback responsibility |
|---|---|---|---|
| 0 | WString | name | Stable group ordering/identity |
| S / +4 / +8 | f32 ×3 | volume, pan, tune | Linear gain/pan/frequency ratio inherited by voices |
| S+12 | bool | key_tracking | Repitch with note relative to zone root when enabled |
| S+13 | bool | reverse | Reverse the selected source view |
| S+14 | bool | release_trigger | Select at real note/gate release under pedal/script policy |
| S+15 | bool | release_trigger_note_monophonic | Cut same-note release voices on retrigger |
| S+16 | i32 | rls_trig_counter | Release age/decay counter; exact gain law unresolved |
| S+20 | i16 | midi_channel | Native group MIDI channel filter; sentinel retained |
| S+22 | i32 | voice_group_index | 1-based voice-limit assignment, zero unassigned |
| S+26 | i32 | fx_idx_amp_split_point | Voice insert ordering before/after amplifier |
| S+30 / +31 | bool ×2 | muted, soloed | Group audibility and solo arbitration |
| S+32 | i32 | interp_quality | Source interpolation policy, serialized ID retained |
| remainder | bytes | public extension | No guessed meanings |

**There are no standalone group key/velocity ranges in this public record.**
Mapping ranges belong to zones. The Start on Key criterion is a latched
keyswitch condition, not a per-note mapping range. Treating its keys as the
playable group range rejects otherwise valid notes.

The current modern private-rack locator verifies 136 twelve-byte records,
then 24 bytes, before the insert rack. Those 136 records correspond to the
triangular collection of eight-byte masks (1+2+...+16). The first u32 of each
record is the byte count 8; remaining mask bits/state are unresolved. The
private rack ends at its parsed cursor, then a flag precedes BParSrcMode.
A zero flag still has a source record; it is not an absence marker.

### Zone records inside 0x34

| Offset | Type | Field | Playback responsibility |
|---|---|---|---|
| 0 / 4 / 8 | i32 ×3 | sample_start, sample_end, sample_start_mod_range | Frames; end is signed relative to source end; modulation is a range, not a second start |
| 12 / 14 | i16 ×2 | low/high velocity | Inclusive mapping range |
| 16 / 18 | i16 ×2 | low/high key | Inclusive mapping range |
| 20 / 22 | i16 ×2 | low/high velocity fade | Fade widths inside velocity range |
| 24 / 26 | i16 ×2 | low/high key fade | Fade widths inside key range |
| 28 | i16 | root_key | Recorded pitch root for key tracking |
| 30 / 34 / 38 | f32 ×3 | zone_volume, zone_pan, zone_tune | Linear gain, signed pan, linear pitch ratio |
| 42, v0x9a+ only | bytes[6] | filename_prefix | Preserved; semantic meaning unresolved |
| F = 42 (old) / 48 (v0x9a+) | i32 | filename_id | File table reference |
| F+4 / +8 | i32 ×2 | sample_data_type, sample_rate | Saved sample metadata, validate against decoded source |
| F+12 | u8 | num_channels | Saved channel count |
| F+13 / +17 | i32 ×2 | num_frames, reserved1 | Saved source length; reserved word retained |
| F+21, v0x95 and earlier | i32 | reserved2 | Historical word removed at v0x96 |
| R = F+25 (v0x95) / F+21 (v0x96+) | i32 | root_note | Sample metadata root, distinct from zone root |
| R+4 | f32 | tuning | Sample metadata tuning, distinct from zone tuning |
| R+8 / +9 | bool / i32 | reserved3, reserved4 | Retained, semantics unresolved |
| remainder | bytes | unknown_tail | Retained verbatim in memory |

Known public sizes are 80 bytes at v0x95, 76 at v0x96..0x99, and 82 at v0x9a,
before extension bytes. The six-byte prefix changes sample-reference offsets;
it is not safe to keep reading the file ID at 42 for v0x9a.
