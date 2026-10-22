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

## Pass 2 (decoded, not ported, no native vector yet)

Evidence: vtable rows from `kontakt-rdata/vtables.tsv.gz`, decompile cache
`Research/KONTRA/source/kontakt8/*.c`. Status stays **missing**: no kernel is
ported and no kemu vector exists, so no row can be "parity".

### Chainsaw kernel = HM2 core (identified)

- Chainsaw wrapper bases are `Stereorizer<HM2>`; the core class is
  `hm2::dsp::effektrig::HM2`, vtable `0x14769a9e8` (58-byte slot 0 ctor, 61-byte `0x14769aaa0` variant).
- Wrapper slot 23 `1408fe290` calls the core at `+0x1340` through vtable slot 19
  (`0x98`), with the channel pointer arrays at `+0x22b0` (input) and `+0x2340`
  (output), block size in the 4th argument.
- **HM2 slot 19 = `141971820` (2317 B), the per-block process.** Read in full
  (`0141970000.c:825`). Per sample: input `(L+R) * DAT_144714900` through a
  trapezoidal one-pole pair (state `+0xe24..+0xe2c`, `+0xe30`), a feedback
  stage on `+0xe60..+0xe78` and `+0xe3c..+0xe54`, the sub-processes
  `FUN_1419714c0`, `FUN_1419590a0`, `FUN_14194c5b0` (the last two
  not read), a `sqrt(x^2+eps)` soft-saturator, a `FUN_141971150` lookup
  (not read) on the `fma` path, and the output is written to both channels.
  Smoothing and the reset are in `FUN_141972140` (83 B). Not read: `1419714c0`,
  `1419590a0`, `14194c5b0`, `141971150`, the constants `DAT_14769ac*`.
- Apply `140aba230` writes the cores through **HM2 slot 12 = `1409a9ec0`** (194 B),
  so the clamp indices 0..3 are the HM2 knob slots.
- Other HM2 slots: 3 `14099a700` (951 B), 6 `1419709e0` (358 B), 8 `1419721a0` (164 B, reset),
  20 `141970b50` (1533 B), 18 `1419723c0` (29 B).
- Still to do: read the four helper functions and constants, then vectors by
  calling slot 19 directly on a prepared HM2 object (no driver, so the
  ring-buffer fault in `engine-k-stomps-full/tools/re/kemu_chainsaw.py` is not hit).

### Stereo Tune per-chunk core (partly read)

- Driver `140aa7320` (176 B): loops over 32-sample chunks (`+0x120` total,
  `+0x1bc` countdown), calls `FUN_140acb6c0` on the boundary, `FUN_140aaf640`
  per chunk, then `FUN_140ac21f0` (shared post).
- `140aaf640` (2583 B, `0140aa0000.c:11795`) is a **modulated delay line**:
  per channel, a phase/rate pair (`+0x1e0..+0x1f8`, incremented per sample) drives
  the read position in a ring of length `*(+0x238)`, with an offset `+0x23c`/`+0x240`,
  a 4-point cubic (Lagrange-style, constants `DAT_14470bad8`, `DAT_144fc40e0`, `DAT_144f385a8`)
  interpolator, and a one-pole/allpass pair (`+0x304..+0x31c`) on the write
  side. Gain table at `*(+0x248)`. The two channels read the same ring with
  different offsets (cross-tap). Not read in full: the second channel half
  (decompile lines ~11900+) and the constants.

### Surround Panner: slot 23 is a routing driver, the law is in helpers

- Slot 23 `140905fa0` (87 B) = `FUN_140ab0070(this, buf, 0, n, mode)` then
  `FUN_140ac21f0` (post). Not a DSP core.
- `140ab0070` (1880 B) is the generic per-channel block driver: per source
  it either applies the scalar gain at `+0xc00` (fast path, sum into the
  output buffer) or calls `FUN_140ab4eb0` (3813 B, the panned path, flag 1/0).
  The LFE sub-object at `+0x7a0` gets vtable slots `0x58`, `0x30`, `0x38`,
  `0xb8` (block prepare, process pair, process).
- **Speaker gain law** `FUN_140a94580` (511 B), mode at `+0x394`:
  - mode 0 (default): `g_i = w_i / sum_j w_j` with `w_i = (FUN_140a93cb0 / d_i^2)^p`,
    `p = +0x34c`, `d_i = max(FUN_140a94180(src, i), 0.01)` (`DAT_1447148b4`).
  - mode 1: `x = d * p * 2pi`; `g = FUN_140ac3230(x + pi/2)` when `x <= pi/2`, else 0.
  - mode 2: `g = 2^(d * p * -8)` (`DAT_144714998` = 2.0, `DAT_144fc4d20` = -8).
- `FUN_140a94180` (234 B): Euclidean distance between the source
  (`FUN_140a985b0`, at `+0x348` scaled by `DAT_1447148d0`=0.1) and speaker `i`
  (`+0x1c0+12i`), scaled by `DAT_144fc3e50` if flag = 1.
- `FUN_140a985b0` (520 B): source position from `+0x284/+0x288`, angle via a
  1024-entry table at `0x14a612760` (sin-like, indexed `(a/2pi)*1024`),
  adds `+0x350/+0x354` offsets, clamps to [0,1].
- `FUN_140ac3230` (244 B): periodic lookup in a 1024-entry table at
  `0x14a611750`, scaled by `fVar1` = `DAT_14470bad4` (1.0).
- **Blocker for the vector**: both tables (`0x14a612760`, `0x14a611750`) are
  `.data (bss-or-unbacked)` in `constants.tsv`, so their contents are filled
  at runtime. The initializer has not been found (no decompile references in
  `0140ac0000.c` or `0140a90000.c` writing them). Find it before a vector.
- Constants confirmed from `constants.tsv`: `0.01` (`1447148b4`), `0.1`
  (`1447148d0`), `0.5`, `1.0`, `1024` (`144fc4b0c`), `1/pi` (`144f38960`),
  `2/pi` (`144f38988`), `pi`, `2pi`, `pi/2`, `1e-6`, `1e-7`, `-1`, `2.0`, `-8`,
  `0.7071` (`144fc3b60`); `144fc3e50` = 1.17851126, `144fc43c0` = 2pi (f32).
