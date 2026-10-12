# Falcon module coverage (UVI corpus)

Every module type the local UVI corpus (660 programs: 620 Augmented Orchestra, 40 VWinds Clarinets,
Double Reeds and Flutes) references. Counts are programs containing the module; parameters are
the attributes the corpus authors actually write. Ranges, defaults and units come from the
Falcon scripting reference catalog (lua.uvi.net, `falcon_api`); behaviour comes from the
Falcon manual. IR status: **yes** (translated and rendered), **approximated** (translated with a
reported difference), **dropped** (not translated; reported as `unsupported`).

Regenerate the counts with the ignored `census_modules` survey in `crates/sampler-uvi/src/lib.rs`.

## VWinds: what is modelled and what is sampled

Measured on the three VWinds libraries only (40 programs; `census_modules` pointed at those
libraries). Per program: about 178 keygroups (7100 over 40 programs), one `OnePole` insert in
most keygroups (6318 total, Freq/KeyTracking/Mode), one `GainMatrix` per program-level rack
(6180 elements, Gain_i_j routing matrix up to 12x12), `DigitalEq` (80), `Convolver` (376,
SamplePath/Dry/Wet), `SampledReverb` (240), `EffectRack` (440 chains), `Gain` inserts (988) and
`BusRouter` sends (7220). There is no CombFilter, XpanderFilter or MS20 in VWinds: those belong
to Augmented Orchestra (620 programs). The script (setParameter, corpus-wide 95909 calls)
drives these modules live from airflow (CC1), vibrato and legato state.

- Sampled: the instrument tone, attacks, release and transition material (SamplePlayer).
- Modelling (all from effect modules, structural reading of the program XML, not yet measured
  against a reference render): the key-tracked one-pole filter (OnePole, manual: 6 dB/octave
  low or high pass) shaping brightness with airflow; the `GainMatrix` crossfading or routing
  layer/oscillator signals under script control; `DigitalEq` and `Convolver` for the body and
  mic character (impulse-response based, so sample-derived rather than synthesized);
  `SampledReverb` for the room.
- Script logic, not DSP: overlap legato, velocity-controlled glide, vibrato modes, MPE
  (see `UVI_SCRIPT_COVERAGE.md`).

Current IR status: OnePole, GainMatrix, DigitalEq, Convolver, SampledReverb, EffectRack and
Gain inserts are all **dropped**, so VWinds renders the raw samples with the script's
volume/tune/pan control. The v1 evidence (`UVI_VWINDS_PERFORMANCE_EVIDENCE`, nine
performance tapes) established that the authored control paths run and render finite PCM; it
made no claim about modelled-transition fidelity or native PCM parity, and this status
repeats that limit.

## Modules

| Module | Container | Programs | IR status | Notes |
|---|---|---:|---|---|
| AuxEffect | Auxs | 660 | approximated | bus is created, its effect chain is dropped |
| AuxEffect | Chains | 660 | approximated | bus is created, its effect chain is dropped |
| BusRouter | BusRouters | 660 | yes | layer sends to aux buses; keygroup routers not translated |
| ConstantModulation | ControlSignalSources | 660 | yes |  |
| Gain | Inserts | 660 | dropped | insert not translated |
| Keygroup | Keygroups | 660 | yes / approximated | keys, velocity, Gain, fades reported; ExclusiveGroup, Polyphony-like fields dropped |
| LFO | ControlSignalSources | 660 | yes / approximated | shared-instance behaviour inferred |
| Layer | Layers | 660 | yes / approximated | Gain, Pan, Mute, keys; Pan uses a linear balance law (UVI: cos2); CustomPolyphony, NumVoicesPerNote, PlayMode, Portamento dropped |
| OnePole | Inserts | 660 | dropped | insert effects are not translated; reported as unsupported modules |
| SamplePlayer | Oscillators | 660 | yes / approximated | zones, loops, tune, gain, pan, key tracking; NoteTracking other than 0 or 1 becomes a middle-of-zone detune; SampleStart, PlayDirection, extra loops reported |
| SampledReverb | Inserts | 660 | dropped | insert effects are not translated; reported as unsupported modules |
| ScriptEventModulation | ControlSignalSources | 660 | yes | driven by sendScriptModulation |
| ThreeBandShelves | Inserts | 660 | dropped | insert effects are not translated; reported as unsupported modules |
| AHD | ControlSignalSources | 620 | dropped | not translated |
| CombFilter | Inserts | 620 | dropped | insert effects are not translated; reported as unsupported modules |
| DAHDSR | ControlSignalSources | 620 | yes / approximated | linear stages instead of the analog law; VelocitySens/VelocityAmount reported |
| DiodeClipper | Inserts | 620 | dropped | insert effects are not translated; reported as unsupported modules |
| Drive | Inserts | 620 | dropped | insert effects are not translated; reported as unsupported modules |
| DualDelay | Inserts | 620 | dropped | insert effects are not translated; reported as unsupported modules |
| FeedbackMachine | Inserts | 620 | dropped | insert effects are not translated; reported as unsupported modules |
| Flanger | Inserts | 620 | dropped | insert effects are not translated; reported as unsupported modules |
| MS20 | Inserts | 620 | dropped | insert effects are not translated; reported as unsupported modules |
| Maximizer | Inserts | 620 | dropped | insert effects are not translated; reported as unsupported modules |
| MultiLFO | ControlSignalSources | 620 | yes |  |
| Phasor | Inserts | 620 | dropped | insert effects are not translated; reported as unsupported modules |
| Program | UVI4 | 620 | approximated | Gain; Polyphony, NotePolyphony, Streaming, Transpose dropped (setParameter Program.Polyphony does not reach the runtime) |
| SparkVerb | Inserts | 620 | dropped | insert effects are not translated; reported as unsupported modules |
| StepEnvelope | ControlSignalSources | 620 | yes |  |
| WaveShaper | Inserts | 620 | dropped | insert effects are not translated; reported as unsupported modules |
| XpanderFilter | Inserts | 620 | dropped | insert effects are not translated; reported as unsupported modules |
| Convolver | Inserts | 40 | dropped | insert effects are not translated; reported as unsupported modules |
| DigitalEq | Inserts | 40 | dropped | insert effects are not translated; reported as unsupported modules |
| EffectRack | Inserts | 40 | dropped | insert effects are not translated; reported as unsupported modules |
| GainMatrix | Inserts | 40 | dropped | insert effects are not translated; reported as unsupported modules |
| Program |  | 40 | approximated | Gain; Polyphony, NotePolyphony, Streaming, Transpose dropped (setParameter Program.Polyphony does not reach the runtime) |
| TrackDelay | Inserts | 40 | dropped | insert effects are not translated; reported as unsupported modules |

## Parameters used by the corpus (catalog ranges)

### AuxEffect (660 programs, approximated)

Bypass [false..true, def false]; Gain [0.000..1.995, def 1.000]; Pan [-1.000..1.000, def 0.000]; PreInsert [false..true, def true]

### AuxEffect (660 programs, approximated)

Bypass [false..true, def false]; Gain [0.000..1.995, def 1.000]; Pan [-1.000..1.000, def 0.000]; PreInsert [false..true, def true]

### BusRouter (660 programs, yes)

BusRouterVersion (not in catalog); Bypass [false..true, def false]; Destination (not in catalog); Gain [0.000..1.995, def 0.000]; PreFader [false..true, def false]

### ConstantModulation (660 programs, yes)

Bipolar [false..true, def false]; Bypass [false..true, def false]; Style [0..1, def 0]; Value [0.000..1.000, def 0.000 % normalized]

### Gain (660 programs, dropped)

Bypass [false..true, def false]; Volume [0.000..3.981, def 1.000]

### Keygroup (660 programs, yes / approximated)

Bypass [false..true, def false]; BypassInsertFX [false..true, def false]; ExclusiveGroup [0..32, def 0]; FXPostGain [false..true, def false]; FadeCurve [1.000..2.000, def 2.000]; Gain [0.000..1.995, def 1.000]; HighKey [0..127, def 127 midi key]; HighKeyFade [0..127, def 0]; HighVelocity [1..127, def 127]; HighVelocityFade [0..127, def 0]; LatchTrigger [false..true, def false]; LowKey [0..127, def 0 midi key]; LowKeyFade [0..127, def 0]; LowVelocity [1..127, def 1]; LowVelocityFade [0..127, def 0]; OutputName (not in catalog); Pan [-1.000..1.000, def 0.000]; TriggerMode [0..4, def 0]; TriggerRule [0..3, def 0]; TriggerSync [0..2, def 0]

### LFO (660 programs, yes / approximated)

Bipolar [false..true, def true]; Bypass [false..true, def false]; DelayTime [0.000..10.000, def 0.000 second]; Depth [0.000..1.000, def 1.000 % normalized]; Freq [0.000..20.000, def 0.500 Hz (beat ratio when synced: 4 is 1/1, 0.25 is 1/16, ...)]; Phase [0.000..1.000, def 0.000]; Retrigger [0..3, def 1]; RiseTime [0.000..10.000, def 0.000 second]; Smooth [0.000..1.000, def 0.000 second]; SyncToHost [false..true, def false]; WaveFormType [0..9, def 0]

### Layer (660 programs, yes / approximated)

Bypass [false..true, def false]; BypassInsertFX [false..true, def false]; CustomPolyphony [0..256, def 0]; Gain [0.000..1.995, def 1.000]; HighKey [0..127, def 127 midi key]; LowKey [0..127, def 0 midi key]; MidiMute [false..true, def false]; Mute [false..true, def false]; NumVoicesPerNote [1..256, def 1]; OutputName (not in catalog); Pan [-1.000..1.000, def 0.000]; PlayMode [0..4, def 0]; PortamentoCurve [-1.000..1.000, def 0.000 % normalized]; PortamentoMode [0..1, def 0]; PortamentoTime [0.000..10.000, def 0.030 second]; Solo [false..true, def false]; VelocityCurve [-1.000..127.000, def 0.000]

### OnePole (660 programs, dropped)

Bypass [false..true, def false]; Freq [20.000..20000.000, def 1000.000 Hz]; KeyTracking [0.000..1.000, def 0.000 % normalized]; Mode [0..1, def 0]

### SamplePlayer (660 programs, yes / approximated)

AllowStreaming [false..true, def true]; BaseNote [0..127, def 60 midi key]; Bypass [false..true, def false]; CoarseTune [-24..24, def 0 semitones]; FineTune [-100..100, def 0 cents]; Gain [0.000..1.995, def 1.000]; InterpolationMode [0..2, def 1]; NoteTracking [-2.000..2.000, def 1.000 % normalized]; Pitch [1.000..0.000, def 0.000 semitones]; Reverse [false..true, def false]; SamplePath (not in catalog); SamplePurged (not in catalog); SampleStart [0.000..1.000, def 0.000 % normalized]

### SampledReverb (660 programs, dropped)

Bypass [false..true, def false]; DampingHigh [-1.000..1.000, def 0.000 % normalized]; DampingLow [-1.000..1.000, def 0.000 % normalized]; Dry [0.000..1.000, def 0.500 % normalized]; NormalizePower [false..true, def true]; PreDelay [0.000..100.000, def 50.000 millisecond]; SamplePath (not in catalog); SampledReverbVersion (not in catalog); Time [0.100..1.000, def 1.000 % normalized]; UseWindowAtOriginalSize (not in catalog); Wet [0.000..1.000, def 0.500 % normalized]; Width [-1.000..1.000, def 0.000 % normalized]

### ScriptEventModulation (660 programs, yes)

Bipolar [false..true, def true]; Bypass [false..true, def false]; EventId [0..127, def 0]

### ThreeBandShelves (660 programs, dropped)

Bypass [false..true, def false]; FreqLowMid [20.000..1000.000, def 200.000 Hz]; FreqMidHigh [2000.000..20000.000, def 4000.000 Hz]; GainHigh [-24.000..24.000, def 0.000 decibel]; GainLow [-24.000..24.000, def 0.000 decibel]; GainMid [-24.000..24.000, def 0.000 decibel]

### AHD (620 programs, dropped)

AttackCurve [-1.000..1.000, def 0.000]; AttackTime [0.000..10.000, def 0.000 second]; Bypass [false..true, def false]; DecayCurve [-1.000..1.000, def 0.000]; DecayTime [0.000..30.000, def 0.100 second]; HoldTime [0.000..10.000, def 1.000 second]; NoteOffRetrigger [false..true, def false]; Retrigger [0..2, def 1]; VelocityAmount [0.000..1.000, def 0.000 % normalized]; VelocitySens [-1.000..1.000, def 0.750 % normalized]

### CombFilter (620 programs, dropped)

Bypass [false..true, def false]; Freq [20.000..20000.000, def 1000.000 Hz]; KeyTracking [0.000..1.000, def 0.000 % normalized]; Mode [0..1, def 0]; Q [0.000..1.000, def 0.000 % normalized]

### DAHDSR (620 programs, yes / approximated)

AttackCurve [-1.000..1.000, def 0.000]; AttackTime [0.000..10.000, def 0.000 second]; Bypass [false..true, def false]; DecayCurve [-1.000..1.000, def 0.000]; DecayTime [0.000..30.000, def 0.000 second]; DelayTime [0.000..10.000, def 0.000 second]; HoldTime [0.000..10.000, def 0.000 second]; NoteOffRetrigger [false..true, def false]; ReleaseCurve [-1.000..1.000, def 0.000]; ReleaseTime [0.000..20.000, def 0.050 second]; Retrigger [0..2, def 1]; SustainLevel [0.000..1.000, def 1.000 % normalized]; VelocityAmount [0.000..1.000, def 0.000 % normalized]; VelocitySens [-1.000..1.000, def 0.750 % normalized]

### DiodeClipper (620 programs, dropped)

Asymmetry [0.000..1.000, def 0.000 % normalized]; Bypass [false..true, def false]; Drive [0.000..30.000, def 0.000 decibel]; HighPass [1.000..20000.000, def 1.000 Hz]; OutputGain [-40.000..10.000, def 0.000 decibel]; Tone [100.000..20000.000, def 20000.000 Hz]

### Drive (620 programs, dropped)

Bypass [false..true, def false]; DriveAmount [0.000..1.000, def 0.000 % normalized]; Mode [0..2, def 0]; Oversampling [0..4, def 0]

### DualDelay (620 programs, dropped)

Bypass [false..true, def false]; DelayRatio [-0.900..0.900, def 0.000]; DelayTime [0.001..5.000, def 0.125 second (beat ratio when synced: 4 is 1/1, 0.25 is 1/16, ...)]; DualDelayVersion (not in catalog); Feedback [0.000..1.000, def 0.300 % normalized]; FeedbackRatio [-0.900..0.900, def 0.000]; HighCut [1000.000..20000.000, def 20000.000 Hz]; InputRotation [-1.000..1.000, def 0.000]; InputWidth [0.000..1.000, def 1.000 % normalized]; Interpolation [0..2, def 1]; LowCut [20.000..4000.000, def 20.000 Hz]; Mix [0.000..1.000, def 0.500 % normalized]; ModChannelOffset [0.000..1.000, def 1.000 % normalized]; ModDepth [0.000..20.000, def 0.000 millisecond]; ModRate [0.100..10.000, def 1.000 Hz]; OutputRotation [-1.000..1.000, def 0.000]; OutputWidth [0.000..1.000, def 1.000 % normalized]; PeakFreq [20.000..20000.000, def 1000.000 Hz]; PeakGain [-20.000..20.000, def 0.000 decibel]; PeakQ [0.100..10.000, def 1.000]; Rotation [-180.000..180.000, def 0.000]; SyncToHost [false..true, def false]

### FeedbackMachine (620 programs, dropped)

Bypass [false..true, def false]; DelayTime [0.002..1.000, def 0.005 second]; Feedback [0.000..1.000, def 0.500 % normalized]; Mix [0.000..1.000, def 0.500 % normalized]

### Flanger (620 programs, dropped)

Bypass [false..true, def false]; DelayTime [0.000..1.000, def 0.200 second]; Depth [0.000..1.000, def 0.080 % normalized]; Feedback [0.000..1.000, def 0.000 % normalized]; Mix [0.000..1.000, def 1.000 % normalized]; Speed [0.010..10.000, def 0.800 Hz (beat ratio when synced: 4 is 1/1, 0.25 is 1/16, ...)]; SyncToHost [false..true, def false]

### MS20 (620 programs, dropped)

Bypass [false..true, def false]; Freq [20.000..20000.000, def 1000.000 Hz]; KeyTracking [0.000..1.000, def 0.000 % normalized]; Morph [0.000..1.000, def 0.000 % normalized]; Q [0.000..1.000, def 0.000 % normalized]; ReferenceVoltage [0.500..5.000, def 1.000]; ResonanceTrim [0.000..1.000, def 0.500 % normalized]

### Maximizer (620 programs, dropped)

Attack [0.000..20.000, def 0.000 millisecond]; Bypass [false..true, def false]; Ceiling [-20.000..0.000, def -0.100 decibel]; Knee [0.000..10.000, def 0.000 decibel]; Lookahead [1.000..20.000, def 2.000 millisecond]; Release [0.100..1000.000, def 10.000 millisecond]; ReleaseBlend [0.000..1.000, def 0.100 % normalized]; SlewRate [0.000..200.000, def 20.000 millisecond]; Threshold [-40.000..0.000, def -6.000 decibel]

### MultiLFO (620 programs, yes)

Bipolar [false..true, def true]; Bypass [false..true, def false]; Depth [0.000..1.000, def 1.000 % normalized]; Freq [0.000..20.000, def 0.500 Hz (beat ratio when synced: 4 is 1/1, 0.25 is 1/16, ...)]; Invert [false..true, def false]; NoiseDepth [-1.000..1.000, def 0.000 % normalized]; NormalizeOutput [false..true, def true]; Phase [0.000..1.000, def 0.000]; PulseWidth [0.050..0.950, def 0.500 % normalized]; Retrigger [0..3, def 1]; RiseTime [0.000..10.000, def 0.000 second]; SawDepth [-1.000..1.000, def 0.000 % normalized]; SineDepth [-1.000..1.000, def 0.000 % normalized]; Smooth [0.000..1.000, def 0.000 second]; SquareDepth [-1.000..1.000, def 0.000 % normalized]; SyncToHost [false..true, def false]; TriangleDepth [-1.000..1.000, def 0.000 % normalized]

### Phasor (620 programs, dropped)

Bypass [false..true, def false]; Depth [0.000..1.000, def 1.000 % normalized]; Feedback [-0.990..0.990, def 0.700 % normalized]; LFOSHape [0..3, def 0]; MaxFreq [20.000..20000.000, def 3000.000 Hz]; MinFreq [20.000..20000.000, def 200.000 Hz]; Order [1..12, def 3]; Speed [0.010..10.000, def 0.300 Hz (beat ratio when synced: 4 is 1/1, 0.25 is 1/16, ...)]; Spread [0.000..1.000, def 1.000 % normalized]; SyncToHost [false..true, def false]

### Program (620 programs, approximated)

Bypass [false..true, def false]; BypassInsertFX [false..true, def false]; Gain [0.000..1.995, def 1.000]; LoopProgram [false..true, def false]; NotePolyphony [0..256, def 0]; OutputName (not in catalog); Pan [-1.000..1.000, def 0.000]; Polyphony [1..256, def 16]; ProgramPath (not in catalog); Streaming [false..true, def true]; TransposeOctaves [-2..2, def 0]; TransposeSemiTones [-24..24, def 0]

### SparkVerb (620 programs, dropped)

Bypass [false..true, def false]; DecayHigh [0.100..10.000, def 1.000]; DecayLow [0.100..10.000, def 1.000]; DecayTime [0.100..10.000, def 1.000]; Diffusion [0.000..1.000, def 0.618 % normalized]; DiffusionOnOff [false..true, def false]; DiffusionStart [1.000..10.000, def 5.000 millisecond]; FreqHigh [2000.000..20000.000, def 12000.000 Hz]; FreqLow [10.000..1000.000, def 250.000 Hz]; HiCut [false..true, def false]; LowCut [false..true, def false]; Mix [0.000..1.000, def 0.500 % normalized]; MixMode [0..1, def 1]; ModDepth [0.000..20.000, def 4.000 cents]; ModRate [0.250..4.000, def 1.000]; Mode [0..2, def 1]; PreDelay [0.000..100.000, def 0.000 millisecond]; Quality [2..4, def 3]; Rolloff [2000.000..20000.000, def 20000.000 Hz]; RoomSize [4.000..50.000, def 20.000]; Shape [0.000..1.000, def 0.000 % normalized]; SparkVerbVersion (not in catalog); Width [0.000..1.000, def 1.000 % normalized]

### StepEnvelope (620 programs, yes)

Bipolar [false..true, def false]; Bypass [false..true, def false]; Freq [0.000..20.000, def 1.000 Hz (beat ratio when synced: 4 is 1/1, 0.25 is 1/16, ...)]; InterpolationMode [0..1, def 0]; Levels (not in catalog); NumSteps [1..128, def 16]; Retrigger [0..2, def 0]; Smooth [0.000..1.000, def 0.000 second]; SyncToHost [false..true, def false]

### WaveShaper (620 programs, dropped)

Amount [0.000..1.000, def 0.000 % normalized]; Bypass [false..true, def false]; InputGain [-40.000..40.000, def 0.000 decibel]; Knee [-10.000..10.000, def 0.000]; Mix [0.000..1.000, def 1.000 % normalized]; Mode [0..11, def 0]; OutputGain [-40.000..40.000, def 0.000 decibel]; Oversampling [0..4, def 0]; PostFreq [2.000..20000.000, def 20.000 Hz]; PreFreq [20.000..22000.000, def 20000.000 Hz]

### XpanderFilter (620 programs, dropped)

Algorithm [0..1, def 0]; Bypass [false..true, def false]; DistortionType [0..2, def 0]; Drive [-20.000..20.000, def 0.000 decibel]; Fat [0.000..1.000, def 1.000 % normalized]; Freq [20.000..20000.000, def 1000.000 Hz]; KeyTracking [0.000..1.000, def 0.000 % normalized]; Mode [0..36, def 3]; Oversampling [0..1, def 1]; Q [0.000..1.000, def 0.000 % normalized]

### Convolver (40 programs, dropped)

Bypass [false..true, def false]; ConvolverVersion (not in catalog); Dry [0.000..1.000, def 0.000 % normalized]; NormalizePower [false..true, def true]; SamplePath (not in catalog); Wet [0.000..1.000, def 1.000 % normalized]

### DigitalEq (40 programs, dropped)

Bandwidth1 [0.010..10.000, def 1.000]; Bandwidth10 [0.010..10.000, def 1.000]; Bandwidth11 [0.010..10.000, def 1.000]; Bandwidth12 [0.010..10.000, def 1.000]; Bandwidth13 [0.010..10.000, def 1.000]; Bandwidth14 [0.010..10.000, def 1.000]; Bandwidth15 [0.010..10.000, def 1.000]; Bandwidth16 [0.010..10.000, def 1.000]; Bandwidth2 [0.010..10.000, def 1.000]; Bandwidth3 [0.010..10.000, def 1.000]; Bandwidth4 [0.010..10.000, def 1.000]; Bandwidth5 [0.010..10.000, def 1.000]; Bandwidth6 [0.010..10.000, def 1.000]; Bandwidth7 [0.010..10.000, def 1.000]; Bandwidth8 [0.010..10.000, def 1.000]; Bandwidth9 [0.010..10.000, def 1.000]; Bypass [false..true, def false]; Channels1 [0..2, def 0]; Channels10 [0..2, def 0]; Channels11 [0..2, def 0]; Channels12 [0..2, def 0]; Channels13 [0..2, def 0]; Channels14 [0..2, def 0]; Channels15 [0..2, def 0]; Channels16 [0..2, def 0]; Channels2 [0..2, def 0]; Channels3 [0..2, def 0]; Channels4 [0..2, def 0]; Channels5 [0..2, def 0]; Channels6 [0..2, def 0]; Channels7 [0..2, def 0]; Channels8 [0..2, def 0]; Channels9 [0..2, def 0]; Enabled1 [false..true, def true]; Enabled10 [false..true, def true]; Enabled11 [false..true, def true]; Enabled12 [false..true, def true]; Enabled13 [false..true, def true]; Enabled14 [false..true, def true]; Enabled15 [false..true, def true]; Enabled16 [false..true, def true]; Enabled2 [false..true, def true]; Enabled3 [false..true, def true]; Enabled4 [false..true, def true]; Enabled5 [false..true, def true]; Enabled6 [false..true, def true]; Enabled7 [false..true, def true]; Enabled8 [false..true, def true]; Enabled9 [false..true, def true]; Freq1 [10.000..22000.000, def 1000.000 Hz]; Freq10 [10.000..22000.000, def 1000.000 Hz]; Freq11 [10.000..22000.000, def 1000.000 Hz]; Freq12 [10.000..22000.000, def 1000.000 Hz]; Freq13 [10.000..22000.000, def 1000.000 Hz]; Freq14 [10.000..22000.000, def 1000.000 Hz]; Freq15 [10.000..22000.000, def 1000.000 Hz]; Freq16 [10.000..22000.000, def 1000.000 Hz]; Freq2 [10.000..22000.000, def 1000.000 Hz]; Freq3 [10.000..22000.000, def 1000.000 Hz]; Freq4 [10.000..22000.000, def 1000.000 Hz]; Freq5 [10.000..22000.000, def 1000.000 Hz]; Freq6 [10.000..22000.000, def 1000.000 Hz]; Freq7 [10.000..22000.000, def 1000.000 Hz]; Freq8 [10.000..22000.000, def 1000.000 Hz]; Freq9 [10.000..22000.000, def 1000.000 Hz]; Gain1 [-30.000..30.000, def 0.000 decibel]; Gain10 [-30.000..30.000, def 0.000 decibel]; Gain11 [-30.000..30.000, def 0.000 decibel]; Gain12 [-30.000..30.000, def 0.000 decibel]; Gain13 [-30.000..30.000, def 0.000 decibel]; Gain14 [-30.000..30.000, def 0.000 decibel]; Gain15 [-30.000..30.000, def 0.000 decibel]; Gain16 [-30.000..30.000, def 0.000 decibel]; Gain2 [-30.000..30.000, def 0.000 decibel]; Gain3 [-30.000..30.000, def 0.000 decibel]; Gain4 [-30.000..30.000, def 0.000 decibel]; Gain5 [-30.000..30.000, def 0.000 decibel]; Gain6 [-30.000..30.000, def 0.000 decibel]; Gain7 [-30.000..30.000, def 0.000 decibel]; Gain8 [-30.000..30.000, def 0.000 decibel]; Gain9 [-30.000..30.000, def 0.000 decibel]; GainScale [-2.000..2.000, def 1.000 % normalized]; KeyTracking [-1.000..1.000, def 0.000 % normalized]; OverallGain [-30.000..30.000, def 0.000 decibel]; Q1 [0.018..28.284, def 0.707]; Q10 [0.018..28.284, def 0.707]; Q11 [0.018..28.284, def 0.707]; Q12 [0.018..28.284, def 0.707]; Q13 [0.018..28.284, def 0.707]; Q14 [0.018..28.284, def 0.707]; Q15 [0.018..28.284, def 0.707]; Q16 [0.018..28.284, def 0.707]; Q2 [0.018..28.284, def 0.707]; Q3 [0.018..28.284, def 0.707]; Q4 [0.018..28.284, def 0.707]; Q5 [0.018..28.284, def 0.707]; Q6 [0.018..28.284, def 0.707]; Q7 [0.018..28.284, def 0.707]; Q8 [0.018..28.284, def 0.707]; Q9 [0.018..28.284, def 0.707]; Slope1 [0..7, def 1]; Slope10 [0..7, def 1]; Slope11 [0..7, def 1]; Slope12 [0..7, def 1]; Slope13 [0..7, def 1]; Slope14 [0..7, def 1]; Slope15 [0..7, def 1]; Slope16 [0..7, def 1]; Slope2 [0..7, def 1]; Slope3 [0..7, def 1]; Slope4 [0..7, def 1]; Slope5 [0..7, def 1]; Slope6 [0..7, def 1]; Slope7 [0..7, def 1]; Slope8 [0..7, def 1]; Slope9 [0..7, def 1]; StereoMode [0..1, def 0]; Transpose [-10.000..10.000, def 0.000]; Type1 [0..6, def 0]; Type10 [0..6, def 0]; Type11 [0..6, def 0]; Type12 [0..6, def 0]; Type13 [0..6, def 0]; Type14 [0..6, def 0]; Type15 [0..6, def 0]; Type16 [0..6, def 0]; Type2 [0..6, def 0]; Type3 [0..6, def 0]; Type4 [0..6, def 0]; Type5 [0..6, def 0]; Type6 [0..6, def 0]; Type7 [0..6, def 0]; Type8 [0..6, def 0]; Type9 [0..6, def 0]; Visible1 (not in catalog); Visible10 (not in catalog); Visible11 (not in catalog); Visible12 (not in catalog); Visible13 (not in catalog); Visible14 (not in catalog); Visible15 (not in catalog); Visible16 (not in catalog); Visible2 (not in catalog); Visible3 (not in catalog); Visible4 (not in catalog); Visible5 (not in catalog); Visible6 (not in catalog); Visible7 (not in catalog); Visible8 (not in catalog); Visible9 (not in catalog)

### EffectRack (40 programs, dropped)

Bypass [false..true, def false]

### GainMatrix (40 programs, dropped)

Bypass [false..true, def false]; Gain_10_1 [-1.000..1.000, def 0.000]; Gain_10_10 [-1.000..1.000, def 1.000]; Gain_10_11 [-1.000..1.000, def 0.000]; Gain_10_12 [-1.000..1.000, def 0.000]; Gain_10_2 [-1.000..1.000, def 0.000]; Gain_10_3 [-1.000..1.000, def 0.000]; Gain_10_4 [-1.000..1.000, def 0.000]; Gain_10_5 [-1.000..1.000, def 0.000]; Gain_10_6 [-1.000..1.000, def 0.000]; Gain_10_7 [-1.000..1.000, def 0.000]; Gain_10_8 [-1.000..1.000, def 0.000]; Gain_10_9 [-1.000..1.000, def 0.000]; Gain_11_1 [-1.000..1.000, def 0.000]; Gain_11_10 [-1.000..1.000, def 0.000]; Gain_11_11 [-1.000..1.000, def 1.000]; Gain_11_12 [-1.000..1.000, def 0.000]; Gain_11_2 [-1.000..1.000, def 0.000]; Gain_11_3 [-1.000..1.000, def 0.000]; Gain_11_4 [-1.000..1.000, def 0.000]; Gain_11_5 [-1.000..1.000, def 0.000]; Gain_11_6 [-1.000..1.000, def 0.000]; Gain_11_7 [-1.000..1.000, def 0.000]; Gain_11_8 [-1.000..1.000, def 0.000]; Gain_11_9 [-1.000..1.000, def 0.000]; Gain_12_1 [-1.000..1.000, def 0.000]; Gain_12_10 [-1.000..1.000, def 0.000]; Gain_12_11 [-1.000..1.000, def 0.000]; Gain_12_12 [-1.000..1.000, def 1.000]; Gain_12_2 [-1.000..1.000, def 0.000]; Gain_12_3 [-1.000..1.000, def 0.000]; Gain_12_4 [-1.000..1.000, def 0.000]; Gain_12_5 [-1.000..1.000, def 0.000]; Gain_12_6 [-1.000..1.000, def 0.000]; Gain_12_7 [-1.000..1.000, def 0.000]; Gain_12_8 [-1.000..1.000, def 0.000]; Gain_12_9 [-1.000..1.000, def 0.000]; Gain_1_1 [-1.000..1.000, def 1.000]; Gain_1_10 [-1.000..1.000, def 0.000]; Gain_1_11 [-1.000..1.000, def 0.000]; Gain_1_12 [-1.000..1.000, def 0.000]; Gain_1_2 [-1.000..1.000, def 0.000]; Gain_1_3 [-1.000..1.000, def 0.000]; Gain_1_4 [-1.000..1.000, def 0.000]; Gain_1_5 [-1.000..1.000, def 0.000]; Gain_1_6 [-1.000..1.000, def 0.000]; Gain_1_7 [-1.000..1.000, def 0.000]; Gain_1_8 [-1.000..1.000, def 0.000]; Gain_1_9 [-1.000..1.000, def 0.000]; Gain_2_1 [-1.000..1.000, def 0.000]; Gain_2_10 [-1.000..1.000, def 0.000]; Gain_2_11 [-1.000..1.000, def 0.000]; Gain_2_12 [-1.000..1.000, def 0.000]; Gain_2_2 [-1.000..1.000, def 1.000]; Gain_2_3 [-1.000..1.000, def 0.000]; Gain_2_4 [-1.000..1.000, def 0.000]; Gain_2_5 [-1.000..1.000, def 0.000]; Gain_2_6 [-1.000..1.000, def 0.000]; Gain_2_7 [-1.000..1.000, def 0.000]; Gain_2_8 [-1.000..1.000, def 0.000]; Gain_2_9 [-1.000..1.000, def 0.000]; Gain_3_1 [-1.000..1.000, def 0.000]; Gain_3_10 [-1.000..1.000, def 0.000]; Gain_3_11 [-1.000..1.000, def 0.000]; Gain_3_12 [-1.000..1.000, def 0.000]; Gain_3_2 [-1.000..1.000, def 0.000]; Gain_3_3 [-1.000..1.000, def 1.000]; Gain_3_4 [-1.000..1.000, def 0.000]; Gain_3_5 [-1.000..1.000, def 0.000]; Gain_3_6 [-1.000..1.000, def 0.000]; Gain_3_7 [-1.000..1.000, def 0.000]; Gain_3_8 [-1.000..1.000, def 0.000]; Gain_3_9 [-1.000..1.000, def 0.000]; Gain_4_1 [-1.000..1.000, def 0.000]; Gain_4_10 [-1.000..1.000, def 0.000]; Gain_4_11 [-1.000..1.000, def 0.000]; Gain_4_12 [-1.000..1.000, def 0.000]; Gain_4_2 [-1.000..1.000, def 0.000]; Gain_4_3 [-1.000..1.000, def 0.000]; Gain_4_4 [-1.000..1.000, def 1.000]; Gain_4_5 [-1.000..1.000, def 0.000]; Gain_4_6 [-1.000..1.000, def 0.000]; Gain_4_7 [-1.000..1.000, def 0.000]; Gain_4_8 [-1.000..1.000, def 0.000]; Gain_4_9 [-1.000..1.000, def 0.000]; Gain_5_1 [-1.000..1.000, def 0.000]; Gain_5_10 [-1.000..1.000, def 0.000]; Gain_5_11 [-1.000..1.000, def 0.000]; Gain_5_12 [-1.000..1.000, def 0.000]; Gain_5_2 [-1.000..1.000, def 0.000]; Gain_5_3 [-1.000..1.000, def 0.000]; Gain_5_4 [-1.000..1.000, def 0.000]; Gain_5_5 [-1.000..1.000, def 1.000]; Gain_5_6 [-1.000..1.000, def 0.000]; Gain_5_7 [-1.000..1.000, def 0.000]; Gain_5_8 [-1.000..1.000, def 0.000]; Gain_5_9 [-1.000..1.000, def 0.000]; Gain_6_1 [-1.000..1.000, def 0.000]; Gain_6_10 [-1.000..1.000, def 0.000]; Gain_6_11 [-1.000..1.000, def 0.000]; Gain_6_12 [-1.000..1.000, def 0.000]; Gain_6_2 [-1.000..1.000, def 0.000]; Gain_6_3 [-1.000..1.000, def 0.000]; Gain_6_4 [-1.000..1.000, def 0.000]; Gain_6_5 [-1.000..1.000, def 0.000]; Gain_6_6 [-1.000..1.000, def 1.000]; Gain_6_7 [-1.000..1.000, def 0.000]; Gain_6_8 [-1.000..1.000, def 0.000]; Gain_6_9 [-1.000..1.000, def 0.000]; Gain_7_1 [-1.000..1.000, def 0.000]; Gain_7_10 [-1.000..1.000, def 0.000]; Gain_7_11 [-1.000..1.000, def 0.000]; Gain_7_12 [-1.000..1.000, def 0.000]; Gain_7_2 [-1.000..1.000, def 0.000]; Gain_7_3 [-1.000..1.000, def 0.000]; Gain_7_4 [-1.000..1.000, def 0.000]; Gain_7_5 [-1.000..1.000, def 0.000]; Gain_7_6 [-1.000..1.000, def 0.000]; Gain_7_7 [-1.000..1.000, def 1.000]; Gain_7_8 [-1.000..1.000, def 0.000]; Gain_7_9 [-1.000..1.000, def 0.000]; Gain_8_1 [-1.000..1.000, def 0.000]; Gain_8_10 [-1.000..1.000, def 0.000]; Gain_8_11 [-1.000..1.000, def 0.000]; Gain_8_12 [-1.000..1.000, def 0.000]; Gain_8_2 [-1.000..1.000, def 0.000]; Gain_8_3 [-1.000..1.000, def 0.000]; Gain_8_4 [-1.000..1.000, def 0.000]; Gain_8_5 [-1.000..1.000, def 0.000]; Gain_8_6 [-1.000..1.000, def 0.000]; Gain_8_7 [-1.000..1.000, def 0.000]; Gain_8_8 [-1.000..1.000, def 1.000]; Gain_8_9 [-1.000..1.000, def 0.000]; Gain_9_1 [-1.000..1.000, def 0.000]; Gain_9_10 [-1.000..1.000, def 0.000]; Gain_9_11 [-1.000..1.000, def 0.000]; Gain_9_12 [-1.000..1.000, def 0.000]; Gain_9_2 [-1.000..1.000, def 0.000]; Gain_9_3 [-1.000..1.000, def 0.000]; Gain_9_4 [-1.000..1.000, def 0.000]; Gain_9_5 [-1.000..1.000, def 0.000]; Gain_9_6 [-1.000..1.000, def 0.000]; Gain_9_7 [-1.000..1.000, def 0.000]; Gain_9_8 [-1.000..1.000, def 0.000]; Gain_9_9 [-1.000..1.000, def 1.000]

### Program (40 programs, approximated)

Bypass [false..true, def false]; BypassInsertFX [false..true, def false]; Gain [0.000..1.995, def 1.000]; LoopProgram [false..true, def false]; NotePolyphony [0..256, def 0]; OutputName (not in catalog); Pan [-1.000..1.000, def 0.000]; Polyphony [1..256, def 16]; ProgramPath (not in catalog); Streaming [false..true, def true]; TransposeOctaves [-2..2, def 0]; TransposeSemiTones [-24..24, def 0]

### TrackDelay (40 programs, dropped)

Bypass [false..true, def false]; DelayTime [0.000..5.000, def 0.000 second (beat ratio when synced: 4 is 1/1, 0.25 is 1/16, ...)]; SyncToHost [false..true, def false]

