# Lifecycle, UI mapping and core audit (2026-10-21)

Baseline: `origin/main` at `ad88769da` (no `src/` or `crates/` change since `6955c4e8b`, which the
line numbers below were read from). Scope: instrument load/unload/switch, state recall, UI bindings,
and what a sturdier core would look like.

## How to read this

Evidence level on every finding:

- **TEST** — a test in this PR (or PR #131) reproduces or demonstrates it. Tests marked
  `#[ignore = "known bug …"]` fail today and run in 0.1 s; run them with
  `build-slot cargo test -p kontakto --lib -- plugin::lifecycle_audit_tests --include-ignored`.
- **READ** — I read the full code path myself and the conclusion follows from the source.
- **SUB** — reported by a read-only sub-audit and spot-checked, not independently reproduced.
  Treat the trigger as likely, not proven.

Nothing here was run against a real DAW or a real library corpus. Findings about audibility
("click", "wrong pitch") are inferences from code unless a TEST says otherwise. Not traced end to end:
plugin destroy/deactivate order, convolution IR release, editor open/close/reopen during a load,
KSP script timers across plugin drop. They are listed under *Not covered*.

## Summary

The load path is better than its reputation in one respect: a failed or canceled replacement leaves
the old instrument playing, recall during a load is fenced by `recall_revision`, stale audio/UI
generations are rejected, and the existing transaction tests (`src/plugin.rs:4330`, `:4416`,
`:4506`) pin that down. The defects are elsewhere:

1. Commit is not atomic. `load_part_with` writes about a dozen separately locked cells in sequence
   while holding the `view` lock (`src/plugin.rs:2606-2732`), and other code takes those locks in the
   opposite order.
2. All control-plane work is serialized behind the loader, so a long load stalls the mixer, library
   scans, routing and every other slot.
3. A switch is a wholesale part swap: tails are cut, controller state is not replayed, and nothing
   tells the audio thread that a part was prepared for another sample rate.
4. Intent (what the user picked), observation (what is playing) and persisted state are one
   `Selection`/`PartView` blob with a hand-rolled `attempted` tuple as the only dedup key. Most
   "reload does the wrong thing" bugs are that conflation.
5. Control state has at least six stores and two `ControlId` types. UI mapping bugs
   (dropped UVI XY edits, no host readback) live in the seams between them.

## Findings, ranked

### F-01 High — Commit is non-atomic and lock order is inverted (READ)

`load_part_with` holds `shared.view` from `:2606` to `:2732` and, inside it, takes
`atoms.ingress` (`:2622`, `:2706`), `engine_meters`, `uvi_meters`, `waveforms`, `effect_controls`,
`engine_bindings` (`:2687`), `node_meters` (`:2712`), `controls` (`:2713`), `scripts` (`:2721`) and
`stream` (`:2727`). `PartShared` has 11 `Mutex` fields and `Shared` 9 (`:586-640`, `:986-1066`).

`Shared::refresh_uvi` takes them the other way round: `part.scripts` (`:1941`) then `view` (`:1960`).
It is called from the loader tick (`:2293`, same thread as commit, harmless) **and from host
`pre_save`** via `SamplerParams::capture_ui_controls` (`:474`). A save on the host thread racing a
commit for the same UVI slot with a changed script revision is an ABBA deadlock on the host's main
thread. The window is the whole commit critical section. No test exercises it.

Related: while `view` is held, `*atoms.scripts = loaded.scripts` (`:2721`) and
`*atoms.stream = loaded.stream` (`:2727`) drop the previous values. Dropping a UVI `ScriptThread`
joins the Lua thread (`crates/sampler-uvi/src/scripted/thread.rs:1032-1040`); dropping the last
`Streamer` joins its decode threads (`crates/sampler-kontakt/src/stream.rs:2272-2280`). The UI locks
`view` every frame (`src/ui/mod.rs:423`), so a slow join is a UI freeze. Separately
`capture_ui_controls` holds `atoms.scripts` across `uvi.state()`, which waits up to 2 s
(`thread.rs:1581-1591`).

Because the cells are written one after another, a reader between two writes sees a mixed
instrument (new `controls`, old `node_meters`). The generation checks in `process` and
`set_control_at` mask most of that, which is why it mostly works.

Fix direction: build one immutable `InstrumentSnapshot` off-lock and publish it with a single
pointer store (see Part 3). Interim: document and enforce one lock order (`view` before per-part
cells) and make `refresh_uvi` follow it.

### F-02 High — Control-plane head-of-line blocking behind the loader (READ)

`Load` is `SERIALIZED` (`:2284`). `Load::run` does housekeeping first, then loops over slots calling
`load_part` (`:2335-2344`), and a single `load_part_with` can block for the whole preparation
(`prepare_waiting`, `src/sound/v2.rs:3991-4007`). While it waits, only `service_current_ui`
(script effects and ingress refresh, `:2267-2280`, every 20 ms via `load_wait.rs:9-37`) runs.

Everything else lives only in the loader tick and therefore waits:

- `push_mix` (`:2307-2332`) is the **only producer** of `Shared::controls` (`:2330`). The `Mix` carries part
  volume/pan/mute/solo/output routing, editor engine offsets, articulation routes and per-node mixes
  (`src/sound/mix.rs:64-72`). Mute or fader moves made during a load reach the audio thread after it.
- Library scan start/finish is consumed in `poll_libraries` (`:2304`, `Scanner::poll` at
  `src/library.rs:1555`).
- `route`, `align`, `trim_streams`, `discard` drain (`:2296`), `flush_ready` (`:2288`).
- Every other slot's load (they run one after another in the same `run`).

The audio thread never blocks, so it keeps playing; the symptom is a UI that acknowledges input
and does nothing for the length of the load. Not measured: load durations on real libraries.

### F-03 High — UVI XY pads and multi-cell edits are silently dropped (READ)

An XY drag pushes two edits per frame, indices `2c` and `2c+1` (`src/ui/ir_view.rs:2173-2189`). The UVI
dispatch accepts only `edits.len() == 1` with index 0 (`src/ui/part.rs:845-849`, `_ => false`). The
control id was already inserted into `edited_controls` (`:833-835`), so the scalar fallback at
`:861-874` is suppressed too, and the draft is discarded (`:857-859`) with no notice. The XY binding
is only `components[0]` (`crates/sampler-uvi/src/script/ui.rs:1068-1076`). Existing widget-level tests
(`src/ui/v2_tests.rs:4053-4150`) bypass this dispatcher.

This is a concrete instance of "UI not mapped correctly": the widget draws, accepts the gesture, and
the gesture goes nowhere. Cheap reproduction needs a UVI `UiSnapshot` fixture
(`src/plugin.rs:5369-5456` has one); the fix needs a UVI batch/component edit API, so it is not in
this PR.

### F-04 High — Stale `control_values` follow a preset replacement (TEST, fixed in #131)

`replace_part` (`src/ui/mod.rs:849`) cleared everything but `Part::control_values`. KSP control ids
are `fnv("ksp/<slot>/<variable>")` (`crates/sampler-ksp/src/lib.rs:937`), so two scripts declaring
`$volume` share one id, and the loader feeds the saved values to the new preset's `on init`
(`src/plugin.rs:2544-2548` → `src/sound/v2.rs:3691-3714` → `crates/sampler-kontakt/src/load.rs:939`).
`select_snapshot` already cleared them (`src/plugin.rs:196-198`), which shows the intent.
Fix and test: PR #131 (`ui::tests::replacing_a_part_drops_the_old_presets_saved_controls`).

Sub-audits also flagged `script_state` and `uvi_state` surviving replacement. Both are guarded by a
`*_source` identity at load (`:2521-2522`, `:2572`), so they do not leak across presets; they do
resurrect A's last **host-saved** state on A→B→A (F-10).

### F-05 High — Parts keep their old rate across a sample-rate change (TEST)

`V2Core::reset` stores the new rate and calls `panic()` (`src/sound/v2.rs:2730-2744`); its comment
says parts keep their prepared rate and the shell reloads them. Nothing in `render` compares
`part.runtime.sample_rate()` with the core rate (`rg` finds no such guard), so until the reload
completes, new notes play the old-rate material at the new host rate. A host that activates at 48 kHz
and starts an offline bounce immediately can render the whole start of the file wrong.
Test: `plugin::lifecycle_audit_tests::a_part_prepared_for_another_rate_is_silent_until_it_is_reloaded`
(ignored; `reset(44100)` on a 48 kHz part, note 60, output is non-zero).
Fix direction: carry the prepared rate in `Part`, render silence (or a ramp) when it differs from the
core rate, and show "Loading…" for the slot.

### F-06 Medium — Re-selecting the active preset after a canceled or failed switch reloads it; failed loads are sticky (TEST)

The only dedup key is `PartView::attempted` (`:1080`, checked at `:2461`). After A→B(canceled)→A the
cancel arm clears it (`:2738`), so A, which is still playing, is rebuilt: new generation, voices cut
(`install`, F-08). After a **failed** switch `attempted` stays on the failed target (`:2742-2747`),
so re-selecting the failed preset does nothing (by design, tested at `:4375`), and re-selecting the
old one rebuilds it. Meanwhile the selection, the picker title and the next/previous arrows
(`src/ui/rack.rs:1168-1181`) sit on the failed path, the header name shows the active instrument
(`rack.rs:22-33`), and a host save records the failed path with `capture_ui_controls` skipping the
slot (`:1649-1660`).

Tests (ignored): `returning_to_the_active_source_after_a_canceled_switch_does_not_reload`,
`returning_to_the_active_source_after_a_failed_switch_does_not_reload`. A passing control
(`an_unchanged_source_is_not_reloaded`) shows the plain duplicate request is already a no-op.

Fix direction: derive "needs load" from `intent != active` (plus an explicit recall/force flag), not
from a history tuple (Part 3, `SlotIntent`).

### F-07 Medium — One bad saved-state blob makes a preset unloadable (READ + SUB)

Oversized (>8 MiB) or malformed `uvi_state` returns `Err` before preparation (`:2521-2533`,
`:2563-2568`); malformed script state fails at `:2577` and in
`src/sound/v2/persistence.rs:469-476`. The result is "Load failed" with `attempted` retained, so
every host recall of the project fails identically until the user picks another preset. There is no
fallback to loading with default state plus a diagnostic. Save has no byte cap
(`:1674-1677`) while load enforces 8 MiB, and `validate` caps counts, not bytes
(`crates/sampler-uvi/src/script/ui.rs:1475-1485`), so a script that stores a few MiB of `Bytes` can
write a project that cannot reopen (SUB, size trigger inferred). A failed same-source recall also
leaves `recalled_source` set (`:2736-2746` never clears it), which blocks live capture on later saves
(`:1649-1656`) (SUB).

### F-08 Medium — The swap cuts tails, orphans held notes and replays no controller state (READ)

`V2Core::install` (`src/sound/v2.rs:2746-2774`) replaces the part wholesale. Voices and runtime-owned
bus/FX tails of the old part vanish in one block (inferred click); held keys become `ORPHAN`
(`:2751-2753`) so the old part's note-offs and `onRelease` never run; CLAP orphans do get `NOTE_END`
(`:2966-2979`; I found no stuck-note path, SUB). The new part starts with a fresh receiver
(`crates/sampler-midi/src/mpe.rs:17-24`) and nothing replays CC64/CC66, bend, timbre or RPN state, so
host sustain or bend can disagree with the part until the host resends. Also, the UI sees the new
instrument at commit (`:2677`) before audio installs it through the `ready` queue (`:3397-3415`);
generation gating (`:3373`) keeps values consistent but the UI shows a state audio is not yet in.

### F-09 Medium — Host automation: no generation check, no readback, names from the first part (READ + SUB)

`set_host_parameter_at` (`:1762`) is the epoch-checked API but has no production caller. Real host
parameters go through `V2Core::host_parameter` (`src/sound/v2.rs:1417-1439`), which broadcasts to
every installed part with no generation check. `automation::slots` takes names/ranges from the first
part only (`src/plugin/automation.rs:128-140`, with a `ponytail:` note) while delivery uses each
part's own range (`src/sound/v2.rs:267-295`). Nothing emits output parameter changes, so after a
switch or a UI edit the host's automation lane keeps its old value (SUB, by grep). `LoadRequest`
carries no host-parameter values and `install` replays none, so a fresh part starts at defaults
until the host sends an event. Slot names and routes do republish after a switch (`:1463-1464`).

### F-10 Medium — Live persistence is only captured on host save (READ + SUB)

`capture_ui_controls` is called from `pre_save` (`:424`) and multi-save (`src/ui/header.rs:581`),
never on a switch. After A→B→A the reload restores A's last host-saved `script_state`
(source-guarded, `:2572`), not A's live state at switch time. Same for `uvi_state`.

### F-11 Medium — One unknown KSP command discards the whole script slot (READ)

`sema.rs:361-362` and `:849-853` fault on any identifier not in the builtin table;
`load.rs:1067-1121` then drops that slot's script and records one `unsupported` entry. The names in
the audit brief (`load_ensemble`, `load_patch`, `load_snapshot`, `save_snapshot`) are not in
`crates/sampler-ksp/src/builtins.rs` or the manual-derived `KSP_SURFACE.json`; whether Kontakt 8
accepts them is unverified here. The real corpus hits this class: `docs/audit-2026-10-08/ksp-sema-faults.md`
records 45 faulted script slots across 30 multi items from `subscribe_async` (30) and `mf_get_first`
(15). `allow_group`/`disallow_group` are native only in note/release callbacks; elsewhere they warn
and do nothing (`lower.rs:2179`, `:3042-3044`; asserted by `crates/sampler-ksp/tests/groups.rs:185-205`),
and `KSP_COVERAGE.md:60-61` says "Native" without that qualifier. This is the main reason
"patch switching inside the instrument" does not work for scripts that use these.

### F-12 Medium — A corrupt (present) sample aborts the whole instrument (SUB)

Missing files degrade to "N not translated" (`library.rs:647-654` → `:2666-2670`). A present file
with an unreadable header returns `Err` from `samples.rs:614-626` through `?` at `library.rs:661`
and `load.rs:198`/`:288-290`, so one bad sample fails the load and the old report stays. The same
kind of fault (corrupt record) should degrade like a missing one and be listed in the UI.

### F-13 Medium — UVI has no in-patch program/layer switch (SUB)

No `loadProgram`-style API exists (`crates/sampler-uvi/src/script_prelude.lua:438` returns nil for
unknown members; `programChange()` only emits MIDI, `script.rs:2924`). Layer/keygroup edits are
overlays on the live graph. Only a header/browser selection can change program, via a full load.
Same-owner snapshot reuse (`src/plugin/snapshot_reuse.rs`) is Kontakt-`.nki`-only
(`snapshot_base`, `:189-194`). This is a feature gap, listed because the audit brief expected it.

### F-14 Low-Medium — Rapid snapshot clicks collapse; UI shows a snapshot audio never loaded (SUB)

The picker computes next/previous from the committed `part.snapshot` (`rack.rs:1168`);
`queue_snapshot` overwrites a single pending request (`:1470-1495`) and the selection only changes at
validation (`:2216`). Two clicks before validation request the same target. The post-validation
`select_snapshot` (`:2216`) runs before commit, so a failing snapshot leaves the dropdown on it.

### F-15 Low — Unload does not reset all per-part cells (READ)

The empty-path branch (`:2474-2500`) clears ingress, meters, waveforms, stream and byte counts but not
`controls` (`:602`), `scripts` (`:614`), `engine_bindings` (`:607`), `node_meters` (`:611`),
`effect_controls`, `articulation` or `snapshot_pending`. Persistence is gated by `active_source`, so
nothing is saved from them, but readers (`display_values`, `control_values`) can return the old
instrument's values and a UVI `ScriptThread` is kept alive until the next load. The unload test
(`:4157-4174`) asserts view fields only. Unload and switch release resources by different code.

### F-16 Low — Teardown and failure containment (SUB)

- `Shared::drop` only shuts the picker runtime (`:1155-1158`). `canceled()` has no stop flag, so a
  running load finishes after destroy; the last `Arc<SamplerParams>` can drop on a task-pool worker,
  where it joins the Lua thread, the grower and the picker (`src/sound/v2.rs:256-263`).
- The UVI script thread has no `catch_unwind` (`crates/sampler-uvi/src/scripted/thread.rs:412`). A
  panic or the early `break` at `:478` leaves the instrument silent with no status.
- `attempted`/`loading`/"Loading…" are set at `:2461-2469` with no guard. moose catches handler
  panics (`moose-core/src/tasks.rs:157-165`), but a panic inside `load_part_with` leaves the slot at
  "Loading…" and the dedup key blocks a retry. The early return at `:2608-2610` also skips resetting
  them; it is unreachable today (generation only changes inside the serialized loader).
- Commit order matters under a panic: generation is bumped (`:2623`) before the view and cells are
  rewritten and long before `publish_part` (`:2733`), so a panic in between leaves the UI on a
  generation audio never receives.
- WAV loads and header reads are not cancelable (`src/sound/v2.rs:3923`, `:3671-3714`); cancel only
  wastes work.

### F-17 Low — UI plumbing seams (READ + SUB)

- Two `ControlId` types (`sampler_core::ControlId`, `sampler_ui_ir::ControlId`) converted by `.0`
  (the compiler stopped me writing a test across them).
- Refused scalar edits are silent: `let _admitted` at `src/ui/part.rs:866`; the knob snaps back after
  one frame.
- `cx.state.faces` is inserted and never evicted (`part.rs:470`), so art and native UI state of
  removed parts persist; `refresh_generation` resets `uvi_snapshot` (`part.rs:114-135`).
- `submit_ui_widgets` fallback ignores `source_slot` (`src/sound/v2.rs:799-801`); a part with several
  scripts sharing an explicit control id could route an edit to the wrong widget (SUSPECTED).
- Readback: the `epoch == generation` gate is right (`:3373`, `:3596-3603`), but `refresh_controls`
  uses `try_lock` (`:685`) and stores the revision marker regardless, so a contended lock defers the
  readback to the next 100 ms poll; engine meters update only on that poll (`:3375-3378`).

### Things I checked that are fine

- Canceled and failed replacement keep audio, view, controls and ownership (`:4330`, `:4506`).
- Recall during preparation: `recall_revision` fences it (`:2512`, `:2613`, test `:4416`).
- Host save during an in-flight switch does not write the old instrument's values into the new
  source (`:1660`, test `:4330`).
- Orphaned CLAP notes end exactly once and refusals are retried (`src/sound/v2.rs:2966-2979`, test
  `:5288`).
- Missing sample files are reported, not fatal; the status line carries the count (`:2666-2670`).
- A panic in `Load::run` does not kill the loader lane (moose catches it).

## Part 2 — UI mapping

What exists: the health model classifies a widget as unbound when it is interactive and its binding is
not a `Control` in the script's control set (`src/ui/health.rs:69-104`); drivers are
`tools/corpus-health/ui-run.sh` → `ui-summary.py`, `crates/sampler-uvi/examples/ui_health.rs`,
`tools/library-inspector.html` ("Interactive without binding"), and
`tools/corpus-health/preset_switch_probe.py` (KSP edits after a switch). The last published numbers
are partial: Kontakt 39/40 usable and 27/40 complete on a sample (`KONTAKT_UI_HEALTH.md:67-72`),
UVI baseline 0/392,199 bound widgets over 239 programs with the post-fix census pending
(`UVI_UI_REPORT.md:18-23`).

What this audit adds, from code only:

| Class | Where | Finding |
| --- | --- | --- |
| Gesture dropped | `part.rs:845-849` | F-03: UVI XY and multi-index edits |
| Value leak across switch | `ui/mod.rs:849` | F-04 (fixed in #131) |
| Not reflected back | `automation.rs`, `v2.rs:1417` | F-09: no output param changes; first-part names |
| Silent refusal | `part.rs:866` | F-17: refused knob edit gives no notice |
| Stale/pending vs active | `rack.rs:22-33`, `:1168` | F-06, F-14: name from active, title/arrows from selection |
| Not evicted | `part.rs:470` | F-17: faces survive part removal |
| Unbound classification | `health.rs:69-104` | TextEdit, FileSelector, Table, MouseArea with variable bindings count as unbound, so the health score under-reports usability for those |

Script-created UI: KSP runtime-created widgets (`create_ui_*`) have no hits in `crates/`, `src/` or
`docs/` (rg), so I could not assess them. UVI dynamic widgets are covered by
`src/plugin.rs:5334-5363`.

I could not enumerate the "many places" the brief refers to from code reading alone. The next step
that scales is to run the existing UI health and preset-switch probes over the corpus on the machine
that has the libraries and rank unbound/mismatched widgets by class; the table above is the set of
mechanisms to expect, not a census.

## Part 3 — A sturdier core from first principles

### What the code is today

Threads: host/audio (`Sampler::process`), the serialized `Load` lane, one scoped `sampler-load`
worker per load (16 MiB stack, `src/sound/v2.rs:3969-4007`), the UI frame thread, a Lua thread per UVI
part, stream decode threads, a grower thread per part, the library scanner, the waveform provider,
the picker runtime.

State: `Selection` (persisted intent, `RwLock`), `View`/`PartView` (published observation, one
`Mutex<View>` cloned per frame), `PartShared` (11 mutexes per slot), `Shared` (9 mutexes; roughly 47 atomics across the two), and the audio-side `V2Core`. "Which version is current" is
tracked by `generation`, `recall_revision`, `snapshot_sequence`, `scalar_revision`,
`native_revision`, `ui_revision`, `core.epoch`, `PlanId`, `port_names_revision`,
`host_presentation_revision`, `ResourceRevision` and the scanner's `wanted/started/stamp`. Lock
ordering is by convention (comments such as `:1446`). `Load::run` is a 68-line tick plus a
323-line `load_part_with`; `process` is 360 lines.

What is good and must survive: the audio side is allocation-free and tested as such; hand-offs to
audio are bounded SPSC queues (`ready`, `discard`, `controls`, `keyboard`, `ControlClient`); the
engine crates (`sampler-core`, `sampler-ir`, loaders) are separate from the shell; candidates are
prepared privately and only committed on success; generation stamps already exist; the
transaction tests are an excellent characterization suite.

### Principles

1. **Intent, observation and persistence are different things.** Intent is what the user/host asked
   for; observation is what is installed and audible; persistence is what is saved. Today they share
   one struct and are reconciled by comparing tuples.
2. **One immutable snapshot per loaded instrument, published with one pointer store.** Readers never
   see half an instrument. Everything the commit currently assigns field by field becomes a field of
   the snapshot.
3. **One owner for each resource, dropped by one reaper, never under a lock.**
4. **One store per piece of control state, with a typed edit path.** Readers get atomics or
   snapshots, not locks.
5. **Failures are contained at slot level and leave a consistent, diagnosed slot.**

### Lifecycle state machines

**Plugin** (owner: host wrapper; thread: host main)

| State | Entered by | Allowed | Notes |
| --- | --- | --- | --- |
| `Created` | instantiate | init params, spawn control-plane tick | no audio resources |
| `Ready(rate, max_block)` | activate/`reset` | install parts, process | `rate` is a first-class epoch (F-05) |
| `Processing` / `Idle` | host start/stop | — | audio thread only reads |
| `Destroying` | destroy | cancel all tickets, join workers | single cancel token, reaper flushed |

**Slot** (one per rack slot; owner: the slot coordinator, i.e. the control-plane, not the UI)

```
Empty ──intent(src)──▶ Loading{ticket}             (no active)
Active{snap} ──intent(src')──▶ Active{snap}+Loading{ticket}
Loading ──ok(ticket current)──▶ Publishing{ticket}  ──audio ack──▶ Active{snap'}
Loading ──err / canceled──▶ Active{snap}+Failed{err}   (or Empty+Failed)
Active ──intent(empty)──▶ Retiring ──ack──▶ Empty
any ──intent == active source (no force)──▶ no-op          (fixes F-06)
any ──rate change / host recall──▶ intent(force=true, new epoch)
```

Invariants: at most one live ticket per slot; a commit whose ticket is not the latest is discarded;
`Active` always names exactly one `InstrumentSnapshot`; `Failed` never changes `Active`; the audio
side reports the installed ticket back, and the UI shows `Publishing` until then (closes the
commit→install window of F-08).

**Instrument snapshot** (immutable after `Built`)

`Building (loader worker) → Built → Published(ticket) → Installed (audio ack) → Retiring (audio
drops it) → Released (reaper)`. Resources hang off the snapshot: runtime `Part`, streams, preload
memory, convolution IRs, script threads, art. Release is the snapshot's `Drop`, executed only by the
reaper thread.

### Who runs what

| Thread | Does | Never does |
| --- | --- | --- |
| Audio | install a ready `Part` by ticket; render; apply typed control edits; ack `installed_ticket`; refuse parts whose rate ≠ core | lock, allocate, drop snapshots |
| Control plane (one non-blocking tick, ~100 Hz) | mix push, routing, align, scans, discard/reaper drain, intent → ticket, host automation tables | prepare instruments, wait on loaders |
| Loader worker(s) | build a snapshot for a ticket, cancel-aware; decode state with fallback | publish partially, touch `View` |
| UI | write intents; read `Arc<Snapshot>` + atomic control mirror; optimistic overlay keyed by `(ticket, id)` | take mutexes held by commit, drop resources |
| Reaper | drop retired snapshots, join script/stream threads, free IRs | run while holding any shared lock |

Splitting the control-plane tick from the loader removes F-02 outright. Admission control (memory
budget, concurrent loads) is a counter on the coordinator, not a reason to serialize everything.

### Control state: one source of truth

Authoritative value lives in the audio-side runtime (it already is: `Runtime` control values with
revisions, `docs/architecture-v2/CONTROL_STATE.md`). Everything else is a mirror:

- Writers (UI, host automation, script, recall) submit `ControlEdit { ticket, id: ControlId, value,
  origin }` through the existing `ControlClient` SPSC; stale tickets are rejected at the boundary.
- The audio side publishes values at block end into a per-snapshot `[AtomicU64]` mirror (no 100 ms
  poll, no `try_lock`), plus an output parameter queue for the host (F-09).
- One `ControlId` type, namespaced by instrument identity so saved values are keyed `(source, id)`
  and can never apply to another preset (F-04 by construction).
- The host automation table is part of the snapshot: slot → (control, range, name). Installing a
  snapshot swaps the table atomically; the host sees one coherent set.
- UVI component edits get a first-class `ControlEdit::Components` (F-03) instead of being squeezed
  through the scalar path.

### Resource ownership

One rule: **a snapshot owns what it loaded and nothing else does.** Today ownership is spread over
`Part`, `PartShared.stream/scripts/…`, `PartView.*` and the library scanner. Unload and switch must
call the same retire path (F-15). No resource is dropped while a lock is held (F-01); joins happen on
the reaper. A `Drop` counter in tests asserts no leaked part after N random operations.

### Failure containment

- Build and commit run under `catch_unwind` at slot level; a panic sets `Failed` and keeps `Active`.
- State blobs decode with fallback: bad `uvi_state`/`script_state` loads defaults and a diagnostic,
  never "Load failed" (F-07). Saves are bounded to what loads accept.
- Corrupt records degrade like missing ones and appear in the same report (F-12).
- Script/Lua owner liveness is observed (heartbeat/flag); a dead owner marks the slot degraded in
  the UI instead of silence (F-16).
- Destroy cancels via one token, and the last-reference drop happens on the reaper, not a pool worker.

### Keep / rewrite

| Keep | Rewrite |
| --- | --- |
| `sampler-core` runtime, ownership, control crate, DSP; `sampler-ir`; Kontakt/UVI readers | `Shared`, `PartShared`, `PartView`, `View` → `Rack`, `SlotCell`, `InstrumentSnapshot` |
| `V2Core` structure, `ready`/`discard`/`controls` rings, `ControlClient`, `install` (extended) | `load_part_with` → `SlotCoordinator` (intent, ticket, publish) |
| Same-owner snapshot reuse as an optimization behind a ticket | `refresh_uvi`, `capture_ui_controls`, `reload_ui_controls` → snapshot-keyed capture/recall |
| `Selection` as persisted host state | `Selection` split into `Persisted` and `Intent`; header writes only `Intent` |
| The transaction tests | `attempted`, `recalled_source`, `recall_revision` → `ticket` + `force` |

### Migration order (incremental, behind the existing tests)

Each step is a PR that keeps `cargo test -p kontakto --lib` green and removes or ignores one
characterization test as it lands.

0. **Done here:** characterization tests (`lifecycle_audit_tests`), F-04 fix (#131).
1. **Rate guard (F-05).** Carry prepared rate in `Part`; silent when different. Small, no structure
   change. Flip the ignored test.
2. **Intent vs active (F-06).** Add `active_identity` to `PartView`; skip load when intent equals it
   and no force. Recall and rate change set `force`. Flip two ignored tests.
3. **State machine as data.** Add `SlotPhase` and `ticket` to `PartView` (set where `attempted` is set
   today), show it in the UI ("Loading", "Failed – kept previous"), derive `attempted` from it. No
   behavior change beyond UI text.
4. **Split the loader tick from loading (F-02).** Move `push_mix`, `route`, `align`, scans, discard
   drain into a control-plane tick that never waits; keep per-slot loads serialized for now. Test:
   mixer edit during a blocked `prepare` closure reaches `controls` within one tick.
5. **`InstrumentSnapshot` + single publish (F-01, F-15).** Gather the fields assigned at
   `plugin.rs:2631-2733` into one struct behind `Mutex<Arc<_>>`; per-part cells become accessors; the
   unload branch builds an empty snapshot through the same path; drops move to the reaper. Delete the
   `refresh_uvi` lock inversion by construction. Add a seeded random-ops model test (switch, cancel,
   recall, rate change, unload, save/restore) asserting: UI-active equals audio-installed after
   quiescence, generations monotonic, no leaked parts, reselect-active is a no-op, save→restore is
   idempotent. It needs no new dependency; the injected `prepare` closure and `transaction_candidate`
   already make it fast.
6. **Audio-acked install and tail handover (F-08).** `installed_ticket` atomic; keep the outgoing part
   for a short fade/tail instead of dropping it; replay sustain/bend/RPN from a host-controller
   mirror.
7. **Typed control layer (F-03, F-09, F-17).** Single `ControlId`, namespaced saved values, atomics
   mirror, output parameter queue, UVI component edits, notice on refused edits.
8. **Containment (F-07, F-12, F-16).** `catch_unwind` per slot, state-decode fallback, corrupt-record
   degradation, script-owner liveness, destroy token.

Measure each step with the preset-switch probe (`tools/corpus-health/PRESET_SWITCH.md`) and the
existing CPU/allocation gates; success criteria: no `Mutex` left in `PartShared`; a mixer edit during a
load reaches audio within 50 ms; a switch under a held chord has no discontinuity above a stated
threshold; zero lock-order cycles in a loom-free ordered-lock test; no panic leaves a slot at
"Loading…".

## Not covered

Plugin deactivate/destroy order beyond what is cited; convolution IR and stream release timing;
editor open/close/reopen during a load (only the face-retention seam was read); KSP timers across
plugin drop; real-host behavior for any finding; load durations and memory peaks (both the old and
new instrument are resident during a switch, which is a budget question this audit did not
measure); whether Kontakt 8 accepts the brief's `load_ensemble`/`load_patch` names.

## Reproduction

```sh
build-slot cargo test -p kontakto --lib -- plugin::lifecycle_audit_tests --include-ignored   # 3 expected failures
build-slot cargo test -p kontakto --lib -- ui::tests::replacing_a_part   # PR #131
```

Observed on this branch: `an_unchanged_source_is_not_reloaded` passes; the other three fail on the
assertions quoted above. Incremental test build 38 s, run 0.1 s.
