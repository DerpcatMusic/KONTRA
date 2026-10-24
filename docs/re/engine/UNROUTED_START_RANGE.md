# Unrouted sample start range: attack and release tests (2026-10-24)

Question: does Kontakt 8 start an unrouted zone (`sample_start_mod_range` > 0, no SampleStart route) at a
random per-note offset in [0, range)? KONTRA does this for note-on admissions only
(`prepare/selection.rs`, `random_delay`); release admissions have no delay path.

## Library survey (IR of 28 NKIs sampled across /data/main/Libraries/Kontakt)

Zones with `start_range` > 0 and no SampleStart route:

| Instrument | Attack zones | Release zones | start_range |
|---|---|---|---|
| 3 Trombones KS | 6548 | 1816 | 5120 |
| 3 Trombones Mutes KS | 2728 | 816 | 5120 |
| 4 Horns Stopped Performance | 2328 | 120 | 5120 |
| 2 Horns Mutes SusToFlutter / Solo French Horn Mute SusToFlutter | 2328 | 120 | 5120 |
| Solo Cimbasso Marcato, 4 Horns Mutes Marcato, Solo Bass Trombone Metal Mute, Solo Tenor Trombone Metal Mute, Solo Tuba Mute Flutter | 0 | 152-608 | n/a |

Routed (SampleStart route, so the random law is off): Areia (all instruments), Chorus, Dolce (Controller 113 S. Start slider),
Pacific Ensembles Strings (attack); Dolce and Pacific (release) as well.

Trills (Afflatus Chapter II Brass, Solo Trumpet Trills): 560 zones start 0 (attack), 488 zones start 7680 (GateRelease,
groups 16-31, key 52-83, velocity 1-65 and 66-127), no routes.

## Attack test: 3 Trombones KS, key 50 vel 100, 16 notes, 2 s spacing

- KONTRA true per-note delay traced (`KDELAY` trace on the build, one line per admitted voice; all 16 voices of a note share
  one draw): 0-104 ms, spread as expected for uniform over 5120 frames.
- Method check: KONTRA onset (threshold -20 dB) minus traced delay = 10.5 ms, sd 0.6 ms over 16 notes. The onset
  measurement is therefore consistent with the traced delay.
- Kontakt (one capture, 16 notes): onsets 32-37 ms (10 notes) and 11-16 ms (6 notes). Assuming the same 10.5 ms attack
  and the 34 ms capture latency from Trills, the implied Kontakt delays would be about -8 to -33 ms, which is impossible.
  The two groups suggest Kontakt selects a different attack sample per note (round robin), so per-note attack timing is
  not comparable with KONTRA's single attack sample. The Kontakt spread is not the uniform 0-104 ms predicted by the law.
- The repeat capture (second timeline) did not produce usable audio alignment (onsets at the first 2 notes only, then 0);
  it is not used.

## Release test: Trills key 60 vel 100, GateRelease zones, 8 notes, 2 s spacing, 1 s hold

- KONTRA: `random_delay` is never called on the release path, so release voices start at once.
- Kontakt: release onset cannot be read from the threshold onset, because the note-on tail overlaps the release
  sample (the peak-relative threshold fires at 0 ms). Waveform cross-correlation against KONTRA gave r about 0.3 and lags at the
  search edge, so no per-note delay can be extracted from this capture. Inconclusive.

## Conclusion

- No change to `voice_mod.rs`. The attack test does not show the uniform 0-104 ms Kontakt spread that a scale change would fit,
  and the release test is inconclusive. Changing the scale on this evidence would be a guess.
- Open: a note-on capture on an instrument whose attack sample is the same across notes (single RR layer, or with RR
  disabled), and a release capture with the held note silenced before note-off (e.g. CC120 on a separate channel) so the
  release sample is isolated.

## Method (scripts in ~/.cache/kontra-ksp/onset/)

- `onset.py` (threshold onset, attack), `xcorr.py` (waveform lag), `relonset.py` (release, inconclusive).
- Trace: `KDELAY=1 sampler-native render-kontakt-midi ...` on a build with the temporary eprintln at `prepare/selection.rs` after
  `random_delay` (not committed).
- Survey: `ir_json` example (`crates/sampler-kontakt/examples/ir_json.rs`) over `find ... -name '*.nki' | awk 'NR%25==1'`.
