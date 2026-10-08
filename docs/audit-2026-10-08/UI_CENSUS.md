# Whole-corpus Original UI census

Scope 5, 2026-10-08. Coverage: **COMPLETE**. Frozen installed corpus: **834 Kontakt paths (781 NKI + 53 NKM), 660 UVI programs; 1,494 item IDs**. One row per path/program ID; each Kontakt multi includes every embedded program observed by its production loader. Scope 5 adds scanner instrumentation and audit documentation; product fixes belong to the frozen integration checkpoint. Generated from the current cache at 2026-10-08T18:37:47+00:00.

**Load admission, authored UI painting and audible audition are separate results.** The plain per-instrument answer is in [v1.tsv](/home/derpcat/.cache/kontra-scan/results/v1.tsv) and [v2.tsv](/home/derpcat/.cache/kontra-scan/results/v2.tsv). `loads=yes` means the production importer and initial playable bank/plan returned successfully. A missing image, script callback fault or silent note can coexist with admitted loading. `loads=no` includes a bounded 90-second worker timeout; it is an observed failure under this probe, not proof of permanent incompatibility.

## Frozen builds and reproducibility

Current v2 product checkpoint: `integrate/core-v2@9993db691a5f69d31980357694a678e358785e5e`, with the shared scanner extension. Native observer source: `tools/kontra-scan-native-9993-20261008@ef88dcb6bd5e50ff12ad8317bdb73da92fcd415f`. The installed README distinguishes the actual frozen binary source from later instrumentation. Historical partial v2 `7e82b152` results are preserved at audit checkpoint `236d3882` and do not enter these current-revision counts. Pinned Kontakt v1: `0cb7a8a0` plus scanner adapter `audit/ui-census-v1-scanner-20261008@59c6cbbbcc72cc38efda00fbde8cf9c2be8b4076`. UVI v1 is explicitly a later, separate baseline: sidecar `audit/uvi-v1-scanner-20261008@026bdbb49f29a5ad752b3470a5f6f64a20a8957d`, product base `4bffbb18`; pinned Kontakt is unchanged. All are optimized release. The adjacent [installed README](/home/derpcat/.cache/kontra-scan/bin/README.md) records exact binary hashes, build date, rebuilding and limits.

| Adapter | SHA-256 |
| --- | --- |
| v1 | 870cea2140b5c9db5361831664966848302a2f57e82fcb6c7ed1b3534545ce5e |
| v2 | 19e2f2c76771ceeb1f5db47956d404a290e16ef61dabb6e34909176b362b6751 |
| v1 UVI sidecar | 34a61e82ca6f09afc0eab692a1bf0b6765edbd7d063fccca94d95e699a93b9c1 |

```sh
~/.cache/kontakto-heavy ~/.cache/kontra-scan/bin/kontra-scan-v2 \
  --list ~/.cache/kontra-scan/v2-items.tsv --start 0 --count 25 \
  --out ~/.cache/kontra-scan/results/v2
```

Rerun the same arguments after exit 75, then advance the slice. Use the same command with `kontra-scan-v1` for the paired adapter. Every shard owns exactly one heavy call, defaults to 235 seconds, and starts a new item only if its full timeout fits. Per-item cache identity includes the binary, item/container size and mtime, sidecar and shared note-plan digest. Canonical results exclude stale revisions/signatures. No decrypted scripts, resources, PCM, keys, authored error messages, names or property payloads are persisted; counters/hashes and a small gallery of OUR renders are retained.

Validation: stdlib driver/cache/privacy checks pass; three optimized Rust scanner checks pass on each baseline; both required wrapper `cargo test --release --features shots --no-run` checks passed before push. The phase tests exercise actual evaluator/VM boundaries, including persistence failure suppressed by a successful public compiler result, v1 waiting callbacks and compiler-disabled blocks.

## Exhaustive category counts

Counts below include only the current installed revision and manifest IDs. Categories are mutually exclusive with precedence budget-hit → error → blank → missing-images → no-ui → original-ok. Mechanisms later overlap; their counts cannot be added as instruments unlocked. `original-ok` certifies requested-image resolution and successful authored-view construction/paint, not vendor pixel, gesture, typography, automation or callback parity.

| Build | Rows / 1494 | Kontakt / 834 | UVI / 660 | Loads yes | Loads no | Original OK | Missing images | Blank | No UI | Error | Budget hit | Audible | Silent |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| v1 | 1494 | 834 | 660 | 844 | 650 | 732 | 142 | 0 | 0 | 620 | 0 | 522 | 4 |
| v2 | 1494 | 834 | 660 | 1493 | 1 | 772 | 663 | 0 | 0 | 59 | 0 | 1078 | 67 |

Paired coverage: **1494/1494**. V1 loads / v2 does not: **0** ([complete observed list](/home/derpcat/.cache/kontra-scan/results/v1-loads-v2-doesnt.tsv)). V2 loads / v1 does not: **649**. Fallback-note comparisons: **62**, excluded from parity. Both-loaded note mismatches: **0**, excluded from sound-regression claims. Counts are exhaustive only when the coverage marker is COMPLETE.

Same-note auditions among fully admitted, non-fallback pairs: **60 v1 audible / v2 silent**, and **0 v2 audible / v1 silent**. These are observed half-second probe differences, not native-host sound certification or permanent silence. Both versions used the same recorded note; resource residency, callback state and signal-graph stages require diagnosis before assigning a root cause. The per-item TSVs retain every note and result. No-safe-key programs remain unmeasured and are excluded.

| Library | Same-note v1 audible / v2 silent |
| --- | --- |
| Audio Imperia Dolce | 60 |

The UVI auditor’s prior stopped sample admitted AO **0/80 on v1 vs 80/80 on v2**, with v1 graph-preflight rejection and no timeouts. That earlier observation is separate from the current paired census. V1’s stronger typed UI/state does not imply stronger format/graph admission. The paired table above is the reproducible comparison at the declared baselines.

### Load onset and first authored frame (section J)

Numeric timing fields observe actual output/paint from the first production program import, with Original painting and audition concurrent. The lexical metadata prepass, process spawn and PNG/hash work are outside this clock. A multi shares the item clock. first_audio_ms observes the first finite, exactly nonzero output block; the audible result separately requires amplitude above1e-5. Missing/silent/no-safe-key output remains unknown, never a zero onset. These one-shot CPU-scanner wall times include machine contention and are not matched native-host or warm-cache performance acceptance.

| Build | Corpus | Timing field | Observed numeric | Unknown | Median ms | p95 ms |
| --- | --- | --- | --- | --- | --- | --- |
| v2 | Kontakt | first_audio_ms | 458 | 376 | 1700.12 | 4316.91 |
| v2 | Kontakt | ui_first_frame_ms | 834 | 0 | 479.95 | 3861.65 |
| v2 | UVI | first_audio_ms | 656 | 4 | 8775.31 | 14724.49 |
| v2 | UVI | ui_first_frame_ms | 651 | 9 | 10330.35 | 16619.17 |
| v1 | Kontakt | first_audio_ms | 482 | 352 | 1445.53 | 5728.55 |
| v1 | Kontakt | ui_first_frame_ms | 834 | 0 | 409.8 | 2239.53 |
| v1 | UVI | first_audio_ms | 40 | 620 | 2108.99 | 4150.02 |
| v1 | UVI | ui_first_frame_ms | 40 | 620 | 2116.92 | 4150.74 |

| Build | Product cache state | Rows |
| --- | --- | --- |
| v2 | cold | 1494 |
| v1 | cold | 1494 |

load_ms is unchanged and includes pinned-v1 deferred initial sample-bank preload; it is not first sound. cache_state describes the product metadata/header cache, not metrics reuse or OS page cache. Pinned Kontakt v1 scanner disables those cache reads/writes; frozen v2 has no product metadata cache, so those adapters report cold. UVI sidecar uses Worker::start after its metadata/assets prepass, includes required pre-audition native snapshots, and observes concurrent paint/audio. Its load_ms retains its separate earlier legacy origin. The common driver forces persistent decoded PCM caching off; that observed product condition is cold. Unknown remains explicit. OS cache is uncontrolled. Future integration cache paths require actual cache-hit telemetry before warm/cold acceptance.

### Per-library breakdown

| Build | Library | Rows | Loads | Does not load | Original OK | Missing images | Blank | Error | Budget |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| v1 | ANALOG STRINGS | 1 | 1 | 0 | 1 | 0 | 0 | 0 | 0 |
| v1 | Afflatus Chapter II Brass | 348 | 348 | 0 | 320 | 28 | 0 | 0 | 0 |
| v1 | Areia 1.2.0 [Audio Imperia] | 155 | 155 | 0 | 148 | 7 | 0 | 0 | 0 |
| v1 | Audio Imperia CHORUS | 42 | 42 | 0 | 42 | 0 | 0 | 0 | 0 |
| v1 | Audio Imperia Dolce | 77 | 77 | 0 | 77 | 0 | 0 | 0 | 0 |
| v1 | Conflux 1.1.0 [Native Instruments] | 51 | 21 | 30 | 0 | 51 | 0 | 0 | 0 |
| v1 | Morphology Evolved [Zero-G] rutracker.org | 1 | 1 | 0 | 1 | 0 | 0 | 0 | 0 |
| v1 | Pacific Ensemble Strings | 49 | 49 | 0 | 0 | 49 | 0 | 0 | 0 |
| v1 | Performance Samples Vista | 7 | 7 | 0 | 0 | 7 | 0 | 0 | 0 |
| v1 | Solo | 100 | 100 | 0 | 100 | 0 | 0 | 0 | 0 |
| v1 | UVI - Augmented Orchestra v1.1.2-R2R | 620 | 0 | 620 | 0 | 0 | 0 | 620 | 0 |
| v1 | Una Corda Library | 3 | 3 | 0 | 3 | 0 | 0 | 0 | 0 |
| v1 | VWinds - Clarinets | 16 | 16 | 0 | 16 | 0 | 0 | 0 | 0 |
| v1 | VWinds - Double Reeds | 15 | 15 | 0 | 15 | 0 | 0 | 0 | 0 |
| v1 | VWinds - Flutes | 9 | 9 | 0 | 9 | 0 | 0 | 0 | 0 |
| v2 | ANALOG STRINGS | 1 | 1 | 0 | 1 | 0 | 0 | 0 | 0 |
| v2 | Afflatus Chapter II Brass | 348 | 348 | 0 | 348 | 0 | 0 | 0 | 0 |
| v2 | Areia 1.2.0 [Audio Imperia] | 155 | 155 | 0 | 155 | 0 | 0 | 0 | 0 |
| v2 | Audio Imperia CHORUS | 42 | 42 | 0 | 42 | 0 | 0 | 0 | 0 |
| v2 | Audio Imperia Dolce | 77 | 77 | 0 | 77 | 0 | 0 | 0 | 0 |
| v2 | Conflux 1.1.0 [Native Instruments] | 51 | 51 | 0 | 1 | 0 | 0 | 50 | 0 |
| v2 | Morphology Evolved [Zero-G] rutracker.org | 1 | 1 | 0 | 1 | 0 | 0 | 0 | 0 |
| v2 | Pacific Ensemble Strings | 49 | 49 | 0 | 4 | 45 | 0 | 0 | 0 |
| v2 | Performance Samples Vista | 7 | 7 | 0 | 0 | 7 | 0 | 0 | 0 |
| v2 | Solo | 100 | 100 | 0 | 100 | 0 | 0 | 0 | 0 |
| v2 | UVI - Augmented Orchestra v1.1.2-R2R | 620 | 619 | 1 | 0 | 611 | 0 | 9 | 0 |
| v2 | Una Corda Library | 3 | 3 | 0 | 3 | 0 | 0 | 0 | 0 |
| v2 | VWinds - Clarinets | 16 | 16 | 0 | 16 | 0 | 0 | 0 | 0 |
| v2 | VWinds - Double Reeds | 15 | 15 | 0 | 15 | 0 | 0 | 0 | 0 |
| v2 | VWinds - Flutes | 9 | 9 | 0 | 9 | 0 | 0 | 0 | 0 |

## Exhaustive measured failure mechanisms

At frozen checkpoint 9993, Kontakt Source.read consumes Resources.read as Option; typed read_result failures are masked by that compatibility adapter. A reported lookup-not-found therefore does not prove an absent file: invalid, ambiguous, inaccessible or corrupt resources can yield the same observation. Request namespace and own-index attribution are not exposed by this frozen collector and remain unknown. Future typed resolver changes require a separate checkpoint measurement.

An unsupported parameter can be nonvisual metadata. An outside-page or zero-size widget can be authored intentionally. The frozen scalar criterion excludes typed text/array/service bindings. Separate bound_typed counts validate text/array targets in installed KSP models; they do not prove live typed edits. UVI targets and phantom-free controls stay unknown where the frozen baseline has no accessor or origin marker. A page mostly one colour is an unreadability candidate, not proof of native mismatch. Those distinctions are retained in the report rather than labeling every occurrence broken.

| Mechanism | v2 item incidence | v1 item incidence |
| --- | --- | --- |
| geometry: outside authored page candidate | 1066 | 457 |
| custom font requested | 761 | 0 |
| font declaration not resolved by service | 717 | 0 |
| image lookup/decode failure | 664 | 142 |
| UI missing-images | 663 | 142 |
| unsupported_params: unsupported UI feature (private identifier omitted) | 659 | 0 |
| placeholder_widgets: ui_level_meter | 621 | 0 |
| passive paint changes semantic value | 616 | 0 |
| no safe audition key; sound unmeasured | 348 | 348 |
| selected-note audition silent in 0.5-second probe | 67 | 4 |
| audition uses fallback note; parity excluded | 62 | 29 |
| UI error | 59 | 620 |
| zero retained sample zones: unknown | 56 | 23 |
| visible widget lacks scalar readback binding | 53 | 0 |
| Native bridge candidate geometry: outside authored page candidate | 51 | 0 |
| Native bridge candidate geometry: zero sized visible widget | 51 | 0 |
| Native bridge candidate placeholder_widgets: ui_level_meter | 51 | 0 |
| Native bridge candidate placeholder_widgets: ui_text_edit | 51 | 0 |
| Native bridge candidate unsupported_params: $CONTROL_PAR_NKS_NUM_VALUES | 51 | 0 |
| Native bridge candidate unsupported_params: $CONTROL_PAR_NKS_STR_VALUES[] | 51 | 0 |
| Native bridge candidate unsupported_params: $CONTROL_PAR_NKS_STYLE | 51 | 0 |
| Native bridge candidate unsupported_params: $CONTROL_PAR_NKS_TYPE | 51 | 0 |
| page >90% plain background candidate | 51 | 0 |
| Native bridge candidate unsupported_params: $CONTROL_PAR_CUSTOM_ID | 50 | 0 |
| KSP compiler rejection | 30 | 0 |
| KSP init not_started | 30 | 0 |
| KSP persistence_changed not_started | 30 | 0 |
| Lua init fault | 28 | 0 |
| placeholder_widgets: ui_table | 22 | 0 |
| unsupported_params: $CONTROL_PAR_CUSTOM_ID | 15 | 0 |
| unsupported_params: $CONTROL_PAR_NKS_STYLE | 15 | 0 |
| placeholder_widgets: ui_waveform | 3 | 0 |
| geometry: zero sized visible widget | 2 | 0 |
| bounded worker timeout | 1 | 0 |
| unsupported_params: $CONTROL_PAR_AUTOMATION_ID[] | 1 | 0 |
| unsupported_params: $CONTROL_PAR_CURSOR_PICTURE[] | 1 | 0 |
| KSP persistence_changed waiting | 0 | 44 |

### Script-slot, callback and saved-state partitions

Raw slots are partitioned before compilation into decode_failed / bypassed / inline_nonempty / linked_only / empty. Only actual record/parameter errors count as decode_failed; saved-table uncertainty retains decoded source disposition independently. Wire slot, owner and program index are distinct from compact runtime admission. Active slots skip bypassed/empty slots. V1 compile-admitted allows disabled non-init callback blocks; compile-clean requires zero `Program.errors`. Init and persistence_changed completion/faults are independently observed, never inferred from public `Ok`. `absent`, `compile_disabled`, `entered`, `completed`, `faulted`, `budget_stopped`, `waiting`, `deferred` and `dropped` remain distinct. Diagnostics contain a fixed safe category, static builtin and numeric location only.

| Field | v2 sum / observed rows | v1 sum / observed rows |
| --- | --- | --- |
| bound_typed | 352 / 834 | 246 / 834 |
| sample_zone_count | 17805342 / 1493 | 13419231 / 804 |
| slots_seen | 4715 / 834 | 4715 / 834 |
| slots_decode_failed | 0 / 834 | 0 / 834 |
| slots_bypassed | 0 / 834 | 0 / 834 |
| slots_inline_nonempty | 1143 / 834 | 1143 / 834 |
| slots_linked_only | 0 / 834 | 0 / 834 |
| slots_empty | 3572 / 834 | 3572 / 834 |
| active_script_slots | 1143 / 834 | 1143 / 834 |
| compiled_script_slots | 1098 / 1493 | 1053 / 834 |
| clean_compiled_slots | 1098 / 1493 | 1053 / 834 |
| disabled_block_errors | 0 / 1493 | 0 / 834 |
| init_callbacks_completed | 1098 / 1493 | 1053 / 834 |
| persistence_changed_completed | 519 / 1493 | 460 / 834 |
| load_fault_records | 0 / 1493 | 1719 / 834 |
| ksp_runtime_fault_records | 0 / 1493 | 0 / 874 |

| Build | Observed callback phase | Status | Slot observations |
| --- | --- | --- | --- |
| v2 | init | absent | 0 |
| v2 | init | compile_disabled | 0 |
| v2 | init | entered | 0 |
| v2 | init | completed | 1098 |
| v2 | init | faulted | 0 |
| v2 | init | budget_stopped | 0 |
| v2 | init | waiting | 0 |
| v2 | init | deferred | 0 |
| v2 | init | dropped | 0 |
| v2 | init | unknown | 0 |
| v2 | persistence_changed | absent | 579 |
| v2 | persistence_changed | compile_disabled | 0 |
| v2 | persistence_changed | entered | 0 |
| v2 | persistence_changed | completed | 519 |
| v2 | persistence_changed | faulted | 0 |
| v2 | persistence_changed | budget_stopped | 0 |
| v2 | persistence_changed | waiting | 0 |
| v2 | persistence_changed | deferred | 0 |
| v2 | persistence_changed | dropped | 0 |
| v2 | persistence_changed | unknown | 0 |
| v1 | init | absent | 0 |
| v1 | init | compile_disabled | 0 |
| v1 | init | entered | 0 |
| v1 | init | completed | 1053 |
| v1 | init | faulted | 0 |
| v1 | init | budget_stopped | 0 |
| v1 | init | waiting | 0 |
| v1 | init | deferred | 0 |
| v1 | init | dropped | 0 |
| v1 | init | unknown | 0 |
| v1 | persistence_changed | absent | 549 |
| v1 | persistence_changed | compile_disabled | 0 |
| v1 | persistence_changed | entered | 0 |
| v1 | persistence_changed | completed | 460 |
| v1 | persistence_changed | faulted | 0 |
| v1 | persistence_changed | budget_stopped | 0 |
| v1 | persistence_changed | waiting | 44 |
| v1 | persistence_changed | deferred | 0 |
| v1 | persistence_changed | dropped | 0 |
| v1 | persistence_changed | unknown | 0 |

v2 saved-table integrity (raw slot observations): decoded=4715. Only complete raw histograms enter the counts below; these are known-subset counts, and all-slot raw totals remain unknown when any histogram is incomplete.

v1 saved-table integrity (raw slot observations): unknown=4715. Only complete raw histograms enter the counts below; these are known-subset counts, and all-slot raw totals remain unknown when any histogram is incomplete.

| Build | Fixed sigil | Raw complete-table entries | Admitted entries |
| --- | --- | --- | --- |
| v2 | $ | 202124 | 202124 |
| v2 | ~ | 0 | 0 |
| v2 | % | 30030 | 30030 |
| v2 | ? | 1 | 1 |
| v2 | @ | 2004 | 2004 |
| v2 | ! | 2707 | 2707 |
| v2 | empty | 0 | 0 |
| v2 | other | 0 | 0 |
| v1 | $ | 0 | 201164 |
| v1 | ~ | 0 | 0 |
| v1 | % | 0 | 28935 |
| v1 | ? | 0 | 1 |
| v1 | @ | 0 | 1974 |
| v1 | ! | 0 | 2497 |
| v1 | empty | 0 | 0 |
| v1 | other | 0 | 0 |

Saved sigils use only `$ ~ % ? @ ! empty other`; malformed table framing is distinguished from params() returning an empty Vec. Raw and admitted counts do not prove declaration-aware restoration. The manifest contains no NKSN snapshots; the separate prior 1,103-snapshot audit is not this denominator.

## Conflux mandatory first witness

**v2: loads yes; Original original-ok; bindings 74/80; audition yes; note {"0":[60,64]}; load 351.013923 ms; first sound 400.716472 ms; first authored frame 744.47229 ms; product cache cold; peak RSS 344.2 MB.**

Main authored view: 378 widgets, 101 visible, 69/75 scalar bindings; 4 legacy asset declarations / 0 observed missing resources; actual lookup 167/167, decode 33/33. Native consumer attempted True; Native paint OK True; decoded package fonts 0. Declared background [240, 239, 228, 255]; plain fraction 0.0000%. This fraction measures pixels matching the retained legacy declared background; a Native package can cover that colour completely. It does not establish the effective Native page background. The renderer auditor’s historical 94.94% figure belongs to its earlier capture/layout.

**v1: loads yes; Original missing-images; bindings 78/78; audition yes; note {"0":[60,64]}; load 134.102572 ms; first sound 146.11175200000002 ms; first authored frame 354.094091 ms; product cache cold; peak RSS 88.96 MB.**

This checkpoint includes the integration owners’ script, widget, Native frontend and load fixes. V2 Conflux was measured again against the frozen current binary; the unchanged v1 baseline reuses its earlier signature-matched per-item witness. Their timings are not a simultaneous benchmark. Historical claims that Conflux had no Native consumer or that its page was predominantly cream do not describe this new v2 checkpoint. Scalar and typed readback remain separate, and passive paint cannot certify pointer gestures, typography or automation. W0’s separate matched Conflux gesture test reports 124 witnesses passing; full corpus gesture coverage remains unknown.

## Ranked systemic fixes

Reach is an overlap-aware union of measured mechanism candidates or active source-token users, never a sum of occurrences. “Fully unlocked” is unmeasured for every fix until matched native render, gesture, callback and state tests pass. Ranking covers remaining observed failures and explicitly named qualification gaps. Already integrated Original-default and Conflux frontend repairs are historical context, not fresh failures. Candidate counts do not establish defects or the number fully unlocked. Effort S = localized existing-path repair, M = several adapters plus tests, L = shared typed service/lifecycle work. V1 is the first semantic reference; keep v2 ownership/real-time boundaries.

| Rank | Mechanism | Effort | Measured v2 candidate items | Fully unlocked | Root repair / v1 reference | Proof |
| --- | --- | --- | --- | --- | --- | --- |
| 1 | Remaining authored resource resolution | M/L | 664 | Unknown | Repair existing source-family lookup/decoder paths for the remaining requested resources; retain successful Native package routing. | Same item/page/value/DPI resolves every requested asset and produces an improved matched Original render. |
| 2 | Playable-range and same-note sound qualification | M | 415 | Unknown | Resolve unselected programs using authored key/state contracts; diagnose equal-note audible/silent differences with per-node signal-graph traces before assigning a DSP or streaming repair. Unselected sound remains unmeasured. | Declared-valid keys intersect non-purged/non-bypassed velocity64 coverage; both versions use the same recorded key and keyswitch. Trace the differing stages; no invented per-library notes. |
| 3 | KSP callback/compiler gaps remaining after integration | M | 30 | Unknown | Repair the measured static builtin/category at exact slot ownership; preserve actual init and persistence phase boundaries. | A failing-first fixture completes the repaired callback, then the same item loses its measured fault or compiler rejection. |
| 4 | Lua initialization, runtime and budget faults | M | 28 | Unknown | Close the measured scripted-worker API/budget gap using the existing host; do not replace scripts with the offline loader. | Same authored scene/callback completes, with init/runtime fault counts separate and budget stops measured. |
| 5 | Original paint and frontend completion | M/L | 59 | Unknown | Repair actual frontend/paint failures or bound incremental scene work; the successful Conflux Native path is already integrated. | Large and failure-prone scenes paint without budget/error placeholders; request, consumer entry and authored paint success are independently observed. |
| 6 | Remaining scalar/typed/service binding qualification | M/L | 0 | Unknown | Inspect only targets lacking both observed scalar and typed readback; separately qualify service-backed widgets and current callbacks. | Each actual widget edit reaches its owning target, reads back exact type/value and runs the correct callback. Passive paint alone cannot prove this. |
| 7 | Legacy geometry, parenting and strip fidelity | M | 1442 | Unknown | Confirm retained bridge candidates against authored layout before changing the shared geometry/frame path; hidden/outside widgets may be intentional. | Nested panels, axes, HiDPI, visibility and frame endpoints match at identical state. Native scene layout requires its own exposed inventory. |
| 8 | Remaining font and background fidelity | M | 717 | Unknown | Repair actual unresolved fonts or proven contrast/style mismatches; Native package font inventory is separate from graph font usage. | Real authored fonts/styles and text/background regions match. A colour coverage candidate alone does not prove unreadability. |
| 9 | Saved-state and snapshot qualification | M/L | 834 | Unknown | Qualify complete fixed-sigil restoration, menu semantic values, string-array empties and snapshot policies on the integrated path; unknown v1 raw histograms remain unknown. | State save/reopen and all four snapshot policies retain exact values before persistence_changed. The1494 manifest contains no standalone snapshots. |
| 10 | Corpus interaction and automation qualification | M | 1493 | Unknown | Extend the existing real gesture/host gate beyond Conflux; current passive census does not establish drag, wheel, reset or automation failure. | Actual pointer/key/wheel/reset/host gestures work on each widget family, with exact typed readback and callback ownership across waits. |

## Exhaustive spec incidence and status matrix

The generated scanner whitelist is the union of the repository’s UI widgets, CONTROL_PAR identifiers, UI helpers/callbacks and persistence helpers. Comments and strings are skipped; one leading underscore alias is canonicalized. The whitelist is generated from compiler UI/keyboard/persistence builtin tables plus CONTROL_PAR/spec inventory; its digest/count is attached to each metadata record. Active incidence includes inactive preprocessor branches/unreachable functions and is not an execution count. Bypassed incidence is separate. Linked unresolved/native-package declarations can be absent from lexical counters; actual renderer widget inventory supplements them. Zero means unobserved in the surfaced inline declarations, not proof of complete corpus absence; decode failures, linked/native packages and generated UI can hide lexical use even after the manifest is complete.

The expected contracts and historical 7e82 inspected status/source references below reuse the params auditor’s exhaustive matrix at `origin/audit/ui-params-20261008`. Its F1–F10 definitions and target tests are in [UI_PARAMS.md](https://github.com/DerpcatMusic/KONTRA/blob/f3242f451ce245a12ebe9fc99a64402572f2a8cd/docs/audit-2026-10-08/UI_PARAMS.md); rendering and gesture status is completed by [UI_RENDER.md](https://github.com/DerpcatMusic/KONTRA/blob/3c185d4dface744617d9407823032a1eb471bff8/docs/audit-2026-10-08/UI_RENDER.md), [UI_WIDGETS.md](https://github.com/DerpcatMusic/KONTRA/blob/c7781195f02910500577032741c061768eadf781/docs/audit-2026-10-08/UI_WIDGETS.md) and [UI_LOOP.md](https://github.com/DerpcatMusic/KONTRA/blob/982100c90d92ab31e127a542495d7ff45d86a3dd/docs/audit-2026-10-08/UI_LOOP.md). Historical statuses are not current 9993 failures: this checkpoint includes subsequent implementation fixes. Current execution/paint counts are shown separately; unmeasured current semantic support remains unknown. Correct in the historical matrix means inspected/tested mechanism, not complete vendor fidelity.

| Spec token | Expected contract | Historical 7e82 inspection / evidence / fix reference | v2 active items | v1 active items | v2 bypassed items | v2 token occurrences |
| --- | --- | --- | --- | --- | --- | --- |
| $CONTROL_PAR_ACTIVE_INDEX | I; active XY X-coordinate index or none | missing; opaque mirror, no active-cursor state; ui:538 → F2/F10 | 0 | 0 | 0 | 0 |
| $CONTROL_PAR_ALLOW_AUTOMATION | I boolean, idx for XY; init applicability | partial; scalar IR flag, no host export/per-cursor path; ui:498 → F2/F9 | 833 | 833 | 0 | 51570 |
| $CONTROL_PAR_AUTOMATION_ID | I; vendor host parameter ID, idx for XY | partial; scalar metadata, no host mapping or conflict/range policy; ui:498 → F2/F9 | 429 | 429 | 0 | 20578 |
| $CONTROL_PAR_AUTOMATION_NAME | S; host name, idx for XY | partial; scalar IR name only; ui:498 → F2/F9 | 777 | 777 | 0 | 28195 |
| $CONTROL_PAR_BAR_COLOR | I RGB color; relevant widget paint slot | partial; IR color fields, no complete meter/waveform/table paint; ui:54,457 → F7; render/widgets scopes | 101 | 101 | 0 | 326 |
| $CONTROL_PAR_BASEPATH | S; file-selector root path | partial; IR field only; no browsing or path-boundary service; ui:408 / eval:1259 → F4/F7/F10 | 10 | 10 | 0 | 61 |
| $CONTROL_PAR_BG_ALPHA | I; opacity component | missing; unsupported independent alpha and gradient fields; ui:538 → F7; render scope | 0 | 0 | 0 | 0 |
| $CONTROL_PAR_BG_COLOR | I RGB color; relevant widget paint slot | partial; IR color fields, no complete meter/waveform/table paint; ui:54,457 → F7; render/widgets scopes | 5 | 5 | 0 | 38 |
| $CONTROL_PAR_COLUMN_WIDTH | I; file-selector column pixels | partial; IR metadata only; ui:416 → F7/F10 | 10 | 10 | 0 | 13 |
| $CONTROL_PAR_CURSOR_PICTURE | S; asset name, XY cursor may be indexed | partial; scalar asset references emitted; typed getters and indexed cursor pictures missing; ui:508 / lib:307 → F2/F4; resolution: render scope | 1 | 1 | 0 | 1 |
| $CONTROL_PAR_CUSTOM_ID | I; user metadata tag | partial; numeric store/readback works, IR labels metadata unsupported; ui:538 → F7 | 51 | 51 | 0 | 285 |
| $CONTROL_PAR_DEFAULT_VALUE | I; reset target in raw control units | partial; IR field mapped; missing defaults/readback not unified; ui:309 → F4/F1 | 834 | 834 | 0 | 166129 |
| $CONTROL_PAR_DISABLE_TEXT_SHIFTING | I boolean; pressed caption shift policy | missing; unsupported property; ui:538 → F7; render scope | 0 | 0 | 0 | 0 |
| $CONTROL_PAR_DND_ACCEPT_ARRAY | I policy; accepted drop count/type | missing; no mouse-area drop service/context; ui:538 → F7/F10 | 0 | 0 | 0 | 0 |
| $CONTROL_PAR_DND_ACCEPT_AUDIO | I policy; accepted drop count/type | missing; no mouse-area drop service/context; ui:538 → F7/F10 | 0 | 0 | 0 | 0 |
| $CONTROL_PAR_DND_ACCEPT_MIDI | I policy; accepted drop count/type | missing; no mouse-area drop service/context; ui:538 → F7/F10 | 0 | 0 | 0 | 0 |
| $CONTROL_PAR_DND_BEHAVIOUR | I; label MIDI export policy/area identity | missing; opaque unsupported metadata; ui:538 → F7/F10 | 30 | 30 | 0 | 60 |
| $CONTROL_PAR_FILEPATH | S; selected file under basepath | missing; unsupported projection and no selected-file state; ui:538 / eval:1249 → F4/F10 | 0 | 0 | 0 | 0 |
| $CONTROL_PAR_FILE_TYPE | I enum; selector filter | partial; IR filter mapped; no picker/callback service; ui:410 → F7/F10 | 10 | 10 | 0 | 13 |
| $CONTROL_PAR_FONT_TYPE | I; factory/custom font ID | partial; default/state font lookup and rendering incomplete; ui:504 → F7; render scope | 783 | 783 | 0 | 1487266 |
| $CONTROL_PAR_FONT_TYPE_OFF_HOVER | I; font ID for interaction state | missing; stored as unsupported, no state style projection; ui:538 → F7; render scope | 0 | 0 | 0 | 0 |
| $CONTROL_PAR_FONT_TYPE_OFF_PRESSED | I; font ID for interaction state | missing; stored as unsupported, no state style projection; ui:538 → F7; render scope | 0 | 0 | 0 | 0 |
| $CONTROL_PAR_FONT_TYPE_ON | I; font ID for interaction state | missing; stored as unsupported, no state style projection; ui:538 → F7; render scope | 0 | 0 | 0 | 0 |
| $CONTROL_PAR_FONT_TYPE_ON_HOVER | I; font ID for interaction state | missing; stored as unsupported, no state style projection; ui:538 → F7; render scope | 0 | 0 | 0 | 0 |
| $CONTROL_PAR_FONT_TYPE_ON_PRESSED | I; font ID for interaction state | missing; stored as unsupported, no state style projection; ui:538 → F7; render scope | 0 | 0 | 0 | 0 |
| $CONTROL_PAR_GRID_HEIGHT | I; grid position/size | missing; opaque store; private move_control tags differ; eval:963 / ui:8 → F8 | 0 | 0 | 0 | 0 |
| $CONTROL_PAR_GRID_WIDTH | I; grid position/size | missing; opaque store; private move_control tags differ; eval:963 / ui:8 → F8 | 0 | 0 | 0 | 0 |
| $CONTROL_PAR_GRID_X | I; grid position/size | missing; opaque store; private move_control tags differ; eval:963 / ui:8 → F8 | 0 | 0 | 0 | 0 |
| $CONTROL_PAR_GRID_Y | I; grid position/size | missing; opaque store; private move_control tags differ; eval:963 / ui:8 → F8 | 0 | 0 | 0 | 0 |
| $CONTROL_PAR_HEIGHT | I; pixel size | partial; omitted getter = 0; one missing axis resets both; ui:423 / ir_view:137 → F8 | 803 | 803 | 0 | 1628198 |
| $CONTROL_PAR_HELP | S; caption/lines/value label/help/short caption | partial; init scalar/textline overlay mapped; runtime getters empty; aliases discarded; eval:925 / ui:479 / lower:2087 → F1/F2/F4 | 623 | 623 | 0 | 16767 |
| $CONTROL_PAR_HIDE | I mask; hide whole/parts or indexed XY cursor | partial; whole/inherited hide mapped; mod-light and indexed hide unavailable; ui:433,538 → F7/F10 | 834 | 834 | 0 | 6123805 |
| $CONTROL_PAR_IDENTIFIER | S read-only; declaration name without sigil | wrong; never synthesized from Widget.name; init getter returns "0", runtime ignored; eval:589,911 / lower:2087 → F4 | 0 | 0 | 0 | 0 |
| $CONTROL_PAR_KEY | Uncertain historical/vendor extension; needs profile-specific reference | missing; opaque symbol, no semantic handler; ui:538 → F7; do not invent units | 0 | 0 | 0 | 0 |
| $CONTROL_PAR_KEY_ALT | I read-only; interaction modifier snapshot | missing; no modifier fields in callback admission; control.rs:194 / lower:2920 → F4/F10 | 289 | 289 | 0 | 6166 |
| $CONTROL_PAR_KEY_CONTROL | I read-only; interaction modifier snapshot | missing; no modifier fields in callback admission; control.rs:194 / lower:2920 → F4/F10 | 481 | 481 | 0 | 23345 |
| $CONTROL_PAR_KEY_SHIFT | I read-only; interaction modifier snapshot | missing; no modifier fields in callback admission; control.rs:194 / lower:2920 → F4/F10 | 57 | 57 | 0 | 64 |
| $CONTROL_PAR_LABEL | S; caption/lines/value label/help/short caption | partial; init scalar/textline overlay mapped; runtime getters empty; aliases discarded; eval:925 / ui:479 / lower:2087 → F1/F2/F4 | 777 | 777 | 0 | 176100 |
| $CONTROL_PAR_MAX_VALUE | I read-only; declared bounds | partial; declared getters seeded; illegal writes override IR but not core domain; eval:621 / lib:745 / ui:305 → F4/F7 | 51 | 51 | 0 | 1887 |
| $CONTROL_PAR_MIDI_EXPORT_AREA_IDX | I; label MIDI export policy/area identity | missing; opaque unsupported metadata; ui:538 → F7/F10 | 30 | 30 | 0 | 60 |
| $CONTROL_PAR_MIN_VALUE | I read-only; declared bounds | partial; declared getters seeded; illegal writes override IR but not core domain; eval:621 / lib:745 / ui:305 → F4/F7 | 51 | 51 | 0 | 1122 |
| $CONTROL_PAR_MOUSE_BEHAVIOUR | I signed sensitivity; source gesture axis/travel | wrong; negative maps horizontal in IR; source/v1 slider semantics differ; ui:448 → F7; widgets scope | 783 | 783 | 0 | 22584 |
| $CONTROL_PAR_MOUSE_BEHAVIOUR_X | I; XY axis sensitivities | partial; IR stores absolute magnitude, not full gesture/coordinate semantics; ui:388 → F2/F10 | 1 | 1 | 0 | 2 |
| $CONTROL_PAR_MOUSE_BEHAVIOUR_Y | I; XY axis sensitivities | partial; IR stores absolute magnitude, not full gesture/coordinate semantics; ui:388 → F2/F10 | 0 | 0 | 0 | 0 |
| $CONTROL_PAR_MOUSE_MODE | I enum; XY click/drag policy | partial; IR field only, no typed XY input admission; ui:392 → F10 | 1 | 1 | 0 | 1 |
| $CONTROL_PAR_NKS_NUM_VALUES | Uncertain vendor extension; resolve its legal type/index/context | missing; interned/stored but unsupported semantic projection; ui:538 → F7 | 51 | 51 | 0 | 1483 |
| $CONTROL_PAR_NKS_STR_VALUES | Uncertain vendor extension; resolve its legal type/index/context | missing; interned/stored but unsupported semantic projection; ui:538 → F7 | 51 | 51 | 0 | 2554 |
| $CONTROL_PAR_NKS_STYLE | Uncertain vendor extension; resolve its legal type/index/context | missing; interned/stored but unsupported semantic projection; ui:538 → F7 | 51 | 51 | 0 | 2028 |
| $CONTROL_PAR_NKS_TYPE | Uncertain vendor extension; resolve its legal type/index/context | missing; interned/stored but unsupported semantic projection; ui:538 → F7 | 51 | 51 | 0 | 2315 |
| $CONTROL_PAR_NONE | I sentinel; no operation | wrong; generic stores and unsupported emission instead of no-op; eval:567 / ui:538 → F7 | 0 | 0 | 0 | 0 |
| $CONTROL_PAR_NUM_ITEMS | I read-only; current menu item count | partial; init computed, runtime not seeded/derived; eval:600 / lower:2928 → F4 | 51 | 51 | 0 | 51 |
| $CONTROL_PAR_OFF_COLOR | I RGB color; relevant widget paint slot | partial; IR color fields, no complete meter/waveform/table paint; ui:54,457 → F7; render/widgets scopes | 2 | 2 | 0 | 20 |
| $CONTROL_PAR_ON_COLOR | I RGB color; relevant widget paint slot | partial; IR color fields, no complete meter/waveform/table paint; ui:54,457 → F7; render/widgets scopes | 2 | 2 | 0 | 20 |
| $CONTROL_PAR_OVERLOAD_COLOR | I RGB color; relevant widget paint slot | partial; IR color fields, no complete meter/waveform/table paint; ui:54,457 → F7; render/widgets scopes | 0 | 0 | 0 | 0 |
| $CONTROL_PAR_PARALLAX_X | I; wavetable view displacement | partial; IR retains integer pair, no visualization; ui:397 → F7; render scope | 1 | 1 | 0 | 1 |
| $CONTROL_PAR_PARALLAX_Y | I; wavetable view displacement | partial; IR retains integer pair, no visualization; ui:397 → F7; render scope | 1 | 1 | 0 | 1 |
| $CONTROL_PAR_PARENT_PANEL | I panel UI ID; child local geometry/visibility | partial; valid lookup/nesting correct; default detach and cycle cases unverified; ui:530 / ir:557,567 → F8 | 31 | 31 | 0 | 780 |
| $CONTROL_PAR_PEAK_COLOR | I RGB color; relevant widget paint slot | partial; IR color fields, no complete meter/waveform/table paint; ui:54,457 → F7; render/widgets scopes | 1 | 1 | 0 | 4 |
| $CONTROL_PAR_PICTURE | S; asset name, XY cursor may be indexed | partial; scalar asset references emitted; typed getters and indexed cursor pictures missing; ui:508 / lib:307 → F2/F4; resolution: render scope | 834 | 834 | 0 | 1579274 |
| $CONTROL_PAR_PICTURE_STATE | I; explicit picture frame where supported | partial; scalar frame mapped, applicability/state behavior not enforced; ui:522 → F7; paint states: render scope | 726 | 726 | 0 | 155471 |
| $CONTROL_PAR_POS_X | I; local pixel position | partial; explicit mirror/rect, absent defaults and grid precedence differ; ui:423 → F4/F8 | 834 | 834 | 0 | 3219925 |
| $CONTROL_PAR_POS_Y | I; local pixel position | partial; explicit mirror/rect, absent defaults and grid precedence differ; ui:423 → F4/F8 | 834 | 834 | 0 | 3264133 |
| $CONTROL_PAR_RANGE_MAX | I; level-meter display bounds | missing; no meter-range field in IR kind; ui:401,538 → F7 | 0 | 0 | 0 | 0 |
| $CONTROL_PAR_RANGE_MIN | I; level-meter display bounds | missing; no meter-range field in IR kind; ui:401,538 → F7 | 0 | 0 | 0 | 0 |
| $CONTROL_PAR_RECEIVE_DRAG_EVENTS | I boolean; drag vs drop callback policy | missing; no source event payload; ui:538 / control.rs:194 → F10 | 0 | 0 | 0 | 0 |
| $CONTROL_PAR_SELECTED_ITEM_IDX | I; menu selected position | wrong; generic metadata independent of semantic menu value; eval:589 / ui:538 → F4 | 23 | 23 | 0 | 62 |
| $CONTROL_PAR_SHORT_NAME | S; caption/lines/value label/help/short caption | partial; init scalar/textline overlay mapped; runtime getters empty; aliases discarded; eval:925 / ui:479 / lower:2087 → F1/F2/F4 | 51 | 51 | 0 | 4946 |
| $CONTROL_PAR_SHOW_ARROWS | I boolean; value-edit arrow visibility | partial; IR boolean, value edit lacks full native interaction; ui:353 → F7; widgets scope | 416 | 416 | 0 | 14043 |
| $CONTROL_PAR_SLICEMARKERS_COLOR | I RGB color; relevant widget paint slot | partial; IR color fields, no complete meter/waveform/table paint; ui:54,457 → F7; render/widgets scopes | 0 | 0 | 0 | 0 |
| $CONTROL_PAR_TEXT | S; caption/lines/value label/help/short caption | partial; init scalar/textline overlay mapped; runtime getters empty; aliases discarded; eval:925 / ui:479 / lower:2087 → F1/F2/F4 | 834 | 834 | 0 | 606835 |
| $CONTROL_PAR_TEXTLINE | S; caption/lines/value label/help/short caption | partial; init scalar/textline overlay mapped; runtime getters empty; aliases discarded; eval:925 / ui:479 / lower:2087 → F1/F2/F4 | 52 | 52 | 0 | 53 |
| $CONTROL_PAR_TEXTPOS_Y | I; caption/value vertical pixel offset | partial for TEXTPOS_Y, missing VALUEPOS_Y; ui:446,538; renderer does not consume offset → F7; render scope | 783 | 783 | 0 | 70713 |
| $CONTROL_PAR_TEXT_ALIGNMENT | I; horizontal text alignment | wrong when set alone; style only created if FONT_TYPE exists; ui:504; audit alignment probe → F7 | 834 | 834 | 0 | 73578 |
| $CONTROL_PAR_TYPE | I read-only; vendor control type | partial; static UI lookup correct; dynamic ID sees sparse mirror default; eval:595 / lower:2914 → F4 | 0 | 0 | 0 | 0 |
| $CONTROL_PAR_UNIT | I enum; native display unit | partial; integer unit translated to string; invalid enum/context unchecked; eval:948 / ui:218,319 → F1/F7 | 0 | 0 | 0 | 0 |
| $CONTROL_PAR_VALUE | I scalar, I table cells or R XY coordinates; no recursive callback | partial; scalar values native; indexed table/XY VM and presentation diverge; eval:567,886 / lower:2874 → F2/F10 | 834 | 834 | 0 | 1441658 |
| $CONTROL_PAR_VALUEPOS_Y | I; caption/value vertical pixel offset | partial for TEXTPOS_Y, missing VALUEPOS_Y; ui:446,538; renderer does not consume offset → F7; render scope | 0 | 0 | 0 | 0 |
| $CONTROL_PAR_VERTICAL | I boolean; meter orientation | partial; IR mapped; no live meter source; ui:401 / ui:598 → F7/F10 | 2 | 2 | 0 | 20 |
| $CONTROL_PAR_WAVETABLE | Uncertain historical/vendor extension; needs profile-specific reference | missing; opaque symbol, no semantic handler; ui:538 → F7; do not invent units | 0 | 0 | 0 | 0 |
| $CONTROL_PAR_WAVETABLE_ALPHA | I; opacity component | missing; unsupported independent alpha and gradient fields; ui:538 → F7; render scope | 0 | 0 | 0 | 0 |
| $CONTROL_PAR_WAVETABLE_COLOR | I RGB; wavetable/gradient end color | missing; unsupported style fields; ui:538 → F7; render scope | 0 | 0 | 0 | 0 |
| $CONTROL_PAR_WAVETABLE_END_ALPHA | I; opacity component | missing; unsupported independent alpha and gradient fields; ui:538 → F7; render scope | 0 | 0 | 0 | 0 |
| $CONTROL_PAR_WAVETABLE_END_COLOR | I RGB; wavetable/gradient end color | missing; unsupported style fields; ui:538 → F7; render scope | 0 | 0 | 0 | 0 |
| $CONTROL_PAR_WAVE_ALPHA | I; opacity component | missing; unsupported independent alpha and gradient fields; ui:538 → F7; render scope | 0 | 0 | 0 | 0 |
| $CONTROL_PAR_WAVE_COLOR | I RGB color; relevant widget paint slot | partial; IR color fields, no complete meter/waveform/table paint; ui:54,457 → F7; render/widgets scopes | 0 | 0 | 0 | 0 |
| $CONTROL_PAR_WAVE_CURSOR_COLOR | I RGB color; relevant widget paint slot | partial; IR color fields, no complete meter/waveform/table paint; ui:54,457 → F7; render/widgets scopes | 0 | 0 | 0 | 0 |
| $CONTROL_PAR_WAVE_END_ | Historical waveform-end identifier/alias; confirm profile spelling and indexed units | Unknown alias semantics: lexical incidence retained; compare standard WAVE_END native getter/setter on a minimal waveform fixture. | 0 | 0 | 0 | 0 |
| $CONTROL_PAR_WAVE_END_ALPHA | I; opacity component | missing; unsupported independent alpha and gradient fields; ui:538 → F7; render scope | 0 | 0 | 0 | 0 |
| $CONTROL_PAR_WAVE_END_COLOR | I RGB; wavetable/gradient end color | missing; unsupported style fields; ui:538 → F7; render scope | 0 | 0 | 0 | 0 |
| $CONTROL_PAR_WF_VIS_MODE | I enum; waveform display mode | missing; Kind::Waveform contains no mode; ui:394,538 → F7; render scope | 0 | 0 | 0 | 0 |
| $CONTROL_PAR_WIDTH | I; pixel size | partial; omitted getter = 0; one missing axis resets both; ui:423 / ir_view:137 → F8 | 834 | 834 | 0 | 2285369 |
| $CONTROL_PAR_WT_VIS_MODE | I enum; wavetable visualization | partial; metadata retained, no real source/display; ui:396 → F7; render scope | 1 | 1 | 0 | 3 |
| $CONTROL_PAR_WT_ZONE | I; attached source-zone ID | missing; opaque unsupported property; ui:538 → F7/F10 | 1 | 1 | 0 | 5 |
| $CONTROL_PAR_X | Vendor XY indexed axis/property extension; verify native profile and get/set units before claiming support | Unknown vendor semantics: fixed public token is counted, no authored value persisted; measure typed axis/index set/get against v1 and native host. | 0 | 0 | 0 | 0 |
| $CONTROL_PAR_Y | Vendor XY indexed axis/property extension; verify native profile and get/set units before claiming support | Unknown vendor semantics: fixed public token is counted, no authored value persisted; measure typed axis/index set/get against v1 and native host. | 0 | 0 | 0 | 0 |
| $CONTROL_PAR_ZERO_LINE_COLOR | I RGB color; relevant widget paint slot | partial; IR color fields, no complete meter/waveform/table paint; ui:54,457 → F7; render/widgets scopes | 1 | 1 | 0 | 4 |
| $CONTROL_PAR_Z_LAYER | I; layer then vendor widget/declaration order | partial; IR stores z; draw_order only sorts sibling z/declaration, missing widget priority; ui:432 / ir:583 → F7; render scope | 764 | 764 | 0 | 59583 |
| $HIDE_PART_BG | Bit mask selects background/title/value/cursor/modulation-light/whole widget visibility | Partial: whole/inherited hide mapped; finer source parts need widget rendering/event fidelity. UI_PARAMS F7/F10; UI_RENDER hide/z matrix. | 408 | 408 | 0 | 11565 |
| $HIDE_PART_CURSOR | Bit mask selects background/title/value/cursor/modulation-light/whole widget visibility | Partial: whole/inherited hide mapped; finer source parts need widget rendering/event fidelity. UI_PARAMS F7/F10; UI_RENDER hide/z matrix. | 0 | 0 | 0 | 0 |
| $HIDE_PART_MOD_LIGHT | Bit mask selects background/title/value/cursor/modulation-light/whole widget visibility | Partial: whole/inherited hide mapped; finer source parts need widget rendering/event fidelity. UI_PARAMS F7/F10; UI_RENDER hide/z matrix. | 0 | 0 | 0 | 0 |
| $HIDE_PART_NOTHING | Bit mask selects background/title/value/cursor/modulation-light/whole widget visibility | Partial: whole/inherited hide mapped; finer source parts need widget rendering/event fidelity. UI_PARAMS F7/F10; UI_RENDER hide/z matrix. | 831 | 831 | 0 | 4596170 |
| $HIDE_PART_TITLE | Bit mask selects background/title/value/cursor/modulation-light/whole widget visibility | Partial: whole/inherited hide mapped; finer source parts need widget rendering/event fidelity. UI_PARAMS F7/F10; UI_RENDER hide/z matrix. | 0 | 0 | 0 | 0 |
| $HIDE_PART_VALUE | Bit mask selects background/title/value/cursor/modulation-light/whole widget visibility | Partial: whole/inherited hide mapped; finer source parts need widget rendering/event fidelity. UI_PARAMS F7/F10; UI_RENDER hide/z matrix. | 0 | 0 | 0 | 0 |
| $HIDE_WHOLE_CONTROL | Bit mask selects background/title/value/cursor/modulation-light/whole widget visibility | Partial: whole/inherited hide mapped; finer source parts need widget rendering/event fidelity. UI_PARAMS F7/F10; UI_RENDER hide/z matrix. | 831 | 831 | 0 | 1503546 |
| $INST_ICON_ID | Special instrument icon/wallpaper UI identity; resolve authored source resource | Partial/missing: classic wallpaper and native frontend resolution are incomplete. UI_RENDER native/wallpaper matrix; route existing source-family resolver. | 834 | 834 | 0 | 840 |
| $INST_WALLPAPER_ID | Special instrument icon/wallpaper UI identity; resolve authored source resource | Partial/missing: classic wallpaper and native frontend resolution are incomplete. UI_RENDER native/wallpaper matrix; route existing source-family resolver. | 783 | 783 | 0 | 789 |
| $KNOB_UNIT_DB | Display-unit enum: none, dB, Hz, ms, octaves, percent or semitones; preserve raw control value | Partial: init unit mapped, runtime knob aliases/display getter incomplete. UI_PARAMS F1/F4/F7; v1 live knob metadata is reference. | 57 | 57 | 0 | 212 |
| $KNOB_UNIT_HZ | Display-unit enum: none, dB, Hz, ms, octaves, percent or semitones; preserve raw control value | Partial: init unit mapped, runtime knob aliases/display getter incomplete. UI_PARAMS F1/F4/F7; v1 live knob metadata is reference. | 1 | 1 | 0 | 23 |
| $KNOB_UNIT_MS | Display-unit enum: none, dB, Hz, ms, octaves, percent or semitones; preserve raw control value | Partial: init unit mapped, runtime knob aliases/display getter incomplete. UI_PARAMS F1/F4/F7; v1 live knob metadata is reference. | 101 | 101 | 0 | 262 |
| $KNOB_UNIT_NONE | Display-unit enum: none, dB, Hz, ms, octaves, percent or semitones; preserve raw control value | Partial: init unit mapped, runtime knob aliases/display getter incomplete. UI_PARAMS F1/F4/F7; v1 live knob metadata is reference. | 1 | 1 | 0 | 5 |
| $KNOB_UNIT_OCT | Display-unit enum: none, dB, Hz, ms, octaves, percent or semitones; preserve raw control value | Partial: init unit mapped, runtime knob aliases/display getter incomplete. UI_PARAMS F1/F4/F7; v1 live knob metadata is reference. | 1 | 1 | 0 | 2 |
| $KNOB_UNIT_PERCENT | Display-unit enum: none, dB, Hz, ms, octaves, percent or semitones; preserve raw control value | Partial: init unit mapped, runtime knob aliases/display getter incomplete. UI_PARAMS F1/F4/F7; v1 live knob metadata is reference. | 52 | 52 | 0 | 366 |
| $KNOB_UNIT_ST | Display-unit enum: none, dB, Hz, ms, octaves, percent or semitones; preserve raw control value | Partial: init unit mapped, runtime knob aliases/display getter incomplete. UI_PARAMS F1/F4/F7; v1 live knob metadata is reference. | 1 | 1 | 0 | 6 |
| $NI_CONTROL_PAR_IDX | Originating mouse event/inside state or indexed control position in its UI callback | Missing: ControlContext lacks source event fields; carry typed originating event across waits. UI_PARAMS F4/F10; sampler-core/control.rs:194. | 51 | 51 | 0 | 4721 |
| $NI_CONTROL_TYPE_BUTTON | Read-only vendor widget type enum used by TYPE lookup | Partial: static UI type lookup retained; dynamic runtime ID getter uses sparse mirror. UI_PARAMS F4; typed lookup fixture. | 0 | 0 | 0 | 0 |
| $NI_CONTROL_TYPE_FILE_SELECTOR | Read-only vendor widget type enum used by TYPE lookup | Partial: static UI type lookup retained; dynamic runtime ID getter uses sparse mirror. UI_PARAMS F4; typed lookup fixture. | 0 | 0 | 0 | 0 |
| $NI_CONTROL_TYPE_KNOB | Read-only vendor widget type enum used by TYPE lookup | Partial: static UI type lookup retained; dynamic runtime ID getter uses sparse mirror. UI_PARAMS F4; typed lookup fixture. | 0 | 0 | 0 | 0 |
| $NI_CONTROL_TYPE_LABEL | Read-only vendor widget type enum used by TYPE lookup | Partial: static UI type lookup retained; dynamic runtime ID getter uses sparse mirror. UI_PARAMS F4; typed lookup fixture. | 0 | 0 | 0 | 0 |
| $NI_CONTROL_TYPE_LEVEL_METER | Read-only vendor widget type enum used by TYPE lookup | Partial: static UI type lookup retained; dynamic runtime ID getter uses sparse mirror. UI_PARAMS F4; typed lookup fixture. | 0 | 0 | 0 | 0 |
| $NI_CONTROL_TYPE_MENU | Read-only vendor widget type enum used by TYPE lookup | Partial: static UI type lookup retained; dynamic runtime ID getter uses sparse mirror. UI_PARAMS F4; typed lookup fixture. | 0 | 0 | 0 | 0 |
| $NI_CONTROL_TYPE_MOUSE_AREA | Read-only vendor widget type enum used by TYPE lookup | Partial: static UI type lookup retained; dynamic runtime ID getter uses sparse mirror. UI_PARAMS F4; typed lookup fixture. | 0 | 0 | 0 | 0 |
| $NI_CONTROL_TYPE_NONE | Read-only vendor widget type enum used by TYPE lookup | Partial: static UI type lookup retained; dynamic runtime ID getter uses sparse mirror. UI_PARAMS F4; typed lookup fixture. | 0 | 0 | 0 | 0 |
| $NI_CONTROL_TYPE_PANEL | Read-only vendor widget type enum used by TYPE lookup | Partial: static UI type lookup retained; dynamic runtime ID getter uses sparse mirror. UI_PARAMS F4; typed lookup fixture. | 0 | 0 | 0 | 0 |
| $NI_CONTROL_TYPE_SLIDER | Read-only vendor widget type enum used by TYPE lookup | Partial: static UI type lookup retained; dynamic runtime ID getter uses sparse mirror. UI_PARAMS F4; typed lookup fixture. | 0 | 0 | 0 | 0 |
| $NI_CONTROL_TYPE_SWITCH | Read-only vendor widget type enum used by TYPE lookup | Partial: static UI type lookup retained; dynamic runtime ID getter uses sparse mirror. UI_PARAMS F4; typed lookup fixture. | 0 | 0 | 0 | 0 |
| $NI_CONTROL_TYPE_TABLE | Read-only vendor widget type enum used by TYPE lookup | Partial: static UI type lookup retained; dynamic runtime ID getter uses sparse mirror. UI_PARAMS F4; typed lookup fixture. | 0 | 0 | 0 | 0 |
| $NI_CONTROL_TYPE_TEXT_EDIT | Read-only vendor widget type enum used by TYPE lookup | Partial: static UI type lookup retained; dynamic runtime ID getter uses sparse mirror. UI_PARAMS F4; typed lookup fixture. | 0 | 0 | 0 | 0 |
| $NI_CONTROL_TYPE_VALUE_EDIT | Read-only vendor widget type enum used by TYPE lookup | Partial: static UI type lookup retained; dynamic runtime ID getter uses sparse mirror. UI_PARAMS F4; typed lookup fixture. | 0 | 0 | 0 | 0 |
| $NI_CONTROL_TYPE_WAVEFORM | Read-only vendor widget type enum used by TYPE lookup | Partial: static UI type lookup retained; dynamic runtime ID getter uses sparse mirror. UI_PARAMS F4; typed lookup fixture. | 0 | 0 | 0 | 0 |
| $NI_CONTROL_TYPE_WAVETABLE | Read-only vendor widget type enum used by TYPE lookup | Partial: static UI type lookup retained; dynamic runtime ID getter uses sparse mirror. UI_PARAMS F4; typed lookup fixture. | 0 | 0 | 0 | 0 |
| $NI_CONTROL_TYPE_XY | Read-only vendor widget type enum used by TYPE lookup | Partial: static UI type lookup retained; dynamic runtime ID getter uses sparse mirror. UI_PARAMS F4; typed lookup fixture. | 0 | 0 | 0 | 0 |
| $NI_DND_ACCEPT_MULTIPLE | Drop acceptance cardinality or file selector filter enum | Partial/missing: metadata does not supply file/drop event payload or typed callback admission. UI_PARAMS F7/F10. | 0 | 0 | 0 | 0 |
| $NI_DND_ACCEPT_NONE | Drop acceptance cardinality or file selector filter enum | Partial/missing: metadata does not supply file/drop event payload or typed callback admission. UI_PARAMS F7/F10. | 0 | 0 | 0 | 0 |
| $NI_DND_ACCEPT_ONE | Drop acceptance cardinality or file selector filter enum | Partial/missing: metadata does not supply file/drop event payload or typed callback admission. UI_PARAMS F7/F10. | 0 | 0 | 0 | 0 |
| $NI_FILE_TYPE_ARRAY | Drop acceptance cardinality or file selector filter enum | Partial/missing: metadata does not supply file/drop event payload or typed callback admission. UI_PARAMS F7/F10. | 10 | 10 | 0 | 13 |
| $NI_FILE_TYPE_AUDIO | Drop acceptance cardinality or file selector filter enum | Partial/missing: metadata does not supply file/drop event payload or typed callback admission. UI_PARAMS F7/F10. | 0 | 0 | 0 | 0 |
| $NI_FILE_TYPE_MIDI | Drop acceptance cardinality or file selector filter enum | Partial/missing: metadata does not supply file/drop event payload or typed callback admission. UI_PARAMS F7/F10. | 0 | 0 | 0 | 0 |
| $NI_MOUSE_EVENT_TYPE | Originating mouse event/inside state or indexed control position in its UI callback | Missing: ControlContext lacks source event fields; carry typed originating event across waits. UI_PARAMS F4/F10; sampler-core/control.rs:194. | 0 | 0 | 0 | 0 |
| $NI_MOUSE_EVENT_TYPE_DRAG | Originating mouse event/inside state or indexed control position in its UI callback | Missing: ControlContext lacks source event fields; carry typed originating event across waits. UI_PARAMS F4/F10; sampler-core/control.rs:194. | 0 | 0 | 0 | 0 |
| $NI_MOUSE_EVENT_TYPE_LEFT_BUTTON_DOWN | Originating mouse event/inside state or indexed control position in its UI callback | Missing: ControlContext lacks source event fields; carry typed originating event across waits. UI_PARAMS F4/F10; sampler-core/control.rs:194. | 0 | 0 | 0 | 0 |
| $NI_MOUSE_EVENT_TYPE_LEFT_BUTTON_UP | Originating mouse event/inside state or indexed control position in its UI callback | Missing: ControlContext lacks source event fields; carry typed originating event across waits. UI_PARAMS F4/F10; sampler-core/control.rs:194. | 0 | 0 | 0 | 0 |
| $NI_MOUSE_OVER_CONTROL | Originating mouse event/inside state or indexed control position in its UI callback | Missing: ControlContext lacks source event fields; carry typed originating event across waits. UI_PARAMS F4/F10; sampler-core/control.rs:194. | 0 | 0 | 0 | 0 |
| $NI_WF_VIS_MODE_1 | Waveform/wavetable visualization mode, flags or indexed source cursor/table property; profile applicability must be verified | Partial/missing: declaration metadata is not live attachment/visualization/cursor or MIDI drag service. UI_PARAMS F2/F7/F10; UI_WIDGETS waveform/wavetable rows. | 0 | 0 | 0 | 0 |
| $NI_WF_VIS_MODE_2 | Waveform/wavetable visualization mode, flags or indexed source cursor/table property; profile applicability must be verified | Partial/missing: declaration metadata is not live attachment/visualization/cursor or MIDI drag service. UI_PARAMS F2/F7/F10; UI_WIDGETS waveform/wavetable rows. | 0 | 0 | 0 | 0 |
| $NI_WF_VIS_MODE_3 | Waveform/wavetable visualization mode, flags or indexed source cursor/table property; profile applicability must be verified | Partial/missing: declaration metadata is not live attachment/visualization/cursor or MIDI drag service. UI_PARAMS F2/F7/F10; UI_WIDGETS waveform/wavetable rows. | 0 | 0 | 0 | 0 |
| $NI_WT_VIS_2D | Waveform/wavetable visualization mode, flags or indexed source cursor/table property; profile applicability must be verified | Partial/missing: declaration metadata is not live attachment/visualization/cursor or MIDI drag service. UI_PARAMS F2/F7/F10; UI_WIDGETS waveform/wavetable rows. | 1 | 1 | 0 | 1 |
| $NI_WT_VIS_3D | Waveform/wavetable visualization mode, flags or indexed source cursor/table property; profile applicability must be verified | Partial/missing: declaration metadata is not live attachment/visualization/cursor or MIDI drag service. UI_PARAMS F2/F7/F10; UI_WIDGETS waveform/wavetable rows. | 1 | 1 | 0 | 2 |
| $UI_WAVEFORM_TABLE_IS_BIPOLAR | Waveform/wavetable visualization mode, flags or indexed source cursor/table property; profile applicability must be verified | Partial/missing: declaration metadata is not live attachment/visualization/cursor or MIDI drag service. UI_PARAMS F2/F7/F10; UI_WIDGETS waveform/wavetable rows. | 0 | 0 | 0 | 0 |
| $UI_WAVEFORM_USE_MIDI_DRAG | Waveform/wavetable visualization mode, flags or indexed source cursor/table property; profile applicability must be verified | Partial/missing: declaration metadata is not live attachment/visualization/cursor or MIDI drag service. UI_PARAMS F2/F7/F10; UI_WIDGETS waveform/wavetable rows. | 0 | 0 | 0 | 0 |
| $UI_WAVEFORM_USE_SLICES | Waveform/wavetable visualization mode, flags or indexed source cursor/table property; profile applicability must be verified | Partial/missing: declaration metadata is not live attachment/visualization/cursor or MIDI drag service. UI_PARAMS F2/F7/F10; UI_WIDGETS waveform/wavetable rows. | 0 | 0 | 0 | 0 |
| $UI_WAVEFORM_USE_TABLE | Waveform/wavetable visualization mode, flags or indexed source cursor/table property; profile applicability must be verified | Partial/missing: declaration metadata is not live attachment/visualization/cursor or MIDI drag service. UI_PARAMS F2/F7/F10; UI_WIDGETS waveform/wavetable rows. | 3 | 3 | 0 | 66 |
| $UI_WF_PROP_FLAGS | Waveform/wavetable visualization mode, flags or indexed source cursor/table property; profile applicability must be verified | Partial/missing: declaration metadata is not live attachment/visualization/cursor or MIDI drag service. UI_PARAMS F2/F7/F10; UI_WIDGETS waveform/wavetable rows. | 0 | 0 | 0 | 0 |
| $UI_WF_PROP_MIDI_DRAG_START_NOTE | Waveform/wavetable visualization mode, flags or indexed source cursor/table property; profile applicability must be verified | Partial/missing: declaration metadata is not live attachment/visualization/cursor or MIDI drag service. UI_PARAMS F2/F7/F10; UI_WIDGETS waveform/wavetable rows. | 0 | 0 | 0 | 0 |
| $UI_WF_PROP_PLAY_CURSOR | Waveform/wavetable visualization mode, flags or indexed source cursor/table property; profile applicability must be verified | Partial/missing: declaration metadata is not live attachment/visualization/cursor or MIDI drag service. UI_PARAMS F2/F7/F10; UI_WIDGETS waveform/wavetable rows. | 3 | 3 | 0 | 12 |
| $UI_WF_PROP_TABLE_IDX_HIGHLIGHT | Waveform/wavetable visualization mode, flags or indexed source cursor/table property; profile applicability must be verified | Partial/missing: declaration metadata is not live attachment/visualization/cursor or MIDI drag service. UI_PARAMS F2/F7/F10; UI_WIDGETS waveform/wavetable rows. | 0 | 0 | 0 | 0 |
| $UI_WF_PROP_TABLE_VAL | Waveform/wavetable visualization mode, flags or indexed source cursor/table property; profile applicability must be verified | Partial/missing: declaration metadata is not live attachment/visualization/cursor or MIDI drag service. UI_PARAMS F2/F7/F10; UI_WIDGETS waveform/wavetable rows. | 0 | 0 | 0 | 0 |
| add_menu_item | append ordered text + semantic value | ksp/partial init correct; runtime emitted, unhandled; eval:980 / lib:307 → F1/F4 | 834 | 834 | 0 | 32816 |
| add_text_line | append label line | ksp/partial init concatenates; runtime discarded; eval:925 / lib:307 → F1 | 496 | 496 | 0 | 61851 |
| attach_level_meter | bind meter to group/slot/channel/bus source | ksp/partial request/IR retains bus+channel, not complete source; ui:598 → F7/F10 | 53 | 53 | 0 | 299 |
| attach_zone | bind waveform to zone and flags | ksp/missing service; init request/runtime Host only; eval:1259 / lib:307 → F7/F10 | 3 | 3 | 0 | 66 |
| expose_controls | expose declared identifiers across slots to Komplete UI | ksp/missing; init no-op, no exported registry; eval:1125 → F7/F10 | 51 | 51 | 0 | 262 |
| fs_get_filename | selected filename/path from file callback | ksp/missing; empty string/ignored; eval:1249 / lower:2087 → F4/F10 | 10 | 10 | 0 | 28 |
| fs_navigate | select neighboring file and invoke its handler | ksp/missing; Host effect discarded; lower:2065 / lib:307 → F10 | 10 | 10 | 0 | 26 |
| get_control_par | partial; static VALUE/TYPE native and sparse int readback | ksp/eval:589; lower:2910; default/derived/input fields absent | 834 | 834 | 0 | 831798 |
| get_control_par_arr | wrong; init map lookup; runtime omits index and initial indexed store | ksp/eval:914; lower:2928; lib:741 | 52 | 52 | 0 | 58 |
| get_control_par_real | wrong; init int property converted to real; runtime ignored | ksp/eval:902; lower:2087 | 0 | 0 | 0 | 0 |
| get_control_par_real_arr | partial init map only; runtime ignored | ksp/eval:914; lower:2087 | 0 | 0 | 0 | 0 |
| get_control_par_str | wrong; init only reads stored property or converts 0; runtime ignored | ksp/eval:910; lower:2087 | 51 | 51 | 0 | 1206 |
| get_control_par_str_arr | partial init map only; runtime ignored | ksp/eval:914; lower:2087 | 0 | 0 | 0 | 0 |
| get_folder | Return the requested host/resource folder path for a native folder-ID enum | Missing: init evaluator returns an empty string (sampler-ksp/eval.rs:1265); add bounded source-family folder service, compare v1 path resolution. | 63 | 63 | 0 | 91 |
| get_font_id | resource font name to font ID | ksp/partial init registers font name, not full font selection; eval:1053 / ui:504 → F7; render | 371 | 371 | 0 | 748 |
| get_key_color | keyboard visual/type/name/pressed/range state in vendor units | ksp/partial init keys/ranges; runtime consumer handles only color/type/pressed/name; trigger state zero; eval:1063 / lib:290 → F1/F4/F10; keyboard/render owner | 0 | 0 | 0 | 0 |
| get_key_name | keyboard visual/type/name/pressed/range state in vendor units | ksp/partial init keys/ranges; runtime consumer handles only color/type/pressed/name; trigger state zero; eval:1063 / lib:290 → F1/F4/F10; keyboard/render owner | 0 | 0 | 0 | 0 |
| get_key_triggerstate | keyboard visual/type/name/pressed/range state in vendor units | ksp/partial init keys/ranges; runtime consumer handles only color/type/pressed/name; trigger state zero; eval:1063 / lib:290 → F1/F4/F10; keyboard/render owner | 0 | 0 | 0 | 0 |
| get_key_type | keyboard visual/type/name/pressed/range state in vendor units | ksp/partial init keys/ranges; runtime consumer handles only color/type/pressed/name; trigger state zero; eval:1063 / lib:290 → F1/F4/F10; keyboard/render owner | 0 | 0 | 0 | 0 |
| get_keyrange_max_note | keyboard visual/type/name/pressed/range state in vendor units | ksp/partial init keys/ranges; runtime consumer handles only color/type/pressed/name; trigger state zero; eval:1063 / lib:290 → F1/F4/F10; keyboard/render owner | 0 | 0 | 0 | 0 |
| get_keyrange_min_note | keyboard visual/type/name/pressed/range state in vendor units | ksp/partial init keys/ranges; runtime consumer handles only color/type/pressed/name; trigger state zero; eval:1063 / lib:290 → F1/F4/F10; keyboard/render owner | 0 | 0 | 0 | 0 |
| get_keyrange_name | keyboard visual/type/name/pressed/range state in vendor units | ksp/partial init keys/ranges; runtime consumer handles only color/type/pressed/name; trigger state zero; eval:1063 / lib:290 → F1/F4/F10; keyboard/render owner | 0 | 0 | 0 | 0 |
| get_menu_item_str | current item caption by index | ksp/partial init correct; runtime ignored; eval:1007 / lower:2087 → F4 | 54 | 54 | 0 | 477 |
| get_menu_item_value | current semantic item value by index | ksp/partial init correct; runtime ignored; eval:1007 / lower:2087 → F4 | 51 | 51 | 0 | 323 |
| get_menu_item_visibility | current per-item visibility | ksp/partial init correct; runtime ignored; eval:1007 / lower:2087 → F4 | 0 | 0 | 0 | 0 |
| get_num_menu_items | current item count (historical helper) | ksp/partial init correct; runtime ignored; eval:1018 / lower:2087 → F4 | 0 | 0 | 0 | 0 |
| get_ui_id | correct for declared widgets and slot-local arithmetic | ksp/eval:880; lower:1980; FIRST_UI_ID=32768; NCKP tree order | 834 | 834 | 0 | 17474808 |
| get_ui_wf_property | waveform cursor/flags/indexed slice state | ksp/missing; returns 0; eval:1252 / lower:2087 → F7/F10 | 0 | 0 | 0 | 0 |
| hide_part | visibility mask immediately updates widget | ksp/partial init; runtime discarded; eval:946 / lib:307 → F1 | 51 | 51 | 0 | 561 |
| load_performance_view | init .nckp once per slot, widget declaration tree | ksp/partial literal pre-scan, partial type IDs and hierarchy; nckp:17,85 / load:583 → F7/F8 | 51 | 51 | 0 | 51 |
| make_instr_persistent | executed declaration flag: instrument only | ksp/partial static flag; recall ignores exclusion; sema:355 / snapshot:97 → F3/F5/F6 | 55 | 55 | 0 | 1661 |
| make_perfview | init activates authored performance page | ksp/correct activation subset; conflict with NCKP not enforced; eval:1049 → F7 | 833 | 833 | 0 | 839 |
| make_persistent | executed declaration flag: instrument + snapshots | ksp/partial static flag; current-state save path absent; sema:355 / model:164 → F3/F5 | 834 | 834 | 0 | 236870 |
| move_control | grid placement, (0,0) hidden; all callbacks | ksp/partial private grid tags at init; runtime discarded; eval:963 / ui:436 → F1/F8 | 52 | 52 | 0 | 3515 |
| move_control_px | local pixel placement; all callbacks | ksp/partial init pixel props; stale grid tags remain; runtime discarded; eval:963 → F1/F8 | 829 | 829 | 0 | 104790 |
| persistence_changed | Callback after native saved values are restored; derived UI/state is rebuilt before publication | Broad order retained, but faults can be suppressed as warnings; scanner independently observes completion/fault. UI_PARAMS lifecycle matrix; actual evaluator/VM scanner phase tests. | 430 | 430 | 0 | 549 |
| read_persistent_var | immediate pending saved restore, then consume entry | ksp/wrong duplicate restoration; no persistence-kind check; eval:338,1132 → F5 | 813 | 813 | 0 | 100010 |
| remove_keyrange | keyboard visual/type/name/pressed/range state in vendor units | ksp/partial init keys/ranges; runtime consumer handles only color/type/pressed/name; trigger state zero; eval:1063 / lib:290 → F1/F4/F10; keyboard/render owner | 407 | 407 | 0 | 147108 |
| reset_nks_nav | reset NKS navigation metadata | ksp/missing; Host request emitted, not consumed; eval:1259 / lib:307 → F7 | 51 | 51 | 0 | 101 |
| set_control_help | tooltip content | ksp/partial init correct; runtime discarded; eval:925 → F1 | 400 | 400 | 0 | 11302 |
| set_control_par | partial; scalar integer metadata/value | ksp/eval:567; lower:2874; lib:307; missing legality/schema | 834 | 834 | 0 | 19292926 |
| set_control_par_arr | wrong; init separate map, runtime scalar mirror | ksp/eval:886; lower:2904; lib:314 | 53 | 53 | 0 | 363 |
| set_control_par_real | wrong for fractional metadata at init; runtime effect scalar only | ksp/eval:580 casts non-string v.int(); lib:309 preserves runtime IEEE bits | 0 | 0 | 0 | 0 |
| set_control_par_real_arr | wrong; values retained only as indexed properties at init; runtime effect unhandled | ksp/eval:886; lower:2874; lib:307 lacks case | 0 | 0 | 0 | 0 |
| set_control_par_str | partial; init strings and direct runtime effects | ksp/eval:881; lib:313; typed getter missing and effect text bounded | 834 | 834 | 0 | 2411025 |
| set_control_par_str_arr | partial; init textline map, runtime effect inserted | ksp/eval:895 capped at 65536 lines; lib:315; indexed images/automation not projected | 52 | 52 | 0 | 2555 |
| set_key_color | keyboard visual/type/name/pressed/range state in vendor units | ksp/partial init keys/ranges; runtime consumer handles only color/type/pressed/name; trigger state zero; eval:1063 / lib:290 → F1/F4/F10; keyboard/render owner | 834 | 834 | 0 | 1495105 |
| set_key_name | keyboard visual/type/name/pressed/range state in vendor units | ksp/partial init keys/ranges; runtime consumer handles only color/type/pressed/name; trigger state zero; eval:1063 / lib:290 → F1/F4/F10; keyboard/render owner | 752 | 752 | 0 | 685237 |
| set_key_pressed | keyboard visual/type/name/pressed/range state in vendor units | ksp/partial init keys/ranges; runtime consumer handles only color/type/pressed/name; trigger state zero; eval:1063 / lib:290 → F1/F4/F10; keyboard/render owner | 774 | 774 | 0 | 317686 |
| set_key_pressed_support | keyboard visual/type/name/pressed/range state in vendor units | ksp/partial init keys/ranges; runtime consumer handles only color/type/pressed/name; trigger state zero; eval:1063 / lib:290 → F1/F4/F10; keyboard/render owner | 773 | 773 | 0 | 829 |
| set_key_type | keyboard visual/type/name/pressed/range state in vendor units | ksp/partial init keys/ranges; runtime consumer handles only color/type/pressed/name; trigger state zero; eval:1063 / lib:290 → F1/F4/F10; keyboard/render owner | 777 | 777 | 0 | 1408583 |
| set_keyrange | keyboard visual/type/name/pressed/range state in vendor units | ksp/partial init keys/ranges; runtime consumer handles only color/type/pressed/name; trigger state zero; eval:1063 / lib:290 → F1/F4/F10; keyboard/render owner | 407 | 407 | 0 | 800889 |
| set_knob_defval | raw reset value | ksp/partial init property; runtime discarded; eval:946 → F1/F4 | 0 | 0 | 0 | 0 |
| set_knob_label | formatted value text | ksp/partial init property; runtime discarded; eval:925 → F1 | 108 | 108 | 0 | 2316 |
| set_knob_unit | display unit enum | ksp/partial init property; runtime discarded; eval:946 → F1 | 108 | 108 | 0 | 552 |
| set_menu_item_str | mutate existing item caption | ksp/partial init correct; runtime discarded; eval:988 → F1/F4 | 429 | 429 | 0 | 222605 |
| set_menu_item_value | mutate existing semantic item value | ksp/partial init correct; runtime discarded; eval:988 → F1/F4 | 0 | 0 | 0 | 0 |
| set_menu_item_visibility | mutate item visibility; selected hidden item contract | ksp/partial init data; runtime discarded; eval:988 → F1/F4 | 426 | 426 | 0 | 757028 |
| set_nks_nav_name | NKS navigation metadata name | ksp/missing; Host request emitted, not consumed; eval:1259 / lib:307 → F7 | 51 | 51 | 0 | 2609 |
| set_nks_nav_par | NKS navigation parameter metadata | ksp/missing; Host request emitted, not consumed; eval:1259 / lib:307 → F7 | 51 | 51 | 0 | 11260 |
| set_script_title | slot/page title, init | ksp/partial retained; native display/context limits unverified; eval:1035 → F7 | 834 | 834 | 0 | 1123 |
| set_skin_offset | wallpaper crop/scroll pixels; runtime legal | ksp/wrong after init; lower calls init-only; eval:1019 / lower:2096 → F1; render | 353 | 353 | 0 | 369 |
| set_snapshot_type | four-valued recall/native-save policy across slots | ksp/missing host policy; init request retained only; eval:1125 / snapshot:97 → F6 | 55 | 55 | 0 | 55 |
| set_table_steps_shown | display window/step count | ksp/partial init mapped; runtime discarded; eval:946 / ui:383 → F1/F2 | 152 | 152 | 0 | 8462 |
| set_text | replace label content or widget caption | ksp/partial init; runtime Host effect discarded; eval:925 / lib:307 → F1 | 555 | 555 | 0 | 48514 |
| set_ui_color | performance background color; runtime legal | ksp/wrong after init; lower calls init-only; eval:1125 / lower:2096 → F1 | 399 | 399 | 0 | 399 |
| set_ui_height | init view height in grid rows | ksp/partial retained; invalid value policy not enforced; eval:1023 / ui:280 → F7/F8 | 21 | 21 | 0 | 42 |
| set_ui_height_px | init view height in pixels | ksp/partial retained; invalid range/default/header semantics; eval:1027 / ui:286 → F7/F8; render | 834 | 834 | 0 | 971 |
| set_ui_wf_property | waveform cursor/flags/slice state | ksp/missing; init request/runtime Host discarded; eval:1259 / lib:307 → F7/F10 | 3 | 3 | 0 | 12 |
| set_ui_width_px | init view width in pixels | ksp/partial retained; invalid range policy unchecked; eval:1031 / ui:291 → F7/F8 | 775 | 775 | 0 | 831 |
| show_library_tab | Request the host library/browser tab to become visible | Missing: evaluator no-op and lowerer empty result (sampler-ksp/eval.rs:1139, lower.rs:2095); route an explicit editor/host request. | 375 | 375 | 0 | 381 |
| ui_button | I; 0/1 | partial; scalar callback native; caption/menu/visibility feedback can be lost; ksp/ui.rs:325, model.rs:255, sampler-core/src/control.rs:194 → F2/F4/F7/F10 | 827 | 827 | 0 | 15709 |
| ui_control | Run the originating control callback after its user edit, with current value and originating context; programmatic writes must not recurse | Partial: scalar control callback admission exists; typed service/event context and wait retention need widget/loop proof. UI_PARAMS F4/F10; UI_LOOP callback matrix. | 834 | 834 | 0 | 185077 |
| ui_controls | Multi-control callback dispatch with the native changed-control context and ordering | Unknown: distinguish callback syntax/profile from single-control admission; authored multi-control fixture plus ordered runtime observations required. KSP_SURFACE callback inventory. | 0 | 0 | 0 | 0 |
| ui_file_selector | no native saved-variable serializer; persist separate path | missing selected-file state/context; BASEPATH/FILE_TYPE metadata only; ksp/ui.rs:325, model.rs:255, sampler-core/src/control.rs:194 → F2/F4/F7/F10 | 10 | 10 | 0 | 13 |
| ui_knob | I; bounded raw scalar | partial; scalar control + callback works; display/default/automation/getter gaps; ksp/ui.rs:325, model.rs:255, sampler-core/src/control.rs:194 → F2/F4/F7/F10 | 108 | 108 | 0 | 1706 |
| ui_label | no native saved-variable serializer; rebuild text from other vars | partial caption/indexed text projection; runtime aliases absent, no live string getter; ksp/ui.rs:325, model.rs:255, sampler-core/src/control.rs:194 → F2/F4/F7/F10 | 833 | 833 | 0 | 122275 |
| ui_level_meter | no native saved-variable serializer; live source state | partial colors/orientation; source attachment/ranges/getters absent; ksp/ui.rs:325, model.rs:255, sampler-core/src/control.rs:194 → F2/F4/F7/F10 | 2 | 2 | 0 | 20 |
| ui_menu | I semantic value; native file stores selected position | partial; restore maps existing item index; invalid/early indexes fall to raw value; live getter/index/item updates missing; ksp/ui.rs:325, model.rs:255, sampler-core/src/control.rs:194 → F2/F4/F7/F10 | 834 | 834 | 0 | 8347 |
| ui_mouse_area | no native saved-variable serializer; source mouse/drop event | missing event fields/payload/drag policy; declaration/outline is not behavior; ksp/ui.rs:325, model.rs:255, sampler-core/src/control.rs:194 → F2/F4/F7/F10 | 0 | 0 | 0 | 0 |
| ui_panel | no native saved-variable serializer; no scalar musical value | partial parent offsets + inherited hide correct; full geometry/cycles/Z policy incomplete; ksp/ui.rs:325, model.rs:255, sampler-core/src/control.rs:194 → F2/F4/F7/F10 | 31 | 31 | 0 | 208 |
| ui_slider | I; bounded raw scalar | partial; scalar callback works; source axis sign/sensitivity projection wrong; ksp/ui.rs:325, model.rs:255, sampler-core/src/control.rs:194 → F2/F4/F7/F10 | 834 | 834 | 0 | 32408 |
| ui_switch | I; 0/1 | partial; scalar callback native; state fonts/pressed behavior absent; ksp/ui.rs:325, model.rs:255, sampler-core/src/control.rs:194 → F2/F4/F7/F10 | 833 | 833 | 0 | 95834 |
| ui_table | all I cells, not ordinary-array tail compression | wrong `_arr`/VM/IR coherence; Binding::Variable has no public UI cell edit callback service; ksp/ui.rs:325, model.rs:255, sampler-core/src/control.rs:194 → F2/F4/F7/F10 | 192 | 192 | 0 | 2565 |
| ui_text_edit | S current bytes | partial restored text model; no public UI text edit/callback service; IR editor placeholder; ksp/ui.rs:325, model.rs:255, sampler-core/src/control.rs:194 → F2/F4/F7/F10 | 422 | 422 | 0 | 2536 |
| ui_update | UI update callback executes at the native scheduled UI refresh boundary | Partial/unknown: source support is distinct from UI publication cadence; verify actual callback scheduling and edits across waits. UI_LOOP lifecycle matrix. | 0 | 0 | 0 | 0 |
| ui_value_edit | I bounded raw scalar | partial scalar callback native; arrows/value offset and display editing incomplete; ksp/ui.rs:325, model.rs:255, sampler-core/src/control.rs:194 → F2/F4/F7/F10 | 830 | 830 | 0 | 31777 |
| ui_waveform | native saved bounded I base state; not audio/zone/cursor serialization | partial declaration; attachment/getter/runtime cursor service missing; ksp/ui.rs:325, model.rs:255, sampler-core/src/control.rs:194 → F2/F4/F7/F10 | 3 | 3 | 0 | 3 |
| ui_wavetable | native saved bounded I base state; not wavetable asset serialization | partial declaration; zone/mode/color/source service missing; ksp/ui.rs:325, model.rs:255, sampler-core/src/control.rs:194 → F2/F4/F7/F10 | 1 | 1 | 0 | 1 |
| ui_xy | all R coordinate pairs, not ordinary-array tail compression | missing typed edit; sensitivity/mode only metadata; per-cursor property and readback gaps; ksp/ui.rs:325, model.rs:255, sampler-core/src/control.rs:194 → F2/F4/F7/F10 | 1 | 1 | 0 | 1 |

### Actual authored widget inventory

Counts sum all observed retained bridge views/programs; Native scene-tree kinds are not exposed by this inventory. They are declarations retained in UI models, including hidden widgets, not unique visible controls or source-token occurrences. Different v1/v2 projection and shell layouts prevent treating count differences as native parity.

| Widget kind | v2 widgets | v1 widgets |
| --- | --- | --- |
| image | 81967 | 0 |
| ui_button | 2854190 | 6679 |
| ui_file_selector | 13 | 0 |
| ui_knob | 453511 | 484 |
| ui_label | 2547637 | 18507 |
| ui_level_meter | 5278 | 302 |
| ui_menu | 59887 | 1186 |
| ui_panel | 317846 | 0 |
| ui_slider | 77414 | 10942 |
| ui_switch | 96992 | 7456 |
| ui_table | 6523 | 226 |
| ui_text_edit | 1912 | 20 |
| ui_value_edit | 86471 | 597 |
| ui_waveform | 28 | 3 |
| ui_wavetable | 1 | 0 |
| ui_xy | 1 | 0 |

## Small gallery of OUR Original renders

Images below are rendered output, never extracted library assets. The gallery is under 50 MB. Screenshots show one observed page; successful paint does not certify interaction or a native-host match.

**v2: Conflux 1.1.0 [Native Instruments] — original-ok; loads yes.**

![v2 Original renderer](census-gallery/v2-1.png)

**v2: UVI - Augmented Orchestra v1.1.2-R2R — missing-images; loads yes.**

![v2 Original renderer](census-gallery/v2-2.png)

**v2: VWinds - Clarinets — original-ok; loads yes.**

![v2 Original renderer](census-gallery/v2-3.png)

**v1: Conflux 1.1.0 [Native Instruments] — missing-images; loads yes.**

![v1 Original renderer](census-gallery/v1-4.png)

## Other scopes and unknowns

- Original uses the actual Native consumer when a Native package is present. Consumer admission and paint success are separate. Legacy widget/geometry/strip counts and placeholder/property flags describe the retained bridge model, not Native scene-tree layout. A bridge placeholder flag does not mean the consumed Native frontend lacks that widget. Native font file decode counts, when exposed, do not certify actual graph typography.
- Readability/native pixel parity: background fractions are candidates; compare authored fixtures and matched native screenshots at identical page, value, state and DPI. Uniform hidden views are not a blank visible main page.
- Bound controls: this probe checks scalar readback. Gesture, typed cell/text/file edit and source callback/automation parity need actual pointer/host traces from widgets/loop scopes.
- UVI native-key metadata records authored declarations separately from sample audibility. Sidecar uses conservative native snapshots; absent or conflicting declarations remain unknown.
- Shared auditions: v2 establishes per-program key/velocity. Declared white keys intersect retained sample-zone coverage at velocity64; then zone coverage near middle C; then fallback. Avoid invalid/control/keyswitch declarations. Source is native_declared / zone_coverage / fallback, never density mislabeled native-valid. One half-second audible note is not full sample/DSP correctness; silent is inconclusive for articulation/CC/noise/control regions. No audio regression claim without identical per-ID notes. The corrected scanner sends a safe explicit fallback when surviving load-time zone coverage is empty, so note callbacks still run; a fully invalid keyboard yields no safe audition and plays_note=no, not a false silent-note claim.
- Instrumentation coverage: unknown is distinct from zero. V1 UVI sidecar does not expose every requested Lua/asset/budget field; those remain unknown. Its shared-note-plan override label is kept as adapter_pick_source, while pick_source uses the matching current v2 witness’s common-plan origin; unavailable origin stays unknown. Asset success counts are observed requests, not every archive member. Native font metrics remain unknown where the frozen binary does not expose its package font inventory; legacy font-service zeros cannot establish a Native font failure. V1 whole-editor pixels cannot establish authored-page background coverage. V2 paints standalone authored pages, not the whole plugin shell; a whole-editor tree budget failure on another UI branch must remain a separate measurement.
- Cache reproducibility: input resources must remain immutable; container mtime/size does not detect a loose-resource replacement. Use a fresh output after resource changes. Cold/warm load wall time and peak RSS are single runs, not matched performance benchmarks.
- Scope/format limits: multi/bank execution uses admitted production programs; unresolved linked scripts/native packages need resource traversal. Snapshot files/recovery saves are excluded from the frozen1494-ID list. Fully unlocked and corrected native-host parity remain unmeasured.
- Sound correctness: selected sample family, round-robin order, filter/effect retention and native internal DSP parity are UNKNOWN. Retained zone counts and audible mixed output do not identify the samples or certify the processing graph.
- Reuse before rebuilding: v1 live UI semantics, prior Kontakt/native-resource and UVI typed-UI branches, and the existing strict typed persistence decoder are references. Audit tools do not merge those product fixes. Source line spans belong to the frozen baseline; future integration must regenerate counts with a newly built shared scanner.

## Requested extension fields and privacy boundary

[Symbol aggregates](/home/derpcat/.cache/kontra-scan/results/v2/symbol-aggregates.tsv) include attempted/parsed/lexical coverage, scanner digest, NKI/NKM and program-owner incidence, initialized widget kinds and fixed saved sigils. [V1 Original OK / v2 missing or error](/home/derpcat/.cache/kontra-scan/v1ok-v2missing.tsv) is separate from the load-admission regression list.

First nine stable fields: `path library loads ui controls_bound plays_note load_ms peak_rss_mb reason`. All columns below are exported by the ONE shared CLI; raw detailed metrics preserve independent slot/phase ownership. CONTROL_PAR refs are the `$CONTROL_PAR_*` subset of `ui_api_refs`, not a separate duplicated column.

`lua_init_faults`, `lua_init_first`, `lua_runtime_faults`, `lua_runtime_first`, `lua_budget_hits`, `controls_declared`, `controls_bound_declared`, `asset_lookup_requested`, `asset_lookup_ok`, `asset_decode_requested`, `asset_decode_ok`, `font_declared`, `font_success`, `paint_ok`, `paint_error`, `load_path`, `sample_resident_bytes`, `underruns`, `ksp_compile_ok`, `ksp_init_ok`, `first_script_error`, `active_script_slots`, `compiled_script_slots`, `clean_compiled_slots`, `init_callbacks_completed`, `persistence_changed_completed`, `load_fault_records`, `disabled_block_errors`, `widget_kind_counts`, `ui_api_refs`, `bypassed_ui_api_refs`, `saved_entry_sigils`, `custom_font_uses`, `picture_strips`, `picture_frames`, `picture_margins`, `resource_failure_reasons`, `page_background_rgba`, `plain_background_fraction`, `note_picked`, `note_policy`, `audition_status`, `pick_source`, `native_valid_keys`, `native_key_conflicts`, `native_preferred_note`, `slots_seen`, `slots_decode_failed`, `slots_bypassed`, `slots_inline_nonempty`, `slots_linked_only`, `slots_empty`, `admitted_saved_entry_sigils`, `ksp_runtime_fault_records`, `sample_zone_count`, `zero_zone_reason`, `fallback_note`, `keyswitch_picked`, `bound_typed`, `phantom_free_controls`, `first_audio_ms`, `ui_first_frame_ms`, `cache_state`, `keyboard_reason_counts`

Detailed JSON retains Lua init/runtime safe first diagnostic categories/digests, actual paint and budget status, widget kinds, lookup/decode/font success/failure categories, frames/strips/margins, declared/observed background RGBA and pixel fraction, load path, sample residency/underruns, runtime behavior outcomes, wire/runtime slots, compile admission/cleanliness, and independent init/persistence phase outcomes. Never serialize authored fault messages, identifiers, saved values, source text or resource bytes.


## KSP keyboard classifier correction

The frozen census retains the original 348 no-safe-key observations. Corrected scanner-only probes on the exact same IDs are reported separately in [UI_CENSUS_KEYBOARD.md](UI_CENSUS_KEYBOARD.md). Those rows establish a collector classification bug, not product silence. The targeted receipt preserves both binary versions and identical-note checks; do not combine their timing observations as one frozen binary.
