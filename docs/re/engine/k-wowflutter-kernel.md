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
