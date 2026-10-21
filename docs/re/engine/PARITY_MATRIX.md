# Kontakt 8 parity matrix

One row per Kontakt module. Status: **missing** / **partial** / **parity**. Evidence: **vector** (native vectors / Wine-oracle dump), **decomp** (constants confirmed in the Kontakt 8 decompile), **docs** (public docs only), **none**. A row becomes `parity` only with a bit-exact or tolerance test against native data. Owners update their rows in the same PR as the code. Initial statuses are `unknown`: the owner fills them from the unit doc in `docs/re/engine/<slug>.md`.


## Sources (k-sampler-source, k-wavetable-tonemachine, k-timemachine, k-beat-s1200-mp60, k-voice-engine)

| Module | Status | Evidence | Notes |
| --- | --- | --- | --- |
| Sampler | unknown | none | |
| DFD | unknown | none | |
| Wavetable | unknown | none | |
| Tone Machine | unknown | none | |
| Time Machine | unknown | none | |
| Time Machine 2 | unknown | none | |
| Time Machine Pro | unknown | none | |
| Beat Machine | unknown | none | |
| S1200 Machine | unknown | none | |
| MP60 Machine | unknown | none | |
| Voice/group/zone engine (RR, release triggers, stealing, legato, choke) | unknown | none | |

## Filters (filt-sv, filt-ladder-monark, filt-ar-daft, filt-legacy, filt-multi-versatile, filt-formant-effect)

| Module | Status | Evidence | Notes |
| --- | --- | --- | --- |
| SV LP1 | parity | vector | Wine capture rows (res 0, .5, .9, u 0..1) within 1 dB (most rows 0.01 dB); guard test `sv_responses_match_the_kontakt_capture` |
| SV LP2 | partial | vector | Capture res 0 within 1 dB at u .25..1; u = 0 open (G10), res .9 open (G4). filt-sv
| SV LP4 | partial | vector | Capture res 0 u .25..1 within 1 dB; u=0 open (G10), res .9 open (G4). filt-sv |
| SV LP6 | partial | vector | Capture res 0 u .25..1 within 1 dB; u=0 open (G10), res .9 open (G4). filt-sv |
| SV HP1 | parity | vector | Wine capture rows (res 0, .5, .9, u 0..1) within 1 dB (most rows 0.01 dB); guard test |
| SV HP2 | partial | vector | Capture res 0 within 1 dB at all u; res .9 open (G4). filt-sv
| SV HP4 | partial | vector | Capture res 0 within 1 dB at all u; res .9 open (G4). filt-sv |
| SV BP6 | partial | vector | Capture res 0 within 1 dB except u = 0 (G10); res .9 open (G4). filt-sv |
| SV HP6 | partial | vector | Capture res 0 within 1 dB at all u; res .9 open (G4). filt-sv |
| SV BP2 | partial | vector | Capture res 0 u 0...75 within 1 dB; u=1 (G12) and res .9 (G4) open. filt-sv |
| SV BP4 | partial | vector | Capture res 0 within 1 dB at all u; res .9 open (G4). filt-sv |
| SV Notch | partial | vector | Notch 4 and Notch 6 captured: res 0 within 1 dB (Notch 6 30 Hz null open with G10); res .9 open (G4). filt-sv
| SV Par. LP/HP | partial | vector | Native vector at bandwidth .3, u .5, res .5 (1e-6); gains and `l` confirmed from the binary (G8, G9 closed); bandwidth record path (G7) and a capture open. filt-sv |
| SV Par. BP/BP | partial | vector | Native vector at bandwidth .3, u .5, res .5 (1e-6); gains and `l` confirmed (G8, G9 closed); G7 and a capture open. filt-sv |
| SV Ser. LP/HP | partial | vector | Native vector at bandwidth .3, u .5, res .5 (1e-6); gains and `l` confirmed (G8, G9 closed); G7 and a capture open. filt-sv |
| Ladder LP1 | unknown | none | |
| Ladder LP2 | unknown | none | |
| Ladder LP3 | unknown | none | |
| Ladder LP4 | unknown | none | |
| Ladder HP1 | unknown | none | |
| Ladder HP2 | unknown | none | |
| Ladder HP3 | unknown | none | |
| Ladder HP4 | unknown | none | |
| Ladder BP2 | unknown | none | |
| Ladder BP4 | unknown | none | |
| Ladder Peak | unknown | none | |
| Ladder Notch | unknown | none | |
| Monark | unknown | none | |
| PRO-53 | unknown | none | |
| AR LP2 | unknown | none | |
| AR LP4 | unknown | none | |
| AR LP2/4 | unknown | none | |
| AR HP2 | unknown | none | |
| AR HP4 | unknown | none | |
| AR HP2/4 | unknown | none | |
| AR BP2 | unknown | none | |
| AR BP4 | unknown | none | |
| AR BP2/4 | unknown | none | |
| Daft | unknown | none | |
| Daft HP | unknown | none | |
| Legacy LP1 | unknown | none | |
| Legacy LP2 | unknown | none | |
| Legacy LP4 | unknown | none | |
| Legacy LP6 | unknown | none | |
| Legacy Ladder | unknown | none | |
| Legacy HP1 | unknown | none | |
| Legacy HP2 | unknown | none | |
| Legacy HP4 | unknown | none | |
| Legacy BP2 | unknown | none | |
| Legacy BP4 | unknown | none | |
| Legacy BR4 | unknown | none | |
| 3x2 Versatile | unknown | none | |
| Dual SKF | unknown | none | |
| Simple LP/HP | unknown | none | |
| Formant I | unknown | none | |
| Formant II | unknown | none | |
| Phaser (filter) | unknown | none | |
| Vowel A | unknown | none | |
| Vowel B | unknown | none | |

## EQ (eq-kontakt)

| Module | Status | Evidence | Notes |
| --- | --- | --- | --- |
| Solid G-EQ | unknown | none | |
| AET Filter (EQ aspects) | unknown | none | |

## Dynamics (dyn-compressors, dyn-limiters-transient)

| Module | Status | Evidence | Notes |
| --- | --- | --- | --- |
| Compressor | unknown | none | |
| Feedback Compressor | unknown | none | |
| Limiter | unknown | none | |
| Solid Bus Comp | unknown | none | |
| Supercharger GT | unknown | none | |
| Transient Master | unknown | none | |
| Transparent Limiter | unknown | none | |
| Gainer | unknown | none | |

## Amplifiers (k-amps-a, k-amps-b)

| Module | Status | Evidence | Notes |
| --- | --- | --- | --- |
| ACBox | unknown | none | |
| Bass Invader | unknown | none | |
| Bass Pro | unknown | none | |
| Cabinet | unknown | none | |
| EP Preamps | unknown | none | |
| HotSolo | unknown | none | |
| Jump | unknown | none | |
| Reverb Delight | unknown | none | |
| Super Fast 100 | unknown | none | |
| Twang | unknown | none | |
| Van51 | unknown | none | |

## Stomps / Lo-Fi / Tape (k-stomps-a, k-stomps-b, k-lofi-tape)

| Module | Status | Evidence | Notes |
| --- | --- | --- | --- |
| Big Fuzz | unknown | none | |
| Cat | unknown | none | |
| Chainsaw | unknown | none | |
| Cry Wah | unknown | none | |
| Dirt | unknown | none | |
| Distortion | unknown | none | |
| DStortion | unknown | none | |
| Fuzz | unknown | none | |
| Kolor | unknown | none | |
| Saturator | unknown | none | |
| Skreamer | unknown | none | |
| Skreamer Deluxe | unknown | none | |
| Bite | unknown | none | |
| Lo-Fi | unknown | none | |
| Tape Saturator | unknown | none | |
| Wow/Flutter | unknown | none | |

## Modulation FX (k-modfx-classic, k-modfx-freak)

| Module | Status | Evidence | Notes |
| --- | --- | --- | --- |
| Choral | unknown | none | |
| Flair | unknown | none | |
| Freak | unknown | none | |
| Phasis | unknown | none | |
| Ring Modulator | unknown | none | |
| Rotator | unknown | none | |
| Vibrato/Chorus | unknown | none | |
| Legacy Chorus | unknown | none | |
| Legacy Flanger | unknown | none | |
| Legacy Phaser | unknown | none | |

## Delays (k-delays)

| Module | Status | Evidence | Notes |
| --- | --- | --- | --- |
| PsycheDelay | unknown | none | |
| Replika Delay | unknown | none | |
| Twin Delay | unknown | none | |
| Legacy Delay | unknown | none | |

## Reverbs / Convolution (k-reverbs, k-convolution)

| Module | Status | Evidence | Notes |
| --- | --- | --- | --- |
| Convolution | unknown | none | |
| Plate Reverb | unknown | none | |
| Raum | unknown | none | |
| Reverb | unknown | none | |
| Legacy Reverb | unknown | none | |

## Spatial / Mangling / Utilities (k-spatial, k-mangling, k-utilities-routing)

| Module | Status | Evidence | Notes |
| --- | --- | --- | --- |
| Stereo Modeller | unknown | none | |
| Stereo Tune | unknown | none | |
| Surround Panner | unknown | none | |
| Beat Masher | unknown | none | |
| Beat Slicer | unknown | none | |
| Gater | unknown | none | |
| Reverse Grain | unknown | none | |
| Transpose Stretch | unknown | none | |
| Inverter | unknown | none | |
| Send Levels | unknown | none | |
| Group/instrument/bus/main insert routing and send taps | unknown | none | |
| Effect slot ordering and processing scopes | unknown | none | |

## Modulation sources (k-mod-envelopes, k-mod-lfo-step, k-mod-external)

| Module | Status | Evidence | Notes |
| --- | --- | --- | --- |
| AHDSR | unknown | none | |
| DBD | unknown | none | |
| Flexible envelope | unknown | none | |
| LFO | unknown | none | |
| Multi Digital | unknown | none | |
| Step modulator | unknown | none | |
| Envelope follower | unknown | none | |
| Glide | unknown | none | |
| External MIDI/performance sources | unknown | none | |
| Modulation routing (target, depth/polarity, shaping, smoothing) | unknown | none | |

## Script and library (k-ksp-engine-params, k-library-load)

| Module | Status | Evidence | Notes |
| --- | --- | --- | --- |
| KSP engine parameters (set/get_engine_par, enumerations, display conversions) | unknown | none | |
| NKI / NKM / NKB loading | unknown | none | |
| Snapshots and base-instrument identity | unknown | none | |
| NCW / WAV / AIFF decoding | unknown | none | |
| Saved-state round trips | unknown | none | |
