# Spike P3.1c: can Luau's collector run inline on the audio thread for large UVI hosts?

Task P3.1c · Follows [P3.1b](luau-alloc-mlua.md) · Spike code: `tools/spikes/luau-gc/` (standalone crate, not a workspace member; analysis scripts in `tools/spikes/luau-gc/scripts/`) · Hook: `ScriptHost::spike_lua` in `crates/sampler-uvi/src/script.rs`

## Verdict

- **Yes, under 0.5 ms is reachable for Augmented Orchestra on the audio thread, and the fix is mostly Lua-side bookkeeping, not the collector.** Measured on the real Waltz Night 1 host (340 MB after init): one leftover per-load list (`__touched`, 84,514 slots, 2.0 MB) was the largest object. Clearing it after load cut the worst single collector call from **0.45–0.55 ms to 0.12 ms** (0 KiB budget, back-to-back A/B).
- **The heap size is not what stalls.** The Luau collector cannot split one table's walk, so the worst step is the largest single object (about 3–5 ns per array slot). A 437 MB heap of 4e6 linked-list nodes has a worst step of **7 µs**. P3.1b's 14.5–16 ms stall is one 4e6-slot array.
- **Recommended for P3.2:** (a) one bounded `LUA_GCSTEP` per audio block after the render span, (b) clear per-load lists after prepare, (c) a table-size rule (no table over 8,192 slots on the audio state), (d) share retained-XML parameter templates across instances to halve the heap (memory win, not a latency fix). No helper thread, no heap-size cap for latency.

## Why Augmented Orchestra's heap is 340 MB

Measured on `UVI - Augmented Orchestra v1.1.2-R2R/Augmented Orchestra.ufs`, program `Presets/02 Action/Waltz Night 1.uvip`, through `ScriptHost::new_profiled` (the same path as P3.1b).

| measure | value |
|---|---:|
| Lua heap after init (`LUA_GCCOUNT`) | 340,235 KiB (332 MiB) |
| after one full collect | 267,956 KiB (262 MiB), 71 MiB garbage from init |
| objects after collect (`lua_memorydump`) | 930,932; **914,160 tables (98.3 % of bytes)**, average 294 B |
| host init | 2.6–3.1 s (noisy; other load on the machine) |

Decomposition, by table shape (string keys of each table, exact byte sizes from the heap dump; `scripts/shapes.py`):

| shape (dominant string keys) | tables | MiB | % of heap |
|---|---:|---:|---:|
| retained-XML parameter template `{name, displayName, description, provenance="retained-xml", readOnly, serialize, type, id}` | 480,104 | 139.2 | 54.1 |
| element instance `{__id, __set, bypass, children, displayName, id, name, parameterDefinitions}` | 60,006 | 32.1 | 12.5 |
| per-element `parameterDefinitions` container (array of 8 plus a name index) | 60,004 | 24.7 | 9.6 |
| child-list element `{…, parent}` | 15,932 | 8.5 | 3.3 |
| `{element, id}` wrappers | 60,051 | 6.4 | 2.5 |
| `{Ratio}` records | 75,136 | 5.7 | 2.2 |
| `__touched` (one array of 84,514 element tables) | 1 | 2.0 | 0.8 |
| all other shapes (each under 3 %), including 3 threads (main-thread stack ≈ 3.2 MB, ≈200k slots), strings and protos | | 39.8 | 15.2 |

What this says:

- **About 60,000 modulation/connection elements** (their XML attributes are `Bypass`, `ConnectionMode`, `Destination`, `Inverted`, `Mapper`, `Name`, `Ratio`, `Source`; `connectionmap` and `elementDetails.connections` hold them) are each materialised as a full Lua element. That is about 4.4 KB per element.
- **54 % of the heap is copies of 14 distinct templates.** Every element gets its own copy of its eight retained-XML parameter records, made by `catalog_copy::cloner` (`crates/sampler-uvi/src/script/catalog_copy.rs`) and `native.definitions` (`script.rs`, around 1945–2040). All eight records are `provenance = "retained-xml"`, and the copies differ only in `id` and in the constant-property fields. This is the static data that can move out of Lua: one record per kind and name, with a small per-element overlay for `id`, `Value`, and `Style`. Estimated saving: about 130 MB (53 %). Not built.
- **Strings are not the problem.** Only 10,087 strings (0.46 MB) and 471 protos (0.36 MB).
- **The `__touched` array is the one large object.** It is a Lua-side list of every element touched by `setParameter` (`script_prelude.lua`, `__touched[#__touched + 1] = self`). Only `insert_overrides` reads it, at load. It keeps 84,514 elements alive after load.

## Why the worst pause is what it is

Luau's `traversetable` walks a table's whole array and node part inside one `propagatemark` call (`luau0-src` 0.22.0+luau740, `VM/src/lgc.cpp:343`). `lua_gc(LUA_GCSTEP)` only checks its budget *between* objects. A single large table is therefore an indivisible step. The atomic phase is also a single call, and it clears the unused part of each thread stack (`clearstack`, `lgc.cpp:489`).

The synthetic cases in `luau-gc synth|shapes|slots|stack` isolate each factor. All numbers are release builds on the AMD Ryzen 7 7800X3D; the machine was shared, so single maxima vary by up to 2×.

| experiment | heap | worst single collector call |
|---|---|---:|
| `synth`: one array of 4e6 `{i}` tables (P3.1b shape), cycle 1, 0 KiB | 316 MB | 13.9 ms (step #10, the array walk) |
| same, steady cycles, 0 / 64 KiB | 316 MB | 19.6–20.3 ms / 20.5–24.5 ms |
| `shapes`: array of 4e6 `{i}` tables | 316 MB | 19.2 ms |
| `shapes`: 4e6 linked-list nodes `{v, next}` | **437 MB** | **7.0 µs** |
| `shapes`: one array of 4e6 numbers | 66 MB | 2.2 ms |
| `shapes`: one array of 4e6 interned strings | 222 MB | 15.3 ms |
| `slots`: one array of 125k / 250k / 500k / 1M / 2M / 4M slots | 10–316 MB | 0.54 / 0.74 / 1.63 / 4.4 / 9.5 / 18.7 ms (3.0–4.8 ns per slot) |
| `stack`: main-thread stack grown to 0 / 50k / 200k / 400k slots (linked list, 11 MB heap), cycle 1 | 11 MB | 12 / 26 / 55 / 93 µs |
| same, cycle 2 | | 11 / 15 / 11 / 24 µs (the stack halves each cycle) |

Conclusions: the worst step tracks the largest object (about 4 ns per array slot), the heap total does not. The thread-stack clear costs about 0.2 ns per slot and shrinks on its own.

For P3.1b's 14.5–16 ms: this is the 4e6-slot array walk at step #10, confirmed above.

## Real hosts

Each row is the worst single `LUA_GCSTEP` call across 3–4 full cycles after one full collect, unless noted. Budget is `kb` KiB per call; `kb = 0` is one internal Luau step per call.

**Augmented Orchestra, Waltz Night 1, back-to-back A/B, 4 cycles** (`__touched` cleared after load or not):

| budget | p50 µs | p99 µs | worst µs, not cleared (steady) | worst µs, cleared (steady) |
|---:|---:|---:|---:|---:|
| 0 | 0.3 | 0.3 | 548 (first cycle 394) | **120** (first cycle 107) |
| 8 | 3.3 / 3.4 | 3.4 / 3.6 | 430 | 430 |
| 64 | 26.6 / 27.1 | 27.4 / 28.0 | 531 (first cycle 454) | 394 (first cycle 190) |

The 3.4 ms outlier in the cleared run's budget-1 row is a single call during another process's load, not part of the distribution. Budgets 8 and 64 keep a worst call near 0.4 ms even after clearing, because one call can finish a cycle and run the atomic phase too; budget 0 is the only row with a clean drop. Uncleared, 14 cycles: worst per cycle 270–400 µs at budget 0, flat, so the first-cycle spike is not the whole story.

**Augmented Orchestra, note stream** (6,000 blocks of 1.33 ms, one note per 67 blocks, `__touched` cleared):

| collector | cycles | gc p99 µs | gc max µs | note callback max µs | block max µs | heap change KiB |
|---|---:|---:|---:|---:|---:|---:|
| stopped | 0 | – | – | 351 | 352 | +3,410 |
| step 0 KiB | 0 | 0.3 | 127 | 250 | 254 | +3,011 |
| step 8 KiB | 0 | 3.2 | 163 | 330 | 344 | +3,065 |
| step 64 KiB | 2 | 25.9 | 217 | 253 | 464 | +879 |

The note callback itself peaks at 250–350 µs on the first notes. That is script work, not GC, but it uses most of the 0.5 ms budget before any collector call, so it needs its own measurement in P3.2.

**VWinds A Clarinet, `Presets/Clarinet A.uvip`** (cleared, 3 cycles; note stream 20,000 blocks):

| budget KiB | p50 µs | worst µs |
|---:|---:|---:|
| 0 | 0.2 | 28 |
| 8 | 1.3 | 6 |
| 64 | 8.4 | 36 |

Note stream: step 64 KiB has gc p99 9.8 µs, gc max 51 µs, block max 185 µs (note callback max 165 µs). VWinds' worst call is well under 0.5 ms with any budget. The ~0.3 ms that P3.1b cited for VWinds-sized heaps was a 1e5-table synthetic. VWinds heap: 6.7 MiB after init, 4.1 MiB after collect (the arena peak in P3.1b was 8.8 MB, see Deviations).

## Per-object decomposition

`scripts/attribute.py` claims every object by the first root that reaches it, cutting `parent` back-edges and `env`. Roots are the `_G` entries, then the registry entries, then the main thread. The 261.5 MiB total is exact; the split is first-claim, not a dominator tree.

- With `parent` and `env` cut, 98 % of the bytes are still reached through `registry.__mlua_uservalues`: the 110 userdata, each with a `{members, fields}` table. Their `fields` tables hold the element tree. `members` reaches under 0.3 MB; `fields` reaches the rest.
- `__touched` (84,514 slots) lives in the script environment (`registry` slot 3), which is also `lua.globals()` in mlua's sandbox. Luau's original globals table (`registry` slot 2) has 34 entries and no `__touched`.

## Strategy evaluation (measured worst call, Augmented Orchestra unless noted)

- **(a) Small steps every block.** Works, as long as no object is large. Budget 0 gives 120 µs worst after clearing; budget 64 KiB gives p50 27 µs and about 0.4 ms worst. Per-cycle cost is about 122k calls at budget 0 (about 160 s at 1.33 ms blocks) and about 2,400 calls at 64 KiB (about 3 s). **Recommended.**
- **(b) Static data out of the Lua heap.** Halves the heap (estimate: 261 → about 130 MB) and shortens cycles. It does not change the worst call, because the templates are small (about 290 B each). Medium effort (catalog cloner, prelude accessors, three consumers in `script.rs`). Worth doing for memory, not for the audio budget.
- **(c) GC on a helper thread, state parked between callbacks.** Rejected. Luau is single-owner. Any callback that arrives during a helper step must wait for the same lock, so the stall moves to the audio thread instead of disappearing. Removing it needs a second state and a handover that the audio thread never blocks on, which is not provably safe here. Not built.
- **(d) Luau internals.** `GOAL`, `STEPMUL` and `STEPSIZE` do not change the worst call (they change cycle count). Making `traversetable` incremental would be a fork of luau0-src. Not needed once (a) and the table-size rule hold. The thread-stack clear is about 50 µs at 200k slots, which is small.

## Heap rules for P3.2

1. **Step budget.** `lua_gc(LUA_GCSTEP, 0)` once per audio block after the render span, raised to 8–64 KiB only when the cycle falls behind its trigger. Target: p99 at most 50 µs per call at 64 KiB (measured 27 µs).
2. **Clear per-load lists after prepare.** `__touched` (84,514 slots on AO, 215 on VWinds) is only read by `insert_overrides`, on the load path (`crates/sampler-uvi/src/lib.rs:1695` and `:2489`). It must be emptied after load, in every environment that holds one. The spike clears it by scanning the first 64 registry slots for environment tables (`host::clear_touched`).
3. **Table-size rule.** No table with more than 8,192 array or node slots on the audio state (about 40 µs at 4.4 ns per slot). Chunk any list that must stay (such as `__touched`). P3.2 needs a test that walks the heap and fails on a violation.
4. **Heap cap.** Latency does not depend on heap size, so no cap is needed for it. Cycle length does: about 2,400 calls per 262 MB cycle at 64 KiB. Keep P3.1b's arena rule for memory, and treat 512 MiB of live heap per script state as the review threshold (AO is 340 MB after init).
5. **Shared templates (b).** Share the retained-XML records per kind; keep only `id` and the constant-field overrides per element.

## What P3.2 must implement

- The per-block step in the audio callback, with the budget schedule in rule 1.
- The post-load clear (rule 2) in the same place the load path stops reading `__touched`, plus a unit test that the list is empty after prepare.
- The table-size test (rule 3) against AO and VWinds hosts.
- Template sharing (rule b) as a follow-up, not a blocker.
- Remove the spike hook (`ScriptHost::spike_lua`) with the spike.

## Deviations from the brief

- **Only two hosts were measured for GC:** Waltz Night 1 and VWinds A Clarinet. The other AO program (Mod Bright Horns, 355 MB) and the other VWinds banks were not run through the collector. The decomposition is from the Waltz Night dump only.
- **Noise.** The machine was shared. Another agent's `luau-gc` run on the same AO program overlapped part of the measurements, and one row has a 3.4 ms outlier. Read the A/B table as relative; the cleaned-up numbers are medians and p99 plus the back-to-back maxima.
- **Phase attribution is inferred,** not instrumented. Luau exposes no phase counter without `LUAI_GCMETRICS`. The atomic-phase claims rest on the slowest-step position and on the synthetic experiments.
- **Residual after clearing.** Budget 0 gives 120 µs after clearing, and budgets 8 and 64 give about 0.4 ms. I did not isolate the cause of the extra time at budgets 8 and 64. The most likely explanation is one call that finishes a cycle and runs the atomic phase, but that is inference.
- **Memory numbers differ from P3.1b.** P3.1b reported 8.8 MB for VWinds Clarinet from `lua_memory_bytes` through its arena; here `LUA_GCCOUNT` reads 6.7 MiB after init on the same program. P3.1b's figure included profiling overhead and arena accounting; the AO figure here (340 MB) is close to P3.1b's (379 MB, another program).
- **Teardown section is opt-in** (`LUAU_GC_TEARDOWN=1`). It runs a full collect per global and takes minutes on AO; the Python attribution replaces it. Its count label was also wrong and is fixed.
- **Hook.** `ScriptHost::spike_lua(&self) -> &Lua` (`#[doc(hidden)]`, borrowed, in `crates/sampler-uvi/src/script.rs`). The spike drives `lua_gc` through `mlua::ffi` itself. No new cargo features.

## Reproduce

```
build-slot cargo build --release --manifest-path tools/spikes/luau-gc/Cargo.toml
luau-gc host "<bank.ufs>" "<program.uvip>"             # heap, collector, notes
LUAU_GC_CLEAR_TOUCHED=1 luau-gc host ...               # after load, clear per-load lists
LUAU_GC_KEEP_DUMP=1 LUAU_GC_SCRATCH=<dir> luau-gc host ... # keep the lua_memorydump JSON
python3 -I tools/spikes/luau-gc/scripts/attribute.py <dump.json>
python3 -I tools/spikes/luau-gc/scripts/shapes.py <dump.json>
luau-gc synth | shapes | slots | stack                 # synthetic isolation
```

Environment knobs: `LUAU_GC_CYCLES`, `LUAU_GC_MAX_STEPS`, `LUAU_GC_BLOCKS`, `LUAU_GC_BLOCK_MS`, `LUAU_GC_NOTE_EVERY`, `LUAU_GC_SKIP` (`heap,gc,notes`), `LUAU_GC_TEARDOWN`, `LUAU_GC_SCRATCH` (default `/data/dev/workspace/scratch/luau-gc`).
