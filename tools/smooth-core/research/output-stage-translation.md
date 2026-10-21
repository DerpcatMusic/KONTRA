# Output stage of the process block: translation notes (0x890a00 output side)

Scope: `0x890a00..0x893bd7` output side, the gain helper `0x899ab0`, the setter fields it reads, the
M/S helper `0x8a3b20`. Code: `src/reference/output.rs`. Evidence: emulated binary via
`re-tools/emulate/reference_output_fixture.py` (`ramp` and `targets` modes).

## Verified (emulation, bit-exact unless stated)

- **Gain helper 0x899ab0.** Record `+0` target `t`, `+4` current `g`, `+8` coefficient `a`. Gate: outer
  skip when `|t-1| < 1e-3` and `|t-g| < 1e-3` (g not snapped). Zero path when `|t| < 1e-3` and
  `|g| < 1e-3`: channels zeroed through `0x1800010d0`, g not stepped. Otherwise per sample
  `g = (t-g)*a + g`, then `sample *= g` on every channel. Fixture: 573 cases, 3 rates, 1 to 5
  channels, gate boundaries, n = 0.
- **Ramp time base.** `a = 1 - expf(-1/(0.03 * fs))`, constructor `0x88fa90`. Constant
  `-33.333336` at `0x18209da5c`. The base is the sample rate because the helper steps per sample.
- **Gate and abs constants.** Gate `1e-3` at `0x18209d03c`; abs mask `0x7fffffff` at `0x18209ee00`.
- **Targets (0x891e0e..0x891ed8), W = wet trim, M = mix, O = out trim, D = delta, B = bypass,
  S = solo, cl = delta_mode:**
  - `wet = B ? 0 : (cl ? -(W*M) : W*M)` at `+0xbf34`
  - `dry = B ? 1 : (cl ? W*M : 1 - M)` at `+0xbf28`
  - `out = B ? 1 : O` at `+0xbf58`
  - `main = S ? 0 : 1` at `+0xbf40` (float, from `cvtdq2ps`)
  - `solo = S ? 1 : 0` at `+0xbf4c` (float)
  - `cl = D || (channels > 2 && any byte +6 of records 0x20c+8k, k<5) || (S && byte +0x236)`
  Fixture: 32 cases through the setter, see `targets`.
- **Setter fields (0x894370).** Case `0x5d` (93) wet trim: `+0x94 = powf(10, dB*0.05)`. Case `0x5e` (94)
  mix: `+0x98 = pct * 0.01`. Case `0x5f` (95) out trim: `+0x9c = powf(10, dB*0.05)`. Case `0x60` (96)
  delta: `+0xa8`, `value >= 0.5`. Case `0x61` (97) bypass: `+0xa9`, `value >= 0.5`. Case `0x0e` (14)
  sidechain: `+0xa5`. Case `0x10` (16) M/S: `+0xa6`. Case `0x0f` (15): `+0xa7`. Case `0x5b` (91) solo band:
  `+0x100`, value minus 1, clamped to 7.
- **Setter normalisation.** `x` is clamped to `[0, 1]`, passed as a double to the descriptor callback
  `[desc+0x50](desc, x)`, which returns a double. The result is clamped to `[desc+0x28, desc+0x30]`. The
  decompile loses the `afStackX_10[2] = x` store; the asm (`0x894370`) shows it.

## Read, not verified

- **M/S helper 0x8a3b20.** `dst = (g(s0+s1), g(s0-s1))`. Encode `g = 0.5` at `0x1808928a3`; decode
  `g = 1.0` at `0x18089a5bc`. Matches `stereo_pair_probe.py` (error < 1e-7, not bit-exact).
- **Focus weights** `gL = clamp(2(1-f))`, `gR = clamp(2f)` (`0x180890c92`). From the decompile, not
  re-checked here.

## Open (not translated)

- **Dry/wet summation and the write to the output table.** The region `0x891fxx..0x893bd7` is
  stack-aliased (about 1,300 decompile lines) and calls `0x899cd0`, `0x89a1a0`, `0x5e9a0` and the
  copy/zero wrappers. `process_output_block` is therefore not provided.
- **Band-solo filter.** `0x89a1a0` per-sample, `0x89bf10` coefficient store (table in
  `stereo-ms-sidechain-output.md`). Coefficient smoothing toward its target (one-pole rate
  `*(record+0x38)`) is not decoded.
- **Sidechain meter** (`sidechain_input_level`, host id 13) written near `0x893928`: not read.
- **Setter case `0x5b` edge case.** The decompile gives value 0 -> index 7 (unsigned `uVar11 - 1`).
  Not checked in the asm; the fixture only uses 0 and 1..8, and the Rust side does not model it.
- **Sidechain listen (`+0xa7`)** and **`+0x236`** setter: not decoded (the fixture writes `+0x236` directly).
- **Clipping**: none found in the translated region; not checked in the summation.
