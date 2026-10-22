# Kontakt 8 .rdata extraction (cached once, do not re-open the binary)

Facts pulled straight from the Kontakt 8 PE file (vtables, `std::function` bodies,
constants, switch/jump tables) so RE agents can grep them instead of opening the
binary or Ghidra again. Everything here is data, not code: do not paste it into
the engine, translate the logic yourself.

## Input

- Binary: `/data/dev/workspace/re-binaries/Kontakt8.exe`
  (sha256 `0fe6356e0879d058b6e5b73507c54c5e345cea451b35287c974e438291d4dae8`, ImageBase `0x140000000`, SizeOfImage `0xb254000`)
- Decompile export (used only for function boundaries, DAT/LAB reference lists and literal stores):
  `/data/dev/workspace/Research/KONTRA/source/kontakt8/*.c` (175503 `// ==== <addr> <name> size=N` headers)

Addresses are hex without `0x`, as in the `.c` files. Section names come from the PE.

## Files

| file | rows | what |
|---|---|---|
| `vtables.tsv.gz` | 168921 slot rows, 13220 vtables | one row per MSVC RTTI vtable slot: `class  vtable_va  slot  target_va  target_section  decompiled_size  col_va  this_offset  bases`. Gzipped (31 MB plain) |
| `funcimpl.tsv` | 15764 slot rows, 2518 vtables | every `_Func_impl*` vtable (the `std::function` implementation classes), one row per slot, plus `refs_from_fn` = decompiled functions that install that vtable |
| `vtable_xrefs.tsv` | 34958 | every `lea reg,[rip+x]` that points at an RTTI vtable: `vtable_va  class  site_or_fn_va  fn_name  fn_size`. This is how a ctor (or any function) that installs a vtable is found |
| `constants.tsv` | 74697 | every `DAT_14xxxxxxx` the decompile mentions: `addr  section  u32  u32_dec  f32  u64  f64  refs_in_c  c_literal_stores  c_first_literal_store` |
| `tables.tsv` | 700 tables with rows, 64 words each (T4 has none, see gaps) | named tables (`T1..T4`, the three `DAT_` tables from the brief, `LAB_141aaaffc`), 11 `LAB_` tables used as indexed tables, and 683 `switchdataD_` jump tables. Columns: `table base_va idx va u32 s32 f32 target_if_image_base target_if_table_base section` |
| `extract_rdata.py` | | the script that produced everything above |

Each file is plain TSV with a header row. Sizes: about 18.5 MB uncompressed plus the 1.5 MB gzip.

## How to grep

```sh
# all slots of a class (class names are raw MSVC-ish, '@' separates scopes)
zcat docs/re/kontakt-rdata/vtables.tsv.gz | rg -F 'FenderDeluxeReverb@'
# the vtable at a given VA, all slots
zcat docs/re/kontakt-rdata/vtables.tsv.gz | rg -P '\t1476991e8\t'
# which functions install a vtable (ctor lookup)
rg -n -P '\t141958050\t' docs/re/kontakt-rdata/vtable_xrefs.tsv
# which _Func_impl vtables a ctor creates, and their bodies
rg -n '141958050' docs/re/kontakt-rdata/funcimpl.tsv
# constant at an address
rg -n -P '^14a6b2600\t' docs/re/kontakt-rdata/constants.tsv
# one jump table
rg -n -P '^LAB_141aaaffc\t' docs/re/kontakt-rdata/tables.tsv
```

Little-endian reads: a u16 table indexed with `*(ushort *)(&LAB_x + i*2)` is in the
u32 column of `tables.tsv`; word `k` holds entries `2k` (low half) and `2k+1` (high half).

## How it was produced

```sh
python3 -I docs/re/kontakt-rdata/extract_rdata.py \
  /data/dev/workspace/re-binaries/Kontakt8.exe \
  /data/dev/workspace/Research/KONTRA/source/kontakt8 \
  docs/re/kontakt-rdata
```

Only `pefile` is used, read-only. No Ghidra, radare2, Wine or unpacker was started. Runtime is about 30 s.
Steps:

1. **RTTI vtables.** Every 8-byte aligned pointer in the non-executable sections that points
   at a valid x64 `RTTICompleteObjectLocator` (signature 1, `pSelf` equals its own RVA, TypeDescriptor
   name starts `.?A`) marks a vtable at `pointer + 8`. Base classes come from the
   ClassHierarchyDescriptor.
2. **Slots.** Consecutive pointers into executable sections, starting at the vtable. The scan stops at the first
   non-code pointer (the next vtable's COL pointer or data). Max 4096 slots.
3. **Targets.** `decompiled_size` is the `size=` from the matching `.c` header, `-` if the target has no
   decompile header (usually a thunk or data).
4. **xrefs.** A byte pattern `REX.W 8D modrm(rip-relative)` in `.text`, target = next IP + disp32, kept when
   the target is an RTTI vtable. Attributed to the decompiled function that contains the site.
   This is a byte scan, not a disassembly. Treat a hit as a strong candidate, then confirm in the `.c`.
5. **Constants.** Every `DAT_14xxxxxxx` in the `.c` export. Values are read from the file. `.data` and `.bss`
   beyond the raw data read as unbacked (`bss-or-unbacked`, no value). For those, `c_literal_stores` and
   `c_first_literal_store` give the literal the decompile shows being stored.
6. **Tables.** The named addresses plus every `LAB_` used as `&LAB_x + ...` or `(&LAB_x)[...]`
   (indexed, not a code label) and every `switchdataD_` label. 64 words each. Rows past the real case
   count, or past an overlapping neighbour table, are not valid entries.

## Known gaps and cautions

- **Slot roles are not named.** The export has no symbols. For `_Func_impl` vtables, slots 0 and 1 are
  normally the destructor pair, and the callable (`_Do_call`) is a later slot. This is an MSVC STL
  convention, not verified against this binary. Check a candidate by reading its body in the `.c`.
  Example: the `FenderDeluxeReverb` ctor `141958050` installs the vtable `1476991e8` (lambda_1) and
  the slot targets are in `funcimpl.tsv`.
- **Unbacked constants are runtime-built.** `DAT_14a6b2600` (the T4 table) is in `.data` past the file data,
  so it reads as zero here. It is built at startup by `FUN_1404d14d0` (`1404d14d0`, size 214, a loop of
  `pow()` calls writing float pairs), so T4 has no rows in `tables.tsv`. Read that function for the values.
- **Table bases.** `LAB_141aaaffc` is dispatched in `FUN_141aaac90` (ends at `141aaacfa`).
  `target_if_image_base` (`141aaacfa`, `141aaad06`, ...) lands right after that function, so the
  image-base reading looks correct for that table. `target_if_table_base` lands about 1 MB away. The
  other 10 `LAB_` tables and the `switchdataD_` tables are not checked. Verify against the switch's
  case code before trusting a base.
- **Table lengths.** 64 words is a cap, not a length. The Ghidra switch text gives the real case count.
- **Coverage.** `vtables` covers only classes with RTTI. Vtables without RTTI, and functions the export
  could not decompile, are not represented.
- **Overlap.** `T1`, `T2`, `T3` are 0x30 apart, so their 64-word dumps overlap. Only the first 12 words of each are
  meaningful.
