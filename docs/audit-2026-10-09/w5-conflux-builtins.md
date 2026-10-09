# W5 Conflux builtin reports on 381

Base: `54d9a5c5` (0.3.381). Branch: `v2/w5-conflux-builtins-381`.

The five reported builtin names contain four runtime gaps and one context classification error. Static init evaluation of a menu getter did not establish callback support.

| Command | Finding and fix |
| --- | --- |
| `get_menu_item_value` | Callback lowering returned zero. Seed authored menu values in the existing bounded script store and update it alongside the existing menu effects. |
| `get_menu_item_str` | Callback lowering returned empty text. Read authored/live menu text through the existing bounded text-property bank, including string concatenation. |
| `get_zone_par` | Conflux requests `ZONE_PAR_GROUP`. Port v1's Group/LowKey/HighKey selectors, preserving physical source zone IDs and holes at init and runtime. Other selectors retain an unsupported diagnostic. |
| `set_event_par_arr` | Conflux writes `EVENT_PAR_MOD_VALUE_ID` through marked-event selectors. Reuse the bounded event scan for mark unions and all events; retain per-plan isolation and modulator clamping. |
| `ignore_controller` | Conflux calls it outside a controller callback. This is an ignored context warning, not an unknown command. Valid controller suppression remains covered by the controller tests. |

Adapted v1 sources: `0cb7a8a0:src/ksp/calls.rs` (menu and zone semantics, controller context) and `0cb7a8a0:src/ksp/runtime.rs` (event target selection). No v1 file-format compatibility was added.

Receipts: `/mnt/Windows11/DEV_WORKSPACE/kontra-runs/w5-conflux-builtins-381/`.

The valid failing-first receipts are `builtins-red-valid.log`, `selection-red-valid.log`, and `controller-red-valid.log`: five targeted failures on the unchanged 381 production sources. Earlier malformed fixture/compilation attempts are not counted as RED evidence.

Targeted GREEN: strings 13/13, event selection 38/38, controllers 10/10, functions 3/3, modulation 1/1. The string and marked-event probes run callback/render or dispatch with heap allocation forbidden. Tests include live menu edits, empty-menu insertion, invalid indices, signed-32 minimum menu values, dynamic zone selectors, source-ID holes, marked-event selection, and all-event custom values.

The opt-in production test `plugin::persistence_tests::conflux_builtin_report_contexts` loads the installed Conflux instrument through the host and requires none of these five names to remain in unsupported script diagnostics. On unchanged 381 production sources it fails with all five names unresolved (`conflux-red.log`, 0 passed / 1 failed). Its receipt prints only public builtin/parameter identifiers, line coordinates, and counts; decrypted script text and state stay in memory.

Production-load GREEN: 1 passed / 0 failed, with `CONFLUX_UNKNOWN_COMMANDS 0` (`conflux-green.log`). The addressed five-name diagnostic delta is 5 → 0. This is the production host load witness, not a whole-corpus scanner rerun. The affected-package compile gate passed: `cargo test --no-run --locked --profile ci -p sampler-core -p sampler-ksp -p sampler-kontakt -p kontakto --features shots` (`no-run.log`). Full suite/scanner execution remains W0's batch gate.

Source fix: `05034e28`. Production-load RED/GREEN and compile receipts are recorded in `VALIDATION.json`. This work does not establish native Kontakt audio/gesture parity or implement additional zone parameters. The separate persistence READY is `31b24985`; W11 owns gesture attribution. No schema acceptance was broadened here.

NEXT: W0's 382 integration and sweep; W11's Conflux/Analog/Dolce gesture attribution.
