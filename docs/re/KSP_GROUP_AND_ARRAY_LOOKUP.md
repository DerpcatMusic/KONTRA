# KSP find_group and array bounds: native semantics

Evidence: `Research/KONTRA/source/kontakt8` decompile (Kontakt 8.13.1 Portable). Checked 2026-10-22 against origin/main; the warning text and exact lookup already match native.

## find_group / get_group_idx

- Dispatch: `0140780000.c:7474-7483`. A miss emits `%s(): Group "%s" was not found. Index will be invalid!` (`find_group` or `get_group_idx`) and the index is invalid (-1).
- Lookup: `FUN_14087eae0` (`0140870000.c:10271`). Linear scan of the instrument's group list, comparing each group name byte for byte. Case-sensitive, exact, no trimming. Miss returns `0xFFFFFFFF`.
- KONTRA: `crates/sampler-ksp/src/lower.rs` (constant names) and `eval.rs` (computed names) both do exact matching with the same warning text.

Areia (Pyramid (Tonal and Atonal) script) calls `find_group("10VlnsChnswStbsBr1FMpDyn1RR1")` at line 23623 and `find_group("4CllChnswStbsBr21ClsDyn1RR1")` at line 23658, both as `if (find_group(...)>0)`. Neither name exists in the shipped instrument (checked the 16 Violins NKI: 1122 groups, none containing `Chns`). Kontakt returns -1, the branch is skipped and the warning is printed. KONTRA does the same. The warning is native and is not a KONTRA gap.

## Array index out of range

- `FUN_1407a1230` (`01407a0000.c:652-760`): a read out of range yields the default value 0 (empty text for string arrays). A write out of range has no effect. No auto-resize.
- Message: `SCRIPT WARNING (Line: N, Script: M): Array index out of range ...` and `Array "%s[%d]" is out of range. Index [%d] is invalid. ...`, emitted only while the script-warning flag at instance offset `+0x83b2` is set (set at `0140770000.c:9284`).
- KONTRA: `crates/sampler-ksp/src/eval.rs` (`read_elem`, `write_elem`, `SysElem`) matches the value semantics.

Affected authored scripts (the out-of-range index comes from the script, not the engine):
- Dolce: `%_legato_event_id` declared with `$MAX_NUM_ARTS*$MAX_NUM_POLY_LEGATO_VOICES` (72) entries, indexed up to 76 by the authored `$MAX_NUM_POLY_LEGATO_VOICES*$artcount+$count` loop.
- Afflatus: `!key_art_names` declared with `$num_articulations` (2 in some instruments) and read with fixed indices up to 6 in UI label code.

Consequence: these warnings reproduce native script diagnostics. They are not engine gaps and should be classed as native script warnings, not as unsupported features.
