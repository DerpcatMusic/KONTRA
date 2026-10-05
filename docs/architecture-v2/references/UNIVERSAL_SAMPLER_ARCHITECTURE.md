# Universal sampler core: research and Rust architecture

**Research date: 5 October 2026.** Targets: Kontakt/KSP, Falcon/UVI-script, UVI Workstation, Decent Sampler, SampleTank, and extensible support for other multisampler formats.

**Status:** architectural research and proposed implementation contract. No claim is made that proprietary engine internals have been inspected, that file/asset access is solved, that this design is implemented, or that any performance numbers have been benchmarked. The accompanying JSON is a conformance-scenario catalogue, not an executed test suite.

## Executive decision

Build a **deterministic musical-event kernel, versioned compatibility frontends, a compiled multirate render engine, and an asynchronous asset system**. Do not build a generic sample player and bolt scripting onto it afterwards.

The central abstraction is a **logical performance note with an identity and lifecycle**, not an audio-file playback cursor. One incoming note can be suppressed, transformed, delayed, split into child notes, select multiple recorded layers, own multiple microphone voices, survive pedal hold, generate release samples, and remain relevant to suspended scripts after its attack source has finished.

Compatibility belongs in explicit semantic profiles. Shared execution primitives are desirable; pretending that every vendor's groups, waits, release triggers, parameter units, or voice IDs mean the same thing is not.

The first delivery should prove the difficult musical semantics with a headless renderer and tiny fixtures. A browser full of formats is not evidence of compatibility.

---

## 1. Evidence that changes the design

This section is a compact evidence map. The later architecture is a proposal derived from these constraints, not a description of unpublished vendor implementations. Source identifiers resolve in the reference registry at the end.

### Kontakt

Kontakt's hierarchy separates instruments, groups, zones and samples. Group conditions include keys, controllers and variation selection; source modes include conventional playback and more expensive time/pitch processing. This requires preserving the original scope of mapping and DSP decisions rather than flattening everything to key ranges. [NI01]

Group insert effects operate per voice; buses and instrument inserts operate on summed signals. Thus a group compressor cannot automatically become a bus compressor during import, even when the visible parameter values match. [NI02]

KSP is an event-and-state language, including generated events, callbacks, persistent state and note-associated polyphonic variables. The Kontakt Lua API instead serves instrument creation/editing. Those are separate compatibility surfaces. [KS02][KS03][KS04][NI04]

### Falcon and UVI

The public object model exposes programs, layers, keygroups and oscillators; these are not interchangeable aliases for Kontakt groups and zones. Falcon is a hybrid source engine, so mapping ordinary PCM does not cover all programs. [UV07][UV08][UV09][UV10][UV12]

UVI-script documents a customized Lua 5.1 runtime, cooperative callbacks, a restricted environment and pooled memory for real-time execution. Lua syntax support alone does not reproduce that environment. [UV03][UV04]

Defined UVI callbacks must forward events when appropriate; defining `onEvent` changes dispatch by taking precedence over specialized callbacks. This is a reason to model event routing and state visibility explicitly. [UV05]

The public sample-mapping guide additionally describes key, velocity and two script-supplied dimensions. Treat this as a version-dependent capability, not evidence that every deployed Falcon/Workstation build has the same feature. [UV15]

### UVI Workstation

Workstation's part settings include MIDI routing, streaming, key limits, velocity remapping and a part-mute keyswitch. The velocity setting is not simply an instrument's sample velocity-zone range. Preserve the distinction between host/part performance transforms and internal sample selection. [UV14]

### Decent Sampler

The declarative format now covers considerably more than key/velocity rectangles: trigger modes, controller conditions, variation selection, oscillators and audio routing are documented. Its `random` and `true_random` options are not synonymous. [DS02]

MIDI bindings can affect controls and group behavior, and note bindings can consume notes. Therefore the control-value model must continue to exist with no graphical editor open. [DS03]

### SampleTank

SampleTank's public materials emphasize multitimbral parts, articulation editing, streaming, a modulation matrix, performance processors and effects. Model the performance generator as a separate subsystem. Public material reviewed here does not establish a documented third-party scripting contract comparable to KSP/UVI-script. [IK01][IK02]

### Other engines worth studying

SFZ is an excellent vocabulary for declarative conditions, but release behavior is explicitly documented as varying between players. A single generic “SFZ compatible” flag is therefore inadequate. [SF01][SF02]

sfizz demonstrates region/voice/resource separation. LinuxSampler documents a mostly format-independent script engine extended by format-specific functions. HISE exposes generated-note attachment and delayed-note safeguards. These are useful references, not proofs of Kontakt compatibility. [SF03][LS01][HI01]

---

## 2. Define compatibility as a vector

For every imported instrument record at least six independent results:

| Dimension | Question | Example failure |
|---|---|---|
| Asset access | Can the authorized data and referenced resources be read? | Missing container decoder or unavailable sample |
| Structural import | Are hierarchy, mappings, loops and routing represented? | A nested keygroup is flattened incorrectly |
| Event behavior | Do gestures choose and schedule the same notes/regions? | A consumed keyswitch leaks into playback |
| Script behavior | Do programs observe the same values, timing and identities? | A resumed callback targets the wrong note |
| Audio behavior | Are sources, filters, envelopes and effects equivalent? | A substituted stretch algorithm sounds different |
| State and presentation | Do controls, automation and recall behave correctly? | A hidden control loses its restored value |

Recommended feature statuses: `exact`, `translated_with_verified_semantics`, `approximate`, `unsupported`, `blocked_asset`, and `unverified`. “Exact” needs a stated reference version and test evidence; importing successfully is not enough.

Attach every diagnostic to a source object path, feature, source version, and audible/behavioral impact. Example: `Program/Layer[2]/Keygroup[4]/Oscillator[1]: stretch algorithm substituted; note timing verified, timbre unverified`.

Provide strict import and explicit best-effort import. Strict import should not silently omit an unknown opcode or replace an unavailable oscillator with ordinary playback. Best-effort import may do so only with a visible report and a persistent approximation marker.

Keep file access and authorization outside the render architecture. A readable filename, a supported container signature and a playable instrument are different milestones. The user's own asset-access approach can plug into a narrow authorized-asset interface.

A useful compatibility profile contains:

```text
vendor + product family + engine/version range
format dialect and parser revision
parameter defaults, inheritance, units and transfer curves
callback dispatch and event-forwarding rules
note pairing, pedal and release-selection rules
random/sequence scope and advancement rules
source/effect implementation identities and quality modes
persistence order and supported UI/control semantics
fixture-set revision and known limitations
```

Do not make `latest` a behavior identifier. Store a concrete profile in saved projects so an engine update does not unexpectedly reinterpret an old song.

---

## 3. Preserve structure, compile execution

Use three representations, with distinct responsibilities.

### 3.1 Source representation

Keep vendor object identities, ordering, names, indices, defaults and extension fields. Preserve unrecognized material where practical for diagnostics and future re-import. This representation is for editing, inspection and faithful interpretation, not audio rendering.

A keygroup has an identity even if its mapping can later be lowered into a region index. A script that asks for layer 2's fourth keygroup must not accidentally address an optimization-generated node.

### 3.2 Semantic instrument representation

Express explicitly:

```text
assets and sample views
performance dimensions and articulation state machines
mapping predicates and selection policies
event processor graph and scripts
source factories and source-mode parameters
modulation graph, scope and update rate
audio routing graph and channel layouts
logical controls, host automation and persistence
compatibility profile and unresolved requirements
```

A sample view is not the same as a sample asset: many views can use different roots, loops, offsets and tuning over the same immutable decoded data.

### 3.3 Prepared render plan

Compile to immutable, cache-friendly tables and specialized kernels. Resolve names, parameter IDs, opcode dispatch, graph topology and most inherited defaults before activation. Render-time mutable state belongs to bounded arenas: notes, sources, envelopes, script continuations, smoothing state and queues.

Retain a mapping from prepared objects back to source identities for scripts and debugging. A slow authoring representation and a fast render representation are compatible goals when they are not the same data structure.

### 3.4 Keep the graphs separate

The authoring tree organizes objects. The event graph routes and transforms gestures. The modulation graph computes parameter changes. The audio graph combines signals. These have different edges and lifetimes.

A script can route an event to a layer that is not its audio parent. A global LFO can modulate many voices without making their audio monophonic. A shared reverb can receive multiple buses without creating shared voice ownership.

Treat audio feedback as an explicit delayed edge or a supported internally causal processor. Do not accept arbitrary zero-delay cycles merely because a graphical editor can draw them. Modulation feedback also needs a declared evaluation rule.

---

## 4. The musical kernel: identity before sound

### 4.1 Four identities

Separate these objects:

**Input note token.** The original external event address, including protocol, port, channel, key and optional host note ID. Preserve it even after transposition or rerouting.

**Logical note.** A musical event instance owned by the engine. It can be a transformed input note or a script-generated child. Give it a generational handle, parent link, ownership policy and compatibility-visible IDs.

**Voice family.** A set of render voices that form one coordinated attack or transition. Examples include microphone perspectives, phase-related layers, and selected dynamic layers. Family structure is configurable; some imported profiles intentionally behave differently.

**Render voice.** One stateful source and its voice-local processing. A granular source may internally own many grains without exposing every grain as a host note.

This prevents a per-note command from accidentally meaning “change one arbitrarily selected sample cursor.” Conversely, APIs that genuinely target one oscillator/voice must not be widened to the whole family.

### 4.2 Generational handles

Use an index plus generation counter. Delayed scripts retain the full handle. Reusing a slot must not let a stale fade, note-off or tuning command modify the replacement voice.

Host IDs and script IDs are namespaced external identities, not direct array indices. They can be negative, absent, reused, or scoped differently. Translate through verified adapter rules.

A bounded note arena needs admission control. Before accepting a generated note, reserve enough ownership and cleanup capacity to finish it later. Never accept unlimited note-ons and discover at note-off that the terminal queue is full.

### 4.3 Lifecycle state

Do not collapse these into a Boolean `playing`:

```text
physical key state
logical gate state
pending onset / scheduled cancellation
sustain-held or sostenuto-captured state
attack/sustain/release envelope phase
source playback completion
release-trigger eligibility and completion
pending child notes and script continuations
per-note DSP tail completion
host terminal-notification state
```

A shared bus reverb tail is normally not a separate lifetime for every contributing host note. Preserve per-note ownership where it exists, but do not retain all input note IDs until a global reverb becomes silent.

A note may be logically held while its one-shot source has already ended. A key can be released while its envelope is still sustained by a pedal. A source can be stolen while a callback still exists. Each situation needs a specified transition.

### 4.4 Note pairing

MIDI 1 note pairing without explicit IDs can be ambiguous for overlapping identical notes. Choose a native policy and test each imported profile; do not claim one FIFO/LIFO rule is universally correct.

Host-native addressing must follow its own contract rather than that MIDI fallback. CLAP, for example, has note IDs, wildcard addressing and distinct terminal events. Its native velocity-zero note-on is not the MIDI 1 velocity-zero note-off convention. [HO01]

Preserve original and transformed addresses separately. A transposed note must release the transformed sound while the host is notified using the original external identity.

---

## 5. Event time and state visibility

### 5.1 Sample time is the render clock

Use a monotonically advancing engine sample clock with a sufficiently wide integer representation. Keep host musical position, tempo and transport epoch separately. A transport seek is not a reversal of the allocator's lifetime clock.

Scheduled jobs have a clock domain: absolute engine sample, musical beat, or permitted non-real-time wall-clock task. Convert at defined boundaries. Beat-scheduled jobs must respond to tempo changes according to their profile rather than always using the tempo at creation.

KSP `wait` suspends the callback, not the audio thread; its timing API also includes beat ticks and a separate timer with different behavior during offline rendering. UVI likewise exposes cooperative waits. [KS05][UV03]

### 5.2 Render between event boundaries

Within each audio block, merge external events, internal note events, timer resumptions and automation discontinuities. Render only until the next event boundary, apply the ordered transitions, then continue.

Conceptually:

```text
while cursor < block_end:
    boundary = earliest relevant timestamp or block_end
    render(cursor .. boundary)
    apply events due at boundary in a defined stable order
    resume due continuations within fuel and queue budgets
    cursor = boundary
```

This is a specification sketch, not ready-to-paste code. A real implementation must handle events exactly at block end, zero-length blocks, newly scheduled events at the current timestamp, cancellation, and a bounded number of zero-time transitions.

Preserve host order for equal timestamps unless a documented adapter rule says otherwise. Do not globally reorder note-offs before note-ons: a keyswitch, controller or expression event at the same sample can depend on ordering.

### 5.3 Projected MIDI state

Maintain raw input state separately from the state visible downstream of each event-processing stage. A controller swallowed by a script must not have already changed the downstream modulation state merely because a global CC array was updated on input.

Likewise, the definition of “keys currently held” may refer to physical input, a processor's incoming stream, or a transformed logical-note set. Expose the correct projection to each compatibility API.

This is a frequent architectural blind spot: perfect timestamps cannot repair state that was made visible at the wrong stage.

### 5.4 Immediate script observation

A command followed by a query in the same callback needs the source runtime's read-after-write semantics. Deferring every script command to the end of the audio block changes behavior.

Use immediate bounded logical-state updates and correctly timed DSP parameter events. Defer only operations whose API is actually asynchronous. For expensive structural changes, prepare new state outside rendering or support a bounded prepared variant; do not invent asynchronous semantics for a synchronous vendor call.

---

## 6. Articulation is a policy system, not a keyswitch table

Represent performance dimensions independently, with explicit scope and update timing.

| Dimension | Required distinctions |
|---|---|
| Key | Played key, transformed key, root, key tracking, keyrange versus transpose |
| Velocity | Incoming velocity, transformed velocity, sample selection, amplitude curve, release velocity |
| Controller | Gate predicate, sample-triggering event, continuous modulation, latched state |
| Switch | Momentary, latched, next-note-only, previous-key, held-key, exclusive or additive |
| Channel | Port/channel part routing, articulation selection, expressive member channel |
| Variation | Sequential RR, random, no-repeat random, shuffle bag, script-selected variation |
| Phrase | First, legato, repeated, interval, direction, elapsed gap, speed, held-note priority |
| Release | Key release, gate release, pedal release, source end, explicit scripted release |
| Coordination | Microphone family, dynamic layer, transition pair, exclusive group |

A key can act as a control without sounding. Velocity can select a layer without changing amplitude. A CC can generate a pedal-noise sample without any note-on. A legato transition can need the previous note even if the new note's source starts after a delay.

### 6.1 Compile selection

Do not test every region against every note in a large library. Build candidate indices by key and coarse routing scope, then intersect with active articulation/controller masks and evaluate remaining predicates.

Evaluate shared predicates once. Use sparse candidate lists when occupancy is low and bitsets when groups are dense. Benchmark the crossover rather than assuming bitsets always win. Dynamic script changes invalidate only the affected index/activation data where the API permits it.

Avoid materializing a complete Cartesian product of key × velocity × articulation × RR × microphones. Store actual regions and factorized selection rules.

### 6.2 Round robin

RR requires a specified scope and advancement rule. Per key, per group, per channel, per articulation and global sequences are different instruments. Advance on a matching gesture, an accepted note, or a created voice only when the profile says so.

Native default: choose a take once for a coordinated family, then map all microphone perspectives to that take. Imported behavior can override this. Separate release variation can have a different number of takes and a different counter.

Store the attack's decision record even when releases intentionally use an independent policy. Do not force every release to reuse the attack's RR index. SFZ documentation explicitly illustrates incompatible release matching and counter behavior across engines. [SF02]

Use explicit random policies. A “no immediate repeat” draw is not a shuffle bag, and both differ from independent random draws. Save seeds/counters where repeatability is requested; source-runtime randomness may require separate characterization.

### 6.3 True legato

Separate three features: mono note priority, pitch glide, and recorded interval transitions. A glide is not a substitute for a recorded bow or breath transition.

A transition policy may inspect from-note, to-note, interval sign, velocity, overlap, time gap, articulation and transition age. It may select a special source, delay a sustain entry, shorten a previous source, or suppress a release. All generated sources remain attached to logical ownership, not guessed later from pitch.

---

## 7. Pedals, releases and expressive MIDI

### 7.1 Pedals

A piano-capable core needs more than `CC64 >= 64`. Provide separate support for sustain, sostenuto capture, half-pedal damping, repedaling and pedal-noise triggers. These are optional instrument policies, not mandatory behavior for a drum kit.

Sostenuto captures an eligible set at its transition; later notes should not automatically join that captured set. A pedal-up can release many notes simultaneously, so release-voice allocation and event bursts need bounded but sufficient capacity.

Differentiate all-sound-off, all-notes-off and reset-controllers. Panic must resolve note ownership, pending onsets and script-generated children, not simply zero the current output buffer.

### 7.2 Release decision context

Keep attack articulation, onset velocity, elapsed duration, mapping generation, sequence choices, microphone family and controller snapshots available to the release policy.

Then explicitly select among policies such as:

```text
use attack snapshot
re-evaluate selected state at key release
wait for effective gate/pedal release
use independent release mapping and sequence
use only script-generated release voices
allow release noise without a surviving attack source
```

A native violin can keep its attack articulation for release after a keyswitch changes. An imported script may deliberately consult the current articulation instead. Supporting both is the purpose of a profile.

### 7.3 Protocol-neutral expression

The canonical event model should preserve high-resolution values and protocol addressing rather than immediately turning everything into MIDI 1 bytes. Include note-on/off, pressure, pitch, CC, bank/program selection, RPN/NRPN state, channel modes, host parameter automation and transport.

MPE uses channels to carry per-note expression. Keep expressive member-channel identity separate from musical part/articulation routing, otherwise a channel-based articulation patch can randomly switch while playing an MPE controller. [MI01]

VST3 can provide a note ID and separate note tuning. CLAP has its own expression and parameter-modulation contracts. Do not add a host's global modulation twice when its scoped value already incorporates that contribution. [HO03][HO01]

Treat MIDI 2.0/UMP as a separate adapter with preserved group and controller precision. Do not claim that every vendor accepting internal per-note controller commands accepts external MIDI 2.0; the fetched KSP documentation explicitly limits its current callback to internally generated events. [KS02]

Microtuning should be an optional note-tuning service with native tables and adapters for supported external tuning protocols. Specify whether changes retune existing notes or future notes only. Do not hide it in a final coarse transpose.

---

## 8. Sound-source and DSP architecture

### 8.1 Source contract

A source implementation declares its output layout, persistent state size, scratch requirements, input asset dependencies, latency, tail behavior, supported rate changes and cost class.

The baseline family is ordinary sample playback. Additional families include tempo-independent stretching, granular playback, sliced loops, wavetable/VA/FM or other synthesis where required by imported instruments. These share note ownership and modulation infrastructure without pretending to be the same DSP algorithm.

Expose source capabilities rather than a universal list of meaningless parameters. A continuous scrub parameter, for example, is not equivalent to an offset consumed only before the first rendered sample.

### 8.2 Ordinary playback

For transposition by `cents`, a basic source-position increment in frames per output frame is:

```text
increment = source_sample_rate / output_sample_rate * 2^(cents / 1200)
```

Compute or ramp pitch at the required rate. Keep position precision adequate for long files; do not use a low-precision float cursor without testing accumulated phase and loop behavior.

Separate a direct-read exact-ratio fast path from general interpolation. It is valid only when position, rate, direction and boundary conditions actually permit it. Do not silently select a cheaper source mode because an expensive patch is under load in compatibility mode.

Bandlimited rate conversion requires filtering appropriate to the conversion ratio; interpolation order alone is not a guarantee against aliasing when playing samples faster. The textbook treatment gives the relevant lowpass/rate relationship. [DP01]

Native quality choices to evaluate: low-cost linear/cubic modes for explicitly chosen character or preview; a table-driven/polyphase windowed-sinc path for normal quality; wider filters for offline/high quality. Select taps and phase-table size from measured passband error, stopband behavior and CPU cost, not marketing names.

### 8.3 Loops and boundaries

Specify start/end indexing, inclusive versus exclusive loop boundaries, forward/reverse/ping-pong motion, finite loop counts, sustain-loop exit, loop crossfade and release-tail playback. Reject invalid lengths safely.

Provide guard samples for interpolation across page and loop boundaries. Crossfade loops require two source positions and therefore an expanded asset-demand window. A reverse or granular voice can invalidate a purely sequential prefetch assumption.

Use independent envelopes and source-end rules. A source reaching its end can complete before a logical note is released; a release envelope can terminate a source before its recorded tail ends.

### 8.4 Stereo and microphones

Preserve channel layout, channel order, pan law, balance versus true panning, width operations and authored inter-microphone offsets. Do not “correct” recorded microphone delays automatically.

Share source position and modulation computation across channels only when they are mathematically identical. Separate microphones usually share a take decision but contain different PCM; their reads and mixing do not disappear merely because their note ID matches.

### 8.5 Filters and effects

Compile per-voice processing separately from family, layer, bus, part and master processing. Label every node with state scope. Preserve pre-envelope versus post-envelope position and serial versus send routing.

In general `f(a) + f(b)` is not `f(a + b)`. Nonlinear distortion and compression make this particularly obvious. Even linear processing needs matching coefficients, states, initialization, automation and routing before it can be safely moved or shared.

A missing proprietary filter can be approximated by a native one, but the import report must identify the substitution. Matching a cutoff label is not matching resonance, drive, oversampling, saturation, smoothing or internal gain.

### 8.6 Latency

Keep source lookahead, processor delay and host-reported latency explicit. Align parallel paths where the chosen processing requires it. A known sample can be prefetched, but future live modulation cannot simply be assumed known.

Time-stretch algorithms have their own input/output timing and automation alignment. The Signalsmith author documentation is a useful example of why one global “delay samples” integer is not the whole algorithm contract. [DP02]

---

## 9. Morphing is several different operations

**Discrete selection** chooses one source by velocity, switch or controller region. It changes articulation/timbre without requiring simultaneous rendering of both choices.

**Amplitude crossfade** mixes sources with gains. For two zero-mean signals with powers `P_A`, `P_B` and correlation `rho`:

```text
P_out = g_A^2 P_A + g_B^2 P_B + 2 g_A g_B rho sqrt(P_A P_B)
```

This is the expansion of the squared mixed signal and explains why a single universal crossfade curve is wrong. Equal-power curves fit equal-power uncorrelated material; linear complementary gains preserve amplitude for identical in-phase material. Recorded layers are not guaranteed to meet either assumption.

**Parameter morph** interpolates controls of one source/processor. Pitch should normally be interpolated in a pitch domain, frequency in an appropriate perceptual domain, and gain with an explicit amplitude/dB rule. Discrete model changes need a defined transition, not interpolation of enum integers.

**Spectral-envelope morph** reshapes spectral characteristics. Kontakt AET uses analyzed spectral information and morph maps; it is not just a gain crossfade between two simultaneously played samples. Supporting this requires a distinct analysis-data and processing path. [NI03]

**General audio/timbre morph** can require aligned time-frequency analysis, phase handling and resynthesis. Treat this as its own source/effect family with explicit latency, memory and quality choices. Do not make it a dependency of ordinary drum playback.

### Silence does not always mean a voice can be deleted

A dynamic layer at zero gain may need to become audible halfway through a held note. Options are: keep rendering, retain a virtual timeline and reconstruct state later, or make restart behavior an explicit native instrument choice.

Virtualizing the cursor alone is insufficient when a filter, stretch window, random process or convolution history must be restored. Exact reconstruction may cost more than keeping a cheap path active. Distinguish exact silence elimination from perceptual/approximate virtualization.

Use an explicit layer state model: audible, prepared-but-muted, virtualizable, and fully dormant. The compiler and source implementation decide which transitions are valid.

---

## 10. Modulation: scope multiplied by rate

Describe every modulation source and destination by scope, rate, units, initial value, smoothing behavior and dependency set.

Scopes may include engine, part, layer, logical note, voice family and render voice. Rates may be load-time constant, note-on-only, event-updated, control-rate or audio-rate.

Native optimization: compute a shared LFO once only when its phase, reset policy, random state and all inputs are shared. A same-frequency LFO retriggered separately per note is not shared state.

Compile fan-out, constant-fold fixed subgraphs and recompute event-driven values only when dirty. Batch compatible voice kernels in SIMD-friendly layouts. Avoid recalculating identical controller transforms for every sample of every voice.

Do not globally demote modulation to block rate. Fast pitch modulation, filter movement and sample-accurate host changes can require finer resolution. A control quantum is an implementation option with tested error, not a universal fidelity guarantee.

### Units belong to the parameter type

Use typed domains such as `Cents`, `Semitones`, `Hertz`, `Decibels`, `LinearGain`, `Seconds`, `Samples` and `Beats`. Preserve vendor units at the adapter boundary.

The KSP/UVI guide illustrates major conversion hazards: microseconds versus milliseconds for waits, KSP millicents versus UVI fractional semitones for tuning, and millidecibels versus decibels/linear volume. Indexing and normalized parameter spaces also differ. Do not implement a generic division-by-1000 shim for every command. [UV02]

Smoothing belongs at the appropriate semantic destination and unit domain. Preserve deliberately immediate changes, such as a start offset that must take effect before rendering begins. For filters, stable coefficient update schemes matter more than mechanically lerping every stored coefficient.

---

## 11. Disk streaming is a deadline system

### 11.1 Asset model

An immutable asset record describes content, codec, channel layout and sample rate. A sample view adds root, loop and playback metadata. A decoded page cache is shared across views and notes; cursors and DSP history are not.

Cache keys should include asset identity and decode/analysis revision. Content hashes are useful but compute them outside rendering. Path remapping is a load-time concern; do not run filesystem searches from note-on.

### 11.2 Resident data

Preload the attack and any required start-offset interval. Keep loop hot spots and interpolation guards available where the budget allows. Do not assume the first few milliseconds are enough for every legal seek, reverse or stretch operation.

Choose preload policy from expected seek cost, source rate, block size and available RAM. Expose requested versus admitted preload and make startup readiness visible.

### 11.3 Scheduling

A source reports future data windows with a deadline in engine time. Workers perform file access, decompression and decode into a bounded cache. Prioritize imminent attack and continuation pages over speculative work; deduplicate requests across voices.

A page request must describe actual source demand, including pitch ratio, reverse motion, stretch analysis windows, two-sided loop crossfades and interpolation support. “Read next page” alone is insufficient.

### 11.4 Callback contract

The real-time renderer never waits for a disk page or a worker mutex. Missing data uses a documented native policy, such as rejecting a not-ready onset, fading a starved source, or an explicitly enabled approximate continuation. Record the incident without synchronous logging.

Reference profiles may have different live-versus-offline behavior. The UVI mapping guide explicitly describes asynchronous loading and different handling when rendering offline. Do not infer a successful load merely from a request being accepted. [UV15]

For a native offline renderer, prepare data outside the time-critical render call or use a distinct non-real-time execution contract. A plugin adapter must honor what its host allows; offline is not a blanket permission to ignore every host constraint.

### 11.5 Memory lifetime

New prepared plans receive a generation. Existing voices retain the old generation they need. Swap admission for new notes atomically, then reclaim old assets only after their last required use, outside the callback.

Bound the number of retained generations. Otherwise rapid preset changes with long tails can retain unbounded memory. When the limit is reached, use a specified transition, refusal or fade policy rather than blocking the audio thread.

---

## 12. Scripting architecture

### 12.1 Shared services, separate language frontends

Provide a typed internal command/query interface for note creation and ownership, routing, parameter access, waits, control state, persistence and asynchronous tasks. Every operation declares allowed execution contexts, cost class and target scope.

KSP and Lua can use those services while preserving different language semantics. Do not force both to become one source language. A function-name translation is not a runtime implementation.

### 12.2 KSP frontend

Parse and validate into a typed representation, then execute compact bytecode. Preserve integer behavior, real conversion, arrays, global versus polyphonic state, callback restrictions and versioned built-ins.

Use fixed-capacity or load-time-sized stacks and continuation storage. A suspended callback stores program counter, call state, note association and wake condition. It does not own an OS thread.

Important implementation stages:

| Stage | Coverage |
|---|---|
| Musical minimum | Note/release/controller callbacks, suppression, generated notes, group eligibility, event parameters |
| Ownership and time | Child-note links, waits, cancellation, fades, callback identity, delayed release |
| State | Persistent variables, snapshot restoration, inter-script communication, asynchronous completion |
| Control model | Logical widgets, value callbacks, host automation, built-in UI state and notifications |
| Extended API | Engine parameters, MIDI objects, newer callbacks and version-specific commands |

Do not postpone the first meaningful KSP vertical slice until after every open format works. Kontakt compatibility is a central target; it must challenge the core design early.

A generated event may have a release relationship different from its duration. KSP documents special `play_note` duration values and offset limits; represent these as explicit semantics rather than magic values passed unchanged to an unrelated backend. [KS04]

### 12.3 UVI/Lua frontend

Start with a language/API capability manifest. Required components include Lua value behavior, tables, closures, coroutine continuation, engine object handles, event forwarding, per-voice changes, widgets, persistence and asynchronous operations.

A generic Rust Lua binding is useful for an API prototype or a non-real-time control environment. It does not by itself implement UVI's custom pool, callback scheduler or object model. [RU01][UV04]

A production real-time Lua path needs an audited allocator, bounded continuations, controlled garbage collection strategy, instruction budgets, restricted built-ins and safe failure transitions. An instruction hook alone does not bound a long native function such as an expensive string operation or sort.

Do not promise that arbitrary Lua can be fully compatible, unboundedly expressive and provably bounded in time simultaneously. Define resource limits and a compatibility status when a script exceeds them. A custom Rust interpreter is also a large semantic project, not a shortcut around this problem.

Keep UVI control creation and restore order versioned. A widget callback is not automatically a yieldable musical coroutine. Translate the actual allowed context rather than assuming every callback can `wait`.

### 12.4 Native authoring API

For new instruments, offer declarative mapping/state machines and a bounded event language or bytecode API for musical behavior. Lua can provide authoring, tooling and control-plane flexibility. Handwritten Rust DSP modules remain the high-performance source/effect implementation path.

Native safety defaults can be better than historical quirks. Keep those improvements opt-in when importing a library whose behavior depends on the old semantics.

### 12.5 Security and failure

Treat presets and scripts as untrusted input. Bound parse depth, decompressed size, array sizes, callback count and scheduled-event growth. Restrict asset paths and module access. Avoid network and arbitrary OS access in the real-time environment.

On script failure, stop that script's future generation, release or cancel its owned notes according to a defined policy, and surface a precise diagnostic. Do not silently turn off the script and leave its children sounding forever.

---

## 13. Logical UI, automation and persistence

The visible editor is a view of an instrument state model; it is not the owner of musical controls.

Controls need stable identities, value domains, defaults, enabled/visible state, callback links, host mappings and persistence flags. These objects exist headlessly. Opening or closing the editor must not initialize/reset musical state.

Serialize a coherent snapshot, not a mixture of values sampled while the audio thread is mutating them. Decide which state is musical recall, which is transient voice state, and which is optional exact-continuation state.

Persist asset references, compatibility profile, parameter values, switch state, script-persistent data and versioned migrations. Save RR/random state when the native instrument's reproducibility setting requests it; importing a source profile may require different rules.

Host automation IDs must remain stable across sessions and graph optimization. Distinguish a host automation assignment from a UI widget index. A parameter rename should not silently redirect an old DAW automation lane.

Program changes and live-set switches use prepared slots or asynchronous readiness states. Define whether old voices finish, crossfade or are choked. Keep old ownership valid until that transition actually completes.

---

## 14. Rust module boundaries and real-time discipline

A proposed workspace:

```text
sampler-types        IDs, units, events, clocks, protocol-neutral contracts
sampler-ir           source-preserving and semantic instrument models
sampler-compile      validation, indices, graph lowering, specialization
sampler-event        routing, note state, articulation, scheduler, ownership
sampler-voice        bounded arenas, family allocation, stealing, lifecycle
sampler-dsp          sources, interpolation, envelopes, filters, mixing
sampler-stream       assets, decode workers, page cache, preload and deadlines
sampler-script-api   typed musical command/query services
sampler-ksp          parser, semantic checks, bytecode, compatibility adapter
sampler-uvi          Lua runtime integration, UVI objects and callback profile
sampler-import-*     format frontends; no direct render-thread dependencies
sampler-state        controls, automation, snapshots, migrations
sampler-host-*       CLAP / VST3 / standalone adapters
sampler-render       headless rendering and event-trace tool
sampler-conformance  fixtures, differential tests and benchmark manifests
```

These are responsibility boundaries, not a requirement to immediately create fourteen separate crates. Keep build complexity proportionate; split when it enforces a useful dependency constraint.

### 14.1 Hot path

Use compact typed IDs, contiguous arrays, preallocated arenas, explicit kernel variants and data layouts suited to the workload. Traits are useful at architecture boundaries; avoid per-sample dynamic lookup by strings or large object hierarchies.

A structure-of-arrays or array-of-small-structures layout can help batch envelopes, gains and filters. PCM reads at unrelated cursors can be less SIMD-friendly than arithmetic; measure rather than assuming wide vectors eliminate memory costs.

Precompute lookup tables and prepared filter data outside rendering. Keep cold metadata out of hot voice state. Avoid unpredictable branches where a prepared specialized kernel can represent the same semantics.

### 14.2 Forbidden-by-default callback operations

No file/network access, blocking locks, dynamic thread creation, general-purpose task spawning, heap growth, synchronous text logging, or unbounded data-structure work. Avoid deallocation too: dropping the final `Arc` can destroy a large graph on the audio thread.

Use explicit deferred reclamation and bounded transfer queues. Every full-queue case needs a policy. Reserve capacity for releases and cleanup; control telemetry can be lossy, ownership transitions cannot be casually discarded.

`rtrb` is a documented fixed-capacity wait-free SPSC option; it does not solve multi-producer scheduling, payload allocation or overflow policy for you. [RU02]

Symphonia is a useful pure-Rust decode/demux candidate on workers. It is not a Kontakt/Falcon interpreter or a proof of sample-exact metadata import. [RU03]

### 14.3 Multicore

Start with a reliable serial renderer. Add coarse parallel render tasks only after measuring the crossover and respecting host scheduling. Do not give every voice or instrument its own OS thread.

A DAW already schedules plugins. Extra private pools can oversubscribe cores and worsen small-block deadlines. The CLAP host-pool extension is optional and explicitly cautions about synchronization and hard-real-time behavior. [HO02]

Use a serial fallback and deterministic reduction order where repeatability matters. Shared global Rayon-style work queues are not a default audio-callback strategy.

### 14.4 GPU

Do not place a discrete-GPU synchronization round trip on the default short-buffer note-render path. Evaluate GPU work for offline analysis, waveform preparation or sufficiently large optional batches with measured latency. This is a design recommendation, not a claim that GPU audio can never work.

---

## 15. CPU optimization in the correct order

There is no meaningful “least CPU” without an identical feature set, quality target and workload. A sample voice with one gain stage is not comparable to a stereo stretch voice with a resonant nonlinear filter.

Start by reducing unnecessary work: indexed region selection; no processing for inactive capabilities; shared decoded data; correct-scope shared modulators; removal of provably redundant gains or routing; precomputed constants; prepared source/kernel variants.

Then improve locality and batching: contiguous voice state, small hot records, coherent family rendering, vectorized envelopes/gains/mixing, controlled scratch reuse and fewer page-cache lookups.

Only then pursue instruction-level optimization, approximations and thread parallelism. Preserve an exact profile separately from explicitly chosen native quality reductions.

A cost model for experiments:

```text
block_time ≈ event_cost + script_cost + selection_cost
           + sum(source_cost + voice_DSP_cost + mixing_cost)
           + bus_DSP_cost + cache/queue_overhead
```

Measure each term. Do not assume scripting is the bottleneck because it is interpreted: a callback running once per note may be much cheaper than hundreds of continuously stretched sample voices.

Six simultaneously held notes × two active dynamic layers × four stereo microphones means 48 stereo render voices before release layers or transition sources. This is an illustrative multiplication, not an engine benchmark. Different PCM cannot generally be rendered at constant cost as the number of microphones grows.

### Voice stealing

Native policy can prefer inaudible/released/quiet families, protect fresh attacks, and use bounded fades. Store an inexpensive salience estimate updated at a declared rate.

Steal coordinated families when required to keep a take coherent; imported profiles may require individual-voice behavior. Separate logical note limits, source-voice limits, grain limits and expensive-engine limits. One global polyphony integer is too coarse.

Stealing must cancel or invalidate future commands safely, retain needed release context when required, and finalize host identity correctly. Avoid allocating a second full victim voice just to fade it unless that reserve is budgeted.

---

## 16. Conformance before broad claims

### 16.1 Three kinds of tests

**Native contract tests** assert the behavior deliberately specified for new instruments.

**Documented profile tests** assert a concrete public rule for a stated version/profile.

**Reference probes** record what a licensed target engine actually does where the documentation is incomplete, contradictory or implementation-dependent. Do not invent a golden result for those probes.

The accompanying catalogue separates these kinds. It contains proposed scenarios, not vendor render results.

### 16.2 Tiny fixtures

Use deliberately simple audio: tagged impulses, tones with distinct frequencies, known loops, stereo channels with different markers, and one feature per instrument. A two-zone fixture often reveals a semantic difference more clearly than a 20 GB library.

Record event traces, selected source IDs, note ownership, controller state, callback/wake order and output audio. The trace explains why a render differs; a WAV alone usually does not.

### 16.3 Differential comparison

Run the same input sequence through KONTRA and a pinned licensed reference on a machine where the reference is supported. Compare exact sample timing and discrete selection where expected. Null-test only when the underlying algorithms and random states are expected to match.

For approximate DSP, compare onset, pitch, envelope, frequency response, spectral error, stereo behavior and listening results. An aligned low null residual does not prove script compatibility; a different reverb tail does not prove the MIDI engine is wrong.

### 16.4 Essential regression cases

Repeated same-key notes, keyswitch changes during sustain, independent release RR, pedal-up bursts, ignored controllers, per-note expression at note-on time, delayed note-on canceled before sounding, callback resume after stealing, mapping reload with held notes, UI-closed recall and host output queue failure all belong in the first serious fixture set.

A particularly important host case is terminal event backpressure. Retain a pending terminal notification until accepted under the adapter's contract; do not recycle ownership merely because an output push was attempted. Retry timing and repeated-key addressing need explicit adapter tests. [HO01]

### 16.5 Fuzzing and invariants

Assert finite samples, no out-of-bounds access, no stale generation access, bounded event growth, valid lifecycle transitions, eventual reclamation, no negative ownership counts and no lost cleanup under admitted workloads.

Fuzz parsers separately from render scheduling. Include malformed paths, huge declared arrays, deeply nested structures, corrupt loops, truncated codecs and scripts that generate recursively at the same timestamp.

---

## 17. Benchmark plan: no fabricated speed claims

Use a matrix of sample rates and block sizes, including small live-performance buffers. Report wall-time distribution per callback, worst observed time, deadline misses, active logical notes, render voices, memory residency, page misses and worker backlog.

Compare like-for-like: identical audio, same number of microphones/layers, same source mode, same interpolation quality, same effects, same gain and same output count. Record warm versus cold cache and separate loading from rendering.

Suggested workload families: plain one-shots; sustained loops; multimic velocity crossfade; release-heavy piano with pedal; interval legato; dense CC/MPE streams; scripting-heavy pattern generator; high transposition; reverse/offset streaming; stretch/granular voices; long shared effects; rapid preset switching.

Add an allocation/deallocation detector around the callback. Stress GUI-open/closed, transport seek, sample-rate changes, host buffer variability and suspend/resume. Average CPU percentage alone conceals missed deadlines.

Set release gates before optimization. A proposed initial gate is zero unintended note loss or stuck ownership across the admitted stress suite and no callback allocation/deallocation. Numerical latency/CPU targets should come from the intended devices and hosts, not invented universal numbers.

---

## 18. Implementation order and acceptance gates

### Phase A: semantic kernel vertical slice

Build canonical events, generational notes, ownership, sample-time scheduling and a headless trace renderer. Use one PCM source. Prove repeated-note pairing, delayed cancellation, pedal state, family ownership and cleanup.

**Exit:** tiny native fixtures show deterministic, explicitly specified event behavior across block boundaries. No stuck note or stale-handle mutation in randomized sequences.

### Phase B: declarative multisampling and first KSP slice

Implement indexed mapping, velocity/CC conditions, articulation state, RR, release policies, mic families and a modest SFZ/Decent frontend. In parallel, implement a KSP fixture that suppresses an input, creates children, waits and releases correctly.

**Exit:** declarative and scripted fixtures use the same ownership kernel without incompatible special-case bypasses. Unsupported imports generate precise diagnostics.

### Phase C: production asset delivery

Add attack preload, page cache, decode workers, offset/loop guards and prepared-plan swaps. Test large libraries, cold pages, multiple parts and held-note replacement.

**Exit:** no audio-thread I/O or waiting; defined underrun behavior; bounded memory across repeated preset changes.

### Phase D: KSP behavior and control model

Broaden command semantics, polyphonic state, callback timing, persistence, UI-value state and engine-parameter access. Rank coverage by real library features, not by number of function names stubbed.

**Exit:** pinned representative fixtures pass their documented/probed behavioral expectations. GUI closure does not change the instrument.

### Phase E: UVI hierarchy and Lua runtime

Implement program/layer/keygroup/oscillator identity, routing, coroutine scheduling, object API and a bounded Lua execution policy. Add source types needed by selected target libraries rather than every oscillator at once.

**Exit:** explicit capability reports; no claim that generic Lua or plain samples provide full Falcon equivalence.

### Phase F: broader fidelity and advanced processing

Expand SampleTank/other frontends as public knowledge and reference fixtures permit. Add proprietary-DSP approximations with honest labels, native high-quality morphing, advanced stretch and large-scale performance optimization.

**Exit:** each claimed compatibility increment has a fixture, version, diagnostic path and measured cost. Broad format claims never outrun semantic evidence.

---

## 19. Decisions to make before implementation expands

Choose the native repeated-key policy, note/family/voice budget model, terminal-event reserve, tempo-wait behavior, pedal/release policy, random reproducibility model and prepared-plan retirement policy.

Choose how strict imported profiles are allowed to be, which DSP substitutions are acceptable, and whether the first UVI runtime targets a bounded real-time subset or broader behavior with explicit limitations. Those are product decisions, not details that a parser can settle.

Choose native plugin formats and hosts for the conformance matrix. Treat host lifecycle behavior as part of the engine's correctness boundary, not a final packaging task.

Do not decide the final browser or visual editor before these contracts are usable headlessly. The most valuable early UI is an event trace and a reason-for-selection inspector.

---

## 20. Open questions and evidence limits

Proprietary filter, stretch and modulation algorithms require implementation work and reference measurements beyond public parameter names. This report does not establish bit-identical emulation.

Several public pages are version-skewed. In this research the Decent product page and developer guide identified different versions. The SampleTank overview and specification used inconsistent sample-engine counts. Record those ambiguities; do not turn marketing wording into an internal architecture claim. [DS01][DS04][IK01][IK02]

The UVI HTML API was substantially more useful than a general product manual for scripting details. The Workstation settings page was inspected as a PDF image; an attempted Falcon manual retrieval was unsuccessful. No inaccessible manual is treated as evidence.

No repository audit was performed. The module names in this document are proposed responsibilities, not statements about the current KONTRA repository.

No cross-engine renders or CPU benchmarks were executed. Reference tests listed here remain work to perform with pinned legitimate installations and fixtures.

## Final recommendation

Build one shared execution substrate with **multiple explicitly versioned musical personalities**. Make logical note ownership, event timing, scope and state visibility correct first. Compile declarative instruments and script services onto that substrate. Keep audio sources modular, streaming asynchronous, UI state headless and unsupported behavior visible.

That architecture can grow from simple sample maps into complex Kontakt and Falcon instruments without discarding the core each time another library reveals a new meaning of “note,” “group,” “release,” or “morph.”

---

## Primary-source registry

The references below were used for public behavior and interface facts. Architecture recommendations elsewhere are proposals. These are live documentation URLs, not archived snapshots; pin copies or revisions before using them as a permanent conformance specification.

**[NI01] Kontakt: Classic view**  
https://docs.native-instruments.com/online-guides/kontakt-manual/en/classic-view  
Hierarchy, mapping, group conditions, source modes and disk preload.

**[NI02] Kontakt: filters and effects signal flow**  
https://docs.native-instruments.com/online-guides/kontakt-manual/en/using-filters-and-effects-in-classic-view  
Per-voice group inserts versus processing of summed buses and instruments.

**[NI03] Kontakt: effect reference**  
https://docs.native-instruments.com/online-guides/kontakt-manual/en/effect-reference  
AET spectral-envelope morphing and other effect families.

**[KS01] KSP reference entry point**  
https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/welcome-to-ksp  
Versioned scripting reference; fetched reference identifies Kontakt 8.12.

**[KS02] KSP callbacks**  
https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/callbacks  
Callbacks and permissions; note_controller currently documents internally generated MIDI 2.0-style events, not external MIDI 2.0 input.

**[KS03] KSP variables**  
https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/variables  
Value types, note-associated polyphonic variables and persistence.

**[KS04] KSP general commands**  
https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/general-commands  
Generated note identities, play_note duration and start-offset behavior.

**[KS05] KSP time-related commands**  
https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/time-related-commands  
Suspended callbacks, waits, ticks and clocks.

**[NI04] Kontakt Lua API**  
https://docs.native-instruments.com/ni-tech-manuals/kontakt-api-reference-manual/en/welcome-to-the-kontakt-lua-api-reference-manual  
Instrument editing/creation API, distinct from KSP musical runtime.

**[UV01] UVI-script documentation**  
https://lua.uvi.net/  
Primary musical event scripting documentation.

**[UV02] UVI: porting from KSP**  
https://lua.uvi.net/_ksp_porting.html  
Useful cross-runtime comparison; analogous operations are not proof of complete equivalence.

**[UV03] UVI: timing**  
https://lua.uvi.net/_time_intro.html  
Cooperative timing, callback suspension and note-related waits.

**[UV04] UVI: Lua language runtime**  
https://lua.uvi.net/_lua_reference.html  
Lua 5.1, custom extensions, pooled real-time memory and restricted libraries.

**[UV05] UVI: event callbacks**  
https://lua.uvi.net/group___event_callbacks.html  
Callback forwarding, onEvent precedence, lifecycle and initialization.

**[UV06] UVI: voice manipulation**  
https://lua.uvi.net/_voice_intro.html  
Voice-addressed changes and fades.

**[UV07] UVI: Program**  
https://lua.uvi.net/class_program.html  
Program object model.

**[UV08] UVI: Layer**  
https://lua.uvi.net/class_layer.html  
Layer object model.

**[UV09] UVI: Keygroup**  
https://lua.uvi.net/class_keygroup.html  
Keygroup object model.

**[UV10] UVI: Oscillator**  
https://lua.uvi.net/class_oscillator.html  
Oscillator object model.

**[UV11] UVI: elements and parameters**  
https://lua.uvi.net/_elements.html  
Public engine object and parameter catalog; not proprietary DSP implementations.

**[UV12] Falcon product specification**  
https://www.uvi.net/falcon  
Hybrid synthesis/sampling and supported product capabilities.

**[UV13] UVI Workstation product specification**  
https://www.uvi.net/uvi-workstation  
Player scope, file types and performance functions.

**[UV14] UVI Workstation manual**  
https://cdn.uvi.net/UVIWS_Uvi_workstation/manuals/UVI_Workstation_manual_en.pdf  
Part routing, keyswitches, outputs, streaming and multis; settings page visually inspected.

**[UV15] UVI: sample mapping**  
https://lua.uvi.net/_sample_mapping_intro.html  
Four-axis dispatch, asynchronous mapping loading and sample-start/preload relationships. Confirm applicability against the target installed version.

**[DS01] Decent Sampler developer guide**  
https://decentsampler-developers-guide.readthedocs.io/en/latest/  
Developer guide landing page; fetched guide identifies 1.34.0.

**[DS02] Decent Sampler: groups**  
https://decentsampler-developers-guide.readthedocs.io/en/latest/the-groups-element.html  
Mapping, random modes, trigger conditions, oscillator and routing declarations.

**[DS03] Decent Sampler: MIDI**  
https://decentsampler-developers-guide.readthedocs.io/en/latest/the-midi-element.html  
CC/note bindings and swallowed notes; relevant to headless control state.

**[DS04] Decent Sampler product/release notes**  
https://www.decentsamples.com/product/decent-sampler-plugin/  
Fetched product version 1.36.1 differs from fetched guide version; pin behavior rather than assuming latest docs fully match.

**[IK01] SampleTank 4 overview**  
https://www.ikmultimedia.com/products/st4/  
Articulations, streaming, performance processors, effects and live workflow.

**[IK02] SampleTank 4 specifications**  
https://www.ikmultimedia.com/products/st4/index.php?p=specs  
16-part workstation and public playback capabilities. Engine-count wording differs from overview; do not infer internals.

**[SF01] SFZ format reference**  
https://sfzformat.com/  
Open declarative mapping vocabulary and extension ecosystem.

**[SF02] SFZ: trigger**  
https://sfzformat.com/opcodes/trigger/  
Release/physical-key-release distinction and documented player differences.

**[SF03] sfizz engine description**  
https://sfz.tools/sfizz/engine_description/  
Region/voice/resource separation; architecture reference, not a Rust dependency.

**[SF04] sfizz implementation status**  
https://sfz.tools/sfizz/development/status/  
Example of reporting support at feature/opcode granularity.

**[LS01] LinuxSampler instrument scripting**  
https://doc.linuxsampler.org/Instrument_Scripts/  
Format-independent script engine with format-specific extension layer.

**[LS02] LinuxSampler NKSP language**  
https://doc.linuxsampler.org/Instrument_Scripts/NKSP_Language/  
Related but distinct language semantics; not a drop-in promise for KSP.

**[HI01] HISE Synth API**  
https://docs.hise.audio/scripting/scripting-api/synth/index.html  
Generated-note attachment, delayed-note cancellation, deferred callbacks and voice indexing.

**[HI02] HISE Scriptnode**  
https://docs.hise.audio/scriptnode/index.html  
Example of modular DSP composition separate from instrument event logic.

**[HO01] CLAP event ABI**  
https://raw.githubusercontent.com/free-audio/clap/main/include/clap/events.h  
Native note identities, sample times, expression, terminal events and output queue contract.

**[HO02] CLAP host thread pool**  
https://raw.githubusercontent.com/free-audio/clap/main/include/clap/ext/thread-pool.h  
Optional host task execution with explicit synchronization/real-time caveat.

**[HO03] VST3 NoteOnEvent**  
https://steinbergmedia.github.io/vst3_doc/vstinterfaces/structSteinberg_1_1Vst_1_1NoteOnEvent.html  
Optional note ID, normalized velocity and tuning in cents.

**[MI01] MIDI Association MPE**  
https://midi.org/mpe-midi-polyphonic-expression  
Channel-based per-note expression model.

**[RU01] mlua crate documentation**  
https://docs.rs/mlua/latest/mlua/  
Rust embedding API; not an assurance that ordinary Lua execution is audio-thread-safe.

**[RU02] rtrb crate documentation**  
https://docs.rs/rtrb/latest/rtrb/  
Fixed-capacity wait-free SPSC queue; full queues return errors and payload behavior still matters.

**[RU03] Symphonia crate documentation**  
https://docs.rs/symphonia/latest/symphonia/  
Pure Rust multimedia demuxing and audio decoding; use outside the render callback.

**[DP01] Julius O. Smith: windowed-sinc interpolation**  
https://www.dsprelated.com/freebooks/pasp/Windowed_Sinc_Interpolation.html  
Author textbook treatment of bandlimited interpolation and rate-dependent filtering.

**[DP02] Signalsmith Stretch author documentation**  
https://signalsmith-audio.co.uk/code/stretch/  
Time/pitch processing with explicit input/output latency and automation alignment.

**[DP03] Rubber Band author documentation**  
https://breakfastquay.com/rubberband/  
Independent time/pitch processing; C++ and licensing considerations, not a proprietary-engine emulator.
