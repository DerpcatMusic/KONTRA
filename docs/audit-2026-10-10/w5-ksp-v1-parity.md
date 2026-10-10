# W5 KSP v1 → v2 parity, UI first

Reference: our `0cb7a8a0:src/ksp/{builtins,calls,compile,runtime,ui}.rs`. Candidate base: `cb98834e`. The [180-command census](w5-ksp-v1-parity.tsv) lists every public v1 command, both signatures, known missing/partial execution, and historical quick-corpus diagnostic counts. Counts are prioritization evidence from the existing receipt, not a new scanner or native parity result. Internal `#` arithmetic intrinsics are compiler-selected implementations, not public commands.

v2 recognizes 224 public commands. Four public v1 names are absent: `get_keyrange_min_note`, `get_keyrange_max_note`, `get_keyrange_name`, and `set_map_editor_event_color`; the last is a v1 no-op. `SET_CONDITION`/`RESET_CONDITION` run in v2 preprocessing instead of the runtime. Recognition and an emitted host effect are not evidence that a command executes. `routed-unverified` rows explicitly need edge tests; this census does not claim semantic parity for those rows.

## UI execution, first priority

All 16 v2 `ui_*` declaration kinds have typed HIR entries. Literal declaration support does not establish every typed value, indexed setter, asset or interaction edge. Script state feeds `sampler-ksp::ScriptView` and `sampler-ui-ir`; W3 consumes that authored output.

| Surface | Known difference on the base |
| --- | --- |
| Menu getters/edits | Init menu getters work; callback getters return zero/empty. Host menu edits are applied later and do not update the callback's getter state. Historical receipt: 930 value calls, 642 text calls, roughly 30 Conflux multi items. Port the already tested `05034e28` implementation to this base. |
| `hide_part`, knob label/default/unit/help, `set_text`, `add_text_line`, positioning | Host display effects exist; same-callback readback uses stale keyed/text properties. A page callback must observe its own writes immediately. |
| Menu control properties | v1 VALUE/SELECTED_ITEM_IDX returns the selected item index, while the menu variable holds the authored item value. v2 reads the raw value; NUM_ITEMS and SELECTED_ITEM_IDX lack live menu readback. |
| `set_control_par` | Numeric HIDE, Z_LAYER, PARENT_PANEL and other authored property writes have a runtime keyed store plus host UI effect. Retain this shared representation. Scalar/indexed real and text getter/value coverage is incomplete. |
| Indexed real/text control getters | Callback `get_control_par_real[_arr]` and `get_control_par_str_arr` have no lowering route despite init support and setters. |
| Waveform state | Callback `get_ui_wf_property` returns zero; `attach_zone`/`set_ui_wf_property` host effects have no consumer. |
| Assets | v2 init `get_folder` has no library/patch environment; both versions return empty in callbacks. `fs_get_filename` has a runtime route but init returns empty. Runtime font lookup is init-only. Picture/font/resource resolution remains the loader/W3 lane. |
| Global callbacks | v1 starts `ui_controls` before the local callback and then starts `ui_update`; v2 rejects both names. Specific `ui_control($var)` callbacks exist. Global callback context/ordering must be ported at the dispatch boundary, not faked in the renderer. |
| Keyboard ranges | Three getters are absent. Range setter/remover host effects have no runtime consumer; keyboard getter callbacks return defaults. |

## Other commands and callbacks

The TSV contains the remaining known callback-default getters, unconsumed engine/resource effects, init-only paths, and conditional selectors. Important remaining classes: array file jobs and completions; PGS strings and runtime key creation; mutable zones and snapshot-mode admission; note-controller/RPN/NRPN commands; all-event/marked-event operations beyond CUSTOM/MOD_VALUE_ID; text-array search/sort/equality; exact lookup miss/name behavior; invalid-shift validation; and optional `ignore_event()`/fourth `fade_out` arguments. v1 itself limits Time Machine voice allocation, level-meter taps, output redirection and map-editor color: these are not completed v1 features to port blindly.

Additional callback differences: `note_controller` is absent, `_pgs_changed` alias is rejected, transport listener registration/retuning validation differs, and timer beat scheduling plus `wait_ticks` assume fixed 120 BPM. v1 uses host quarter duration. v2 already owns a host clock conversion operation, so the latter requires no new scheduler or mirror. Persistent state, scheduled persistence completion and listener wait-liveness from accepted `31b`/`60c` remain in the base.

The shared-target attempts in this run are excluded. Coordinator BUILD CORRECTION requires reruns through the corrected per-worktree wrapper. No wrapper/environment override is set by this lane.

## First validated slice, f2c59e2c

The corrected per-worktree receipts in `/mnt/Windows11/DEV_WORKSPACE/kontra-runs/w5-ksp-parity-394` retain the failing contracts. On the same61 cases, valid baseline tests had45 passes/16 failures; the slice passes all61. The expanded targeted checks pass104 total, the sampler-core/KSP/Kontakt/plugin area compiles with `--no-run`, and the production Conflux command report gives unknown0. Shared-target compiler failures and invalid test-fixture attempts are excluded; no new full scanner or timing delta is claimed.

Implemented boundaries: menu count/value/text/visibility and live edits; init/callback menu VALUE and SELECTED_ITEM_IDX derived from authored item values; scalar/indexed real VALUE reads; named UI property readback; dynamic declared TYPE/range metadata; physical zone group/key getters; marked-event CUSTOM/MOD_VALUE_ID writes; ignore_controller context warning; host-tempo wait_ticks and beat listeners; native7-bit MSB/LSB and signed-zero signbit; numeric/invalid font-name init semantics. These are tested edges, not blanket command parity. Runtime named-font lookup, string arrays, knob unit representation, read-only setter enforcement, dynamic menu property selectors and invalid-selection snapping remain open.

Menu index scans retain hidden entries and return the first authored match for duplicate values, matching v1 `ControlState::selected_menu`. W5 owns runtime/logical value and index semantics; W3 owns caption rendering and popup visibility. Distinct native selection among duplicate-valued entries is unverified and cannot be represented by logical value alone; no new selection state is introduced without that evidence.

READY is held: source review found read-modify-write setter arguments being re-evaluated after callback state changed, so host transport can differ from script readback. Seven retained contracts on853a1a34 cover named text, menu numeric/text/append, numeric/text properties and XY values. A source-only fix is prepared to share evaluated arguments between both consumers. The RED job is queued through the wrapper behind the active quiet window.

NEXT: menu RMW RED→GREEN→READY, then global ui_controls/ui_update dispatch. W8 owns per-host-block callback budgets; W6 owns optional native ModScale intensity laws.
