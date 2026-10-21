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

## Solo path (translated: `solo.rs`, `process_output_block`; fixtures `solo_filter.json`, `output_solo.json`)

Entry points: `0x89a1a0` (filter, Tilt copy), `0x89bf10` (mode and targets), `0x88e150` (TPT
coefficients). Records and state are per channel, 0x78 bytes at `obj+0xb7a8`.

- **Solo band.** The process block reads the solo index from `obj+0x100` (i32, `-1` = off) and
  copies it to `obj+0x4c`. `solo_active` is the band's `used` byte (`+0x10c + 0x20*idx`), and
  `+0xaa` holds it. The band record passed to the filter is `obj+0x10c+0x20*idx`.
- **Shape field is `+0x1c`, not `+0x18`.** The filter and `0x89bf10` read the shape at `+0x1c`,
  which the setter fills from index 27 (`selector`). Index 26 (`shape`, `+0x18`) does not change
  the solo filter. The constructor leaves `+0x1c` at 2 (bell) for band 1. Verified by probes
  (`0x124` written by 26, `0x128` by 27, solo reads `0x128`). The port has to decide whether
  `selector` is the solo shape in SMOOTH; the fixture drives index 27.
- **Frequency `+0x04`, half bandwidth `+0x0c` (octaves).**
- **Mode and targets (`0x89bf10`).** Shapes `{2,3}` -> mode 2 (band-pass), `{1,7}` -> 0 (low-pass),
  `{0,6}` -> 1 (high-pass), `{4}` -> 3 (notch), `{5}` -> copy (no state). For filter modes:
  `t = fs * 0.499`, `A = max(f, 1e-6)`, `R = (t >= f) ? A : t` (target `+0x10`); target k
  (`+0x18`) is `max(hb, 0)` for modes 2 and 3 and `1.0` for modes 0 and 1.
- **Coefficients (`0x88e150`, per sample).** `f = clamp(R, 1e-6, 0.499 fs)`, `g = tan(pi f / fs)`,
  `hb = max(k, 0)/2`, `lo, hi = g * 2^{-hb}, g * 2^{+hb}` (`exp2` of `log2(g) ∓ hb`),
  `k = 1 / max(g / (hi - lo), 1e-9)` (or `1 / 1e-9` when `hi - lo <= 1e-30`, where the binary uses
  `1e9`), `a1 = 1/((k+g)g + 1)`, `a2 = a1 g`, `a3 = a2 g`. Stored at `+0x40` (g), `+0x48` (k),
  `+0x50..0x60` (a1..a3).
- **Smoothing (`0x89a1a0`, per sample, per channel).** `R += (tR - R) * rate` and
  `k += (tk - k) * rate` with `rate = +0x38` while `rate < 1.0`; otherwise snap to the targets.
  The constructor sets `rate = 1 - exp(-1/(0.03 fs))` in f64 (`+0x30` holds 30, the ms value). The
  gain records use the f32 constant `-33.333336` instead, so the two rates differ at 1e-5.
- **TPT update.** `v3 = x - ic2`, `v1 = a1 ic1 + a2 v3`, `v2 = a2 ic1 + ic2 + a3 v3`,
  `ic1 = 2 v1 - ic1`, `ic2 = 2 v2 - ic2`, `hp = (x - k v1) - v2`. Output by mode:
  0 `v2`, 1 `hp`, 2 `k v1`, 3 `hp + v2`. Stored as f32.
- **Fixture.** `solo_filter.json`: 302 cases at 44.1 and 48 kHz, every shape, both channel
  counts, the copy branch with mismatched frames, rates 1.0 / 1.5 / 0.5 / 0.01 / 1e-4, and the
  constructor state. Outputs and state fields match bit-for-bit (max state error 1.5e-16 in
  the replay).
- **Solo input is the sum, not the wet block.** The filter reads `out` after the dry add, that
  is `W g_wet + D g_dry`, before the main ramp. This is exact in the trace (max difference 0)
  with trims. (An identical result with identity gains is why the first reading looked like
  the wet block.)
- **Order inside a sub-block (trace of `0x890a00`, `research/re-tools/emulate` probes):**
  wet copy and ramp; dry copy and ramp; `out += dry`; then if the solo path runs: solo filter on
  the sum, solo ramp (`+0xbf4c`) on the solo frame, main ramp (`+0xbf40`) on `out`, `out += solo`;
  then the out ramp (`+0xbf58`).
- **Solo gate (`0x8923a7`, per sub-block).** Runs when `!bypass && (solo_active || solo.g > 1e-3)`.
  The 1e-3 is `[0x18209d03c]`. Bypass skips the path entirely, so the main and solo records
  are not ramped in a bypassed sub-block.
- **Constructor records.** dry `(0,0)`, wet `(1,1)`, main `(1,1)`, solo `(0,0)`, out `(1,1)`,
  each with the ramp coefficient (`fixtures/output_solo.json`, `rec_in`).
- **Fixture.** `output_solo.json`: 5 cases, 7 blocks of N = 2600 (solo on, fade-out, trims, delta,
  bypass, Tilt). Output channels match to 1e-6 of each channel's peak; records and solo state
  match after each block. A mutation that disables the solo path fails the test.

## Open (not translated)

- **Wet and dry roles, with a non-identity wet path.** Still unconfirmed. Driving the band
  (depth set through the setter, bell at 1 kHz, Q 1, and the solo index) does not change the
  wet path in this stand-in: `wet == delayed input` to 7e-7 for every setting tried. The band's
  gains need the detector's host state (`0x619fc0`, bins and weights, see `re-latency-fft-scale.md`
  section 14). Until then the roles rest on the record assignment (wet record ramps the buffer
  copied from the processed side) and on the solo input, which is the sum above.
- **Sidechain meter** (`sidechain_input_level`, host id 13) near `0x893928`: not read.
- **Setter case `0x5b` edge case.** The decompile gives value 0 -> index 7 (unsigned `uVar11 - 1`).
  Not checked in the asm; the fixture only uses 0 and 1..8, and the Rust side does not model it.
- **Sidechain listen (`+0xa7`)** and **`+0x236`** setter: not decoded (the fixture writes `+0x236` directly).
- **Clipping**: none found in the translated region; not checked.
