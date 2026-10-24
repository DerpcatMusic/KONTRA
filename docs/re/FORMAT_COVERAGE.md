# Library format coverage census

Tool: `tools/format-census` (release build). It parses every file under a
library root with the reader KONTRA ships for that family and prints one TSV
line per file (`family`, `outcome`, `detail`). Instrument files are also built
through the streamed loader, so script/engine warnings count as `partial`.
Protected content is read only through the library's own local access data
(`library-access`, the same path the plugin uses). Nothing is decrypted or
exported, and no sample audio is decoded.

Reproduce (one process, niced):

```
nice -n 10 build-slot cargo build --release -p format-census
format-census /data/main/Libraries/Kontakt --only nki > nki.tsv 2> nki.agg
format-census --ser                        # Kontakt SER classes, native schema status
format-census --explain <file.nki>         # full error chain for one file
format-census --resource <archive.nkr> <member...>
format-census --ncw <dir>                  # full NCW decode of every .ncw below dir
```

Scope of this pass: installed `/data/main/Libraries` on 2026-10-23, release
build of this branch. Counts are files, not libraries, unless marked.

## Kontakt (`/data/main/Libraries/Kontakt`)

| Family | Files | Outcome |
| --- | ---: | --- |
| `.nki` NIS preset | 682 | 47 full, 633 partial, 2 failed |
| `.nkm` multi | 54 | 1 full, 53 partial, 0 failed |
| `.nksn` snapshot | 1139 | chunk framing OK (container only, not playable content) |
| `.nkx` / `.nkr` resource archive | 229 / 12 | detected (member bytes not decoded in the census) |
| `.nicnt` access data | 9 | archive detected |
| `.nkc` NI stream cache | 241 | not a preset chunk container (decoded through the NCW stream path, not here) |
| `.nka` text/array data | 716 | preset text lists; `load_array` consumers listed below |
| `.ncw` audio | 21066 | header parse OK (census does not decode samples; full decode via `--ncw`) |

Partial reasons are per feature, counted in files (an instrument can have
several):

| Rank | Partial feature (NKI, count of files) | Kind |
| ---: | --- | --- |
| 1 | KSP `polyphonic` declaration initializer ignored: 349 | sampler-ksp semantics (`sema.rs:705`) |
| 2 | KSP read of a polyphonic variable outside a note callback: 266 | sampler-ksp (`lower.rs:1093`) |
| 3 | KSP `find_group: group not found`: 227 | sampler-ksp / group lookup |
| 4 | Zone keys beyond the runtime pitch range, narrowed: 161 (51 of 53 NKM too) | engine pitch limit (`load.rs:603`) |
| 5 | `keyswitch script` (not translated): 105 | keyswitch.rs:177 |
| 6 | KSP array index out of bounds (`!` and `%` arrays): 131 | sampler-ksp eval |
| 7 | `load_array` of `data/*.nka` not loaded: 50 NKM (Conflux), 1 NKI | reader/resource (below) |
| 8 | `filter law` not modelled: 6 | engine |
| 9 | Wavetable, Convolution IR, Tape Saturator live controls, LFO waveform: under 5 each | engine |

Hard failures (NKI):

- 2 × `Areia 1.2.0 / Legacy Patches` (`Legato` 01 and 05): stream cache NCW
  `invalid NCW block signature at block 4, channel 0`. The NCW header is
  accepted (21066 headers parse), but this block fails in `vendor/ncw`
  block decode. Not fixed in this pass.

Multi-only load gap:

- 50 multis reference `Resources/data/{vel_db,gui__fx__num_irs,gui__fx__num_cab_irs,gui__fx__ir_names,gui__fx__cab_ir_names}.nka`.
  `vel_db.nka` is absent from `Conflux.nkr`. The `gui__fx__*` members are present
  (11-byte payloads) but the array completion is rejected, so the warning
  "not loaded" is emitted. Source: `crates/sampler-ksp/src/eval.rs:2130`
  and `crates/sampler-kontakt/src/load.rs:723`.

## UVI (`/data/main/Libraries/UVI`)

| Family | Files | Outcome |
| --- | ---: | --- |
| `.ufs` UFS2 bank (program translate, no sample decode) | 26 | 5 translate, 21 fail |
| `.r2ruvi` | 75 | 48-byte license tokens, not content (no reader needed) |
| `.parts`, `.pdf` | 4 | not content |

The 21 failures all have the same cause, and the message is now visible:
`Program "Program": live global requires the actual shared owner modulator;
source ScriptEventModulation ... shared source lifecycle/input actor is not
implemented`, plus one `shared Frequency connection has no unique real Freq
control`. These come from the intentional refusal in
`crates/sampler-uvi/src/lib.rs:1646` (`require_ordinary_source_publication`)
and `inserts.rs:74`. Before this pass the reason was hidden: `access.rs`
`failure_reason` kept only the outer context. It now keeps the full chain.
The fix needs the actual script-event source lifecycle (engine work), not a
reader change.

Gating: the 21 banks are the bulk of Augmented Orchestra and VWinds
(Clarinets, Double Reeds, Flutes). Each of those is a whole-bank load failure,
so this is the highest-impact item.

## Not KONTRA-readable today (no reader)

| Family | Files | Size | Notes |
| --- | ---: | ---: | --- |
| Ample `.ascf` (`aswv` magic, `PleaseDontHack` marker) | 482 | 20.4 GB | Ample Sound waves. Unknown container/codec. Needs RE before any reader. |
| Ample `.cf` / `.dat` | 7 / 4 | small | text configs (`1007496.77…`, `Sus1-64-1-1|…`) |
| Modo Drum `.pak` (`IKMPAK`) | 3 | 5.0 GB | IK pak archive with sample member names. TOC not yet decoded. |
| Superior Drummer `.obw` (`RIFF…ttbwoidx`) | 13 | ~1.8 GB largest | RIFF wrapper around a Toontrack index chunk. Not a plain WAV. |
| Superior Drummer `.s20` (`#V 2.0`) | 18 | ~15 KB each | text sample-project files that reference `.obw` banks |

Ample, Modo and Superior Drummer were already outside the requested
Kontakt/UVI scope in `docs/diagnostics/20261015-library-triage.md`.

## Kontakt SER coverage (`format-census --ser`)

`ni_file::kontakt::native_schema::readers()` lists 118 serializable classes
(`tools/re/ser_catalog.py`, 118 rows in `docs/re/kontakt-catalog/ser_classes.tsv`):

| Status | Classes |
| --- | ---: |
| typed | 93 |
| stub | 9 |
| host-branch | 7 |
| nested | 4 |
| not-a-reader | 4 |
| virtual-call-on-stream | 1 |

`docs/re/kontakt-catalog/reader_schemas.tsv` has 235 rows, not the 409 cited
in the lane brief. Counts above come from the code, not the brief.

## Ranked gap table (files or libraries affected)

| # | Gap | Affected | Fixed here? |
| ---: | --- | --- | --- |
| 1 | Kontakt NKI/NKM partial: KSP polyphonic/array/group warnings (sampler-ksp) | 633 NKI + 53 NKM | no; script semantics, needs Kontakt doc check |
| 2 | Ample `.ascf` sample format has no reader | 482 files, 20.4 GB, 4 Ample libraries | no; needs RE of container/codec |
| 3 | UVI UFS banks: script-event global Gain/Pan source unimplemented | 21 of 26 banks, 2 UVI products | diagnostic only (`access.rs` chain); engine work next |
| 4 | Zone keys beyond runtime pitch range narrowed | 161 NKI + 51 NKM | no; engine pitch limit |
| 5 | Keyswitch script not translated | 105 NKI | no |
| 6 | `load_array` `data/*.nka` not loaded (multis) | 50 NKM (Conflux) | no; open: vel_db missing, gui__fx decode rejected |
| 7 | Modo Drum `.pak` IKMPAK has no reader | 3 files, 5.0 GB | no |
| 8 | Areia legacy NCW stream cache block signature | 2 NKI | no; in `vendor/ncw` block decode |
| 9 | Superior Drummer `.obw` / `.s20` have no reader | 31 files | no |
| 10 | Filter law, wavetable, convolution IR not modelled | 6 + under 5 each | no; engine |

## Next steps, in order

1. `load_array`: find why the 11-byte `gui__fx__*.nka` payloads fail
   `ArrayFileCompletion::from_bytes` (`sampler-core/src/array_file.rs`), and
   where `vel_db.nka` lives (not in `Conflux.nkr`).
2. UVI: implement the script-event global source lifecycle for `live global`
   Gain/Pan (`sampler-uvi/src/inserts.rs:74`). That would unblock 21 banks.
3. NCW: check the block-4 channel signature in the Areia stream caches
   against the FORMAT notes in `vendor/ncw`.
4. Ample `.ascf`: RE the `aswv` container with the native oracle before any reader.

## Full-corpus pass, 2026-10-24 (installed `/data/main/Libraries/Kontakt`)

Run per library (11 roots, 4 in parallel), release build of this branch.
Measured, not estimated. Files, not libraries.

| Family | Files | Outcome |
| --- | ---: | --- |
| `.nki` | 682 | 251 full, 429 partial, 2 failed |
| `.nkm` | 54 | 2 full, 52 partial, 0 failed |
| `.nkx` | 229 | 229 archive index OK (1,107,379 members indexed) |
| `.nkr` / `.nicnt` | 12 / 9 | archive index OK |
| `.nkc` | 241 | census reads them as chunk containers: `failed` (see below) |
| `.ncw` | 21066 | header parse OK (full decode not run) |
| `.nksn` | 1139 | chunk framing OK |

### Load failures (what they are)

- **2 NKI, Areia 1.2.0 Legacy Patches (Legato 01, 05): hard failure.** Root
  cause is in `Areia_0.nkx`, not the NKI reader. Member
  `Samples/Areia_16VlnsLgtSstndVFMDyn3RR1_75_87_12.ncw` decodes its first
  groups correctly, then group 4 (table offset 10420, after two stereo
  groups of 2464 bytes each) has no block signature under any keystream start
  in 0..65536 (`LibraryKey::apply_at`). The raw bytes at that group do not
  decrypt to a block header with the library's keystream. Treat as corrupt
  shipped data until Kontakt is checked with the oracle.
- **241 `.nkc` "failed"**: not a load failure. `.nkc` is an NI stream cache
  (`NICache`, magic `0x7A10E13F`), not a preset chunk container; the census
  parses it with the chunk reader and rejects it. Needs NICache-aware
  classification in the census; no sample is lost.

### Monolith (`.nkx`) resolution

- `format-census --nkx <archive|dir>` decodes every NCW member of an archive
  through the archive index. On `Areia_0.nkx`: 6050 of 6067 directory entries
  have a valid member header at their declared offset; the other 17 are all
  Dyn3 `VFM` members in the `75_87` to `77_81` run. For those 17 the valid
  header sits exactly 0x8000 bytes before the declared offset (verified by
  signature scan; their payloads are not decoded yet). Other `.nkx` files
  have no invalid entries.
- The reader now names the reason: `Invalid resource member: Invalid NKX
  member signature` instead of a bare `Invalid resource member`.

### Missing content, not a reader gap

- `load_array: data/vel_db.nka not loaded` (51 Conflux multis): `vel_db.nka`
  is absent from `Samples/Conflux.nkr`, `Conflux.nicnt`, `CNX_01.nkx` and
  `Conflux.nkc` (census `--resource` on each: `absent`). The script declares
  `%vel_db[1408]` and loads it in init; whether Kontakt restores it from the
  preset's saved state is not verified.

### Script warnings (not format; sampler-ksp)

- `find_group(): Group "..." was not found` (Areia 150, Audio Imperia Dolce
  77 in every Legato-type instrument): the script looks up group names that
  this instrument does not define. Kontakt prints the same text, but whether
  it reaches the same branch is not verified.
- `keyswitch script` (102), `array index N out of bounds` (about 131).

### Tooling added

- `format-census --nkx`, `CENSUS_PATHS=1` (adds a local path column; never
  committed output), `.nkx`/`.nkr` now index-only (NIFile::read validated
  whole multi-GB monoliths and stalled the census for hours).

### Next, in order

1. Areia legacy: check group 4 of member 75_87_12 against Kontakt with the
   oracle (does Kontakt play Legato 01 with that member's zones silent?).
   Decide between "damaged shipped data" and "our keystream model is wrong".
2. The 17 shifted entries: if the -0x8000 header is the real one, add the
   recovery in the reader and decode the payloads with `--nkx`.
3. NKC: NICache classification in the census.
4. `vel_db`: check the NKM saved state for `%vel_db` (preset-level persistence).

## Partial NKI causes, 2026-10-24 (pre-fix census; 429 partial NKI)

Instruments per cause (an instrument can carry several causes). Measured with
the census value mode (local only). "Real loss" means sound or articulation
data is dropped; "report" means the reader says something it could not map
while the instrument still plays what the file describes.

| Cause | Instruments | Libraries | Class |
| --- | ---: | ---: | --- |
| KSP `find_group` on an absent group name | 227 | 2 (Areia, Dolce) | script semantics (open: needs KSP check) |
| KSP array index out of bounds, read returns zero | 134 | 3 | script semantics (KSP lane) |
| Keyswitch script: control-key hint, no switch found | 105 | 3 (Afflatus 102) | report, not a loss (see below) |
| KSP other script semantics | 17 | 8 | script semantics |
| DFD preload window not applied to stream head | 14 | 2 | real: engine streaming, not format |
| Loop crossfade longer than loop, clamped | 6 | 3 | Kontakt-matching clamp, documented in load.rs |
| Parameter clamped to declared range | 5 | 2 | report |
| Modulation of a module parameter / modulator intensity / RandomBipolar | 6 | 3 | real: modulation not modelled |
| Tape Saturator, Solid G-EQ, Solid Bus Comp, Filter law, Convolution, LFO waveform, wavetable, effect, Jump, time machine, native interface, dropped | 26 | 5 | real: FX not modelled (engine lanes) |

Keyswitch decision (open for the user): all 102 Afflatus reports are the same
text, `keyswitch hints (control keys [21, 21]) but no recognized switch in on note`.
The script's only key declaration is `set_key_type(%KS_keys[$KS_Base], $NI_KEY_TYPE_CONTROL)`
with the name "Reset RR"; articulations come from the native group start rows, which
are unaffected. The existing test `reset_and_breath_keys_are_hints_not_switch_sets`
deliberately keeps this as a report, so changing it is a reporting policy choice.

## Fixes landed in this pass

- NKX stale directory offset (PR 595): 17 members of `Areia_0.nkx` now resolve.
  Five sampled members checked: all resolve, shifted ones decrypt to the NCW magic.
- `load_array` warning suppressed when the array holds saved state (sampler-ksp):
  Conflux multis save `%vel_db:ints` in the preset (checked on Adios Horner program 1),
  so the absent data file loses nothing.
- NKC classified by NICache magic (census), not the chunk reader: 241 files are
  stream caches (`container-only`), not failures.
- Keyswitch control-key hint with no switch: reported as `Reason::Info` (PR 602).
- KSP `find_group` misses and array out-of-range reads: `Info` notes when the
  message is a native Kontakt diagnostic (PR 608). Those lookups are dead guards in the
  developer branch that the repo docs describe as native diagnostics.
- NCW stream errors name the sample file and block (PR 624).
- Census: `CENSUS_MEMBERS` and `CENSUS_PATHS` for checking one archive member (PR 625).

## Still open

- Areia Violins Legacy Legato and Violins 8va Legato (Areia_0.nkx): the stream
  cache fails at `block 11`. Now named (PR 624), the failing member is
  `Samples/Areia_16VlnsLgtSstndVFMDyn3RR1_75_87_12.ncw`, the only one of 4,574
  members this instrument uses that fails a full decode (`block 4, channel 0`).
  Blocks 0–3 decode. From block 4 onward no NCW block signature appears at any
  keystream alignment (0..131072 searched with `LibraryKey::apply_at`), and the
  member has no other copy in the library. The payload looks damaged or truncated.
  Open: an oracle check of whether Kontakt plays this member. Until then the
  instrument fails to load; a degrade-to-silence path would be a product decision.
- `find_group` (227 instruments): PR 608 reports these as native-diagnostic notes.
  Still open: confirm in Kontakt that the absent group names are reached.

