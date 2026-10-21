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
- **Focus weights** `gL = clamp(2(1-f))`, `gR = clamp(2f)` (snippet `0x892b34..0x892bf7`): verified bit-exact on the isolated snippet (`sidechain.rs`). Which band's `f` and which buffers: open.

## Output sum (verified by emulation, `process_output_block`)

- **Sub-blocks of at most 512 samples.** The ramp is called once per sub-block: N = 512 for a
  4096 block (8 calls), N = min(512, remaining) in general (chunk counts checked at 8, 100, 300,
  512, 1024, 1500, 2048, 4096). Gate decisions are per sub-block.
- **Sum per sub-block** (`0x891da2` .. `0x8935d1`): `out = wet` (copy `0x893de0`); `out *= g_wet`
  (record `+0xbf34`, ramp `0x891f3e`); `scratch = dry` then `scratch *= g_dry` (record `+0xbf28`,
  ramp `0x892250`, scratch `0x40000040/0x40000080`); `out += scratch` (IPP add `0x180001370`,
  `dst += src`, `dst` = second argument); `out *= g_out` (record `+0xbf58`, ramp `0x8935d1`).
- **Record and state.** Constructor state: dry `(0, 0, a)`, wet `(1, 1, a)`, out `(1, 1, a)`.
  Mix 0 leaves dry ramping 0 -> 1 and wet 1 -> 0.058 after 4096 samples (matches `1 - (1-a)^n`).
- **Equation.** `out = g_out * (g_wet * W + g_dry * D)`, with per-sample ramps. Fixture:
  `fixtures/output_block.json`, 7 configurations (mix, wet and out trims, delta, bypass), 2600
  samples, compared to 1e-6 relative to each channel's peak.

### Caveat: the stand-in cannot tell wet from dry

With the constructor state and no band processing the emulated wet path equals the delayed input
(max |W - D| = 7e-7 at 48 kHz, latency 2304). So the fixture cannot distinguish the wet and dry
roles: swapping the two ramps gives the same output. The record-to-signal assignment above comes
from the code order (`0x891f3e` ramps the buffers that `0x893de0` copied from the processed side;
`0x892250` ramps the scratch copy of the delayed input). A non-identity wet path (band gains,
solo, or a changed FFT path) is needed to check it.

### Helpers identified

- `0x180001370`: `dst[i] += src[i]` (rcx = src, rdx = dst, r8 = n; r9 unused, xmm1 unused).
  Checked by emulation, with the selector-0 implementation at `0x9ea0`.
- `0x1800010a0` copy(src, dst, n); `0x180001070` mulC; `0x1800011c0` sub (`src2 - src1`);
  `0x1800015b0` add (`a + b -> dst`); `0x1800010d0` zero-fill. Verified by probes except `0x1070`.
- All IPP wrappers share the selector `[0x21ed508]`. Selector 0 is the one the harness pins.
  Elementwise add, mul and sub are exact at every selector, so bit-exactness does not depend on it.
- Host ids: `isset(9)` at `0x892dd3` (band solo channels), `isset(13)` at `0x8935e5`
  (sidechain meter).
- The writer analysis shows `0x899ca1` writes the output 4096 times per 4096-block, once per
  sample. That is the per-sample delayed-input writer. Not yet mapped to its function.

## Open (not translated)

- **Band-solo filter.** `0x89a1a0` per-sample, `0x89bf10` coefficient store (table in
  `stereo-ms-sidechain-output.md`). Coefficient smoothing toward its target (one-pole rate
  `*(record+0x38)`) is not decoded. `process_output_block` panics on `solo_active`.
- **Solo records** (`main +0xbf40`, `solo +0xbf4c`): their ramps are not in the fixture (solo off).
- **Sidechain meter** (`sidechain_input_level`, host id 13): translated in `src/reference/sidechain.rs` (absolute peak of the side per 512 sub-block, verified).
- **Setter case `0x5b`:** translated (`solo_band_index`): signed `min(round(v) - 1, 7)`, so 0 gives -1 (no solo). The decompile's 'value 0 -> index 7' is wrong.
- **Sidechain listen (`+0xa7`)**: translated (`listen_block`), verified emulated with the out-trim ramp. **`+0x236`** setter: not decoded (the fixture writes `+0x236` directly).
- **Clipping**: none found in the translated region; not checked.
- **Wet/dry role** (see caveat above).
