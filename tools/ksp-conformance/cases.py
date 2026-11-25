"""KSP conformance cases: small init-time scripts whose integer results Kontakt makes observable.

A case is (id, category, setup, results). `setup` is KSP statements run in `on init`
before the results are computed; `results` are KSP integer expressions over the setup
variables. gen.py wraps every result in a bit encoder (see ENCODER below) so Kontakt and
the KONTRA interpreter both report the 32 bits of each result through
set_engine_par(ENGINE_PAR_VOLUME, 1000, group, -1, -1). Group 0 of a clear bit stays at
its default level; a set bit reads back at about -167 dB (see docs/re/KSP_CONFORMANCE.md).

KSP syntax: conditions are parenthesised, `#` is not-equal, there is no `then`.

Results are computed in init. An optional sixth field is KSP text after `on init`
(functions, other callbacks): Kontakt compiles it with the unit, so a callback body there
tests what compiles outside init, though only init runs.
"""
import pathlib

# Result k, bit b goes to group 32*k + b. `$cv` holds the result, `$cb` the bit index.
ENCODER = """  $cv := {expr}
  $cb := 0
  while ($cb < 32)
    if ((sh_right($cv, $cb) mod 2) # 0)
      set_engine_par($ENGINE_PAR_VOLUME, 1000, $cb + {base}, -1, -1)
    end if
    inc($cb)
  end while
"""

DECLS = "  declare $cv\n  declare $cb\n"

CASES = [
    # --- integer arithmetic: overflow and truncation -------------------------------
    ("arith_add_wrap", "arithmetic", "declare $a := 2147483647", ["$a + 1"]),
    ("arith_sub_wrap", "arithmetic", "declare $a := -2147483647", ["$a - 2"]),
    ("arith_mul_wrap", "arithmetic", "declare $a := 65536", ["$a * 65536"]),
    ("arith_mul_neg", "arithmetic", "declare $a := -46341", ["$a * $a"]),
    ("arith_neg_min", "arithmetic", "declare $a := -2147483647 - 1", ["-$a", "$a - 1"]),
    ("arith_div_trunc_pos", "arithmetic", "", ["7 / 2", "-7 / 2"]),
    ("arith_mod_signs", "arithmetic", "", ["-7 mod 2", "7 mod -2"]),
    ("arith_mod_negneg", "arithmetic", "", ["-7 mod -2"]),
    ("arith_precedence", "arithmetic", "", ["2 + 3 * 4", "10 - 2 - 3", "2 * 3 mod 4"]),
    ("arith_abs", "arithmetic", "declare $a := -2147483647", ["abs($a)", "abs(-5)"]),
    # --- real arithmetic: real variables are `~name`, conversions are explicit ----
    ("real_round_half", "real", "declare ~h := 2.5\n  declare ~n := -2.5", ["real_to_int(round(~h))", "real_to_int(round(~n))"]),
    ("real_round_down", "real", "declare ~h := 0.49999", ["real_to_int(round(~h))"]),
    ("real_to_int_trunc", "real", "", ["real_to_int(3.9)", "real_to_int(-3.9)"]),
    ("real_to_int_big", "real", "declare ~r := 2147483648.0", ["real_to_int(~r)"]),
    ("real_int_real_exact", "real", "", ["real_to_int(int_to_real(16777217))"]),
    ("real_div_mixed", "real", "declare ~x := 7.0", ["real_to_int(~x / 2.0)", "real_to_int(~x / 2.0 * 2.0)"]),
    ("real_add_exact", "real", "", ["real_to_int(2.5 + 2.5)"]),
    # --- typing: a real assigned to an integer variable (not wrapped) -----------
    ("type_real_to_int_assign", "typing", "", ["round(2.5)", "7.0 / 2"]),
    # --- shifts ----------------------------------------------------------------------
    ("shift_left_31", "shift", "", ["sh_left(1, 31)", "sh_left(3, 30)"]),
    ("shift_right_negative", "shift", "declare $a := -8", ["sh_right($a, 1)", "sh_right($a, 31)"]),
    ("shift_right_positive", "shift", "declare $a := 1000", ["sh_right($a, 3)", "sh_right($a, 0)"]),
    # --- logic and comparison: conditions only (`if`), results as flags ---------
    ("logic_not", "logic", "declare $a := 0\n  declare $b := 0\n  if (not 0)\n    $a := 1\n  end if\n  if (not 5)\n    $b := 1\n  end if", ["$a", "$b"]),
    ("logic_and_or", "logic", "declare $a := 0\n  declare $b := 0\n  if (5 and 3)\n    $a := 1\n  end if\n  if (0 or 2)\n    $b := 1\n  end if", ["$a", "$b"]),
    ("logic_compare", "logic", "declare $a := 3\n  declare $x := 0\n  declare $y := 0\n  if ($a > 2)\n    $x := 1\n  end if\n  if ($a < 2)\n    $y := 1\n  end if", ["$x", "$y"]),
    ("logic_in_range", "logic", "declare $a := 0\n  declare $b := 0\n  if (in_range(5, 1, 10))\n    $a := 1\n  end if\n  if (in_range(11, 1, 10))\n    $b := 1\n  end if", ["$a", "$b"]),
    # --- inc / dec ---------------------------------------------------------------
    ("incdec_sequence", "incdec", "declare $a := 5\n  inc($a)\n  inc($a)\n  dec($a)", ["$a"]),
    ("incdec_wrap", "incdec", "declare $a := 2147483647\n  inc($a)", ["$a"]),
    # --- arrays -------------------------------------------------------------------
    ("array_store_load", "array", "declare %a[4]\n  %a[2] := 7\n  %a[3] := %a[2] * 3", ["%a[3]", "%a[0]"]),
    ("array_num_elements", "array", "declare %a[9]", ["num_elements(%a)"]),
    ("array_init_list", "array", "declare %a[3] := (10, 20, 30)", ["%a[1]", "%a[2] - %a[0]"]),
    ("array_out_of_bounds_read", "array", "declare %a[3] := (1, 2, 3)", ["%a[5]"]),
    ("array_out_of_bounds_write", "array", "declare %a[3] := (1, 2, 3)\n  %a[7] := 9", ["%a[2]"]),
    # --- while loops --------------------------------------------------------------
    ("loop_count", "loop", "declare $i := 0\n  declare $s := 0\n  while ($i < 10)\n    $s := $s + $i\n    inc($i)\n  end while", ["$s", "$i"]),
    ("loop_nested", "loop", "declare $i := 0\n  declare $j := 0\n  declare $s := 0\n  while ($i < 4)\n    $j := 0\n    while ($j < 3)\n      inc($s)\n      inc($j)\n    end while\n    inc($i)\n  end while", ["$s"]),
    # --- user functions (procedures: called with `call`, globals declared in on init) ---
    ("function_updates_global", "function", "declare $g := 1\n  call bump()\n  call bump()", ["$g"], "function bump()\n  $g := $g * 3\nend function"),
    ("function_nested_call", "function", "declare $g := 2\n  call outer()", ["$g"], "function inner()\n  $g := $g + 10\nend function\nfunction outer()\n  call inner()\n  $g := $g * 2\nend function"),
    # --- boolean operators on variables (flags set in conditions) ----------------
    ("logic_vars", "logic", "declare $t := 1\n  declare $f := 0\n  declare $a := 0\n  declare $b := 0\n  declare $c := 0\n  declare $d := 0\n  if ($t and $f)\n    $a := 1\n  end if\n  if ($t or $f)\n    $b := 1\n  end if\n  if (not $f)\n    $c := 1\n  end if\n  if (not $t)\n    $d := 1\n  end if", ["$a", "$b"]),
    ("logic_vars_2", "logic", "declare $t := 1\n  declare $f := 0\n  declare $a := 0\n  declare $b := 0\n  if ($t and $t)\n    $a := 1\n  end if\n  if ($f or $f)\n    $b := 1\n  end if", ["$a", "$b"]),
    # --- condition truth, typing and call probes (slice 2) -----------------------
    ("cond_bare_int", "logic", "declare $a := 0\n  if (1)\n    $a := 1\n  end if", ["$a"]),
    ("cond_int_and", "logic", "declare $a := 0\n  if (1 and 1)\n    $a := 1\n  end if", ["$a"]),
    ("cond_not_compare", "logic", "declare $t := 1\n  declare $f := 0\n  declare $a := 0\n  declare $b := 0\n  if (not ($f = 1))\n    $a := 1\n  end if\n  if (not ($t = 1))\n    $b := 1\n  end if", ["$a", "$b"]),
    ("cond_compare_ops", "logic", "declare $t := 1\n  declare $a := 0\n  declare $b := 0\n  if ($t # 0)\n    $a := 1\n  end if\n  if ($t > 0)\n    $b := 1\n  end if", ["$a", "$b"]),
    ("typing_bool_assign", "typing", "declare $t := 1\n  declare $x := 5\n  $x := ($t = 1)", ["$x"]),
    ("typing_real_assign", "typing", "declare $x := 5\n  $x := 7.5 / 2", ["$x"]),
    ("call_init_empty", "function", "call f", ["0"], "function f\nend function"),
    ("call_init_global", "function", "call f", ["$g"], "declare $g := 1\nfunction f\n  $g := 7\nend function"),
    # --- slice 3: numeric builtins and edge values --------------------------------
    ("builtin_sgn", "builtin", "", ["sgn(-5)", "sgn(0)", "sgn(7)"]),
    ("builtin_minmax", "builtin", "", ["min(-3, 2)", "max(-3, 2)", "min(5, 5)"]),
    ("builtin_msb_lsb", "builtin", "", ["msb(1024)", "lsb(40)"]),
    ("builtin_msb_zero", "builtin", "", ["msb(0)", "lsb(0)"]),
    ("builtin_sh_32", "builtin", "", ["sh_left(1, 32)", "sh_right(-1, 40)"]),
    ("builtin_sh_negative", "builtin", "", ["sh_left(1, -1)", "sh_right(8, -2)"]),
    ("builtin_pow", "builtin", "", ["real_to_int(pow(2.0, 10.0))", "real_to_int(pow(2.0, -1.0) * 8.0)"]),
    ("builtin_floor_ceil", "builtin", "", ["real_to_int(floor(-2.5))", "real_to_int(ceil(-2.5))", "real_to_int(ceil(2.1))"]),
    ("builtin_round_neg", "builtin", "", ["real_to_int(round(-0.5))", "real_to_int(round(-1.5))"]),
    ("builtin_int_conv", "builtin", "", ["int(3.9)", "int(-3.9)"]),
    ("builtin_sqrt", "builtin", "", ["real_to_int(sqrt(16.0))", "real_to_int(sqrt(2.0) * sqrt(2.0) * 1000.0)"]),
    ("builtin_real_literal", "real", "", ["real_to_int(1.5e3)", "real_to_int(0.1 * 10.0)"]),
    ("arith_div_zero", "arithmetic", "declare $z := 0", ["7 / $z"]),
    ("arith_mod_zero", "arithmetic", "declare $z := 0", ["7 mod $z"]),
    ("arith_unary_prec", "arithmetic", "", ["-2 * -3", "2 - -3", "-2 * 3 + 1"]),
    ("arith_compound_neg", "arithmetic", "declare $a := 5", ["$a - (-$a)"]),
    ("lit_int_max", "arithmetic", "declare $a := 2147483647", ["$a"]),
    ("lit_int_overflow", "arithmetic", "declare $a := 2147483648", ["$a"]),
    ("intval_and_value", "intvalue", "", ["6 and 3"]),
    ("intval_or_value", "intvalue", "", ["6 or 0"]),
    ("intval_bitand", "intvalue", "", ["6 .and. 3", "6 .or. 3"]),
    ("intval_not_value", "intvalue", "", ["not 0"]),
    ("typing_bool_arith", "typing", "", ["(3 > 2) + 0"]),
    # --- slice 3b: min/max forms and integer literal range -------------------------
    ("builtin_min_lit", "builtin", "", ["min(3, 2)"]),
    ("builtin_max_lit", "builtin", "", ["max(3, 2)"]),
    ("builtin_min_var", "builtin", "declare $a := 3", ["min($a, 2)"]),
    ("builtin_max_neg", "builtin", "", ["max(-3, 2)"]),
    ("lit_u32_max", "arithmetic", "declare $a := 4294967295", ["$a"]),
    ("lit_u32_over", "arithmetic", "declare $a := 4294967296", ["$a"]),
    ("lit_3e9", "arithmetic", "declare $a := 3000000000", ["$a"]),
    ("lit_hex_top", "arithmetic", "declare $a := 80000000h", ["$a"]),
    ("lit_neg_min", "arithmetic", "declare $a := -2147483648", ["$a"]),
    ("lit_neg_over", "arithmetic", "declare $a := -2147483649", ["$a"]),
    # --- slice 3c: builtins returning booleans used as numbers ----------------------
    ("bool_inrange_arith", "typing", "", ["in_range(5, 1, 5) + 0"]),
    ("bool_pgs_arith", "typing", "pgs_create_key(TEST, 4)", ["pgs_key_exists(TEST) + 0"]),
    # --- slice 4: integer xor / bitwise not, boolean xor ---------------------------
    ("int_xor_value", "bitops", "", ["6 xor 3"]),
    ("int_bitnot_dot", "bitops", "", [".not. 5", ".not. 0"]),
    ("bool_xor_cond", "logic", "declare $a := 0\n  declare $b := 0\n  if ((1 = 1) xor (2 = 3))\n    $a := 1\n  end if\n  if ((1 = 1) xor (2 = 2))\n    $b := 1\n  end if", ["$a", "$b"]),
    ("int_xor_cond", "logic", "declare $a := 0\n  if (1 xor 0)\n    $a := 1\n  end if", ["$a"]),
    # --- slice 4: array search and sort at init --------------------------------------
    ("array_search_value", "array2", "declare %a[5] := (4, 9, 2, 9, 1)", ["search(%a, 9, 0, 4)", "search(%a, 9, 2, 4)", "search(%a, 7, 0, 4)"]),
    ("array_search_short", "array2", "declare %a[5] := (4, 9, 2, 9, 1)", ["search(%a, 9)"]),
    ("array_sort_asc", "array2", "declare %a[4] := (3, 1, 4, 2)\n  sort(%a, 0, 3)", ["%a[0]", "%a[1]", "%a[2]", "%a[3]"]),
    # --- slice 4: string comparison and concatenation at init ----------------------
    ("string_eq_literal", "string", "declare @s := \"abc\"\n  declare $e := 0\n  if (@s = \"abc\")\n    $e := 1\n  end if", ["$e"]),
    ("string_ne_literal", "string", "declare @s := \"abc\"\n  declare $e := 0\n  if (@s # \"abd\")\n    $e := 1\n  end if", ["$e"]),
    ("string_concat_eq", "string", "declare @s := \"ab\"\n  declare @t := \"c\"\n  declare $e := 0\n  if (@s & @t = \"abc\")\n    $e := 1\n  end if", ["$e"]),
    ("string_case_sensitive", "string", "declare $e := 0\n  if (\"ABC\" = \"abc\")\n    $e := 1\n  end if", ["$e"]),
    # --- slice 4b: sort arity and string forms (probe which Kontakt accepts) --------
    ("sort_asc_two", "array2", "declare %a[4] := (3, 1, 4, 2)\n  sort(%a, 0)", ["%a[0]", "%a[1]", "%a[2]", "%a[3]"]),
    ("sort_desc_two", "array2", "declare %a[4] := (3, 1, 4, 2)\n  sort(%a, 1)", ["%a[0]", "%a[3]"]),
    ("str_decl_init", "string", "declare @s := \"abc\"", ["0"]),
    ("str_assign_decl", "string", "declare @s\n  @s := \"abc\"", ["0"]),
    ("str_lit_cond", "string", "declare $e := 0\n  if (\"abc\" = \"abc\")\n    $e := 1\n  end if", ["$e"]),
    ("str_var_cond", "string", "declare @s := \"abc\"\n  declare $e := 0\n  if (@s = \"abc\")\n    $e := 1\n  end if", ["$e"]),
    ("str_var_ne_cond", "string", "declare @s := \"abc\"\n  declare $e := 0\n  if (@s # \"abd\")\n    $e := 1\n  end if", ["$e"]),
    # --- control readback at init (UI widgets created in on init) -----------------
    ("ctl_knob_default", "control", "declare ui_knob $k(0,127,1)", ["get_control_par($k, $CONTROL_PAR_VALUE, -1)", "get_control_par($k, $CONTROL_PAR_DEFAULT_VALUE, -1)"]),
    ("ctl_knob_set_high", "control", "declare ui_knob $k(0,127,1)\n  set_control_par($k, $CONTROL_PAR_VALUE, 150)", ["get_control_par($k, $CONTROL_PAR_VALUE, -1)"]),
    ("ctl_knob_set_neg", "control", "declare ui_knob $k(0,127,1)\n  set_control_par($k, $CONTROL_PAR_VALUE, -5)", ["get_control_par($k, $CONTROL_PAR_VALUE, -1)"]),
    ("ctl_switch_set", "control", "declare ui_switch $s\n  set_control_par($s, $CONTROL_PAR_VALUE, 1)", ["get_control_par($s, $CONTROL_PAR_VALUE, -1)"]),
    ("ctl_hide_set", "control", "declare ui_knob $k(0,127,1)\n  set_control_par($k, $CONTROL_PAR_HIDE, 1)", ["get_control_par($k, $CONTROL_PAR_HIDE, -1)"]),
    ("ctl_pos_x_set", "control", "declare ui_knob $k(0,127,1)\n  set_control_par($k, $CONTROL_PAR_POS_X, 33)", ["get_control_par($k, $CONTROL_PAR_POS_X, -1)"]),
    ("ctl_menu_items", "control", "declare ui_menu $m\n  add_menu_item($m, \"a\", 5)\n  add_menu_item($m, \"b\", 7)", ["get_num_menu_items($m)", "get_menu_item_value($m, 1)"]),
    ("diag_decl_switch", "control", "declare ui_switch $s", ["0"]),
    ("diag_set_switch", "control", "declare ui_switch $s\n  set_control_par($s, $CONTROL_PAR_VALUE, 1)", ["0"]),
    ("diag_get_switch", "control", "declare ui_switch $s", ["get_control_par($s, $CONTROL_PAR_VALUE, -1)"]),
    ("diag_get_int", "control", "declare $x := 3", ["get_control_par(0, $CONTROL_PAR_VALUE, -1)"]),
    ("ui_var_knob_default", "control", "declare ui_knob $k(0,127,1)", ["$k"]),
    ("ui_var_knob_set_high", "control", "declare ui_knob $k(0,127,1)\n  set_control_par($k, $CONTROL_PAR_VALUE, 150)", ["$k"]),
    ("ui_var_knob_set_neg", "control", "declare ui_knob $k(0,127,1)\n  set_control_par($k, $CONTROL_PAR_VALUE, -5)", ["$k"]),
    ("ui_var_switch_set", "control", "declare ui_switch $s\n  set_control_par($s, $CONTROL_PAR_VALUE, 1)", ["$s"]),
    ("diag_get_index0", "control", "declare ui_switch $s", ["get_control_par($s, $CONTROL_PAR_VALUE, 0)"]),
    ("diag_menu_add", "control", "declare ui_menu $m\n  add_menu_item($m, \"a\", 5)", ["0"]),
    ("diag_menu_count", "control", "declare ui_menu $m", ["get_num_menu_items($m)"]),
    ("diag_menu_count_after_add", "control", "declare ui_menu $m\n  add_menu_item($m, \"a\", 5)", ["get_num_menu_items($m)"]),
    # --- manual commands (docs/ksp/MANUAL-COVERAGE.md): values KONTRA guesses until the oracle runs ---
    ("manual_voice_limit_default", "manual", "", ["get_voice_limit($NI_VL_TMPRO_STANDARD)", "get_voice_limit($NI_VL_TMPRO_HQ)"]),
    ("manual_voice_limit_set", "manual", "set_voice_limit($NI_VL_TMPRO_STANDARD, 11)", ["get_voice_limit($NI_VL_TMPRO_STANDARD)"]),
    ("manual_keyrange_inside", "manual", "set_keyrange(36, 72, \"R\")", ["get_keyrange_min_note(50)", "get_keyrange_max_note(50)"]),
    ("manual_keyrange_outside", "manual", "set_keyrange(36, 72, \"R\")", ["get_keyrange_min_note(20)", "get_keyrange_max_note(20)"]),
    ("manual_keyrange_removed", "manual", "set_keyrange(36, 72, \"R\")\n  remove_keyrange(50)", ["get_keyrange_min_note(50)", "get_keyrange_max_note(40)"]),
    ("manual_detect_constants", "manual", "", ["$NI_DETECT_KEY_INVALID", "$NI_DETECT_SAMPLE_TYPE_DRUM", "$NI_DETECT_DRUM_TYPE_KICK", "$NI_DETECT_INSTRUMENT_TYPE_BASS"]),
    # --- slice 6: which part of the init-call units Kontakt rejects (2026-11-25) ----
    ("fn_empty_uncalled", "function", "", ["0"], "function f\nend function"),
    ("fn_before_init_later_var", "function", "declare $g := 1", ["$g"], "function f\n  $g := 7\nend function"),
    ("call_init_nonempty", "function", "declare $g := 1\n  call f", ["$g"], "", "function f\n  $g := 7\nend function"),
    ("call_init_before_body", "function", "call f", ["0"], "function f\n  message(\"\")\nend function"),
    ("call_init_before_paren", "function", "call f()", ["0"], "function f()\n  message(\"\")\nend function"),
    ("call_note_after", "function", "declare $g := 1", ["$g"], "", "function f\n  $g := 7\nend function\non note\n  call f\nend on"),
    # --- slice 6: arity and place of the slice-5 rejections ----------------------
    ("ctl_get_two_args", "control", "declare ui_knob $k(0,127,1)\n  set_control_par(get_ui_id($k), $CONTROL_PAR_VALUE, 9)", ["get_control_par(get_ui_id($k), $CONTROL_PAR_VALUE)"]),
    ("ctl_menu_count_note", "control", "declare ui_menu $m\n  add_menu_item($m, \"a\", 5)", ["0"], "", "on note\n  message(get_num_menu_items($m))\nend on"),
    ("str_assign_cond", "string", "declare @s\n  @s := \"abc\"\n  declare $e := 0\n  if (@s = \"abc\")\n    $e := 1\n  end if", ["$e"]),
    ("str_cond_note", "string", "declare @s\n  declare $e := 0", ["0"], "", "on note\n  if (@s = \"abc\")\n    $e := 1\n  end if\nend on"),
    ("minmax_note", "builtin", "declare $x := 0", ["0"], "", "on note\n  $x := min(3, 2)\nend on"),
    # --- slice 7: manual commands no conformance unit named yet (2026-11-25) ------
    ("cmd_array_equal", "manual", "declare %a[3] := (1, 2, 3)\n  declare %b[3] := (1, 2, 3)\n  declare %c[3] := (1, 2, 4)\n  declare $x := 0\n  declare $y := 0\n  if (array_equal(%a, %b))\n    $x := 1\n  end if\n  if (array_equal(%a, %c))\n    $y := 1\n  end if", ["$x", "$y"]),
    ("cmd_purge_state", "manual", "", ["get_purge_state(0)", "get_purge_state(3)"]),
    ("cmd_key_type", "manual", "set_key_type(60, $NI_KEY_TYPE_CONTROL)", ["get_key_type(60)", "get_key_type(61)"]),
    ("cmd_key_triggerstate", "manual", "set_key_pressed_support(1)\n  set_key_pressed(60, 1)", ["get_key_triggerstate(60)", "get_key_triggerstate(61)"]),
    ("cmd_group_idx", "manual", "", ["get_group_idx(\"no such group\")"]),
    ("cmd_watch_init", "manual", "declare $w\n  declare %wa[3]\n  watch_var($w)\n  watch_array_idx(%wa, 1)", ["0"]),
    ("cmd_watch_note", "manual", "declare $w", ["0"], "", "on note\n  watch_var($w)\nend on"),
    ("cmd_wait_ticks_note", "manual", "", ["0"], "", "on note\n  wait_ticks(10)\nend on"),
    ("cmd_map_color_init", "manual", "set_map_editor_event_color(000FF00h)", ["0"]),
    ("cmd_map_color_note", "manual", "", ["0"], "", "on note\n  set_map_editor_event_color(000FF00h)\nend on"),
    ("cmd_ui_height", "manual", "set_ui_height(4)", ["0"]),
    ("cmd_mf_nav", "manual", "declare $a\n  declare $b\n  declare $l\n  declare $p\n  mf_set_buffer_size(4)\n  $a := mf_insert_event(0, 10, $MIDI_COMMAND_NOTE_ON, 60, 100)\n  $b := mf_insert_event(0, 30, $MIDI_COMMAND_NOTE_ON, 62, 100)\n  mf_get_last(0)\n  $l := mf_get_id()\n  mf_get_prev(0)\n  $p := mf_get_id()", ["$l - $b", "$p - $a"]),
    ("cmd_mf_mark", "manual", "declare $a\n  declare $b\n  mf_set_buffer_size(4)\n  $a := mf_insert_event(0, 10, $MIDI_COMMAND_NOTE_ON, 60, 100)\n  $b := mf_insert_event(0, 30, $MIDI_COMMAND_NOTE_ON, 62, 100)\n  mf_set_mark($a, $MARK_1, 1)", ["mf_get_mark($a, $MARK_1)", "mf_get_mark($b, $MARK_1)"]),
    ("cmd_mf_export", "manual", "mf_set_num_export_areas(4)\n  mf_set_export_area(\"x\", -1, -1, -1, -1)\n  mf_copy_export_area(1)", ["0"]),
    ("cmd_mf_reset", "manual", "declare $a\n  mf_set_buffer_size(4)\n  $a := mf_insert_event(0, 10, $MIDI_COMMAND_NOTE_ON, 60, 100)\n  mf_reset()", ["mf_get_buffer_size()", "mf_get_num_tracks()"]),
    # --- slice 8: where the manual's "only in on init" commands compile (2026-11-25) ---
] + [
    (f"place_{name}_{where}", "place", decl + ("\n  " + stmt if where == "init" else ""), ["0"], "",
     "" if where == "init" else f"on note\n  {stmt}\nend on")
    for name, decl, stmt in [
        ("read_persistent_var", "declare $w", "read_persistent_var($w)"),
        ("watch_array_idx", "declare %wa[3]", "watch_array_idx(%wa, 1)"),
        ("disable_logging", "", "disable_logging($NI_LOG_MESSAGE)"),
        ("set_listener", "", "set_listener($NI_SIGNAL_TIMER_MS, 100000)"),
        ("expose_controls", "", "expose_controls"),
        ("get_font_id", "declare $f", "$f := get_font_id(\"none.png\")"),
        ("load_performance_view", "", "load_performance_view(\"none\")"),
        ("make_perfview", "", "make_perfview"),
        ("set_ui_height_px", "", "set_ui_height_px(300)"),
        ("set_ui_width_px", "", "set_ui_width_px(700)"),
        ("load_komplete_ui", "", "load_komplete_ui(\"none\")"),
        ("find_zone", "declare $z", "$z := find_zone(\"none\")"),
        ("add_menu_item", "declare ui_menu $m", "add_menu_item($m, \"b\", 2)"),
        ("set_controller", "", "set_controller(1, 64)"),
        ("set_poly_at", "", "set_poly_at(60, 10)"),
        ("set_note_controller", "", "set_note_controller(1, 60, 10)"),
    ]
    for where in ("init", "note")
] + [
    # The same rule in on persistence_changed (where a snapshot's values land).
    (f"place_{name}_pc", "place", decl, ["0"], "", f"on persistence_changed\n  {stmt}\nend on")
    for name, decl, stmt in [
        ("read_persistent_var", "declare $w", "read_persistent_var($w)"),
        ("watch_var", "declare $w", "watch_var($w)"),
        ("add_menu_item", "declare ui_menu $m", "add_menu_item($m, \"b\", 2)"),
        ("find_zone", "declare $z", "$z := find_zone(\"none\")"),
        ("set_listener", "", "set_listener($NI_SIGNAL_TIMER_MS, 100000)"),
        ("set_ui_height_px", "", "set_ui_height_px(300)"),
    ]
] + [
    ("place_set_ui_height_note", "place", "", ["0"], "", "on note\n  set_ui_height(4)\nend on"),
    ("place_read_persistent_var_ui", "place", "declare ui_knob $k(0, 10, 1)\n  declare $w", ["0"], "", "on ui_control($k)\n  read_persistent_var($w)\nend on"),
] + [
    # --- slice 8: declaration order outside functions ---------------------------
    ("order_note_before_init", "order", "declare $g", ["$g"], "on note\n  $g := 2\nend on"),
    ("order_fn_calls_later_fn", "order", "declare $g := 1", ["$g"], "", "function a\n  call b\nend function\nfunction b\n  $g := 2\nend function\non note\n  call a\nend on"),
    ("order_bare_name_call", "order", "declare $g := 1", ["$g"], "", "function b\n  $g := 2\nend function\non note\n  b\nend on"),
    # --- slice 9: declaration order inside init, real constants -------------------------
    ("order_assign_before_declare", "order", "$late := 1\n  declare $late := 2", ["$late"]),
    ("order_read_before_declare", "order", "declare $a := $late\n  declare $late := 2", ["$a"]),
    ("real_const_math", "real", "", ["real_to_int(~NI_MATH_PI * 1000000.0)", "real_to_int(~NI_MATH_E * 1000000.0)"]),
    ("real_const_detect_pitch_tempo", "real", "", ["real_to_int(~NI_DETECT_PITCH_INVALID * 1000.0)", "real_to_int(~NI_DETECT_TEMPO_INVALID * 1000.0)"]),
    ("real_const_detect_levels", "real", "", ["real_to_int(~NI_DETECT_LOUDNESS_INVALID)", "real_to_int(~NI_DETECT_PEAK_INVALID)"]),
    ("real_const_detect_rms", "real", "", ["real_to_int(~NI_DETECT_RMS_INVALID)"]),
    # --- slice 9: built-in variables read in init (66 empty groups, transport stopped) --
    ("init_num_zones", "builtin", "", ["$NUM_ZONES", "get_num_zones()"]),
    ("init_groups_selected", "builtin", "", ["%GROUPS_SELECTED[0]", "%GROUPS_SELECTED[1]"]),
    ("init_groups_affected", "builtin", "", ["num_elements(%GROUPS_AFFECTED)", "$REF_GROUP_IDX"]),
    ("init_played_voices", "builtin", "", ["$PLAYED_VOICES_INST", "$PLAYED_VOICES_TOTAL"]),
    ("init_duration_quarter", "builtin", "", ["$DURATION_QUARTER", "$DURATION_EIGHTH"]),
    ("init_duration_sixteenth", "builtin", "", ["$DURATION_SIXTEENTH", "$DURATION_QUARTER_TRIPLET"]),
    ("init_duration_triplets", "builtin", "", ["$DURATION_EIGHTH_TRIPLET", "$DURATION_SIXTEENTH_TRIPLET"]),
    ("init_distance_bar", "builtin", "", ["$DISTANCE_BAR_START", "%KEY_DOWN_OCT[0]"]),
    ("init_rpn", "builtin", "", ["$RPN_ADDRESS", "$RPN_VALUE"]),
    # Each name Kontakt rejected above, alone in init and read in `on note` / `on rpn`.
    ("init_groups_affected_only", "builtin", "", ["num_elements(%GROUPS_AFFECTED)"]),
    ("init_ref_group_only", "builtin", "", ["$REF_GROUP_IDX"]),
    ("init_distance_bar_only", "builtin", "", ["$DISTANCE_BAR_START"]),
    ("init_key_down_oct_only", "builtin", "", ["%KEY_DOWN_OCT[0]"]),
    ("init_rpn_address_only", "builtin", "", ["$RPN_ADDRESS"]),
    ("init_rpn_value_only", "builtin", "", ["$RPN_VALUE"]),
    ("note_groups_affected", "builtin", "declare $x", ["0"], "", "on note\n  $x := num_elements(%GROUPS_AFFECTED)\n  $x := $REF_GROUP_IDX\nend on"),
    ("note_distance_bar", "builtin", "declare $x", ["0"], "", "on note\n  $x := $DISTANCE_BAR_START\n  $x := %KEY_DOWN_OCT[0]\nend on"),
    ("rpn_reads_rpn", "builtin", "declare $x", ["0"], "", "on rpn\n  $x := $RPN_ADDRESS + $RPN_VALUE\nend on\non nrpn\n  $x := $RPN_ADDRESS + $RPN_VALUE\nend on"),
    ("const_cb_type_async_out", "builtin", "", ["$NI_CB_TYPE_ASYNC_OUT"]),
    ("init_callback_type", "builtin", "", ["$NI_CALLBACK_TYPE"]),
    ("init_saves_array_str", "builtin", "declare %a[2]\n  save_array_str(%a, \"/tmp/kontra-unit.nka\")", ["0"]),
    ("note_reads_rpn", "builtin", "declare $x", ["0"], "", "on note\n  $x := $RPN_ADDRESS + $RPN_VALUE\nend on"),
]

# --- slice 6: every measured constant (crates/script/ksp/data/constants.tsv) ------
# Bit b of a result is set when the name has the value the table gives it, so a unit
# reads -1 where Kontakt and KONTRA agree on all 32 names. One unknown name fails the unit.
TSV = pathlib.Path(__file__).resolve().parent / "../../crates/script/ksp/data/constants.tsv"
CONSTANTS = [l.split("\t") for l in TSV.read_text().splitlines() if l and l[0] != "#"]


def _constant_unit(n, rows):
    setup = ["declare $r0 := 0", "declare $r1 := 0"]
    for k, (name, value) in enumerate(rows):
        setup += [f"if ({name} = {int(value)})", f"  $r{k // 32} := $r{k // 32} .or. sh_left(1, {k % 32})", "end if"]
    return (f"const_{n:02d}", "constant", "\n  ".join(setup), ["$r0", "$r1"][: (len(rows) + 31) // 32])


CASES += [_constant_unit(n, CONSTANTS[i:i + 64]) for n, i in enumerate(range(0, len(CONSTANTS), 64))]
