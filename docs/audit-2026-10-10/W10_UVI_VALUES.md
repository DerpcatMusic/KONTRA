# W10 UVI authored widget values — READY

Product: `aceb2f9e9ff6fff1cfee3ae949a4f2675ff5618e`. Reference: `4bffbb18:src/uvi/host.rs:1434-1447,2644-2678`, adapted to existing v2 engine bindings. Programmatic values retain independence from display endpoints. Integer writes truncate toward zero. Scalar/Table values, native scalar constructor endpoints/defaults, and Table defaults use v1 finite float32 conversion. Overflow fails before changing the old value or invoking its callback.

OnOffButton requires a boolean script value; renderer numeric edits convert to that boolean at the UI bridge. Momentary Button exposes no value/getValue/setValue/setRange control and remains stateless. Non-value widgets reject value methods. Bound parameter widgets use the existing parameterDefinitions.default rather than a parameter-name string; the engine parameter service remains authoritative.

## Verification

Three behavioral contracts failed first, then passed: range independence/integer truncation/button capabilities/toggle input; float32 defaults/writes and atomic overflow failure; and reuse of the real catalog default with a saved engine value. Targeted class/construction/enum/module/UI/assets/value checks passed **31/31**. `sampler-uvi --no-run`, root `--lib --features shots --no-run`, the real shared-renderer fixture and `git diff --check` passed on the corrected target. The unit drained inactive/MainPID 0.

The real renderer retained its declared strip positions and callback-selected red→yellow frames and `25 %`→`50 %` readout. Both PNGs were visually inspected; their hashes equal the enum/readout receipt.

Exact receipt: `/mnt/Windows11/DEV_WORKSPACE/kontra-runs/w10-uvi-scan-344/ui-values-READY.json`, with product/source/log/PNG hashes and wrapper/target. Logs use `ui-values-*.log`; screenshots are in `value-ui-shots/`.

This slice does not establish installed-bank or native Falcon parity. All 26/660 protected-bank cells remain UNKNOWN/PARKED. Shared f64 geometry and authored paint-order projection are in progress with W3/W1; W10 owns UVI lowering. Modifier transport and other untested widget method/constructor contracts remain open. Callback timing acceptance and the earlier off-CPU deadline cause remain open.

NEXT: UVI f64 geometry and authored child-order lowering when W3's shared field contract is available.
