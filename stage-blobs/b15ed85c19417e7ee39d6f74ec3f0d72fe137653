# UVI script host adversarial audit

Date: 2026-10-08. Branch: `v2/gpt-uvi-audit`, based on `699758ab`
(`origin/integrate/core-v2`). This is an audit, not an implementation: only this
report and authored tests were changed. The UVI implementation owner should
fix and promote the relevant ignored tests.

## Evidence and counting

Primary specifications: [UVI Script API](https://lua.uvi.net/),
[Falcon manual](https://uvi.s3.us-east-1.amazonaws.com/UVIFC/falcon_2026_manual.pdf)
(Components / Event Processors; Interface / Events; Learning Falcon 303 / Using
the Script Processor), and the read-only RE document
`/home/derpcat/.t3/worktrees/KONTAKTO/t3code-80fe786b/docs/DSP_FORMAT_SPECIFICATION.md:209`
through its “UVI XML and resource syntax” section, especially line 267.
No decompiled implementation or vendor Lua was copied into the deliverable.
Installed programs and scripts were read in memory; logs contain only metadata,
command summaries, and errors, never decrypted program/script/sample content.

The read-only `~/.cache/kontakto-corpus/full3.jsonl` contains **660 distinct
completed UVI programs**, **655** with `sound.note == "started"`, and **307**
with `sound.perf.peak_voices == 0` (all 307 started). Of the 660, 644 report at
least one bound script. The zero-voice partition is **275 Augmented Orchestra,
16 VWinds Clarinets, 9 VWinds Flutes, and 7 VWinds Double Reeds**. None of these
307 became audible in the corpus controller retry. This is not 307 independently
proven API bugs: a probe outside the script's playable range, a callback error,
and a generated note selecting no zone can all produce this record.

Corpus SHA-256: `862d5fbd22a4961d4d7d3719830b2d4f20f254f370f8bd4b3a3d288b67c79618`.
Catalog SHA-256: `d33c77d57a9582f955e6cadf0f5f01e2e7e87de6651fd2add2838eb7ebdab422`.
The cached corpus does not identify its source commit; fresh script probes below
run on this audit's base. Do not claim a native Falcon A/B render from these data.

**Count convention:** an *observed* count is an actual corpus/probe result; an
*exposed* count means the program loads code mentioning the API or reaches the
shared host surface. Exposure is not proof that every program hits that defect
on its default note. A “0 demonstrated” count means the authored conformance
repro proves the API defect but no installed-program trigger was measured.
Counts overlap and must not be added.

## Reproduction

`crates/sampler-uvi/tests/uvi_audit.rs` contains small original scripts, with
known failures ignored individually. The baseline for `run`, milliseconds,
`waitBeat` and virtual timestamps remains a normal passing test. Execute failures
explicitly:

```sh
/home/derpcat/.cache/kontakto-heavy cargo test -p sampler-uvi --test uvi_audit -- --include-ignored --test-threads=1
UVI_AUDIT_CATALOG=/home/derpcat/.t3/worktrees/KONTAKTO/t3code-80fe786b/artifacts/engine-analysis-2026-10-07/catalog.json \
  /home/derpcat/.cache/kontakto-heavy cargo test -p sampler-uvi --test uvi_audit_catalog -- --ignored --nocapture
UVI_AUDIT_CORPUS=/home/derpcat/.cache/kontakto-corpus/full3.jsonl \
  /home/derpcat/.cache/kontakto-heavy cargo test -p sampler-uvi --test uvi_audit_corpus -- --ignored --nocapture --test-threads=1
```

Use `UVI_AUDIT_ONLY` to limit the corpus probe to a program or bank substring.
The corpus probe does not decode or render samples. It records initialization
errors, note-generated commands, controller-retry commands, custom-state key
counts, and symbols in modules actually loaded through `require`.

## Top causes of the 307 zero-voice records

Pending fresh metadata probe results; the cached record alone cannot establish
causality. The ranked findings below include separate reporting and test-harness
faults so a missing diagnostic is not mistaken for successful Lua execution.

## Ranked findings

### 1. Runtime Lua errors disappear from corpus and live reports

[API callbacks](https://lua.uvi.net/group___event_callbacks.html) execute the
script's event-processing decisions. The product's load/report contract requires
runtime failures to remain visible. `script.rs:1246` retains errors privately in
`ScriptHost::findings()`, but `scripted/thread.rs:90` snapshots findings only at
load and never publishes later ones. `tools/corpus-health/src/main.rs:1069`
collects faults only for `Rig::Midi`; line 1097 also excludes the UVI rig from
selection diagnosis. **307 observed zero-voice records all have empty script
faults and null selection diagnosis**; 644 bound-program reports are exposed.
Corpus repro: `Presets/00 Orchestra/01 Strings/V Strings Harmonic.uvip` has
`note="started"`, zero voices, empty faults, and null selection. The fresh
metadata test supplies the missing distinction. Fix the diagnostic path before
using empty error lists to exonerate the scripts.

### 2. Physical and generated note IDs collide and overwrite ownership

[Voice lifecycle](https://lua.uvi.net/_voice_intro.html) requires unique IDs.
`script.rs:510` initializes generated IDs at zero, and `script.rs:1317` increments
that counter; `scripted.rs:184` starts physical IDs at one. Both are keys of
`Driver.notes` (`scripted.rs:151`, `:236`, `:532`). The first generated play can
replace the physical parent's entry; later children can attach to a sibling,
and note-off closes the wrong note. Failing test:
`driver_noteoff_closes_original_physical_note`. **644 bound programs exposed;
0 installed-program causal reproductions counted yet**. The corpus's stuck
logical notes are consistent with this defect, not proof of its sole cause.

### 3. postEvent invents a new identity; delayed calls return an unrelated ID

[Events API](https://lua.uvi.net/group___events.html) specifies the event's voice
ID as the return value. `script_prelude.lua:283` forwards NoteOn through
`playNote(e)`, which always allocates a fresh ID (`script.rs:1317`). At line 278,
the delayed path reserves one ID, discards it, and allocates another on resume.
Fading/releasing the returned delayed ID addresses no eventual voice. Failing
tests: `forwarded_note_keeps_identity`,
`delayed_event_returns_its_actual_voice_id`. **Exposure count pending source
census; 0 installed-program delayed-ID failures demonstrated**.

### 4. Custom ScriptData never reaches onLoad; onSave has no host route

[onLoad/onSave](https://lua.uvi.net/group___event_callbacks.html) preserve custom
state after widget restoration. `script.rs:943` invokes only script bodies,
`__restore`, and `onInit`; it never calls `onLoad`. There is no save/restore API
on `ScriptHost` or `ScriptThread`. `script.rs:536` flattens ScriptData attributes
into the widget-name map; `lib.rs:426` emits behaviors with `state: Vec::new()`.
The RE spec's line 267 explicitly requires script-defined keys, comma-decimal
lists and literal `sequlenght` to survive. **Exposure/custom-state counts pending
fresh probe**. Repro: the corpus program above, plus the survey's `state_keys`
and `onLoad`/`onSave` symbol fields. Preserve raw lexical state first, decode by
the script-state contract, then restore widgets and invoke onLoad before events;
verify the exact onInit/onLoad ordering against native Falcon rather than guessing.

### 5. Live parameter writes reach only five correctly scoped catalog rows

[Element.setParameter](https://lua.uvi.net/class_element.html) controls engine
parameters. `script.rs:604` accepts only Gain/Pan on Program/Layer/Keygroup and
Polyphony on Program. All other writes remain Lua overlays
(`script_prelude.lua:244`). Inserts can be patched at offline initialization
(`script.rs:1195`, `lib.rs:1369`); this is not live automation, and the live loader
at `src/sound/v2.rs:1099` never applies these overrides. **657 cached programs
report unsupported parameter writes; all 307 zero-voice programs are in that
set**. Test: `oscillator_gain_emits_an_engine_write`; corpus: V Strings Harmonic.
The two Keygroup Gain/Pan rows are additionally mis-scoped (finding 8), leaving
only **5 of 2,877** catalog rows with the intended live destination.

### 6. Omitted parameters return zero instead of typed catalog defaults

[Element.getParameter](https://lua.uvi.net/class_element.html) returns the
parameter's typed value, including engine defaults. `script.rs:630` reads only
XML attributes; `script_prelude.lua:230` turns a missing value into numeric zero.
For example Layer.Gain must be 1, OnePole.Freq 1000 Hz, and DAHDSR.ReleaseTime
0.05 seconds. **657 programs report parameter access/connection gaps; 307 are
zero-voice. Exact catalog mismatch count pending exhaustive probe**. Tests:
`omitted_gain_defaults_to_unity`,
`every_catalog_parameter_has_typed_defaults_and_definitions`.

### 7. parameterDefinitions do not describe the catalog or valid numeric IDs

[Parameter definitions](https://lua.uvi.net/class_element.html) have numeric IDs
and typed metadata. `script_prelude.lua:217` assigns string IDs, hard-coded
`min=0,max=1`, zero defaults, and no type/readOnly/serialize fields. Line 220
also treats structural XML attributes such as Name as synthesis parameters;
line 191 covers only 14 selected types. **All 2,877 definitions across 167
element types fail the typed-ID contract; program exposure pending census**.
Tests: `filter_definition_uses_catalog_range` and the exhaustive catalog probe.
OnePole.Freq is 20–20,000 Hz, not 0–1. These are functional metadata, not just UI
labels: scripts discover parameter IDs/ranges from them.

### 8. Keygroup writes control its entire layer; layer writes touch only osc 1

[Keygroup](https://lua.uvi.net/class_keygroup.html) parameters are independent
engine elements. `script.rs:396` maps every keygroup onto `Scope::Layer`.
`scripted.rs:376` selects only the first group whose `osc == 1` for a layer.
In stacked layers, every other oscillator retains its old gain/pan. Core group
writes are absolute (`sampler-core/src/script_params.rs:499`), while the driver
passes an authored-relative delta (`scripted.rs:388`); authored non-unity group
gains/pans can therefore be subtracted a second time (the core baseline is set
in `sampler-core/src/lower.rs:337`). **Program counts pending census; 0 measured
installed-program isolation failures**. Test: `keygroup_writes_keep_distinct_scopes`;
corpus: V Strings Harmonic's stacked oscillator routing. Use one destination per
actual element and apply the core's absolute/relative contract consistently.

### 9. getParameterConnections fabricates an entry and drops real routing

[Connections API](https://lua.uvi.net/class_element.html) exposes actual
SignalConnections. `script_prelude.lua:250` returns the same inert object at
*every numeric index*, even when no connection exists; `pairs` sees no entries.
The real `<Connections>` nodes and mapper, ratio, bypass and inversion are not
represented. **657 observed programs report this gap, including all 307
zero-voice records**. Test: `connections_do_not_fabricate_entries`; corpus:
V Strings Harmonic (`Keygroup.Gain`, `SamplePlayer.Pitch`, `OnePole.Freq` and
many more). RE spec `DSP_FORMAT_SPECIFICATION.md:238` forbids flattening these
scopes. A truthy fake connection can select a branch that an empty list would not.

### 10. Async APIs silently swallow completion callbacks and task states

[Async API](https://lua.uvi.net/group___async.html) and
[async guide](https://lua.uvi.net/_async_intro.html) define task completion and
callbacks. `script_prelude.lua:40` creates inert globals, whose `__call` at line
22 ignores every argument. No loadSample/loadImpulse/loadData/saveState operation
or failure callback occurs. A fake `finished`/`success` table is truthy, so
polling can also believe a failed operation succeeded. **25 cached programs
report loadData (17 zero-voice); 15 report loadImpulse (all 15 zero-voice), with
overlap; loaded-code exposure pending census**. Test:
`async_api_completes_instead_of_swallowing_callback`; corpus: the survey identifies
program IDs. Unsupported I/O must complete with an explicit failure, not consume
callbacks forever.

### 11. Host UI exports pictures/variables, but no Lua control-edit route

[Widget changed callbacks](https://lua.uvi.net/_u_i_page.html) make the instrument
interactive. `script/ui.rs:95` exports `Binding::Variable { script: 0, name }`;
`ScriptThread::Message` (`scripted/thread.rs:28`) has no widget edit message.
`src/sound/v2.rs:1120` installs UVI with an empty control list; `:807` edits only
core controls. ScriptHost has no public widget setter. **644 bound programs are
exposed; 0 end-to-end UI edit reproductions counted**. Repro: V Strings Harmonic
or any exported UVI knob; attempting to route its variable binding cannot reach
Lua's changed function. The existing “0 UI unsupported” claim in
`UVI_SCRIPT_COVERAGE.md` establishes export coverage only.

### 12. Persistent widgets restore differently by constructor style

[Widget persistence](https://lua.uvi.net/class_widget.html) is true by default.
`script_prelude.lua:103` captures saved values only for table-style constructors;
positional Knob/Menu/Button/Table instances lose saved state. Later assignment
`w.persistent=false` is ignored because `__saved` was already captured. The
restore at line 127 does not notify Table.changed. **Program constructor-style
count not measured; 0 installed-program exact triggers demonstrated**. Tests:
`positional_widget_restores_saved_value`,
`persistent_false_assignment_prevents_restore`. Restore from the final widget
persistence property, not the constructor argument shape.

### 13. Table constructor and callback-suppression semantics are wrong

[Table API](https://lua.uvi.net/class_table.html) accepts six positional arguments
and `setValue(index,value,callChangedCallback)`. `script_prelude.lua:70` accepts
only five; lines 88–90 ignore positional table default/min/max. `:144` treats
the second value as the notify slot and always invokes changed, ignoring the
third argument. **Loaded-code Table exposure pending census; 0 exact installed
triggers counted**. Tests: `table_positional_constructor_preserves_values`,
`table_setvalue_can_suppress_callback`. This can mutate sequencer state during a
supposedly silent initialization or restore.

### 14. Undefined globals/widget properties are truthy stubs rather than nil

[Lua 5.1](https://www.lua.org/manual/5.1/manual.html#2.2) specifies nil for absent
variables/keys. `script_prelude.lua:32` substitutes stubs; `script.rs:667` relies
on a source-text heuristic for lower-case assignments only. Uppercase forward
references and names inside dead code/comments can become fake API objects.
`script_prelude.lua:182` also fabricates missing `changed`/properties. **617 cached
programs report fake globals callback/Mapper, including 275 zero-voice AO
programs; causality for those 275 remains to be measured**. Tests:
`undefined_global_remains_nil_even_when_called_later`,
`unset_widget_changed_is_nil`. Fallbacks must not change script control flow.

### 15. Non-note callbacks cannot filter incoming engine events

[Forwarding rules](https://lua.uvi.net/_agents_md.html) require a defined callback
to decide whether its event is forwarded. `src/sound/v2.rs:388` calls tell_script
and immediately applies the original CC/bend/pressure packet; `:356` explicitly
states this bypass. The worker cannot swallow or replace that original event.
`scripted.rs:255` also unconditionally key-ups a physical note after onRelease.
**Controller-handler exposure pending census; 0 filter-specific corpus triggers
measured**. Corpus: VWinds controller processing; authored repro contract:
`function onController(e) end` must prevent the original downstream CC, while a
postEvent must deliver exactly one. Note-on callback presence/suppression and
onEvent precedence are implemented; that does not establish every event path.

### 16. Pitch/program callback fields and public CC constant are incompatible

[Pitch/program callbacks](https://lua.uvi.net/group___event_callbacks.html) use
`e.bend` and `e.program`; [Event enum](https://lua.uvi.net/class_event.html)
includes `ControlChange`. `script.rs:1110`/`:1126` set only `value` and
`script_prelude.lua:52` defines only `Controller`. Forwarding at `:290` and `:296`
also reads value, discarding the documented fields. **40 pitch-handler programs
exposed in the existing source census; 0 program handlers there; fresh counts
below. 0 raw-CC-constant corpus triggers demonstrated**. Tests:
`pitch_callback_exposes_bend`, `program_callback_exposes_program`,
`public_controlchange_event_constant_exists`. The API itself has a documentation
inconsistency between Controller prose and the ControlChange enum; support the
public spelling and record native enum values before relying on numeric literals.

### 17. spawn/run inherit note context; isNoteHeld follows any held key

[Threading](https://lua.uvi.net/group___time.html) and
[voice guide](https://lua.uvi.net/_voice_intro.html) distinguish note callback
threads from spawned threads. `script.rs:742`/`:754` copy `current` into new
threads, so their `waitForRelease` and `duration=-1` follow the parent's note.
`script.rs:799` implements isNoteHeld as *any key down*, not the originating
note's gate. **spawn/run/isNoteHeld exposure pending census; 0 installed-program
exact multi-key failures measured**. Tests:
`spawn_does_not_inherit_release_wait`, `note_held_does_not_follow_another_key`.
The single-key baseline cannot catch this.

### 18. Spawn scheduling is not FIFO; realtime timestamps are not honored

[spawn/run](https://lua.uvi.net/group___time.html) distinguish deferred FIFO
creation from immediate run-until-wait. `script.rs:977` pops the last task,
appends the remaining queue, and reverses; three tasks a,b,c run b,a,c.
`scripted.rs:311` applies Play immediately, ignoring `Play.at_ms`; Change/Fade
also ignore their timestamps at `:324`/`:339`. `ScriptThread` advances after
draining every input (`scripted/thread.rs:97`), which can move the clock past
an earlier scheduled wait. **Thread API exposure pending census; 0 quantified
realtime jitter programs**. Test: `spawn_runs_in_creation_order`. The passing
baseline verifies offline ms/beat arithmetic, not worker timing; apply queued
commands at their sample timestamps and measure late-command latency separately.

### 19. Transport, meter and sample rate are invented or disconnected

[Musical context](https://lua.uvi.net/group___context.html) distinguishes host
song position from monotonic running time. `script.rs:780` hard-codes 4/4;
`:782`/`:783` return identical elapsed-ms/current-tempo values even while stopped
or after seeking. `HostInput` (`scripted.rs:38`) has no beat-position/meter field,
and the v2 input path never sends Tempo/Transport. Offline load paths use
`Config::default()` at `lib.rs:1339`/`:1429`, retaining 48 kHz despite the supplied
rate. **620 beat-context programs exposed in the previous census; exact fresh
count below; all 644 bound programs exposed to non-48-kHz conversion mismatch**.
Test: `stopped_transport_beat_is_not_elapsed_time`. `waitBeat` converting at the
current tempo when called is consistent with the API's explicit implementation;
do not label lack of mid-wait rescheduling a proven bug without native measurement.

### 20. Fade layers, change smoothing, and sample offsets are discarded

[Voice manipulation](https://lua.uvi.net/group___voice.html) supports layer-targeted
fades, optional smoothing and millisecond sample offsets. `script.rs:840–888`
drops fade layer arguments; `:811` ignores immediate. `Command::Fade` has no
layer target, so the core fades the entire note (`scripted.rs:342`).
setSampleOffset is only an unknown inert global. **660 fade users / 620 change
users / 40 offset users exposed in the prior census; fresh counts below;
0 exact layer/smoothing/offset corpus failures quantified**. Tests:
`fade_layer_argument_is_preserved`, `change_immediate_flag_is_preserved`.
Linear gain -> dB and tune-in-semitones conversions are otherwise implemented;
core has a separate fade multiplier (`sampler-core/src/script_params.rs:431`).

## Additional findings and coverage limits

- **releaseVoice return value:** [releaseVoice](https://lua.uvi.net/group___events.html)
  returns whether a matching voice existed. `script.rs:709` always returns true.
  Test `release_nonexistent_voice_is_false`; 620 source-census users exposed,
  0 installed-program stale-ID branching failures measured.
- **Full numeric playNote table form:** [playNote](https://lua.uvi.net/group___events.html)
  permits positional indices in its table form. `script.rs:1284` reads numeric
  keys only for the first three fields. Test
  `playnote_numeric_table_keeps_layer_and_oscillator`; 0 corpus uses of that exact
  form demonstrated. Named form and positional function arguments do parse.
- **Tree omissions:** [Program](https://lua.uvi.net/class_program.html) and
  [Element](https://lua.uvi.net/class_element.html) require part, children,
  synthChildren, mods, displayName, path, numParams and eventProcessors.
  `script.rs:413` builds only selected collection fields, and `:549` installs an
  inert Part only as parent. Test `synthesis_tree_aliases_exist`; all 644 bound
  programs exposed to the reduced tree, exact property users pending census.
  Oscillator.sampleInfo is also absent. Some name-indexed list lookup and parent
  references work; that is not the full synthesis hierarchy.
- **Multiple/bypassed ScriptProcessors:** Falcon's [Events processing order](https://uvi.s3.us-east-1.amazonaws.com/UVIFC/falcon_2026_manual.pdf)
  is top to bottom. `script.rs:943` puts every descendant script in one Lua state,
  overwriting callbacks and merging saved widget names across processors
  (`:532`); Bypass is ignored. Test recipe: two processors, first transposes,
  second forwards, then repeat with the second bypassed. Installed-program
  count pending survey's processor_count; 0 exact chain/bypass failures measured.
- **Detached note lifetime:** [voice guide](https://lua.uvi.net/_voice_intro.html)
  says a spawned/non-note `playNote(...,-1)` has no automatic release.
  `scripted.rs:106`/`:552` imposes an unrequested 5,000 ms lifetime. 0 installed
  detached-note lifetime cases measured; all generated-note users exposed.
- **Worker queues:** `scripted/thread.rs:203`, `:216`, `:224` discard failed event
  queue pushes without reporting. Command back-pressure instead blocks the
  worker. No stress count was measured; 644 bound programs are exposed. This is
  distinct from sample timing and requires a burst/overrun regression.
- **Luau vs Lua 5.1:** [UVI language](https://lua.uvi.net/_lua_reference.html)
  documents the bit library; only Luau bit32 exists (`script.rs:489`, prelude
  has no bit alias). Test `uvi_bit_library_is_available`; 0 installed bit.band
  users pending census. [Luau compatibility](https://luau.org/compatibility/)
  also documents absent tail calls, closure identity reuse, table-literal
  assignment order and different metamethod equality. Compile success alone
  proves none of those behaviors. getfenv/setfenv, math.pow and table.maxn are
  supported by Luau; do not claim they are missing. The host's nil/stub changes
  (finding 14), require resolution below and table-backed fake userdata are more
  immediate compatibility risks than syntax. Native userdata identity/lifetime
  behavior has not been measured.
- **require resolution/built-ins:** `script.rs:55` selects the shortest suffix
  path across the entire bank, with no script-directory context or ambiguity
  rejection. `script_prelude.lua:326` makes every uvi.* built-in a fake module.
  **617 observed cached programs load a stub uvi.ChordRec; 275 are zero-voice**.
  Repro: V Strings Harmonic. [Falcon scripts](https://support.uvi.net/hc/en-us/articles/360000047157-About-UVIScript)
  are engine-integrated processors, not arbitrary neutral objects. 0 measured
  duplicate-module-path programs; builtin ChordRec behavior remains open.
- **hasParameter/type validation:** [Element](https://lua.uvi.net/class_element.html)
  has hasParameter; prelude defines none. Unknown and ill-typed writes are
  accepted into overlays (`script_prelude.lua:244`), even NaN before the native
  finite guard. Catalog range enforcement is absent. 657 cached parameter users
  exposed; the exact invalid-value trigger count is 0 measured. Do not infer
  native clamp-vs-error behavior merely from a Min/Max table.

## Exhaustive catalog review

Read-only source:
`/home/derpcat/.t3/worktrees/KONTAKTO/t3code-80fe786b/artifacts/engine-analysis-2026-10-07/catalog.json`.
All **167 types / 2,877 rows** were reviewed, not a hand-picked parameter list.
There are **1,637 floats, 790 integers, 450 booleans**. The host reads booleans
serialized as 0/1 as numbers (`script.rs:643`), so `if bypass then` treats even
zero as true in Lua. Test `parameter_boolean_type_is_preserved`.

| Category | Element types | Parameter rows |
|---|---:|---:|
| FX | 83 | 1,005 |
| Modulation | 15 | 132 |
| Event Processor | 5 | 682 |
| Oscillator | 24 | 708 |
| Legacy | 32 | 252 |
| Other | 8 | 98 |
| Total | 167 | 2,877 |

The exhaustive authored test constructs each type without optional attributes,
checks each declared default against getParameter, and checks every definition's
numeric ID/type/range. It uses inserts as a generic metadata-access container;
it does not assert those types are valid native InsertEffect instances. Correct
parameter metadata must be independent of which XML attribute happened to be
serialized. The live-write reachability analysis uses real Program/Layer/Keygroup
scopes, rather than this generic container.

Catalog hazards: **36 rows have Min > Max**, including oscillator Pitch,
LoopLabOscillator.SliceStart, Engine/Program.MemorySize and Layer.VoiceCount.
Some are dynamic/read-only metadata; preserve this evidence and establish their
native descriptor semantics instead of blindly clamping them to a reversed
range. Program/Layer/Keygroup Gain is linear 0–1.995; Pan is -1–1. SamplePlayer
CoarseTune is semitones, FineTune is cents; sample offsets can be normalized or
milliseconds depending on the parameter name. DAHDSR/Layer timing parameters
are seconds, while playNote/wait/fade API arguments are milliseconds. LFO.Freq
changes interpretation when SyncToHost is enabled. Unit display formatting must
not change these stored values.

## Validation and handoff

Pending final runnable results and exact corpus census. Runtime code is untouched.
Promote individual ignored tests when their fixes land, expose UVI runtime
findings to the report/corpus, and rerun the 307-program subset before claiming
silence is resolved. Native Falcon measurements are still required for enum
numeric values, ScriptData encoding/restore sequencing, parameter clamping,
change smoothing, and async task failure contracts.
