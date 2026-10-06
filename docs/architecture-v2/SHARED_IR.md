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
Kontakt-container and UVI source frontends are active required work. SFZ and all
other format implementations are deferred by subsequent explicit user direction
until full Kontakt/Falcon parity and demonstrated performance superiority. The
format-neutral architecture remains; no SFZ frontend has been implemented.

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
  sandbox removals and documented semantic differences. Ordinary script numbers
  do not preserve every integer above 2^53. The pinned source now also exposes a
  separate exact-int64 C API, verified below; do not conflate the two value types. Native IDs remain opaque checked handles. UVI's `bit` API is not
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

## Pinned executable Luau probe

Evaluated upstream `luau-lang/luau` commit
`421cc8158a752c8933a3d73555fd79df631a90b6`, with its MIT license inspected.
No Luau dependency was added to the plugin or any native crate. The isolated
[`tools/luau_probe.cpp`](../../tools/luau_probe.cpp) checks ordinary Lua-style
closures/tables, two independently yielding callbacks resumed out of order through
an embedded host function, Luau annotations/compound assignment, coroutine close,
protected errors and bytecode-loop interruption.

All assertions pass. Ordinary numbers lose `+1` at 2^53; the pinned source's separate
`lua_pushinteger64`/`lua_tointeger64` API roundtrips 2^53+1 exactly. The public
compatibility page and evolving source do not expose an identical numeric surface,
so production selection must pin a reviewed version and its enabled features.

A deliberately allocating callback creating 10,000 retained tables made **62 calls**
to the supplied realloc allocator (Luau internally pools smaller objects). Total
probe peak was 1,498,320 bytes; closing the VM returned live bytes to zero. This
measures allocator entry, not GC pause bounds or sampler performance. Ordinary
VM use is therefore not automatically an allocation-free audio callback. The probe
also verifies that stock `bit`, `class` and `table.copy` UVI-facing names are absent;
they require explicit vendor service/library semantics.

Loop interruption uses Luau's `gc == -1` execution safepoints; nonnegative values
identify GC phases. A first probe incorrectly treated that flag as Boolean and was
terminated; the corrected probe runs with an external timeout. This is not a bound
on long native functions, nor a replacement for script admission and cost limits.

Reproduce with the pinned source checked out under ignored
`artifacts/luau-evaluation`:

```sh
cmake -S artifacts/luau-evaluation -B artifacts/luau-evaluation/build -DCMAKE_BUILD_TYPE=Release -DLUAU_BUILD_CLI=OFF -DLUAU_BUILD_TESTS=OFF
cmake --build artifacts/luau-evaluation/build --target Luau.Compiler Luau.VM -j 4
c++ -std=c++17 -O2 -I artifacts/luau-evaluation/VM/include -I artifacts/luau-evaluation/Compiler/include tools/luau_probe.cpp artifacts/luau-evaluation/build/libLuau.Compiler.a artifacts/luau-evaluation/build/libLuau.Ast.a artifacts/luau-evaluation/build/libLuau.Bytecode.a artifacts/luau-evaluation/build/libLuau.VM.a artifacts/luau-evaluation/build/libLuau.Common.a -o artifacts/luau-probe
timeout 20s artifacts/luau-probe
```

Current conclusion: retain Luau as a strong candidate for the Lua-family frontend.
UVI's complete API, preallocated allocation/GC strategy, native callback service
binding and actual library compatibility remain required work. Do not translate
all other formats through Luau solely to obtain a shared engine; the native IR
already supplies that engine boundary.
