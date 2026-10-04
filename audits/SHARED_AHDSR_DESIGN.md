# Primary shared AHDSR: accepted source-range design

The private waveform/cache primitives in `engine/ahdsr.rs` are implemented.
Their real accepted-range caller and strict import/plugin admission remain in
integration; the primitives alone do not establish that support. Evidence and the pinned Kontakt8.13.1 executable are recorded in
[MONO_LEGATO.md](MONO_LEGATO.md). No scenario below has been executed.

## Smallest typed boundary

Keep the waveform/clone operations in `engine/ahdsr.rs`. A fresh
`engine/ahdsr_shared.rs` should own only the prepared primary-source rows and
accepted range preparation, using the existing `rtc` host range. It should
not decode MIDI, run KSP, allocate host aliases or infer physical note policy.
The following acceptance signatures describe the caller contract:

```rust
struct PrimarySource { group: u16, slot: u16 }
struct AcceptedStart(u32); // ordinal in a sealed accepted range
enum CloneRef { Active(u64), Starting { start: AcceptedStart, zone: u32 } }
// Active is Player's existing unique voice-admission identity.

struct AcceptedClone {
    clone: CloneRef,
    source: PrimarySource,
    order: u32, // roster order; never a guessed future Player admission ID
    birth: u64, // provenance only; never the release-countdown origin
    entry_offset: i32, // current native voice+170, relative to this range
}
struct AcceptedOff { clone: CloneRef, at: u64 }
struct AcceptedRange<'a> {
    start: u64,
    end: u64,
    clones: &'a [AcceptedClone],
    pending: &'a [AcceptedOff],
}
struct StartReceipt {
    token: AcceptedStart,
    event: EventId,
    admission: u64,
    group: u16,
    zone: u32,
}
```

One accepted note can create several zone voices. Its token binds to an
ordered list of `StartReceipt`s, rather than pretending one note is one
clone. The roster must identify the zone as well as its source when binding
that list. Existing clones retain actual admission identities; prospective
clones use the accepted start/zone pair and ordered roster position until their
receipt provides an actual identity. The projection must not guess future
`EventId` or Player admission-counter values. A range is accepted only if all starts, effective logical releases
and eligible source-clone identities are determined before the first point.
The caller supplies these accepted identities; this API cannot seal an
unaccepted future KSP release. A native range has at most1024 frames and hence
at most32 requested points for an ordinary primary source.

Rows and receipt capacity are prepared with the instrument. The implemented
range owns one worker-prepared full `Owners` projection so exact free-list and
generation decisions use the actual alias allocator. This is additional owner
storage; it is not a second Commands Start/Release snapshot pool or a second
live alias registry. The normalized host stream is traversed without a new MIDI
event-count quota. Each clone needs at most one pending native release offset
in a range, matching `voice+0x509/+0x50c`. The receipt binder verifies the
prepared bank identity, source identity and ordered zone roster. It must not
replace an unmatched token with a different generation or key. The projection
must establish those decisions before commitment; a late mismatch is an
integration error, not a reason to silently substitute an ordinary envelope.

## Source and cache algorithm

1. Remove already retired clones at the range boundary, preserving admission
   order. Ordinary native retirement decrements prototype count and unlinks
   its clone; it does not reset or transfer AHDSR phase. Source rows belong to
   the prepared group/original source slot.
2. Choose the first eligible active clone for that source. A released clone
   remains eligible. A fresh first clone after idle has its own attack reset;
   AHDSR's false virtual `+0x28` prevents prototype initialization-copy or idle
   catch-up. Do not seed that clone from stale prototype phase.
3. Visit pending released clones in admission order. For each matching source,
   compute signed `((off - range.start) - released_clone.entry_offset) / 32`
   with Rust's signed division toward zero, and call the
   selected producer's queued release hook with that count. Use the released
   clone's entry, not the producer's entry. A later call overwrites its earlier
   countdown; neither minimum-offset reduction nor a held-key gate is native.
4. Generate `ceil((range_frames - producer.entry_offset)/32)` raw points once. The waveform
   honors the pending countdown before entering release. Save the count, range
   tag and selective ending state. This source computation can be staged
   without advancing a real voice, alias, `Onset`, cursor or engine clock.
5. Commit the staged candidate only when the established eligible producer
   actually passes its source plan. Later consumers select the suffix
   `max(producer_count - ceil((range_frames - consumer.entry_offset)/32), 0)`. Compute the
   suffix once per clone/range; progressive fragments continue that consumer's
   point/interpolation cursor rather than recomputing a shorter suffix.
6. On a cache hit copy the prototype's selective ending state into the clone.
   Keep each clone's target interpolation history, local pending action and
   done byte. Group/prototype state remains after its producer ends. No source
   waveform advances during a zero-voice range.

The entry offset is mutable native scheduling state, not `off - birth`.
Creation `0x14084f986..0x14084f98d` initializes `+0x170` from the note packet
timestamp shifted right10. A successfully rendered range clears it at
`0x140918028..0x14091802b`. The renderer's delayed/not-ready branch is entered
at `0x1409163b7..0x1409163ca` when the offset is outside this range or sample
readiness bit1000 is absent. At `0x1409183be..0x1409183d7` that branch clears
an offset smaller than the range, otherwise subtracts the range frame count.
Thus an already active clone normally has offset0 in its next range; a future
start counts down through ranges. The projection must retain these explicit
updates, rather than reconstructing every entry from permanent note birth.
Any unsupported early-release/delayed-admission offset case must be identified
before commitment; unsigned subtraction or flooring negative differences is
not an equivalent native release calculation.

Native virtual `+0x98` copies phase `C0`, recurrence `B8`, remaining points
`BC`, countdown `10C`, scale `110`, offset `114` and note-off `119`. Rust's
corresponding fields are `phase/state/left/mul/offset`, plus the two currently
missing countdown/note-off fields. `coefficient` must be restored from the
clone's same prepared phase coefficients; calling `enter` during a state copy
would recalculate the release baseline and lose the copied recurrence.
Prepared counts/coefficients/sustain `100`, forced-completion flag `B3`, done
`B0` and pending action `B1` are not shared-copy fields.

For the initial strict unrouted primary AHDSR, pending initialization does not
reset copied phase: native `ae46d0` returns false when its five routed parameter
pointers are null, and waveform resets only on a true retarget result. Queued
release sets local pending2 and note-off; waveform clears the pending action
when its countdown expires. A release received while already in Release/Done
does not move the phase backwards.

Queued release has priority when its countdown and a finite stage expire at
the same point. `0x140ae42d5` subtracts the countdown and `0x140ae42e1`
branches on that result before `0x140ae4323` tests the stage's remaining
points. The zero-countdown path installs Release at `0x140ae4317` and calls
stage setup at `0x140ae4393`; `0x140ae5e40` therefore captures the prior
phase's post-multiply recurrence baseline. Calling ordinary `Source::point`
and then `Source::release` would first enter Hold/Sustain and lose that
baseline. The existing ordinary path now uses private `advance_point` for
its value/multiply/guarded decrement and immediately advances its phase as
before. A shared generator must resolve countdown expiry before that phase
advance. The initial caller must prove a nonnegative folded countdown; the
native signed negative-count batch is not approximated by clamping it to zero.

Done is clone-local. Reset clears `B0`; the clone's own waveform sets it after
reaching phase5. A consumer that merely copies a finished phase does not become
done on that cache hit. If it becomes producer in a later range, its waveform
can then set local done. The retirement bridge must preserve this distinction
and consumption of the current range. The waveform sets done after generating
its whole requested point array; it does not require the consumer's two
interpolation endpoints to be zero. Even when the last emitted control point
precedes the transition to Done, the waveform can set local done at that call's
end. Current ordinary `ahdsr::Native::done()` waits for both zero endpoints;
that rule must remain on its ordinary path and cannot substitute for the
shared native wrapper's range-end completion. Native completion additionally tests
voice `+0x511` before setting retirement. The common renderer clears that byte
at `0x140916c87`. Processing dispatcher `0x14088b480` walks enabled voice
module slots `+0x870`, split into the two stages by prepared group `+0x908`.
Its post stage propagates prior module activity through virtual `+0xa0`, then
stores each module's virtual `+0xa8` result into `+0x511` at
`0x14088da12`. An already latched envelope-completion byte `+0x512` supplies
inactive input to the first such module; a module can still report active
output. Thus `+0x511` is processing-chain activity, not shared-source phase.
With every post slot absent or bypassed, the checked walk leaves the cleared
byte zero and the source done gate can mark retirement. Class-specific module
tail behavior remains outside this source-only design; an initial Engine
bridge can preflight no active post-amplitude voice modules. It must not use
`phase == Done` to collapse their lifetimes or clear live module state early.

The primary volume consumer is separately identified. TargetID2 in
`0x1409519e0` selects work buffer `+0x2a10`; common renderer clamps its
published points to nonnegative values, then `0x140917049` selects endpoint
state `voice+0x3b0` and `0x14091709b` calls scalar interpolation `0x1408e4110`.
The old per-voice cursor `+0x500` is saved at `0x140916985..0x140916993` and
updated modulo32 at `0x140916998..0x14091699f`. Voice creation calls reset
`0x14094d6a0` at `0x14084f90b`; reset zeroes both float endpoints with a qword
store at `0x14094d6f1`, clears initialized byte `+0x3b8` at `0x14094d6f8` and
cursor `+0x500` at `0x14094d79a`. `0x1408e411c..0x1408e412b` seeds current
from point0; with a fresh zero cursor its first interpolation boundary then
copies current to previous and reads point0 again. Fresh shared consumers
therefore prime both endpoints from their selected point0. Subsequent ranges
retain the separate endpoints/cursor; copying AHDSR phase cannot re-prime them.

## One projection shared with physical acceptance

An authored/direct caller can supply a sealed accepted range immediately.
Plugin support requires one pure input-acceptance reducer used by both live
ingress and a prepared projection, rather than a mirrored MIDI policy.

The reducer's state is the existing exact owner generations/free order,
anonymous `InputKey` rows and physical sustain/sostenuto state. Its outputs
are accepted start/root bindings and effective source Offs. Source release
occurs after pedal policy, independently of physical key-up and release-sample
snapshot storage. CC64-up, CC66-up and CC121 can fan out effective Offs;
CC123..127 use the same physical note selection and pedal rules. Existing
native sustain flags, configured input/home routing and fixed MPE manager
scope remain inputs to that reducer.

Exact `Owners::allocate` rejects a duplicate concrete tuple for its entire
live lifetime; the plugin reclaims these rows through `finish_host_notes`
only after its host-range render. A prepared copy can therefore forecast its
within-range free-list/generation decisions exactly. Future starts use tokens
until their actual live receipt. They never create premature live owners,
keys, `NOTE_END`, `Onset` or voice-census/TOTAL entries. Anonymous offs must
remain separate from exact wildcard offs and match all selected held physical
events, including layered voices, by the shared reducer.

The first plugin slice must be preflighted before audio: no Runtime; fixed
input routing/MPE membership; alignment holding disabled; static deterministic
zone selection and resident source readiness; no voice-limit pressure; no
unforecastable source edits, detach/reset or alias-census decisions. Per-note
input admission must itself be projected faithfully, or its available/current
capacity must prove that no in-range census can change admission. Supporting
that slice requires the resolver, not just copying raw NoteOff timestamps.
These restrictions describe a prospective capability, not current admission.
An unsealed or unsupported range must return a specific preparation error
before advancing a source or emitting PCM. It must not drop a group or fall
back silently. Import admission stays closed until a real caller can fulfill
that contract.

A sealed source Off already owns its queued control-point countdown. Its later
live receipt must still release the voice's held, loop and filter metadata at
the actual ingress time, but must not also call immediate `Source::release`.
That source-only release branch requires the sealed clone receipt; ordinary
`Native::release` keeps its existing immediate behavior. Forecasting a source
Off must never release physical or live voice metadata ahead of the callback.

Broader scripted delivery needs an acceptance boundary redesign: currently
`Host::note_off` creates `Note::Release` only after release forwarding, ignore
and wait decisions; Player then resolves pedal delivery at command application.
Future rows are absent until their callback runs. The source reducer cannot
predict arbitrary callback branches. Moving callbacks early, shadow-running
KSP, rewriting its live getters, or adding audio latency is outside this
proposal. Engine's progressive render phases can expose accepted receipts
without changing that chronology, but they alone cannot seal future callbacks.

## Edits and staged proof

- `engine/ahdsr.rs`: selective state, queued point-countdown action and explicit
  clone-local waveform completion; retain the current ordinary path.
- Fresh `engine/ahdsr_shared.rs`: prepared primary rows, sealed accepted-range
  validation, raw point cache and token/receipt binding. No MIDI/KSP policy.
- `engine/instrument.rs` and import/group metadata: sparse group/original-slot
  ownership, only after coordinator approves strict admission.
- `Player::release_voices`, `pedal_up`, render producer selection and the
  physical acceptance reducer: coordinated spans only; no independent edits
  to the active Rack, loop, envelope or CC integration lanes.
- Plugin integration is a separate consumer of the same acceptance reducer;
  direct batch support must not be reported as complete plugin support.

Staged independent PCM should include older attack plus later start17/Off48
in a128-frame range; two pending releases with the later admission overwriting
the first countdown; CC64 deferral; CC66 captured older root with an uncaptured
later root; same group on different physical channels; distinct sibling groups;
producer retirement with a surviving copied clone; and restart after idle.
The scalar oracle schedules source release at the independently calculated
point boundary and keeps independent consumer interpolation endpoints. Fixed
and fragmented rendering, exact lifecycle receipts and zero allocations are
required observations. These are future checks, not executed evidence.

## Source primitive checkpoint

`ahdsr::Shared::stage` stages at most32 raw points without changing the live
clone. A `Candidate` keeps selective `Ending` separate from its same-producer
source and local completion byte. Producer commit restores its own staged
source; follower copy restores only the native selective fields and derives
the phase coefficient from its own static prepared geometry. Neither operation
changes interpolation history. Generation-tagged bindings validate cache
coverage before mutation and preserve the point cursor across fragments.
`render_cached`/`skip_cached` report an exclusive range-end terminal only when
that clone's own waveform has latched completion and all its range frames have
been consumed. An unsealed cache consumer is a contract error.

The shared source preserves native prepared Sustain/Done counts separately
from the ordinary Source's zero-as-indefinite representation:
`0x140ae467f` stores INT_MAX at D0 and `0x140ae4689` stores it at D8. Both
remaining-stage points and the queued countdown decrement during generation.
The queue-expiry branch uses raw Release setup, preserving Release with a zero
prepared count if expiry occurs at the final requested point. A later waveform
request advances that zero-length Release; the first call does not fabricate
local completion. Zero-request generation still tests its final source phase,
as native `0x140ae4131` jumps directly to the completion test.

Static source comparison verifies that the ordinary setup scalar block,
value/multiply/decrement recurrence, interpolation expression and statement
order, ordinary completion/consume and existing staged tests remain unchanged.
`git diff --check` passed. This checkpoint did not run Cargo, tests, scenarios
or native waveform emulation; executable validation belongs to the root's
coherent integration gate.

The `Amplitude::Shared` consumer now delegates phase, level, local completion
and cached render/skip to that source. Generic cache-free render/skip are
contract guards. Real delivered release is source-only no-op after the caller
checks the bound current-range receipt, preserving ordinary and Native
immediate release behavior. `release_is_sealed` exposes that current-range
binding check; it does not prove MIDI acceptance by itself. The caller owns
the accepted-note/pedal transaction and same-offset On receipt binding. Static
comparison also verifies every Ordinary/Native Amplitude arm and all source
outside that enum/implementation are unchanged. Strict capability and actual
Voice/provider ingress integration remain a coordinated caller requirement.


## Live caller integration checkpoint

The current production caller is the plugin's lossless normalized input stream.
`prepare_shared_sources` projects each part's actual reached input order for
one existing balanced native RTC span (at most1024 frames). It validates all
parts' candidates before `commit_shared_source` binds any live source. Host
ranges exceeding1024 frames re-seal at the existing balanced boundaries;
progressive render fragments retain one generation and one cache cursor.
No future physical numeric controller state, compound decoder state, library
receiver, callback, key or live voice is advanced by this traversal.

`AcceptedStart` is the normalized input ordinal. Actual admission must match
that ordinal, zone, source range timestamp, physical input/key, exact HostRef
generation and original velocity. Prospective binding occurs before usage,
voice-pressure handling or prepared route-row acquisition. The source owner
retains existing interpolation endpoints; cached predictive skip uses a copied
bound source and cannot nominate or commit a producer. A live effective Off
suppresses only immediate envelope release after the provider verifies the
actual clone admission and exact sealed Off timestamp. `SourceNote`/`Pedals`
still update live held, source/loop/filter and physical metadata at actual
chronological ingress.

Strict admission currently requires the original saved sole-primary v11 AHDSR
record, ordinary unlooped resident Sampler with native quality0, static source
parameters, identity Router routing, no MPE, no KSP, no alignment hold and no
voice pressure. Any release-trigger sibling is rejected because the current
attack roster cannot forecast those extra starts. Ordinary pedal-held release
lookups remain legal when the installed bank contains no release-trigger group:
their eventual current-bank zone lookup cannot create an additional voice.
All ordinary attack siblings are included in a conservative total voice count.
Whole-range sample-EOF/readiness uncertainty, source edits, unsupported input
management/parameters/channel modes and end-offset starts are rejected before
that range emits PCM. Legal received scalar controls remain chronological.
Router's real On resets the delivered key expression immediately before note
publication (`articulate.rs` input_impl); the future source bound therefore
starts from that same default expression, then overlays existing exact/per-note
inputs and authored Host tune. Public/direct Engine starts cannot manufacture
a sealed receipt, and standalone Engine render cannot deliver future receipts;
both return the existing checked unsupported-window failure for these banks.

The plugin propagates preparation rejection as `ProcessStatus::Error`, clears
remaining output and records a private static source reason through its bounded
diagnostics channel. Central compile-only gate 37908 passed. The root then ran
`accepted_shared_primary_plugin_pcm_keeps_overwritten_off_pedals_restart_and_range_ends_without_heap`:
the corrected implementation passed in 0.79s, comparing complete plugin PCM to
independent literal per-source gain samples with pedals, overwritten Offs,
restarts, mute, balanced 513/512 ranges, exact host end tuples and zero callback
allocations. The actual unsupported-admission proof passed in 0.20s. The first
musical run exposed an overbroad rejection of ordinary pedal-held release
lookups; the fix retains the rejection of actual release-trigger siblings.
These checks exercise the bounded core caller, not a Kontakt audio comparison.
Arbitrary future KSP decisions, delayed/streamed/looped source
ownership and general shared-source admission remain open. The source-free,
ordinary Native and previously admitted retriggered paths keep their existing
owners. Import diagnostics identify the admitted shared source only after the
complete original source roster proves its sole-primary marker; unsupported
wrapper/target/roster diagnostics remain unchanged.

## Direct static MPE acceptance (executed bounded proof)

The direct caller now projects an immutable MPE topology as well as physical
note/Off receipts. `Router::source_topology` keeps identity key/channel routing
and no articulation mapping. An already installed Router configuration reads
Engine's authoritative negotiated topology; pending first setup uses the same
canonical `MpeZones::configure` and Router setup tuple as actual `feed_on_port`.
It does not configure the real Engine, advance a controller, mutate a physical
owner or run a callback. Pending setup that changes topology while any live
voice or exact Host owner remains is refused. Membership renegotiation, dynamic KSP decisions, alignment hold and unsupported
management inputs remain diagnosed. The subsequent RPN0 sensitivity extension
below has its own bounded admission and staged proof.

`MidiState::mpe_role_changes` is the single changed-role mask used by actual
`Engine::configure_mpe` and copied acceptance storage. A pending setup receipt
is consumed at the first reached input in that source range. Actual ingress
decodes the first MIDI1 On/Off/zero-Off velocity protocol before setup, whereas
MIDI1 CC decoding follows setup. The projection preserves exactly that order:
first MIDI1 On can consume an old prefix once, then changed-role copied pedals
and prefixes reset; first CC88 is received once after reset. A first HostOn
consumes no old prefix and resets it before subsequent MIDI1 input. Neither
copied reset nor a future pedal release changes the live receiver early.

Manager CC64/CC66/CC121 fanout uses the same `Player::control_channels_for`
policy as real controller delivery. Existing `Pedals`, `SourceNote`, exact
root capture and accepted effective-Off timestamps perform the projection;
there is no separate manager-mask rule. Member pressure, CC74 and normal bend
remain chronological consumer input. Member reuse still executes the real
Router `FreezeExpression` at its On, preserving released consumer expression
history independently of the selectively copied AHDSR source history.

Sample readiness conservatively bounds the complete sealed span by the full
negotiated member plus manager sensitivities, existing physical overrides,
source/static/group/part tuning and current per-note tuning. A prospective
zone entry can clear an ordinary MIDI1 override: the bound takes the maximum
of that old override and the new zone range, so old12 to default48 is safe;
a retained native96 override is bounded96. Controller reset clears bend but
preserves configured sensitivity/tuning. This is a source-coverage bound, not
a second pitch renderer or permission to forecast future live controls.
Insufficient unlooped source coverage rejects before publication/audio, using
the existing checked failure. Source wave generation, selective clone copy,
interpolation endpoints, producer/Off ordering and local completion arithmetic
are unchanged.

The independent actual plugin proof is staged separately in
`plugin/shared_primary_mpe_tests.rs`. It uses original saved sole-primary
records, exterior literal AHDSR gain samples, ordinary source-free per-root
PCM references, member and manager bend, pressure/CC74, same-key overlap/reuse,
manager sustain/sostenuto/reset, exact owner endings and callback allocation
checks. First-setup capsules distinguish HostOn then MIDI1 input, MIDI1 On
before setup reset and CC88 after setup reset, including preexisting pedals
and prefixes with no voice. This is bounded direct static-MPE acceptance,
not arbitrary KSP or all MPE renegotiation/native audio parity. This author
ran no Cargo/build/tests; root owns coherent validation.


Root's actual plugin checks passed: the static-MPE complete PCM trajectory in
0.19s and all three first-setup capsules in 0.17s, followed by green final
builds. The first oracle incorrectly routed handwritten per-sample source gains
through the separate 5ms HostExpression gain smoother. The corrected oracle
renders otherwise-unity ordinary PCM with independently authored pitch, then
multiplies the literal source gain externally. Tolerances, waveform references,
precise onset receipts, exact endings and allocation assertions were retained.
This remains a bounded core caller proof, not native Kontakt host audio parity.

## Unchanged-role MPE RPN0 sensitivity (staged)

The shared-primary accepted-range owner now retains copied canonical
`Midi1Decoder` records rather than a prefix-only approximation. Raw MIDI1 CC
uses the existing decoder's `cc`; legacy synthetic `In::Cc` uses its `parameter`
entry point; native MIDI2 control never enters the MIDI1 compound decoder. First
note velocity decoding still precedes prospective Router setup, while first CC
decoding follows setup. Changed-role setup resets copied records at that exact
receipt; CC121 uses the existing decoder's controller-reset defaults and the
same physical manager/member fanout as live input.

Only registered RPN0 sensitivity on an existing MPE manager/member is admitted.
`MpeZones::parameter` supplies the exact coarse/fine/relative range law. Its
ranges-only updates have a zero changed-role mask, so actual Engine
reconciliation does not stop voices or reset their source ownership. Relative
baseline seeding delegates through one helper shared with live MidiState; no
second compound parameter decoder or future live state is introduced. Null
selection and inert NRPN remain inert. Registered tuning, ordinary-channel
sensitivity, native parameter messages and RPN6 membership changes remain
explicitly unsupported in this accepted source path. Membership needs its own
forced-stop and copied decoder/pedal receipt contract.

Admission records the maximum combined member/manager sensitivity encountered
for every physical input across the complete chronological projection,
including initial values and retained physical override precedence. Immutable
source geometry is retained per existing/starting clone. All EOF/readiness
checks run against these extrema before staging any candidate waveform or
committing a producer. An earlier large sensitivity followed by a smaller one
cannot pass through a final-value-only check; no global pitch cap is imposed.
The check remains conservative across the accepted span. Live numeric bend,
sensitivity, receiver history and FreezeExpression still change only at actual
ingress timestamps. A released member snapshot retains its captured member
semitone contribution, while manager bend/range remains a live consumer input.

The independent plugin proof is staged by the existing isolated MPE fixture
owner using literal pitch trajectories and exterior AHDSR scalar samples;
source waveform, copied-source arithmetic and interpolation are unchanged.
No build or test was run by this author for this extension; root owns validation.
