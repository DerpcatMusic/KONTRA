# W10 numeric UVI UI metadata and readouts — READY

Product: `e1916ec51e7fd15bba5d6a49eb9fb30974180287`. References: `4bffbb18:src/uvi/host.rs:2626-2628` (native Unit/Mapper IDs) and `4bffbb18:src/ui/uvi_instrument.rs:155-187` (readouts). Unit and Mapper expose the real numeric constants; an unknown constant is nil. Existing v2 named metadata and normalized mapper helpers continue to work. Numeric mapper IDs project to the existing shared mapper names.

The v1 formatter runs in the UVI projection. Percent normalization, seconds/milliseconds, Hz/kHz, decibels, linear gain and center pan change the displayed text only. Raw values, ranges, strip position and edit payloads retain their own units. An authored nonempty `displayText` retains precedence. Nonzero Pan and UviFilter formatting remain unverified, as in the v1 source; this slice does not add guessed formats.

## Verification and pixels

One authored enum/readout contract failed first, then passed. It checks all native IDs, undefined constants, numeric and existing named mapper behavior, unchanged raw percent values, normalized edits, time/frequency/gain readouts, center pan and authored text precedence. The targeted class/construction/enum/module/UI/assets checks passed **28/28**. `sampler-uvi --no-run`, root `--lib --features shots --no-run` and `git diff --check` passed on the corrected worktree target.

The shared-renderer fixture adds an authored NumBox to the existing real Lua/XML page. The Button callback changes its raw value from `0.25` to `0.5`; the snapshots assert those values and `25 %`→`50 %` text. The renderer test passed. Both screenshots were visually inspected: the percentage field changes alongside Ready→M and red→yellow strip frames, at the declared positions. Screenshot hashes:

- Before: `7fbf12365ec4577ee22a711f3d5ef6d48ae100a9ca21eeeb441793217bca6a93`.
- After: `e493dfe0d93602d2cf05212c36af681bd4f05ca3c80f152da3d70800ddaada44`.

Exact receipt: `/mnt/Windows11/DEV_WORKSPACE/kontra-runs/w10-uvi-scan-344/ui-enums-READY.json`. It retains source, log and PNG digests, product/build provenance, wrapper/target and outcomes. Raw logs use `ui-enums-*.log`; PNGs are in `enum-ui-shots/`. The unit drained inactive/MainPID 0.

Protected 26-bank/660-program fidelity remains UNKNOWN/PARKED. Native-v1 cells requiring an official reader remain unavailable. Shared fractional geometry and authored paint-order work is owned by W3, with W1 handling publication; W10 owns UVI lowering/tests. Callback deadline acceptance remains open.

NEXT: authored value-control semantics; UVI geometry/order lowering when the shared contract is available.
