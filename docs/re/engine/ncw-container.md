# NCW container (Kontakt 8 writer, decompile-confirmed)

Source: Kontakt 8 decompile, `0140970000.c` (writer `FUN_14097cfa0`, 1670 B),
`0141080000.c` (sample conversion `FUN_141086fc0` → NCW or WAV writer).

Confirmed by the writer's header/block stores:

| Offset | Size | Field | Writer evidence |
|---|---|---|---|
| 0 | 8 | signature `01 A8 9E D6 31 01 00 00` | stores `0xd69ea801` then `0x131` (`FUN_14097cfa0`, stack block `local_8c8`) |
| 8 | 2 | channels | `param_1[0x11]` |
| 10 | 2 | bits per sample | `*(int*)(param_1+0x8c)` |
| 12 | 4 | sample rate | `param_1[0x12]` |
| 16 | 4 | frames | `param_1[0x10]` |
| 20 | 4 | blocks offset = 120 | constant `0x78` |
| 24 | 4 | data offset = 124 + 4·blocks | `blocks*4 + 0x7c` |
| 28 | 4 | data size | `blocks * channels * 0x10 + …` |
| 32 | 4 | format flag | `1` iff byte `param_1+0x94` ≠ 0 (32-bit payload) |

Block header (staged per channel, 0x810 bytes = 16 + 2048): magic `0x160C9A3E`
(stored LE as `16 0C 9A 3E`), zero dword, u16 block flag (`2` iff the 32-bit flag
is set), u16 field from `FUN_140828210` (range helper over the block's 512 samples).

Sample count per block: 512 (`0x200` loop bound in both helpers).

Matches the vendored `vendor/ncw` constants (`BLOCK_MAGIC`, `SAMPLES_PER_BLOCK`,
signature variant `…31…`, 120-byte header).

## Not yet recovered

- The packed payload (`FUN_1407d25c0` staging, the delta/bit-pack loop in
  `FUN_14097cfa0`, and the field's exact bit-width law in `FUN_140828210`). The
  vendored codec is the only implementation here.
- Bytes 36–119 of the header: the writer does not clearly initialise them.
- Native vectors: no `.ncw` file exists on the installed library volume
  (`find /mnt/MAIN_STORAGE/Libraries -iname '*.ncw'` returns nothing), so
  bit-exact verification is blocked until a Kontakt-written NCW is available.
