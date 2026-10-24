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

## Round 4 (2026-10-24): remaining rows, measured or documented

Same probe method as round 3 (volume readback, calibrated in the same session). Compile errors are
not observable through the oracle's Lua API (`set_instrument_script_source` returns success for an
undeclared name), so text-level compile rows stay unmeasured.

| row | measurement | decision |
|---|---|---|
| 8 `set_menu_item_visibility` out of range | UI state only (menu visibility); no audio effect to read back | documented approximation: out-of-range index is ignored (KONTRA's warning) |
| 9 `%CC` write lowered as `set_controller` | MIDI controller output; not in the volume readback. `KSP_FRONTEND.md` documents `set_controller(number,value)` as a downstream CC | documented approximation (lowering matches the documented role); a MIDI oracle run is needed to close it |
| 14 `attach_zone` / `set_ui_wf_property` | waveform UI state; `w5-ksp-v1-parity.md` records "no consumer" | documented approximation, UI only |
| 7 `$name clamped to its declared range` | measured: `declare ui_knob $k(0,100,1)`; `$k := 500` reads 100, `$k := -20` reads 0; a plain `$p := 500` reads 500 | KONTRA's clamp matches Kontakt for declared ranges; the warning text is ours. Keep (no behaviour change). |
| 10 host transport reads in `on init` (`$DurationQuarter`, `$DurationSixteenth`, `$SignatureNum/Denom`) | every read gives the default volume (-6 dB): the init does not complete without a host transport | not measurable under the oracle; the warning stays (informational) |
| 12 `get_ui_id(<plain variable>)` compile error | not observable through the oracle Lua API | FAITHFUL (likely), unverified |
| 15 later `on ui_control` replaces an earlier one | fires on UI events, not measurable | callback family: wave70 lane |
| 16, 17 single-use symbols and UI stubs | UI-only or single-use | no action this round |

Remaining rows are either wave70's (find_group, arrays, callbacks) or need a UI or MIDI oracle
(`%CC` output, menu visibility, waveform UI). Closing them needs that tooling, not another volume probe.

## Round 5 (2026-10-24): fresh gaps from the census, no new PRs

Source: `~/.cache/kontra-ksp-survey/full-r1.jsonl` (1470 records, 481 with `unsupported_ranked`),
taken before the Round 2-4 merges. Ranked by instruments, not by occurrences. Only the census is
used (no corpus sweep, no oracle). The census is dominated by Conflux (one library, parked).

| rank | gap | instruments | libraries | reading | decision |
|---:|---|---:|---|---|---|
| 1 | `find_group("…")` name not found (warning text) | 229 | Areia, Dolce | the scripts ask for group names the instrument does not have (`4CllChnsw…`, `10VlnsChnsw…`); the lookup is exact and case-sensitive, like Kontakt's | likely faithful (Kontakt would warn too); needs an oracle run to confirm, so not changed |
| 2 | keyswitch hints without a recognised switch | 104 | Afflatus Chapter II Brass (Brass Shorts family) | `keyswitch.rs:423`: hints exist but the detector finds no switch set in `on note`; articulations do not switch | **top universal real gap**, needs the script source to read the `on note` pattern (no public accessor for raw script text) |
| 3 | `!name` array index out of bounds | 77 | 1 | reads return empty text, as the warning says | faithful by construction; only the wording differs |
| 4 | `%name` array index out of bounds | 54 | 1 | same mechanism (`%_legato_aips_post_sustain_event_id`) | faithful by construction |
| 5 | `$ENGINE_PAR_WT_MOD_AMOUNT` unsupported | 52 | 2 | WT_MOD, parked | parked |
| 6 | `get_engine_par_disp_ext` unsupported | 51 | Conflux | display text of an engine parameter | ledger Approximate; Conflux only |
| 7 | `load_array: data/vel_db.nka` not loaded | 51 | Conflux | Conflux ships no `data/` folder; Kontakt would fail the same load | faithful |
| 8 | `RandomBipolar` external modulation omitted | 51 | Conflux | `library.rs:2254`, `UnknownLawOmitted`: the law is not measured | next real modulation gap; needs a measured law |
| 9 | modulation target / intensity / module-parameter, effect | 30-51 each | Conflux (almost all) | one library's mod graph | not universal |

Nothing in this round is a universal translation fix that can be measured today. Rank 2 is the one
to pursue next: it needs a raw script-text accessor (or the NKI script chunk decoded) to read the
`on note` switch pattern before any change. Rank 8 needs a measured RandomBipolar law.

## Round 6 (2026-10-24): census rerun, breath key decision, backlog

Census rerun: `tools/corpus-health/run.sh run full-r2.jsonl --tier full --workers 2` over
`/data/main/Libraries/Kontakt` (735 items, nice 19, 12 GiB scope). The record count and the
unsupported ranking match round 5 except for the keyswitch row, which is now Info.

Decisions and measurements:

- **Afflatus breath key 91 (`2 Horns Repetitions`): one-shot trigger (Option B).** Key 91 starts
  groups 28-31; key 60 starts 0-3 (`tests/afflatus_breath_keyswitch.rs`, PR #656). The keyswitch
  is not an articulation entry. Hints without a recognised switch set are Info now, not a gap
  (PR #658).
- **Breath level (backlog, oracle needed for the verdict).** Decoded source peaks through
  `Prepared::source_storage` and `copy_resident_range`: breath sources 0.00013-0.00023
  (about -78 dBFS); plain sources for key 60 0.011-0.033 (about -40 to -30 dBFS). The -38 dB
  gap is already in the decoded data, before any playback. Open question: whether the decode is
  faithful for those sample files. Oracle check: capture key 91 vs 60 in Kontakt when the oracle
  is free; compare the breath sample level there.

Next gaps, ranked by instruments (round 6 census; Info items and faithful warnings excluded):

| rank | gap | instruments | libraries | reading |
|---:|---|---:|---|---|
| 1 | `effect`: unmodelled Kontakt effect modules | 36 | Conflux, Areia | `effects/mod.rs:1094`: each effect module with no `color`/`timemod`/`dynamics`/`mangling` model. Closing it is DSP RE per module (large) |
| 2 | `wavetable source` not modelled | 16 | Conflux, Morphology | `library.rs:1811`; the WT_MOD path, parked |
| 3 | `DFD preload window not applied to stream head` | 11 | Audio Imperia Dolce | `library.rs:327`; preload sizing for the stream head. No timbre effect, so not a parity gap for output |
| 4 | `Tape Saturator: Creative FX live controls` | 7 | Conflux, Morphology | UI live controls of an effect, not output |
| 5 | `loop crossfade longer than its loop, clamped` | 4 | Audio Imperia CHORUS | `load.rs:751`; the clamp follows Kontakt v1 (`play_map`), faithful |

Not universal: `find_group` misses (Areia, Dolce, faithful: the scripts request groups the
instrument does not have), array index warnings (faithful), `RandomBipolar` and
`get_engine_par_disp_ext` (Conflux only, parked), `load_array` of a `data/` file that the library
does not ship (faithful).

Next universal candidate with output effect: rank 1 (effects), which needs one module measured
at a time. Start with the module behind the Areia rows (the Areia instrument whose `effect`
entry names the module, then one oracle capture of that module on or off).

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
