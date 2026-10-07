# Kontakt 8 KSP behavioral audit

Audit branch: `v2/gpt-ksp-audit`, based on `699758ab` (`origin/integrate/core-v2`). Audit date: 2026-10-08. No runtime implementation was changed. The reproduction suite is [`crates/sampler-ksp/tests/audit.rs`](../../crates/sampler-ksp/tests/audit.rs); known failures are explicitly ignored so they can be enabled by the fixing agent.

The implementation is substantially less complete than `KSP_COVERAGE.md` suggests. Parsing an identifier, emitting a host request, or remembering a written value does not establish the corresponding Kontakt behavior. In particular, the production effect consumer currently applies presentation changes, not arbitrary engine, file, asynchronous, or scheduler requests.

## Evidence and ranking

The static census covered **2,741 distinct instrument paths and 52 distinct cached scripts**, from the existing `~/.cache/kontakto-ksp/ksp-all/manifest.tsv`. It deduplicates paths, strips comments and string literals, and follows named `call` functions when computing callback reachability. The broad engine-gap counts detect unsupported-parameter symbols in callback-reachable code; they do not prove that every referenced symbol is actually passed to a setter on an executed branch. It is a static exposure census, **not a count of instruments proven to sound wrong**. Conditional branches, actual control usage, saved state, module existence, and particular argument values can narrow the affected set. Counts overlap and must not be added. Repeated scripts do not constitute independent behavioral measurements.

The first 20 findings are ranked by distinct corpus exposure, with severity breaking ties. Where an issue needs a specific argument, more than twelve IDs, a long string, or a muted group, that narrower affected population is unknown; those findings appear later rather than receiving the entire symbol population. A synthetic failure establishes a contract mismatch in KONTRA, not a reference-host PCM comparison of every listed instrument.

Severity: **critical** means a broad engine control path cannot affect sound; **high** means wrong selection, scheduling, loading, event ownership, or state; **medium** means presentation/readback or narrower compatibility failures; **low** means accepting invalid source. Tests use an authored one-group, unity-PCM plan at 48 kHz, so audio/state assertions are deterministic. The async IR probe has no host resource service or convolution rack and demonstrates the missing service boundary; it is not an IR DSP comparison.

References are the live [Kontakt 8 KSP manual](https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/), the read-only RE documents `t3code-80fe786b/docs/DSP_FORMAT_SPECIFICATION.md` and `DSP_SYSTEM_INVENTORY.md`, and the supplied complete identifier catalog. The latter inventory explicitly says identifier extraction does not certify enum values or parameter laws (inventory lines 160–164, 226). No new Kontakt reference-host renders were performed. Unmeasured laws and exceptional arithmetic cases are left open, not declared compatible or invented as bugs.

## Ranked findings

### KSP-01 — Most engine parameter writes have no engine consumer

**Critical; exposure 2,741/2,741**, including a non-init callback reachable in every instrument. The catalog contains 676 distinct `$ENGINE_PAR_*` identifiers in 679 scoped rows; **663 have no specialized runtime dispatch**. Generic writes only update an instance-local key/value mirror and emit `set_engine_par`. The production consumer discards that service. Thus filter/EQ, effect-specific, source, routing/send, modulation, module-type and group-start writes do not apply their documented state. Even supported volume/pan/tune fall into this path when the parameter or addressing operands are not compile-time recognized.

- Manual: [set_engine_par and addressing](https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/engine-parameter-commands), [per-module parameters](https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/engine-parameters).
- Code: `crates/sampler-ksp/src/lower.rs:1916`, `:1921`, `:2146`; `src/sound/v2.rs:759`; `src/sound/mod.rs:232`; `crates/sampler-kontakt/src/effects.rs:175` (only five parameter names); `src/plugin.rs:892`; `crates/sampler-ksp/src/lib.rs:311` (UI-only accepted services).
- Failure: `dynamic_engine_parameter_write_changes_audio` writes a variable containing `$ENGINE_PAR_VOLUME = 0`; PCM remains `[1,1]` instead of silence. Module-by-module dispatch and value laws are listed in [KSP_AUDIT_ENGINE_PAR.tsv](KSP_AUDIT_ENGINE_PAR.tsv). This is not a claim that all saved effect DSP is absent: saved translation and live KSP mutation are different paths.

### KSP-02 — Engine display queries always return empty text

**High; exposure 2,741**, with runtime display queries reachable in all 2,741. Init display queries are reachable in 2,367. `get_engine_par_disp` and its extended form do not format either the current or requested parameter value.

- Manual: [get_engine_par_disp / get_engine_par_disp_ext](https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/engine-parameter-commands).
- Code: `crates/sampler-ksp/src/eval.rs:1223`; `crates/sampler-ksp/src/lower.rs:986`.
- Corpus: all 2,741 contain the display call; e.g. Audio Imperia Dolce, `01 7 1st Violins/Dolce - 01 7 1st Violins - Legato.nki`. `engine_display_query_returns_pan_text` directly tests runtime engine display readback and requires a nonempty pan string. Both init and runtime source return empty text; precise formatting still needs a native reference oracle.

### KSP-03 — Purge is attenuation, with no residency, load-time action, or status

**High; exposure 2,738**; purge is reachable in UI callbacks for 2,738 and load-time persistence callbacks for 369. Runtime purge toggles a group attenuation layer; it does not unload/reload sample residency. Load-time evaluator requests have no applied purge state. `get_purge_state` reads zero even for loaded samples. Async identities/completions are also missing (KSP-19). The correct callback restriction matters: the reproducer uses `on persistence_changed`, **not an illegal init purge**.

- Manual: [purge_group / get_purge_state](https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/group-commands).
- Code: `crates/sampler-ksp/src/eval.rs:1247`, `:1253`; `crates/sampler-ksp/src/lower.rs:1818`; `src/plugin.rs:892`.
- Failures: `purge_state_reports_loaded_samples` returns 0 instead of 1; `load_persistence_purge_excludes_group_from_playback` still renders `[1,1]` after load-time purge. Corpus example: Pacific Ensemble Strings, `10 Cellos/Pacific - Ens Strings - 10 Cellos - Trills.nki`.

### KSP-04 — Host built-ins are zero/default placeholders, not maintained state

**High; exposure 2,738** for the censused host/time/voice/output variables. `$NUM_OUTPUT_CHANNELS` alone appears in 2,732; `$NI_SONG_POSITION` in 349; musical duration in 5. Init has fixed 120-BPM/4-4 values and zero for output count and most other values. Runtime maps 32 host slots, initialized to zero; the production tree has no call to `Runtime::set_host_value`. This affects tempo, position, transport, signatures, output/zone/voice counts, async IDs/status, channel and other host state. The existing native `$ENGINE_UPTIME`, `%KEY_DOWN`, and ordinary CC paths are not included in this accusation.

- Manual: [built-in variables, Time and Transport / Events and MIDI / General](https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/built-in-variables-and-constants).
- Code: `crates/sampler-ksp/src/eval.rs:514`; `crates/sampler-ksp/src/lower.rs:72`, `:856`; `crates/sampler-core/src/ops.rs:528`, `:559`, `:898`.
- Failure: `musical_duration_is_available_in_note_callback` reads 0 instead of 500000 at the default 120 BPM. Census confirms actual output-count/position references, not just catalog entries. Exact host-specific defaults need reference-host measurements when the adapter is implemented.

### KSP-05 — Modulator/target lookup fabricates an index from the name

**High; exposure 2,737** (`find_mod`; two also use `get_mod_idx`, four use `find_target`). Lookup ignores owning group/modulator existence and hashes names. Missing modulators never reliably return -1. Envelope application recognizes only the hash of `ENV_AHDSR`, not a loaded modulator identity. Renamed envelopes or the same name on another target cannot be addressed correctly.

- Manual: [get_mod_idx / get_target_idx and historical find aliases](https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/engine-parameter-commands).
- Code: `crates/sampler-ksp/src/eval.rs:1238`; `crates/sampler-ksp/src/lower.rs:48`, `:1658`, `:1864`.
- Failure: `missing_modulator_returns_not_found` returns 83100167 rather than -1. Every runtime `find_mod` user is exposed, but correctly default-named AHDSR writes can work through the special case; 2,737 is not a measured broken-envelope total.

### KSP-06 — Init engine writes and authored reads use an isolated mirror

**High; exposure 2,625** with init-reachable `set_engine_par`; init-reachable unsupported-parameter mentions occur in 2,641. The init evaluator starts with an empty engine map and no authored engine model. Binding a compiled script does not replay its init pan/volume/envelope mutations into the runtime. Kontakt translation harvests init writes but applies only the five rack properties from KSP-01. Consequently UI defaults and engine state can disagree from the first note.

- Manual: [on init and on persistence_changed](https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/callbacks), [get/set_engine_par](https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/engine-parameter-commands).
- Code: `crates/sampler-ksp/src/eval.rs:148`, `:164`, `:1214`; `crates/sampler-ksp/src/lib.rs:757`; `crates/sampler-kontakt/src/library.rs:258`; `crates/sampler-kontakt/src/effects.rs:175`.
- Failures: `init_engine_read_matches_authored_center_pan` returns 0 instead of 500000; `init_engine_write_reaches_runtime` returns 500000 instead of the init-written 1000000. Corpus: Dolce legato above. Library translation separately evaluates init to collect writes, to detect dynamic slots, and again to compile runtime (`library.rs:258`, `:283`, `load.rs:611`); random/time/resource-dependent init can therefore disagree across those snapshots. That additional effect requires a full loader/reference comparison before assigning an affected count.

### KSP-07 — Runtime UI shortcut commands do not update presentation state

**Medium; exposure at least 2,018** (`set_knob_label` runtime reachable); `set_text` is runtime reachable in 505. Commands such as `set_text`, `set_knob_label`, `set_knob_unit`, movement and help emit effects, but the view consumer accepts only a small `set_control_par*` subset and keyboard services. Initial UI evaluation does implement many of them; runtime behavior is different.

- Manual: [set_text / set_knob_label / set_knob_unit / move_control](https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/user-interface-commands).
- Code: `crates/sampler-ksp/src/lower.rs:2008`; `crates/sampler-ksp/src/lib.rs:311`; `src/plugin.rs:892`.
- Failure: `set_text_updates_the_runtime_ui_model` drains emitted effects through the real view consumer; text remains `old` instead of `new`. Corpus count is an exposure lower bound for the family, not a union computed by adding commands.

### KSP-08 — Release-trigger counter reset has no effect

**High; exposure 1,923**, all reachable from note callbacks. The command is emitted to the host, whose effect consumer has no reset handler. Existing release-trigger timing is not proof that resetting that timing works; scripts explicitly call the reset to establish a different release-age origin.

- Manual: [reset_rls_trig_counter, Preprocessor & System Scripts](https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/advanced-concepts#preprocessor---system-scripts).
- Code: `crates/sampler-ksp/src/lower.rs:1997`; `src/plugin.rs:892`.
- Corpus: Pacific Ensemble Strings, `10 Cellos/Pacific - Ens Strings - 10 Cellos - Trills.nki`; 1,923 paths in the static census reference it in note-reachable code. This finding is source-traced and corpus-backed, not a reference PCM measurement; the fixing agent needs a release-age test with a native release-trigger group, not merely a script-cell roundtrip.

### KSP-09 — Current-event group parameter edits are silently dropped

**High; exposure 1,552** for `$EVENT_PAR_ALLOW_GROUP`. Writes can modify only notes in `self.deferred`; the currently entering MIDI note is not there. Readback of the allow state is not implemented. `$ALL_GROUPS` is lowered but still hits this restriction. The ordinary `allow_group`/`disallow_group` pending-selection path is separate and does work in existing tests.

- Manual: [set_event_par_arr / get_event_par_arr](https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/event-commands), [allow_group / disallow_group](https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/group-commands).
- Code: `crates/sampler-ksp/src/lower.rs:1687`; `crates/sampler-core/src/behavior.rs:1945` (deferred-only lookup).
- Failures: `set_event_group_can_disable_current_note_group` still creates one voice; `current_event_group_allow_state_is_readable` reads 0 rather than 1. The exposure count does not distinguish deferred-generated-note uses that already work from current-event uses.

### KSP-10 — Transport listener subscriptions never dispatch

**High; exposure 717** for transport start/stop subscriptions (369 start, 717 stop, deduplicated union 717). Listener drivers are built only for millisecond/beat timers. Binding adds only PGS signal callbacks; no transport signal reaches the listener. Timer listeners generating notes were tested and pass; do not conflate them with this gap.

- Manual: [set_listener and change_listener_par](https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/time-related-commands), [on listener](https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/callbacks).
- Code: `crates/sampler-ksp/src/lib.rs:430`, `:690`; `crates/sampler-ksp/src/lower.rs:72`.
- Corpus: CHORUS women traditional-syllables multi patch uses the listener infrastructure. `listener_can_generate_notes_without_input` is a passing timer baseline; transport dispatch needs an adapter-level reproducer because no KSP transport-ingress API is exposed.

### KSP-11 — Absolute mode 2 accumulates in change_vol and change_pan

**High; exposure ceiling 467** (`change_vol` users); `change_pan` has two users and shares the mode-dispatch bug (union not measured); the number exercising mode 2 or non-unity zone gain is not measured. All nonzero relative arguments become `true`, so mode 2 incorrectly adds. The voice layer multiplies an already-authored region gain, so mode 0 has no mechanism to replace zone volume while mode 2 preserves it.

- Manual: [change_vol / change_pan and their three modes](https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/event-commands).
- Code: `crates/sampler-ksp/src/lower.rs:2120`; `crates/sampler-core/src/script_params.rs:102`, `:136`.
- Failures: `relative_mode_two_is_absolute` applies -6000 then -3000 with mode 2 and reads -9000 instead of -3000; `pan_mode_two_is_absolute` applies 1000 then -1000 with mode 2 and expects -1000 instead of an accumulated zero. The zone-gain half is source-traced and needs a non-unity-region PCM probe before claiming a particular corpus instrument affected.

### KSP-12 — Persistence callback has init identity; live recall has no callback

**High; exposure 377** with `on persistence_changed`. The load sequence does restore persistent state after init and then execute persistence_changed, but the evaluator always reports init callback type. The compiled runtime persistence entry is never bound. Core control recall changes values without invoking persistence_changed, so snapshot-style recall cannot trigger the script's rebuild through this path.

- Manual: [on persistence_changed / on init](https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/callbacks).
- Code: `crates/sampler-ksp/src/eval.rs:176`, `:182`, `:518`; `crates/sampler-ksp/src/lib.rs:412`, `:683`; `crates/sampler-core/src/control.rs:300`; `crates/sampler-core/src/control/transfer.rs:171`.
- Failure: `persistence_callback_has_its_own_type` reads 0 instead of 11. Existing saved-array/UI tests validate the load order, not live recall callback dispatch. Full host snapshot replacement/recompilation may execute a fresh load sequence; this finding specifically identifies the live recall API and the unbound compiled entry.

### KSP-13 — Runtime menu mutation has no consumer

**Medium; exposure 373** for runtime-reachable `set_menu_item_visibility`. Menu item string/value/visibility mutations are emitted and discarded by the presentation consumer. Getter defaults/empty text cannot track the resulting item state. Init menu evaluation is a different implemented path.

- Manual: [menu item commands and getters](https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/user-interface-commands).
- Code: `crates/sampler-ksp/src/lower.rs:2022`, `:986`; `crates/sampler-ksp/src/lib.rs:311`.
- Corpus: CHORUS women traditional-syllables multi patch; 373 distinct instruments reach visibility writes outside init. `set_text_updates_the_runtime_ui_model` reproduces the same missing-shortcut consumer mechanism; a menu-specific state probe remains to be added by the fixing agent.

### KSP-14 — Callback IDs alias event IDs or zero

**High; exposure 373** for `$NI_CALLBACK_ID`. Note and release callbacks receive the same event ID; plan/controller/UI callbacks lack a unique callback identity and can read zero. A callback ID must identify one invocation independently of the note it operates on; stop_wait cannot be repaired by event IDs alone.

- Manual: [$NI_CALLBACK_ID](https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/built-in-variables-and-constants), [stop_wait](https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/time-related-commands).
- Code: `crates/sampler-ksp/src/lower.rs:823`, `:856`; `crates/sampler-core/src/behavior.rs:1966`.
- Failure: `note_and_release_have_distinct_callback_ids` reads 1 for both invocations. Corpus: CHORUS multi patch above.

### KSP-15 — Event source reports the reading slot instead of the creator

**High; exposure 369** for `$EVENT_PAR_SOURCE`. The native event stores only host/script provenance. Lowering converts script provenance into the reader's own slot, losing the script that called play_note.

- Manual: [get_event_par, $EVENT_PAR_SOURCE](https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/event-commands).
- Code: `crates/sampler-ksp/src/lower.rs:1524`; `crates/sampler-core/src/script_params.rs:543`.
- Failure: `event_source_reports_creating_script_slot`: slot 0 creates the event, slot 1 reads source 1 rather than 0. This directly exercises ordered module translation/binding, not a single-script approximation.

### KSP-16 — Init current-script slot is always zero

**Medium; exposure 369** for `$CURRENT_SCRIPT_SLOT`; affected instruments in slots above zero are not separately counted. The environment carries the authored slot, and runtime lowering uses it, but init evaluation ignores it. Slot-dependent setup and PGS names can diverge between init and performance.

- Manual: [$CURRENT_SCRIPT_SLOT](https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/built-in-variables-and-constants).
- Code: `crates/sampler-ksp/src/eval.rs:528`; `crates/sampler-kontakt/src/load.rs:535`.
- Failure: `init_current_script_slot_uses_environment` compiles slot 3 and reads 0 instead of 3.

### KSP-17 — Group selection/affected arrays have fake contents and length

**High; exposure 368** for `%GROUPS_SELECTED`; `%GROUPS_AFFECTED` has no census users. The selected-for-editing array always reads zero with no editor adapter. The affected-groups array also always reads zero and has a static 4096-element size instead of the current event's mapped groups. These arrays are not the current allow/disallow mask; implementing one as the other would also be incorrect.

- Manual: [%GROUPS_SELECTED / %GROUPS_AFFECTED](https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/built-in-variables-and-constants).
- Code: `crates/sampler-ksp/src/builtins.rs:396`; `crates/sampler-ksp/src/lower.rs:879`, `:908`.
- Failure: `affected_groups_have_dynamic_size` returns 4096 instead of 1. Corpus: CHORUS multi patch uses `%GROUPS_SELECTED`; this selection/editor use is separate from the synthetic affected-array failure.

### KSP-18 — stop_wait neither resumes nor disables subsequent waits

**High; exposure 368**. Emission is the entire implementation; no scheduler state changes. Both the immediate-resume mode and the mode that skips following waits are absent. Callback identity must be fixed together with KSP-14.

- Manual: [stop_wait](https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/time-related-commands).
- Code: `crates/sampler-ksp/src/lower.rs:2000`; `src/plugin.rs:892`; `crates/sampler-core/src/behavior.rs:1966`.
- Failure: `stop_wait_resumes_suspended_callback` remains at 0 after a controller cancels its one-second wait; expected 1. Corpus: CHORUS multi patch.

### KSP-19 — IR loading and asynchronous completion are absent

**High; exposure 354** for `load_ir_sample`: init reachable in 348, non-init reachable in 6. Async-complete callbacks occur in 16 instruments, wait_async in 8. Init loading requests return zero without loading; runtime requests have no service consumer. wait_async is an explicit no-op, async entries are not bound, and IDs/status slots are never populated. Success/failure cannot drive the script's intended UI or subsequent work.

- Manual: [load_ir_sample and load/save behavior](https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/load-save-commands), [on async_complete](https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/callbacks), [wait_async](https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/time-related-commands).
- Code: `crates/sampler-ksp/src/eval.rs:1253`; `crates/sampler-ksp/src/lower.rs:1980`, `:2004`; `crates/sampler-ksp/src/lib.rs:412`, `:684`; `src/plugin.rs:892`.
- Failure: `asynchronous_ir_request_has_identity_and_completion` invokes the UI callback and observes no completion. Its absent convolution rack limits it to a service-boundary probe. Corpus: Afflatus Chapter II Brass, `3. Curated Ensembles/Mountain Hotel Cup Mute.nki`; a loaded convolution-rack success/failure reference render remains required for the fix.

### KSP-20 — Runtime MIDI tick conversion is off by 1000

**High; exposure 349** for ticks_to_ms, 1 for ms_to_ticks. The runtime code interprets its microsecond input/output as milliseconds. Init uses a different implementation. Both paths also assume 120 BPM; wait_ticks uses a rounded 520-microsecond tick, making 960 ticks 499200 rather than 500000 microseconds even at that tempo.

- Manual: [ms_to_ticks / ticks_to_ms / wait_ticks](https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/time-related-commands).
- Code: `crates/sampler-ksp/src/lower.rs:1418`, `:1608`; `crates/sampler-ksp/src/eval.rs:805`.
- Failures: `runtime_time_conversion_uses_microseconds` gives 960000 instead of 960; `runtime_tick_conversion_returns_microseconds` gives 500 instead of 500000. Corpus: ANALOG STRINGS uses ms_to_ticks; ticks_to_ms appears in 349 paths. wait_ticks has no cached corpus users and needs a tempo-change/rounding scheduler probe.

## Additional gaps and source-traced limitations

| ID | Severity / corpus evidence | Behavior and manual citation | Code file:line | Reproducer or corpus instrument |
|---|---|---|---|---|
| KSP-21 | High; save_array 341, load_array 107, load_array_str 7; union not measured | File arrays never load/save; init cannot synchronously read NKA/resource data, runtime has no async transfer. `get_folder` cannot resolve real resource/library paths. [Load/Save Commands](https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/load-save-commands) | `crates/sampler-ksp/src/eval.rs:1253`; `crates/sampler-ksp/src/lower.rs:2038`, `:986`; `src/plugin.rs:892` | Areia, `05 Measured Tremolo/Areia - 01 16 Violins - Measured Tremolo.nki` (load_array). No decrypted array was copied or loaded for this audit. |
| KSP-22 | High; get_engine_par 237 | Generic reads return only local prior writes or 0, not authored/current module state. Specialized slot writes do not update that mirror; writes by another script are not shared. [Engine Parameter Commands](https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/engine-parameter-commands) | `crates/sampler-ksp/src/eval.rs:1214`; `crates/sampler-ksp/src/lower.rs:1840`, `:1920` | `init_engine_read_matches_authored_center_pan`; Dolce legato. The probe establishes init read failure; cross-slot/slot-rack readback still needs its own configured-rack test. |
| KSP-23 | High; poly_at callback 1; async_complete 16; rpn/nrpn 0 | poly_at, rpn/nrpn entries compile but never bind; ingress pressure/bend only updates expression state and cannot deliver virtual controller numbers 128/129. `%POLY_AT` is unmaintained. [Callbacks](https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/callbacks), [Events/MIDI built-ins](https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/built-in-variables-and-constants) | `crates/sampler-ksp/src/lib.rs:412`, `:676`; `crates/sampler-midi/src/ingress.rs:183`; `crates/sampler-core/src/controller_event.rs:68` | `pitch_bend_can_trigger_controller_callback` gets InvalidInput for number 128. The count of pitch-bend-dependent corpus callbacks was not measured. |
| KSP-24 | High; 4 instruments in a historical 781-instrument completed translation subset, not a total-corpus census | Only one start-on-key criterion is modeled. Compound, controller, cycle/RR/random/slice start criteria are reported unsupported, then fail to provide their selection semantics. Live start-option ENGINE_PAR writes are also unapplied. [Group Start Options](https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/engine-parameters#group-start-options) | `crates/sampler-kontakt/src/keyswitch.rs:59`, `:72`; `crates/sampler-kontakt/src/library.rs:578`; `crates/sampler-ksp/src/lower.rs:1922` | Performance Samples Vista, `Bonus/Vista - 3 Violins FFF Overlay.nki`; existing `keyswitch.rs:911` fixture explicitly expects an unsupported RR group. Cached evidence: `~/.cache/kontakto-corpus/full3.jsonl`. |
| KSP-25 | High; 6 event_status users | Active events report default 0, not queued-note state; this prevents lifecycle decisions even though native event ownership is retained. [event_status](https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/event-commands) | `crates/sampler-ksp/src/lower.rs:2050` (default fallback) | `active_event_status_is_note_queue` reads 0 instead of 1; Conflux `Instruments/Conflux.nki`. |
| KSP-26 | High; PGS reads in 5 instruments, init reads in 3 | Scripts evaluate init independently; later slots cannot read earlier init PGS writes. Shared storage is merged only after compile, and duplicate keys keep first values. Runtime numeric PGS propagation does work in existing tests; this is specifically initialization. [PGS](https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/advanced-concepts#pgs) | `crates/sampler-ksp/src/eval.rs:148`; `crates/sampler-ksp/src/lib.rs:441`, `:767`; `crates/sampler-kontakt/src/load.rs:611` | `later_script_init_reads_prior_slot_pgs_state` reads 0 instead of 42. Creating/writing PGS in 2,392 instruments does not mean all 2,392 require cross-slot init reads. |
| KSP-27 | Medium; 4 get_menu_item_str users, 1 get_control_par_str user | Runtime string-valued builtin getters always append nothing, even following implemented string-property writes. [UI getters](https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/user-interface-commands) | `crates/sampler-ksp/src/lower.rs:986` | `text_property_write_is_visible_to_script` gets empty instead of `new`. |
| KSP-28 | High; set_control_par_arr 3, get_control_par_arr 2 | Indexed property keys omit the index; two table positions alias. Setting CONTROL_PAR_VALUE does not update the script's table array. [set/get_control_par_arr](https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/user-interface-commands), [table control values](https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/control-parameters) | `crates/sampler-ksp/src/lower.rs:312`, `:2814`, `:2842`, `:2866` | `table_value_setter_updates_script_array` gets 0 instead of 7; `indexed_control_properties_do_not_alias` gets 9 instead of 7. |
| KSP-29 | Medium; play-position 3, release-velocity 1 | Release velocity and play-position event parameters fall through to zero rather than actual event/voice state. [get_event_par](https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/event-commands) | `crates/sampler-ksp/src/lower.rs:1509`, `:2050` | `release_velocity_is_event_parameter` supplies 64 and reads 0. Play-position needs a running-voice reference probe for its exact reported position. |
| KSP-30 | High; get_event_ids 1 | Enumeration does not fill the destination array with active IDs. [get_event_ids](https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/event-commands) | `crates/sampler-ksp/src/lower.rs:2050` | `get_event_ids_contains_current_event` cannot find the current event; Conflux. |
| KSP-31 | High; marks/by_marks 1, ALL_EVENTS 6 | No mark state; mark setters are discarded effects, getters return 0. Direct by_marks/ALL_EVENTS operations are explicitly ignored. Stored selector IDs are not resolved as event sets either. [event marks / by_marks](https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/event-commands) | `crates/sampler-ksp/src/lower.rs:1049`, `:1992`; `src/plugin.rs:892` | `event_mark_is_readable` gets 0 instead of 1; `note_off_by_marks_releases_matching_event` leaves the event held. ANALOG STRINGS, `Instruments/ANALOG STRINGS.nki`. |
| KSP-32 | Medium; reset timer 1 | KSP_TIMER is the audio sample clock in microseconds and reset has no effect; it is not a resettable execution timer. [reset_ksp_timer](https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/time-related-commands) | `crates/sampler-ksp/src/lower.rs:836`, `:2001`; `crates/sampler-core/src/ops.rs:906` | `reset_ksp_timer_resets_readback` sees about 10000 after reset; ANALOG STRINGS. Execution-time exactness remains a native-host measurement task. |
| KSP-33 | High; sort 1, real-sort subset unknown | Real arrays are accepted by sort but values travel through signed integer operations/comparisons; double bit patterns can fault the callback. [sort](https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/array-commands) | `crates/sampler-ksp/src/lower.rs:2500` | `real_array_sort_executes` leaves its result 0 instead of 1. Integer sorting passes; the one corpus sort is not claimed to be real without an argument census. |
| KSP-34 | Medium; long-string affected population unknown | Runtime text cells hold 256 bytes, below the documented 320-character string capacity; strings generated at runtime truncate earlier than init strings. [string variables/arrays](https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/variables) | `crates/sampler-core/src/ops.rs:11`, `:371`; `crates/sampler-core/src/behavior.rs:362` | `string_capacity_covers_320_characters` builds 300 ASCII characters and keeps only 256. Non-ASCII character/byte semantics need a separate reference test. |
| KSP-35 | Medium; no catalogued CUSTOM users | The 16-element custom event-parameter array is absent; only four legacy EVENT_PAR_0..3 slots are supported. [set/get_event_par_arr](https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/event-commands) | `crates/sampler-ksp/src/lower.rs:1747`, `:2050` | `custom_event_array_parameters_roundtrip` at index 15 returns 0 instead of 42. |
| KSP-36 | High; cached corpus has no mf_* users | MIDI object buffer, import, traversal, mutation, marks, tracks and export commands are not implemented; mf_set_buffer_size fails compilation. This is absent functionality, not a successfully parsed surface. [MIDI Object Commands](https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/midi-object-commands), [save_midi_file](https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/load-save-commands) | `crates/sampler-ksp/src/builtins.rs:1` (catalog), `crates/sampler-ksp/src/sema.rs:817` builtin resolution | `midi_file_buffer_commands_are_supported` fails compilation. No native MIDI-object reference comparison was performed. |
| KSP-37 | Medium; ui_controls/ui_update/note_controller census 0 | Global UI callback and update/note-controller callbacks are rejected; therefore global-before-individual UI ordering cannot occur. Multiscript MIDI-in semantics are also outside the supported callback set. [Callbacks](https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/callbacks) | `crates/sampler-ksp/src/sema.rs:110` | `global_ui_callback_is_supported` fails compilation. Multiscript MIDI-in needs a multiscript adapter, not a fabricated instrument-note callback. |
| KSP-38 | Low; real search census 0 | search and array_equal accept real arrays although the documented operations exclude them; bit equality is not a valid substitute for rejected syntax. [Array Commands](https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/array-commands) | `crates/sampler-ksp/src/lower.rs:2637`; `crates/sampler-ksp/src/builtins.rs` array signatures | `real_search_is_rejected_as_documented` compiles successfully when rejection is expected. |
| KSP-39 | High; muted-group affected population not measured | Muted groups are removed and group names compacted, changing original group indexes. Scripts and init engine writes using authored numeric indexes can address the wrong following group; group lookup cannot find the missing index-preserving entry. [Group indexes](https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/group-commands), [engine group addressing](https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/engine-parameter-commands) | `crates/sampler-kontakt/src/library.rs:255`, `:560`, `:578`; `crates/sampler-kontakt/src/load.rs:581` | **Source-traced candidate; no executed reproduction yet.** Authored groups `[muted A, active B]`, script targets original group 1; runtime B becomes group 0. A constructed NKI or corpus muted-group count is still needed; do not label every group-index user affected. |
| KSP-40 | Medium; >12 distinct per-note IDs not observed | From-script modulator values plus legacy custom parameters share twelve native slots, silently dropping further distinct IDs. IDs up to 1000 are valid. Value clamping for ordinary MOD_VALUE_ID **is implemented**; do not file that as missing. Unbounded MOD_VALUE_EX_ID is unsupported. [set_event_par_arr](https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/event-commands) | `crates/sampler-core/src/script_params.rs:308`, `:334`, `:528`; `crates/sampler-ksp/src/lower.rs:1720` | `thirteen_script_modulator_ids_roundtrip` writes IDs 0..12 on one event and reads ID 12. Cached MOD_VALUE_ID symbol users: 1,911, but the code comment and known corpus usages describe fewer IDs; this is a capacity ceiling, not 1,911 measured failures. |

## All ENGINE_PAR modules and value laws

The [TSV appendix](KSP_AUDIT_ENGINE_PAR.tsv) has one row for **every scoped `$ENGINE_PAR_*` catalog entry**, including group start and module type/subtype parameters. It preserves duplicate category membership and direct manual anchors. The catalog also includes `$EFFECT_TYPE_*`, `$FILTER_TYPE_*` and other enums; those are not silently counted as ENGINE_PAR parameters. Parsing their names does not certify their numeric enum identity.

| Family | Scoped ENGINE_PAR rows | Behavioral result |
|---|---:|---|
| Instrument, Source and Amplifier | 38 | Only conditionally recognized volume/pan/tune write/read native state. Source, routing, send levels and remaining laws have no live application. |
| Filters and EQs | 52 | No live per-module law dispatch. |
| Insert effects, general | 2 | Bypass/output gain special cases; readback and arbitrary module edits remain incomplete. |
| Insert Dynamics / Amps / Stomps | 41 / 113 / 76 | No per-module live laws. |
| Insert Lo-Fi / Tape / Modulation | 16 / 13 / 77 | No per-module live laws. |
| Insert Mangling / Spatial / Utilities | 36 / 14 / 12 | No per-module live laws. |
| Send effects, general | 3 | Bypass/output gain/dry special cases. |
| Send Delay / Reverb / Modulation / Utilities | 59 / 45 / 15 / 1 | No per-module live laws. IR loading is independently absent. |
| Modulation | 50 | Four AHDSR stage parameters plus attack curve, only for the fabricated default-name identity; other routes/types/laws unapplied. |
| Module Types and Subtypes | 5 | Generic mirror/outbox; no module replacement lifecycle. |
| Group Start Options | 11 | Generic mirror/outbox; saved start options have the limited translation in KSP-24. |

The **13 specialized names are conditional support, not certified laws**. Source arithmetic at `lower.rs:2190`, `:2220`, `:2250`, `:2283`, `:2330` uses cubic effect/sustain gain, exponential AHDSR timing, logarithmic volume and linear pan/tune. These must be checked against configured Kontakt modules at endpoints, intermediate values, sample rates and live voice stages. The RE inventory's independently measured Daft filter laws (`DSP_SYSTEM_INVENTORY.md:55`) and gain/control-cadence cases do not certify these KSP conversion formulas. No attempt was made to invent 663 missing module laws from identifier spelling.

An additional source-visible edge: engine volume 0 is clamped to 1 before logarithmic conversion (`lower.rs:2289`), leaving a tiny nonzero gain; whether a concrete signal path flushes that away and the exact native endpoint need measurement. It is not counted as a reproduced audio failure in this report. AHDSR sustain conversion quantizes to a 0..1000 gain scale; exact curve/quantization and active-voice update behavior are also open reference comparisons.

## What passed, and arithmetic/lifetime limits

The audit's passing baselines are `integer_and_polyphonic_baseline`, `ignore_event_accepts_an_aliased_current_id`, and `listener_can_generate_notes_without_input`. Signed integer wrapping and truncating negative division behave as the current tests expect; two overlapping notes keep separate polyphonic cells through release. A copied current event ID suppresses the note, and a timer listener can generate audio without input. These are retained as normal tests, not mislabeled failures.

Existing tests to preserve include `event_ids.rs:82`, `:153`, `:256`, `:302`, `:374`, `:434`, `:483` (stored/generated IDs, stale-ID isolation, release order, pending attack and release-once behavior); `compile.rs:245`, `:452` (polyphonic lifetime and waits); `params.rs:135`, `:193`, `:244`, `:325`, `:341`, `:365` (runtime group mute/engine read, PGS signals, held keys, block-invariant fades, slot writes); and `strings.rs:135`, `ui.rs:122` (load-time persistence). A note ID remains valid while native ownership/callbacks/voices retain it and retired IDs do not select a reused slot (`sampler-core/src/note_event.rs:104`, `:117`). The exact native retirement point across release tails and synthetic note_off should still be recorded against Kontakt.

Integer operations use signed 32-bit wrapping; division truncates toward zero, divide-by-zero returns 0, and MIN/-1 wraps (`sampler-core/src/integer.rs`). Real cells use double precision; invalid arithmetic can produce nonfinite values. The manual states storage ranges/precision but does not settle every overflow, divide-by-zero, NaN/Inf or conversion diagnostic. **These exceptional cases are unverified**, not asserted compatible merely because Rust has an answer. Required reference cases: MAX+1, MIN-1, MIN/-1, division by zero, integer conversion outside range, real zero division, sqrt/log domain errors, NaN/Inf comparisons, and init/runtime parity.

Late note/velocity writes are confined to a reached module projection (`sampler-core/src/behavior.rs:1577`, `sampler-core/src/note_event.rs:208`); forwarded downstream copies and already committed voices retain their properties. This source trace is consistent with the manual restriction after the first wait, but is not a reference-host validation of every callback/alias case.

Likewise, ordinary wait uses the native sample-clock scheduler and existing wait/polyphonic tests pass. Remaining reference cases include tempo changes during wait_ticks, zero/negative waits, late change_note/change_velo restrictions, ID lifetime after release tails, callback admission/order under reentrant script-generated events, PGS change ordering during init, and exact persistent strings/arrays/snapshot exclusions. The saved IR has scalar text, integer arrays and real arrays but no string-array variant (`sampler-ir/src/lib.rs:1188`), so persistent string-array end-to-end roundtrip remains absent and needs importer evidence before assigning corpus impact.

## Reproduction and handoff

Run every cargo command through the required shared guard:

```sh
/home/derpcat/.cache/kontakto-heavy cargo test -p sampler-ksp --test audit
/home/derpcat/.cache/kontakto-heavy cargo test -p sampler-ksp --test audit -- --include-ignored
/home/derpcat/.cache/kontakto-heavy cargo test --no-run -p sampler-ksp -p sampler-core -p sampler-kontakt
```

Verified on this branch after restoring the cancelled audit:

- `cargo test --no-run -p sampler-ksp -p sampler-core -p sampler-kontakt`: exit 0.
- Normal tests for those three crates: **519 passed, 0 failed, 46 ignored**, across 76 reported test groups; exit 0. The normal run preceded the last two opt-in probe additions, which were compiled and exercised in the following audit run.
- Audit with `--include-ignored`: **43 probes, 3 passed and 40 failed**, exit 101 as expected; no ignored probes. This includes the corrected legal-callback purge/IR cases, thirteen-modulator-ID capacity, direct engine display readback, and pan mode 2.
- Final compile check after the audit: exit 0. `git diff --check`: clean.
- Exact catalog validation: appendix matches all **679 scoped rows / 676 unique ENGINE_PAR identifiers** in the supplied JSON catalog.

Logs: `/home/derpcat/.cache/kontakto-gpt-ksp-audit/verify-final.log` (final sequential verification) and `audit-tests.log` (earlier 40-probe run). The final observed values include dynamic volume PCM `[1,1]` instead of silence; load-time persistence purge PCM `[1,1]`; pan mode 2 at 0 instead of -1000; modulator ID 12 at 0 instead of 42; and no asynchronous completion for the IR request.

The opt-in suite is deliberately expected to fail until implementations change. Re-enable each repaired contract as a normal regression test. Prioritize real engine dispatch/readback and one shared host-service/async/scheduler path; fixing an isolated symbol's mirror would leave its sibling calls broken. Use the configured Kontakt reference recorder to measure unresolved value laws and callback/retirement edge cases; do not treat this static census as a reference render. No samples, decrypted scripts, key material, runtime fixes or reference worktree edits are included in this branch.
