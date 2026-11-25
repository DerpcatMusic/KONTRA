# KSP conformance against Kontakt 8

Status: slice 8 (2026-11-25), 212 units. Suite: `tools/ksp-conformance/`. Test: `crates/app/host/tests/ksp_conformance.rs`
(each unit's `on init` on a fresh engine with 66 groups, read back the way the probe reads Kontakt).

## How a result gets out of Kontakt

Kontakt runs under Wine (`tools/kontakt-oracle/dump.py`, one session under
`flock /tmp/kontra-oracle.lock`). Scripts run their `on init`, so each unit is init-only.

Readback channels tried (`tools/ksp-conformance/probes/`):

- **Instrument volume/pan/tune from `set_engine_par(...)` values.** Not usable.
  `readback_calibrate.lua`: 0, 1, -1 and -1000 read back as -inf, -100 (pan) or -36 (tune).
- **Group volume, bit channel. Used.** Result `k` bit `b` is written as
  `set_engine_par($ENGINE_PAR_VOLUME, 1000, 32*k + b, -1, -1)` when set, and read back as
  `get_group_volume(i, 32*k + b)`. A set bit reads -167.38 dB; a clear bit keeps its group
  default. Group addressing is 0-based (`group_bit_calibrate.lua`).
- **Markers (slice 2).** Group 64 is written first in `on init` and group 65 last. A unit
  whose start marker does not read back did not compile: Kontakt drops a script it rejects
  without any error we can read. The end marker shows init ran to the end.

Per unit, `gen.py` wraps the case's results in this encoder (`cases.py: ENCODER`) and emits
the same `.ksp` text for Kontakt (generated probe, 16 units per session) and for the
interpreter (`cases/<id>.ksp`). `golden.json` has `compiled`, `completed` and `results`.

Encoder syntax is KSP: parenthesised conditions, `#` for not-equal, no `then`, no
parentheses on `call`/`function` names. Real variables are `~name`.

Run: `tools/ksp-conformance/run.sh` (oracle, all batches), then
`matari fast kontra -p kontra-host -- -E 'test(every_unit_matches_kontakt)'`.
An optional sixth case field is KSP text after `on init` (functions, other callbacks): Kontakt
compiles it with the unit, so it tests what compiles outside init. Run batches through
`tools/oracle/oracle-slot` directly; wrapping it in `flock /tmp/kontra-oracle.lock` deadlocks.

## Kontakt rules (measured)

Each rule is backed by a golden unit or a probe whose start marker proves compilation.

| Rule | Evidence |
|---|---|
| A condition must be a comparison or boolean term. `if (1)`, `if (1 and 1)`, `if ($t and $f)` are compile errors. | `cond_bare_int`, `cond_int_and`, `logic_*` rejected |
| `and`, `or`, `not` of comparisons are boolean and true when the comparison is. | `pL0`–`pL4`, `qN1`, `qB0`, `qB1` |
| A boolean cannot be assigned to an integer variable. | `typing_bool_assign` rejected |
| A real cannot be assigned to an integer variable. | `typing_real_assign`, `type_real_to_int_assign` rejected |
| `call` is not allowed in `on init`, with or without parentheses, whether the function comes before or after. `call` from `on note` compiles. | `call_init_empty`, `call_init_global`, `mA`–`mD` |
| `#` is not-equal. | `qB0` |

Not probed yet: integer `and`/`or`/`not` used as values (`$x := 1 and 1`). The interpreter
still allows these, typed as integers.

## Results (47 units)

Slice 2 golden set: Kontakt compiles 34 units and rejects 13 (listed above). The interpreter
now matches all 47 (compile result, completion and bits).

## Interpreter changes (slice 2)

- `sema.rs`: a condition must be boolean (`expected a condition, found integer`).
- `sema.rs`: `and`/`or`/`not` give a boolean only when both operands are boolean, otherwise an
  integer.
- `sema.rs`: assigning a boolean to an integer variable is an error.
- `sema.rs`: `call` in `on init` is an error (`call is not allowed in on init`).

## Test rewrites (slice 2)

The sema rules made some existing tests invalid KSP. Each was rewritten to valid KSP with
the same intent, except where noted:

- `luau/tests.rs`: `and`/`or` on integers became `.and.`/`.or.` (bitwise); `while (1)` became
  `while (1 = 1)`; the function call from `on init` was inlined (function coverage in that test
  is lost; the call rule is covered by `functions.rs`).
- `functions.rs`: `while (1)` became `while (1 = 1)`; the init-call part asserts the rejection;
  the runtime part (`on note`) keeps the `exit` coverage.
- `array_value_contract.rs`: `array_equal` results are assigned through `if`. Its return type
  is not verified against Kontakt: the builtin table says boolean, the corpus never uses it.
- `event_ids.rs`: short-circuit check uses `if` with `play_note(...) = 0`.
- `select.rs`: the precedence table checks truth through `if (expr # 0)`. It no longer covers
  the integer value of const folding for these expressions (other tests cover arithmetic).
- `typed_widgets.rs`: the file-selector drop flag is an `if` assignment.
- `init_only_commands.rs`, `listener_contract.rs`, `parity.rs`, `script_state.rs`: init-call
  shapes now assert the rejection or move the call to `on note` / inline it.

## Notes

- The test ratchets: a new mismatch fails it; a fixed unit must leave `KNOWN_MISMATCHES`.
- `golden.json` comes from one oracle run on Kontakt 8.13.1. Regenerate after any
  generator or case change.
- Interpreter micro-bench (init and per-note callback cost on corpus scripts): not started.

## Slice 3 (86 units)

Probes added: numeric builtins (`sgn`, `msb`, `lsb`, `sh_*`, `pow`, `floor`, `ceil`, `round`,
`int`, `sqrt`), literal range, and boolean builtins used as numbers. Results in
`golden.json` (86 units, all decoded from Kontakt 8.13.1).

| Rule | Evidence |
|---|---|
| Integer `and`, `or`, `not` are rejected as values and as conditions; `.and.`/`.or.` are bitwise. | `intval_and_value`, `intval_or_value`, `intval_not_value` rejected; `intval_bitand` = 2, 7 |
| A boolean is not a number: `(3 > 2) + 0` is rejected. | `typing_bool_arith` rejected |
| `in_range(...) + 0` and `pgs_key_exists(...) + 0` are rejected. | `bool_inrange_arith`, `bool_pgs_arith` rejected |
| A decimal literal above 2147483647 clamps to 2147483647. | `lit_int_overflow`, `lit_u32_max`, `lit_u32_over`, `lit_3e9` |
| `-2147483648` reads as -2147483647 (the literal clamps, then negates). `-2147483649` also reads -2147483647. | `lit_neg_min`, `lit_neg_over` |
| `80000000h` wraps to -2147483648 (hex literals are 32-bit). | `lit_hex_top` |
| Division and modulo by zero yield 0 and the script continues. | `arith_div_zero`, `arith_mod_zero` |
| Shift amounts are masked to 5 bits: `sh_left(1, 32)` = 1, `sh_left(1, -1)` = -2147483648. | `builtin_sh_32`, `builtin_sh_negative` |
| `min`/`max` with two arguments are rejected in `on init`, with or without variables or negatives. | `builtin_min*`, `builtin_max*` rejected |

Interpreter changes: `sema.rs` boolean-only and/or/xor/not; boolean operands in arithmetic
are errors; decimal literal clamp; no special case for `-2147483648`.

Open: `min`/`max` (known debt in `KNOWN_MISMATCHES`; no corpus library uses them);
integer `xor` and `.not.` (not probed).

## Slice 4 (107 units)

Probes: integer and boolean `xor`, bitwise `.not.`, `search` and `sort` on arrays, string
declarations and string `=` in conditions. 107 units in `golden.json`, decoded from Kontakt 8.13.1.

| Rule | Evidence |
|---|---|
| Integer `xor` is rejected as a value and as a condition, like integer `and`/`or`. Boolean `xor` works in a condition. | `int_xor_value`, `int_xor_cond` rejected; `bool_xor_cond` = 1, 0 |
| `.not.` is bitwise: `.not. 5` = -6, `.not. 0` = -1. | `int_bitnot_dot` |
| `search(%a, v, start, end)` returns the first index in [start, end) or -1; the two-argument form searches the whole array. | `array_search_value`, `array_search_short` (1, 3, -1, 1) |
| `sort(%a, 0)` sorts ascending, `sort(%a, 1)` descending; the three-argument form `sort(%a, 0, 3)` is rejected. | `sort_asc_two`, `sort_desc_two`; `array_sort_asc` rejected |
| A scalar string declaration with a literal initializer (`declare @s := "abc"`) is rejected; `declare @s` then `@s := "abc"` compiles. | `str_decl_init` rejected; `str_assign_decl` compiles |
| String `=` in a condition is rejected in on init (literal and variable forms). | `str_lit_cond`, `str_var_cond`, `string_case_sensitive` rejected |

Interpreter changes (slice 4): `declare @s := "literal"` is an error (sema.rs). Scalar strings only;
text arrays with literal lists are not measured and still accepted.

Known debt: string `=` in conditions is accepted by the interpreter and rejected by Kontakt in on
init. The `on note` form is not measured yet; the oracle has no note-trigger probe.

## Slice 5 (126 units): control and UI readback in on init

Probes: `set_control_par` on knobs, switches, hide, position and menus; `get_control_par` and
`get_num_menu_items` read back in on init; `ui_knob` and `ui_switch` variables read back in on init;
`add_menu_item` in on init. Decoded from Kontakt 8.13.1 (`ctl_*`, `ui_var_*`, `diag_*`).

| Rule | Evidence |
|---|---|
| `set_control_par` on a knob, switch, hide, or position compiles in on init. | `ctl_knob_set_high`, `ctl_knob_set_neg`, `ctl_switch_set`, `ctl_hide_set`, `ctl_pos_x_set` compile; `diag_set_switch` compiles |
| `get_control_par` used as an integer in on init is rejected. | `diag_get_int`, `diag_get_index0`, `diag_get_switch`, `ctl_knob_default` rejected |
| `get_num_menu_items` in on init is rejected. | `diag_menu_count`, `diag_menu_count_after_add`, `ctl_menu_items` rejected |
| `add_menu_item` in on init compiles. | `diag_menu_add` compiles |
| A `ui_knob`/`ui_switch` variable set in on init reads back 0 in on init. | `ui_var_knob_default`, `ui_var_knob_set_high`, `ui_var_knob_set_neg`, `ui_var_switch_set` compile; read as 0 |

Known debt: `get_control_par` and `get_num_menu_items` are accepted by the interpreter and by
about ten existing tests (on init) that Kontakt rejects. The three `ctl_menu_items`,
`diag_menu_count` and `diag_menu_count_after_add` units are listed in `KNOWN_MISMATCHES`. Moving
those tests to on note or on ui_control, then rejecting the calls in on init, is the next step.

## Slices 6–8 (212 units, 2026-11-25)

The interpreter-side test had been lost in the monorepo import; it is back as
`crates/app/host/tests/ksp_conformance.rs` and every unit matches except the two listed in its
`KNOWN_MISMATCHES` (`load_performance_view` of a file the container lacks: Kontakt rejects it at
compile time, KONTRA's compiler is not given the container).

| Rule | Evidence |
|---|---|
| Every value in `crates/script/ksp/data/constants.tsv` is Kontakt's (one bit per name). `$OUTPUT_TYPE_NOT_SPECIFIED` is 4, not -1. | `const_00`–`const_09`; `tools/kontakt-oracle/probes/ksp/output_type.kspp` |
| `call` is rejected in on init in every form (function before or after, empty or not, with or without parentheses); it compiles in on note. | `call_init_*` rejected; `call_note_after` |
| One pass: a function or callback that names a variable declared later in the text is rejected, a function calling one defined later is rejected, and a bare function name is not a call. | `fn_before_init_later_var`, `order_note_before_init`, `order_fn_calls_later_fn`, `order_bare_name_call` rejected; `fn_empty_uncalled` compiles |
| `min`, `max` and `get_num_menu_items` are not Kontakt commands (rejected in on note too). | `minmax_note`, `ctl_menu_count_note` rejected |
| There is no string comparison: `@s = "abc"` is rejected after a plain declaration and in on note. | `str_assign_cond`, `str_cond_note` rejected |
| `get_control_par` takes two arguments; the slice-5 rejections were its third argument. | `ctl_get_two_args` = 9 |
| `add_menu_item`, `disable_logging`, `find_zone`, `get_font_id`, `read_persistent_var`, `set_listener`, `set_map_editor_event_color`, `set_ui_height`, `set_ui_height_px`, `set_ui_width_px`, `watch_var` and `watch_array_idx` are rejected outside on init (on note, on persistence_changed, on ui_control). `expose_controls`, `make_perfview` compile in on note; `set_controller`, `set_note_controller`, `set_poly_at` compile in on init. | `place_*`, `cmd_watch_note`, `cmd_map_color_note` |
| `sort`/`search` take the whole `<from>, <to>` range or none (manual); `sort(%a, 0, 3)` is rejected. | `array_sort_asc` |
| Time Machine Pro voice limits start at 8 (standard) and 4 (HQ). | `manual_voice_limit_default` |
| Measured values: `array_equal` 1/0; `get_purge_state` 1; `get_key_type` after `set_key_type(60, $NI_KEY_TYPE_CONTROL)` 2, else 0; `get_key_triggerstate` 1/0 after `set_key_pressed`; `get_group_idx` of an unknown name `$NI_NOT_FOUND` (-100); `mf_get_last`/`mf_get_prev` select the last/previous event; `mf_get_mark` reads 0 right after `mf_set_mark`; `mf_reset` empties the object (buffer size 0, tracks 0). | `cmd_*` |

Interpreter changes: decimal literals clamp in the lexer; booleans are not numbers in
arithmetic or assignment; `declare @s := "..."` is an error; text `=`/`#` and `Op::TEq` are gone;
the rules above are compile errors. `$EVENT_PAR_REL_VELOCITY` and `$EVENT_PAR_MIDI_CHANNEL` read
(and the release velocity writes) on a note event; `$EVENT_PAR_MOD_VALUE_EX_ID` is no longer clamped
(manual: "unbounded").

## Slice 9 (240 units, 2026-11-25)

| Rule | Evidence |
|---|---|
| Using a variable before its `declare` is rejected in on init too (assigning or reading). | `order_assign_before_declare`, `order_read_before_declare` rejected |
| `~NI_MATH_PI`, `~NI_MATH_E` are π and e; `~NI_DETECT_TEMPO_INVALID` is -1.0, the pitch, loudness, peak and RMS `_INVALID` are 1 000 000.0. | `real_const_*` |
| In on init: `$NUM_ZONES`, `get_num_zones()`, `$PLAYED_VOICES_*`, `$DISTANCE_BAR_START`, `$REF_GROUP_IDX` read 0; every `%GROUPS_SELECTED` entry reads 1; `$NI_CALLBACK_TYPE` is 5; `$DURATION_*` follow 120 BPM. `save_array_str` compiles. | `init_*` |
| `%GROUPS_AFFECTED`, `%KEY_DOWN_OCT`, `$RPN_ADDRESS`, `$RPN_VALUE` are rejected in on init; the first two compile in on note, the RPN ones only in on rpn / on nrpn. | `init_*_only`, `note_groups_affected`, `note_distance_bar`, `rpn_reads_rpn`, `note_reads_rpn` |
| `$NI_CB_TYPE_ASYNC_OUT` (in the manual) is not a Kontakt 8.13 name: rejected, and absent from the binary. | `const_cb_type_async_out`; `crates/script/ksp/data/kontakt-8.13.1-names.txt` |

`crates/script/ksp/data/kontakt-8.13.1-names.txt` holds the binary's own upper-case names
(`strings` of `Kontakt 8.exe` 8.13.1). `consts::tests::every_known_name_is_kontakts` keeps the
catalog and constants.tsv inside it; the 51 catalog names it removed were manual example
variables, typos, PDF line-wrap fragments, family prefixes and `$NI_MOUSE_EVENT_TYPE_DND_*` (the
binary's names are `$NI_MOUSE_EVENT_TYPE_DRAG`/`_DROP`).

Not measured: how Kontakt delivers RPN/NRPN (KONTRA compiles `on rpn` but never runs it).
A render-session probe (CC 101/100/6/38 over MIDI, `on rpn` saving with `save_array_str`) wrote
no file in four sessions, not even from a timer; that harness is unproven, not Kontakt.
