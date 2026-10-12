# UVI Workstation DSP effect catalogue

Checked 2026-10-09. The pinned **Workstation 4.0.9** registry has 176 registration records. This list covers its **115 FX/Legacy candidate registrations**, matched to the public UVI type/parameter catalogue: 83 FX and 32 Legacy types. The count includes three analysers and processing containers; it is not a claim of 115 sound-changing effects exposed in Workstation's UI. The other 61 registrations cover engine structures, modulation, event processing, sample/oscillator sources and internal infrastructure. Falcon is absent and no Falcon binary is used as proof.

Every entry has **C** evidence: registration/callback/constructor traces in Workstation's loaded code. Nearby string candidates can include aliases or the preceding registration; the selected type association is a static candidate. Shared callbacks/constructors do not prove shared sound, and owner table slot `+0x1f8` is an observed dispatch candidate, not a universally validated audio ABI. Workstation UI availability, live instantiation and full public/saved parameter conversion remain unverified. **K** marks only the specifically executed kernel described in the [kernel index](DSP_EFFECT_CATALOGUE.md#verified-processing-details).

The processing contracts below are engineering interpretations of type/control identities except for explicit native observations and K checks. The UVI API documents a shared interface; an API-only description is not proof of Workstation implementation. The registry trace supplies separate local evidence.

Binary SHA-256 `78729e96b752aea746280275072ad24cb4399a053739c49a161ff1fcfbf85721`; loaded `.text` SHA-256 `684a5557efed9e426a62cbba75c15cc727d764680e1660164b7c5ae88698fda1`; image base `0x140000000`. These addresses do not apply to the separate installed Workstation VST3. Exact trace locations and all typed parameter rows (range/default/unit/modulability) are in the tracked [manifest](research/dsp-effects-20261009/catalogue-evidence.json). Raw proof: [compiled structure](../artifacts/engine-analysis-2026-10-07/uvi-workstation/compiled-module-structure.json), [complete pseudocode](../artifacts/engine-analysis-2026-10-07/uvi-workstation/complete-export/pseudocode.c), [pointer-export pseudocode](../artifacts/engine-analysis-2026-10-07/uvi-workstation/pointer-export/pseudocode.c).

Control IDs below omit common gain/bypass/routing controls and abbreviate large repeated arrays; the manifest retains every parameter row. A saved tag match proves presence in the embedded factory-preset corpus, not successful execution of that preset.

The live shared UVI API now lists ten additional FX types with no match in the retained Workstation registry. See the [source audit](research/dsp-effects-20261009/README.md#live-uvi-interface-cross-check); those public interfaces are not counted as native Workstation implementations here.

## FX

### 3 Band Compressor — `ThreeBandComp`

Split-band dynamics; three threshold/ratio/timing/makeup/mix states and two crossovers.

Control IDs: `LowMidFreq`, `MidHighFreq`, `Solo0`, `Threshold0`, `Ratio0`, `Attack0`, `Release0`, `Makeup0`, `Mix0`, `AutoMakeup0`, `corresponding band 1/2 controls`.

C: registration `0x140e7d338` → callback `0x140e871a0` → constructor `0x14127b570` (`complete-export/pseudocode.c`, C line 6811472, completed_with_warnings). Owner dispatch candidate `0x14127e310` (`complete-export/pseudocode.c`, line 6809951, completed_with_warnings). Embedded saved tag: `ThreeBandComp`. [UVI parameter interface](https://lua.uvi.net/_elements.html#ThreeBandComp).

### 3 Band Limiter — `ThreeBandLimiter`

Split-band ceiling control; independent thresholds, margins, release and mixing with two crossovers.

Control IDs: `LowMidFreq`, `MidHighFreq`, `Threshold0`, `Margin0`, `Release0`, `Mix0`, `corresponding band 1/2 controls`.

C: registration `0x140e7d3e2` → callback `0x140e88fe0` → constructor `0x14127fb20` (`complete-export/pseudocode.c`, C line 6812385, completed_with_warnings). Owner dispatch candidate `0x14158b250` (`complete-export/pseudocode.c`, line 7742065, completed). Embedded saved tag: `ThreeBandLimiter`. [UVI parameter interface](https://lua.uvi.net/_elements.html#ThreeBandLimiter).

### 3 Band Shelf — `ThreeBandShelves`

Three spectral gain regions with independently positioned low/mid and mid/high boundaries.

Control IDs: `FreqLowMid`, `FreqMidHigh`, `GainLow`, `GainMid`, `GainHigh`.

C: registration `0x140e7bccb` → callback `0x140e89090` → constructor `0x141292970` (`complete-export/pseudocode.c`, C line 6829218, completed_with_warnings). Owner dispatch candidate `0x141293cf0` (`complete-export/pseudocode.c`, line 6827174, completed_with_warnings). [UVI parameter interface](https://lua.uvi.net/_elements.html#ThreeBandShelves).

### Analog Chorus — `AnalogChorus`

Modulation voices controlled by speed, width, intensity and transient enhancement; model-specific interpolation and voicing remain unresolved.

Control IDs: `Speed`, `Width`, `Intensity`, `TransientEnhancement`.

C: registration `0x140e79e29` → callback `0x140e848e0` → constructor `0x1412e2500` (`complete-export/pseudocode.c`, C line 6891517, completed). Owner dispatch candidate `0x1412e2f20` (`pointer-export/pseudocode.c`, line 168811, completed_with_warnings). [UVI parameter interface](https://lua.uvi.net/_elements.html#AnalogChorus).

### Analog Crunch — `GarageFuzz`

Nonlinear overdrive/trash path with gain, mix and oversampling; retain its own curve rather than another fuzz model.

Control IDs: `InputGain`, `Overdrive`, `Trash`, `Mix`, `OutputGain`, `Oversampling`.

C: registration `0x140e7c969` → callback `0x140e86250` → constructor `0x14125d660` (`complete-export/pseudocode.c`, C line 6791278, completed_with_warnings). Owner dispatch candidate `0x14125ec00` (`pointer-export/pseudocode.c`, line 167991, completed_with_warnings). Embedded saved tag: `GarageFuzz`. [UVI parameter interface](https://lua.uvi.net/_elements.html#GarageFuzz).

### Analog Filter — `Retro`

Mode-selectable resonant filter with key tracking; mode and frequency/Q laws remain unresolved.

Control IDs: `Freq`, `Q`, `KeyTracking`, `Mode`.

C: registration `0x140e7b279` → callback `0x140e87d50` → constructor `0x1410fe8b0` (`complete-export/pseudocode.c`, C line 6469817, completed_with_warnings). Owner dispatch candidate `0x1410ff860` (`complete-export/pseudocode.c`, line 6466747, completed_with_warnings). Embedded saved tag: `Retro`. [UVI parameter interface](https://lua.uvi.net/_elements.html#Retro).

### Analog Flanger — `AnalogFlanger`

Speed/width/intensity modulation with transient enhancement; shares a traced constructor with AnalogChorus, without proven mode-selection semantics.

Control IDs: `Speed`, `Width`, `Intensity`, `TransientEnhancement`.

C: registration `0x140e79f25` → callback `0x140e848e0` → constructor `0x1412e2500` (`complete-export/pseudocode.c`, C line 6891517, completed). Owner dispatch candidate `0x1412e2f20` (`pointer-export/pseudocode.c`, line 168811, completed_with_warnings). [UVI parameter interface](https://lua.uvi.net/_elements.html#AnalogFlanger).

### Analog Tape Delay — `Buzz`

Delay with trash, distortion mode, stereo spread and interpolation-time control; coloration belongs to the feedback/delay design.

Control IDs: `Trash`, `Spread`, `Delay`, `Feedback`, `Wet`, `Dry`, `DistortionMode`, `InterpolationTime`.

C: registration `0x140e790b6` → callback `0x140e84780` → constructor `0x1412dc360` (`complete-export/pseudocode.c`, C line 6888390, completed_with_warnings). Owner dispatch candidate `0x1412ddc00` (`complete-export/pseudocode.c`, line 6886502, completed_with_warnings). Embedded saved tag: `Buzz`. [UVI parameter interface](https://lua.uvi.net/_elements.html#Buzz).

### Autopan — `Autopan`

Mode-selectable periodic channel-gain movement with depth and speed; retain oscillator phase and stereo law.

Control IDs: `Depth`, `Speed`, `Mode`.

C: registration `0x140e7c128` → callback `0x140e84040` → constructor `0x1412d8540` (`complete-export/pseudocode.c`, C line 6882714, completed_with_warnings). Owner dispatch candidate `0x1412d9650` (`complete-export/pseudocode.c`, line 6884388, completed_with_warnings). Embedded saved tag: `Autopan`. [UVI parameter interface](https://lua.uvi.net/_elements.html#Autopan).

### Big Pi Tone — `BigMuffTone`

Tone/gain coloration stage; exact filter coefficients and topology remain unresolved.

Control IDs: `Tone`, `Gain`.

C: registration `0x140e7ba4a` → callback `0x140e84280` → constructor `0x141255b00` (`complete-export/pseudocode.c`, C line 6780033, completed_with_warnings). Owner dispatch candidate `0x141256270` (`complete-export/pseudocode.c`, line 6778220, completed_with_warnings). [UVI parameter interface](https://lua.uvi.net/_elements.html#BigMuffTone).

### Biquad Filter — `BiquadFilter`

Mode-selectable second-order filtering with frequency, Q and key tracking; coefficient laws are not yet verified.

Control IDs: `Mode`, `Freq`, `Q`, `KeyTracking`.

C: registration `0x140e7a83d` → callback `0x141104570` → constructor `0x1411039e0` (`complete-export/pseudocode.c`, C line 6473769, completed_with_warnings). Owner dispatch candidate `0x141104780` (`complete-export/pseudocode.c`, line 6470469, completed). [UVI parameter interface](https://lua.uvi.net/_elements.html#BiquadFilter).

### Bloom — `Bloom`

Reverberant/diffuse response with rise, tap structure, decay bands, modulation and freeze; several stateful sections must be retained.

Control IDs: `ReverbGain`, `ModDepth`, `ModRate`, `Diffusion`, `Rise`, `Width`, `RoomSize`, `DecayTime`, `DecayLow`, `DecayHigh`, `FreqLow`, `FreqHigh`, `Shape`, `Mix`, `StructuralMode`, `TapoutMode`, `Freeze`, `Rolloff`, `PreDelay`, `TapAmount`.

C: registration `0x140e79900` → callback `0x140e843e0` → constructor `0x14130c5b0` (`complete-export/pseudocode.c`, C line 6932017, completed_with_warnings). Owner dispatch candidate `0x14130d3f0` (`complete-export/pseudocode.c`, line 6927198, completed_with_warnings). [UVI parameter interface](https://lua.uvi.net/_elements.html#Bloom).

### Brickwall Filter — `BrickwallFilter`

Steep filtering with slope and soft bypass; latency, design and transition behaviour remain unresolved.

Control IDs: `Slope`, `SoftBypass`, `Freq`.

C: registration `0x140e7a9dd` → callback `0x140e84570` → constructor `0x14129b380` (`complete-export/pseudocode.c`, C line 6836525, completed_with_warnings). Owner dispatch candidate `0x14129c510` (`complete-export/pseudocode.c`, line 6834363, completed_with_warnings). [UVI parameter interface](https://lua.uvi.net/_elements.html#BrickwallFilter).

### Comb Filter — `CombFilter`

Frequency/Q/mode comb response; delay-line tuning, sign and interpolation remain unresolved.

Control IDs: `Mode`, `Freq`, `Q`, `KeyTracking`.

C: registration `0x140e7aa87` → callback `0x141106250` → constructor `0x1411056e0` (`complete-export/pseudocode.c`, C line 6474971, completed_with_warnings). Owner dispatch candidate `0x141106490` (`complete-export/pseudocode.c`, line 6471873, completed_with_warnings). [UVI parameter interface](https://lua.uvi.net/_elements.html#CombFilter).

### Compressor Expander — `CompExp`

Compression plus gating/expansion with separate thresholds, ratios and timing; preserve both detector paths and makeup/mix.

Control IDs: `AutoMakeUp`, `MakeUpGain`, `CompThreshold`, `CompRatio`, `CompAttack`, `CompRelease`, `GateThreshold`, `GateRatio`, `GateAttack`, `GateRelease`, `Mix`.

C: registration `0x140e7d00d` → callback `0x140e84af0` → constructor `0x1412047d0` (`complete-export/pseudocode.c`, C line 6708242, completed_with_warnings). Owner dispatch candidate `0x1412055a0` (`pointer-export/pseudocode.c`, line 162888, completed_with_warnings). Embedded saved tag: `CompExp`. [UVI parameter interface](https://lua.uvi.net/_elements.html#CompExp).

### Convolver — `Convolver`

Impulse-response processor with dry/wet and power normalization; IR loading, partitioning and normalization law remain unresolved.

Control IDs: `Dry`, `Wet`, `NormalizePower`.

C: registration `0x140e7bad8` → callback `0x140e84d00` → constructor `0x141229e40` (`complete-export/pseudocode.c`, C line 6745142, completed_with_warnings). Owner dispatch candidate `0x14122d1a0` (`complete-export/pseudocode.c`, line 6745477, completed_with_warnings). [UVI parameter interface](https://lua.uvi.net/_elements.html#Convolver).

### Crossover Filter — `CrossOverFilter`

Band splitting from low/high boundaries; output routing, filter slopes and reconstruction phase remain unresolved.

Control IDs: `LowFrequency`, `HighFrequency`.

C: registration `0x140e7abd5` → callback `0x140e84db0` → constructor `0x14129dbd0` (`complete-export/pseudocode.c`, C line 6838797, completed_with_warnings). Owner dispatch candidate `0x14129eb80` (`complete-export/pseudocode.c`, line 6836991, completed). [UVI parameter interface](https://lua.uvi.net/_elements.html#CrossOverFilter).

### Diffuse Delay — `DiffuseDelay`

Delay with diffusion, crosstalk, damping, randomness and modulation; keep diffusion and random/modulation history.

Control IDs: `DelayTime`, `TimeSpread`, `Diffusion`, `Feedback`, `Damp`, `CrossTalk`, `Mix`, `Randomness`, `ModRate`, `ModDepth`.

C: registration `0x140e79160` → callback `0x140e85070` → constructor `0x141231c00` (`complete-export/pseudocode.c`, C line 6751401, completed_with_warnings). Owner dispatch candidate `0x141232700` (`pointer-export/pseudocode.c`, line 166088, completed_with_warnings). [UVI parameter interface](https://lua.uvi.net/_elements.html#DiffuseDelay).

### Diffusion — `Diffusion`

Diffusion stage with amount, start and mix; exact scattering/all-pass structure remains unresolved.

Control IDs: `Diffusion`, `DiffusionStart`, `Mix`.

C: registration `0x140e7998e` → callback `0x140e851d0` → constructor `0x14130fc60` (`complete-export/pseudocode.c`, C line 6932509, completed_with_warnings). Owner dispatch candidate `0x1413102b0` (`pointer-export/pseudocode.c`, line 173174, completed_with_warnings). [UVI parameter interface](https://lua.uvi.net/_elements.html#Diffusion).

### Digital Eq — `DigitalEq`

Sixteen individually typed/enabled bands, with channel selection, slopes, bandwidth and global tuning/gain transforms.

Control IDs: `StereoMode`, `Transpose`, `KeyTracking`, `OverallGain`, `GainScale`, `Enabled/Type/Slope/Channels/Freq/Q/Gain/Bandwidth 1…16`.

C: registration `0x140e7bb77` → callback `0x140e85280` → constructor `0x14128a700` (`complete-export/pseudocode.c`, C line 6821393, completed_with_warnings). Owner dispatch candidate `0x14128b730` (`complete-export/pseudocode.c`, line 6818881, completed_with_warnings). [UVI parameter interface](https://lua.uvi.net/_elements.html#DigitalEq).

### Digital Filter — `DigitalFilter`

Filtering with slope, spread, Q, gain and bandwidth; preserve these dimensions rather than only a cutoff knob.

Control IDs: `Slope`, `Freq`, `KeyTracking`, `Spread`, `Q`, `Gain`, `Bandwidth`.

C: registration `0x140e7ac7f` → callback `0x140e85330` → constructor `0x1412864a0` (`complete-export/pseudocode.c`, C line 6817092, completed_with_warnings). Owner dispatch candidate `0x141287010` (`complete-export/pseudocode.c`, line 6817598, completed_with_warnings). [UVI parameter interface](https://lua.uvi.net/_elements.html#DigitalFilter).

### Diode Clipper — `DiodeClipper`

Drive/tone clipping with high-pass and asymmetry; exact nonlinear transfer and filter placement remain unresolved.

Control IDs: `Drive`, `Tone`, `HighPass`, `Asymmetry`, `OutputGain`.

C: registration `0x140e7c6c3` → callback `0x140e85410` → constructor `0x141257010` (`complete-export/pseudocode.c`, C line 6781371, completed_with_warnings). Owner dispatch candidate `0x1412578f0` (`complete-export/pseudocode.c`, line 6779172, completed). [UVI parameter interface](https://lua.uvi.net/_elements.html#DiodeClipper).

### Dispersor — `Dispersor`

Cascaded all-pass sections redistribute phase/time. Native stage recurrence and persistent history are now verified in 168 original-byte blocks.

Control IDs: `Spread`, `Freq`, `Stages`.

C: registration `0x140e7d8ea` → callback `0x140e854c0` → constructor `0x1412e8130` (`complete-export/pseudocode.c`, C line 6895518, completed_with_warnings). Owner dispatch candidate `0x1412e8750` (`pointer-export/pseudocode.c`, line 171251, completed_with_warnings). **K (partial):** [kernel details](DSP_EFFECT_CATALOGUE.md#verified-processing-details). [UVI parameter interface](https://lua.uvi.net/_elements.html#Dispersor).

### Drive — `Drive`

Mode-dependent nonlinear drive with oversampling; mode/curve and resampling stages remain unresolved.

Control IDs: `DriveAmount`, `Mode`, `Oversampling`.

C: registration `0x140e7c751` → callback `0x1410fcde0` → constructor `0x1410fc480` (`complete-export/pseudocode.c`, C line 6464792, completed_with_warnings). Owner dispatch candidate `0x1410fd010` (`complete-export/pseudocode.c`, line 6463857, completed_with_warnings). [UVI parameter interface](https://lua.uvi.net/_elements.html#Drive).

### Dual Delay X — `DualDelayX`

Two delay paths plus tape shaping, dispersion, diffusion, crushing, filters, rotation and ducking; preserve the enabled subgraph and ratios.

Control IDs: `TapeSaturation`, `TapeDrive`, `TapeWarmth`, `Reflection`, `DelayTime`, `LowCut`, `HighCut`, `PeakFreq`, `PeakGain`, `PeakQ`, `PeakCompensation`, `Feedback`, `Mix`, `Rotation`, `InputWidth`, `OutputWidth`, `DelayRatio`, `FeedbackRatio`, `InputRotation`, `OutputRotation`, `Modulation`, `ModDepth`, `ModRate`, `ModChannelOffset`, `Interpolation`, `DispersionSpread`, `DispersionFreq`, `Dispersion`, `DiffusionSpread`, `DiffusionAmount`, `Diffusion`, `CrusherFreq`, `CrusherBits`, `Crusher`, `Filtering`, `DuckerThreshold`, `DuckerAttack`, `DuckerHold`, `DuckerDecay`, `DuckerAttenuation`, `DuckerBypass`.

C: registration `0x140e792b4` → callback `0x140e857b0` → constructor `0x14123c900` (`complete-export/pseudocode.c`, C line 6765009, completed_with_warnings). Owner dispatch candidate `0x14123e1f0` (`complete-export/pseudocode.c`, line 6756047, completed_with_warnings). [UVI parameter interface](https://lua.uvi.net/_elements.html#DualDelayX).

### Effect Rack — `EffectRack`

Effect container/routing object; its empty scalar parameter list does not mean it is an audio identity operation.

C: registration `0x140e7d796` → callback `0x140e85860` → constructor `0x1412441a0` (`complete-export/pseudocode.c`, C line 6763685, completed_with_warnings). Owner dispatch candidate `0x141245ad0` (`complete-export/pseudocode.c`, line 6763642, completed). [UVI parameter interface](https://lua.uvi.net/_elements.html#EffectRack).

### Ensemble 505 — `Ensemble`

Mode/tone multi-voice modulation with mix; voice layout and modulation tables remain unresolved.

Control IDs: `Mode`, `Tone`, `Mix`.

C: registration `0x140e7a021` → callback `0x140e859d0` → constructor `0x1412ec950` (`complete-export/pseudocode.c`, C line 6901729, completed_with_warnings). Owner dispatch candidate `0x1412ed190` (`pointer-export/pseudocode.c`, line 171996, completed_with_warnings). [UVI parameter interface](https://lua.uvi.net/_elements.html#Ensemble).

### Exciter — `Exciter`

Mode-dependent harmonic generation with amount, input/output gain, mix and oversampling; spectrum and nonlinear curve remain unresolved.

Control IDs: `InputGain`, `Amount`, `Mix`, `Mode`, `OutputGain`, `Oversampling`.

C: registration `0x140e7c8ca` → callback `0x140e85a80` → constructor `0x14125bc00` (`complete-export/pseudocode.c`, C line 6786695, completed_with_warnings). Owner dispatch candidate `0x14125c680` (`complete-export/pseudocode.c`, line 6784499, completed). Embedded saved tag: `Exciter`. [UVI parameter interface](https://lua.uvi.net/_elements.html#Exciter).

### Feedback Compressor — `FeedbackCompressor`

Dynamics with knee and dynamic-range controls; detector placement, timing and gain law remain unresolved.

Control IDs: `Threshold`, `Ratio`, `Knee`, `DynamicRange`, `Attack`, `Release`, `Makeup`, `Mix`, `OutputGain`, `InputGain`.

C: registration `0x140e7d0b7` → callback `0x140e85be0` → constructor `0x14120a440` (`complete-export/pseudocode.c`, C line 6718020, completed_with_warnings). Owner dispatch candidate `0x14120af00` (`pointer-export/pseudocode.c`, line 168944, completed_with_warnings). [UVI parameter interface](https://lua.uvi.net/_elements.html#FeedbackCompressor).

### Feedback Machine — `FeedbackMachine`

Feedback-capable processing container with delay time and mix; nested processing and loop scheduling are separate contracts.

Control IDs: `DelayTime`, `Feedback`, `Mix`.

C: registration `0x140e7d840` → callback `0x140e85c90` → constructor `0x141246b70` (`complete-export/pseudocode.c`, C line 6767565, completed_with_warnings). Owner dispatch candidate `0x141247480` (`complete-export/pseudocode.c`, line 6766906, completed_with_warnings). [UVI parameter interface](https://lua.uvi.net/_elements.html#FeedbackMachine).

### Flanger — `Flanger`

Feedback comb modulation with explicit delay time, depth and speed; interpolation and modulation phase remain unresolved.

Control IDs: `Speed`, `Feedback`, `DelayTime`, `Depth`, `Mix`.

C: registration `0x140e7a0af` → callback `0x140e85d50` → constructor `0x1412ee350` (`complete-export/pseudocode.c`, C line 6902619, completed_with_warnings). Owner dispatch candidate `0x1412eeec0` (`complete-export/pseudocode.c`, line 6901293, completed_with_warnings). Embedded saved tag: `Flanger`. [UVI parameter interface](https://lua.uvi.net/_elements.html#Flanger).

### Formant Crusher — `FormantCrusher`

Formant/filter morphing plus fractional decimation. Native prefilter→decimator→mix chain is observed; only the isolated decimator is executed/verified.

Control IDs: `Morph`, `Q`, `Formant`, `Mix`, `Bite`, `FilterA`, `FilterB`.

C: registration `0x140e7ad29` → callback `0x140e85e30` → constructor `0x1412d35e0` (`complete-export/pseudocode.c`, C line 6880872, completed_with_warnings). Owner dispatch candidate `0x1412d4000` (`complete-export/pseudocode.c`, line 6880375, completed_with_warnings). **K (partial):** [kernel details](DSP_EFFECT_CATALOGUE.md#verified-processing-details). [UVI parameter interface](https://lua.uvi.net/_elements.html#FormantCrusher).

### Freq Shifter — `FreqShifter`

Frequency-offset processing with mirror, channel spread, crossover and pre-high-pass options; preserve sideband/phase state. Exact quadrature coefficients remain unresolved.

Control IDs: `CoarseFreq`, `FineFreq`, `ChannelSpread`, `CrossoverFreq`, `PreHighPassFreq`, `Mix`, `WetGain`, `Mirror`, `PreHighPassEnabled`, `CrossoverEnabled`.

C: registration `0x140e7a14e` → callback `0x140e85ee0` → constructor `0x1412fd580` (`complete-export/pseudocode.c`, C line 6918012, completed_with_warnings). Owner dispatch candidate `0x1412fdf70` (`pointer-export/pseudocode.c`, line 174663, completed_with_warnings). [UVI parameter interface](https://lua.uvi.net/_elements.html#FreqShifter).

### Fuzz — `StraightFuzz`

Fuzz-specific overdrive/trash path with gain, mix and oversampling; its transfer curve remains unresolved.

Control IDs: `InputGain`, `Overdrive`, `Trash`, `Mix`, `OutputGain`, `Oversampling`.

C: registration `0x140e7cc11` → callback `0x140e88c00` → constructor `0x1412672e0` (`complete-export/pseudocode.c`, C line 6796560, completed_with_warnings). Owner dispatch candidate `0x141268840` (`pointer-export/pseudocode.c`, line 166712, completed_with_warnings). Embedded saved tag: `StraightFuzz`. [UVI parameter interface](https://lua.uvi.net/_elements.html#StraightFuzz).

### Gain — `Gain`

Volume processing with control buffers and channel-layout dispatch; complete gain scheduling is not yet verified.

Control IDs: `Volume`.

C: registration `0x140e7c1ab` → callback `0x140e860f0` → constructor `0x1413369b0` (`complete-export/pseudocode.c`, C line 6963133, completed_with_warnings). Embedded saved tag: `Gain`. [UVI parameter interface](https://lua.uvi.net/_elements.html#Gain).

### Gain Matrix — `GainMatrix`

Twelve-by-twelve channel gain/routing controls; traced code computes into scratch before copying outputs back, preserving input data for mixing.

Control IDs: `Gain_1_1 … Gain_12_12`.

C: registration `0x140e7c24a` → callback `0x140e861a0` → constructor `0x141337740` (`complete-export/pseudocode.c`, C line 6965660, completed_with_warnings). [UVI parameter interface](https://lua.uvi.net/_elements.html#GainMatrix).

### Gate — `Gate`

Threshold-based attenuation with attack/release and mix; hysteresis, detector and smoothing remain unresolved.

Control IDs: `Threshold`, `Attack`, `Release`, `Mix`.

C: registration `0x140e7d145` → callback `0x141206680` → constructor `0x141206680` (`complete-export/pseudocode.c`, C line 6710343, completed). Owner dispatch candidate `0x1412071d0` (`complete-export/pseudocode.c`, line 6711088, completed_with_warnings). Owner dispatch candidate `0x141206f30` (`complete-export/pseudocode.c`, line 6710789, completed_with_warnings). [UVI parameter interface](https://lua.uvi.net/_elements.html#Gate).

### Granulizer — `Granulizer`

Pitch/detune, grain period/density/jitter, delay/spread, feedback and direction; the traced audio wrapper delegates to per-channel grain state of stride 0xa80.

Control IDs: `CoarseTune`, `FineTune`, `DetuneSpread`, `Delay`, `DelaySpread`, `Period`, `Jitter`, `Density`, `Mix`, `Feedback`, `Direction`, `DensityCompensation`.

C: registration `0x140e7a1f8` → callback `0x140e86330` → constructor `0x1412cd6f0` (`complete-export/pseudocode.c`, C line 6878303, completed_with_warnings). Owner dispatch candidate `0x1412ce110` (`complete-export/pseudocode.c`, line 6875856, completed). [UVI parameter interface](https://lua.uvi.net/_elements.html#Granulizer).

### Guitar Boxes — `BoxModeler`

Cabinet/microphone coloration with model, microphone type, separation/spread and mix; exact model filters/IRs remain unresolved.

Control IDs: `Mix`, `Separation`, `Spread`, `Model`, `MicType`.

C: registration `0x140e7c619` → callback `0x140e844c0` → constructor `0x141227470` (`complete-export/pseudocode.c`, C line 6742175, completed_with_warnings). Owner dispatch candidate `0x1412289a0` (`pointer-export/pseudocode.c`, line 163596, completed_with_warnings). Embedded saved tag: `BoxModeler`. [UVI parameter interface](https://lua.uvi.net/_elements.html#BoxModeler).

### Harmonic Resonators — `HarmonicResonators`

Six individually tuned/gained/coupled resonator bands, decay/tone and note tracking; traced processing has per-channel state stride 0x3c0.

Control IDs: `BandCoarse/Fine/Gain/Coupling/Enable 1…6`, `Note`, `Tone`, `Decay`, `DecayScaling`, `Gain`, `Mix`, `ModDepth`, `CouplingMode`, `KeyTracking`.

C: registration `0x140e7bc21` → callback `0x140e86410` → constructor `0x1412a2dc0` (`complete-export/pseudocode.c`, C line 6844692, completed_with_warnings). Owner dispatch candidate `0x1412a3dd0` (`complete-export/pseudocode.c`, line 6839583, completed). [UVI parameter interface](https://lua.uvi.net/_elements.html#HarmonicResonators).

### Harmonizer — `Harmonizer`

Five independently enabled/detuned/gained voices with grain size; preserve voice buffers and windowing.

Control IDs: `Enabled0`, `Detune0`, `Gain0`, `Enabled1`, `Detune1`, `Gain1`, `Enabled2`, `Detune2`, `Gain2`, `Enabled3`, `Detune3`, `Gain3`, `Enabled4`, `Detune4`, `Gain4`, `GrainSize`.

C: registration `0x140e7d642` → callback `0x140e864f0` → constructor `0x141305360` (`complete-export/pseudocode.c`, C line 6924985, completed_with_warnings). Owner dispatch candidate `0x141305d30` (`pointer-export/pseudocode.c`, line 171158, completed_with_warnings). [UVI parameter interface](https://lua.uvi.net/_elements.html#Harmonizer).

### IReverb — `SampledReverb`

IR-based space processing with time, low/high damping, predelay, width and normalization; saved SamplePath is additional resource evidence.

Control IDs: `Dry`, `Wet`, `Time`, `DampingLow`, `DampingHigh`, `PreDelay`, `Width`, `NormalizePower`.

C: registration `0x140e79a2d` → callback `0x140e88420` → constructor `0x14103aa90` (`complete-export/pseudocode.c`, C line 6292856, completed_with_warnings). Owner dispatch candidate `0x14103ff00` (`complete-export/pseudocode.c`, line 6277403, completed_with_warnings). Embedded saved tag: `SampledReverb`. [UVI parameter interface](https://lua.uvi.net/_elements.html#SampledReverb).

### Ladder — `MoogLadder`

Ladder-filter controls include voltage reference, resonance trim, gain compensation, algorithm and oversampling type; nonlinear topology remains unresolved.

Control IDs: `Freq`, `Q`, `KeyTracking`, `VoltageRef`, `ResonanceTrim`, `GainCompensation`, `Algorithm`, `Oversampling`, `OversamplingType`.

C: registration `0x140e7af27` → callback `0x1412ac010` → constructor `0x1412ab600` (`complete-export/pseudocode.c`, C line 6848313, completed_with_warnings). Owner dispatch candidate `0x1412ac470` (`complete-export/pseudocode.c`, line 6847456, completed). [UVI parameter interface](https://lua.uvi.net/_elements.html#MoogLadder).

### LowPass 12 — `IIRLFOFilter`

Resonant low-pass with frequency/Q/key tracking; exact coefficient and modulation laws remain unresolved.

Control IDs: `Freq`, `Q`, `KeyTracking`.

C: registration `0x140e7add3` → callback `0x140e86680` → constructor `0x1412a8830` (`complete-export/pseudocode.c`, C line 6846967, completed_with_warnings). Owner dispatch candidate `0x1412a91d0` (`complete-export/pseudocode.c`, line 6845757, completed). [UVI parameter interface](https://lua.uvi.net/_elements.html#IIRLFOFilter).

### LowPass 24 — `MoogFilter`

Resonant low-pass family with frequency/Q/key tracking; keep it distinct from MoogLadder and its extra model controls.

Control IDs: `Freq`, `Q`, `KeyTracking`.

C: registration `0x140e7ae7d` → callback `0x140e870f0` → constructor `0x1412a9900` (`complete-export/pseudocode.c`, C line 6846604, completed_with_warnings). Owner dispatch candidate `0x1412aa220` (`complete-export/pseudocode.c`, line 6846281, completed). [UVI parameter interface](https://lua.uvi.net/_elements.html#MoogFilter).

### Magnetic Bass Shaper — `HighpassFilterRL`

Bass coloration with high-pass, drive, model and inductor controls; physical-model equations remain unresolved.

Control IDs: `HighPass`, `Drive`, `InputGain`, `OutputGain`, `Model`, `Inductor`.

C: registration `0x140e7ca13` → callback `0x140e865a0` → constructor `0x1412a5dd0` (`complete-export/pseudocode.c`, C line 6841473, completed_with_warnings). Owner dispatch candidate `0x1412a6650` (`complete-export/pseudocode.c`, line 6841379, completed). [UVI parameter interface](https://lua.uvi.net/_elements.html#HighpassFilterRL).

### Maximizer — `Maximizer`

Level/ceiling processor with lookahead, knee, release blend and slew-rate control; latency and gain trajectory remain unresolved.

Control IDs: `Threshold`, `Attack`, `Lookahead`, `Release`, `Ceiling`, `Knee`, `ReleaseBlend`, `SlewRate`.

C: registration `0x140e7d28e` → callback `0x140e86d90` → constructor `0x141278ac0` (`complete-export/pseudocode.c`, C line 6808657, completed_with_warnings). Owner dispatch candidate `0x14127a750` (`pointer-export/pseudocode.c`, line 169948, completed_with_warnings). [UVI parameter interface](https://lua.uvi.net/_elements.html#Maximizer).

### One Pole — `OnePole`

Low/high-pass recurrence with per-channel history; scalar and stereo SIMD operation orders are verified, while cutoff-to-coefficient conversion is incomplete.

Control IDs: `Freq`, `KeyTracking`, `Mode`.

C: registration `0x140e7b125` → callback `0x140e87470` → constructor `0x1412b47c0` (`complete-export/pseudocode.c`, C line 6856445, completed_with_warnings). Owner dispatch candidate `0x1412b5180` (`complete-export/pseudocode.c`, line 6854576, completed). **K (partial):** [kernel details](DSP_EFFECT_CATALOGUE.md#verified-processing-details). [UVI parameter interface](https://lua.uvi.net/_elements.html#OnePole).

### Opal — `LA2A`

Optical/tube dynamics contract with limiter mode, opto type, presence, tube/makeup drive, linking and responsiveness; model state is not a generic compressor preset.

Control IDs: `InputGain`, `Compression`, `Presence`, `Limiter`, `Makeup`, `MakeupDrive`, `Tube`, `Mix`, `LinkMode`, `OptoType`, `Treble`, `External`, `VU`, `Responsiveness`.

C: registration `0x140e7d1e4` → callback `0x140e86940` → constructor `0x14120fd20` (`complete-export/pseudocode.c`, C line 6719725, completed_with_warnings). Owner dispatch candidate `0x141210ca0` (`complete-export/pseudocode.c`, line 6721097, completed_with_warnings). [UVI parameter interface](https://lua.uvi.net/_elements.html#LA2A).

### Overdrive — `SmoothOverdrive`

Smooth overdrive/trash path with gain, mix and oversampling; transfer curve and alias suppression remain unresolved.

Control IDs: `InputGain`, `Overdrive`, `Trash`, `Mix`, `OutputGain`, `Oversampling`.

C: registration `0x140e7cb67` → callback `0x140e887f0` → constructor `0x141264790` (`complete-export/pseudocode.c`, C line 6797046, completed_with_warnings). Owner dispatch candidate `0x141265d50` (`pointer-export/pseudocode.c`, line 167444, completed_with_warnings). Embedded saved tag: `SmoothOverdrive`. [UVI parameter interface](https://lua.uvi.net/_elements.html#SmoothOverdrive).

### Phase Meter — `PhaseMeter`

Analysis utility with refresh/decay/amplitude mode; audio mutation is not established by its registration.

Control IDs: `RefreshRate`, `Decay`, `AmpMode`.

C: registration `0x140e78eb8` → callback `0x140e87810` → constructor `0x141173100` (`complete-export/pseudocode.c`, C line 6589708, completed_with_warnings). Owner dispatch candidate `0x14067bac0` (`complete-export/pseudocode.c`, line 4212883, completed). [UVI parameter interface](https://lua.uvi.net/_elements.html#PhaseMeter).

### Phasor — `Phasor`

Modulated phase stages with frequency range, feedback, spread and LFO shape; retain phase history and stage arrangement.

Control IDs: `Speed`, `MinFreq`, `MaxFreq`, `Feedback`, `Depth`, `Spread`, `LFOSHape`.

C: registration `0x140e7a2a2` → callback `0x140e87970` → constructor `0x1412f2a00` (`complete-export/pseudocode.c`, C line 6909072, completed_with_warnings). Owner dispatch candidate `0x1412f36a0` (`complete-export/pseudocode.c`, line 6906130, completed). [UVI parameter interface](https://lua.uvi.net/_elements.html#Phasor).

### Phasor Filter — `PhasorFilter`

Pitch/key-tracked phase filtering with cutoff mode, feedback, depth and spread; coefficient laws remain unresolved.

Control IDs: `KeyTracking`, `Freq`, `Feedback`, `Depth`, `Spread`, `CutoffMode`.

C: registration `0x140e7b1cf` → callback `0x140e87a20` → constructor `0x1412f50e0` (`complete-export/pseudocode.c`, C line 6909704, completed_with_warnings). Owner dispatch candidate `0x1412f59b0` (`complete-export/pseudocode.c`, line 6908034, completed_with_warnings). [UVI parameter interface](https://lua.uvi.net/_elements.html#PhasorFilter).

### Redux — `Redux2`

Rate/bit reduction with jitter, quantizer law, dither, configurable filter position, antialiasing and band limit; retain random and sample-hold state.

Control IDs: `Frequency`, `Bits`, `Jitter`, `QuantizeLaw`, `Dither`, `FilterFreq`, `FilterRes`, `FilterMode`, `FilterPosition`, `Antialiasing`, `Mix`, `BandLimitFreq`.

C: registration `0x140e7d994` → callback `0x140e87bf0` → constructor `0x1411e4060` (`complete-export/pseudocode.c`, C line 6683737, completed_with_warnings). Owner dispatch candidate `0x1411e4de0` (`complete-export/pseudocode.c`, line 6681436, completed). [UVI parameter interface](https://lua.uvi.net/_elements.html#Redux2).

### Rez Filter — `Rez`

Mode-dependent resonant filtering with key tracking; exact topology and nonlinear response remain unresolved.

Control IDs: `Freq`, `Q`, `KeyTracking`, `Mode`.

C: registration `0x140e7b323` → callback `0x140e87e00` → constructor `0x141101550` (`complete-export/pseudocode.c`, C line 6472301, completed_with_warnings). Owner dispatch candidate `0x1411020e0` (`complete-export/pseudocode.c`, line 6468541, completed). Embedded saved tag: `Rez`. [UVI parameter interface](https://lua.uvi.net/_elements.html#Rez).

### Rotary — `Rotary2`

Independent horn/drum acceleration/deceleration, geometry/directivity, crossover, saturation, reflection and room options; multiple motion/acoustic states are required.

Control IDs: `HornAccelerationTime`, `HornDecelerationTime`, `DrumAccelerationTime`, `DrumDecelerationTime`, `TremoloSpeed`, `ChoraleSpeed`, `SpeedMode`, `Model`, `MicDistance`, `MicAngle`, `AngleSkew`, `DistanceSkew`, `Drive`, `Reflection`, `HornFilter`, `DrumFilter`, `DrumVolume`, `HornVolume`, `OutputGain`, `Radius`, `EnableCrossover`, `DrumHP`, `HornDirectivity`, `DrumDirectivity`, `ReverbType`, `DelayCompensation`, `HornLengthMod`, `BoxReverb`, `ReverbGain`, `EnableSaturation`, `SphericalDamp`, `EnablePreemphasis`, `PreemphasisDepth`.

C: registration `0x140e7a34c` → callback `0x140e88010` → constructor `0x14132ca80` (`complete-export/pseudocode.c`, C line 6957069, completed_with_warnings). Owner dispatch candidate `0x14132dd50` (`pointer-export/pseudocode.c`, line 186879, completed_with_warnings). [UVI parameter interface](https://lua.uvi.net/_elements.html#Rotary2).

### SVF — `SVF`

State-variable filter with filter and drive modes; the exact state update and nonlinear law remain unresolved.

Control IDs: `Freq`, `Q`, `KeyTracking`, `FilterMode`, `DriveMode`.

C: registration `0x140e7b45b` → callback `0x140e88220` → constructor `0x1412b9210` (`complete-export/pseudocode.c`, C line 6861481, completed_with_warnings). Owner dispatch candidate `0x1412b9ae0` (`complete-export/pseudocode.c`, line 6858758, completed). [UVI parameter interface](https://lua.uvi.net/_elements.html#SVF).

### Sallen-Key Filter — `SKF`

Sallen-Key family with saturation range/bias and output gain; preserve nonlinear operating range as well as frequency/Q.

Control IDs: `Freq`, `Q`, `KeyTracking`, `SaturationRange`, `SaturationBias`, `Output`.

C: registration `0x140e7b3cd` → callback `0x140e88170` → constructor `0x1412b6270` (`complete-export/pseudocode.c`, C line 6858160, completed_with_warnings). Owner dispatch candidate `0x1412b6e60` (`complete-export/pseudocode.c`, line 6855940, completed_with_warnings). [UVI parameter interface](https://lua.uvi.net/_elements.html#SKF).

### Shifter — `Shifter`

Pitch/grain processing with fine tune, tuning quantization, grain size and mix; resampling/windowing and latency remain unresolved.

Control IDs: `Detune`, `FineTune`, `QuantizeTune`, `GrainSize`, `Mix`.

C: registration `0x140e7d6ec` → callback `0x140e88690` → constructor `0x141309160` (`complete-export/pseudocode.c`, C line 6927827, completed_with_warnings). Owner dispatch candidate `0x141309a70` (`complete-export/pseudocode.c`, line 6925638, completed_with_warnings). [UVI parameter interface](https://lua.uvi.net/_elements.html#Shifter).

### SparkVerb — `SparkVerb`

Reverberant network with decay bands, diffusion/modulation, freeze, quality, matrix-version and feedback-filter controls; exact delays/matrices remain unresolved.

Control IDs: `ModDepth`, `ModRate`, `Diffusion`, `DiffusionStart`, `Width`, `RoomSize`, `DecayTime`, `DecayLow`, `DecayHigh`, `FreqLow`, `FreqHigh`, `Shape`, `Mix`, `Quality`, `Mode`, `Freeze`, `HiCut`, `LowCut`, `Rolloff`, `DiffusionOnOff`, `PreDelay`, `MixMode`, `UseOldInputMatrix`, `UseOldOutputMatrix`, `UseFilterInFeedback`, `EarlyShape`.

C: registration `0x140e79ad7` → callback `0x140e888a0` → constructor `0x14131fb50` (`complete-export/pseudocode.c`, C line 6949362, completed_with_warnings). Owner dispatch candidate `0x141320980` (`complete-export/pseudocode.c`, line 6944645, completed_with_warnings). [UVI parameter interface](https://lua.uvi.net/_elements.html#SparkVerb).

### Spectrum Analyzer — `SpectrumAnalyzer`

FFT analysis with size, window, display scales and refresh; classify it separately from sound-changing effects.

Control IDs: `LogFFTSize`, `GraphType`, `XScaleType`, `WindowType`, `RefreshRate`, `MaxDecibels`, `RangeDecibels`.

C: registration `0x140e78f62` → callback `0x140e88950` → constructor `0x141174430` (`complete-export/pseudocode.c`, C line 6590439, completed_with_warnings). Owner dispatch candidate `0x14067bac0` (`complete-export/pseudocode.c`, line 4212883, completed). [UVI parameter interface](https://lua.uvi.net/_elements.html#SpectrumAnalyzer).

### Studio Limiter — `BrickwallLimiter`

Threshold/margin peak containment with release and mix; exact detection/lookahead and gain law remain unresolved.

Control IDs: `Threshold`, `Margin`, `Release`, `Mix`.

C: registration `0x140e7cf63` → callback `0x140e84620` → constructor `0x141275e60` (`complete-export/pseudocode.c`, C line 6808225, completed_with_warnings). Owner dispatch candidate `0x1412767f0` (`complete-export/pseudocode.c`, line 6804846, completed_with_warnings). Embedded saved tag: `BrickwallLimiter`. [UVI parameter interface](https://lua.uvi.net/_elements.html#BrickwallLimiter).

### TS Overdrive — `TSDrive`

Voltage-referenced overdrive with asymmetry and low/high tone controls; circuit coefficients and nonlinear law remain unresolved.

Control IDs: `Drive`, `InputVoltage`, `OutputGain`, `LowFreq`, `HighFreq`, `Asymmetry`.

C: registration `0x140e7ccbb` → callback `0x140e88cf0` → constructor `0x14126a9d0` (`complete-export/pseudocode.c`, C line 6799811, completed_with_warnings). Owner dispatch candidate `0x14126b3f0` (`complete-export/pseudocode.c`, line 6796047, completed_with_warnings). [UVI parameter interface](https://lua.uvi.net/_elements.html#TSDrive).

### Tape Echo — `TapeEcho`

Multiple echo taps with speed/mode, feedback, tape-stop, noise and reverb; tape transport and noise histories are distinct states.

Control IDs: `Speed`, `Feedback`, `Mode`, `Bass`, `Treble`, `Delay1`, `Delay2`, `Delay3`, `Mix`, `ReverbGain`, `NoiseLevel`, `TapeStop`.

C: registration `0x140e7935e` → callback `0x140e88e50` → constructor `0x141253870` (`complete-export/pseudocode.c`, C line 6778717, completed_with_warnings). Owner dispatch candidate `0x141254780` (`complete-export/pseudocode.c`, line 6782078, completed_with_warnings). [UVI parameter interface](https://lua.uvi.net/_elements.html#TapeEcho).

### Thorus — `WhiteChorus`

Multiple modulation voices with edge, tone, crossover, low gain, trim and mode; inter-voice modulation remains unresolved.

Control IDs: `Speed`, `Depth`, `Edge`, `Tone`, `Crossover`, `LowGain`, `Mix`, `Trim`, `NumVoices`, `Mode`.

C: registration `0x140e7a3f6` → callback `0x140e89a90` → constructor `0x1412fab70` (`complete-export/pseudocode.c`, C line 6915868, completed_with_warnings). Owner dispatch candidate `0x1412fb810` (`complete-export/pseudocode.c`, line 6912284, completed). [UVI parameter interface](https://lua.uvi.net/_elements.html#WhiteChorus).

### Tilt — `Tilt`

Frequency-centered spectral tilt; exact shelf pairing and gain normalization remain unresolved.

Control IDs: `Tilt`, `Freq`.

C: registration `0x140e7bd59` → callback `0x140e89140` → constructor `0x141294a10` (`complete-export/pseudocode.c`, C line 6829729, completed_with_warnings). Owner dispatch candidate `0x141295000` (`complete-export/pseudocode.c`, line 6828882, completed_with_warnings). [UVI parameter interface](https://lua.uvi.net/_elements.html#Tilt).

### Tone Stack — `ToneStack`

Model-dependent bass/mid/treble network plus gain; coefficient coupling and circuit models remain unresolved.

Control IDs: `Bass`, `Mid`, `Treble`, `Gain`, `Model`.

C: registration `0x140e7bdf8` → callback `0x140e891f0` → constructor `0x1412bac50` (`complete-export/pseudocode.c`, C line 6864694, completed_with_warnings). Owner dispatch candidate `0x1412bb540` (`complete-export/pseudocode.c`, line 6859918, completed_with_warnings). [UVI parameter interface](https://lua.uvi.net/_elements.html#ToneStack).

### Track Delay — `TrackDelay`

Timing offset in milliseconds; fractional-delay policy, latency and automation transitions remain unresolved.

Control IDs: `DelayTime`.

C: registration `0x140e79408` → callback `0x140e892a0` → constructor `0x141339b70` (`complete-export/pseudocode.c`, C line 6967433, completed_with_warnings). Owner dispatch candidate `0x14133a2e0` (`complete-export/pseudocode.c`, line 6966165, completed_with_warnings). [UVI parameter interface](https://lua.uvi.net/_elements.html#TrackDelay).

### Tremolo — `Tremolo`

Periodic amplitude modulation with depth and frequency; waveform and phase law remain unresolved.

Control IDs: `Depth`, `Freq`.

C: registration `0x140e7c2d8` → callback `0x140e89350` → constructor `0x1412f89d0` (`complete-export/pseudocode.c`, C line 6913120, completed_with_warnings). Owner dispatch candidate `0x1412f9610` (`complete-export/pseudocode.c`, line 6911979, completed_with_warnings). Embedded saved tag: `Tremolo`. [UVI parameter interface](https://lua.uvi.net/_elements.html#Tremolo).

### Tube Amp — `JCMAmp`

Voltage/drive amp coloration with coupled tone controls; exact nonlinear stages and coefficient laws remain unresolved.

Control IDs: `Drive`, `Bass`, `Middle`, `Treble`, `InputVoltage`, `OutputGain`.

C: registration `0x140e7cabd` → callback `0x140e867e0` → constructor `0x141260ba0` (`complete-export/pseudocode.c`, C line 6792518, completed_with_warnings). Owner dispatch candidate `0x141261790` (`complete-export/pseudocode.c`, line 6790203, completed_with_warnings). [UVI parameter interface](https://lua.uvi.net/_elements.html#JCMAmp).

### Tuner — `Tuner`

Pitch-analysis utility with frequency limits and refresh; no sound-changing kernel is inferred from its name.

Control IDs: `RefreshRate`, `FreqMin`, `FreqMax`.

C: registration `0x140e7900c` → callback `0x140e89400` → constructor `0x141176410` (`complete-export/pseudocode.c`, C line 6589299, completed_with_warnings). Owner dispatch candidate `0x14067bac0` (`complete-export/pseudocode.c`, line 4212883, completed). [UVI parameter interface](https://lua.uvi.net/_elements.html#Tuner).

### UVI Filter — `UviFilter`

Cutoff/Q filtering with a mapping-mode selector; preserve the selector and modulation conversion.

Control IDs: `Cutoff`, `Q`, `MappingMode`.

C: registration `0x140e7b4fa` → callback `0x140e89770` → constructor `0x1412bc140` (`complete-export/pseudocode.c`, C line 6865629, completed_with_warnings). Owner dispatch candidate `0x1412bd130` (`pointer-export/pseudocode.c`, line 168389, completed_with_warnings). Embedded saved tag: `UviFilter`. [UVI parameter interface](https://lua.uvi.net/_elements.html#UviFilter).

### UVI Wide — `Widener`

Stereo decorrelation/width with side-channel shelf controls; routing and delay interpolation remain unresolved.

Control IDs: `Delay`, `Width`, `HighShelfFreqSide`, `HighSelfGainSide`.

C: registration `0x140e7c377` → callback `0x140e89b40` → constructor `0x1412cf7a0` (`complete-export/pseudocode.c`, C line 6877453, completed_with_warnings). Owner dispatch candidate `0x1412d0160` (`complete-export/pseudocode.c`, line 6877121, completed). Embedded saved tag: `Widener`. [UVI parameter interface](https://lua.uvi.net/_elements.html#Widener).

### UVInyl — `Vinyl`

Wear/year/RPM coloration plus separate mechanical/electrical noise, dust and scratch states; traced code includes a piecewise shaper and scheduled event loops.

Control IDs: `MechanicalNoiseGain`, `Wear`, `ElectricalNoiseGain`, `DustGain`, `DustAmount`, `ScratchGain`, `ScratchAmount`, `Year`, `RPMType`, `MixToMono`.

C: registration `0x140e7da3e` → callback `0x140e89820` → constructor `0x14133e190` (`complete-export/pseudocode.c`, C line 6977590, completed_with_warnings). Owner dispatch candidate `0x14133f360` (`complete-export/pseudocode.c`, line 6970666, completed). Embedded saved tag: `Vinyl`. [UVI parameter interface](https://lua.uvi.net/_elements.html#Vinyl).

### VCF-20 — `MS20`

Voltage-referenced resonant filter with morph and resonance trim; model-specific nonlinear state remains unresolved.

Control IDs: `Freq`, `Q`, `KeyTracking`, `ResonanceTrim`, `ReferenceVoltage`, `Morph`.

C: registration `0x140e7afd1` → callback `0x140e86c30` → constructor `0x1412af0a0` (`complete-export/pseudocode.c`, C line 6852556, completed_with_warnings). Owner dispatch candidate `0x1412afb10` (`complete-export/pseudocode.c`, line 6850736, completed_with_warnings). [UVI parameter interface](https://lua.uvi.net/_elements.html#MS20).

### VCF-20 Dual — `MS20Dual`

Separate low/high filter frequency/Q with shared model calibration; preserve both stages and their order.

Control IDs: `Freq`, `Q`, `KeyTracking`, `FreqHP`, `Q_HP`, `ResonanceTrim`, `ReferenceVoltage`.

C: registration `0x140e7b07b` → callback `0x140e86ce0` → constructor `0x1412b2430` (`complete-export/pseudocode.c`, C line 6854742, completed_with_warnings). Owner dispatch candidate `0x1412b2f10` (`complete-export/pseudocode.c`, line 6853196, completed_with_warnings). [UVI parameter interface](https://lua.uvi.net/_elements.html#MS20Dual).

### VCF-4023 — `ARP4023`

Voltage-referenced filtering with separate OTA/OPA saturation, high-pass, alternate output and oversampling; circuit equations remain unresolved.

Control IDs: `VoltageReference`, `Oversampling`, `HighPassCutoff`, `OTA_Saturation`, `OPA_Saturation`, `AltOutput`, `Freq`, `Q`, `KeyTracking`.

C: registration `0x140e7a776` → callback `0x140e83ca0` → constructor `0x141298430` (`complete-export/pseudocode.c`, C line 6833868, completed_with_warnings). Owner dispatch candidate `0x1412990c0` (`complete-export/pseudocode.c`, line 6832099, completed_with_warnings). [UVI parameter interface](https://lua.uvi.net/_elements.html#ARP4023).

### Velvet Delay — `DiffuseDelayNested`

Nested/diffuse delay with spread, seed and shape/curve controls; preserve seeded topology and separate algorithm from DiffuseDelay.

Control IDs: `DelayTime`, `TimeSpread`, `Diffusion`, `Mix`, `Seed`, `Shape`, `ShapeCurve`.

C: registration `0x140e7920a` → callback `0x140e85120` → constructor `0x141234190` (`complete-export/pseudocode.c`, C line 6754815, completed_with_warnings). Owner dispatch candidate `0x141234d20` (`pointer-export/pseudocode.c`, line 164994, completed_with_warnings). [UVI parameter interface](https://lua.uvi.net/_elements.html#DiffuseDelayNested).

### Vowel Filter — `VowelFilter`

Formant morph between two filter selections with Q, mode and number of formants; coefficient interpolation remains unresolved.

Control IDs: `Morph`, `Q`, `Formant`, `FilterA`, `FilterB`, `Mode`, `NumFormants`.

C: registration `0x140e7b5a4` → callback `0x140e89930` → constructor `0x1413454e0` (`complete-export/pseudocode.c`, C line 6978319, completed_with_warnings). Owner dispatch candidate `0x141345ef0` (`complete-export/pseudocode.c`, line 6977358, completed_with_warnings). [UVI parameter interface](https://lua.uvi.net/_elements.html#VowelFilter).

### Vowels — `Vowels`

Two-dimensional vowel morph with formant scale, mode and count; preserve both morph axes and resonant-band states.

Control IDs: `MorphX`, `MorphY`, `Formant`, `Mode`, `NumFormants`.

C: registration `0x140e7b64e` → callback `0x140e899e0` → constructor `0x1413476e0` (`complete-export/pseudocode.c`, C line 6981437, completed_with_warnings). Owner dispatch candidate `0x141348030` (`complete-export/pseudocode.c`, line 6979636, completed_with_warnings). [UVI parameter interface](https://lua.uvi.net/_elements.html#Vowels).

### WahWah — `WahWah`

Model-dependent pedal, drive and master controls; moving resonance and nonlinear placement remain unresolved.

Control IDs: `Wah`, `Drive`, `Master`, `Model`.

C: registration `0x140e7b6f8` → callback `0x140e84f10` → constructor `0x1412e6960` (`complete-export/pseudocode.c`, C line 6894382, completed_with_warnings). Owner dispatch candidate `0x1412e70c0` (`complete-export/pseudocode.c`, line 6893093, completed_with_warnings). [UVI parameter interface](https://lua.uvi.net/_elements.html#WahWah).

### Wave Shaper — `WaveShaper`

Mode-selectable shaping with knee, pre/post frequency, oversampling and mix. Full/half rectifier kernels are verified; other modes and surrounding stages are not.

Control IDs: `InputGain`, `OutputGain`, `Amount`, `Knee`, `Mix`, `PreFreq`, `PostFreq`, `Oversampling`, `Mode`.

C: registration `0x140e7cd65` → callback `0x141270040` → constructor `0x14126f680` (`complete-export/pseudocode.c`, C line 6804300, completed_with_warnings). Owner dispatch candidate `0x141270260` (`complete-export/pseudocode.c`, line 6800512, completed). **K (partial):** [kernel details](DSP_EFFECT_CATALOGUE.md#verified-processing-details). [UVI parameter interface](https://lua.uvi.net/_elements.html#WaveShaper).

### Xpander Filter — `XpanderFilter`

Multi-mode filtering with distortion type, drive/fat, algorithm and oversampling; selectors and nonlinear state are distinct.

Control IDs: `Freq`, `Q`, `KeyTracking`, `Mode`, `DistortionType`, `Drive`, `Fat`, `Algorithm`, `Oversampling`.

C: registration `0x140e7b7a2` → callback `0x140e89bf0` → constructor `0x1412be450` (`complete-export/pseudocode.c`, C line 6868117, completed_with_warnings). Owner dispatch candidate `0x1412bf1c0` (`complete-export/pseudocode.c`, line 6863967, completed). [UVI parameter interface](https://lua.uvi.net/_elements.html#XpanderFilter).


## Legacy

### 2 Band EQ — `TwoBandEq`

Low/high frequency and gain shaping; filter type and coefficient laws remain unresolved.

Control IDs: `LowFreq`, `LowGain`, `HighFreq`, `HighGain`.

C: registration `0x140e7c09a` → callback `0x140e894b0` → constructor `0x141295980` (`complete-export/pseudocode.c`, C line 6831559, completed_with_warnings). Owner dispatch candidate `0x141296c20` (`pointer-export/pseudocode.c`, line 166634, completed). Embedded saved tag: `TwoBandEq`. [UVI parameter interface](https://lua.uvi.net/_elements.html#TwoBandEq).

### 3 Band EQ — `ThreeBandEq`

Low/high gains plus a frequency/Q mid band; coefficient laws and shelving frequencies remain unresolved.

Control IDs: `LowGain`, `MidFreq`, `MidGain`, `MidQ`, `HighGain`.

C: registration `0x140e7bff0` → callback `0x140e88f30` → constructor `0x1412909b0` (`complete-export/pseudocode.c`, C line 6826417, completed_with_warnings). Owner dispatch candidate `0x141291db0` (`complete-export/pseudocode.c`, line 6825718, completed). Embedded saved tag: `ThreeBandEq`. [UVI parameter interface](https://lua.uvi.net/_elements.html#ThreeBandEq).

### 8 Band EQ — `ParametricEQ`

Eight independently enabled frequency/Q/gain bands; preserve ordered band state and bypass behaviour.

Control IDs: `Freq1`, `Q1`, `Gain1`, `Enable1`, `Freq2`, `Q2`, `Gain2`, `Enable2`, `Freq3`, `Q3`, `Gain3`, `Enable3`, `Freq4`, `Q4`, `Gain4`, `Enable4`, `Freq5`, `Q5`, `Gain5`, `Enable5`, `Freq6`, `Q6`, `Gain6`, `Enable6`, `Freq7`, `Q7`, `Gain7`, `Enable7`, `Freq8`, `Q8`, `Gain8`, `Enable8`.

C: registration `0x140e7bea2` → callback `0x140e87600` → constructor `0x14128dda0` (`complete-export/pseudocode.c`, C line 6822012, completed_with_warnings). Owner dispatch candidate `0x14128eb60` (`complete-export/pseudocode.c`, line 6822649, completed_with_warnings). [UVI parameter interface](https://lua.uvi.net/_elements.html#ParametricEQ).

### Auto Wah — `AutoWah`

Envelope-responsive resonance with sensitivity, Q and drive; envelope timing and cutoff mapping remain unresolved.

Control IDs: `Q`, `Sensitivity`, `Drive`, `Gain`.

C: registration `0x140e7b84c` → callback `0x140e83f90` → constructor `0x1412daf60` (`complete-export/pseudocode.c`, C line 6885751, completed_with_warnings). Owner dispatch candidate `0x1412db9a0` (`complete-export/pseudocode.c`, line 6884242, completed_with_warnings). Embedded saved tag: `AutoWah`. [UVI parameter interface](https://lua.uvi.net/_elements.html#AutoWah).

### Beat Repeat — `BeatRepeat`

Captured audio repetition with time, fade and active state; capture/retrigger and tempo boundaries need explicit handling.

Control IDs: `Time`, `FadeTime`, `Mix`, `Active`.

C: registration `0x140e7dae8` → callback `0x140e841d0` → constructor `0x14122e530` (`complete-export/pseudocode.c`, C line 6750613, completed_with_warnings). Owner dispatch candidate `0x14122f5d0` (`complete-export/pseudocode.c`, line 6749284, completed_with_warnings). Embedded saved tag: `BeatRepeat`. [UVI parameter interface](https://lua.uvi.net/_elements.html#BeatRepeat).

### Chorus — `Chorus`

Legacy modulated delay with speed, delay time, depth and mix; retain its independent history/model.

Control IDs: `Speed`, `DelayTime`, `Depth`, `Mix`.

C: registration `0x140e7a487` → callback `0x140e84830` → constructor `0x1412df8a0` (`complete-export/pseudocode.c`, C line 6889985, completed_with_warnings). Owner dispatch candidate `0x1412e02a0` (`complete-export/pseudocode.c`, line 6889280, completed_with_warnings). Embedded saved tag: `Chorus`. [UVI parameter interface](https://lua.uvi.net/_elements.html#Chorus).

### Compressor — `Compressor`

Threshold/ratio dynamics with attack, release, makeup and mix; a shared constructor candidate also appears for Gate.

Control IDs: `Threshold`, `Attack`, `Release`, `Ratio`, `MakeUp`, `Mix`.

C: registration `0x140e7d473` → callback `0x141206680` → constructor `0x141206680` (`complete-export/pseudocode.c`, C line 6710343, completed). Owner dispatch candidate `0x1412071d0` (`complete-export/pseudocode.c`, line 6711088, completed_with_warnings). Owner dispatch candidate `0x141206f30` (`complete-export/pseudocode.c`, line 6710789, completed_with_warnings). [UVI parameter interface](https://lua.uvi.net/_elements.html#Compressor).

### Cross Phaser — `CrossPhaser`

Phase modulation with frequency range and feedback; cross-channel signal topology remains unresolved.

Control IDs: `Speed`, `MinFreq`, `MaxFreq`, `Feedback`, `Depth`.

C: registration `0x140e7a526` → callback `0x140e84e60` → constructor `0x1412e4080` (`complete-export/pseudocode.c`, C line 6893435, completed_with_warnings). Owner dispatch candidate `0x1412e4c90` (`complete-export/pseudocode.c`, line 6892387, completed_with_warnings). Embedded saved tag: `CrossPhaser`. [UVI parameter interface](https://lua.uvi.net/_elements.html#CrossPhaser).

### Double Drive — `DoubleDrive`

Two drive sections with low/high shaping and gain; preserve pre/post nonlinear order.

Control IDs: `PreDrive`, `PostDrive`, `LowFreq`, `HighFreq`, `Gain`.

C: registration `0x140e7ce0f` → callback `0x140e85570` → constructor `0x141258ec0` (`complete-export/pseudocode.c`, C line 6784560, completed_with_warnings). Owner dispatch candidate `0x14125a110` (`complete-export/pseudocode.c`, line 6781819, completed_with_warnings). Embedded saved tag: `DoubleDrive`. [UVI parameter interface](https://lua.uvi.net/_elements.html#DoubleDrive).

### Dual Delay — `DualDelay`

Dual delays with unequal time/feedback ratios, EQ, rotations, widths and modulation; preserve the channel/feedback matrix.

Control IDs: `DelayTime`, `Feedback`, `LowCut`, `HighCut`, `PeakFreq`, `PeakGain`, `PeakQ`, `Mix`, `Rotation`, `InputWidth`, `OutputWidth`, `DelayRatio`, `FeedbackRatio`, `InputRotation`, `OutputRotation`, `ModDepth`, `ModRate`, `ModChannelOffset`, `Interpolation`.

C: registration `0x140e794b2` → callback `0x140e85700` → constructor `0x1412370b0` (`complete-export/pseudocode.c`, C line 6756298, completed_with_warnings). Owner dispatch candidate `0x1412380d0` (`complete-export/pseudocode.c`, line 6752942, completed). [UVI parameter interface](https://lua.uvi.net/_elements.html#DualDelay).

### FX Delay — `FxDelay`

Delay with feedback, cutoff, speed and drive bypass; exact embedded processing remains unresolved.

Control IDs: `DelayTime`, `Feedback`, `Cutoff`, `Speed`, `Mix`, `BypassDrive`.

C: registration `0x140e79606` → callback `0x140e85f90` → constructor `0x141248ce0` (`complete-export/pseudocode.c`, C line 6769880, completed_with_warnings). Owner dispatch candidate `0x141249980` (`complete-export/pseudocode.c`, line 6768529, completed_with_warnings). Embedded saved tag: `FxDelay`. [UVI parameter interface](https://lua.uvi.net/_elements.html#FxDelay).

### FX Filter — `FxFilter`

Low/high frequency and resonance controls; two-stage ordering and coefficient laws remain unresolved.

Control IDs: `LowFreq`, `HighFreq`, `LowQ`, `HighQ`.

C: registration `0x140e7b8f6` → callback `0x140e86040` → constructor `0x14129f8c0` (`complete-export/pseudocode.c`, C line 6839214, completed_with_warnings). Owner dispatch candidate `0x1412a09b0` (`complete-export/pseudocode.c`, line 6838459, completed_with_warnings). [UVI parameter interface](https://lua.uvi.net/_elements.html#FxFilter).

### Fat Delay — `FatDelay`

Delay with diffusion and cutoff; preserve the diffusion section and feedback history.

Control IDs: `DelayTime`, `Feedback`, `Diffusion`, `Cutoff`, `Mix`.

C: registration `0x140e7955c` → callback `0x140e85b30` → constructor `0x141242120` (`complete-export/pseudocode.c`, C line 6761042, completed_with_warnings). Owner dispatch candidate `0x141242d10` (`complete-export/pseudocode.c`, line 6760000, completed_with_warnings). Embedded saved tag: `FatDelay`. [UVI parameter interface](https://lua.uvi.net/_elements.html#FatDelay).

### Gate Reverb — `GateReverb`

Reverberation with separate gate time, predelay, decay and high damping; gating envelope is additional state.

Control IDs: `DecayTime`, `HighDamp`, `GateTime`, `PreDelay`, `Mix`.

C: registration `0x140e79cd5` → callback `0x140e89e00` → constructor `0x141314130` (`complete-export/pseudocode.c`, C line 6933696, completed). Owner dispatch candidate `0x141317bb0` (`complete-export/pseudocode.c`, line 6941923, completed_with_warnings). Embedded saved tag: `GateReverb`. [UVI parameter interface](https://lua.uvi.net/_elements.html#GateReverb).

### Limiter — `Limiter`

Legacy gain/threshold/release/ceiling processing with mix; keep its response separate from BrickwallLimiter.

Control IDs: `Gain`, `Threshold`, `Release`, `Ceiling`, `Mix`.

C: registration `0x140e7d4f9` → callback `0x140e86b50` → constructor `0x141277290` (`complete-export/pseudocode.c`, C line 6806836, completed_with_warnings). Owner dispatch candidate `0x1412781e0` (`complete-export/pseudocode.c`, line 6805750, completed_with_warnings). [UVI parameter interface](https://lua.uvi.net/_elements.html#Limiter).

### Phaser — `Phaser`

Legacy phase modulation with min/max frequency, depth and feedback; coefficients and modulation remain unresolved.

Control IDs: `Speed`, `MinFreq`, `MaxFreq`, `Feedback`, `Depth`.

C: registration `0x140e7a5d0` → callback `0x140e878c0` → constructor `0x1412f03c0` (`complete-export/pseudocode.c`, C line 6906821, completed_with_warnings). Owner dispatch candidate `0x1412f0ec0` (`complete-export/pseudocode.c`, line 6903120, completed_with_warnings). Embedded saved tag: `Phaser`. [UVI parameter interface](https://lua.uvi.net/_elements.html#Phaser).

### Ping Pong Delay — `PingPongDelay`

Alternating/stereo-routed delay with spread, damping and feedback; the feedback routing must be established.

Control IDs: `DelayTime`, `Feedback`, `Cutoff`, `Spread`, `Mix`.

C: registration `0x140e796b0` → callback `0x140e87ad0` → constructor `0x14124b660` (`complete-export/pseudocode.c`, C line 6772319, completed_with_warnings). Owner dispatch candidate `0x14124c290` (`complete-export/pseudocode.c`, line 6770350, completed_with_warnings). Embedded saved tag: `PingPongDelay`. [UVI parameter interface](https://lua.uvi.net/_elements.html#PingPongDelay).

### Plain Reverb — `PlainReverb`

Legacy reverberation with decay, high damping and mix; exact network remains unresolved.

Control IDs: `DecayTime`, `HighDamp`, `Mix`.

C: registration `0x140e79d7f` → callback `0x140e89eb0` → constructor `0x141314b20` (`complete-export/pseudocode.c`, C line 6934657, completed). Owner dispatch candidate `0x141319930` (`complete-export/pseudocode.c`, line 6938816, completed_with_warnings). [UVI parameter interface](https://lua.uvi.net/_elements.html#PlainReverb).

### Predelay Verb — `DelayedReverb`

Legacy reverberation with an independent predelay; preserve delay history and reverb state.

Control IDs: `DecayTime`, `HighDamp`, `PreDelay`, `Mix`.

C: registration `0x140e79b81` → callback `0x140e89ca0` → constructor `0x1413144e0` (`complete-export/pseudocode.c`, C line 6934315, completed). Owner dispatch candidate `0x1413165f0` (`complete-export/pseudocode.c`, line 6936947, completed_with_warnings). Embedded saved tag: `DelayedReverb`. [UVI parameter interface](https://lua.uvi.net/_elements.html#DelayedReverb).

### Redux — `Redux`

Legacy rate/bit reduction with quantizer law, jitter, dither and configurable filters; keep version-specific state distinct from Redux2.

Control IDs: `Frequency`, `Bits`, `Jitter`, `QuantizeLaw`, `Dither`, `FilterFreq`, `FilterRes`, `FilterMode`, `FilterPosition`, `Antialiasing`, `Mix`.

C: registration `0x140e7dc3c` → callback `0x140e87ca0` → constructor `0x1412d4c10` (`complete-export/pseudocode.c`, C line 6881295, completed_with_warnings). Owner dispatch candidate `0x1412d6ba0` (`complete-export/pseudocode.c`, line 6881904, completed_with_warnings). [UVI parameter interface](https://lua.uvi.net/_elements.html#Redux).

### Ring Modulator — `RingModulator`

Carrier multiplication with frequency and mix; waveform and oscillator phase remain unresolved.

Control IDs: `Freq`, `Mix`.

C: registration `0x140e7dce6` → callback `0x140e87eb0` → constructor `0x1412f76e0` (`complete-export/pseudocode.c`, C line 6912639, completed_with_warnings). Owner dispatch candidate `0x1412f82b0` (`complete-export/pseudocode.c`, line 6911597, completed_with_warnings). Embedded saved tag: `RingModulator`. [UVI parameter interface](https://lua.uvi.net/_elements.html#RingModulator).

### Robotizer — `Robotizer`

Interpolated unipolar carrier with a second-table phase offset, optional biquad and wet/dry sum. Exact PCM and phase/filter history verified in 864 original-byte blocks; the public FM conversion remains unresolved.

Control IDs: `Freq`, `Depth`, `FM`, `Cutoff`, `Mix`.

C: registration `0x140e7dd90` → callback `0x140e87f60` → constructor `0x141306b30` (`complete-export/pseudocode.c`, C line 6924299, completed_with_warnings). Owner dispatch candidate `0x141307d20` (`complete-export/pseudocode.c`, line 6923783, completed_with_warnings). Embedded saved tag: `Robotizer`. **K (partial):** [kernel details](DSP_EFFECT_CATALOGUE.md#verified-processing-details). [UVI parameter interface](https://lua.uvi.net/_elements.html#Robotizer).

### Rotary — `Rotary`

Horn/drum rotation model with speed modes, geometry, drive, reflection, crossover and room processing; distinct from Rotary2 and simpler legacy types.

Control IDs: `TremoloSpeed`, `ChoraleSpeed`, `SpeedMode`, `Model`, `MicDistance`, `MicAngle`, `Drive`, `Reflection`, `BoxReverb`, `HornFilter`, `DrumVolume`, `HornVolume`, `Radius`, `DrumCrossover`, `DrumHP`.

C: registration `0x140e7a67a` → callback `0x140e880c0` → constructor `0x141325420` (`complete-export/pseudocode.c`, C line 6950904, completed_with_warnings). Owner dispatch candidate `0x141325f80` (`pointer-export/pseudocode.c`, line 178803, completed_with_warnings). [UVI parameter interface](https://lua.uvi.net/_elements.html#Rotary).

### Rotary Simple — `OneWayRotarySpeaker`

Single rotating section with speed, depth, distance and mix; motion and acoustic state remain unresolved.

Control IDs: `Speed`, `Depth`, `MicDistance`, `Mix`.

C: registration `0x140e7c421` → callback `0x140e87520` → constructor `0x1413325e0` (`complete-export/pseudocode.c`, C line 6958058, completed_with_warnings). Owner dispatch candidate `0x141333030` (`complete-export/pseudocode.c`, line 6956517, completed_with_warnings). Embedded saved tag: `OneWayRotarySpeaker`. [UVI parameter interface](https://lua.uvi.net/_elements.html#OneWayRotarySpeaker).

### Rotary Speaker — `TwoWayRotarySpeaker`

Low/high rotating sections with independent speeds and a crossover; crossover state and stereo geometry remain unresolved.

Control IDs: `LowSpeed`, `HighSpeed`, `Depth`, `MicDistance`, `CrossFrequency`.

C: registration `0x140e7c51d` → callback `0x140e89560` → constructor `0x141334c40` (`complete-export/pseudocode.c`, C line 6961061, completed_with_warnings). Owner dispatch candidate `0x141335690` (`complete-export/pseudocode.c`, line 6959396, completed_with_warnings). Embedded saved tag: `TwoWayRotarySpeaker`. [UVI parameter interface](https://lua.uvi.net/_elements.html#TwoWayRotarySpeaker).

### Simple Delay — `SimpleDelay`

Legacy feedback delay with cutoff and mix; delay/interpolation and filter law remain unresolved.

Control IDs: `DelayTime`, `Feedback`, `Cutoff`, `Mix`.

C: registration `0x140e7975a` → callback `0x140e88740` → constructor `0x14124e5a0` (`complete-export/pseudocode.c`, C line 6776055, completed_with_warnings). Owner dispatch candidate `0x14124f060` (`complete-export/pseudocode.c`, line 6773009, completed_with_warnings). [UVI parameter interface](https://lua.uvi.net/_elements.html#SimpleDelay).

### Simple Reverb — `FilteredReverb`

Legacy decay/cutoff/high-damping reverberation with mix; exact filter/network remains unresolved.

Control IDs: `DecayTime`, `Cutoff`, `HighDamp`, `Mix`.

C: registration `0x140e79c2b` → callback `0x140e89d50` → constructor `0x1413147a0` (`complete-export/pseudocode.c`, C line 6934475, completed). Owner dispatch candidate `0x141315240` (`complete-export/pseudocode.c`, line 6936317, completed_with_warnings). Embedded saved tag: `FilteredReverb`. [UVI parameter interface](https://lua.uvi.net/_elements.html#FilteredReverb).

### Stereo Delay — `StereoDelay`

Separate left/right delay times with feedback and cutoff; channel coupling and interpolation remain unresolved.

Control IDs: `Feedback`, `LeftDelayTime`, `RightDelayTime`, `Cutoff`, `Mix`.

C: registration `0x140e79856` → callback `0x140e88b50` → constructor `0x141250620` (`complete-export/pseudocode.c`, C line 6777194, completed_with_warnings). Owner dispatch candidate `0x1412512c0` (`complete-export/pseudocode.c`, line 6775238, completed_with_warnings). Embedded saved tag: `StereoDelay`. [UVI parameter interface](https://lua.uvi.net/_elements.html#StereoDelay).

### TalkBox — `TalkBox`

Vowel-mode processing after a filter stage; static kernel uses a five-choice, ten-history double-precision recurrence. Runtime coefficient tables remain unverified.

Control IDs: `Freq`, `Q`, `FilterMode`, `VowelMode`.

C: registration `0x140e7b9a0` → callback `0x140e88da0` → constructor `0x141341c10` (`complete-export/pseudocode.c`, C line 6974735, completed_with_warnings). Owner dispatch candidate `0x1413424d0` (`complete-export/pseudocode.c`, line 6974192, completed). Embedded saved tag: `TalkBox`. [UVI parameter interface](https://lua.uvi.net/_elements.html#TalkBox).

### UVI Destructor — `BitCrusher`

Sample holding, distinctive quantization, rational saturation, optional biquad and wet/dry sum; 2,268 original-byte blocks verify authored internal state.

Control IDs: `EffectiveSampleRate`, `BitSize`, `Drive`, `Cutoff`, `Mix`.

C: registration `0x140e7db92` → callback `0x140e84330` → constructor `0x1412d06a0` (`complete-export/pseudocode.c`, C line 6877960, completed_with_warnings). Owner dispatch candidate `0x1412d1770` (`complete-export/pseudocode.c`, line 6877834, completed_with_warnings). Embedded saved tag: `BitCrusher`. **K (partial):** [kernel details](DSP_EFFECT_CATALOGUE.md#verified-processing-details). [UVI parameter interface](https://lua.uvi.net/_elements.html#BitCrusher).

### UVI Drive — `UviDrive`

Drive with cutoff/Q and gain; nonlinear curve, resonant filtering and their order remain unresolved.

Control IDs: `Drive`, `Cutoff`, `Q`, `Gain`.

C: registration `0x140e7ceb9` → callback `0x140e896c0` → constructor `0x14126c8f0` (`complete-export/pseudocode.c`, C line 6801856, completed_with_warnings). Owner dispatch candidate `0x14126d940` (`complete-export/pseudocode.c`, line 6799060, completed_with_warnings). Embedded saved tag: `UviDrive`. [UVI parameter interface](https://lua.uvi.net/_elements.html#UviDrive).

### UVI Mastering — `UVIMastering`

Composite mastering chain; saved presets contain Gain, CrossOver, Exciter, CompExp, SoftLimiter and BrickwallLimiter children.

Control IDs: `TubeAmount`, `LowVolume`, `MediumVolume`, `HighVolume`, `CompThres`, `CompAttack`, `LimiterThres`, `LowMidFreq`, `MidHighFreq`.

C: registration `0x140e7d598` → callback `0x140e89610` → constructor `0x141281b60` (`complete-export/pseudocode.c`, C line 6815057, completed_with_warnings). Owner dispatch candidate `0x14158b250` (`complete-export/pseudocode.c`, line 7742065, completed). Embedded saved tag: `UVIMastering`. [UVI parameter interface](https://lua.uvi.net/_elements.html#UVIMastering).


## Saved XML names and aliases

All 49 observed audio-processor tags are retained in `saved_xml_schemas` in the manifest, including attributes, example values, members and child topology. The other seven wrapper/metadata/event/script tags are separately indexed without copying scripts. Names can differ from the public IDs above. Candidate associations below need a saved-state reader/factory trace before treating them as exact numeric or module mappings.

| Observed name | Candidate association / distinction |
| --- | --- |
| `FilteredDelay` | SimpleDelay/StereoDelay neighbourhood; exact registration unresolved |
| `Chorus2` | AnalogChorus/AnalogFlanger shared callback candidate; exact selector unresolved |
| `Flanger2` | AnalogFlanger candidate; exact selector unresolved |
| `Parametric` | ParametricEQ candidate; serialized tag differs |
| `CrossOver` | three-output crossover in UVIMastering presets; CrossOverFilter association unresolved |
| `SoftLimiter` | child processor in UVIMastering presets; independent registry/API association unresolved |

`FXPreset` and `ModulePreset` are wrappers; `Properties` is metadata. `Arpeggiator`, `ScriptProcessor`, `script` and `ScriptData` are separately indexed. Saved routing/identity attributes and unknown child processors must be preserved.
