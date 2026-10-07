# MPE coverage across the installed Kontakt libraries

Per-note MPE response of one instrument per library, measured by
`crates/sampler-native/tests/mpe_corpus.rs` (ignored survey; run with
`KONTRA_KONTAKT_LIBRARIES=... cargo test --release -p sampler-native --test mpe_corpus -- --ignored --nocapture`,
`KONTRA_MPE_LIMIT` for more instruments per library, `KONTRA_MPE_SCRIPTS=0` to load without scripts).
The probe (`crates/sampler-midi/src/probe.rs`) plays one note on lower-zone member channel 1 and, after
8192 frames, sends a +2 semitone bend (member range 48) or full channel pressure, then compares the next
8192 frames with an untouched note.

- Pitch: the frequency ratio found by correlating log band powers (24 bands per octave from 400 Hz,
  Hann-windowed mid signal) of the bent and the plain render; 1.122 is a clean +2 st. Ok is 1.06 to 1.19.
  The earlier zero-crossing count was wrong for noise-like and layered material (it misreported Areia,
  Solo Violin and the Pacific risers as unresponsive); the spectral estimate replaced it.
- Pressure: RMS with full pressure over without, in dB; the default pressure law is +6.0 dB. Ok is 1 dB or more.

| Library | Instrument | Pitch | Pressure |
|---|---|---|---|
| Afflatus Chapter II Brass | 2 Horns KS | ok 1.116 | ok 5.9 dB |
| ANALOG STRINGS | ANALOG STRINGS | NO 1.005 | ok 6.0 dB |
| Areia 1.2.0 | 01 Areia - 16 Violins - Core Techniques | ok 1.122 | ok 6.0 dB |
| Audio Imperia CHORUS | Women - Traditional Articulations | NO, silent | NO, silent |
| Audio Imperia Dolce | 7 1st Violins - Legato | ok 1.122 | ok 6.0 dB |
| Conflux 1.1.0 | Conflux | ok 1.121 | ok 6.0 dB |
| Morphology Evolved | Morphology Evolved | ok 1.122 | ok 5.9 dB |
| Pacific Ensemble Strings | 10 Cellos - FX - Cluster Risers | ok 1.118 | ok 6.0 dB |
| Performance Samples Vista | 3 Violins FFF Overlay | ok 1.122 | ok 6.0 dB |
| Solo | 01 Solo Violin | ok 1.122 | ok 6.0 dB |
| Una Corda Library | Una Corda Cotton | ok 1.121 | ok 6.0 dB |

## Open

- Chorus is silent with or without scripts (a format gap, the same as the one-rack multi).
- ANALOG STRINGS answers pressure but its pitch does not move under a member-channel bend (ratio 1.005).
  Not yet traced: a synth-style source may hold a fixed pitch, or the instrument's script may own it.
- Script-played notes (KSP `play_note`) inherit the parent's expression (`Inheritance::Expression`);
  the survey found no script-child bend failure once the metric was fixed.
- One instrument per library is a sample, not coverage of every patch; raise `KONTRA_MPE_LIMIT` for more.
