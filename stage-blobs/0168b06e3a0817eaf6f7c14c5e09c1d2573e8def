# `will_never_terminate`: bounded native event/source contract

Read-only evidence, 2026-10-04, installed Kontakt 8.13.1 PE64. This does not
claim execution of the factory instrument or an implemented command.

The [primary KSP manual](https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/advanced-concepts)
describes this command as telling the script engine that an event will never
finish, and identifies the release-trigger system script as its consumer.
The installed embedded system script calls `_will_never_terminate` immediately
after `play_note(...,0,0)` and before setting system event parameter 3. The
installed Unisono-Portamento script also calls it on its generated repeated-key
Organ click event (see `MONO_LEGATO.md`). Neither use makes duration zero alone
equivalent to this command.

## Exact command identity

Static decoding of the native lexer tables closes both spellings. Transition
table `0x1477608b0`, signed-16 acceptance table `0x1478d36b0`, and lexer
`0x1419f25b0` give `_will_never_terminate` state 2879/action 349 and the spelling
without an underscore state 2872/action 350. Both actions reach
`0x1419f364d`, which returns token `0x216`. Static LALR-table decoding of
`on note\n_will_never_terminate(1)\nend on` accepts and selects rule 574 at
`0x1419f6498`; that reduction loads command token `0x27` at `0x1419f64ac`.
No native code or instrument was executed. Adjacent reset-release-counter
token `0x215` selects rule 573 and command token `0x5f`, providing a separate
cross-check against the already closed release-counter command.

`0x1407a9100` calls AST builder `0x140793660`: command token `0x27` becomes
AST opcode `0x6c`. Statement dispatcher `0x140798900` selects
`0x14079aab9`, evaluates its integer argument, and calls `0x1409550d0`.
That helper selects exact event ID, marked events, or all events from the
packed program's audio-event list at `program+0x113a0`, checks matching packed
program ownership at `event+0x18`, and ORs `event+0x3c` with `0x20` at
`0x14095515a`. It does not allocate or directly modify a script-slot pin.

## Voice-creation consequences

The complete common initializer `0x14097be30` initializes two local flags to
zero, then visits installed sources. Under event flag `0x20`,
the branch `0x14097c3ea` records that a source was installed. Envelope-class
source flag `source+0xa4` bit 1 qualifies the source's `+0xb3` marker, set after
source reset at `0x14097c474`. AHDSR/Flex constructors set this class bit;
the LFO constructor does not. Target records are `0x88` bytes; target kind 2
is independently registered as volume at `0x140a6d476`–`0x140a6d485`.

An envelope source with a volume target takes the branch to `0x14097c445`
without setting the fallback-loop local. A source that does not meet that
condition sets the fallback local. The final predicate at `0x14097c945`
sets voice loop index `+0x160` to 8 if no source was installed, or if any
source set the fallback local. Thus loop disabling is conditional: a roster
consisting entirely of eligible envelope-to-volume sources retains its sample
loop; a source-free event instead runs to sample EOF. The loop index is an
index into eight `0x38`-byte loop records, with 8 as the sentinel, independently
confirmed by `0x1408ed120` and `0x140960c00`. It is not an event-lifetime enum.

AHDSR waveform code `0x140ae4340` increments the completed stage index.
`0x140ae4356` tests `source+0xb3`; `0x140ae4361` substitutes stage 4 for
would-enter stage 3 before stage initializer `0x140ae5e40`. Stage 2
initializer uses the stored sustain (+0x100 minus BASE) as its offset/scale;
stage 3 uses the sustain-plus-BASE table entry as its initial level; stage 4
at `0x140ae5ec6` captures the current output as its release scale. This is
direct Decay to Release without entering or publishing Sustain, preserving
the current decay endpoint rather than resetting it to authored sustain.
AHD mode (+0x118) follows its distinct stage 0/1/2 to 5 branch.

AHDSR vtable `0x144729108` reset virtual +0x78 points to `0x140ae5820`.
The reset clears +0xb3 at `0x140ae5844`, walks zero-count initial stages
through the table at +0xc4, and initializes the selected stage before the
common initializer sets +0xb3. Therefore an initially selected Sustain
requires a separate startup policy; setting this marker cannot be modeled
as a retroactive release. A first finite subset requires at least one
nonzero Attack/Hold/Decay source count at actual voice construction. This
is source state at voice creation, separate from the group's authored
OneShot/retrigger options.
Flex marker consumption, bypassed/shared source qualification, and late calls
after source creation remain outside the closed subset.

## Runtime and exact-owner boundary

The existing Rust `Runtime::Events` owns generated event records and release
timers. `event_status` currently includes engine voice activity; Sample event
records may be recycled under pool pressure. Exact host owners remain pinned
by sounding voices, queued Start/Release payloads, delayed Runtime work,
callbacks, and held physical roots. `Engine::mark_host_notes` and the existing
voice terminal path retire an owner only after those references disappear.
Kontakt's flag setter above does not establish an immortal metadata/pin policy.
The native queue-membership query at `0x1409554b0` checks event lists without a
special flag-`0x20` status branch. Exact CLAP NOTE_END is the local host contract,
not a native Kontakt CLAP claim.

The implemented bounded source policy is a per-event marker copied into
the existing NoteSpec and queued Start snapshot. It reuses the actual raw
sole-primary source marker and existing prepared primary-amplitude capability,
initially admitting only an active ordinary native AHDSR-to-volume source,
forward Sampler mode, no sample loops, and no additional effective
internal/external/filter/wavetable sources. Admission uses the same initialized
native Source state as construction to require a finite Attack/Hold/Decay stage;
it does not duplicate time-count arithmetic. The transition marker is set after
native reset/construction and before its first published point.

Source-free and looped rosters are deferred: decoded empty vectors alone do not
prove an empty original raw roster, and the native loop fallback predicate must
remain conditional. Unsupported marked starts reject admission with an explicit
diagnostic/count. A generic admission failure message must also cover queue
exhaustion or no admitted voice, without falsely labeling those as unsupported
geometry. A late setter does not retroactively mutate an immutable queued Start
or an already-created source. Existing release/cancel/timer/pin/event-status and
host-retirement owners remain unchanged. The staged PCM checks have not been executed; native CLAP parity is not claimed.


## Natural retirement and implemented admission

The independent bounded lifecycle trace closes the eligible volume source's
finished virtual at `0x140946ae8` to `voice+0x184=3` at `0x140946b3a`, retirement
`+0x8fc=1` at `0x140946b4a`, and renderer `0x140906c86` engine virtual `+0x70`.
Constructor `0x1407f3fa0` installs vtable `0x144e98c60`; this virtual is
`0x14096a740`. It unlinks the voice through `0x140755d40` at `0x14096a941`,
decrements event voice count `+0x4d4` at `0x14096aa05`, then for the duration-zero
bit-1 event permits program unlink `0x140987bf0` and free `0x14086d1b0` when
voice count and continuation count `+0x34` are nonpositive and `+0x38` is zero.
No flag-`0x20` guard appears here or in timed/release cleanup
`0x140950103`/`0x140950cd6`. Wait rows increment `+0x34` at `0x14079eaef` and
`0x1407a01ea`; resumption decrements it at `0x1407980d6`. The exact `+0x38` and
bit-2 qualifications remain named by field, not speculative semantics.

The query is independently identified as `event_status`: DFA state 1924
accepts action 81, lexer `0x1419f2ce3` returns token `0x150`, accepted parser
rule 769 at `0x1419f71a9` builds builtin `0x65`. Builder `0x140797030` either
queries `0x1409554b0` directly for a constant or emits AST `0x1b`; evaluator
`0x14078bf1c` calls the same query. This closes lexical identity without
inferring its four nonzero list results from audible voice activity.

Rust `Event.force_completion` starts false for each newborn event, and the
builtin uses existing exact/marked/ALL target selection. `NoteSpec` copies it
into the private owned Start payload. `Host::play_note` rejects unsupported
actual attack rosters before queueing; `Player::start_with_completion` repeats
that check against live settings before any source or sibling mutation.
Marked duration-zero/system-release-bypassed starts also reject any matching
immediate release roster, whose marker propagation is unproved. A rejected
marked attack returns before the unmarked release fallback. Unmarked release-
only admissions preserve their existing path. Direct notes and newly generated
private release starts keep the marker false.

`Native::supports_completion` shares constructor phase selection. The marker
is installed after `Native::new`, and both normal and zero-duration traversal
skip Sustain at the Decay boundary. No late call rewrites already queued
payloads or already constructed sources. The staged `engine/wnt_tests.rs`
checks complete stereo Engine PCM against literal independent native source
points, finite exact-owner completion, waited onset, mute, empty and irregular
fragments, physical release/pedal handling, late-call nonmutation and explicit
zero-A/H/D, AHD, extra-source, loop, bypass and immediate-release rejection.
No Cargo or tests were executed by this owner.

## Ordinary event aliases, terminal Off, and pressure recycling

The follow-up bounded trace closes ordinary logical note ownership separately
from source voice ownership. Ordinary Note On dispatcher `0x1408f46c0` sets
event flag `+0x3c & 2` before final zone admission. Cleanup
`0x140950103`/`0x140950cd6` tests that flag in addition to source count `+0x4d4`,
continuation count `+0x34`, and field `+0x38`. An ordinary held event with a
finite attack at EOF, or with no admitted attack, therefore need not leave the
instrument note queue merely because its voice roster is empty. This is
separate from duration-zero bit-1 source retirement documented above.

The public `note_off` command is independently named: lexer DFA state1116,
action185, lexer target `0x1419f30f3`, token `0x1ba`, rule328 target
`0x1419f4ca8`, builder selector `0x58`, AST `0x5b`, command target
`0x14079bd79`, and selector helper `0x140956530`. That helper queues an Off in
`event+0x4e0` rather than deleting the source event's identity at the script
call. Off dispatch `0x140950d61` enters `0x1408e32a0` and `0x1408f4160`; the
associated original event is loaded from `Off+0x20`, field `+0x38` is cleared
at `0x1408f417f`, and the original event's logical bit2 is cleared at
`0x1408f428a`. Sounding voices and suspended continuations still retain the
original queue event after this terminal forwarding. WNT bit20 adds no
immortal metadata qualification. The complete bounded disassembly and lexical
reductions are retained in
`/tmp/kontra-native-wnt-lifecycle-closure-20261004.txt` for the pinned PE/SHA
identified above; no native executable was run.

The [NI event commands manual](https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/event-commands)
identifies public status1 as instrument note-queue membership and status2 as
multiscript MIDI-queue membership, with zero for inactive events. An ignored
instrument event does not become a multiscript MIDI event. Internal native
query results3/4 remain unclassified. Native getter lookup and the exact
meaning of `+0x38` outside the traced Off dispatch remain qualified.

The implemented Runtime repair keeps the admitted real `Engine::EventId` on
the bounded Event row through terminal forwarding and records that forwarding
with an explicit `released_to_engine` bit. `Event::active(engine)` distinguishes
logical held/pending admission, waiting callbacks, and the retained actual or
unapplied queued audio alias. `event_status`, shared exact/marked/ALL targets,
and `get_event_ids` use this same membership predicate. Terminal forwarding
and shared targets reject a stale completed Sample alias, avoiding a newly
created release child from a late timer/Off. The existing pool-pressure pass
only reclaims a nonheld, callback-free, untimed terminal/Sample row whose
Engine alias is actually inactive; it cannot recycle a still-audible source.
Canceled/choked rows follow the existing silencing/alias-clear path. There is
no new per-render Event-pool sweep. `Host::voice_active` recognizes only actual
voices and unapplied Start/Release commands with the exact Engine ID.

The staged `engine/event_lifetime_tests.rs` compares complete stereo PCM with
direct Engine admissions and automation, independent of Runtime status and
alias lookup. It covers a release-callback wait followed by audible tail gain
and pan changes, ordinary finite attack EOF while held, EOF-only continuation
ownership, active event enumeration, actual finite Sample retirement, more
than4096 admissions preserving a long sounding Sample alias, then pending
Starts/retired row generation reuse and status-controlled replacement audio.
Fixed and irregular fragments include empty renders and callback allocation
checks. The LogEngine has no source EOF and still reports admitted aliases
active; its former claim of5001 completed Sample recycles is replaced with
honest bounded exhaustion. No Cargo or tests were executed by this owner.

The independently copied `Player::PendingRelease` is future private release
source state, not a Runtime pointer or an additional voice with the source
ID. Private release admission allocates a new Engine ID at `trigger_release`;
its PCM and exact host ownership can outlive the original Runtime event.
Native source-event queue membership while a no-attack private pedal release
is deferred has not been closed, so this repair does not infer a new Runtime
pin from that copied record. Direct metadata getters on retained terminal
rows and `$PLAYED_VOICES_*` (currently Event row count) remain separate open
compatibility gaps; active metadata count is not claimed to equal native voice
count or audible voice count. CLAP NOTE_END uses the existing exact host owner
references and timestamps rather than Runtime metadata pool occupancy.

The staged tail oracle also includes a separate512-frame private release
sample admitted at frame176, ending688. Parent gain/pan edits at256 address
only the retained original Engine alias; the release child keeps its own ID
and parameters. Late terminal Off after the parent EOF must not admit another
child. The source-root host end remains the latest genuine source endpoint1024.

The callback fixture exposed and repairs a separate scheduler timestamp error:
interleaved `Runtime::process_at` runs after preceding DSP has advanced
`Player::now`, while callback offsets still refer to the outer render origin.
`Host::frame_base` is initialized from the current clock for external ingress;
both render Runtime entry borrows use the stable outer `render_start`.
`host_callback_finished` records `frame_base+at` rather than adding the offset
twice to an already advanced clock. The waited EOF-only callback's independent
end remains480 for every fragment schedule. No additional timing trait or
persistent scheduler state is introduced.


### Direct event introspection and actual instrument voice count

The pinned PE/SHA above closes invalid direct getter ownership independently
of public status. `get_event_par` lexer state2118/action172/token0x23d reduces
rule627 (`0x1419f66ac`) through `7aaf90` to command AST0x2f;
`get_event_par_arr` state2660/action173/token0x23e reduces rule628
(`0x1419f66c4`) through `7ab0a0` to AST0x31. The command switch at
`0x140798900` routes those to `0x14079f192`/`0x14079f1cf`, which call
`0x14098f120`. The nonnegative instrument-program branch resolves the program,
walks its `+0x113a0` note queue by `event+0x4f0`, and compares exact `event+0x10`
IDs before calling the parameter accessor `0x1409918b0`. The helper initializes
its result to zero and returns that zero on a missing/unlinked event; it does
not consult an inactive allocation row. A live event with no zone can still
return zone-1 through the successful accessor, separately from a missing ID0.

`get_event_mark` state2284/action160/token0x1ab reduces rule334 with builder
selector0x2c to command AST0x56. Its `0x140798b00` command branch calls
`0x14087df20`, which walks the same program queue and returns false on a missing
exact ID, otherwise testing `event+0x260`. `set_event_mark` rule333/AST0x55
and `delete_event_mark` rule335/AST0x57 enter `0x140798ac6`/`0x140798b54` and
call `0x14095a030`; the exact-ID path checks actual program voice aliases in
`+0x11390` and then note-queue entries in `+0x113a0`, modifying marks only for
matched owners. Bounded lexical, builder, command and helper artifacts are
`/tmp/kontra-native-introspection-{lexical,builders,command,helper,mark-helper,mark-mutations,mark-mutation-helper}-20261004.txt`.
No native executable was run. The integer-expression and command switches
reuse opcode numbers; command0x56 is the mark getter, while integer-expression
0x56 is an unrelated UI getter and is not used as evidence here.

The implementation applies the existing `Event::active(engine)` membership to
scalar/array direct getters, mark reads and exact mark mutations. Retained
terminal metadata now returns zero/false without reviving a generation;
logical held roots, waiting callbacks and actual release tails remain valid.
This follows the same bounded ownership predicate used for status and targets.
The no-attack pedal-deferred native queue boundary remains qualified above.

The [NI built-in variables manual](https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/built-in-variables-and-constants)
defines `$PLAYED_VOICES_INST` for the current instrument and
`$PLAYED_VOICES_TOTAL` across all instruments. A borrowed `KspEngine` hook now
returns the actual Player sample-voice count, including release tails and
private release children; pending Starts, held metadata after finite source
EOF and callback-only owners contribute no sample voice. Logging/setup adapters
have no sample DSP and return zero. TOTAL is a separate supported name with
zero plus a bounded deduplicated unavailable diagnostic: there is currently
no authoritative multi-instrument count supplied to this Host. The
implementation does not substitute metadata count or the current instrument's
count for that missing owner. An actual Rack-wide TOTAL count remains open.

The existing complete stereo lifecycle PCM oracle now makes its post-Off
parent gain/pan edits conditional on successful direct scalar/custom/group/mark
reads. It checks retained callback getters before and after callback completion,
dead getter and mark results after source retirement, and actual instrument
counts0 before queued Starts,2 at256 (parent plus private child),3 at512
(new attack plus parent/private child),1 at1100, and0 at1450 despite a held
finite-source root. The real-source pool-pressure branch only starts the
independent replacement sample when its retired generation has zero direct
getters and there are no admitted sample voices; pending Starts remain
independent queue owners. The reference PCM and literal exact host ends remain
unchanged. A separate mock assertion documents the unsupported TOTAL diagnostic
and its deduplication rather than pretending the log owns sample voices.
No Cargo or tests were executed by this owner.
