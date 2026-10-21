# KSP engine parameters (set/get_engine_par, enumerations, display)

Owner: `k-ksp-engine-params`. Scope: the script-facing engine-parameter surface
(`set_engine_par`, `get_engine_par`, `get_engine_par_disp`, and the `_m`
variants) as Kontakt 8 exposes it, against `crates/sampler-ksp` and
`crates/sampler-core/src/engine_parameter*.rs`.

## Evidence used

- Kontakt 8 decompile, `source/kontakt8/0140780000.c` (READY). The builtin
  error strings in that bucket name every entry point and its failure cases
  (lines ~3145-3710 for get/_disp/_m, ~5738-5794 for set).
- Repo table `crates/sampler-core/src/engine_parameter_names.rs` (692 names,
  index = engine parameter id; first listed index wins, see
  `engine_parameter_id`).
- No native ids are confirmed by the decompile: only `ENGINE_PAR_PAN` and
  `ENGINE_PAR_VOLUME` appear as strings in the whole decompile. Ids therefore
  remain `none` evidence until the Wine oracle dumps them.

## Builtins (native names and call counts in the decompile)

| Builtin | Native mentions | Repo (`builtins.rs`) | Status |
| --- | --- | --- | --- |
| `set_engine_par` | 33 | `[I I I I I]` `SetEnginePar` | present, approximate |
| `get_engine_par` | 4 | `[I I I I]` `GetEnginePar` | present, approximate |
| `get_engine_par_disp` | 2 | `[I I I I]` `GetEngineParDisp` | present, approximate |
| `set_engine_par_m` | 10 | — | **missing** |
| `get_engine_par_m` | 7 | — | **missing** |
| `get_engine_par_disp_m` | 5 | — | **missing** |
| `get_engine_par_disp_ext` | 0 | `[I I I I I]` `GetEngineParDispExt` | **unproven**: no native string; keep only if a public-doc signature confirms it |

## Native semantics (from the error paths)

- `set_engine_par`: fails with "cannot be used with this engine parameter",
  "Engine parameter is not supported", and "Invalid group index". Convolution
  parameters are always asynchronous ("Asynchronous mode is always used for
  Convolution engine parameters").
- `get_engine_par`: same classes of failure, plus the `_disp` variant rejects
  parameters that have no display conversion.
- `_m` variants address a **slot**: `get_engine_par_m` rejects slot index
  outside 0..63 ("Slot index must be between 0 and 63") and inactive slots
  ("Slot is not active"). A negative slot takes the instrument-level path
  (native checks special-case ids 0x294 and 0x296 there).
- `get_engine_par_disp_m` additionally addresses an **output channel**
  ("Output channel index must be between 0 and %d", "Output channel is not
  active"). Output-channel count comes from the active output list.
- The `_m` dispatch maps the script id through a native table
  (`DAT_14a5ec800`) to an internal id. Special cases key on internal ids
  0x294..0x2a5 and 0x2de; the mapping from those to the repo's script names
  is not yet confirmed (see above).

## Display conversions

`engine_parameter_display` (repo) renders a bare number with no unit, because
scripts append the unit themselves; `-inf` for `ENGINE_PAR_SUSTAIN` at 0 and
`ENGINE_PAR_VOLUME` at 0. The decompile does not confirm these strings; the
values stay repo-evidence only until the oracle is run.

## Gap table

| Item | Shared engine has | Missing | Evidence |
| --- | --- | --- | --- |
| Engine-parameter id table (692 names) | yes | native id confirmation | none (decompile has 2 strings) |
| `set_engine_par` / `get_engine_par` | yes, approximate | exact error reporting (fault text is not stored; `EngineParameterOutcome` is silent) | decomp error paths |
| `get_engine_par_disp` | yes | native conversion check | repo only |
| `set_engine_par_m`, `get_engine_par_m`, `get_engine_par_disp_m` | no | slot and output-channel addressing, argument order | decomp (names, slot range, error paths); arg order unconfirmed |
| Unsupported-parameter diagnosis | build-time warning for a catalog `ENGINE_PAR_*` with no engine id, and for a constant group index outside `-1..groups` (`engine_par_rejection`, `tests/engine_par_unsupported.rs`); write dropped, read 0 | runtime fault with native text; `on init` bodies are evaluated (no warning yet); `get_engine_par_disp` not checked | decomp error strings |
| `get_engine_par_disp_ext` | builtin exists | confirm it is native | not in decompile |

## Next

1. Confirm the `_m` argument order: follow `FUN_140747bc0` / `FUN_14098d680`
   in `0140780000.c` around the get_engine_par_m dispatch (~line 3588).
2. Add the three `_m` builtins behind the existing slot/outcome path; keep the
   code additive.
3. Surface the same diagnostic for `get_engine_par_disp` and for `on init`
   evaluation (eval.rs), and attach native fault text to `EngineParameterOutcome`.
4. Confirm ids and display strings with a Wine-oracle dump, then move the row
   to `parity`.
