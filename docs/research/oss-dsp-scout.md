# OSS DSP scout: modern, parameterisable models for Kontakt parity

Goal: for each Kontakt DSP class we already emulate, name one modern, high-quality open-source model that can be **molded** (parameter mapping) to the measured Kontakt response shape, rather than cloning a per-effect implementation. "Drop-in" means the same transfer function with better topology or numerics. "Character change" means the sound changes on purpose and must be opted in.

## Licence verification

Licences were checked against the GitHub API (`gh api repos/<o>/<r>`), the repo `LICENSE` file, or the per-module header. Rules: only permissive code is adoptable (MIT, BSD, Apache-2.0, ISC, Zlib, BSL-1.0). GPL/LGPL code is read-only for ideas.

| Repo | Licence (verified how) | Adoptable |
| --- | --- | --- |
| Signalsmith-Audio/dsp | MIT (API) | yes |
| Signalsmith-Audio/signalsmith-stretch | MIT (API) | yes |
| airwindows/airwindows | MIT (API) | yes |
| HEnquist/rubato | MIT / Apache-2.0 dual (`LICENSE-MIT`, `LICENSE-APACHE` present; API says NOASSERTION) | yes |
| RustAudio/dasp | MIT (`LICENSE-MIT`) | yes |
| SamiPerttu/fundsp | Apache-2.0 (API) | yes |
| electro-smith/DaisySP | MIT (`LICENSE` text) | yes |
| thestk/stk | STK-4.3 licence, MIT-like (`licenses/stk-4.3.0.md` in faustlibraries; read full text before copying) | yes, after reading |
| pichenettes/stmlib | MIT (`LICENSE` text) | yes |
| pichenettes/eurorack | MIT for STM32F code (README) | yes, per file, check |
| HiFi-LoFi/FFTConvolver | MIT (API) | yes |
| libsndfile/libsamplerate | BSD-2-Clause (API) | yes |
| sfztools/sfizz | BSD-2-Clause (API, archived) | yes |
| shibatch/sleef | BSL-1.0 (API) | yes |
| Chowdhury-DSP/chowdsp_wdf | BSD-3-Clause (API) | yes |
| Chowdhury-DSP/chowdsp_utils, `modules/dsp/*` | **GPLv3** per module header (`license: GPLv3` in chowdsp_eq, _compressor, _reverb, _waveshapers, _filters). The repo-level LICENSE.md defers to per-module licences | **no**, read only |
| grame-cncm/faustlibraries | Per-function licence declared in each `.lib` (filters.lib says so explicitly) | per function, read first |
| surge-synthesizer/surge, mtytel/vital, DISTRHO/Cardinal | GPL-3.0 (API) | read only |
| breakfastquay/rubberband | GPL-2.0 (API) plus commercial | read only |
| csound/csound | LGPL-2.1 (API) | read only |
| SoundTouch | repo not found under the names tried; LGPL per upstream, not re-verified | read only |
| Elastique (zplane) | proprietary | no |

Not verified in this lane: Vicanek, Huovilainen, Simper, Zavalishin, Parker/ADAA, Giannoulis/GMR, Jot FDN. These are papers and books, so the algorithms are adoptable as ideas and must be re-implemented from the equations, not from any code that comes with them.

## Existing module map (crates/sampler-core/src/dsp)

| Class | Our module(s) | Kontakt parity state (PARITY_MATRIX) |
| --- | --- | --- |
| Ladder | `filter/ladder.rs`, `ladder_kernel.rs` | bit-exact steady, 2e-7 moving, HQ flag matched |
| SVF / TPT | `filter/svf.rs`, `tpt_svf.rs`, `chamberlin.rs` | parity to capture and kemu |
| Biquad / EQ | `biquad.rs`, `eq_family/{solid,generic,digital,legacy}.rs` | Solid G-EQ partial (bell at knob <= 0.5 matched) |
| Formant / Vowel / Phaser | `filter/formant.rs`, `phaser.rs` | parity (kemu) |
| Daft / Monark / Versatile / PRO-53 | `filter/daft.rs`, `filter/cascade.rs` | parity (kernel or kemu) |
| Compressor / limiter / gate | `dynamics/*`, `db_limiter.rs`, `gater.rs` | partial, no native vectors |
| Saturation / waveshaping | `color/{saturator,shaping,tape,tubescreamer,...}`, `shaper.rs` | partial |
| Oversampling | `hiir.rs`, `kaiser.rs`, `color/hilbert.rs` | HIIR matches UVI design |
| Reverb | `space/network.rs`, `space/convolver.rs`, `space/resample.rs` | Plate partial; Convolution decomp only |
| Delay / modulation | `timemod/*`, `replika/*`, `ringmod.rs` | partial, several missing |
| Time-stretch / pitch | `timemod/tape.rs`, WSOLA stand-in for Time Machine Pro | partial, Tone Machine missing |
| SRC / interpolation | `space/resample.rs`, `kernels.rs` | not yet compared to a Kontakt capture |

## Table per DSP class

Expected CPU is a reasoned estimate from algorithm structure, **not measured**. Adopt numbers only after a bench.

### 1. Filters and EQ

| Candidate | Licence | Quality claim | Algorithm | Expected CPU | Maps to | Molding to Kontakt |
| --- | --- | --- | --- | --- | --- | --- |
| Signalsmith `BiquadDesign` (`filters.h`, Vicanek matched design) | MIT | Matches the analog bell/shelf magnitude at high frequency better than bilinear RBJ (the file cites Vicanek) | Biquad coefficients from frequency, Q, gain with matched HF response | ~ biquad, 1 mul-add chain per sample | `biquad.rs`, `eq_family/solid.rs` bell sections | **Drop-in** for the coefficient designer. Same bell shape, cleaner HF. Fit: keep Kontakt's gain/Q law, replace only the designer. |
| Cytomic / Simper TPT SVF (paper) | idea only | Stable under fast modulation (zero-delay feedback) | Trapezoidal integrators, LP/HP/BP/notch mix | ~ SVF | `tpt_svf.rs`, `filter/svf.rs` | **Drop-in**: we already have it. The shelf/bell from its mixing coefficients (`A`, `m0..m2`) is the molding path for Solid G-EQ shelves. |
| Zavalishin TPT ladder (book, CC BY-NC-SA: read only, not copy) | idea only | Zero-delay ladder with the same sweep character | TPT one-pole cascade with feedback | ~ 4 TPT stages + tanh | `filter/ladder.rs`, `ladder_kernel.rs` | **Drop-in** only if the topology matches Kontakt's ladder. Our ladder is already bit-exact to kemu, so use it only as a numerics cross-check. |
| Huovilainen nonlinear ladder (paper) | idea only | Corrects the cutoff shift of the naive Moog ladder | Delay-free solve with tanh and half-sample correction | ~ 4 stages + Newton iteration | `ladder.rs` HQ path | **Character change**: the correction moves cutoff, so gate it behind Kontakt's HQ flag, which we already emulate. |
| DaisySP `Svf`, `Ladder`, `Biquad` (MIT) | MIT | Clean, small reference kernels | SVF / Huovilainen-style ladder | ~ same | `svf.rs` cross-check | Reference only. Good for an independent golden test. |
| chowdsp_wdf (BSD-3) | BSD-3 | Wave-digital ladder and diode-clipper models with circuit-accurate tone | WDF tree per circuit | ~ 2-4x SVF per voice | `tone.rs`, `tonestack.rs`, `filter/ar.rs` (AR) | **Character change**: mold by fitting component values to the Kontakt curve. Only for analog-modelled Kontakt products. |
| Faust `filters.lib` (per-function licence) | per function | Large reference set | Various | varies | cross-check | Read first, adopt per function. |

### 2. Dynamics (compressor, limiter, gate, transient shaper)

| Candidate | Licence | Quality claim | Algorithm | Expected CPU | Maps to | Molding to Kontakt |
| --- | --- | --- | --- | --- | --- | --- |
| Giannoulis, Massberg, Reiss, "Digital dynamic range compressor design" (JAES 2012) | idea only | Standard reference for a generic static curve with knee, plus ballistics | Smooth gain computer (soft knee), peak/RMS detector, attack/release one-pole | ~ cheap, one log/exp per block for the detector | `dynamics/computer.rs`, `dynamics/ballistics.rs`, `dynamics/detector.rs` | **Molded**: a single model with threshold, ratio, knee, attack, release. Fit the knee and ratio from the measured Kontakt gain curve, and the ballistics from a step response. This is the best fit for the Enhanced/Pro/Classic and Feedback compressors (all currently partial, no vectors). |
| Airwindows `Pressure`, `Console`, `Compressor` (MIT) | MIT | Character reference, not measured parity | Simple detector and gain law | very low | `dynamics/*` (character option) | **Character change**. Use only as an opt-in "vintage" mode. |
| Signalsmith `envelopes.h` `BoxFilter`, `BoxSum` (MIT) | MIT | Fixed-cost moving average | Sliding-window sum | low | RMS detector in `dynamics/detector.rs` | **Drop-in** for an RMS detector, if Kontakt's detector is windowed. Fit the window length to the measured knee. |
| fundsp (Apache-2.0) `limiter`, dynamic nodes | Apache-2.0 | Rust-native, simple | Lookahead peak limiter | moderate | `db_limiter.rs`, Limiter | Reference for a Rust lookahead design. Limiter has no native vector yet. |
| Transient shaper | no clean OSS model found in this lane | - | fast/slow envelope difference | low | Transient Master (partial) | Build from the GMR detector with two time constants. Fit the control law to Transient Master decompile. |

### 3. Saturation, waveshaping and anti-aliasing

| Candidate | Licence | Quality claim | Algorithm | Expected CPU | Maps to | Molding to Kontakt |
| --- | --- | --- | --- | --- | --- | --- |
| Parker, Esqueda, Bilbao (DAFx 2016): ADAA for waveshapers | idea only | Cuts aliasing without oversampling, for any static curve f(x) | First-order antiderivative F1 of f, output (F1(x_n) - F1(x_{n-1})) / (x_n - x_{n-1}) with a fallback near equal inputs | ~ 1 extra f and F1 eval per sample | `color/shaping.rs`, `color/saturator.rs`, `shaper.rs` | **Molded**: take any measured Kontakt transfer curve (Saturator, Tape, Tube, Drive) and apply ADAA. Our existing curves stay the same, only the aliasing changes. |
| Signalsmith `Oversampler2xFIR`, `kaiser.rs`-style windowed sinc (`rates.h`, MIT) | MIT | Linear-phase FIR 2x oversampler | Kaiser-windowed FIR up and down | ~ 2x the nonlinear cost plus FIR taps | `hiir.rs`, `kaiser.rs` (HQ path) | **Drop-in** for an HQ mode. Latency is the same class as our HIIR, and it stays linear phase. |
| sleef (BSL-1.0) | BSL-1.0 | Vectorised math (tanh, exp, log), accurate to a few ULP | SIMD polynomial kernels | fast | saturators, dynamics log/exp | **Drop-in** for transcendental calls. Sits behind the existing SIMD crate (`sampler-simd`). |
| chowdsp_waveshapers (GPLv3) | GPL | - | - | - | read only | do not adopt. |

### 4. Reverb

| Candidate | Licence | Quality claim | Algorithm | Expected CPU | Maps to | Molding to Kontakt |
| --- | --- | --- | --- | --- | --- | --- |
| Jot FDN with Hadamard mixing (Jot & Chaigne 1991, paper) | idea only | Tunable density, decay per band, and colour | Delay lines + orthogonal feedback matrix + per-band T60 filters | ~ N delay reads + N x N mix (Hadamard is N log N) | `space/network.rs` (Galois/Raum) | **Molded**: per-band T60 shelves fit to the measured Kontakt decay curve; density set by delay-line count and lengths. Replaces per-product reverb cores with one network. Character change where density differs, so keep the measured-density case as the parity target. |
| Dattorro plate (paper) | idea only | Known plate topology | Input diffusion + figure-of-eight tank | low | Plate Reverb (partial) | Already close in our Plate core. Use as a cross-check, not a rewrite. |
| FFTConvolver (MIT) | MIT | Uniform partitioned convolution, zero latency after first partition | Partitioned FFT overlap-add | ~ FFT per partition, scales with IR length | `space/convolver.rs` | **Drop-in** for Convolution (currently decomp only). The IR size path still needs the Elastique stand-in, which is out of scope here. |
| Airwindows `Verbity`, `Galactic`-class (MIT) | MIT | Character reference | - | low | Reverb character option | Character change only. |

### 5. Delay, chorus, phaser, flanger, modulation

| Candidate | Licence | Quality claim | Algorithm | Expected CPU | Maps to | Molding to Kontakt |
| --- | --- | --- | --- | --- | --- | --- |
| Signalsmith `delay.h` fractional buffer and `CubicLfo` (`envelopes.h`, MIT) | MIT | Clean fractional-read interpolation; random-phase LFO with smoothing | Hermite / cubic read, smoothed rate | low | `timemod/*`, Legacy Chorus/Flanger/Delay, Vibrato/Chorus | **Molded**: the same delay line and LFO serve chorus, flanger, and Legacy Delay. Fit depth and rate law to the Kontakt measurement. Fills the missing Vibrato/Chorus row without a new per-effect DSP. |
| DaisySP `Chorus`, `Flanger`, `Phaser`, `Delay` (MIT) | MIT | Reference modulation kit | Standard delay-line modulators | low | cross-check | Reference only. |
| Phaser allpass chain (general design) | idea | Standard | Allpass cascade with feedback | low | `phaser.rs` (Kontakt phaser is kemu-parity) | Already matched. Keep. |

### 6. Pitch, time-stretch, sample-rate conversion

| Candidate | Licence | Quality claim | Algorithm | Expected CPU | Maps to | Molding to Kontakt |
| --- | --- | --- | --- | --- | --- | --- |
| Signalsmith Stretch (MIT) | MIT | Author's README: best for 0.75x to 1.5x stretch, handles multi-octave pitch shift; presets `presetDefault` and `presetCheaper` | Phase vocoder with the ADC22 "four ways" final approach | moderate (FFT per hop) | `timemod/tape.rs`, Time Machine Pro (WSOLA stand-in), Time Machine 2 (TD-PSOLA) | **Molded**: a candidate replacement engine for the Time Machine family. The presets map to the Kontakt quality knobs. Speed law and grain size are the fit parameters. |
| rubato (MIT/Apache) | MIT / Apache-2.0 | Rust-native sinc resampler, async and sync variants | Windowed sinc with polynomial interpolation | per-sample sinc taps | `space/resample.rs`, `kernels.rs`, interpolation for sample playback | **Molded**: window and cutoff parameters are the fit knobs. Fit against a measured Kontakt interpolation capture (not yet captured). |
| libsamplerate (BSD-2) | BSD-2 | Reference SRC with selectable quality | Sinc best / medium / fast, ZOH and linear | per quality | cross-check for `space/resample.rs` | Reference and golden test generator. |
| sfizz (BSD-2) | BSD-2, archived | Reference SFZ sampler; interpolation options | linear / cubic / Hermite / sinc | low | sample voice interpolation | Reference for the voice path. Archived, so no maintenance. |
| Rubber Band (GPL-2 + commercial), SoundTouch (LGPL), Elastique (proprietary) | no | - | - | - | read only | Do not adopt. Read Rubber Band's R3 for ideas only. |

## Ranked shortlist (benchmark against ours, 12 items)

1. **Signalsmith `BiquadDesign` (MIT)**: molded EQ bell/shelf designer. Biggest, cheapest win for Solid G-EQ and EQ bands. Drop-in.
2. **Giannoulis-Massberg-Reiss generic compressor (idea)**: single molded model for Compressor Enhanced/Pro/Classic and Feedback. Fit to Kontakt static curve and step response. Highest value, since dynamics have no native vectors.
3. **ADAA over measured Kontakt curves (idea)**: anti-aliased saturator/tape/tube/drive, same curves. Drop-in in shape, cleaner aliasing.
4. **Signalsmith `Oversampler2xFIR` and FIR designer (MIT)**: HQ path for nonlinear blocks and the Ladder HQ flag. Linear-phase alternative to HIIR.
5. **Signalsmith Stretch (MIT)**: candidate engine for Time Machine family (replaces WSOLA stand-in). Benchmark quality and CPU vs our current path.
6. **rubato (MIT/Apache)**: sinc SRC with fit-able window and cutoff, for sample playback and resampling. Benchmark against a Kontakt interpolation capture.
7. **Jot FDN with Hadamard (idea)**: one molded reverb network with per-band T60 fit, for Raum / Reverb / Galois.
8. **FFTConvolver (MIT)**: Convolution parity path with partitioned FFT.
9. **Signalsmith `delay.h` + `CubicLfo` (MIT)**: shared delay and LFO for chorus, flanger, Legacy Delay, Vibrato. Fills the Vibrato/Chorus gap.
10. **Cytomic/Simper SVF and TPT ladder (idea)**: cross-check for our SVF and ladder numerics (already bit-exact to kemu; use as an independent golden).
11. **sleef (BSL-1.0)**: SIMD transcendentals for saturators and dynamics, behind `sampler-simd`.
12. **chowdsp_wdf (BSD-3)**: WDF ladder and diode models for analog-modelled Kontakt products. Character change, so opt-in only.

Read-only (GPL/LGPL, ideas only): chowdsp_utils DSP modules (GPLv3), Rubber Band, Surge XT, Vital, Cardinal, Csound (LGPL-2.1).

## Notes

- Several Kontakt filters (Daft, Monark, Versatile, PRO-53, Formant, Ladder) already have bit-level or kemu-level parity. For those, the modern upgrade is numerics only (ADAA, oversampling, TPT), and it must be gated behind an opt-in so the default stays the measured response.
- CPU figures are structural estimates, not measurements. Benchmark each shortlist item with `sampler-perf` before adopting.
- Licences for papers and books are not verified as code; only algorithms are used, re-implemented from the equations.
