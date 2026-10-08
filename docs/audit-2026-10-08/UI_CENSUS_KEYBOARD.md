# Shared scanner keyboard correction

The 348 paired loaded/no-audition observations in the original 9993 census reflect a scanner classification bug. They do not establish silence or a product playback failure. The affected LF-terminated manifest is frozen at `~/.cache/kontra-scan/keyboard-reset/items.tsv`, SHA-256 `cf0ccb4b6afdc131ed620a1a067e7fd9da91a346a4d1dd9c6516bd102a3dac1c`. This is the exact set from the KSP auditor's `audit/ksp-20261008@9f2aef8b` handoff; no library-specific MIDI rules were added.

NI's [keyboard-command reference](https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/keyboard-commands) defines key type NONE as a reset to ordinary host behavior. Colour INACTIVE resets the visual keyboard to black/white; NONE restores its native appearance, which can include an internal keyswitch. The shared scanner previously rejected type NONE and colour INACTIVE, and treated colour NONE as positive playability evidence.

The corrected scanner hard-excludes only explicit KSP CONTROL intent, while retaining the existing mapped-articulation keyswitch exclusion. Type NONE and colour INACTIVE/NONE fall back to mapped zones; colour NONE alone is not positive evidence. Type DEFAULT, mapped-default colour and explicit white remain preference hints intersected with zone coverage. An empty mapping may still use the existing flagged fallback, which is excluded from sound parity. An all-CONTROL keyboard still has no safe key. Shared note plans respect final exclusions.

V2 merges optional kind/colour properties independently in runtime slot order, so a later reset clears an earlier declaration and an unwritten property keeps its prior value. V1 reads its existing final instrument-global `HostState.keyboard`, avoiding a selected-page projection, and normalizes only fixed public numeric/symbolic enums. These are scanner-only changes. UVI's RGBA classification contract is separate and unchanged.

`keyboard_reason_counts` exports per-program fixed kind categories (default/control/reset/other/unset), colour categories (mapped-default/inactive-reset/native-reset/white/other/unset), explicit-control excluded-key count, zero reset exclusions, and `post-load-before-audition` phase. All 128 MIDI keys enter each category partition. Unavailable/old/UVI records retain null rather than invented zeros; no authored names, messages or property payloads leave memory.

The three shared Rust fixtures cover reset declarations with mapped coverage, explicit CONTROL plus independent last-writer property semantics, and an all-CONTROL keyboard. They run on both adapters. The Python exporter check verifies reason-count preservation and unavailable evidence. A targeted paired rerun uses separate corrected optimized probe binaries and per-item metrics caches under `~/.cache/kontra-scan/keyboard-reset/`; the historical census binaries, frozen v1 reference and old rows are preserved. Each collector invocation owns one <=235-second heavy shard. V2 establishes the numeric plan before v1, and final verification requires all 348 IDs, current binary/plan signatures and identical notes.

The first corrected v2 witness, 2 Horns KS, auditions `[60,64]` with its existing mapped-default preference and numeric keyswitch 24, and emits audible audio. The final snapshot contains 117 reset types and 11 CONTROL types; colours partition as 74 inactive-reset, 42 mapped-default and 12 other. Only the 11 explicit CONTROL keys are excluded. The former classifier rejected all 128 type declarations. These are generic policy results on a real witness, not a per-library key override. Full paired measurements and binary receipts follow. Native audio parity, UI gestures and product changes are outside this correction.

## Complete paired correction receipt

**All 348 formerly no/no IDs now audition audibly in both engines.** Every pair uses the same note/velocity; there are zero mismatches, zero fallback picks and zero no-safe-key observations. Both snapshots agree exactly on fixed category/exclusion counts for every ID. The source/digest and final cache-signature checks passed.

| Observation | Original v1 | Original v2 | Corrected v1 | Corrected v2 |
| --- | --- | --- | --- | --- |
| Loaded IDs | 348 | 348 | 348 | 348 |
| No safe audition key | 348 | 348 | 0 | 0 |
| Audible auditions | 0 (not measured) | 0 (not measured) | 348 | 348 |
| Silent auditions | 0 (not measured) | 0 (not measured) | 0 | 0 |

All corrected picks are classified `native_declared`: mapped-default colour hints intersect actual zone coverage. NONE/reset types do not certify playability or forbid the note. These are half-second scanner auditions, not native-host PCM parity or full scripting certification. The original frozen census is retained; combining its timing observations with these corrected probes would mix scanner versions.

Final category totals are identical on both sides (348 snapshots, 44,544 keys):

| Fixed category | Each engine |
| --- | --- |
| kind: control | 585 |
| kind: default | 0 |
| kind: other | 0 |
| kind: reset | 43959 |
| kind: unset | 0 |
| colour: inactive-reset | 31378 |
| colour: mapped-default | 12168 |
| colour: native-reset | 0 |
| colour: other | 998 |
| colour: unset | 0 |
| colour: white | 0 |
| excluded: explicit-control | 585 |
| reset-based exclusions | 0 |

V2 scanner source: `audit/ui-census-20261008@cf184ad40b40d14e5f8b1fc15bc5569d2860412d` (product 9993). Pinned-v1 probe source: `audit/ui-census-v1-scanner-20261008@238d33b4ab7cd1a28cbfba85e014338181e0959a` (product 0cb7a8a0). Three optimized keyboard fixtures pass on each; v2 shots no-run and v1 lib shots no-run passed before push, as did shared exporter/cache/privacy checks. The optimized probes were frozen outside the build targets before their idle targets were pruned. No installed plugin or frozen v1 reference was changed.

| Artifact | SHA-256 |
| --- | --- |
| kontra-scan-v2 | 2379cd944536d08f1e707efd5927cb01e5335b15b39555330b3479b3ed155189 |
| kontra_scan.py | 60967b3bdbeb0854bdfb523037a29d3d96276d318ced6b6fd3adb587e512859b |
| kontra-scan-v1 | 0242ba5a8c2c9fd0226d4433509f8a55acc7fb573901790a1920e9fc0aee150c |

Local evidence: [complete receipt](/home/derpcat/.cache/kontra-scan/keyboard-reset/COMPLETE.json), [build receipt](/home/derpcat/.cache/kontra-scan/keyboard-reset/BUILD.json), [reason totals](/home/derpcat/.cache/kontra-scan/keyboard-reset/reason-counts.json), [v1 results](/home/derpcat/.cache/kontra-scan/keyboard-reset/results/v1/results.tsv), [v2 results](/home/derpcat/.cache/kontra-scan/keyboard-reset/results/v2/results.tsv). Per-item numeric plans and cache signatures are retained alongside these files. Source touchpoints: `src/ui/scan.rs:504`, `tools/kontra-scan/metrics.rs:124`, `tools/kontra-scan/v1-instrumentation.patch:482`, and `tools/kontra-scan/kontra_scan.py:166`. Runtime view order follows the existing behavior/compiler order at `crates/sampler-kontakt/src/load.rs:718,859`; the scanner does not introduce a new execution order.
