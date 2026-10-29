# Gold standard: voice lifecycle and event timing

Status: research, read-only. Written 2026-10-27 on branch `v3-P7-ir-prepare`.
Tags: `[source]` file:line in this repo or a reference repo, `[binary]` read from a vendor binary, `[doc]` vendor manual, `[web]` online doc, `[corpus]` dumped or counted library scripts, `[inferred]` reasoning not yet measured.

Scope: note-on to voice start, the event queue, script callbacks, allocation, stealing, fades, note-off and release, sustain pedal, polyphony and voice groups (choke/exclusive), legato, release triggers.

---

## 0. Summary

- KONTRA's engine core is sound on timing: one ordered queue, sample-accurate dispatch inside a block, notes that own their voices, and a script that cannot move musical time. That part should stay. [source] `crates/engine/engine/src/lib.rs:278-311`, `queue.rs:52-92`.
- The voice-lifecycle model is thin. The engine has a single steal rule, one fixed 5 ms fade, one-directional choke, no exclusive sets, no monophonic release, no one-shot, no legato position carry, and no note-scoped limits. Kontakt, HISE, sfizz, UVI and Decent all express these as data.
- The biggest real gap is upstream: the fresh Kontakt translator maps only the release flag. Voice-group polyphony, exclusion, one-shot and kill mode never reach the plan. [source] `crates/format/kontakt/src/translate.rs:34,178-181`, and no `polyphony`/`off_by`/`one_shot` mapping exists anywhere in `crates/format/kontakt/src`.
- Gold standard in one line: **every format compiles into one plan of voice groups (limits, steal policy, fade, exclusion edges) plus note-scoped rules, executed by one start pipeline and one victim function, timed by the same sample clock.**

---

## 1. Reference: Kontakt 8

### 1.1 Voice groups and allocation [doc][binary]

- 128 predefined voice groups per instrument; unassigned groups share the instrument pool. Each has: Voices (max), Mode (Kill Any / Oldest / Newest / Highest / Lowest), Pref.Rel (prefer released), FadeTime (ms; voices may temporarily exceed the max while fading), Excl.Grp (one of 16 exclusive groups). [doc] Kontakt manual, Classic view › Voice Groups (manual pp.156–157).
- Exclusive groups: a voice in one voice group cuts all still-sounding voices in *other* voice groups of the same exclusive set. A domain never excludes itself. Same manual section.
- Instrument-level Voices Handling tab: default Mode and a global fadeout time "a short fade-out ... eliminates clicking". [doc] Manual, Classic view › Voices Handling.
- Lua API defaults: voice group `mode` "oldest", `voices` 1, `fade_time` 10 ms, `prefer_released` true, `exclusive_group` nil; instrument `voice_stealing_fadeout` 10. [doc] `LUA_API_Manual.pdf`, Instrument and Group sections.
- Binary: group record reader at `0x140d049b0`, kill mode +0x2c (u16), pref byte +0x30, count +0x28, fade +0x34, exclusion +0x38; defaults (oldest, 1 voice, pref nonzero, 10 ms, exclusion -1) are materialised only when a group assigns the slot. [binary] `docs/re/KONTAKT_VOICE_GROUP_DEFAULTS.md` (addresses 0x140d14978–0x140d149a1).
- Victim walk: candidates are voices with state 0 or 2; three matches gate the walk (plan generation, voice-group domain, source domain). Kill selectors: Any ranks by a per-voice integer (+0x51c) not by level; Oldest/Newest use a per-voice age counter reset at start; Highest/Lowest use note key. Ties: first in list. Prefer-released runs the same selector first on releasing voices. Cut: state → fading with a float fade length. [binary] `docs/re/engine/k-voice-engine.md` (FUN_14086fea0, 0140860000.c, decompile export).
- Unrecovered: the Any key meaning, tie-breaks by birth order, exact fade law, and the per-key table written after a cut. [inferred from doc] Open items O1–O3 in `docs/re/engine/k-voice-engine.md`.

### 1.2 Sample-level choke and release [doc]

- Sample-level Choke (Group mode): samples sharing a choke value are mutually exclusive; a newer one cancels an older one. Four choke groups per kit. [doc] Kontakt manual, Sample/Choke (Melody vs Group mode, around line 2269 of the text dump).
- Trigger Style: Hold / One-Shot (Loop off); Hold / Latch (Loop on). One-Shot plays to the end marker regardless of key-up. [doc] same section.
- Legato (Melody engine): a legato note continues the playback position of a still-sounding note instead of restarting from the start marker. [doc] same section; Group-level Legato button: "carry its current playback position over to each following note". [doc] Group tab.

### 1.3 Release triggers [doc]

- Release Trigger samples fire on note-off. `t(ms)` counts from note-on and freezes at note-off as a modulation source (release samples shorter for longer held notes). [doc] Group tab, release-trigger section (manual p.205 per `docs/re/engine/BASIC_BEHAVIORS.md`).
- Monophonic (release only): a repeated note cuts all sounding release samples of the same key, leaving one release at a time. [doc] same section.
- Release-trigger groups are suppressed at key-up while CC64 >= 64 and fire at pedal-up. [binary] `docs/re/engine/BASIC_BEHAVIORS.md` row 26 (decompile generator `0140840000.c`).

### 1.4 Sustain pedal (CC64) [doc]

- Controller tab, three modes: **Pedal + CC** (sustain and CC64 is a normal controller), **Sustain Pedal without Controller** (sustain only), **Controller Only** (no sustain; CC64 is a controller). [doc] Kontakt manual, Classic view › Controller tab (p.108).
- Accept all notes off / all sounds off is a separate switch. [doc] same tab.

### 1.5 Script side (KSP) [doc]

- `note_off(id, offset_us)`: equivalent to a key release; always triggers `on release`; the optional offset replaces an event's own duration. [doc] KSP manual, General Commands › note_off().
- `play_note(note, vel, sample_offset_us, duration_us)`: sample offset is a play position; duration `-1` stops on the parent's release, `0` plays the whole sample. [doc] KSP manual, play_note().
- `fade_out(id, fade_us, stop_voice)`: voice-level fade; `stop_voice` 1 stops, 0 keeps running. [doc] KSP manual, Event Commands › fade_out().
- `ignore_event`, `wait`, `$EVENT_PAR_REL_VELOCITY` (release velocity readable in `on release`). [doc] KSP manual, `on release` example and `get_event_par`.
- `play_note` has no delayed-start argument; delayed musical events are built with `wait`. [doc] KSP manual, play_note() signature. "Child is queued at wait time, parent note not moved" [inferred from the documented signature and the `wait` examples].

### 1.6 Corpus evidence: how real libraries use this (Kontakt) [corpus]

Method: `dump_scripts` (existing `sampler-kontakt` example, read-only, no build) run over `/data/main/Libraries/Kontakt`: 26 distinct scripts from 405 instrument uses. Only Afflatus (22 scripts, 348 uses), ANALOG STRINGS (3) and Areia (1, 54 uses) yielded scripts. The other folders (Conflux, Dolce, CHORUS, Una Corda, Morphology, Pacific, Shreddage) have `make_persistent` scripts per `docs/re/ksp/PERSISTENCE_CENSUS.md`, but the dumper produced no output for them. Open gap, see section 9.

| Behaviour | Afflatus Chapter II Brass | ANALOG STRINGS | Areia 1.2.0 |
|---|---|---|---|
| `on note` / `on release` with `ignore_event` | 22/22 | 3/3 | 1/1 |
| `play_note` with duration (release samples) | 22/22 | 3/3 | 1/1 |
| `note_off` on a host note | 22/22 | 3/3 | 1/1 |
| `fade_out` | 10/22 | 3/3 | 1/1 (`fade_out($ALL_EVENTS,20000,1)` ×4) |
| `get_event_par` / `set_event_par` for per-event state | 22/22 | 3/3 | 1/1 |
| `ENGINE_PAR_SUSTAIN` written per articulation | — | 1/3 | 1/1 |
| Script-implemented sustain pedal (`wait` loop on `$sustain_ctrl`) | 22/22 | — | — |
| `KEY_DOWN` | — | — | 1/1 |

Concrete reliance:

- **Afflatus** implements its own release trigger and sustain pedal in KSP: `on release` stores hold state, waits in `wait(2000)` while the pedal is down, picks release groups with `allow_group`/`disallow_group`, computes a release start offset from ENGINE_UPTIME minus a note-on stamp in `$EVENT_PAR_2`, then `play_note(..., 50000)` and `note_off`. [corpus] `on release` in the dumped script, around lines 1999–2065. Consequence: the engine's own release trigger and CC64 paths are not what this library uses; it reproduces them with integer-ms timing.
- **ANALOG STRINGS** implements mono/legato fades in script: "Fade Out: Sets the fade out time of previous notes. Only works when Vol Env Release is disabled." [corpus] control help text in two of three scripts. So monophonic behaviour is library code, not an engine voice-group mode.
- **Areia** cuts previous notes with `fade_out($ALL_EVENTS,20000,1)` (20 ms, stop voice) and chains child notes with `note_off(%notes__note2last_child_id_src0[$EVENT_NOTE])`. [corpus]
- Afflatus and Areia both lean on `set_event_par`/`get_event_par` with `$EVENT_PAR_0..3` as event-scoped storage, which KONTRA already supports (`script.rs:354-386`, `ledger.rs` custom[4]).

### 1.7 Corpus evidence: UVI and drum libraries

- **UVI binary** (`/data/dev/workspace/re-binaries/UVIWorkstationx64.exe`) names the per-program voice model: `Poly`, `Mono Retrigger`, `Mono Portamento`, `Poly Portamento`, `Mono Portamento Slide`, `Constant Time` / `Proportional Time`, `NumVoicesPerNote`, `VoiceCount`, `CustomPolyphony`, `NotePolyphony`, `Polyphony`, `ExclusiveGroup`, `ReleaseTrigger`, `Hold/Legato/Sustenuto Pedal (on/off)`. [binary] string table; semantics not recovered.
- **VWinds (Flutes, Clarinets, Double Reeds)** manual: an overlapping note "automatically stop[s] the first note and play[s] a legato instead of a normal sustain"; transitions are Real (recorded), Modeled, or Smart (chosen by interval); velocity sets legato attack separately from sustain attack. [doc] `/data/main/Libraries/UVI/VWinds - Flutes/Documentation/VWinds_Flutes.pdf`, Legato Transitions section (lines ~264–320 of its text).
- **Augmented Orchestra** (UVI): one `.ufs`, not decoded. **UVI Starter.ufs**: absent at that path (0 files). Not analysed further.
- **MODO DRUM** `.pak`: cymbal "Choke" articulations are recorded samples ("Crash Cymbal 1 Choke", "China Cymbal Choke"), i.e. the choke is a note, not an engine rule. [binary-strings] `strings` on the `.pak`. Consequence: the engine must cut the open cymbal when the choke note sounds, via exclusion.
- **Ample** (`.ascf` plus `*_DBConfig.cf`) and **EZDrummer / Superior Drummer** (`.s20`, `.obw`): formats not decoded; no voice-lifecycle data read. Declared as not analysed.

### 1.8 Engine facts Kontakt actually implements [doc][binary]

- Voice pool is per instrument (`Max Voices` on the header, default 256 in this repo's translator); the CPU overload protection (Relaxed/Medium/Strict) kills voices under load. [doc] Kontakt manual, Instrument header; CPU protection section. KONTRA has no equivalent (see gaps).
- Time Machine Pro has its own voice limits (standard, HQ) independent of the instrument limit. [doc] Voices Handling. [binary] `docs/re/engine/KONTAKT_CPU_ARCHITECTURE.md` (steal fade bounded at 1000 at `01410a0000.c:3523`).

---

## 2. Reference: UVI Workstation / UVIScript [binary][repo]

- Repo coverage of UVIScript: `docs/architecture-v2/UVI_SCRIPT_COVERAGE.md`. Implemented: `onNote`, `onRelease` (`wait_for_release_resumes_on_note_off_and_release_is_called`), `playNote`, `releaseVoice`, `fadein`/`fadeout`, `wait`, `spawn`, `run`, `waitBeat`, `changeVolume`/`changeTune`/`changePan`. `setParameter` is a stub for `Program.Polyphony`, `OnePole.Freq` and every layer gain. [repo] same file, lines 20–58, 93.
- UVIScript callbacks are cooperative coroutines; `spawn` is deferred to the end of the current instant; `wait` and note-release suspension need sample/beat scheduling. [repo] `docs/FALCON_RUNTIME_UI_GROUNDWORK.md:52`.
- UVI's voice model is per program/layer (see section 1.7 binary strings), not per engine: `Poly` vs `Mono` modes are a program setting, and `ExclusiveGroup` is a layer or program attribute.
- `/home/derpcat/Documents/UVI/Falcon/UVIScript` is an empty directory on this machine: no UVI script manual is available locally. Online docs were not reachable with the queries tried; not verified.

---

## 3. Reference: HISE (open source, C++) [source]

Repo: `/data/dev/workspace/Research/samplers/hise/hi_core`.

- Voice limit: `ModulatorSynth` has `VoiceLimit` (default -1 = unlimited) and `KillFadeTime` default **20 ms**. `hi_dsp/modules/ModulatorSynth.cpp:51-58, 82, 86`.
- Kill order when the limit is reached: `killLastVoice` first looks for voices already tailing off (oldest by uptime), force-kills a voice already being killed with its siblings if no room, then falls back to the oldest by uptime across all notes. `ModulatorSynth.cpp:1689-1760`. `findVoiceToSteal` returns a voice already being killed first. `:1650-1660`.
- Sustain pedal per voice: note-off stops a voice only if neither sustain nor sostenuto is down on that voice. `ModulatorSynth.cpp:1425-1463` (check at 1457). CC64/66/67 dispatch at `:859-861`.
- Event timing: an event with a sample timestamp inside a block is a delayed sound (`delayedSounds`, `delayTimeSamples` counted down per block) and starts at its exact sample in `renderNextBlockWithModulators`. `ModulatorSynth.cpp:509-515, 606-640, 1377-1395`. Note-off by event ID. `:1425`.
- Killing is by event ID with a fixed fade (`killFadeTime`); fade-out at uptime `setFadeOutAtUptime` for fixed-length notes. `:629, 1366`.
- Design intent: HISE keeps per-voice state (uptime, sustain flags, kill state), and the sampler's voice limit is a synth-level setting with a global kill fade. HISE does not have voice groups or exclusion sets in this path [inferred from the functions above; not exhaustively searched].

---

## 4. Reference: sfizz (open source SFZ engine) [source]

Repo: `/data/dev/workspace/Research/samplers/sfizz/src/sfizz`.

- Limits are checked in five ordered levels per trigger: note polyphony, region polyphony, group polyphony, set polyphony, engine polyphony. `VoiceManager.cpp:147-153`.
- `note_polyphony` (per key, per group) with `selfMask`: `dontMask` steals the oldest, `mask` steals the lowest-velocity; target is `note_polyphony - 1`. `VoiceManager.cpp:202-245`.
- Stealers: `FirstStealer`, `OldestStealer`, `EnvelopeAndAgeStealer` (kills a low-power voice older than a threshold; uses average power and age thresholds). `VoiceStealing.cpp` (`stealEnvelopeAndAge`, `genericPolyphonyCheck`). Selected in `VoiceManager::setStealingAlgorithm`, `VoiceManager.cpp:123-142`. Default is Oldest (`:65`).
- Off (choke/exclusion): a region's `off_by` names a group; the victim's `off_mode` is `fast` (release time = `Default::offTime`), `time` (`off_time`), or `normal` (its own release). `Voice.cpp:605-620`. Sister voices of one note are turned off together via `SisterVoiceRing::offAllSisters`, `VoiceManager.cpp:196-199`.
- Note-off honours sustain and sostenuto per region: a voice releases on note-off only if its `checkSustain` / `checkSostenuto` gates are not down, `Voice.cpp:623-650`; CC changes with thresholds re-check at `:652-697` and `sustainCancelsRelease` cancels release on re-press (`:685-695`).
- Event timing: every start/release takes a `delay` in samples; `startVoice` computes `initialDelay_` and the voice's start timestamp from the midi clock. `Voice.cpp:408-440`, `off`/`release` take the same delay.
- Design intent: sfizz treats the SFZ opcodes as the data model, and the engine is a list of voices with a stealing strategy per engine. There is no voice-group concept beyond polyphony groups.

---

## 5. Online references [web]

- **SFZ `off_mode`** (sfzformat.com/opcodes/off_mode): `fast` (default), `normal` (region enters release), `time` (ARIA only; default `off_time` 6 ms). Applies only when `off_by` triggers the shutdown. [web]
- **SFZ `polyphony`** (sfzformat.com/opcodes/polyphony): region or group voice limit; special values `legato_high`, `legato_last`, `legato_low` each set the limit to 1 and keep only the highest, last, or lowest note sounding. [web]
- **SFZ `note_polyphony`**: applies per pitch; default lower-velocity notes do not mute higher ones; examples cite sustain-pedal piano and dulcimer. [web]
- **Decent Sampler**: voice limits are tags (`polyphony` on a tag, applied to groups or samples); `silencedByTags` cuts another group's voices; `silencingMode` `fast` (default) or `normal` (through release); newer `silencingDecay`; `trigger` = attack / release / first / legato / continuous; `releaseTriggerDecay` attenuates releases by held time. [web] decentsampler developer guide, groups and tags pages (versions 1.11–1.15; the group table did not list `silencingDecay`).
- **HISE docs**: the landing page did not expose voice pages; forum threads say voice-limit kills fade with KillFadeTime and that legato uses kill-duplicates plus a voice count of 1. [web] Forum posts only; not authoritative.
- **EastWest Opus / Koda / SFZ manuals**: not reached with the queries tried; not verified.

---

## 6. KONTRA now (fresh engine) [source]

### 6.1 Clock, queue, dispatch
- Single clock `Engine.now`; `process()` pushes every host event with its sample time, then renders to the next due item, so host block size does not change output. `lib.rs:278-311`. Late input applies at once (`:281-284` pushes, `:289` pops `at <= now`). Queue: fixed capacity, ordered by `(time, seq)`, refusal counted, never reordered. `queue.rs:52-92`, `:67-79`.
- Queue capacity is per instrument (`plan.queue = 1024`, `plan.rs:207-208`, `plan.rs:235`) and bounds one block's input plus all pending callbacks, resumes, and releases.
- Dispatch: `Input`, `Resume(co)`, `Release(note)` at `dispatch` `lib.rs:330-347`. `note_off`/`choke`/`expression`/`ump` handlers `:529-708`.
- Span rendering: each span stops at the next queued time or block end, max 256 frames (`MAX_SPAN`, `lib.rs:44-45`, `:292-298`). Voices start and release at that exact frame.

### 6.2 Note-on to voice start
- `note_on` inserts into the ledger, runs the `on note` callback (if any), then `start_pending` starts all zones at `self.now` (`lib.rs:360-380`, `script.rs:207-235`). A callback that waits starts its note at once (`start_pending` runs after `run_callback`, `script.rs:166-177`).
- Zone selection: `start_zones` (`lib.rs:385-447`) checks trigger accept, group mask, key, velocity, keyswitch, CC ranges, bus enabled, round robin. Per selected zone it runs `choke_by` (`:414`), `limit_group` (`:415`), then starts the voice with `VoiceStart { region, step, gain, envelope }` (`:426-434`) and calls `free_slot` (`:436`).
- Trigger First/Legato use "other notes held, not pedal-held" (`script.rs:221-230`). Legato selects zones only; no position carry.
- Gain is set immediately (`voice/src/lib.rs:69-78`, target = gain), with 2 ms one-pole smoothing (`GAIN_SMOOTH`, `:24`). No start ramp.
- Sample offset: `play_note` offset in µs becomes a region start offset in frames (`lib.rs:421-425`, `script.rs:288`). Not a delayed start.

### 6.3 Allocation, stealing, fades
- Fade: fixed `FADE_SECONDS = 0.005` (`lib.rs:46-47`) for steal, choke, group limit, bus disable, and fade_out fallback.
- Choke/exclusion: `choke_by(group)` fades every voice (other notes) whose zone has `off_by == Some(group)` (`lib.rs:450-467`). One direction only; no sets; applied once per matching zone of a note.
- Group limit: `limit_group` fades the oldest-released non-fading voice of the group when the count reaches the zone group's `polyphony` (`lib.rs:470-493`); no note-scoped exemption.
- Instrument limit: `free_slot` fades the oldest-released non-fading voice when `playing >= plan.voices` (`lib.rs:497-510`, one rule for all). If no idle slot exists, it cuts the oldest fading voice with no fade (`:513-522`, `voices_cut` stat).
- Pool: `voices + voices/4 + 1` (`lib.rs:130`), allocated in `new`.
- Victim order everywhere: released first, then oldest start sample; ties go to the lowest slot index (`min_by_key`).
- Plan has no kill mode, no prefer-released flag, no fade time per group, no exclusion sets, no one-shot, no note-polyphony (`plan.rs:116-210`).

### 6.4 Note-off, release, pedal
- `note_off` ignores release velocity (`lib.rs:342`); `on release` callback can `ignore_release` (`:540-546`).
- Sustain: per-channel pedal state (`lib.rs:98-99`, `:689-707`). Notes released while the pedal is down get `pedal_held` and wait; pedal-up calls `release` for them (`:696-700`).
- `release()` sets voices released (`lib.rs:564-590`) and starts release-trigger zones at now (`:581`, `Trigger::Release`); release triggers attenuate by held seconds (`:416-420`). Child notes release with their parent (`:583-588`).
- Choke action (`Event::Choke`) marks the note released and fades its voices without starting release zones (`lib.rs:592-606`).
- `end_silent_notes` keeps a host note alive while release triggers may still play (`lib.rs:712-746`).

### 6.5 Script timing rules
- `wait(µs)` queues a resume at `now + max(1, µs·rate/1e6)` frames; if the queue is full the callback is cancelled (`script.rs:193-205`, the cancel at `:200-203`).
- `play_note(note, vel, offset_µs, duration_µs)`: child note with `Origin::Script`/`Child`; duration queues a `Release` at `now + duration` (`script.rs:267-311`, queue at `:301-306`).
- `fade_out` always stops the voice; `stop_voice` is ignored (`script.rs:336-352`, ponytail comment `:342-343`).
- `ignore_event` on a not-yet-started note marks it ignored (`script.rs:313-325`).
- `EngineUptime` = integer ms of `now` (`script.rs:433-435`).
- `$EVENT_PAR_REL_VELOCITY`, `$EVENT_PAR_PLAY_POS`, `$EVENT_PAR_ZONE_ID` return 0 (`script.rs:354-367`).

### 6.6 Format side [source]
- Kontakt translator: `voices: 256` for the instrument (`translate.rs:34`); group `trigger` from `release_trigger` only (`translate.rs:177-181`); `voice group` only produces an audible diagnostic (`translate.rs:186-188`). No polyphony, off_by, exclusion, one-shot, kill mode, fade, or sustain-mode fields are mapped in the fresh crate.
- Prepare maps group `polyphony` → `plan.polyphony` (`prepare.rs:31-36`, `:73`), `off_by` → per zone (`:245`), `one_shot` → per zone field that nothing reads (`:251`; `plan.rs:151`; no reader in `crates/engine`).

### 6.7 Stats
- `voices_stolen`, `voices_cut`, `notes_refused`, `events_refused`, `missing_frames`, `actions_unhandled`, `messages` (`lib.rs:49-66`). Good for measurement; `events_refused` covers queue overflow but not dropped `pending` notes (see gap 12).

---

## 7. Comparison

| Behaviour | Kontakt 8 | HISE | sfizz | Decent | UVI | KONTRA now |
|---|---|---|---|---|---|---|
| Per-group voice limit | yes, 128 voice groups | per synth | per group, per region, per note | per tag | per program/layer | per group, one rule |
| Steal policy | Any / Oldest / Newest / Highest / Lowest | oldest by uptime, tailing first | oldest, first, envelope+age | not documented | not decoded | oldest, released first |
| Prefer released | yes (default on, Lua) | not seen | via envelope stealer | not seen | not decoded | yes, fixed |
| Fade on steal | 10 ms default | 20 ms default | off_mode fast/normal/time | silencingDecay / fast/normal | not decoded | 5 ms fixed |
| Exclusion | 16 symmetric sets | not seen | off_by directed, sister rings | silencedByTags | ExclusiveGroup | one directed edge per zone |
| Note-polyphony (same key) | no | not seen | yes, selfMask | no | NotePolyphony | no |
| Monophonic release | yes | not seen | not seen | not seen | ReleaseTrigger | no |
| One-shot | Trigger Style | yes | loop_mode one_shot | no | not decoded | field exists, unused |
| Sustain pedal modes | 3 modes | per voice, sustain+sostenuto | per region, sustain+sostenuto | not seen | Hold/Sustenuto/Legato pedal | single channel, one mode |
| Legato position carry | yes (Melody engine, group Legato) | not seen | not seen | legato trigger | recorded transitions | no |
| Release velocity | readable in KSP | not seen | not seen | not seen | not decoded | dropped |
| Sample-accurate start | yes (inferred from manual) | per block with timestamp | per delay | not seen | not decoded | yes, queue |

---

## 8. Gaps, ranked by audible / behavioural impact

Impact: H = audible on common libraries, M = audible in some, L = edge or robustness. Each has the measurement that will prove it.

1. **[H] Kontakt instruments lose their voice structure at translation.** Polyphony, voice-group exclusion, kill mode, one-shot, and fade never reach the plan. A hi-hat choke or a cymbal choke does nothing; instrument polyphony falls to 256 for everything. [source] `translate.rs:34,178-181`; no mapping for `polyphony`/`off_by`/`one_shot` in `crates/format/kontakt/src`. Measure: load a Kontakt instrument with a voice group of 1 across two groups; assert the second voice steals the first (oracle: Kontakt render of the same patch).
2. **[H] No one-shot.** `one_shot` is carried to `Zone` (`prepare.rs:251`) and never read; `note_off` releases every note (`lib.rs:529-561`). Drum hits and one-shot loops release at key-up. Libraries: MODO DRUM (cymbal samples) and any one-shot patch. Measure: zone with `one_shot`, note-off at 0.1 s; output must be unchanged after key-up (Kontakt render comparison).
3. **[H] Layers of one note can steal each other.** `limit_group` and `free_slot` count and select voices without excluding the note's own voices; the tie on `started` goes to the lowest slot index (`lib.rs:470-493`, `:497-510`). A three-layer note in a group with limit 1 keeps one layer, perhaps the wrong one. Per-bus instruments (mic positions) lose a bus. [inferred from code; not run] Measure: M4 below.
4. **[H] One-directional choke only.** `choke_by` cuts only victims whose zone names the new group (`lib.rs:450-467`). Kontakt exclusion is symmetric across 16 sets with the victim's fade; sfizz uses directed `off_by` with `off_mode`; Decent uses `silencedByTags`. Measure: exclusive set of voice groups A and B; play A then B and B then A; both directions must cut.
5. **[M-H] Fixed steal policy.** Only released-first then oldest (`lib.rs:497-510`). Kontakt offers Any / Newest / Highest / Lowest per group with a prefer-released switch, and the victim choice changes the sound in fast passages. Measure: M3 with a Kontakt patch set to Highest.
6. **[M-H] Steal fade is 5 ms fixed and hard cuts exist.** `FADE_SECONDS = 0.005` (`lib.rs:47`), and `voices_cut` (`lib.rs:513-522`) cuts with no fade. Kontakt default 10 ms per group, HISE 20 ms; sfizz `off_mode` chooses. Audible click or mismatch in steal-heavy patches. Measure: envelope decay of a stolen voice in ms against the oracle; `voices_cut` must be 0 for the reference patches.
7. **[M] No monophonic release.** Repeated release samples of one key stack (Kontakt manual, Group Release Trigger). Harpsichord / piano release libraries. Measure: two repeated notes, release-trigger zone; one release voice must sound.
8. **[M] No legato playback continuity.** `Trigger::Legato` selects zones but the new voice restarts at the start marker (`script.rs:225-230`, `lib.rs:421`). Kontakt Legato and UVI/VWinds recorded transitions carry position or play a transition sample. Measure: overlapping notes; compare playhead at the second note against Kontakt Legato on.
9. **[M] Pedal model is one mode.** CC64 is always a sustain pedal and also a controller (`lib.rs:689-707`); no Controller-only or Sustain-only mode; no sostenuto; `ignore_controller` on CC64 returns before the pedal update (`lib.rs:684-687`), which silently disables sustain for scripts that ignore CC64 [inferred from code]. Measure: M6 and a script that ignores CC64.
10. **[M] Release velocity dropped.** `NoteOffVelocity` is ignored (`lib.rs:342`); `$EVENT_PAR_REL_VELOCITY` returns 0 (`script.rs:354-367`). Scripts that trigger releases by release velocity get 0. Measure: KSP `on release` reading REL_VELOCITY against Kontakt.
11. **[M] Input overflow drops silently.** Each block's input shares the 1024-slot queue (`lib.rs:281-284`, `plan.rs:235`). Dense MPE / CC streams can refuse note-ons; they are counted in `events_refused` but the user hears a missing note. Measure: 1200 events in one block; expect no note-on refused, or a reported refusal.
12. **[M] Silent pending drop.** `play_note` inserts a note into the ledger but skips `pending` when the pending list is full (`script.rs:307-309`): the note never starts, and no stat counts it. Measure: fill pending capacity, assert `notes_refused` or a new stat increments.
13. **[M] `fade_out` ignores `stop_voice`.** Always stops (`script.rs:342-343`). Scripts that use `stop_voice = 0` to keep tails get a cut. Measure: KSP `fade_out(..., 0)` render against Kontakt.
14. **[L] Choke sets released without release triggers.** `choke()` (`lib.rs:592-606`) marks the note released; release zones do not fire. Probably right for choke, unverified. Measure: release-trigger patch plus choke event.
15. **[L] Voice start has no gain ramp.** Gain jumps to target (`voice/src/lib.rs:69-78`); offset starts (`play_note` offset) can click at a non-zero sample. Measure: offset start on a sine; check first-sample discontinuity.
16. **[L] Late input has no reported lateness.** Events stamped before `now` apply at `now` (`lib.rs:281-289`) with no stat. Measure: stats counter for late events.
17. **[L] Ledger insert is a linear scan** over capacity (`ledger.rs:150-162`). Fine at 128; note for the 65536 cap.

---

## 9. Coverage limits of this study

- Kontakt library scripts: only Afflatus, ANALOG STRINGS and Areia were dumped. The `dump_scripts` run produced no `.ksp` for Conflux, Dolce, CHORUS, Una Corda, Morphology, Pacific, Shreddage, or Vista. The persistence census shows scripts exist for several of them, so the dumper missed them (cause not investigated). Next step: re-run per library folder and check for skipped messages.
- Kontakt binary: the victim walk and fade law come from the existing decompile notes (`docs/re/engine/k-voice-engine.md`); the decompile export was not re-read here. Open items O1–O3 are still open.
- UVI: no UVIScript manual locally, and the Augmented Orchestra `.ufs` is not decoded.
- HISE: only `ModulatorSynth.cpp` read; `ModulatorSampler` and `KillStateHandler` not read.
- sfizz: `VoiceManager`, `VoiceStealing`, `Voice` read; `SisterVoiceRing` not read in full.
- Online manuals: EastWest, Koda and HISE voice pages were not reached.
- Nothing was built or run. All KONTRA claims are read from source.

---

## 10. Gold standard: the design KONTRA should implement

Principle: **one plan model, one start pipeline, one victim function.** Every format (Kontakt NKI, UVI, SFZ, Decent, EastWest, HISE) translates into this data. Scripts and sampler behaviour go through the same data, not special cases.

### 10.1 Data model (plan, immutable, built off-thread)

- `VoiceGroup { id, max_voices: u16, steal: Steal, prefer_released: bool, fade: Duration, exclusive: Option<ExclusiveSet>, note_polyphony: Option<u8> , one_shot_owner: bool }` with `Steal = Oldest | Newest | Highest | Lowest | Any`. Kontakt: 128 groups, default `Oldest`, `prefer_released = true`, `fade = 10 ms`. SFZ `polyphony` maps to `max_voices`, `legato_*` maps to `max_voices = 1, steal = Highest/Newest/Lowest`. Decent `polyphony` tag maps to a shared voice group.
- `ExclusiveSet(u8)`: symmetric membership, 16 sets (Kontakt). A group in a set cuts every other group in the same set; the victim's `fade` applies. SFZ `off_by` compiles to a directed edge set with `off_mode` (`Fast` = 10 ms, `Release`, `Time(ms)`). The engine stores one edge list `Vec<OffEdge { from, to, mode }>`; a Kontakt set compiles to all pairs. One mechanism.
- `Zone.group: u32` (voice group), `Zone.one_shot` read by note-off. `Trigger { Attack, Release{ monophonic: bool, decay_db_per_s: f32 }, First, Legato }`. Kontakt `t(ms)` stays a modulation source.
- `PedalMode { SustainAndCc, SustainOnly, ControllerOnly }` per instrument (Kontakt), plus `sostenuto: bool` per zone (SFZ).
- `NoteEvent` keeps `release_velocity: f64`; `$EVENT_PAR_REL_VELOCITY` reads it.

### 10.2 Runtime: note start (one pipeline, same sample)

1. Select zones for the note (existing code, unchanged).
2. Resolve the note's voice-group cuts once per note, not per zone: for each group touched, apply exclusive/off edges to voices of *other notes* (never the note's own voices).
3. Apply group limits with the note's own voices excluded from the count and as victims; victims are chosen by the group's `Steal` policy with `prefer_released` first.
4. Apply instrument limit with the same function (`pick_victim(candidates, policy)`).
5. Start every zone voice at the note's sample (existing rule 4). Ties break by note start sample, then voice index; never by slot index alone.
6. Victims fade for `group.fade` (default 10 ms). A fading voice counts only in the fade pool. If the fade pool is full, the oldest fading voice is cut without fade and counted in a stat that the test suite asserts on.

### 10.3 Runtime: note-off and release

- Note-off at sample t: host note marked released (or pedal-held if the pedal rule holds), `release_velocity` stored, `on release` runs at t (existing).
- Release triggers start at t. Monophonic release triggers cut earlier release voices of the same key, all layers, fade `group.fade` (existing ledger identity used).
- One-shot groups ignore note-off for their own voices.
- Pedal: `SustainAndCc` holds per channel; pedal-up at t releases pedal-held notes at t. Release triggers of a pedal-held note start at pedal-up. A script that ignores CC64 does not change the pedal state.

### 10.4 Runtime: script timing (unchanged law, made explicit)

- Callbacks run at their event sample. `on note` notes start at that sample when the callback returns or first waits.
- `wait(µs)` resumes at `now + round(µs·rate)`; `play_note` child notes start at `now`; `offset` is a play position; `duration` schedules a release.
- Musical time is not moved by scripts. Script delay is a `wait` on the event queue, and the event time is kept.
- Queue capacity: separate reservation for host input (one block), script resumes and releases (fixed), and a refusal stat for each source. A note-on that cannot be queued is reported, never silently dropped.

### 10.5 Real-time rules

- No allocation after `Engine::new` (existing rule; keep the alloc-counter tests).
- Victim selection is O(voices) per start and O(group) per group check; group membership kept as an intrusive list or per-group index, not by scanning the whole pool per zone.
- Queue is bounded; overflow is counted per source and visible in stats.
- Sample-accurate: every start/release/fade is an exact frame in the render loop (existing). A fade is a frame count, not a float time, computed once at plan build.
- Deterministic: same input, same victims, same frames, regardless of host block size.

### 10.6 Format translation (what each source provides)

| Source | Provides | Gold field |
|---|---|---|
| Kontakt NKI | voice group (128), max, kill mode, pref.rel, fade, exclusion 0–15, group trigger, one-shot, release t(ms), monophonic, pedal mode | VoiceGroup, ExclusiveSet, Trigger, PedalMode |
| UVI program/layer | Poly/Mono, voice count, ExclusiveGroup, ReleaseTrigger, Legato pedal | VoiceGroup, ExclusiveSet, Trigger, Legato |
| SFZ | polyphony, note_polyphony, off_by, off_mode, off_time, sustain/sostenuto CC, trigger, loop_mode | VoiceGroup, OffEdge, note-polyphony, PedalMode |
| Decent Sampler | polyphony tags, silencedByTags, silencingMode, silencingDecay, trigger, releaseTriggerDecay | VoiceGroup, OffEdge, Trigger |
| HISE / scripting | killFadeTime, voice limit | Instrument fade, VoiceGroup |

---

## 11. Measurements that prove each gold-standard claim

| # | Claim | Test or measurement | Pass condition |
|---|---|---|---|
| G1 | Note-on lands on its sample, any block size | impulse-in at sample k, block sizes 1, 7, 64, 1024, offsets 0–255 | first nonzero frame == k in every run; output bit-identical across block sizes |
| G2 | Same-sample order is deterministic | note-off and note-on at one sample, two orders | same output for same input order; documented order rule |
| G3 | Victim choice matches the policy | Kontakt oracle patch: voice group 2, notes 60/62/64 sustained, each policy | identify surviving voices per policy against the render |
| G4 | A note never steals its own layers | 3 layers, group limit 1; count layers after the note starts | the note keeps the layer the policy chooses, never a random sibling; test fails on current code |
| G5 | Exclusion is symmetric | groups A, B in one set; play A→B and B→A | both cut the other, fade = victim group fade |
| G6 | Steal fade matches the group fade | envelope decay of a stolen voice at 48 kHz | 60 dB in group fade ±1 ms; `voices_cut == 0` on reference patches |
| G7 | One-shot ignores note-off | one-shot zone, note-off at 0.1 s | output after note-off identical to no note-off |
| G8 | Monophonic release keeps one release | two repeated notes, release zone | one release voice sounding; all layers of the first cut |
| G9 | Pedal-up releases at pedal-up sample | CC64 down, note, note-off, CC64 up at sample p | release trigger starts at p, not at note-off |
| G10 | Legato carries position (if enabled) | overlap two notes on a looped sample | playhead at second note start == playhead of first note at that sample (oracle) |
| G11 | Late input does not drift later events | stamp events 2 blocks in the past, then on time | later events on their own samples; late count reported |
| G12 | Queue overflow is never silent | 1500 events in one block with 4 note-ons | every note either sounds or increments a named refusal stat |
| G13 | No allocation in process | existing allocation-counting tests | zero heap activity inside process spans |
| G14 | Steal cost bounded | 256 voices, 10,000 note-ons random | per-start time stays within a fixed budget; no O(pool²) scans |
| G15 | Release velocity reaches scripts | KSP `on release` reading `$EVENT_PAR_REL_VELOCITY` | value equals the host release velocity |

Oracle: Kontakt renders under `tools/kontakt-oracle` (Wine) or the native oracle `docs/re/NATIVE_ORACLE.md`. Do not claim parity without a labelled render (per `docs/re/KONTAKT_VOICE_GROUP_DEFAULTS.md`: "no calibrated native audio recording is claimed").

---

## 12. Recommended order

1. Map the Kontakt voice group, exclusion, one-shot and fade fields in the fresh translator (gaps 1–2). Biggest audible win, no DSP change.
2. Note-scoped counting and one victim function (gaps 3, 5, G4). Fixes layer self-steal.
3. Symmetric exclusion sets and directed off edges (gap 4).
4. Fade per group, no hard cuts (gap 6).
5. Monophonic release and pedal modes (gaps 7, 9).
6. Legato position carry, behind a flag until G10 is measured (gap 8).
7. Queue reservation and refusal stats (gaps 11–12).

Nothing in this document changes DSP or the audible sound of existing patches on its own; items 1–4 do, and each needs its G-test first.
