# Kontakt effects, batch A: Kolor, Skreamer Deluxe, Chainsaw, Stereo Tune, Surround Panner

Status: **not modeled**. This pass located the processing chain for each module
from the RTTI vtables and the decompile cache. No DSP is ported, no kemu vector
exists, and no line of the kernels was compared. Every address below is from
`docs/re/kontakt-rdata/vtables.tsv.gz` (slot rows) and the decompile cache
`/data/dev/workspace/Research/KONTRA/source/kontakt8/*.c`.

## Method

1. Vtable of the BEffect class and of `ModulatableDSPCoreWrapper<BEffect*Traits>`
   via `vtables.tsv.gz` (class name, vtable VA, slot, target, size).
2. Slot 23 of the BEffect class is the block driver. Confirmed for Kolor by
   reading the body (below). Same slot is the driver for Ring Mod and Rotator
   (`PARITY_MATRIX.md`).
3. Slot 0x98 (= index 19) of an effect core is its per-block process. Confirmed
   by reading the call in the driver.
4. Slot 29 of the BEffect class is the parameter apply (setter) for Chainsaw, Kolor,
   Skreamer Deluxe, Stereo Tune (sizes below). Not yet read for all of them.

## Kolor (`BEffectKolor` 0x144e93550, wrapper 0x144e93440)

| Step | Address | Size | What is confirmed |
| --- | --- | --- | --- |
| BEffect ctor | `1407c7950` | 106 | Allocates 0x9ac0 bytes, calls the wrapper ctor `1407f10e0` |
| wrapper ctor | `1407f10e0` | 467 | Installs `ModulatableDSPCoreWrapper<BEffectKolorTraits,0>` vtable; the core object is at byte offset **0x1c0** (qword index 0x38) and its ctor is `141976b20` |
| wrapper slot 23 (block driver) | `140900eb0` | 1542 | Runs pre stage `140ac28f0`, post stage `140ac21f0`, then calls the core: `(*(core+0x1c0)->vtbl[0x98])` (vtable slot 19) |
| core vtable | `14769af00` | | slot 19 = `14197a2c0` (2139 bytes): the Kolor per-block process |
| core slot 20 | `141979410` | 1607 | second kernel-sized function on the core (not read) |
| core slot 3 | `14099c140` | 951 | not read |
| apply (slot 29 of BEffectKolor) | `140abae20` | 392 | not read |

Inside `14197a2c0` (read, not yet ported): a per-block parameter smoother
(`param+0x7e0..0x7ec`); a DSP table lookup `FUN_1419749f0` indexed by the mode
(`param+0x7dc`, stride `0x700`, base `+0x800`), and a mode-dependent gain table
at `DAT_14769aea8`; a chain of trapezoidal one-pole/biquad-style sections (state
at `+0x9484`, `+0x9568`, `+0x9670`, `+0x9708`); a per-mode branch on modes 2, 4
and 5 (`+0x9400`). The function is numeric and mode-driven; the lookup body
`1419749f0` and the constants at `DAT_14769aed0` / `DAT_14769aea8` still need to
be read before a port.

Mode table and knob registrations: see `k-stomps-b.md` (unchanged, still
measured).

## Skreamer Deluxe (`BEffectSkreamerDlx` 0x144e93da8, wrapper 0x144e93c98)

| Step | Address | Size |
| --- | --- | --- |
| wrapper slot 23 (block driver) | `140902480` | 2283 |
| wrapper slot 1 | `1408dc800` | 329 |
| wrapper slot 18 | `1408804d0` | 12 |
| BEffect slot 29 (apply) | `140abb9f0` | 748 |

The driver is the same length as the Chainsaw driver (both 2283 bytes); the
two are probably one template with different cores. Not compared yet.

## Chainsaw (`BEffectChainsaw` 0x144f15e68, wrapper 0x144f15d58)

| Step | Address | Size |
| --- | --- | --- |
| wrapper slot 23 (block driver) | `1408fe290` | 2283 |
| wrapper slot 1 | `1408dbd40` | 242 |
| BEffect slot 29 (apply) | `140aba230` | 485 |

Apply `140aba230` is the one already named in `PARITY_MATRIX.md`. Its body is
confirmed to clamp the four knobs and to write the two cores through
`FUN_1409a9ec0` (channels 0..3 at `+0x1340` and `+0x200`, state at `+0x229c`
...`+0x22a8`). Kernel: not read.

## Stereo Tune (`BEffectStereoTune` 0x144fc2868)

| Step | Address | Size |
| --- | --- | --- |
| BEffect slot 23 (driver candidate) | `140aa7320` | 176 |
| BEffect slot 5 | `140ab8230` | 572 |
| BEffect slot 29 (apply) | `140abbce0` | 335 |
| BEffect slot 21 | `140a98850` | 139 |

Slot 23 calls `FUN_140aaf640` per chunk and `FUN_140acb6c0` on the 32-sample
boundary, then `FUN_140ac21f0` (shared post stage). `FUN_140aaf640` is the
per-chunk core and is not read. The reader (`140ac6120`, four floats) is
already confirmed (`PARITY_MATRIX.md`).

## Surround Panner (`BEffectSurroundPanner` 0x144e94e68)

| Step | Address | Size |
| --- | --- | --- |
| BEffect slot 23 | `140905fa0` | 87 |
| BEffect slot 5 | `14094cde0` | 50 |
| BEffect slot 29 | `1404f99a0` | 3 (no-op) |
| BEffect slot 0 | `14081d530` | 149 |

Slot 23 is small. It most likely dispatches to a core that is not in this
vtable. The speaker matrix and LFE routing are still unrecovered.

## Open, in order

1. Kolor: read `1419749f0` (table lookup) and the constants `DAT_14769aed0`,
   `DAT_14769aea8`; then port `14197a2c0` to the one engine DSP and build kemu
   vectors (`tools/re/*_kemu_vectors.py` templates).
2. Skreamer Deluxe and Chainsaw: diff `140902480` and `1408fe290` (same length);
   identify the shared core, then read the Chainsaw kernel behind `140aba230`.
3. Stereo Tune: read `FUN_140aaf640`.
4. Surround Panner: find the core that slot 23 dispatches to.
