# Universal script layer (`sampler-script`)

Status: phase 1, 2026-10-23. Owner: script lane. Crate: `crates/sampler-script`.

## Decision

One format-neutral Luau runtime owns VM lifecycle, sandbox, compiler policy,
work budgets and GC budgeting. Format crates are front ends: `sampler-uvi`
(Lua 5.1-compatible UVI API surface, today the only consumer) and a KSP front
end that will compile KSP to Luau source. Front ends never create a VM.
Runtime choice: **Luau** (mlua 0.12.2, `luau` interpreter; `luau-jit` opt-in
per VM). Evidence below; LuaJIT and piccolo were measured or checked and rejected.

## Layers

| Layer | Where | Owns |
|---|---|---|
| Runtime | `crates/sampler-script` | `new_vm(memory)` (library set: table, string, math, coroutine, bit; no io/os/package/debug), `compiler()` policy, `compile_bytecode`, `check_syntax`, `Budget` (checkpoints, remaining work, exhaustion, wall deadline as observation), `bounded_gc_step` |
| Front end (UVI) | `crates/sampler-uvi/src/script.rs` | UVI API globals, widgets, `wait`/`spawn` scheduler, `playNote` mapping, prelude (`script_prelude.lua`) |
| Front end (KSP) | planned, `crates/sampler-ksp` | KSP-to-Luau compiler (not started in this pass, see Remaining) |
| Engine | `sampler-core` | Unchanged. Scripts emit commands; the engine applies them on its own thread |

Migrated in this change: VM creation (`Lua::new_with` + memory limit) and the
work budget (`remaining_work`, `work_exhausted`, `vm_checkpoints`, `deadline`
become `Shared::budget: Rc<sampler_script::Budget>`). The interrupt closure and
the wait queue stay in UVI for now (see Remaining).

## Budgets and GC

- Work is counted in Luau interrupt checkpoints (calls and loop back-edges).
  Exhaustion aborts the callback with `work budget exceeded`. Deterministic,
  independent of scheduler timing. Wall deadline is observation only.
- Memory: `set_memory_limit`, unchanged from UVI (`Config::memory`).
- GC on the audio-safe path: `bounded_gc_step(lua, n)` runs at most `n`
  incremental steps and reports cycle completion. Scripts never run on the
  audio thread; this exists so control-thread GC can be sliced.

## Engine API table (format-neutral, target)

| Concept | Neutral name | UVI today | KSP today (`sampler-ksp`) |
|---|---|---|---|
| Note start | `play_note` | `playNote(note, vel, dur, layer, ch, input, vol, pan, tune, slice, oscIndex)` | `play_note` |
| Note end | `release` | `Runtime::release_at`, `duration=-1` | `note_off`, `on release` |
| Wait | `wait` | `wait`/`spawn` coroutines | `wait(ms)` coroutine suspension |
| Parameters | `set_param`/`get_param` | `setParameter`/`getParameter` | `set_engine_par` / `get_engine_par` (returns -1 on unknown) |
| Modulation | `send_mod` | `sendScriptModulation` | `set_event_par`-style / `$CONTROL_PAR` writes |
| MIDI | `post_event` | `postEvent` | `ignore_event`, `set_event_par` |
| Timers | `now` | `getTime` | `$EVENT_PAR_PLAY_POS` (microseconds) |
| UI | `ui_*` hooks | widget constructors | `ui_*` declarations, `set_control_par` |

Callbacks: `onInit`/`on init`, `onNote`/`on note`, `onRelease`/`on release`,
`onController`/`on controller`. KSP semantics (integer/real typing, polyphonic
note-owned variables, message-passing rules) belong to the KSP front end; the
runtime only provides the VM and budgets.

## Runtime survey (2026-10-23)

Candidates checked: mlua backends (Luau interp, Luau+JIT, LuaJIT, PUC Lua 5.1),
piccolo, and the Luau bytecode cache. Sources: crates.io API, mlua 0.12.2
source (`~/.cargo/registry`), [mlua README](https://github.com/mlua-rs/mlua),
[piccolo README](https://github.com/kyren/piccolo),
[piccolo docs.rs](https://docs.rs/crate/piccolo/latest).

| Candidate | Verdict | Reason |
|---|---|---|
| **Luau interp** (mlua `luau`) | Chosen (baseline) | Sandbox, interrupt checkpoints, memory limit, bytecode load. Lua 5.1 derivative; 660 UVI scripts compile. |
| **Luau + native codegen** (mlua `luau-jit`) | Opt-in per VM | Fastest callbacks of the Luau modes, but codegen runs at load: ~7.6 ms extra per VM in our measure. Keep off for init-heavy VMs. |
| **LuaJIT** (mlua `luajit`, vendored) | Rejected for now | Fastest in the micro-bench and true Lua 5.1, but no Luau sandbox: globals and `io`/`os` must be stripped by hand; the checkpoint budget needs a count hook; no wasm target; a second Lua runtime in the binary. Our 660-script corpus already runs on Luau. |
| **PUC Lua 5.1** (mlua `lua51`) | Rejected | Slower than LuaJIT and no advantage over Luau for us; same hook and sandbox issues. |
| **piccolo** 0.3.3 (pure Rust, stackless, fuel) | Rejected | README: `string`, `table`, `io`, `os`, `package`, `utf8` "missing or very sparsely implemented"; no `math` mention. UVI scripts need string patterns and `table.insert`. Last crates.io release 2024-06; API unstable pre-1.0. Fuel model is attractive, but the stdlib gap blocks it. |
| Wasm (wasmtime/wasmi) | Not evaluated | Would need a script-to-wasm compiler; no evidence it beats Luau for this workload. Out of scope for phase 1. |

## Measurements

Harness: `/data/dev/workspace/scratch/lua-bench` (scratch, not in repo). Same
workload per engine: load = create VM and run the real UVI prelude
(`script_prelude.lua`, 13 KB); event = `onNote(note, vel)` with 16 table slots
and an 8-step arithmetic loop, 200,000 calls, best of 5. Host shared with other
agents: expect ±30 %. Two runs per engine shown as ranges.

| Engine | VM create | Prelude load (source) | Prelude load (bytecode) | onNote per call |
|---|---|---|---|---|
| Luau interp | 0.06 ms | 2.1 ms | **0.18 ms** | 327 ns |
| Luau + JIT | 0.22 ms | 9.2–9.4 ms | 7.5–7.8 ms | 72–125 ns |
| LuaJIT | 0.05 ms | **0.57–0.60 ms** | n/a | 50–66 ns |
| LuaJIT + count hook (1000 instr) | n/a | n/a | n/a | 64 ns (no measurable cost here) |

Compile alone (Luau): 1.4–2.2 ms for the prelude. Bytecode is 28.6 KB.

Reading:
- Load: bytecode caching is the cheap win for the interpreter (12x, 2.1 ms →
  0.18 ms). With JIT, codegen dominates the load (7.5 ms of 7.8 ms), so caching
  alone does not fix JIT load cost.
- Per event: JIT is 3–5x faster than the interpreter on this loop; LuaJIT is
  comparable to Luau JIT.
- The UVI init of about 4 s is not Lua engine time: the prelude costs 0.2–10 ms
  per VM. The bottleneck is host work around the script (graph, sample loading),
  which other perf lanes own.

Limits of this evidence: the event workload is synthetic and small. A real UVI
bank script (Augmented Orchestra `Main`, 69 KB) and a KSP-compiled script were
not benchmarked in this pass; the bank files are protected and need the reader
path. Both are required before the JIT default is changed.

## Decision by data

- Stay on Luau. Sandbox, deterministic checkpoints and memory limit are hard
  requirements for untrusted third-party scripts, and LuaJIT loses them.
- Bytecode caching is adopted now: `sampler_script::compile_bytecode`. Key the
  cache by script hash and crate version (Luau bytecode is version-bound).
- JIT stays per-VM opt-in (current UVI default is `luau-jit`). Recommendation
  for the perf lane: interpreter plus bytecode cache for init, JIT for
  callback-heavy programs measured with a real bank. Not changed here: another
  lane owns `perf/uvi-luau-jit`.

## Remaining

1. UVI wait/spawn queue (`Waiting`, `advance`) into a generic scheduler in
   `sampler-script`. Not started; entangled with input authority.
2. KSP-to-Luau compiler and trace-equivalence on real corpus scripts. Not
   started in this pass. Corpus dump tool exists
   (`crates/sampler-kontakt/examples/dump_scripts.rs`); it was built, not run.
3. Real UVI bank and KSP benchmarks (see Limits).
4. Pre-existing failure: `cargo test -p sampler-uvi --test parity`
   `ledger_matches_generated_table` (UVI_PARITY_LEDGER.md out of date). This
   diff does not touch the ledger; not verified on main.
