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

### Kolor emulation status (2026-10-23, `tools/re/kolor_kemu_probe.py`)

Established under kemu (native code, no port):

- Construction: `BEffectKolor` ctor `1407c7950` runs (allocator `140711a10` intercepted). The core is
  the object at `obj+0x1c0`; its vtable is `14769af00`, slot 19 = `14197a2c0`, slot 20 = `141979410`.
- Block call (read from `140900eb0` at `0x140901088..0x140901178`): core process is
  `(this=core, in_chans, out_chans, count)`; the driver passes the same two-pointer array for both.
  The kernel reads the dry sample from `in[k]` and writes `out[k]`.
- Prepare = core slot 20, `141979410(this, rate f32)`. It seeds `+0x7e0` (smoother), `+0x7e4`/`+0x7e8`
  (= 1), `+0x9744` (= rate), calls `FUN_1419790c0`.
- Per-mode seed `14197acc0(core, mode)` writes `+0x7dc`, clears `+0x7f4`, and resets the two shaper
  objects of that mode at `core+0x800+(2m+k)*0x700` through `141974d90`.
- Apply `140abae20` (slot 29) calls the wrapper setter `1409aaa90` (slot 32) for knobs 0..13; knob
  values land at `core+0x5c`.
- Shaper dispatch `1419749f0` returns the first float of its stage output (`movss xmm0,[rsp+0x20]` at
  `141974b43`; disassembly in `docs/re/disasm/kolor-shaper-dispatch-1419749f0.txt`). Case 9 is
  `1419748b0` (`docs/re/disasm/kolor-case9-1419748b0.txt`).
- Ctor builds each shaper template in a 0x6f0 scratch via `141975910`/`141975c10`/... and copies it with
  `1419735c0`/`1419738d0`/`141973be0`/`141973ef0`/`1419732b0`. Writers into the mode-9 object are the copy
  at `0x1419734f3`.
- Dry path works: with `+0x78 = 0`, output = gain ramp `+0x7c4` times input (peak 0.0074 on a 0.5 sine
  in the first block, from the ramp starting at 0).

Blocker (evidence): with `+0x78 = 1` (wet) the output is exactly zero in all 10 modes. The biquad
coefficient blocks at `0x9400..0x96b8` are only partly sane: values such as `0x955c = 1.3e11`,
`0x94c0 = 4.8e20`, `0x9508 = 13676.7` are non-physical, so the knob-driven design step is not run
correctly. The writer is the control refresh `141940330`, gated by flags at `param+0x498` and by
`+0x515`, which the probe does not yet satisfy. Next: trace which function writes `+0x9508`/`+0x955c`
during the block (same write-tracer approach as `probe5`), and resolve the `+0x498` flag source.

Not yet done: the helpers `14194cc70`, `14194c830`, `141974860`,
`14197adb0`, `14197a150`, `14197ae80`, `14194cfb0`, `1419748b0` (called from the
shaper dispatch; their bodies are read from the cache, not yet ported); the exact coefficient meaning of the biquad state; the
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

**Stereo combine (decoded 2026-10-23, `0140900000.c` lines 1790-1860).** Stereo branch
(wrapper byte `+0x1c0 == 0`): `memmove(in1 <- in0)` after saving R to `+0x1280`/`+0x1300`.
Core `+0x1480` runs on `[L, L]` into the host outputs; core `+0x200` runs on `[R, R]` into
`+0x1380`; `memmove(out1 <- +0x1380)`. Each core keeps its own state, and the two cores
share the knobs. Ported as `Stereorizer<Tubescreamer>` (`dsp/color/stereorizer.rs`).

**Gate (decoded).** `DAT_1447148a0` = 9.99999975e-06 (1e-5). The gate test only moves the
status word `+0x2514` from 1 to 2 when the peak `+0x2500` falls below `+0x2510 * 1e-5`. Nothing
reads that state for audio; `+0x2514` is queried by the host, and the silence path (`memset`
of outputs) runs only while the transition flag `+0x2518` is set (set and cleared around the
core reset in `140971540`). Steady-state audio is unaffected, so the port omits it.

**Still not decoded:** the control ramp `FUN_141ee2160` (kernels types 1, 3, 4, 5, 6;
type 2 is a copy). It runs only when the ramp flags `+0x19fc` / `+0x77c` are set, which
the steady-state path does not need. The mono branch (flag `+0x1c0 != 0`, core `+0x1480`
in place) is not modeled. No native stereo capture exists; the port's vectors are the
native mono Tubescreamer vectors applied per channel.

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

## Pass 3: direct HM2 kemu probe (blocked, 2026-10-23)

Probe: `tools/re/chainsaw_hm2_kemu.py` (runs, no fault, output 0). Sequence: ctor `14196fd00`,
slot 5 `14099c970`, slot 3 `14099a700` (rate, 32), slot 6 `1419709e0`, slot 20 `141970b50` (rate),
knobs `1409a9ec0` 0..3, slot 19 `141971820`. The ring-buffer fault of the driver path is not hit.

Blocker: output is exactly zero. The per-block gain is `obj+0x40`, copied from `+0x180[3]` only in
the block that is gated by the dirty flag `+0x215`. `1409a9ec0` takes the `+0x148 != 0` branch after
prepare (zeroes `+0x198[i]`, so nothing is copied), and no decompiled writer sets `+0x215` to 1
(`0140990000.c:10227` and `0141970000.c:1162` only clear it). Needed before a vector: read
`1409abdd0` (the `+0x214` deferred-apply path, called from slot 8 `1419721a0`) and the setter that
raises `+0x215`, then confirm the apply order.

Helpers still unread for the port: `14194c5b0` (580 B), `1419590a0` (397 B), `141950580`
(the 10x10 solver used by `141971150`), `1419723e0` (685 B, the reset-constants path), `141936990`,
`141ee86c0`. Constants: the `DAT_14769ab*`/`DAT_14769ac*` table and `DAT_14470bad*` (read via `pf`).
`1419714c0` (863 B) is a Newton-style solver with up to 20 iterations and an exp-polynomial
approximation; its polynomial constants `DAT_14769ab64/ab68` and `DAT_144e79a48` are read here.
No Rust port and no native vector exist, so the row stays **missing**.

## Pass 4: dirty-flag hypothesis rejected, probe knob bug fixed (blocked, 2026-10-23)

Corrects pass 3. Evidence: `tools/re/chainsaw_hm2_kemu.py` (fixed), `docs/re/disasm/chainsaw-hm2-215-scan.txt`.

1. **The knob value is a float in xmm2, not an int in r8.** `1409a9ec0` does `movss [rcx+r9*4+0x150], xmm2`
   (disasm at `0x1409a9eca`). Pass 3 passed the bit pattern as an integer, so the knobs held garbage
   (`0x31932fad`) and the gain read was meaningless. The probe now passes `f={2: v}`.
2. **The per-block gain does not need `+0x215`.** With correct knobs, the apply's else branch
   (`+0x148 != 0`, the prepared path) writes `+0x34[i] = +0x180[i] = value` directly, so `+0x40`
   (= `+0x34 + 3*4`) is 0.5 before the block. The dirty path (`+0x215 != 0`) in `141971820` only
   refreshes `+0x180` from the LFO state; it is not the only route to the gain.
3. **No code raises `+0x215`.** The `.text` scan (disp32 0x210..0x215, capstone-decoded) finds no store
   in the HM2 class range that sets that byte. The only uses are clears (`141972140` at
   `0141970000.c:1162`, `1409x` at `0140990000.c:10227`) and the read `cmp byte [rdi+0x215],0` in
   `141971820`. The dirty path is therefore not reachable from any prepared object by writes we can find.
   Full hit list: `docs/re/disasm/chainsaw-hm2-215-scan.txt`.
4. **Real blocker: the solver chain is zero.** After the fixed knobs the block still outputs exactly 0.
   The chain is `141971150` (835 B, state-space step with a 10x10 solve `141950580`), which feeds
   `1419590a0` (397 B, reads a 4x12 coefficient block at `+0x520..+0x5a0`), then `14194c5b0` and the
   shaper `1419714c0`. Measured after the prepare order plus a redesign (`1419709e0` again):
   `+0x520..+0x57c` = 0 (`1419590a0` output is therefore the bias `param[0x30]` = 0), while `+0x750`
   (the `141971150` matrix) is non-zero (7.23, 1.75). The writers of `+0x584` and up are the
   template constructors `141974510`, `141975080`, `141976810` (used for the `+0x800`/`+0x8e0` shaper
   objects, `0141970000.c:5834..5838`); no writer of `+0x520..+0x57c` exists in the HM2 functions.
   So that block is either built by a template copy we have not found, or it is a sub-object whose
   constructor the wrapper does not run. Next: find which function owns `+0x4c0` (the `1419714c0` object)
   and whether its ctor is reached from `14196fd00` or from `14196fab0` (the base ctor, `0141960000.c:12982`).
5. Helpers still unread: `14194c5b0`, `141950580`, `1419723e0` (read partially, it consumes the
   `+0x520` block), `141936990`, `141ee86c0`, `1409abdd0` (the deferred apply, reads `+0x148`/`+0x1b0`
   /`+0x1f8` via `param[9]`, `param[6]`, `param[12]`; it is not a `+0x215` writer).

Status: **still missing**. No Rust port, no kemu vector, no PARITY_MATRIX row change.
