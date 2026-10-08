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
source (gain/tune 1, high key/velocity 127, absent channel/keyswitch and sample-start range -1, absent voice-group 0,
other scalar 0); they are not asserted to be every native factory default.

## Family ownership

| Family | Owner | Detailed field/version evidence |
|---|---|---|
| Scripts and persistence; snapshots 0x4f–0x51 | `gpt-decipher-persist` | [KSP_PERSISTENCE_FORMAT.md](KSP_PERSISTENCE_FORMAT.md) |
| Program/group/zone, voice groups, loops, source and criteria | `gpt-format-objects` | [KONTAKT_OBJECTS.md](KONTAKT_OBJECTS.md) |
| FX wrappers/payloads, internal/external modulation, buses | `gpt-format-fxmod` | [KONTAKT_FX_MOD_FORMAT.md](KONTAKT_FX_MOD_FORMAT.md) |
| NKS versions, old XML envelopes, monoliths and sample framing | `gpt-format-legacy` | [KONTAKT_LEGACY_FORMATS.md](KONTAKT_LEGACY_FORMATS.md) |
| Filename/resource metadata, save settings, quick browse, banks; master map | `gpt-format-gaps` | This document; [metadata reader](../../crates/sampler-kontakt/src/metadata.rs) |
| Library UI and resource use | `gpt-kontakt-ui` | [KONTAKT_UI_HEALTH.md](KONTAKT_UI_HEALTH.md) |
| Native parameter laws / DSP consumption | `gpt-decipher-dsp` | Coordinator's DSP evidence; format decoding alone is insufficient |

## Exhaustive discovered registries

[ID registry](KONTAKT_ID_REGISTRY.tsv) enumerates every dispatch slot from
Kontakt `0x00` through `0x64`, including holes `0x31`, `0x48`, `0x49`, `0x62`,
and every NISD/NIK4/RKTR item recognized by the local enum. The census also
found Kontakt IDs `0x65`, `0x66`, `0x68`, `0x69`, `0x6a`, `0x72`, `0x74`;
these are registered as observed unknowns owned by FX/mod until identified.
An apparent structured version is a candidate until its schema is established;
`0x48` yields `0xff00` from a framing attempt, not a certified version.

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

[Declaration coverage](kontakt-census/declaration-coverage.tsv) joins every
registered field to source evidence, v1 importer evidence where established,
v2 status, family ownership, RE evidence limits and measured file/non-neutral
counts by version. `unmeasured; not zero` means the declaration has no matching
semantic census row. Detailed named FX/mod/source/persistence fields remain
in their family evidence; unknown tails remain region/byte observations.

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
| 0x28 | Full common public prefix through credits/categories; later resource/wallpaper references set `None`; private bytes raw | Common params, gain, ranges and metadata; verify individual uses in objects doc | Volume/pan/tune/transpose used; program ranges/metadata unused; new borrowed resource view retains four references and later bytes |
| 0x32 | Vendored voice-group decoder incomplete and unsafe on unsupported versions | Own decoder: 128-bit mask and one-based assignment | Own decoder used; source wrapper completeness belongs to objects agent |
| 0x2c / 0x34 | Zone trim/ranges/fades/root/gain/pan/tune/sample reference and metadata/reserved fields | Mapping/loops used | Playback uses mapping prefix, sample identity and trim; additional sample metadata and private bytes raw/unknown |
| 0x0e, 0x0f, 0x38 | Source identity, bounded wavetable v0x106 and criteria rows; remaining source body partly raw | Source identity/wavetable metadata; criteria used | Source modes can fall back to sampler with caveat; no claim that wavetable metadata implements wavetable DSP |
| 0x25, 0x3a, 0x45 | FX wrapper type/bypass/wet/dry; slot flags/count; bus metadata | Group/instrument/send/main/bus FX paths | Decoder coverage ≠ effect/kernel coverage; bypassed values retained; malformed-wrapper reporting belongs to fxmod |
| 0x0c, 0x0d, 0x3b, 0x3c | Physical slots, source/target/intensity/lag/invert/name/shaper, unknown flags and sentinels | Modulation and curves used selectively | Supported routes used, module/source gaps and some unknown flags report unsupported; exact non-default ranking in fxmod doc |
| 0x47 | Upstream-style vendored stub only reads structured flag | Unread | New bounded source decoder; two filename references/scalar/three flags retained, meanings unknown |
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

`ProgramResources::parse` reads four translated BFN references after the
common public prefix/categories: resource container, snapshot factory
subdirectory, full preset path and wallpaper. The independent
`ProgramVA8PublicParams.tcl` establishes their order. It explicitly admits
six corpus versions `0xa8`, `0xab`, `0xae`, `0xb1`, `0xb3`, `0xb5` and
preserves later bytes. The convenience reader previously returned `None`
for container/wallpaper. These views are decoded-unused by playback; target
namespace and authored container binding remain open.

NKR directory/resource headers and NICNT embedded FileContainer resources
are already read by `nkr::Archive` and `ResourceContainer`. Metadata and
access discovery are distinct: NICNT access fields are private, never census
outputs. Authored resource-container references, NIS resource items and
heuristic fallback search still need a unified source-to-resource binding;
reading a nearby NKR does not establish that it is the authored one.

### Save settings and quick browse

`SaveSettings::parse` validates unstructured v0x10. Its public prefix is
**15 bytes**: `u32 BFNTrns`, `i32 BFNOrig`, `i32 unknown`, then three exact
boolean bytes. These are filename **references**, not inline BFileName records
or a negative marker followed by a string. The first synthetic implementation
made that incorrect assumption; the census rejected it in all 831 observed
files, so it was replaced before claiming corpus support. Extensions remain
bounded and preserved. References use an all-ones/negative-one absent census
baseline, without assigning their target namespace.

Evidence: independent `SaveSettings.tcl` field widths; native reader
`0x140d052d0` and writer `0x140d13870` in the read-only engine inventory,
which call translated/original BFN helpers before the scalar and three flags;
and the measured public region, exactly 15 bytes in all 940 records in 831
files. Native semantic names for the scalar/flags remain **unestablished**;
they must not affect playback based on guesses.

`QuickBrowse::parse` reads v1's i32 and retains any extension/private/child
state. Its meaning and playback relevance are unknown. Zero is a census
baseline, not evidence of a native setting default.

### Banks and old XML

New bank views retain master gain/tune/tempo/name and extensions, including
observed v0x76 in 50 NKM files and v0x73 in three NKM files; lists retain
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
time. Reproductions should shard inputs into 100 files or fewer, use a
separate output directory/completion marker per shard, and release the wrapper
slot before the next shard. Targeted updates use `--metadata-only`; they do
not retraverse the group/zone/DSP regions already measured. Join disjoint
shards using `python3 tools/kontakt-format/report.py <baseline census>
--shards <shard directory>`; never add overlapping surveys. The installed-tree inventory has 781 NKI, 53 NKM, 1103 NKSN,
270 NKX, 12 NKR and 9 NICNT; recovery backups under Pacific Ensemble Strings
are excluded consistently with the family corpus inventory. The 1,494-line
harness manifest also contains UVI, so it is not the denominator for Kontakt
structures. Snapshot-only counts must not be mistaken for playable presets.

## Measured impact ranking

The baseline census completed with **1,937/1,937 parsed files**: 781 NKI,
53 NKM and 1,103 NKSN. It observed 641 structure/version/field rows and
34,382 byte-offset rows. This checks framing/decoder coverage, not sound,
sample resolution, script execution or activation. Targeted corrected
metadata/resource measurements are recorded separately below.

The checked-in [field counts](kontakt-census/fields.tsv),
[annotated impact](kontakt-census/field-impact.tsv),
[unknown byte lanes](kontakt-census/byte-profile.tsv) and
[decoder errors](kontakt-census/errors.tsv) are numeric aggregate evidence.
They retain per-version denominators; do not add file counts across versions
that may coexist in one file. A `structure` row counts presence, not a
non-default setting. NIK4 header magic is labelled `BPatchHeaderV42`, not
falsely called version 2141753362.

| Gap / field family | Files present / non-neutral evidence | Status and likely consequence | Owner |
|---|---|---|---|
| NIS ControllerAssignments `0x79` | 1,937 present; properties unmeasured | unread; host assignment/automation admission unknown, not proof that scripts or all MIDI CC are broken | gap map; KSP/host runtime for application |
| Modern filename metadata `0x4b` v2 | 1,583; tail nonzero in 1,583; sample u32 nonzero in 775 of 782 sample-bearing files | raw/unknown tail and sample word; identity/cache/search consequences need semantic evidence | `gpt-format-gaps` |
| AHDSR `0x3f` v0x11 | 1,736 present, including snapshots | decoded core stages; remaining sync/tail fields raw, possible envelope timing differences | `gpt-format-fxmod`, then DSP |
| Group-private/source data | v0x95: 782/782 nonzero private; v0x96: 52/52 | partially decoded; machine/source controls, private masks and policies can change rendering; region count is not the number of active source modes | `gpt-format-objects`, then DSP |
| Program-private policy and resource extension | six versions: 3, 8, 729, 42, 31, 21 files (834 total); every private region nonzero | raw/unknown; controller/voice/HQ policies need field-level evidence | objects owns private policy; gaps owns resource references |
| Saved native compact source snapshot | v2: 801/801 nonzero source; v4: 101/101, eight-byte trailing state nonzero in 101 | partial overlay; unassigned source/selection state can change preset sound | persistence, objects and FX/mod |
| Snapshot-to-instrument binding | 1,103 snapshots; 1,003 unique metadata candidates; 100 unresolved, 0 ambiguous | unresolved Una Corda identity must remain explicit; a guessed base can receive the wrong state | persistence/core loader |
| Linked script filename | 833 of 834 instrument/multi files nonempty | decoded+used after this port; presence does not prove an external file exists or reload changes sound | `gpt-format-gaps`; UI owns shared NICNT discovery |
| Save settings `0x47` v0x10 | 831 files / 940 records; all public regions nonzero | newly decoded references/flags; meanings unknown, no established audio consequence | `gpt-format-gaps` |
| Amp split point | group v0x95: 782/782 nonzero; v0x96: 52/52 | decoded-unused at baseline; moving inserts across the amplitude stage can change dynamics/order | FX/mod and DSP |
| Program DFD preload override | all 834 program-bearing files nonzero against zero baseline | decoded-unused; streaming policy/startup cost impact | objects/streaming owner |
| Release counter | group v0x95: 97/782 nonzero; v0x96: 0/52 | decoded, source law not fully modeled; release age/dynamics need native comparison | objects/DSP |
| Banks / containers / program numbers | 53 NKM; no NKB; bank gain/tune unit in all 53; tempo nonzero in all 53 | decoded-unused scalar state and preserved lists; MIDI program selection unmodeled | `gpt-format-gaps`, then core |
| Quick browse `0x4e` v1 | 834 files; 0 nonzero known integers | raw/unknown meaning, low measured non-neutral impact | `gpt-format-gaps` |
| Counted/tuned loops | 787 loop-bearing files; 0 non-neutral counts/tunes; 733 nonzero crossfades, 3 alternating | count/tune unsupported with zero local activation evidence; crossfade/alternating used | objects/core |
| Old XML / standalone NKS / NKB | 0 installed fixtures | unvalidated; legacy fixes envelope safety, XML-to-IR and true bank playback remain open | legacy; gaps/core for banks |

The baseline also observed full zone `0x2c` v0x9c in 52 files / 104,760
records, in addition to v0x98/99/9a. These came from zone-list traversal,
not compact snapshot inference. Objects corrected its family table and extended borrowed admission to this
version with common-prefix/truncation checks; its complete 834-file survey
is underway after its parser checks and broader no-run passed. Its pushed
implementation is `c8e2fbce`, with the doc correction in `a1f1ff45`.
No library identity is inferred from the count.

The unknown-offset profile has a 4,096-byte ceiling per region. Bytes past
that ceiling have aggregate nonzero counts only. Variable-size offsets are
not semantic fields. Family surveys supply named fields, finer versions,
missing-source modes and exact payload laws; a full parse does not mean every
proprietary field is deciphered.

Snapshot candidate counts: ANALOG STRINGS 701, Conflux 201, Morphology Evolved
101. The remaining 100 are in Una Corda Library and fail the metadata-only
match. These are **candidate identity** counts, not validated overlays;
installed names can differ from saved template identity. The private cache
contains per-path bindings; the checked-in report contains aggregate totals.

FX/mod handoff: `v2/gpt-format-fxmod` commits `775d0f84`, `a490f7be` were pushed
after package no-run validation. They correct signed target flag 0x02, Ladder
offsets/v0x92 byte, malformed occupied-slot handling, 20 fixed FX layouts and
counted convolution/send/filter fields; retain authored FX/mod IR including
bypassed/muted state; and overlay snapshot modulation. Unit checks passed;
real-library and named-field census evidence is still pending. The persistence
family's initial 3,938-path census includes recovery files and has a different
denominator from this 1,937-path census; its entry counts must stay separate. New IDs above
0x64 stay opaque. Remaining laws include newer FX/filter controls, envelope
sync/loop state, unknown LFO sync/type 6, unsupported destinations and external
source laws. DBD/glide are absent from this baseline census, not certified.

### Corrected metadata acceptance

The targeted metadata traversal accepted all 1,937 presets/snapshots. Every
observed SaveSettings record now parses: **831 files / 940 records**, with
no extension bytes and no remaining SaveSettings decoder failures.
Non-neutral counts: translated reference 831 (baseline all ones), original
reference 0 (baseline -1), scalar 0 (baseline 0), flags 3 / 399 / 831 (baseline
false). These observed flags have no assigned native meanings.

## Tests and integration handoff

Synthetic tests exercise complete/truncated filename v2 records, v3 metadata,
unknown versions, invalid settings flags, preserved tails, all six program
resource-reference versions with variable metadata and every truncated prefix, sparse slot 63,
program numbers and negative list counts. Resource tests cover linked script
reload, case-insensitive Windows path basenames, saved encodings, absent/empty
links and no host-path following. Corpus checks establish observed layouts
only; native semantic/sonic claims require matched Kontakt measurements.

Do not merge family branches by copying their current source files. Integrate
their commits, regenerate the declaration registry at the combined head,
and append their per-field evidence to the master measurements. The master
remains owned by `gpt-format-gaps`; families own their detailed payload specs.
