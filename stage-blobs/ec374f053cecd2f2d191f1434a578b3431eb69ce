# Native/script round 2: source, documentation, and pending runtime evidence

Worktree: `fix-ksp-service`; branch: `takeover/native-ksp-20261010`.
No cargo/rustc/clippy/build, plugin install, reference execution, reader process,
library extraction, or frozen-reference modification occurred in this round.
Only integration may run the combined candidate batch.

## Ready menu getter source

Commit: `a39b07f5fc9d37a9a06719a7ab6238a1e7e9429c`.
Cherry-pick independently onto the already-integrated W5 live-menu/evaluated-setter
base. Do not merge this old worker branch wholesale.

- `crates/sampler-ksp/src/lower.rs:3750-3820`: dynamic integer parameter
  dispatch routes NUM_ITEMS and SELECTED_ITEM_IDX through existing live-menu
  helpers. Other selectors retain their previous fallback.
- `crates/sampler-ksp/tests/engine_par_contract_audit.rs:384-438`: two authored
  regressions cover same-callback count/value/visibility edits, generic HIDE
  fallthrough, and empty-menu defaults.
- Own legacy reference: `0cb7a8a0:src/ksp/calls.rs:1299-1307`.
  Its dynamic parameter dispatch supplies the same count/index distinction.
  This is our implementation history, not Kontakt measurement.

Authoritative requirement:
[NI control parameters, ui_menu](https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/control-parameters#ui_menu-731962).
Fetched HTML SHA256:
`8b1d2badc937f5980135c8b671b1d6c2bd906d2f4c80db520ccae270a766858c`,
matching the repository's previously pinned KSP_SURFACE source hash.
The document specifies entry count and the selected entry's index, not its
authored logical value. The immutable shipped KSP_Manual.pdf corroborates
those two definitions (printed p297; converted text lines10996-11004).
PDF SHA256:
`b708b039ae3f8e93eaff7d2bbf3247df595a1416b16a8607d3e016e96a5fd282`.
No manual content is copied into the repository.

No-build checks passed: diff whitespace; Python checks of the two manual
statements, dynamic-only integer admission, existing helper routing,
selected-index default, branch landing, and regression presence.
They establish source invariants, not Rust execution.

Combined batch filter:
`cargo test -p sampler-ksp --test engine_par_contract_audit dynamic_menu_properties`.

Dynamic menu VALUE conversion, read-only setter enforcement, and native
selection with duplicate logical values remain separate gaps. The helper
retains our existing hidden/duplicate selection policy; native parity is UNKNOWN.

## Next-batch fade seam

Commit: `9fe764e4dcb3e876f82a1d19e2656cf8fe02ffe6`.
This commit MUST be paired with the DSP-owned script_params runtime/signature
change before compilation. It is not independently compile-ready.

- `sampler-core/src/behavior.rs:39-63`: public FadeCurve and checked internal
  selector decoding.
- `behavior.rs:385-392,824-831,2465-2485`: optional operand-local admission
  and dispatch; omission retains Linear.
- `sampler-core/src/lib.rs:59-63`: enum export.
- `sampler-core/src/parallel.rs:697-706`: existing direct test caller supplies
  Linear explicitly.
- `sampler-ksp/src/builtins.rs:129-130,639-643`: fade_out accepts its optional
  fourth argument; five names map to INTERNAL enum selectors.
- `sampler-ksp/src/lower.rs:2276-2310`: curve argument2 for fade_in and
  argument3 for fade_out. Curve local2 and dynamic-stop local3 do not overlap.
  Existing event selector admission is unchanged; ALL_EVENTS/by_marks are held.
- `sampler-core/src/behavior.rs:2921`: operand-local/selector regression.
- `sampler-ksp/tests/compile.rs:128`: signature/arity regression.
- `sampler-ksp/tests/params.rs:158,199`: variable-array selectors, all five
  documented quarter-time values, time-mirrored fade-out, three block sizes,
  and checked unknown-selector policy.

Authoritative requirement:
[fade_in](https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/event-commands#fade_in--)
and
[fade_out](https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/event-commands#fade_out--).
The current manual identifies the optional curves as Kontakt8.12 additions.
Archived HTML SHA256:
`57f8f69b4deec41cc0eb98fd1cc5196f3c02b72e3d222faff7b91fe8551626ce`.
The scout's private official-docs manifest retains retrieval metadata.
No copied implementation or decompiled pseudocode was used.

No-build checks passed: rustfmt syntax/formatting, diff whitespace, Python
source invariants, independent documented-equation evaluation of the test's
quarter-time data. They do not execute the Rust fade implementation.

Combined next-batch filters, AFTER pairing DSP:
`cargo test -p sampler-core fade_curve_`;
`cargo test -p sampler-ksp --test compile --test params fade_curve_`;
then the full params area suite.

Unknown selector rejection is our explicit admission policy, not established
native behavior. Supplied native diagnostic strings include both unknown-curve
diagnostics and LINEAR fallback wording; their actual branches are unresolved.
Do not claim native ordinal identity or numerical/render parity.

## Read-only native evidence and its limit

REA opened the Kontakt8 PE at
`/home/derpcat/Desktop/Kontakt 8/x64/Kontakt 8.exe`.
Executable SHA256:
`fe12b6a7b652cfd026b5bec62ae6c096b2dfddcc0e17249ba518cb3ef0f0c8e0`.
A focused search_strings request timed out at330000ms. No retry was made;
close_binary completed. There was no native/reference process execution.

The coordinator supplied static NI_FADE name locations
`0x4e84898,0x4e848a8,0x4e848c0,0x4e848d8,0x4e848f0`.
Those locations establish names only. No runtime/native PASS follows.
The immutable shipped manual is the equivalent check for documented menu
semantics; current authoritative event documentation supplies the fade contract.

## UVI Lua language/sandbox characterization

Authoritative reference:
[UVI LuaVM](https://lua.uvi.net/_lua_reference.html#LuaVM).
Retrieved2026-10-10; HTML SHA256:
`339c49ba91c9acb7eef7c49014801fc426fc78f6bc2a060dd45ae5de79b54bec`.
UVI explicitly specifies a sandboxed Lua5.1 VM and an unmodified standard
Lua5.1.4 compiler. Our Luau+JIT is a different runtime.
This difference is a compatibility risk, NOT evidence that all current scripts
break. No VM or dependency replacement is included.

Source checked at `9fe764e4dcb3e876f82a1d19e2656cf8fe02ffe6`:

| Construct/boundary | Exact source evidence | Evidence status |
|---|---|---|
| VM choice | sampler-uvi/Cargo.toml:41, luau/luau-jit | Source proven, not vendor VM equivalence |
| Loaded libraries | script.rs:919-923, TABLE/STRING/MATH/COROUTINE/BIT | Source proven; individual function semantics unverified |
| Filesystem/process sandbox | script_prelude.lua:455 disables dynamic loaders; script.rs:919-923 excludes io/os/package/debug | Owned capability boundary; vendor's complete sandbox inventory UNKNOWN |
| Authored library extensions | script_prelude.lua:438-443 copies table/string/math | Source support; native mutation behavior UNKNOWN |
| table.copy | script_prelude.lua:446-450 shallow-copy helper | Source support; vendor edge semantics UNKNOWN |
| Named module loading | script_prelude.lua:456-473 | Owned bank-scoped route, not package/OS require parity |
| Functions/environments, varargs, patterns, numeric conversion, ipairs, modulo | new inline lua_compatibility tests | Prepared representative probes; Rust execution PENDING |
| bit library | a8c29411380e0c60b9b6671b8697a0c305ce6da5:script_prelude.lua:55 aliases bit=bit32 | Already present in newer integrated source; do not duplicate the old ignored-gap fix |

The existing uvi_audit_corpus.rs:49-59 metadata census recognizes
bit.band/getfenv/setfenv/math.pow/table.maxn/newproxy. This indicates constructs
worth probing; it does not establish execution or native parity. This round did
not read or dump proprietary library scripts. New fixtures are entirely
authored inline scripts. Existing coroutine/sandbox regressions remain intact.

The Cargo comment is corrected to avoid calling Luau the vendor's documented
VM. New tests preserve supported extensions and do not demand wholesale Lua
compatibility from three passing examples.
Combined later-batch filter:
`cargo test -p sampler-uvi --test lua_compatibility`.

NEXT: pair the fade seam with DSP runtime, receive combined-batch receipts,
resolve any actual failures, then compare representative UVI language boundaries
without replacing the VM merely because its name differs.
