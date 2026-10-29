# Corpus load gap ranking

Snapshot: `kontra-inspect load` at `6075a76643e51b2b5dc6e8d78c4a1f8040cf3002`; 1342/1342 programs succeeded (682 Kontakt, 660 UVI); 0 UVI bank enumeration errors. This is historical data, not current parity.

Each feature is deduplicated across programs and impact classes, then ranked by distinct affected libraries; feature name breaks ties. Impact is taken from diagnostics where available; Unknown is not zero or a severity claim. Lane ownership is keyword triage, with H as fallback. The same feature may have multiple impact classes. UI assets not resolved by the loader are not necessarily absent; init events do not cover later callbacks. Purged-only zones are excluded.

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
| 16 | UI unsupported: Unsupported { widget: Some(WidgetRef(12)), feature: "$CONTROL_PAR_TEXTLINE", value: "10 None" } | 1 | Unknown | I (triage) |
| 17 | UI unsupported: Unsupported { widget: Some(WidgetRef(345)), feature: "$CONTROL_PAR_TEXTLINE", value: "10 None" } | 1 | Unknown | I (triage) |
| 18 | UI unsupported: Unsupported { widget: Some(WidgetRef(38)), feature: "$CONTROL_PAR_TEXTLINE", value: "10 None" } | 1 | Unknown | I (triage) |
| 19 | UI unsupported: Unsupported { widget: Some(WidgetRef(39)), feature: "$CONTROL_PAR_TEXTLINE", value: "10 None" } | 1 | Unknown | I (triage) |
| 20 | UI unsupported: Unsupported { widget: Some(WidgetRef(49)), feature: "$CONTROL_PAR_TEXTLINE", value: "10 None" } | 1 | Unknown | I (triage) |
| 21 | UI unsupported: Unsupported { widget: Some(WidgetRef(50)), feature: "$CONTROL_PAR_TEXTLINE", value: "10 None" } | 1 | Unknown | I (triage) |
| 22 | bypassed effect Distortion | 1 | Cosmetic | H (triage) |
| 23 | bypassed effect Jump | 1 | Cosmetic | H (triage) |
| 24 | bypassed effect Lo-Fi | 1 | Cosmetic | H (triage) |
| 25 | bypassed effect Rotator | 1 | Cosmetic | H (triage) |
| 26 | bypassed effect Saturation | 1 | Cosmetic | H (triage) |
| 27 | bypassed effect Skreamer | 1 | Cosmetic | H (triage) |
| 28 | bypassed effect Tape Saturator | 1 | Cosmetic | H (triage) |
| 29 | effect Tape Saturator | 1 | Audible | H (triage) |
| 30 | external modulator KeyPosition -> module ahdsr_attack | 1 | Audible | H (triage) |
| 31 | external modulator MidiCc(1) -> warpFactor | 1 | Audible | H (triage) |
| 32 | external modulator MidiCc(1) -> warpFactor2 | 1 | Audible | H (triage) |
| 33 | external modulator MidiCc(1) -> wavetableInharmonic | 1 | Audible | H (triage) |
| 34 | external modulator MidiCc(1) -> wavetableModAmount | 1 | Audible | H (triage) |
| 35 | external modulator MidiCc(1) -> wavetableModFrequency | 1 | Audible | H (triage) |
| 36 | external modulator MidiCc(1) -> wavetablePosition | 1 | Audible | H (triage) |
| 37 | external modulator PitchBend -> pan | 1 | Audible | H (triage) |
| 38 | external modulator PitchBend -> wavetableInharmonic | 1 | Audible | H (triage) |
| 39 | external modulator PitchBend -> wavetablePosition | 1 | Audible | H (triage) |
| 40 | external modulator RandomBipolar -> module filterCutoff | 1 | Audible | H (triage) |
| 41 | external modulator RandomBipolar -> module filterQ | 1 | Audible | H (triage) |
| 42 | external modulator RandomBipolar -> module frequency | 1 | Audible | H (triage) |
| 43 | external modulator RandomBipolar -> pan | 1 | Audible | H (triage) |
| 44 | external modulator RandomBipolar -> pitch | 1 | Audible | H (triage) |
| 45 | external modulator RandomBipolar -> warpFactor | 1 | Audible | H (triage) |
| 46 | external modulator RandomBipolar -> warpFactor2 | 1 | Audible | H (triage) |
| 47 | external modulator RandomBipolar -> wavetableModAmount | 1 | Audible | H (triage) |
| 48 | external modulator RandomBipolar -> wavetablePosition | 1 | Audible | H (triage) |
| 49 | external modulator RandomUnipolar -> module intensity | 1 | Audible | H (triage) |
| 50 | external modulator Script(0) -> warpFactor | 1 | Audible | B |
| 51 | external modulator Script(0) -> warpFactor2 | 1 | Audible | B |
| 52 | external modulator Script(0) -> wavetableInharmonic | 1 | Audible | B |
| 53 | external modulator Script(0) -> wavetableModAmount | 1 | Audible | B |
| 54 | external modulator Script(0) -> wavetablePosition | 1 | Audible | B |
| 55 | external modulator Velocity -> warpFactor | 1 | Audible | H (triage) |
| 56 | external modulator Velocity -> warpFactor2 | 1 | Audible | H (triage) |
| 57 | external modulator Velocity -> wavetableModAmount | 1 | Audible | H (triage) |
| 58 | external modulator Velocity -> wavetablePosition | 1 | Audible | H (triage) |
| 59 | impulse response size | 1 | Audible | H (triage) |
| 60 | insert CombFilter | 1 | Audible, Latent | H (triage) |
| 61 | insert Drive | 1 | Audible, Latent | H (triage) |
| 62 | insert FeedbackMachine | 1 | Audible, Latent | H (triage) |
| 63 | insert XpanderFilter | 1 | Audible, Latent | H (triage) |
| 64 | internal modulator AHDSR -> module frequency | 1 | Audible | H (triage) |
| 65 | internal modulator AHDSR -> module intensity | 1 | Audible | H (triage) |
| 66 | internal modulator AHDSR -> warpFactor | 1 | Audible | H (triage) |
| 67 | internal modulator AHDSR -> warpFactor2 | 1 | Audible | H (triage) |
| 68 | internal modulator AHDSR -> wavetableInharmonic | 1 | Audible | H (triage) |
| 69 | internal modulator AHDSR -> wavetableModAmount | 1 | Audible | H (triage) |
| 70 | internal modulator AHDSR -> wavetableModFrequency | 1 | Audible | H (triage) |
| 71 | internal modulator AHDSR -> wavetablePosition | 1 | Audible | H (triage) |
| 72 | internal modulator LFO -> module frequency | 1 | Audible | H (triage) |
| 73 | internal modulator LFO -> module intensity | 1 | Audible | H (triage) |
| 74 | internal modulator LFO -> warpFactor | 1 | Audible | H (triage) |
| 75 | internal modulator LFO -> warpFactor2 | 1 | Audible | H (triage) |
| 76 | internal modulator LFO -> wavetableInharmonic | 1 | Audible | H (triage) |
| 77 | internal modulator LFO -> wavetableModAmount | 1 | Audible | H (triage) |
| 78 | internal modulator LFO -> wavetableModFrequency | 1 | Audible | H (triage) |
| 79 | internal modulator LFO -> wavetablePosition | 1 | Audible | H (triage) |
| 80 | modulation Amount | 1 | Audible | H (triage) |
| 81 | modulation Depth | 1 | Audible, Latent | H (triage) |
| 82 | modulation Feedback | 1 | Audible | H (triage) |
| 83 | modulation Freq | 1 | Audible, Latent | H (triage) |
| 84 | modulation Gain | 1 | Audible, Latent | H (triage) |
| 85 | modulation MIDICurve_1 | 1 | Audible | H (triage) |
| 86 | modulation Mix | 1 | Audible, Latent | H (triage) |
| 87 | modulation Pitch | 1 | Latent | H (triage) |
| 88 | modulation Q | 1 | Audible | H (triage) |
| 89 | modulation Speed | 1 | Audible | H (triage) |
| 90 | modulation XpanderDrive_4 | 1 | Audible | H (triage) |
| 91 | script init unanswered: AttachZone | 1 | Unknown | B |
| 92 | script init unanswered: ExposeControls | 1 | Unknown | B |
| 93 | script init unanswered: FindZone | 1 | Unknown | B |
| 94 | script init unanswered: GetEngineParDispExt | 1 | Unknown | B |
| 95 | script init unanswered: GetZonePar | 1 | Unknown | B |
| 96 | script init unanswered: LoadArrayStr | 1 | Unknown | B |
| 97 | script init unanswered: LoadNativeUi | 1 | Unknown | B |
| 98 | script init unanswered: ResetNksNav | 1 | Unknown | B |
| 99 | script init unanswered: SaveArrayStr | 1 | Unknown | B |
| 100 | script init unanswered: SetNksNavName | 1 | Unknown | B |
| 101 | script init unanswered: SetNksNavPar | 1 | Unknown | B |
| 102 | script sample | 1 | Broken | B |
| 103 | silent effect Convolution | 1 | Cosmetic | H (triage) |
| 104 | unassigned external modulator | 1 | Cosmetic | H (triage) |
| 105 | wavetable inharmonic | 1 | Audible | H (triage) |
| 106 | wavetable modulation | 1 | Audible | H (triage) |
| 107 | wavetable random phase | 1 | Audible | H (triage) |

This load-only census reports translator diagnostics, initialization, prepared zones and UI model observations. It does not exercise notes, later callbacks or moving controls, and cannot prove the absence of unreported gaps. Detailed diagnostic messages and affected paths are retained in `scratch/laneG/corpus/summary.json` on the census machine.
