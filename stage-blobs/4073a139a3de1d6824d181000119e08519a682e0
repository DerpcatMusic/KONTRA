# Program and instrument-bank switching

Read-only design audit, 2026-10-04. Runtime switching remains unsupported.
The MIDI1/MIDI2 Program Change diagnostic correction makes this limitation
visible; it does not select a program, load a preset or complete instrument-bank
support. The bounded native dispatch and retention evidence below is now established; executable bank switching remains unsupported.

## Primary contract

The [NI Kontakt classic-view manual](https://docs.native-instruments.com/ni-tech-manuals/kontakt-manual/en/classic-view)
describes an Instrument Bank as up to 128 instruments in one rack slot, receiving
on one MIDI input channel. Program Change selects the active instrument in that
bank. This describes an instrument-bank roster; it does not establish that a
normal NKI, a rack slot index or a snapshot ordinal is a MIDI program number.
The manual's visible slot numbering must be distinguished from wire program
values when closing the importer mapping.

MIDI2's optional bank pair belongs to its Program Change message; it must not be
expanded into fabricated physical CC0/32 receipts. See
[UMP v1.1.2 section 7.4.9](https://amei-music.github.io/midi2.0-docs/amei-pdf/M2-104-UM_v1-1-2_UMP_and_MIDI_2-0_Protocol_Specification.pdf).
That packet geometry alone does not define a Kontakt instrument-bank mapping.

## Current source boundary

- `src/import.rs:329–341`, `multi_programs`, reads the Kontakt Bank's SlotList
  and each slot's ProgramList. It explicitly rejects a ProgramList with more
  than one program. The returned `u32` is the outer slot identifier.
- `src/import.rs:342–348`, `read_multi`/`read_program`, preserves that container
  selection. `parse` at `src/import.rs:794–798` accepts only index zero for an
  NKI; for an NKM it selects the matching outer slot from `multi_programs`.
- `src/plugin.rs:35–103`, `Part`/`Part::source`, stores a source identity of
  `(path, program, snapshot)`. Its `program` currently carries that container
  selection index. Connecting MIDI Program Change directly to `Part.program`
  would change the meaning of existing saved state and select a rack slot
  instead of an instrument within a bank.
- `src/plugin.rs:2935`, `exact_host_input`, validates original raw addresses and
  packet geometry before unsupported Program Change handling.
  `typed_host_input` handles typed MIDI1/MIDI2 requests. Paired raw events use
  their semantic companion once; bare raw MIDI1 and MT2/MT4 UMP requests report
  the same unsupported boundary. No program/bank action or synthetic CC is
  produced. The full PCM/exact-owner fixture is
  `src/plugin/performance_tests/program_change.rs`; this audit does not claim a
  test result.

## Smallest required owners before implementation

1. Preserve the real bank roster while importing ProgramList: outer rack slot,
   actual program slot/wire mapping, program source, and empty-slot behavior.
   Keep this separate from the existing container-selection index and from
   snapshots. Do not infer roster entries from file names or rack order.
2. Prepare the supported roster and its immutable playback resources off the
   audio thread. Retain each program's initialized script/effect/control state
   according to proved native behavior, with bounded ownership and explicit
   resource retirement. A wire event must not parse, load, allocate or destroy
   an instrument on the callback thread.
3. Give the receiving bank one selector owner scoped to its actual physical
   input port/channel and supported group. Preserve event timestamp and arrival
   order. MIDI1 accepted bank-select receipts and a MIDI2 optional bank field
   need an explicit supported mapping and omission/reset policy; numeric caches
   do not supply that policy. Delivered KSP/articulation channel numbers must not
   replace the physical selector address.
4. Install only a prepared, current selection. Existing `Handoff::Part` and
   `Retired` at `src/plugin.rs:1047–1085` already separate audio installation
   from worker destruction. `prepare_snapshot` at `src/plugin.rs:2008–2082`
   supplies the existing source/epoch/generation cancellation pattern for a
   worker transaction. Reuse those owners where their semantics fit; neither
   path currently implements a MIDI bank selector.

## Native lifetime evidence still required

`Engine::set_bank` at `src/engine/mod.rs:346–387` chokes exact host roots, cleans
up scripted input notes, clears voices and queued commands, then replaces the
bank. This is an existing source-replacement policy, not evidence of the native
musical Program Change policy.

Before choosing that path or retaining multiple prepared program instances,
trace or measure a real native instrument bank for switches while a key is
held, during release, under sustain/sostenuto, and with waited callbacks or
generated children. Close which old program owns the subsequent physical/exact
NoteOff, whether release-trigger samples and effect tails continue, which
callbacks/controller state survive reactivation, and when exact NOTE_END is
returned. Include empty slots, rapid repeated changes and incoming note ordering
at the switch frame. A later expression for an old exact tuple must retain its
original program generation or be rejected by a proved terminal policy.

The meaningful future proof is a real imported bank, physical selector input
and complete audio/lifetime comparison across those cases, including missing
or stale worker completion. Current diagnostic PCM equality proves requests
leave existing playback alone; it cannot prove switching semantics.


## Pinned native selection and retained ownership closure

Static captures use Kontakt8.13.1,178386944-byte PE image, SHA256
`0fe6356e0879d058b6e5b73507c54c5e345cea451b35287c974e438291d4dae8`,
image base `0x140000000`. Protected captures are
`artifacts/core-native/program-bank-*`; no native executable or audio fixture
was run for this investigation. The full dispatcher captures carry the actual
branches rather than a name-based inference.

- Native wire dispatch `0x1408ed170`, status-C0 branch
  `0x1408ed361..0x1408ed3aa`, extracts MT2 or MT4 program, masks127,
  calls `0x1409194b0`, and returns. It does not consume the MIDI2 bank pair,
  emit CC0/32, stop voices, cancel callbacks, reset pedals or unlink events.
  `0x1409194b0` matches eligible outer containers by physical channel selector,
  requires container mode2, and writes selected-program word `+0x44a`.
  The UI setter `0x1408bdc60` also changes that word without cleanup;
  `0x14094ed20` is an initializer, not a switch operation.
- New physical NoteOn at `0x14086cd30` uses selected-program `+0x44a`,
  skips an empty selected pointer, and captures
  `outer_slot*128+inner_program` in event `+0x18`. Earlier events keep that
  identity. Ordinary Off `0x14086ca40` searches the global held-note chain,
  binds the original event and original release velocity, and resolves its
  saved program rather than the current selector. Off dispatcher
  `0x1408f4160` resolves that same saved identity, clears logical-held bit2,
  runs the original program's release callbacks and visits its `+0x11390`
  voice list. Repeated Off checks the original pending-Off pointer.
- Generated `play_note` at `0x14079edc7..0x14079ede7` writes event `+0x18`
  from the current script context's packed program, resolves that program,
  and inserts the child into its `+0x113a0` event list via `0x140996050`.
  The child is not redirected by a later active selector. This also bounds
  generated release children from an old program's release callback.
- Untargeted ordinary CC/bend/pressure enqueue `0x14086c670` initially
  selects the current program. Explicitly targeted delivery instead uses
  the supplied packed program. Wire CC dispatch `0x1408ed3db..0x1408ed42c`
  additionally walks per-controller delivery requests and re-enqueues the
  original receipt with each requesting program's saved identity.
  Helper `0x140955ac0` deduplicates requests by packed program;
  `0x14086c670` removes the matching request when it delivers the receipt.
  The actual system sustain template `0x144f17420` ignores a pedal-held
  release and requests64; template `0x144f174f0` subsequently calls
  `note_off(by_marks(...))` on64up. Thus old inactive programs can receive
  their requested sustain-up. A blanket current-program-only controller
  implementation would lose this proved path. These templates do not
  establish a corresponding CC66 policy.
- Audio container traversal `0x140889a30` reads the roster at
  container `+0x418/+0x420`, whose entries are i16 inner program IDs;
  it does not read selected-program `+0x44a`. It visits each retained
  program's DSP/racks/returned sends and sums through `0x1408ef460`.
  Consequently selector changes alone do not remove old program voices
  or effect instances. Existing effect activity/expiry laws remain their
  own DSP policy; no arbitrary new bank-tail timeout is established.

The native ordinary event cleanup described in `KSP_EVENT_COMPLETION.md`
already distinguishes logical-held flags, voices and continuations. Selection
adds no new terminal condition in the inspected switch path. Native VST3
queue retirement is not evidence of a CLAP NOTE_END ABI emission. KONTRA must
retain its existing exact host address/generation through all original-program
voices, callbacks and release descendants and report completion through its
actual supported host contract; this audit does not invent a Kontakt CLAP event.

## Bounded implementation consequence

A prepared bank container must retain one Engine/Runtime/effect state per actual
occupied inner slot, render those states after selection changes, and route an
old note's Off/expression by its original program owner. Selection alone must
not call `Engine::set_bank`. New On and ordinary controller delivery target the
selected program; requested old-program CC64 delivery is an additional proved
path, not all-controller fanout. Same-frame arrival order and empty slots need
one selector transaction on the existing input timeline.

Before exposing this slice, preserve the discarded ProgramList i16 slot field
in the importer rather than confusing it with `Part.program`'s outer container
index. Prepare resources off audio; keep reactivation state and asynchronous
source epochs attached to the retained program. Container routing/amplifier
requires an explicit projection from those retained program engines. The
container owns the mode2 sample-voice rule, while its admission count and
victim selection retain the original inner-program owner; the exact native
closure below supersedes the earlier shared max-note inference. Remaining
strict limits are bank-select mapping, CC66/reset fanout, admission of the
container rule within prepared storage and unsupported controller requests. They must remain
explicit until implemented; the diagnostic correction does not close them.


### Implemented slot-retention prerequisite

The already-read signed i16 row field is retained beside its `Program` in
`vendor/ni-file/src/kontakt/objects/program_list.rs`, using one paired entry
owner, `Vec<(i16, Program)>`. The previous reader at25–36 explicitly
discarded it. The only production consuming projection found in this repository
is `src/import.rs:338`; adapt that projection to extract the Program while
preserving its existing outer-slot result and its existing multi-program
rejection. Do not silently enable bank loading or reinterpret `Part.program`.
This preparatory change preserves file information needed by a later actual
roster API without changing any playback or saved-state semantics. Its focused
parser proof should author two sparse row IDs in a real structured-object list,
assert both IDs and programs survive in order, and retain the existing bank
rejection. An exposed0..127 wire-slot validation remains a later bank-API policy;
the lossless parser should preserve the original i16 rather than guess it.

Implementation beyond that prerequisite needs a prepared per-slot roster owner
in import/plugin, one ordered selector per bank container, and retained
Engine/Runtime/effect instances plus exact tuple-to-program ownership. Container
routing/amplifier and per-inner voice-rule projections, plus the bounded controller
request mechanism must be represented before advertisement. A meaningful
switch proof must compare full old/new PCM and independent old-root release
geometry across held switches, pedal requests, waited child admission and
empty-slot ordering; parser retention alone cannot pass for program switching.
The implementation and staged check are described below; test execution remains
owned by the root task.


The bounded native archive read now independently closes that row field's
routing use. BProgramContainer's secondary archive vtable `0x144e983f0`
uses child reader `0x140d06870` at+60. It requires chunk ID0x36, reads an
u16 row count, then reads each signed i16 row ID, adds its parent context's
packed slot base from `0x14098d680`, installs/looks up that exact packed
program through `0x14094f280`/`0x1409804e0`, and reads its nested Program.
The reader does not substitute dense iteration order for the row ID. Capture
`program-bank-archive-child-list.c` retains this actual archive chain.
The current lossless parser proposal therefore preserves a routing field
used by the native loader, not an unnamed padding word. Negative or oversized
raw rows still require explicit validation at the future bank API boundary;
this bounded trace is not permission to manufacture a wire-slot fallback.

`0x14098d680` is the three-instruction `outer_slot << 7` leaf, confirming
that the archive combines outer-slot128base with the actual signed inner row.


The paired-entry prerequisite is now applied: `ProgramList.programs` owns
`Vec<(i16, Program)>`; its reader retains each original row with the entire
nested object. `multi_programs` extracts only the Program for its existing
outer-container projection and still rejects more than one row. No wire event,
selector, bank-loading policy or saved `Part.program` meaning changes.
The staged `serialized_program_slots_preserve_payload_and_outer_container_selection`
check reads actual serialized Chunk/StructuredObject bytes, verifies ordered
rows7/119/-3 and all public/private/child payloads, reads a nested Bank→SlotList
outer5→ProgramList inner119 back from serialized bytes, and checks the
multi-program rejection. Negative raw row preservation is lossless metadata,
not an advertised valid wire program. This author ran no Cargo or tests;
central verification belongs to the root task.


## Selected versus requested CC66/121/123 delivery

Further protected captures: `program-bank-accepted-control.c`,
`program-bank-controller-system-boundary.asm`,
`program-bank-note-off-selector.c`, and `program-bank-note-off-context.asm`.
The primary [Instrument Options manual](https://docs.native-instruments.com/ni-tech-manuals/kontakt-manual/en/classic-view)
describes the CC120/123 acceptance option per instrument; it supplies no
cross-program bank reset law. The bounded native chain now establishes:

| Receipt | Normal destination | Additional old-program destination | Closed action |
| --- | --- | --- | --- |
| CC66 | Selected program via86c670 | Explicit packed target or matching controller request | Ordinary raw/callback/accepted modulation delivery; automatic native sostenuto/reset fanout unclosed |
| CC121 | Selected program via86c670 | Explicit packed target or matching controller request | Ordinary raw/callback/accepted modulation delivery; native bank-wide Reset All Controllers action unclosed |
| CC123 | Selected program via86c670 | Explicit packed target or matching controller request | Receiving program's generated note_off(ALL) when its saved standard-note option is enabled |

`0x1408783fe..0x140878424` tests BProgram `+0x1fb26` and appends actual
template `0x144f18350`: CC120 fades ALL for10000microseconds then sends Off;
CC123 sends Off without that fade. `0x140956530` resolves its packed-program
argument and traverses only that program's `+0x113a0` queue; ALL is not an
all-bank roster operation. Original events and callbacks remain in their
original program after the selector changes.

The request mechanism is generic, not hardcoded64: interpreter
`0x14079ab1c..0x14079ab26` passes the requested controller and script-context
packed program to `0x140955ac0`; the named error bounds controller0..127.
The helper deduplicates by program identity. Wire dispatcher
`0x1408ed3db..0x1408ed42c` targets every remaining request after normal selected
delivery; enqueue `0x14086c670` removes a matching request before allocation.
Thus an explicitly requesting old program can receive66/121/123 as well as64.
The observed system script creates an automatic64 request; no automatic66 or
121 request is established by the inspected templates. Absence in these spans
is not a proof that no other native layer implements either control.

Raw and accepted state have different owners. `0x14086bf00` records the
physical receipt in engine-wide raw arrays before program callbacks.
`0x140cee5b0` writes the receiving program's accepted values/timestamps, and
`0x140865440` visits only its resolved voice list for modulation. Their complete
inspected bodies have no66/121/123 reset case. A raw projection therefore must
not impersonate accepted controller delivery or perform CC121 in every
inactive Engine. KONTRA's existing mandatory physical121 behavior remains a
separate supported MIDI policy, not a newly proved native bank-wide law.

### Routing integration boundary

The coherent bank ingress transaction must own one physical decoder and one
ordered selector. Decode a wire receipt once, project shared raw input once,
then deliver the same precise ReceivedControl to the selected program plus
actual requesting programs, deduplicating a selected requester. Original
note/Off/expression ownership remains independent of that selector. Consume
request state with actual targeted enqueue chronology, including same-frame
arrival order and empty selected slots.

Existing `Engine::receive_library_control` provides a raw receiver projection
without callback/accepted-control work. It does not update inactive Runtime
public%CC arrays, which currently update through `Runtime::handle` delivered
Controller work. That raw-view boundary must be implemented honestly before
bank advertisement. Existing `control_from_received_scoped_inner` performs
mandatory physical121 cleanup and is not a raw-only broadcast API. Physical
Midi1Decoder/RPN/velocity-prefix state must not become independent per-program
copies that restart or diverge when selection changes.

`cc_delivery_request` currently has no implemented KONTRA builtin. A request
bitset without a real bank consumer would be a dead API; none is added here.
The actual bank implementation must provide the request owner/hook and its
consumer together. Implicit sustain64 delivery must remain reachable for old
release owners. CC66/121 special native bank fanout and exact dynamic host
behavior remain evidence gaps, not license
for unconditional all-program controller fanout. No production code, Cargo or
test execution accompanies this delivery audit.

### Executable script-free core subset (2026-10-05)

`engine::program_bank::ProgramBank::load` now consumes the actual
`read_instrument_bank` importer and its typed parent/container records. Its
`MainStereo` argument is the caller's explicit association of saved host-channel
selections 0/1 with the main stereo sink; it does not claim the unresolved native
audio binder is an output-record index cast. Admission requires the one native
record at index 0, two channels, auxiliary selector -1, unit output gain and no
output FX, plus neutral master gain/tune. The saved positive master tempo is
retained as milli-BPM/1000 fallback; a valid host tempo supersedes it.

The first subset keeps static unit/center inner amplifiers and requires their
known standard volume/pan option disabled. It rejects CC7/10 and paired fine
receipts while the distinct mode2 common-fresh inner amplifier is being closed.
The container amplifier follows the sum and receives the OR of actual inner
`render_interval` work receipts. At this checkpoint the constructor admitted
only a container rule nonbinding against `MAX_VOICES * program_count`.
That implementation boundary was based on an earlier shared-pool inference;
the per-inner native closure below supersedes it and supports ordinary
`Bank.polyphony` reuse for lower caps after the root's verification gate.
Nonneutral routing, learned container bindings, scripts,
MPE/PCK/current-generation input expression and shared-AHDSR future ranges are
explicitly outside admission. Existing plugin MIDI1/2 Program Change diagnostics
remain; this core constructor is not plugin program-switching advertisement.

One decoder owns the physical prefix/paired CC/RPN transaction across selection.
Selected programs accept its decoded receipt; old CC64-request recipients accept
only the immutable canonical pedal projection, without decoding again. Unknown
RPN/NRPN reports `Unsupported` after its generic raw/controller receipt; its
parameter configuration is not applied. CC66/121/123 retain the selected-only
law established above. Exact pool storage and the raw library bank are borrowed
through a private safe RAII loan; original program active lists retain root
provenance. Shared retirement requires local active membership, and reset checks
generation before freeing. The new staged actual-NKM PCM proof independently
schedules two engines with literal Tune=1 and container gains, exercises sparse
holes, old release samples and NOTE_END backpressure, and derives exact ends
249/274/626/731 from source geometry. It has not been executed by this agent.

### Container-shared rule, per-inner pressure and victims (2026-10-05)

Exact native admission `8e4d90` resolves Voice+8d4 as
`outer_slot * 128 + inner_program`. Mode2 selects Container+e760 for the
rule at `8e4e23..8e4e3c`; it compares that rule's maximum with the original
inner Program+1c070 counter at `8e4fbc..8e4fd5`. Start `96a380` and removal
`96a740` independently resolve that same flattened owner before increment
`96a649` and decrement `96ad62`. The ordinary victim call passes its fifth
argument as0; `86ffad..86ffb9` consequently requires the victim Voice+8d4
to equal the incoming Voice+8d4. An inactive sibling program is outside this
ordinary pressure victim pool.

This explicitly supersedes the earlier claims of shared max-note ownership,
container-wide voice-limit arbitration, and a shared total ceiling. The
container shares policy across its retained programs; their sample-voice
counts and ordinary victims keep their original inner-program ownership.
The earlier admission and archive captures remain preserved. The additional
complete captures and minimal implementation handoff are documented in
[`program-container-voice-budget-contract.md`](../artifacts/core-native/program-container-voice-budget-contract.md).

VoiceGroup version0x60's fade field is also closed: reader `d049b0` and
complete writer `d13370` place milliseconds at rule+34, already exposed by
typed `VoiceGroupParams::fade_ms`. Victim `870095..8700b2` converts that field
as signed32 and passes it to the ordinary fade path. Existing `VoiceLimit`
milliseconds-to-seconds conversion therefore supplies the reuse point.
Retain validated kill/preference/exclusion semantics, reject max0 pending
its complete native semantics, reject fade above `i32::MAX`, and describe
the existing prepared `MAX_VOICES` storage bound honestly.

The approved implementation is to construct each inner instrument's
`VoiceLimit` from the container rule, remove the nonbinding roster-product
guard and remove the later `bank.set_polyphony(MAX_VOICES)` overwrite.
It needs no global arbitration, all-program victim scan, new root owner,
or CPU-dependent admission. Production and test execution remain with the
runtime owner and root after the active checkpoint.

The focused functional fixture should load an actual sparse NKM with a unit,
centered container amplifier and a max1/oldest/non-preferred-released rule.
Hold a root in program119, switch to7, and start two distinct exact roots on
program7's same sounding key before either source expires. Pressure must
fade the older program7 sample while the earlier program119 root remains
audible. An independent two-Engine oracle should leave both inner banks
unlimited and explicitly schedule the already-known program7 victim's fade
at the second admission frame, rather than invoking the same cap-selection
code. Compare complete stereo PCM through natural/release termination,
repeat whole and fragmented host buffers, and verify original exact NOTE_END
tuples and source-derived terminal frames, including the inactive program119
root after its original Off. Prepare the known victim identity and output
storage outside the allocation check; an active-voice count alone cannot
prove victim ownership or sound preservation. This is a staged proposal,
not a test result or an implemented fixture.

The first actual ownership run exposed an invalid fixture assumption: RPN2
coarse tuning ignores CC38, so that receipt cannot initialize a newly selected
program's coarse value. The corrected stimulus keeps CC38 as an ignored
distractor, asserts its program remains at neutral, then sends supported CC6=65
under the shared RPN2 selector. The literal Tune=1 reference, PCM tolerance and
274 source endpoint are unchanged; this is not a NOTE_END production repair.
The first PCM scenario proves ownership using the existing unit inner adapter;
mode2 common-fresh startup/retention requires the separately captured native
trajectory before wider amplifier admission.
