# Functionality gaps: not translated / not playing (full-tier survey 2026-10-22)

Source: `scratch/survey/full-20261022.jsonl` (1395 records: 682 kontakt, 53 kontakt-multi, 660 uvi-program). Mined with `python -I` only; no new survey, render or build beyond one scoped test. Appendix tables below are generated from the same file (record ids are full paths; short names in the tables).

Scope: Kontakt only. UVI translate/load blockers (653 + 7 records) are another lane. Tone/Beat, Replika, Flair and Phasis are out of scope.

## What the survey says (Kontakt, 735 records)

| measure | count | note |
|---|---:|---|
| load ok | 735 / 735 | no Kontakt load failures; all zones load |
| stage `ok` | 718 | includes records with no sound (see silent row) |
| stage `note-on/selection` | 15 | Dolce 10, Pacific 5; all 15 `note suppressed by its script` at the key-60 probe |
| stage `release` / `script-compile` | 1 / 1 | Dolce Bend FX (stored long release); Shreddage E-Upright (`get_ui_id requires a UI control`) |
| quiet or silent (peak < -60 dBFS or no output) | 46 | Dolce 24 of 77, Pacific 18 of 49, Conflux multis 3, Areia Pads 1 |
| `needs_controller` true | 666 | informational, not a failure by itself |

## Ranked root causes

Ranked by instruments affected with silence or missing sound first, then by features that drop or misplay content. Counts are Kontakt instruments (survey `unsupported` keys, normalized by the survey). "Quiet" = overlap with the 46 quiet records.

| # | root cause | instruments | quiet overlap | status at HEAD | failing records (examples) | owner lane | action |
|---:|---|---:|---:|---|---|---|---|
| 1 | Dolce and Pacific ensemble patches silent or near-silent at the probe key 60. 15 show `note suppressed by its script` (selection: 0 voices played); the rest sit at -60 to -90 dBFS with no diagnostics (Pacific Trills: no output at all) | 42 | 42 | **unresolved**; not reproduced here | `Dolce - 07 7 1st Violins - Staccato.nki`, `Dolce - 09 7 1st Violins - Pizzicato.nki`, `Pacific - Ens Strings - 10 Cellos - Trills.nki`, `Pacific - Ens Strings - Solo Harp - Harmonic Pluck.nki` | k-ksp-engine-params, k-voice-engine | Native oracle at each patch's own playable key before any code change. The Pyramid behaviour (Dolce Staccato) remaps key 60 with `change_note($EVENT_ID, ...)` then `ignore_event($EVENT_ID)` in the articulation-switch branch (show_script lines ~68787-68795); the key-60 probe may simply hit a non-playable articulation key. Dolce Legato matches native (`docs/re/NATIVE_ORACLE.md`). |
| 2 | Conflux multis (.nkm) with no output: almost all zones rejected by group state, 2 accepted | 3 | 3 | **unresolved**; stage recorded `ok`, so not in the failure table | `Ray Line.nkm` (5827 of 5855 zones rejected by Group), `Symmetric Mindcontrol.nkm` (5827 of 5855), `Spiral Echoes.nkm` (3981 of 3996) | Conflux lane (group on/off state from script init) | Find which group-enable call the Conflux script makes on init and whether our group state default differs. Needs native comparison. |
| 3 | Keyswitch articulations not switched: `keyswitch script` unsupported; 327 instruments with articulation switches found, 0 migrated to zone selectors | 105 | 0 | live (`crates/sampler-kontakt/src/keyswitch.rs:177`); another agent has a `fix-keyswitch-articulations` worktree | `2 Horns Staccato.nki`, `2 Horns Staccatissimo.nki`, `2 Horns Repetitions.nki` | keyswitch lane | Not touched here. Only the default articulation is reachable for the unrecognised patterns. |
| 4 | Script engine-parameter and modulation writes dropped: `set_engine_par` (52), `get_engine_par_disp_ext` (51), `modulation of a module parameter` (55), `modulation target` (51), `modulation of a modulator's intensity` (51), `external modulation from RandomBipolar` (51) | 56 unique | 3 | live (`library.rs:2187`, `library.rs:2230`, `sampler-ksp` engine-par path) | `2 Horns KS.nki` (1131 dropped writes to `$ENGINE_PAR_MOD_TARGET_INTENSITY`, `InvalidInput`), `Conflux.nki`, `A Forgotten Romance.nkm` | k-ksp-engine-params, k-mod-external | Large; not touched. Writes are dropped, not faulted, so the notes still sound. |
| 5 | Zones narrowed out of the resampler window: `keys beyond the runtime's pitch range, narrowed` | 93 | 5 | live (`load.rs` `fit_keys`, window = `PITCH_STEPS` = 1/256..256, i.e. about +/-8 octaves, adjusted by asset/output rate) | `Areia - 03 6 Celli - Legato.nki`, `Areia - 04 4 Double Basses - Legato.nki`, `04 Areia - 4 Double Basses - Core Techniques.nki` | k-sampler-source (resampler range) | Only the extreme keys of a zone are lost. Widening the step table is a DSP change; not touched. |
| 6 | Wavetable zones dropped by the loader when the length is not whole 2048-frame cycles | 32 | 1 | **fixed in this PR**. The engine already floors to whole cycles (`sampler-core/src/wavetable.rs`, commit 60a1d6285); the loader still dropped the zone | `Conflux.nki`, `A Forgotten Romance.nkm`, `Adios Horner.nkm` | k-wavetable-tonemachine | Done: `wavetable_cycles_admitted` in `load.rs`. Misaligned starts and sub-cycle zones still dropped. `wavetable source` (20 instruments, unsupported source mode) remains. |
| 7 | Effects and filter laws not translated: Convolution IR latency bypass and preserve length (418 instruments), `effect` (39), `Filter: filter law` (56), Freak live controls (17), Solid G-EQ smoothing (15), DFD preload (14), Tape Saturator (10), Solid Bus Comp (9), Wow/Flutter (9), Raum feedback limiter (7), Bite (7) | 431 unique | 4 | live | `Areia - 05 16Vlns+10Vls 8va - Legato.nki`, `Areia - 01 Pads.nki`, `2 Horns KS.nki` | FX / filter lanes | Not touched. Largest by count is Convolution (416 features), but most of those instruments sound. |
| 8 | Script warnings, mostly benign: `find_group` not found (229), `polyphonic variables start at N; initializer ignored` (349), `polyphonic variable read outside a note callback is 0` (271), `array index N out of bounds` (134) | 623 unique | 24 (poly read and find_group, all in the Dolce quiet set) | mixed, see notes | `Dolce - 05 7 1st Violins - Spiccato.nki` (quiet), `2 Horns KS.nki` (init warning only) | ksp runtime | See notes below. Only the poly-read warning is a candidate for the Dolce silence; it is unverified. |
| 9 | `load_array: data/<file>.nka not loaded` (mode 2) | 51 | 3 | live (`sampler-ksp/src/eval.rs:2140`; loader looks only in `Resources/data/<stem>.nka`, `load.rs` `data_arrays`) | `Conflux.nki`, `A Forgotten Romance.nkm`, `Adios Horner.nkm` | k-ksp-engine-params / loader | Library `.nka` files sit under `Library Data/...` (716 `.nka` under `/data/main/Libraries/Kontakt`). The path rule for mode 2 is unverified; not touched. |
| 10 | Script does not compile: `get_ui_id requires a UI control` | 1 | 0 | live (sema) | `Shreddage 3.5 E-Upright.nki` | sampler-ksp | Instrument loads and sounds (-17 dBFS) without its script. |
| - | Survey tooling: `ok` stage with no output (Conflux, Areia Pads) and quiet records are not counted as failures | - | - | open | see appendix A | corpus-health | Add a stage for `peak None` and `< -60 dBFS`. |
| - | UVI translate-to-IR (653) and load (7) | 660 | 0 | skipped | UVI records | UVI lane | Out of scope. |

## Notes on the script warnings (row 8)

- `find_group: group not found` is benign for the Dolce Staccato record: the script only checks sibling patch groups, e.g. `if (find_group("10VlnsChnswStbsBr1FMpDyn1RR1")>0)` (show_script lines 18820 and 18855, behaviour `Pyramid (Tonal and Atonal)`). Our constant-name lookup (`sampler-ksp/src/lower.rs` ~2270) warns with the Kontakt wording `find_group(): Group "..." was not found. Index will be invalid!`. The survey string is older than HEAD, so the 229 count is stale in wording (the count itself may still hold).
- `polyphonic variable read outside a note callback is 0` (`lower.rs:1093`): reads from callbacks other than note or release return 0. The 24 Dolce quiet records have this and `find_group`. Whether Kontakt returns 0 there is unverified. This is the first thing to check with the native oracle, together with row 1.
- `polyphonic variables start at N; initializer ignored` (349) occurs in 0 quiet instruments and matches a Kontakt rule. Not a gap.
- `array index N out of bounds ... read returns zero` (134): the script reads out of range, which Kontakt also reports. Not treated as a gap without a native read.

## Survey staleness

- Wavetable zone-dropped (row 6) is fixed in this PR for the loader. The engine change came earlier (60a1d6285, PR #209).
- The `find_group` wording changed after the survey ran. A re-run is needed before the 229 count is trusted. No re-run was done for this task.

## Fixed in this PR

- `crates/sampler-kontakt/src/load.rs`: `wavetable_cycles_admitted(start, end)` replaces the whole-cycle length check. Test `wavetable_admits_partial_trailing_cycle_but_not_misaligned_start`. Scoped test passed: `build-slot cargo test -p sampler-kontakt --lib wavetable_admits`.
- `docs/re/engine/k-wavetable-tonemachine.md`: edge-case line updated to the floored-cycle rule.

Not run: no survey, no render, no Kontakt oracle session, no workspace build.

## Appendix A: quiet or silent Kontakt records

Generated from the survey. `peak` is dBFS of the first note; `why` is the survey's `why_silent`; `verdicts` are the selection verdict counts at the probe key.

| record | zones | key | peak dBFS | why | selection verdicts |
|---|---:|---:|---:|---|---|
| `Areia 1.2.0 [Audio Imperia]/Instruments/06 Sound Design/Areia - 01 Pads.nki` | 20 | 60 | -62.9 | - | - |
| `Audio Imperia Dolce/Instruments/01 7 1st Violins/Dolce - 05 7 1st Violins - Spiccato.nki` | 384 | 60 | -78.4 | - | - |
| `Audio Imperia Dolce/Instruments/01 7 1st Violins/Dolce - 06 7 1st Violins - Energetic Shorts.nki` | 192 | 60 | -74.5 | - | - |
| `Audio Imperia Dolce/Instruments/01 7 1st Violins/Dolce - 07 7 1st Violins - Staccato.nki` | 384 | 60 | -83.0 | key 60: note suppressed by its script | Group 126, Velocity 240, accepted 18 |
| `Audio Imperia Dolce/Instruments/01 7 1st Violins/Dolce - 09 7 1st Violins - Pizzicato.nki` | 252 | 60 | -90.0 | key 60: note suppressed by its script | Group 72, Velocity 168, accepted 12 |
| `Audio Imperia Dolce/Instruments/02 5 2nd Violins/Dolce - 05 5 2nd Violins - Spiccato.nki` | 384 | 60 | -82.0 | key 60: note suppressed by its script | Group 126, Velocity 240, accepted 18 |
| `Audio Imperia Dolce/Instruments/02 5 2nd Violins/Dolce - 06 5 2nd Violins - Energetic Shorts.nki` | 240 | 60 | -79.5 | - | - |
| `Audio Imperia Dolce/Instruments/02 5 2nd Violins/Dolce - 08 5 2nd Violins - Fast Staccato.nki` | 144 | 60 | -78.2 | - | - |
| `Audio Imperia Dolce/Instruments/02 5 2nd Violins/Dolce - 07 5 2nd Violins - Staccato.nki` | 288 | 60 | -88.0 | key 60: note suppressed by its script | Group 42, Velocity 240, accepted 6 |
| `Audio Imperia Dolce/Instruments/02 5 2nd Violins/Dolce - 09 5 2nd Violins - Frog Staccato FFF.nki` | 48 | 60 | -70.0 | - | - |
| `Audio Imperia Dolce/Instruments/02 5 2nd Violins/Dolce - 11 5 2nd Violins - Pizzicato.nki` | 252 | 60 | -90.5 | key 60: note suppressed by its script | Group 72, Velocity 168, accepted 12 |
| `Audio Imperia Dolce/Instruments/03 4 Violas/Dolce - 04 4 Violas - Harmonics.nki` | 36 | 60 | -64.9 | - | - |
| `Audio Imperia Dolce/Instruments/03 4 Violas/Dolce - 05 4 Violas - Spiccato.nki` | 384 | 60 | -81.9 | key 60: note suppressed by its script | Group 84, Velocity 288, accepted 12 |
| `Audio Imperia Dolce/Instruments/03 4 Violas/Dolce - 06 4 Violas - Staccato.nki` | 384 | 60 | -76.7 | - | - |
| `Audio Imperia Dolce/Instruments/03 4 Violas/Dolce - 08 4 Violas - Pizzicato.nki` | 336 | 60 | -76.6 | - | - |
| `Audio Imperia Dolce/Instruments/04 3 Celli/Dolce - 05 3 Celli - Spiccato.nki` | 384 | 60 | -82.8 | key 60: note suppressed by its script | Group 84, Velocity 288, accepted 12 |
| `Audio Imperia Dolce/Instruments/04 3 Celli/Dolce - 06 3 Celli - Energetic Shorts.nki` | 192 | 60 | -84.1 | key 60: note suppressed by its script | Group 84, Velocity 96, accepted 12 |
| `Audio Imperia Dolce/Instruments/04 3 Celli/Dolce - 07 3 Celli - Staccato.nki` | 288 | 60 | -74.0 | - | - |
| `Audio Imperia Dolce/Instruments/04 3 Celli/Dolce - 09 3 Celli - Pizzicato.nki` | 294 | 60 | -79.7 | - | - |
| `Audio Imperia Dolce/Instruments/05 3 Basses/Dolce - 05 3 Basses - Spiccato.nki` | 192 | 60 | -75.7 | - | - |
| `Audio Imperia Dolce/Instruments/05 3 Basses/Dolce - 08 3 Basses - Pizzicato.nki` | 294 | 60 | -77.9 | - | - |
| `Audio Imperia Dolce/Instruments/05 3 Basses/Dolce - 06 3 Basses - Staccato.nki` | 384 | 60 | -70.1 | - | - |
| `Audio Imperia Dolce/Instruments/06 Ensemble Patches/Dolce - 04 Ensemble - Spiccato.nki` | 240 | 60 | -81.2 | key 60: note suppressed by its script | Group 126, Velocity 96, accepted 18 |
| `Audio Imperia Dolce/Instruments/06 Ensemble Patches/Dolce - 05 Ensemble - Staccato.nki` | 240 | 60 | -80.7 | key 60: note suppressed by its script | Group 126, Velocity 96, accepted 18 |
| `Audio Imperia Dolce/Instruments/06 Ensemble Patches/Dolce - 07 Ensemble - Pizzicato.nki` | 210 | 60 | -79.4 | - | - |
| `Conflux 1.1.0 [Native Instruments]/Multis/Ray Line.nkm` | 5855 | 60 | none | - | Group 5827, accepted 2 |
| `Conflux 1.1.0 [Native Instruments]/Multis/Symmetric Mindcontrol.nkm` | 5855 | 60 | none | - | Group 5827, accepted 2 |
| `Conflux 1.1.0 [Native Instruments]/Multis/Spiral Echoes.nkm` | 3996 | 60 | none | - | Group 3981, accepted 2 |
| `Pacific Ensemble Strings/Instruments/10 Cellos/Pacific - Ens Strings - 10 Cellos - Sordino Sustains.nki` | 28 | 60 | -72.1 | - | - |
| `Pacific Ensemble Strings/Instruments/10 Cellos/Pacific - Ens Strings - 10 Cellos - Tremolos.nki` | 48 | 60 | -72.5 | - | - |
| `Pacific Ensemble Strings/Instruments/10 Cellos/Pacific - Ens Strings - 10 Cellos - Trills.nki` | 48 | 60 | none | key 60: note suppressed by its script | - |
| `Pacific Ensemble Strings/Instruments/12 Violas/Pacific - Ens Strings - 12 Violas - Sordino Sustains.nki` | 36 | 60 | -69.9 | - | - |
| `Pacific Ensemble Strings/Instruments/12 Violas/Pacific - Ens Strings - 12 Violas - Tremolos.nki` | 54 | 60 | -71.7 | - | - |
| `Pacific Ensemble Strings/Instruments/12 Violas/Pacific - Ens Strings - 12 Violas - Trills.nki` | 64 | 60 | none | key 60: note suppressed by its script | - |
| `Pacific Ensemble Strings/Instruments/16 Violins/Pacific - Ens Strings - 16 Violins - Sordino Sustains.nki` | 36 | 60 | -64.3 | - | - |
| `Pacific Ensemble Strings/Instruments/16 Violins/Pacific - Ens Strings - 16 Violins - Tremolos.nki` | 48 | 60 | -72.0 | - | - |
| `Pacific Ensemble Strings/Instruments/16 Violins/Pacific - Ens Strings - 16 Violins - Trills.nki` | 48 | 60 | none | key 60: note suppressed by its script | - |
| `Pacific Ensemble Strings/Instruments/8 Basses/Pacific - Ens Strings - 8 Basses - Tremolos.nki` | 48 | 60 | -67.5 | - | - |
| `Pacific Ensemble Strings/Instruments/8 Basses/Pacific - Ens Strings - 8 Basses - Sordino Sustains.nki` | 44 | 60 | -75.9 | - | - |
| `Pacific Ensemble Strings/Instruments/Bonus Content/Lite Full Strings Patches/Pacific - Ens Strings - Lite Full Str - Sordino Sustains.nki` | 36 | 60 | -65.3 | - | - |
| `Pacific Ensemble Strings/Instruments/Bonus Content/Lite Full Strings Patches/Pacific - Ens Strings - Lite Full Str - Tremolos.nki` | 36 | 60 | -70.9 | - | - |
| `Pacific Ensemble Strings/Instruments/Bonus Content/Lite Full Strings Patches/Pacific - Ens Strings - Lite Full Str - Sustains.nki` | 20 | 60 | -60.3 | - | - |
| `Pacific Ensemble Strings/Instruments/Bonus Content/Lite Full Strings Patches/Pacific - Ens Strings - Lite Full Str - Whisper Sustains.nki` | 36 | 60 | -66.9 | - | - |
| `Pacific Ensemble Strings/Instruments/Bonus Content/Lite Full Strings Patches/Pacific - Ens Strings - Lite Full Str - Trills.nki` | 72 | 60 | none | key 60: note suppressed by its script | - |
| `Pacific Ensemble Strings/Instruments/Bonus Content/Solo Harp/Pacific - Ens Strings - Solo Harp - Harmonic Pluck.nki` | 80 | 60 | -81.6 | key 60: note suppressed by its script | Group 46, Trigger 48, Velocity 144, accepted 2 |
| `Pacific Ensemble Strings/Instruments/Bonus Content/Solo Harp/Pacific - Ens Strings - Solo Harp - Normale Pluck.nki` | 160 | 60 | -75.7 | - | - |

## Appendix B: wavetable zone-dropped records (row 6)

| record | wavetable feature (survey) |
|---|---|
| `Conflux 1.1.0 [Native Instruments]/Instruments/Conflux.nki` | wavetable requires complete N-frame cycles, zone dropped |
| `Conflux 1.1.0 [Native Instruments]/Multis/A Forgotten Romance.nkm` | wavetable requires complete N-frame cycles, zone dropped |
| `Conflux 1.1.0 [Native Instruments]/Multis/Adios Horner.nkm` | wavetable requires complete N-frame cycles, zone dropped |
| `Conflux 1.1.0 [Native Instruments]/Multis/Airy Soul.nkm` | wavetable requires complete N-frame cycles, zone dropped |
| `Conflux 1.1.0 [Native Instruments]/Multis/Alpha Flow.nkm` | wavetable requires complete N-frame cycles, zone dropped |
| `Conflux 1.1.0 [Native Instruments]/Multis/Collapse Mallet.nkm` | wavetable requires complete N-frame cycles, zone dropped |
| `Conflux 1.1.0 [Native Instruments]/Multis/Dance Temple.nkm` | wavetable requires complete N-frame cycles, zone dropped |
| `Conflux 1.1.0 [Native Instruments]/Multis/Fire Heat.nkm` | wavetable requires complete N-frame cycles, zone dropped |
| `Conflux 1.1.0 [Native Instruments]/Multis/Hybrid Walk.nkm` | wavetable requires complete N-frame cycles, zone dropped |
| `Conflux 1.1.0 [Native Instruments]/Multis/Ivory Down.nkm` | wavetable requires complete N-frame cycles, zone dropped |
| `Conflux 1.1.0 [Native Instruments]/Multis/Love Memory.nkm` | wavetable requires complete N-frame cycles, zone dropped |
| `Conflux 1.1.0 [Native Instruments]/Multis/Mantel Sticks.nkm` | wavetable requires complete N-frame cycles, zone dropped |
| `Conflux 1.1.0 [Native Instruments]/Multis/Modular Fantasy.nkm` | wavetable requires complete N-frame cycles, zone dropped |
| `Conflux 1.1.0 [Native Instruments]/Multis/Mystic Chops.nkm` | wavetable requires complete N-frame cycles, zone dropped |
| `Conflux 1.1.0 [Native Instruments]/Multis/New Romantics.nkm` | wavetable requires complete N-frame cycles, zone dropped |
| `Conflux 1.1.0 [Native Instruments]/Multis/Online Quote.nkm` | wavetable requires complete N-frame cycles, zone dropped |
| `Conflux 1.1.0 [Native Instruments]/Multis/Other Worlds.nkm` | wavetable requires complete N-frame cycles, zone dropped |
| `Conflux 1.1.0 [Native Instruments]/Multis/Perusing Wheels.nkm` | wavetable requires complete N-frame cycles, zone dropped |
| `Conflux 1.1.0 [Native Instruments]/Multis/Planet Garden.nkm` | wavetable requires complete N-frame cycles, zone dropped |
| `Conflux 1.1.0 [Native Instruments]/Multis/Pop Ivory.nkm` | wavetable requires complete N-frame cycles, zone dropped |
| `Conflux 1.1.0 [Native Instruments]/Multis/Rainy Moondays.nkm` | wavetable requires complete N-frame cycles, zone dropped |
| `Conflux 1.1.0 [Native Instruments]/Multis/Royal Ride.nkm` | wavetable requires complete N-frame cycles, zone dropped |
| `Conflux 1.1.0 [Native Instruments]/Multis/Sad Wolves.nkm` | wavetable requires complete N-frame cycles, zone dropped |
| `Conflux 1.1.0 [Native Instruments]/Multis/Scape City.nkm` | wavetable requires complete N-frame cycles, zone dropped |
| `Conflux 1.1.0 [Native Instruments]/Multis/Shadow Spaces.nkm` | wavetable requires complete N-frame cycles, zone dropped |
| `Conflux 1.1.0 [Native Instruments]/Multis/Silver Winter.nkm` | wavetable requires complete N-frame cycles, zone dropped |
| `Conflux 1.1.0 [Native Instruments]/Multis/Tension Time.nkm` | wavetable requires complete N-frame cycles, zone dropped |
| `Conflux 1.1.0 [Native Instruments]/Multis/Time Crepuscle.nkm` | wavetable requires complete N-frame cycles, zone dropped |
| `Conflux 1.1.0 [Native Instruments]/Multis/Spiral Echoes.nkm` | wavetable requires complete N-frame cycles, zone dropped |
| `Conflux 1.1.0 [Native Instruments]/Multis/Time Hunt.nkm` | wavetable requires complete N-frame cycles, zone dropped |
| `Conflux 1.1.0 [Native Instruments]/Multis/Warning.nkm` | wavetable requires complete N-frame cycles, zone dropped |
| `Conflux 1.1.0 [Native Instruments]/Multis/Yield Mono.nkm` | wavetable requires complete N-frame cycles, zone dropped |

## Appendix C: stage not `ok` (Kontakt)

| record | stage | load ok | zones |
|---|---|---|---:|
| `Audio Imperia Dolce/Instruments/01 7 1st Violins/Dolce - 07 7 1st Violins - Staccato.nki` | note-on/selection | True | 384 |
| `Audio Imperia Dolce/Instruments/01 7 1st Violins/Dolce - 09 7 1st Violins - Pizzicato.nki` | note-on/selection | True | 252 |
| `Audio Imperia Dolce/Instruments/02 5 2nd Violins/Dolce - 05 5 2nd Violins - Spiccato.nki` | note-on/selection | True | 384 |
| `Audio Imperia Dolce/Instruments/02 5 2nd Violins/Dolce - 07 5 2nd Violins - Staccato.nki` | note-on/selection | True | 288 |
| `Audio Imperia Dolce/Instruments/02 5 2nd Violins/Dolce - 11 5 2nd Violins - Pizzicato.nki` | note-on/selection | True | 252 |
| `Audio Imperia Dolce/Instruments/03 4 Violas/Dolce - 05 4 Violas - Spiccato.nki` | note-on/selection | True | 384 |
| `Audio Imperia Dolce/Instruments/04 3 Celli/Dolce - 05 3 Celli - Spiccato.nki` | note-on/selection | True | 384 |
| `Audio Imperia Dolce/Instruments/04 3 Celli/Dolce - 06 3 Celli - Energetic Shorts.nki` | note-on/selection | True | 192 |
| `Audio Imperia Dolce/Instruments/05 3 Basses/Dolce - 13 3 Basses - Bend FX.nki` | release | True | 24 |
| `Audio Imperia Dolce/Instruments/06 Ensemble Patches/Dolce - 04 Ensemble - Spiccato.nki` | note-on/selection | True | 240 |
| `Audio Imperia Dolce/Instruments/06 Ensemble Patches/Dolce - 05 Ensemble - Staccato.nki` | note-on/selection | True | 240 |
| `Pacific Ensemble Strings/Instruments/10 Cellos/Pacific - Ens Strings - 10 Cellos - Trills.nki` | note-on/selection | True | 48 |
| `Pacific Ensemble Strings/Instruments/12 Violas/Pacific - Ens Strings - 12 Violas - Trills.nki` | note-on/selection | True | 64 |
| `Pacific Ensemble Strings/Instruments/16 Violins/Pacific - Ens Strings - 16 Violins - Trills.nki` | note-on/selection | True | 48 |
| `Pacific Ensemble Strings/Instruments/Bonus Content/Lite Full Strings Patches/Pacific - Ens Strings - Lite Full Str - Trills.nki` | note-on/selection | True | 72 |
| `Pacific Ensemble Strings/Instruments/Bonus Content/Solo Harp/Pacific - Ens Strings - Solo Harp - Harmonic Pluck.nki` | note-on/selection | True | 80 |
| `Shreddage 3.5 E-Upright 1.0.1 [Impact Soundworks] rutracker.org/Instruments/Shreddage 3.5 E-Upright.nki` | script-compile | True | 892 |
