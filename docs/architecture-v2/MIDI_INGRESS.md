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

The current musical ingress applies un-attributed notes, sustain/sostenuto and
zero-valued CC123 (All Notes Off).
Full input address and protocol reach the core's physical-note matching; note-off
uses its FIFO overlap policy. Release velocity and attributes are returned to the
caller. Unknown release attributes do not strand a note. Unsupported Note On
attributes return `Unsupported` without creating a partial note.

This is partial V2-02/V2-15 evidence, **not full MIDI 2.0 support**. Per-note and
channel expression, management, attribute pitch, program selection, MIDI-CI,
SysEx, JR timestamps, device transport and MPE remain unimplemented. Decoding a
message does not imply that the instrument consumes it. `Applied::Unsupported`
is observable; there is no approximation through the old engine.

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
explicit policies. This command is not hard silence: CC120 All Sound Off remains
unsupported. Pedal state is unchanged.

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
