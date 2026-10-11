# Native UI readback retention — source candidate, not host acceptance

## Candidate custody

Own checkout: `/mnt/Windows11/DEV_WORKSPACE/kontra-worktrees/ui-takeover-20261010`, branch `v2/pi-ui-browser-status-20261010`.

Source stack after the preserved browser-caption pair:

1. `c381c09ff1e2c748117af0d0879fb6e2ab32346a`: actual Session regression (not executed).
2. `ab6cc66edc1962507230a2e48a272128c9b842dd`: local Session retention fix.
3. `dad99963a720496d5556a333f33a011f25e37b83`: publication-seed regression (not executed).
4. `50c17016a4cb2ed6ffe3c6414c389a7c7e00c31c`: same retention seam in both publication seed and local Session.

Integration's `68b8e747` contains the browser-caption slices, but neither this stack nor Native diagnostics `140c89a3` was present when equivalence was reported. Cherry-pick these four narrow commits in order, or compare their combined patch before taking an equivalent change. No W4 Mapping stack, f64/order stack, parser/DSP change, release/version change, installation or publication is included.

## Source finding and implementing paths

Pinned to **`50c17016a4cb2ed6ffe3c6414c389a7c7e00c31c`**:

- `src/ui/native_runtime.rs:615–638`, `update_widget`: retain an unchanged live typed value; normalize only `.value` for the full authored-widget comparison so an equal live override does not trigger a clone of every metadata field. Full metadata equality still covers kind, identity, binding, geometry, styles, captions, images and properties. A changed authored widget is copied normally.
- `src/ui/native_runtime.rs:814–850`, `Session::update_view`: source-slot match, shared widget update, meters, then numeric fallback only if typed input is absent and the authored value permits numeric telemetry. Same signature, lock and source identity as before.
- `src/ui/native_ui.rs:280–304`, `State::update_view`: use that same retention seam for the publication seed. The seed deliberately does **not** overlay scalar telemetry; the local Session still does. No new cache, snapshot owner, observer, worker or retained package is introduced.
- `src/ui/part.rs:472–485`: production publication/paint route; `State::view` in `src/ui/native_ui.rs:404` makes the second Session update. Neither pass is removed.
- `src/ui/native_runtime.rs:423–461`, `Parameter::value`, and `:532–570`, `ksp_control_property`: unchanged consumers of the published live value and authored metadata.

The old Session compared the whole Widget (including live `.value`) to authored state, reset it through `clone_from`, then cloned the typed override unconditionally. State's seed also copied the whole widget and typed value unconditionally. These are concrete repeated-copy mechanisms in source, **not** evidence of a Conflux freeze, a leak, or an application CPU/RSS win. Graph rebuild, upstream face materialization and render/present costs remain outside this slice.

Known bound: a divergent *authored* heap-valued value still needs a transient clone for comparison. The regression's zero-heap scope is unchanged live typed readback over authored scalar state, plus an already-warm meter map. Do not report universal zero-allocation Native UI.

## Native reference / precise documentation

Authoritative Kontakt KSP specification snapshots are held in `/mnt/Windows11/DEV_WORKSPACE/kontra-runs/pi-ui-round2-clone-20261010/`; `native-docs.json` records requested/resolved URLs and hashes. They are public manuals, not extracted instrument scripts or sample payloads.

- [NI UI control commands](https://docs.native-instruments.com/online-guides/ksp-manual/en/user-interface-commands), SHA256 `ad269b9e26a32e9f1a8aea9b00d8e199af43551adb3b47cf71b93a7234e09d5e`:
  - `#get_control_par--` / `#set_control_par--`: read/change the specified control's parameters; string variants support text and automation names.
  - `#get_control_par_arr--` / `#set_control_par_arr--`: indexed widget state, with real-array variants for XY axes and string-array variants for cursor automation names.
- [NI UI controls](https://docs.native-instruments.com/online-guides/ksp-manual/en/user-interface-controls), SHA256 `c736e2f0a27e8131a8cf8fa2669a8f1b2ad26a27dadcbc916c20ea00f4ed518d`:
  - `#ui_text_edit`: a string-variable-backed edit control and `ui_control` callback.
  - `#ui_table`: integer-array-backed columns, with selected-step state.
  - `#ui_xy`: real-array cursor axes; even indexes are X, odd indexes Y; axis range 0–1.

These immutable official native-spec receipts justify preserving typed state and live metadata rather than overwriting them with stale scalar telemetry. They are an equivalent native-spec check for the semantics protected here, **not** proof of the NativeUI `connect_parameter` API, Kontakt allocation strategy, callback ordering, or native runtime parity. The developer Native UI documentation endpoint returned HTTP500. REA tools were discovered and `binary_session` confirmed no target open; native UI capture/observation require macOS and are unavailable on this Linux host. No executable, Wine, official reader or third-party library opener was launched. Native host behavior and performance remain **UNKNOWN**.

Frozen own-v1 comparison: `0cb7a8a0b4d43086596a64c77320caa1b26d6d98:src/ksp/ui.rs:322–423`, `Ui::refresh`, separates metadata and value revisions and reuses typed storage with a bounded refresh budget. V1 has no `src/ui/native_runtime.rs` counterpart. This candidate follows its reuse principle; it does not claim a literal VM/runtime port or a v1 parity/performance measurement.

## Checks actually performed

- `git diff --check`: PASS.
- `rustfmt --edition 2024 --check src/ui/native_runtime.rs src/ui/native_ui.rs`: PASS (syntax/format only, no typecheck).
- Impeccable mechanical detector on the initial changed NativeRuntime target: no findings. No visuals change in this slice; no screenshot or runtime visual pass is claimed.
- `check_readback_model.py`: 13,824 finite old-vs-candidate source-transition comparisons PASS. It covers typed persistence/removal, authored scalar/text/array state, absent/present author widgets and scalar telemetry, and metadata changes. This is a Python semantic model, **not Rust execution or an allocation measurement**.
- Graft ask/skeleton/callers were used first; its v1 graph does not index branch NativeRuntime. Branch-local callers were then checked before the multi-file change.

No Cargo/rustc/build/clippy, stale binary validation, native capture, measurement, dev server or install was performed by this worker. Other worktrees and dirty work were not edited.

## One combined mega-batch validation request

Integration owns compilation and all Rust test execution. Include, in the same frozen combined batch:

1. `ui::native_runtime::tests::native_readback_reuses_unchanged_widget_storage_and_keeps_source_semantics` (`src/ui/native_runtime.rs:998–1106`): actual Session updates, existing thread-local allocator, 64 warmed repeated calls for each of five typed values; requires zero heap calls for this bounded fixture. Checks source isolation, meters, metadata changes, typed removal and preservation of declared text/array values against scalar telemetry.
2. `ui::native_ui::tests::native_seed_reuses_unchanged_readback_and_keeps_authored_scalar_policy` (`src/ui/native_ui.rs:1518–1584`): exact publication method before a local Session exists, no resource worker. Same bounded allocator check; other source remains unchanged, metadata update survives, scalar policy stays authored.
3. Existing `legacy_component_reads_the_published_ir_and_produces_a_typed_edit`, plus NativeRuntime/NativeUI regressions and root no-run in the coordinator's combined plan.

The prepared new assertions are expected to fail on their respective pre-fix implementations, but **RED has not been executed**. Do not split the user-requested mega-batch into per-worker compile cycles for that claim. Return actual pass/fail, candidate SHA/manifest, command and logs to UI.

NEXT: integrate the four source commits into the combined manifest and execute the bounded regressions. Then sample the exact combined host artifact on Conflux idle/drag/tab/scroll/reopen, alongside frozen v1 and Kontakt, with real readiness/drop attribution. This source change alone cannot satisfy the lower-CPU-and-RAM goal or close Conflux freeze attribution.
