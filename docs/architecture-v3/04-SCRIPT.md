# 04 Script: one host, two front ends

Today there are three ways to run KSP: the `eval.rs` tree-walker for
`on init`, behavior bytecode for callbacks, and the `luau/` path. UVI Lua
also runs separately, on mlua/Luau on a script thread. That thread makes
scripted notes 1–2 blocks late, and its offline host behaves differently
from real time (PLAYBACK_AUDIT.md). v3 has one scheduler on the audio
thread. KSP compiles to `kontra-vm` bytecode, and Lua runs inline through
the same `HostApi`.

## 1. Layers

```
kontra-ksp  (front end)      kontra-lua (front end)
  lex → parse → sema            Luau chunk, UVIScript API binding
  → lower to vm bytecode        (coroutines = callbacks)
            \                       /
             ▼                     ▼
          kontra-vm:  Program, Instance, Scheduler, HostApi trait
                              │
                              ▼
          kontra-engine:  implements HostApi (notes, engine params, UI store, time)
```

- `kontra-vm` knows no language and no format. It runs bytecode programs and
  schedules coroutines for any front end that implements `Frontend`.
- `kontra-engine` implements `HostApi` once. Both languages call the same
  functions (`play_note`, `set_engine_par`, `wait`, `set_control`, …).

## 2. `kontra-vm`

```rust
pub struct Program { code: Box<[Op]>, consts: Box<[Value]>, callbacks: CallbackTable, layout: VarLayout }
pub struct Instance { vars: Box<[Value]>, arrays: ArrayArena, persist: PersistMap, coroutines: Pool<Coroutine> }

pub enum Step { Done, Wait(Frames), WaitUntil(SampleTime), Yield, Fault(Fault) }

pub trait HostApi {                                   // implemented by the engine, once
    fn now(&self) -> SampleTime;
    fn play_note(&mut self, cx: &Ctx, n: NoteSpec) -> NoteRef;
    fn note_off(&mut self, cx: &Ctx, n: NoteRef, at: Delay);
    fn ignore_event(&mut self, cx: &Ctx, e: EventId);
    fn engine_par(&mut self, cx: &Ctx, target: ParTarget, value: f32, at: Delay);
    fn control(&mut self, id: ControlId, value: ControlValue);   // → LatestCell, never a queue
    fn message(&mut self, msg: MessageRef);                       // index + args; UI formats
    // … generated from builtins! (08-MACROS.md), not hand-listed
}
```

- **Scheduler.** A callback is a coroutine on the engine's `EventQueue`. When
  it hits `wait(n)` it pushes `Work::Resume` for `now + n` and yields.
  Same-sample callbacks run in `seq` order. This replaces the yielded list,
  the deferral queue and fuel.
- **Limits.** KSP's `while` iteration limit works as it does in Kontakt: the
  loop exits and execution continues (#687). There is no per-block fuel and
  no abort-and-release. A runaway Lua callback hits an instruction-count
  hook with the same rule.
- **`on init` runs on the VM** at prepare, off the audio thread, against a
  `HostApi` that records UI declarations and initial engine parameters into
  the plan. `eval.rs` is deleted. A builtin is implemented once and runs the
  same in `init` and in callbacks.
- **Persistence.** `make_persistent` and `read_persistent_var` go through
  `PersistMap`; state save and recall serialize it through `protocol`.

## 3. `kontra-ksp`

```
script/ksp/src/
├── lib.rs            compile(source, Dialect) -> Result<Program, Diagnostics>
├── lex.rs
├── parse.rs          AST (one enum per syntax class)
├── sema.rs           names, types, const folding, families, preprocessor (SET_CONDITION / USE_CODE_IF)
├── lower.rs          AST → vm Op (the only lowering)
├── builtins.rs       builtins! { ... } table: name, signature, HostApi call, init-only?, deprecated?
├── consts.rs         $ENGINE_PAR_* / $EVENT_PAR_* / $CONTROL_PAR_* tables → kontra-params ids
└── ui.rs             ui_* declarations → kontra-ui-model widgets
```

- **One builtin table.** Each row gives the KSP name, its arguments, the
  `HostApi` method it calls and its flags. From it, `builtins!` generates
  semantic checks, lowering and docs, and the Scripts tab lists unsupported
  builtins from that same table. The existing `builtins!` macro is a start;
  v3 extends it to cover lowering too.
- **Constants map to the catalog.** `$ENGINE_PAR_CUTOFF` resolves to
  `kontra-params` `filter::Cutoff` through one table. That replaces three of
  the five parameter registries (00-BASELINE.md).
- **Diagnostics.** Every unsupported builtin or constant gets a
  `Diagnostic { span, code, impact }`. The parity ledger reads them and
  nothing fails silently (memory: script parity workbench).
- **Modern, compatible behaviour.** Behaviour matches Kontakt where a script
  can observe it: event ids, `$EVENT_PAR_*`, ordering and while limits. The
  internals don't copy Kontakt's limits. Arrays grow, there are no 32k caps,
  and MPE and per-note expression are native.

## 4. `kontra-lua`

```
script/lua/src/
├── lib.rs           load(chunk) -> Program-like handle; one Luau VM per instrument
├── api.rs           UVIScript API (Event, playNote, wait, Knob, …) → HostApi, generated table
├── sched.rs         Lua coroutines ↔ vm Scheduler (Resume work items)
└── alloc.rs         arena allocator for the Luau state (no system malloc on audio)
```

- **Inline on the audio thread.** A Lua callback runs at its event's
  sample, just like KSP. `ScriptThread`, the 1 ms poll and the separate
  offline host are deleted. Offline and real time run the same path.
- **Real-time safety.** The Luau state lives in a TLSF arena (`alloc.rs`,
  `rlsf`): `kontra-lua` installs a routing `#[global_allocator]`, since
  mlua 0.12 allocates through the Rust global allocator (option A of
  `spikes/luau-alloc-mlua.md`). A load grows the arena; an audio callback
  never does, and an allocation that does not fit is counted as a spill.
  GC steps run in a bounded slice after the render span.
- **Same scheduler.** `wait(ms)` becomes `Work::Resume` at the exact
  sample. Calling `playNote` from Lua creates a ledger child, exactly as KSP
  `play_note` does.

## 5. UI from scripts

Script UI writes go to `kontra-ui-model`'s control store: one `LatestCell`
per control, written by audio and read by the UI. They never go through a
queue. That removes the `EFFECT_CAPACITY` outbox and the 1024 `ArrayQueue`.
With those gone, 300 `set_control_par` calls per note (Areia) can no longer
stall or abort a note. `message()` stores an index and its arguments; the
UI does the formatting.

## 6. What gets deleted

`sampler-ksp/src/eval.rs`, `sampler-ksp/src/luau/`,
`sampler-core/src/behavior*` fuel and deferral, the UI effect outbox,
`sampler-uvi/src/scripted/thread.rs`, the inline offline Lua host, and
`BehaviorOverload`.
