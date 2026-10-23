"""KSP conformance cases: small init-time scripts whose integer results Kontakt makes observable.

A case is (id, category, setup, results). `setup` is KSP statements run in `on init`
before the results are computed; `results` are KSP integer expressions over the setup
variables. gen.py wraps every result in a bit encoder (see ENCODER below) so Kontakt and
the KONTRA interpreter both report the 32 bits of each result through
set_engine_par(ENGINE_PAR_VOLUME, 1000, group, -1, -1). Group 0 of a clear bit stays at
its default level; a set bit reads back at about -167 dB (see docs/re/KSP_CONFORMANCE.md).

KSP syntax: conditions are parenthesised, `#` is not-equal, there is no `then`.

Keep every case to init-time statements: callbacks, waits and UI are later slices.
"""

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
]
