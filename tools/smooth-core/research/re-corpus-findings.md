# soothe3 pseudocode corpus: findings

Corpus: 31,052 `.pdata` functions in `.text` and `IPPCODE`, decompiled with r2 + Ghidra
`pdg` (31,038 ok, 14 timed out or crashed Ghidra even alone; listed at the end).
Index: `corpus.sqlite`, 149,704 call edges, 8,141 string refs, 7,025 float-constant refs,
1,918 RTTI vtables. Build and query: `re-tools/README.md`.
Addresses below are VA (image base `0x180000000`); RVAs are the VA minus the base.

Status legend: **located** = address and code shown; **partial** = part of the answer;
**not located** = not found with these tools.

## 1. Forward FFT scale (spec constructor) — not located

Established from the wrapper `fcn.180001640` (rva `0x1640`):

```c
if (*arg3 != 7) return 0xfffffff3;                 // spec ID
iVar5 = *(arg3 + 4);                               // order
if (*(arg3 + 8) == 0)  table_unscaled[order](arg1, arg2);
else                   table_scaled[order](arg1, arg2, *(arg3 + 0x10));  // float scale
```

So `spec+0x10` is the only forward scale input, and `spec+8` selects scaled or unscaled.

Not located: the constructor. Three store patterns found no match: `*argN = 7` with
`+4`, `+8`, `+0x10` stores in the same function (the only two `*argN = 7` hits, `0x7cc120`
and `0x7cb320`, use a different 16-byte layout); `CONCAT44(…,7)` (none). The constructor
may build the spec on the stack and copy it, or write the ID inside a larger store.
Next step: follow the callers of the `table_scaled`/`table_unscaled` entries
(`0x181e16ab0`, `0x181e16ae0`) and the spec allocator, not store patterns.

## 2. Mid/side encode/decode and constants — resolved

Resolved in `stereo-ms-sidechain-output.md` section 1: encode gain 0.5 at `0x1808928a3`,
decode gain 1.0 at `0x18089a5bc`, through helper `0x1808a3b20` (emulated). The text below is the
earlier state and is kept for the trail.

- The stereo-mode flag is `*(arg1 + 0xa6)`, written by the setter at `0x89467e`
  (`0x89445b`, `0x894370` are the same family) and read in the process function
  `0x890a00` (`cVar38 = *(arg1 + 0xa6)`, lines 2942–2967 of the annotated file). The
  mid/side branch is `if (cVar38 != '\0')`.
- The parameter string "Toggle the stereo processing mode between left/right and
  mid/side." is referenced from `0x180663850`; the "Stereo mode" name from `0x180658e90`.
- Constants: `0x890a00` has no scalar float loads in the index. The matrix constants
  (1/2, 1/√2) must come from packed (`mulps`) or table loads, which the index does not
  yet capture. 80 functions load 1/√2 as float32 (`corpus_query.py by-const 0.70710678`);
  none has been checked against the mid/side branch yet.

Not located: the exact encode and decode arithmetic and its constants.

## 3. Latency fields — partial, getter confirmed

- `getLatency` `0x18088f610` (verified in pseudocode):
  ```c
  if (*(arg1 + 0x237) == 0 && *(arg1 + 0xab) == 0)
      return H(arg1 + 0x5f8) * *(arg1 + 0x2c) + *(arg1 + 0x278);
  return *(arg1 + 0x278);
  ```
  `H` is `fcn.180894d20` (the helper in the earlier notes).
- Writers of `+0x278`: eight functions. Two plan constructors write all of
  `[0x2c] = 1`, `[0x278] = …`, `[0x30] = …`:
  - `0x18063ceb0`, called from `0x18063ab82` at `0x63b9cf` and `0x63c96b`. `[0x278] = in_stack_00000040[3]`
    and `[0x30] = *in_stack_00000090`, so both values come from a caller table.
  - `0x18066fda0`, called from `0x18066e500` (`Footer::slot3`) at `0x66f32f` and `0x66fc00`.
    Same pattern.
  Other writers store `0` or copy another object's field (`0x4b9920`, `0x326d50`, `0x4b9b20`,
  `0x460690`, `0x6b86c0`, `0x633fa0`).
- `[0x2c78]` has no `) = ` writer in the corpus (it may be written through a computed
  pointer). `[0x2f10]` also has no writer.

Not located: the per-quality and per-size numbers. They are inside the caller tables
(`0x18063ab82`, `0x18066e500`), which the next pass should decode.

## 4. Sidechain detector selection (`+0xa5`) — partial

- Setters: `0x89445b` and `0x89467e` write `*(arg1 + 0xa5) = bVar4` (sidechain,
  parameter 14), with `*(arg1 + 0xa6) = bVar4` (stereo mode) in the same function. A
  return path reads the old value first.
- Readers: `0x890a00`, `0x893bd7`, `0x894370`, `0x8943a1`, `0x8947bd` and
  `0x19b6b80`-family functions. `0x4a5e60` reads parameter id `0xa5` through a vtable
  call, not the field.

Not located: the mux that chooses the detector input from `+0xa5`. The reads are in
the process path; the branch that uses them is not yet decoded.

## Not decompiled (14 functions, `missing` in the index)

Ghidra timed out or crashed (`pdg`) even as single-function batches:
`007fa936`, `007fa9b0` (crash), `009f2780`, `009f7270`, `009fc1f0`, `009fefb0`,
`00ab9f00`, `00abcfb0`, `00c10a60`, `00c30fb0`, `00c31536`, `0190e3a0`, `0190ee50`,
`0190fde0`. Their call edges are still in the index; the bodies are not.

## Index limits

- Indirect calls through vtables are not in the call table. Use `vtable` instead.
- Packed SSE constants are not indexed; only scalar `movss`/`movsd` loads.
- Output is `pdg` text without type recovery. Search by the offset literal (for example
  `0x278`) and check the context.
