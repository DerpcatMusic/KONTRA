# Kontakt census, 2026-11-13

All 682 `.nki` under `/data/main/Libraries/Kontakt` (ANALOG STRINGS, Afflatus Brass, Areia, Audio Imperia
Chorus and Dolce, Conflux, Morphology, Pacific Ensemble Strings, Vista, Shreddage, Una Corda, ...) played by
`tools/kontakt-census.py`: one note (key 60, velocity 100, CC1 and CC11 at 127) through `kontra-inspect play`
(corpus build, 3 niced jobs, about 12 minutes). Raw rows: `PluginArtifacts/kontakt-census-run/play.jsonl`
(also `load` only: `rows.jsonl`).

Result: **682 of 682 played, 0 errors, 0 incomplete
renders, 0 script faults. 50 are silent at key 60: all 4 Double Basses / 6Cll+4DblBss patches in Areia (40), 4 Dolce,
4 Pacific Ensemble, 2 Afflatus Slides patches, which are out of range at key 60, not gaps.**

The translator reports every feature it knows it cannot play, so the ranked list is short. Anything it
ignores without a diagnostic is not in this table; that needs the oracle comparison, not a census.

| # | Feature (diagnostic / action) | Instruments | Where | Impact | Audible estimate |
|---|---|---|---|---|---|
| 1 | `unassigned external modulator` | 152 | Areia | Cosmetic | None: a modulator with no source is inert in Kontakt too |
| 2 | `sample_only_referenced_by_purged_groups` (load) | 284 | all | Benign | None: samples no kept group plays |
| 3 | `GetEngineParDisp` unanswered (script reads a display string of an effect-slot par) | 5 | ANALOG STRINGS, Areia Pads and 3 more | Script label | Label text only |
| 4 | `SetEnginePar` unanswered (effect-slot pars `$ENGINE_PAR_RT_*`, `CH_*`, `FL_*`, `PH_*`, `DL_*`, `NOISE*`) | 2 (15 calls) | ANALOG STRINGS, Conflux | Audible if the slot's effect exists | Chorus/flanger/phaser/rotator/delay knobs on the ANALOG GUI do nothing |
| 5 | `silent effect Convolution` | 2 | Areia legacy 16Vlns+10Vls, 6Cll+4DblBsss legato | Cosmetic | None heard (IR missing) |
| 6 | `impulse response size` (early 0.619 / late 1.003) | 1 | ANALOG STRINGS | Audible | Reverb tail length/level differs |
| 7 | `bypassed effect Jump` | 1 | ANALOG STRINGS | Cosmetic | None |
| 8 | `bypassed effect Rotator` | 1 | Areia Pads | Cosmetic | None |
| 9 | `MIDI automation of a missing script control` | 1 | Conflux | Audible | One automated GUI control has no target |
| 10 | `MIDI automation of volume` | 1 | Vista 3 Violins FFF Overlay | Audible | Volume automation ignored |

Only ten distinct items exist, so there is no top 15. Every item above 2 instruments is cosmetic or benign.
The audible ones (4, 6, 9, 10) each touch one or two instruments.
