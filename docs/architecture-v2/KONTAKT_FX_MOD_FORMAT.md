# Kontakt effect and modulation serialization

Research branch: `v2/gpt-format-fxmod`; base `fb456b41`. This family document is
owned by the FX/modulation format agent. The master gap map is owned by
`gpt-format-gaps` and is not modified here.

## Evidence and limits

The installed corpus manifest is `/home/derpcat/.cache/kontakto-corpus/items.tsv`.
The metadata-only census is `crates/sampler-kontakt/examples/fx_mod_survey.rs`.
It reads expanded records in memory, including bypassed modules and occupied
slots in muted groups. It emits only aggregate metadata, never library bytes,
scripts, samples, access values or keys. Corpus validation and per-field/version
file counts will be appended after the census completes.

Sources:

* Vendored `vendor/ni-file/src/kontakt/objects/` readers, pinned by this branch.
* Read-only v1 `decipher-readers-v1/audits/MODULATION.md` and `src/fx/params.rs`.
* Read-only `t3code-80fe786b/docs/DSP_FORMAT_SPECIFICATION.md`, section
  **Effect parameter payloads**, and `DSP_SYSTEM_INVENTORY.md`.
* `t3code-80fe786b/artifacts/engine-analysis-2026-10-07/` KSP identifier catalog,
  cached NI engine parameter reference, native typed reader evidence.
* [NI KSP engine parameter reference](https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/engine-parameters).
* [HexFiend templates](https://github.com/monomadic/hexfiend-templates), fetched
  separately; these frame FX wrappers but do not decode their parameters.

The ni-file upstream is DMCA-blocked according to the coordinator. Upstream
coverage percentages, implementation claims and comparisons are **unverified**;
the vendored copy is the implementation evidence. KSP parameter list order is a
candidate naming aid, not proof of binary order or normalization. Full-length
parse establishes storage, not sound parity. An absent corpus type cannot be
certified by a synthetic record. Unknown fields must remain unknown.

## Framing and scopes

`Chunk = u16 id, u32 byte_count, payload`. Object payload starts with a byte
serialization flag and a little-endian `u16` version. Structured objects contain
length-prefixed private, public and child byte regions. Unstructured objects
store their remaining bytes as public data.

The FX wrapper is `0x25`; its first child identifies the concrete module.
The internal effect enumeration in wrapper private bytes is **not** that child
serialization ID. Arrays `0x3a` have eight physical slots with presence byte then
chunk. Versions `0x10`/`0x12` are uncounted; `0x13` prefixes a `u32` slot count.
Version `0x11` has no verified reader here. Program racks appear in insert, send,
main order; KSP generic addresses are respectively `1,0,2`. Bus `0x45` children
carry their own `0x3a`; generic bus address is `1000 + physical bus index`.
Group inserts live after 136 twelve-byte private records and a 24-byte trailer;
group source data follows the rack, so a streaming rack reader must not require
end-of-private-data immediately after the eight slots.

## Modulation assignments

Internal array `0x3b` has sixteen slots; external `0x3c` has 32 legacy slots and
counted version `0x13` admits 32 or 64. Slots retain physical indices.
Internal assignment `0x0d` versions `0x80/0x81` and external `0x0c` versions
`0x100..0x104` store targets in private data.

Target list: `u32 count`, then all headers, then all shapers. Each header is
`str8 parameter, f32 magnitude, i16 unknown, u8 flags, u16 smoothing_ms,
str8 target_name, [u8 module_slot], u8 invert`. String is `u32 byte_count` plus
UTF-8 bytes. Group/source targets omit `module_slot`: volume, pan, pitch,
playPos, loopStart, loopLength, warpFactor, warpFactor2, wavetablePosition,
wavetableInharmonic, wavetableModAmount, wavetableModFrequency. All other
destinations carry the slot byte, including destinations on another modulator.

Flag `0x02` is negative depth, independent of Invert. Native setter/read/write
evidence in v1 **Saved target direction and signed intensity aliases** identifies
the setter's sign bit and the serialized field. Thus signed physical depth is
`magnitude * (flags & 2 != 0 ? -1 : 1)`. Other flag bits remain opaque. `pitch`
depth is twelve semitones per unit; filterCutoff is ten octaves per unit, per
the existing v2 reference measurements. This does not establish volume, pan,
filter resonance, envelope-time or all other destination laws.

Shaper kind `0` has no data; kind `1` has enable byte and 128 float values;
kind `2` has enable byte, byte point count and `(x,y,curvature)` triples.
Disabled curves retain their data. Smoothing is stored independently per target.
The reader's linear breakpoint evaluator is not an exact curved-shaper DSP law.
V2's existing shaped-route inversion behavior is documented separately in its
reference tests and is not generalized to previously unsupported destinations.

After internal targets: four bytes `[routersOpen,bypass,retrigger,unknown]`,
`u32 unknown_id, str8 name, u32 category`. Category `2` nests envelope wrapper
`0x07`; category `1` has a direct source child. External targets end with
`str8 name,u32 category`; category `1` has source code and four retained bytes;
category `2` has no source and two retained bytes. Then `u32 unknown_id` and
version footer: none through `0x102`, one byte at `0x103`, two at `0x104`.
Unknown footer bytes are not assumed booleans.

External source codes: 1 pitch bend, 2 poly aftertouch, 3 channel aftertouch,
4 CC plus a byte number, 5 key, 6 velocity, 7 release velocity, 8 release
counter, 9 constant, 10 random unipolar, 11 random bipolar, 12 script plus
u32 value ID. Unobserved codes require native or controlled-saved evidence;
the manual's source order alone is insufficient certification.

## Source schemas

AHDSR `0x3f`, version `0x11`: attack curve, attack ms, decay ms, hold ms,
release ms, sustain linear gain, AHD-only byte; four trailing 13-byte packed
sync records and any retained extensions. The trailing records are not
proven arbitrary coefficients. Stage sync requires separate decoding.

Flex `0x40`, versions `0x11/0x12`: `u32 last_point,u32 unknown_index,u32 sustain`,
then `last_point+1` triples `(delta_ms,level,curve)`; 13-byte tail at `0x11`,
15 at `0x12`. Time is delta from the previous point; sustain is a point index.
Unknown index/tail need native loop/sync evidence before execution.

LFO `0x08`, versions `0x71..0x73`: waveform u32; fade-in, rate/count, pulse width,
phase float values; normalization byte; frequency sync record `(note_value,
unknown,unknown,flag)`; fade sync record of the same form. Simple waveforms
0..4 have 47 public bytes; Multi types 5/6 append five float weights; `0x73`
appends a byte. Native XML/typed-reader evidence names sine, rectangle,
triangle, sawtooth, random weights. Frequency free mode note value `-1` means
Hz; sync cycle length is note value times count in beats. Fade sync duration
is note value times count in beats. Unknown sync words/flags, type 6 behavior,
legacy Multi ripples and the final version flag are not replaced with guesses.

DBD `0x41`, glide `0x0b`, step and older source types require corpus/native
reader evidence before their fields can be declared decoded. An ID/name-only
mapping is not support.
