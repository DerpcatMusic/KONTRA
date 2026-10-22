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
| Beat Machine | missing | decomp (partial) | Stored mode 4; no BBeatMachine class found; `BSlice` (0x90-byte object, `0140d20000.c:700`) and `disallow_group` beatmachine message (`0140870000.c`) are the only leads. Slice/pattern law unlocated. See docs/re/engine/k-beat-s1200-mp60.md |
| S1200 Machine | partial | decomp + focused golden tests | Stored mode 6; filter modes 1..=5 run (AA IIR, mode biquads, 26 kHz hold, quantiser) on voices with `Group.machine`. Filter modes 0, 6, 7 not recovered; no render comparison against Kontakt yet. See docs/re/engine/k-beat-s1200-mp60.md |
| MP60 Machine | missing | decomp (partial) + focused golden tests | Stored mode 7; IIR chain (stage 1, 12-bit quantiser, stage 2) with constants from the constructor, DC/stability and block-invariance tests; rate law and source fetch not decoded; not wired. See docs/re/engine/k-beat-s1200-mp60.md |
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
| Formant I | partial | vector (Kontakt 8 measured, 9 steps, `formant_responses_match_kontakt_measurements`) | static Talk/Sharp/Size parity to 0.1 dB at 30..15000 Hz; live lanes lack the native per-sample coefficient ramp (decompiled `FUN_140b00750`, 15 ms length unread) |
| Formant II | partial | vector (Kontakt 8 measured, 8 steps, same test, table 2) | static parity; live lanes route to table 2 (decompile, control 3 selector) but are not measured on Kontakt; the static flag value for type 91 is not read as a constant |
| Phaser (filter) | partial | vector (Kontakt 8 measured, 8 steps, `phaser_filter_matches_kontakt_measurements`) | static cutoff and resonance feedback parity to 0.1 dB; live Cutoff/Resonance lanes not wired |
| Vowel A | partial | vector (Kontakt 8 measured, 8 steps, `vowel_filters_match_kontakt_measurements`) | static parity to 0.1 dB; live morph lanes not wired |
| Vowel B | partial | vector (Kontakt 8 measured, 8 steps, same test) | static parity to 0.1 dB; live morph lanes not wired |

## EQ (eq-kontakt)

| Module | Status | Evidence | Notes |
| --- | --- | --- | --- |
| Solid G-EQ | partial | vector + decomp | Spline tables and per-section topology from Wine-oracle fits (`docs/re/kontakt-bindings/solid-geq-fits.tsv`); decomp confirms knob storage and recompute path. Open: LF/HF shelf law (fit residual 0.2-1.1 dB), coefficient glide writer (20 ms unconfirmed), live bell/pass switches, bank 16 not applied. See `docs/re/engine/eq-kontakt.md` |
| Filters/EQ 1/2/3-band (legacy, kinds 22-24) | partial | vector | Band shape and serial cascade proved by oracle (rms 0.000 dB, `eq-topology.tsv`); kind 22 x3 multi-slot write not proven (measurement artefact) |
| Pro EQ | n/a (not in Kontakt 8) | decomp + presets | No Pro EQ string in the Kontakt 8 decompile and no Pro EQ preset under `Filters/` (only `Filters/EQ` and `Filters/Solid G-EQ`) |
| AET Filter | owned by filt-formant-effect | - | Effect type, not an EQ; see `docs/re/engine/filt-formant-effect.md` |

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
| Fuzz | partial | vector | Static kernel matches kemu native vectors (`amp/fuzz.rs`). Live amount/bass/treble lanes are wired. Unconfirmed: live identity law and saved-record offsets beyond the reader layout; mono is stored only. |
| Kolor | missing | decomp | Reader, 10-mode table and 11 knob registrations decompiled (`docs/re/engine/k-stomps-b.md`). No DSP: processing function not located, float-to-knob tie unconfirmed. |
| Saturator | partial | vector | Classic shape law and Enhanced/Drums curve points from Wine-oracle probes; live Shape on the physical slot. Enhanced/Drums PCM is not vector-tested; internal-modulator 32-frame clock missing. |
| Skreamer | partial | vector | Static core matches the kemu model of the native core (`skreamer.rs` tests); five live knobs wired. Missing: moving-control PCM against native data. |
| Skreamer Deluxe | missing | decomp | Reader (mode int + 5 floats + byte) and knob names drive/tone/bass/mid/treble decompiled. Not mapped: the earlier Tubescreamer mapping was wrong. Reported as not modeled. |
| Bite | unknown | none | |
| Lo-Fi | unknown | none | |
| Tape Saturator | unknown | none | |
| Wow/Flutter | unknown | none | |

## Modulation FX (k-modfx-classic, k-modfx-freak)

| Module | Status | Evidence | Notes |
| --- | --- | --- | --- |
| Choral | partial | decomp + kemu vectors | 4 modes, laws in `choral()`; kemu-rendered native f32 vectors, no Wine-oracle dump yet. `docs/re/engine/k-modfx-classic.md` |
| Flair | missing | bindings measured (TSV) | Pitch note writer, resampler, start phase not recovered; unsupported with reason |
| Freak | unknown | none | |
| Phasis | missing | bindings measured (TSV) | Field-to-core binding and oversampling resampler not recovered; unsupported with reason |
| Ring Modulator | unknown | none | |
| Rotator | unknown | none | |
| Vibrato/Chorus | missing | bindings measured (TSV) | Wave-digital LC ladder not recovered; unsupported with reason |
| Legacy Chorus | partial | Wine oracle + decomp | Two-head structure confirmed by decompile; oracle shows ~8.85 ms echoes, engine base is 0 (contradicted). Base-time source and Depth law open |
| Legacy Flanger | parity | decomp constants + kemu native vectors | Mapping and vectors present; not yet checked against Wine oracle |
| Legacy Phaser | parity | decomp constants + independent vector | 4-stage cascade; vector computed independently from decompile constants, not native-data |

## Delays (k-delays)

| Module | Status | Evidence | Notes |
| --- | --- | --- | --- |
| PsycheDelay | unknown | none | |
| Replika Delay | partial | decomp | v0x12 reader; RDL_TIME (documented law) only; core setter binding read (docs/re/engine/k-delays.md), core DSP not yet read |
| Twin Delay | unknown | none | |
| Legacy Delay | unknown | none | |

## Reverbs / Convolution (k-reverbs, k-convolution)

| Module | Status | Evidence | Notes |
| --- | --- | --- | --- |
| Convolution | partial | decomp | Reverse, early/late filters and composition, envelope, Auto Gain, predelay: decompile-confirmed structure. Size range and identity at 1.0 confirmed; resize engine `0x1419b3450`, S.Rate, crossover default, latency, true-stereo still missing. See docs/re/engine/k-convolution.md |
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
| AHDSR | partial | decompile (reader 0x140cffb40, converters 0x1407b85a0/85e0, constants from PE), recorded control table | Time and sustain laws fixed in lowering; control rate R, note-off and retrigger open |
| DBD | partial | decompile (resolver 0x1409651b0 mode 2, prep 0x140ae5d70), disasm argument order | Prep law in engine (`Breakpoints::dbd`); converter identity and IR mapping still open |
| Flexible envelope | partial | engine builders and loop/release laws; `docs/re/kontakt-flex-loop-jump.md` | Point, curve and release laws covered; retrigger and smoothing open |
| LFO | unknown | none | |
| Multi Digital | unknown | none | |
| Step modulator | unknown | none | |
| Envelope follower | partial | decomp | Executable (`ModSource::Follower`), decompile law and binary constants, one-span audio delay; no native vector (`k-mod-external.md`) |
| Glide | partial | decomp | Executable (`ModSource::Glide`), setup law from FUN_140cebba0; origin per plan, octave units unverified (`k-mod-external.md`) |
| External MIDI/performance sources | partial | none | 13 `ExternalSource` variants mapped (`modulation_objects.rs:84-96`); no per-source vectors (`k-mod-external.md`) |
| Modulation routing (target, depth/polarity, shaping, smoothing) | partial | none | Depth sign/invert tested (`library.rs:6063`); negative lag and shape law unproven (`k-mod-external.md`) |

## Script and library (k-ksp-engine-params, k-library-load)

| Module | Status | Evidence | Notes |
| --- | --- | --- | --- |
| KSP engine parameters (set/get_engine_par, enumerations, display conversions) | partial | decomp | `_m` builtins (set/get_engine_par_m, get_engine_par_disp_m) missing; unaddressable ids and invalid constant group indexes are now diagnosed at build time (not runtime faults); `_disp`/`on init` not yet diagnosed; ids unconfirmed. See `docs/re/engine/k-ksp-engine-params.md`. |
| NKI / NKM / NKB loading | unknown | none | |
| Snapshots and base-instrument identity | unknown | none | |
| NCW / WAV / AIFF decoding | unknown | none | |
| Saved-state round trips | unknown | none | |
