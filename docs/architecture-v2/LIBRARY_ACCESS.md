# Library access in v2

Branch: `v2/library-access`. Access to installed protected content is behind
`library-access`; disabling it retains APIs that return access-disabled errors.
Library access state, decoded programs, pictures and sample data stay in memory.
UVI reader values come from the user's hash-verified official installation;
Kontakt access values come from the owning library's NICNT.

## Picture resolver API

`sampler_kontakt::ResourceContainer` indexes NICNT, NKR and NKX resources:

```rust
let mut container = sampler_kontakt::ResourceContainer::open(path)?;
let names = container.names();
let wallpaper: Option<Vec<u8>> = container.read(".LibBrowser.png")?;
```

`read` returns `None` for absent names and an error for malformed containers,
unsupported protection or missing local access data. Reads are bounded to
32 MiB per member. Names are case insensitive; NICNT's `|` separators also
accept `/`. NKR/NKX encrypted members use the same access layer as samples.
No extracted resource files or key caches are created.

The installed test reads the wallpaper in `Afflatus Chapter II Brass.nicnt`
and validates its complete PNG framing. Authored tests cover NKR versions
0x110/0x111, invalid container ranges/markers and the disabled-feature API.
Implementation: `fed97989`.
An installed Solo `Pyramid v1.0.6.nkr` test also validates an encrypted PNG
buffer and confirms that reading the same member without local access fails.
The older Areia NKR also has plaintext PNGs bearing a library-key hint. Only
complete PNG framing with intact chunk CRCs overrides that hint; explicitly
encrypted headers still require local access. All 2,220 pictures in the 12
installed NKR containers return recognizable image buffers.
Plaintext picture-layout companions are recognized only as numeric/boolean
property lists with animation metadata, at most 64 KiB and 32 unique fields.
Arbitrary text and KSP scripts do not override protection. The installed survey
also returns all 2,203 picture-layout companions as valid UTF-8 buffers,
including all 127 from the older Areia NKR.

## UVI programs

`sampler_uvi::Bank::open`, `programs`, `program`, `resource` and
`decode_resource` provide access without IR translation. `load_program` and
`load_program_with_options` assemble playable plans; the options API supports
restricting the loaded key range. `sampler-native render-uvi` mirrors
`render-kontakt`; `sampler-native census ROOT ...` reports JSON status lines.

Program XML is bounded to 32 MiB and one million nodes, with DTDs disabled.
This admits the installed Augmented Orchestra programs with more than
200,000 nodes. Explicit `$Bank.ufs/` resources resolve within their named bank.
XML bounds: `f24c85c6`; resource paths: `2b9a8b3d`; memory-only access: `2a6ba617`.
Protected and clear XML share the parser and byte bounds: `9309bf5a`.
Missing lossless audio references can resolve to a unique member with the same
directory and sample stem; exact paths take priority and ambiguity is rejected
(`c5b0bf8a`). This handles the installed WAV reference backed by a FLAC member.

Full installed UVI census on 2026-10-06: 26/26 banks and 660/660 programs open.
Programs decoding every referenced sample improved from 40/660 to 660/660;
banks with every program decoding improved from 25/26 to 26/26. All 620 XML
admission failures and the subsequently exposed audio-format reference failure
are resolved. The census stores status only, without program or sample bytes.

Protected `render-uvi` checks produced finite, nonzero audio for Augmented
Orchestra's `V Strings Bartok` (note 60) and `BSS Phased Flatterzunge` (note 35).
`render-kontakt` also rendered `Una Corda Pure` and `Conflux` (note 60;
Conflux with `--no-scripts`). Each check rendered
158,400 stereo frames at 48 kHz. The opt-in UVI integration test additionally
renders directly into RAM. Rendered note performances are verification outputs;
no original programs, pictures, samples or access state are extracted.

Access/decode census success does not imply complete frontend semantics:
this branch reports unsupported Lua processors and modulation/effect mappings.
For example, VWinds `Clarinet A` has saved zero layer gains that need its Lua
initialization; its samples decode but its current translated render is silent.
Those mappings remain with the IR frontend owner.

Kontakt's `Content/...` sample references can resolve from its local player
installation, including Wine's standard Native Instruments and VST3 folders.
Discovery is bounded and cached; `KONTRA_KONTAKT_CONTENT` overrides it with a
path list of `Content` directories. Lookups cannot ascend or leave those roots.
Multiple player copies must contain byte-identical assets (comparison bounded
to 32 MiB). The installed Conflux multi census improved from 20/50 to 50/50
decoding by reading the actual Chords/Phrases tool WAVs, without synthesizing
or bundling replacements. `Samples` keeps its existing public API.

Full installed Kontakt census on 2026-10-06: all 11 libraries and all 835
programs open. Programs decoding every referenced sample improved from
803/835 to 833/835; libraries with every program decoding improved from
9/11 to 10/11. NKI instruments remain at 781/781 opening and 779/781 decoding.
NKM multis remain at 54/54 opening and improved from 24/54 to 54/54 decoding.
The corrected census groups Morphology within the selected library root,
instead of finding an unrelated `Samples` directory above that root.

| Library | Programs opening | Programs decoding |
| --- | ---: | ---: |
| ANALOG STRINGS | 1/1 | 1/1 |
| Afflatus Chapter II Brass | 348/348 | 348/348 |
| Areia | 155/155 | 153/155 |
| Audio Imperia CHORUS | 42/42 | 42/42 |
| Audio Imperia Dolce | 77/77 | 77/77 |
| Conflux | 51/51 | 51/51 |
| Morphology Evolved | 1/1 | 1/1 |
| Pacific Ensemble Strings | 50/50 | 50/50 |
| Performance Samples Vista | 7/7 | 7/7 |
| Solo | 100/100 | 100/100 |
| Una Corda | 3/3 | 3/3 |

The two remaining failures are Areia's legacy `01 16 Violins - Legato` and
`05 16Vlns+10Vls 8va - Legato`. Both reference the same member in `Areia_0.nkx`:
`Samples/Areia_16VlnsLgtSstndVFMDyn3RR1_75_87_12.ncw`. Its first four blocks
parse, then decoding fails with `invalid NCW block signature at block 4,
channel 0`. The payload fits its archive range; no alternate member was found
in the installed Areia archives. Whether its tail is damaged or uses another
encoding remains unresolved; an original alternate copy is needed for
comparison. No missing audio is synthesized or silently discarded.
An additional RAM-only experiment tested every phase of all eight installed
library access streams against three failing block headers, both directly and
as a second cipher layer. It found no candidate. Neither stored nor decoded
tails contain standard or reversed NCW block signatures.
The failing member's header form, key hint and all currently ignored header
fields also match 6,049 NCW peers in the same archive; they do not identify a
different container profile. A bounded metadata search of mounted storage
found no alternate Areia bank or installer (14,643 directories and 234,516
files checked; the Projects root reached its directory budget). These checks
do not establish that the source audio is corrupt.
A subsequent full Areia sample audit checked all 241,820 NCW members across
all 40 NKX archives, continuing past errors. It decoded 241,802 members;
all 18 failures are in `Areia_0.nkx`: the NCW tail above and 17 members with
invalid NKX member signatures. The other 39 archives decode completely.
The program census counts remain unchanged because it records each program's
first failure. The audit retained only counts/reasons; its log, temporary
probe and worktree scratch build were deleted after recording the verdict.
The 17 invalid headers are consecutive in physical file order, immediately
after the member with the failing NCW tail. Their directory-entry kind matches
all 6,050 valid member headers in that archive. The signature failures therefore
are not explained by a different directory-entry kind. No aligned 1/2/4/8 MiB
window fits the observed valid/invalid boundaries; a damaged download chunk is
still a hypothesis, not an established cause.
A broader bounded backup search also matched full paths, so generic installer
filenames inside Areia folders were included. It checked 39,273 directories
and 466,071 files, including user caches and Wine user data, without finding
an alternate source. The Projects root completed; Gaming and local application
data reached their directory budgets. Installed library roots, repositories
and build caches were excluded. Search dumps/probes were deleted after the
small summary was recorded.

Installed container coverage is 781 NKI and 54 NKM NIS v1 files, 270 NKX
archives (56 version 0x110; 214 version 0x111), and 12 NKR archives (two version
0x110; ten version 0x111). No legacy NKS instrument container was found in
this installed census. Legacy NKS v1/v2 embedded monolith support remains
unimplemented in the vendor reader.

## Verification on the shared machine

Run every Cargo command and real-library census/render with
`/home/derpcat/.cache/kontakto-heavy COMMAND ...`.
The shared helper allows three jobs and waits for at least 10 GiB available RAM.
The helper now selects a per-worktree target directory automatically, with
sccache from `~/.cargo/config.toml`; do not set `CARGO_TARGET_DIR` or
`RUSTC_WRAPPER` yourself. The parent retains the slot guard and
closes its descriptor in the command's child process, so persistent daemons
such as sccache cannot inherit a slot after the job finishes.

The final access/resource changes pass locked offline tests and Clippy with
`--all-targets -- -D warnings` for `sampler-kontakt`, `sampler-uvi`,
`sampler-native`, `sampler-ir` and `sampler-core`. The three access/native crates
also pass tests and strict Clippy with `--no-default-features`; installed
protected tests skip in that configuration. Tests use the optimized `ci`
profile and one test thread on this machine.
After recording each scan/experiment verdict, remove its dumps, logs, rendered
verification files and scratch builds. Retain only small status summaries;
never persist original decrypted content or access material.
