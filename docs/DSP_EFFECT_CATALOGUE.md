# Kontakt and Workstation: DSP effects, processing and proof

Checked **2026-10-09** against the retained native-code corpus and primary interface references. The catalogue has **239 entries**: Kontakt's **65 effects and 59 filters**, and **115 Workstation FX/Legacy registry candidates**. These are reference/registration entries, not 239 independently verified algorithms. Workstation's count includes analysers and containers; compiled registration does not establish UI availability. Falcon is absent.

| Catalogue | Contents | Proof retained |
| --- | --- | --- |
| [Kontakt](KONTAKT_DSP_EFFECTS.md) | Every effect/filter reference entry, its processing contract and observed control labels | Primary source anchor; native type/table candidate for 114 of 124 entries; explicit gaps for the remaining ten |
| [Workstation](WORKSTATION_DSP_EFFECTS.md) | All 83 FX and 32 Legacy public types matched to local registration candidates | Registration→callback→constructor, candidate owner dispatch, 1,257 typed parameter rows, saved-tag evidence |
| [Evidence manifest](research/dsp-effects-20261009/catalogue-evidence.json) | One record per entry, source hashes, binary identities, native addresses and decompiler locations | All 49 observed audio-processor XML schemas, plus a separate index of seven wrapper/metadata/event/script tags |
| [New original-byte checks](research/dsp-effects-20261009/verify_specialized_kernels.py) | Dispersor and Robotizer equations checked against the pinned loaded instructions | [1,032 passing blocks and explicit verification limits](research/dsp-effects-20261009/specialized-kernel-results.json) |
| [Verification ledger](research/dsp-effects-20261009/README.md) | Commands, results and source-version boundaries | All five native suites replayed successfully: **5,039 bounded cases**; ten additional live UVI API names kept separate from installed-native candidates |

Each catalogue entry states what processing must be represented, how far its implementation has been established, and where to inspect the proof. The descriptions are engineering interpretations of effect/control identities unless an executed kernel or a specific static observation is identified. A name such as “tape”, “ladder” or “tube” does not establish a transfer function, circuit or oversampling algorithm. Vendor descriptive prose is not reproduced.

**L** means interface evidence with the native association unresolved. **C** means native code/type/table evidence for a candidate association. **K** means the stated bounded kernel/control check passes against original instructions. These labels distinguish evidence; no entry is certified as a complete effect or a host-level audio match.

## Specialized processing that the compatibility model needs

The per-effect catalogues retain separate identities and controls. The following distinctions explain why a small collection of generic effects cannot represent the whole inventory.

| Processing requirement | Kontakt examples | Workstation examples | State/behaviour to preserve |
| --- | --- | --- | --- |
| Buffered slicing, grains, transposition and stretching | Beat Masher, Beat Slicer, Reverse Grain, Transpose Stretch | BeatRepeat, Granulizer, Harmonizer, Shifter | Capture/retrigger boundaries, grain windows/overlap, independent pitch/time, feedback and tempo |
| Moving formants and spectral morphing | Formant I/II, Vowel A/B, AET Filter | FormantCrusher, VowelFilter, Vowels, TalkBox | Multiple resonances, coefficient morphs, spectral analysis resources and filter history |
| Carrier/sideband processing | Freak, Ring Modulator | FreqShifter, RingModulator, Robotizer | Carrier phase, amplitude polarity, phase modulation, channel relationships and filters |
| Phase dispersion and tuned resonances | Phasis, Flair, filter Phaser | Dispersor, Phasor, HarmonicResonators | All-pass history, notch/resonance layout, feedback and coupling |
| Rotating-speaker/acoustic models | Rotator | Rotary2, Rotary, OneWayRotarySpeaker, TwoWayRotarySpeaker | Horn/drum motion, acceleration/braking, crossover, geometry and reflections |
| Nonlinear multi-stage models | Amp/stomp families, Dirt, Kolor, Supercharger GT | JCMAmp, ToneStack, DiodeClipper, TSDrive, LA2A | Stage order, transfer curves, calibration, detector/feedback state and oversampling |
| Quantization and unstable media | Bite, Lo-Fi, Tape Saturator, Wow/Flutter | BitCrusher, Redux/Redux2, Vinyl, TapeEcho | Quantizer boundaries, sampling phase, noise/jitter seeds, transport modulation and wear |
| Envelope shaping and model-specific dynamics | Transient Master, Solid Bus Comp, Transparent Limiter | FeedbackCompressor, Maximizer, three-band dynamics | Transient/sustain envelopes, detector linking, lookahead and independent band state |
| Composite graphs and multichannel routing | Send Levels, Surround Panner, Twin Delay | EffectRack, FeedbackMachine, UVIMastering, GainMatrix, DualDelayX | Ordered/parallel nodes, loop scheduling, channel matrices, send taps and processing scope |

These are implementation requirements inferred from the interfaces and observed structures. Their presence does not certify that any current KONTRA approximation implements them. Preserve original type/version, controls, routing, resources and unsupported metadata until a corresponding processor is established.

Coverage follows the hashed reference captures and the retained native registry. The current [UVI shared interface](https://lua.uvi.net/_elements.html) has ten further FX names with no match in those registry records; the [source audit](research/dsp-effects-20261009/README.md#live-uvi-interface-cross-check) records them without attributing their DSP to Workstation 4.0.9. All 115 catalogue source anchors still exist on the live page.

## Binary identities and evidence boundaries

| Native image | SHA-256 | Address basis |
| --- | --- | --- |
| Kontakt 8.13.1 standalone | `0fe6356e0879d058b6e5b73507c54c5e345cea451b35287c974e438291d4dae8` | Image base `0x140000000` |
| Workstation 4.0.9 standalone | `78729e96b752aea746280275072ad24cb4399a053739c49a161ff1fcfbf85721` | Image base `0x140000000` |
| Workstation retained loaded `.text` | `684a5557efed9e426a62cbba75c15cc727d764680e1660164b7c5ae88698fda1` | Overlaid in an isolated CPU emulator for K checks |

The installed VST3s are separate images. Kontakt's small portable VST3 wrapper is also separate from its `Kontakt 8.vst3plugin` audio payload. None of the addresses here can be reused in another image just by changing the base. See the existing [plugin topology and format specification](DSP_FORMAT_SPECIFICATION.md).

Raw decompiler exports, static disassembly, source captures and the loaded-text snapshot are local research prerequisites under `artifacts/engine-analysis-2026-10-07/` and the pinned cache. They are not embedded in the tracked manifest. The manifest is an evidence index; a clean checkout alone cannot execute native-byte checks. Decompiler warnings are retained in each address record, and a completed decompilation is not proof that its inferred types or operations are correct.

## Verified processing details

`f32(v)` below means rounding to IEEE float32 after that operation. Native rounding order can change output bits even when equations are mathematically equivalent. All sample tests use authored state/input unless otherwise stated; they compare original instructions with reference equations, not with KONTRA's output.

### Workstation Dispersor — new check

The selected static chain is registration `0x140e7d8ea` → callback `0x140e854c0` → constructor `0x1412e8130`; owner dispatch candidate `0x1412e8750` calls kernel `0x1412ea230` on the non-stereo path of one internal branch. The [compiled structure](../artifacts/engine-analysis-2026-10-07/uvi-workstation/compiled-module-structure.json) pins the constructor/table chain. The call is in [pointer-export pseudocode](../artifacts/engine-analysis-2026-10-07/uvi-workstation/pointer-export/pseudocode.c), line 171309; the kernel's export is in [complete pseudocode](../artifacts/engine-analysis-2026-10-07/uvi-workstation/complete-export/pseudocode.c), around line 6897185.

The isolated stage array contains one float32 coefficient at `+0`, stage histories starting at `+4`, and a **u32 stage count at `+0x104`**. The decompiler misleadingly assigns a float type to the count. Original-byte execution confirms the integer input used by the check.

```text
for each sample:
    for each stage i:
        y = f32(f32(a*x) + state[i])
        state[i] = f32(x - f32(a*y))
        x = y
    output = x
```

Ignoring rounding, one stage has `H(z) = (a + z^-1)/(1 + a*z^-1)`: an all-pass section for real stable `a`. Cascading the stages changes phase/time dispersion while preserving the ideal magnitude response. This is a concrete processing equation, not a reverb/delay classification.

**168 consecutive-block cases pass**, covering stage counts 0/1/3/4/8/16/64, coefficients −0.75/0/0.75 and frame lengths 0/1/3/4/17/32/33/129. Output PCM, stage history, count and output guards match exactly. There are no helper substitutions. Public `Freq`, `Spread`, `Stages` conversion, other internal branches, stereo dispatch, reset and full host restoration remain unresolved. Tested stage counts do not establish public bounds.

### Workstation Robotizer — new check

Static chain: registration `0x140e7dd90` → callback `0x140e87f60` → constructor `0x141306b30` → owner dispatch `0x141307d20`. The [complete export](../artifacts/engine-analysis-2026-10-07/uvi-workstation/complete-export/pseudocode.c), line 6923783, exposes the whole checked kernel without helper calls. Exact registration identities are also retained in the tracked manifest.

The kernel reads two 257-entry float tables through state `+0x38` and `+0x58`. For a u32 phase `p`, lookup uses `i=p>>24`, `t=f32((p & 0xffffff)*2^-24)` and `L(p)=f32(f32(f32(1-t)*table[i])+f32(t*table[i+1]))`. The boundary entry at index 256 is necessary.

```text
carrier = f32(f32(L_carrier(u32(p + offset)) + 1) * 0.5)
wet = f32(carrier * input)
wet = selected_biquad(wet) or wet
output = f32(f32(f32(1-mix)*input) + f32(wet*mix))
p = u32(p + carrier_increment)
offset = u32(trunc(f32(f32(f32(L_mod(q + q_offset)+1)*depth)*2^31)))
q = u32(q + mod_increment)
```

The second table supplies a **phase offset** to the carrier on the following sample. The interface calls its control `FM`; the kernel evidence establishes this offset recurrence, without establishing the public control-to-depth/increment conversion. For authored bipolar tables the carrier gain is unipolar, so replacing it with direct bipolar ring multiplication would differ.

State locations: depth `+0x20`, mix `+0x24`, filter pointer `+0x28`, carrier phase/increment/offset `+0x4c/+0x50/+0x54`, and modulation phase/increment/offset `+0x6c/+0x70/+0x74`. Carrier/modulation state advances once per frame across all channels. Filter-state mode `+0x1e0` selects bypass or one of two independent biquad history regions. The check authors stable coefficients rather than claiming a recovered cutoff conversion.

**864 consecutive-block cases pass**, covering two authored carrier tables, 1/2/12 channels, two depths, three mixes, both filter branches and bypass, and frame lengths 0/1/3/16/17/32/33/129. PCM, both phases, carrier offset, selected filter histories and guards match exactly, with no helper substitutions. Tests use modulation phase offset zero. Factory tables, parameter mappings, exceptional inputs, reset, bypass and live automation remain unresolved.

Proof for both new checks: [runnable script](research/dsp-effects-20261009/verify_specialized_kernels.py) and [pinned results](research/dsp-effects-20261009/specialized-kernel-results.json).

### Workstation FormantCrusher

Native processing `0x1412d4000` has a prefilter→fractional decimator→wet-mix path. Isolated decimator `0x1412d43c0` keeps a **double fractional period/phase** alongside float32 held/current/ramp values:

```text
if phase <= 0:
    held = input
    delta = f32(f32(held-current) / f32(period))
    phase += period                         # float64
old = current
current = f32(current + delta)
output = f32(f32(f32(held-old)*blend) + old)
phase -= 1                                  # float64
```

**495 blocks** match output and state. Rounding the period to an integer changes capture timing. Public morph/formant/Q/bite controls, preceding filter and full mixing are outside this proof. [Equations and scope](DSP_FORMAT_SPECIFICATION.md#workstation-formant-crusher-fractional-decimation-established-by-original-bytes), [original-byte check](../artifacts/engine-analysis-2026-10-07/uvi-workstation/verify_shape_decimation.py).

### Workstation BitCrusher / UVI Destructor

Kernel `0x1412d1770` implements sample holding, quantization, rational saturation, optional biquad and wet/dry mixing. Its phase restarts at zero **per channel per processing call**, while held samples persist. The quantizer uses `floor(t)+(t<0)` before truncation: with exponent 2 and zero drive, held `−0.5` yields `−0.25`. A conventional truncating quantizer or persistent decimation phase would differ. Zero wet mix still advances internal history.

**2,268 blocks** match PCM/held/filter state with no substitutions, across channel counts, quantization exponents, rates, drives, mixes, filter branches and block boundaries. Public conversion/filter design and exceptional quantizer values remain unresolved. [Complete established equations](DSP_FORMAT_SPECIFICATION.md#workstation-bitcrusher-processing-established-by-original-bytes), [original-byte check](../artifacts/engine-analysis-2026-10-07/uvi-workstation/verify_shape_decimation.py), [results](../artifacts/engine-analysis-2026-10-07/uvi-workstation/shape-decimation-machine-code-verification.json).

### Workstation WaveShaper

Internal dispatch `0x141271fd0` selects full rectification at mode 6 and half at mode 7. Four constant/changing-gain branch kernels are checked. Full rectification clears the sign bit, including for NaNs. Half rectification uses SSE maximum with positive zero as the second operand, producing positive zero for unordered NaNs and either signed zero. Replacing this with a high-level comparison may change exceptional values.

**480 blocks** pass, including SIMD boundaries, signed zero, subnormals, infinities and NaN payloads. Other shapes, public mode mapping, gain/filter/mix and oversampling remain unresolved. [Kernel details](DSP_FORMAT_SPECIFICATION.md#workstation-waveshaper-rectifier-kernels-established-by-original-bytes), [original-byte check](../artifacts/engine-analysis-2026-10-07/uvi-workstation/verify_shape_decimation.py).

### Workstation OnePole

General-channel kernel `0x1412b3e30` uses `b=f32(1-a)` and `y=f32(y+f32(f32(x-y)*b))`; low-pass emits `y`, high-pass emits `f32(x-y)`. Stereo kernel `0x1412b40d0` uses interleaved recurrences with different float32 operation order and separate history areas.

**480 audio blocks plus ten setter cases** pass. Frequency/key-tracking updates mark coefficients dirty; mode switching does not. Exact cutoff conversion depends on a runtime multiplier outside the retained `.text`; the checks author `a`. Layout transitions and complete host parity remain unresolved. [State layout and stereo equations](DSP_FORMAT_SPECIFICATION.md#workstation-onepole-sample-kernels-established-by-original-bytes), [original-byte check](../artifacts/engine-analysis-2026-10-07/uvi-workstation/verify_onepole_code.py), [results](../artifacts/engine-analysis-2026-10-07/uvi-workstation/onepole-machine-code-verification.json).

### Kontakt Gainer, Daft/Ladder control paths and Compressor detector

The existing original-byte pass establishes the following bounded behaviours. It does not certify complete effects.

| Module/path | Native address | Established detail | Limit |
| --- | --- | --- | --- |
| Gainer | `0x140aa2980` | `out=f32(input*gain)`, then `gain=f32(gain+f32(f32(target-gain)*k))`, `k=0.0005555555690079927` (`0x3a11a2b4`); channel blocks share initial gain and persistent gain advances once by frame count | Unity shortcut uses a substituted common mixing helper; full host output not checked |
| Daft controls | setter `0x140b05900`, cutoff `0x140b04c80` | Lookup `T[i]=f32(2^(i/60−20))`, linear interpolation; cutoff input position `f32(f32(x*625)+1481.881591796875)`, clamped 1..30000 Hz; resonance `1−(1−x)^2`; response switches at 0.5 | Floating-point power and some setup helpers substituted; full nonlinear audio unverified |
| Daft scheduling | `0x1408fa270` | 32-sample cadence persists across blocks; control clamping and quantized ramp setup checked | Complete resampling and host automation not checked |
| Daft subtype | `0x140af0170` | Internal values 68/69 select 0/1, default 0 | Internal selector values are not proven saved/public IDs |
| Ladder subtype | `0x140af01d0` | Internal values 22..33 select 0..11, default 0 | Whole filter audio and public response mapping unverified |
| Compressor detector | dispatcher `0x1409047a0`, variant 0 `0x140ab2250` | Linked detection takes the signed channel mean before absolute value; opposite-polarity channels cancel | Five detector cases; complete gain law and other variants not checked |

The retained pass contains **70 cases**, with substitutions explicitly listed where used. [Detailed scope](DSP_SYSTEM_INVENTORY.md#original-machine-code-dsp-checks), [runnable check](../artifacts/engine-analysis-2026-10-07/deep-dsp/verify_machine_code.py), [results](../artifacts/engine-analysis-2026-10-07/deep-dsp/machine-code-verification.json).

## Additional static processing observations

These explain specialized structures but are **C evidence**, not numerical verification.

| Processor | Native evidence | Observed structure | What is missing |
| --- | --- | --- | --- |
| TalkBox | `0x1413424d0`, complete export line 6974192 | Calls setup and preceding filter processing, then selects one of five 11-double coefficient blocks at `0x141ea1830 + mode*0x58`. It applies `y[n]=c0*x[n]+Σ(ci*y[n−i], i=1..10)` with ten double histories per channel | Runtime coefficient tables, mode/parameter restoration and original-byte audio checks |
| Granulizer | wrapper `0x1412ce110` → `0x1415928e0` → `0x141591770`; complete export lines 6875856/7749072/7750677 | Wrapper delegates to per-channel objects spaced `0xa80`; public period/density/jitter/direction/feedback controls identify required grain behaviours | Grain kernel reconstruction, windows, random state, scheduling and public laws |
| HarmonicResonators | `0x1412a3dd0`, complete export line 6839583 | Allocates six scratch buffers and a further working buffer; processes per-channel state at `state+0xc8+channel*0x3c0` through `0x1411b0ec0` | Coupling matrices, decay/tuning laws and sample-kernel checks |
| Vinyl | `0x14133f360` → `0x14133ba90`, complete export lines 6970666/6968531 | Multi-stage helper processing, piecewise shaping and scheduled event loops; matches the need for wear/noise/dust/scratch state | Table values, event/noise generation, exact stage meaning and host restoration |
| GainMatrix | `0x141338530`, [gain/routing topology](../artifacts/engine-analysis-2026-10-07/uvi-workstation/gain-routing-topology.json) | Per-input/output control handles; outputs are written to scratch before copy-back | Runtime dispatch pointers and full original-byte execution |
| UVIMastering | [saved resource schemas](research/dsp-effects-20261009/catalogue-evidence.json), `UVIMastering.children` | Composite contains Gain, CrossOver, Exciter, CompExp, SoftLimiter and BrickwallLimiter children | Exact nesting order/wiring per preset, SoftLimiter identity and complete audio |

The existing [Workstation LFO check](../artifacts/engine-analysis-2026-10-07/uvi-workstation/verify_lfo_code.py) additionally verifies **204 setter/control-buffer cases** with authored lookup tables, fixed-point phase, 32-frame points and lookahead. It is control infrastructure evidence, not verification of every modulated effect's waveform or scheduling. [LFO scope](DSP_FORMAT_SPECIFICATION.md#workstation-lfo-mechanics-established-by-original-bytes).

## Verification and remaining reconstruction work

Run the tracked catalogue checker:

```bash
python docs/research/dsp-effects-20261009/verify_catalogue.py
python docs/research/dsp-effects-20261009/verify_specialized_kernels.py
```

The first checks complete inventory coverage, source hashes, native record correspondence, C-location markers, parameter rows, saved tags and local Markdown links. The second executes the new pinned native-byte cases. The existing Kontakt, OnePole, shaping/decimation and LFO checks are separately linked above. These checks need the retained private corpus, cache and installed Python dependencies; no application/server is launched.

For each remaining C/L entry, reconstruction still needs the public/saved parameter→native-state mapping, exact kernel/coefficient/table values, channel/scope dispatch, state/reset/bypass, latency, resampling/oversampling, automation/block-boundary behaviour and reference-host output. Whole effects must be compared through the appropriate installed audio image before declaring compatibility. The per-effect lists retain unknowns explicitly, so a generic approximation cannot silently count as verified support.
