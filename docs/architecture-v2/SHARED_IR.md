# Shared IR and language execution

The product direction is one shared instrument/runtime architecture with source
frontends for Kontakt/KSP, UVI/Falcon, SFZ and other formats. This follows the
original architecture's source → semantic → prepared split; the user's 2026-10-06
clarification makes it explicit. Do not build a complete engine per format.

## What every frontend contributes

| Representation | Contents | Ownership / execution |
| --- | --- | --- |
| Source model | Original hierarchy, names/IDs, raw units/defaults, version, resource references and unrecognized meaning | Control/worker; retained for diagnostics and correct lowering |
| Semantic instrument IR | Source views/mappings, selection/articulation rules, scoped modulation and DSP graphs, event routing, controls, behavior modules and capability requirements | Format-independent definitions with explicit source profiles |
| Prepared execution IR | Resolved dense tables, source/DSP schedules, validated behavior instructions/service bindings, resource budgets and immutable assets | Prepared off audio; one native runtime executes admitted work |
| Mutable execution state | Notes/families, expression, cursors/DSP history, script instances, continuations, control values and selection counters | One writer per domain; explicit generation retention and retirement |

Current code implements pieces of the semantic/prepared representations through
`Region`, `Playback`, `Envelope`, `Modulation`, control definitions, selection
policies, `Program`/`Instruction` and `Prepared`. These are real shared execution
paths, not a completed universal instrument schema. KSP already lowers to the
native behavior instruction set. A new source frontend must use those same owners.
The SFZ, Kontakt-container and UVI source frontends are still required work.

Keep source translators as small as the semantics allow. SFZ mapping inheritance
is not the same job as KSP language execution or UVI coroutine/object behavior.
A translator cannot discard unsupported meaning to remain small. Preserve it in
the source/capability report, add the required shared primitive or profile, and
reject unsupported execution explicitly. No fallback to the old core is allowed.

The shared IR must preserve scope (voice/group/part/bus), source units, nonlinear
DSP placement, callback time and synchronous read-after-write behavior. It is not
an opcode-name substitution table or the lowest common subset of vendors.

## Luau evaluation

Luau is a candidate implementation for Lua-family source behavior and new authoring
scripts, not the universal instrument IR and not the per-sample DSP executor.
Adopting it must not replace exact native control identities or bounded ownership
with garbage-collected script objects. A behavior module binds to typed native
services; its language/value implementation can differ while the musical owners,
asset services, controls and DSP remain shared.

Official documentation reviewed 2026-10-06:

- [Luau compatibility](https://luau.org/compatibility/): based on Lua 5.1, with
  sandbox removals and documented semantic differences. Numeric values do not gain
  a Lua 5.3-style integer type; exact integers above 2^53 cannot be treated as plain
  script numbers. Native IDs remain opaque checked handles. UVI's `bit` API is not
  automatically equivalent to Luau's `bit32` API.
- [UVI Lua runtime](https://lua.uvi.net/_lua_reference.html): Lua 5.1 plus a custom
  API, coroutine model and preallocated realtime memory pool. Source-language
  overlap is encouraging; it does not implement event forwarding, object addressing,
  widgets, `require` resolution, persistence or the custom API by itself.
- [Luau performance](https://luau.org/performance/): optimized bytecode execution,
  allocation and incremental GC still include assists and indivisible GC work.
  Throughput improvements do not establish a bounded audio callback.
- [Luau embedding](https://luau.org/sandbox/): memory limits are host policy;
  interrupts do not bound a single expensive native function. Restrict/admit native
  services by cost and compile trusted source locally; arbitrary external bytecode
  is not a supported input contract.

Evaluate a pinned Luau build with authored Lua 5.1 and Luau fixtures covering
closures/tables, coroutine resume/cancel, protected calls, numeric/bit behavior,
module resolution and host-service invocation. Then evaluate actual readable UVI
fixtures and allocator/GC failure paths. Admission/failure must retain native note
and callback cleanup. No Luau production dependency or all-format rewrite is
adopted merely because it is under evaluation; choose it when these requirements have evidence.

KSP retains its own exact numeric and callback semantics. Komplete Script retains
its typed/reactive component semantics. SFZ declarative instruments compile directly
into the instrument IR without an artificial script. Even if Luau is adopted for
Lua execution, these frontends still bind the same native services and prepared DSP.
