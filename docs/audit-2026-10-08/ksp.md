# KSP corpus and fidelity audit — 2026-10-08

Baseline: integration `7e82b152`. v1: pinned `0cb7a8a0`, not a current v1 branch. Audit worktree: `audit-ksp`; branch `audit/ksp-20261008`. This phase adds measurement tools and contract probes only.

## Verdict

**v2 is worse in runtime fidelity; NKI frontend admission is equal with resources supplied, and multi-script compilation remains incomplete in both engines.** A high compile rate is not a high compatibility rate. v2 admits commands which become unconsumed effects, zero/default queries, or no-ops. Several corresponding v1 paths have actual scheduler/UI/host implementations. Neither implementation is certified against native Kontakt by this audit.

| Population | Files | Files with active scripts | Active script slots | v2 compile | v2 init evaluation returns OK | v1 compile without disabled-block errors | v1 load returns OK | v1 load without retained faults |
|---|---:|---:|---:|---|---|---|---|---|
| NKI | 781 | 778 | 788 | 788/788 (100.00%) | 788/788 (100.00%) | 788/788 (100.00%) | 788/788 (100.00%) | 648/788 (82.23%) |
| NKM | 54 | 54 | 353 | 308/353 (87.25%) | 308/353 (87.25%) | 323/353 (91.50%) | 353/353 (100.00%) | 353/353 (100.00%) |
| Whole physical corpus | 835 | 832 | 1141 | 1,096/1,141 (96.06%) | 1,096/1,141 (96.06%) | 1,111/1,141 (97.37%) | 1,141/1,141 (100.00%) | 1,001/1,141 (87.73%) |

There are **49 unique NKI source bodies**, **63 across NKI/NKM**. The NKI per-instrument all-slot rate is **778/778 (100%) in both engines**; three NKI have no active scripts. Across script-bearing containers, every slot compiles in **802/832 v2 paths**, and **802/832 v1 paths**. UI-IR conversion and empty-plan binding succeed for all 1,096 v2-admitted slots. The master-list subset is 1,092/1,137 v2 slots and 1,107/1,137 clean v1 slots.

The resource-free first pass gave v1 787/788 NKI admissions; supplying Conflux’s 378 `.nckp` controls removes that context-induced failure. Resource-free all-file results were v2 1,096/1,141 and clean v1 1,060/1,141. Resource/group-context correction preserves the 1,141-slot denominator. The final probe uses embedded programs with their own groups and bank scripts, ignores unrelated top-level objects when a multi bank exists, and distinguishes compiler-disabled blocks from successful Runtime::load. **Neither a successful load nor a zero fault count establishes native-host compatibility.** v2 init evaluation also cannot validate its missing host services; its init/UI warnings occur in all 778 scripted NKI paths.

Component timings in the resource-aware NKI pass (median / p95 / maximum):

| Measured operation | Median | p95 | Maximum |
|---|---:|---:|---:|
| v2 parse/type/init harvest | 7.88 ms | 227.99 ms | 2801.06 ms |
| v2 full compile with init | 16.85 ms | 647.12 ms | 11695.62 ms |
| v1 Runtime::load including its compile/init | 5.48 ms | 159.74 ms | 1058.61 ms |

These are sequential component probes, warm filesystem state and adapter-dependent work; the v1 standalone compiler observation is outside its load timer. Do not sum cached observations as benchmark work or present an apples-to-apples whole-instrument speed ratio. Load/RSS/streaming/audio rates belong to the shared scanner and loading/DSP audits.


## Population and method

The installed Kontakt tree is mounted at `/mnt/MAIN_STORAGE`. The master census contains 781 NKI, 53 NKM and 1,103 snapshots. A current traversal which does not follow directory symlinks finds the same 781 NKI and 54 NKM; the one extra multi is outside the master list. The whole script-bearing physical tree, including this extra multi, is measured. Canonicalizing the 2,835 alias-inclusive NKI/NKM paths yields exactly the same 835 real paths; the canonical sets match. Snapshots carry saved tables, not independent scripts; use their base instrument declarations, rather than counting them as extra compiled instruments.

**The previous 2,741-NKI denominator is not 2,741 independent instruments.** `Pacific Ensemble Strings/KONTRA project recovery/Z:/LIBRARIES/Kontakt Libraries/Pacific Ensemble Strings` resolves back to the parent library. Following the link repeats the recovery hierarchy until filesystem symlink traversal fails. A fresh `os.walk(..., followlinks=True)` reproduced 2,741 NKI, 94 NKM and 1,103 NKSN paths. Rankings below count the 781 NKI paths without these recursive aliases. Multi counts and unique script-source counts are separate. Counts overlap; do not add them. Static symbol exposure is a ceiling for affected instruments, not proof that every conditional call executes or changes sound.

`examples/ksp_audit.rs` traverses decrypted containers **in memory**, skips bypassed and whitespace-only script slots as the production translator does, supplies translated group names, original slot and saved scalar/numeric-array values, and calls v2 `init_engine_pars`, `compile_with`, UI-IR conversion and empty-plan binding. It compares the same source against the unchanged v1 compiler and runtime in a disposable Git archive, through stdin/stdout pipes. No extracted scripts, samples, saved text or keys are written. Per-item results contain counts, hashes and metadata only. Results are cached within each shard by source, slot, group names and saved state; no cross-context success is inferred. The lexer-like token census strips comments and strings but does not prove callback reachability. Lowering coverage counts are restricted to successfully compiled call sites.

This is a specialized KSP frontend/service probe, complementing the shared full-instrument scanner; its results must not be relabeled as whole-instrument load/play/UI rates.

v1 uses `LogEngine`, eight outputs, zero zones, independent script initialization at slot zero, and its supplied saved table. Its compile metric excludes programs with disabled-block compiler errors; its init metric is successful `Runtime::load` return, **not** verified service completion or absence of retained load-time fault records. v2's init metric includes parse/type analysis and init evaluation; persistence-callback faults can be warnings rather than admission failures. The first pass omits resource performance views; a resource-aware pass supplies `.nckp` controls to both engines and records retained v1 load-time fault records. Both passes use isolated slots; their service limitations remain. These are controlled frontend/service-boundary measurements, not a complete instrument load, five-slot shared-PGS initialization, reference-host render, or an assertion that every callback branch runs successfully.

The v1 load retains fault records in **140/788 NKI script slots** despite returning success. A follow-up of 15 representative NKI paths covers **17 source hashes with faults** and reports only static fault kinds: **14 array-bounds records** (read 0/write ignored) and **3 missing-PGS-key records**. Those sampled counts are not the whole-corpus fault frequency. They are nonfatal reports, can depend on zero-zone/isolated-slot host context, and do not establish 140 aborted init callbacks.

The full native-host fidelity rate remains **unknown**. It requires matching Kontakt inputs, selected snapshot, host transport, UI actions and rendered audio.

## Findings ranked by instrument exposure and user impact

Severity: P0 = reproduced user-visible breakage; P1 = incomplete behavior with corpus exposure but conditional/native impact not fully measured; P2 = narrower conformance limit. Effort S/M/L is implementation scope, not elapsed time. Counts are distinct NKI paths unless explicitly marked NKM. Ties are ordered by user impact. All source references are at baseline `7e82b152`.

| Rank | Severity / exposure ceiling | Evidence and root cause | Concrete fix / effort |
|---:|---|---|---|
| 1 | **P0 — 775/781** engine-control users | Dynamic volume probe leaves unity PCM instead of silence; every engine user emits Approximate/Effect call sites. Most parameters only write a private mirror (`lower.rs:1960`); production `lib.rs:278` cannot apply them. | Merge `22009d1e` for its volume/pan/tune cases, then shared engine dispatch/readback in KSP/core/effects and typed module laws. **L** |
| 2 | **P0 — 775/781** explicit persistent-declaration users | Serialized `Part` has no script/control state (`src/plugin.rs:35`, `:981`), unlike v1 `Part::script_state`. These fields cannot capture a changed persistent value in host/multi serialization. | Merge UI scalar-state fix `47f14b59`, then script cells, arrays/text, callback transaction and snapshot exclusions across plugin/IR/KSP/core. **L** |
| 3 | **P0 — 775/781** purge users | Legal persistence callback purge still plays unity PCM; loaded status reads zero. Init requests are ignored (`eval.rs:1252`); runtime attenuation substitutes for sample residency. | Carry load-time purge into group admission; add residency/load service, real status and async completion in Kontakt/core/plugin. **L** |
| 4 | **P0 — 775/781** engine-display users | Pan-display probe returns empty; init/runtime get_engine_par_disp fallbacks return empty (`eval.rs:1228`, `lower.rs:986`). | Shared module value formatting from real engine state; map `_ext` requested-value formatting. **M** |
| 5 | **P0 — 774/781** modulator lookup users | Missing modulator returns a name hash instead of -1; envelope routing recognizes only a default-name hash (`eval.rs:1243`, `lower.rs:1903`). | Preserve native group/modulator/target identity in IR/import; resolve names by owning engine object. **L** |
| 6 | **P0 — at least 772/781** output-count references | Runtime host slots stay zero; musical duration probe reads zero. Production never calls `set_host_value` (`lower.rs:72`, `crates/sampler-core/src/ops.rs:559`). | Populate host state on load and audio-block/transport ingress; retain callback-local ID/type/source state. **M** |
| 7 | **P1 — 717/781** transport-signal users | Start/stop subscriptions compile but have no signal ingress; only timer/PGS drivers bind (`lib.rs:426`, `:688`). v1 dispatches host transport. | Add transport listener edges and ordering to MIDI/host ingress, KSP binding and core scheduler. **M** |
| 8 | **P0 — 505/781** set_text users; 58 knob-label users | Authored runtime label stays `old`; init shortcuts exist but production consumer rejects runtime shortcuts (`lib.rs:278`). | One complete typed UI effect consumer/model update path; thread-safe dispatch/redraw in plugin/UI. **M** |
| 9 | **P0 — 371/781** persistence-callback users | Callback reports INIT; compiled entry never binds on live recall (`eval.rs:523`, `lib.rs:426`). Host save/export gap is rank 2. | Implement script-aware recall and callback type/order in KSP/core/plugin; keep load-time restore ordering. **M** |
| 10 | **P0 — 370/781** callback-ID users | Note/release IDs alias event IDs; stop_wait test cannot resume the coroutine (`lower.rs:823`, `:2038`). | Invocation IDs plus scheduler cancellation/mode state; do not use event identity as callback identity. **M** |
| 11 | **P1 — 370/781** menu-mutation users | Runtime visibility/string requests are discarded; getters cannot observe changes (`lower.rs:2022`, `lib.rs:278`). | Extend the shared UI consumer and menu readback; merge passive-menu fix `47f14b59`. **M** |
| 12 | **P1 — 369/781 NKI; 422/834 master NKI/NKM** saved string-array paths | `!` entries are discarded in `library.rs:1460`; Saved/environment has no typed text-array path. | Merge decoder `847d4670`; wire typed text arrays through IR, load and capture. **M** |
| 13 | **P0 — 368/781** stop_wait users | Effect emitted with no scheduler consumer; both resume and skip-future-wait modes absent. | Same callback-lifetime repair as rank 10, core behavior and KSP lowering. **M** |
| 14 | **P1 — 351/781** IR-load users; wait_async 8; async callbacks 10 | Load request returns zero/no installation; async completion never binds (`eval.rs:1258`, `lib.rs:426`). | Resource/IR/NKA job service, request IDs, failure status, cancellation, waiters and async ingress. **L** |
| 15 | **P0 — 349/781** tick-conversion users | Runtime 960 ticks becomes 500 us, expected 500,000; reverse conversion ×1,000. Fixed-tempo waits/listeners are separate gaps (`lower.rs:1418`, `:1612`). | Tempo-aware integer time conversion and scheduler deadlines; preserve rounding/sub-frame credit. **M** |
| 16 | **P0 — 309/781** EVENT_PAR_ALLOW_GROUP references | Current-event group mask write drops the entering note: lookup is deferred-note-only (`crates/sampler-core/src/behavior.rs:1994`). | Update the currently admitted event's group projection and query mask; preserve generated-note path. **M** |
| 17 | **P1 — 283/781** release-trigger-counter users | Reset request has no applied release-age state (`lower.rs:2035`). | Event/voice release origin in core and typed reset dispatch; measure actual release-trigger group audio. **M** |
| 18 | **P0 — 143/781** table-bearing instruments | Dense values reach UI IR, renderer draws empty bars and has no array editing (`ir_view.rs:348`); indexed setter probe also aliases cells. | Correct indexed property keys/variable writeback in KSP, then editable dense table renderer. **M** |
| 19 | **P0 — 30/54 NKM**, including 15 mf_get_first users | 45 script slots fail v2 compilation. 30 slots use subscribe_async; 15 more use MIDI-object traversal. v1 disables 19 blocks in the former; admits the latter with unsupported-command diagnostics. | Add real async subscription/MIDI-object adapter and builtins in KSP/sema/lower/core; retain correct program ownership and multi event/async context. **L** |
| 20 | **P1 — 1/781** XY instrument, plus 101 saved snapshots | XY IR retains real-array coordinates/config, renderer is a placeholder (`ir_view.rs:356`); v1 also has only a partial fallback. | XY paint/hit/drag + real-array writeback and indexed-property model, then native cursor-state replay. **M** |

Init engine replay/readback is part of rank 1; unsupported raw parameter symbols occur in 775 instruments. Its narrower init-reachable population was not recomputed without a call graph; do not reuse the prior alias-inflated number. Runtime call-site coverage below distinguishes native and unapplied emissions. Source-only exceptional cases (muted group indices, >12 modulator IDs, real-sort users, native persistence consumption) retain unknown affected populations.

## Unsupported builtins and engine parameters

The table ranks commands by **script-slot occurrences**, with independent source hashes and distinct NKI paths alongside. Symbols are found after stripping comments/strings; source occurrences include init-only calls. Slots repeat across instrument variants and are not independent executions. Whole-module completeness and native laws are not inferred from catalog size.

| Command / callback | Script slots | Source hashes | NKI paths | Remaining behavior |
|---|---:|---:|---:|---|
| get_engine_par_disp | 775 | 42 | 775 | empty text |
| purge_group | 775 | 42 | 775 | attenuation/status/load-time gap |
| set_engine_par | 775 | 42 | 775 | generic/dynamic dispatch unapplied |
| find_mod | 774 | 41 | 774 | fabricated identity |
| set_text | 507 | 29 | 505 | runtime shortcut rejected |
| set_menu_item_visibility | 370 | 9 | 370 | runtime mutation rejected |
| stop_wait | 368 | 7 | 368 | unconsumed scheduler request |
| load_ir_sample | 351 | 25 | 351 | no resource/install completion |
| ticks_to_ms | 349 | 23 | 349 | units/tempo approximation |
| save_array | 335 | 6 | 335 | no file service |
| reset_rls_trig_counter | 283 | 12 | 283 | unconsumed reset request |
| get_engine_par | 235 | 6 | 234 | private mirror/authored-state gap |
| load_array | 101 | 3 | 101 | no file service |
| set_knob_label | 59 | 13 | 58 | runtime shortcut rejected |
| async_complete | 10 | 5 | 10 | entry not bound |
| wait_async | 8 | 4 | 8 | Native-labelled no-op |
| load_array_str | 7 | 3 | 7 | no file service |
| save_array_str | 7 | 3 | 7 | no file service |
| get_engine_par_disp_ext | 1 | 1 | 1 | empty text |
| ms_to_ticks | 1 | 1 | 1 | units/tempo approximation |


A machine-readable companion, [ksp-builtins.tsv](ksp-builtins.tsv), also ranks every non-Native emitted builtin by runtime script-slot occurrences, retaining paths, hashes, emitted call-site counts after lowering/inlining and status (not execution counts). This is lowering coverage, not a parser whitelist. A command can occur in several coverage categories and can have implemented cases: e.g. set_control_par VALUE works while unsupported properties do not. Generic set_engine_par and purge_group appear Native in some categories despite the demonstrated service/semantic gaps. The table above calls those out explicitly.

The newly found whole-corpus compiler failures are **subscribe_async in 30 NKM script slots / 2 source hashes**, and **mf_get_first in 15 NKM slots / 1 hash**. They were absent from the previous NKI-only audit. The latter belongs to the missing MIDI-object command family; the former to async subscription. No NKI uses either command in this census. These 45 failures overlap **30** multi paths; they must not be added as 45 broken multis. Ownership-labelled remeasurement places these failures in **embedded programs**, with one translated group. The earlier zero-group result came from a probe context omission, not script ownership. Production `sampler-kontakt/src/library.rs:91` selects these embedded programs; the frontend census still is not a complete rack-load failure measurement. Bank-global scripting remains a separate importer/service boundary, not the explanation for these failures.

`Coverage::Native` describes emitted code, not native Kontakt fidelity. In particular `wait_async` is classified Native but is a no-op (`lower.rs:2018`). `get_engine_par` is classified Native even for a private write mirror (`lower.rs:1954`). Host/Effect entries need a production consumer before claiming functionality. Init-only calls are not represented in runtime-lowering coverage; the static column includes them.

There are 676 distinct catalogued `$ENGINE_PAR_*` names. **14 names have some specialized lowering; 662 have no specialized dispatch**. Conditional support is not module-law certification:

| Parameter family | Current v2 set/get behavior | Required boundary |
|---|---|---|
| VOLUME / PAN / TUNE | Literal parameter and slot/generic recognition reaches native group/instrument layers. Dynamic parameter falls through to mirror/outbox. Init writes are not replayed. Generic reads start at zero in init. | Share authored/current state and implement dynamic addressing; merge `22009d1e` for its covered cases. |
| ATTACK / DECAY / RELEASE / SUSTAIN / ATK_CURVE | Writes specialize only the fabricated hash of `ENV_AHDSR`. Reads use a mirror; no authored modulator identity. | Preserve real modulator/target identities and native laws. |
| EFFECT_BYPASS / SEND_EFFECT_BYPASS / INSERT_EFFECT_OUTPUT_GAIN / SEND_EFFECT_OUTPUT_GAIN / SEND_EFFECT_DRY_LEVEL | Slot-control writes and five init-rack properties exist; arbitrary slot/module readback is incomplete. | Validate addressed module and expose current state to every script slot. |
| OUTPUT_CHANNEL | Special group-to-bus write path is approximate; not complete source/output routing. | Translate owning output objects and validate route/readback. |
| Remaining filter, EQ, FX, modulator, source, send-level, type/subtype and group-start parameters | Private mirror plus effect emission; production UI effect consumer rejects `set_engine_par`. | Typed shared engine dispatch plus DSP/module/routing consumers. |
| get_engine_par_disp / _ext | Empty text at init and runtime. | Format the current/requested value using actual module law. |

Source: `crates/sampler-ksp/src/lower.rs:1856`, `:1903`, `:1931`, `:1954`, `:2184`, `:2203`, `:2220`; `crates/sampler-kontakt/src/effects.rs:175`; `src/plugin.rs:892`; `crates/sampler-ksp/src/lib.rs:278`. The prior full scoped parameter appendix is on `v2/gpt-ksp-audit@a1446ec6`, `docs/architecture-v2/KSP_AUDIT_ENGINE_PAR.tsv`; its 679 scoped rows include duplicate category membership. Do not confuse that row count with 676 names.

[ksp-engine-par.tsv](ksp-engine-par.tsv) ranks every observed parameter with path/slot/source counts and conditional specialization status. Largest raw parameter exposures below are identifier mentions, not executed module writes. Each 676-name catalog entry still needs addressed get/set/value-law verification.

| Parameter / family representative | NKI paths | Current limitation |
|---|---:|---|
| VOLUME / PAN / OUTPUT_CHANNEL | 775 / 773 / 773 | Literal specialized paths; dynamic/init/readback/routing gaps above |
| MOD_TARGET_INTENSITY | 767 | No specialized module dispatch |
| DECAY / ATTACK / RELEASE / SUSTAIN / ATK_CURVE | 760 / 735 / 722 / 715 / 503 | Default-name AHDSR specialization only |
| EFFECT_BYPASS / SEND_OUTPUT_GAIN / SEND_DRY_LEVEL | 719 / 719 / 718 | Limited recognized rack cases |
| EFFECT_TYPE / SEND_EFFECT_BYPASS | 370 / 370 | Type replacement absent; bypass conditional |
| SEND_EFFECT_TYPE / SENDLEVEL_0 | 369 / 369 | Generic private mirror/outbox |
| RV2_TIME / RV2_PREDELAY / RV2_TYPE | 368 each | Reverb module state/laws not dispatched |
| INSERT_EFFECT_OUTPUT_GAIN | 358 | Recognized rack write; readback/context incomplete |
| STEREO | 350 | Generic mirror/outbox |
| HOLD | 229 | Not among specialized AHDSR stages |
| GN_GAIN | 58 | Generic mirror/outbox |

The union of NKI paths referencing at least one nonspecialized parameter is **775/781**. Unsupported module laws occur even in scripts where some specialized volume/envelope controls work. v1's substantially broader host API and command handling do not prove every one of its parameter laws either; a 676-parameter native get/set test was not run.

## Callback timing and lifecycle

Authored v1 bridge probes at 48 kHz observe: ms_to_ticks(500000)=960; default quarter duration=500000 us; nonempty pan text `L 100`; transport listener fires once; stop_wait resumes its waiting note; early native menu read produces item value 80; invalid saved menu index chooses first item value 20. The same v2 contracts fail in the reused/follow-up suites. The v1 ordinary-wait probe advances 48 then one more frame: the continuation runs when processing the frame at its deadline, not at the end boundary of the preceding block. Both engines restore explicit saved 5 again after an init assignment of 99. These are authored adapter probes, not recordings from Kontakt.


| Contract | v2 result / evidence | v1 comparison and remaining measurement |
|---|---|---|
| wait(microseconds) | Native sample scheduler; waits round upward to frames, with sub-frame credit for short repeated waits (`sampler-core/src/behavior.rs:1883`). Callback wait lifetime permits delayed work after an originating note releases. Existing block/rate/held/release tests exercise it. | v1 coroutines also implement waits. Native tiny/zero/negative waits and simultaneous callbacks still need differential recordings. |
| play_note duration 0 / -1 / positive | 0 maps to UntilSilent; -1 maps to parent gate only in note context; positive microseconds convert to frames (`lower.rs:2819`). Invalid other negatives fault. Existing delayed-child/offset/stage tests cover ownership and onset. | v1 maps native note lengths and queues release deadlines. Neither passing parser nor these authored tests establishes every library's musical behavior. |
| wait_ticks / beat listener | Hardcoded 120 BPM. wait_ticks uses 520 us/tick, so 960 ticks is 499,200 us rather than 500,000; beat listener uses fixed 500,000-us quarter (`lower.rs:1612`, `:197`, `:231`). | v1 derives durations from `quarter_us` and current host tempo (`v1 src/ksp/calls.rs:833`). Need tempo changes while suspended and transport-loop discontinuities. |
| ms_to_ticks / ticks_to_ms | Runtime conversion is off by 1,000 for the documented microsecond units (`lower.rs:1418`); init is a different implementation. Existing probes supply 500,000 and 960. | v1 uses 960 ticks/current quarter microseconds (`v1 calls.rs:310`). |
| set_listener transport start/stop | Timer listeners run, but start/stop subscriptions never bind to host transport (`lib.rs:688`, `:426`). | v1 exposes `set_host_transport` and listener dispatch (`v1 runtime.rs:2182`). |
| callback IDs / stop_wait | Note/release IDs alias the event ID; plan callbacks read a host default. `stop_wait` emits an unhandled service (`lower.rs:823`, `:2038`). | v1 has per-coroutine wait state, cancellation requests and mode handling (`v1 calls.rs:855`). |
| async loading / wait_async / async_complete | IR/NKA requests have no production service; no async identity/completion path is bound, and wait_async does nothing (`eval.rs:1258`, `lower.rs:2018`, `lib.rs:426`). | v1 has pending async IDs, suspended waiters, async-complete ingress and file jobs. Actual successful IR/NKA native-host comparisons remain required. |
| persistence_changed | Executes during control-thread load after restore, but still reports INIT type. Compiled runtime entry is not bound on recall (`eval.rs:181`, `:523`, `lib.rs:426`). | v1 has explicit callback scheduling; verify snapshot types 0–3 and instrument-only variables against Kontakt. |

Official contracts: [NI time-related commands](https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/time-related-commands), [NI callbacks](https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/callbacks). The code and authored probes establish KONTRA behavior; the manuals define the target.

## Script UI and sampler-ui-ir

| Widget / page asset | NKI paths | Widget instances |
|---|---:|---:|
| Button | 775 | 14,189 |
| Label | 775 | 119,472 |
| Menu | 775 | 7,102 |
| Slider | 775 | 31,536 |
| Switch | 774 | 92,914 |
| ValueEdit | 774 | 30,508 |
| TextEdit | 370 | 1,550 |
| Table | 143 | 371 |
| Knob | 58 | 494 |
| FileSelector | 7 | 7 |
| LevelMeter | 3 | 26 |
| Panel | 2 | 134 |
| Wavetable | 1 | 1 |
| Xy | 1 | 1 |
| Wallpaper | 775 | Page image, not a widget |
| Waveform | 0 | 0 |

Counts include resource-authored Conflux controls; they are instantiated declarations/assets, not independently interactive widgets. Resource-free guessed controls inflated Conflux knobs (596 versus 494 across NKI); use the actual `.nckp` kinds.


`sampler-ksp/src/ui.rs:321` maps every declared kind to a typed IR kind. Representation and interaction are different coverage questions:

| Source widget | UI IR and binding | Current production result |
|---|---|---|
| Knob | `Kind::Knob { range, display }`, `Binding::Control` | Scalar value and callback path exists. Vertical drag exists, but source sensitivity is ignored. Runtime shortcut label/unit/help changes are discarded. |
| Slider | `Kind::Slider { range, orientation: Horizontal }`, `Binding::Control` | Scalar drag exists. The mapper hardcodes horizontal orientation; renderer uses orientation. Some bitmap sliders look like knobs; native mouse behavior needs explicit mapping. |
| Menu | Ordered item text/value/visibility in `Kind::Menu`, scalar control | Valid native saved index is remapped to semantic item value. Renderer cycles on click, and passive rendering rewrites unmatched values (`src/ui/ir_view.rs:306`). Runtime menu mutation services are rejected. |
| Table | `Kind::Table { columns, range, cells, ... }`, `Binding::Variable` | Initial cells reach IR, but renderer draws one-pixel empty bars and has no variable-array writeback (`ir_view.rs:348`). v1 renders actual dense cells (`v1 perf_view.rs:1201`); interactive v1 table editing is not established. |
| XY | `Kind::Xy { cursors, sensitivity, mouse_mode }`, real-array variable binding | IR retains cursor count/configuration; renderer paints a placeholder and does not display/edit coordinates (`ir_view.rs:356`). v1 also has a partial/blank fallback, not established functional XY interaction. |
| Label | `Kind::Label`, text/picture/style | Initial text/pictures map. Runtime `set_text` is not consumed. Renderer forces one line; source text layout and bitmap fonts remain incomplete. |
| Wallpaper | Page `Background.image`, asset metadata and skin offset | Declared picture/metadata maps; UI probe does not resolve actual art. Renderer/resource parity is independently audited by UI agent. Wallpaper state/cropping/nine-slice/font limitations are documented on `v2/gpt-kontakt-ui`. |
| Text edit / file selector / waveform / wavetable | Typed IR with variable/sample/file semantics | Placeholder faces; no equivalent working edit/picker/waveform path here. Conflux's native/Komplete UI request is separate from KSP UI and is not solved by admitting KSP declarations. |
| Level meter | `Binding::Meter`, channel/bus | Production renderer draws a constant silent meter (`ir_view.rs:347`). |

UI-related merge work: `v2/gpt-kontakt-ui@cda4ce23`, especially `47f14b59`, has scalar recall/admission/redraw and passive-menu fixes. It does **not** implement the general runtime UI shortcut consumer or table/XY interaction. Review its diffs in the audit/UI integration plan rather than rebuilding those fixes.

## Persistence

`make_persistent` and `make_instr_persistent` are declaration persistence modes, not runtime host services (`hir.rs:52`); `read_persistent_var` restores from the supplied environment at the point of the init call (`eval.rs:1132`). The evaluator then restores every persistent declaration again at init completion (`eval.rs:181`). Valid native menu index-to-item-value mapping and ordinary numeric-array repeated-tail fill are **already fixed**, at `eval.rs:338`.

The missing boundaries are:

1. **String arrays:** `sampler-kontakt/src/library.rs:1460` accepts `$ ~ @ % ?` but drops `!`; `load.rs:550` only constructs numeric array environments. The raw declaration-aware census on `v2/gpt-decipher-persist@6ae15820` measured **2,707 saved string-array entries in 422/834 master NKI/multi paths**. They are not “unsupported declarations”: the values are decoded away before restore. Add typed text arrays through `sampler-ir::Saved`, load admission and persistence capture. Preserve LF cell boundaries, empty cells and terminal LF, rather than whitespace splitting.
2. **Native menus versus host values:** the evaluator assumes every supplied menu integer is a native saved position. Valid positions work; invalid positions fall through as semantic values. Native and host-state origins need separate types/admission. Una Corda's velocity index fix is already integrated; do not regress it.
3. **Early menu reads:** `read_persistent_var` before `add_menu_item` writes a raw index to the script variable, instead of preserving the pending menu selection while items are built. v1 explicitly stores a pending `native_menu_index` (`v1 ui.rs:37`, `:68`). The authored follow-up probe observes derived init text, not merely the final selection.
4. **Pending saved entry consumption:** inspected Kontakt restore code consumes the explicit-read entry; current v2 and v1 restore again at init completion. The new authored probe isolates this. The consumption rule is a static RE finding and still needs a live Kontakt oracle before assigning a corpus breakage count.
5. **Lifecycle/export:** persistence metadata exposes control/cell/text locations, but audited `src/plugin.rs:35`’s serialized `Part` has neither `script_state` nor saved control values. `PartAtoms::control_values` (`:349`) exposes live scalar values without including them in host/multi state. v1 retains `Part::script_state` (`v1 plugin.rs:47`) and script persistence snapshots (`:453`). The core scalar capture API also excludes ordinary persistent cells and text arrays. Recall does not dispatch the persistence callback. Implement a script-aware capture/restore transaction and snapshot modes; retaining only host control values is insufficient.
6. **Repeated initialization:** import harvests init engine writes (`library.rs:267`), recompiles to discover dynamic effect slots (`:283`), then preparation compiles/executes init again (`load.rs:611`). Random/time/resource-dependent results can disagree. Measure and retain one initialized model with the authored engine context; do not reuse a naked source-only cache across different saved state or groups.

The typed record reader already exists on **`v2/gpt-decipher-persist@6ae15820`**, implementation `847d4670`: `SavedEntry`, declaration-aware `MenuIndex`, `ArrayTail`, real/integer/text arrays, limits and malformed-entry errors. **Merge the reader, then wire it into production restore.** That branch deliberately does not modify runtime policy; merging it alone will not restore dropped arrays or complete snapshots. It also handles legacy v0x50 password layout; no installed v0x50 records were measured.

Reused declaration-aware corpus evidence is `~/.cache/kontakto-gpt-decipher-persist/aggregate.tsv`: all 1,937 master paths completed with zero framing errors; 1,103 snapshots include 801 v1 and 302 v3 tables. It measured shorter encoded integer arrays in 441 paths, native menu records in all 1,937 paths, and XY saved arrays in one instrument plus 101 snapshots. These are declaration-context and saved-record counts, not counts of independently rendered instruments. The current filesystem inventory independently verifies the 1,103 snapshot count.

Official lifecycle target: [NI persistent variables](https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/variables). File grammar and pending-consumption evidence: `v2/gpt-decipher-persist`’s `KSP_PERSISTENCE_FORMAT.md`, “Instrument state, snapshots, init and NKA.”

## Status of the previous ranked audit

The reused suite is unchanged from the previous branch: **43 probes: 3 passing normal baselines and 40 opt-in failures**. The fresh integration run has exactly the same result (normal baselines pass, every enabled failure still fails). Forty tests are not a one-to-one mapping to forty findings: some findings have several probes, others were source-only. The table records that distinction rather than inventing new executions. Every named probe below was executed and failed at `7e82b152`; source-only entries remain open, with the specific missing follow-up noted.

| Prior ID | Fresh verification / remaining limit |
|---|---|
| KSP-01 | dynamic_engine_parameter_write_changes_audio |
| KSP-02 | engine_display_query_returns_pan_text |
| KSP-03 | purge_state_reports_loaded_samples; load_persistence_purge_excludes_group_from_playback |
| KSP-04 | musical_duration_is_available_in_note_callback |
| KSP-05 | missing_modulator_returns_not_found |
| KSP-06 | init_engine_read_matches_authored_center_pan; init_engine_write_reaches_runtime |
| KSP-07 | set_text_updates_the_runtime_ui_model |
| KSP-08 | Source-traced: release-reset emission still has no consumer; native release-age probe required |
| KSP-09 | set_event_group_can_disable_current_note_group; current_event_group_allow_state_is_readable |
| KSP-10 | Source-traced: timer/PGS binding remains; no transport ingress |
| KSP-11 | relative_mode_two_is_absolute; pan_mode_two_is_absolute |
| KSP-12 | persistence_callback_has_its_own_type |
| KSP-13 | Source-traced: menu effects still absent from production consumer |
| KSP-14 | note_and_release_have_distinct_callback_ids |
| KSP-15 | event_source_reports_creating_script_slot |
| KSP-16 | init_current_script_slot_uses_environment |
| KSP-17 | affected_groups_have_dynamic_size; editor selection remains a placeholder |
| KSP-18 | stop_wait_resumes_suspended_callback |
| KSP-19 | asynchronous_ir_request_has_identity_and_completion |
| KSP-20 | runtime_time_conversion_uses_microseconds; runtime_tick_conversion_returns_microseconds |
| KSP-21 | Source-traced: file/NKA host effects lack load/save consumer |
| KSP-22 | init_engine_read_matches_authored_center_pan; cross-slot/rack reads remain unmeasured |
| KSP-23 | pitch_bend_can_trigger_controller_callback; poly/rpn/async binding still absent |
| KSP-24 | Source-traced: keyswitch start-condition translation remains limited; no fresh native group-start probe |
| KSP-25 | active_event_status_is_note_queue |
| KSP-26 | later_script_init_reads_prior_slot_pgs_state |
| KSP-27 | text_property_write_is_visible_to_script |
| KSP-28 | table_value_setter_updates_script_array; indexed_control_properties_do_not_alias |
| KSP-29 | release_velocity_is_event_parameter; play-position reference still required |
| KSP-30 | get_event_ids_contains_current_event |
| KSP-31 | event_mark_is_readable; note_off_by_marks_releases_matching_event |
| KSP-32 | reset_ksp_timer_resets_readback |
| KSP-33 | real_array_sort_executes |
| KSP-34 | string_capacity_covers_320_characters |
| KSP-35 | custom_event_array_parameters_roundtrip |
| KSP-36 | midi_file_buffer_commands_are_supported; new NKM mf_get_first admission failure |
| KSP-37 | global_ui_callback_is_supported; multiscript MIDI-in adapter still absent |
| KSP-38 | real_search_is_rejected_as_documented |
| KSP-39 | Source-traced candidate: original muted-group indices still compacted; affected population and executed NKI repro unknown |
| KSP-40 | thirteen_script_modulator_ids_roundtrip |

The three normal baselines preserve integer/polyphonic state, aliased ignore_event and a timer listener generating notes. Existing native wait/event-ID/fade/group/rack/PGS tests also pass. The follow-up baseline passes valid native menu index restoration and numeric compressed-tail fill; do not reopen those already integrated fixes. Three follow-up probes intentionally fail: explicit-read consumption, menu selection before item construction, and invalid native menu index. The consumption probe tests a static RE interpretation shared by v1/v2, not a live native-host result.


## Prior work to merge, not reimplement

| Branch at SHA | What it supplies | Current integration status / limits |
|---|---|---|
| `v2/ksp-runtime-engine-par@aabc7c13`, fix `22009d1e` | Dynamic volume/pan/tune dispatch, init group/instrument replay, neutral init reads; imports/re-enables three audit probes | Unmerged into audited `7e82b152`. Partial KSP-01/06 fix, not all 662 parameter laws or authored non-neutral init reads. |
| `v2/gpt-decipher-persist@6ae15820`, reader `847d4670` | Typed saved-record decoder and declaration-aware corpus evidence | Unmerged. Consumer/runtime work still needed. |
| `v2/gpt-kontakt-ui@cda4ce23`, `47f14b59` | Scalar recall, passive-menu rendering, queued controls and UI parity evidence | Unmerged. Coordinate with UI owner; not a general host-service dispatcher. |
| `v2/gpt-ksp-audit@a1446ec6` | Ranked behavioral report, complete engine-parameter catalog appendix, authored contract probes | Unmerged evidence/probes, no runtime fixes. This audit reuses the probes against the actual integration revision. |
| Integrated `8a3fb769` / batch `88ba32f1` | Valid native menu selection indices resolve to item values | Already present; passing follow-up and existing UI tests. |
| Integrated `88fdfd6b` | Numeric array compressed tails repeat their last decoded value | Already present; passing follow-up and existing string/array tests. |
| Integrated `30953f9c` | Adds event-source host/script query and group routing surfaces | Does not preserve creator slot or fix current-event group writes. Retest actual contracts instead of trusting the commit title. |

## Shared scanner production follow-up

After the shared scanner became available, this audit consumed its canonical v2 Conflux result and ran the matching **shared v1 binary** for frozen-list row 0. No further independent corpus collector was launched. Both digests match the shared README: v1 `4ab053cde8eb1197591cc3696ef38e99709a6ac52174c56fccc24f5596129caf`, v2 `1d28987a6aa6afd089277221b1249561b478a9ae387181ee945e9df6ac4c919a`; source branch `tools/kontra-scan@54f7ea57`, pinned to the same v1/v2 baselines as this report.

| Conflux.nki production observation | Shared v1 | Shared v2 |
|---|---:|---:|
| Load admitted | yes | yes |
| Mapped-note audition | audible | audible |
| Original UI | missing-images | missing-images |
| Missing requested images | 1 | 1 |
| Visible interactive bindings | 78/78 | 107/113 |
| Load wall time | 1,786.14 ms | 18,941.86 ms |
| Worker peak RSS | 69.29 MB | 229.71 MB |

The UI and binding populations differ between renderers; 78 versus 113 is not a count of identical controls. These are single-instrument observations at different moments, not corpus rates or a controlled repeated benchmark. The README defines v1 load time as initial streaming-bank construction excluding UI rendering; v2 uses the production streamed loader and includes asset metadata resolution. Audible output establishes that this selected audition produces audio, not that KSP callbacks/engine parameters match Kontakt.

The canonical v2 JSON also records ignored engine-display queries (441 ordinary / 474 extended), event-status, menu/control string getters, event enumeration and other KSP warnings. Its successful production load therefore confirms the main distinction in this report: admission/audio can succeed while substantial script semantics remain incomplete.

Evidence: `~/.cache/kontra-scan/results/v2/results.tsv` and its digest-keyed JSON cache; own shared-v1 output `~/.cache/kontakto-audit-ksp/shared-scanner/v1/{results.tsv,cache/}`; `shared-v1.log`. At inspection the canonical v2 TSV had **one row** and the full sweep was still in progress. Requested additions through the coordinator: active/cleanly compiled script slots, init completion, load-time fault records, and separate init/persistence callback outcomes. Until those arrive, the shared scanner cannot supply a fresh whole-corpus KSP compile/init rate.

## Unknowns and concrete measurement plan

- **Native audio fidelity:** no fresh Kontakt reference-host PCM in this run. Record matched NKI/snapshot/key/velocity/CC/transport/UI scenarios; compare onset/release timing, group choice, engine values and audio at 44.1/48/96 kHz. Prior synthetic failures are definitive differences in KONTRA contracts, not quantified PCM error across every exposed instrument.
- **Five-slot init semantics:** benchmark actual ordered slots including PGS, inherited preprocessor definitions, authored engine state, original group indices and real resource containers. Isolated scripts do not establish cross-slot correctness.
- **Strict v1 init completion:** retained load-time fault records are counted separately from successful Runtime::load. Array bounds reports ignore the read/write and continue; missing PGS keys can arise from isolated slots. They are not automatically stopped-init callbacks. Capture callback entry/exit and service outcomes with all ordered slots in a matched adapter. `LogEngine` cannot validate resource completion, modulator existence or sample/IR installation.
- **Saved-entry behavioral impact:** 422 string-array paths and 441 compressed-array paths are raw master-file exposures. Read assignments/callback reachability and chosen snapshots in memory, then verify affected UI/engine outputs. Counts of invalid native menu indices, early menu reads and assignments after explicit persistence reads remain unmeasured.
- **Host state/timing:** inject changing BPM/signature/position/transport and verify timer/listener ordering, `stop_wait` modes, note durations, callback IDs and async failure/cancellation. Add actual audio-block adapter ingress; merely writing host slots in a synthetic test is insufficient.
- **Parameter laws:** independently check all supported conversions at endpoints/intermediate values and while voices are active. Saved static FX translation and live scripted edits are different coverage paths. Missing enum values cannot be inferred safely from identifier spelling or hashes.
- **UI resources/interactions:** resolve real NKR/NICNT `.nckp` and art, then render and manipulate controls in the shared preview. This probe validates IR data, not bitmap resolution, geometry, popup interaction or Conflux's Komplete frontend.
- **Frontend cost:** per-script timings above isolate parser/init/compiler/load components and include in-shard cache reuse. They are not matched whole-library load times or audio callback CPU. The loader's repeated-init root cause should be checked with a single initialized artifact and the same instrument/snapshot in both products.

## Reproduction and validation

```sh
cd /home/derpcat/.t3/worktrees/KONTAKTO/audit-ksp
bash tools/audit-ksp/prepare-v1.sh
~/.cache/kontakto-heavy cargo build --profile ci --locked \
  --no-default-features --features library-access --example ksp_audit
/mnt/Windows11/DEV_WORKSPACE/Toolchains/User/cargo-target/kontakto-audit-ksp/ci/examples/ksp_audit --check
# Historical KSP corpus measurement: tools/audit-ksp/run.py (already completed).
# Further corpus work uses the shared scanner, with KSP columns requested
# through the coordinator; do not launch another independent collector.
~/.cache/kontakto-heavy ~/.cache/kontra-scan/bin/kontra-scan-v1 \
  --list ~/.cache/kontra-scan/kontakt-items.tsv --start 0 --count 1 \
  --out ~/.cache/kontakto-audit-ksp/shared-scanner/v1
python3 tools/audit-ksp/summarize.py > ~/.cache/kontakto-audit-ksp/summary.json
~/.cache/kontakto-heavy cargo test --locked -p sampler-ksp
# Known failures are deliberately ignored in normal CI; run to measure them:
~/.cache/kontakto-heavy cargo test --locked -p sampler-ksp --test audit -- --ignored
~/.cache/kontakto-heavy cargo test --locked -p sampler-ksp --test audit_followup -- --ignored
~/.cache/kontakto-heavy python3 tools/audit-ksp/v1_contracts.py
~/.cache/kontakto-heavy cargo test --locked --no-run
```

The raw source cache referenced by the old audit no longer exists. Do not regenerate it with `dump_scripts`; this probe reads directly into memory. Shards have an outer 290-second timeout and release the heavy slot between calls; results live only under `~/.cache/kontakto-audit-ksp`. The resource-aware JSON shards retain only metadata, and `isolated-shards/` retains the earlier resource-free measurement. The runner resumes completed shards; use a separate clean cache generation when changing probe/context rather than silently mixing versions. No server is started. Checks which intentionally demonstrate contract failures must not be reported as passing compatibility tests.

Fresh verification logs are in the private audit cache: `ksp-tests.log` (123 passed / 0 failed / 43 intentionally ignored), `contracts.log` (40 expected failures), `followup-failures.log` (3 expected failures), `v1-contracts.jsonl`, `resources-shards.log`, `nkm-groups.jsonl`, `fault-kinds.jsonl`, `probe-selfcheck.log`, and `no-run-final.log`. The opt-in failure suites are evidence and remain ignored in ordinary CI. The shared scanner supplies separate production load/play/UI observations; they do not replace callback-level KSP measurements. Future corpus validation uses its frozen lists and resumable outputs.
