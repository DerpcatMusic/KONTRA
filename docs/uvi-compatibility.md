# UVI capabilities and validation evidence

Status reviewed 2026-10-03. KONTRA has a native, feature-gated UVI interoperability path for bounded archive inspection, protected-program decoding, graph preservation, multichannel sample loading and offline script/render experiments. The latest controlled paid-corpus audit rendered **40 of 40 programs** with their original scripts, producing finite, nonzero outputs. This is **not a claim that every Falcon program or the audited commercial libraries plays correctly**, and it does not add a complete Falcon instrument catalog or editor to the plugin.

The [format](FALCON_FORMAT_GROUNDWORK.md) and [runtime/UI](FALCON_RUNTIME_UI_GROUNDWORK.md) groundwork reports retain the earlier 2026-10-02 audit. Their then-unknown format/runtime observations must not be read as the current implementation status. [Observed UFS2 layout](uvi-format.md) documents the separately scoped structural inspector.

## Native ownership and boundaries

The `uvi` Cargo feature enables these modules. Archive, program, script and renderer identities remain native UVI objects rather than being flattened into Kontakt groups or translated to KSP.

| Module | Responsibility | Boundary |
| --- | --- | --- |
| [`ufs`](../src/uvi/ufs.rs) | Bounded UFS2 records, linked directory leaves, member names, exact byte spans and encryption modes | Index-node bodies remain opaque. A decoded filename does not establish playable content. |
| [`crypto`](../src/uvi/crypto.rs) | Offset-seeded name/content transforms, 512-byte content resets, PasswordV2 program wrappers and generic stream-state recovery | Reader namespaces and content state are local inputs. Unsupported legacy program protection fails explicitly. |
| [`program`](../src/uvi/program.rs) | Program/Layer/Keygroup/Oscillator graph, stable node IDs, attributes, connections and sampled zones | Preserves unknown modules; parsing does not execute them. XML limits and DTD rejection apply before execution. |
| [`library`](../src/uvi/library.rs) | Bank-relative resource paths, source-only Lua modules and shared sample identity | Rejects ambiguous resources and unresolved volume aliases; local decoding stays bounded. |
| [`sample`](../src/uvi/sample.rs) | WAV/AIFF/FLAC decoding, every source channel, RIFF sampler metadata and ordered mono bundles | Keeps inclusive loop endpoints, fractional positions, play counts and unity notes. Decoder success is separate from renderer support for those fields. |
| [`storage`](../src/uvi/storage.rs) | Resident PCM using the sampler's existing exact I16/I24 packing, with f32 fallback | Retains scalar channel order and source values; storage pairs are not stereo downmixing. |
| [`script`](../src/uvi/script.rs) | Lua 5.1 callbacks, cooperative scheduling, note identity and timed command output | Allocates offline; it must not run in the plugin audio callback. |
| [`host`](../src/uvi/host.rs) | UVI object identity, typed parameters, widget state, approved module loading and resource requests | State-only UI is not a painted Falcon editor. The CLI supplies bounded bank-relative audio/data/state reads; general asynchronous operations require an approved host capability. |
| [`modulation`](../src/uvi/modulation.rs) | Native connection graph, mappers, script modulation, nested Ratio targets and control sources | Does not lower to Kontakt's fixed modulation slots. Remaining conversion/timing limitations produce diagnostics. |
| [`dsp`](../src/uvi/dsp.rs) | Gain, 1–12-channel GainMatrix, OnePole and TrackDelay | Original implementations with explicit parameter/rate/channel bounds. |
| [`effects`](../src/uvi/effects.rs) | DigitalEq, ThreeBandShelves, Convolver and SampledReverb preparation | Reuses the convolution kernel with UVI preparation; matching module names do not imply matching sound. |
| [`resampling`](../src/uvi/resampling.rs) | Bounded rational FIR impulse reconstruction | Original native-measured 44.1/88.2 to 48 kHz curves cover all 238 audited IR assets; uncommon endpoint ratios remain diagnosed. |
| [`filter`](../src/uvi/filter.rs) | Xpander ladder solvers, modes and oversampling | Authored native comparisons cover the admitted 48 kHz path; other-rate fidelity remains limited. |
| [`time_effects`](../src/uvi/time_effects.rs) | Stereo DualDelay, feedback/filter/control state | WhiteChorus remains unavailable; wider source routing is rejected. |
| [`generator`](../src/uvi/generator.rs) | Per-voice Analog and external-wavetable oscillators | Supports bounded settings and rejects unsupported combinations; factory tables are not supplied. Original anti-aliasing/interpolation laws do not establish numerical parity. |
| [`playback`](../src/uvi/playback.rs) | Offline native graph routing, voices, multichannel processing and rendering | Preflight rejects unsupported processors and nondefault behaviors, including initially bypassed modules that scripts might enable. |
| [`cli`](../src/uvi/cli.rs), [`uvi` entry points](../src/uvi/mod.rs) | Local inspection/check/decode/render commands and the separate open-mapping path | Offline commands do not establish plugin integration or real-time safety. |

The open lowercase `layers/layer/zone` mapping format has a separate path into existing sample/engine primitives. It is not interchangeable with uppercase Program XML. Its current playback boundary is mono/stereo; native Program processing separately retains multichannel resources.

## Protected local resources

The inspected UFS2 layout supports clear members, metadata-keyed members and content-keyed members. The native CLI's `uvi-key` command recovers a candidate content state from a standard PNG signature, then validates the **entire PNG's chunk CRCs** before writing a new owner-only, bank-bound state file. The Python inspector has a separate generic C-backed recovery self-check. Prefix agreement alone is not accepted as verification.

Reader namespace discovery and extraction are scoped to supplied official reader binaries and their observed layouts. The native CLI currently verifies the fingerprint/layout of official UVI Workstation 4.0.9 x64; the Python directory inspector locates its metadata namespace by matching the encrypted root name. PasswordV2 wrappers are decoded and parsed separately from the outer archive transform. Neither public source nor this report contains commercial content-state values, reader namespace literals, commercial scripts, sample data, artwork, or member/path inventories. Private state is an access input, not a redistributable fixture or evidence of rights to another library.

The corresponding offline command surfaces include `uvi-bank`, `uvi-key`, `uvi-program`, `uvi-decode`, `uvi-check` and `uvi-play`. `uvi-check` runs more than a syntax check: it can initialize program scripts. The corpus syntax-only result below came from compiling Lua chunks into functions **without executing them**, not from running `uvi-check` on every commercial preset.

## Privately owned corpus: established facts

The paid corpus comprises 25 locally owned UFS banks. Only aggregate format observations are published here; the inputs, decoded programs/modules, access state and exact identities remain private.

| Check | Validated result | What it establishes |
| --- | ---: | --- |
| Archive directory traversal | 59,529 file paths and 813 directories across 25 banks | Observed linked-leaf/name/path structure, not audio behavior |
| Content-state verification | 25/25 banks; 9 distinct states | A complete protected PNG passed all chunk CRCs in every bank |
| Program decoding and native parsing | 40/40 programs; 330,045 graph nodes | Actual Rust Program parser accepts the decoded graphs |
| Lua source decoding and compilation | 120 source modules plus 40 embedded scripts; 160/160 pass | UTF-8 source compiles with the actual MLua vendored Lua 5.1 compiler; no commercial script execution in this check |
| Serialized script API version | `21` in all 40 ScriptProcessors | Includes the audited original/V2 bank pairs; version alone does not prove host API coverage |
| Audio metadata inspection | 48,440 assets; about 24.8 MB of header reads | Establishes format/header/resource structure; the full decode check below is separate |
| Recorded channel counts | 28,707 mono; 18,492 stereo; 1,227 six-channel; 13 four-channel | Channel preservation is necessary; the remaining CAF's dimensions are unparsed |
| Full FLAC/WAV audio decode | 48,439/48,439 assets across all 25 banks; zero failures | Actual `sample::decode` and packed storage; all 48,300 FLACs pass their stored MD5, all 139 WAVs decode; the unused CAF is outside this check |
| Decoded geometry and finite values | 5,254,583,798 frames; 7,778,267,036 scalar values; zero nonfinite values | Every decoded frame/channel/rate count agrees with its inspected header; WAV has no FLAC MD5 claim |
| Independent channel comparison | One six-channel source; 488,700 decoded scalar values | Exact agreement with the independent libFLAC 16-bit decode for that source |
| Packed-storage regression | Actual current wrapper linked against existing `audio::Pcm` | Exact I16/I24/f32 and negative-zero bits, 1/2/6/10/12-channel scalar order, random/reverse reads, padding and invalid-input checks; separate from a full integration run |
| RIFF sample loops | 26,529 loops in 26,529 assets | Every inspected loop has kind `0`, fraction `0`, play count `0`; this does not validate other loop modes |
| Program IR references | All 240 SampledReverb and 376 Convolver references resolve | Includes four-channel Convolver resources; resource resolution does not validate IR channel mapping |
| Controlled offline render, round 5 | 40/40 programs complete; all 40 outputs nonzero | Original Program XML, source modules, embedded script and approved bank resources execute in the actual CLI; one short note test per program, not reference-host equivalence |
| Independent render-output validation | All 40 completed outputs: 72,000 frames, stereo, 48 kHz, float32; zero nonfinite samples | Checks every output scalar independently of the CLI report; zero private log messages or dropped logs |

All 14,208 SamplePlayers resolve after retaining the observed starred sibling-file lists. Of these players, 7,292 refer to a single file, 988 to two mono operands, 2,052 to ten and 3,876 to twelve. Expanding the lists resolves all 76,300 member references, covering 29,086 unique sampled audio resources. Every one of the 6,916 lists has matching frame count, sample rate, complete loop tuples and RIFF unity note across its mono operands. The sole CAF has no program audio references.

The full audio check used a private Rust harness compiled from retained snapshots of the actual sample/storage implementation and linked to the existing sampler's real `audio::Pcm`. Four workers read, decode, verify and drop one asset at a time; they emitted aggregate/per-asset metadata rather than PCM or extracted audio files. All FLACs supplied a nonzero stored MD5, so none counted as verified through an absent checksum. All 139 WAVs additionally passed with the latest native-measured RIFF extent handling. The complete pass took 639.6 seconds on the audit host; this is a recorded run, not a playback-performance benchmark.

The loader assembles ordered mono operands into channel-preserving samples and validates their agreement. Original official-reader probes establish exact stereo equivalence for a two-operand list and channel ordering across all ten and twelve positions of larger lists. Separate authored probes informed multichannel output layouts and keygroup pan behavior. Corpus header coherence, authored assembly checks and these scoped native comparisons are separate evidence; complete bundle/routing parity is not established.

The header survey also found 764 stereo assets whose raw RIFF loop end exceeds the decoded frame count and equals twice that count minus one. Their RIFF format/data dimensions agree with the FLAC dimensions. The decoder preserves those original loop tuples; separate native fixtures establish clamping only the effective playback end to the final frame, without scaling the start. This runtime treatment does not make the raw metadata valid or establish other loop modes.

The programs contain 14,208 SamplePlayers, 6,318 OnePole filters, 6,180 GainMatrix processors, 988 Gain processors, 440 EffectRacks, 1,080 AuxEffects, 160 TrackDelays, 80 DigitalEqs, 160 ThreeBandShelves, 376 Convolvers and 240 SampledReverbs. Their control graphs contain 192,196 SignalConnections, 1,762 ControlSignalMappers, 408 ScriptEventModulations, 291 ConstantModulations and 111 LFOs. These counts establish actual implementation requirements; they are not counts of verified audible processors.

A separately inspected free Starter corpus contains 50 decoded programs and 75 XML node kinds, including generators and effects beyond the paid corpus's sample-based module set. Its wider module diversity prevents treating successful paid graph parsing as general Falcon coverage. The historical official UVI examples likewise include sample-mapping and synthesized sources.

A second survey used the same retained round-3 CLI snapshot on all 50 free Starter programs. Thirty-two completed original-script initialization and the note/release timeline, each forwarding two event commands with no logged or dropped messages. Eighteen stopped before initialization at the explicit multiple-ScriptProcessor gate. Those programs contain two to seven ScriptProcessors; the entire free corpus has 79, including 56 at Program scope and 23 at Layer scope. Scoped event-chain execution is required rather than combining their sources into one script.

The current CLI executes isolated Program and Layer script chains with measured lifecycle ordering, deferred forwarding and layer-scoped release. The numbers below retain that earlier single-script baseline; they are not an audit of the current chain implementation.

All 32 programs that reached renderer preflight had unsupported behavior, so no complete original Starter program was rendered in this survey. The following counts cover those 32 reached programs, overlap, and include processors that are initially bypassed:

| Preflight gate | Programs affected |
| --- | ---: |
| Control graph | 32 |
| AnalogADSR | 27 |
| SparkVerb | 25 |
| Maximizer | 23 |
| DualDelay | 20 |
| WaveTableOscillator settings | 17 |
| DAHDSR | 16 |
| XpanderFilter | 13 |
| WaveShaper | 13 |
| WhiteChorus | 11 |

The first reported control-graph gates were unsupported ConnectionMode `1` in 16 programs, unsupported source kinds in twelve, ambiguous graph paths in three and an unsupported control source in one. Wavetable gates include unison, FM, phase distortion and random phase; nine reached programs also require Analog oscillator unison. These observations prioritize actual missing semantics; a supported oscillator alone does not satisfy the envelopes, effects and control graph of a complete program.

## Sample residency and streaming

The native Program resource path currently loads resident PCM. It retains source channels and caches ordered resource identities; it is not a protected-UFS streaming implementation. Serialized streaming flags are metadata, not evidence that disk streaming occurs.

The sample decoder bounds encoded plus decoded data at 256 MiB; mono bundle assembly also bounds its decoded input/result. `Library::samples` charges each final cached sample's actual packed storage once against a 512 MiB retained-PCM budget. Source checksum verification precedes storage construction. Values exactly representable in existing I16/I24 storage use that representation; other values retain f32, including negative zero. This is lossless packing, not a bit-depth reduction or channel fold.

The renderer also bounds aggregate retained processor buffers to 256 MiB across global inserts and all voices. The count includes delay lines and retained convolution spectra/history, and is checked as processors are prepared and after live resource changes. Shared FFT plans, allocation bookkeeping and transient preparation copies remain outside this count; this is not a total process-memory cap.

The f32-only baseline for unique program samples plus IRs ranges from 246,639,188 to 856,538,932 bytes: 14 of 40 programs exceed 512 MiB. The same header-derived input sets require an estimated **123,329,694–428,278,588 bytes with lossless packing**, including odd-scalar pair padding, so all 40 fit the retained-PCM budget by metadata precision. Across those logical inputs, 8,081 use 16-bit precision and 70 use 24-bit precision; no starred bundle mixes operand precisions.

Across the complete FLAC/WAV asset inventory, actual packed allocations summed to 15,557,288,256 bytes, exactly matching the inspected bit-depth geometry with odd-scalar padding. The largest individual asset used 3,121,020 packed bytes. These are sums and individual decoded sizes from a decode/drop audit, not an allocation of the whole corpus or measurements of assembled program residency.

Bundle assembly consumes operands and caches the assembled result rather than retaining duplicate source PCM. Temporary decode/assembly buffers, allocator overhead, scripts, effect/IR preparation and output buffers are outside that final-PCM estimate. The estimates are not measurements of peak process memory or proof that every complete program loads/renders. Protected-UFS streaming remains unavailable; the current corpus's final PCM estimates alone do not justify adding a streaming framework, while larger logical assets or tighter total-memory requirements would require a separate bounded source path.

## Lua, KSP and H.A.T.

UVI specifies a sandboxed Lua 5.1 host with engine objects, callbacks, module restrictions and real-time memory constraints. KONTRA's interpreter is the matching Lua 5.1 language profile, but the present UVI scheduler/host is an **offline implementation that allocates**. Compiler acceptance does not validate initialization, callback behavior, asynchronous resources or audio-thread safety. [UVI Lua reference](https://lua.uvi.net/_lua_reference.html)

A stock Lua VM does not automatically replace KSP. KSP syntax, Kontakt parameter addresses, voice ownership, persistence and host bindings remain part of the existing Kontakt runtime. UVI scripts need their own typed Program/Layer/Keygroup/Oscillator objects, event forwarding and cooperative timing. Reuse below those host contracts does not erase the distinction. UVI documents default forwarding and `onEvent` precedence explicitly. [UVI callbacks](https://lua.uvi.net/group___event_callbacks.html)

The commercial corpus's original/V2 pairs all serialize API version 21, while many module contents differ. Static call observations include voice fades, sample offsets, script modulation, resource/state operations and editable menus. Compilation does not establish that each such call is implemented with reference-host semantics.

The vendor describes H.A.T. as proprietary Harmonic Alignment Technology combining recorded samples with modeling and continuous air-flow/articulation control. That is a behavioral requirement, not evidence that H.A.T. is a standalone native opcode or that generic sample playback reproduces it. The observed scripts, control graphs and channel bundles must cooperate correctly before a claim about those instruments' expressive behavior is justified. [Double Reeds manual](https://www.acousticsamples.net/index.php?product_id=107&route=product/productmanual), [Flutes manual](https://www.acousticsamples.net/index.php?product_id=119&route=product/productmanual)

The native CLI accepts `--events timeline.json` in place of `--notes` for controller, bend, pressure, transport and overlapping-note checks. Frames are 48 kHz sample positions and channels are zero based. A complete chronological input file can contain:

```json
[
  {"frame":0,"kind":{"Controller":{"channel":0,"controller":1,"value":127}}},
  {"frame":0,"kind":{"NoteOn":{"channel":0,"note":60,"velocity":100}}},
  {"frame":24000,"kind":{"NoteOff":{"channel":0,"note":60}}}
]
```

The interpreter validates timeline bounds, MIDI ranges and finite transport/bend values before loading script sources. This input surface enables offline expressive tests; it does not add audio-thread execution.

## Reference comparisons and remaining gates

The round-5 audit used a retained snapshot of the optimized CLI, at most four concurrent processes, and a velocity-100 note from 0 to 500 ms followed by rendering through 1,500 ms. Every program first received note 60; four completed but silent tests produced nonzero output when retried at a note selected from active keygroup ranges. All 40 final outputs passed the independent WAV checks above, with no operation failures or logged/dropped messages. The earlier control-source, impulse-channel mapping, rounded gain-bound and random/smoothed LFO gates cleared in this snapshot. This establishes the tested timeline, not every note or expressive control. The free Starter survey above retains the separate round-3 baseline.

Original synthetic probes against official UVI hosts informed the implemented OnePole and TrackDelay laws, EQ filter shapes/slopes and selected typed-parameter/widget behavior. Native lifecycle probes establish the implemented order `onLoad` → widget restoration → `onInit`; this corrects the older inferred ordering in the historical audit. A separate 54-case native pan comparison establishes centered mono gain of 0.5 per stereo output and stereo unity gain. Native keygroup-matrix probes preserve the input bus width; a mono input does not acquire twelve channels merely because all matrix coefficient fields exist. Source comments identify the scoped comparisons and retain fidelity diagnostics. These probes do not constitute commercial-preset reference renders or complete module parity.

Current explicit limitations include forwarded-release key matching and duplicated-handle direct-release semantics; unsupported wider Falcon generators/processors; unresolved volume aliases; protected UFS streaming; full asynchronous load/save/browse behavior; a painted UVI editor and plugin automation integration; and real-time execution of the allocating Lua host. Preflight or operation-specific errors must remain visible when a behavior is unavailable.

The native renderer retains diagnostics for its original interpolation, crossfade, voice-stealing and immediate-release choices. Authored native fixtures cover matrix width, mono/stereo pan, selected multichannel output layouts and aligned SamplePlayer gain controls; overlapping keygroup selection and nonaligned control timing remain unverified. DSP bypass transitions, LFO scheduling and modulation timing/rounding, IR routing/normalization/rate conversion and reverb preparation still require scoped reference comparisons. Native random-LFO clock seeds and cross-voice RNG ordering cannot be reconstructed from serialized programs; admitting the measured waveform/smoothing law does not establish matching random output. Initially bypassed unsupported modules are not silently dropped because scripts can enable them later.

The all-bank results above establish directory traversal, independently validated content access, native graph parsing, Lua compilation, header/resource coherence and complete FLAC/WAV decoding with all supplied FLAC checksums verified. The render audit additionally establishes complete resource preparation and original-script execution for all 40 paid programs under this short test. It does **not** establish every script branch, controller sequence or articulation, decoding of the unused CAF, sustained or exhaustive musical behavior, end-to-end reference renders, H.A.T. fidelity or general Falcon compatibility.

## Provenance and reproducibility

Development used public primary documentation, locally owned program/resource observations, static analysis of supplied official UVI reader executables, and original synthetic host probes. This is **not clean-room development**. Mathematical/format observations must be distinguished from vendor implementation expression, and access to the official reader must remain accurately recorded. See [contribution provenance rules](../CONTRIBUTING.md#rights-and-provenance) and [third-party records](../THIRD_PARTY.md).

Public runnable checks use authored fixtures: `python3 tools/inspect_uvi.py --self-test`, `python3 tools/inspect_uvi.py --self-test-recovery`, and the UVI module tests with the `uvi` feature. Those checks exercise the public implementation without distributing the commercial corpus. The paid-corpus validation additionally used private inputs with the real Rust graph parser, MLua Lua 5.1 compiler, full sample decoder and optimized offline CLI; it cannot be reproduced from public synthetic fixtures alone. Private acquisition records, access state, extracted scripts/audio/IRs and path lists are not committed or attached to reports.
