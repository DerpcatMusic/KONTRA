# NCKP real-resource value regression correction — source only

## Custody and scope

Base: `bac3c616824cf26e2a888c09cb21433b727d9079`, tree
`6a275cfca85d081b135449a46e11238f6b5aae6d`, version 0.3.403.
Test correction: `4a22450db8b18c6cc8b2f15d67e8327c6c902ccb`, tree
`5a08ae434cd3138deb297bfdf11c48567c248398`.
Own checkout: `/home/derpcat/.t3/worktrees/KONTAKTO/nckp-round6-fix`;
branch `pi/nckp-round6-fix`. Only changed Rust file:
`crates/sampler-kontakt/tests/nckp_resources.rs`.
Production code, parser, model, parameter catalog, fixtures, dependencies and
manifests are unchanged. No redundant VALUE property storage was added.
The coordinator explicitly accepted the evidence-backed test-only correction.

All changed/new Rust tests are **NOT_RUN**. Direct rustfmt formatting/syntax
and git whitespace checks passed; these do not prove compilation or execution.
No Cargo/rustc/clippy/build, runtime binary, native host, official reader,
protected library, generic gate/probe, install/publication, server, nested
agent or schedule was launched. Frozen worktrees and old receipts were not
changed. No target-directory or wrapper environment variable was set.

## Original RED and root cause

The original integration round5 resource test ran at the immutable base above:
`nckp-resources.log`, SHA256
`2e4cc31f2b81ad8f578a8f20d1345b5dc87bca960551fb080fa494e6c0c3e90f`.
Artifact SHA256:
`50d4f0624fa57bdbd9d815bada3011d81baf92bef5c6ecc02645601354304dad`.
Both hashes were independently rechecked without executing the artifact.
Its command/custody is recorded in the unchanged sibling
`nckp-resources.json`. Result: **2 PASS / 1 FAIL**, not GREEN.

Failure: original test line99 expected the generic property map to contain
`$CONTROL_PAR_VALUE = Int(17)` but found None. Assertions for the selected
resource width640, exactly one widget, `$Real` identity and a real control
had already passed. Thus the observed failure was not evidence of a wrong
resource or missing control. Three hypotheses were checked statically:
incorrect test storage expectation; loss of init state in compilation;
incorrect NCKP control mapping. The first explains the exact failure:

1. `nckp::walk/kind` maps the authored resource's index7 and name Real to a
   Slider named `$Real` (`nckp.rs:105–156`). `control:262–283` reads its
   min0/max100/default42. The original fixture is retained.
2. `initialize_scripts` discovers/reads/parses the resource, supplies it in
   the script environment and calls the real initializer (`load.rs:726–798`).
   `compile_ui` shares this path with the playable/UI consumer
   (`load.rs:806–847,873–905`). The test does not substitute a compiler.
3. Semantic resolution assigns described scalar controls `Home::Control`
   (`sema.rs:438–506`). `initialize_inner` evaluates actual init after
   semantic resolution (`lib.rs:984–1044`).
4. `Eval::set_property` handles scalar CONTROL_PAR_VALUE by calling
   `write_var` and returning **before** generic property insertion
   (`eval.rs:752–773`). `write_var` writes `st.controls[ui]`
   (`eval.rs:563–585`). This preserves a single source of current value.
5. `compile_initialized_inner` makes the host control default from that
   initialized control value (`lib.rs:1118–1158`). `model::assemble` makes
   `Widget.value = WidgetValue::Int(init.controls[i])`; generic properties
   come from a separate map (`model.rs:283–353`).
6. UI projection assigns typed `sampler_ui_ir::Widget.value` from this value
   (`ui.rs:478–488`) and a control binding from the host ID
   (`ui.rs:549–557`). The unrelated IR field `initial_value` is for scalar
   state without a numeric range; it is **not** the slider's value witness.
   The test does not assert it or the slider's reset/default range property.

Every source span above is pinned to the correction SHA, with file/span
SHA256 in `NCKP_ROUND6_FIX.json`. These production blobs are unchanged from
bac3. Source tracing is not a new runtime witness; the corrected assertion
has not yet been executed.

## Static native requirements check

Equivalent evidence here is the existing immutable **official NI KSP
manual**, not a native process. The previous REA receipt was target_unavailable;
no available native runtime witness is claimed. Re-reading the cached exact
sections avoids redownload and protected library/native execution.

Archive: coordinator `handoff/official-docs/ksp-ui.html`, SHA256
`ad269b9e26a32e9f1a8aea9b00d8e199af43551adb3b47cf71b93a7234e09d5e`.
Original retrieval: 2026-10-10T02:13:00.540072+00:00; current unversioned manual.
Rehash matched. Three exact sections were extracted again from that HTML;
URLs, extraction boundaries, normalized text offsets, full excerpts and
excerpt hashes are in the JSON manifest:

- [load_performance_view()](https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/user-interface-commands#load_performance_view--):
  “All contained controls are accessible as if they were declared and set up
  in KSP; variable names can be identified in Creator Tools.” Its filename,
  init-only, one-view and make_perfview restrictions remain unchanged.
- [get_ui_id()](https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/user-interface-commands#get_ui_id--):
  the official example calls
  `set_control_par(%ID[$i], $CONTROL_PAR_VALUE, $Set)` and explicitly says
  it uses these IDs “to set multiple knobs to the same value.” This is a
  current-control-value requirement, not a requirement for an internal
  generic property-map entry.
- [set_control_par()](https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/user-interface-commands#set_control_par--):
  changes a specified UI widget parameter, taking its UI ID and parameter
  value. There is no vendor requirement to duplicate scalar state in our
  internal property representation.

Preprocessor cache hash also rechecked:
`10063fe89b2b8b2bb1f5176154293b4351dcb02c3ccb53a872dedb171a91df14`
for `ksp-advanced.html`. Discovery/preprocessor policy is not changed.
These requirements justify asserting the real current control value; they do
not certify the authored JSON schema, missing-name fallback, our derived host
IDs, native numeric UI-ID allocation, or runtime parity. Native parity stays
**UNKNOWN**.

## Regression requirements and future integration gate

Corrected existing positive (`nckp_resources.rs:66–114`) retains all three
comment/message/inactive-preprocessor decoy cases and the true resource
width640/name `$Real`/one-control requirements. It now asserts **exactly**:

- actual `WidgetValue::Int(17)`;
- stable `derived_control_id(0, "$Real")` identity;
- a single host control named `$Real` with that ID and
  `ControlValue::Integer(17)` initialized default;
- one projected widget with typed `Value::Integer(17)`.

New sparse-description negative (`117–153`) verifies that a separately
referenced `$Missing` does not acquire a control, does not render and retains
its explicit unbound/NotModeled diagnostic. `$Real` still has the exact ID and
value17. This locks down our existing fallback policy, not vendor parity.
The original dynamic/concat negative (`156–184`) and all missing/corrupt/
traversal/extension diagnostics (`187–212`) are unchanged. No test was removed,
ignored, relaxed or assigned a tolerance. New target inventory is four tests.

Original helper receipt is still 2 PASS (33 authored cases), and original
resource negatives are still 2 PASS; none is a pass of this new source tree.
Receipt hashes are in the JSON manifest.

**Request to sole integration owner, next combined serialized batch only:**

1. `sampler-ksp --test nckp` — both discovery tests, unchanged33 cases.
2. `sampler-kontakt --test nckp_resources` — all four tests, no filter/ignore.
3. Related existing neighbors: `sampler-ksp --test compile
   performance_view_controls_and_indexed_properties_reach_the_model`;
   `sampler-ksp --test ui
   missing_performance_description_does_not_create_visible_unsized_knob`;
   `sampler-ksp --lib nckp::tests::nests_names_types_and_properties`.

These are target selectors for the coordinator's next authorized combined
build/direct-run manifest, **not** permission for a worker/per-lane rerun or
another build appended to round5. Use shared Cargo/sccache configuration;
never set/export CARGO_TARGET_DIR or RUSTC_WRAPPER. Pin the assembled source
SHA/tree and artifact/log hashes; preserve original round5 RED.

NEXT: coordinator cherry-picks the test correction followed by its audit
commit. Integration alone runs the next combined batch. If it is GREEN,
that proves only our tested production-control seam; authorized native
witnesses and comparable v1/Kontakt CPU+RAM measurement remain separate gates.
Significantly lower CPU **and** RAM than both remains **UNACHIEVED**.
