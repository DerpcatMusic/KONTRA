# Corpus load gap ranking

Snapshot: `kontra-inspect load` at `6075a76643e51b2b5dc6e8d78c4a1f8040cf3002`; 1342/1342 programs succeeded (682 Kontakt, 660 UVI); 0 UVI bank enumeration errors. This is historical data, not current parity.

Each feature is deduplicated across programs and impact classes, then ranked by distinct affected libraries; feature name breaks ties. Impact is taken from diagnostics where available; Unknown is not zero or a severity claim. Lane ownership is keyword triage, with H as fallback. The same feature may have multiple impact classes. UI unsupported entries are grouped by feature rather than widget reference; UI assets not resolved by the loader are not necessarily absent; init events do not cover later callbacks. Purged-only zones are excluded.

| Rank | Feature | Libraries | Impact | Owner |
|---:|---|---:|---|---|
| 1 | script init unanswered: GetEngineParDisp | 8 | Unknown | B |
| 2 | UI asset not resolved: Image | 4 | Unknown | I (triage) |
| 3 | script init fault: out_of_range | 4 | Unknown | B |
| 4 | script init unanswered: SetEnginePar | 4 | Unknown | B |
| 5 | script init unanswered: SetSnapshotType | 4 | Unknown | B |
| 6 | DigitalEq band order | 3 | Audible | H (triage) |
| 7 | script compile | 3 | Broken | B |
| 8 | script init unanswered: GetFolder | 3 | Unknown | B |
| 9 | script init unanswered: LoadArray | 3 | Unknown | B |
| 10 | script not compiled | 3 | Unknown | B |
| 11 | script init fault: divide_by_zero | 2 | Unknown | B |
| 12 | script init unanswered: GetEnginePar | 2 | Unknown | B |
| 13 | script init unanswered: LoadPerformanceView | 2 | Unknown | B |
| 14 | script init unanswered: SetControlParStrArr | 2 | Unknown | B |
| 15 | UI asset not resolved: TrueTypeFont | 1 | Unknown | I (triage) |
| 16 | UI unsupported: $CONTROL_PAR_TEXTLINE | 1 | Unknown | I (triage) |
| 17 | bypassed effect Distortion | 1 | Cosmetic | H (triage) |
| 18 | bypassed effect Jump | 1 | Cosmetic | H (triage) |
| 19 | bypassed effect Lo-Fi | 1 | Cosmetic | H (triage) |
| 20 | bypassed effect Rotator | 1 | Cosmetic | H (triage) |
| 21 | bypassed effect Saturation | 1 | Cosmetic | H (triage) |
| 22 | bypassed effect Skreamer | 1 | Cosmetic | H (triage) |
| 23 | bypassed effect Tape Saturator | 1 | Cosmetic | H (triage) |
| 24 | effect Tape Saturator | 1 | Audible | H (triage) |
| 25 | external modulator KeyPosition -> module ahdsr_attack | 1 | Audible | H (triage) |
| 26 | external modulator MidiCc(1) -> warpFactor | 1 | Audible | H (triage) |
| 27 | external modulator MidiCc(1) -> warpFactor2 | 1 | Audible | H (triage) |
| 28 | external modulator MidiCc(1) -> wavetableInharmonic | 1 | Audible | H (triage) |
| 29 | external modulator MidiCc(1) -> wavetableModAmount | 1 | Audible | H (triage) |
| 30 | external modulator MidiCc(1) -> wavetableModFrequency | 1 | Audible | H (triage) |
| 31 | external modulator MidiCc(1) -> wavetablePosition | 1 | Audible | H (triage) |
| 32 | external modulator PitchBend -> pan | 1 | Audible | H (triage) |
| 33 | external modulator PitchBend -> wavetableInharmonic | 1 | Audible | H (triage) |
| 34 | external modulator PitchBend -> wavetablePosition | 1 | Audible | H (triage) |
| 35 | external modulator RandomBipolar -> module filterCutoff | 1 | Audible | H (triage) |
| 36 | external modulator RandomBipolar -> module filterQ | 1 | Audible | H (triage) |
| 37 | external modulator RandomBipolar -> module frequency | 1 | Audible | H (triage) |
| 38 | external modulator RandomBipolar -> pan | 1 | Audible | H (triage) |
| 39 | external modulator RandomBipolar -> pitch | 1 | Audible | H (triage) |
| 40 | external modulator RandomBipolar -> warpFactor | 1 | Audible | H (triage) |
| 41 | external modulator RandomBipolar -> warpFactor2 | 1 | Audible | H (triage) |
| 42 | external modulator RandomBipolar -> wavetableModAmount | 1 | Audible | H (triage) |
| 43 | external modulator RandomBipolar -> wavetablePosition | 1 | Audible | H (triage) |
| 44 | external modulator RandomUnipolar -> module intensity | 1 | Audible | H (triage) |
| 45 | external modulator Script(0) -> warpFactor | 1 | Audible | H (triage) |
| 46 | external modulator Script(0) -> warpFactor2 | 1 | Audible | H (triage) |
| 47 | external modulator Script(0) -> wavetableInharmonic | 1 | Audible | H (triage) |
| 48 | external modulator Script(0) -> wavetableModAmount | 1 | Audible | H (triage) |
| 49 | external modulator Script(0) -> wavetablePosition | 1 | Audible | H (triage) |
| 50 | external modulator Velocity -> warpFactor | 1 | Audible | H (triage) |
| 51 | external modulator Velocity -> warpFactor2 | 1 | Audible | H (triage) |
| 52 | external modulator Velocity -> wavetableModAmount | 1 | Audible | H (triage) |
| 53 | external modulator Velocity -> wavetablePosition | 1 | Audible | H (triage) |
| 54 | impulse response size | 1 | Audible | H (triage) |
| 55 | insert CombFilter | 1 | Audible, Latent | H (triage) |
| 56 | insert Drive | 1 | Audible, Latent | H (triage) |
| 57 | insert FeedbackMachine | 1 | Audible, Latent | H (triage) |
| 58 | insert XpanderFilter | 1 | Audible, Latent | H (triage) |
| 59 | internal modulator AHDSR -> module frequency | 1 | Audible | H (triage) |
| 60 | internal modulator AHDSR -> module intensity | 1 | Audible | H (triage) |
| 61 | internal modulator AHDSR -> warpFactor | 1 | Audible | H (triage) |
| 62 | internal modulator AHDSR -> warpFactor2 | 1 | Audible | H (triage) |
| 63 | internal modulator AHDSR -> wavetableInharmonic | 1 | Audible | H (triage) |
| 64 | internal modulator AHDSR -> wavetableModAmount | 1 | Audible | H (triage) |
| 65 | internal modulator AHDSR -> wavetableModFrequency | 1 | Audible | H (triage) |
| 66 | internal modulator AHDSR -> wavetablePosition | 1 | Audible | H (triage) |
| 67 | internal modulator LFO -> module frequency | 1 | Audible | H (triage) |
| 68 | internal modulator LFO -> module intensity | 1 | Audible | H (triage) |
| 69 | internal modulator LFO -> warpFactor | 1 | Audible | H (triage) |
| 70 | internal modulator LFO -> warpFactor2 | 1 | Audible | H (triage) |
| 71 | internal modulator LFO -> wavetableInharmonic | 1 | Audible | H (triage) |
| 72 | internal modulator LFO -> wavetableModAmount | 1 | Audible | H (triage) |
| 73 | internal modulator LFO -> wavetableModFrequency | 1 | Audible | H (triage) |
| 74 | internal modulator LFO -> wavetablePosition | 1 | Audible | H (triage) |
| 75 | modulation Amount | 1 | Audible | H (triage) |
| 76 | modulation Depth | 1 | Audible, Latent | H (triage) |
| 77 | modulation Feedback | 1 | Audible | H (triage) |
| 78 | modulation Freq | 1 | Audible, Latent | H (triage) |
| 79 | modulation Gain | 1 | Audible, Latent | H (triage) |
| 80 | modulation MIDICurve_1 | 1 | Audible | H (triage) |
| 81 | modulation Mix | 1 | Audible, Latent | H (triage) |
| 82 | modulation Pitch | 1 | Latent | H (triage) |
| 83 | modulation Q | 1 | Audible | H (triage) |
| 84 | modulation Speed | 1 | Audible | H (triage) |
| 85 | modulation XpanderDrive_4 | 1 | Audible | H (triage) |
| 86 | script init unanswered: AttachZone | 1 | Unknown | B |
| 87 | script init unanswered: ExposeControls | 1 | Unknown | B |
| 88 | script init unanswered: FindZone | 1 | Unknown | B |
| 89 | script init unanswered: GetEngineParDispExt | 1 | Unknown | B |
| 90 | script init unanswered: GetZonePar | 1 | Unknown | B |
| 91 | script init unanswered: LoadArrayStr | 1 | Unknown | B |
| 92 | script init unanswered: LoadNativeUi | 1 | Unknown | B |
| 93 | script init unanswered: ResetNksNav | 1 | Unknown | B |
| 94 | script init unanswered: SaveArrayStr | 1 | Unknown | B |
| 95 | script init unanswered: SetNksNavName | 1 | Unknown | B |
| 96 | script init unanswered: SetNksNavPar | 1 | Unknown | B |
| 97 | script sample | 1 | Broken | B |
| 98 | silent effect Convolution | 1 | Cosmetic | H (triage) |
| 99 | unassigned external modulator | 1 | Cosmetic | H (triage) |
| 100 | wavetable inharmonic | 1 | Audible | H (triage) |
| 101 | wavetable modulation | 1 | Audible | H (triage) |
| 102 | wavetable random phase | 1 | Audible | H (triage) |

This load-only census reports translator diagnostics, initialization, prepared zones and UI model observations. It does not exercise notes, later callbacks or moving controls, and cannot prove the absence of unreported gaps. Detailed diagnostic messages and affected paths are retained in `scratch/laneG/corpus/summary.json` on the census machine.
