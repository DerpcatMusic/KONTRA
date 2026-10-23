# KSP conformance against Kontakt 8

Status: slice 1 (2026-10-23). Suite: `tools/ksp-conformance/`. Test: `crates/sampler-ksp/tests/conformance.rs`.

## How a result gets out of Kontakt

Kontakt runs under Wine (`tools/kontakt-oracle/dump.py`, one session under
`flock /tmp/kontra-oracle.lock`). Scripts run their `on init`, so each unit is init-only.

Readback channels tried (`tools/ksp-conformance/probes/`):

- **Instrument volume/pan/tune from `set_engine_par(...)` values.** Not usable. Values 0, 1, -1
  and -1000 read back as -inf, -100 (pan) or -36 (tune), and only 1000 moves the level
  (`readback_calibrate.lua`).
- **Group volume, bit channel. Used.** Result `k` bit `b` is written as
  `set_engine_par($ENGINE_PAR_VOLUME, 1000, 32*k + b, -1, -1)` when the bit is set, and read
  back as `get_group_volume(i, 32*k + b)`. A set bit reads -167.38 dB; a clear bit keeps its
  group default (0 dB for group 0, -6 dB for the others). Group addressing is 0-based
  (`group_bit_calibrate.lua`).

Per unit, `gen.py` wraps the case's results in this encoder (`cases.py: ENCODER`) and emits
the same `.ksp` text for Kontakt (generated probe, 16 units per session) and for the
interpreter (`cases/<id>.ksp`). `golden.json` holds the Kontakt values; the test decodes
the same bits from `init_engine_pars` and compares.

Encoder syntax is KSP: parenthesised conditions, `#` for not-equal, no `then`.
Real variables are `~name`.

Run: `tools/ksp-conformance/run.sh` (oracle, all batches), then
`cargo test -p sampler-ksp --test conformance`.

## Results (39 units from 38 cases)

| Category | Units | Kontakt = interpreter | Mismatch |
|---|---:|---:|---:|
| arithmetic (wrap, truncating `/`, C-style `mod`, precedence, abs) | 11 | 11 | 0 |
| real (`round`, `real_to_int`, `~` vars, exact int->real) | 7 | 7 | 0 |
| array (store/load, `num_elements`, init list, out-of-bounds read/write) | 5 | 5 | 0 |
| shift (`sh_left`, `sh_right` on negatives) | 3 | 3 | 0 |
| loop (`while`, nested) | 2 | 2 | 0 |
| incdec (`inc`, wrap) | 2 | 2 | 0 |
| logic (`not`, `and`/`or`, comparisons, `in_range`) | 6 | 2 | 4 |
| function (`call` of user procedures from `on init`) | 2 | 0 | 2 |
| typing (real assigned to an integer) | 1 | 0 | 1 |
| **Total** | **39** | **32** | **7** |

The seven mismatching units are listed in `KNOWN_MISMATCHES` in the test. Removing an entry
is part of the fix that closes it.

### Mismatches and the Kontakt behaviour observed

1. **Logic is condition-only in Kontakt (4 units: `logic_not`, `logic_and_or`, `logic_vars`,
   `logic_vars_2`).** Measured (`pr_*` probe): a bare variable as a condition is false
   (`if ($t)`), `if (1 and 1)` and `if ($t and 1)` are false, `if (not ...)` is false in
   every form tried, and `if (($t = 1) and ($t = 1))` / `or` are true. Only comparison-based
   terms count as true. The interpreter treats these as numeric truth. Fix in `lower.rs`
   (`ExprKind::Not`, condition lowering) and `sema.rs` typing.
2. **Real assigned to an integer (`type_real_to_int_assign`).** Kontakt compiles
   `$cv := round(2.5)` and `$cv := 7.0 / 2` and yields 0. The interpreter rejects it with
   "expected integer, found real". Fix: accept and match Kontakt's value; confirm the 0 with
   a probe before coding it.
3. **`call` from `on init` (`function_updates_global`, `function_nested_call`).** Kontakt
   reads 0 for the caller's global after `call` in `on init`; the interpreter applies the
   call. Not yet probed: whether Kontakt runs the procedure at all in init, or whether
   `on init` globals are invisible inside procedures. Next probe: a procedure that writes a
   flag only, and a read of the same global from `on init` after the call.

Not in this slice: strings, callbacks (`on note`, `on listener`, `wait`), UI controls,
persistence, polyphonic variables. These need a different readback (note-time and listener
values) and are the next slices.

## Notes

- The test ratchets: a new mismatch fails it; a fixed unit must leave `KNOWN_MISMATCHES`.
- `golden.json` comes from one oracle run on Kontakt 8.13.1. Regenerate after any
  generator or case change.
- Interpreter micro-bench (init and per-note callback cost on corpus scripts): not started.
