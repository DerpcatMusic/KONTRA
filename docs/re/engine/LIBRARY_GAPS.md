# Library gaps (full-tier corpus survey, 2026-10-22, main 9c19034+)

Source: `corpus-health run --tier full` over /data/main/Libraries (1395 instruments/programs). Raw: `scratch/survey/full-20261022.jsonl`.
Re-run each merge wave (see docs/library-inspector.md); this file is regenerated, ranked by instruments affected.

| kind | total | loads | sounds |
|---|---|---|---|
| kontakt | 682 | 682 | 667 |
| kontakt-multi | 53 | 53 | 50 |
| uvi-program | 660 | **0** | 0 |

## Ranked gaps

| # | impact | gap | lane |
|---|---|---|---|
| 1 | 660 UVI programs | Translate refuses: "live global ... requires the actual shared owner modulator; source ScriptEventModulation" (every UVI program fails to load) | k-mod-external / UVI mapping |
| 2 | 416 | Convolution: IR latency compensation bypass, IR preserve length unsupported | k-convolution |
| 3 | 349/271/229 | Script runtime: polyphonic variable init, poly read outside note callback, find_group not found | k-ksp-engine-params |
| 4 | 105 | keyswitch script: 2959 articulations found, 0 migrated to zone selectors | k-ksp-engine-params |
| 5 | 93 | keys beyond runtime pitch range narrowed | k-sampler-source |
| 6 | 56 | Filter: filter law unsupported | filt-* lanes |
| 7 | 55 + 51 + 51 | modulation of a module parameter / modulator intensity / target; external RandomBipolar | k-mod-external, k-mod-lfo-step |
| 8 | 52 | set_engine_par, get_engine_par_disp_ext unsupported | k-ksp-engine-params |
| 9 | 39 | unsupported effect | owning FX lane |
| 10 | 32 + 20 | wavetable: needs complete N-frame cycles (zone dropped); wavetable source | k-wavetable-tonemachine |
| 11 | 17 | Freak: Creative FX live controls | k-modfx-freak |
| 12 | 15 | Solid G-EQ smoothing | eq-kontakt |
| 13 | 14 | DFD preload override | k-sampler-source |
| 14 | 10 | Replika parameters | k-delays |
| 15 | 7 | "native Frequency connection attachment and typed parameter cadence unqualified" (Prepare) | k-mod-external |
| 16 | 18 | silent at a covered key | k-voice-engine |
| 17 | 1 | script compile failure | k-ksp-engine-params |
| - | 51 | instruments missing the 64-frame deadline | k-voice-engine |
| - | 0 | MPE probes unexercised (pitch/pressure responses 0 of 0) | k-voice-engine |
