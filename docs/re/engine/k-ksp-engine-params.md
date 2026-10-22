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

## Blockers found in the READY decompile (2026-10-22)

- KSP builtin names (`set_engine_par_m`, even `set_key_color`) and ENGINE_PAR
  name strings are not literals in `source/kontakt8`. The `_m` argument order
  and the name-to-id table cannot be read from the decompile text. They are
  in the binary's string/builtin tables; they need the Ghidra project
  (`ghidra/Kontakt8.gpr`, xrefs) or the Wine oracle.
- The builder at `0140780000.c:1` (`FUN_140780c80`) fills
  `DAT_14a5ec800`, a 1024-entry int table (memcpy of a 0x1000-byte local). It
  is indexed by the numeric engine-parameter value, not by repo id. Its
  contents are compiled-in values, not names.
- Catalog gap: 48 names in `vendor_constants.txt` have no engine id. Two are
  single names (`ENGINE_PAR_WT_MOD_AMOUNT`, `ENGINE_PAR_BASSPRO_GEQ_#K`); the
  rest are numbered families (`RV#_*`, `SF#_*`, `V#_*`, `GAIN#`, `FREQ#`,
  `BW#`, `WT_FORM#`). Instance counts are not in the decompile; repo usage
  shows instances such as `GAIN1..3`, `RV2_*`, `V5_*`. Adding ids without a
  DSP consumer would turn the diagnostic into a silent no-op, so the ids
  wait for their DSP.
- `on init` bodies are interpreted (`eval.rs`), not lowered, so the
  build-time diagnostic does not reach them. Needs a context-independent HIR
  pass (none exists yet).

## Native evidence from the PE (2026-10-22)

- `$ENGINE_PAR_*` names are in `.rdata`; 708 are registered by code. Each
  registration is `mov dword [rbp+X], <value>` then `lea r8,[rbp+X]`,
  `lea rdx,[name]`, `call FUN_140775ef0`. The value is the script constant
  (`$ENGINE_PAR_VOLUME` = 422, `$ENGINE_PAR_PAN` = 423). Table:
  `crates/sampler-core/src/engine_par_ids.tsv` (the single source; `build.rs` generates the id table from it, and rows with `-` stay unbound).
- 690 of the 708 native names have a repo id. The 18 without one are listed
  in that TSV with `in_repo_id_table=no`. None has an engine consumer yet,
  so no id is added (an id with no consumer would hide the diagnostic).
- Builtin names (`set_engine_par_m` and the rest) are not stored as text in
  the PE, so the builtin table is not recoverable from strings.
- `set_engine_par_m` builds a 5-argument node (type 0x3b) and
  `get_engine_par_m` a 4-argument node (type 0x3c), the same arities as
  `set_engine_par` / `get_engine_par`. Both report
  `can only be used in a multi script!` at compile time. This repo has no
  multi-script concept, so both are compile errors here.
- `get_engine_par_disp_m` is not implemented: its arity and multi-script
  message were not confirmed.

## Next

1. Confirm the `_m` argument order: follow `FUN_140747bc0` / `FUN_14098d680`
   in `0140780000.c` around the get_engine_par_m dispatch (~line 3588).
2. Add the three `_m` builtins behind the existing slot/outcome path; keep the
   code additive.
3. Surface the same diagnostic for `get_engine_par_disp` and for `on init`
   evaluation (eval.rs), and attach native fault text to `EngineParameterOutcome`.
4. Confirm ids and display strings with a Wine-oracle dump, then move the row
   to `parity`.

## Status (final lane pass)

- Ids: `crates/sampler-core/src/engine_par_ids.tsv` is the single source (710 rows). 692 bound. The `#`-templates in the catalog all resolve to bound concrete instances; the only native names without an id are the 18 listed in the matrix.
- Internal ids: the builder table at `0x140780c80` was simulated from its frame stores (rbp = rsp + 0x100, array at rsp + 0x20, memset to -1). 335 native constants have an internal id in the TSV (`internal_id`). Reverb special cases: RV_SIZE 0x294, RV_COLOUR 0x295, RV_STEREO 0x296, RV2_DAMPING 0x2a2, RV2_MOD 0x2a3, RDL_NOISE 0x2de.
- `get/set_engine_par_m`: argument arity 4/5 from the statement constructors; multi-script-only compile errors.
- `get_engine_par_disp_m`: args (id, slot-or-output, generic, channel-search flag), from the call site at `0140780000.c:8100`. Slot range 0..63, output range 0..127, "not active" checks. Diagnosed as not modelled.
- Display text: each module's display vtable (`FUN_1408a6400` and others), not one table. Not ported.
- Consumers: none found for the 18 unbound names; each is blocked on a live-control path owned by another lane (Raum live controls, sends live, wavetable/ext-mod live, MIDI filter).
