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
installation, including Wine's standard Native Instruments and VST3 folders.
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
