# NCKP precompile resource-name discovery

## Scope and status

Code and test source: `8bbbbb978e5ad09b62373f88cee447b71f70ba3d`, tree
`9613782fd95efd91fdd157f0693e29f4f8cfa389`. Parent/base:
`9075da681fbca5019640619822a4540bfd2a57a1`, tree
`ee617e185aecada9d74e22b5e1c2ad499af23f28`.

This change replaces the substring/first-quote resource-name scanner. It does
not implement Komplete UI, change NCKP JSON control schemas, or change resource
file access. No Rust build or test was run by this worker. Native runtime
parity is **UNKNOWN**. Comparative CPU and RAM acceptance is **UNACHIEVED**;
this change contains no comparable v1/Kontakt measurement.

## Primary native requirements

The immutable official NI KSP manual is used as the allowed static native-spec
check, not as native execution evidence:

- Source: [User Interface Commands / load_performance_view()](https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/user-interface-commands#load_performance_view--).
- Retrieved: `2026-10-10T02:13:00.540072+00:00`, HTTP 200, unversioned current
  manual. Archive: coordinator `handoff/official-docs/ksp-ui.html`.
- HTML SHA256: `ad269b9e26a32e9f1a8aea9b00d8e199af43551adb3b47cf71b93a7234e09d5e`.
- Section says: the argument is the `.nckp` filename **without extension**, as a
  string; only one view can be loaded per script slot; the command is available
  only in `on init`; it cannot be used with `make_perfview`; the file belongs in
  the resource container's `performance_view` subfolder. Contained controls
  become KSP variables with their Creator Tools names.
- The manual uses both `.nckp` and `.nkcp` in that section. The existing loader
  uses `.nckp`; this change does not reinterpret the apparent documentation typo.

Condition handling is checked against [Advanced Concepts / Preprocessor &
System Scripts](https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/advanced-concepts#preprocessor---system-scripts).
Archive: `handoff/official-docs/ksp-advanced.html`, retrieved
`2026-10-10T03:05:30.054823+00:00`, HTTP 200, SHA256
`10063fe89b2b8b2bb1f5176154293b4351dcb02c3ccb53a872dedb171a91df14`.
The section says excluded `USE_CODE_IF` regions do not reach the parser and
conditions are processed before script execution. It also specifies that
conditions pass to later slots. This discovery helper intentionally uses the
same empty inherited set as the current compiler; it does **not** establish
native cross-slot condition inheritance.

The scout's `handoff/roadmap-round4-rea-current-document.json` reports
`target_unavailable` (no app open). It is not a native witness. No native host,
official reader, installed instrument, proprietary script/resource, frozen
reference checkout, or extracted/decrypted asset was used here.

## Source trace at the code SHA

All paths below are pinned to `8bbbbb978e5ad09b62373f88cee447b71f70ba3d`.
Unchanged caller/compiler paths retain their exact base blobs.

| Path and lines | Symbol / role |
| --- | --- |
| `crates/sampler-ksp/src/nckp.rs:17–79` | `view_name`, `view_in`: existing lexer, source-order preprocessor and AST parser; exact command matching; single complete literal request in init |
| `crates/sampler-ksp/src/lexer.rs:83–227,233–313` | `lex`, `preprocess`: comments, strings, continuations, spans and source conditions |
| `crates/sampler-ksp/src/parser.rs:38–113,181–272,376–483` | `parse`, `statement`, `expr`: command argument structure and complete expressions |
| `crates/sampler-kontakt/src/load.rs:737–791` | `initialize_scripts`: discovery → `Resources/performance_view/{name}.nckp` → resource read → JSON parse → environment → initialize |
| `crates/sampler-ksp/src/lib.rs:997–1044` | `initialize_inner`: same lexer/preprocessor/parser, then semantic resolution using supplied performance controls |
| `crates/sampler-ksp/src/sema.rs:438–506` | `load_performance_view`, `performance_widget`: declare the supplied controls by name before initialization |
| `crates/sampler-ksp/src/eval.rs:794–849,1236–1241` | `apply_performance_view`, builtin dispatch: apply loaded page/control properties during init |
| `crates/sampler-kontakt/src/load.rs:806–911` | `compile_ui`, `compile_ui_initialized`: actual production/UI consumer used by the new caller-seam tests |
| `crates/sampler-kontakt/src/resources.rs:55–68,214–258` | `normalize`, `read_result`, `read`: existing namespace/traversal/size checks and existing Option read/error mapping; unchanged |

### Deliberate limits

- The public helper still returns `Option<&str>`. `None` means no discoverable
  literal request, including malformed syntax, a nonliteral argument, multiple
  syntactic requests, calls outside init or a `make_perfview` conflict. It does
  not mean the native feature passed or that the script compiled.
- Fully parenthesized literals are accepted. Comments, message strings,
  similarly named identifiers and inactive preprocessor branches are not calls.
- Neither `"base" & "suffix"` nor `"base" & @suffix` is partially accepted.
  Both return `None`, as do computed variables. `sema::fold` at lines 1241–1356
  can fold resolved HIR concatenations, but HIR resolution requires performance
  controls that have not yet been read. Re-running semantic analysis or init to
  discover a filename would introduce a new circular dependency or execute KSP.
  No second expression evaluator or dependency was added.
- Nested init statements are inspected for a unique literal. This is static
  declaration discovery, not a prediction of whether an `if`, `while` or
  `select` branch will execute. Multiple requests across branches are refused.
  Calls through user functions are not evaluated or followed. An unused function
  containing a request also makes discovery conservative (`None`).
- The existing lexer preserves backslashes in strings; it does not support an
  embedded escaped double quote. Discovery follows that exact supported grammar,
  not an invented escaping rule. Native escaping/case/resource-edge parity is
  **UNKNOWN**.
- The helper does not strip an authored extension, normalize an identifier's
  case, invent a file-dialog feature, or bypass the existing resource boundary.
  An authored `explicit.nckp` still produces the existing suffixed lookup
  `explicit.nckp.nckp` and missing-resource report.
- `None` leaves the existing default view environment in place. The compiler
  owns syntax/semantic diagnostics and can still create unbound fallback handles.
  This fix does not add a rejection diagnostic for every unsupported discovery
  form or certify that fallback rendering is native behavior.
- Scripts without the builtin text return early without allocating tokens/AST.
  For scripts containing it, parsing is an extra off-thread preparation pass
  whose memory is dropped before the compiler runs. No CPU/RAM savings or
  regression measurement is claimed.

## Regression source and integration request

New tests call the **production helper**, not a Python scanner model:

- `crates/sampler-ksp/tests/nckp.rs:4–58`:
  `resource_name_comes_from_a_complete_literal_init_command` (11 authored cases).
- `crates/sampler-ksp/tests/nckp.rs:61–94`:
  `resource_name_never_guesses_from_nonliteral_or_ambiguous_source` (22 authored
  cases, including malformed syntax, escaping limits, concat and multiple calls).

Production resource/control seam:

- `crates/sampler-kontakt/tests/nckp_resources.rs:66–104`:
  `precompile_discovery_loads_real_controls_not_comment_or_message_decoys`.
  Distinct authored real/decoy files; `compile_ui` must return the real width,
  named bound control and the init-written control value. Includes inactive code.
- `crates/sampler-kontakt/tests/nckp_resources.rs:107–135`:
  `precompile_discovery_does_not_read_a_fragment_or_later_unrelated_literal`.
  Authored fragment/unrelated/complete files all exist; dynamic and concatenated
  names must not populate any view from a guessed file.
- `crates/sampler-kontakt/tests/nckp_resources.rs:138–163`:
  `literal_resource_read_failures_keep_the_existing_load_diagnostic`.
  Missing, corrupt, traversal and explicit-extension cases retain the actual
  loader's performance-view `InvalidValue` report.

All fixture resources are created under a unique temporary directory owned by
that test and removed on drop. No NKI file or proprietary content is opened.

Source-only checks completed: direct `rustfmt --edition 2024 --check` on the
three Rust files; `git diff --cached --check`. These establish formatting/parser
syntax and whitespace cleanliness only, **not Rust type checking or test PASS**.
The previous scout's 5/5 ASCII scanner-model receipt is not reused as validation.

**NEXT:** The sole integration owner may include these targets in a future
combined serialized validation run after cherry-picking this code commit:

```sh
cargo test --locked -p sampler-ksp --test nckp
cargo test --locked -p sampler-kontakt --test nckp_resources
```

These are target selectors, not a request to interrupt the active batch or
launch immediate separate builds. No worker runs Cargo/rustc. Integration must
use the shared Cargo/sccache configuration without setting `CARGO_TARGET_DIR`
or `RUSTC_WRAPPER`, and pin the cherry-picked SHA/tree in each receipt.
Additional unchanged regressions: `sampler-ksp --test compile
performance_view_controls_and_indexed_properties_reach_the_model` and
`sampler-ksp --test ui
missing_performance_description_does_not_create_visible_unsized_knob`.
After source/runtime validation, native resource-edge witnesses and comparable
v1/Kontakt CPU/RAM measurements are still required for the overall acceptance.
