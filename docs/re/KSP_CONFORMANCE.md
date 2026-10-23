# KSP conformance against Kontakt 8

Status: slice 1 (2026-10-23). Suite: `tools/ksp-conformance/`. Test: `crates/sampler-ksp/tests/conformance.rs`.

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
`cargo test -p sampler-ksp --test conformance`.

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
