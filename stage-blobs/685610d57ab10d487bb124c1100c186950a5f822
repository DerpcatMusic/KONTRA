# Kontakt, Falcon and Workstation DSP / system inventory

Inventory dates: 2026-10-07 through 2026-10-08. Repository inspected: `0cb7a8a0b4d43086596a64c77320caa1b26d6d98`.

Kontakt 8.13.1 Portable is present as Windows x64 binaries under Wine on Linux x86_64. No Falcon executable or plug-in was found in the scanned user, mounted-volume, system plug-in and application locations. An old Wine registry uninstall record points to the absent `C:\Program Files\UVI\Falcon` directory. Twenty-six `.ufs` files totaling 24,255,705,669 bytes were observed under `/mnt/MAIN_STORAGE/Libraries/UVI`, and a cached official Falcon manual is present; these do not establish an installed Falcon engine.

## Evidence and scope

The installed Kontakt executable is 178,386,944 bytes and its portable VST3 module is 2,484,224 bytes. Both expose readable PE metadata and executable sections. The engine contains 230,554 Windows unwind records, 15,110 RTTI-name candidates and 13,220 validated RTTI-linked vtable runs. DSP-name matching selects 1,359 type candidates and 3,880 distinct virtual-method addresses; helper, template and UI types are included, and the name heuristic can omit DSP types. All observed RTTI types and vtable entries are also preserved separately. These counts are static observations, not counts of unique original functions or verified audio algorithms.

The static entry map combines unwind starts, RTTI virtual methods, direct-call destinations, executable exports and the entry point. All candidates are accounted for in the recovery and decompilation ledgers.

| Primary Ghidra result | Kontakt engine | Portable VST3 |
| --- | ---: | ---: |
| Static entry candidates | 255,893 | 9,354 |
| Pseudocode emitted | 255,396 | 9,354 |
| Decompilation failed | 493 | 0 |
| Entries not recovered | 4 | 0 |
| Successful outputs with warnings | 81,513 | 2,704 |

Together, 264,750 primary entries emitted pseudocode. Full executable-section disassembly, imports, exports, strings, type/vtable records and per-entry ledgers are kept in ignored `artifacts/engine-analysis-2026-10-07/`. Automated pseudocode infers names, types and control flow. Warnings, failures, unresolved indirect calls and unrecognized leaf routines prevent a claim of recovered original source or exact sound equivalence.

A longer follow-up recovered 25 additional engine pseudocode outputs, all with warnings. Only complete blocks bounded by a subsequent address marker and backed by a complete ledger row were retained from the interrupted retry jobs. Their [validated supplement](../artifacts/engine-analysis-2026-10-07/deep-dsp/retry-validated/summary.json) brings the combined primary/supplemental count to 264,775 outputs. The remaining 468 primary failures and four unrecovered entries remain explicit; the full failure retry did not finish and the primary coverage table is unchanged.

A separate r2ghidra fallback emitted unvalidated snippets for three of the four unrecovered Ghidra addresses; one remained without a function. This does not establish valid source boundaries or DSP behavior.

The VST3 PE exports `GetPluginFactory`, `InitDll` and `ExitDll`. The executable exports 13 entries, primarily Qt-related hooks, rather than a public DSP API. Its sections include ordinary executable code, `IPPCODE`, data and resources. Imported Windows services cover audio/multimedia, graphics, COM, files, threading, networking and the C/C++ runtime. DSP and source-machine RTTI is present in the executable; a portable plug-in ABI alone does not specify its internal DSP.

Private [analysis index](../artifacts/engine-analysis-2026-10-07/README.md), [documented module/parameter JSON](../artifacts/engine-analysis-2026-10-07/catalog.json), [Kontakt identifier JSON](../artifacts/engine-analysis-2026-10-07/kontakt-ksp-identifiers.json) and [verification result](../artifacts/engine-analysis-2026-10-07/verification.json) are local, ignored artifacts.

## Original machine-code DSP checks

The follow-up uses Unicorn x86-64 to execute selected instructions from the unchanged Kontakt 8.13.1 image in a bounded memory model. Seventy input/block/state cases pass, covering gain processing, gain-state copying, Daft and Ladder subtype selection, Daft parameter laws, control cadence, ramp updates, sample-rate setup and one compressor channel-link path. The Windows application is not launched. The private [runnable check](../artifacts/engine-analysis-2026-10-07/deep-dsp/verify_machine_code.py) and [results](../artifacts/engine-analysis-2026-10-07/deep-dsp/machine-code-verification.json) record the reference hash and helper substitutions.

Mixing, base-object copy, resampler setup and selected core dispatches are stubbed in the tests that isolate those callers. The lookup initializer substitutes Python `math.pow` for the imported Windows CRT function. The compressor channel-link test executes its selected kernel without helper stubs. These checks establish the stated local behavior; they do not establish complete host rendering, preset restoration or audio equivalence.

### Gainer

The processing entry is `0x140aa2980`, associated with `BEffectGainer`. The target gain is float32 at object offset `0x1c0`; persistent current gain is at `0x1c4`. Each sample uses the current state before advancing it:

```text
output[n] = f32(input[n] * state[n])
state[n+1] = f32(state[n] + f32(f32(target - state[n]) * k))
k = 0.0005555555690079927  (float32 bits 0x3a11a2b4)
```

The recurrence uses separate SSE subtraction, multiplication and addition. Fusing operations can change rounding. Each channel begins with the same block-start gain; the persistent gain advances by the frame count once rather than once per channel. Output samples and final states match this recurrence byte for byte in the tested non-unity cases, including stereo and a 1,024-frame block.

When both current and target gains lie within the double-precision comparison bounds `[0.999, 1.001]`, the kernel leaves its output scratch untouched and requests the common mix helper's unity shortcut. The resulting host output depends on that helper, which is stubbed here.

The gain-copy entry `0x1408374b0` always copies the target from the source object. Its local branch copies the target into current state only when source byte `0x1a2` is nonzero and destination byte `0x1b7` is zero. Four flag combinations are verified with the base-copy helper stubbed; the flags' complete lifecycle meaning remains unassigned.

KONTRA currently stores `Dsp::Gain(f32)` and multiplies both channels by that value in `src/fx/processor.rs:910`. This path has no separate persistent gain ramp. Matching the reference requires the initial/current state, update/reset contract and unity mixing behavior as well as the coefficient.

### Daft parameter laws and scheduling

The outer `BFilterDaft` owns a `NI::KONTAKTFX::FilterDaft` core at offset `0x210`. The parameter setter is `0x140b05900`; cutoff processing tail-calls `0x140b04c80`. The table initializer is `0x1404d14d0`.

The initializer fills 2,401 float32 values `T[i] = f32(2^(i/60 - 20))`, with a float32 forward difference beside each value and a zero final difference. The implementation calculates the exponent in double precision using a stored `1/60` constant and calls CRT `pow`. Setters use float32 linear interpolation between adjacent entries; replacing this with a direct exponential changes interpolation and rounding.

| Core parameter index | Observed law |
| --- | --- |
| 0: leading gain | Stores `12*x` dB; amplitude uses table position `f32(f32(x*119.58940887451172)+1200)`, approximately `10^(12*x/20)` |
| 1: cutoff | Uses table position `f32(f32(x*625)+1481.881591796875)`; resulting frequency clamps to `[1, 30000]` Hz |
| 2: resonance | Maps to `f32(1 - f32(f32(1-x)*f32(1-x)))`, retaining the raw input separately |
| 3: response mode | Uses `clamp(trunc(f32(2*x)), 0, 1)` and complementary mode weights; the tested threshold is 0.5 |

The cutoff setter also derives a feedback scale: float32 `1.600000023841858 - min(14000, cutoff/2)*0.00005714285725844093`. Preserve the actual float32 operation order for a faithful implementation.

| Normalized input | Gain dB | Interpolated amplitude | Cutoff Hz |
| ---: | ---: | ---: | ---: |
| 0 | 0 | 1 | 25.956726 |
| 0.25 | 3 | 1.412546 | 157.827438 |
| 0.5 | 6 | 1.995283 | 959.662048 |
| 0.75 | 9 | 2.818422 | 5835.129395 |
| 1 | 12 | 3.981133 | 30000 |

These values come from original setter/initializer instructions with the substituted `pow` helper. They do not certify the Windows CRT's last-bit results. Signed leading gain is observable: input `-0.25` stores `-3` dB and produces approximately `0.707950` amplitude. The low-level setter accepts wider values; the outer modulation path clamps incoming control values to `[0, 1]`.

Outer processing entry `0x1408fa270` maintains a countdown at offset `0x1bc`, splits audio at 32-frame boundaries, and carries the countdown across host blocks. At each boundary it consumes one value from each enabled control stream, clamps it, dispatches parameter indices 0–3, and calls the core update. The stream index advances by one float per control event, independently of audio-frame offsets. Twelve cases verify calls, split lengths, channel pointers and countdowns across zero/odd/large blocks, with the core DSP and common mixing helpers stubbed.

Core setup `0x140afd210` configures paired 2:1 and 1:2 resamplers and stores the doubled processing rate. Outer setup `0x140970630` supplies a 32-frame quantum. Ramp length is quantized from an approximately 0.002-second duration; core update `0x140b03620` prepares four float32 deltas and snaps components whose squared delta is below the double constant `1e-15`.

| Input sample rate | Ramp countdown at quantum 32 | Input frames per ramp | Delta multiplier per doubled-rate sample |
| ---: | ---: | ---: | ---: |
| 8000 | 1 | 32 | 1/64 |
| 44100 | 3 | 96 | 1/192 |
| 48000 | 3 | 96 | 1/192 |
| 96000 | 6 | 192 | 1/384 |
| 192000 | 12 | 384 | 1/768 |

Eight setup cases verify the fields and resampler call arguments with setup helpers stubbed. Three ramp-state cases verify change admission, delta calculation, small-change snapping, countdown completion and phase transitions. The complete doubled-rate audio kernel, resampler coefficients, nonlinear feedback and reset behavior remain outside the emulated audio comparisons. The recovered core processing entry is `0x140affa30`.

KONTRA's current Daft dispatch at `src/engine/filter.rs:68` selects a linear state-variable proxy for stored IDs 70/71. These parameter laws, control scheduling and nonlinear doubled-rate path are further compatibility work; retaining a saved field does not reproduce its signal behavior.

### Subtype selection and compressor linking

Daft wrapper `0x140af0170` maps internal input 68 to mode 0 and 69 to mode 1; the tested other values fall back to mode 0. Ladder wrapper `0x140af01d0` maps internal inputs 22–33 to modes 0–11 and otherwise falls back to mode 0. Fourteen boundary/signed input cases exercise both wrappers. These are internal selectors: their numeric inputs are not proven to be NKI serialized IDs, so they do not justify changing KONTRA's stored IDs.

Compressor dispatcher `0x1409047a0` selects `0x140ab2250`, `0x140ab2c80` or `0x140ab35d0` for local variant values 0, 1 or 2. The verified variant-0 linking branch, enabled by byte `0x1c8`, constructs a signed arithmetic channel mean before absolute-value detection. Five cases verify the scratch values for stereo, three channels, odd and 1,024-frame blocks, and disabled linking. Equal opposite-polarity stereo inputs produce a zero linked detector signal. The complete threshold, envelope and gain law and the public meaning of the variant enumeration are not certified by this property check.

## Machine and binary identity

The host is Linux x86_64 with 16 logical CPUs; the reference binaries are Windows x64 PE images in a Wine prefix. This study reads files and metadata without launching either audio engine.

| Image | Version resource | SHA-256 |
| --- | --- | --- |
| Kontakt executable | 8.13.1 | `0fe6356e0879d058b6e5b73507c54c5e345cea451b35287c974e438291d4dae8` |
| Kontakt Portable VST3 | 8.13.1.0 | `799120e1611318b60d2ef267cb21ad2df2753012c262c3ca1fc734ba25d40b46` |
| Cached UVI Workstation executable | 4.0.9 | `78729e96b752aea746280275072ad24cb4399a053739c49a161ff1fcfbf85721` |

The executable and VST3 module have separate images, import surfaces and function maps. Their exact loading/communication relationship was not established. The VST3 ABI describes hosting entry points; it does not establish preset, library, script or DSP compatibility. Older backup/installer copies were discovered but are outside this run's selected reference version. Dependency DLLs and resources were not independently decompiled.

A follow-up cache/download/mounted-volume scan found a 64,649,552-byte UVI Workstation image, not Falcon, at `/home/derpcat/.codex/cache/kontakto-uvi-official-reader/app/UVIWorkstationx64.exe`. Its version resources identify UVI Workstation 4.0.9. Its PKCS7 certificate data includes a UVI subject; complete Authenticode integrity was not verified. Static collection records 79,599 unwind ranges and 385 RTTI-name candidates, but the current MSVC collector finds no validated RTTI-linked vtables or DSP-name candidates. This collector limitation is not evidence that the image has no DSP. [Private Workstation metadata](../artifacts/engine-analysis-2026-10-07/uvi-workstation/pe.json). Its algorithms are not assumed identical to Falcon's.

The Workstation follow-up analyzes a hash-pinned retained loaded-code snapshot. Across the initial map, static-pointer supplement and successful factory retry, 88,170 distinct candidate addresses emit pseudocode; 83 initial failures remain. All 176 direct registration calls in the main factory are indexed. The embedded ZIP exposes 408 effect/script presets and 97 arpeggiator presets, with 56 tags and 1,357 tag/attribute pairs. Original-byte LFO execution passes 204 additional isolated checks with authored custom tables and no helper substitutions. OnePole passes another 490 setter/sample-block checks, verifying scalar and stereo SIMD PCM/history with authored coefficients; cutoff conversion remains incomplete. See the [format/DSP specification](DSP_FORMAT_SPECIFICATION.md) for exact coverage, byte layouts, XML syntax, all observed field spellings, provenance and unresolved algorithms.

## Kontakt source engines and signal flow

Documented source modes: Sampler, DFD, Wavetable, Tone Machine, Time Machine, Time Machine 2, Time Machine Pro, Beat Machine, S1200 Machine and MP60 Machine. Preserve source mode, key/root/tuning and interpolation settings, sample/loop boundaries, reverse flags, stretching/formant controls, streaming state and voice lifecycle independently. [NI source-module reference](https://docs.native-instruments.com/ni-tech-manuals/kontakt-manual/en/classic-view).

Audio runs from zone/source voices through group inserts and the amplifier, optional post-amplifier group stages, instrument buses and inserts, send returns, main effects and output/auxiliary routing. Group, bus, instrument and main chains provide eight slots; insert order, processing scope and send-tap placement affect the result. A bus or send processing a summed signal differs from an effect instanced per voice. [NI routing reference](https://docs.native-instruments.com/ni-tech-manuals/kontakt-manual/en/using-filters-and-effects-in-classic-view).

Documented modulation covers AHDSR, DBD and flexible envelopes; LFO and Multi Digital sources; step modulation, envelope following, glide and external MIDI/performance sources. The local binary additionally exposes named LFO, envelope, voice, source-machine, convolution and filter classes in the private RTTI inventory. The route requires target identity, depth/polarity, shaping, smoothing, retrigger/phase, timing and bypass behavior. [NI modulation reference](https://docs.native-instruments.com/ni-tech-manuals/kontakt-manual/en/modulation).

## Kontakt filters — 59 documented entries

Catalog identifiers from the [NI reference](https://docs.native-instruments.com/ni-tech-manuals/kontakt-manual/en/filter-reference), retrieved 2026-10-07. Listing an identifier does not verify its presence, availability or implementation in every installed version.

| Category | Modules |
| --- | --- |
| Lowpass filters | SV LP1, SV LP2, SV LP4, SV LP6, Ladder LP1, Ladder LP2, Ladder LP3, Ladder LP4, Monark, AR LP2, AR LP4, AR LP2/4, Daft, PRO-53, Legacy LP1, Legacy LP2, Legacy LP4, Legacy LP6, Legacy Ladder |
| Highpass filters | SV HP1, SV HP2, SV HP4, SV HP6, Ladder HP1, Ladder HP2, Ladder HP3, Ladder HP4, AR HP2, AR HP4, AR HP2/4, Daft HP, Legacy HP1, Legacy HP2, Legacy HP4 |
| Bandpass | SV BP2, SV BP4, Ladder BP2, Ladder BP4, AR BP2, AR BP4, AR BP2/4, Legacy BP2, Legacy BP4 |
| Peak and notch filters | SV Notch, Ladder Peak, Ladder Notch, Legacy BR4 |
| Multi | SV Par. LP/HP, SV Par. BP/BP, SV Ser. LP/HP, 3x2 Versatile, Dual SKF, Simple LP/HP |
| Effect filters | Formant I, Formant II, Phaser, Vowel A, Vowel B |
| Equalizers | Solid G-EQ |

## Kontakt effects — 65 documented entries

Catalog identifiers from the [NI reference](https://docs.native-instruments.com/ni-tech-manuals/kontakt-manual/en/effect-reference), retrieved 2026-10-07. Listing an identifier does not verify its presence, availability or implementation in every installed version.

| Category | Modules |
| --- | --- |
| Dynamics | Compressor, Feedback Compressor, Limiter, Solid Bus Comp, Supercharger GT, Transient Master, Transparent Limiter |
| Amplifiers | ACBox, Bass Invader, Bass Pro, Cabinet, EP Preamps, HotSolo, Jump, Reverb Delight, Super Fast 100, Twang, Van51 |
| Stomps | Big Fuzz, Cat, Chainsaw, Cry Wah, Dirt, Distortion, DStortion, Fuzz, Kolor, Saturator, Skreamer, Skreamer Deluxe |
| Lo-Fi | Bite, Lo-Fi |
| Tape | Tape Saturator, Wow/Flutter |
| Modulation | Choral, Flair, Freak, Phasis, Ring Modulator, Rotator, Vibrato/Chorus, Legacy Chorus, Legacy Flanger, Legacy Phaser |
| Mangling | Beat Masher, Beat Slicer, Gater, Reverse Grain, Transpose Stretch |
| Delays | PsycheDelay, Replika Delay, Twin Delay, Legacy Delay |
| Reverbs | Convolution, Plate Reverb, Raum, Reverb, Legacy Reverb |
| Spatial | Stereo Modeller, Stereo Tune, Surround Panner |
| Utilities | AET Filter, Gainer, Inverter, Send Levels |

## Kontakt system contracts

Compatibility includes NKI instruments, NKM multis, snapshots and base-instrument identity, banks/program switching, sample/resource resolution, NCW/WAV/AIFF decoding, saved-state roundtrips, disk residency and underruns, polyphony/stealing, pedals and overlapping-note ownership, transport/tempo, automation, routing, KSP callbacks and scheduling, persistent variables, asynchronous operations, UI/resources and parameter readback. Each imported parameter must affect the corresponding state or be diagnosed as unsupported. [NI file formats](https://docs.native-instruments.com/ni-tech-manuals/kontakt-manual/en/file-formats), [KSP engine parameters](https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/engine-parameters).

## Kontakt script-facing DSP and system identifiers

The private `kontakt-ksp-identifiers.json` lists all 1,024 distinct identifiers observed in NI's engine-parameter reference, with 1,027 scoped category records and source anchors. It includes parameter names, module/filter/source enumerations and group-start constants. Shared identifiers may occur in more than one category. This extracts identifier facts only; numeric enum values, serialized field mappings, display conversion and DSP parameter laws still require confirmation against the reference version. [NI engine-parameter reference](https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/engine-parameters).

## Falcon module and parameter catalog

The public UVI element reference supplies 167 module/structural types: 83 FX, 32 legacy processors, 24 oscillators, 15 modulation types, five native event-processor types and eight structural types. The private `catalog.json` records all 2,877 documented parameter rows with internal module IDs, parameter IDs, types, minimums, maximums, defaults, units, modulability and source anchors; descriptive prose is excluded. These are interface facts, not oscillator/effect implementations. [UVI element/parameter reference](https://lua.uvi.net/_elements.html).

| Type | Catalog names |
| --- | --- |
| FX | 3 Band Compressor, 3 Band Limiter, 3 Band Shelf, Analog Chorus, Analog Crunch, Analog Filter, Analog Flanger, Analog Tape Delay, Autopan, Big Pi Tone, Biquad Filter, Bloom, Brickwall Filter, Comb Filter, Compressor Expander, Convolver, Crossover Filter, Diffuse Delay, Diffusion, Digital Eq, Digital Filter, Diode Clipper, Dispersor, Drive, Dual Delay X, Effect Rack, Ensemble 505, Exciter, Feedback Compressor, Feedback Machine, Flanger, Formant Crusher, Freq Shifter, Fuzz, Gain, Gain Matrix, Gate, Granulizer, Guitar Boxes, Harmonic Resonators, Harmonizer, IReverb, Ladder, LowPass 12, LowPass 24, Magnetic Bass Shaper, Maximizer, One Pole, Opal, Overdrive, Phase Meter, Phasor, Phasor Filter, Redux, Rez Filter, Rotary, SVF, Sallen-Key Filter, Shifter, SparkVerb, Spectrum Analyzer, Studio Limiter, TS Overdrive, Tape Echo, Thorus, Tilt, Tone Stack, Track Delay, Tremolo, Tube Amp, Tuner, UVI Filter, UVI Wide, UVInyl, VCF-20, VCF-20 Dual, VCF-4023, Velvet Delay, Vowel Filter, Vowels, WahWah, Wave Shaper, Xpander Filter |
| Modulation | AHD, Analog ADSR, Attack Decay, DAHDSR, Drunk, Flow Noise, LFO, Macro, Multi Envelope, Multi LFO, Parametric LFO, Script Event Modulation, Smooth Random, Step Envelope, Voice Modulator |
| Event Processor | Arpeggiator, MIDI Out, MIDI Player, Micro Tuner, Script Processor |
| Oscillator | 8o8 Bass Drum, Additive, Analog, Analog Stack, Bowed String, Drum, FM, Grains, Harmonic Resonators, IRCAM Granular, IRCAM Multi Granular, IRCAM Scrub, IRCAM Stretch, Noise, Organ, Phase Shaper, Pluck, Sample, Slice, Stretch, SupraSaw, Texture, VOSIM, Wavetable |
| Legacy | 2 Band EQ, 3 Band EQ, 8 Band EQ, Auto Wah, Beat Repeat, Chorus, Compressor, Cross Phaser, Double Drive, Dual Delay, FX Delay, FX Filter, Fat Delay, Gate Reverb, Limiter, Phaser, Ping Pong Delay, Plain Reverb, Predelay Verb, Redux, Ring Modulator, Robotizer, Rotary, Rotary Simple, Rotary Speaker, Simple Delay, Simple Reverb, Stereo Delay, TalkBox, UVI Destructor, UVI Drive, UVI Mastering |
| Other | AuxEffect, BusRouter, Engine, Keygroup, Layer, Part, Program, Synth |

## Falcon architecture and system contracts

The documented hierarchy is Multi/Synth → Part → Program → Layer → Keygroup → Oscillator. Preserve multiple oscillators, key/velocity and dimensional mappings, per-voice and shared modulators, scoped event chains, inserts, aux routing, and EffectRack parallel/multiband processing. Triggered envelopes have keygroup scope. Flattening these nodes into Kontakt groups loses routing and lifetime distinctions. [Falcon documentation](https://www.uvi.net/falcon), [UVI structural parameters](https://lua.uvi.net/_elements.html#ElemOther).

Programs/multis use UVIP/UVIM; clear sample maps use DMAP/XML. UFS header recognition does not supply a member index, clear program state or decoded sample resources. UVIScript requires its Lua host bindings, typed parameter access, note/event identity and forwarding, coroutine waits, transport/tempo, asynchronous sample/IR/MIDI operations, widget callbacks, persistence and restore ordering. [UVI API](https://lua.uvi.net/_a_p_i_page.html), [sample mapping](https://lua.uvi.net/_sample_mapping_intro.html), [existing format evidence](FALCON_FORMAT_GROUNDWORK.md), [runtime/UI evidence](FALCON_RUNTIME_UI_GROUNDWORK.md).

The subsequent [format specification](DSP_FORMAT_SPECIFICATION.md) extends the UFS evidence to 129,237 directory entries across all 26 local banks, including branch nodes and leaf chains. The independent numeric parser leaves encoded names opaque; the upgraded v2 reader separately decoded names and resolved every path across the same 26 banks, matching all 127,828 file and 1,409 folder counts. Transformed member payloads remain outside this corpus check. Workstation's compiled structure map covers 176 registrations and 1,208 simple accessors, with 3,624 original-byte accessor cases. Additional DSP checks establish four WaveShaper rectifier kernels in 480 blocks, Formant Crusher's fractional decimator in 495 blocks and BitCrusher's holding/quantization/saturation/filter/mixing path in 2,268 blocks. Complete effects and public parameter mappings remain unverified.

The October 8 topology pass identifies the distinct 178,689,024-byte `Kontakt 8.vst3plugin` audio payload behind the portable wrapper. This image has 229,907 unwind records, 15,010 RTTI candidates, 13,203 validated RTTI-linked table runs and 3,848 DSP-name-selected virtual targets; these are static candidates, without a new full decompilation. The payload exposes processor and controller interfaces on one `NI::AB::InterfaceVST3` object. All 272 isolated original-code interface queries pass, with three public method-slot maps verified against official headers. The [plugin topology](../artifacts/engine-analysis-2026-10-07/plugin-topology.json) keeps this payload distinct from the standalone EXE, auxiliary DLL and fallback classes. The [format topology](../artifacts/engine-analysis-2026-10-07/uvi-workstation/format-topology.json) indexes 507 XML documents, 1,312 nodes and 80 parent/child pairs; named modulation, containment and archive trees remain separate layers. Static Gain Matrix evidence establishes scratch-buffer routing, with runtime vector arithmetic still unresolved.

The installed 55,250,272-byte Workstation 4.0.9 VST3 bundle was also pinned separately from the cached standalone image. Its processor and controller are separate JUCE objects. Forty-two original-byte queries establish their three public interface tables and receiver offsets without reusing the standalone snapshot. [Independent plugin metadata](../artifacts/engine-analysis-2026-10-07/uvi-plugin/pe.json) records 80,888 unwind ranges, 5,746 RTTI candidates and 6,184 validated table runs. All six NI/UVI public interface tables and 314 total query cases are retained in the plugin topology artifact; live host construction and audio remain untested.

The cached manual identifies itself as Software Version 2025, EN251016. Its normalized appendix names cover eight sample and 16 synthesis oscillators, 87 effects/rack items, 32 legacy effects, 67 event processors/templates and 15 modulator/external-source entries. Rack presets and script-based event templates are broader than the native module list. Their parameter sets and underlying scripts were not recovered from an installed Falcon binary. Manual paths/page numbers remain in the private catalog.

## Current KONTRA frontier

The existing [feature inventory](FEATURES.md) and [compatibility checklist](COMPATIBILITY.md) describe known limits and were not re-certified by this static study. Current `src/fx/kind.rs` recognizes 45 effect identifiers. `src/fx/blocks.rs` has generic defaults for 16 kinds, with separate gain, stereo, reverb, convolution, send, filter and EQ paths. Recognition and retained parameter values exceed implemented signal processing. `src/engine/filter.rs:68` dispatches 26 native/legacy filter IDs plus KONTRA's old 1000 alias; some dispatches are explicitly proxies. These source facts do not establish reference-host parity.

Use the existing importer, voice/streaming machinery, modulation admission and FX stages for supported Kontakt paths. Separate v1/v2 integration worktrees already contain UVI bank access, program translation, sample decoding and loading; the older metadata inspector in this checkout is not the entire UVI implementation. The new reader patches extend those actual loaders. Complete Falcon oscillators, scoped routing, vendor DSP and UVIScript parity remain separate frontiers.

## Additional player research against v2

Six GPT-6.1-Sol agents with high reasoning inspected the v2 checkout at `e882a5d40d2705f18deab33e94ace8d701e7ebb6`, then researched one family each using primary vendor documentation and bounded local/public fixture checks. Their reports include exact code anchors, verified observations, unresolved grammar and the smallest useful adapter. They made no production changes or builds; this is research, not newly enabled playback support.

| Player family | Established format evidence | Next useful adapter |
| --- | --- | --- |
| Spitfire dedicated players | `Patches` / `Presets` / `Samples` layout; dedicated generations differ from Kontakt editions. Private preset/audio grammar unverified. [Vendor layout](https://support.spitfireaudio.com/en/articles/11816038-standard-folder-structure-for-spitfire-libraries), [research](../artifacts/engine-analysis-2026-10-07/external-format-research/spitfire.json). | Content inventory/probe; native import needs actual metadata and sample fixtures. |
| Orchestral Tools SINE | `.otmeta` metadata and `.otarc` sample companions per microphone. Internal framing/codec unverified. [Vendor download documentation](https://orchestraltools.helpscoutdocs.com/article/335-using-direct-downloads), [research](../artifacts/engine-analysis-2026-10-07/external-format-research/sine.json). | Metadata/resource inspector first; preserve imported articulation mappings when playback becomes possible. |
| EastWest PLAY / OPUS | PLAY `.ewi` instruments/multis and encrypted `.ews` samples; `.ewui` / `.ewus` are update markers. Hollywood Opus Edition and Hollywood Strings 2 use OPUS; older products require separate profiles. OPUS byte grammar unverified. [PLAY manual](https://media.soundsonline.com/manuals/EW-Play-6-User-Manual.pdf), [vendor FAQ](https://www.soundsonline.com/support/faq), [research](../artifacts/engine-analysis-2026-10-07/external-format-research/eastwest.json). | Version-aware inventory, then a fixture-backed short articulation and one mic. |
| Vienna Synchron / Vienna Instruments | Synchron clipboard export is JSON; screenshot patch identities use `vol://` and `.vsynpatch`. Public legacy metadata fixtures verify `PRX2` headers in `.vipst` / `.vipreset` and `CcnK` / `FPCh` in `.fxp`. Saved Synchron preset/archive grammar remains unverified. [Synchron workflow](https://www.vsl.co.at/manuals/synchron-player/custom-preset-creation), [legacy fixtures](https://support.presonus.com/hc/en-us/articles/210049003-Using-the-Vienna-Symphonic-Library-with-Notion), [research](../artifacts/engine-analysis-2026-10-07/external-format-research/vienna.json). | Clipboard JSON inspection preserving the full dimension tree and unresolved resource identities. |
| Steinberg HALion / HALion Sonic | Public VST3 preset framing: 48-byte `VST3` header and indexed chunks. An official 9,105-byte fixture contains `Prog` and `Info`; HALion state and VST Sound payload grammar remain opaque. [VST3 specification](https://steinbergmedia.github.io/vst3_dev_portal/pages/Technical%2BDocumentation/Locations%2BFormat/Preset%2BFormat.html), [research](../artifacts/engine-analysis-2026-10-07/external-format-research/halion.json). | Bounded preset metadata inspection; an authored Lua introspection snapshot plus loose sample export could provide a separate interchange adapter. |
| Decent Sampler | Published `.dspreset` XML, unencrypted `.dslibrary` ZIP and `.dsbundle` folder structure. [Vendor format documentation](https://decentsampler-developers-guide.readthedocs.io/en/stable/introduction.html), [packaging](https://www.decentsamples.com/2024/01/24/q-is-it-possible-to-extract-the-samples-contained-within-decent-sampler-instruments/), [research](../artifacts/engine-analysis-2026-10-07/external-format-research/decent.json). | Best immediate native importer candidate: translate a basic XML/loose-WAV subset into existing IR and render through the existing engine. |

The common route is source reader/translator → `sampler_ir::Instrument` plus ordered sample resources → existing decoding or streaming → `sampler_core::lower::lower_with` → immutable `Prepared` → shared `Runtime`. The IR describes semantics; it does not decode a vendor archive. Runtime audio rendering should retain the existing source, note ownership, streaming, variation and bus services. New formats need their own reader/resource mapping, rather than a new playback engine.

Exact blockers found in v2:

- `src/sound/v2.rs:212` calls `assign_alternatives(32)` when applying articulation drivers, replacing imported CC/channel/program alternatives. Preserve source selections before claiming imported switching works.
- `crates/sampler-core/src/lower.rs:422` rejects `First` and `Legato` despite their IR enum variants. Recorded interval transitions require additional semantics and reference fixtures.
- The current single articulation selector does not express Vienna's recursive independent dimensions or HALion's multiple MegaTrig selectors. Continuous parallel crossfades, selector ownership and round-robin reset rules need distinct treatment.
- HALion Lua 5.2.3, thread contexts, note lifetimes and slot-local/shared state differ from the UVI Lua host. Reusing the language name alone cannot supply compatible execution. [HALion Lua profile](https://steinbergmedia.github.io/halion-script-api/HALion-Script/pages/What-is-HALion-Script.html), [note lifetime](https://steinbergmedia.github.io/halion-script-api/HALion-Script/pages/playNote.html).
- Group effects in Decent can run per note; they need voice-scoped chains. The lowering path rejects semantic controls, group chains and Delay; loop-crossfade curves and output-stage dynamics also need explicit compatibility decisions.

No native fixtures were found for SINE, EastWest, Spitfire dedicated players or current Vienna/HALion content in the reported bounded scans. Public preset headers and documented file extensions establish inspection possibilities, not decoded sample playback. The report artifacts record per-family source links and validation plans. Further candidates include Groove Agent/Beat Agent and SFZ engine profiles; these have not received separate agents in this batch. [Groove Agent kit export](https://www.steinberg.help/r/groove-agent/6.0/en/halion/topics/beat_agent/exporting_kits_t.html), [sfizz implementation](https://github.com/sfztools/sfizz).

## Verification required for compatibility

Before claiming a module compatible, establish its serialized identity and parameter laws, coefficient/state layout, modulation and smoothing cadence, channel/voice scope, reset and saved-state semantics, latency and tail. Compare impulse/sine/sweep and note/controller sequences against the same reference version at multiple sample rates and block sizes; include extremes, bypass, automation, state reload and overlapping voices. A complete automated export is only input to that work. The targeted original-byte checks above cover isolated behavior; no complete reference-host rendering or end-to-end sound-equivalence comparison was executed.
