# Kontakt format gap map

Owner: `gpt-format-gaps`, branch `v2/gpt-format-gaps`. Baseline:
`origin/integrate/core-v2` at `fb456b41` (2026-10-08). This is a serialization
inventory and playback admission map, not a claim of complete Kontakt emulation.

## Evidence and interpretation

Compare five sources separately:

1. Upstream [ni-file](https://github.com/monomadic/ni-file): **unverified**.
   On 2026-10-08 GitHub returned 403 / 451 and a DMCA takedown notice. The
   inherited README estimates (v2 90%, v4.22+ 75%, v5–7 65%, no NKS monolith)
   are historical author estimates, not measurements or a current upstream diff.
2. Vendored `vendor/ni-file`: actual source, including local decoder extensions.
   A name in `KontaktObject` is **not** a decoded body. In particular many FX
   IDs only select a unit enum variant; `SaveSettings` only consumes the flag;
   `VoiceGroups` is incomplete despite having a type.
3. v1 importer: read-only `feat/decipher-readers-v1` at `4bffbb18`,
   `/home/derpcat/.t3/worktrees/KONTAKTO/decipher-readers-v1/src/import.rs`.
   Its vendored definitions do not by themselves establish importer use.
4. v2: `crates/sampler-kontakt`, both the borrowed source views and the real
   translator in `library.rs`, then `sampler-ir` / `sampler-core` lowering.
   Keeping a source value does not imply audio uses it.
5. RE specs: read-only `t3code-80fe786b/docs/DSP_FORMAT_SPECIFICATION.md`,
   especially “Kontakt binary grammar”, “NIS item and data-layer framing”,
   “Kontakt chunks and structured objects”, and “Effect parameter payloads”.
   These establish framing and selected layouts, not every unknown body's
   semantics. Source namespace IDs must not be conflated with KSP IDs.

The independent author's [hex templates at c6f309b](https://github.com/monomadic/hexfiend-templates/tree/c6f309bae04a03967b94f54d81dc2050f827a1e8)
were accessible. They support filename v2 framing and SaveSettings field widths;
unknown labels in those templates remain unknown here.

Statuses: **decoded+used** means affects playback or supplies its executable
source/assets; **decoded-unused** means readable but ignored by playback;
**raw/unknown** means bounded/preserved without established semantics;
**unread** means the active importer does not enter that region. Presence is
not activation. Unknown bytes use a **nonzero** baseline only; this is not a
Kontakt factory default. Numeric neutral baselines are stated in the census
source (gain/tune 1, high key/velocity 127, absent group/channel/keyswitch -1,
other scalar 0); they are not asserted to be every native factory default.

## Family ownership

| Family | Owner | Detailed field/version evidence |
|---|---|---|
| Scripts and persistence; snapshots 0x4f–0x51 | `gpt-decipher-persist` | [KONTAKT_SCRIPT_PERSISTENCE.md](KONTAKT_SCRIPT_PERSISTENCE.md) (confirm final family filename when integrating) |
| Program/group/zone, voice groups, loops, source and criteria | `gpt-format-objects` | [KONTAKT_OBJECTS.md](KONTAKT_OBJECTS.md) |
| FX wrappers/payloads, internal/external modulation, buses | `gpt-format-fxmod` | [KONTAKT_FX_MOD_FORMAT.md](KONTAKT_FX_MOD_FORMAT.md) |
| NKS versions, old XML envelopes, monoliths and sample framing | `gpt-format-legacy` | [KONTAKT_LEGACY_FORMAT.md](KONTAKT_LEGACY_FORMAT.md) (confirm final family filename when integrating) |
| Filename/resource metadata, save settings, quick browse, banks; master map | `gpt-format-gaps` | This document; [metadata reader](../../crates/sampler-kontakt/src/metadata.rs) |
| Library UI and resource use | `gpt-kontakt-ui` | [KONTAKT_UI_HEALTH.md](KONTAKT_UI_HEALTH.md) |
| Native parameter laws / DSP consumption | `gpt-decipher-dsp` | Coordinator's DSP evidence; format decoding alone is insufficient |

## Exhaustive discovered registries

[ID registry](KONTAKT_ID_REGISTRY.tsv) enumerates every dispatch slot from
Kontakt `0x00` through `0x64`, including holes `0x31`, `0x48`, `0x49`, `0x62`,
and every NISD/NIK4/RKTR item recognized by the local enum. IDs beyond this
range are unknown and must still be retained when observed.

[Field registry](KONTAKT_FIELD_REGISTRY.tsv) lists each public source/view
record field, its type, declared versions, exact source line and presence of
its definition in v1. This includes wrapper fields, private bytes, extension
bytes and container metadata, not just the convenient parameter structs.
It is mechanically reproducible with `python3 tools/kontakt-format/inventory.py`.
“Present in v1” is definition coverage, **not** playback use. Empty NIS property
files represent unread properties; the ID registry exposes these even when
there is no Rust field definition to list. Dynamic arrays, enums and unknown
regions retain their raw representation rather than assigning fictitious
fixed fields.

The registries are a complete inventory of **discovered source declarations
and IDs**, not proof that every proprietary field or future version is known.
Family evidence and measured byte profiles identify the remaining regions.

## Container and NIS layers

| Structure / known version | Vendored decoder | v1 importer | v2 playback path | RE establishes |
|---|---|---|---|---|
| NKS V1/V2/V42 | Header and compressed body; V1/V2 XML wrapper readers; monolith refused | Expands then assumes Kontakt chunks; XML not translated | Same chunk-only active import path at baseline | Header dispatch, zlib vs FastLZ; distinct XML/chunk grammar |
| NIS item header v1 | length, magic, flags, reserved, UUID, children/descriptors/trailer retained | Framed, then schema-selected | Borrowed framing plus active owned decoder | 40-byte item, 20-byte data layer; child descriptors independent of inferred ID |
| NISD Item 0x01 | Terminal raw properties | Framing | Framing | Terminal layer condition |
| Bank/Preset/BankContainer/PresetContainer 0x64–67 | Preset has factory marker/app/version; bank/container property stubs | Preset only used to find sound payload | Preset envelope used; bank properties unread | Versioned item inheritance; no complete bank payload spec |
| BinaryChunk 0x68 | Property stub | Unread | Raw/unknown | Framing only |
| Authorization 0x6a | License fields retained privately | Access path only | Access path only; never emitted by census | Framing only; no claimed playback semantics |
| SoundInfo 0x6c | Property stub / sound-info identifiers | Unread | Unread | Framing only |
| PresetChunk 0x6d v1 | version/checksum/version/u64 size/bytes | Executable Kontakt chunk source | decoded+used, borrowed view also retains checksum | Raw chunk extent; no claim of checksum verification |
| ExternalFileReference 0x6e | Stub (`value` comment) | Unread | Unread | No established body semantics |
| Resources 0x6f | Stub; sample/picture/binary counts named in comments | Unread | Unread | No complete resource-item grammar |
| AudioSample/InternalResource/Picture 0x70–72 | Property stubs | Unread | Unread | Framing only; loose/NKR asset path is separate |
| Subtree 0x73 v1 | compression marker, sizes, FastLZ/plain bytes | decoded+used | decoded+used | Compression framing and exact decoded length |
| Encryption 0x74 v1 | encrypted marker, subtree, caller-supplied library key | decoded+used | decoded+used | Marker separate from subtree; opaque without access |
| AppSpecific 0x75 v1 | authoring app/version, nested subtree | decoded+used | decoded+used | Nested wrapper framing |
| RepositoryRoot 0x76 | repository version/magic/type/path segments | Schema dispatch | Schema dispatch | Framing; domain bytes reversed on wire |
| Automation/ControllerAssignments 0x78–79 | Property stubs | Unread | Unread | No body semantics established |
| Module/ModuleBank 0x7a–7b | Property stubs | Unread | Unread | No body semantics established |
| NIK4 SoundPreset 0x03 / SoundHeader 0x04 | inherited preset / BPatchHeaderV42 | Payload/header decode | Payload used; most descriptive header fields unused | Selected header widths / strings / reserved words |
| RKTR 0x01–06 | Reaktor item names | Outside Kontakt import | Outside Kontakt scope | No Kontakt playback claim |

All NIS unknown properties and descriptor fields are retained at source level.
Their default semantics cannot be inferred from a property version or UUID.
No authorization, access fields, decrypted scripts/presets or samples are
written by the census; only aggregate counts and ordinary library paths.

## Kontakt chunk coverage by family

| IDs | Fields decoded by vendored ni-file | v1 | v2 actually used / gaps |
|---|---|---|---|
| 0x03, 0x29, 0x36, 0x37 | Bank volume/tune/tempo/name; container name/volume/pan; program list count (program-number word discarded); 64-bit slot occupancy | One program per slot required; names exposed; bank scalar state not applied | Programs decoded and flattened; bank/container scalar state unused; new borrowed lists preserve program numbers and sparse slots |
| 0x06 | Optional source/name/link, editor flags, bypass, password hash, persistence strings | Source and saved persistence; linked file reload | Source/state used; linked resources now reloaded; editor/password are decoded-unused; family owns state grammar |
| 0x04 / 0x33 | Group name, gain/pan/tune, key tracking, reverse, release, monophony/counter, MIDI channel, voice index, amp split, mute/solo, interpolation, source/criteria; private rack | Many used; source metadata not equivalent to native source DSP | Gain/pan/tune, reverse, release, mute, routing/mods used; MIDI filter/source modes have explicit caveats; private unknown records remain raw |
| 0x05 / 0x39 | Loop mode/start/length/count/alternating/tune/crossfade; mask; source reader preserves holes | Loop playback/alternating support | Loop region used; unsupported modes surfaced; compare loop tune/count/crossfade to lowering in objects doc |
| 0x28 | Full common public prefix through credits/categories; later resource/wallpaper references set `None`; private bytes raw | Common params, gain, ranges and metadata; verify individual uses in objects doc | Host volume used; many program scalars/metadata still unused; later fields raw/unknown |
| 0x32 | Vendored voice-group decoder incomplete and unsafe on unsupported versions | Own decoder: 128-bit mask and one-based assignment | Own decoder used; source wrapper completeness belongs to objects agent |
| 0x2c / 0x34 | Zone trim/ranges/fades/root/gain/pan/tune/sample reference and metadata/reserved fields | Mapping/loops used | Playback uses mapping prefix, sample identity and trim; additional sample metadata and private bytes raw/unknown |
| 0x0e, 0x0f, 0x38 | Source identity, bounded wavetable v0x106 and criteria rows; remaining source body partly raw | Source identity/wavetable metadata; criteria used | Source modes can fall back to sampler with caveat; no claim that wavetable metadata implements wavetable DSP |
| 0x25, 0x3a, 0x45 | FX wrapper type/bypass/wet/dry; slot flags/count; bus metadata | Group/instrument/send/main/bus FX paths | Decoder coverage ≠ effect/kernel coverage; bypassed values retained; malformed-wrapper reporting belongs to fxmod |
| 0x0c, 0x0d, 0x3b, 0x3c | Physical slots, source/target/intensity/lag/invert/name/shaper, unknown flags and sentinels | Modulation and curves used selectively | Supported routes used, module/source gaps and some unknown flags report unsupported; exact non-default ranking in fxmod doc |
| 0x47 | Upstream-style vendored stub only reads structured flag | Unread | New bounded source decoder; paths/scalar/three flags decoded-unused, meanings unknown |
| 0x4e | i32 unknown after structured header | Unread | New bounded source decoder; no audio consequence established |
| 0x3d / 0x4b | Legacy/modern special/sample/other tables; timestamps converted to Date; v2 u32 sample word discarded in convenience decoder; v3 prefix/suffix skipped | Samples used; NKR and IR names used; legacy IR table dropped | Samples and IRs used; resource names discovered heuristically; new borrowed v2/v3 table preserves metadata |
| 0x4f–51 | Snapshot/group-snapshot fields, metadata partial | Overlay native/script state with identity guard | Overlay supported; full family state/profile evidence owned by persistence |
| All remaining IDs in ID registry | Some named only, some private raw objects or selected payload helpers | Generally unread outside decoded paths | raw/unknown or unread; do not silently label enum names “decoded” |

## Non-family work completed here

### Filename table and resource resolution

`FileTable::parse` reads modern v2 and v3 without normalizing filename
segments. V2 keeps the three namespaces, full u64 timestamps, one u32 record
per sample and trailing bytes. V3 keeps a single indexed namespace, each
8-byte prefix and 20-byte suffix, and trailing bytes. No guessed v3 category,
checksum or timestamp meaning is assigned.

Path segment tags: 1 drive, 2 directory, 3 parent, 4 file, 5 opaque text
segment, 6 special location, 8 library/archive node, 9 multi-file node,
11 snapshot library anchor. Text-bearing segments preserve UTF-16 code units;
non-text anchors have no payload. New views expose the tag rather than
flattening anchors into arbitrary host absolute paths. Unknown tags and
versions fail explicitly. Existing `FNTableRecord` already preserves v2;
this change adds borrowed inspection and v3 retention rather than replacing it.

V1 linked-script reload was absent from v2. V2 now resolves the authored link's
basename under the instrument's `Resources/scripts` (loose or NKR), reloads
nonempty external source ahead of saved source, keeps saved source when the
link is absent/empty, and reports unresolved links when no saved source exists.
Bypassed slots remain inactive. UTF-8 BOM, UTF-16LE BOM/heuristic and v1's
legacy Latin-1 fallback are supported; Windows-1252 control-range punctuation
remains a stated limitation. Resource reads are bounded at 32 MiB, matching
`ResourceContainer`'s existing limit. No script/sample data is extracted.

NKR directory/resource headers and NICNT embedded FileContainer resources
are already read by `nkr::Archive` and `ResourceContainer`. Metadata and
access discovery are distinct: NICNT access fields are private, never census
outputs. Authored resource-container references, NIS resource items and
heuristic fallback search still need a unified source-to-resource binding;
reading a nearby NKR does not establish that it is the authored one.

### Save settings and quick browse

New `SaveSettings::parse` validates unstructured v0x10, retains two native
filename encodings, an i32 and three exact boolean bytes, then preserves
extensions. Segment filenames and negative marker + UTF-16 reference
encodings are preserved separately. Native semantic names for the i32/flags
are **unestablished**; these values must not change playback based on guesses.
The author's SaveSettings template supports the field widths, while corpus
acceptance determines which encodings actually occur here.

`QuickBrowse::parse` reads v1's i32 and retains any extension/private/child
state. Its meaning and playback relevance are unknown. Zero is a census
baseline, not evidence of a native setting default.

### Banks and old XML

New bank views retain master gain/tune/tempo/name and extensions; lists retain
signed program numbers and original 64-bit slot-mask identities. This is a
metadata reader, not MIDI program-switch playback. V1 rejected slots with
multiple programs; baseline v2 flattens programs and loses program numbers.
A real `.nkb` needs program selection plus bank/container/master scalar
admission, output routing and bank scripts. The corpus contains no `.nkb`, so
zero observations do not validate that implementation.

Neither v1 nor baseline v2 imports Kontakt XML into playback: both expand NKS
and then expect binary chunks. Vendored KontaktV1/V2 wrappers retain XML text
but do not interpret its instrument schema; some error paths panic. There
are no installed old XML presets in this corpus. XML envelope/parser safety
belongs to legacy; XML-to-IR schema reconstruction remains open pending real
fixtures. It would be misleading to invent a schema from zero examples.

## Reproducing the census

`format_survey <newline-separated preset paths> <output directory>` is the
sampler-kontakt example. It counts each file once per structure/version/field,
even when thousands of records repeat; a later non-neutral record correctly
raises that file's non-neutral count. `fields.tsv` includes occurrence counts;
`byte-profile.tsv` counts nonzero bytes at each observed public/private offset
(up to 4096, remainder separately aggregated). Offset lanes in variable-size
regions are **not** semantic fields and must be interpreted with source layout.
Scripts/password/access properties have no byte-level profile.

All nested group/zone/loop/program/slot/FX/mod arrays are traversed, including
bypassed modules and group-private FX racks. Unknown bodies remain aggregate
regions. Failures are counted, not silently turned into absent structures.
Only aggregate metadata is written; decrypted buffers stay in memory.

Run builds/tests and corpus work through `kontakto-heavy`, one heavy job at a
time. The shared installed-tree inventory has 781 NKI, 53 NKM, 1103 NKSN,
270 NKX, 12 NKR and 9 NICNT; recovery backups under Pacific Ensemble Strings
are excluded consistently with the family corpus inventory. The 1,494-line
harness manifest also contains UVI, so it is not the denominator for Kontakt
structures. Snapshot-only counts must not be mistaken for playable presets.

## Measured impact ranking

Pending the full field census and final family reports. The checked-in census
will state denominators, decoder failures and each non-neutral baseline;
no estimated percentages or presence-as-activation claims are used.

## Tests and integration handoff

Synthetic tests exercise complete/truncated filename v2 records, v3 metadata,
unknown versions, invalid settings flags, preserved tails, sparse slot 63,
program numbers and negative list counts. Resource tests cover linked script
reload, case-insensitive Windows path basenames, saved encodings, absent/empty
links and no host-path following. Corpus checks establish observed layouts
only; native semantic/sonic claims require matched Kontakt measurements.

Do not merge family branches by copying their current source files. Integrate
their commits, regenerate the declaration registry at the combined head,
and append their per-field evidence to the master measurements. The master
remains owned by `gpt-format-gaps`; families own their detailed payload specs.
