# Kontakto

Native Linux Kontakt sampler workbench using MOOSE + MUI. CLAP, VST3 and standalone targets. Reads the installed library folder directly; library files are never modified or extracted in place.

```sh
cargo moose install --clap --vst3 --user
cargo run --release --features standalone --bin kontakto-standalone
cargo run --release --no-default-features --bin kontakto -- scan
cargo run --release --no-default-features --bin kontakto -- inspect '/path/to/instrument.nki'
cargo run --release --no-default-features --bin kontakto -- inspect-mods '/path/to/instrument.nki'
cargo run --release --no-default-features --bin kontakto -- audit > audit.json
cargo run --release --no-default-features --bin kontakto -- render '/path/to/instrument.nki' /tmp/instrument.wav
```

Default library root: `/mnt/MAIN_STORAGE/Libraries/Kontakt`. Change it under Folders. The right browser displays local library artwork at its source aspect ratio, with separate Instruments (NKI) and Multis (NKM) filters. Samples, archives and mic/round-robin groups never populate the preset picker. Click an already loaded instrument to focus it; drag to add another instance. Click or drag an NKI into the rack; drop onto an existing instrument name to replace that part. Drag rack names to reorder without reloading their engines. Duplicate, remove, mute, solo and panic are wired to playback.

The rack holds 16 instruments, each with an independently selected group, MIDI input port/channel, stereo output, level and pan. Drag numeric values or double-click to type; channel 0 means Omni. Four MIDI input ports and eight stereo audio buses are declared to the host. Activate/connect auxiliary outputs in the DAW before selecting them. MIDI thru forwards note, controller and pitch-bend messages to one MIDI output. Device connections belong to the host. File-manager drops accept a single NKM or one or more NKIs; a drop on a rack name replaces that part, with remaining files added to free slots. Unsupported files and drops exceeding rack capacity are refused.

The black-and-white keyboard sends note-on at mouse press and note-off at release; keyboard Enter and Audition play a short preview. Mapping, Groups and Info inspect the selected rack part. The editor reflows at actual window size from 900×640 upward. Imports, artwork reads and decoding run outside the audio callback. Host state saves the entire rack, ordering, routing and controls. Samples have a 1 GiB per-part and 2 GiB total rack RAM ceiling.

## Compatibility

This is currently **a multi-instrument rack with manual group playback**, not a complete Kontakt replacement. Binary NKI groups/zones, key/velocity maps, tuning, sample boundaries, forward loops, WAV/AIFF/NCW, NKX members, polyphony, sustain and pitch bend are implemented. Samples are decoded into RAM with a 1 GiB bank limit. KSP playback callbacks, scripted articulation/legato/round-robin, Kontakt effects/modulation and disk streaming are not implemented. Import diagnostics identify these limits. A successful parse or nonzero render is not evidence of Kontakt sonic parity.

Encrypted preset subtrees and supported encrypted archive members use the owning library's existing HU/JDX fields in its `.nicnt`. AES/legacy resource decoding happens in memory before decompression; each archive caches its generated cipher stream. No access keys are embedded, logged or written to plugin state. Missing keys and invalid decompression produce errors. Legacy archive ciphers other than the supported library-key scheme are rejected. A `.nicnt` created only for library registration may have no access fields; readable Vista presets do not need them.

An archive directory/member pointing to zero-filled or invalid bytes is reported as damaged or missing. Intact siblings remain usable. Decryption cannot recover absent sample data. The audit lists missing references separately from instrument parse errors. It evaluates bounded KSP initialization previews, but does not decode every sample or execute playback callbacks.

## Checks

```sh
cargo test --lib --test playback
cargo test --manifest-path vendor/ni-file/Cargo.toml --test compatibility
cargo test --manifest-path vendor/ncw/Cargo.toml
# Requires this machine's local Vista and Una Corda libraries:
cargo test --test playback -- --include-ignored
```

The parser tests cover sparse loop slots, group ownership, truncation, clear/encrypted archive members, direct/encoded offsets, deterministic resource cipher vectors, wrong keys and decompression size bounds. Playback tests cover mapping, pitch, reverse, boundaries, polyphony, sustain, file resolution, layering, mute/solo, MIDI-port isolation, routing changes and separate audio buses. Native MUI input tests exercise drag-to-add, reordering, typed routing edits, mute/remove and piano press/release. Plugin process tests check bus isolation and MIDI-thru timestamps. Proprietary samples, keys, preset contents and renders are excluded from version control. Upstream ni-file unit tests depend on author fixtures absent from this checkout; its synthetic `compatibility` target is the runnable parser check.

## Local audit, 2026-09-29

All **778 installed NKI instruments parse**, up from 56 before encrypted subtree support. All 722 previously encrypted presets now decode with their library metadata. **118 instruments have no missing sample references**: 100 Solo, 7 Vista and 11 Pacific. The other 660 report missing or damaged resources; successful parsing does not make those resources recoverable. This audit validates mappings and reference resolution, not every sample payload or scripted playback.

Una Corda Cotton, Vista Harp and a Pacific cello pizzicato group produced finite, nonzero offline renders. The expanded rack passed clap-validator and pluginval after correcting its fixed eight-output VST3 topology. Current runs and local evidence are under ignored `artifacts/`; summary: `artifacts/audit-summary.json`.

## Source provenance

- MOOSE: `Matari-Audio/moose`, revision `bffa4677d0b82119d38566ce7e932dc5c463d497`.
- `vendor/moose-mui`: MOOSE revision above, with a small `MuiEditor::on_files` and `on_cancel` callbacks exposing MUI’s existing native file-drop handling; original Truce license retained. The latter releases editor-held notes on focus loss.
- `vendor/ni-file`: [Ma5onic/ni-file](https://github.com/Ma5onic/ni-file), revision `1b7a518243125857fddec8217167b47a35cb58fa`; local parser, archive and resource-decoding fixes. The checkout does not include an explicit license grant; resolve that before publishing or distributing this project as FOSS.
- `vendor/ncw`: [monomadic/ncw](https://github.com/monomadic/ncw), revision `75af0c022f4c1b80d76e51199d5947a51cd8faf8`, version 0.4.0, MIT/Apache-2.0. Upgraded from ni-file's 0.1.2 dependency; upstream codec source and tests retained.
- Archive/resource format research: [nkxtract](https://github.com/maxton/nkxtract) (`ca40dbf546bc35e1a7a7ab207968862704d7cad8`) and [unnks](https://github.com/JimiHFord/unnks) (`eb595382db60dd3391e984173a219a03ebecc622`). Those reference projects are GPL-licensed. This repository has not been cleared for redistribution.

UI font: Noto Sans variable from google/fonts, bundled under the SIL Open Font License in `assets/OFL.txt`. UI visual references: Kontakt Player overview and https://kodasampler.com/; no product artwork is bundled.

Preset selection opens a dedicated Instrument view. A bounded KSP initializer resolves integer/string variables, arrays, arithmetic, conditions, loops, called functions, control IDs, labels, menus, positions and hidden panels. Vista Harp (47 controls) and Vista 3 Cellos (48 controls) initialize successfully. The native view displays the authored layout as a **disabled preview**, over the named local wallpaper; custom widget skins are approximated, saved values are not restored, and control/MIDI callbacks are not executed. Working manual level, pan and audition remain separate. Raw group selection lives only in the Groups editor; Previous/Next in the performance view navigates complete instruments or multis from the same library. Unsupported initialization fails explicitly; no partial script drives audio. No library UI assets are bundled.

`cargo run --no-default-features --bin kontakto -- ui <instrument.nki>` emits the resolved interface and compatibility diagnostics as JSON. Initialization is limited by source size, token/nesting counts, array storage, control counts, evaluated value allocation and an instruction budget; it runs on the import worker, never the audio thread. Source behavior follows the [NI KSP UI commands](https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/user-interface-commands). This is an initialization subset, not a complete KSP interpreter.

NKM import unwraps Kontakt's AppSpecific container and loads each embedded program from the shared sample table into a rack part. The three installed Chorus multis are recognized; Traditional Syllables contains a controller plus Women and Men instruments. A controller with zero zones remains a named part without being treated as a missing sample. Missing-reference diagnostics consider only samples used by that program. Native NKM routing, bank/master processing and multiscript behavior are not restored; missing Chorus resources still prevent its sampled parts from playing. Program banks with multiple switchable programs and multis beyond 16 parts are rejected. Inspect metadata with `kontakto inspect-multi <multi.nkm>`.

## Complete local requirement inventory

[The per-library compatibility matrix](audits/README.md) covers all 781 preset files (778 NKI + 3 NKM), containing 787 programs across eight libraries. Every program parses. Of these, 666 have missing/damaged references; the 121 complete reference sets include three zero-zone multi controllers. This is a static requirements inventory, not a claim of full playback compatibility. Effect/module presence includes default and bypassed objects, and opaque private parameters still require research.

```sh
kontakto audit > audits/library-compatibility.json
kontakto audit-structure > audits/source-structures.json
kontakto audit-scripts > audits/script-requirements.json
python3 tools/summarize_audit.py
```

The ignored JSON reports retain affected paths, script callbacks/calls, uppercase variable/constant references, UI control types, initialization blockers and nested effect/modulation chunk IDs. Script-only rescans avoid sample/archive resolution. Static script scanning accepts up to 128 MiB independently of the stricter execution limits, so large scripts remain visible in the inventory even when the preview cannot execute them. No script source, access keys or sample payloads are included in these reports.

This pass also fixes group-condition lookup by chunk ID, malformed condition errors, and KSP continuation lines, hexadecimal integer literals and bitwise expressions. KSP syntax references: [NI arithmetic operators](https://www.native-instruments.com/fileadmin/ni_media/downloads/manuals/kontakt/KSP_Reference_Manual_26_08_2020_ENGLISH.pdf) and [control parameters](https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/control-parameters). These parser additions do not implement KSP playback callbacks, native Kontakt DSP or missing resources.

Missing-resource lists are deduplicated by stored path; every zone still retains its own availability state.

## Shared-cause compatibility work

[Script correlations](audits/CORRELATIONS.md) compare the same 796 active script slots before and after shared parser changes. Initialization previews increased from 7 to 31; this does not imply scripted playback. The interpreter now handles first-match `select/case` ranges, real-number variables/arrays/arithmetic, explicit numeric conversions (including legacy names), and bit shifts. Initialization parses only reached functions and caches their parsed bodies. Flat array literals use the global parse budget while each expression retains a separate 512-token bound; execution, nesting and allocation ceilings remain enforced. Unexecuted playback callbacks are not validated as working code. Shared lexical handling also accepts tabs and repeated whitespace, comment separators, and parenthesized control statements without a space; quoted text remains unchanged and `:=` inside labels is not an assignment. This clears six Una Corda block-parser failures across three presets, which now stop at unimplemented PGS/listener services; completed initialization remains 31.

[Resource correlations](audits/RESOURCES.md) group unresolved member references by archive instead of treating every affected preset as a separate defect. `kontakto audit-archives > audits/archive-health.json` checks container indexes and member headers, not full sample payloads. Invalid member headers are also summarized in importer diagnostics. Observed zero-filled headers in sampled Afflatus and Areia containers cannot be repaired by changing script parsing or decryption.

```sh
python3 tools/compare_scripts.py audits/script-baseline.json audits/script-requirements.json > audits/CORRELATIONS.md
kontakto audit-archives > audits/archive-health.json
python3 tools/correlate_resources.py
```

Language behavior follows NI's [control statements](https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/control-statements), [arithmetic reference](https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/arithmetic-commands---operators), and [legacy conversion aliases](https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/version-history).


## Shared initialization services and resource damage

[Host-service audit](audits/HOST-SERVICES.md): initialization previews increased from 31 to 36 across the same 796 script slots. PGS integer/string storage now lives per instrument and is shared across initialization slots, with key/index/memory checks. Failed slots roll back their shared-state writes. Keyboard setters/getters retain state, and listener registration validates signal types and intervals. PGS-change and listener callbacks are not dispatched; keyboard metadata is not connected to the live keybed. These remain explicitly diagnosed initialization previews, not scripted playback.

[Recovery checks](audits/RECOVERY.md): all 951,741 invalid member headers were verified zero-filled, and local duplicate/sibling searches found no replacement candidates. Archive parsing now preserves intact siblings when another member has a truncated header/payload or unsupported version, and reports each reason separately. It does not overwrite files or manufacture missing sample data. `audit-archives` reports `member_issues` while retaining the existing aggregate fields.

Service semantics: NI's [PGS reference](https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/advanced-concepts), [keyboard commands](https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/keyboard-commands), and [listener commands](https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/time-related-commands).


Control IDs now follow Kontakt declaration order starting at 32768, including ordinary variables and constants. Control access translates those IDs separately from storage indexes and rejects IDs that refer to non-controls. This fixes scripts that calculate control IDs; knob labels, units and help text are also retained as properties, with string-property lookup supported. [NI UI command reference](https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/user-interface-commands).
