# Native UMP ingress

## Pinned protocol and scope

The wire contract is MIDI Association/AMEI **M2-104-UM v1.1.2**, published
2023-11-10: [official specification PDF](https://amei-music.github.io/midi2.0-docs/amei-pdf/M2-104-UM_v1-1-2_UMP_and_MIDI_2-0_Protocol_Specification.pdf).
Sections 2.1, 3, 4, 7 and channel-voice diagrams were checked directly.
Words supplied to the decoder are native-endian `u32`; transport byte order is
outside this crate. Protocol selection is explicit per group, never inferred
from incoming notes. MIDI 2.0 zero-velocity Note On remains Note On.
Reserved fields are ignored and reserved message types retain their defined
packet lengths. A truncated packet ends iteration rather than guessing a new
boundary inside its payload.

`sampler-midi` depends only on the new `sampler-core`. It borrows input words and
performs no heap work. The decoder preserves integer resolution, group/channel,
controller namespace/bank/index, relative signed values, note attributes and
management flags. Unsupported packet types remain inspectable as raw words.
Floating-point projection happens only on an explicit consumer request.

The current musical ingress applies ordinary notes, MIDI 2.0 Pitch 7.9 note attributes, sustain/sostenuto and
zero-valued CC123 (All Notes Off) and CC120 (All Sound Off).
Full input address and protocol reach the core's physical-note matching; note-off
uses its FIFO overlap policy. Release velocity and attributes are returned to the
caller. Unknown release attributes do not strand a note. Unsupported Note On
attributes return `Unsupported` without creating a partial note.

This is partial V2-02/V2-15 evidence, **not full MIDI 2.0 support**. Per-note and
channel expression in ordinary ingress, management, program selection, MIDI-CI,
SysEx, JR timestamps and device transport remain unimplemented. The separate fixed-zone
MPE note/expression projection below is partial receiver work. Decoding a
message does not imply that the instrument consumes it. `Applied::Unsupported`
is observable; there is no approximation through the old engine.

## Absolute note pitch

Attribute type 3 follows section 7.4.15.3 of the pinned specification. Its 16-bit
unsigned Q7.9 value becomes an exact native `NotePitch::Absolute` value; all nine
fractional bits survive. The transmitted Note Number remains the physical address
for key-up/FIFO matching. This sampler chooses the integer part of absolute pitch
for region selection, an explicit receiver policy allowed by section 7.4.15.

Absolute pitch overrides the prepared per-key tuning for this note, with live
expression applied relatively afterward. It does not rewrite a tuning table or
persist into later note-ons. Native generated `Play` instructions transpose the
inherent pitch while preserving its fraction, independently of their expression
inheritance policy. Unsupported resulting source rates fail before publishing
partial layers. Fixed-pitch regions remain authored fixed-pitch sources.

Real UMP tests check adjacent fractional values, audible analytic pitch, a physical
note index different from the musical key, unknown release attributes, subsequent
ordinary tuned notes, and failed rate admission. Callback heap guards and block
sizes 1/7/64/256 cover the path. Registered per-note Pitch 7.25 and live per-note
pitch bend remain unimplemented; this attribute does not imply those capabilities.

## Native integration and time

The independent offline executable now uses UMP for its note input. Its demo
splits rendering at fixed sample timestamps, applying note-on at zero, sustain
at 0.25 s, note-off at 0.5 s and pedal-up at 1 s. The 50 ms envelope release ends
at frame 50,400 at 48 kHz. WAV copy rendering uses the same MIDI 2.0 note ingress.
This is a sample-time composition root, not a live device clock synchronizer.

## Executable evidence

`cargo test --locked -p sampler-midi` covers all 16 packet lengths and truncations,
all group/channel addresses, channel-voice golden vectors, reserved fields,
10,000 deterministic framing inputs, 7/14/16/32-bit resolution, protocol mismatch,
unsupported admission, pedals and terminal identity. The ingress test counts
allocations and deallocations around actual note/pedal/render/retirement work.

All three new crates pass strict all-target Clippy, release tests on Rust 1.99,
and tests on MSRV 1.92. The scoped Rust Doctor 0.7.0 scan reports **91**, complete
and authoritative, zero errors, 83 warnings; no rules disabled. Local evidence
is under ignored `artifacts/architecture-v2/midi-*`, including source hashes.
The full workspace baseline scan against `47d6aff` passed with no new errors.
The native demo produced 96,000 stereo frames with the expected held interval
and exact silent suffix after release. Production legacy playback is unchanged.

## Host block contract

`Ingress::render` accepts already-framed packets with sample offsets and an explicit
per-call event budget. Before touching audio or runtime it checks the total budget,
clock overflow, monotonic offsets and block bounds. Invalid batches leave both
unchanged; the host decides its failure-output policy. Events at the exclusive end
belong to the next block; an empty block can apply offset-zero events without
advancing time. Equal offsets preserve input order. Previously queued internal
work at a boundary runs before external packets at that boundary.

Every admitted batch event reports its own result. A full note pool does not skip
a later release in the same batch. Unsupported messages and protocol/admission
errors are observable through the callback, which the host must keep realtime-safe.
Work is bounded by the caller's event budget, block size and prepared core limits.
No queue, allocation or sorting occurs in this path.

The native executable now uses this same processor with 256-frame blocks. Its
output is byte-identical to the prior explicitly split demo. Independent tests
cover every block size 1–32, same-frame note-on/off ordering, empty blocks, invalid
and descending offsets, budget rejection, continued cleanup after capacity failure,
and zero callback allocations/frees. Rust 1.99 strict Clippy and tests plus MSRV
1.92 tests pass. The updated authoritative new-core score remains 91.

## Channel-scoped All Notes Off

CC123 with value zero now releases all held input keys in the addressed protocol,
port, group and channel. `Applied::AllNotesOff` reports the number released;
repetition reports zero. Nonzero values remain explicitly unsupported. Each channel
is an independent receiver here; Omni/Mono mode and multichannel receiver mappings
are not implemented. UMP Appendix B.1 preserves MIDI channel-mode definitions
within each group and excludes other groups. See the pinned specification above
and the [MIDI Association message summary](https://midi.org/summary-of-midi-1-0-messages).

The native gate policy respects sustain and captured sostenuto and starts ordinary
release envelopes only when the effective gate closes. Linked children follow that
gate. Independent generated-note durations and callback-retained waits keep their
explicit policies. This command is not hard silence; CC120 has the separate contract below. Pedal
state is unchanged.

The core scans bounded note/channel storage once, then uses its existing release
propagation and cleanup. It neither registers a channel nor enqueues a command, so
full channel/command pools cannot prevent cleanup. Terminal delivery still requires
sink acceptance. MIDI 1.0 and 2.0 tests compare exact output across blocks 1–16 for
no pedal, sustain and selective sostenuto capture. Separate capacity/identity checks
cover other ports, groups and protocols, invalid addresses, unsupported values,
repetition, future expression cleanup and terminal rejection. All operations are
allocation/free checked.

The complete authoritative new-core scan remains 90 with zero errors and 119
warnings. Evidence uses `artifacts/architecture-v2/notes-off-*`.

## Channel-scoped All Sound Off

Zero-valued CC120 hard-stops every admitted source voice in the input domain,
including release tails and delayed starts. Native pending callbacks in that domain
are cancelled even when their wait lifetime is callback-retained; completion/fault
records still require acceptance. Independent generated descendants are included.
Other domains and pedal controller values remain unchanged. `Applied::AllSoundOff`
reports stopped voice reservations, including delayed sources. Nonzero values are
unsupported. This implements the immediate-silence purpose described in the
[MIDI Association summary](https://midi.org/summary-of-midi-1-0-messages); callback
cancellation and resource reporting are explicit native policies.

Every note now retains its immutable originating channel address, inherited on
child admission independently of logical transposition and expression inheritance.
Scope matching therefore uses bounded linear arena scans without repeatedly walking
ancestry. Sources, pending work and controllers keep their separate owners.

Hard silence closes musical gates but preserves physical input keys until key-up
(or an explicit release/panic). Such input owners cannot emit NOTE_END early. A new
same-key note therefore cannot steal the older silenced note's FIFO key-up. A queued
physical key-up survives silence; an earlier actual key-up cancels that queued work
before the input can retire, including while sustain remains down. This distinction
is necessary even when no voices or behavior records remain.

Checks cover saturated queues, multi-generation independent descendants, release
tails, delayed starts, retained callbacks, unaffected channel execution, unchanged
pedals and exact audio across partitions. MIDI 1.0/2.0 checks exercise same-key reuse,
late scheduled key-up cancellation, unsupported values and exactly-once terminals.
Actual work remains allocation/free checked. The authoritative score remains 90,
zero errors and 119 warnings. Evidence uses `artifacts/architecture-v2/sound-off-*`.

## Fixed-zone MPE note/expression projection

The separate `Mpe` adapter pins [M1-100-UM MPE v1.1, 14-Apr-2022](https://midi.org/mpe-midi-polyphonic-expression),
sections 2.2.4–2.2.8 and Appendices C/D. Construction binds one runtime identity,
port/group and lower or upper zone with 1–15 members. Runtime identity survives
moves and plan adoption; a foreign runtime is rejected before any mutation.
Construction allocates explicit note-binding and gesture budgets off the audio
thread. Apply performs no heap work. One adapter owns admission within its input
domain. Raw-event interception precedes this projection; consumed messages must
not reach it. A raw scripting interception API is still pending.

Current support is MIDI 1.0 UMP note-on/off, pitch bend, channel pressure and CC74,
with initial
manager/member ranges of 2/48 semitones and whole-semitone RPN 0 updates below. Manager and member bends add; projection
uses a piecewise bipolar scale with exact center and endpoints. Both channel values
are retained while idle and installed before source selection or note programs.
Multiple active notes on a member share its gestures. Each admitted root retains
its own generational binding and member-pitch snapshot. The core's physical
`key_down` state decides which members can change: a released note freezes its
member pitch immediately, including under sustain, while manager pitch continues
to reach the retained owner. Native scheduled key-up follows the same rule.
Channel reuse cannot retarget an earlier tail. Linked generated notes share their
owner; explicit snapshot/independent inheritance keeps its native meaning.

`Runtime::set_expressions` preflights all owners and source rates before committing
a gesture. Invalid/stale/duplicate owners reject the complete batch. Scratch is
allocated with the expression arena (one optional projected-expression record per reserved owner); a batch
clears that bounded storage, projects each owner once, and scans occupied voices
once. An unchanged-pitch batch skips voice validation. Failed batches do not
publish expression or adapter controller state. Due work runs before preflight,
as for other immediate core operations. The MPE binding budget includes tails and
unaccepted terminal notes; stale bindings are reclaimed without pinning owners.

Checks cover both zones, overlapping same-key notes, held tails, scheduled key-up,
member reuse, manager propagation, failed gesture rollback, full binding budgets,
terminal rejection, foreign runtime/group/channel isolation, exact audio across
1/7/64/256/512-frame partitions, and allocation/deallocation guards.

`cargo run --release -p sampler-midi --example expression_workload` measures a
whole-zone gesture separately from resampling and admission. One idle CPU-2 run
on the local Ryzen 7800X3D measured medians 0.85/3.38/13.78 microseconds for
64/256/1024 notes, about 13.2–13.5 ns per note. With 64 active and 4096 reserved,
the median was 1.62 microseconds. These are local observations, not deadlines or
full audio callback guarantees; CSV is `artifacts/mpe-expression-workload.csv`.

MCM configuration, fractional/relative RPN, zone channel modes and ordinary
MIDI 2.0 expression routing remain unsupported. Do not forward
unsupported zone controls to ordinary channel ingress as a substitute. Source
rate limits can reject otherwise valid MPE pitches; the current 1/256..16 source
step range does not establish full MPE compliance. No importer-specific MPE code
or old-core runtime is involved, and no live host/UI is connected yet.

### Pressure and timbre

Channel pressure combines by maximum; CC74 combines as member plus manager minus
64, saturated to 0..127. Manager defaults are pressure 0 and CC74 64, leaving the
member unchanged. Manager notes use neutral member values, avoiding double
application. These are explicit native receiver choices under Appendix D, not
claims that every vendor instrument uses the same mappings. Original 7-bit values
remain in channel/member snapshots. Native expression receives the exact integer
projection `floor(value * u32::MAX / 127)`; this is not UMP bit-depth translation.

As with pitch, member pressure/timbre freezes at physical key-up, and new notes use
current idle controller state. Manager controls combine with each retained member
snapshot, including tails. Each gesture writes only its own expression dimension;
it cannot overwrite native gain, pan, pitch or another controller dimension.
Pressure/timbre are modulation inputs, not hardwired gain/filter shortcuts;
[prepared native programs](MODULATION.md) can now map them to gain, balance and pitch. Tests cover both zones, initial
state, saturation, tail/reuse isolation, manager combination and preservation of
unrelated expression under heap guards.

The workload now includes all three gestures. One local run measured 1024-note
medians of 14.58 microseconds for pitch, 10.45 for pressure and 10.37 for timbre.
The pressure run included a 2.39 ms maximum scheduler outlier, reinforcing that
these process timings are observations rather than realtime guarantees. Evidence
uses `artifacts/mpe-controls-*`; filtering/rendering remains outside this workload.

### Whole-semitone pitch-bend sensitivity

RPN 0 now accepts CC101/100 selection in either order, CC6 sensitivity from 0 to
96 semitones and a zero CC38. Per-channel selectors start null; NRPN selection
(CC99/98) disables RPN data entry, and null or unknown RPNs cannot change pitch.
Nonzero fractional CC38 and relative data entry remain explicitly unsupported.
`Applied::Configuration` reports selector/zero-LSB state handling; actual range
changes report the affected expression-owner count. It does not mean the entire
MPE receiver has been configured.

The manager range is independent. The last accepted member sensitivity updates
every member channel in that zone, as required by section 2.2.5. Active member
notes reproject their retained raw 14-bit bends; released notes keep their prior
member pitch in semitones. Manager range changes still affect retained sounding
owners. Idle controller state is updated for future notes. Raw bend values are
stored separately from projected note values, so selecting a zero range cannot
lose the position needed when a nonzero range is restored.

Projection, source-rate preflight, expression commit, channel snapshots and range
commit form one transaction after due work. A rate failure on any source preserves
all prior expression/range/controller state. Bounds above 96 reject rather than
clamp. Accepting a range while idle does not guarantee every subsequent note's
source rate is supported; ordinary source admission still validates that case.

Heap-guarded lower/upper-zone tests cover different member bends, shared range
updates, independent manager sensitivity, released-tail snapshots, late failure
rollback, out-of-range input, partial/null/NRPN selectors, unsupported fractions
and exact restoration after zero sensitivity. Release, Clippy and MSRV checks
remain the validation gate. The separate gesture workload still measured roughly
14.9/10.4/10.3 microseconds at 1024 notes for pitch/pressure/timbre; local evidence
uses `artifacts/mpe-rpn-*` and excludes rendering.

### Zone sustain and sostenuto

Manager CC64/66 now apply to all member channels and the manager in one native
`ChannelScope` event. The scope names a protocol/port/group and a 16-bit physical
channel set; it does not define musical part or articulation routing. Member
CC64/66 returns `Applied::Ignored` without changing controller state, following
section 2.3.1. Unimplemented channel-mode messages remain unsupported; in particular,
Appendix E prohibits manager CC120, so it must not be approximated as zone panic.

Pedal-down preflights all missing controller domains before registering or changing
any of them. The scope requires at most `members + 1` channel reservations, which
also retain the pedal value for future notes. Pedal-up changes existing domains
without reserving absent channels, so capacity cannot strand held notes. Channel
values are updated together, followed by one note walk and ordinary release/cleanup.
Sostenuto tracks rising edges per channel; repeated down does not recapture later
notes. The single-channel and scope paths share the note-gating implementation.

Tests cover both zones, sustain-held versus physically held keys, selective
sostenuto capture, ignored member pedals, protocol/port/group isolation, failed
whole-zone reservation without partial state or leaked slots, pedal-up under full
capacity, and full fifteen-member masks. Existing core gate, scheduler, behavior,
plan and MIDI checks still pass under the shared implementation. All event,
rendering and cleanup checks retain allocation/deallocation guards.
