# UVI named-parameter fault: exact exponential endpoints

Status: **SOURCE_READY; new/changed Rust tests NOT_RUN; native parity UNKNOWN.**
Performance acceptance (lower CPU and RAM than both frozen v1 and Kontakt) remains **UNACHIEVED**.

## Custody and ordered source slice

Exclusive checkout: `/home/derpcat/.t3/worktrees/KONTAKTO/uvi-setter-round6-fix`.
Branch: `pi/uvi-setter-round6-fix`.
Frozen base: `bac3c616824cf26e2a888c09cb21433b727d9079`, tree `6a275cfca85d081b135449a46e11238f6b5aae6d` (version 403).

1. Test source: `76d7682d5f17fc4aca2cb387939109b087a0489b`.
2. Product source: `10c29088a85776f50f395d5d58ec4ba87d3eb465`, tree `12fbc7fd14341569a5744cffdde58ba51366cac9`.
3. This audit is committed separately. It does not change product source.

Only two Rust files differ from the frozen base. No manifests, UVI production code, PCM fixture, proprietary material, other worker checkout, or frozen worktree were changed. No Cargo/rustc/build/test/clippy, Lua/ScriptHost execution, host/native process, reader, generic gate, server, install, publication, nested agent, or schedule was invoked. `rustfmt --check`, Git integrity checks and an independent Python arithmetic model are **source-only** checks.

## Diagnosis: physical bound is not an approximate decoded bound

The existing RED receipt is `pi-integration-round5/uvi-host-parameters.json`; its log SHA256 is `6b7b632bc059313f269de00d5af17b1e1aff8a244913b25b1e481cf47961ce63`. It records 17 passing neighbors and one failure, `named_parameter_checks_and_writes_do_not_build_full_catalogs`, with an initialization Lua fault. The old Rust assertion printed fault categories but not the retained Lua message. Thus the exact runtime message/line is not recovered from that log. The new assertion prints `findings()` and the endpoint Lua assertions name their failure.

Source-level cause of the first readback assertion:

1. The test's actual `ScriptHost::new` installs the prelude, builds the XML element identity, and runs the script body through `load_scripts`/`resume`.
2. `element.setParameter('Freq',99999)` obtains OnePole's exact declared bounds and clamps to `20000` (existing KONTRA policy).
3. `native.setParam` selects the insert's binding and calls the shared `EngineParameterLaw::normalized_value(20000)` before queue admission.
4. The binding is `Exponential { low: 20, high: 20000 }`. Before this fix, `decode(1000000)` used `exp(log(20) + (log(20000)-log(20))*1000000/1000000)`.
5. The independent numeric check gives `19999.99999999998`, below the **physical declared maximum** `20000`. Strict admission therefore rejects a valid endpoint, so no command or Lua readback overlay is accepted. The original `getParameter('Freq')==20000` assertion fails.

This is not an unsupported setter or a bad test fixture. OnePole Freq has a real supported binding; the existing synthetic cutoff PCM test passed on the base. The catalog declares 20–20000 Hz inclusive bounds. No original assertion was removed, ignored, relaxed, or replaced by acceptance of an unsupported write. Source/numeric diagnosis is not a rerun of the compiled fault; the integration owner must verify the diagnosis with the next combined batch.

## Minimal shared fix and caller/law review

`EngineParameterLaw::decode` now returns `low` and `high` exactly at the clamped normalized endpoints of the **Exponential** branch. Its interior expression and operation order are unchanged. **ShiftedExponential** reuses that same exponential conversion, then keeps the existing `max(low)-offset` operation. This covers the same numerical fault for the production AHDSR time law without a second approximation.

`normalized_value`, `valid`, `encode`, signatures, scopes, graph identities, queue admission and ACK behavior are unchanged. There is **no epsilon or tolerance**. Declared physical exterior values still fail strict comparison, including the immediately adjacent representable exterior floats. Exact endpoint Runtime control writes now use the same conversion as admission; fixing only an input comparison would leave incorrect decoded endpoint values and could reject decoded lower endpoints in UI validation.

All seven laws were inspected: SignedNormalized, Linear, Exponential, ShiftedExponential, AhdsrCurve, DecibelGain and CubicGain. Only the two exponential decode arms changed. Interior and non-exponential formulas remain byte-identical. The equal-bound Exponential case retains `encode`'s existing zero result; endpoint decoding is now exact. Interior formulas, including equal-bound interior behavior, are intentionally unchanged.

Direct `normalized_value` callers: UVI `native.setParam`, effects-editor `valid`, and own host/fixture/core-law tests. Decode callers: shared `normalized_value`; Runtime bound control writes; `ControlState::playing`; editor descriptor validation/position-to-value; group editor frequency/gain/parameter readbacks; Kontakt wavetable defaults (Linear) and effect gain (CubicGain); and own law, signal/user-override, UI and sound tests. Exponential cutoff and shifted-envelope production consumers therefore get the corrected endpoints without caller-specific tolerances or mirrors. No caller signature changes were needed. `evidence.json` pins the relevant source symbols and lines to the product SHA.

## Authoritative evidence, not native execution

Read-only immutable official archives were used as the task's allowed requirements-equivalent evidence rather than opening a native binary or running an official host:

- `https://lua.uvi.net/_elements.html#OnePole`, SHA256 `ddca396e24ff7a05dbd3be9b2b073cb5a4a7adfb67fbd00d75e597f6c0a08d0f`: Freq is float, min `20.000`, max `20000.000`, default `1000.000`, unit Hz, description “Filter cutoff frequency”. This is the precise physical range at issue.
- `https://lua.uvi.net/class_element.html#a876941566822f111c98750e3c954a94c`, SHA256 `b4516f54b9fd2061c5ba134afc30930b4c7922a4ed78b3badd022a395e5b5200`: get/set use name or ID and the declared value type; unknown names/IDs and mismatched Lua types are silently ignored.
- The same Element archive's `getParameter` anchor `a249fb1fa90b51d0631486cb422b05841` and `hasParameter` anchor `a51567c163828d317b94d2c6f02eae4a8` specify typed readback and boolean existence by name or ID.

Both complete archive digests were rechecked; exact sections/rows and acquisition records are in `evidence.json`. These are current unversioned UVI/Falcon documentation, not proof of an installed native version's numerical behavior. The geometric service normalization, normalized 0–1000000 command range, finite/out-of-range rejection and Lua out-of-range clamping are KONTRA implementation contracts, not certified native UVI requirements. No Kontakt numerical parity is claimed for the shared shifted law. No vendor evidence establishes the local Custom/Destination retention model or fixed queue capacity.

## Regression source and known neighbors

New core test `engine_parameters::law_tests::exponential_parameter_endpoints_are_exact_and_reject_exterior_neighbors` checks exact low/high decode and admission, normalized clamping, degenerate endpoint range, immediately adjacent exterior rejection, immediately adjacent interior acceptance, NaN and both infinities, and interior midpoint roundtrip. It covers OnePole's exact range and both production shifted-envelope time ranges. The old implementation would fail its exact endpoint assertions; this predicted RED has **not** been executed.

The original `named_parameter_checks_and_writes_do_not_build_full_catalogs` test retains every old condition: named existence, numeric identity, invalid numeric read errors, wrong Bypass type rejection, no eager per-element full catalog, local Custom readback and isolated catalog descriptors. It adds low-endpoint readback, exact command count/order `[1000000,0,normalized(100),normalized(100)]`, exact cutoff address/XML insert identity, and the sole final Freq override. This witnesses the actual production `ScriptHost` queue seam, not a fake acceptance table. These additions are **NOT_RUN**.

The other 17 host tests, the script unit tests, and all three own synthetic PCM fixtures remain byte-identical to the base. Rehashed base receipts report 17 host PASS / 1 FAIL, 19 script unit PASS and three exact own PCM PASS. Those results apply only to frozen `bac3`, not this source slice. Unsupported catalog fields, wrong types, unknown names/IDs, full queue rejection and Custom/Destination local-model behavior are unchanged.

## Exact future combined validation request

**Integration owner only, one future combined mega-batch. No fix append or per-lane rerun.** Add the new core test to that batch's build/list/expected-test manifest. Preserve the existing full targets/filters:

- sampler-core lib: `engine_parameters::law_tests::` (two tests, including the new endpoint test).
- sampler-core existing controls and shared user-override neighbors, as in the combined manifest.
- sampler-uvi host_parameters target: all 18 tests, unfiltered.
- sampler-uvi lib: `script::tests::` (19 existing neighbors).
- sampler-uvi fixture target, only these three exact owned synthetic tests:
  - `initialized_controller_and_widget_cutoff_writes_change_production_pcm`
  - `streamed_bus_gain_has_live_catalog_defaults_and_controller_readback`
  - `set_parameter_reaches_the_runtime`

Use `--test-threads=1`; use `--exact` for the three fixture names. No entire UVI/proprietary fixture target or generic gate fallback. Existing compilation and runtime artifact digests must not be reused as validation of the new source SHA.

NEXT: parent assembles and freezes a new combined candidate; integration records its one authorized build, exact test lists/filters, binary hashes, logs and custody. A fresh GREEN result is still required. Native execution parity remains UNKNOWN, CPU/RAM acceptance remains UNACHIEVED, and the separate type-recognized insert versus actually admitted DSP-lane inventory/ACK limitation remains open. This numerical fix does not broaden admission to unregistered processors or fix that separate gap.
