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

## Installed reader discovery

Reader selection uses `KONTRA_UVI_READER`, then the saved `uvi_reader` path
(the same JSON key as v1), then read-only standard installation discovery.
Candidates include native Windows/macOS locations, `WINEPREFIX`, `~/.wine`,
and common Bottles/Lutris prefix folders. No application is shipped, copied,
downloaded or installed. Discovery checks existence; opening or selecting a
reader retains the existing namespace/hash validation. This hotfix does not
add reader formats: the decoder currently verifies Windows x64 Workstation
4.0.9; an unsupported Falcon/macOS binary gets a visible validation error.

Failed banks stay visible as one catalog diagnostic per cause, carrying all
affected locations. The Falcon/UVI browser offers **Locate…**, with an inline
path fallback when a file dialog is unavailable. Validation runs on the picker
worker; successful selection saves preferences and rescans every open instance.
Scans use the current settings snapshot even before its asynchronous disk save.
Unknown JSON settings continue to survive saves through the existing flatten
map. Failed/null bank metadata was already retried; a regression now checks
that successful replacement survives index save/reload without rereading.

The installed regression uses copies of settings/index with `env -i`. Its
baseline on 0.3.306 found zero Falcon libraries from 26 banks: the old Wine
candidate omitted the installed `UVI Workstation x64` directory variant.
Synthetic tests cover missing-reader diagnostics and keyboard-accessible Locate,
separately from the real automatic-discovery and saved-selection witnesses.
