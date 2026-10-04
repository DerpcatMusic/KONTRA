# Mono, legato and release Note Mono ownership

Read-only native evidence, 2026-10-04. The binary is the pinned Kontakt8.13.1
executable identified in [CORE_NATIVE_CONTROLLER_EVIDENCE.md](../docs/CORE_NATIVE_CONTROLLER_EVIDENCE.md).
No native executable was activated and no tracing configuration changed.

## Distinct product controls

The [Kontakt classic-view manual](https://docs.native-instruments.com/ni-tech-manuals/kontakt-manual/en/classic-view)
describes source Release Trigger Monophonic as cutting prior sounding release
samples on repeated notes. Its voice-group capacity counts sample voices and
provides victim selection; this does not describe held-note fallback or voice
reuse. Tone/Time Machine source Legato carries the previous playback cursor to
the next sample. These are distinct from the
[polyphonic Glide modulation source](https://docs.native-instruments.com/ni-tech-manuals/kontakt-manual/en/modulation),
and from library-authored KSP note priority or envelope retrigger policy.

The publisher's [Falcon2026 manual](https://s3.amazonaws.com/uvi/UVIFC/falcon_manual.pdf)
has the recorded SHA-256 `23c2d8ce9ae3abd5bf3a3a65d5cb3f8744f395315e45fbc300c412d330cec117`.
The Layer editor, PDF p32, assigns last-triggered-note behavior to Mono modes
and separate constant/proportional-time portamento choices. Its Script Processor
Legato, p288, instead crossfades attacks with an independent Retrigger switch;
Microtuning, p285, exposes lowest/highest Key Priority. These establish useful
comparative requirements, not Kontakt serialized field identities.

## Bounded native bindings

- Literal `releaseTriggerNoteMonophonic` starts at `0x144f19188`. Two named
  group readers compare that literal at `0x14091bd26` and `0x1409204d0`,
  compare its value with `yes`, and store group byte `+0x674` at
  `0x14091bd82` and `0x14092051c`. The adjacent Release Trigger switch
  is independently stored at `+0x673`; release counter is `+0x678`.
- Correct source XML reader entry `0x14092db60` maps `tone_machine`,
  `time_machine`, `streaming`, `slice_machine`, `time_machine2`,
  `sp1200_machine`, `mpc60_machine` to source mode `+8` values1,3,4,5,6,7,8.
  Its `legato` literal `0x144f1de34` is referenced at `0x14092dde7`;
  the `yes` comparison stores source byte `+0x8c` at `0x14092de2e`.
  This is source state, not program or group held-key priority.
- The same reader compares preceding `retrigger` at `0x144f1de28` and,
  for versions below0x90, copies that saved compatibility switch to `+0x8c`
  at `0x14092e306`. Do not infer a generic modern envelope switch from it.
- Source UI update entry `0x1405fab70` supplies the `Legato` label
  `0x14472bb6c` at `0x1405fb270`. This label independently belongs to
  source controls; it does not establish last/high/low note selection.

Bounded strings/bindings found no admitted program/group keyboard-priority or
single-trigger byte. This is a statement about the inspected paths, not a proof
that no such native policy exists. Source Legato's archive location, runtime
cursor eligibility and reset/reuse remain unresolved. Tone/Time DSP remains
unsupported; substituting a sampler cursor copy would invent audible behavior.

## Current Rust owners and remaining boundary

`import::Group.release_trigger_note_monophonic` retains the saved group flag.
`Bank.note_mono_releases` enables scanning only when a release group requests it.
`Player::start` cuts prior sounding release voices matching delivered note and
channel plus captured physical input channel. Deferred pedal releases retain
their separate identities. A polyphonic sibling release group is unaffected.
The cutter runs before zone matching; current group masks do not suppress cuts
of already sounding tails. Physical/delivered scope, group-mask timing and the
existing1ms fade are KONTRA receiver policy, not native-equivalence claims from
the above field-binding evidence.

Physical exact roots and unscripted `InputKey` records own key lifetime and pedal
capture. KSP input queues retain exact generations by physical channel/key;
`%KEY_DOWN` is an aggregate physical projection and `$NOTE_HELD` queries the
current event. Generated notes and waited callbacks retain their parent owner.
These owners support authored note-priority scripts, but neither aggregate
projection establishes a native group-level mono policy. Voice capacity1 and
kill-high/kill-low remain sample-voice stealing, without held-key fallback.

The staged `release_mono_tests` complete-PCM gates compare saved group flags
with explicit owner-selected reference fades. Independent constant-source and
linear-fade bounds cover every frame, distinct physical/delivered channels,
different pitches, same-key exact roots, sustain deferral, group masks,
polyphonic sibling tails, manual generated release durations, shared/scalar
rendering, irregular/empty fragments, exact terminal clocks and callback heap
work. They establish this receiver policy when executed; they do not measure
Kontakt PCM. No Cargo command or scenario was executed by this evidence author.

Program/group last/high/low priority, held-note fallback, voice reuse and envelope
single-trigger remain open until their design, saved identity and native trigger
rules are established. Polyphonic Glide has its own evidence and owner.

## Actual factory mono policy and saved controls

The installed Kontakt factory preset is
`Presets/Scripts/Performance/Unisono - Portamento.nkp`, beneath the pinned
Kontakt8 installation directory. Its SHA-256 is
`a28ddcb10444feba9136e73b41dcbfa8e87f9ec6574729b557cfaeb2b0cb4f05`.
The18,186-byte legacy `Kon3` container has a zlib stream at file offset`0xaa`;
inflation produces90,303 bytes of `K2_Preset` XML. The `Script` element contains
the original source in `V[name=sourceText]`'s **value attribute**, alongside
persistent controls. Read-only extraction is
`/tmp/kontra-unisono-script-source.ksp`; it was not executed or wholesale
vendored into Rust fixtures.

The publisher's [Factory Library Scripts manual](https://www.native-instruments.com/fileadmin/ni_media/downloads/manuals/kontakt/Kontakt_Factory_Library_Scripts_Manual_English_21062021.pdf),
printed p27/PDF p26, distinguishes Mono On, Legato and Offset. On creates new
samples, Legato retains the sounding sample, and Offset creates a sample at an
offset derived from elapsed held duration, restricted to Sampler mode. Key-Up
Trigger and Note Priority separately control held-note fallback. Auto Portamento
and relative glide are additional choices; they do not identify group-level
Glide records.

The extracted original has the following concrete trigger rules:

- Lines417–422 register Next0, Last1, Low2 and High3. Lines2155–2168 map
  saved `$mono_mode_knob`0/1/2/3 to Off/On/Legato/Offset controls.
- Lines780–789 retain incoming event IDs, velocities and note ordering in
  `%pressed_id`, `%pressed_velo`, `%last_id` and `%hold_arr`. This is script
  state, not a physical MIDI receiver's channel mode or a new group flag.
- Lines1000–1028 apply low/high priority to new attacks as well as release
  fallback. Unsuitable attacks are ignored while their held-key state remains.
- Lines1034–1045 ignore a new Legato event and call relative `change_tune` on
  the existing `$mono_id`. The original sample cursor and generated event's
  exact/physical provenance remain owned by that event; no root handoff appears
  in this path.
- Lines1132–1137 create replacement samples. Offset is
  `($ENGINE_UPTIME-$mono_start_time)*1000` microseconds: wall-clock held time,
  not the pitched sample cursor. Start time is assigned only when entering
  the phrase at line930.
- Lines1486–1489 clear a pressed key only when its event ID matches, protecting
  a later same-key generation. Lines1562–1599 select nearest, last, lowest or
  highest remaining key. Nearest scans upward with strict improvement, so an
  equal-distance tie retains the lower note. Last compares stored event IDs.
  Lines1634–1647 retune Legato fallback; lines1665–1700 generate On/Offset
  fallback with separately configured velocity policy.

Saved preset values include Mono Off0, Key-Up Trigger1, priority0, Portamento0,
relative glide0, unison voices1, amplitude-release0 and both group menus0.
Source initialization sets priority1 before persistence is restored: do not
substitute that initializer for the saved priority0. The controls are ordinary
persisted KSP variables, not evidence for a native program/group mono byte.

`mono_legato_tests` stages an independently authored reduction covering the
three active modes and all four priorities. Every output frame is compared with
an exterior, explicitly listed selection/pitch/start-offset schedule using
`Engine::start_event`, `change_event` and `fade_event`, without a reference
held-key scan. The changing source makes cursor restarts and retunes audible.
The fixture includes transposed delivered notes, two physical channels sharing
one script scope, an independent delivered-channel sibling, a home-channel
change, original-owner versus ignored-root expression, exact terminal roots,
shared/scalar rendering, empty/irregular fragments and callback heap guards.
An additional full-PCM case releases the first physical key at frame600 while
its generated Legato event survives until frame1248. Later expression on that
released exact root still reaches its sample; a newer ignored root's expression
does not acquire it. The independent cursor sum at frame1200 is1440 source
frames, covering the preserved cursor through both retunes and key-up fallback.
The fixture deliberately chooses an explicit1ms replacement/terminal fade;
native preset defaults, pedal/unison/humanization/group choices and other fade
branches are outside this reduction. It does not prove complete factory-preset
support or Kontakt PCM equivalence. No scenario or Cargo command was executed.

## Factory dependency and shared-envelope limitations

A read-only inventory of the original script's42 call/syntax identifiers finds
the ordinary registered builtins, old underscore aliases and compiler syntax.
That is not a successful compilation or DSP-support result. In particular,
original line1066 calls `_will_never_terminate` in its repeated-note Organ
keyclick branch. The independently traced command/source policy is now
implemented only for an active raw sole-primary ordinary AHDSR-to-volume
source in forward, unlooped Sampler geometry. It skips Sustain at the Decay
boundary; it does not make event metadata immortal. Extra/shared/bypassed/Flex,
source-free, looped and immediate release-group rosters remain explicit
unsupported admissions. The [event-completion audit](KSP_EVENT_COMPLETION.md)
records the exact lexical, source and natural-retirement evidence and the
staged complete-PCM checks. This does not establish the original preset's
keyclick source geometry or full execution; the reduced factory fixture still
excludes that branch. Broader Runtime queue/status alias lifetime work remains
independent of this source-policy repair.

Further bounded shared-AHDSR tracing confirms group source buffer`+0x50`,
shared-source flag`+0xa0` and a template pointer`+0x18`. The common voice-source
installer at`0x14097be30` invokes reset`+0x78` on the copied source at
`0x14097c46c`. Its optional template-copy path is gated by virtual`+0x28`,
which AHDSR's vtable maps to the false-returning`0x14050f330`. This rules out
assuming that this installer copies AHDSR phase from a held voice. The common
render path at`0x140916800`–`0x1409168cc` instead writes/reuses a shared source
buffer per source-clock tag and copies state between template and voice through
virtual`+0x98`. The bounded follow-up below closes ordinary producer retirement
and restart after an idle render generation. These source-owner facts alone do
not admit shared Retrigger0 or generic keyboard priority.

## Shared AHDSR source ownership and advancement (bounded follow-up)

This follow-up uses the same pinned Kontakt8.13.1 executable. The
[Kontakt modulation manual](https://docs.native-instruments.com/ni-tech-manuals/kontakt-manual/en/modulation)
describes the disabled Retrigger switch in terms of source continuity and
restart after the last released note. That wording does not independently
require holding the release stage until every key is up. The following closes
the cache owner, ordinary render scheduling and logical release/retirement;
it does not yet admit the saved switch. These are modulation-source facts,
independent of factory keyboard
priority, sample capacity and physical MIDI receiver modes.

**Owner identity.** Native voice creation `0x14084f380` resolves the zone from
program `+0x11340`, then indexes program `+0x11370` with zone `+0xa20` to select
the prepared group. The voice stores packed program identity at `+0x8d4`, that
group index at `+0x8dc`, the prepared group pointer at `+0x8e8` and the zone
pointer at `+0x8f0`. Thus `+0x8dc` is a group index, not a zone index or channel.
Group source preparation `0x140973970` owns prototypes at
`group+0xd68+original_slot*16`. The voice's packed source list at `+0x590`
retains the original slot through count/map `+0x810/+0x811`; source `+0x18`
points back to its prototype. This establishes prepared program/group/source
ownership, without inventing a per-channel envelope owner.

**Selective state and cache.** AHDSR base constructor `0x1407f6b30` sets
source flag `+0xa4` bit1 at `0x1407f6b5a` and bit7 at `0x1407f6b61`. The
identified target-bearing source uses vtable `0x144729108`. Disabled saved
Retrigger `record+0xab8` and bit7 select shared flag `+0xa0` and buffer `+0x50`.
The buffer has space for1024 float control points, count at byte `+0x1000` and
clock tag at byte `+0x1004`; allocation is `0x1008` bytes. Common source
installation `0x14097be30` copies a prototype into the voice and resets its
AHDSR clone at `0x14097c46c`. AHDSR virtual `+0x28` returns false, so the
optional initialization copy used by the identified shared Multi LFO classes
does not apply to AHDSR. The prototype `+0x58` count increments for a newly
copied voice and decrements on ordinary retirement, as traced below; it counts
active clones rather than physical held keys.

AHDSR virtual state copy `+0x98`, concrete `0x1405e4fc0..0x1405e5015`, copies
phase `+0xc0`, recurrence scalar `+0xb8`, remaining count `+0xbc`, release
countdown `+0x10c`, scale/offset `+0x110/+0x114` and note-off status `+0x119`.
It does not copy prepared sustain `+0x100`, pending action `+0xb1` or done byte
`+0xb0`. Setup `0x140ae45f0` initializes `+0x100` to1.075; retarget
`0x140ae46d0` reads it minus the0.075 base and updates it from the sustain
route `+0x140`. Phase-entry `0x140ae5e40` uses that value as the sustain
baseline, and steady-source virtual `+0x68 = 0x1405e9610` returns it minus
the base. This field is not the separate target interpolation history.
Copying an entire Rust envelope state between voices would therefore
claim more than this native hook proves.

**Ordinary scheduling.** Main dispatch `0x140906d0a..0x140906e7b` walks the
global active voice list, head `engine+0x11ed0`, through next `voice+0x920`.
The separately established tail admission means this is admission order; it
is not a held-key stack. An eligible voice with a shared source whose buffer
tag differs from `engine+0x1067c` is rendered serially through the call at
`0x140906dfd` to `0x1409462c0`, before worker queueing. That routine calls the
common voice renderer `0x140916350` and marks `voice+0x520` at
`0x140946b57`. Released active voices are not excluded by a held-note test in
this prepass. The first eligible source consumer, rather than the newest key,
supplies the missing cache.

The common renderer requests `ceil((frames - *(voice+0x170))/32)` control points
and saves padding at source `+0x5c`. On a missing shared tag it calls the
clone waveform at `0x14091682e`, copies the clone into its prototype at
`0x14091683b`, then stores count/tag. A later consumer of the same tag selects
the tail at `max(cached_count-requested_count,0)` and copies prototype state
into its clone at `0x140916875`. Per-voice target output/history remains
separate. With no active voices the checked main prepass jumps from
`0x140906d16` to `0x140906e6b`; AHDSR catch-up virtual `+0x48` is the empty
`0x1404f99a0`. These checked paths do not advance a source during an ordinary
zero-voice gap. They do not prove every reset or auxiliary dispatch path.

**Logical release.** Note-off dispatch `0x1408f4160` walks the
program's voice list `+0x11390/next+0x1a0`, matches native event ID and optional
group, and queues `voice+0x509/+0x50c` at `0x1408f444a..0x1408f4457`.
Main dispatch consumes the pending offset and calls `0x14086aea0` at
`0x140906926`. Its shared envelope branch at `0x14086b129..0x14086b18b`
finds the earliest global active clone with the same packed program, group
index and source prototype, then calls that clone's release virtual `+0x80`.
Concrete AHDSR release `0x1406030a0` sets note-off `+0x119`, pending action
`+0xb1=2` and, outside AHD Only, release countdown `+0x10c`. The checked
redirect and concrete release contain no last-held count gate. Release stage
selection is therefore separate from the documented restart eligibility; this
is not evidence for adding a group-held release barrier.

The preceding dispatcher `0x14094fe20` identifies this path as kind1 and calls
`0x1408f4160` at `0x140950d75`. Its prelude `0x1408e32a0` queues child releases
by parent event ID before that call. Physical input matcher `0x14086ca40`
compares the received note with the original note at event `+0x50` when set,
otherwise the current note; it also matches physical/channel addressing,
unforwarded callback slot and absence of an existing pending release. It
allocates a release packet in engine pool `+0x14070` for each matching root and
saves it at event `+0x4e0`. These checked allocation and delivered-release
paths do not inspect a shared-envelope held-key count. This does not replace
the independently documented native sustain/sostenuto and callback policies.

**Ordinary producer retirement and idle restart.** The concrete engine vtable
`0x144e98c60` has retirement entry `+0x70 = 0x14096a740`. Main dispatch
`0x140906c70..0x140906c8f` calls it for active voices marked `+0x8fc` before
the shared-source prepass. Group unlink `0x140987b80` removes the voice from
the distinct group list `+0x58/next+0x1b0` and decrements group count `+0x60`.
When that count reaches zero its16-slot buffer-clear loop explicitly skips
shared-capable bit7 with saved Retrigger0. Retirement then walks the retiring
voice's sources and decrements prototype `+0x58`, clamped at zero, at
`0x14096ab20..0x14096ab53`. It unlinks the global pool through
`0x14086d2a0`, then frees the voice's source block `+0x8b0`. The checked
retirement path calls neither AHDSR reset `+0x78` nor source copy `+0x98`:
a surviving eligible clone becomes the next producer by the normal prepass.
The prototype state and shared buffer remain owned by the prepared group.

Main dispatch advances engine frame counter `+0x27cd0` at `0x140908869` and
each worker source tag `+0x1067c+worker*8` at `0x1409088a0`, by the processed
frame count even when no voices were active. A first new clone after such an
idle generation increments prototype count from zero to one and receives its
own reset at `0x14097c46c`. AHDSR's false `+0x28` suppresses both the optional
idle catch-up and prototype-to-clone initialization copy. With the old cache
tag stale, that clone's fresh attack supplies the first new waveform, then
publishes its selective state into the prototype. Overlapping active or
released voices instead continue the shared cache through ordinary selective
copy. This reset eligibility follows active clone/cache lifetime; it does not
require inventing a physical-channel history or held-key priority.

**Clone completion is not copied phase completion.** AHDSR reset
`0x140ae583b` writes word `+0xb0=0x0100`: local done byte `+0xb0` is zero
and pending byte `+0xb1` is one. The concrete done getter `0x1405edd60`
returns that byte. Waveform completion `0x140ae43a8..0x140ae43bb` sets it
when the clone's phase reaches five or higher; selective state copy
`0x1405e4fc0` does not copy it. Completion scan
`0x140946ad0..0x140946b4a` checks volume target2, the source done virtual
`+0x88` and envelope flags, then sets voice `+0x512` and, when applicable,
retirement marker `+0x8fc`. Consequently a later cache consumer can copy a
finished phase without becoming locally done on that same cache hit. If it
later supplies a missing generation, its waveform invocation sets its own
done byte before that completion scan. Deriving all clone retirement from a
single shared `phase == Done` would collapse this native distinction.

The volume consumer's history is separately located: targetID2 in
`0x1409519e0` selects work `+0x2a10`, whose published points the common
renderer clamps to nonnegative values. At `0x140917049` it selects endpoints
`voice+0x3b0`, passed to scalar interpolation `0x1408e4110` at
`0x14091709b`. Old voice cursor `+0x500` is saved at
`0x140916985..0x140916993` and updated modulo32 at
`0x140916998..0x14091699f`. Voice creation `0x14084f90b` calls reset
`0x14094d6a0`, which zeroes both endpoints at `0x14094d6f1`, initialized byte
`+0x3b8` at `0x14094d6f8`, and cursor at `0x14094d79a`. The scalar consumer
seeds current from point0, then a zero-cursor boundary sets previous/current
from that same point. This closes fresh shared-consumer priming, while later
ranges retain their own endpoints and cursor. The source sustain field
`+0x100` is distinct from these endpoints.

Completion's `voice+0x511` gate is processing-chain activity. The common
renderer clears it at `0x140916c87`. Dispatcher `0x14088b480` walks enabled
voice module slots `+0x870`, split by prepared group `+0x908`; in its post
stage it propagates activity through virtual `+0xa0`, then stores the module's
virtual `+0xa8` result at `0x14088da12`. Latched source completion `+0x512`
passes inactive input to the first module, which may still report active
output. With all post modules absent or bypassed, the checked walk leaves
`+0x511` zero and the done-source check marks retirement. Class-specific
module tail behavior is not proved by this dispatcher and cannot be replaced
by a shared `phase == Done` test.

**Queued release offset and host generation boundary.** The shared release
path computes its point countdown at `0x14086b0f5..0x14086b116` from the
released voice's queued `+0x50c` minus that same voice's current render-start
`+0x170`, using signed division by32 rounded toward zero. After locating the
earliest matching source clone, it passes this countdown unchanged at
`0x14086b186..0x14086b18b`; it does not rebase against the selected producer's
render start. The common renderer clears the consumed voice start offset at
`0x140918028..0x14091802b`, so this is a current-generation offset rather than
a permanent note-on timestamp. Creation `0x14084f986..0x14084f98d` initializes
it from the note packet timestamp shifted right10. The renderer branches at
`0x1409163b7..0x1409163ca` for an offset outside the current range or missing
sample readiness bit1000. Its corresponding
`0x1409183be..0x1409183d7` tail clears an offset smaller than the range;
otherwise it subtracts the range frame count. A forecast must carry that
scheduling field across ranges, not blindly subtract absolute note birth.
Concrete AHDSR waveform `0x140ae40f0` limits each phase
segment by the pending countdown and enters release when it expires. This
is an offset action prepared before the waveform call, not an immediate
per-sample mutation at physical packet arrival.

Main pending-release pass `0x140906905..0x14090693c` visits the global list
in admission order before the shared prepass. Concrete release
`0x1406030a0` does not test whether note-off `+0x119` is already set: every
call writes pending action2 and, for ordinary AHDSR, the supplied countdown.
Multiple due same-source clones can therefore overwrite the selected
producer's countdown before its waveform invocation. The checked code does
not implement an earliest-offset reduction or a first-release-wins policy.

The checked direct callers of main renderer `0x140906000` are the wrapper
`0x140d38620..0x140d38fbc`. It reads the host frame count at argument2
`+0x1c`, ingresses incoming events through `0x140941b50` with sample offsets
encoded by a ten-bit left shift, then calls scheduling `0x140952580`, event
dispatch `0x14094fe20` and main rendering in that order at
`0x140d38b62/0x140d38b71/0x140d38b88` for host ranges up to1024 frames.
Larger ranges use balanced chunks no larger than1024 at
`0x140d38d01/0x140d38d0f/0x140d38d26`. Event dispatch considers the release
pool `engine+0x14078` across the whole current range before waveform
production. There is no mandatory32-frame outer split in this checked path.

For example, a128-frame native generation may contain an older producer
starting at0, a later same-source clone starting at17 and that later clone's
delivered release at48. The checked release arithmetic gives
`(48 - 17) / 32 == 0`, delivered to the older producer before its waveform
call. This arithmetic example identifies the required scheduling contract;
it is not a recorded native PCM result or a claim of a Kontakt defect.
Progressively emitting an audio prefix and only then recomputing the shared
source when the later release is delivered cannot in general reproduce a
generation whose release action was already prepared before its first
waveform. Accepted callback release decisions, delayed child releases and
pedal delivery must be included in that preparation; raw physical packet
look-ahead alone is insufficient.

The source-level design boundary is now precise: any implementation must own
the shared prototype/cache per prepared group and original source slot, retain
per-voice selective state/output, and supply each source-clock range once in
native consumer order. A physical channel cache, generic last-note priority or
a shared envelope advanced once per voice is outside that evidence. Rust's
current `ahdsr::Native` also owns per-voice interpolation endpoints/offset and
uses immediate per-note release, while the native shared cache renders a source
clock range with an offset pending release. A faithful bridge must preserve
that range's release scheduling and each clone's local pending/done fields.
Ordinary shared AHDSR admission additionally needs a proved generation-level
delivered-event preparation boundary (or an equivalent source-only forecast
that preserves those accepted decisions). A lazy correction after emitting
the prefix does not close that contract. The LFO's release-free future
waveform cache is not by itself that bridge.
Progressive render subdivisions, a new attack during a release tail, source
bypass/parameter edits within a cached range and group replacement remain
required admission boundaries. No Rust production behavior,
record admission, Cargo command or audio scenario was changed or executed in
this follow-up.

### Explicit authored KONTRA MonoRetrigger (production checkpoint)

The core now exposes `Engine::set_note_mode(delivered_channel, NoteMode)` and
`note_mode`. `Poly` is the default; `MonoRetrigger` is an explicitly local
last-accepted-held-root policy. It has no CC126/127, imported Kontakt flag,
persistence, factory-script, or Falcon mapping. Configuration requires an
idle script-free delivered channel, complete resident physical sources, and
no shared AHDSR source. Shared Digital sources retain their existing group
waveform ownership; shared pitch admission additionally requires a current
host range declared before input. Bank replacement, reset and
script installation restore Poly; compatible resident fills retain the mode.
Configured Mono rejects rootless starts.

Accepted candidates stay in the existing anonymous InputKey and exact Owner
rows. A receipt stores EventId, checked monotonically increasing acceptance
ordinal, and the mask of groups that actually published a first-sample-ready
source. The mask is group-level, not a promise of an immutable zone roster.
Ignored/unmapped/failed starts cannot become fallback candidates. Physical
birth, canonical velocity, per-note binding and exact HostRef generation stay
in their original owners. An incoming anonymous Mono candidate remains local
until actual source admission succeeds. Rejected same-key input retains the
previous held accepted row, including detached per-note generation, while raw
key velocity, timestamp and receiver attributes remain chronological. This
retention uses the existing row; it adds no alternate registry. Explicit Off
must still address the original accepted delivered channel if a caller changes
its routing before a failed same-key retry; no cross-home reroute law is added.
A separate per-delivered-channel selected receipt
protects the sounding EventId after key-up under pedals or anonymous row
reuse; it is a selector, not a second held-key registry. Inline GroupMask costs
512 bytes per prepared root, allocated with the existing off-thread row pools.

A fallback starts fresh source/envelope age using that accepted receipt.
Physical root birth remains original; native RTC and group admission remain
chronological at the new source start. Held exact owners still pin NOTE_END
after natural source EOF. EventId wrap skips the bounded retained local roots,
voices, pending releases and selectors; this does not claim protection for
arbitrary KSP-only queued identities without physical roots or audio.

`Voice::prepare_source` extracts the existing plan prefix through source
admission without the subsequent onset/pitch, gain, filter-follow or bypassed
waveform clock commits. Ordinary render invokes that prefix then commits once.
Mono uses the same prefix on a temporary resident candidate for one output
sample before usage, pressure/victim selection or publication. This is a
first-sample readiness check, not a guarantee against future expression edits
or a later larger trajectory failing normal render-time validation. Failed
candidates return their prepared route row. On the first successful candidate,
the copied group clock and native RTC/release cutter commit once before
make_room/publication, preserving ordinary prelude order. Only then does
replacement fade the selected source; already-dying tails are not restarted.

CC123..127 and MPE role cleanup defer selection refresh until the complete
canonical release/cleanup transaction finishes. Scoped physical sound-off
refreshes only changed root selection; an unchanged root/ordinal does not
retrigger. Pedal hold and effective source Off continue to use the existing
canonical SourceNote/Pedals release owner.

Five registered `engine::player::note_mode_tests` scenarios are staged:
complete stereo PCM against a separately scheduled Poly rig for fallback,
channel isolation, expressions, ignored/clock-rejected candidates and ordinal
exhaustion; exact-root natural EOF and allocator wrap with fresh fallback and
NOTE_END; pedal-held key reuse and scoped/MPE cleanup without intermediate
attacks; native release-cutter ordering before group pressure with rejected
admission retaining the prior audio. Central harness85841 compiled the earlier
checkpoint and observed passing EOF/wrap and pedal/MPE scenarios. Central
harness97083 compiled the newer same-key retention checkpoint; EOF/wrap,
pedal/MPE and native pressure/cutter scenarios passed. The fallback scenario
matched its independently scheduled Poly rig bit for bit in both whole and
split unshared rendering, then its cross-partition comparison differed by at
most two bit steps / 3.7252903e-9. This is established existing sum ordering:
B64 released at96 ends at289; fragment-end swap_remove moves A196 from slot6
to B's slot2 at295 in split output, at384 in whole output. Seven same-sign
contributions therefore add in a different order. Only the cross-partition
oracle now uses the two-sum rounding bound 2*gamma6/(1-gamma6)*channel_peak,
where gamma6=6*u/(1-6*u) and u=f32::EPSILON/2. Both same-partition musical
oracles remain bit-exact, including rejected same-key retries and source end289.
Central gate68232 compiled this correction and all four scenarios passed,
including both shared and unshared lanes for the main musical oracle.

The fifth scenario stages real prepared-zone revision rejection, parent-rebased
resident fill, later held-note fallback, and resident-to-streamed replacement
against complete independently scheduled Poly PCM and exact ends. A rejected
upgrade retains the local note policy and roots. A compatible resident fill
also retains them; only the actual replacement path clears them. The setter's
resident source predicate is reused by the upgrade compatibility boundary, so
an incoming streamed bank cannot retain an unsupported configured Mono policy.
Central gate27103 compiled the fifth scenario and reached rejection, resident
fill and fallback; its later streamed handoff was correctly rejected because
the fixture still named the pre-fill parent. The fixture now prepares that
handoff against the fill's actual State and asserts parent readiness before
replacement. Central gate27853 subsequently observed all five scenarios pass.
This agent has not executed these scenarios.

The sixth isolated child, `note_mode_shared_volume_tests`, compares local Mono
with independently scheduled Poly starts and fades through rejected same-key
input, held-note fallback, channel isolation, live synchronized frequency and
volume depth, mute/bypass, complete idle and a new exact root. Its counterfactual
uses per-note waveform restart and must audibly differ. Both render schedules
declare the same 256-frame host ranges, then use fixed or irregular processing
fragments inside them. This distinction matters: shared waveform generation
uses each host range's padded control points and native suffix selection;
different host ranges need not sample identical points at a voice boundary.
The initial child incorrectly varied host ranges alongside fragments and first
differed at frame33. Declaring the same host ranges corrected that fixture
without changing production arithmetic or its PCM bound. Root's current
central harness observed the corrected child pass in 0.03s; each partition's
Mono-versus-manual-Poly complete stereo PCM has zero bit differences. Exact
root endpoints321/1089 and the zero-allocation assertion passed.

The subsequent shared-pitch checkpoint is implemented but awaits central
compilation and PCM validation. Its first-sample resident source probe requires
cached group waveform points to validate the source trajectory, whereas
volume-only sources cannot change that trajectory. `Counter::span_at` projects
only current declared balanced geometry, without advancing RTC cells. Missing
or expired ranges reject the candidate before its admission prelude or the
previous source's fade. `Shared::stage_preview` uses the same point and padded
phase recurrence as rendering, changes only copied candidate metadata, and
never nominates a producer or commits group phase. A private preview marker
forces rendering to regenerate an uncommitted preview, even at the same base,
so a subsequent ordered frequency/bypass edit still affects the actual first
generation. Already committed generation caches remain immutable.

The seventh isolated child adds paired shared Digital pitch/volume targets to
the same independent musical Poly schedule. It stages a synchronized frequency
edit after successful first-sample admission but before first PCM, then a
mapped huge-finite-tune candidate that fails at the next host range followed
by another same-time source edit. Each partition retains strict complete PCM,
exact endpoints and zero allocation checks. These are local authored policy
checks, not evidence for native keyboard mono, legato, or voice reuse.
