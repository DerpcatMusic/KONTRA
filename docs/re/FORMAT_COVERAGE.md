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
