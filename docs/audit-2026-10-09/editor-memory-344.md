# Conflux retained editor allocation accounting

Status: READY shared memory fixes, product source `58d035b615cf727b51c41b938dab091bbe32c76d`, branch `v2/fix-editor-memory-344`. Three actual failing-first regressions passed, along with eight Native and four picture checks and optimized CI area no-run. Fresh Conflux scanner before/after is `Original OK` with all three render hashes identical. Retained RSS improvement remains UNKNOWN because the matched harness retained unequal decoded art; this is not a native-host memory or timing claim.

## Checkpoint and artifact distinctions

W8 product-equivalent `6a1e031c` / `f0c00ea2` frozen stage probe measured 115.2734375 MiB before the editor and 198.51171875 MiB with the harness retained, a delta of 83.23828125 MiB. The before field is after load and at least 375 paced 64-frame audio blocks, not exact publication. The probe has no rejected heap-trim product change.

`audit_frames` creates a 1180x760 harness, whose constructor idles three frames, explicitly idles four more, holds it for four wall seconds, then samples Linux VmRSS/VmHWM before dropping it. It builds an element tree and MUI scene; it does not rasterize or create a GPU device. There are no GPU textures or atlas allocations in this checkpoint. Post-function `malloc_trim` runs after the harness drops, so its lower RSS does not isolate trimming with a live editor.

Frozen v1 product `0cb7a8a0`, stage instrumentation `2f51deec`, probe SHA256 `761f9c8ebaa0847d281e4d665d7776b9fdb1119a31e542b53840701fac5e7d6f`, measured 95.03515625 -> 114.53125 MiB (+19.49609375). Frozen SHA256SUMS passed for all seven artifacts. This is the same instrument/checkpoint, but v1 does not consume Conflux Native UI and its scanner reports missing images. It is not authored-feature parity. The frozen v1 plugin 0.3.152 is a different artifact from this stage probe.

## Attribution

Receipt directory: `/mnt/Windows11/DEV_WORKSPACE/kontra-runs/w3-editor-memory-344`.

Test-only instrumentation extends the existing numeric allocator recorder to bracket the retained editor, records glibc mallinfo2 before/after and counts Native Lua, package and decoded art bytes. It writes no instrument source, authored text, resource bytes or PCM. Stack traces record allocations of at least 4096 bytes; small allocations and memory outside the allocator are not fully partitioned. The stack recorder touches its own static pages, so its RSS is not an acceptance measurement.

A fresh settled, untraced baseline on `60d99edc` measured 114.7265625 -> 202.64453125 MiB (+87.91796875). Unlike W8's original checkpoint, this explicitly pumps frames until the asynchronous Native session exists and pictures have no pending jobs, then holds for four seconds. Otherwise the package can finish during the hold after frames stopped, leaving the harness on a loading scene. It does not invoke full GC or live-editor heap trimming.

Incremental live glibc allocations were 59,090,960 bytes (56.35 MiB); free arena space increased 18,733,040 bytes (17.87 MiB). Native counters were 22,394,716 Lua bytes, 14,032,736 decoded art bytes, 727,378 package bytes, zero pending pictures. Idle allocator capacity explains part of the RSS gap; these counters do not exactly partition every RSS byte.

The symbolized large-allocation trace retained 48,963,748 bytes (46.70 MiB): Lua 18.38 MiB, decoded pictures 13.34 MiB, MUI scene 6.36 MiB, MUI layout 5.25 MiB, fonts/shaping 1.96 MiB, other/resource allocations 1.40 MiB. Cumulative traced allocation was 312.56 MiB across 30,216 allocation/free events. The codec's final Vec-to-Arc conversion duplicates pixels transiently; opaque WebP additionally grew an RGB Vec geometrically to six bytes per pixel before converting it to four-byte RGBA.

## Lua initialization and repeated layout work

One `Session`/Lua VM is retained per editor id (`native_ui::State::view` creates it only when absent from LOCAL). `Session::new` initializes the VM and caches required modules in `package.loaded`; render uses its existing root function. We do not create a VM per widget or frame. Frozen v1 has no Native Lua UI implementation to transplant.

The same stack trace attributes 3,044,740 Lua allocation bytes to VM/module initialization, 22,665,048 to component graph rebuilding and 193,448,592 to host graph lowering/callback reads. Repeated recursive `flexibility` reads alone account for 97,313,368 bytes; the table-reading helper adds 45,185,608 bytes across its callers. Protected mlua string-key lookups allocate temporary Lua C closures, so repeatedly walking the same immutable subtree creates substantial churn. These are cumulative allocation bytes, not retained RSS.

A targeted synthetic regression repeats layout queries over one 64-level resolved graph, checks flexible geometry, bounds allocator calls, then verifies a fresh fixed graph produces new flags. A five-line product change reuses derived flags within each resolved graph; each render creates new graph nodes after parameter/state edits. This does not cache the component graph or require a new invalidation model.

## Shared candidate

The WebP decoder reserves the final RGBA capacity once, using the exact-reservation approach from v1 `0cb7a8a0:src/artwork.rs`. It retains dimension limits, cancellation/selection behavior and opaque alpha expansion. Its synthetic pixel-preservation regression failed before the fix at 2,621,572 peak bytes for 262,144 pixels (ten bytes per pixel).

The Native resolver retains resolved children and a projection of non-child properties. It no longer retains the raw child factories in numeric properties and `props.children` as well. Source property tables and callbacks remain reusable; callbacks that intentionally capture source properties keep those references. A weak-reference regression failed before the fix because an unused child factory survived full GC. It also checks sparse children and reusable properties/callbacks.

No library-specific workaround, graph cache, forced GC, heap trim, dependency or allocator tuning is introduced. Product changes touch `src/ui/native_runtime/runtime.lua` and `src/ui/picture_decode.rs`; diagnostics also touch `src/ui/native_runtime.rs`, `src/ui/native_ui.rs`, `src/ui/tests.rs`, `src/plugin.rs`, and `src/allocation_audit.rs`.

## Checkpoint correction after the first candidate observation

The first untraced candidate pair measured 123.3359375 -> 202.890625 MiB, but decoded 11,704,736 art bytes rather than the baseline 14,032,736. It is not an accepted memory comparison. Test-only `58d035b6` now requires three consecutive ready frames with unchanged decoded art bytes before the retained hold. The matched before and after binaries must use that same checkpoint, with full GC and allocator tracing disabled. The existing numeric receipts are preserved as diagnostic observations, not overwritten with accepted labels.

## Validation and next work

Receipts include `RED-2.log`, `LAYOUT-RED.log`, `ALL-GREEN.log`, `NEIGHBOR-NATIVE.log`, `NEIGHBOR-PICTURES.log`, `AREA-NO-RUN.log` and `PIXEL-CHECK.json`. Scanner candidate SHA256 is `99ccd8c61af869a015977045558359e781164ce959f1303b94004e75e1965ae1`; all three authored view hashes equal the frozen pre-fix renderer. No full suite, corpus-wide gate, Windows check or current-candidate CLAP host run was performed for this batch. W13 independently measures frozen381 real-host lifecycle memory; the candidate is excluded from that identity. Next work measures owner retention across close/destroy/reopen cycles, including renderer/device resources absent from this harness.

The identical three-stable-frame pair also rejected itself: pre-fix `af889092` retained 16,360,736 decoded art bytes and candidate `58d035b6` retained 14,032,736. The observed RSS deltas (+91.8671875 and +80.87109375 MiB) are diagnostic only. The candidate is justified by three actual failing-first shared-code regressions, not a claimed retained RSS saving. Further repetitive attribution was parked; numeric receipts remain intact.
