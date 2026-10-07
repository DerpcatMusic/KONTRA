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
