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

## Reader changes and executable limits

`EffectParameters::read(id, version, public)` exposes exact-length fixed fields
for Delay, Chorus, Flanger, Gainer, Phaser, Compressor, Inverter, Limiter,
Surround Panner, Distortion, Stereo Modeller, Lo-Fi, Skreamer, Rotator, Tape
Saturator, Transient Master, Solid G-EQ, Solid Bus Comp, Feedback Compressor,
and Reverb. Send Levels, Convolution and Filter/EQ have counted or conditional
layouts. Packed bytes remain bytes; Convolution block size and IR index remain
integers. Unassigned parameter names follow the reference specification.
A matching length proves field framing, not the complete physical or KSP law.
Unknown IDs and mismatched known layouts retain opaque public data in memory.

The Ladder reader now consumes the leading parameter separately from cutoff
and resonance, plus the byte between subtype IDs at version `0x92`. Previously
v2 shifted those fields or discarded modern records. Malformed occupied slots
now produce errors rather than silently disappearing from a rack.

`Instrument.source_parameters` retains version-pinned records at their original
instrument/group/bus/rack/slot locations, including bypassed FX, muted-group
sources, signed depths, target names and slots, shaper tables/breakpoint curves,
smoothing, retrigger, source payloads and unknown fields. Envelope timing records
expose three float words plus their raw byte, without asserting sync semantics.
Additional AHDSR bytes remain opaque. The IR's debug output hides opaque bytes.
These records are authored state; script writes continue through the existing
executable translation. Lowering does not execute the storage records.

Snapshot group modulation arrays now overlay base arrays before translation,
including the source records for muted groups. Empty saved arrays replace base
arrays; absent arrays leave base state. Group topology and unrelated children
remain unchanged. Corpus census covers manifest instruments/multis; this
snapshot behavior currently has an authored fixture, not a whole-snapshot census.

Executable gaps include unknown/newer FX and filter subtype laws; DBD, glide and
step sources; AHDSR/Flex stage sync and loop metadata; LFO type 6, legacy waveform
ripples and unknown sync words; release velocity and random bipolar execution;
unproven destination intensity/normalization laws and modulation of envelope
parameters. Retaining bytes is not execution or sound-parity certification.
The DSP agent should consume established fields and physical units, independently
establish remaining laws, and keep unknown fields out of audible execution.

## Reproducing the census

Build `cargo build -p sampler-kontakt --example fx_mod_survey` through
`/home/derpcat/.cache/kontakto-heavy`. Run the resulting executable through that
wrapper with positional arguments `items.tsv start limit cache-directory`.
`start` and `limit` address original manifest line numbers; only Kontakt rows
are read. Each call stops starting new items after 240 seconds, and writes
one atomic metadata-only TSV per completed item. Run another wrapper call for
the next shard; existing completed item files are skipped. Never set
`KONTAKTO_HEAVY_SLOTS`, or launch another local heavy job concurrently.

Merge with `python3 tools/fx-mod-survey.py cache-directory`. `COUNT` rows give
object occurrences and affected-file counts. `FIELD` rows give object ID,
version, field path, observed files, files with values differing from the most
frequent value, distinct signatures and that baseline. Float arrays and opaque
regions use lengths/fingerprints; these are variation evidence, not decoded
laws. Per-item cached `VALUE` rows retain scalar metadata frequencies.
Run `python3 tools/fx-mod-survey.py --check` for the aggregation check.

## Corpus validation table

Master structural census, 2026-10-08: **1937/1937 containers parsed**
(instruments, multis and snapshots). Source: the format-gaps agent's read-only
`~/.cache/kontakto-gpt-format-gaps/census/{fields,files,byte-profile,errors}.tsv`.
The following counts select its Kontakt `structure` rows, not NIS chunk IDs.
Records include compact snapshot state, muted groups and bypassed effects.
A file can appear in multiple version rows; affected-file counts are not additive.
These are **structural counts**, not yet full field-decoder or audible validation.
The master's reported field error is save settings (831 files/940 records),
which belongs to the gaps agent.

| Object | Version | Files | Records |
| --- | --- | ---: | ---: |
| `0x07` Envelope wrapper | `0x90` | 1,736 | 1,633,242 |
| `0x08` LFO | `0x71` | 702 | 1,684,800 |
| `0x08` LFO | `0x73` | 153 | 377,706 |
| `0x0c` External assignment | `0x100` | 812 | 5,962,119 |
| `0x0c` External assignment | `0x101` | 100 | 115,236 |
| `0x0c` External assignment | `0x102` | 667 | 1,256,463 |
| `0x0c` External assignment | `0x103` | 31 | 116,372 |
| `0x0c` External assignment | `0x104` | 122 | 218,554 |
| `0x0d` Internal assignment | `0x80` | 812 | 2,708,938 |
| `0x0d` Internal assignment | `0x81` | 924 | 986,810 |
| `0x10` Delay (legacy) | `0x51` | 705 | 5,619 |
| `0x11` Chorus (legacy) | `0x51` | 705 | 705 |
| `0x12` Flanger (legacy) | `0x51` | 3 | 3 |
| `0x13` Gainer | `0x50` | 296 | 602 |
| `0x14` Phaser (legacy) | `0x51` | 705 | 705 |
| `0x15` Reverb (legacy) | `0x50` | 702 | 2,808 |
| `0x16` Convolution | `0x70` | 1,251 | 2,236 |
| `0x17` Send Levels | `0x51` | 1,715 | 2,027 |
| `0x18` Filter | `0x92` | 1,228 | 719,272 |
| `0x18` Filter | `0x95` | 102 | 92,317 |
| `0x19` Compressor | `0x70` | 808 | 3,719 |
| `0x1a` Inverter | `0x60` | 832 | 191,503 |
| `0x1c` Limiter | `0x60` | 702 | 702 |
| `0x1d` Surround Panner | `0x80` | 810 | 339,428 |
| `0x1e` Distortion | `0x60` | 706 | 339,070 |
| `0x1f` Stereo Modeller | `0x70` | 1,265 | 26,135 |
| `0x20` Lo-Fi | `0x70` | 705 | 339,069 |
| `0x21` Skreamer | `0x50` | 104 | 286 |
| `0x22` Rotator | `0x50` | 3 | 3 |
| `0x25` FX wrapper | `0x50` | 1,736 | 2,072,739 |
| `0x3a` FX array | `0x12` | 1,583 | 29,379 |
| `0x3a` FX array | `0x13` | 153 | 3,857 |
| `0x3b` Internal array | `0x12` | 782 | 226,278 |
| `0x3b` Internal array | `0x13` | 52 | 5,675 |
| `0x3c` External array | `0x12` | 782 | 226,278 |
| `0x3c` External array | `0x13` | 52 | 5,675 |
| `0x3f` AHDSR | `0x11` | 1,736 | 1,626,358 |
| `0x40` Flex | `0x11` | 170 | 5,876 |
| `0x40` Flex | `0x12` | 6 | 1,008 |
| `0x42` Tape Saturator | `0x10` | 211 | 525 |
| `0x43` Transient Master | `0x10` | 103 | 103 |
| `0x44` Solid G-EQ | `0x10` | 103 | 2,287 |
| `0x44` Solid G-EQ | `0x11` | 12 | 12 |
| `0x45` Insert bus | `0x11` | 1,541 | 24,656 |
| `0x45` Insert bus | `0x12` | 195 | 4,016 |
| `0x46` Solid Bus Comp | `0x12` | 109 | 109 |
| `0x4c` Feedback Compressor | `0x10` | 103 | 103 |
| `0x4d` Jump | `0x10` | 702 | 2,808 |
| `0x56` DStortion | `0x10` | 2 | 2 |
| `0x59` Reverb | `0x10` | 374 | 377 |
| `0x5a` Replika | `0x12` | 72 | 74 |
| `0x5b` Phasis | `0x51` | 10 | 10 |
| `0x5c` Flair | `0x51` | 11 | 11 |
| `0x5d` Choral | `0x51` | 17 | 18 |
| `0x60` Supercharger | `0x51` | 8 | 8 |
| `0x63` Psyche Delay | `0x10` | 19 | 19 |
| `0x65` Unknown; retained opaque | `0x10` | 13 | 13 |
| `0x66` Unknown; retained opaque | `0x10` | 7 | 7 |
| `0x68` Unknown; retained opaque | `0x10` | 17 | 21 |
| `0x69` Unknown; retained opaque | `0x10` | 7 | 7 |
| `0x6a` Unknown; retained opaque | `0x10` | 9 | 9 |
| `0x72` Unknown; retained opaque | `0x1` | 5 | 5 |
| `0x74` Unknown; retained opaque | `0x1` | 2 | 2 |

No DBD (`0x41`) or glide (`0x0b`) source appeared in these structural rows.
No `0x09`/`0x0a` source appeared; step-source identity/layout remains unverified.
Absence does not establish support. New `0x65/66/68/69/6a/72/74` objects remain
unknown, with no names or normalization invented. Older missing FX types and
versions likewise need independent native or controlled-saved fixtures.

Bus `0x45` carries a UTF-16 name, float volume, float pan and signed output
index, followed by retained extension bytes. The master profile observes up to
36 public bytes for `0x12`, versus 28 for `0x11`; string length contributes to
these sizes, so the difference alone does not identify eight fixed new fields.
The reader now keeps every byte after output instead of silently discarding it.
