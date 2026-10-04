# UVI capabilities and validation evidence

Status reviewed 2026-10-04. KONTRA has a native, feature-gated UVI interoperability path for bounded archive inspection, protected-program decoding, graph preservation, multichannel sample loading and offline script/render experiments. The latest controlled paid-corpus audit rendered **40 of 40 programs** with their original scripts under both short-note and expressive timelines, producing finite, nonzero outputs. This is **not a claim that every Falcon program or the audited commercial libraries plays correctly**, and it does not establish complete Falcon catalog, editor or sound compatibility. Supported programs now have an optional per-part plugin player, shared rack mixing and native control panels; see the [player ownership boundary](PLAYER_BACKEND_BOUNDARY.md).

The [format](FALCON_FORMAT_GROUNDWORK.md) and [runtime/UI](FALCON_RUNTIME_UI_GROUNDWORK.md) groundwork reports retain the earlier 2026-10-02 audit. Their then-unknown format/runtime observations must not be read as the current implementation status. [Observed UFS2 layout](uvi-format.md) documents the separately scoped structural inspector.

## Current test checkpoint follow-up

The latest source adds exact hosted-root choking without synthesizing release callbacks, transport updates without redundant per-callback play/stop callbacks, and bounded program-state capture. State retains native persistent widgets/onSave data, original-node typed parameter deltas and approved sample/impulse paths. A fresh replacement validates its graph fingerprint, processor set, resources and DSP overrides; constructor/onLoad/onInit precedence follows the retained native measurements. Voice state and transport are excluded. Missing Gain/Pan defaults are scoped by the native measurements below; a complete property inventory remains unfinished. Ordinary playback and UI polling do not invoke onSave.

Explicit rack Save requests a processed audio boundary and fails rather than accepting stale native controls. The framework's host pre-save hook cannot return a failure: a failed capture retains the previous successful bytes and writes a diagnostic. Resume processing and save again when a partial packet or queued edit has not settled. No file-write capability is granted to native scripts by this state transport.

Missing original-node Gain/Pan defaults are admitted only where fresh official
Workstation probes establish them: Program/Layer/Keygroup expose Gain1 and Pan0,
SamplePlayer exposes Gain1 with no invented Pan property. Native setters accept
out-of-range Lua values despite advisory metadata ranges; existing audio-domain
validation remains separate. Saved deltas and restored PCM have authored checks.
Synthetic Part/Synth audio setters and a complete missing-property inventory remain
unsupported.

The new per-preset diagnosis distinguishes decoded graph nodes, static preflight admission/rejection, initialization phases and actual packet/voice counters. Explicit worker snapshots additionally expose instrumented per-node processed block counts and their captured frame; uninstrumented nodes remain unknown. This does not establish audibility or Falcon equivalence. Worker load details survive preflight failure and enter the existing bounded Logs/export journal. Resource, Lua and DSP errors preserve their actual causes; the instrumented execution boundaries add processor IDs and frames. See [testing and diagnosis](UVI_TEST_CHECKPOINT.md).

Button and OnOffButton labels now default off, Knob labels/values default on, and empty native captions remain empty. Custom artwork does not change these defaults. A genuine initialized VWinds source-helper capture retains 273 widgets and 38 authorized images. The intrinsic 720×480 stage now scales uniformly up and down, with captions overlaid on full knob frames and authored small menus retaining their bounds. Placement/hit-target checks at 480 and 1080 pixels wide pass. This is a rendered panel from actual source and resources, not a complete native window screenshot or pixel-parity proof.

Stationary 48-kHz stereo Exciter Modes0/1 at Oversampling0 and individually measured SparkVerb delay-layout tuples are admitted; Exciter coefficients remain fitted and carry a fidelity diagnostic. Static square LFO uses measured control points; unmeasured live waveform/connected-parameter changes remain gated. WaveTable scalar endpoint conversion groundwork does not enable its unverified moving control routes. **Starter coverage remains 1/50**: no additional original preset is advertised as fully playable from these leaf changes. Prior paid-worker measurements include missed render deadlines; realtime readiness remains unproven.

Combined optimized CI validation at installed checkpoint `75998c3` with `uvi,standalone` passed **977 library tests, 89 CLI/playback tests and four additional binary/integration checks**, with 35 external-fixture tests ignored and one screenshot excluded. This includes the native two-part rack save/reopen PCM regression and diagnostic failure cases. Subsequent source changes use focused functional checks and feature-isolation checks; those broad counts do not describe the newer source. The older corpus and native-comparison measurements below remain separately scoped.

A subsequent source follow-up admits bounded larger PNG image wavetables using
the measured native software-image rescaler. Ten original fixtures match complete
pre-FFT normalized helper output. This corrects an earlier raw-import assumption
inferred from downstream oscillator audio; native FFT/harmonic generation,
constant-input final output and the Windows hardware branch remain unverified.
JPEG, transparency and moving controls remain gated. This source change follows
installed checkpoint `75998c3` and is not included in that build.

Current audio-resource completion checks cover successful and failed `loadSample`
and `loadImpulse` tasks: callbacks run after return through the existing product
scheduler and may yield. Failed reads/metadata retain the previous resource and
record `uvi.host/resource_task_failed` warnings with bounded reasons; exact task
errors remain available to Lua. Native background interleaving is unverified.
Compiled numeric modulation target slots and single-pass PCM validation preserve
paid PCM/command/state captures in focused comparisons. Renderer CPU medians
improved 7.51%/12.70% and preload CPU medians 5.85%/9.48% for the original/V2
Clarinet respectively; these separate captures do not establish overall loading
latency or sustainable realtime performance. The selected preload datasets made
zero sample-decryption calls.

## Current evidence ledger

An actual Alto Flute 2 hosted-worker/Bridge probe reproduced audible playback
followed by `RequestCapacity`: the worker fell behind the bounded request queue
while its initialization status remained Ready. A single held note also
reproduced the failure. The user's earlier report lacks the exact adapter cause,
so this establishes a failure mechanism rather than identifying that historical
incident conclusively. First-failure atoms now retain the endpoint error, stage,
frame and Rust source location without audio-thread formatting or logging.
Increasing queue capacity alone would not establish sustainable playback.

Augmented Orchestra's Bartok member is a valid single-file ZIP wrapper. Bounded
CRC-checked inflation parses its 97,046 retained graph nodes and 6,984 zones;
installed checkpoint `25105ae` reports 787 explicit preflight issues after removing
a duplicate StepEnvelope rejection formerly attributed to ControlGraph. The next
source pass implements the measured global StepEnvelope scope and reports 786
issues: both Step source blockers disappear, while the first existing deterministic
LFO smoothing blocker becomes visible. This does not admit the program.
Dedicated CombFilter, MS20, Flanger, Drive, Diode and MultiLFO leaves match scoped
authored native comparisons but remain outside Program admission. Connected
control/graph lifecycle, unmeasured callback stages and broader settings retain
explicit rejection reasons. See [MS20 evidence](UVI_MS20_SCALAR_EVIDENCE.md),
[Comb held-Value boundary](UVI_COMB_VALUE_CONTROL_EVIDENCE.md),
[Diode circuit boundary](UVI_DIODE_CIRCUIT_EVIDENCE.md) and
[MultiLFO endpoint boundary](UVI_MULTILFO_ENDPOINT_EVIDENCE.md).

A subsequent decode-only frontier checked all 50 Starter programs, all 40
declared VWinds programs across 25 banks, and eight selected Augmented Orchestra
programs: 98/98 decoded and 41 passed static preflight (40 VWinds, one Starter).
All eight Augmented members use ZIP wrappers and exceed 16 MiB expanded XML.
Their combined blockers include 3,112 CombFilter, 3,104 MS20 and 32 Flanger nodes.
This scan did not load samples, initialize scripts or play those programs.

Changing only the lookup-only numeric name index to the existing `FxHashMap`
preserves six actual Alto corpus hashes and counts. Two controlled render-loop
counter pairs reduced instructions by 10.88% and 10.87%. One non-overlapping
pair reduced thread CPU by 4.31%; the second timing pair overlapped other work.
The paced Bridge still reached RequestCapacity, so this is an incremental cost
reduction, not a deadline or blackout fix.

The combined Renderer and graph lookup changes plus per-evaluation Constant/
Script source memo reduce instructions by 26.67% in two alternating actual Alto
corpus comparisons, preserving all six PCM/event/state hashes and counts.
Memoization resets between voices, instances, inputs and parameter edits; LFO
and stochastic clocks are unchanged. External builds confound elapsed/CPU
comparisons. The paced combined probe still reaches RequestCapacity at frame
41,984, so sustainable playback remains unresolved.

The next source pass adds per-evaluation LFO source memoization only after
successful evaluation, preserving original signed-zero, derived-infinite
frequency and depth-limit paths. Thirty-seven focused modulation checks and
all six actual Alto PCM/event/state hashes match. Two alternating counter pairs
reduce instructions a further 3.65% relative to the combined lookup/Constant/
Script memo baseline. The paced candidate still reaches RequestCapacity at
frame 300,032; renderer wall-time spikes and worker/callback overhead remain
under investigation. This is not a blackout fix or a realtime guarantee.

Subsequent controlled subphase measurements locate two cold-packet CPU spikes
in repeated SampledReverb impulse reconstruction. The next source reuses
immutable PCM reads within each FIR reconstruction call without changing the
curve, setter order or prepared-resource lifetime. Startup packet CPU improves
37.7% at frame zero and 50.5% at frame 2304 in the bounded paired captures;
sustained CPU improvement is not demonstrated. Both candidate paced captures
have zero Bridge underruns, while one baseline has 38 despite its separate
Worker underrun counter remaining zero. The [IR evidence](UVI_IR_SOURCE_READ_EVIDENCE.md)
records this distinction. Fresh integrated source `72c671e` separately passes
10- and 20-second Alto Worker/Bridge replays with finite nonzero audio, Ready
status, zero endpoint failures, Bridge underruns, backpressure and worker errors.
Its 38 copied modules match that recorded commit; unchanged external SDK glue
is disclosed. These are source-helper observations, not a live Bitwig guarantee.

Installed `25105ae` also initializes the single statically admitted Starter FM
preset. One original-script MIDI-60 probe (120 ms hold plus one-second tail)
renders 53,760 finite stereo frames with nonzero output and zero worker errors;
seven instrumented source/FX nodes process 210 blocks each. This adds a scoped
generator-path observation beyond the paid sample-player corpus. It does not
expand the 1/50 Starter compatibility claim to full musical/native parity or
verify a live plugin endpoint. Catalog, decode and admission counts remain
distinct as recorded in the [checkpoint report](UVI_TEST_CHECKPOINT.md).

Live loading snapshots report stage durations, graph counts, current initial
resource, successful aliases/unique decodes and resident PCM. An actual Clarinet
V2 source-worker capture included 52 partial resource updates and ended at
196/196 initial resources and 317,220,848 PCM bytes. These counts exclude later
Lua-requested resources. Genuine bank-font loading uses the existing UI font
API and shared resource budget; native visual comparison and advanced display
support remain incomplete.

The next StepEnvelope source retains authoritative Transport beat/playing at
the renderer frame rather than deriving every hosted beat from elapsed audio
time. Its independent running-host projection has 108 native endpoint comparisons
and 434 separately identified float32 consumer-model rows, plus five focused
source checks. Nonzero starts, aligned positive seeks and aligned running tempo
changes are covered; stops, negative positions, unaligned snapshots, connected
Step parameters and frequency/rate/block-size changes remain explicit gates.
The constructor/full host and whole native program audio remain unmeasured.

The next authored NumBox readout uses bank fonts, alignment, ink and background
resources within its original bounds. Fourteen focused UI checks preserve real
numeric typing; 32 settled corpus readout checks cover geometry at 360/720 px.
Native display precision remains uncalibrated: a long frequency can still
ellipsize in its authored 38-pixel box. This is source-render evidence, not a
native pixel comparison.

The previous installed `f4e2a17f8c35def9bda97e652c61e708ea95893f` CLI (SHA-256
`6a80a63eaf1e9c4091adf2000c80886f91fc1a48ea5faec0e064c8f8dc1d7c5b`)
was audited with at most two concurrent workers against the exact 40 catalog
programs in 25 local paid banks. Each passed parsing, static preflight, the
CLI script check, initialization-only Worker diagnosis and a short authored-note
Worker render. All 40 WAVs independently verified as 72,000 finite stereo frames
at 48 kHz with nonzero output. This pass used rootless CLI inputs; it does not
verify Bitwig adoption, the typed-root host transport, every articulation,
controls/artwork, sustained deadlines or native musical fidelity. No native
program comparison or full asset-decode repeat belongs to this pass.

The short notes exercised 195 of the 14,208 declared SamplePlayer nodes. Of
330,045 parsed nodes, 903 instrumented nodes recorded processing; uninstrumented
containers, routing/control nodes and scripts remain unknown. Processing can
include silence or release tails. The banks declare 8,335 PNG, 26 JPEG and 15 SVG
members, but these directory counts are not image/UI decode coverage. No synthesis
generator was exercised by this paid-corpus note pass. The retained 3,505 deadline
misses in 11,280 blocks under two audit jobs do not establish sustainable realtime
performance or a new controlled timing benchmark.

A separately instrumented five-program investigation established that the authored
callbacks narrow their serialized zone ranges: the original/V2 Clarinets use
49–93 instead of 48–94; the other three use 24–59, 52–84 and 74–108, each excluding
the outer mapping keys. All 70 original note callbacks arrived, while 40 original
outside-range probes emitted no Starts. Additional fresh boundary probes confirm
script suppression before Renderer admission; this does not justify changing the
renderer. The observed range globals are bank conventions, not a universal API.

That investigation also reproduced generated noise velocities outside our old
admission range. Unchanged native `postEvent` binding/decoder instructions now
establish numeric truncation, signed 32-bit narrowing and clamp to 0–127.
Generated NoteOn handling applies that measured conversion and Renderer admits
zero; incoming MIDI validation retains its separate contract. Four focused
checks and five actual Clarinet source probes cover release velocity0, -2→0,
134→127, the authored outside-range suppression and a normal control note, with
finite 12,000-frame renders and no callback/renderer errors. The full native
playNote helper wrapper and end-to-end native PCM remain unmeasured; this is a
specific decoder correction, not a complete expressive parity claim.

Evidence is tracked separately rather than collapsed into a compatibility percentage:

| Evidence | Meaning | Does not establish |
| --- | --- | --- |
| Parsed | Bounded container/program decoding completed | Any runtime behavior |
| Preflight admitted | Known graph passed static gates | Resource or initialization success |
| Ready | Resources, Lua and renderer initialized | Notes, controls or audible output |
| Exercised | The stated events reached this renderer; output and counters were checked | Unexercised articulations or callbacks |
| UI captured / artwork decoded | Initialized owned snapshots and approved images were obtained | Drawing, hit targets, interaction or native pixel fidelity |
| Native compared | The stated fixture/settings were compared with the official reader | Other parameters, rates or complete instruments |

UVI's [scripting reference](https://lua.uvi.net/) identifies Lua 5.1 and a
specialized musical-event/engine/UI API. Language compatibility alone therefore
does not establish host API parity. Primary documentation, authored checks,
native observations and inferred behavior must remain distinguishable when
expanding an admission gate. The current reader namespace binding is specifically
verified against Workstation 4.0.9 x64; newer documentation is not evidence that
this binary implements every newly documented API.

## Native ownership and boundaries

The `uvi` Cargo feature enables these modules. Archive, program, script and renderer identities remain native UVI objects rather than being flattened into Kontakt groups or translated to KSP.

| Module | Responsibility | Boundary |
| --- | --- | --- |
| [`ufs`](../src/uvi/ufs.rs) | Bounded UFS2 records, linked directory leaves, member names, exact byte spans and encryption modes | Index-node bodies remain opaque. A decoded filename does not establish playable content. |
| [`crypto`](../src/uvi/crypto.rs) | Offset-seeded name/content transforms, 512-byte content resets, PasswordV2 program wrappers and generic stream-state recovery | Reader namespaces and content state are local inputs. Unsupported legacy program protection fails explicitly. |
| [`diagnostics`](../src/uvi/diagnostics.rs) | Parsed node identities and exact static preflight reasons | Omits source/attributes/asset paths; static admission is not per-node execution or sound fidelity. |
| [`state`](../src/uvi/state.rs) | Bounded versioned native persistence, original graph fingerprint and approved resource targets | Rack state transports no voices, Lua handles or additional resource authority. |
| [`program`](../src/uvi/program.rs) | Program/Layer/Keygroup/Oscillator graph, stable node IDs, attributes, connections and sampled zones | Preserves unknown modules; parsing does not execute them. XML limits and DTD rejection apply before execution. |
| [`library`](../src/uvi/library.rs) | Bank-relative resource paths, source-only Lua modules, shared sample identity and worker-owned resource capabilities | Rejects ambiguous resources and unresolved volume aliases; PCM and alias caches are bounded. Resource callbacks perform disk I/O off the audio thread. |
| [`sample`](../src/uvi/sample.rs) | WAV/AIFF/FLAC and finalized CAF LPCM decoding, every source channel, RIFF sampler metadata and ordered mono bundles | Keeps inclusive loop endpoints, fractional positions, play counts and unity notes. Decoder success is separate from renderer support for those fields. |
| [`storage`](../src/uvi/storage.rs) | Resident PCM using the sampler's existing exact I16/I24 packing, with f32 fallback | Retains scalar channel order and source values; storage pairs are not stereo downmixing. |
| [`script`](../src/uvi/script.rs) | Persistent Lua 5.1 sessions, cooperative scheduling, note identity and actual-rate timed command output | Allocating, thread-owned VM with incremental GC and bounded full-collection fallback; it must not run in the plugin audio callback. |
| [`host`](../src/uvi/host.rs) | UVI object identity, typed parameters, widget state, owned UI snapshots, approved module loading and resource requests | Snapshots contain no Lua handles or callbacks; native panel painting and live edits remain separate. The CLI supplies bounded bank-relative audio/data/state reads; general asynchronous operations require an approved host capability. |
| [`modulation`](../src/uvi/modulation.rs) | Native connection graph, mappers, script modulation, nested Ratio targets and control sources | Does not lower to Kontakt's fixed modulation slots. Remaining conversion/timing limitations produce diagnostics. |
| [`dsp`](../src/uvi/dsp.rs) | Gain, 1–12-channel GainMatrix, OnePole and TrackDelay | Original implementations with explicit parameter/rate/channel bounds. |
| [`exciter`](../src/uvi/exciter.rs) | Fitted bounded stationary stereo processing at 48 kHz | Modes0/1 and OS0 only; coefficients remain explicitly approximate. |
| [`effects`](../src/uvi/effects.rs) | DigitalEq, ParametricEQ, ThreeBandShelves, Convolver and SampledReverb preparation | Reuses the convolution kernel with UVI preparation; matching module names do not imply matching sound. |
| [`resampling`](../src/uvi/resampling.rs) | Bounded rational FIR impulse reconstruction | Original native-measured 44.1/88.2 to 48 kHz curves cover all 238 audited IR assets; uncommon endpoint ratios remain diagnosed. |
| [`filter`](../src/uvi/filter.rs) | Xpander ladder solvers, modes and oversampling | Original elliptic reconstruction and authored native cross-rate comparisons cover admitted static settings; live control fidelity remains limited. |
| [`time_effects`](../src/uvi/time_effects.rs) | Stereo DualDelay, bounded DualDelayX tape/diffusion and WhiteChorus | Diffusion is limited to 48 kHz, Spread 20 and readers 0/1. Chorus startup, modulation and phase/read rounding retain explicit diagnostics. Wider source routing is rejected. |
| [`waveshaper`](../src/uvi/waveshaper.rs) | Per-bus nonlinear transfer and oversampling | Eleven measured modes and OS0/1 are admitted; Mode4, higher oversampling and unverified live controls remain gated or diagnosed. |
| [`maximizer`](../src/uvi/maximizer.rs) | Linked stereo lookahead and gain reduction | Measured bounded settings are admitted; attack, true-peak and alternate ceiling modes remain gated, with a slew-fidelity diagnostic. |
| [`phasor`](../src/uvi/phasor.rs) | Measured allpass cascade, feedback, native control clock and bypass state | Bounded mono/stereo triangle/sine settings; stochastic shapes and unverified transition behavior remain gated or diagnosed. |
| [`sparkverb`](../src/uvi/sparkverb.rs) | Stereo feedback network, diffusion and output filtering | Native comparisons cover bounded static layouts and moving modes 1/2. Moving mode 0, uncertain prime-delay boundaries and unmeasured shapes remain gated. Native moving comparisons use 256-frame blocks; smaller host spans have separate, unresolved native behavior. |
| [`generator`](../src/uvi/generator.rs) | Per-voice Analog, external-wavetable and bounded four-operator FM oscillators | Measured unison, phase and modulation settings are admitted. PNG bitmap conversion is limited to 128 rows; larger native resampling and JPEG remain gated. Ambiguous bank wavetable cycle geometry is rejected; native random sequences and full anti-aliasing parity are unclaimed. |
| [`worker`](../src/uvi/worker.rs) | Dedicated thread owning the VM, resources and renderer; bounded fixed event/PCM packets | Generation and activation epochs reject stale data. The exclusive callback-facing port allocates nothing; controller creation, private diagnostics, stop and both controller/owned-endpoint destruction belong off audio. CPU deadlines and plugin integration are separate work. |
| [`player`](../src/uvi/player.rs) | Persistent VM, prepared resource revisions and renderer clocks in one worker-owned session | Exclusive block boundaries retain future callbacks; validation precedes mutation, and execution failures require replacement. Allocating playback is not audio-callback safe. |
| [`playback`](../src/uvi/playback.rs) | Offline native graph routing, voices, multichannel processing and rendering | Preflight rejects unsupported processors and nondefault behaviors, including initially bypassed modules that scripts might enable. |
| [`cli`](../src/uvi/cli.rs), [`uvi` entry points](../src/uvi/mod.rs) | Local inspection/check/decode/render commands and the separate open-mapping path | Offline commands do not establish plugin integration or real-time safety. |

The open lowercase `layers/layer/zone` mapping format has a separate path into existing sample/engine primitives. It is not interchangeable with uppercase Program XML. Its current playback boundary is mono/stereo; native Program processing separately retains multichannel resources.

## Plugin catalog and scripted controls: source follow-up

The feature-gated browser now keeps Kontakt and UVI entries together with typed format identities. A UVI selection retains the real bank path, bank UUID and exact embedded Program member, rather than manufacturing a Kontakt file path. Scanning and inventory run off the UI thread, with bounded per-bank and aggregate caches. An actual private Scanner check found **25 banks and 40 programs with zero inventory failures**. Header identity is checked before accepting a cached result. Non-UTF-8 bank paths remain unsupported by the persisted catalog and report an explicit status.

Rows, keyboard selection, drag actions, recents, favorites and requested targets retain this identity. Selecting a UVI program stages preparation on the serialized loader while preserving an already playing Kontakt part. The allocating worker and its blocking destructor stay with that loader; no worker enters the audio engine's handoff or destruction path. Exact request, bank/member, destination identity, activation epoch, sample rate and catalog revision reject stale preparation. Supported programs activate through the native per-part endpoint after shared delay storage and the audio callback acknowledge preparation. Local reader selection is available through settings/API or `KONTRA_UVI_READER`; private authority files are read through `KONTRA_UVI_AUTHORITY_DIR`, not persisted in projects. A graphical reader picker remains pending. Catalog checks cover legacy keyed/positional state and cache invalidation; these later source changes are outside checkpoint `542049e`.

`host::snapshot_ui` and `Session::ui_snapshot` copy initialized widget state into bounded, owned data: processor-local constructor IDs, parent relationships, local/absolute bounds, effective visibility, values, menu entries, styles and bank-relative artwork references. Snapshotting does not call Lua callbacks or advance the session clock. An actual initialized VWinds program yielded **273 widgets, 24 effectively visible**. The published static preview uses that state and original local artwork; it is not a running native editor. Authored checks cover processor scope, stable IDs, malformed state and ownership after the VM is destroyed.

The worker's off-audio snapshot mailbox coalesces requests to the latest panel and returns an owned reply with request, generation, activation epoch and playback-boundary identity. It remains responsive while playback is idle or the audio output queue is full; the callback-facing fixed packet port does no UI locking, allocation or destruction. Invalid processor requests return a fixed error without failing the player.

`Worker::take_audio_port` transfers packet cursors and pending output once into an exclusive owned endpoint. The controller retains thread, status and UI ownership; its subsequent packet calls return `PortTaken` without consuming queues. Authored checks preserve queued/future packets, reject a second extraction, cover failed/stopped activations and prove zero callback allocations. A real authored worker produces nonzero finite audio through the extracted endpoint while its controller services snapshots. The plugin must retire the endpoint on its serialized loader before stopping or destroying the controller; callback adoption, bounded block adaptation, common latency and rooted host-note completion are now integrated. The earlier endpoint-only checkpoint was `b4526e7`; the later per-part integration is `510446d`.

Typed numeric, menu, Table-cell, OnOffButton and Button edits now enter the same absolute frame timeline as MIDI. Their original changed callbacks run on the existing scoped Lua scheduler and can yield across worker packets. UI admission validates identity, type, range, visibility and enabled ancestors; these GUI rules do not restrict programmatic Lua setters. Initial admission precedes clock mutation, then execution revalidates after pending callbacks. Nonfatal rejections return a fixed per-request bit mask; callback/scheduler execution failure stops the player. UI edits precede MIDI at equal frames, with stable order within each stream. No independent idle edit advances the audio clock. Authored end-to-end worker checks cover unchanged PCM/counts between bulk and packet execution, frame-zero edits before notes, yielding callbacks, malformed inputs, nonfatal rejections, snapshots and fatal callback failure. Native panel painting and plugin activation are integrated for supported controls; complex displays and native font/unit fidelity remain incomplete.

Opt-in hosted Players now retain backend activation-local ancestry separately from opaque Lua voice IDs and the core's canonical host-note identity. Whole hosted batches validate before callbacks or clock mutation. Held inputs, delayed script descendants, future commands and actual surviving DSP launch instances pin their tokens; shared effect tails and retained Lua handles do not. Native release matching remains FIFO even when script IDs alias. Completions use a conservative exclusive packet boundary and remain counted against the 4096-root ledger until acknowledged after durable transfer; they do not assert the exact sample of last audible output. The opt-in worker transport below carries these records; the live adapter maps canonical core identities and delivers End only after the corresponding buffered PCM has been consumed.

The candidate's direct source harness passes 123 checks, including 19 authored Player/Renderer end-to-end checks, with nonzero legacy PCM equality, delayed descendants, aliased IDs, keygroup siblings, malformed batches and completion backpressure. Thirteen Session tests and one Renderer test are retained in the repository. All-target `cargo check` and the full optimized CI regression pass also succeed: 919 tests pass, 35 are ignored and the screenshot test is filtered. In a separate warmed 32-packet fixture, legacy Session and Renderer allocation counts/requested bytes match their baselines; hosted census adds 32 calls and 3072 requested bytes. Constructors, the first packet, Lua allocation and CPU timing are excluded. Voice metadata grows by 32 inline bytes; Task/Forward also gain optional ancestry. Ledger/census allocations stay on the worker, not the audio callback. This source follow-up is outside the immutable paid-corpus checkpoint below.

`9888f85` adds `Worker::start_hosted`, fixed `HostedRequest` packets and an independent 4096-record completion queue. Ordinary MIDI and rooted inputs share a 256-entry packet bound; UI retains its separate 64-entry bound. At equal frames the existing scheduler orders UI, rooted entries, then ordinary MIDI. `Worker::start` retains the ordinary Player and allocates no hosted queues or census. Full submissions return the packet intact without moving the cursor. Completion retries retain their exact suffix during idle and audio-output backpressure; a durable push precedes Session acknowledgement. A failed acknowledgement or queued authoritative-lifetime rejection aborts the activation explicitly. The future core adapter must retire its canonical owners on that failure.

Completion polling takes the current consumed boundary on the worker's activation timeline, translated from host time through admitted buffering latency. Future records stay inline, and late PCM discard does not discard a valid completion. Durable transfer is not immediate authorization for host NoteEnd. Fourteen authored-bank repository tests and a separate direct 37-check transport run cover actual construction, scheduling, snapshots, fences, backpressure and callback allocation behavior. The combined transport/override source passes all-target metadata checking and the full optimized CI suite: **937 tests pass**, 35 are ignored and the screenshot test is filtered. Stop, join and heavy endpoint/controller destruction remain off audio. The completion cancellation counter covers durable queued and controller-held inline records; detached-port inline records and untransferred Session roots are excluded. No live plugin adoption or external backend ABI is implemented by this change.

`f024616` registers validated numeric overrides on the native modulation graph at the Renderer's sole live write, removing repeated full-map scans from its internal scope/release evaluation. Public external-map APIs still validate the entire caller map. Parsed nonfinite Text retains its existing successful-setter/delayed-evaluation error behavior through a renderer-owned fallback until all bad entries are repaired. Four new authored proofs cover atomic registration, graph isolation, nested mapping, malformed Inputs, external validation and partial repair; combined actual-source direct checks pass 81/81, including the ancestry/FIFO test.

A separate eight-run private comparison on the earlier `99bb313`/`191e481` source wrappers preserves complete paid PCM and musical/host command hashes, all 825 blocks and authored whole/partitioned mutation output. Median Renderer thread CPU is 3783.519 versus 2854.117 ms (24.56% lower); packet CPU is 3957.538 versus 3008.676 ms (23.98%). The unchanged Session also shifts 11.28%, and background load varies, so every percent cannot be attributed exclusively to the code. Later ancestry/transport changes are absent from those measured wrappers. Renderer deadline misses remain 27/37/19 of 825 blocks, and the first block still takes roughly 73–76 ms. This does not establish real-time safety. Paid steady render allocation traffic is unchanged; constructors request 6,074,400 additional Rust bytes, with a separately measured constructor-boundary net increase of 1,490,896 bytes. These are allocation-boundary measurements, not RSS/peak memory. CachedParameter grows by 16 inline bytes; authored absent-field registration adds 344 requested bytes. No current whole-product timing claim is made.

## Protected local resources

The inspected UFS2 layout supports clear members, metadata-keyed members and content-keyed members. The native CLI's `uvi-key` command recovers a candidate content state from a standard PNG signature, then validates the **entire PNG's chunk CRCs** before writing a new owner-only, bank-bound state file. The Python inspector has a separate generic C-backed recovery self-check. Prefix agreement alone is not accepted as verification.

Reader namespace discovery and extraction are scoped to supplied official reader binaries and their observed layouts. The native CLI currently verifies the fingerprint/layout of official UVI Workstation 4.0.9 x64; the Python directory inspector locates its metadata namespace by matching the encrypted root name. PasswordV2 wrappers are decoded and parsed separately from the outer archive transform. Neither public source nor this report contains commercial content-state values, reader namespace literals, commercial scripts, sample data, artwork, or member/path inventories. Private state is an access input, not a redistributable fixture or evidence of rights to another library.

The `uvi-play` path owns one persistent Player across the file. Its optional `--block-frames <frames>` setting changes processing granularity; the default uses the largest bounded block. Both modes advance Lua and DSP through the same processing horizon, including planned-source lookahead in the padded tail. The audio-player ingress accepts 1–1000 BPM, while the separate Lua-only session retains positive finite tempo support. Graphs with planned native control sources require multiples of 256 frames; their final block is padded and the WAV trimmed to its requested extent. Resource aliases resolved by Lua are installed before the corresponding block reaches DSP. `--worker` exercises the dedicated thread through fixed 256-frame generation/epoch-stamped packets and reports preparation/render timing separately; offline polling counts are not hardware underruns. This is a worker/offline path, not live plugin integration.

The corresponding offline command surfaces include `uvi-bank`, `uvi-key`, `uvi-program`, `uvi-decode`, `uvi-check` and `uvi-play`. `uvi-check` runs more than a syntax check: it can initialize program scripts. The corpus syntax-only result below came from compiling Lua chunks into functions **without executing them**, not from running `uvi-check` on every commercial preset.

## Privately owned corpus: established facts

The latest completed immutable audit is clean checkpoint `f02461696d295a19beae4e6ff70ed3036450174c` (0.3.148), executable SHA-256 `39b97d8026746a62332e01c5b085799bfefecaa21bee2603da7f3e705c4c0f24`, using the optimized CI profile without ThinLTO. **All 40 short, 40 expressive and four additional-rate paid renders pass**. All fifty Starter script checks pass; one complete Starter render is audible, and the other 49 fail at their explicit preflight gates. The 91 written WAVs from 140 actual render attempts, including four silent first-note attempts and two smoke tests, independently pass complete geometry and finite-value checks. The executable hash and private permissions are unchanged; full asset decoding was not repeated.

Against `191e481` in the same profile, 38/40 PCM outputs match in each paid batch and all four rate outputs match. All eighty paid timelines and four rate probes retain their event/host-command counts; all fifty Starter preflight lists and script-check counts match. Differences remain confined to the same two previously demonstrated stochastic cases. The retained repeat study below supplies their variability evidence; no new repeat study was needed. This audit exercises ordinary CLI rendering, not the newly rooted Worker transport, live plugin adoption, or vendor-paid native parity.

The preceding clean integration audit is checkpoint `191e48118c95dc38da80a56f23a6152356cac229` (0.3.148), executable SHA-256 `f6685b118b8a2ae5bd9930b1b5a1998e783e8a0cab52d75fd3f7bb6d51fcccc9`, also using the optimized CI profile without ThinLTO. All 40 short, 40 expressive and four additional-rate paid renders pass. All fifty Starter script checks pass; one complete Starter render is audible, and the other 49 fail at explicit preflight gates. All 91 written WAVs pass complete geometry and finite-value checks.

Against `9d69ae0` in the same profile, 38/40 PCM outputs match in each paid batch and all four rate outputs match; event/host-command counts for all eighty paid timelines and four rate probes are unchanged. All fifty Starter preflight lists and check counts match. Targeted repeats of the remaining two paid cases show **run-to-run PCM variation within both immutable binaries**: each binary/case/timeline combination produces three distinct hashes across its original and two repeated renders, with stable command counts. The sixteen repeat WAVs also pass full geometry/finite checks, and peak ranges overlap across revisions. These observations confound assigning cross-version differences to the integration; they do not exclude small version effects or establish vendor-native parity.

Both varying programs contain an active random LFO whose output modulates another LFO's frequency/depth and reaches active Gain routes. The admitted random LFO constructor seeds from the process clock. A read-only probe of the actual graph finds three distinct control fingerprints across three fresh constructions, while repeated evaluation of each graph at the same frame is identical, before script/audio execution. This establishes a source of run-varying control values without rewriting the original programs or attributing the variation to unordered traversal. Native random-sequence parity remains unclaimed.

The preceding immutable audit is checkpoint `9d69ae0b62a1ec73e85a70f50e73a6d016e0fb0a`, executable SHA-256 `4f2dcacf2fdade1fba16268893bf0a03070def835d8579c9bbffb548563ec976`, built with the optimized CI profile without ThinLTO. **All 40 short, 40 expressive and four additional-rate paid renders pass**, as do all fifty Starter script checks. Starter program 39 remains audible; the other 49 renders fail explicitly at preflight. All 91 written WAVs, including silent first-note retries and two smoke tests, independently pass complete geometry and finite-value checks. The immutable hash and private permissions were verified; full asset decoding was not repeated.

Against `b4526e7` in the same optimized CI profile, **38/40 short, 38/40 expressive and all four additional-rate PCM outputs are byte-identical**. All eighty paid timelines and four rate probes retain their event and host-command counts. Only the same two paid programs change; maximum absolute expressive window RMS delta is 0.0000350192. Forty-nine Starter preflight lists are identical; the remaining DualDelayX rejection narrows from active diffusion to the unverified Spread setting and still rejects playback. The retained release-profile `542049e` comparison also preserves 38/40 outputs in each paid batch and all four rates. These are regression observations, not original commercial-program audio parity or sustainable real-time playback.

The preceding correction audit is checkpoint `b4526e785b745f03025485561f98d80eaa319f6a`, executable SHA-256 `3159ff2cb7697a0e51931534b7b3b2b52b4685e3f01d0c0e62ba5785e6384c3b`, built with the optimized CI profile without ThinLTO. **All 40 short, 40 expressive and four additional-rate paid renders pass**, restoring the initialization failures described below. All fifty Starter script checks pass. Starter program 39 renders finite, audible stereo float audio; the other 49 render attempts retain their exact explicit preflight failures. All 91 written WAVs, including two smoke tests and four initial silent short attempts subsequently retried at active notes, independently pass complete geometry and finite-value checks. The binary hash is unchanged before and after; the full asset decode was not repeated.

Against successful checkpoint `542049e`, **38/40 short and 38/40 expressive PCM outputs are byte-identical**, as are all four representative 44.1/96 kHz outputs. Event and host-command counts match for all eighty paid timelines. The same two programs change in both paid batches; maximum absolute expressive window RMS delta is 0.0000183514. Starter 39's output is byte-identical to `3137780`. These are executable regressions, not vendor-paid native audio parity or sustainable real-time playback.

The preceding failed checkpoint `31377808f20c8f470ff0f6ae15c3e3980aa7eac6`, executable SHA-256 `52aad712ae87186a1edbd6cf08668e0b35a1ead083f62441acc3269a04550d23`, passed 802 authored checks but failed all 84 paid attempts and four Starter checks with `error converting Lua nil to function`. Its failure-complete audit is retained. The shared widget-persistence correction restores these cases without changing Starter 39's successful waveform behavior.

The historical successful paid-library checkpoint is `542049e9dc615a4e7241042374b12ad254923674`, executable SHA-256 `01d1ea6c94eb188d2346be789e646188ab969b9b3238fa6b1a09b18f8c82120d`, built with the release profile. Its **40/40 short and 40/40 expressive paid renders** complete with finite, nonzero stereo float output and no private or dropped logs. All fifty Starter script checks complete; **zero of fifty complete Starter renders succeed**: forty-nine reproduce their explicit graph-preflight failures, while the sole preflight-clear program fails renderer preparation on LFO waveform type 2. Image-backed JPEG conversion fails explicitly during generator preflight. The following comparisons describe this successful historical checkpoint.

Four additional representative paid renders at 44.1 and 96 kHz pass and are byte-identical to the previous `0f94569` checkpoint. Short outputs contain 74,970 and 163,200 frames; expressive outputs contain 194,040 and 422,400. Their first audible frame is 8,822 or 19,202 respectively, consistent with the authored 200 ms note start plus two samples. Independent complete RIFF, format, byte-rate, block-alignment, extent and finite-value checks pass for all **84 outputs**. The full 48,439-asset FLAC/WAV decode was not repeated.

Compared with checkpoint `0f94569`, all paid successes are preserved and 38/40 short plus 38/40 expressive WAVs are byte-identical. The same two programs change in both batches; maximum absolute expressive window RMS delta is 0.0000322215. Host-command counts remain unchanged. Short event-command totals increase by nine in twenty programs, fifteen in eight and eighteen in twelve because the persistent Player advances Lua and DSP through the common padded processing horizon. WAV extents retain the requested length. Before the immutable copy, the default-feature UVI test run passed 673 unit tests, 89 playback integration tests and one UVI integration test, with 34 ignored tests and one screenshot test filtered. These are checkpoint and file-validation results, not original commercial-program audio parity. Source changes after this checkpoint require their own validation.

The detailed tables and Starter gate inventory below retain the earlier `6d6ecc1` audit as historical evidence; they are not a current count of implemented DSP modules.

The paid corpus comprises 25 locally owned UFS banks. Only aggregate format observations are published here; the inputs, decoded programs/modules, access state and exact identities remain private.

| Check | Validated result | What it establishes |
| --- | ---: | --- |
| Archive directory traversal | 59,529 file paths and 813 directories across 25 banks | Observed linked-leaf/name/path structure, not audio behavior |
| Content-state verification | 25/25 banks; 9 distinct states | A complete protected PNG passed all chunk CRCs in every bank |
| Program decoding and native parsing | 40/40 programs; 330,045 graph nodes | Actual Rust Program parser accepts the decoded graphs |
| Lua source decoding and compilation | 120 source modules plus 40 embedded scripts; 160/160 pass | UTF-8 source compiles with the actual MLua vendored Lua 5.1 compiler; no commercial script execution in this check |
| Serialized script API version | `21` in all 40 ScriptProcessors | Includes the audited original/V2 bank pairs; version alone does not prove host API coverage |
| Audio metadata inspection | 48,440 assets; about 24.8 MB of header reads | Establishes format/header/resource structure; the full decode check below is separate |
| Recorded channel counts | 28,707 mono; 18,492 stereo; 1,227 six-channel; 13 four-channel | Channel preservation is necessary; the separately decoded CAF adds one four-channel source |
| Full FLAC/WAV audio decode | 48,439/48,439 assets across all 25 banks; zero failures | Actual `sample::decode` and packed storage; all 48,300 FLACs pass their stored MD5, all 139 WAVs decode; the unused CAF is outside this check |
| Decoded geometry and finite values | 5,254,583,798 frames; 7,778,267,036 scalar values; zero nonfinite values | Every decoded frame/channel/rate count agrees with its inspected header; WAV has no FLAC MD5 claim |
| Independent channel comparison | One six-channel source; 488,700 decoded scalar values | Exact agreement with the independent libFLAC 16-bit decode for that source |
| Packed-storage regression | Actual current wrapper linked against existing `audio::Pcm` | Exact I16/I24/f32 and negative-zero bits, 1/2/6/10/12-channel scalar order, random/reverse reads, padding and invalid-input checks; separate from a full integration run |
| RIFF sample loops | 26,529 loops in 26,529 assets | Every inspected loop has kind `0`, fraction `0`, play count `0`; this does not validate other loop modes |
| Program IR references | All 240 SampledReverb and 376 Convolver references resolve | Includes four-channel Convolver resources; resource resolution does not validate IR channel mapping |
| Controlled offline render, checkpoint `6d6ecc1` | 40/40 programs complete; all 40 outputs nonzero | Original Program XML, source modules, embedded script and approved bank resources execute in the actual scoped-chain CLI; one short note test per program, not reference-host equivalence |
| Independent render-output validation | All 40 completed outputs: 72,000 frames, stereo, 48 kHz, float32; zero nonfinite samples | Checks every output scalar independently of the CLI report; zero private log messages or dropped logs |
| Expressive baseline, checkpoint `74326b5` | 38/40 programs complete; two DigitalEq frequency-range failures | Retained comparison baseline; these failures are cleared in the later checkpoint below |
| Expressive render, checkpoint `6d6ecc1` | 40/40 programs complete; all 40 outputs nonzero | Exercises the same 24 ordered CC/bend/overlap/retrigger events per program; no operation failures, private log messages or dropped logs |
| Expressive output and window validation | All 40 completed outputs: 211,200 frames, stereo, 48 kHz, float32; zero nonfinite samples | Every output is silent before the first note and nonzero during both overlap windows; later release/tail energy is an observation, not proof of correct voice ownership |

All 14,208 SamplePlayers resolve after retaining the observed starred sibling-file lists. Of these players, 7,292 refer to a single file, 988 to two mono operands, 2,052 to ten and 3,876 to twelve. Expanding the lists resolves all 76,300 member references, covering 29,086 unique sampled audio resources. Every one of the 6,916 lists has matching frame count, sample rate, complete loop tuples and RIFF unity note across its mono operands. The sole CAF has no program audio references.

A subsequent, separate finalized CAF LPCM decoder check closes the sole excluded audio asset: 44.1 kHz, four channels, 22,050 frames of signed big-endian packed 24-bit PCM. All 88,200 decoded scalar values agree exactly with independently installed libsndfile, and packed storage uses 264,600 bytes. Combined with the retained FLAC/WAV pass, **all 48,440 audio assets have decoded successfully**; this does not claim that the previous full pass was rerun after adding CAF dispatch. CAF compressed codecs, indefinite chunk lengths and uninterpreted marker metadata remain outside the admitted path. Authored checks cover endian, integer/float, packet geometry, extent, padding, finite values and resource limits.

The full audio check used a private Rust harness compiled from retained snapshots of the actual sample/storage implementation and linked to the existing sampler's real `audio::Pcm`. Four workers read, decode, verify and drop one asset at a time; they emitted aggregate/per-asset metadata rather than PCM or extracted audio files. All FLACs supplied a nonzero stored MD5, so none counted as verified through an absent checksum. All 139 WAVs additionally passed with the latest native-measured RIFF extent handling. The complete pass took 639.6 seconds on the audit host; this is a recorded run, not a playback-performance benchmark.

The loader assembles ordered mono operands into channel-preserving samples and validates their agreement. Original official-reader probes establish exact stereo equivalence for a two-operand list and channel ordering across all ten and twelve positions of larger lists. Separate authored probes informed multichannel output layouts and keygroup pan behavior. Corpus header coherence, authored assembly checks and these scoped native comparisons are separate evidence; complete bundle/routing parity is not established.

The header survey also found 764 stereo assets whose raw RIFF loop end exceeds the decoded frame count and equals twice that count minus one. Their RIFF format/data dimensions agree with the FLAC dimensions. The decoder preserves those original loop tuples; separate native fixtures establish clamping only the effective playback end to the final frame, without scaling the start. This runtime treatment does not make the raw metadata valid or establish other loop modes.

The programs contain 14,208 SamplePlayers, 6,318 OnePole filters, 6,180 GainMatrix processors, 988 Gain processors, 440 EffectRacks, 1,080 AuxEffects, 160 TrackDelays, 80 DigitalEqs, 160 ThreeBandShelves, 376 Convolvers and 240 SampledReverbs. Their control graphs contain 192,196 SignalConnections, 1,762 ControlSignalMappers, 408 ScriptEventModulations, 291 ConstantModulations and 111 LFOs. These counts establish actual implementation requirements; they are not counts of verified audible processors.

A separately inspected free Starter corpus contains 50 decoded programs and 75 XML node kinds, including generators and effects beyond the paid corpus's sample-based module set. Its wider module diversity prevents treating successful paid graph parsing as general Falcon coverage. The historical official UVI examples likewise include sample-mapping and synthesized sources.

The historical survey used the same retained checkpoint `6d6ecc1` binary on all 50 free Starter programs. All fifty completed original-script initialization and the note/release timeline, producing 385 event commands and 173 host commands with no logged or dropped messages. This clears the two missing-helper failures in the earlier `74326b5` survey. The actual CLI supplied approved bank resource capabilities throughout.

The CLI executes isolated Program and Layer script chains with measured lifecycle ordering, deferred forwarding and layer-scoped release. This clears the earlier single-script gate that prevented eighteen Starter programs from being checked. The complete free corpus contains 79 ScriptProcessors, including 56 at Program scope and 23 at Layer scope; programs contain one to seven processors. This test does not establish all callbacks or complete scoped voice semantics.

All 50 programs that reached renderer preflight had unsupported behavior, so no complete original Starter program was rendered in this survey. The following counts cover those 50 reached programs, overlap, and include processors that are initially bypassed:

| Preflight gate | Programs affected |
| --- | ---: |
| SparkVerb | 35 |
| Control graph | 32 |
| WaveTableOscillator settings | 15 |
| Layer settings | 15 |
| ParametricEQ | 12 |
| Phasor | 11 |
| FmOscillator | 11 |
| PluckOscillator | 10 |
| Drive | 10 |
| DualDelayX | 9 |

The first reported control-graph gates were unsupported source kinds in seventeen programs, unsupported control sources in twelve, ambiguous graph paths in two and an unverified Mode1 producer in one. These are first reported reasons per affected program, not an exhaustive inventory of every missing route. Oscillator gates retain unsupported combinations even when some Analog/wavetable settings are implemented. These observations prioritize actual missing semantics; a supported oscillator alone does not satisfy the effects and control graph of a complete program.

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

The native CLI accepts `--events timeline.json` in place of `--notes` for controller, bend, pressure, transport and overlapping-note checks. Frames use the chosen `--sample-rate` (48 kHz by default), and channels are zero based. Note specifications retain millisecond units. A complete chronological 48 kHz input file can contain:

```json
[
  {"frame":0,"kind":{"Controller":{"channel":0,"controller":1,"value":127}}},
  {"frame":0,"kind":{"NoteOn":{"channel":0,"note":60,"velocity":100}}},
  {"frame":24000,"kind":{"NoteOff":{"channel":0,"note":60}}}
]
```

The interpreter validates timeline bounds, MIDI ranges and finite transport/bend values before loading script sources. This input surface enables offline expressive tests; it does not add audio-thread execution.

`script::Session` keeps one Program-chain VM across ordered inputs and drains, preserving pending callbacks and future commands. Its clock, waits and reported sampling rate use the supplied output rate; sample offsets retain source-time fractions independently of output rate and pitch. Authored tests exercise 70,400 notes and 140,800 callbacks, retained handles after 61 seconds, LIFO held registrations, future outputs, and rejection of out-of-order input before mutation. Pending state, Lua memory and callback work remain bounded; each wait is capped at 60 seconds, and monotonically issued IDs eventually fail on `u32` exhaustion. The source follow-up takes one incremental GC step per drain and falls back to a full collection after 64 drains or 32,768 metadata entries. Weak-handle pruning follows completed collections so inspection does not keep dead userdata alive. This reduces measured VM cost but does not make allocation or collection safe for the audio callback.

`library::BankResources` owns the decoded audio cache and exposes a bank-only resource callback. Alias snapshots share PCM allocations; a revision changes only after a new alias resolves successfully. The cache charges unique packed PCM against 512 MiB and retained alias strings plus inline key/value sizes against 16 MiB; allocator and hash-table overhead are excluded. `Renderer::install_prepared_samples` validates additions before merging, rejects conflicting aliases, and updates processor resource maps without replacing active convolution state. Decoding and preparation must remain off the audio callback.

## Reference comparisons and remaining gates

The checkpoint `6d6ecc1` audit used one retained optimized CLI binary for both paid and free checks, at most four concurrent processes, and a velocity-100 note from 0 to 500 ms followed by rendering through 1,500 ms. Its SHA-256 is `9bd3b12aed4d9c8bf072707da9f3e8b2803fd6bcc0c5a974f2f1ac5477e94585`; the executable embeds revision `6d6ecc14056e063f0e79e3f3a06b2dab1bb035ab`. Every paid program first received note 60; four completed but silent tests produced nonzero output when retried at a note selected from active keygroup ranges. Two further programs produced very quiet default-note output outside their serialized active sample ranges; earlier checkpoint supplemental in-range tests produced substantially stronger output and passed the same independent WAV checks. The current expressive timelines use valid notes for those programs. All 40 final short regression outputs passed the checks above, with no operation failures or logged/dropped messages. This establishes the tested timelines, not every note or expressive control.

The same checkpoint also ran forty expressive timelines using metadata-valid notes, CC1/CC2/CC11 changes, normalized pitch bend, same-key and distinct-key overlaps, releases and retriggers. Each contains 24 exactly ordered events through 3,400 ms, with rendering through 4,400 ms. All forty completed with finite, nonzero outputs and no logged/dropped messages; the two earlier DigitalEq frequency failures are cleared. Independent peak/RMS windows cover held notes, both overlap types, staged releases, retrigger and the final tail. All outputs were silent before the first note and nonzero in both overlap windows. Release/tail energy alone cannot distinguish convolution decay from incorrectly retained voices. The comparison against `74326b5` retains window and command-count changes; 36 of the 38 commonly completed programs have byte-identical PCM output, while two differ with a maximum absolute scalar difference of 0.0018891. These are checkpoint regression observations, not verified expressive fidelity. Short renders took 88.79 seconds and expressive renders 202.70 seconds under this concurrent audit; these are batch timings, not real-time throughput guarantees.

A separate stock Workstation investigation auto-indexed the byte-identical free Starter bank in an isolated Wine prefix, producing one registered volume and fifty preset records. The official VST instantiated and opened its browser, but graphics failures prevented a verified original-preset selection and positive native audio capture. No Program resource paths were rewritten. This establishes bank registration only; it does not supply an end-to-end native reference render.

Original synthetic probes against official UVI hosts informed the implemented OnePole and TrackDelay laws, EQ filter shapes/slopes and selected typed-parameter/widget behavior. Native lifecycle probes establish the implemented order `onLoad` → widget restoration → `onInit`; this corrects the older inferred ordering in the historical audit. A separate 54-case native pan comparison establishes centered mono gain of 0.5 per stereo output and stereo unity gain. Native keygroup-matrix probes preserve the input bus width; a mono input does not acquire twelve channels merely because all matrix coefficient fields exist. Source comments identify the scoped comparisons and retain fidelity diagnostics. These probes do not constitute commercial-preset reference renders or complete module parity.

Native probes now distinguish per-processor posting registrations, per-callback held state, and independent DSP launches sharing one opaque voice handle. Forwarded releases match the key and issuing layer and release the oldest matching launch; gain, tune and fades affect all matching live voices within their issuing scope. Delayed posts survive an early release, and controls before DSP start do not alter their future note. These scoped checks do not establish all native key-buffer or coroutine behavior.

Current explicit limitations include unsupported wider Falcon generators/processors; unresolved volume aliases; protected UFS streaming; full asynchronous load/save/browse behavior; complete editor fonts/units/advanced displays and plugin automation integration; and real-time execution of the allocating Lua host. Preflight or operation-specific errors must remain visible when a behavior is unavailable.

Source corrections after the recorded audit implement per-voice relative controls, declaration-order Constant chains and source-ancestry scoping for Layer-issued ScriptModulation. Their authored/native checks remain separate from the immutable corpus results above.

The native renderer retains diagnostics for its original interpolation, crossfade, voice-stealing and immediate-release choices. Authored native fixtures cover matrix width, mono/stereo pan, selected multichannel output layouts and aligned SamplePlayer gain controls; overlapping keygroup selection and nonaligned control timing remain unverified. DSP bypass transitions, LFO scheduling and modulation timing/rounding, IR routing/normalization/rate conversion and reverb preparation still require scoped reference comparisons. Native random-LFO clock seeds and cross-voice RNG ordering cannot be reconstructed from serialized programs; admitting the measured waveform/smoothing law does not establish matching random output. Initially bypassed unsupported modules are not silently dropped because scripts can enable them later.

The all-bank results above establish directory traversal, independently validated content access, native graph parsing, Lua compilation, header/resource coherence and complete FLAC/WAV decoding with all supplied FLAC checksums verified. The successful render audits additionally establish complete resource preparation and original-script execution for all 40 paid programs under their recorded short and expressive tests; `b4526e7` restores the initialization regression in `3137780`. These results do **not** establish every script branch, controller sequence or articulation, CAF marker semantics, sustained or exhaustive musical behavior, end-to-end reference renders, H.A.T. fidelity or general Falcon compatibility.

The persistent Player/worker source follow-up passed the full default-feature CI-profile command with `--features uvi -- --skip screenshot`: **673 unit tests, 89 playback integration tests and one UVI integration test pass**, 34 tests are ignored, one screenshot test is filtered, and zero doctests are present. The separate no-default-feature UVI integration check also passes. The held source files did not change during the full run.

One subsequent paid expressive test produced byte-identical bulk, 256-frame and dedicated-worker WAVs (211,200 frames at 48 kHz), with identical 5,833 event/438 host-command counts and no private/dropped logs or worker errors. During concurrent build/reference activity, the worker recorded 2.451 seconds initialization and 8.414 seconds in `Player::render` for 4.4 seconds of output; 790 of 825 calls exceeded their 256-frame audio duration. These are wall-time observations on the audit host, not isolated CPU timings or arrival deadlines. Thread/stage profiling and optimization remain necessary before live plugin playback can be claimed.

A later paired capture measured actual thread CPU separately on the same 4.4-second paid timeline, 825 blocks of 256 frames at 48 kHz. Numeric parameter caching and incremental GC reduced combined session/renderer CPU from **8.413 to 6.670 seconds (20.7%)**: renderer 7.575 to 6.449 seconds and session 0.834 to 0.217 seconds. Renderer allocation/reallocation calls fell from 107,825,548 to 18,755,458; requested allocation traffic fell from 2.726 to 1.918 GB, which is not resident memory. Complete PCM hashes match before and after for both the paid capture and a separate authored live-mutation/resampling probe; event and host-command counts are unchanged.

The next capture compares three fixed artifacts in the same run. Indexed graph memo storage and persistent numeric output slots reduce total CPU from **8.458 seconds at baseline to 6.345 with the prior optimization and 4.394 with the new step**: 48.1% less than baseline. Renderer allocation calls become **221,226**, about 1.05 per audio frame, with 89.25 MB requested allocation traffic; session allocation traffic is unchanged from the prior step. All three complete paid and authored PCM hashes match, with 5,833 event and 438 host commands and no logs. The renderer's 54 focused checks also pass against the copied artifact. Average CPU is now close to the audio duration, but **353 of 825 blocks still exceed 5.33 ms wall time**, with a roughly 72 ms CPU maximum. A separate CPU capture still exceeds deadlines in 356 of 819 blocks after explicitly excluding initial preparation and all five note-start blocks. These measurements do not establish sustainable real-time playback or sufficient headroom for a live plugin.

The combined source at `3137780` passes `cargo test --locked --profile ci --features uvi -- --skip screenshot`: **709 library unit tests, two CLI tests, 89 playback integration tests and two UVI integration tests**, with 35 ignored tests and one filtered screenshot test. This includes the native catalog's staged-controller invariants, both existing rack-drop interactions, all eight worker/control-transport checks and the measured filter scalar-startup checks. Native filter observations captured after silence retain their exact vectors and tolerances with an explicit 256-frame silent prefix; cold behavior has separate authored/native fixtures. The held source did not change during the passing run. The subsequent actual corpus audit exposed the shared initialization failure described above, so these authored checks are insufficient to establish preserved paid-library behavior.

The correction separates persistent parameter widgets from stateless Buttons and nonparameter containers in both restore and save. Restoration had requested a `setValue` function from every persistent widget, including native Buttons whose API deliberately has none. A full authored Session now verifies mixed-widget restoration, unchanged push semantics and a save/load round trip; an actual parameter widget with a missing setter still fails explicitly. The corrected held source passes the same full command with **711 library unit, two CLI, 89 playback integration and two UVI integration checks** (804 total), with the same ignored/filtered counts. Its actual immutable paid-library regression audit is recorded above.

Checkpoint `9d69ae0` passes **722 library unit, two CLI, 89 playback integration and two UVI integration checks** (815 total), with the same ignored/filtered counts. It adds the exclusive audio endpoint, scalar Xpander bypass-resume comparisons, bounded DualDelayX diffusion and graph lookup reuse without removing public validation. PNG conversion now rejects heights above 128 and follows measured floating-point normalization; fresh 96/128-row native captures match within 2.98e-8, while six earlier authored waveforms retain their measured bounds. JPEG and larger-image resampling remain gated. The diagnostics contract uses its existing isolated real journal so parallel producers cannot evict the asserted rows from global history; its assertions and production recording are unchanged. Its completed immutable corpus comparison is recorded above; the later isolated graph lookup measurement is recorded below.

The subsequent clean integration `191e48118c95dc38da80a56f23a6152356cac229` preserves the stable 0.3.148 engine/plugin base, byte-identical UVI source from `9d69ae0`, and the separately verified X11 visibility correction. The same full CI command passes **812 library unit, two CLI, 89 playback integration and two UVI integration checks** (905 total), with 35 ignored and one screenshot filtered. Both default and `uvi,standalone,shell` metadata checks and 31 focused integration/startup checks also pass. Its clean, no-default-feature optimized CI executable completed the actual-corpus regression audit recorded above. A separate default-feature, UVI-disabled all-target metadata check passes; the normal/build dependency graph contains no MLua, Lua source, UVI XML parser or FLAC codec. Shared image/support dependencies remain for their existing consumers. This integration does not yet activate live UVI plugin playback.

A subsequent isolated source comparison swaps only the prior graph lookup implementation against the accepted `9d69ae0`/`191e481` implementation, using the same actual Renderer and preserved 0.3.148 SDK dependencies. Three alternating timing pairs plus one separate allocation pair preserve complete paid and authored PCM/command hashes and counts. Median Renderer thread CPU for the 4.4-second paid expressive fixture falls from **4.204 to 3.768 seconds (10.37%)**; contiguous packet CPU falls from 4.375 to 3.934 seconds. Renderer allocation traffic remains identical: 220,326 allocations, 900 reallocations and 89,252,656 requested bytes. Every latest timing run still has **342 of 825 Renderer wall-time deadline misses**, with about 72.7 ms first-block CPU. The coordinated window retained the user's desktop/DAW background processes. These bounded workstation observations establish a CPU improvement, not sustained real-time playback or native-program parity.

The later catalog entry-point correction prevents UVI-disabled builds from scanning and advertising UFS banks, while preserving typed saved references. The actual UVI-disabled Scanner regression and existing UVI-enabled inventory/cache UUID replacement regression both pass. Complete Kontakt disablement and the common live backend dispatch remain planned work in [the player boundary contract](PLAYER_BACKEND_BOUNDARY.md).

The browser/diagnostic follow-up uses block-level instrumented evidence counters,
with explicit unknown rows for uninstrumented nodes. Four alternating baseline
and final-counter captures of the same 4-second paid Clarinet fixture preserve
complete PCM and event/host-command hashes. Packet thread CPU was 2463/2604 ms
for baseline and 2458/2447 ms with final counters on this machine. These short,
scoped pairs show no measured regression; they establish neither a speedup nor
sustained real-time readiness. An attempted scope-plan optimization was omitted
because its earlier alternating timing results did not establish a reliable gain.

## Provenance and reproducibility

Development used public primary documentation, locally owned program/resource observations, static analysis of supplied official UVI reader executables, and original synthetic host probes. This is **not clean-room development**. Mathematical/format observations must be distinguished from vendor implementation expression, and access to the official reader must remain accurately recorded. See [contribution provenance rules](../CONTRIBUTING.md#rights-and-provenance) and [third-party records](../THIRD_PARTY.md).

Public runnable checks use authored fixtures: `python3 tools/inspect_uvi.py --self-test`, `python3 tools/inspect_uvi.py --self-test-recovery`, and the UVI module tests with the `uvi` feature. Those checks exercise the public implementation without distributing the commercial corpus. The paid-corpus validation additionally used private inputs with the real Rust graph parser, MLua Lua 5.1 compiler, full sample decoder and optimized offline CLI; it cannot be reproduced from public synthetic fixtures alone. Private acquisition records, access state, extracted scripts/audio/IRs and path lists are not committed or attached to reports.
