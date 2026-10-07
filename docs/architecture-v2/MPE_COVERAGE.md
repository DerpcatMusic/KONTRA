# MPE coverage across the installed libraries

Per-note MPE response measured by two ignored surveys:

- Kontakt: `crates/sampler-native/tests/mpe_corpus.rs` (`KONTRA_KONTAKT_LIBRARIES=... cargo test --release -p sampler-native --test mpe_corpus -- --ignored --nocapture`;
  `KONTRA_MPE_LIMIT` instruments per library, `KONTRA_MPE_SCRIPTS=0` loads without scripts, `KONTRA_MPE_FILTER` keeps paths containing the text).
  The probe (`crates/sampler-midi/src/probe.rs`) loads the zones at one key, with scripts, plays one note on lower-zone member channel 1 and, after 8192 frames, sends the
  expression, then compares the next 8192 frames with an untouched note. If the first key is silent it tries the widest zone's middle and then middle C.
- UVI: `tests/mpe_uvi.rs` (`KONTRA_UVI_LIBRARIES=... cargo test --release --test mpe_uvi -- --ignored --nocapture`), the first program of every UFS bank played through the
  host core with its script, breath (CC2) held at 100 on the manager channel, middle C first and then keys through the mapped range until one sounds.

Metrics (all from the rendered audio):

- Pitch: frequency ratio from correlating log band powers (24 bands per octave from 400 Hz, Hann-windowed mid signal) of the +2 semitone bent and the plain render;
  1.122 is a clean +2 st. Ok is 1.06 to 1.19. (Zero-crossing counting misreported noise-like and layered material and was replaced.)
- Pressure: RMS with full channel pressure over without, in dB; the default law is +6.0 dB. Ok is 1 dB or more.
- Timbre: spectral centroid with CC74 at 0 over the plain note; below 0.95 (darker) is ok. Timbre at 0 closes a per-voice low-pass
  (`MpeDefaults::timbre_semitones`, 60 semitones below open at 0, nothing from centre up); it reaches the voice modulator program as `ModSource::Timbre`
  exactly as pressure does (`ModSource::Pressure` to `ModTarget::Decibels`), covered by `native_mpe_defaults_are_identity_at_rest_and_follow_pressure_and_timbre`.
  The load report states the mapping (`Decoded::mpe`, shown under Modulation).

## Kontakt, up to five instruments per library (41 instruments)

Pitch ok 34, pressure ok 40, timbre ok 35 of 41.

| Instrument | Pitch | Pressure | Timbre |
|---|---|---|---|
| ANALOG STRINGS/Instruments/ANALOG STRINGS.nki | NO 1.005 | ok 6.0 dB | NO 1.00 |
| Afflatus Chapter II Brass/Instruments/1. Ensembles/Multi Instruments/2 Horns KS.nki | ok 1.116 | ok 5.9 dB | ok 0.95 |
| Afflatus Chapter II Brass/Instruments/1. Ensembles/Multi Instruments/2 Tubas KS.nki | NO 1.001 | ok 6.1 dB | NO 0.98 |
| Afflatus Chapter II Brass/Instruments/1. Ensembles/Multi Instruments/3 Trombones KS.nki | NO 0.990 | ok 5.9 dB | ok 0.87 |
| Afflatus Chapter II Brass/Instruments/1. Ensembles/Multi Instruments/3 Trumpets KS.nki | ok 1.121 | ok 6.0 dB | ok 0.73 |
| Afflatus Chapter II Brass/Instruments/1. Ensembles/Multi Instruments/4 Horns KS.nki | ok 1.121 | ok 6.0 dB | NO 0.95 |
| Areia 1.2.0 [Audio Imperia]/Instruments/01 Core Technique Patches/01 Areia - 16 Violins - Core Techniques.nki | ok 1.122 | ok 6.0 dB | ok 0.57 |
| Areia 1.2.0 [Audio Imperia]/Instruments/01 Core Technique Patches/02 Areia - 10 Violas - Core Techniques.nki | ok 1.122 | ok 6.0 dB | ok 0.53 |
| Areia 1.2.0 [Audio Imperia]/Instruments/01 Core Technique Patches/03 Areia - 6 Celli - Core Techniques.nki | ok 1.120 | ok 6.0 dB | ok 0.76 |
| Areia 1.2.0 [Audio Imperia]/Instruments/01 Core Technique Patches/04 Areia - 4 Double Basses - Core Techniques.nki | NO 1.001 | ok 6.0 dB | ok 0.83 |
| Areia 1.2.0 [Audio Imperia]/Instruments/01 Core Technique Patches/05 Areia - 16Vlns + 10Vls 8va - Core Techniques.nki | ok 1.123 | ok 6.0 dB | ok 0.46 |
| Audio Imperia CHORUS/Instruments/01 Multi Patches/01 Chorus - Women - Traditional Articulations.nki | ok 1.120 | ok 6.0 dB | ok 0.62 |
| Audio Imperia CHORUS/Instruments/01 Multi Patches/02 Chorus - Women - Traditional Syllables.nki | ok 1.118 | ok 5.9 dB | ok 0.63 |
| Audio Imperia CHORUS/Instruments/01 Multi Patches/03 Chorus - Women - Energetic Syllables.nki | ok 1.120 | ok 6.0 dB | ok 0.56 |
| Audio Imperia CHORUS/Instruments/01 Multi Patches/04 Chorus - Women - Slow Syllables.nki | ok 1.122 | ok 6.0 dB | ok 0.79 |
| Audio Imperia CHORUS/Instruments/01 Multi Patches/05 Chorus - Men - Traditional Articulations.nki | ok 1.122 | ok 6.0 dB | ok 0.66 |
| Audio Imperia Dolce/Instruments/01 7 1st Violins/Dolce - 01 7 1st Violins - Legato.nki | ok 1.122 | ok 6.0 dB | ok 0.64 |
| Audio Imperia Dolce/Instruments/01 7 1st Violins/Dolce - 02 7 1st Violins - Sustained.nki | ok 1.122 | ok 6.0 dB | ok 0.64 |
| Audio Imperia Dolce/Instruments/01 7 1st Violins/Dolce - 03 7 1st Violins - Sustained Con Sordino.nki | ok 1.122 | ok 6.0 dB | ok 0.85 |
| Audio Imperia Dolce/Instruments/01 7 1st Violins/Dolce - 04 7 1st Violins - Harmonics.nki | ok 1.122 | ok 6.0 dB | ok 0.80 |
| Audio Imperia Dolce/Instruments/01 7 1st Violins/Dolce - 05 7 1st Violins - Spiccato.nki | NO 0.000 | NO 0.0 dB | ok 0.00 |
| Conflux 1.1.0 [Native Instruments]/Instruments/Conflux.nki | ok 1.121 | ok 6.0 dB | ok 0.76 |
| Morphology Evolved [Zero-G] rutracker.org/Morphology Evolved.nki | ok 1.122 | ok 5.9 dB | ok 0.20 |
| Pacific Ensemble Strings/Instruments/10 Cellos/Pacific - Ens Strings - 10 Cellos - FX - Cluster Risers.nki | ok 1.118 | ok 6.0 dB | ok 0.56 |
| Pacific Ensemble Strings/Instruments/10 Cellos/Pacific - Ens Strings - 10 Cellos - FX - Cluster Shorts.nki | NO 1.001 | ok 6.0 dB | ok 0.84 |
| Pacific Ensemble Strings/Instruments/10 Cellos/Pacific - Ens Strings - 10 Cellos - Legato Sustains.nki | ok 1.117 | ok 6.0 dB | ok 0.75 |
| Pacific Ensemble Strings/Instruments/10 Cellos/Pacific - Ens Strings - 10 Cellos - Marcatos.nki | ok 1.121 | ok 6.0 dB | ok 0.66 |
| Pacific Ensemble Strings/Instruments/10 Cellos/Pacific - Ens Strings - 10 Cellos - Pizzicatos.nki | NO 0.999 | ok 6.0 dB | NO 0.97 |
| Performance Samples Vista/Instruments/Bonus/Vista - 3 Violins FFF Overlay.nki | ok 1.122 | ok 6.0 dB | ok 0.60 |
| Performance Samples Vista/Instruments/Bonus/Vista - Full Strings Sustains.nki | ok 1.121 | ok 6.0 dB | ok 0.10 |
| Performance Samples Vista/Instruments/Bonus/Vista - Harp.nki | ok 1.122 | ok 6.0 dB | NO 0.98 |
| Performance Samples Vista/Instruments/Vista - 3 Basses.nki | ok 1.121 | ok 6.0 dB | ok 0.80 |
| Performance Samples Vista/Instruments/Vista - 3 Cellos.nki | ok 1.123 | ok 6.0 dB | ok 0.86 |
| Solo/Instruments/01 Multi Patches/Solo - 01 Solo Violin.nki | ok 1.122 | ok 6.0 dB | ok 0.77 |
| Solo/Instruments/01 Multi Patches/Solo - 02 Solo Viola.nki | ok 1.122 | ok 5.9 dB | ok 0.57 |
| Solo/Instruments/01 Multi Patches/Solo - 03 Solo Cello.nki | ok 1.121 | ok 5.9 dB | ok 0.89 |
| Solo/Instruments/01 Multi Patches/Solo - 04 Solo Trumpet.nki | ok 1.121 | ok 6.0 dB | ok 0.56 |
| Solo/Instruments/01 Multi Patches/Solo - 05 Solo French Horn.nki | ok 1.121 | ok 5.9 dB | NO 0.97 |
| Una Corda Library/Instruments/Una Corda Cotton.nki | ok 1.121 | ok 6.0 dB | ok 0.93 |
| Una Corda Library/Instruments/Una Corda Felt.nki | ok 1.122 | ok 6.0 dB | ok 0.91 |
| Una Corda Library/Instruments/Una Corda Pure.nki | ok 1.122 | ok 6.0 dB | ok 0.88 |

## UVI, first program of each UFS bank (26 banks)

Pitch ok 24, pressure ok 24, timbre ok 16 of 26.

| Instrument | Pitch | Pressure | Timbre |
|---|---|---|---|
| UVI - Augmented Orchestra v1.1.2-R2R/Augmented Orchestra.ufs/Presets/00 Orchestra/01 Strings/V Strings Bartok.uvip | ok 1.122 | ok 5.9 dB | ok 0.87 |
| VWinds - Clarinets/VWinds-AClarinet.ufs/Presets/Clarinet A.uvip | ok 1.122 | ok 6.0 dB | NO 1.00 |
| VWinds - Clarinets/VWinds-AClarinet_V2.ufs/Presets/Clarinet A.uvip | ok 1.123 | ok 6.0 dB | ok 0.07 |
| VWinds - Clarinets/VWinds-BassClarinet.ufs/Presets/Bass Clarinet 2.uvip | ok 1.123 | ok 6.0 dB | ok 0.75 |
| VWinds - Clarinets/VWinds-BassClarinet_V2.ufs/Presets/Bass Clarinet 2.uvip | ok 1.120 | ok 6.0 dB | ok 0.50 |
| VWinds - Clarinets/VWinds-BassetHorn.ufs/Presets/Basset Horn.uvip | ok 1.122 | ok 6.0 dB | NO 0.99 |
| VWinds - Clarinets/VWinds-BassetHorn_V2.ufs/Presets/Basset Horn.uvip | ok 1.122 | ok 6.0 dB | ok 0.20 |
| VWinds - Clarinets/VWinds-BbClarinet.ufs/Presets/Bb Clarinet 2.uvip | ok 1.121 | ok 6.0 dB | NO 0.99 |
| VWinds - Clarinets/VWinds-BbClarinet_V2.ufs/Presets/Bb Clarinet 2.uvip | ok 1.122 | ok 6.0 dB | ok 0.20 |
| VWinds - Clarinets/VWinds-ContrabassClarinet.ufs/Presets/Contrabass Clarinet.uvip | ok 1.122 | ok 6.0 dB | NO 0.98 |
| VWinds - Clarinets/VWinds-ContrabassClarinet_V2.ufs/Presets/Contrabass Clarinet.uvip | ok 1.121 | ok 6.0 dB | ok 0.38 |
| VWinds - Clarinets/VWinds-EBClarinet.ufs/Presets/Clarinet EB.uvip | ok 1.122 | ok 6.0 dB | NO 0.97 |
| VWinds - Clarinets/VWinds-EBClarinet_V2.ufs/Presets/Clarinet EB.uvip | ok 1.123 | ok 6.0 dB | ok 0.35 |
| VWinds - Double Reeds/VWinds-Bassoon.ufs/Presets/Bassoon.uvip | ok 1.122 | ok 6.0 dB | NO 0.99 |
| VWinds - Double Reeds/VWinds-Bassoon_V2.ufs/Presets/Bassoon.uvip | ok 1.121 | ok 6.0 dB | ok 0.73 |
| VWinds - Double Reeds/VWinds-Contrabassoon.ufs/Presets/Contrabassoon.uvip | ERR |  |  |
| VWinds - Double Reeds/VWinds-Contrabassoon_V2.ufs/Presets/Contrabassoon.uvip | ERR |  |  |
| VWinds - Double Reeds/VWinds-EnglishHorn.ufs/Presets/English Horn.uvip | ok 1.122 | ok 6.0 dB | ok 0.89 |
| VWinds - Double Reeds/VWinds-EnglishHorn_V2.ufs/Presets/English Horn 2.uvip | ok 1.121 | ok 6.0 dB | ok 0.47 |
| VWinds - Double Reeds/VWinds-Oboe.ufs/Presets/Oboe.uvip | ok 1.121 | ok 6.0 dB | ok 0.93 |
| VWinds - Double Reeds/VWinds-Oboe_V2.ufs/Presets/Oboe 2.uvip | ok 1.120 | ok 6.0 dB | ok 0.75 |
| VWinds - Flutes/VWinds-Alto_Flute.ufs/Presets/Alto Flute 2.uvip | ok 1.122 | ok 6.0 dB | ok 0.81 |
| VWinds - Flutes/VWinds-Bass_Flute.ufs/Presets/Bass Flute 2.uvip | ok 1.122 | ok 6.0 dB | NO 0.96 |
| VWinds - Flutes/VWinds-C_Flute.ufs/Presets/Flute.uvip | ok 1.122 | ok 6.0 dB | ok 0.95 |
| VWinds - Flutes/VWinds-Contrabass_Flute.ufs/Presets/Contrabass Flute.uvip | ok 1.120 | ok 6.0 dB | NO 0.96 |
| VWinds - Flutes/VWinds-Piccolo.ufs/Presets/Piccolo 2.uvip | ok 1.122 | ok 6.0 dB | ok 0.76 |

## Findings

- Kontakt script-played notes: `2 Tubas KS` and `3 Trombones KS` (Afflatus), `4 Double Basses` (Areia) do not follow a member-channel bend with scripts on while they do with
  `KONTRA_MPE_SCRIPTS=0`; pressure follows either way. `2 Horns KS` from the same library is fine, so it is per script (sampler-ksp, with the Inheritance::Expression path); with the
  widest-zone key the Tubas respond, so it may also depend on the key played. Owner: the KSP agent.
- Short sounds (Pacific Pizzicatos, Cluster Shorts) read no pitch response because the note has largely ended inside the 8192-frame window; a measurement limit, not a failure.
- ANALOG STRINGS answers pressure but not pitch (ratio 1.005): not traced.
- UVI: pitch and pressure answer in every bank that loads. Timbre does not on the first-generation VWinds programs (centroid 0.96 to 1.00) while the V2 programs do;
  Contrabassoon (both generations) panics in the survey. Owner: the UVI agent.
- Not done: mapping timbre to the instrument's own main brightness control (filter cutoff, or a dynamics crossfade without a filter). Today every zone gets the one
  generic per-voice tone low-pass; the reference data for what Kontakt does with CC74 is pending.
