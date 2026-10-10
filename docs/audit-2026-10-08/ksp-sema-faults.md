# Embedded KSP semantic fault attribution

W5's RAM-only initializer probe reproduced W3's 45 baseline faults across 30
program-0 multi items. All 45 phase/line/offset triples exactly match
`~/.cache/kontakto-w3/native-caption/residual-ksp-faults.json`.

| Rank | Fixed public reason | Command | Faulted slots | Arguments |
|---|---|---|---:|---:|
| 1 | unknown-command | `subscribe_async` | 30 | 3 |
| 2 | unknown-command | `mf_get_first` | 15 | 1 |
| 3 | No additional kind observed | — | 0 | — |

These are compiler faults in embedded programs. The 45 slots overlap 30 items;
this receipt does not claim 45 broken multis or a new Native-paint regression.
W3's prior receipt deliberately retained only `sema/stage-error` and numeric
locations; command attribution here comes from the actual current error in RAM,
not an inference from those locations. The historical KSP audit independently
reported the same two command counts.

The feature-gated `sampler-kontakt` example `ksp_fault_kinds` accepts an owned
four-column manifest (item hash, path, program index, script slot). It decodes
once per program, evaluates selected slots, and emits only fixed public reason
categories, catalog command names and numeric metadata. Private identifiers,
diagnostic messages, authored source and decoder stderr are not exported. An
optional second argument supplies a frozen public NI command-heading list for
attribution; declaration facts retain only line, numeric initializer and fixed
scalar/constant categories. Its
self-checks cover private-name withholding and nested/quoted argument commas.
The frozen before probe used an empty environment except for slot. Subsequent
probes include translated group names and callback lowering at 48 kHz, while
omitting saved values and performance resources. Neither is a production
initialization or playback verdict.

Numeric receipt: `~/.cache/kontakto-w5/ksp-coverage/fault-kinds-before.json`.
Frozen initial probe SHA256: `9a0e9c44dd1ae0f52945484f745e332f888f2ea9be67ff3cd7ad5536e83021fb`.

No runtime/compiler semantic fix is claimed by this diagnostic change. W11 owns
persistence callback Control-context admission separately. The CPU item stays
parked; Conflux/256's approximately +5 µs p50 disclosure remains open.

The coordinator moved the documented MIDI object family ahead of the unverified
subscription command. `subscribe_async` still has no verified three-argument
contract; it is not admitted as a no-op.

## MIDI object family

The shared plan object now supplies traversal, legacy getters/setters, event
parameters, marks and track selectors, buffer edits, insertion/removal and export
areas. It also imports channel-voice events from SMF format 0/1 files, pairs note
offs into note lengths, and exports the selected area. The API uses physical event
IDs and one mutable object across script slots, init evaluation and native
callbacks. Plan transfers retain that object.

The implementation follows NI's [MIDI object reference](https://docs.native-instruments.com/online-guides/ksp-manual/en/midi-object-commands.html),
[load/save reference](https://docs.native-instruments.com/online-guides/ksp-manual/en/load-save-commands)
and [legacy command inventory](https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/version-history).
There is no v1 implementation of this family to port from `0cb7a8a0`.

Buffers are reserved off audio for plans that use MIDI commands. Runtime edits,
captures, completion dispatch, cancellation and waits perform no audio-owner
allocations or frees. The default object is empty. The edit limit is one million
active plus inactive events, with 512 export areas. Event IDs survive position
edits and track/all-event/marked selections share one selector implementation.

File operations use the existing host effect worker and typed control queue.
Owned payloads return to the producer for disposal; save writes to a temporary
sibling before replacing the destination. Import/resize run synchronously in init
and asynchronously in callbacks. Reset/save requested during init start when the
plan activates. Each completion is identified by plan, physical script instance
and job; the callback keeps its own ID/status through waits. Pending jobs pin the
owning generation, and panic cancels them. MIDI `wait_async` suspends until the job
completes; invalid or completed IDs continue immediately.

Failing-first evidence: the first cursor admission test failed on
`mf_get_first` at offset 29. After the initial cursor implementation, all 15
selected slots instead failed on `mf_get_command` (zero arguments). This was
0/15 fully admitted, not an instrument playback success. The full family was
implemented together before the next 15-slot compilation.

Targeted tests: 15 MIDI tests PASS, covering traversal, init/live shared slots,
getters/edits, selectors/marks, buffer insert/remove, export validation, file note
pairing and empty tracks, malformed files, async identity across waits,
cancellation, init job completion, `wait_async` and bounded init callback nesting.
The instruction size guard still passes at at most 32 bytes. Empty tracks now
survive file export; its failing-first roundtrip returned one track instead of
two. The host control-queue save test and six core plan lifecycle tests passed
with the init/wait additions. Core/KSP/Kontakt (scan enabled) area and root lib no-run checks PASS.
The full KSP suite is **200 PASS, 0 FAIL, 40 ignored**; all 15 MIDI tests pass.
The final host control-queue save/complete test is **1 PASS**. The earlier six
core plan lifecycle tests also passed. No larger scanner/playback gate was run.

Numeric receipts are in `~/.cache/kontakto-w5/ksp-coverage/`:
`fault-kinds-before.json`, `mf-first-after.json`, `mf-family-after.json`,
`fault-next-attribution.json` and the MIDI test logs. The probe includes group names and full callback lowering but
omits saved state and performance resources. Admission is a compile verdict,
not a full Original playback or native Kontakt comparison.

Limits to retain: SMF decoding currently accepts PPQ format 0/1 channel-voice
objects; SMPTE divisions fail explicitly. Meta/SysEx events are not exposed as
MIDI object events. Input is bounded to 256 MiB, and paths use the existing
fixed-capacity text transport. Nested init async completions are limited to eight
with an explicit fault; the recursive fixture first overflowed the debug thread
stack under the attempted 64-level guard and now passes. A failing-first adoption fixture fills the effect queue with 256 old-plan jobs;
early completion of the new unpublished init job underflowed its plan-pin counter.
Completion and capture now reject unpublished jobs before touching that pin. The
ordinary startup fixture was corrected because runtime construction already
publishes its initial jobs. No native
MIDI-file/export-area comparison or CPU measurement is claimed by this semantic
change. Input file PPQ is preserved; Kontakt PPQ normalization has not been
verified. Conflux/256's prior approximately +5 microsecond p50 disclosure remains
open.

The whole-family 15-slot probe is **0/15 admitted**. All 15 move past the MIDI
commands to a different unknown command at line 3051, offset 114475. A separate
one-slot RAM attribution confirms that its name is outside the `mf_*` family
and is not declared as a function in the raw script. It is absent from the frozen
public catalog; its name remains withheld pending a public specification.
Frozen whole-family probe SHA256:
`2599ef0f48bfba7eec3e74988e2a44ea2def10faff2f0d8900b50762d2a871af`.

The final two-slot numeric attribution uses a union of 173 command headings from
NI's current manual and the [archived KSP manual](https://www.native-instruments.com/fileadmin/ni_media/downloads/manuals/kontakt/KSP_Reference_Manual_English_28_01_21.pdf),
in addition to the frozen catalog. The next command still has no verified public
match. It takes two arguments (integer variable, integer array); the first has a
plain raw declaration at line 53. The subscription call still has the observed
shape (integer variable, 0, 0), with no matching plain declaration located by this
probe. That does not establish the argument's declaration origin or a contract.
Final attribution binary SHA256:
`ce3b7c8b928f2b1766179c5cb45647ca7b1eb46cc4d8b97b591eef73f2f6fd48`.
Public heading-list SHA256:
`b2338bd1c0b46aa9f63cb518b7ca95cc151aa150fc1157600afa8013ebd9c144`.
Archived primary PDF SHA256:
`ca141a6cbbbe7cefd9c4b82c3d5f2ea1d4698597d0e80de57346bbed1eaa00b4`.

Parked by coordinator: subscription origin is explicitly **UNKNOWN**; the RAM
probe finds an undeclared NI-prefixed scalar, absent from saved state and the
checked v1/manual symbols. This does not establish an engine contract.

Symptom: 30 subscription faults persist; the 15 MIDI-family slots reach another
undocumented command after the implemented family.
Best hypothesis: an internal subscription interface; origin and semantics remain
UNKNOWN, with no no-op admission.
Next step: obtain a documented contract or native behavior witness before
revisiting either remaining command.

NEXT: Areia scanner settle and UI-effect application before snapshot.
