# Core and MIDI reference audit

Checked 2026-10-04. This is the evidence ledger for
[the refactor plan](CORE_MIDI_REFACTOR.md), not a claim of complete compatibility.
Public manuals establish intended controls and observable behavior. Native record
IDs, value laws, processing order and PCM equivalence require authored fixtures
and measurements. Existing regression tests establish our behavior only.

## Authority and versions

- Protocol units and addressing: [VST3 note-expression definitions](https://github.com/steinbergmedia/vst3_pluginterfaces/blob/master/vst/ivstnoteexpression.h),
  [CLAP events](https://github.com/free-audio/clap/blob/main/include/clap/events.h),
  [MIDI CC assignments](https://midi.org/midi-1-0-control-change-messages),
  [Reset All Controllers](https://midi.org/response-to-reset-all-controllers), and
  [MPE](https://midi.org/mpe-midi-polyphonic-expression).
- Native instrument behavior: Kontakt's live
  [instrument API](https://docs.native-instruments.com/ni-tech-manuals/kontakt-api-reference-manual/en/instrument),
  [group API](https://docs.native-instruments.com/ni-tech-manuals/kontakt-api-reference-manual/en/group),
  [classic-view manual](https://docs.native-instruments.com/ni-tech-manuals/kontakt-manual/en/classic-view),
  [modulation manual](https://docs.native-instruments.com/ni-tech-manuals/kontakt-manual/en/modulation),
  and [KSP callbacks](https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/callbacks).
  The API navigation names Kontakt 8.3; KSP navigation names 8.12. Do not assume
  these rolling pages describe every older serialized instrument identically.
- Comparative instrument design: publisher-linked
  [Falcon manual](https://s3.amazonaws.com/uvi/UVIFC/falcon_manual.pdf), cover
  marked SOFTWARE VERSION 2026; [Surge XT manual](https://surge-synthesizer.github.io/manual-xt/),
  marked XT 1.3; Bitwig's rolling [note-expression guide](https://www.bitwig.com/userguide/latest/working_with_note_events/)
  and [modulation guide](https://www.bitwig.com/userguide/latest/the_unified_modulation_system/).
  These establish useful instrument requirements, not Kontakt compatibility.
- Framework guidance: official [Truce MIDI guide](https://truce.audio/docs/guide/midi/)
  and [processing guide](https://truce.audio/docs/guide/processing/). This repository
  uses the Moose fork, version 7.0.0, pinned in Cargo to revision
  `bffa4677d0b82119d38566ce7e932dc5c463d497`, with local CLAP/VST3/parameter/UI
  patches. The pinned source and vendor patches determine available APIs;
  live upstream documentation does not override them.

Falcon's downloaded reference PDF SHA-256:
`23c2d8ce9ae3abd5bf3a3a65d5cb3f8744f395315e45fbc300c412d330cec117`.
Page numbers below are PDF page numbers. The manual is not copied into the repo.

## Confirmations, gaps and acceptance evidence

| Area | Reference checkpoint | Current assessment and required work |
| --- | --- | --- |
| Instrument controller policy | Kontakt instrument API, Controller options: CC7/10 enablement, CC7 range, CC64 modes, CC120/123 enablement | Verified private fields and opaque tails survive import/cache/export. InstrumentState owns policy; physical controller state remains separate. Enable flags and ControllerOnly sustain are implemented. SustainWithoutController modulation-source availability remains unresolved. Explicit Panic retains independent cleanup. |
| CC7/10 placement | Kontakt generated system script, traced setters/getters and default rack/container chain; classic-view Controller tab | Native CC7/10 writes instrument Volume/Pan before user callbacks in the authored parameter namespace. The default Native program amplifier now follows program racks/local returns; direct group/bus outputs bypass it. The main/nonlinear full-PCM reference passed. Compatibility retains squared output gain and exact-center balance. Other bank modes, external sends, module activity/tails and math-library equivalence remain open. |
| Voice limits | Kontakt classic view, Voice Groups: sample voices, stealing modes, exclusion and fade overlap | Stage1 retains stored policy. Test layered sample counting and owner survival when one layer is stolen. Kill Any's actual heuristic remains unproved. |
| Released-voice preference | Kontakt classic view, Pref.Rel description | The prose describes retention priority, conflicting with our released-first victim rank. Retain the saved flag, mark interpretation unresolved, and compare controlled renders before changing it. |
| Absent fade setting | Kontakt instrument API, Voice Handling: default fadeout 10 ms | Our engine fallback is 5 ms. A public API creation default does not establish old-file absence semantics. Test both saved and absent records; avoid a silent global default change. |
| Controller initialization | Kontakt modulation manual, Assignment controls: default value -1 defers modulation until input arrives; native external-modulator reader/writer and setup at 0x140d09620/0x140d15c10/0x1409766d1 | Saved signed defaults now survive import as `ModAssignment.cc_default`. Accepted physical receipt is distinct from numeric cache state. Unavailable routes skip shaping, lag and target binding; nonnegative defaults substitute until first accepted input. Frozen MPE CC74 tails retain receipt state, and exact per-note brightness retains precedence. New banks clear receipt validity; residency upgrades preserve it. Full PCM proof is staged, unrun; native first-event lag initialization remains unproved. |
| Envelope ownership | Kontakt modulation manual, AHDSR/AHD and retrigger controls; Falcon pp.335,337: Analog ADSR and DAHDSR, curves and modes | Stage1 repaired pitch clocks; it did not implement every mode. Scope trigger state explicitly, preserve release from current level, and admit native mode IDs only after fixture evidence. Do not equate differently named envelope models or curve algorithms. |
| Playback-state getters | [KSP event commands](https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/event-commands) and [built-in constants](https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/built-in-variables-and-constants): PLAY_POS is the zone cursor in microseconds; ZONE_ID requires a running voice, returns the highest active zone, and returns -1 without one | Timer callbacks now interleave completed DSP intervals. Getters query actual voices and mapped source geometry/rate, including reverse and loops; queued or expired voices supply no predicted zone. Both choose the highest zone, with a stable first voice for duplicate-zone ties as our explicit receiver policy. Native tie behavior and wavetable cursor identity remain unproved; wavetable PLAY_POS retains zero. Complete PCM proof is staged, unrun. |
| Flex held sustain | Kontakt modulation manual, Flexible Envelopes: sustain freezes at the second point while held; release enters the following section | Positive quiet and exact-zero held levels remain live. Gain can expose quiet sustain and later release points can rise from zero. Bounded saved v12 Oneshot and wider Loop sections with a zero endpoint are admitted from verified switch bytes. Loop returns retain the reached endpoint; release enters the following section. Native startup/curve parity remains open. Complete PCM scenarios are staged and unrun. |
| Modulator scope | Falcon p.18: hierarchy and keygroup envelope triggering; Surge XT, Modulation Sources: voice and scene scope | Model prepared routes separately from mutable voice/global clocks. Every source and target needs a scope; do not accidentally restart global modulators for each sample layer. |
| Group filter envelopes | Native InternalModArray16 and existing Bank admission bound | Playback now retains all16 routed envelope sources, fixing silent loss of sources5–16. Original source indices, bypass/depth writes and voice clocks remain addressable. Independent one-envelope PCM comparisons are staged, unrun. |
| Live insert matrices | Local complete signal-flow audit | Gain/stereo transitions formerly lasted one host fragment. Retained5ms per-voice clocks now keep retargeting and endpoints independent of fragmentation; new voices start at current prepared matrices. Native smoothing duration is unproved. Full-PCM scalar-oracle recording is staged, unrun. |
| LFOs and glide | Kontakt modulation manual: retrigger/phase/sync and polyphonic Glide with time/speed modes | Selected retriggered unsynchronized Glide Time/Speed sources now use traced program-container note history across receiving channels, clamped signed intervals and per-voice travel. Glide and LFO sources share one retained pitch destination. Sync, shared retrigger, further targets and native math-kernel equivalence remain open; this is independent of mono key priority. |
| Legacy LFO fade across targets | Local native BLfoMultiWithTargets final-waveform consumer at 0x140b07290 and reset at 0x1405f45a0; [modulation evidence](../audits/MODULATION.md) | Saved admitted sine-only volume routes now retain the same unsynchronized fade used by pitch. Shared source slots advance once per control point, then each target applies its conversion/lag. Complete PCM proof is staged, unrun; general waveform and free-running scope remain unresolved. |
| Digital Multi LFO | Kontakt modulation manual distinguishes arithmetic Multi Digital waveforms; local waveform6 factory/reset/consumer evidence in [MODULATION.md](../audits/MODULATION.md) | Bounded retriggered sine/rectangle/triangle/saw mixes retain shared source points, normalization, fade and the strict phase-1 endpoint. Unsupported clock geometry fails explicitly. Random/free-running/sync and native sine-table parity remain open; full PCM and exact terminal scenarios are staged, unrun. |
| Mono/legato | Surge XT, Play Mode: priority, fallback, single-trigger and pedal policies | Voice capacity one is insufficient. Add an explicit held-note policy and retrigger policy only with the relevant ownership tests. |
| Velocity and pressure | Falcon p.348: release velocity, poly and channel aftertouch; Surge XT, Modulation Sources: note-local expression | Canonical onset/release values now survive host ingress, script waits, deferred releases and DSP. Zone/KSP projections retain their own source metadata; release never overwrites onset. Recorded precision and physical-owner checks cover implemented paths. |
| Authored versus part controls | Falcon p.32: independent Program and Part gain/pan | Authored controls now belong to InstrumentState, separately from prepared zone values and part controls. Saved-zero restoration and nonlinear amplifier/send placement are checked. Final imported tuning/pan comparison uses resident samples so asynchronous disk readiness cannot substitute a silent reference. |
| Group parameter units | Kontakt8.13.1 generic group Volume/Pan setter/getter chains | Native group parameters now share the traced cubic4 gain and Pan float roundtrip with instrument parameters. Setup/live display uses the same conversion; physical CC7/10 authority remains instrument-only. Compatibility conversion is retained. Readback and complete PCM proofs are staged, unrun. |
| Exact host expressions | VST3 predefined units; CLAP exact note addressing and NOTE_END | The three advertised VST3 types use SDK units. Brightness5 additionally consumes exact normalized data through the authored CC74 source; advertisement/default metadata and actual exported-host PCM validation remain pending. SDK PolyPressure retains its separate event contract. Keep exact owner generations through release, descendants and tail processing; retire only when the owner has no remaining voice or pending musical work. Do not freeze exact host expression merely because MPE member expression freezes. |
| Input replacement | Pinned single-port PartControls routing and CLAP NOTE_END delivery contract | Port changes clear old received tuning, bend sensitivity, decoder configuration and raw library controls. Port/channel reroutes cancel voices and physical work while retaining terminal exact identities/timestamps for NOTE_END retry. Same-ID new-port roots and late old-port input remain separate. Complete plugin PCM proof is staged, unrun. |
| Transport precision | Truce MIDI guide; pinned Moose `events.rs` and `midi.rs` | `moose.toml` opts into MIDI2 input. The fork's `lossless_iter` preserves exact events and same-offset insertion order. Dedicated controller/per-note paths retain canonical precision and addressing; unsupported groups/indices reject explicitly. Generic downconversion must not collapse per-note controls into channel CC. Full external-host interoperability remains unverified. |
| Format coverage | Truce MIDI guide's format table versus our VST3 shim | Upstream describes six predefined VST3 types and a tuning path limited to ±48 semitones. Our exact path advertises three and retains SDK ±120. Preserve the fork's exact route and actual capability declaration; never copy the upstream table into our compatibility claims. |
| Received parameters | NI KSP callbacks: received RPN/NRPN; UMP v1.1.2 §7.4.6–7.4.7 | One physical MIDI1 decoder retains selection, null/reset and absolute/relative provenance independently of authored notifications. Native MIDI2 CC bypasses that decoder; unified native parameter messages retain their exact path. Implemented native parameter laws remain address-specific; unknown addresses receive callbacks without invented DSP. Native relative RPN0/6 remain unsupported. |
| MPE lifecycle | MIDI Association MPE specification | Same-event dual-zone installation and scoped entering/leaving owner cleanup now have recorded performance evidence. Member expression freezes on physical release, including pedal-held tails and waiting starts. Exact host expressions retain their independent owner. Pressure/brightness combination laws are local policies unless independently proved. |
| Realtime processing | Truce processing guide | Follow bounded, allocation-free event processing. A voice count of zero alone cannot establish idle: inspect pending script work and effect tails. Parameter smoothing must use elapsed sample time rather than fragment length. |
| Queued parameter timing | NI KSP wait semantics; CLAP sample-offset header; pinned Engine write/command queues | Each event interval now completes bus mixing, output controls and program effects before later queued writes. Return/direct buffers retain their block offsets. Waited writes at samples 24/72/96 have complete main/direct PCM and exact-owner proofs staged, unrun. Bus ramps retain the existing prepared 128-frame receiver trajectory across fragments; native smoothing equivalence is unproved. Bus-only processing also survives bank removal. |
| Streaming and interpolation | Native ordinary Sampler saved-quality reader, quality0 null-table consumer, captured High/Ultra tables and separate wavetable dispatch; [counted-loop evidence](../audits/COUNTED_LOOPS.md) | Normal/High/Ultra Sampler and recognized Native DFD use verified kernels with physical crop support and counted/ordered packets. The historical5.04s check used native DFD capture inputs later found to have unbound loop rows; that historical DFD loop-domain evidence is withdrawn. The repaired bound captures and actual saved/cache RAM/streaming complete-output check passed in3.99s, including Normal crop/tiny assets and unchanged native endpoints. Fractional/alternating loops, positive HQ crossfades, alias rejection, NI async-I/O/deadlines and general native parity remain separate boundaries. |
| Event membership and voice census | [KSP built-ins](https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/built-in-variables-and-constants), native lookup/lifetime paths and [completion evidence](../audits/KSP_EVENT_COMPLETION.md) | Direct getters/marks return inactive defaults after active membership ends. INST counts actual sample voices, excluding retained metadata and unconstructed pending starts. Aligned Rack phases supply TOTAL at a common DSP boundary; standalone Engine calls have no peer owner. Public prose defines current/all instruments but does not prove native same-frame ordering or layer-count parity. Private release children stay separately identified. |

NI's [time-related commands](https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/time-related-commands)
define `wait` in microseconds and resume the callback after that interval. The
[CLAP event header](https://github.com/free-audio/clap/blob/main/include/clap/events.h)
defines event time as a sample offset in the current buffer. The local interval
processing repair preserves these existing queue clocks through the full audio
chain; public timing documentation does not establish Kontakt's exact parameter
smoothing or full DSP equivalence.

## MIDI2 source decisions, 2026-10-04

The [UMP and MIDI2 protocol v1.1.2](https://amei-music.github.io/midi2.0-docs/amei-pdf/M2-104-UM_v1-1-2_UMP_and_MIDI_2-0_Protocol_Specification.pdf)
is the native wire authority. Section 7.4.5 and Appendix C establish persistent
port/channel/key controller generations: Detach freezes prior notes, Reset changes
current controls, and combined flags detach first. Figure 51 places Reset in bit0
and Detach in bit1; the pinned Moose comment reverses those names.

Section 7.4.6 recommends receivers ignore native MIDI2 CC0/6/32/38/98/99/100/101;
this receiver implements that recommendation before raw cache/callback delivery.
Native CC96/97 remains ordinary authored input and cannot mutate MIDI1 selection
or data entry. Typed ControlChange2 and raw UMP fallback use the same adapter;
§7.4.7's unified Registered/Assignable messages retain full-width values and
their independent absolute/relative path. Mixed-protocol playback proof is
staged, unrun.

Sections 7.4.7.1 and 7.4.13 distinguish channel sensitivity's semitone/cents fields
from per-note sensitivity's Q7.25 interval. Per-note ranges bind at NoteOn;
48 semitones is our compatibility default, not a mandated protocol default.
Section 7.4.15 distinguishes persistent PN3 Q7.25 absolute pitch from root-local
NoteOn attribute3 Q7.9 pitch. Our later-write precedence and attribute fallback
after Reset are explicitly local policies.

Table22 defines the generic registered controller indices carried into modulation.
Appendix B.2 preserves registered/assignable controllers and per-note bend on CC121.
Poly pressure remains subject to the ordinary reset described by
[RP015](https://midi.org/response-to-reset-all-controllers).
Source implementation is complete for the native per-note data/callback slice;
this cross-check does not claim native host interoperability or library equivalence.

## Received relative parameter authority, 2026-10-04

[MIDI Association RP-018](https://midi.org/response-to-data-increment-decrement-controllers)
defines CC96/97 as one-step parameter changes with an ignored data byte. RPN0
changes the supported cents LSB while retaining the semitone MSB when that byte
remains representable. Its carry-at100 example describes a receiver without
cents support. An unknown parameter
does not acquire a native DSP mapping merely because a relative message was
decoded. Implementation must use the currently accepted parameter value and
keep received relative callback intent separate from synthesized absolute writes.

[MIDI1 Detailed Specification v4.2.1](https://midi.org/midi-1-0-detailed-specification),
printed page18, assigns absolute RPN0 Data Entry MSB to semitones and LSB to
cents. It adds no restriction below the ordinary seven-bit data-byte domain:
absolute LSB100–127 is valid, including MSB127/LSB127 =128.27 semitones. RP-018's
page1 unsupported-LSB decimal carry is not an absolute admission limit. The
receiver retains the semitone field for representable cents updates. Only a
decrement from cents0 or increment from cents127 retains the previous local
numeric saturation/carry policy over0–128.27; that boundary representation is
not claimed as native Kontakt behavior.
MPE's chosen96-semitone receiver limit remains separate, and native32-bit
relative RPN0 has no inferred compound-field law.

Downloaded primary PDF SHA-256 values (documents remain outside the repository):

- MIDI1 v4.2.1: `93cbd078d9f57cd5b20349bbd08fd877f057a74e61cf923a7cec5f1b65bc80c4`.
- RP-018, MMA approval9/97 and AMEI approval10/97:
  `0b419fefb40d65148a4a5f8385aeaef60e5b16e25b25d226b1b90f140c213810`.

RPN1/2 tuning also follows the channel-based MIDI1 parameter definitions and
RP-018 step sizes. [MIDI2 bit scaling v1.0.2](https://amei-music.github.io/midi2.0-docs/amei-pdf/M2-115-U_v1-0-2_MIDI%202.0%20Bit%20Scaling%20and%20Resolution.pdf)
section 4 defines zero-extension scaling for these registered parameters. Native
fine tuning retains its 32-bit fraction; wire translation rounding is distinct
from the existing upper-14-bit local KSP view. The public manuals do not establish
optional MPE manager tuning fanout, so this implementation uses direct physical
channel scope. Source traversal no longer clamps valid pitch to 32x: resident
samples gather actual interpolation taps and streams use bounded sparse packets.
Numerical geometry limits fail explicitly. This preserves pitch; it does not
establish anti-aliasing quality or disk deadline performance.

## Local native-record evidence, 2026-10-04

Read-only analysis used the locally installed Kontakt 8.13.1 executable, SHA-256
`0fe6356e0879d058b6e5b73507c54c5e345cea451b35287c974e438291d4dae8`.
Native disassembly and extracted templates remain outside this repository.
Public manuals still establish intended behavior; these binary findings establish
specific serialization/binding details for that version.

- Start-criteria UI binding at `0x14061c020`, archive reader at `0x140d04400`
  and writer at `0x140d12ec0` agree on modes: Always 0, Key 1, Controller 2,
  Round Robin 3, Random 4 and Slice Trigger 5. Operators are AND 0, AND NOT 1,
  OR 2. The subsequent bounded generator/dispatcher and Boolean-parser traces
  recorded in [native evidence](CORE_NATIVE_CONTROLLER_EVIDENCE.md) establish
  final slot7 admission, key-span latch ownership, shared RR/random clocks,
  delivered-channel raw CC reads, AND precedence over OR, and independent
  release-prelude/condition guards. These supported predicates are implemented;
  Slice, native CRT random-sequence parity and CC121 raw-bank resets remain open.
- Program private-reader `0x140d0a2c0`, named-reader `0x14092e8b0` and generated
  native actions agree on the private-prefix fields: pedal mode at offset 0,
  CC7/10 enable at 4, CC120/123 enable at 5 and signed CC7 maximum dB at 6.
  Archive pedal values 0/1/2 correspond to sustain plus controller, sustain
  without controller and controller only; archive value 2 maps to runtime 3.
  Only verified archive versions may expose this interpretation. Preserve the
  original version and bytes, including fields the engine does not interpret.

Production integration must keep accepted physical controllers and parameter
decoders separate from these authored instrument policies. Disabled native actions
must not erase the received message or invent a protocol reset. Explicit Panic
and MPE ownership reconciliation retain their independent cleanup semantics.

The same executable's generated system template writes CC7/10 to instrument
Volume/Pan before user script slots. The bounded setter/getter traces establish
the coefficient, float conversion and shared parameter identity recorded in
[CORE_NATIVE_CONTROLLER_EVIDENCE.md](CORE_NATIVE_CONTROLLER_EVIDENCE.md). Those
laws now select a separate Native policy; absent or invalid private records retain
Compatibility policy. Continuous high-resolution extensions, existing DSP signal
placement and host-versus-WindowsCRT math are separate equivalence boundaries.

## Deferred articulation migration

The user requested seamless migration from library keyswitches to channel and
velocity selectors, then explicitly deferred it until after the core work.
Keep this requirement in Stage4. Existing `Articulate` already saves channel and
velocity mappings; `Router::switch` replays a velocity-100 keyswitch pulse or a
UI-control value of 1. This establishes useful behavior for tested latches,
not universal library migration.

Future work must preserve momentary/latching behavior, switch velocity and
passthrough, independent musical velocity, round robins, legato history and
the originating articulation during delayed attack/release. Use stable selector
identity and instrument path plus program, rather than labels alone. Capture
selection with the note owner; shared authored script globals may require
explicit compatibility handling. MPE member channels and articulation channels
need a defined conflict policy. Prefer reusing `Articulate`, prepared `Route`
and `Router` over a second routing framework.

Comparative references for that later stage:
[HALion trigger section](https://www.steinberg.help/api/khub/documents/tn44SHdRckATS246A89asA/content),
[Vienna Synchron dimension tree](https://www.vsl.co.at/manuals/synchron-player/dimension-tree-view),
and [SFZ held switches](https://sfzformat.com/opcodes/sw_down/).
Their temporary/toggle/multidimensional behavior explains why a single immediate
keyswitch pulse cannot promise equivalent behavior for every instrument.

## Evidence required before claiming completion

Use protocol endpoint/address/reset tests, script callbacks across slots and
waits, same-pitch owners, pedals and generated children. Require zero audio-thread
heap work and whole-versus-fragmented PCM/state checks for changed processing.
Use controlled Kontakt save/API/readback/render fixtures for native record
interpretation and sound-equivalence claims. Run actual external CLAP/VST3 host
checks separately from the framework's direct process harness.

## Release selection evidence from supplied sessions

The supplied METROPOLIS session reports `%NOTE_DURATION` as unsupported, returning
zero. NI documents this 128-key array as elapsed microseconds and specifically
recommends it for release callbacks in the
[KSP variable reference](https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/built-in-variables-and-constants).
A duration-dependent script can therefore choose the wrong release group even
when onset velocity and sustain mapping are correct. The runtime now retains
start and first-release sample timestamps per event, including generated notes;
retained callbacks use their own event rather than a newer same-key event. Only
scripts declaring the array refresh its bounded 128-cell snapshot at callback
resume and after script note creation/release edits, preserving ordinary indexed
and bulk reads. Regression coverage includes
waits, duplicate keys, generated notes, block partition, read-only enforcement,
and release-group PCM against an independently authored static selector. This
establishes local behavior; the supplied logs contain no Symphony NKI/script
payload establishing that this library uses this specific selector.

Native `Group.start_criteria` remains a separate playback gap. The importer
retains the sparse four-row mask and each row's `mode`, `next_criteria`, key
range, controller/range, cycle class, slice-zone/index and sequencer-only fields,
plus unknown trailing bytes. `Bank::matching_zones` applies channel, note,
velocity, release-trigger and script group-mask filters, but does not evaluate
those native conditions. The importer already warns about this limitation.
NI documents keyswitch latches, controller ranges, round-robin/random cycles
and AND/AND-NOT/OR composition in the
[group API](https://docs.native-instruments.com/ni-tech-manuals/kontakt-api-reference-manual/en/group)
and [classic-view manual](https://docs.native-instruments.com/ni-tech-manuals/kontakt-manual/en/classic-view).
These pages do not identify serialized numeric mode/operator IDs. The local
binary evidence above now establishes those bindings independently of enum
order. Native playback eligibility still needs verified latch/counter scope,
inactive-row handling and boolean composition, including sparse criteria rows.
Do not infer those execution semantics merely from the now-known numeric IDs.

The manual's native release-trigger counter is also independent of script array
reads: its time parameter freezes at the corresponding physical note-off, not
pedal-up or the start of a delayed tail. Existing per-event native counter
handling and script duration handling must retain separate owners when a key is
retriggered. The
[KSP advanced concepts](https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/advanced-concepts)
reference documents system group-start/release-trigger bypass conditions; a
script-selected tail must not accidentally inherit unrelated native eligibility.

## Native per-note callbacks

NI explicitly defines genuine MIDI2 Registered/Assignable Per-Note Controllers
and Per-Note Pitch Bend as inputs to
[`on note_controller`](https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/callbacks#on-note-controller).
The controller indices and scalar domains follow
[`set_note_controller`](https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/general-commands).
Original wire payloads remain independent of these KSP projections. Authored
messages start in subsequent slots and require authored receiver behavior; they
must not implicitly project into the native input pool. Captured-generation
acceptance across waits is the local ownership rule preserving original identity;
it does not change the runtime's existing first-wait forwarding contract.

UMP v1.1.2 Appendix D.1.4 specifies downsizing by shifting away the low bits;
Appendix D.2 applies it to velocity, controllers, pressure and bend. This is
independent of normalized continuous DSP arithmetic. MIDI1 wire translation's
nonzero NoteOn requirement is a separate transport rule; a native MIDI2 zero
NoteOn remains a NoteOn in this engine. Native projections and host-derived
rounded projections therefore retain their own metadata.

## Native loop records

NI's [Sample Loop manual](https://docs.native-instruments.com/ni-tech-manuals/kontakt-manual/en/classic-view)
states that a zero loop count repeats indefinitely and never reaches following
sample material or loop regions. The ordinary sampler's independently traced
counter decrements at each end crossing before deciding whether to repeat:
count one completes the current partial pass without an extra replay; count two
allows one repeat. This differs from the embedded Beat Machine script's
jump-before-decrement sequence, which is not evidence for ordinary playback.

Counted and ordered forward UntilEnd/UntilRelease regions now use the saved
physical slot order, source phase, first/final interpolation stencils and
release-before-boundary ordering. Inactive holes survive import/cache/NKI output.
The admitted subset requires unity tuning, no alternation and bounded geometry.
Positive crossfade is limited to integral source points and pending endpoints;
its gain retains physical512-frame native float rounding. Fractional crossfade
remains guarded because the native producer quantum is not mapped. Negative signed counts repeat indefinitely while suppressing the right wrap stencil; zero retains both wrap stencils. Unsupported combinations retain precise
diagnostics rather than selecting a later region. [COUNTED_LOOPS.md](../audits/COUNTED_LOOPS.md)
records the native reader, selector, counter, release and resampler addresses,
including narrower local admission boundaries. Complete handwritten-phase and independently unrolled-source PCM scenarios are
staged and unrun. Negative counts include independent physical EOF census.

## Native mono and Glide boundaries

[MONO_LEGATO.md](../audits/MONO_LEGATO.md) distinguishes the saved release-trigger
NoteMono flag from generic keyboard Mono/Legato. Falcon's layer key priority and
portamento controls do not establish an equivalent Kontakt saved control. The
current release-tail cutting policy has independent PCM scenarios; native policy
equivalence and Tone/Time source Legato remain open.

[MODULATION.md](../audits/MODULATION.md) records direct Glide child0B under
internal source category5, mode/time conversion, original dispatch history,
waited source initialization and combined destination interpolation. Unsupported
records are diagnosed rather than silently approximated as a sine or envelope.


## RPN0 coarse-field retention, 2026-10-04

[MIDI1 Detailed Specification v4.2.1](https://midi.org/midi-1-0-detailed-specification),
printed page12, permits subsequent fine adjustments using only an LSB; the
existing MSB remains in force. Printed page18 defines RPN0's independent
semitone MSB and cents LSB. Combining these rules with legal seven-bit cents
means the MSB cannot be recovered by flooring the summed sensitivity when an
absolute cents field exceeds99. CC121 clears selection/transaction state but
preserves the recognized parameter configuration; its coarse field must survive
with that configuration. The downloaded primary PDF matches the retained
SHA-256 `93cbd078d9f57cd5b20349bbd08fd877f057a74e61cf923a7cec5f1b65bc80c4`
and is held outside the repository at
`/tmp/kontra-primary-midi1-rpn-baseline-20261004.pdf`.

[UMP v1.1.2 section7.4.7.1](https://amei-music.github.io/midi2.0-docs/amei-pdf/M2-104-UM_v1-1-2_UMP_and_MIDI_2-0_Protocol_Specification.pdf)
uses the same RPN0 definitions and packs the coarse HCU field into the highest
seven bits, the cents field into the next seven, ignoring the lower18 bits.
The local mixed-protocol owner therefore retains native absolute coarse fields
and reseeds only an already selected same-parameter MIDI1 baseline. No protocol
message is synthesized and a null selector stays null. RP-018's decimal
relative carry remains a separate operation.

The [Truce MIDI guide](https://truce.audio/docs/guide/midi/) exposes typed MIDI2
input beside ordinary MIDI1 events. The pinned Moose revision
`bffa4677d0b82119d38566ce7e932dc5c463d497` local `moose-core/src/events.rs`
retains RegisteredController bank/index/value32 and cross-lane insertion order
through `lossless_iter`; `midi.rs::downconvert_to_midi1` has no RPN-controller
conversion. KONTRA's native parameter ingress preserves that unified payload,
while physical MIDI1 decoding uses the same channel's recognized configuration.
These are the actual local ingress paths used by the repair; no external host
interoperability or native Kontakt mixed-protocol scheduling equivalence is
claimed. The focused Engine oracle uses independent literal bend-derived note
tuning and is staged, unrun.


### Relative RPN0 field-state evidence boundary

[RP-018, Response to Data Increment/Decrement Controllers](https://midi.org/response-to-data-increment-decrement-controllers),
page1, first distinguishes contiguous parameters from RPN0's semitone/cents
fields, then specifies ignoring the CC96/97 value byte and changing RPN0's LSB
by one. Its final recommendation paragraph describes carry when the receiver
does not support the LSB, with the RPN0 cents100 example under that explicit
qualification. A supported cents LSB changes by one while its result remains a
legal seven-bit byte, so10/127 followed by CC97 retains10/126; a later fine25
means10.25. It does not close the overflow/underflow policy when the resulting
cents byte would be128 or−1. The retained primary PDF is
`artifacts/core-native/rpn0-relative-field-authority-rp018.pdf`, SHA-256
`0b419fefb40d65148a4a5f8385aeaef60e5b16e25b25d226b1b90f140c213810`.

The [official MIDI1 CC table](https://midi.org/midi-1-0-control-change-messages)
refers Data Increment/Decrement to RP-018 and makes null selection inactive.
UMP section7.4.7.1 closes absolute RPN0 field packing, not this MIDI1 relative
representation question. Pinned Moose preserves the incoming relative message;
it supplies no receiver arithmetic policy. The bounded Kontakt ordinary-ingress
scan at86cd30 closes note allocation only, not an RPN0 relative owner. The
supported-LSB repair follows RP-018 for representable updates in ordinary and
MPE receivers. Only nonrepresentable endpoints retain the existing explicit
local numeric policy. No native Kontakt relative-field equivalence is claimed.
The independent literal-tune ordinary/MPE complete PCM proof is staged, unrun.
