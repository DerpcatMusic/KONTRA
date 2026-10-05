# MIDI controller contract

This records the implemented controller behavior and its ownership boundaries.
Accepting all 128 MIDI 1 controller numbers does not imply implementing every
standardized action. This is a Kontakt instrument player with authored scripts
and modulation, not a General MIDI sound module.

The [reference audit](CORE_MIDI_REFERENCES.md) separates documented requirements
from implemented behavior and measured compatibility.

## Ownership and delivery

- The transport adapter decodes host events. `articulate::In` carries musical
  input; host notes also retain their exact host identity.
- `articulate::Router` owns articulation routing, physical key-to-delivered-note
  mappings, and a projection of Engine-owned MPE setup. Channel articulation mode delivers
  several physical channels to the part's logical home channel. The physical
  `input_channel` remains the owner for releases, modulation and cancellation;
  the delivered `channel` enters library script callbacks.
- `Player.midi` (`engine/midi_state.rs`) owns applied CC, bend, pressure,
  expression, reset defaults and physical parameter transactions. Player owns
  pedals, keys and voice lifetimes. Voices read their captured physical controller
  row independently of library delivery. Controller cache updates
  become audible only if a native action or authored modulation reads them.
- KSP owns callback execution and its script-visible `%CC` snapshot. Its scalar
  controller snapshot is not a second array of per-channel engine state.
  Delayed callbacks and commands retain their channel and physical input owner.
- Native group conditions read a distinct raw library-receiver bank by delivered
  channel, including authored MIDI_CHANNEL retargeting. Physical ingress updates
  that bank before user callbacks; accepted physical modulation still follows
  callback ignore/wait. Authored controllers update the raw bank at first
  delivery, and forwarding a waited callback cannot restore an older raw value.
- `crate::midi::reset_controller_value` owns the shared, transport-independent
  reset policy. Native playback and KSP use the same pure function.
- `crate::midi::Midi1Decoder` owns physical wire pairing, velocity prefixes and
  received RPN/NRPN transactions; MidiState keeps one instance per input channel. Authored parameter messages
  do not feed this decoder. Engine owns dual-zone negotiation and reconciles
  affected physical lifetimes before publishing a changed zone.
- `Control` retains normalized CC/pressure and signed bend through delayed
  callbacks. MIDI 1 and KSP byte views are explicit projections. Physical targets
  update their input row; authored targets broadcast to all instrument rows.
  Callback cleanup provenance does not restrict authored broadcast delivery.
- MPE member expression freezes at physical release. Native MIDI2 per-note controls
  remain shared across same-address tails until Detach; exact host owners retain
  their own live overrides. These are distinct lifetimes.

Code anchors: [Router](../src/articulate.rs), [engine MIDI state](../src/engine/midi_state.rs),
[voice inputs](../src/engine/voice.rs), [KSP work and forwarding](../src/ksp/runtime.rs),
[script command application](../src/engine/script.rs), [reset policy](../src/midi.rs).
Relevant functions are `Router::input`, `feed_on_port`, `Engine::control_from`,
`Player::control`, `Context::inputs`, `Runtime::received_control`, and `Player::apply`.

Each part supports one selected input port. Changing that port clears received
tuning, bend-range negotiation, MIDI1 decoder configuration and the old raw
library receiver; authored initialization is replayed. Changing port or channel
retires old physical input work and voices while preserving exact-note terminal
identities and timestamps until the host accepts NOTE_END. Late old-port traffic
cannot target a new-port root that reuses the same note ID. Ordinary audio resets
keep their existing received-configuration retention policy. The complete plugin
PCM reroute/retry scenario is staged and unrun.

## Complete CC coverage

Every number in 0..127 appears in exactly one row below. **Raw** means the accepted
value is retained in its physical-input row after script
forwarding. **Authored** means a script or instrument modulation assignment can
give that value musical meaning; it is not an automatic standardized action.
**Native** means the player performs the stated action itself. Incoming CC below
120 enters `on controller` when scripts are present; accepted cache/modulation
delivery can be ignored or delayed. Enabled Native CC7/10 standard actions have
already updated their instrument parameter at the original input timestamp.
Channel-mode messages 120..127 have special engine handling. Disabled authored
native CC120/123 actions preserve ownership and use ordinary controller delivery.
Initialization uses authored controller commands directly.

| CC | Implemented action | Limit or intentionally authored behavior |
| --- | --- | --- |
| 0 | MIDI1 raw, authored; native MIDI2 ignored | No bank/program selection |
| 1–5 | Raw, authored | No universal wheel/breath/foot/portamento action |
| 6 | MIDI1 raw, authored; received parameter MSB; native MIDI2 ignored | Complete MIDI1 absolute entries enter RPN/NRPN callbacks; MSB clears fine data |
| 7 | Raw; enabled instrument volume | Verified Native instrument Volume conversion; Compatibility squared output gain. Authored CC7/10 enable flag gates the physical action |
| 8–9 | Raw, authored | No automatic balance action |
| 10 | Raw; enabled instrument pan | Verified Native instrument Pan conversion; Compatibility output balance with source-resolution-aware exact center |
| 11–31 | Raw, authored | CC11 expression requires an authored assignment |
| 32 | MIDI1 raw bank selector LSB; native MIDI2 ignored | No bank/program switching |
| 33–37 | Raw fine bytes; combined ordinary controller values | Requires known coarse data; raw callbacks retain the fine index |
| 38 | MIDI1 raw, authored; received parameter LSB; native MIDI2 ignored | MIDI1 absolute callback requires known coarse data for the selected address |
| 39–63 | Raw fine bytes; combined ordinary controller values | Paired volume/pan follow their instrument policy; expression remains authored |
| 64 | Raw; enabled native channel sustain | ControllerOnly and KSP system-sustain bypass disable native hold; mode-1 modulation-source availability remains pending |
| 65 | Raw, authored | No native portamento switch |
| 66 | Raw; native channel sostenuto | Captures held note owners on the down edge, including release-only keys |
| 67–69 | Raw, authored | No native soft pedal, legato, or second hold |
| 70–73 | Raw, authored | No automatic tone/envelope rewrites |
| 74 | Raw, authored; MPE brightness input | No mandatory filter assignment |
| 75–87 | Raw, authored | No automatic decay, vibrato, or portamento mapping |
| 88 | Raw; MIDI1 high-resolution velocity prefix | One-shot next matching-channel wire Note On/Off; authored writes do not arm it |
| 89–95 | Raw, authored | No automatic effects-send mapping |
| 96–97 | Raw, authored; MIDI1 decoded relative intent and RPN0/1/2 steps | Native MIDI2 CC remains ordinary authored input and never enters the MIDI1 parameter decoder |
| 98–99 | MIDI1 raw, authored; select received NRPN; native MIDI2 ignored | Deselect the same MIDI1 decoder's RPN selection |
| 100–101 | MIDI1 raw, authored; select received RPN; native MIDI2 ignored | Physical MIDI1 RPN0 sensitivity and RPN1/2 tuning; MPE RPN0/6 negotiation |
| 102–119 | Raw, authored | Extra controllers can have authored reset defaults |
| 120 | Raw; enabled native all sound off | Authored CC120/123 flag gates physical cut and lifetime cleanup |
| 121 | Raw; native reset | Reset policy below; preserves held keys |
| 122 | Raw only | No local-control switching |
| 123 | Raw; enabled native all notes off | Authored CC120/123 flag gates physical release; pedals can retain sound |
| 124–127 | Raw; implicit native all notes off | Original mode number/value retained; no omni/mono/poly switching |

Standard meanings and reserved ranges follow the
[MIDI Association CC table](https://midi.org/midi-1-0-control-change-messages).
Source: `Player::control`; script delivery: `Engine::control_from` and `Runtime::handle`.

Enabled received CC123 and received CC124–127 release physical MIDI owners through the ordinary
Note Off path, including MPE manager-selected members. Sustain and sostenuto can
retain their sound until pedal release. Another physical input sharing the same
logical delivery channel remains held. CC124–127 retain their original mode
number and value; their implicit note release implements the
[MIDI Association mode-message table](https://midi.org/midi-1-0-control-change-messages),
without enabling Omni, Mono, or Poly mode switching. The direct public CC123
operation retains its existing logical-channel cleanup behavior. Validation of
the expanded mode range is pending the final implementation checkpoint.

Physical CC120 uses the traced10ms fade for validated Native policy and the
existing5ms fade for Compatibility. Explicit logical CC/Panic and MPE zone
reconciliation retain their separate5ms cleanup policy. Received MPE manager
cleanup selects the manager's physical member mask before consulting captured
library delivery channels; a remapped home channel cannot select another zone.

Scripted physical roots retain their own accepted sostenuto capture, including
anonymous roots and exact roots without a surviving attack voice. Capture occurs
at final accepted CC66 delivery, before later ingress key-up; ignored callbacks
cannot create capture, and waits retain their delivery time. Physical MPE manager
masks use the same fanout as playback. Exact-owner capture is synchronized at this
edge; a receipt prevents queued DSP from capturing a newer post-edge root.
Generated descendants keep their own policy, and actual attack-voice capture
remains authoritative. Repeated pedal-down does not recapture later roots.

Physical CC121 clears scoped root capture before synthetic controller callbacks,
including a callback that ignores synthetic CC66-up. Accepted authored CC121 uses
the same root-reset hook. The complete PCM scenarios in
[sostenuto_root_tests.rs](../src/engine/sostenuto_root_tests.rs) use independently
timed samples, exact and anonymous roots, generated children, same-frame queued
controls, MPE manager routing, waited releases, reused owners, exact overlays and
physical source EOF. They are staged and unrun. Mandatory physical reset also
clears captured, unapplied input-root release snapshots on the selected physical
channels. Both immediate and waited release scenarios retain their original
timestamps; ordinary later pedal-up does not advance queued releases.

Received CC122 reaches `on controller`, including ignored and waited callbacks.
CC123–127 retain their scoped note-release actions and pedal behavior. This does
not implement Local keyboard routing, Basic Channel assignment or Omni/Mono/Poly
reconfiguration; [CHANNEL_MODES.md](../audits/CHANNEL_MODES.md) records the native,
MIDI and pinned-framework boundaries.

Verified native private records now retain CC7/10 and CC120/123 enablement,
CC64 mode and CC7 maximum dB across import, cache and supported NKI writing.
InstrumentState owns these authored policies; MidiState retains accepted physical
controllers independently. Native playback honors the enable flags and
ControllerOnly pedal mode. SustainWithoutController modulation-source availability remains
unimplemented until its binding is proved.
See the [native evidence](CORE_NATIVE_CONTROLLER_EVIDENCE.md) and
[instrument options reference](https://docs.native-instruments.com/ni-tech-manuals/kontakt-api-reference-manual/en/instrument).

Native policy is selected only from validated private records. Its generated
CC7/10 actions write the same instrument Volume/Pan namespace as authored
`set_engine_par`. They precede user controller callbacks, so ignore or wait cannot
delay the standard action; later forwarding updates accepted physical state
without replaying it. A later authored parameter write clears that parameter's
physical override. Same-frame order and getter authority use the existing bounded
parameter-write timeline. Saved/authored raw values remain distinct from inverse
readback of an effective physical value.

Native MIDI1 CC7 multiplies its byte by coefficient3113/3937/4956/6243/7874 for
maximum−12/−6/0/+6/+12dB. The integer parameter is normalized via `f64 * 1e-6`,
cast to `f32`, cubed, clamped to0–1 and multiplied by4. CC10 uses coefficient7874
and the traced float bipolar/clamp/rounding sequence. Native pan at byte64 is
approximately+0.007871985, retaining the native off-center result. Paired14 and
native32 use their original normalized fraction in the continuous extension of
these mappings. That extension is local policy, not measured Kontakt MIDI2
equivalence. The traced default Native program amplifier now runs after the
program racks and local returns, before instrument summing. Direct group/bus
outputs bypass it. Gain uses the native1/1800 one-pole and near-unity branch;
pan uses the traced rate-scaled slew. Branch decisions retain their host-window
scope. Complete main/nonlinear PCM passed the focused independent reference;
direct-output verification is separately recorded in the proof ledger. Other
native bank modes, external sends, activity/tail parity and math-library
equivalence remain open.

Compatibility policy retains instrument-wide squared CC7 output gain and its
centered CC10 balance. `Player::output` combines these with authored controls and
uses a retained5ms sample clock. Voice expression gain/pan has an independent5ms
clock at the group Amplifier position. These local clocks retain state through
internal fragments; Native program gain keeps its separate native window law. Authored
`set_controller(7/10)` retains this output-control namespace, including inside
Native instruments; it is distinct from `set_engine_par`. Disabling the physical
standard action selects the authored baseline while preserving the physical
input. CC7/10 remain instrument-wide rather than per-channel gain.

Compatibility MIDI1 pan preserves its exact arithmetic. Paired14 pan has center8192;
native32 pan has center0x80000000. Each side uses its own endpoint denominator,
giving exact center0 and endpoints−1/+1. This balance curve is a local playback
policy. Authored modulation continues to receive the original unipolar fraction.

CC11 and other raw controls modulate only authored routes. For example,
`Context::inputs` supplies a voice's channel CC row to `ModTable`. Adding an
unconditional CC11 multiplier would double-apply expression on instruments that
already assign it to volume. CC72/73/75 similarly must not overwrite imported
envelopes. `ModTable::scale_envelope` applies authored attack/release sources at
voice start; per-note expression remains a separate voice gain/pan/tuning input.

## Ordinary MIDI1 paired controls and velocity prefixes

One physical-input `Midi1Decoder` owns ordinary paired-controller transactions,
received parameter selection, and the pending CC88 prefix. Ordinary coarse
controllers1–31 pair with fine controllers33–63, excluding data-entry6/38.
Bank selectors0/32 retain their separate bank-selection meaning. A received MSB
clears its fine bits; later fine-only messages use the known coarse value.
Reset performance pairs use the explicitly configured reset baseline.
A fine message received before any known coarse value remains raw without
inventing a baseline. This follows
[MIDI1 Detailed Specification §Control Change, pp11–12](https://midi.org/midi-1-0-detailed-specification).

The local normalized curve preserves unpaired 7-bit values as byte/127. After
the first fine message with known coarse data, the paired controller uses the
combined 14-bit value/16383; subsequent coarse updates clear fine bits. Raw CC
callbacks keep the original controller number and byte, including a fine source
such as CC39. The combined value retains the physical message's input timestamp
and normalized precision. Cache/modulation acceptance follows forwarding.
Native7/10 standard parameter actions precede the callback; Compatibility default
actions retain callback wait/ignore behavior.
Authored and synthetic controls do not arm wire pairing. CC121 preserves volume
and pan coarse/fine values and their waiting intents; other performance pairs,
parameter selection, and the pending velocity prefix reset.

[CA-031](https://midi.org/high-resolution-velocity-prefix) defines CC88 as the
lower seven bits of the next wire Note On or Note Off on that physical channel.
Other MIDI messages may intervene. Every matching note consumes the prefix;
zero-velocity Note On still acts as Note Off and ignores the prefix. Explicit
Note Off release velocity can receive it, including a zero upper byte. The
resulting `Velocity` preserves the original upper byte for script/zone lookup
while carrying the full 14-bit normalized value to DSP. Native MIDI2 and authored
notes do not consume MIDI1 prefixes. Validation of this integration is pending
the final implementation checkpoint.

## CC121, panic, and host reset

CC121 leaves bank0/32, volume7/39, pan10/42, sound70..79, effects91..95, and channel
mode120..127 values unchanged. It restores expression11 and selectors98..101 to
127; modulation1 and pedals64..67 to zero. Bend centers, pressure clears, and
sustain/sostenuto release their holds without releasing physical keys. Other
controllers use the authored device default, or zero when there is none.
Selectors are deselected before reset data-entry callbacks; stored parameter
settings are not themselves reset. This matches the distinctions in
[MMA RP-015](https://midi.org/response-to-reset-all-controllers), whose publisher
download is [rp15.pdf](https://drive.google.com/file/d/1VzU1Bfv8iYLGD23ZfaTJVomEKNTtBh6n/view).

KSP virtual controller slots128/129 represent bend and channel pressure and
reset to zero. Native MPE distinguishes reset pressure from explicitly received
zero: reset restores unity fallback gain; received zero retains the existing
−12 dB pressure floor. `Control::ResetPressure` preserves this intent through
waiting default callbacks, while both expose zero to KSP. They are internal slots,
not additional wire CC numbers. Unknown
slots130..255 have no reset action. The complete pure-policy test sweeps all128
wire controllers, authored defaults, virtual slots, and unknown slots.

CC121 fences older performance-controller work by physical input provenance,
including CC, polyphonic pressure and received parameter callbacks. Two physical
inputs mapped to the same library channel must not cancel each other's waiting
work; authored/UI messages without a physical owner survive. Note and release
callbacks survive. A panic also cancels note lifetimes. A host reset
rebuilds engine state and replays initialization controllers, so an authored
standard controller value can be replayed after host reset even though CC121
restores its performance default. See
`controller_resets_restore_authored_device_defaults_without_audio_allocations`
in [plugin tests](../src/plugin.rs).

## MPE manager and member channels

The current [MMA/AMEI MPE v1.1 specification](https://midi.org/mpe-midi-polyphonic-expression)
uses **manager** for the channel formerly called master. Its publisher-linked
[specification PDF](https://drive.google.com/file/d/1QGjiK2QPPbii8YmES3vDEZlP63OGLiKr)
was checked directly; these statements do not rely on an unofficial draft.
Section2.3.1 recommends ignoring manager-only messages on members. Section2.3.2
requires meaningful combination of manager/member bend, pressure, and CC74.
Section2.2.5 requires the last member RPN0 sensitivity to apply across members.
Section2.4 stops per-note controls from changing a note after NoteOff.
Section2.2.3 requires stopping sound and resetting controls on channels entering
or leaving MPE. The publisher describes v1.1 as an editorial replacement for RP53.

Implemented policy:

- Manager ordinary CC updates are forwarded to member engine caches **after one
  script callback**. A script can ignore or delay them; member channels do not
  run duplicate callbacks. Data-entry/parameter selectors, CC74, and all-notes-off
  have separate paths instead of ordinary fanout.
- Member bend is derived from accepted physical state; manager bend stays shared.
  Member pressure and brightness are combined with manager values in voice
  modulation inputs. Brightness uses saturating addition; pressure uses maximum.
  An authored route determines their audible meaning.
- Member note expression freezes at physical NoteOff, including voices still
  sounding under sustain or sostenuto. `Engine::note_off_from` snapshots matching
  held voices and queued starts before script/native release processing;
  `release_host_note` does the same for exact host roots. KSP forwards the freeze
  before a release callback can wait. Pedals delay envelope release, not this
  expression snapshot. Manager expression continues to affect the zone. Initial
  member state is retained before a note arrives. Existing tests
  `mpe_same_pitch_channel_reuse_preserves_pedal_held_expression` and
  `mpe_script_transposition_keeps_physical_expression_and_release_counter` cover
  pedal-held reuse and delayed scripted release respectively.
- The receiver currently accepts ordinary member CC, including instrument-wide
  volume/pan and member-local pedals. This permissive compatibility policy
  departs from the manager-only admission recommendation. Stricter admission is
  deferred; it must preserve authored initialization and KSP-generated controls.
- Articulation Channel mode disables engine MPE membership because its physical
  channels are mapped to a common logical channel.

## Parameter negotiation and unsupported protocol semantics

Engine tracks RPN address bytes independently per physical channel, including null
selection and NRPN deselection. The same decoder applies MPE RPN0 pitch sensitivity
and RPN6 member count, including zone activation from Off. Member sensitivity is
shared across the zone; manager sensitivity is separate. Fine-only entries require
a known coarse baseline; coarse entries clear fine data. Generic parameter DSP
application, tuning tables and program banks remain unsupported.

Native MIDI2 CC never enters this MIDI1 compound decoder. Following the receiver
recommendation in [UMP v1.1.2 §7.4.6](https://amei-music.github.io/midi2.0-docs/amei-pdf/M2-104-UM_v1-1-2_UMP_and_MIDI_2-0_Protocol_Specification.pdf),
native CC0/6/32/38/98/99/100/101 is ignored before raw cache or script delivery.
Native CC96/97 remains ordinary authored controller input without changing an
armed MIDI1 transaction. Typed events and raw UMP packets share this validation;
native Registered/Assignable Controller messages retain their unified address
and full-width absolute/relative path. Playback proof is staged, unrun.

Outside active MPE zones, the same received MIDI1 RPN0 decoder configures the
addressed physical channel's pitch bend range as semitones plus cents/100. Both
absolute data bytes accept 0–127, so 127 semitones plus 127 cents means 128.27
semitones. Fine-only entries use received coarse data or the integer part
of an already negotiated physical range, capped at coarse127. Without either baseline they leave the
authored fallback intact. Accepted bend precision remains unchanged. This range
replaces only authored Pitch Bend→Pitch and survives CC121 and performance reset.
When that channel enters an active MPE zone, its ordinary MIDI1 override yields
to the zone's required sensitivities; independent native MIDI2 overrides remain.
The units follow [MIDI1 Detailed Specification, Pitch Bend Sensitivity](https://midi.org/midi-1-0-detailed-specification).
This ordinary-channel integration is implemented; playback validation is pending.

Received parameter selection/data-entry CCs still reach ordinary controller
callbacks. Complete absolute data entries additionally enter `on rpn/nrpn` from
slot zero with the captured delivered and physical input channels. These two
messages are queued together before execution: ignoring the raw CC does not
erase its decoded parameter; ignoring a parameter callback suppresses its
forwarding to later scripts. This order is an explicit local policy; the manual
alone does not prove Kontakt's exact callback cadence or ignore interaction.
NI documents received-parameter callbacks in its
[KSP callback reference](https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/callbacks);
`Runtime::rpn` and KSP `set_rpn/nrpn` still deliver authored parameter work directly
to subsequent scripts without re-entering wire decoding or synthesizing native
CC after the final slot.

The received decoder requires both selector bytes initially, then permits
one-byte address changes. CC6 supplies coarse data and clears fine data; CC38
uses the known coarse value for that selected transaction. Unknown fine-only
baselines are not guessed; no per-address effective-value bank is implemented.
CC96/97 produce relative intents internally, without inventing generic arithmetic
or absolute callbacks. Step sizes depend on the parameter; see
[MIDI RP-018](https://midi.org/response-to-data-increment-decrement-controllers).
Recognized RPN0 changes its supported cents field by one regardless of the wire
data byte, retaining the semitone field while the result fits0–127. Ordinary channels require a previously
accepted range; active MPE uses its negotiated manager/member range. After a
relative change, currently selected RPN0 decoders track the accepted coarse
baseline, including other members sharing a zone range. Ordinary channels retain
their compound fields over0–128.27 semitones. Only nonrepresentable cents−1/128
use the existing local numeric carry/saturation policy. That fallback canonicalizes the coarse value as
`min(total_cents / 100, 127)` and fine value as the remaining cents: the upper
128.00–128.27 range therefore remains coarse127/fine100–127, never an illegal
coarse128. This endpoint representation is a receiver policy; RP-018's decimal
carry does not restrict absolute data bytes to 0–99. MPE retains its existing
chosen 96-semitone receiver limit. Unknown parameter values retain authored
relative intent without an invented native action.

RPN1 fine tuning and RPN2 coarse tuning belong to the physical channel. Fine
tuning retains the native 32-bit word and uses `(value - 2^31) / 2^31` semitones;
MIDI1 absolute data zero-extends its 14-bit word by 18 bits. Coarse tuning uses
the upper seven bits minus 64 semitones; lower bits do not alter pitch. Their
neutral initial configuration is a receiver policy. CC121 and existing session
resets preserve these parameter configurations, as they preserve RPN0 ranges.
An authored `set_rpn` is a script notification, never a native tuning write.

RP-018 relative steps are 1/8192 semitone for fine tuning and one semitone for
coarse tuning, independent of the wire data byte. The latest absolute fine write
sets its endpoint resolution: pure MIDI1 stays within 0..16383, while a native
write retains its full fractional word under subsequent relative steps. That
mixed-protocol endpoint choice is explicit local policy. MPE uses direct physical
channel tuning here; optional manager-wide tuning fanout is not inferred.
Cached `f64` ratios compose once after per-note absolute pitch and before source
travel. The Kontakt source planner has no universal 32-source-frame pitch cap:
sample increments use checked 32.32 conversion and each requested source
trajectory must remain representable; wavetable increments reduce finite travel
modulo the cycle. A finite wire tuning value can still overflow its derived
exponential ratio. The planner diagnoses `UnrepresentablePitch` before committing
that source's PCM or clock, rather than clipping its pitch. Underflow to a finite
zero increment retains a supported stationary clock. Audio completion does not
release a still-held physical root; actual Off remains necessary for host
retirement. These are local source-admission policies, separate from the RPN1/2
parameter laws above and from UVI's native parameter behavior.

The independent complete-source PCM and exact-EOF fixture
`full_key_coarse_and_exact_pitch_ranges_reach_finite_plugin_pcm_without_heap`
in [performance_tests.rs](../src/plugin/performance_tests.rs) covers the authored
wide pitch ranges and host partitions. The checked-boundary fixtures
`vst3_extreme_finite_onset_tuning_keeps_processing_bounded_without_heap` in
[plugin.rs](../src/plugin.rs) and
`positive_saved_glide_with_unrepresentable_duration_fails_before_pcm_commit` in
[glide_tests.rs](../src/engine/glide_tests.rs) distinguish representable motion,
stationary underflow, failed source admission and physical note lifetime. These
fixtures do not establish arbitrary source or vendor pitch fidelity.
Both RPN and NRPN `127/127` deselect parameter input, as documented in
[UMP/MIDI 2.0 Appendix D.3.3](https://amei-music.github.io/midi2.0-docs/amei-pdf/M2-104-UM_v1-1-2_UMP_and_MIDI_2-0_Protocol_Specification.pdf).
CC121 clears the affected physical decoder and MPE manager resets also clear
member decoders. Host reset and Panic clear all decoders. Reset fences waiting
received RPN/NRPN work on affected physical inputs, preserving unrelated
channels and authored UI messages. Generic parameter DSP application remains
unsupported. MPE pitch/zone negotiation uses this same Engine decoder. RPN6
reconciliation identifies entering/leaving physical channels, cancels affected
scripted lifetimes, resets their state and publishes membership in the same event.
This reset addresses physical controller rows. Delivered-channel expression
rows can belong to unrelated surviving inputs and are retained; equal channel
numbers do not establish common ownership. Subsequent partial expression edits
preserve those notes' tuning, pan and brightness. Router note-on still supplies
a fresh destination expression, while direct Engine admission retains explicitly
authored destination state. The complete counterfactual PCM scenario in
[articulate_mpe_expression_tests.rs](../src/articulate_mpe_expression_tests.rs)
is staged and unrun.

ProgramChange has no musical input variant/native action; bank0/32 are raw cache
entries. Program switching remains unsupported; ordinary paired CC decoding is
described above. MIDI1 and native MIDI2 Program Change report this boundary through
the existing unsupported-host counter, including typed events, bare raw MIDI1/UMP
and packets with semantic companions (counted once). Companion processing and ordinary MIDI-thru
retain their existing path; native program/bank messages do not synthesize CC0/32
or select an instrument. The complete plugin PCM, expression/NOTE_END and relay
counterfactual in `program_change.rs` is staged and unrun. Message geometry follows
[UMP v1.1.2 §7.4.9](https://amei-music.github.io/midi2.0-docs/amei-pdf/M2-104-UM_v1-1-2_UMP_and_MIDI_2-0_Protocol_Specification.pdf).
MIDI2 channel CC, pressure and bend preserve their original precision through
canonical control delivery. Registered note gain/pan/tuning take separate per-note
paths; registered CC74 and exact host brightness preserve normalized precision.
Native per-note state is addressed by original port/channel/key, independently of
library transposition and delivery channels. Generation bindings reach exact and
anonymous roots, scripted children, waiting starts and deferred/release-only
samples. Controls can precede NoteOn and remain shared after NoteOff. Detach
freezes all old bindings; Reset clears current controls; combined flags detach
first. MPE snapshots do not suppress these native controls. Exact host overrides
apply last. CC121 resets poly pressure but preserves registered controls and bend.

CC121 resets poly pressure only in the current address generation. Detached
sounding generations retain their final poly pressure, including after the same
port/channel/key is reused. [UMP v1.1.2 §7.4.5 and Appendix C](https://amei-music.github.io/midi2.0-docs/amei-pdf/M2-104-UM_v1-1-2_UMP_and_MIDI_2-0_Protocol_Specification.pdf)
include Poly Aftertouch in per-note management and require detached notes to keep
their previous values through their lifetime. Physical reset still fences waiting
pressure callbacks; exact host pressure remains an independent overlay. The
focused `cc121_resets_current_native_pressure_and_keeps_detached_generations_in_complete_pcm`
scenario uses a pressure-to-volume instrument and an independent gain/tuning
oracle through Detach, reuse, CC121, later pressure, exact overlays and complete
release PCM, checking unrelated physical input, retained registered gain/bend,
raw mode receipt, terminal timestamps, render partitions and callback allocations.
It is staged for central validation.

Registered indices1,2,7,8,10,11,70–79,91–95 retain canonical values for authored
modulation. Existing gain7/pan10/brightness74 compatibility actions remain;
PN3 is persistent Q7.25 absolute pitch and NoteOn attribute3 is root-local Q7.9.
Unsupported profile/manufacturer properties do not erase base notes; reserved
fields are ignored as required by the receiver contract. This implementation
awaits the complete PCM and lifecycle validation pass.

Native MIDI2 Registered Controller bank0/index0 configures channel bend
sensitivity in the addressed physical channel. Its most significant 7 bits are
semitones; the next 7 bits are cents (0–127); the remaining 18 bits are ignored.
The full absolute range through 128.27 semitones is retained. These compound
fields follow MIDI1 Pitch Bend Sensitivity; native 32-bit relative RPN0 remains
unsupported rather than borrowing the MIDI1 CC96/97 law. Before negotiation,
authored Pitch Bend→Pitch sensitivity applies;
a negotiated range replaces that pitch contribution once, including an explicit
zero range. Other authored bend modulation remains active. Native manager and
member channels retain independent range overrides; MIDI1 MPE negotiation still
uses its zone-wide member-sensitivity policy.

Native Registered Controller bank0/index7 configures per-note bend sensitivity
as a full 32-bit unsigned Q7.25 semitone interval (0 through almost 128). The range
is captured for each subsequent physical Note On before script callbacks; a later
range change does not retarget an existing root or its waiting callback. Both
native range configurations survive CC121 and performance resets. The initial
48-semitone per-note range is a local compatibility default; the specification
does not mandate that default. These formats follow
[UMP v1.1.2 §7.4.7.1 and §7.4.13](https://amei-music.github.io/midi2.0-docs/amei-pdf/M2-104-UM_v1-1-2_UMP_and_MIDI_2-0_Protocol_Specification.pdf).
Source: `MidiState::apply_native_parameter`. Native range integration and its
playback validation are pending the final implementation checkpoint.
Native Registered/Assignable Per-Note Controllers and Per-Note Pitch Bend enter
KSP `on note_controller`, retaining their full payload and physical generation.
Scripts receive controller indices 0–255, 256–511 and 512 respectively, with an
explicit 7-bit controller or centered 14-bit bend projection. Registered pitch 3
retains its Q7.25 payload. `ignore_controller` suppresses native acceptance;
existing first-wait forwarding rules apply. Waiting callback state and queued
acceptance retain the original generation across Detach rather than resolving a
new current lane. Authored `set_note_controller` remains an inter-script message
and has no automatic native DSP action.

Defined native controls have their documented local actions; unhandled assignable
or reserved controls do not alias channel CCs. Rejections are counted separately
from bounded-capacity drops and included in plugin expression diagnostics. Exact
host tuple identity remains independently owned by the engine.

Native absolute RPN/NRPN notifications retain their original 32-bit value through
script work; KSP receives an explicit upper-14-bit projection. Recognized native
range configuration applies at ingress before the next physical note binds.
Unknown parameters have authored callbacks without invented DSP actions.

Native relative RPN/NRPN notifications retain signed32 deltas in a separate
parameter-data variant through typed/raw UMP ingress and callbacks. Relative RPN1
fine tuning and RPN7 per-note range apply signed full-resolution changes with
saturation. Relative RPN2 applies whole signed coarse steps; fractional substeps
are discarded toward zero without aggregation. Relative RPN0 and RPN6 native
actions remain unsupported, and unknown addresses never fabricate an absolute
baseline. Authored notifications retain their separate behavior.

## Verification and next implementation boundaries

Preserve zero audio-thread allocation and both direct/scripted execution paths.
Existing tests cover MPE manager callback fanout, independent member expression,
pedals, release snapshots, negotiated bends, controller reset, and channel-owned
script waits. The reset extraction adds a complete controller-policy sweep.

Future behavior changes need focused tests for physical/logical channel mapping;
manager-only member admission with authored initialization intact; interleaved
parameter operations with retained per-address effective values; same-event zone cleanup;
program selection; and Omni/Mono/Poly mode switching. Those features are not implied by this
state ownership refactor or by the full numeric controller coverage above.

Native wire MIDI2 values use explicit upper-bit projections for script/controller
fields: 16-bit velocity drops 9 bits, 32-bit controls drop 25 bits and 32-bit bend
drops 18 bits. The canonical normalized DSP value remains unchanged. Values made
from normalized host/API input retain their existing nearest-value projection;
those inputs do not claim an original MIDI2 wire word. A projected native zero
attack is still an explicit MIDI2 NoteOn, rather than a translated MIDI1 event.


## RPN0 compound-field ownership after fine-only edits

Absolute RPN0 retains its coarse HCU/semitone field independently of the summed
sensitivity. Legal cents100–127 do not implicitly carry that absolute coarse
field: coarse10/cents127 sounds at11.27 semitones, but a subsequent fine-only
cents25 edit means10.25, including after CC121 and parameter reselection.
`MidiState` now owns each recognized sensitivity as one private
`BendSensitivity { semitones, coarse }`; playback borrows that same owner and
uses its numeric projection. Performance/session resets retain both fields,
whereas input replacement and the existing ordinary-MIDI1-to-MPE replacement
policy clear the complete owner. There is no parallel coarse cache or copied
playback table. Unknown fine baselines still do not fabricate a sensitivity.

A recognized native absolute RPN0 updates the already selected physical RPN0
transaction's coarse baseline, preventing later MIDI1 fine input from restoring
an older setting. It does not select a parameter, undo a null selection, or
alter an unrelated RPN/NRPN transaction. Relative MIDI1 cents retain the coarse
field whenever the next cents byte is representable; only nonrepresentable
endpoints use the explicit local numeric carry/saturation policy. Native
relative RPN0 remains unsupported.

The staged `rpn_coarse_tests.rs` complete stereo Engine PCM compares actual
physical bend with independent literal exact-note tuning on two physical inputs
sharing a delivered home channel. It covers fine-only edits after CC121,
native replacement of a selected MIDI1 baseline, null/reselection, legal
absolute cents127, relative carry followed by fine input, waiting note callbacks,
exact tuple retirement/reuse, literal terminal timestamps, empty/irregular
render partitions and callback allocations. The source uses no RPN state or
sensitivity helper in its oracle. No tests or Cargo were executed by this owner.


### MIDI1 relative RPN0 qualification

CC96/97 ignore their value byte and change the supported RPN0 cents LSB by one.
When the next cents byte fits0–127, the recognized ordinary or MPE owner retains
its original semitone MSB and reseeds only an
already selected RPN0 transaction. Null selection still disables the operation;
CC121 clears selection while retaining the recognized compound configuration.
These are separate rules from absolute Data Entry: a fine-only absolute edit
must retain the established coarse field, including legal absolute cents100–127.

Coarse10/cents127 followed by CC97 now retains10/126, sounding at11.26; a
subsequent fine-only cents25 edit means10.25, including after selector reset.
Similarly12/99 plus CC96 retains12/100, so a later fine25 means12.25. RP-018's
carry-at100 example describes a receiver without cents support. It leaves the
supported-LSB result−1/128 boundary unclosed; those two cases retain the current
local numeric carry/saturation policy. No native Kontakt relative-field parity
is claimed. Native MIDI2 relative RPN0 remains separately unsupported.

The staged `relative_rpn0_supported_cents_keep_coarse_fields_in_ordinary_and_mpe_complete_pcm_without_heap`
uses complete stereo Engine PCM against independent literal note tuning,
including selected-member fanout, reset/reselection, frozen release pitch,
same-tuple reuse, exact endings and fixed/irregular renders. It has not been
executed by its author.


### Native tuning and selected MIDI1 callback baselines

Recognized native absolute/relative RPN1 and RPN2 edits now refresh the coarse
baseline of an already selected matching MIDI1 RPN transaction from the same
physical `ChannelTuning` owner that playback uses. A subsequent fine-only CC38
therefore reports the accepted coarse value in `$RPN_VALUE`, rather than an older
CC6 value while DSP uses the new tuning. Native input does not select a parameter,
re-arm null selection, or alter another RPN/NRPN transaction. Existing native
RPN0 and MIDI1 relative tuning use this same guarded decoder helper.

[UMP v1.1.2 §§7.4.7–7.4.8](https://amei-music.github.io/midi2.0-docs/amei-pdf/M2-104-UM_v1-1-2_UMP_and_MIDI_2-0_Protocol_Specification.pdf)
defines the registered destination and current-value updates;
[NI's RPN callback documentation](https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/callbacks)
exposes fine/coarse tuning through `$RPN_ADDRESS` and `$RPN_VALUE`. The mixed
protocol callback cadence remains the stated local policy, not native Kontakt
parity. The focused `rpn_tuning_callback_tests.rs` complete PCM scenario compares
callback-driven gain with independent literal voice gain/tuning, covering native
absolute/relative changes, physical-input isolation, null/other selectors,
waiting callbacks, stale ended tuples, exact tuple reuse and terminal timestamps.
Central functional validation passed the complete callback-driven PCM/lifetime
scenario on 2026-10-04 (0.02s), including allocation and partition assertions.

### VST3 normalized Brightness and SDK pressure

VST3 predefined Brightness (type5) is consumed as an absolute normalized CC74
modulation source in KONTRA. This is a local source mapping; the SDK does not
prescribe a filter curve or a neutral Brightness default. The exact note ID and
original f64 value reach the existing host-note overlay, including its released
family, without writing channel CC74 or the KSP raw controller receiver. Native
SDK PolyPressure remains a separate exact-address pressure event; Expression4
and type6 are not interpreted as pressure.

The public advertised types remain Volume/Pan/Tuning. The initial exact-note
Brightness overlay is absent (`None`), so an unexpressed note retains accepted
physical/MPE CC74 and each authored assignment's own substitute or unavailable
state. Concurrent assignments can therefore start at different values on the
same event bus/channel; no single normalized descriptor default represents that
fallthrough. The SDK defines no universal Brightness curve/default, and its
bus/channel-specific metadata requires a host notification when expression state
changes. The current static shim has no authored consensus/default owner. Adding
a fabricated neutral default or a new sentinel/relative mapping would alter the
established absolute source contract. Explicit type5 consumption remains useful,
while advertising it requires that separate owner/policy closure. Consumption
fixtures are staged; this section does not claim they have passed. Primary SDK:
[ivstnoteexpression.h](https://github.com/steinbergmedia/vst3_pluginterfaces/blob/master/vst/ivstnoteexpression.h).
