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
