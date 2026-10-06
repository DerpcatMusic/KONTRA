# Library access in v2

Branch: `v2/w0-native-provider` (recovered from `v2/linux-native-library-openers`). Access to installed protected content is behind
`library-access`; disabling it retains APIs that return access-disabled errors.
Library access state, decoded programs, pictures and sample data stay in memory.
Kontakt access values come from the owning library's NICNT. UVI UFS v3 and
PasswordV2 namespaces are decoded natively; protected member keys are recovered
from the bank's own encrypted content and verified before use. No Workstation
executable, Wine installation or reader-path setting is needed.

## Native provider validation

The recovered implementation passes 103 UVI area tests and 75 Kontakt library
tests, with 44 and 4 existing ignored tests respectively. Root library test
compilation passes. Both format crates and their tests compile for Windows GNU;
CI also executes the native UVI bank and Kontakt access fixtures on Windows.
Hosted Windows execution is still pending at this checkpoint.

The Linux installed-bank witness opens Augmented Orchestra with an isolated home,
no Wine/Windows search paths and a deliberately nonexistent reader override. It
loads 72 zones and renders 4,800 finite frames with 9,587 audible channel samples
(peak 4.045137, RMS 0.900702). This proves native load/play admission, not native
sound parity, full scripting coverage or quiet load-time/CPU performance. No
library payload, recovered bank key or PCM file is persisted. Combined workspace
and native quick regression checks remain the integration owner's next gate.

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

## Unavailable UVI banks

Catalog failures are grouped as `bank_unreadable`: one record per cause with
all affected locations. The Falcon/UVI tab keeps a visible unsupported-library
message even when no presets were admitted, instead of showing an empty-library
state. Other malformed-bank failures remain separate diagnostics. Successful
libraries stay available alongside the failure summary.

The reader discovery/Locate proposal is parked by the user's 2026-10-09
directive; this diagnostic-only change adds no reader selection, discovery or
external software access. Existing reader-backed receipts retain their original
provenance and do not establish support under the new policy. The release owner
must apply the product's separate access policy; this change only reports failures.
Its tests use synthetic catalog errors and UI fixtures, without opening installed
protected banks or touching a Wine prefix.


## UVI discovery on 0.3.344

The published `7cd326ee5b67` and preceding `81db45a3d462` both omitted UVI
Windows defaults and ignored loose `.ufs` files at a folder-of-libraries root.
A manually selected single-library folder or a nested bank folder still worked.
No new extension or Falcon-tab filter regression was found between those versions.

Discovery now adds Windows `ProgramFiles/UVISoundBanks` (also the native and
x86 Program Files variants) once, independently of previously imported Kontakt
roots. This is UVI's [documented soundware location](https://support.uvi.net/hc/en-us/articles/5265178227869-Step-3-Download-and-Install-Your-UVI-Product).
The `uvi_imported` v2 setting prevents removed defaults from being re-added on
every scan; **Find installed libraries** checks them again. Custom locations on
Windows and Linux remain selectable library roots. Loose banks are individual
libraries and do not hide adjacent nested libraries. Uppercase `.UFS` and direct
bank roots are covered. Failed catalogs retain the existing bank diagnostics;
the browser and Settings also show each affected root's bank count and causes.

Cataloging uses `Bank::catalog`, which reads directory metadata without preparing
content access or reading program/sample payloads. A synthetic bank with valid
directory metadata and unavailable payload access previously disappeared from
the catalog (**0 programs → 1**). Loading remains a separate operation.

Clear, self-authored fixtures reproduced a flat bank folder at **0 libraries /
0 presets before → 2 / 2 after**, and a mixed root at **1 → 3 libraries**.
Default-root discovery, saved-root deduplication, v2 settings migration, warm
catalog reuse, and the UVI browser's bank-file rows have focused regressions.
The authorized installed Linux root retains **4 candidate
library folders / 26 container paths** before and after; this is filesystem-only
identification, not a protected-bank program or playback measurement.

The directory-only follow-up finds **26 / 26 installed banks and 660 program
entries**. The production library scan lists **4 libraries / 660 programs /
0 bank issues**, with no payloads read. Filesystem discovery remains **4 candidate
folders / 26 containers**. A retained earlier baseline at `72bb9767` had zero
UVI libraries and 26 bank metadata rereads; a fresh 0.3.344 payload-opening
baseline was not run under the current directory-only scope.

The frozen W12 census worker succeeds on our
self-authored clear bank (exit 0, valid JSON). This excludes a universal
clear-bank worker failure; it does not establish the payload-stage cause of the
660 installed census failures, whose collector discarded stderr. All installed
directories examined declare encoded program entries and content-protected
members. No content-recovery step was run for this witness.

Content checks use our clear fixtures, with no activation, key extraction or Wine
runs. Actual licensed Windows installations were not available for this witness;
no native Windows installation was mounted for validation. No release/install or full corpus
claim is made. Detailed logs live under
`/mnt/Windows11/DEV_WORKSPACE/kontra-runs/w10-uvi-scan-344/`.

## Missing and dangling library paths

`c6393dce` skipped missing directories without a panic, but filesystem failures
were only logged and dangling entries were silent. Discovery now checks saved
roots before traversing them, records failed directory listings and metadata
reads, and skips unresolved symlinks without following nested links. The preset
walk retains the same failures if a path disappears during cataloging.
Unavailable paths are deduplicated and preserved when the worker publishes its
shelf; both browser tabs and Settings show their per-root count and causes.

A synthetic fixture retains its healthy library and preset while handling 25
dangling directory entries, a dangling bank link, a link cycle, a missing saved
root and a file used as a directory: **29 unavailable paths, 1 surviving library /
1 preset**. Its visible-reason assertion was RED on `c6393dce` and is now GREEN.
A synthetic registry query also verifies that stale registrations are filtered
safely, while invalid paths already saved as roots retain visible reasons.
No native registry or Wine prefix was read by these checks.

The reported Linux unclean exit has no stack or signal identifying its cause.
These checks establish scan survival for the fixtures, not attribution or
reproduction of that tester's crash. The existing merged nightly gates are not
repeated for this follow-up.

## Metadata-only native slot census

`Bank::open_metadata` opens the same validated directory as the full bank opener,
but leaves content access unprepared. The native slot worker uses this route
because program translation needs neither scripts nor samples. Clear and metadata
members can be read; content-protected members still require supplied access.
The full playback opener retains its existing content preparation behavior.

An authored encoded-directory fixture has an accessible metadata program and an
unrelated content member. Its census first failed at content setup, before reading
the accessible program; metadata opening passes without granting access to that
content member or to a content-protected program. The worker now preserves a fixed
`uvi-bank-directory` or `uvi-program-translate` failure stage in its JSON result,
with an error and empty programs, so unavailable inventories stay UNKNOWN. It
never exports the raw native error through this result.

The installed directory check opens **26/26 banks and lists 660 programs**.
All 660 program entries declare content protection; the bounded raw-prefix check
finds **0 recognised XML or ZIP headers**. Metadata-only reads refuse all 660 for
lack of supplied content access. Those inventories remain unavailable, rather than
being reported as measured: **0 before / 0 after**. The authored accessible
program instead moves **0 before / 1 complete inventory after**.

The original census used source `5172a9ef` and discarded native stderr. Its exact
historical failure stage is therefore unproven. An isolated, reader-unavailable
preflight reproduces reader-resolution failure on the authored encoded-directory
fixture and all 26 installed banks; the current metadata opener opens those
same directories. This controlled result does not establish the historical
worker environment or any bank's licensing status. Installed-bank checks never
prepare content access, decode member payloads, or read samples; receipts retain
only hashed bank identifiers and aggregate counts.


## Catalogued UVI programs with load requirements

The browser retains every directory-listed preset, including protected programs.
Each affected library card says “Catalogued · loading limited”; selecting it shows
its bank name and the number of presets that need content access before loading.
Unknown protection modes receive their own reason. These are declared directory
requirements, not a claim that program loading was attempted or that a license is
missing. Scanning never prepares content access or reads sample/program payloads.

The metadata index stores bank status with program paths, so a warm scan keeps the
same message. Index schema 2 rebuilds the disposable catalog from older indexes;
user settings and presets are unaffected. Clear banks receive no access warning.
The authored clear-bank playback test embeds a sine sample, deletes its loose
source, then uses the product's catalog, V2 loader, MIDI note and audio output.
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
`render-kontakt` also rendered `Una Corda Pure` (note 60). Each check rendered
158,400 stereo frames at 48 kHz. The opt-in UVI integration test additionally
renders directly into RAM. Rendered note performances are verification outputs;
no original programs, pictures, samples or access state are extracted.

Access/decode census success does not imply complete frontend semantics:
this branch reports unsupported Lua processors and modulation/effect mappings.
For example, VWinds `Clarinet A` has saved zero layer gains that need its Lua
initialization; its samples decode but its current translated render is silent.
Those mappings remain with the IR frontend owner.

Kontakt's `Content/...` sample references can resolve from its local player
native Windows installation's Native Instruments and VST3 folders. Wine
player discovery is superseded by the native-only policy; no Wine prefix is searched.
Discovery is bounded and cached; `KONTRA_KONTAKT_CONTENT` overrides it with a
path list of `Content` directories. Lookups cannot ascend or leave those roots.
Multiple player copies must contain byte-identical assets (comparison bounded
to 32 MiB). The installed Conflux multi census improved from 20/50 to 50/50
decoding by reading the actual Chords/Phrases tool WAVs, without synthesizing
or bundling replacements. `Samples` keeps its existing public API.

## Verification on the shared machine

Run every Cargo command and real-library census/render with
`/home/derpcat/.cache/kontakto-heavy COMMAND ...`.
The shared helper allows three jobs and waits for at least 10 GiB available RAM.
Use the target directory and sccache from `~/.cargo/config.toml`; do not set
`CARGO_TARGET_DIR` or `RUSTC_WRAPPER`. The parent retains the slot guard and
closes its descriptor in the command's child process, so persistent daemons
such as sccache cannot inherit a slot after the job finishes.
