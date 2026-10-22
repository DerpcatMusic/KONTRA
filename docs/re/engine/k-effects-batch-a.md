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

### Kolor kernel, decoded (2026-10-23, from `0141970000.c` and the binary)

Constants read from `Kontakt8.exe` with radare2 (`pf 8f`, file-backed, not
from the decompile):

| Symbol | Address | Values (index 0..) |
| --- | --- | --- |
| `DAT_14769aed0` | `0x14769aed0` | 26.3290501, 0.836556315, 11.2201843, 0.0222088695, 10.0359087 (indices 5+ not read) |
| `DAT_14769aea8` | `0x14769aea8` | 0.0847919211, -0.364524066, 0.562341332, 0.527866721, 0.121059023, 1.0, 0.189637259, 0.335529774 (indices 8+ not read) |

Per-block (`14197a2c0`, `param_4` samples, two channels):

1. If `param+0x515`: refresh 14 control slots at `param+0x5c` from `param+0x480`
   (helper `141940330`), and notify the slots at `param+0xd0` (vtable slot 2).
2. Gain smoother on `param+0x7e4` toward `param+0x7e8` with one-pole coefficient
   `param+0x7e0`. Below the threshold `param+0x7ec` it snaps and sets the
   `param+0x7f0` flag.
3. Per sample, input is scaled by the smoothed gain and the stage chain runs in
   order:
   - pre-gain `DAT_14769aed0[mode]` times `param+0x7d4` times `param+0x7c0`;
   - a two-pole section (state `+0x9568`, coefficients at `-0xc`..`-0x1`);
   - the shaper `FUN_1419749f0` on the object at
     `param+0x800 + (mode*2 + k)*0x700` (dispatch on the object's type byte at
     `+0x6f0`, cases 0..9; cases 0, 1, 3, 4, 6..9 are read as calls and not yet
     followed);
   - post-gain `DAT_14769aea8[mode]`; modes 2 and 5 multiply by `param+0x9400`;
   - mode 4 adds a second two-pole section (`+0x9568` lane, offset `-0x4e`);
   - a bank of biquad-style sections at `+0x9484` and `+0x9670`, and a mix
     through `param+0x78` (wet) against the dry input, scaled by the gain.
4. A stereo level detector updates `param+0x9748` with attack/release from
   `+0x9754`/`+0x9758`.

Not yet done: the helpers `14194cc70`, `14194c830`, `141974860`,
`14197adb0`, `14197a150`, `14197ae80`, `14194cfb0`, `1419748b0` (called from the
shaper dispatch); the exact coefficient meaning of the biquad state; the
Kolor state init. No kemu vector exists; the port is not started.

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

### Skreamer Deluxe, decoded (2026-10-23, decompile cache and binary constants)

Core type: the wrapper is `ModulatableDSPCoreWrapper<BEffectSkreamerDlxTraits>`
over **`Stereorizer<Tubescreamer>`** (`vtables.tsv.gz`, wrapper rows). So the
kernel is the Tubescreamer core already in `sampler-core/src/dsp/color/tubescreamer.rs`,
not a new one. Its native vectors are `tables/kontakt_tubescreamer_vectors.json`.

**Reader to core index (apply `140abb9f0`, decompile `0140ab0000.c:7935`).** The
apply calls `FUN_1409a9f90(core, index, value)` on both cores (`param+0x1480`
and `param+0x200`). Values are clamped to [0, 1] (`DAT_14470bad4` = 1; the
mode step uses `DAT_14470bad0` = 0.5):

| Reader field | Field | Core index | Knob (KSP name) |
| --- | --- | --- | --- |
| `+0x08` float | drive | 0 | Drive |
| `+0x0c` float | tone | 4 | Tone |
| `+0x10` float | bass | 1 | Bass (`pot a` in the core's own naming) |
| `+0x14` float | mid | 2 | Mid (`pot b`) |
| `+0x18` float | treble | 3 | Treble (`filter`) |
| `+0x1c` int | mode | 6 = `clamp(int * 0.5, 0, 1)` | circuit `int(p6 * 3)` in the core (ints 0, 1, 2) |
| (constant) | level | 5 = 0.5 | written once (`+0x2530` guard) |

This is the same field-to-index table the removed test
`tubescreamer_record_fields_land_on_their_core_indices` asserted
(`[0.1, 0.3, 0.4, 0.6, 0.2, 0.5, 1.0]` for drive, pot a, pot b, filter, tone), so
that mapping was not wrong. Only the names differed. The apply writes indices by
number, and the index meaning comes from the core ctor. Byte `+0x20` is a flag
copied into the wrapper byte `+0x1c0` (see the driver below).

**Driver `140902480` (2283 B), what is confirmed:**
- Pre stage `140ac28f0`, post stage `140ac21f0` (shared with Kolor and Chainsaw).
- Two-path branch on wrapper byte `+0x1c0` (from reader `+0x20`):
  - `+0x1c0 == 0`: stereo branch. Core `+0x1480` runs on the stereo pointer pair
    `+0x2540` to `+0x25d0`, and core `+0x200` runs on a second pair built in
    `+0x1280`/`+0x1300`/`+0x1380`/`+0x1400` (the L/R copies with `memmove`), then a
    combine into `+0x25d8`.
  - `+0x1c0 != 0`: mono branch. Only core `+0x1480` is called.
- Each core call is vtable slot 0x98 (index 19), the same as Kolor.
- Optional control-rate smoother `FUN_141ee2160` (fractional block split by a
  ramp count from `+0x1874`/`+0x17d8`/`+0x17e0`) runs before each core, on the
  flag `+0x19fc` (the input side) and `+0x77c` (the core-2 side).
- Tail: peak-hold at `+0x2510`, and a state at `+0x2514`. When the state is 1 and
  the `+0x2500` level is at or below `+0x2510 * DAT_1447148a0`, it moves to 2. The
  branch that zeroes outputs (`memset`) is not yet read.

**Still not decoded (so no port):** the exact stereo combine (how `+0x25d8`
selects between core outputs), the `FUN_141ee2160` ramp, and the gate's constant
`DAT_1447148a0`. There is no native capture and no kemu harness for the wrapper
(`140902480` calls shared stages, so a kemu run needs them stubbed or emulated).
Next: decode the combine and the gate, then run the driver under kemu with the
core's seeding slot after prepare, and compare to a native capture.

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
