# KSP array value contract — source-only candidate

Base: `13c044d6f9f80576507521a2dc6521e9aad7d9c5` (CURRENT13), not historical source3665.
Private branch: `pi/parity-release-arrays-20261010`.
All authored Rust: **NOT_RUN / NOT_COMPILED**. Native behavior: **UNKNOWN**.
Complete-product CPU AND RAM below both reference products: **UNACHIEVED**.
No build, Rust test executable, host/native job, bank, install, publication, manifest,
wrapper, official ledger, SGN, or random change. This is an unmerged candidate.

## Verified primary requirements

Official unversioned NI KSP Array Commands archive:
`https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/array-commands`.
Archive SHA-256: `1c091362091dcb71c5175fb9130ec450c1a5bce5344f66630c5a10bac71b3c87`.

- `#array_equal--`: equal values return 1, different values return 0; real arrays
  are unsupported; the unequal-length example must produce an error.
  Archived excerpt SHA-256:
  `c04a8f91d63248711c2c41758f8db3bed49027dc55d83cec2842e0f38fa288e7`.
- `#search--`: search an array or a range; return the first matching index or -1;
  real arrays are unsupported.
  Archived excerpt SHA-256:
  `5d66ae98027e7ae64ea00a62bfd65ae664afa0607f768d1cd30742352962f4d4`.

These archived bytes were independently hash-checked. Historical handoff code
references were retraced on CURRENT13; project parity claims are not evidence.

## Small source fix

`Sema::builtin` admits equality only for declared integer arrays of equal size.
Real operands and unequal lengths get positioned errors before initialization,
including calls inside functions and callbacks. Text/system equality operands
get an explicit unsupported-scope error; they cannot reach `Eval::var` or silently
produce a different initializer/runtime result. This restriction and early error
timing are **own safety policy**, not an observed native text/system contract.

Equality initialization now compares integer values directly, without debug-text
allocation per cell. Runtime equality retains its existing cell comparison loop;
unsupported storage or unequal lengths cannot silently return 0.

Search retains real-array, value-type, and paired-endpoint gates. System values
must be integers. System arrays without a runtime reader are rejected at sema;
a context-unavailable reader is rejected during lowering rather than ignored.
The supported system scopes remain CC, KEY_DOWN, CC_TOUCHED in controller context,
EVENT_PAR in note/UI-control context, and GROUPS_AFFECTED in note/release context.
No new system reader or generic array capability was added.

Both init and runtime intersect an inclusive range with actual storage:
`from = max(from, 0)`, `to = min(to, length - 1)`. An empty intersection returns -1.
They do not clamp an entirely absent range onto the nearest existing endpoint.
Fixed system scopes use the existing `SysArray::len`; affected-group runtime
search uses `ReadAffectedGroup { index: None }` for the live count with *either*
arity. Initializer search treats affected groups as empty before a note exists;
other supported initializer system scopes retain their existing zero-value
model, now respecting endpoints. `num_elements` and system element primitives
are unchanged. Range clipping, system init policy, error timing, and fixed
system capacities are **not native-observed parity claims**.

## CURRENT13 consumer trace (base line locations)

- `crates/sampler-ksp/src/lib.rs:846,897,907`: public compile/compile_with →
  compile_inner → initialize → compile_initialized.
- `lib.rs:1033-1106`: initialize_inner lex/preprocess/parse → sema::analyze →
  eval::run. `sema.rs:78-155` analyzes init, all functions, and other callbacks;
  `sema.rs:374,859,1138` route statement/expression builtins through the same gate.
- `init_cache.rs:324-364`: restore_initialized re-parses and re-runs sema before
  restoring owned state, so cached initializer data cannot bypass array admission.
- `eval.rs:244-339,382,448,703,965,983`: init and immediate persistence_changed
  evaluation → block/expr → builtin_inner; owned search at 1088, equality at 1138;
  system search previously returned 0 without endpoints at 989.
- `lib.rs:1128,1216-1288,1596`: compile_initialized_inner consumes the opaque
  initialized HIR/state and lowers callback-specific programs; Script owns init.cells.
- `lower.rs:1566,1868-1870,3177-3266,3430-3471`: builtin dispatch → Gen::search /
  Gen::array_equal. Init is not lowered; functions are lowered in their caller
  context by Unit::program. Existing runtime search did not intersect its explicit
  range, so ReadScriptArray's OOB fallback could masquerade as a zero match.
- `lib.rs:217,649-788`: Script::bind / bind_modules install initialized cells and
  programs per ScriptInstanceId on the Prepared plan.
- `crates/sampler-core/src/behavior.rs:1716,2228-2250,2371-2378,2629`:
  behavior_step reads owned arrays (OOB read zero, dropped OOB write), dispatches
  affected-group count/value reads, and routes Op instructions to Runtime::op_step.
- `script.rs:134-176`: continuation owner → plan generation → program's
  ScriptInstanceId → owned cell bank. `ops.rs:1030` consumes integer clipping ops.
- `prepare/selection.rs:733-788`: affected_group returns live mapped-group count
  for index None; absent indexed group returns -1. Search must not use capacity4096.
- `crates/sampler-kontakt/src/load.rs:726-802,817-898,1005-1006`: actual production
  source caller initialize_scripts → compile_ui_initialized → core lower_with /
  bind_modules. Failed initialization/compile is reported as unsupported and the
  module is omitted. No source bank was opened or product path executed here.

## Authored checks and limits

Dedicated auto-discovered target: `crates/sampler-ksp/tests/array_value_contract.rs`.
Thirteen authored tests (twelve default, one cache-feature test): positioned
real/length/text/system diagnostics in init and note; function/persistence and
cache re-admission; signed integer equality with callback mutation; type/arity
rules; unsupported system scopes/context errors; owned/fixed-system bounds;
live affected-group counts including empty maps; first match and -1 defaults.
Every test is **NOT_RUN / NOT_COMPILED**; runtime assertions describe intended
own behavior only. Existing `tests/params.rs::sort_and_array_equal_run_at_runtime`
and `tests/arrays.rs` are source-traced, not rerun.

Source-only verification performed:

- Independent Python static assertions mask only the advertised array spans and
  compare all remaining sema/eval/lower bytes against CURRENT13, proving SGN/random
  and unrelated branches unchanged. Tracked changed paths exclude all manifests
  and ledgers. Primary archive/excerpt hashes match.
- A Python model of the verified emitted clipping/loop premises matched an
  independent existing-index oracle in **8,748 cases**, making **7,508 in-bounds
  model reads**. It checks empty/reversed/extreme ranges and no signed index wrap.
  This is **not execution of Rust, generated bytecode, or the product**.
- Installed rustfmt 1.99.0 parsed all four changed/authored Rust files using
  `--edition 2024 --emit stdout`; no source-wide reformat was applied. The new
  regression file is formatted. Existing unrelated formatting remains untouched.
- `git diff --check` passed.

Full evidence is retained in coordinator handoff `pi-parity-release-arrays.md/json`
and `pi-parity-release-arrays-static.py`. No native invalid-range law, text/system
support law, native error continuation/timing, timeout/atomicity, or CPU/RAM result
was established. Init inner loops still have their existing fuel/accounting
limits; runtime search/equality retain existing callback instruction fuel and
continuation ownership. No automatic merge is authorized.
