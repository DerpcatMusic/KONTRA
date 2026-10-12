# Kontakt DSP effect catalogue

Checked 2026-10-09. This catalogue covers all **65 effect and 59 filter entries** in the pinned Kontakt reference. Each entry records an engineering processing contract, control identifiers and native evidence. Names/controls are interface facts; the processing contracts are engineering interpretations unless explicitly marked as an executed kernel. Model names do not establish circuit equations.

**L** = documented interface with native association unresolved. **C** = native RTTI/type/table candidate found; public-name mapping and audio algorithm not verified. **K** = bounded original-byte execution verifies the stated kernel or control behaviour only. None means a complete effect or host-level match. Abbreviated class associations (notably Trans, Shaper, IRC and Pro52) remain candidates.

Native addresses use **Kontakt 8.13.1 standalone**, SHA-256 `0fe6356e0879d058b6e5b73507c54c5e345cea451b35287c974e438291d4dae8`, image base `0x140000000`. They must not be transplanted into the separate VST3 audio payload. The proof corpus is [vtable-methods.csv](../artifacts/engine-analysis-2026-10-07/kontakt-engine/vtable-methods.csv); exact C locations refer to [pseudocode.c](../artifacts/engine-analysis-2026-10-07/kontakt-engine/complete-export/pseudocode.c). Decompiler completion is not algorithm correctness.

The tracked [evidence manifest](research/dsp-effects-20261009/catalogue-evidence.json) preserves one record per entry, public source anchors, source hashes and native table/entry locations. The [catalogue index and verified equations](DSP_EFFECT_CATALOGUE.md) explains the checks and remaining work.

## Effects — Dynamics

### Compressor

Envelope-dependent gain reduction; preserve detector mode and channel linking. The native signed-mean linked detector has been checked; the complete gain law has not.

Control labels: `Mode`, `St. Link`, `Thresh.`, `Ratio`, `Attack`, `Release`, `Output`.

C: `BEffectCompressor`; vtable RVA `0x4e94d60`, candidate slot 23 `0x1409047a0` (C line 3610869, completed). Complete effect unverified. **K (partial):** see the named check in the [verified-kernel index](DSP_EFFECT_CATALOGUE.md#verified-processing-details). [NI interface](https://docs.native-instruments.com/ni-tech-manuals/kontakt-manual/en/effect-reference#compressor).

### Feedback Compressor

Feedback-controlled dynamics with parallel blend and an optional quality path; detector placement and nonlinear gain law need reconstruction.

Control labels: `High Quality`, `St. Link`, `Input`, `Ratio`, `Attack`, `Release`, `Makeup`, `Mix`, `Output`, `Gain Reduction Meter`.

C: `BEffectFBComp`; vtable RVA `0x4fc0d80`, candidate slot 23 `0x1408f96d0` (C line 3583382, completed_with_warnings). Complete effect unverified. [NI interface](https://docs.native-instruments.com/ni-tech-manuals/kontakt-manual/en/effect-reference#feedback-compressor).

### Limiter

Peak containment with input gain and release memory; threshold, lookahead and recovery behaviour need native confirmation.

Control labels: `In Gain`, `Release`, `Attenuation`, `Output`.

C: `BEffectLimiter`; vtable RVA `0x4f146f8`, candidate slot 23 `0x140904890` (C line 3615526, completed_with_warnings). Complete effect unverified. [NI interface](https://docs.native-instruments.com/ni-tech-manuals/kontakt-manual/en/effect-reference#limiter).

### Solid Bus Comp

Bus dynamics with ratio, timing, makeup and parallel mixing; stereo coupling and the model-specific detector matter.

Control labels: `Link`, `Threshold`, `Ratio`, `Attack`, `Release`, `Makeup`, `Mix`, `Output`.

C: `BEffectSSLGBusComp`; vtable RVA `0x4f15700`, candidate slot 23 `0x1408fbb60` (C line 3587031, completed_with_warnings). Complete effect unverified. [NI interface](https://docs.native-instruments.com/ni-tech-manuals/kontakt-manual/en/effect-reference#solid-bus-comp).

### Supercharger GT

Dynamics combined with saturation, detector high-pass, character modes and stereo/dual-mono/MS linking; several processing stages must be preserved.

Control labels: `Input`, `Input Level Meter`, `HP Detector`, `Saturation`, `Saturation Mode`, `Compress`, `Gain Reduction Meter`, `Attack`, `Release`, `Character`, `Character Mode`, `Channel Link`, `Mix`, `Output`.

C: `BEffectSupercharger`; vtable RVA `0x4e96d50`, candidate slot 23 `0x140903680` (C line 3608124, completed_with_warnings). Complete effect unverified. [NI interface](https://docs.native-instruments.com/ni-tech-manuals/kontakt-manual/en/effect-reference#supercharger-gt).

### Transient Master

Attack/sustain envelope reshaping with a smooth mode; an implementation needs separate transient and sustain control rather than only a threshold/ratio curve.

Control labels: `Smooth`, `Input`, `Attack`, `Sustain`, `Output`.

C: `BEffectTrans`; vtable RVA `0x4e95dc0`, candidate slot 23 `0x1408fc620` (C line 3588216, completed_with_warnings). Complete effect unverified. [NI interface](https://docs.native-instruments.com/ni-tech-manuals/kontakt-manual/en/effect-reference#transient-master).

### Transparent Limiter

Ceiling-controlled dynamics with release; a named HilbertLimiter core exists, but that name alone does not establish its signal-processing equations.

Control labels: `Threshold`, `Release`, `Ceiling`, `Gain Reduction Meter`, `Output`.

C: `BEffectHilbertLimiter`; vtable RVA `0x4f142c8`, candidate slot 23 `0x1409008a0` (C line 3601319, completed_with_warnings). Complete effect unverified. [NI interface](https://docs.native-instruments.com/ni-tech-manuals/kontakt-manual/en/effect-reference#transparent-limiter).


## Effects — Amplifiers

### ACBox

Amp model with two volume paths, tone controls and tremolo. Native ACBOX stage-specific waveshaping table classes exist; stage connections and curves remain unverified.

Control labels: `Mono`, `Norm`, `Brill`, `Trem`, `Treble`, `Bass`, `Tone`, `Output`.

C: `BEffectACBox`; vtable RVA `0x4fc0708`, candidate slot 23 `0x140a9fec0` (C line 3846253, completed). Complete effect unverified. [NI interface](https://docs.native-instruments.com/ni-tech-manuals/kontakt-manual/en/effect-reference#acbox).

### Bass Invader

Bass amp model with boost, contour, low cut, tone stages and master gain; preserve stage order and nonlinear drive.

Control labels: `High Boost`, `Mid Contour`, `Low Cut`, `Volume`, `Boost`, `Bass`, `Lo Mid`, `Hi Mid`, `Treble`, `Master`, `Output`.

C: `BEffectBassInvader`; vtable RVA `0x4f13c88`, candidate slot 23 `0x1408fc950` (C line 3590951, completed_with_warnings). Complete effect unverified. [NI interface](https://docs.native-instruments.com/ni-tech-manuals/kontakt-manual/en/effect-reference#bass-invader).

### Bass Pro

Bass amp model with switchable voicing, graphic EQ, drive and master gain; the graphic stage is a separate part of the contract.

Control labels: `Bright`, `Ultra Hi`, `Ultra Lo`, `Mono`, `Graphic EQ`, `Graphic EQ Gain`, `Graphic EQ Volume`, `Gain`, `Bass`, `Mid`, `Mid Freq`, `Treble`, `Drive`, `Master`, `Output`.

C: `BEffectBassPro`; vtable RVA `0x4fc1000`, candidate slot 23 `0x140a9ff80` (C line 3846625, completed). Complete effect unverified. [NI interface](https://docs.native-instruments.com/ni-tech-manuals/kontakt-manual/en/effect-reference#bass-pro).

### Cabinet

Speaker/cabinet coloration with model, size and air controls; neither convolution nor a particular filter network is established by these controls.

Control labels: `Cabinet Type`, `Size`, `Air`, `TREB`, `BASS`, `Output`.

C: `BEffectCabinet`; vtable RVA `0x4f14c20`, candidate slot 23 `0x140aa0170` (C line 3847698, completed_with_warnings). Complete effect unverified. [NI interface](https://docs.native-instruments.com/ni-tech-manuals/kontakt-manual/en/effect-reference#cabinet).

### EP Preamps

Electric-piano preamp coloration with drive/EQ modes and tremolo waveform/stereo controls; gain nonlinearity and modulation are separate stages.

Control labels: `Drive Mode`, `EQ Mode`, `Tremolo Mode`, `Wave Mode`, `Drive`, `Bass`, `Mid`, `Treble`, `Rate`, `Amount`, `Width`, `Mono`, `Output`.

C: `BEffectEPPreamps`; vtable RVA `0x4e92f58`, candidate slot 23 `0x1408ff1a0` (C line 3593680, completed_with_warnings). Complete effect unverified. [NI interface](https://docs.native-instruments.com/ni-tech-manuals/kontakt-manual/en/effect-reference#ep-preamps).

### HotSolo

Amp model with normal/overdrive paths, presence and depth; HOTSOLO per-stage waveshaping table classes provide identity evidence, not verified transfer curves.

Control labels: `Overdrive`, `Mono`, `Bass`, `Mid`, `Treble`, `Presence`, `Norm`, `Ovrd`, `Master`, `Depth`, `Output`.

C: `BEffectHotSolo`; vtable RVA `0x4fc0900`, candidate slot 23 `0x140aa2ae0` (C line 3850330, completed). Complete effect unverified. [NI interface](https://docs.native-instruments.com/ni-tech-manuals/kontakt-manual/en/effect-reference#hotsolo).

### Jump

Amp model with high-gain switching, preamp/master and presence; JUMP stage and power-tube table classes exist, with the complete circuit path unresolved.

Control labels: `HiGain`, `Mono`, `PreAmp`, `Master`, `Bass`, `Mid`, `Treble`, `Presence`, `Output`.

C: `BEffectJump`; vtable RVA `0x4fc0330`, candidate slot 23 `0x140aa33c0` (C line 3851062, completed). Complete effect unverified. [NI interface](https://docs.native-instruments.com/ni-tech-manuals/kontakt-manual/en/effect-reference#jump).

### Reverb Delight

Amp processing plus selectable reverb and tremolo with sag; preserve the interaction between amp drive, tremolo and reverb.

Control labels: `Amp Vol`, `Bright`, `Mono`, `Bass`, `Treble`, `Reverb`, `Type`, `Verb Vol`, `Tremolo`, `Trem Dpt`, `Trem Spd`, `Trem Sag`, `Output`.

C: `BEffectReverbDelight`; vtable RVA `0x4fc1548`, candidate slot 23 `0x140a9ef50` (C line 3844789, completed_with_warnings). Complete effect unverified. [NI interface](https://docs.native-instruments.com/ni-tech-manuals/kontakt-manual/en/effect-reference#reverb-delight).

### Super Fast 100

Amp model with normal/overdrive, bright/crunch, tone, presence and depth controls; switching affects a multi-stage processing contract.

Control labels: `Bright`, `Crunch`, `Bass`, `Mid`, `Treble`, `Presence`, `Output`, `Overdrive switch`, `Normal`, `Overdrive`, `Master`, `Depth`.

C: `BEffectSuperFast100`; vtable RVA `0x4f16cd0`, candidate slot 23 `0x140902d80` (C line 3609598, completed_with_warnings). Complete effect unverified. [NI interface](https://docs.native-instruments.com/ni-tech-manuals/kontakt-manual/en/effect-reference#super-fast-100).

### Twang

Amp coloration with bright switching and voice-scope selection; per-voice and summed processing can give different nonlinear results.

Control labels: `Bright`, `Polyphonic`, `Volume`, `Treble`, `Output`.

C: `BEffectTwang`; vtable RVA `0x4f14b18`, candidate slot 23 `0x140ade200` (C line 3924818, completed_with_warnings). Complete effect unverified. [NI interface](https://docs.native-instruments.com/ni-tech-manuals/kontakt-manual/en/effect-reference#twang).

### Van51

Amp model with rhythm/lead and gain/voicing switches; VAN51 stage-specific and power-tube table classes exist, while exact stage routing is unresolved.

Control labels: `Lead`, `HiGain`, `Bright`, `Crunch`, `Mono`, `Rhythm`, `Bass`, `Middle`, `Treble`, `Post Gain`, `Presence`, `Reso`, `Output`.

C: `BEffectVan51`; vtable RVA `0x4fc0570`, candidate slot 23 `0x140aa74b0` (C line 3856041, completed). Complete effect unverified. [NI interface](https://docs.native-instruments.com/ni-tech-manuals/kontakt-manual/en/effect-reference#van51).


## Effects — Stomps

### Big Fuzz

Fuzz nonlinearity plus sustain/tone/voicing; a BigFuzz table class is present, but no verified transfer curve is supplied.

Control labels: `Mono`, `Sustain`, `Tone`, `Bass`, `Bright`, `Output`.

C: `BEffectBigFuzz`; vtable RVA `0x4fc1878`, candidate slot 23 `0x140aa0090` (C line 3846770, completed). Complete effect unverified. [NI interface](https://docs.native-instruments.com/ni-tech-manuals/kontakt-manual/en/effect-reference#big-fuzz).

### Cat

Distortion with filter, voicing and tone stages; input level and ordering affect the generated harmonics.

Control labels: `Mono`, `Volume`, `Filter`, `Distortion`, `Balls`, `Bass`, `Treble`, `Tone`, `Output`.

C: `BEffectCat`; vtable RVA `0x4fc0a38`, candidate slot 23 `0x140aa0450` (C line 3848162, completed). Complete effect unverified. [NI interface](https://docs.native-instruments.com/ni-tech-manuals/kontakt-manual/en/effect-reference#cat).

### Chainsaw

Distortion with independent low/high shaping and output level; its specialized clipping law remains unverified.

Control labels: `Distortion`, `Low`, `High`, `Level`, `Output`.

C: `BEffectChainsaw`; vtable RVA `0x4f15e68`, candidate slot 23 `0x1408fe290` (C line 3593991, completed_with_warnings). Complete effect unverified. [NI interface](https://docs.native-instruments.com/ni-tech-manuals/kontakt-manual/en/effect-reference#chainsaw).

### Cry Wah

Pedal-controlled moving resonance; preserve the pedal-to-frequency/Q law and nonlinear behaviour.

Control labels: `Pedal`, `Mono`, `Output`.

C: `BEffectCryWah`; vtable RVA `0x4fc0c78`, candidate slot 23 `0x140aa0f10` (C line 3848408, completed_with_warnings). Complete effect unverified. [NI interface](https://docs.native-instruments.com/ni-tech-manuals/kontakt-manual/en/effect-reference#cry-wah).

### Dirt

Two distortion sections A/B with separate drive, mode and amount, plus bias, tilt and routing; the routing choice is part of the DSP state.

Control labels: `Safety A/B`, `Drive A/B`, `Mode A/B`, `Amount A/B`, `Bias`, `Tilt`, `Routing`, `Mix`, `Output`.

C: `BEffectDirt`; vtable RVA `0x4fc1218`, candidate slot 23 `0x140a9e940` (C line 3842800, completed_with_warnings). Complete effect unverified. [NI interface](https://docs.native-instruments.com/ni-tech-manuals/kontakt-manual/en/effect-reference#dirt).

### Distortion

Mode-selectable drive with damping; mode changes need a distinct shaping/filter path.

Control labels: `Mode menu`, `Drive`, `Damping`, `Output`.

C: `BEffectDist`; vtable RVA `0x4e95ba0`, candidate slot 23 `0x140aa1580` (C line 3850694, completed_with_warnings). Complete effect unverified. [NI interface](https://docs.native-instruments.com/ni-tech-manuals/kontakt-manual/en/effect-reference#distortion).

### DStortion

Drive with tone and three EQ controls; clipping curve and EQ position remain unresolved.

Control labels: `Mono`, `Volume`, `Tone`, `Drive`, `Bass`, `Mid`, `Treble`, `Output`.

C: `BEffectDStortion`; vtable RVA `0x4fc0b70`, candidate slot 23 `0x140aa1070` (C line 3848632, completed). Complete effect unverified. [NI interface](https://docs.native-instruments.com/ni-tech-manuals/kontakt-manual/en/effect-reference#dstortion).

### Fuzz

Fuzz with amount and bass/bright voicing; a FuzzFace table class is present, without verified table values or signal-path placement.

Control labels: `Mono`, `Amount`, `Bass`, `Bright`, `Output`.

C: `BEffectFuzz`; vtable RVA `0x4fc1fe0`, candidate slot 23 `0x140aa28a0` (C line 3849931, completed). Complete effect unverified. [NI interface](https://docs.native-instruments.com/ni-tech-manuals/kontakt-manual/en/effect-reference#fuzz).

### Kolor

Mode-dependent nonlinear coloration, gain boost, bass preservation and HP/LP/mid shaping; preserve mode and the filter/drive order.

Control labels: `Mode`, `Gain Boost`, `Drive`, `Bass`, `Mid`, `Treble`, `Mix`, `Boost`, `Bass Saver`, `Highpass`, `Freq mid`, `Q mid`, `Lowpass`, `Output`.

C: `BEffectKolor`; vtable RVA `0x4e93550`, candidate slot 23 `0x140900eb0` (C line 3603231, completed_with_warnings). Complete effect unverified. [NI interface](https://docs.native-instruments.com/ni-tech-manuals/kontakt-manual/en/effect-reference#kolor).

### Saturator

Mode-dependent nonlinear shaping; Classic, Enhanced and Drums require separate transfer/state behaviour. The abbreviated native class association is a candidate.

Control labels: `Mode menu`, `Saturation`, `Output`.

C: `BEffectShaper`; vtable RVA `0x4e94f70`, candidate slot 23 `0x140aa4d20` (C line 3854835, completed_with_warnings). Complete effect unverified. [NI interface](https://docs.native-instruments.com/ni-tech-manuals/kontakt-manual/en/effect-reference#saturator).

### Skreamer

Overdrive with tone/voicing and clean blend; nonlinear wet processing and clean summing are separate.

Control labels: `Tone`, `Drive`, `Bass`, `Bright`, `Clean`, `Output`.

C: `BEffectSkreamer`; vtable RVA `0x4f14908`, candidate slot 23 `0x140aa5810` (C line 3854283, completed_with_warnings). Complete effect unverified. [NI interface](https://docs.native-instruments.com/ni-tech-manuals/kontakt-manual/en/effect-reference#skreamer).

### Skreamer Deluxe

Mode-dependent overdrive with tone and bass/mid/treble controls; its filter topology and drive law remain unresolved.

Control labels: `Mode`, `Drive`, `Tone`, `Output`, `Bass`, `Mid`, `Treble`.

C: `BEffectSkreamerDlx`; vtable RVA `0x4e93da8`, candidate slot 23 `0x140902480` (C line 3606665, completed_with_warnings). Complete effect unverified. [NI interface](https://docs.native-instruments.com/ni-tech-manuals/kontakt-manual/en/effect-reference#skreamer-deluxe).


## Effects — Lo-Fi

### Bite

Quantization and rate reduction with jitter, pre/post filters, dither, saturation and DC handling; stochastic state and quantizer boundaries matter.

Control labels: `Freq`, `Jitter`, `Pre Filt`, `Post Filt`, `HP`, `Saturate`, `Bits`, `Crunch`, `Dither`, `Expand`, `DC`, `Mix`, `Output`.

C: `BEffectBite`; vtable RVA `0x4e92d38`, candidate slot 23 `0x1408fdc80` (C line 3592053, completed_with_warnings). Complete effect unverified. [NI interface](https://docs.native-instruments.com/ni-tech-manuals/kontakt-manual/en/effect-reference#bite).

### Lo-Fi

Bit/rate reduction plus colored noise; sample holding, quantization and noise generation are independent behaviours.

Control labels: `Bits`, `S.Rate`, `Noise`, `N.Color`, `Output`.

C: `BEffectLoFi`; vtable RVA `0x4f145f0`, candidate slot 23 `0x140aa34b0` (C line 3852568, completed_with_warnings). Complete effect unverified. [NI interface](https://docs.native-instruments.com/ni-tech-manuals/kontakt-manual/en/effect-reference#lo-fi-528022).


## Effects — Tape

### Tape Saturator

Tape-style nonlinear coloration with warmth, high-frequency rolloff and a quality switch; exact saturation/history/filter design remains unresolved.

Control labels: `High Quality`, `Gain`, `Warmth`, `HF Rolloff`, `Output`.

C: `BEffectTape`; vtable RVA `0x4f14e40`, candidate slot 23 `0x1408fc360` (C line 3587875, completed_with_warnings). Complete effect unverified. [NI interface](https://docs.native-instruments.com/ni-tech-manuals/kontakt-manual/en/effect-reference#tape-saturator).

### Wow/Flutter

Time-varying pitch instability with separate scrape/wow/flutter, speed, age, stereo, gate and saturation controls; retain modulation history.

Control labels: `Scrape`, `Wow`, `Flutter`, `Speed`, `Stereo`, `Age`, `Gate`, `Saturate`, `Mix`, `Output`.

C: `BEffectWowFlutter`; vtable RVA `0x4fc2e48`, candidate slot 23 `0x140a9f850` (C line 3845898, completed_with_warnings). Complete effect unverified. [NI interface](https://docs.native-instruments.com/ni-tech-manuals/kontakt-manual/en/effect-reference#wow-flutter).


## Effects — Modulation

### Choral

Multi-voice modulation with mode, scatter, stereo width, feedback and inversion; voice count and inter-voice phase need preservation.

Control labels: `Rate`, `Mode`, `Synth`, `Ensemble`, `Dimension`, `Universal`, `Amount`, `Voices`, `Delay`, `Width`, `Feedback`, `Scatter`, `Mix`, `Invert`, `Output`.

C: `BEffectChoral`; vtable RVA `0x4e96a58`, candidate slot 23 `0x1408feb90` (C line 3592988, completed_with_warnings). Complete effect unverified. [NI interface](https://docs.native-instruments.com/ni-tech-manuals/kontakt-manual/en/effect-reference#choral).

### Flair

Multiple tuned modulation voices with chord/scan, thru-zero, feedback and detune controls; pitch-related delay lengths distinguish it from a single flanger.

Control labels: `Mode`, `Standard`, `Thru Zero`, `Scan`, `Offset`, `Chord`, `Invert Phase`, `Rate`, `Feedback`, `Amount`, `Width`, `Pitch`, `Damping`, `Voices`, `Detune`, `Mix`, `Output`.

C: `BEffectFlair`; vtable RVA `0x4e96838`, candidate slot 23 `0x1408ff7b0` (C line 3598276, completed_with_warnings). Complete effect unverified. [NI interface](https://docs.native-instruments.com/ni-tech-manuals/kontakt-manual/en/effect-reference#flair).

### Freak

Carrier-based processing with radio, oscillator and sidechain modes; carrier generation, modulation type, filtering and feedback require distinct paths.

Control labels: `Tuning`, `Demod`, `Width`, `Carrier`, `Gate`, `Freq`, `Wide Range`, `Stereo`, `Antifold`, `Contour`, `Release`, `BP Filter`, `BP Freq`.

C: `BEffectFreak`; vtable RVA `0x4fc1328`, candidate slot 23 `0x1408ffc90` (C line 3598918, completed_with_warnings). Complete effect unverified. [NI interface](https://docs.native-instruments.com/ni-tech-manuals/kontakt-manual/en/effect-reference#freak).

### Phasis

Moving notch/phase structure with notch count, center/spread, feedback, modulation mixing and an extended range; keep phase state and polarity.

Control labels: `Rate`, `ULTRA mode`, `Amount`, `Center`, `Stereo`, `Spread`, `Feedback`, `Mod Mix`, `Notches`, `Invert Phase`, `Invert Mod Mix`, `Mix`, `Output`.

C: `BEffectPhasis`; vtable RVA `0x4e96618`, candidate slot 23 `0x1409014c0` (C line 3602670, completed_with_warnings). Complete effect unverified. [NI interface](https://docs.native-instruments.com/ni-tech-manuals/kontakt-manual/en/effect-reference#phasis).

### Ring Modulator

Audio/carrier multiplication with selectable LFO shape, FM, frequency and edge controls; oscillator phase and nesting matter.

Control labels: `Fast Mode`, `LFO: Sine/Square`, `Ring`, `Rate`, `FM`, `Amount`, `Freq`, `Edge`, `Output`.

C: `BEffectRingModulator`; vtable RVA `0x4fc0e98`, candidate slot 23 `0x140aa4830` (C line 3853282, completed). Complete effect unverified. [NI interface](https://docs.native-instruments.com/ni-tech-manuals/kontakt-manual/en/effect-reference#ring-modulator).

### Rotator

Rotating-speaker processing with acceleration/braking, distance and balance; speed-transition state affects the output.

Control labels: `Speed`, `Acceleration and Brake Speed`, `Balance`, `Distance`.

C: `BEffectRotator`; vtable RVA `0x4f14a10`, candidate slot 23 `0x140aa4900` (C line 3853315, completed). Complete effect unverified. [NI interface](https://docs.native-instruments.com/ni-tech-manuals/kontakt-manual/en/effect-reference#rotator).

### Vibrato/Chorus

Pitch modulation with chorus blending, color and stereo width; retain modulation phase and the dry/wet relationship.

Control labels: `Depth`, `Color`, `Rate`, `Blend`, `Width`, `Mix`, `Output`.

C: `BEffectVibratoChorus`; vtable RVA `0x4e94280`, candidate slot 23 `0x140904160` (C line 3611495, completed_with_warnings). Complete effect unverified. [NI interface](https://docs.native-instruments.com/ni-tech-manuals/kontakt-manual/en/effect-reference#vibrato-chorus).

### Legacy Chorus

Modulated short-delay processing with depth, speed and phase; interpolation, delay history and stereo phase remain unresolved.

Control labels: `Depth`, `Speed`, `Phase`, `Return`, `Dry`.

C: `BEffectChorus`; vtable RVA `0x4f13d98`, candidate slot 23 `0x140aa0510` (C line 3849207, completed_with_warnings). Complete effect unverified. [NI interface](https://docs.native-instruments.com/ni-tech-manuals/kontakt-manual/en/effect-reference#legacy-chorus).

### Legacy Flanger

Modulated comb response with feedback, color and phase; interpolation and feedback history must be retained.

Control labels: `Depth`, `Speed`, `Phase`, `Colour`, `Feedback`, `Return`, `Dry`.

C: `BEffectFlanger`; vtable RVA `0x4f13ea0`, candidate slot 23 `0x140aa2350` (C line 3850380, completed_with_warnings). Complete effect unverified. [NI interface](https://docs.native-instruments.com/ni-tech-manuals/kontakt-manual/en/effect-reference#legacy-flanger).

### Legacy Phaser

Modulated phase/notch processing with stereo phase and feedback; stage count, coefficients and state remain unresolved.

Control labels: `Depth`, `Speed`, `Phase`, `Feedb.`, `Return`, `Dry`.

C: `BEffectPhaser`; vtable RVA `0x4f13fa8`, candidate slot 23 `0x140aa3830` (C line 3853057, completed_with_warnings). Complete effect unverified. [NI interface](https://docs.native-instruments.com/ni-tech-manuals/kontakt-manual/en/effect-reference#legacy-phaser).


## Effects — Mangling

### Beat Masher

Captured audio reordering/repetition with length, gate, rotate, reverse and wrap; needs a tempo-aware audio buffer and playback state.

Control labels: `Mash`, `Length`, `Gate`, `Rotate`, `Mix`, `Output`, `Reverse`, `Wrap`.

C: `BEffectBeatMasher`; vtable RVA `0x4e92860`, candidate slot 23 `0x1408fd2e0` (C line 3588977, completed_with_warnings). Complete effect unverified. [NI interface](https://docs.native-instruments.com/ni-tech-manuals/kontakt-manual/en/effect-reference#beat-masher-730782).

### Beat Slicer

Buffer slicing/resequencing with style, pattern, gate and buzz; preserve capture boundaries, slice selection and transport timing.

Control labels: `Slice`, `Style`, `Gate`, `Pattern`, `Two Bars`, `Buzz`, `Mix`, `Output`.

C: `BEffectBeatSlicer`; vtable RVA `0x4e92b18`, candidate slot 23 `0x1408fd7b0` (C line 3589281, completed_with_warnings). Complete effect unverified. [NI interface](https://docs.native-instruments.com/ni-tech-manuals/kontakt-manual/en/effect-reference#beat-slicer-730783).

### Gater

Tempo-aware amplitude patterning with shape, stutter and noise; preserve timing, phase and generated-noise state.

Control labels: `Gate`, `Rate`, `Tempo Sync`, `Noise`, `Mute Input`, `Shape`, `Stutter`, `Mix`, `Output`.

C: `BEffectGater`; vtable RVA `0x4f16470`, candidate slot 23 `0x1409003c0` (C line 3599931, completed_with_warnings). Complete effect unverified. [NI interface](https://docs.native-instruments.com/ni-tech-manuals/kontakt-manual/en/effect-reference#gater-730784).

### Reverse Grain

Grain playback with forward/reverse/inversion, pitch and speed; grain boundaries, windows, buffer history and overlap are distinct requirements.

Control labels: `Rev`, `Fwd`, `Invert Grains`, `Pitch`, `Grain`, `Speed`, `Mix`, `Output`.

C: `BEffectReverseGrain`; vtable RVA `0x4e93b88`, candidate slot 23 `0x140901fb0` (C line 3605627, completed_with_warnings). Complete effect unverified. [NI interface](https://docs.native-instruments.com/ni-tech-manuals/kontakt-manual/en/effect-reference#reverse-grain-730785).

### Transpose Stretch

Buffer/grain transformation with key, transposition and stretch; needs independent playback pitch, grain scheduling and buffer length.

Control labels: `Stretch`, `Grain`, `Two Bars`, `Key`, `Amount`, `Mix`, `Output`.

C: `BEffectTransposeStretch`; vtable RVA `0x4e94060`, candidate slot 23 `0x140903c90` (C line 3610593, completed_with_warnings). Complete effect unverified. [NI interface](https://docs.native-instruments.com/ni-tech-manuals/kontakt-manual/en/effect-reference#transpose-stretch-730786).


## Effects — Delays

### PsycheDelay

Delay with reverse playback, pitch/detune, stereo offset and cross-feedback; the feedback signal can undergo pitch/time transformations.

Control labels: `Reverse`, `Stereo Reverse`, `Stereo Detune`, `Time`, `LR Offset`, `Feedback`, `Cross FB`, `Pitch`, `Detune`, `Dry`.

C: `BEffectPsycheDelay`; vtable RVA `0x4fc2298`, candidate slot 23 `0x140aa3c30` (C line 3852892, completed). Complete effect unverified. [NI interface](https://docs.native-instruments.com/ni-tech-manuals/kontakt-manual/en/effect-reference#psychedelay).

### Replika Delay

Mode-dependent delay with pattern, shuffle, modulation, stereo routing, filtering and ducking; preserve each mode and its internal state.

Control labels: `Mode`, `Time`, `Feedback`, `Low Cut`, `High Cut`, `Ping Pong`, `Stereo`, `Depth`, `Rate`, `Pattern`, `Shuffle`, `Feel`, `Accent`, `Panning`, `Pan`, `Width`, `LR Offset`, `Ducking`, `Sensitivity`, `Release`, `Amount`, `Saturation`, `BBD type`, `Noise`, `Tape Age`, `Flutter`, `Quality`, `Size`, `Dense`, `Modulation`.

C: `BEffectReplikaDelay`; vtable RVA `0x4e963f0`, candidate slot 23 `0x1408f98d0` (C line 3583586, completed_with_warnings). Complete effect unverified. [NI interface](https://docs.native-instruments.com/ni-tech-manuals/kontakt-manual/en/effect-reference#replika-delay).

### Twin Delay

Two delay paths with separate predelay/time/feedback/level and cross-feedback; the two-channel feedback graph matters.

Control labels: `Predly L/R`, `Time L/R`, `Feed L/R`, `Level L/R`, `Cross FB`, `Width`.

C: `BEffectTwinDelay`; vtable RVA `0x4fc2aa8`, candidate slot 23 `0x140aa73d0` (C line 3855337, completed). Complete effect unverified. [NI interface](https://docs.native-instruments.com/ni-tech-manuals/kontakt-manual/en/effect-reference#twin-delay).

### Legacy Delay

Feedback delay with damping and panning; delay interpolation, tempo mapping and block history remain unresolved.

Control labels: `Time`, `Damping`, `Pan`, `Feedb.`, `Return`, `Dry`.

C: `BEffectDelay`; vtable RVA `0x4e93660`, candidate slot 23 `0x140aa1130` (C line 3849057, completed_with_warnings). Complete effect unverified. [NI interface](https://docs.native-instruments.com/ni-tech-manuals/kontakt-manual/en/effect-reference#legacy-delay).


## Effects — Reverbs

### Convolution

Impulse-response filtering with IR envelope, early/late shaping, resampling, reversal and latency controls; partitioning and exact IR preparation remain unresolved.

Control labels: `Impulse Window`, `Pre Dly.`, `Return`, `Dry`, `Latency`, `S.Rate`, `Reverse`, `Auto Gain`, `Volume`, `Early`, `IR Size`, `HighPass`, `LowPass`.

C: `BEffectIRC`; vtable RVA `0x4fc0180`, candidate slot 23 `0x140aa2bf0` (C line 3852212, completed_with_warnings). Complete effect unverified. [NI interface](https://docs.native-instruments.com/ni-tech-manuals/kontakt-manual/en/effect-reference#convolution).

### Plate Reverb

Reverberant network with predelay, decay, low shelf, high damping and stereo control; delay lengths and diffusion topology remain unresolved.

Control labels: `Pre Delay`, `Decay`, `Low Shelf`, `High Damp`, `Stereo`.

C: `BEffectPlateReverb`; vtable RVA `0x4f15070`, candidate slot 23 `0x1408fbf60` (C line 3587442, completed_with_warnings). Complete effect unverified. [NI interface](https://docs.native-instruments.com/ni-tech-manuals/kontakt-manual/en/effect-reference#plate-reverb).

### Raum

Mode-dependent reverberation with predelay feedback, diffusion, modulation and freeze; freeze/history and mode switching are separate contracts.

Control labels: `Mode`, `Predelay`, `Feedback`, `Low Shelf`, `High Cut`, `Freeze`, `Reverb`, `Diffusion`, `Size`, `Sparse`, `Decay`, `Damping`, `Mod`.

C: `BEffectRaum`; vtable RVA `0x4e93878`, candidate slot 23 `0x1409019a0` (C line 3604720, completed_with_warnings). Complete effect unverified. [NI interface](https://docs.native-instruments.com/ni-tech-manuals/kontakt-manual/en/effect-reference#raum-634917).

### Reverb

Mode-dependent space response with diffusion, modulation, frequency shaping and stereo controls; the GaloisReverb association is a candidate, without recovered network coefficients.

Control labels: `Mode`, `Pre-Delay`, `Size`, `Time`, `Damping`, `Diffusion`, `Modulation`, `Low Shelf`, `High Cut`, `Stereo`.

C: `BEffectGaloisReverb`; vtable RVA `0x4e960b8`, candidate slot 23 `0x1408fc160` (C line 3587646, completed_with_warnings). Complete effect unverified. [NI interface](https://docs.native-instruments.com/ni-tech-manuals/kontakt-manual/en/effect-reference#reverb-528046).

### Legacy Reverb

Reverberation with size/color/damping/stereo controls; preserve a distinct legacy state and response.

Control labels: `Pre Dly.`, `Size`, `Colour`, `Damping`, `Stereo`, `Return`, `Dry`.

C: `BEffectReverb`; vtable RVA `0x4f144e8`, candidate slot 23 `0x140aa3d10` (C line 3854427, completed_with_warnings). Complete effect unverified. [NI interface](https://docs.native-instruments.com/ni-tech-manuals/kontakt-manual/en/effect-reference#legacy-reverb).


## Effects — Spatial

### Stereo Modeller

Stereo-image transformation with pseudo-stereo, spread and pan; channel mixing and any decorrelation history require confirmation.

Control labels: `Pseudo Stereo`, `Spread`, `Pan`, `Output`.

C: `BEffectStereo`; vtable RVA `0x4e961d0`, candidate slot 23 `0x140aa5ac0` (C line 3857691, completed_with_warnings). Complete effect unverified. [NI interface](https://docs.native-instruments.com/ni-tech-manuals/kontakt-manual/en/effect-reference#stereo-modeller).

### Stereo Tune

Stereo pitch variation with spread, drift and split; retain the two pitch paths and drift state.

Control labels: `Spread`, `Drift`, `Split`, `Mix`, `Output`.

C: `BEffectStereoTune`; vtable RVA `0x4fc2868`, candidate slot 23 `0x140aa7320` (C line 3854762, completed). Complete effect unverified. [NI interface](https://docs.native-instruments.com/ni-tech-manuals/kontakt-manual/en/effect-reference#stereo-tune).

### Surround Panner

Multichannel gain/routing from position, angle, distance, divergence, LFE and format; preserve speaker layout and automation.

Control labels: `Output`, `Divergence`, `Size`, `LFE`, `X Shift`, `Y Shift`, `Angle`, `Distance`, `Meter`, `Surround Format`, `Mouse Mode`, `Mono Mix`, `Sync`, `Center Mirror`, `X Mirror`, `Y Mirror`, `XY Mirror`, `Individual`, `Algorithm`, `Constant Power`, `Sinusoid`, `Logarithmic`, `Air Absorption`, `Delay`, `Doppler Effect`, `Options`, `Circular motion.`, `Random motion.`, `Fly-by paths.`.

C: `BEffectSurroundPanner`; vtable RVA `0x4e94e68`, candidate slot 23 `0x140905fa0` (C line 3612230, completed). Complete effect unverified. [NI interface](https://docs.native-instruments.com/ni-tech-manuals/kontakt-manual/en/effect-reference#surround-panner).


## Effects — Utilities

### AET Filter

Sample-derived spectral morphing with morph layers/maps and analysis data. Native BDyxMorphGroup/BDyxMorphMap names exist; their exact audio/filter connection is unresolved.

Control labels: `Morph Layer`, `Morph Map`, `Auto add AET velocity morph`, `Add`, `articulation morph`.

L: native effect/kernel association unresolved. [NI interface](https://docs.native-instruments.com/ni-tech-manuals/kontakt-manual/en/effect-reference#aet-filter).

### Gainer

Smoothed gain: output uses the current gain before a float32 one-pole target update. Channel processing shares the starting state; persistent state advances once per frame.

Control labels: `Gain`.

C: `BEffectGainer`; vtable RVA `0x4f140b0`, candidate slot 23 `0x140aa2980` (C line 3850071, completed_with_warnings). Complete effect unverified. **K (partial):** see the named check in the [verified-kernel index](DSP_EFFECT_CATALOGUE.md#verified-processing-details). [NI interface](https://docs.native-instruments.com/ni-tech-manuals/kontakt-manual/en/effect-reference#gainer).

### Inverter

Polarity inversion and left/right exchange; per-channel signs and routing must be distinguished.

Control labels: `Phase`, `Pan L <> R`.

C: `BEffectInverter`; vtable RVA `0x4f14800`, candidate slot 23 `0x140904830` (C line 3610857, completed). Complete effect unverified. [NI interface](https://docs.native-instruments.com/ni-tech-manuals/kontakt-manual/en/effect-reference#inverter).

### Send Levels

Level controls for the existing instrument send effects; preserve send taps, dynamic destination identity and processing scope.

Control labels: `Levels`, `Level meters`.

C: `BEffectSends`; vtable RVA `0x4e97ed8`, candidate slot 23 `0x1404f99a0` (C line 2576223, completed). Complete effect unverified. [NI interface](https://docs.native-instruments.com/ni-tech-manuals/kontakt-manual/en/effect-reference#send-levels).


## Filters — Lowpass filters

### SV LP1

State-variable LP1 response; retain response selection, resonance and integrator history. Native coefficient/response mapping is unverified.

C: `BFilterSV1`; vtable RVA `0x4f102c0`, candidate slot 23 `0x1408fb190` (C line 3586099, completed_with_warnings). Complete effect unverified. [NI interface](https://docs.native-instruments.com/ni-tech-manuals/kontakt-manual/en/filter-reference#sv-lp1).

### SV LP2

State-variable LP2 response; retain response selection, resonance and integrator history. Native coefficient/response mapping is unverified.

C: `BFilterSV2`; vtable RVA `0x4f10650`, candidate slot 23 `0x1408fb400` (C line 3586235, completed_with_warnings). Complete effect unverified. [NI interface](https://docs.native-instruments.com/ni-tech-manuals/kontakt-manual/en/filter-reference#sv-lp2).

### SV LP4

State-variable LP4 response; retain response selection, resonance and integrator history. Native coefficient/response mapping is unverified.

C: `BFilterSV4`; vtable RVA `0x4f109e0`, candidate slot 23 `0x1408fb680` (C line 3586600, completed_with_warnings). Complete effect unverified. [NI interface](https://docs.native-instruments.com/ni-tech-manuals/kontakt-manual/en/filter-reference#sv-lp4).

### SV LP6

State-variable LP6 response; retain response selection, resonance and integrator history. Native coefficient/response mapping is unverified.

C: `BFilterSV6`; vtable RVA `0x4f10d70`, candidate slot 23 `0x1408fb8f0` (C line 3586830, completed_with_warnings). Complete effect unverified. [NI interface](https://docs.native-instruments.com/ni-tech-manuals/kontakt-manual/en/filter-reference#sv-lp6).

### Ladder LP1

Ladder LP1 response; preserve pole/output selection and nonlinear feedback. Internal selector 22..33→0..11 is checked; public/saved selector mapping and full audio are not.

Control labels: `High`, `Gain`.

C: `BFilterLadder`; vtable RVA `0x4f0ff30`, candidate slot 23 `0x1408faa30` (C line 3585222, completed_with_warnings). Complete effect unverified. **K (partial):** see the named check in the [verified-kernel index](DSP_EFFECT_CATALOGUE.md#verified-processing-details). [NI interface](https://docs.native-instruments.com/ni-tech-manuals/kontakt-manual/en/filter-reference#ladder-lp1).

### Ladder LP2

Ladder LP2 response; preserve pole/output selection and nonlinear feedback. Internal selector 22..33→0..11 is checked; public/saved selector mapping and full audio are not.

Control labels: `High`, `Gain`.

C: `BFilterLadder`; vtable RVA `0x4f0ff30`, candidate slot 23 `0x1408faa30` (C line 3585222, completed_with_warnings). Complete effect unverified. **K (partial):** see the named check in the [verified-kernel index](DSP_EFFECT_CATALOGUE.md#verified-processing-details). [NI interface](https://docs.native-instruments.com/ni-tech-manuals/kontakt-manual/en/filter-reference#ladder-lp2).

### Ladder LP3

Ladder LP3 response; preserve pole/output selection and nonlinear feedback. Internal selector 22..33→0..11 is checked; public/saved selector mapping and full audio are not.

Control labels: `High`, `Gain`.

C: `BFilterLadder`; vtable RVA `0x4f0ff30`, candidate slot 23 `0x1408faa30` (C line 3585222, completed_with_warnings). Complete effect unverified. **K (partial):** see the named check in the [verified-kernel index](DSP_EFFECT_CATALOGUE.md#verified-processing-details). [NI interface](https://docs.native-instruments.com/ni-tech-manuals/kontakt-manual/en/filter-reference#ladder-lp3).

### Ladder LP4

Ladder LP4 response; preserve pole/output selection and nonlinear feedback. Internal selector 22..33→0..11 is checked; public/saved selector mapping and full audio are not.

Control labels: `High`, `Gain`.

C: `BFilterLadder`; vtable RVA `0x4f0ff30`, candidate slot 23 `0x1408faa30` (C line 3585222, completed_with_warnings). Complete effect unverified. **K (partial):** see the named check in the [verified-kernel index](DSP_EFFECT_CATALOGUE.md#verified-processing-details). [NI interface](https://docs.native-instruments.com/ni-tech-manuals/kontakt-manual/en/filter-reference#ladder-lp4).

### Monark

Model-specific low-pass with gain, feedback and FM; nonlinear feedback and modulated cutoff are separate requirements.

Control labels: `Cutoff`, `Reso`, `Gain`, `Feedback`, `FM`.

C: `BFilterBlueMonark`; vtable RVA `0x4f12c30`, candidate slot 23 `0x1408f9ad0` (C line 3583828, completed_with_warnings). Complete effect unverified. [NI interface](https://docs.native-instruments.com/ni-tech-manuals/kontakt-manual/en/filter-reference#monark).

### AR LP2

Model-specific LP2 response with resonance; exact native type, stage count, coefficient laws and nonlinearity remain unresolved.

L: native effect/kernel association unresolved. [NI interface](https://docs.native-instruments.com/ni-tech-manuals/kontakt-manual/en/filter-reference#ar-lp2).

### AR LP4

Model-specific LP4 response with resonance; exact native type, stage count, coefficient laws and nonlinearity remain unresolved.

L: native effect/kernel association unresolved. [NI interface](https://docs.native-instruments.com/ni-tech-manuals/kontakt-manual/en/filter-reference#ar-lp4).

### AR LP2/4

Model-specific LP2/4 response with resonance; exact native type, stage count, coefficient laws and nonlinearity remain unresolved.

L: native effect/kernel association unresolved. [NI interface](https://docs.native-instruments.com/ni-tech-manuals/kontakt-manual/en/filter-reference#ar-lp2-4).

### Daft

Low-pass Daft mode; verified nonlinear parameter laws and 32-sample control scheduling, with the complete audio kernel unresolved.

C: `BFilterDaft`; vtable RVA `0x4f11490`, candidate slot 23 `0x1408fa270` (C line 3584437, completed_with_warnings). Complete effect unverified. **K (partial):** see the named check in the [verified-kernel index](DSP_EFFECT_CATALOGUE.md#verified-processing-details). [NI interface](https://docs.native-instruments.com/ni-tech-manuals/kontakt-manual/en/filter-reference#daft).

### PRO-53

Legacy modeled low-pass family; the Pro52 type-name association is a candidate, not a verified public-ID mapping.

C: `BFilterPro52`; vtable RVA `0x4f0fbd0`, candidate slot 23 `0x140ae9f90` (C line 3940926, completed_with_warnings). Complete effect unverified. [NI interface](https://docs.native-instruments.com/ni-tech-manuals/kontakt-manual/en/filter-reference#pro-53).

### Legacy LP1

Legacy filter response; preserve its own coefficient conversion, resonance and history instead of substituting the newer family.

C: `BFilter1PoleLP`; vtable RVA `0x4e95210`, candidate slot 23 `0x140ae9f90` (C line 3940926, completed_with_warnings). Complete effect unverified. [NI interface](https://docs.native-instruments.com/ni-tech-manuals/kontakt-manual/en/filter-reference#legacy-lp1).

### Legacy LP2

Legacy filter response; preserve its own coefficient conversion, resonance and history instead of substituting the newer family.

C: `BFilter2PoleLP`; vtable RVA `0x4e95870`, candidate slot 23 `0x140ae9f90` (C line 3940926, completed_with_warnings). Complete effect unverified. [NI interface](https://docs.native-instruments.com/ni-tech-manuals/kontakt-manual/en/filter-reference#legacy-lp2).

### Legacy LP4

Legacy filter response; preserve its own coefficient conversion, resonance and history instead of substituting the newer family.

C: `BFilter4PoleLP`; vtable RVA `0x4e94528`, candidate slot 23 `0x140ae9f90` (C line 3940926, completed_with_warnings). Complete effect unverified. [NI interface](https://docs.native-instruments.com/ni-tech-manuals/kontakt-manual/en/filter-reference#legacy-lp4).

### Legacy LP6

Legacy filter response; preserve its own coefficient conversion, resonance and history instead of substituting the newer family.

C: `BFilter6PoleLP`; vtable RVA `0x4f0f8a0`, candidate slot 23 `0x140ae9f90` (C line 3940926, completed_with_warnings). Complete effect unverified. [NI interface](https://docs.native-instruments.com/ni-tech-manuals/kontakt-manual/en/filter-reference#legacy-lp6).

### Legacy Ladder

Legacy filter response; preserve its own coefficient conversion, resonance and history instead of substituting the newer family.

C: `BFilterLadder_legacy`; vtable RVA `0x4f12df8`, candidate slot 23 `0x140ae9f90` (C line 3940926, completed_with_warnings). Complete effect unverified. [NI interface](https://docs.native-instruments.com/ni-tech-manuals/kontakt-manual/en/filter-reference#legacy-ladder).


## Filters — Highpass filters

### SV HP1

State-variable HP1 response; retain response selection, resonance and integrator history. Native coefficient/response mapping is unverified.

C: `BFilterSV1`; vtable RVA `0x4f102c0`, candidate slot 23 `0x1408fb190` (C line 3586099, completed_with_warnings). Complete effect unverified. [NI interface](https://docs.native-instruments.com/ni-tech-manuals/kontakt-manual/en/filter-reference#sv-hp1).

### SV HP2

State-variable HP2 response; retain response selection, resonance and integrator history. Native coefficient/response mapping is unverified.

C: `BFilterSV2`; vtable RVA `0x4f10650`, candidate slot 23 `0x1408fb400` (C line 3586235, completed_with_warnings). Complete effect unverified. [NI interface](https://docs.native-instruments.com/ni-tech-manuals/kontakt-manual/en/filter-reference#sv-hp2).

### SV HP4

State-variable HP4 response; retain response selection, resonance and integrator history. Native coefficient/response mapping is unverified.

C: `BFilterSV4`; vtable RVA `0x4f109e0`, candidate slot 23 `0x1408fb680` (C line 3586600, completed_with_warnings). Complete effect unverified. [NI interface](https://docs.native-instruments.com/ni-tech-manuals/kontakt-manual/en/filter-reference#sv-hp4).

### SV HP6

State-variable HP6 response; retain response selection, resonance and integrator history. Native coefficient/response mapping is unverified.

C: `BFilterSV6`; vtable RVA `0x4f10d70`, candidate slot 23 `0x1408fb8f0` (C line 3586830, completed_with_warnings). Complete effect unverified. [NI interface](https://docs.native-instruments.com/ni-tech-manuals/kontakt-manual/en/filter-reference#sv-hp6).

### Ladder HP1

Ladder HP1 response; preserve pole/output selection and nonlinear feedback. Internal selector 22..33→0..11 is checked; public/saved selector mapping and full audio are not.

Control labels: `High`, `Gain`.

C: `BFilterLadder`; vtable RVA `0x4f0ff30`, candidate slot 23 `0x1408faa30` (C line 3585222, completed_with_warnings). Complete effect unverified. **K (partial):** see the named check in the [verified-kernel index](DSP_EFFECT_CATALOGUE.md#verified-processing-details). [NI interface](https://docs.native-instruments.com/ni-tech-manuals/kontakt-manual/en/filter-reference#ladder-hp1).

### Ladder HP2

Ladder HP2 response; preserve pole/output selection and nonlinear feedback. Internal selector 22..33→0..11 is checked; public/saved selector mapping and full audio are not.

Control labels: `High`, `Gain`.

C: `BFilterLadder`; vtable RVA `0x4f0ff30`, candidate slot 23 `0x1408faa30` (C line 3585222, completed_with_warnings). Complete effect unverified. **K (partial):** see the named check in the [verified-kernel index](DSP_EFFECT_CATALOGUE.md#verified-processing-details). [NI interface](https://docs.native-instruments.com/ni-tech-manuals/kontakt-manual/en/filter-reference#ladder-hp2).

### Ladder HP3

Ladder HP3 response; preserve pole/output selection and nonlinear feedback. Internal selector 22..33→0..11 is checked; public/saved selector mapping and full audio are not.

Control labels: `High`, `Gain`.

C: `BFilterLadder`; vtable RVA `0x4f0ff30`, candidate slot 23 `0x1408faa30` (C line 3585222, completed_with_warnings). Complete effect unverified. **K (partial):** see the named check in the [verified-kernel index](DSP_EFFECT_CATALOGUE.md#verified-processing-details). [NI interface](https://docs.native-instruments.com/ni-tech-manuals/kontakt-manual/en/filter-reference#ladder-hp3).

### Ladder HP4

Ladder HP4 response; preserve pole/output selection and nonlinear feedback. Internal selector 22..33→0..11 is checked; public/saved selector mapping and full audio are not.

Control labels: `High`, `Gain`.

C: `BFilterLadder`; vtable RVA `0x4f0ff30`, candidate slot 23 `0x1408faa30` (C line 3585222, completed_with_warnings). Complete effect unverified. **K (partial):** see the named check in the [verified-kernel index](DSP_EFFECT_CATALOGUE.md#verified-processing-details). [NI interface](https://docs.native-instruments.com/ni-tech-manuals/kontakt-manual/en/filter-reference#ladder-hp4).

### AR HP2

Model-specific HP2 response with resonance; exact native type, stage count, coefficient laws and nonlinearity remain unresolved.

L: native effect/kernel association unresolved. [NI interface](https://docs.native-instruments.com/ni-tech-manuals/kontakt-manual/en/filter-reference#ar-hp2).

### AR HP4

Model-specific HP4 response with resonance; exact native type, stage count, coefficient laws and nonlinearity remain unresolved.

L: native effect/kernel association unresolved. [NI interface](https://docs.native-instruments.com/ni-tech-manuals/kontakt-manual/en/filter-reference#ar-hp4).

### AR HP2/4

Model-specific HP2/4 response with resonance; exact native type, stage count, coefficient laws and nonlinearity remain unresolved.

L: native effect/kernel association unresolved. [NI interface](https://docs.native-instruments.com/ni-tech-manuals/kontakt-manual/en/filter-reference#ar-hp2-4).

### Daft HP

High-pass Daft mode; internal subtype selection and parameter/scheduling checks do not establish the whole high-pass audio pipeline.

Control labels: `Gain`.

C: `BFilterDaft`; vtable RVA `0x4f11490`, candidate slot 23 `0x1408fa270` (C line 3584437, completed_with_warnings). Complete effect unverified. **K (partial):** see the named check in the [verified-kernel index](DSP_EFFECT_CATALOGUE.md#verified-processing-details). [NI interface](https://docs.native-instruments.com/ni-tech-manuals/kontakt-manual/en/filter-reference#daft-hp).

### Legacy HP1

Legacy filter response; preserve its own coefficient conversion, resonance and history instead of substituting the newer family.

C: `BFilter1PoleHP`; vtable RVA `0x4e953a8`, candidate slot 23 `0x140ae9f90` (C line 3940926, completed_with_warnings). Complete effect unverified. [NI interface](https://docs.native-instruments.com/ni-tech-manuals/kontakt-manual/en/filter-reference#legacy-hp1).

### Legacy HP2

Legacy filter response; preserve its own coefficient conversion, resonance and history instead of substituting the newer family.

C: `BFilter2PoleHP`; vtable RVA `0x4e95a08`, candidate slot 23 `0x140ae9f90` (C line 3940926, completed_with_warnings). Complete effect unverified. [NI interface](https://docs.native-instruments.com/ni-tech-manuals/kontakt-manual/en/filter-reference#legacy-hp2).

### Legacy HP4

Legacy filter response; preserve its own coefficient conversion, resonance and history instead of substituting the newer family.

C: `BFilter4PoleHP`; vtable RVA `0x4e946c0`, candidate slot 23 `0x140ae9f90` (C line 3940926, completed_with_warnings). Complete effect unverified. [NI interface](https://docs.native-instruments.com/ni-tech-manuals/kontakt-manual/en/filter-reference#legacy-hp4).


## Filters — Bandpass

### SV BP2

State-variable BP2 response; retain response selection, resonance and integrator history. Native coefficient/response mapping is unverified.

C: `BFilterSV2`; vtable RVA `0x4f10650`, candidate slot 23 `0x1408fb400` (C line 3586235, completed_with_warnings). Complete effect unverified. [NI interface](https://docs.native-instruments.com/ni-tech-manuals/kontakt-manual/en/filter-reference#sv-bp2).

### SV BP4

State-variable BP4 response; retain response selection, resonance and integrator history. Native coefficient/response mapping is unverified.

C: `BFilterSV4`; vtable RVA `0x4f109e0`, candidate slot 23 `0x1408fb680` (C line 3586600, completed_with_warnings). Complete effect unverified. [NI interface](https://docs.native-instruments.com/ni-tech-manuals/kontakt-manual/en/filter-reference#sv-bp4).

### Ladder BP2

Ladder BP2 response; preserve pole/output selection and nonlinear feedback. Internal selector 22..33→0..11 is checked; public/saved selector mapping and full audio are not.

Control labels: `High`, `Gain`.

C: `BFilterLadder`; vtable RVA `0x4f0ff30`, candidate slot 23 `0x1408faa30` (C line 3585222, completed_with_warnings). Complete effect unverified. **K (partial):** see the named check in the [verified-kernel index](DSP_EFFECT_CATALOGUE.md#verified-processing-details). [NI interface](https://docs.native-instruments.com/ni-tech-manuals/kontakt-manual/en/filter-reference#ladder-bp2).

### Ladder BP4

Ladder BP4 response; preserve pole/output selection and nonlinear feedback. Internal selector 22..33→0..11 is checked; public/saved selector mapping and full audio are not.

Control labels: `High`, `Gain`.

C: `BFilterLadder`; vtable RVA `0x4f0ff30`, candidate slot 23 `0x1408faa30` (C line 3585222, completed_with_warnings). Complete effect unverified. **K (partial):** see the named check in the [verified-kernel index](DSP_EFFECT_CATALOGUE.md#verified-processing-details). [NI interface](https://docs.native-instruments.com/ni-tech-manuals/kontakt-manual/en/filter-reference#ladder-bp4).

### AR BP2

Model-specific BP2 response with resonance; exact native type, stage count, coefficient laws and nonlinearity remain unresolved.

L: native effect/kernel association unresolved. [NI interface](https://docs.native-instruments.com/ni-tech-manuals/kontakt-manual/en/filter-reference#ar-bp2).

### AR BP4

Model-specific BP4 response with resonance; exact native type, stage count, coefficient laws and nonlinearity remain unresolved.

L: native effect/kernel association unresolved. [NI interface](https://docs.native-instruments.com/ni-tech-manuals/kontakt-manual/en/filter-reference#ar-bp4).

### AR BP2/4

Model-specific BP2/4 response with resonance; exact native type, stage count, coefficient laws and nonlinearity remain unresolved.

L: native effect/kernel association unresolved. [NI interface](https://docs.native-instruments.com/ni-tech-manuals/kontakt-manual/en/filter-reference#ar-bp2-4).

### Legacy BP2

Legacy filter response; preserve its own coefficient conversion, resonance and history instead of substituting the newer family.

C: `BFilter2PoleBP`; vtable RVA `0x4e95540`, candidate slot 23 `0x140ae9f90` (C line 3940926, completed_with_warnings). Complete effect unverified. [NI interface](https://docs.native-instruments.com/ni-tech-manuals/kontakt-manual/en/filter-reference#legacy-bp2).

### Legacy BP4

Legacy filter response; preserve its own coefficient conversion, resonance and history instead of substituting the newer family.

C: `BFilter4PoleBP`; vtable RVA `0x4e94858`, candidate slot 23 `0x140ae9f90` (C line 3940926, completed_with_warnings). Complete effect unverified. [NI interface](https://docs.native-instruments.com/ni-tech-manuals/kontakt-manual/en/filter-reference#legacy-bp4).


## Filters — Peak and notch filters

### SV Notch

State-variable Notch response; retain response selection, resonance and integrator history. Native coefficient/response mapping is unverified.

C: `BFilterSV2`; vtable RVA `0x4f10650`, candidate slot 23 `0x1408fb400` (C line 3586235, completed_with_warnings). Complete effect unverified. [NI interface](https://docs.native-instruments.com/ni-tech-manuals/kontakt-manual/en/filter-reference#sv-notch).

### Ladder Peak

Ladder Peak response; preserve pole/output selection and nonlinear feedback. Internal selector 22..33→0..11 is checked; public/saved selector mapping and full audio are not.

Control labels: `High`, `Gain`.

C: `BFilterLadder`; vtable RVA `0x4f0ff30`, candidate slot 23 `0x1408faa30` (C line 3585222, completed_with_warnings). Complete effect unverified. **K (partial):** see the named check in the [verified-kernel index](DSP_EFFECT_CATALOGUE.md#verified-processing-details). [NI interface](https://docs.native-instruments.com/ni-tech-manuals/kontakt-manual/en/filter-reference#ladder-peak).

### Ladder Notch

Ladder Notch response; preserve pole/output selection and nonlinear feedback. Internal selector 22..33→0..11 is checked; public/saved selector mapping and full audio are not.

Control labels: `High`, `Gain`.

C: `BFilterLadder`; vtable RVA `0x4f0ff30`, candidate slot 23 `0x1408faa30` (C line 3585222, completed_with_warnings). Complete effect unverified. **K (partial):** see the named check in the [verified-kernel index](DSP_EFFECT_CATALOGUE.md#verified-processing-details). [NI interface](https://docs.native-instruments.com/ni-tech-manuals/kontakt-manual/en/filter-reference#ladder-notch).

### Legacy BR4

Legacy filter response; preserve its own coefficient conversion, resonance and history instead of substituting the newer family.

C: `BFilter4PoleBR`; vtable RVA `0x4e949f0`, candidate slot 23 `0x140ae9f90` (C line 3940926, completed_with_warnings). Complete effect unverified. [NI interface](https://docs.native-instruments.com/ni-tech-manuals/kontakt-manual/en/filter-reference#legacy-br4).


## Filters — Multi

### SV Par. LP/HP

Parallel low/high state-variable branches; preserve branch tuning, bandwidth and summing.

Control labels: `Cutoff`, `Reso.`, `B.Width`.

C: `BFilterMultiSV`; vtable RVA `0x4f11100`, candidate slot 23 `0x1408faf10` (C line 3585868, completed_with_warnings). Complete effect unverified. [NI interface](https://docs.native-instruments.com/ni-tech-manuals/kontakt-manual/en/filter-reference#sv-par--lp-hp).

### SV Par. BP/BP

Two parallel band-pass branches; preserve independent branch tuning and resonance.

Control labels: `Cutoff`, `Reso.`, `B.Width`.

C: `BFilterMultiSV`; vtable RVA `0x4f11100`, candidate slot 23 `0x1408faf10` (C line 3585868, completed_with_warnings). Complete effect unverified. [NI interface](https://docs.native-instruments.com/ni-tech-manuals/kontakt-manual/en/filter-reference#sv-par--bp-bp).

### SV Ser. LP/HP

Serial low/high state-variable branches; preserve ordering and both histories.

Control labels: `Cutoff`, `Reso.`, `B.Width`.

C: `BFilterMultiSV`; vtable RVA `0x4f11100`, candidate slot 23 `0x1408faf10` (C line 3585868, completed_with_warnings). Complete effect unverified. [NI interface](https://docs.native-instruments.com/ni-tech-manuals/kontakt-manual/en/filter-reference#sv-ser--lp-hp).

### 3x2 Versatile

Three two-pole sections with type, shifted cutoff, resonance and amount; the section graph and gains matter.

Control labels: `Cutoff`, `Shift 2`, `Shift 3`, `Reso.`, `Type 1`, `Amt. 1`, `Gain`.

C: `BFilterVersatile`; vtable RVA `0x4f132c0`, candidate slot 23 `0x140aea690` (C line 3942515, completed_with_warnings). Complete effect unverified. [NI interface](https://docs.native-instruments.com/ni-tech-manuals/kontakt-manual/en/filter-reference#3x2-versatile).

### Dual SKF

Separate high/low Sallen-Key branches with resonance and FM; nonlinear branch states and routing matter.

Control labels: `Cutoff HP`, `Cutoff LP`, `Reso HP`, `Reso LP`, `FM`.

C: `BFilterDualSKF`; vtable RVA `0x4f12808`, candidate slot 23 `0x1408801e0` (C line 3481937, completed_with_warnings). Complete effect unverified. [NI interface](https://docs.native-instruments.com/ni-tech-manuals/kontakt-manual/en/filter-reference#dual-skf).

### Simple LP/HP

Selectable low/high filtering; preserve cutoff conversion and response selection.

Control labels: `Cutoff`.

C: `BFilterSimpleLPHP`; vtable RVA `0x4f124a8`, candidate slot 23 `0x140908ae0` (C line 3612272, completed_with_warnings). Complete effect unverified. [NI interface](https://docs.native-instruments.com/ni-tech-manuals/kontakt-manual/en/filter-reference#simple-lp-hp).


## Filters — Effect filters

### Formant I

Resonant spectral/formant shaping with Talk, Sharp and Size; formant placement/interpolation remains unresolved.

Control labels: `Talk`, `Sharp`, `Size`.

C: `BFilterFormant`; vtable RVA `0x4f11820`, candidate slot 23 `0x1408fa7b0` (C line 3585085, completed_with_warnings). Complete effect unverified. [NI interface](https://docs.native-instruments.com/ni-tech-manuals/kontakt-manual/en/filter-reference#formant-i).

### Formant II

Second formant response with Talk, Sharp and Size; shared class evidence does not prove the same coefficients as Formant I.

Control labels: `Talk`, `Sharp`, `Size`.

C: `BFilterFormant`; vtable RVA `0x4f11820`, candidate slot 23 `0x1408fa7b0` (C line 3585085, completed_with_warnings). Complete effect unverified. [NI interface](https://docs.native-instruments.com/ni-tech-manuals/kontakt-manual/en/filter-reference#formant-ii).

### Phaser

Phase/notch response with cutoff and resonance; stage topology and coefficient laws remain unresolved.

Control labels: `Cutoff`, `Reso.`.

C: `BFilterPhaser`; vtable RVA `0x4f0fa38`, candidate slot 23 `0x140ae9f90` (C line 3940926, completed_with_warnings). Complete effect unverified. [NI interface](https://docs.native-instruments.com/ni-tech-manuals/kontakt-manual/en/filter-reference#phaser).

### Vowel A

Vowel-like resonances controlled by cutoff and resonance; keep a separate coefficient/morph path from Vowel B.

Control labels: `Cutoff`, `Reso.`.

C: `BFilterVowelA`; vtable RVA `0x4f12f90`, candidate slot 23 `0x140ae9f90` (C line 3940926, completed_with_warnings). Complete effect unverified. [NI interface](https://docs.native-instruments.com/ni-tech-manuals/kontakt-manual/en/filter-reference#vowel-a).

### Vowel B

Second vowel-filter response; resonant-band layout and cutoff mapping remain unresolved.

Control labels: `Cutoff`, `Reso.`.

C: `BFilterVowelB`; vtable RVA `0x4f13128`, candidate slot 23 `0x140ae9f90` (C line 3940926, completed_with_warnings). Complete effect unverified. [NI interface](https://docs.native-instruments.com/ni-tech-manuals/kontakt-manual/en/filter-reference#vowel-b).


## Filters — Equalizers

### Solid G-EQ

High/low cuts, switchable shelf/bell bands and two parametric mid bands; preserve band topology and interactions.

Control labels: `HP Freq`, `Highpass`, `LF Gain`, `LF Freq`, `LF Bell`, `LMF`, `LMF Freq`, `LMF Q`, `HMF Gain`, `HMF Freq`, `HMF Q`, `HF Gain`, `HF Freq`, `HF Bell`, `Output`, `LP Freq`, `Lowpass`.

C: `BEffectSSLGEQ`; vtable RVA `0x4f154d0`, candidate slot 23 `0x1408fbd60` (C line 3587241, completed_with_warnings). Complete effect unverified. [NI interface](https://docs.native-instruments.com/ni-tech-manuals/kontakt-manual/en/filter-reference#solid-g-eq).

