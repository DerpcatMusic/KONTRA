# Conflux saved digital LFO: factory and unsynced fade binding

Verified against **Kontakt 8.13.1 standalone**, image base `0x140000000`, SHA256
`0fe6356e0879d058b6e5b73507c54c5e345cea451b35287c974e438291d4dae8`.
These addresses do not identify the VST3 image.

**The saved unsynced delay is raw milliseconds.** The actual engine setup uses
the digital LFO implementation for enum 6, computes its fade count at
`sample_rate / 32`, and copies the v73 additional flag into the fade mode.
This supports the bounded saved, single-component admission W6 requested.
Transport sync and generic source scheduling remain separate claims.

## Proof chain and exact addresses

| Fact | Native evidence |
|---|---|
| Saved enum is preserved at `BParLFO + 8` | v73 reader `0x140d03660`; [pseudocode line 4720692](../../../artifacts/engine-analysis-2026-10-07/kontakt-engine/complete-export/pseudocode.c#L4720692) |
| `BParLFO` is embedded at `BParInternalMod + 0x550` | constructor `0x1407033b0` writes the BParLFO vtable `0x144e74a78` at `+0x550`; [line 3153159](../../../artifacts/engine-analysis-2026-10-07/kontakt-engine/complete-export/pseudocode.c#L3153159) |
| Actual engine factory reads that same enum at `+0x558` | function `0x140973970`, selector `0x140974c6a`; jump table `0x14097869c`; enum 6 branch `0x140975220` |
| Enum 6 with one target reaches the digital constructor | `0x140975242` calls `0x1407c2110`; constructor installs `0x144729c08`, native RTTI `BLfoMultiDigitalWithTargets<1>`; slot 6 is `0x140b078d0` |
| Enum 5 selects a separate constructor | `0x140975153` calls `0x1407c2d10`; final table `0x144729a78`, `BLfoMultiWithTargets<1>` |
| Initialization uses a control rate of `sample_rate / 32` | `0x14097411c` loads `0.03125f`; `0x140976465–0x140976471` multiplies the engine rate by it and calls virtual init; `0x1405ed630` calls `0x1408dcd90`, which stores the resulting rate at runtime `+0x14` |
| Unsynced delay getter preserves milliseconds | `BNoteValueTime` table `0x144e74260`, slot 5 `0x14072b380` → `0x140a74bf0`; its inactive-note branch returns `XMM1` unchanged |
| Actual saved-delay setup passes `initial[0]` to that getter | `0x140977d3a–0x140977d41` resolves slot 5 of the saved delay object's native vtable; `0x140977d73` loads saved descriptor `+0x55c` (= LFO `+0x0c`); `0x140977d82` calls the getter |
| Fade count and multiplier arguments | `0x140977d85–0x140977dbd`; count at runtime `+0xf8`, f32 multiplier at `+0xf4` |
| Saved v73 additional flag selects fade mode | `0x140977de0–0x140977de7` copies saved `+0x56d` (= LFO `+0x1d`) to runtime `+0xe5` |
| Phase, weights, normalization remain saved values | `0x140977dc5–0x140977dd8`, `0x140977e57–0x140977eae` |
| Reset initializes the fade | `0x1405f45a0`: accumulator `+0xf0 = 0.3f`, remaining `+0xfc = total +0xf8`, phase `+0xb0 = saved +0xe8` |
| Fade application and clamp | original digital kernel `0x140b078d0`; [pseudocode line 3984946](../../../artifacts/engine-analysis-2026-10-07/kontakt-engine/complete-export/pseudocode.c#L3984946) |

The actual factory/setter routine is present in the original disassembly but is
not available as a whole function in the retained pseudocode export. The
[compact instruction receipt](native-binding.asm) preserves the relevant bytes
and addresses. The factory proof does not depend on editor preview
`0x140605430`.

## Unsynced admission laws

The sync guard is the native `BNoteValueTime + 8` active-note byte, **not** the
packed record flag. Native reader `0x140ab5da0` sets that byte when the delay
note value exceeds `9.999999717180685e-10f` (constant `0x144737708`). With the
current public parser's crossed record grouping, the delay note value is
`records[1].values[0]`. Require finite values and an inactive delay note for
this admission. Frequency has the same active-note test on
`records[0].values[0]`; its unsynced getter preserves saved Hz.

For nonnegative, finite delay and a positive rate, the exact count calculation
is:

```text
control_rate = f32(sample_rate * 0.03125f)
N = trunc_i32(f32(f32(saved_ms * control_rate) * 0.0010000000474974513f))
factor = f32(pow(4.333333200878572, 1.0 / double(N)))
```

Truncation is floor for these nonnegative inputs. Preserve the two f32
multiplications; a single f64 `floor(ms * rate / 32 * .001)` can differ at count
boundaries. The pow base equals `1 + 1 / double(0.30000001192092896f)`.
The native setter calls the CRT pow entry `0x1443d10ae`; these probes check the
original count and pow arguments, but do not execute that external CRT call.

For each of the first `N` generated control values, the native kernel performs:

```text
gain = f32(accumulator - 0.30000001192092896f)
output = f32(output * gain)
accumulator = clamp(f32(accumulator * factor), low, high)
```

Reset starts the accumulator at `0.3f`. False/legacy additional flag uses
`[low, high] = [0, 1]`; true uses `[0.3f, 1.3f]`. Gain is applied **before**
updating the accumulator. Countdown decreases by the number of affected
values. Once it reaches zero, subsequent outputs are unfaded. Consequently,
legacy false can cap the faded gain near `0.7`, then return to full gain when
the countdown expires. For `N = 0`, the kernel skips fade application.

Native Hz setter `0x14095ecd0` stores
`double(f32(saved_hz * inverse_control_rate))` at runtime `+0xb8`.
This verifies the scalar setter, not when the generic engine schedules source
generation. The kernel also has a separate normalized-frequency-buffer law;
do not substitute that for the saved-Hz setter.

Saved mix order is sine, rectangle, triangle, saw, random. Runtime fields are
`+0x128`, `+0x134`, `+0x130`, `+0x12c`, `+0x138`, respectively. The engine copies
the original weights unchanged, including non-unit weights. Normalization
is copied from saved `records[0].flag` to runtime `+0x13c`. Use W6's
[existing waveform receipt](/mnt/Windows11/DEV_WORKSPACE/kontra-runs/w6-conflux-digital-lfo/native-waveform.json)
for the measured sine and normalization law.

## Reproducible checks and boundary

Run `python3 docs/research/conflux-lfo-20261009/verify_native_binding.py`.
It hash-pins and emulates original x64 instructions without helper stubs or
launching a host. [native-binding.json](native-binding.json) records:

- Six enum/target-count factory paths, including distinct 5 and 6 constructors.
- **10 reader sync-guard cases**, including the native threshold and its next
  float32 value, with both packed flags.
- **18 saved-delay binding cases** execute the actual saved-field load, native
  vtable lookup and getter together through both tempo-source branches. The
  unsynced result is exactly the saved float32 milliseconds.
- Nine unsynced saved delay values at three rates: **27 exact native counts**,
  including zero and the control-point boundary.
- **18 exact saved-Hz increment setters**.
- Native saved phase, extra-flag, mix-order, and normalization copies.
- **294 bit-exact fade outputs** across counts 1, 2, 5, 24, 100 and both flag
  values, compared with the same native waveform without fade. The multiplier
  is authored using the verified pow arguments; no new waveform-shape claim
  is made.

Retain live diagnostics and existing single-component/route admission guards.
The receipt does not certify the whole engine, synced tempo behavior, live
parameter routes, generic control scheduling, or other waveform components.
No Cargo build was run and no engine or vendor source was changed here.
