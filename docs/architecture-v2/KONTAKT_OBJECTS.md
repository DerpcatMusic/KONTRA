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
| R+8 / +9 | u8 / i32 | reserved3, reserved4 | Retained, semantics unresolved |
| remainder | bytes | unknown_tail | Retained verbatim in memory |

Known public sizes are 80 bytes at v0x95, 76 at v0x96..0x99, and 82 at v0x9a,
before extension bytes. The six-byte prefix changes sample-reference offsets;
it is not safe to keep reading the file ID at 42 for v0x9a.

### Voice limits 0x32 / embedded 0x2b v0x60

Each limit starts `u8 0; u16 0x60; WString name`, then `i16 kill_mode`,
`u8 prefer_released`, `i32 max_num_voices`, `i32 ms_fade_time`, `i32
exclusion_group`. The program limit comes first, followed by the 128-bit
mask and one complete limit for every occupied slot. Negative exclusions
mean disabled in the v1 importer; retain the signed authored value.

Kill modes 0..4 map to Any, Oldest, Newest, Highest, Lowest in the existing
native translator. “Any” currently means quietest in KONTRA, which needs its
own host comparison. Prefer-released means steal released voices first (the
old ni-file comment incorrectly suggested keeping them). A non-negative
exclusion group chokes other assigned groups sharing that exclusion class.
Do not clamp saved voice counts or fade time in the source decoder; policy
validation is a separate lowering step. Voice-group holes cannot be compacted.

### Group start options 0x38 / embedded 0x0f v0x70

`u8 mask` uses four low bits. Each occupied row starts `u8 0; u16 0x70`, then:

| Row payload offset | Type | Field / query suffix | Required behavior |
|---|---|---|---|
| 0 | i32 | mode / MODE | 0 Always, 1 Key, 2 Controller, 3 Round Robin, 4 Random, 5 Slice Trigger |
| 4 | i32 | next_criteria / NEXT_CRIT | Combine with the following row; native IDs 0..2 retained |
| 8 / 10 | i16 ×2 | key_min/max / KEY_MIN/MAX | Latch keyswitch activation, inclusive bounds |
| 12 | i16 | controller / CONTROLLER | Controller number for the row |
| 14 / 16 | i16 ×2 | cc_min/max / CC_MIN/MAX | Controller value acceptance range |
| 18 | i32 | cycle_class / CYCLE_CLASS | Authored round-robin position, not group ID |
| 22 | i32 | slice_zone_idx / ZONE_IDX | Referenced zone for legacy slice triggering |
| 26 | i32 | slice_zone_slice_idx / SLICE_IDX | Referenced slice index |
| 30 | bool | sequencer_only / SEQ_ONLY | Internal slice sequencer gate |

The public query prefixes are `ENGINE_PAR_START_CRITERIA_`. The six mode
constants use `START_CRITERIA_NONE`, `ON_KEY`, `ON_CONTROLLER`,
`CYCLE_ROUND_ROBIN`, `CYCLE_RANDOM`, `SLICE_TRIGGER`. The three operator
constants use `START_CRITERIA_AND_NEXT`, `AND_NOT_NEXT`, `OR_NEXT`. This is
**all twenty identifiers: eleven query parameters + six modes + three
operators**, not twenty modes. The operator numeric/name association still
needs an independently controlled check; the reader exposes exact numbers.

Rows remain in original occupied-mask order, including inactive/default row
parameters and the bounded list extension. `cycle_class` is not a voice-group
assignment. Start on Controller is stateful and can be combined with a
keyswitch or cycle condition. The Always row terminates a populated condition
list. Script `allow_group` / `disallow_group` is a separate selection layer;
KSP/core owners must define how it interacts with native start criteria.

### Loop array 0x39 / embedded 0x05 v0x60

`u8 mask` covers eight slots. Occupied entries have `u8 structured; u16 0x60`.
An unstructured payload has exactly 25 bytes; a structured entry carries its
own bounded private/public/child areas and its public payload starts:

| Offset | Type | Field | Required behavior |
|---|---|---|---|
| 0 | i32 | mode | Native loop mode; disabled entries stay present |
| 4 / 8 | i32 ×2 | loop_start, loop_length | Frame start and length; exclusive end is start+length |
| 12 | i32 | loop_count | Counted traversal; zero is native infinite/default candidate |
| 16 | bool | alternating_loop | Ping-pong traversal |
| 17 | f32 | loop_tuning | Linear loop pitch ratio; not semitones |
| 21 | i32 | x_fade_length | Crossfade length in source frames |

Current playback admits modes 1 continuous and 2 until release, but the mode-2
Kontakt association remains unverified. Count semantics, multiple-loop
transition order and non-unit loop tuning require native comparison. Every
original occupied slot and all these values are now retained in the IR;
playback wiring belongs to the core agent. The old owned reader compacted
loop holes, changing the source slot used in reports and future KSP queries.

## Source-mode records 0x0e

The record lives in the group-private area after its fully parsed insert
rack and a preceding flag. Its own header is `u8 structured=0; u16 version;
u32 serialized_mode`. The new reader consumes one versioned record, leaving
the private group trailer untouched. Source flags and unsupported versions
remain explicit; no source is admitted to DSP by this reader.

Native writer/reader inspection establishes the serialized/internal mapping:

| Stored ID | Internal ID | Source-mode interpretation |
|---:|---:|---|
| 0 | 0 | Sampler |
| 1 | 1 | Tone Machine |
| 2 | 3 | Time Machine 1 |
| 3 | 4 | Time Machine 2 |
| 4 | 5 | Beat Machine |
| 5 | 6 | DFD |
| 6 | 7 | MPC60 |
| 7 | 8 | S1200 |
| 8 | 9 | Time Machine Pro |
| 9 | 2 | Wavetable |

The internal/serialized permutation is verified directly; the names other
than Wavetable still need a controlled saved-mode/native-query cross-check.
The official API's source names are not evidence that its table order equals
either integer namespace. Modern source common fields are:

| Record offset | Type | Decoder field | Meaning confidence |
|---:|---|---|---|
| 3 | u32 | mode | Stored enum verified |
| 7 | f32 | common_float_7 | Layout verified; semantic name unassigned |
| 11 | bool | common_flag_11 | Layout verified; semantic name unassigned |
| 12 | u32 | common_enum_12 | Native reader accepts 1..5; semantic name unassigned |
| 16 | bool | common_flag_16 | Layout verified; semantic name unassigned |
| 17 | f32 | timing_value | Versioned sync/timing value; unit/display law unverified |
| 21 | f32 | timing_unit | Added at v0x102; unit ID stored as float |
| 25 | f32 | timing_free | Added at v0x102; free timing value |
| 29 | bool | timing_flag | Added at v0x102; flag meaning unassigned |

At v0x100/101 the common record ends after offset 20 (21 bytes total).
At v0x102+ it ends after offset 29 (30 bytes total). Do not continue reading
the 32-byte snapshot identity blob as if it were a uniform source schema.

| Stored mode | Extension after common record | Version differences |
|---:|---|---|
| 0, 3, 6, 7 | None | Common fields only |
| 1, 2 | f32, f32, bool | `machine_float_1/2`, `machine_flag` |
| 4 | f32, f32, bool | `slice_float_1/2`, `slice_flag_1`; another bool at v0x106 |
| 5 | bool, u32, u32 | `dfd_flag`, `dfd_integer_1/2`; do not assume sample-frame offsets |
| 8 | bool, bool, f32, f32 | v0x100 uses u32 instead of second bool; retain original version |
| 9 | Four f32, two u32 | Position, form1, phase, phase_random, form_type, quality at v0x103 |
| 9 | bool, f32 | Inharmonic enable/value appended at v0x104 |
| 9 | f32, u32 ×3, f32 ×2, 16 bytes | Form2/type, mod_wave/type, mod_amount/tune and nested tail at v0x105+ |

Wavetable v0x106 offsets are position 30, form1 34, phase 38, phase_random 42,
form_type 46, quality 50, inharmonic_enabled 54, inharmonic 55, form2 59,
form2_type 63, mod_wave 67, mod_type 71, mod_amount 75 and mod_tune 79.
Its 16-byte nested state at 83 is retained as four unassigned u32 words.
The independent existing WavetableSource reader establishes 99-byte length.
Other fields cannot honestly be labelled “fully deciphered” merely because
their scalar values are exposed.

Playback owners: source mode must choose the corresponding playback engine;
time/formant/grain/slice controls require established unit laws and the
correct mode-specific processing. Wavetable position/form/phase controls
belong to oscillator scanning and shaping; phase randomness applies per
trigger. DFD parameters concern source streaming policy, not articulation
selection or sample start. `SourceField` retains byte offset, name and exact
Float/Integer/Flag value; do not normalize unassigned fields in KSP getters.

## IR retention and remaining semantic gaps

`Instrument::kontakt_objects` carries program settings, 128 voice slots,
original groups (including muted ones), and original zones (including missing
samples). It preserves source versions, all eleven start-row parameters and
mask, original loop slots and all loop controls, sample metadata, signed
sentinels, six-byte filename prefixes and zone/list extensions. These are
source indices, not the dense playback group's or prepared region's indices.
Snapshots and script init writes modify playback state without overwriting
the authored capture. The native lowerer does not automatically consume it.

Unresolved required fields: program-private settings including fixed voice
groups, Time Machine voice limits/HQ/legacy flags and controller policy;
source-control semantic names/units outside the established wavetable
prefix; start-operator ID/name association and runtime arbitration; exact
loop mode/count/tune and multiple-loop behavior; historical group-private
versions preceding the fixed modern locator. Raw framing retains these
areas, but a raw blob is not a decoded field. The master gap-map lead should
carry these as open, not mark this family complete.
