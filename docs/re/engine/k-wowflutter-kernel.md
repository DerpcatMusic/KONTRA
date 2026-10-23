# Wow / Flutter per-sample kernel: located, not yet ported (2026-10-23)

Status: **kernel located to function level; port blocked on sub-object readers and constants.**
Supersedes the "not located" note in [k-lofi-tape.md](k-lofi-tape.md) (its vtable-slot ruling-out still holds).

## Evidence chain (Kontakt 8.13.1, decompile cache `source/kontakt8/`)

1. **Core embedding.** Core ctor `FUN_140a842b0` (`0140a80000.c:1794`) sets the wrapper vtable `ModulatableDSPCoreWrapper<BEffectWowFlutterTraits,0>` and calls `FUN_14199bc70(this+0x1c0)`. So the DSP kernel object lives at wrapper `+0x1c0`.
2. **Kernel class.** `FUN_14199bc70` (`0141990000.c:8377`) writes `ni::effektrig::dsp::wowflutter::WowFlutter::vftable` at that object. The vtable is at **`0x1476a3400`** (read from `Kontakt8.exe`; slots 0..31 listed below).
3. **Process entry chain (virtual, no direct call):**
   - vtable slot 7 (`+0x38`) = `FUN_14099f000` (`0140990000.c`, 47 lines): wraps the block with a rate-dependent path (`FUN_141ee2160` on `0x63c` flag), then calls **slot 19 (`+0x98`)** on `this` with `(this, in, out, n)`.
   - slot 19 = **`FUN_14199d080`** (`0141990000.c`, 294 lines) — the kernel block process (described below).
   - The parent block loop that calls `this+0x1c0` `+0x38` is `FUN_140aa2060` (`0140aa0000.c:1287`, 141 lines). It is not in the 96-slot wrapper vtables (`0x144fc2d38`, `0x144fc2e48`), and has no direct caller in the cache. It ends with `FUN_140ac21f0` (the gain-ramped mix) — consistent with the prior note.
4. **Kernel body (`FUN_14199d080`):**
   - Block-level smoothers: pairs `(value, coef)` at `this+0x32a0..0x32e0`, one-pole `v += (target - v) * coef`.
   - Two modulation oscillators: entries at `this+0x6f0` and `this+0x808` (stride `0x118`). Each computes `phase = 2π · f · (…) / rate` with `f` from `+0x1c` / `+0x20` and calls **`FUN_1443c9bd0`** (returns sin/cos in XMM0 and XMM1: stored at `+0x50/+0x58` and `+0x80/+0x88`) → the **sin/cos LFO** (wow and flutter). Integer tap delay at `+0x24` (`int(delay_ms·rate)`).
   - Per-sample loop (`do … while (uVar28 < param_4)`):
     - `FUN_14199cc10(this+0x680, out, inL, inR)`: the modulated delay (2 channels), returns a stereo pair.
     - Output mix: `wet = out·gain(+0x330c)·fVar33 + in·(+0x3310)`, where `fVar33` is a block gain from `+0x32e4..0x32f4` (a level detector with attack/release coefs).
5. **Delay read `FUN_14199cc10`** (`0141990000.c:9179`, 592 B):
   - Input filter `FUN_14199c870(this+0x2c0, x)` writes into ring `[+0x18]`, index `+0x20`, size `+0x24`, scale `+0x2c0c`. Second channel: `FUN_14199c870(this+0x1760, …)` into ring `[+0x50]`, index `+0x58`, size `+0x5c`.
   - Tap update `FUN_14199ce60(this+0x70 + k·0x118)` for `k = 0,1` (543 B; grain-like smoothing of tap gains `+0x0c`, `+0x0f`, `+0x24`).
   - Reads: **virtual** `(**(code**)(this+0x30))` and `(**(code**)(this+0x68))`, i.e. sub-object vtable slots, not yet decoded. These are the fractional-delay readers, so the interpolation law is still unknown.
   - Constants used (from PE `.rdata`/`.data`, not decompiled): `DAT_1447149b8`, `DAT_144714988`, `DAT_1447149e4`, `DAT_1447149f8`, `DAT_144714908`, `DAT_1447148a4`, `DAT_144f38a38`, `DAT_14470bad4` (=1.0), `DAT_144fc4d18`, `DAT_1447148f8`, `DAT_144714b70..7c` (4-lane increments), `DAT_144714b90` (mask).

## Excluded as the wow kernel

- `FUN_141993d90` / `FUN_141992760` / `FUN_1419922e0` / `FUN_141992a90` (same file): a 4-lane feedback matrix with `FUN_1419466a0` delay reads. Not called from the WowFlutter process chain. Probably a different effect's network in the same class file; not used here.
- Vtable slots 0..31 of the wrapper (`0x144fc2e48`, `0x144fc2d38`) and the `FUN_140aa*` callers of `FUN_140ac21f0` are the generic block/mixer path, not the law.

## Remaining blockers (to port bit-exact)

1. Decode the two sub-object readers (`+0x30`, `+0x68` slots of `this+0x38` / `this+0x680`) and `FUN_14199c870` (filter law) and `FUN_14199ce60` (tap smoother).
2. Resolve the 10 constants above (read from the PE image).
3. Build the KEMU harness: seed the wrapper before the block loop (the vtable slot that sets the 10 control targets must run, or KEMU diverges — see `tools/re/kemu_timemod.py`), then dump vectors.

Until (1) and (2) are decoded, the generic law in `dsp/timemod/wowflutter.rs` stays as-is (unconfirmed, labelled in code). PARITY_MATRIX row stays not-parity.

## Pass 3 (2026-10-23): structure verified, port still blocked

Method: type → vtable → process → kernel, decompile cache only (no REA, no Ghidra query). Everything below was read from `source/kontakt8`, addresses as given.

1. **Ctor `FUN_14199bc70`** (`0141990000.c:8365`): `FUN_14192d610` base init, wrapper vtable pair at `+0x0`/`+0x8` (`WowFlutter::vftable`), `FUN_14199b8a0(this+0xd0)`, clears `+0x32a4..+0x32e4`, then seeds: `+0x65c = 1000.0f` (`0x472c4400`), `+0x65f = -4.6f` (`0xc0935d8e`), `+0x65d = 0x3727c5ac`, `+0x924 = 0x433e0ccc`, `+0x32ec = 0.1f` then `0.4f` (`0x3dcccccd` → `0x3ecccccd`), `+0x32f4 = 1 − expf((DAT_144fc40e0 / +0x65c)·(−4.6))`, `+0x65e = 1 − expf((DAT_1447377cc / +0x65c)·(−4.6))`, `+0x32e4 = 0`. Then `FUN_14199de70`. These are one-pole coefficients of the level detector (`+0x32e4..+0x32f4`), not wow/flutter depths. Evidence level: **decompiled**.
2. **Setter registration `FUN_14199de70`** (`0141990000.c:9940`, 710 B): registers **nine** `std::function` handlers (`lambda_1`..`lambda_9`, the `_Func_impl_no_alloc` pattern as in Bite) at `this+0x78 + k·0x40` (`0x78, 0xb8, 0xf8, 0x138, 0x178, 0x1b8, 0x1f8, 0x238, 0x278`), via `FUN_140d396a0`. Each lambda body is a separate `FUN_…` in the cache and is the control→field law. **Not decoded in this pass.** This is the next blocker for the Wow/Flutter/Speed/Stereo constants.
3. **Process `FUN_14199d080`** (`0141990000.c:9342`, 1566 B), confirmed to be the body reached through slot 19 (see above). Per block (`param_4` frames), in order:
   - Per-frame, when `+0x3d5 != 0`: four-lane one-pole smoothers over `+0x3b8` (value), `+0x3a0` (target), `+0x358` (coef), output `+0x340`; for each of the 10 slots `k`, if `+0x358[k] != 0` it stores `+0x340[k]` into `this+0x4c+4k` and calls the slot-2 callback of the object at `this+0xb0 + 0x40·k` (`(**(code**)(*plVar6 + 0x10))`). So the 10 callbacks are per-control notifications, not DSP.
   - Four-lane smoother `+0x32a0..+0x32e0` (pairs `(value, coef)`), with `expf(·/+0x3314)` for each lane (`+0x32c0..+0x32d0` and `+0x32b0..+0x32c0`).
   - LFO update for two modulators at `+0x6f0` and `+0x808` (stride `0x118`): `+0x60 = (fVar34·(+0x1c))·DAT_1447149b8·(DAT_144714988/+0x920)`, then `+0x50 = cos-like(+0x60)·(+0x68)`, `+0x58 = sin-like(+0x60)·(+0x68)` through `FUN_1443c9bd0`; second oscillator from `+0x20` into `+0x80`/`+0x88` (scale `+0x98`); integer tap delay `+0x24 = (int)((+0x920·DAT_1447149e4)/fVar34)`. `FUN_1443c9bd0` is a range-reduction wrapper (`FUN_1443c9bb0` does the core), so the bit-exact sin/cos is not yet proved.
   - Per sample: `FUN_14199cc10(this+0x680, out, inL, inR)`, then `out = y·(+0x330c)·g + x·(+0x3310)`, where `g` is 1 or the level-detector gain `+0x32e4` (only when `+0x3318 != 0`). Evidence: decompiled.
4. **Delay read `FUN_14199cc10`** (`0141990000.c:9179`): input through `FUN_14199c870(this+0x2c0, x)` (filter, scale `+0x2c0c`) written into ring `[+0x18]` at index `+0x20` (size `+0x24`); channel 2 through `FUN_14199c870(this+0x1760)` into ring `[+0x50]` (`+0x58`, `+0x5c`). Tap smoothers `FUN_14199ce60(this+0x70 + k·0x118)` for k=0,1 (decompiled: integer ramp `+0x28`/`+0x2c`, a PRNG `x = x·0x19fb4787 + 0x16d1b86b` at `+0x110` feeding `FUN_140d947a0`, a rotation-pair LFO `(+0x118/+0x120/…)`, and its output `*param_1 = p[5] + p[2] + p[3] + p[4]` (byte offsets `+0x0` = `+0x14 + +0x8 + +0xc + +0x10`), per `FUN_14199ce60` at `0141990000.c:9276`). **The two fractional readers are function pointers stored in the struct, not vtable slots**: `(**(code**)(this+0x30))(…, index)` and `(**(code**)(this+0x68))(this+0x38, index)`. Their targets are set in the ctor chain and are **not yet decoded**; they are the interpolation law.
5. **Blocker A: `FUN_14199ae00`** (`0141990000.c:8074`, 965 B), used by `FUN_14199c870` as the 4-lane feedback matrix. The cache decompile returns only a `movmskps` sign mask, so the real 4×4 mixing result is lost in the export. It needs the disassembly (`rabin2`/`r2` at the address; not REA, which times out here).
6. **Blocker B: `FUN_1443c9bb0`** (the sin/cos core behind `FUN_1443c9bd0`) and the constants `DAT_1447149b8`, `DAT_144714988`, `DAT_1447149e4`, `DAT_144714b90` (sign mask), `DAT_144714b70..7c` (lane increments), `DAT_144fc40e0`, `DAT_1447377cc`, `DAT_1447148f8`, `DAT_144fc4d18`. These need the PE `.rdata` read.

Still missing for a port: (a) lambda_1..9 bodies (control→law), (b) the two fractional readers at `+0x30`/`+0x68`, (c) `FUN_14199ae00` from disassembly, (d) sin/cos core and PE constants, (e) a KEMU wrapper run that seeds the 10 control targets (`tools/re/kemu_timemod.py`). The generic law in `wowflutter.rs` stays **unconfirmed**; the Wow/Flutter parity row stays `partial`. No native vectors exist yet, so nothing is claimed as parity.

## Pass 4 (2026-10-23): lambdas decoded, blocker (c) corrected, port still blocked

Method: decompile cache (`source/kontakt8/0141990000.c`) cross-checked with radare2 disassembly of `Kontakt8.exe` (no analysis, `pD` at the address) and constants read with `pefile` from `.rdata`/`.data`. No REA, no Ghidra.

### (a) Control -> law map (resolved)

`FUN_14199de70` builds nine `_Func_impl_no_alloc` handlers at `this+0x78+0x40k`. Each handler's vtable slot 2 is the invoke function (vtables `0x1476a34c8 + 0x38·k`, read from PE). The handler reads its control from `this+0x4c+4k`, which the process loop (`FUN_14199d080`, `if (+0x358[k] != 0) this+0x4c+4k = +0x340[k]`) refreshes from the smoothers. Index `k` matches the TSV `index` column in `docs/re/kontakt-bindings/fx_bite_dirt_freak_wowflutter_twindelay.tsv` (WOW 0, FLUTTER 1, SCRAPE 2, SPEED 3, STEREO 4, AGE 5, SATURATION 6, GATE 7, MIX 8).

| k | param | invoke | reads | writes | law (constants from PE) |
|---|---|---|---|---|---|
| 0 | WOW | `0x14199c1d0` | ctrl `+0x4c` | `+0x32fc` | `wow² · 4.0` (`DAT_1447149e4`) |
| 1 | FLUTTER | `0x14199c1f0` | ctrl `+0x50` | `+0x3300` | `flutter² · 0.5` (`DAT_14470bad0`) |
| 2 | SCRAPE | `0x14199c210` | ctrl `+0x54` | `+0x930` | `scrape · 0.5` |
| 3 | SPEED | `0x14199c230` | ctrl `+0x58` | `+0x3304` | `speed · 0.75 + 0.05` (`DAT_144714900`, `DAT_1447148c0`) |
| 4 | STEREO | `0x14199c260` | ctrl `+0x5c` | `+0x3308` | `stereo²` |
| 5 | AGE | `0x14199c280` (463 B) | ctrl `+0x60` | `+0x3280`, then a loop | `age · 1.2` (`DAT_144f388bc`); loop over `this+0x940 + j·0x14a0`, j=0,1, per-channel drift state (see open items) |
| 6 | SATURATION | `0x14199c450` | ctrl `+0x64` | calls `FUN_14199e660(this+0x680, sat)` | sets `+0x2c04 = sat`, `+0x2c0c = 1 + (DAT_144fd1c58 - 1)·sat` and per-tap filter coefs (`FUN_14199db20`); full law open |
| 7 | GATE | `0x14199c470` | **`+0x31c`** (not `+0x68`) | `+0x3318` (flag), zeroes `+0x32e4` on change | `flag = (0.5 <= +0x31c)` |
| 8 | MIX | `0x14199c4a0` | ctrl `+0x6c`, clamped to [0,1] | `+0x330c`, `+0x3310` | `wet = sqrt(m)`, `dry = sqrt(1 - m)` (equal-power, not linear) |

Consequence for the port: the current generic law in `wowflutter.rs` (linear `wet`/`dry`, `wow` linear, sine excursions in ms) disagrees with the decoded control laws on every mapped parameter (quadratic wow/flutter, `sqrt` mix). The unconfirmed comment stays until vectors exist.

Open inside (a): the GATE handler reads `+0x31c` while the ctrl slot is `+0x68`; one of the two readings is wrong or `+0x31c` is a second copy. Not resolved.

### Process kernel `FUN_14199d080` (verified from decompile, constants from PE)

- Smoother `+0x3290 = f` is the speed-derived rate multiplier (`this+0x3304` after smoothing).
- LFO entries `E0 = this+0x6f0`, `E1 = this+0x808` (stride 0x118, loop `!= this+0x920`):
  - `E.+0x60 = f · E.+0x1c · 2π · (1.0 / srate)`, with `2π` = `DAT_1447149b8` (double), `1.0` = `DAT_144714988` (double), `srate = this+0x920`.
  - `E.+0x50 = cos(E.+0x60) · E.+0x68`, `E.+0x58 = sin(E.+0x60) · E.+0x68` (via `FUN_1443c9bd0`; the cos/sin return-register order is assumed from the decompile, not yet proved).
  - `E.+0x90 = f · E.+0x20 · 2π / srate`; `E.+0x80`, `E.+0x88` = cos/sin of that `· E.+0x98`.
  - Integer tap delay `E.+0x24 = (int)(srate · 4.0 / f)`.
- `this+0x928 = smoothed(+0x32fc) / f`, `this+0x92c = smoothed(+0x3300) / f`.
- Per sample: `FUN_14199cc10(this+0x680, inL, inR)`, then `out = y·wet·g + x·dry` with `wet = +0x330c`, `dry = +0x3310`, `g = 1` unless the gate flag `+0x3318` is set (level-detector gain `+0x32e4`, release/attack in `+0x32f0/+0x32f4`).

### (c) resolved: `FUN_14199ae00` is not a feedback matrix

Disassembly (`0x14199ae00`..`0x14199b1c4`, 965 B, `ret` at `0x14199b1c4`): input `__m128` at `[rcx]`, output `__m128` in `xmm0` (the cache's `void` return is wrong; callers `FUN_14199c870` and `FUN_14199d080` read `extraout_XMM0`). It is a 4-lane sine/cosine kernel:

- `|x|` via `andps` with `0x144714b90`; reduction `k = round(|x| · c)` with the magic-add trick (`0x1476a3770`), `r = |x| - k·π/2` in two parts (`0x1476a37b0`).
- Polynomial in `r²` (`0x1476a3790`, `0x1476a3720`, `0x147696dd0`, `0x144714b70` ...), blended by quadrant mask (`pslld 30`, `cmpeq` on `0x144f38ef0`), sign from `x` xor quadrant (`0x144714ba0`).
- Any lane with `|x| >= ~26000` (`0x46cadc00`) falls to a Payne-Hanek path using the 2/π table at `0x1476a3390` (integer `imul` chain, `shl/shr 0x3e`).

So the wow kernel's 4-lane sines come from this SIMD `sincos4`. Its polynomial coefficients are at the `0x1476a37xx`/`0x1476a36xx` constants and need a PE dump for bit-exact replication (open).

### Still open (blockers, exact addresses)

1. **(b) fractional readers.** `FUN_14199cc10` calls `(**(code**)(this+0x680+0x30))` and `(**(code**)(this+0x680+0x68))` with `double` positions, i.e. function pointers stored at `this+0x6b0` and `this+0x6e8`. Their writers are not in `0141990000.c`; `this+0x680` is a `FUN_14199e660`/`FUN_14199e150`/`FUN_14199d6a0` object. Next: search the cache for stores to `+0x6b0`/`+0x6e8` on that object type (`0141970000.c:680`, `0141aa0000.c:7786` read the same fields for another type).
2. **LFO entry init.** `E.+0x68`, `E.+0x98`, `E.+0x1c`, `E.+0x20` (depth and base frequency) are not written in `0141990000.c`; the stride-0x118 entries are inline in `this`, so their init is in a constructor or zeroing loop not yet located (`FUN_14199d6a0` zeroes parts of `this`, not these fields).
3. **Age loop** `0x14199c280` inner body calls `FUN_14199dbf0` and `FUN_14199db20` (smoother and filter), not decoded.
4. **Saturation** `FUN_14199e660` filter coefficients (`FUN_14199db20`), not decoded.
5. **Sin/cos core** `FUN_1443c9bb0` (double) and the return-register order of `FUN_1443c9bd0`: not decoded.
6. **KEMU vectors.** No native vectors. `tools/re/kemu_timemod.py` seeding of the nine control targets is not attempted.

Until (1), (2) and (5) are decoded, the generic law in `dsp/timemod/wowflutter.rs` stays **unconfirmed** and the PARITY_MATRIX Wow/Flutter row stays not-parity.

## Pass 5 (2026-10-23): step 1 merged, gate writer resolved, rest still open

Step 1 (code, PR #463, merged a798e72): the resolved control laws (wow `4w²`, flutter `0.5f²`, speed `0.75s+0.05`, stereo `s²`, mix `sqrt(m)` / `sqrt(1-m)`) are in `wow_flutter_laws` (sampler-kontakt) and the mix in `config`/`slot_timemod`. The PE constants (4.0, 0.5, 0.75, 0.05, 1.2) were re-read from Kontakt8.exe `.rdata` and match. The pitch scale is still generic and flagged; the IR fields carry the law values.

### Gate (blocker, now decoded): the flag reads an int written as a sample count

- Handler `0x14199c470` (radare2, `pdf @ 0x14199c470`): `rax = [rcx+8]` (owner), `movss xmm0, [0x14470bad0]` (0.5), `comiss xmm0, [rax+0x31c]`, `setbe cl`, then `cmp [rax+0x3318], cl`; on change, `[rax+0x32e4] = 0` and `[rax+0x3318] = cl`.
- So the gate reads **`+0x31c` as a float**, not the control slot `+0x68`. The earlier "(gate reads +0x68)" note was wrong; the cache decompile (`0141990000.c:8788`) agrees.
- Writer of `+0x31c`: `FUN_14099aac0` (`0140990000.c:9075`, line 9188), called with kernel `+0x1c0` (`0140970000.c:980/988`). It stores an **int** sample count: `max(1, int(ms·1e-3·rate/… + 1))`, bits ~1..1e3.
- A tiny int reinterpreted as a float is far below 0.5, so `0.5 <= bits(+0x31c)` is false on every reachable path. **Consequence: the Gate flag is always 0 in the decoded path, so the Gate control has no audible effect there.** Not yet confirmed by a native vector (no vectors exist).
- Not claimed: whether the owner pointer for handler 7 is the kernel (assumed from the `FUN_14199de70` capture of `this`).

### Still open after pass 5

1. **Fractional readers** at kernel `+0x6b0`/`+0x6e8` (object `+0x680`): no store to these offsets in `0141990000.c`, `0141970000.c`, `0141aa0000.c`; the `+0x680` init (`FUN_14199d6a0`, `FUN_14199e150`) writes other fields. Writers are elsewhere (probably the object's ctor via a generic setter). Not located.
2. **LFO entry init** (`+0x68`, `+0x98`, `+0x1c`, `+0x20`): not written in the file set above; still open.
3. **Age loop** `0x14199c280`: calls `FUN_14199dbf0` / `FUN_14199db20`, not decoded.
4. **Saturation** `FUN_14199db20` coefficients: not decoded.
5. **Sin/cos core** `FUN_1443c9bb0`: the cache decompile (`01443c0000.c:11187`) is an SSE body with no readable semantics; `FUN_1443c9bd0` (`:11538`) is a `|x| >= threshold` range-reduction wrapper. Needs a disassembly pass (`pdf @ 0x1443c9bb0`); not done in this lane.
6. **Step 3 (KEMU vectors)**: not attempted. No native vectors exist, so the PARITY_MATRIX Wow/Flutter row stays `partial`.
