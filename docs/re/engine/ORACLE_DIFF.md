# Kontakt vs KONTRA: audio oracle sample run (2026-10-23)

Measured with `tools/oracle/` (capture, kontra_render, compare). Kontakt 8.13.1 standalone under Wine,
output to the PipeWire null sink `kontra_ref`; KONTRA `sampler-native render-kontakt-midi` on the same
notes. Sample: 101 NKIs from `/data/main/Libraries/Kontakt` (Afflatus Chapter II Brass 30, Areia 1.2.0
25, Dolce 15, Pacific Ensemble Strings 12, Audio Imperia CHORUS 8, Performance Samples Vista 4, Una Corda 3,
ANALOG STRINGS, Conflux, Morphology Evolved, Shreddage 3.5 E-Upright, 1 each). Scenario: 3 notes (keys
60/64/67, velocities 100/90/110, starts 0.2/1.6/3.0 s, 1.0-1.5 s long), 6 s window.

## Result

- 101 instruments: 91 have audio on both sides and are compared. 10 are excluded:
  - 6 silent on BOTH sides (Areia 06 4 Double Basses Pizzicato, 12 4 Double Basses Sul Tasto, 05/11 6Cll 4DblBsss 8va,
    Pacific 10 Cellos Trills, 16 Violins Trills). The scenario does not trigger them (keyswitch or articulation,
    or key range). Not a parity result; they need articulation-specific scenarios.
  - 2 silent in KONTRA only, audible in Kontakt (-34 and -38 dBFS): **Areia 06 6Cll 4DblBsss 8va Legato** and
    **Areia 01 4 Double Basses Legato Fingered**. This is a real mismatch (KONTRA renders nothing). Top priority.
  - 2 Kontakt load failures: Conflux (the NI activation is missing in this Kontakt, as recorded in
    reference-native-oracle) and Shreddage 3.5 E-Upright (not installed or licensed in this Wine prefix).
- Across the 91 compared (median over instruments, deviation from the median):
  - Level: median Kontakt minus KONTRA = -2.1 dB (uniform offset; `calibrate` in tools/kontakt-reference was not
    run here). Per-instrument deviation: 33 instruments beyond +-6 dB; 86 beyond +-1 dB.
  - Timbre (1/3-octave residual after the level offset): median 2.3 dB; 13 instruments beyond 6 dB. The worst are
    Morphology Evolved, 3 Trombones Mutes Performance and Trumpets Trills (12-18 dB).
  - Waveform correlation after alignment: median r = 0.46; 47 instruments below 0.5. That points at the sample,
    articulation or round-robin choice. One 3-note scenario cannot separate those three; a per-articulation scenario
    and RR sweep is the next step.
  - Timing: median deviation 0 ms, IQR -60..+10 ms. Deviations under about 30 ms are noise for legato and trill
    material (envelope cross-correlation). 11 alignments hit the edge of the search and are marked "alignment unreliable".
- Runtime: about 25 s per instrument amortised (the run took about 40 min for 101), dominated by Kontakt loading (Afflatus and Areia are large) and
  the GUI device select. The "few seconds per capture" target is not met; batching 16 instruments per session is the
  main lever (in place).

## Known issues in this run (read before using the numbers)

1. **Capture clock origin bug (fixed in this PR).** The MIDI clock started after recorder start-up, so every slice
   began about 1.5 s early. The absolute median lag of +1530 ms is that artifact, not Kontakt latency. Per-instrument
   deviations are unaffected because the offset is constant. The batch was captured with the pre-fix code; absolute
   latency needs a re-run.
2. The compared overlap is about 4.5 s, not 6 s: the alignment shifts Kontakt by about 1.5 s, and the slices hold
   6.5 s from the slot start.
3. Kontakt's audio device choice does not persist in Settings.cfg (the `AB2 AudioDevice` line is ignored at start);
   the Options dialog is driven by GUI clicks each session. I restored the original `AB2 AudioDevice=... MOTU Mic` line
   after the runs. The `kontra_ref` OutputChannel entries that were already in Settings.cfg are unchanged.
4. **Audio routing.** PipeWire auto-links Kontakt's outputs to the default output chain (`jamesdsp_sink` to the MOTU
   speakers). During one debugging run (about 30 s), Kontakt's outputs were linked to `jamesdsp_sink`. I believe
   Kontakt was silent in that run (its session recording held no Kontakt signal), but that is not verified. The capture now pins the links to
   `kontra_ref` continuously. The default output was not changed.
5. Round-robin and articulation are not identified by this scenario. The waveform numbers are indicators, not verdicts.
6. The comparer is numpy-only, and the timing metric is envelope-based (see above).

## How to run

    tools/oracle/capture NKI MIDI OUT.wav [WINDOW]          # one instrument, MIDI file as given
    tools/oracle/capture.py --batch LIST.tsv OUTDIR         # name<TAB>nki per line; 16 per Kontakt session
    tools/oracle/kontra_render.py LIST.tsv OUTDIR           # same notes through KONTRA
    tools/oracle/compare.py CAPTURE_DIR KONTRA_DIR --md ORACLE_DIFF.md --json ORACLE_DIFF.json

Requires a Kontakt 8 install under Wine with `DeveloperFeatures=dw:1` (see `tools/kontakt-oracle/dump.py`),
the `kontra_ref` PipeWire null sink (created when missing), `aplaymidi`, `xdotool` and `Xvfb`. `sampler-native`
is found under the KONTRA cargo target root, or set `SAMPLER_NATIVE`.

## Ranked mismatch table (generated by `tools/oracle/compare.py`)

Sorted by severity: `|level dev|/1 dB + timbre/3 dB + |timing dev|/10 ms + 5*(1-r)`. Timing and waveform
terms are dropped for "alignment unreliable" rows. Silent rows are at the end of the list.

| # | instrument | level dev dB | timing dev ms | timbre dB | waveform r | flags | severity |
|---|---|---:|---:|---:|---:|---|---:|
| 1 | 008_3_Trumpets_Trills | +24.03 | +30.0 | 12.35 | 0.112 | level, timbre, timing, sample/articulation/RR suspected | 35.59 |
| 2 | 010_4_Horns_Mutes_Legato | -17.81 | +20.0 | 16.41 | 0.431 | level, timbre, timing, sample/articulation/RR suspected | 28.12 |
| 3 | 062_Chorus___01_Ensemble___Legato_Ah | -9.62 | -140.0 | 4.16 | 0.656 | level, timbre, timing, sample/articulation/RR suspected | 26.73 |
| 4 | 060_Chorus___01_Women___Legato_Ah | -8.34 | -150.0 | 3.69 | 0.613 | level, timbre, timing, sample/articulation/RR suspected | 26.51 |
| 5 | 093_Vista___3_Violins_FFF_Overlay | +1.05 | +200.0 | 1.81 | 0.118 | level, timing, sample/articulation/RR suspected | 26.06 |
| 6 | 094_Vista___Full_Strings_Sustains | -0.92 | +200.0 | 1.80 | 0.126 | timing, sample/articulation/RR suspected | 25.89 |
| 7 | 056_01_Chorus___Women___Traditional_Articulations | -8.31 | -130.0 | 3.71 | 0.620 | level, timbre, timing, sample/articulation/RR suspected | 24.45 |
| 8 | 027_Imperial_Brass_B | +9.22 | -70.0 | 10.27 | 0.122 | level, timbre, timing, sample/articulation/RR suspected | 24.03 |
| 9 | 001_2_Horns_KS | +13.10 | +10.0 | 20.03 | 0.539 | level, timbre, sample/articulation/RR suspected | 23.08 |
| 10 | 058_Chorus___01_Men___Legato_Ah | -9.86 | -70.0 | 3.82 | 0.083 | level, timbre, timing, sample/articulation/RR suspected | 22.71 |
| 11 | 028_Mega_Brass | +11.57 | +20.0 | 13.77 | 0.216 | level, timbre, timing, sample/articulation/RR suspected | 22.08 |
| 12 | 022_Solo_Tenor_Trombone_Flutter | +5.42 | -60.0 | 18.37 | 0.155 | level, timbre, timing, sample/articulation/RR suspected | 21.76 |
| 13 | 063_Chorus___06_Ensemble___Sustained_Mmh | -6.23 | -130.0 | 3.15 | 0.708 | level, timbre, timing, start offset | 21.75 |
| 14 | 057_05_Chorus___Men___Traditional_Articulations | -9.83 | -60.0 | 3.74 | 0.110 | level, timbre, timing, sample/articulation/RR suspected | 21.53 |
| 15 | 061_Chorus___06_Women___Sustained_Mmh | -5.53 | -130.0 | 2.92 | 0.698 | level, timing, sample/articulation/RR suspected | 21.01 |
| 16 | 013_Solo_Trumpet_Metal_Mute_KS | +12.32 | +0.0 | 11.60 | 0.237 | level, timbre, sample/articulation/RR suspected | 20.01 |
| 17 | 026_Solo_Tuba_Legato | +9.64 | -60.0 | 5.48 | 0.911 | level, timbre, timing, start offset | 17.91 |
| 18 | 030_Solo_Cimbasso_Performance | +15.26 | -300.0 | 7.32 | 0.395 | level, timbre, alignment unreliable | 17.70 |
| 19 | 050_Areia___01_Full_Ens___Sustained | -3.96 | -120.0 | 1.59 | 0.818 | level, timing, start offset | 17.40 |
| 20 | 070_Dolce___03_4_Violas___Sustained_Con_Sordino | -9.24 | +40.0 | 4.26 | 0.456 | level, timbre, timing, sample/articulation/RR suspected | 17.38 |
| 21 | 002_4_Horns_Stopped_KS | +4.43 | +20.0 | 19.83 | 0.290 | level, timbre, timing, sample/articulation/RR suspected | 16.59 |
| 22 | 075_Dolce___04_3_Basses___Harmonics | -8.49 | +60.0 | 2.29 | 0.773 | level, timing, start offset | 16.39 |
| 23 | 037_Areia___01_10_Violas___Sustained | -3.32 | +90.0 | 1.50 | 0.297 | level, timing, sample/articulation/RR suspected | 16.33 |
| 24 | 047_Areia___15_16Vlns___10Vls_8va___Bartok_Pizzicato | -3.36 | -80.0 | 3.03 | 0.367 | level, timbre, timing, sample/articulation/RR suspected | 15.53 |
| 25 | 052_Areia___14_Full_Ens___Portato | -4.24 | -70.0 | 1.78 | 0.300 | level, timing, sample/articulation/RR suspected | 15.33 |
| 26 | 045_Areia___02_16Vlns___10Vls_8va___Sustained_Con_Sordino | -4.32 | +60.0 | 1.99 | 0.134 | level, timing, sample/articulation/RR suspected | 15.31 |
| 27 | 068_Dolce___08_5_2nd_Violins___Fast_Staccato | -9.91 | +0.0 | 2.45 | 0.198 | level, sample/articulation/RR suspected | 14.74 |
| 28 | 065_Dolce___06_7_1st_Violins___Energetic_Shorts | -9.85 | +0.0 | 3.89 | 0.303 | level, timbre, sample/articulation/RR suspected | 14.63 |
| 29 | 073_Dolce___07_3_Celli___Staccato | -10.82 | +0.0 | 3.38 | 0.469 | level, timbre, sample/articulation/RR suspected | 14.60 |
| 30 | 009_4_Horns_Flutter | +2.63 | +10.0 | 20.67 | 0.215 | level, timbre, sample/articulation/RR suspected | 14.45 |
| 31 | 071_Dolce___08_4_Violas___Pizzicato | -11.25 | +0.0 | 2.86 | 0.573 | level, sample/articulation/RR suspected | 14.34 |
| 32 | 078_Dolce___06_Ensemble___Marcato | -9.62 | +30.0 | 2.38 | 0.816 | level, timing, start offset | 14.34 |
| 33 | 077_Dolce___01_Ensemble___Sustained | -9.18 | +0.0 | 2.76 | 0.210 | level, sample/articulation/RR suspected | 14.05 |
| 34 | 089_Pacific___Ens_Strings___8_Basses___Legato_Sustains | +13.27 | +300.0 | 2.36 | 0.114 | level, alignment unreliable | 14.05 |
| 35 | 072_Dolce___02_3_Celli___Sustained | -9.04 | +30.0 | 2.64 | 0.801 | level, timing, start offset | 13.92 |
| 36 | 040_Areia___03_6_Celli___Spiccato_Fast | -5.56 | +40.0 | 2.44 | 0.412 | level, timing, sample/articulation/RR suspected | 13.31 |
| 37 | 074_Dolce___12_3_Celli___Trill_HT | -8.87 | -20.0 | 2.26 | 0.720 | level, timing, start offset | 13.03 |
| 38 | 055_Areia___04_4_Double_Basses___Perf | -2.74 | -90.0 | 2.24 | 0.916 | level, timing, start offset | 12.91 |
| 39 | 069_Dolce___13_5_2nd_Violins___Tremolo_Sul_Pont | -9.02 | -10.0 | 2.24 | 0.694 | level, sample/articulation/RR suspected | 12.30 |
| 40 | 035_Areia___04_16_Violins___Spiccato_Slow | -3.73 | +40.0 | 4.73 | 0.403 | level, timbre, timing, sample/articulation/RR suspected | 12.29 |
| 41 | 017_Solo_Cimbasso_Mute_AccSusCresc | +10.24 | +300.0 | 5.94 | 0.155 | level, timbre, alignment unreliable | 12.22 |
| 42 | 080_Morphology_Evolved | +6.77 | -300.0 | 15.72 | 0.098 | level, timbre, alignment unreliable | 12.01 |
| 43 | 066_Dolce___11_7_1st_Violins___Tremolo_Sul_Pont | -8.89 | -10.0 | 3.06 | 0.779 | level, timbre, start offset | 12.01 |
| 44 | 059_Chorus___06_Men___Sustained_Mmh | -6.65 | -10.0 | 2.70 | 0.362 | level, sample/articulation/RR suspected | 11.74 |
| 45 | 004_2_Horns_Stopped_Flutter | +0.48 | +20.0 | 15.52 | 0.271 | timbre, timing, sample/articulation/RR suspected | 11.30 |
| 46 | 051_Areia___08_Full_Ens___Marcato_Long | -3.86 | -60.0 | 1.68 | 0.836 | level, timing, start offset | 11.24 |
| 47 | 024_Solo_Trumpet_Metal_Mute_Staccato | +2.21 | -70.0 | 0.17 | 0.684 | level, timing, sample/articulation/RR suspected | 10.85 |
| 48 | 067_Dolce___03_5_2nd_Violins___Sustained_Con_Sordino | -9.32 | +0.0 | 2.52 | 0.902 | level | 10.65 |
| 49 | 034_Areia___02_10_Violas___Performance_Legato | -3.80 | -50.0 | 1.59 | 0.779 | level, timing, start offset | 10.44 |
| 50 | 029_3_Trombones_Mutes_Performance | +4.03 | -300.0 | 17.96 | 0.199 | level, timbre, alignment unreliable | 10.02 |
| 51 | 082_Pacific___Ens_Strings___10_Cellos___Pizzicatos | +5.33 | +0.0 | 2.73 | 0.250 | level, sample/articulation/RR suspected | 9.99 |
| 52 | 016_Solo_Bass_Trombone_Wooden_Mute_Tenuto | +1.84 | -30.0 | 1.29 | 0.098 | level, timing, sample/articulation/RR suspected | 9.78 |
| 53 | 053_Areia___01_10_Violas___Legato_Fingered | -3.56 | +20.0 | 2.07 | 0.315 | level, timing, sample/articulation/RR suspected | 9.68 |
| 54 | 076_Dolce___09_3_Basses___Tremolo | -8.15 | +0.0 | 2.55 | 0.878 | level, start offset | 9.61 |
| 55 | 025_Solo_Trumpet_Tenuto | +2.14 | -70.0 | 0.32 | 0.969 | level, timing, start offset | 9.40 |
| 56 | 064_Dolce___01_7_1st_Violins___Legato | -8.58 | -300.0 | 2.39 | 0.362 | level, alignment unreliable | 9.38 |
| 57 | 085_Pacific___Ens_Strings___12_Violas___Spiccatos | +3.75 | +10.0 | 2.06 | 0.229 | level, sample/articulation/RR suspected | 9.29 |
| 58 | 046_Areia___09_16Vlns___10Vls_8va___Tremolo | -3.87 | -20.0 | 1.94 | 0.447 | level, timing, sample/articulation/RR suspected | 9.28 |
| 59 | 018_Solo_French_Horn_Repetitions | +2.15 | -60.0 | 0.33 | 0.803 | level, timing, start offset | 9.24 |
| 60 | 020_Solo_French_Horn_Stopped_Staccato | +2.11 | -50.0 | 0.18 | 0.612 | level, timing, sample/articulation/RR suspected | 9.11 |
| 61 | 023_Solo_Trumpet_Carbon_Mute_Marcato | +2.21 | -60.0 | 0.90 | 0.928 | level, timing, start offset | 8.88 |
| 62 | 091_Pacific___Ens_Strings___Lite_Full_Str___Pizzicatos | +3.71 | +0.0 | 3.05 | 0.196 | level, timbre, sample/articulation/RR suspected | 8.75 |
| 63 | 087_Pacific___Ens_Strings___16_Violins___Pizzicatos | +4.10 | +0.0 | 2.91 | 0.267 | level, sample/articulation/RR suspected | 8.73 |
| 64 | 021_Solo_Tenor_Trombone_Carbon_Mute_Tenuto | +2.37 | -50.0 | 0.25 | 0.788 | level, timing, start offset | 8.52 |
| 65 | 039_Areia___13_10_Violas___Harmonics | -3.06 | -10.0 | 1.77 | 0.337 | level, sample/articulation/RR suspected | 7.96 |
| 66 | 041_Areia___10_6_Celli___Trill_Half_Tone | -4.38 | -10.0 | 1.75 | 0.608 | level, sample/articulation/RR suspected | 7.92 |
| 67 | 090_Pacific___Ens_Strings___8_Basses___Spiccatos | +3.54 | +0.0 | 1.37 | 0.267 | level, sample/articulation/RR suspected | 7.66 |
| 68 | 038_Areia___07_10_Violas___Marcato_Short | -4.18 | +0.0 | 2.32 | 0.458 | level, sample/articulation/RR suspected | 7.66 |
| 69 | 003_2_Horns_Mutes_AccSusCresc | +4.24 | -10.0 | 0.97 | 0.650 | level, sample/articulation/RR suspected | 7.31 |
| 70 | 092_Pacific___Ens_Strings___Lite_Full_Str___Tremolos | +5.36 | +0.0 | 1.09 | 0.692 | level, sample/articulation/RR suspected | 7.26 |
| 71 | 014_Solo_Bass_Trombone_Marcato | +2.35 | +40.0 | 1.10 | 0.904 | level, timing, start offset | 7.19 |
| 72 | 036_Areia___11_16_Violins___Trill_Whole_Tone | -4.20 | -10.0 | 2.60 | 0.787 | level, start offset | 7.13 |
| 73 | 042_Areia___16_6_Celli___Col_Legno | -2.03 | -10.0 | 3.15 | 0.396 | level, timbre, sample/articulation/RR suspected | 7.10 |
| 74 | 084_Pacific___Ens_Strings___12_Violas___Legato_Sustains | +3.85 | +0.0 | 2.02 | 0.502 | level, sample/articulation/RR suspected | 7.01 |
| 75 | 019_Solo_French_Horn_Mute_Rips | +2.13 | -40.0 | 0.14 | 0.854 | level, timing, start offset | 6.91 |
| 76 | 098_Una_Corda_Cotton | +2.07 | -40.0 | 0.40 | 0.948 | level, timing, start offset | 6.46 |
| 77 | 100_Una_Corda_Pure | +3.05 | -30.0 | 0.25 | 0.957 | level, timing, start offset | 6.35 |
| 78 | 099_Una_Corda_Felt | +2.07 | -30.0 | 0.10 | 0.922 | level, timing, start offset | 5.49 |
| 79 | 031_01_Areia___16_Violins___Core_Techniques | -4.56 | -300.0 | 1.98 | 0.385 | level, alignment unreliable | 5.22 |
| 80 | 095_Vista___3_Basses | -0.05 | +30.0 | 1.70 | 0.823 | timing, start offset | 4.50 |
| 81 | 011_4_Horns_Stopped_Rips | +2.14 | +0.0 | 0.13 | 0.579 | level, sample/articulation/RR suspected | 4.29 |
| 82 | 005_2_Tubas_Marcato | +3.08 | +300.0 | 1.90 | 0.249 | level, alignment unreliable | 3.71 |
| 83 | 032_07_Areia___Full_Ens___Core_Techniques | -3.12 | -300.0 | 1.58 | 0.306 | level, alignment unreliable | 3.65 |
| 84 | 015_Solo_Bass_Trombone_Metal_Mute_Staccatissimo | +2.07 | +10.0 | 0.19 | 0.923 | level, start offset | 3.52 |
| 85 | 012_Bass_Trombone_Cimbasso_Tuba_Staccatissimo | +2.13 | +10.0 | 0.19 | 0.998 | level, start offset | 3.21 |
| 86 | 007_3_Trombones_Mutes_Staccato | +2.17 | +0.0 | 0.20 | 0.851 | level | 2.98 |
| 87 | 006_3_Trombones_Slides | +2.13 | +0.0 | 0.14 | 0.914 | level, start offset | 2.61 |
| 88 | 086_Pacific___Ens_Strings___16_Violins___FX___Cluster_Risers | +2.06 | +0.0 | 0.06 | 0.979 | level, start offset | 2.19 |
| 89 | 081_Pacific___Ens_Strings___10_Cellos___FX___Cluster_Risers | +2.00 | +0.0 | 0.09 | 0.981 | level, start offset | 2.12 |
| 90 | 096_Vista___4_Violas | +0.00 | -300.0 | 2.19 | 0.194 | alignment unreliable | 0.73 |
| 91 | 000_ANALOG_STRINGS | +0.18 | -300.0 | 0.90 | 0.145 | alignment unreliable | 0.48 |
| 92 | 033_Areia___06_6Cll_4DblBsss_8va___Legato | silent (kontra) | | | | capture/render failure, not a parity result | |
| 93 | 043_Areia___06_4_Double_Basses___Pizzicato | silent (kontakt) | | | | capture/render failure, not a parity result | |
| 94 | 044_Areia___12_4_Double_Basses___Sustained_Sul_Tasto | silent (kontakt) | | | | capture/render failure, not a parity result | |
| 95 | 048_Areia___05_6Cll___4DblBsss_8va___Staccato | silent (kontakt) | | | | capture/render failure, not a parity result | |
| 96 | 049_Areia___11_6Cll___4DblBsss_8va___Trill_Whole_Tone | silent (kontakt) | | | | capture/render failure, not a parity result | |
| 97 | 054_Areia___01_4_Double_Basses___Legato_Fingered | silent (kontra) | | | | capture/render failure, not a parity result | |
| 98 | 079_Conflux | silent (capture) | | | | capture/render failure, not a parity result | |
| 99 | 083_Pacific___Ens_Strings___10_Cellos___Trills | silent (kontakt) | | | | capture/render failure, not a parity result | |
| 100 | 088_Pacific___Ens_Strings___16_Violins___Trills | silent (kontakt) | | | | capture/render failure, not a parity result | |
| 101 | 097_Shreddage_3_5_E_Upright | silent (capture) | | | | capture/render failure, not a parity result | |

| - | 033_Areia___06_6Cll_4DblBsss_8va___Legato | excluded: KONTRA silent | | | | | |
| - | 043_Areia___06_4_Double_Basses___Pizzicato | excluded: Kontakt silent | | | | | |
| - | 044_Areia___12_4_Double_Basses___Sustained_Sul_Tasto | excluded: Kontakt silent | | | | | |
| - | 048_Areia___05_6Cll___4DblBsss_8va___Staccato | excluded: Kontakt silent | | | | | |
| - | 049_Areia___11_6Cll___4DblBsss_8va___Trill_Whole_Tone | excluded: Kontakt silent | | | | | |
| - | 054_Areia___01_4_Double_Basses___Legato_Fingered | excluded: KONTRA silent | | | | | |
| - | 079_Conflux | excluded: capture failed: load_instrument failed. | | | | | |
| - | 083_Pacific___Ens_Strings___10_Cellos___Trills | excluded: Kontakt silent | | | | | |
| - | 088_Pacific___Ens_Strings___16_Violins___Trills | excluded: Kontakt silent | | | | | |
| - | 097_Shreddage_3_5_E_Upright | excluded: capture failed: load_instrument failed. | | | | | |
