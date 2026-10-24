# KSP translation gaps (corpus script-load survey)

Owner: corpus script-load survey lane. Shared with the other KSP lanes: pick a row, fix it
universally (never per instrument), add a test, then update its count here.

## Method

- Corpus: `/data/main/Libraries/Kontakt`, 735 items (682 `.nki`, 53 `.nkm`), 11 libraries.
- Run: `corpus-health run OUT --tier full --workers 4` (parse + lower + play probe, script
  diagnostics per instrument). Symbol usage: `corpus-health ksp OUT --md RANKED.md`.
- Script sources: `sampler-kontakt/examples/dump_scripts` (cache only, never committed).
- Counts: **inst** = instruments with the diagnostic, **libs** = libraries, **n** = occurrences.
  Rank = inst x libs (descending), then n.
- Classes: **BUG** = KONTRA defect; **FAITHFUL** = the script itself does this (dead branch,
  out-of-range read) and the message is Kontakt's wording or the behaviour is the same;
  **ENGINE** = needs an engine feature outside the KSP layer; **ASSET** = file missing from the
  install; **HEURISTIC** = a detector in sampler-kontakt that does not resolve a script pattern;
  **APPROX** = documented approximation.
- Kontakt wording check: the message text is in `Kontakt8.exe` for "Index will be invalid" and
  "was not declared or is not allowed here". Other messages ("read returns zero", "clamped to its
  declared range") are KONTRA's own text, so their Kontakt behaviour is not proven.

## Baseline (2026-10-24, origin/main 5a7d253b3)

| check | result |
|---|---|
| parse tier: items failing to load | 1 of 735 (ANALOG STRINGS, native region rejected) + 53 multis (no zone covers any key) |
| script compile errors | 1 instrument (Shreddage E-Upright, Console slot) |
| script warnings (`SCRIPT-WARN`) | 426 across 10 libraries |
| KSP symbols not `full` | 1888 simplified, 26 stub, 56 unsupported (of 1996; corpus-weighted full 13.0%) |

## Ranked gaps

| # | cluster | inst | libs | n | class | where | status / next |
|---:|---|---:|---:|---:|---|---|---|
| 1 | `find_group()` "Group ... was not found" in dead branch (Pyramid script) | 229 | 2 | 229 | FAITHFUL | Areia 16 Violins, Dolce | Script looks up developer-branch groups (`10VlnsChnswStbsBr...`) the instrument does not have; index -1, branch skipped. Kontakt's own text. Keep, or demote to info. |
| 2 | keyswitch detection: "keyswitch hints (...) but no recognized switch" | 105 | 3 | 106 | HEURISTIC | Afflatus Brass (Brass Shorts) | Switch keys live in `%KS_keys[$KS_Base]` with `$KS_Base` a slider; `sampler-kontakt/src/keyswitch.rs` (Detection::Unrecognized) does not resolve an indexed-by-variable base. Playback runs in the script; the articulation metadata/UI is missing. **Top real cluster, next round.** |
| 3 | `array index N out of bounds` (read returns zero/empty) | 77 + 54 + 3 | 1 each | ~2000 | FAITHFUL (unverified) | Brass ASC (`!key_art_names[6]` of length 2), Pyramid (`%_legato_aips_post_sustain_event_id[72]` of 72) | Scripts read past their own declared length. Verify Kontakt returns empty/zero (oracle) before changing. |
| 4 | `set_engine_par($ENGINE_PAR_WT_MOD_AMOUNT)` unsupported | 52 | 2 | 209 | ENGINE | Conflux GUI | No engine id: `sampler-core/src/engine_par_ids.tsv` has `WT_MOD_AMOUNT`, `WT_MOD_TUNE`, `WT_MOD_TYPE`, `WT_MOD_WAVE` with `-` (no id). Needs the wavetable modulation in sampler-core. Wavetable lane. |
| 5 | `get_engine_par_disp_ext(...)` unsupported | 51 | 1 | 204 | ENGINE | Conflux GUI | Same root as 4 (its first argument there is `WT_MOD_AMOUNT`); Conflux labels stay empty. |
| 6 | `load_array: data/vel_db.nka not loaded` | 51 | 1 | 51 | ASSET | Conflux GUI | No `vel_db*` file anywhere in the Conflux install. Not a KONTRA defect unless the file is shipped elsewhere. |
| 7 | `$name clamped to its declared range` | 8 | 3 | 51 | FAITHFUL | Pacific FX Long, Morphology, Una Corda | Clamping is correct; the warning is KONTRA's own. Consider info level. |
| 8 | `menu item N out of range ...; set_menu_item_visibility ignored` | 3 | 1 | 15 | FAITHFUL | Una Corda | Script index equals item count (0-based). Ignored, as Kontakt does. |
| 9 | `DurationSixteenth/Quarter, SignatureNum read in on init takes host transport` | 5 | 2 | 10 | FAITHFUL | Una Corda, Analog Strings | Informational: the host supplies the transport value. |
| 10 | `write to %CC lowered as set_controller` | 5 | 1 | 5 | APPROX | Vista | Documented lowering. |
| 11 | `later ui_control callback replaces an earlier one` | 1 | 1 | 27 | UNKNOWN | Analog Strings | Needs a Kontakt check of duplicate `on ui_control`. |
| 12 | `get_ui_id(): "$last_id_anchor" was not declared or is not allowed here!` (compile error) | 1 | 1 | 1 | FAITHFUL (likely) | Shreddage E-Upright, Console slot | Plain variable passed to `get_ui_id`. Kontakt's wording is in the binary, so Kontakt probably also rejects it. The slot does not run. Confirm with the oracle. |
| 13 | `$INST_LIB_*_ID` unknown UI id 0 / not in performance view | 1 | 1 | 13 | **BUG, fixed** | Shreddage E-Upright | Fixed on this branch (`5246a819d`): vendor ids added, library fields accepted. Test `tests/inst_lib_fields.rs`. |
| 14 | `attach_zone` / `set_ui_wf_property` simplified | 1 | 1 | 2 | APPROX | Areia Sound Design | Documented approximation. |
| 15 | `$CONTROL_PAR_AUTOMATION_ID[]`, `$CONTROL_PAR_CURSOR_PICTURE[]` interface arrays | 1 | 1 | 2 | UNSUPPORTED | Shreddage/Pacific mainScript widget 650 | Array-valued control par in the interface, low impact. |
| 16 | `$MARK_10`, `$ENGINE_PAR_SENDLEVEL_SMOOTHED_0..7`, `@syntax` unsupported | 1 each | 1 each | 1 each | UNSUPPORTED | various | Single-use symbols; no ledger entry (`SYMBOLS.tsv`). |
| 17 | `find_zone` (stub), `$CONTROL_PAR_CUSTOM_ID`, `$CONTROL_PAR_DISABLE_TEXT_SHIFTING` (stub) | 1 each | 1 each | 1 each | STUB | Pacific, Shreddage | Accepted and ignored. |

Not KSP (routed to other lanes, found by the same survey):

- 50 Conflux `.nkm` multis + 3 Audio Imperia CHORUS `.nkm` + 1 `.nkm` "no zone covers any key at any
  velocity" (corpus-health `pick_key`). Multi program selection, not script code.
- ANALOG STRINGS.nki: "zone 12867: Regions rejected by the native runtime: invalid input" (loader).
- SOUND-STUCK (367) and SOUND-SUSPECT (450) in the full tier: engine/sound lanes.

## Round 2 (2026-10-24): builtins by instruments affected

Source: the existing full-tier census (`~/.cache/kontra-ksp-survey/full-r1.jsonl`, 735 items, 11
libraries; per-instrument `scripts.diagnostics` and `unsupported`) and `SYMBOLS.tsv`. No new sweep.
Only KSP-level builtins/constructs; engine-feature and asset rows are listed for completeness.

| rank | item | inst | libs | class | status |
|---:|---|---:|---:|---|---|
| 1 | `set_engine_par($ENGINE_PAR_WT_MOD_AMOUNT)` unsupported; ignored | 52 | 2 | ENGINE | PARKED (WT_MOD blocked: Conflux does not load in the oracle) |
| 2 | `get_engine_par_disp_ext(WT_MOD_AMOUNT)` unsupported; text empty | 51 | 1 | ENGINE | PARKED, same root as 1 |
| 3 | `find_group()` absent group, dead branch | 229 | 2 | FAITHFUL | wave70 lane (find_group-absent) |
| 4 | `array index N out of bounds` (`!name`, `%name`) | 77+54+3 | 1 | FAITHFUL (unverified) | wave70 lane (arrays) |
| 5 | keyswitch base-key hints (`%KS_keys[$KS_Base]`) | 105 | 3 | HEURISTIC | dropped (breath cluster, report-only) |
| 6 | `load_array: data/vel_db.nka not loaded` | 51 | 1 | ASSET | file missing from install |
| 7 | `$name clamped to its declared range` | 8 | 3 | FAITHFUL (KONTRA warning) | noise; no behaviour change |
| 8 | `menu item N out of range; set_menu_item_visibility ignored` | 3 | 1 | APPROX | Una Corda; Kontakt wording unverified |
| 9 | `write to %name lowered as set_controller` | 5 | 1 | APPROX | documented lowering |
| 10 | transport reads in `on init` (`DurationQuarter`, `SignatureNum/Denom`) | 6 | 2 | informational | host transport supplied by loader |
| 11 | `find_zone` (stub: always NOT_FOUND) | 1 | 1 | STUB | 470 uses in one script; manual semantics not verified |
| 12 | `$CONTROL_PAR_CUSTOM_ID`, `$CONTROL_PAR_DISABLE_TEXT_SHIFTING` (stub) | 1 each | 1 | STUB | UI-only; stored, no widget effect |
| 13 | `$MARK_10` unsupported | 1 | 1 | UNSUPPORTED | 527 uses in one script |
| 14 | `$ENGINE_PAR_SENDLEVEL_SMOOTHED_0..7`, `@syntax` | 1 each | 1 | UNSUPPORTED | single use |
| 15 | later `on ui_control` replaces an earlier one | 1 | 1 | UNKNOWN | Kontakt behaviour unverified |

Decision for this round: no universal fix is implemented. The engine-level rows (1-2) are parked, rows
3-4 belong to the wave70 lane, and rows 6-10 are assets, noise or documented approximations. Rows
11-15 each touch one instrument, and their Kontakt semantics (`find_zone`, `$MARK_10`, `on ui_control`
duplicates) cannot be checked here: the KSP manual pages returned HTTP 403/404 and no oracle conformance
run was made. The next step for 11-15 is one oracle-slot probe per symbol (a KSP script that writes the
result into an engine parameter), then the fix.

## Round 3 (2026-10-24): stubs and approximations, measured on Kontakt 8

Method: one oracle-slot probe per symbol (`dump.py` with a Lua probe under `oracle-slot --exclusive`).
Each test instrument is a fresh KSP script that encodes one value into the engine volume
(`1000 * (value + 10)`); calibration instruments in the same session give the dB-to-value map.
Persisted variables cannot be read back, because Kontakt's saved NKI is rejected by our reader
(`.nicnt` access data), so this encoding is the only readback.

| symbol | measured Kontakt 8 behaviour | KONTRA today | action |
|---|---|---|---|
| `$MARK_n` | `$MARK_1`=2, `_2`=4, `_3`=8, `_5`=32, `_10`=1024, `_11`=2048 (bit 2^n) | `1 << (n-1)` (off by one), census "unsupported" for `_10` | fixed in PR #643, with a compile test |
| `find_zone(name)` | index of the zone whose sample name (file stem) equals `name`; `-1` if none or empty; `find_zone("pv_tone")` = 0 and `"pv_other"` = 1 for zones 0 and 1 | always `NOT_FOUND` (stub) | NOT implemented: needs zone sample names in the KSP environment (IR asset to `zone_catalog` mapping and an init-cache field). Group names did not match a zone (`find_zone(group_name(0))` = -1 for an unnamed group); whether a *named* group matches its zone is unmeasured. Morphology's own script is protected, so its call cannot be probed directly. |
| `later on ui_control replaces an earlier one` | not measurable with the volume readback (fires on UI events) | warning | callback family: left to the wave70 conformance lane |

Approximations (round-2 rows 8, 9, 14): not probed. Their effects are UI state (menu visibility) or
MIDI controller writes (`%CC` lowering), and neither shows in the volume readback. A probe needs a UI
or MIDI-driven oracle run, which is a separate piece of tooling. The find_zone plumbing is a
medium-size change (IR, loader, environment, cache) and is recorded here as the next step, not
implemented in this round.

## Round log

- Round 1 (branch `ksp/corpus-survey-r1`): cluster 13 fixed (`5246a819d`), regression test red then
  green, sampler-ksp suite 556 passed, symbol map synced (`ksp_symbols.py check`: 1999 rows, 0
  problems). Parse tier, same corpus: script warning instances 2587 -> 2574 (instruments with a
  warning 322 -> 321); the 13 removed are exactly the unknown-UI-id and perf-view warnings.
  Next: cluster 2 (keyswitch base-key pattern: `%KS_keys[$KS_Base]`, base set by a slider with a
  saved default). `keyswitch.rs` reports keys that depend on runtime state by design, so the fix
  must evaluate the init default and the saved base rather than guess. Then decide 1 and 3 with
  an oracle run.
- Unverified: whether real Kontakt warns or stays silent for clusters 1, 3, 7, 11, 12.
  Do not change behaviour on those without a native measurement (`docs/re/NATIVE_ORACLE.md`).

## How to re-run

```sh
corpus-health run ~/.cache/kontra-ksp-survey/run.jsonl --tier full --workers 4 /data/main/Libraries/Kontakt
corpus-health health ~/.cache/kontra-ksp-survey/run.jsonl
```

The per-script diagnostics are in each record's `scripts.diagnostics`. Group them by `feature`
and `location` to rebuild this table.
