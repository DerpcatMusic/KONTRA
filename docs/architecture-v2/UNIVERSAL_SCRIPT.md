# Universal script layer (`sampler-script`)

Status: phase 2, 2026-10-23 (phase 1 merged as #452). Owner: script lane. Crate: `crates/sampler-script`.

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

## JIT policy (phase 2)

Finding: mlua's default for `luau-jit` builds is `enable_jit = true`
(`mlua/src/state/extra.rs`), so every chunk, in every VM, was natively
compiled inside `load`. That is the "7 ms per VM" cost, and it is paid per
module, not once. `new_vm` now sets the interpreter; `new_vm_with(mem,
Jit::Native)` opts a VM in, and UVI exposes `Config::jit` (default
`Interpreter`). Codegen happens at load only, so the switch applies to chunks
loaded after it, not to running ones.

`perf/uvi-luau-jit` is an ancestor of `main` (no unique commits), so there
was nothing to duplicate.

Measured per-module load, 19 embedded modules of Augmented Orchestra (389 KB
source), best of 5, fresh VM per load:

| Path | Compile from source | Bytecode load, interpreter | Bytecode load, native (default before) |
|---|---|---|---|
| layer.lua 115 KB | 3.4 ms | 0.13 ms | 2.5 ms |
| part.lua 78 KB | 3.4 ms | 0.07 ms | 20.6 ms |
| main.lua 69 KB | 2.1 ms | 0.06 ms | 14.7 ms |
| arpeggiator.lua 36 KB | 1.2 ms | 0.04 ms | 8.0 ms |
| small modules (7–26 KB) | 0.2–0.4 ms | 0.01–0.03 ms | 2–2.7 ms |

## Bytecode cache (phase 2)

`sampler_script::cached_bytecode(dir, source)`: BLAKE3 key over (crate
version, source); file = magic + BLAKE3 of the bytecode + bytecode; write to a
temp file, then rename. A bad checksum, truncation or junk is a miss: recompile
and rewrite. Any cache I/O error falls back to compiling. Directory:
`KONTRA_SCRIPT_CACHE` (empty disables), else `$XDG_CACHE_HOME/kontra/luau`,
else `~/.cache/kontra/luau`. UVI's module `compile` and root-script load go
through it (`chunk_bytes` in `script.rs`), except inspection builds, which need
the coverage compiler.

Whole-host effect, 4 Augmented Orchestra presets (`script_init_bench`): root
scripts are 20 bytes, so `ScriptHost::new` is 615–765 ms with or without the
cache. That time is the 18 MB program XML parse and host work, not Luau. The
cache removes about 11 ms of module compile per VM only for programs that load
these modules. Not measured on a program with a large root script: none of the
survey's "~4 s" UVI programs has one (their time is sample loading, `load_ms`).

## DAW-criteria checklist

Status key: **M** measured here (numbers in the Measured table), **T** test
in `crates/sampler-script/tests/daw_bounds.rs`, **P** partial, **O** open
(not built in this pass).

| # | Criterion | Status | Evidence / next step |
|---|---|---|---|
| 1 | Worst case per event, not mean (p99/p99.9/max, GC pause) | **M** | Measured table. Only synthetic `onNote`; real UVI callbacks not timed. |
| 2 | No allocation or unbounded GC on the audio path | **P** | GC steps are bounded (`bounded_gc_step`, T). Memory cap per VM (T). Audio-thread placement of script callbacks is not verified here. |
| 3 | Deterministic bounded execution; runaway aborts; error isolates | **T** | Budget stops at exactly limit+1 checkpoints; VM survives a refill (T). The host still needs to disable a faulting script. O. |
| 4 | Sample-accurate event timing, wait() across block sizes, offline == realtime | **P** | KSP front end (`sampler-ksp::luau`): wait resumes at the interpreter's exact frame (micro-frame carry copied), 1-frame stepping test and 32/64/512/4096 block rows identical to the interpreter. Offline == realtime not tested. UVI wait: not ported. |
| 5 | Determinism for bounce and recall: seeded RNG, no wall clock, state save/restore, hot reload | **P** (Luau path: pending waits are coroutines and cannot be serialized; save/restore not built) | Budget is checkpoint-based, not timer-based (T). `math.random` seeding exists only under the `scan` feature. Save/restore of script state: O. |
| 6 | Threading: UI/file work through lock-free queues, no blocking | **O** | Not in this crate. Existing UVI command queues are not audited here. |
| 7 | Many instances: per-VM memory and creation cost | **M** | ~0.43 MB per empty VM, ~0.48 MB with a callback chunk; create 0.23 ms native / 0.38 ms interp (includes stdlib setup). Per-voice script state: O. |
| 8 | Tempo/transport, denormals, NaN/inf containment, int vs real | **O** | KSP int semantics belong to the KSP front end (not started). |
| 9 | Startup off the UI thread; off-thread compile | **P** | Bytecode cache makes module load ~20-50x cheaper on the interpreter. Off-thread compile: O. |

## Measured (2026-10-23, release, shared host, `daw_bench 20000`)

| Metric | Interpreter | Native (JIT) |
|---|---|---|
| onNote (8-step loop, hooked with checkpoint), p50 / p99 / p99.9 / max | 230 / 320 / 600 / 38 591 ns | 140 / 181 / 320 / 29 670 ns |
| GC slice, 8 steps, p50 / p99 / p99.9 / max | 2.2 / 2.6 / 4.4 / 17.6 µs | 2.2 / 2.3 / 4.5 / 9.2 µs |
| VM create / empty heap / heap with callback chunk | 0.38 ms / 435 KB / 484 KB | 0.23 ms / 435 KB / 484 KB |
| Runaway loop: checkpoints / wall time to abort | 1 000 001 / 7.5 ms | (interp only) |

Max values are single outliers on a busy host, not bounds; the DAW target is
the p99.9 column. A 230 ns callback is about 4.6 % of a 5 µs sample-voice
budget, so one callback per note is affordable; a 30-40 µs max shows the host
must keep callbacks off the audio thread or at block boundaries (items 2-3).

## KSP front end, phase 3 (2026-10-23)

`crates/sampler-ksp/src/luau`: HIR -> Luau compiler (`compile`), `lower()` (interpreter `on init` + Luau chunk), `check_init` (cell/text/engine equality), `host::Host` (one VM per instrument, one coroutine per callback, exact-frame waits).

Subset: int/real/text scalars and arrays, `if`/`while`/`select`/`call`/`exit`/`continue`/`inc`/`dec`, i32 wrap/div/mod/bit ops, `wait`, `set_engine_par`/`get_engine_par`, `on init`/`on note`/`on release`. `exit` leaves the enclosing callback or function (interpreter semantics, not just the loop).

Corpus: 696 real scripts extracted from `/data/main/Libraries` (`sampler-kontakt/examples/ksp_corpus_extract`), census with `sampler-ksp/examples/luau_census`: 5 fail the front end, 691 compile but use constructs outside the subset, **0 in subset and compared**. Top gaps by site: `set_control_par` (2.2 M), `set_control_par_str` (0.46 M), UI controls (0.40 M), `make_persistent` (0.19 M), `search` (0.16 M), `get_ui_id` (0.14 M), `read_persistent_var` (0.09 M).

Lane B (language/builtins, census `scripts blocked only by non-UI constructs`): 0 of 696 yet. Builtins now in subset: `in_range`, `abs`, `sgn`, `sh_left/right`, `msb/lsb`, `ms_to_ticks/ticks_to_ms`, `min/max`, conversions, `round`, `floor/ceil`, `sqrt/exp/log/log10`, trig, `pow`, `num_elements`, `search`, integer `sort`, `message`. Top remaining non-UI blockers by script: `on controller` (687), `play_note` (684), `disallow_group`/`ignore_event`/`allow_group` (~680), `note_off`, `change_tune`, `get_event_par`, `set_event_par`, `fade_out`: all need the engine action sink, not started.

Per event (`examples/luau_bench`, release, shared host, 20 000 events): interpreter p50 0.8-1.7 µs, p99.9 3.5-11 µs; Luau p50 5.5 µs, p99.9 46-51 µs, max 170-220 µs. The budget hook costs about 1.3 µs of the p50. **Luau is not faster with equal or better tail latency: the interpreter stays the default.**

## Remaining

1. Widen the subset until real scripts are in it: UI controls and `set_control_par*` (record-only), persistence, `search`/`get_ui_id`, `num_elements`, `message`. Then compare traces, not only `on init`.
2. Pending waits and polyphonic state save/restore (checklist 5): coroutines cannot be serialized; needs a continuation form for waits.
3. Real UVI callback benchmark and the DAW checklist items marked O above.
4. `UVI_PARITY_LEDGER.md` embeds `script.rs:LINE` in ~5 800 rows. Any edit to
   `script.rs` moves those numbers and fails `ledger_matches_generated_table`
   until the ledger is regenerated with `parity_summary -- --ledger`.
