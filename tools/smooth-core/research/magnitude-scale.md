# Magnitude scale: what is pinned and what is not

Status: **not pinned.** The port's `frame()` still uses the raw `norm()` of the
windowed FFT bin. Nothing below proves that the reference uses this scale.

## What the reference binary shows

Restored image: `/tmp/opencode/rea/soothe3-restored.bin`, sha256
`0143f03b…bbcfa64`, image base `0x180000000`.

- **The FFT is not an imported library.** The import table has 38 entries, all
  from KERNEL32, USER32, ADVAPI32 and SHELL32 (`r2 -q -c iij`). There is no
  `ipps*` import. Intel IPP is statically linked into a section named
  `IPPCODE` (RVA `0x1cfa000`, VA `0x181cfa000`, 0xb5000 bytes), plus `IPPDATA`.
- **The process path** is `fcn.18088f4f0` (VA `0x18088f4f0`, the decompile in
  `/tmp/opencode/rea/_proc_88f4f0.c`). It calls the thunks `0x180001040`,
  `0x180001190`, `0x180001220`, `0x180001340`, `0x180001370`,
  `0x1800010a0/10d0` and `0x1801c84d0`, plus several `0x18089xxxx` helpers.
  Those thunks dispatch through tables. The index slot for `0x180001040` at
  `0x1821ed508` holds `-1` and has no writer in the image (see
  `rea-8ab6b0.md`), so the runtime target of each thunk is unobserved.
- **No FFT size, scale flag, or 1/N constant was connected to the magnitude.**
  The FFT init call (`ippsFFTGetSize`/`InitAlloc`) and its flag argument were
  not located. Without them the scale is fixed by nothing observed.

## What the port's constants do (reference-derived formulas)

These are the reference formulas as ported in `src/dsp.rs`. They are absolute
in the magnitude `m`, so the magnitude scale changes the behavior:

- soft target: `-ln(max(m * weights, 1))`
- hard target: `-(0.25 + detail^4 / 4) * ln(m^2 * weights + 1)`
- attenuation floor: `ln(1e-3)`
- hard conditioning floor: `peak * 10^((20*detail - 60)/20)`

The `+1` in the hard target and the `max(…, 1)` in the soft target are absolute
constants. Any scale choice moves the level where they bite.

## Level probe (reduced-latency plan, n = 512, 1 kHz tone, depth 0.8, detail 0.4)

Measured with `tools/smooth-core/src/dsp.rs`, module `level_scale`:

| level dBFS | soft reduction dB | hard reduction dB |
|-----------:|------------------:|------------------:|
| -60 | -1.379 | -0.062 |
| -40 | -1.520 | -0.781 |
| -20 | -1.539 | -2.016 |
|   0 | -1.541 | -3.299 |

- Soft is level-invariant from -40 to 0 dBFS to 0.021 dB. The test pins this.
  At -60 dBFS the magnitude approaches the unit floor, so the reduction shrinks.
- Hard is **not** level-invariant over any range that includes the unit floor.
  The `+1` in `ln(m^2+1)` makes it absolute. A ±0.1 dB requirement across
  -60..0 dBFS for hard mode cannot hold under any linear magnitude scale
  unless every bin stays far above 1 at -60 dBFS (a few hundred times the current
  magnitude). Nothing in the reference evidence supports that scale, so the
  requirement as written conflicts with the reference formulas. Either the
  reference normalizes the input level first (not observed), or the level
  requirement applies only to soft mode.

## What would settle the scale

1. Find the IPP FFT init. Locate the `IPPCODE` entry that `fcn.18088f4f0`
   reaches through `0x180001190` (or `0x180001220`), then find the caller that
   passes the FFT length and a flag. The flag (`DIV_FWD_BY_N`, `DIV_INV_BY_N`,
   `NODIV_BY_ANY`) fixes the forward scale.
2. Find the sqrt-Hann (or other) window construction. It may be a table in
   `.rdata` or a loop in `fcn.18088f4f0`.
3. Find the multiply, if any, between the FFT output and the detector's
   `ln` input. The `0x1801c84d0` call with a broadcast constant in XMM3 is the
   candidate, and its constant is not yet read.

Until 1–3 are done, the port keeps raw `norm()` and the level test covers soft
mode only.

## End-to-end probe, default settings (`resonances_are_reduced_and_the_rest_is_left_alone`)

Default `EngineConfig` (n = 4096) and `Settings::default()` (soft, depth 0.35,
detail 0.5). Input: 1 kHz at 1.0, 3 kHz at 0.5, 300 Hz at 0.3, 10 kHz at 0.3,
plus hiss at ±0.025, all scaled by the level. Reduction in dB, steady state.

| level dBFS | 1 kHz | 3 kHz | 300 Hz | 10 kHz |
|-----------:|------:|------:|-------:|-------:|
| -60 | -0.30 | -3.01 | +0.00 | -8.24 |
| -40 | -0.30 | -3.37 | +0.00 | -10.98 |
| -20 | -0.30 | -3.37 | +0.00 | -10.98 |
|   0 | -0.30 | -3.37 | +0.00 | -10.98 |

- Off-resonance (300 Hz) is untouched.
- The 1 kHz steady tone is **only 0.3 dB down**, the largest tone in the mix.
  This does not meet the brief's "clearly audible reduction at resonances".
  The likely cause is the default weighting, which is not yet checked against
  the reference.
- The 10 kHz tone takes the most reduction (11 dB), where the final weight
  opens (see the note in `the_hard_detector_attenuates_where_the_final_weight_opens`).
  A steady tone is a peak in every envelope, so it is pulled down wherever the
  weights let it. Whether the reference behaves this way is unverified.
