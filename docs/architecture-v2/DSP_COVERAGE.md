# DSP coverage (Kontakt and UVI/Falcon)

Module lists come from the RE thread's `DSP_SYSTEM_INVENTORY.md` (59 Kontakt filters, 65 Kontakt effects, 10 source
modes; 83 FX, 32 legacy processors, 24 oscillators and 16 modulation types for UVI/Falcon, the last including
Constant Modulation). Status comes from the code on this branch (`sampler-core/src/dsp.rs`, `sampler-kontakt/src/effects.rs`,
`sampler-kontakt/src/library.rs`, `sampler-uvi/src/inserts.rs`, `modulation.rs`).

**Status.** implemented = translated and rendered by the shared core with laws from a measurement or from original bytes;
partial = renders, but a stored parameter, mode or law is missing or taken from a guess; missing = reported as
`unsupported`, or dropped, so the module does nothing.

**Calibration** records what the module was compared with. `REF s.N` is `KONTAKT_REFERENCE.md` (Kontakt 8.13.1 under
Wine, recorded renders). "inventory" is `DSP_SYSTEM_INVENTORY.md` (original-byte checks). UVI has no native rig, so UVI rows
are `unmeasured (no native rig)` unless a kernel was verified from original bytes. Nothing below has a measured error in
dB except where a number is written.

**Corpus use.** Counts come from `examples/fx_survey.rs` (`ALL=1`, `FILTERS=1`, `VALUES=<feature>`) over the 2,741 NKIs in
`/mnt/MAIN_STORAGE/Libraries/Kontakt` (2026-10-08). The survey sees only what the importer reports, so for a missing
module the count is exact (instances KONTRA drops); for an implemented module it says "modelled" and the instance count is
not tallied. UVI counts are programs from `FALCON_MODULE_COVERAGE.md` (660 + 40 programs).

## Summary

| Group | Modules | Implemented | Partial | Missing | Implemented % | Implemented+partial % |
|---|---:|---:|---:|---:|---:|---:|
| Kontakt filters | 59 | 4 | 2 | 53 | 7% | 10% |
| Kontakt effects | 65 | 3 | 4 | 58 | 5% | 11% |
| Kontakt source modes | 10 | 2 | 0 | 8 | 20% | 20% |
| Kontakt modulation | 10 | 2 | 4 | 4 | 20% | 60% |
| UVI/Falcon FX | 83 | 2 | 5 | 76 | 2% | 8% |
| UVI/Falcon legacy processors | 32 | 0 | 0 | 32 | 0% | 0% |
| UVI/Falcon oscillators | 24 | 0 | 1 | 23 | 0% | 4% |
| UVI/Falcon modulation | 16 | 5 | 2 | 9 | 31% | 44% |
| **Total** | 299 | 18 | 18 | 263 | 6% | 12% |

Percentages count modules, not usage. Weighted by corpus use the picture is different: of the 2,741 Kontakt instruments
250 carry an effect KONTRA drops (9%), and the single biggest gap is filter types: 227 instruments, 3,542 slots.

## Work order (by corpus instances, Kontakt)

1. Filter types: Legacy HP1 (id 3, 1,996 slots), unparsed filter payloads (962 slots, Morphology Evolved 879 and Conflux
   82), Formant I (291), AR LP2/4 (267, cutoff law known), AR/Ladder 100-105 (11).
2. EQ band shape (202 instruments, 12,918 slots): modelled as an RBJ peak, never compared with Kontakt.
3. Reverb algorithm (371 instruments): RT60 calibrated, density and diffusion not.
4. Module-parameter modulation targets (1,311 instruments): `ahdsr_attack`, `ahdsr_release`, `eqGain1-3`, `filterCutoff`,
   `formantTalk`, `startPhase`; and `playPos` sample-start targets (1,111 instruments).
5. Surround Panner (6), Lo-Fi (4), Skreamer (1), Distortion (4), Tape Saturator (4), legacy Chorus/Flanger/Phaser/Delay
   (3-4 each), Rotator, Transient Master, Solid Bus Comp, Feedback Compressor, Solid G-EQ (3 each), Limiter (1).
6. Wavetable source (2 instruments, 31 groups).
7. Everything else has zero instances in the local corpus; it is listed so the inventory is complete, and is ordered after
   the above.

UVI order (programs): Augmented Orchestra (620) uses CombFilter, DiodeClipper, Drive, DualDelay, FeedbackMachine,
Flanger, Maximizer, Phasor, SparkVerb, XpanderFilter, SampledReverb and ThreeBandShelves, none of which are translated.
VWinds (40) uses OnePole (6,318), GainMatrix, DigitalEq, Convolver (376), SampledReverb (240) and TrackDelay.

## Kontakt filters

| Module | Status | Calibration | Corpus use | Notes |
|---|---|---|---|---|
| SV LP1 | missing | unmeasured | 0 | id 52 maps to SV LP2 in the GUI; LP1 id unknown |
| SV LP2 | implemented | measured: cutoff 25*800^x, Q law, bilinear 2-pole (REF s.2) | modelled | id 52 |
| SV LP4 | implemented | shares SV LP2 laws, not measured separately | modelled | id 55 |
| SV LP6 | missing | REF s.2: six cascaded one-poles, -3 dB at 0.352 fc | 0 |  |
| SV HP1 | missing | unmeasured | 0 |  |
| SV HP6 | missing | unmeasured | 0 |  |
| SV HP2 | implemented | shares SV LP2 laws, not measured separately | modelled | id 54 |
| SV HP4 | implemented | shares SV LP2 laws, not measured separately | modelled | id 57 |
| SV BP2 | missing | unmeasured | 0 | id 53/56 per v1 table, unverified |
| SV BP4 | missing | unmeasured | 0 |  |
| SV Notch | missing | unmeasured | 0 |  |
| Ladder LP1 | missing | unmeasured | ids 100-105 (11 slots) and 22-33; 1 instrument | REF s.2: Ladder LP2 = two cascaded one-poles |
| Ladder LP2 | missing | unmeasured | ids 100-105 (11 slots) and 22-33; 1 instrument | REF s.2: Ladder LP2 = two cascaded one-poles |
| Ladder LP3 | missing | unmeasured | ids 100-105 (11 slots) and 22-33; 1 instrument | REF s.2: Ladder LP2 = two cascaded one-poles |
| Ladder LP4 | missing | unmeasured | ids 100-105 (11 slots) and 22-33; 1 instrument | REF s.2: Ladder LP2 = two cascaded one-poles |
| Ladder HP1 | missing | unmeasured | ids 100-105 (11 slots) and 22-33; 1 instrument | REF s.2: Ladder LP2 = two cascaded one-poles |
| Ladder HP2 | missing | unmeasured | ids 100-105 (11 slots) and 22-33; 1 instrument | REF s.2: Ladder LP2 = two cascaded one-poles |
| Ladder HP3 | missing | unmeasured | ids 100-105 (11 slots) and 22-33; 1 instrument | REF s.2: Ladder LP2 = two cascaded one-poles |
| Ladder HP4 | missing | unmeasured | ids 100-105 (11 slots) and 22-33; 1 instrument | REF s.2: Ladder LP2 = two cascaded one-poles |
| Ladder BP2 | missing | unmeasured | ids 100-105 (11 slots) and 22-33; 1 instrument | REF s.2: Ladder LP2 = two cascaded one-poles |
| Ladder BP4 | missing | unmeasured | ids 100-105 (11 slots) and 22-33; 1 instrument | REF s.2: Ladder LP2 = two cascaded one-poles |
| Ladder Peak | missing | unmeasured | ids 100-105 (11 slots) and 22-33; 1 instrument | REF s.2: Ladder LP2 = two cascaded one-poles |
| Ladder Notch | missing | unmeasured | ids 100-105 (11 slots) and 22-33; 1 instrument | REF s.2: Ladder LP2 = two cascaded one-poles |
| Monark | missing | unmeasured | 0 |  |
| Pro-53 | missing | unmeasured | 0 | REF s.2: passband +3.3 dB |
| AR LP2 | missing | unmeasured | 0 | id 100 |
| AR LP4 | missing | unmeasured | 0 | id 103 |
| AR LP2/4 | missing | cutoff law measured: 8.2*4329^x Hz (REF s.8) | 267 slots, ANALOG STRINGS and others (id 106) | pole count and resonance law unknown |
| AR HP2 | missing | unmeasured | 0 |  |
| AR HP4 | missing | unmeasured | 0 |  |
| AR HP2/4 | missing | unmeasured | 0 |  |
| AR BP2 | missing | unmeasured | 0 |  |
| AR BP4 | missing | unmeasured | 0 |  |
| AR BP2/4 | missing | unmeasured | 0 |  |
| Daft | partial | laws verified from original bytes (inventory); audio kernel is a proxy | modelled | ids 70/71 |
| Daft HP | partial | as Daft | modelled | id 71 |
| Legacy LP1 | missing | unmeasured | 0 | id 2 presumed |
| Legacy LP2 | missing | unmeasured | 0 |  |
| Legacy LP4 | missing | unmeasured | 0 |  |
| Legacy LP6 | missing | unmeasured | 0 |  |
| Legacy HP1 | missing | cutoff law unmeasured (GUI 36.1 Hz at stored 0) | 1,996 group-insert slots (Dolce 480 groups, Vista 40, others) | id 3, REF s.3 |
| Legacy HP2 | missing | unmeasured | 0 |  |
| Legacy HP4 | missing | unmeasured | 0 |  |
| Legacy BP2 | missing | unmeasured | 0 |  |
| Legacy BP4 | missing | unmeasured | 0 |  |
| Legacy BR4 | missing | unmeasured | 0 |  |
| Legacy Ladder | missing | unmeasured | 0 |  |
| SV Par. LP/HP | missing | unmeasured | 0 |  |
| SV Par. BP/BP | missing | unmeasured | 0 |  |
| SV Ser. LP/HP | missing | unmeasured | 0 |  |
| 3x2 Versatile | missing | unmeasured | 0 |  |
| Dual SKF | missing | unmeasured | 0 |  |
| Simple LP/HP | missing | unmeasured | 0 |  |
| Formant I | missing | unmeasured | 291 slots (ANALOG STRINGS, Conflux, others) | id 90 |
| Formant II | missing | unmeasured | 0 |  |
| Phaser | missing | unmeasured | 0 | id 13 |
| Vowel A | missing | unmeasured | 0 |  |
| Vowel B | missing | unmeasured | 0 |  |
| Solid G-EQ | missing | unmeasured | 3 instruments, 29 slots (group and bus) |  |

## Kontakt effects

| Module | Status | Calibration | Corpus use | Notes |
|---|---|---|---|---|
| Compressor | partial | measured: ANALOG STRINGS on vs bypassed +8.7 dB Kontakt, +8.2..8.4 dB KONTRA (REF s.26); absolute level of the instrument 14-16 dB hotter than Kontakt | modelled | Classic mode only; modes Enhanced/Pro missing (3 instruments) |
| Feedback Compressor | missing | unmeasured | 3 instruments |  |
| Limiter | missing | unmeasured | 1 instrument | id 0x1c |
| Solid Bus Comp | missing | unmeasured | 3 instruments |  |
| Supercharger GT | missing | unmeasured | 0 |  |
| Transient Master | missing | unmeasured | 3 instruments |  |
| Transparent Limiter | missing | unmeasured | 0 | id 0x5f Hilbert Limiter? |
| ACBox | missing | unmeasured | 0 |  |
| Bass Invader | missing | unmeasured | 0 |  |
| Bass Pro | missing | unmeasured | 0 |  |
| Cabinet | missing | unmeasured | 0 |  |
| EP Preamps | missing | unmeasured | 0 |  |
| HotSolo | missing | unmeasured | 0 |  |
| Jump | missing | unmeasured | 0 |  |
| Reverb Delight | missing | unmeasured | 0 |  |
| Super Fast 100 | missing | unmeasured | 0 |  |
| Twang | missing | unmeasured | 0 |  |
| Van51 | missing | unmeasured | 0 |  |
| Big Fuzz | missing | unmeasured | 0 |  |
| Cat | missing | unmeasured | 0 |  |
| Chainsaw | missing | unmeasured | 0 |  |
| Cry Wah | missing | unmeasured | 0 |  |
| Dirt | missing | unmeasured | 0 |  |
| Distortion | missing | unmeasured | 4 instruments, 11 slots |  |
| DStortion | missing | unmeasured | 0 |  |
| Fuzz | missing | unmeasured | 0 |  |
| Kolor | missing | unmeasured | 0 |  |
| Saturator | missing | unmeasured | 0 |  |
| Skreamer | missing | unmeasured | 1 instrument, 80 group-insert slots |  |
| Skreamer Deluxe | missing | unmeasured | 0 |  |
| Bite | missing | unmeasured | 0 |  |
| Lo-Fi | missing | unmeasured | 4 instruments, 280 slots |  |
| Tape Saturator | missing | unmeasured | 4 instruments |  |
| Wow/Flutter | missing | unmeasured | 0 |  |
| Choral | missing | unmeasured | 0 |  |
| Flair | missing | unmeasured | 0 |  |
| Freak | missing | unmeasured | 0 |  |
| Phasis | missing | unmeasured | 0 |  |
| Ring Modulator | missing | unmeasured | 0 |  |
| Rotator | missing | unmeasured | 3 instruments |  |
| Vibrato/Chorus | missing | unmeasured | 0 |  |
| Legacy Chorus | missing | unmeasured | 4 instruments |  |
| Legacy Flanger | missing | unmeasured | 3 instruments |  |
| Legacy Phaser | missing | unmeasured | 4 instruments |  |
| Beat Masher | missing | unmeasured | 0 |  |
| Beat Slicer | missing | unmeasured | 0 |  |
| Gater | missing | unmeasured | 0 |  |
| Reverse Grain | missing | unmeasured | 0 |  |
| Transpose Stretch | missing | unmeasured | 0 |  |
| PsycheDelay | missing | unmeasured | 0 |  |
| Replika Delay | missing | unmeasured | 0 |  |
| Twin Delay | missing | unmeasured | 0 |  |
| Legacy Delay | missing | unmeasured | 4 instruments, 9 slots |  |
| Convolution | partial | IR length/gain/early-late shaping unmeasured; dry/wet from stored fields | 349 instruments (translated) | 19 instruments report an unmodelled IR parameter; late/early split missing |
| Plate Reverb | missing | unmeasured | 0 |  |
| Raum | missing | unmeasured | 0 |  |
| Reverb | partial | measured: Time (RT60 = 0.82*0.5*40.4^x s), High Cut, Low Shelf displays (REF s.21, s.28); room type, size, damping, modulation, diffusion unmeasured | 371 instruments | own FDN, reported as 'reverb algorithm' unknown law |
| Legacy Reverb | missing | unmeasured | 0 |  |
| Stereo Modeller | partial | measured: spread, pan, output as a 2x2 matrix, residual -119 dB (REF s.20); pseudo-stereo unmeasured | 2 instruments report pseudo-stereo, 8 bus slots | pseudo stereo missing |
| Stereo Tune | missing | unmeasured | 0 |  |
| Surround Panner | missing | unmeasured | 6 instruments, 174 group-insert slots |  |
| AET Filter | missing | unmeasured | 0 |  |
| Gainer | implemented | measured: tau 45 ms (43-49 ms at two step sizes), mix 0.5 on a fresh module (REF s.25); recurrence verified from original bytes | modelled | dry/wet honoured; stored dry is 0 in all corpus slots |
| Inverter | implemented | measured: Output knob does not reach the signal (REF s.27, s.24) | modelled | 1 instrument reports flag order |
| Send Levels | implemented | routing only; levels from stored fields | modelled | bus scope |

## Kontakt source modes

| Module | Status | Calibration | Corpus use | Notes |
|---|---|---|---|---|
| Sampler | implemented | interpolation unmeasured | all |  |
| DFD | implemented | streams from disk; same signal as Sampler | 2,739 instruments, 330,015 groups | reported as 'played as a sampler'; sound is the same, streaming cost is real (underruns measured in perf) |
| Wavetable | missing | unmeasured | 2 instruments, 31 groups | reported as 'wavetable source' |
| Tone Machine | missing | unmeasured | 0 | not present in the corpus |
| Time Machine | missing | unmeasured | 0 | not present in the corpus |
| Time Machine 2 | missing | unmeasured | 0 | not present in the corpus |
| Time Machine Pro | missing | unmeasured | 0 | not present in the corpus |
| Beat Machine | missing | unmeasured | 0 | not present in the corpus |
| S1200 Machine | missing | unmeasured | 0 | not present in the corpus |
| MP60 Machine | missing | unmeasured | 0 | not present in the corpus |

## Kontakt modulation

| Module | Status | Calibration | Corpus use | Notes |
|---|---|---|---|---|
| AHDSR envelope | implemented | measured: attack curve, decay and release times (REF s.5, s.9) | all enveloped groups | hold and volume-envelope laws in REF s.22 |
| Flexible envelope | implemented | measured: segment curve law (REF s.10) | yes |  |
| DBD envelope | missing | unmeasured | 0 reported | no internal-modulator-chunk reports in the corpus |
| LFO | partial | unmeasured | 3 instruments report an unmapped waveform (3,895 entries) | sine/square/triangle mapped; custom weights reported |
| Multi Digital / step modulator | missing | unmeasured | 0 reported |  |
| Envelope follower | missing | unmeasured | 0 reported |  |
| Glide / portamento | missing | unmeasured | not surveyed |  |
| External MIDI / performance sources | partial | unmeasured | velocity, key, CC, pitch bend, aftertouch, constant, script, random mapped | RandomBipolar reported (1 instrument) |
| Module-parameter modulation targets | partial | unmeasured | 1,311 instruments, 63,225 entries report 'modulation of a module parameter' | ahdsr_attack/release, eqGain1-3, filterCutoff, formantTalk, startPhase: see section 'Modulation targets' |
| Signed modulation targets | partial | unmeasured | 1,111 instruments, 67,182 entries | playPos 65,354: sample start position targets |

## UVI/Falcon FX

| Module | Status | Calibration | Corpus use | Notes |
|---|---|---|---|---|
| 3 Band Compressor | missing | unmeasured | 0 |  |
| 3 Band Limiter | missing | unmeasured | 0 |  |
| 3 Band Shelf | missing | unmeasured | 0 |  |
| Analog Chorus | missing | unmeasured | 0 |  |
| Analog Crunch | missing | unmeasured | 0 |  |
| Analog Filter | missing | unmeasured | 0 |  |
| Analog Flanger | missing | unmeasured | 0 |  |
| Analog Tape Delay | missing | unmeasured | 0 |  |
| Autopan | missing | unmeasured | 0 |  |
| Big Pi Tone | missing | unmeasured | 0 |  |
| Biquad Filter | missing | unmeasured | 0 |  |
| Bloom | missing | unmeasured | 0 |  |
| Brickwall Filter | missing | unmeasured | 0 |  |
| Comb Filter | missing | unmeasured | 620 programs |  |
| Compressor Expander | partial | no render comparison | 0 | translated as CompExp |
| Convolver | missing | unmeasured | 416 (376 in VWinds) |  |
| Crossover Filter | missing | unmeasured | 0 |  |
| Diffuse Delay | missing | unmeasured | 0 |  |
| Diffusion | missing | unmeasured | 0 |  |
| Digital Eq | partial | no render comparison (no native rig) | 80+ | bands as peaking biquads; shape unmeasured |
| Digital Filter | missing | unmeasured | 0 |  |
| Diode Clipper | missing | unmeasured | 620 |  |
| Dispersor | missing | unmeasured | 0 |  |
| Drive | missing | unmeasured | 620 |  |
| Dual Delay X | missing | unmeasured | 0 |  |
| Effect Rack | implemented | no render comparison (no native rig) | 440 chains | serial/parallel branches |
| Ensemble 505 | missing | unmeasured | 0 |  |
| Exciter | missing | unmeasured | 0 |  |
| Feedback Compressor | missing | unmeasured | 0 |  |
| Feedback Machine | missing | unmeasured | 620 |  |
| Flanger | missing | unmeasured | 620 |  |
| Formant Crusher | missing | decimator kernel verified from original bytes (fractional period, 495 blocks); public Morph/Q/Formant/Mix/Bite/FilterA/B mapping unknown, core Decimate primitive exists | 0 | wiring waits on the mapping |
| Freq Shifter | missing | unmeasured | 0 |  |
| Fuzz | missing | unmeasured | 0 |  |
| Gain | implemented | no render comparison (no native rig) | 988+ | Volume; dB law from the catalog |
| Gain Matrix | partial | kernel verified from original bytes where noted; no render comparison (no native rig) | 6,180 elements | topology from the inventory (static code); arithmetic order unverified |
| Gate | missing | unmeasured | 0 |  |
| Granulizer | missing | unmeasured | 0 |  |
| Guitar Boxes | missing | unmeasured | 0 |  |
| Harmonic Resonators | missing | unmeasured | 0 |  |
| Harmonizer | missing | unmeasured | 0 |  |
| IReverb | missing | unmeasured | 0 |  |
| Ladder | missing | unmeasured | 0 |  |
| LowPass 12 | missing | unmeasured | 0 |  |
| LowPass 24 | missing | unmeasured | 0 |  |
| Magnetic Bass Shaper | missing | unmeasured | 0 |  |
| Maximizer | missing | unmeasured | 620 |  |
| One Pole | partial | kernel verified from original bytes where noted; no render comparison (no native rig) | 6,318 inserts (VWinds) | coefficient law: inventory documents the key-tracking exponent but not the runtime multiplier |
| Opal | missing | unmeasured | 0 |  |
| Overdrive | missing | unmeasured | 0 |  |
| Phase Meter | missing | unmeasured | 0 |  |
| Phasor | missing | unmeasured | 620 |  |
| Phasor Filter | missing | unmeasured | 0 |  |
| Redux | missing | unmeasured | 0 |  |
| Rez Filter | missing | unmeasured | 0 |  |
| Rotary | missing | unmeasured | 0 |  |
| SVF | missing | unmeasured | 0 |  |
| Sallen-Key Filter | missing | unmeasured | 0 |  |
| Shifter | missing | unmeasured | 0 |  |
| SparkVerb | missing | unmeasured | 620 |  |
| Spectrum Analyzer | missing | unmeasured | 0 |  |
| Studio Limiter | missing | unmeasured | 0 |  |
| TS Overdrive | missing | unmeasured | 0 |  |
| Tape Echo | missing | unmeasured | 0 |  |
| Thorus | missing | unmeasured | 0 |  |
| Tilt | missing | unmeasured | 0 |  |
| Tone Stack | missing | unmeasured | 0 |  |
| Track Delay | missing | unmeasured | 40 |  |
| Tremolo | missing | unmeasured | 0 |  |
| Tube Amp | missing | unmeasured | 0 |  |
| Tuner | missing | unmeasured | 0 |  |
| UVI Filter | missing | unmeasured | 0 |  |
| UVI Wide | missing | unmeasured | 0 |  |
| UVInyl | missing | unmeasured | 0 |  |
| VCF-20 | missing | unmeasured | 0 |  |
| VCF-20 Dual | missing | unmeasured | 0 |  |
| VCF-4023 | missing | unmeasured | 0 |  |
| Velvet Delay | missing | unmeasured | 0 |  |
| Vowel Filter | missing | unmeasured | 0 |  |
| Vowels | missing | unmeasured | 0 |  |
| WahWah | missing | unmeasured | 0 |  |
| Wave Shaper | partial | kernel verified from original bytes where noted; no render comparison (no native rig) | 620 | rectifier modes verified from original bytes (full/half); other shaping modes missing |
| Xpander Filter | missing | unmeasured | 620 |  |

## UVI/Falcon legacy processors

| Module | Status | Calibration | Corpus use | Notes |
|---|---|---|---|---|
| 2 Band EQ | missing | unmeasured | 0 |  |
| 3 Band EQ | missing | unmeasured | 0 |  |
| 8 Band EQ | missing | unmeasured | 0 |  |
| Auto Wah | missing | unmeasured | 0 |  |
| Beat Repeat | missing | unmeasured | 0 |  |
| Chorus | missing | unmeasured | 0 |  |
| Compressor | missing | unmeasured | 0 |  |
| Cross Phaser | missing | unmeasured | 0 |  |
| Double Drive | missing | unmeasured | 0 |  |
| Dual Delay | missing | unmeasured | 0 |  |
| FX Delay | missing | unmeasured | 0 |  |
| FX Filter | missing | unmeasured | 0 |  |
| Fat Delay | missing | unmeasured | 0 |  |
| Gate Reverb | missing | unmeasured | 0 |  |
| Limiter | missing | unmeasured | 0 |  |
| Phaser | missing | unmeasured | 0 |  |
| Ping Pong Delay | missing | unmeasured | 0 |  |
| Plain Reverb | missing | unmeasured | 0 |  |
| Predelay Verb | missing | unmeasured | 0 |  |
| Redux | missing | unmeasured | 0 |  |
| Ring Modulator | missing | unmeasured | 0 |  |
| Robotizer | missing | unmeasured | 0 |  |
| Rotary | missing | unmeasured | 0 |  |
| Rotary Simple | missing | unmeasured | 0 |  |
| Rotary Speaker | missing | unmeasured | 0 |  |
| Simple Delay | missing | unmeasured | 0 |  |
| Simple Reverb | missing | unmeasured | 0 |  |
| Stereo Delay | missing | unmeasured | 0 |  |
| TalkBox | missing | unmeasured | 0 |  |
| UVI Destructor | missing | unmeasured | 0 |  |
| UVI Drive | missing | unmeasured | 0 |  |
| UVI Mastering | missing | unmeasured | 0 |  |

## UVI/Falcon oscillators

| Module | Status | Calibration | Corpus use | Notes |
|---|---|---|---|---|
| 8o8 Bass Drum | missing | unmeasured | 0 in the local corpus |  |
| Additive | missing | unmeasured | 0 in the local corpus |  |
| Analog | missing | unmeasured | 0 in the local corpus |  |
| Analog Stack | missing | unmeasured | 0 in the local corpus |  |
| Bowed String | missing | unmeasured | 0 in the local corpus |  |
| Drum | missing | unmeasured | 0 in the local corpus |  |
| FM | missing | unmeasured | 0 in the local corpus |  |
| Grains | missing | unmeasured | 0 in the local corpus |  |
| Harmonic Resonators | missing | unmeasured | 0 in the local corpus |  |
| IRCAM Granular | missing | unmeasured | 0 in the local corpus |  |
| IRCAM Multi Granular | missing | unmeasured | 0 in the local corpus |  |
| IRCAM Scrub | missing | unmeasured | 0 in the local corpus |  |
| IRCAM Stretch | missing | unmeasured | 0 in the local corpus |  |
| Noise | missing | unmeasured | 0 in the local corpus |  |
| Organ | missing | unmeasured | 0 in the local corpus |  |
| Phase Shaper | missing | unmeasured | 0 in the local corpus |  |
| Pluck | missing | unmeasured | 0 in the local corpus |  |
| Sample | partial | unmeasured (no native rig) | 660+40 programs (SamplePlayer) | zones, loops, tune, gain, pan, key tracking; SampleStart, PlayDirection, extra loops reported |
| Slice | missing | unmeasured | 0 in the local corpus |  |
| Stretch | missing | unmeasured | 0 in the local corpus |  |
| SupraSaw | missing | unmeasured | 0 in the local corpus |  |
| Texture | missing | unmeasured | 0 in the local corpus |  |
| VOSIM | missing | unmeasured | 0 in the local corpus |  |
| Wavetable | missing | unmeasured | 0 in the local corpus |  |

## UVI/Falcon modulation

| Module | Status | Calibration | Corpus use | Notes |
|---|---|---|---|---|
| AHD | implemented | unmeasured (no native rig) | 620 programs | DAHDSR/AHD share a translation |
| Analog ADSR | missing | unmeasured (no native rig) | 0 |  |
| Attack Decay | missing | unmeasured (no native rig) | 0 |  |
| DAHDSR | partial | unmeasured (no native rig) | 620 programs | linear stages instead of the analog law |
| Drunk | missing | unmeasured (no native rig) | 0 |  |
| Flow Noise | missing | unmeasured (no native rig) | 0 |  |
| LFO | partial | unmeasured (no native rig); LFO phase mechanics verified from original bytes for custom tables | 660 programs | shared-instance behaviour inferred |
| Macro | missing | unmeasured (no native rig) | 0 |  |
| Multi Envelope | missing | unmeasured (no native rig) | 0 |  |
| Multi LFO | implemented | unmeasured (no native rig); LFO phase mechanics verified from original bytes for custom tables | 620 programs |  |
| Parametric LFO | missing | unmeasured (no native rig) | 0 |  |
| Script Event Modulation | implemented | unmeasured (no native rig) | 660 programs | driven by sendScriptModulation |
| Smooth Random | missing | unmeasured (no native rig) | 0 |  |
| Step Envelope | implemented | unmeasured (no native rig) | 620 programs |  |
| Voice Modulator | missing | unmeasured (no native rig) | 0 |  |
| Constant Modulation | implemented | n/a | 660 programs |  |
