# Kontakt 8 parity matrix

One row per Kontakt module. Status: **missing** / **partial** / **parity**. Evidence: **vector** (native vectors / Wine-oracle dump), **decomp** (constants confirmed in the Kontakt 8 decompile), **docs** (public docs only), **none**. A row becomes `parity` only with a bit-exact or tolerance test against native data. Owners update their rows in the same PR as the code. Initial statuses are `unknown`: the owner fills them from the unit doc in `docs/re/engine/<slug>.md`.


## Sources (k-sampler-source, k-wavetable-tonemachine, k-timemachine, k-beat-s1200-mp60, k-voice-engine)

| Module | Status | Evidence | Notes |
| --- | --- | --- | --- |
| Sampler | partial | docs, decomp | Loops, reverse, start/end, tune and root map and are covered by engine tests; no native render or oracle dump compared yet (`docs/re/engine/k-sampler-source.md`). Interpolation kernel per group unrecovered (corpus value 0 everywhere). |
| DFD | partial | decomp | Preload unit (bytes), default 60 KiB, global override and 6 KiB window rounding confirmed in the Kontakt 8 decompile. Window law not yet wired into stream head sizing; runtime mode-4 mapping unrecovered. |
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
| AR LP2 | partial | vector | Kernel matches native vectors (2e-6, ar_native_vectors.json, 81 scenarios). Cutoff law decomp-confirmed. Host recording and CPU acceptance open (docs/re/engine/filt-ar-daft.md). |
| AR LP4 | partial | vector | Same kernel and vectors as AR LP2. Host recording open. |
| AR LP2/4 | partial | vector | Same kernel and vectors as AR LP2. Host recording open. |
| AR HP2 | partial | vector | Same kernel and vectors as AR LP2. Host recording open. |
| AR HP4 | partial | vector | Same kernel and vectors as AR LP2. Host recording open. |
| AR HP2/4 | partial | vector | Same kernel and vectors as AR LP2. Host recording open. |
| AR BP2 | partial | vector | Same kernel and vectors as AR LP2. Host recording open. |
| AR BP4 | partial | vector | Same kernel and vectors as AR LP2. Host recording open. |
| AR BP2/4 | partial | vector | Same kernel and vectors as AR LP2. Host recording open. |
| Daft | partial | decomp | Two-stage loop, knees, gain/cutoff/feedback/resonance/response laws and Kaiser x2 transcribed from FUN_140affa30 with image constants; golden hash only. Start-up, ramp clock and Kontakt audio open. |
| Daft HP | partial | decomp | Same loop as Daft (response 1). Golden hash only; Kontakt audio open. |
| SV LP1 | unknown | none | |
| SV LP2 | unknown | none | |
| SV LP4 | unknown | none | |
| SV LP6 | unknown | none | |
| SV HP1 | unknown | none | |
| SV HP2 | unknown | none | |
| SV HP4 | unknown | none | |
| SV HP6 | unknown | none | |
| SV BP2 | unknown | none | |
| SV BP4 | unknown | none | |
| SV Notch | unknown | none | |
| SV Par. LP/HP | unknown | none | |
| SV Par. BP/BP | unknown | none | |
| SV Ser. LP/HP | unknown | none | |
| Ladder LP1 | partial | vector | steady knobs only: native PCM grid (`ladder_native_vectors.rs`, `ladder_native_hq_vectors.rs`); moving controls, flag 0x49 path and HQ-on non-LP4 selectors unvectored |
| Ladder LP2 | partial | vector | steady knobs only: native PCM grid (`ladder_native_vectors.rs`, `ladder_native_hq_vectors.rs`); moving controls, flag 0x49 path and HQ-on non-LP4 selectors unvectored |
| Ladder LP3 | partial | vector | steady knobs only: native PCM grid (`ladder_native_vectors.rs`, `ladder_native_hq_vectors.rs`); moving controls, flag 0x49 path and HQ-on non-LP4 selectors unvectored |
| Ladder LP4 | partial | vector | most native vectors (84 HQ-off, 249 HQ-on cells); steady knobs only; moving controls and flag 0x49 unvectored |
| Ladder HP1 | partial | vector | steady knobs only: native PCM grid (`ladder_native_vectors.rs`, `ladder_native_hq_vectors.rs`); moving controls, flag 0x49 path and HQ-on non-LP4 selectors unvectored |
| Ladder HP2 | partial | vector | steady knobs only: native PCM grid (`ladder_native_vectors.rs`, `ladder_native_hq_vectors.rs`); moving controls, flag 0x49 path and HQ-on non-LP4 selectors unvectored |
| Ladder HP3 | partial | vector | steady knobs only: native PCM grid (`ladder_native_vectors.rs`, `ladder_native_hq_vectors.rs`); moving controls, flag 0x49 path and HQ-on non-LP4 selectors unvectored |
| Ladder HP4 | partial | vector | steady knobs only: native PCM grid (`ladder_native_vectors.rs`, `ladder_native_hq_vectors.rs`); moving controls, flag 0x49 path and HQ-on non-LP4 selectors unvectored |
| Ladder BP2 | partial | vector | steady knobs only: native PCM grid (`ladder_native_vectors.rs`, `ladder_native_hq_vectors.rs`); moving controls, flag 0x49 path and HQ-on non-LP4 selectors unvectored |
| Ladder BP4 | partial | vector | steady knobs only: native PCM grid (`ladder_native_vectors.rs`, `ladder_native_hq_vectors.rs`); moving controls, flag 0x49 path and HQ-on non-LP4 selectors unvectored |
| Ladder Peak | partial | vector | steady knobs only: native PCM grid (`ladder_native_vectors.rs`, `ladder_native_hq_vectors.rs`); moving controls, flag 0x49 path and HQ-on non-LP4 selectors unvectored |
| Ladder Notch | partial | vector | steady knobs only: native PCM grid (`ladder_native_vectors.rs`, `ladder_native_hq_vectors.rs`); moving controls, flag 0x49 path and HQ-on non-LP4 selectors unvectored |
| Monark | partial | vector | kemu vectors for modes LP4/LP2/LP1/BP, variants 0-2, 48 kHz, mono (`kontaktfx_vectors.json`); stereo channel interleave and noise LCG per channel unprobed |
| PRO-53 | partial | vector | kemu stage vectors, 2e-5 relative, 3 knob points (`cascade_stage_vectors.json`); moving controls and stereo unvectored |
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
| Legacy LP1 | partial | vector (Wine oracle, golden 0.1 dB), decomp | static response matches; dynamic cutoff cadence not measured |
| Legacy LP2 | parity | vector (Wine oracle, golden 0.1 dB), decomp | static response; cadence shares the clock (decomp) |
| Legacy LP4 | partial | vector (Wine oracle) | knob 0 (res 0) shape differs: Kontakt ~8 dB/oct tail, ours steeper; mechanism open (filt-legacy doc) |
| Legacy LP6 | partial | vector (Wine oracle) | knob 0 shape differs as LP4 |
| Legacy Ladder | partial | vector (Wine oracle), decomp | knob 0 shape differs; setter and stage clip constants match the decompile |
| Legacy HP1 | partial | vector (Wine oracle, golden 0.1 dB), decomp | static response matches; dynamic cadence not measured |
| Legacy HP2 | parity | vector (Wine oracle, golden 0.1 dB) | static response |
| Legacy HP4 | parity | vector (Wine oracle, golden 0.1 dB) | static response, knob 0 included |
| Legacy BP2 | parity | vector (Wine oracle, golden 0.1 dB) | static response |
| Legacy BP4 | parity | vector (Wine oracle, golden 0.1 dB) | static response |
| Legacy BR4 | parity | vector (Wine oracle, golden 0.1 dB) | static response |
| 3x2 Versatile | unknown | none | |
| Dual SKF | unknown | none | |
| Simple LP/HP | unknown | none | |
| Formant I | partial | vector (Kontakt 8 measured, 9 steps, `formant_responses_match_kontakt_measurements`) | static Talk/Sharp/Size parity to 0.1 dB at 30..15000 Hz; live lanes lack the native per-sample coefficient ramp (decompiled `FUN_140b00750`, 15 ms length unread) |
| Formant II | partial | vector (Kontakt 8 measured, 8 steps, same test, table 2) | static parity; live lanes route to table 2 (decompile, control 3 selector) but are not measured on Kontakt; the static flag value for type 91 is not read as a constant |
| Phaser (filter) | partial | vector (Kontakt 8 measured, 8 steps, `phaser_filter_matches_kontakt_measurements`) | static cutoff and resonance feedback parity to 0.1 dB; live Cutoff/Resonance lanes not wired |
| Vowel A | partial | vector (Kontakt 8 measured, 8 steps, `vowel_filters_match_kontakt_measurements`) | static parity to 0.1 dB; live morph lanes not wired |
| Vowel B | partial | vector (Kontakt 8 measured, 8 steps, same test) | static parity to 0.1 dB; live morph lanes not wired |
| Simple LP/HP | missing | none (Wine oracle inconclusive) | CUTOFF/RESONANCE do not move the response; parameter mapping not traced in the export; smoother and stereo unprobed |
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
| 3x2 Versatile | partial | vector + decomp | Native stage vectors (kemu) at cutoff 0.5 only (`cascade_stage_vectors.json`, VERS-lp / VERS-mixed); laws confirmed in decompile (cutoff setter, resonance, normaliser, process). Missing: cutoff / shift / morph / wet / gain sweeps (Wine oracle), SSE rounding path. See `filt-multi-versatile.md`. |
| Dual SKF | partial | vector + decomp | Native vectors at 48 kHz, mono, 3 knob sets (`kontaktfx_vectors.json`); setParam clamp and drive `2v-1` confirmed in decompile. Missing: record-to-slot mapping (reader branch for type 130 not isolated), other sample rates, stereo. |
| Simple LP/HP | partial | vector | Native vectors (kemu) at 48 kHz, 3 knob sets (`kontaktfx_vectors.json`). Missing: the alternate smoother (u8, rate-based) is unprobed; stereo interleave unprobed. |
| Formant I | unknown | none | |
| Formant II | unknown | none | |
| Phaser (filter) | unknown | none | |
| Vowel A | unknown | none | |
| Vowel B | unknown | none | |

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
| Compressor | partial | decomp | Enhanced and Pro constants confirmed in Kontakt8.exe; Enhanced/Pro have no native vectors yet (computed/golden only); Classic unverified. `docs/re/engine/dyn-compressors.md` |
| Feedback Compressor | partial | decomp | Core setter `0x141ac56d0` and mapping constants confirmed; saved float field 6 has no core input and link/side-chain bytes unbound; no native vectors |
| Limiter | unknown | none | |
| Solid Bus Comp | partial | decomp | Attack/release tables and coefficient law confirmed (docs); core `0x141aaa1e0` located; threshold/ratio/envelope unverified; no native vectors |
| Supercharger GT | missing | vector, decomp | Static native vectors in `docs/re/engine/vectors/supercharger-static.tsv`; MIX result unexplained. Owns the `Rockruepel_CompXT` core (`0x141a820e0`); CompXT law, saved-field→index map and mode meanings not recovered; still diagnosed (NotModeled) |
| Transient Master | unknown | none | |
| Transparent Limiter | unknown | none | |
| Gainer | unknown | none | |
| Compressor | unknown | none | |
| Feedback Compressor | unknown | none | |
| Limiter | partial | decomp | Detector biquad, gain computer, release law (ln2/N), input gain and output clamp [-1, 1] confirmed from 0x140904890 / 0x140960140. Open: no native vector (Wine capture unreliable) |
| Solid Bus Comp | unknown | none | |
| Supercharger GT | unknown | none | |
| Transient Master | partial | decomp | Saved order and constants confirmed (reader slot 8 of BParFXTrans). Clip = control 3 (variant 0x141a59d40). Open: controls 4-7 and bit0 select 64-variant process paths, not implemented (live-only); no native vector |
| Transparent Limiter | partial | decomp | Saved order traced (reader 0x140d01ae0, version 0x51). Core has a key input (Key::External, keyed Hilbert detector, tests). Slot routing of the key bus is not wired, so the Kontakt flag stays diagnosed. No native vector |
| Gainer | partial | decomp | Smoother 1/1800 and unity-window hold (0x140aa2980) implemented. Gain-copy snap (0x1408374b0) documented, not modelled (no object copy in engine). Open: dry != 0 inside the window; no native vector |

## Amplifiers (k-amps-a, k-amps-b)

| Module | Status | Evidence | Notes |
| --- | --- | --- | --- |
| ACBox | parity | vector | Kernel matches native kemu vectors (`amp/acbox.rs`, 1e-6); knob-law coverage limited to the recorded cases |
| Bass Invader | missing | decomp | Field bindings and core layout only; no DSP laws recovered. Docs: `k-amps-a-bassinvader.md` |
| Bass Pro | missing | decomp | Knob-to-field map and apply path; kernel and tone stack not recovered. Docs: `k-amps-a-basspro.md` |
| Cabinet | parity | vector | Kernel matches native kemu vectors (`amp/cabinet.rs`, 1e-6); knob-law coverage limited to the recorded cases |
| EP Preamps | missing | decomp | Drive-core structure partial; no laws; no render on this install. Docs: `k-amps-a-epp.md` |
| HotSolo | missing | decomp | Reader fields and entry points only; no laws. Docs: `k-amps-a-hotsolo.md` |
| Jump | unknown | none | |
| Reverb Delight | unknown | none | |
| Super Fast 100 | unknown | none | |
| Twang | unknown | none | |
| Van51 | unknown | none | |
| ACBox | unknown | none | |
| Bass Invader | unknown | none | |
| Bass Pro | unknown | none | |
| Cabinet | unknown | none | |
| EP Preamps | unknown | none | |
| HotSolo | unknown | none | |
| Jump | partial | fit to one native trace (default knobs, relative RMS 0.242, `jump_default.f32le`) | Shared chain preset (`amp/jump.rs`); knobs ignored (noted). Knob laws and stage tables not read (kernel 0x140aac6f0). |
| Reverb Delight | missing | decomp (reader 0x140ac5e50); Wine-oracle sweep partial | Tremolo and spring-reverb cores not read. MONO field unproved. Diagnosed as unproved. |
| Super Fast 100 | missing | decomp (reader 0x140ac61a0) | No bindings; core not read. Diagnosed as unproved. |
| Twang | partial | vector (`matches_the_native_twang`, kemu vectors) | Core bit-matched to native vectors. Table and reset point dumped under kemu, not re-derived. Knob laws not separately vector-tested. |
| Van51 | missing | decomp (reader 0x140d033e0); Wine-oracle sweep partial | Core not read. Diagnosed as unproved. |

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
| Fuzz | unknown | none | |
| Kolor | unknown | none | |
| Saturator | unknown | none | |
| Skreamer | unknown | none | |
| Skreamer Deluxe | unknown | none | |
| Bite | partial | decomp | Mapped (0x66); core model-checked, not Kontakt output; 20 ms ramps unmodelled. docs/re/engine/k-lofi-tape.md |
| Lo-Fi | partial | none | Mapped (0x20); hold/quantizer/noise tests are spec-derived; flag (P truncation) and reset seed unproved. docs/re/engine/k-lofi-tape.md |
| Tape Saturator | partial | none | Mapped (0x42); table recovered; no Kontakt output vector. docs/re/engine/k-lofi-tape.md |
| Wow/Flutter | partial | decomp | Reader and ids confirmed (FUN_140ac65f0, FUN_140a821c0); params mapped to a generic timemod law; per-sample kernel not located; scrape/age/saturation/gate omitted and diagnosed. docs/re/engine/k-lofi-tape.md |

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
| Choral | unknown | none | |
| Flair | unknown | none | |
| Freak | partial | vector | Native fxemu vectors at 48 kHz pass (`dsp::color::freak::tests::matches_the_native_core`). Hilbert designer (0x141968890) ported in `dsp/color/hilbert.rs`: 44.1 and 96 kHz tables bit-exact, 48 and 88.2 kHz within 4e-6 relative (cause not isolated; ulp-level sensitivity of `log2`). Saved field 17 binding unverified. See `docs/re/engine/k-modfx-freak-ring-rotator.md`. |
| Phasis | unknown | none | |
| Ring Modulator | missing | decomp | Class/module names and LFO waveform count (2) confirmed in decompile. No core, no mapping, no DSP law yet; process method not locatable without vtable slot dump. |
| Rotator | missing | decomp | Class/module names confirmed in decompile (`BEffectRotator`, `RotModuleBase`). No core, no mapping, no law yet; process method not locatable without vtable slot dump. |
| Vibrato/Chorus | unknown | none | |
| Legacy Chorus | unknown | none | |
| Legacy Flanger | unknown | none | |
| Legacy Phaser | unknown | none | |

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
| Beat Masher | missing | none | missing: reader fields and factory/vtable structure confirmed (k-mangling.md); audio kernel and laws not located; no native vector |
| Beat Slicer | missing | none | missing: reader fields and factory/vtable structure confirmed; audio kernel and laws not located; no native vector |
| Gater | missing | none | missing: vtable and child-unit structure confirmed; gate/step/noise kernel not located; no native vector |
| Reverse Grain | missing | none | missing: reader fields confirmed (shared with Transpose Stretch, KE065 unresolved); kernel not located |
| Transpose Stretch | missing | none | missing: reader fields confirmed (shared with Reverse Grain); kernel not located; vtable differs from the other four mangling modules in slots 3, 16, 66 |
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
| AHDSR | unknown | none | |
| DBD | unknown | none | |
| Flexible envelope | unknown | none | |
| LFO | partial | decomp + vector | Native waves and Multi/Digital structure confirmed; fade cadence, fade init, synced fade, held-noise reset seed open. See `docs/re/engine/k-mod-lfo-step.md`. |
| Multi Digital | partial | decomp + vector | Mixer terms and normalisation confirmed; data constants unverified in binary; fade > 0 with components refused. |
| Step modulator | partial | decomp + vector | Render confirmed (`0x140a768e0`); `+0xdd` branch and stored-field setup not modelled (`UnknownLaw`). |
| Envelope follower | unknown | none | |
| Glide | unknown | none | |
| External MIDI/performance sources | unknown | none | |
| Modulation routing (target, depth/polarity, shaping, smoothing) | unknown | none | |

## Script and library (k-ksp-engine-params, k-library-load)

| Module | Status | Evidence | Notes |
| --- | --- | --- | --- |
| KSP engine parameters (set/get_engine_par, enumerations, display conversions) | partial | decomp | `_m` builtins (set/get_engine_par_m, get_engine_par_disp_m) missing; unaddressable ids and invalid constant group indexes are now diagnosed at build time (not runtime faults); `_disp`/`on init` not yet diagnosed; ids unconfirmed. See `docs/re/engine/k-ksp-engine-params.md`. |
| NKI / NKM / NKB loading | unknown | none | |
| Snapshots and base-instrument identity | unknown | none | |
| NCW / WAV / AIFF decoding | unknown | none | |
| Saved-state round trips | unknown | none | |
| KSP engine parameters (set/get_engine_par, enumerations, display conversions) | unknown | none | |
| NKI / NKM / NKB loading | partial | docs | 835 files inspected statically; 40 translated fully (2026-10-13). Open: legacy/multiband EQ, FX, modulation routes, LFO waveform, convolution controls, wavetable fallback (see docs/re/engine/k-library-load.md). Not decompile-confirmed yet. |
| Snapshots and base-instrument identity | partial | docs | Other-files table and per-occurrence resources in place (docs/re/KONTAKT_SNAPSHOT_RESOURCES.md); snapshot lifecycle modes 1/3 not in the full-read path. |
| NCW / WAV / AIFF decoding | partial | none | Vendored NCW round-trip and WAV/AIFF unit tests exist (samples.rs tests). No native vector yet; Kontakt NCW encode/decode located at 0141080000.c:4949-4965 but not read. |
| Saved-state round trips | unknown | none | Not audited in this pass. |
