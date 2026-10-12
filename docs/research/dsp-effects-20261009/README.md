# DSP catalogue verification ledger

All five native suites below were replayed successfully on **2026-10-09**. Together they contain **5,039 bounded cases**, including zero-length inputs, setter checks and audio blocks. This total is not a count of effects or a measure of whole-effect coverage. The catalogue audit also passes.

The [catalogue](../../DSP_EFFECT_CATALOGUE.md), [Kontakt list](../../KONTAKT_DSP_EFFECTS.md) and [Workstation list](../../WORKSTATION_DSP_EFFECTS.md) describe each pinned entry, its processing contract and proof. The [manifest](catalogue-evidence.json) identifies all 239 entries and the hashes of their source inputs. Ten Kontakt entries have no selected native association: the nine AR LP/HP/BP variants and AET Filter.

## Replayed native evidence

| Suite | Passing cases | Established scope | Unverified scope |
| --- | ---: | --- | --- |
| [Kontakt](../../../artifacts/engine-analysis-2026-10-07/deep-dsp/verify_machine_code.py) | 70 | Gainer, Daft/Ladder selectors, control mapping/cadence and one compressor detector path | Some helpers substituted; complete filters, gain laws and host audio |
| [Workstation shaping/decimation](../../../artifacts/engine-analysis-2026-10-07/uvi-workstation/verify_shape_decimation.py) | 3,243 | 480 rectifier, 495 fractional-decimator and 2,268 BitCrusher blocks; exact checked output/state | Other shapes, public conversions and complete processor pipelines |
| [Workstation OnePole](../../../artifacts/engine-analysis-2026-10-07/uvi-workstation/verify_onepole_code.py) | 490 | Ten setters and 480 sample blocks; exact PCM/history | Cutoff conversion and complete host behaviour |
| [Workstation LFO](../../../artifacts/engine-analysis-2026-10-07/uvi-workstation/verify_lfo_code.py) | 204 | Twelve setters and 192 control-buffer blocks | Factory waveforms, authored table provenance and every effect's modulation routing |
| [Workstation Dispersor/Robotizer](verify_specialized_kernels.py) | 1,032 | 168 Dispersor and 864 Robotizer blocks; exact checked PCM/history/guards | Public mappings, factory tables, other branches and full host restoration |

All four Workstation suites execute original instructions without helper substitutions, using authored state/coefficients/tables where documented. Kontakt substitutions are recorded in its [results](../../../artifacts/engine-analysis-2026-10-07/deep-dsp/machine-code-verification.json). None of these suites establishes complete host audio parity or verifies KONTRA's output against either product.

## Replay commands and prerequisites

Run from the repository root in the existing research Python environment:

```bash
python3 docs/research/dsp-effects-20261009/verify_catalogue.py
python3 docs/research/dsp-effects-20261009/verify_specialized_kernels.py
python3 artifacts/engine-analysis-2026-10-07/deep-dsp/verify_machine_code.py
python3 artifacts/engine-analysis-2026-10-07/uvi-workstation/verify_shape_decimation.py
python3 artifacts/engine-analysis-2026-10-07/uvi-workstation/verify_onepole_code.py
python3 artifacts/engine-analysis-2026-10-07/uvi-workstation/verify_lfo_code.py
```

These commands require the retained `artifacts/engine-analysis-2026-10-07/` corpus, pinned native images/cache and installed emulator dependencies. The corpus and native images are separate local prerequisites; a clean checkout does not contain them. The suites launch neither application nor a development server. They do not run Cargo or change its build configuration.

`verify_catalogue.py` checks source hashes, manifest/reference coverage, exact per-effect document headings, native record correspondence, decompiler markers, parameter rows, saved XML tags, local links and retained result identities/counts. It does not execute DSP. The other five commands perform the native-byte replays.

## Live UVI interface cross-check

The current [UVI shared API reference](https://lua.uvi.net/_elements.html) was fetched separately on 2026-10-09. It lists **125 FX/Legacy types**, compared with the **115** matched by this catalogue to retained Workstation registry candidates. All 115 catalogue anchors are present. The [audit record](live-uvi-interface-audit.json) pins the additional capture's SHA-256 and unmatched names.

These ten public names have no exact internal-name match among the retained 176 registration records:

| Public name | Internal type |
| --- | --- |
| [Analog BBD Chorus](https://lua.uvi.net/_elements.html#ChorusBBD) | `ChorusBBD` |
| [Analog BBD Delay](https://lua.uvi.net/_elements.html#DelayBBD) | `DelayBBD` |
| [FET Compressor](https://lua.uvi.net/_elements.html#FETCompressor) | `FETCompressor` |
| [Feedforward Compressor](https://lua.uvi.net/_elements.html#FeedforwardCompressor) | `FeedforwardCompressor` |
| [LPG](https://lua.uvi.net/_elements.html#LowpassGateFX) | `LowpassGateFX` |
| [Loudness Meter](https://lua.uvi.net/_elements.html#LoudnessMeter) | `LoudnessMeter` |
| [Passive EQ](https://lua.uvi.net/_elements.html#Pultec) | `Pultec` |
| [Spectrogram](https://lua.uvi.net/_elements.html#SpectrogramAnalyzer) | `SpectrogramAnalyzer` |
| [Steiner Filter](https://lua.uvi.net/_elements.html#Steiner) | `Steiner` |
| [Wavefolder](https://lua.uvi.net/_elements.html#WavefolderFX) | `WavefolderFX` |

This is a version/association gap, not proof that Workstation cannot instantiate them through another path. The shared API alone does not identify their installed implementation. No Falcon binary was used.

## Compatibility boundary

For unverified effects, the catalogue supplies a separate identity, controls, a processing contract and native/source inspection locations. It does not supply recovered equations for every effect. Parameter conversion, channel/scope routing, reset/bypass, automation, resampling/oversampling, latency and complete reference-host output still need effect-specific verification. Use the marked unknowns when planning reconstruction; a generic approximation is not established compatibility.
