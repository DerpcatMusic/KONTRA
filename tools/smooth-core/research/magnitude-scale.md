# Magnitude scale: what is pinned and what is not

Status: **not pinned.** The port's `frame()` still uses the raw `norm()` of the
windowed FFT bin. Nothing below proves that the reference uses this scale.

Update (re-latency pass): the FFT spec constructor is located and decoded; see
`re-latency-fft-scale.md`. The complex forward is unscaled (IPP flag 8), but the
real leg is scaled by `1/N2` in software, and which leg feeds the detector is still
open. Section "Resolved (partial, 2026-10-21)" below is superseded where it says the
constructor was not found.

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

## Resolved (partial, 2026-10-21): the forward scale is still not pinned

Question: what scale turns the windowed FFT output into the `m` the detector
consumes. **Answer: not recovered.** The FFT engine is now located, the spec
layout is known, and the scale constant is not. Our raw `norm()` stays. No
evidence shows it is wrong, so `frame()` is unchanged.

### Corrections to the notes above

- **The thunk selector does have writers.** The old note said
  `0x1821ed508` had no writer. The sweep index (`xrefs` on `target_va =
  0x1821ed508`) shows two: `0x2fb89f` (`mov dword [0x1821ed508], 0`) and
  `0x2fb8ca` (`mov dword [0x1821ed508], edx`), both inside
  `fcn.1802fb880`. That function is also called from IPPCODE at `0x1dae2a6`.
  Its `edx` comes from `fcn.1802fbb00`, and it falls back to `0x14` when
  `fcn.181dae648` fails. The selector is a per-CPU/threading dispatch, not an
  FFT-specific switch.
- **The thunks are generic.** `0x180001040/…/0x180001490` each do
  `movsxd rax, [0x1821ed508]; lea r10, [table_k]; jmp [r10 + rax*8]`. They have
  22–137 direct callers across the binary, so they are not FFT-specific.
  Table entry `-1` of the first thunk is `0x180001030`, a stub.

### The FFT engine (located, `.text`)

Sweep index: direct `.text` calls into IPPCODE reach 150 entry points.

- **Wrapper `fcn.180001640`** (args `rcx=src, rdx=dst?, r8=spec`; `rdx`/`rsi`
  swapped at entry). Its prologue:
  ```c
  if (spec == NULL || src == NULL || dst == NULL || tmp == NULL) return ...;
  if (*(int*)spec != 7) return -13;            // spec ID check
  int order = *(int*)(spec + 4);               // log2 N
  if (order > 4) goto big;                     // 0x180001712
  if (*(int*)(spec + 8) != 0)                  // scaled flag
      ((void(*)(...,float))table_scaled[order])(src, dst, *(float*)(spec + 0x10));
  else
      table_unscaled[order](src, dst);
  ```
  `table_scaled` is `0x181e16ab0 + 8*order` (entries `0x18000f1c0 …`);
  `table_unscaled` is `0x181e16ae0 + 8*order` (entries `0x1800101a0 …`).
  Orders 5..6 use the tables `0x181e16b08`/`0x181e16b38` (`0x180001741–0x180001770`).
  Orders 7..16 call `fcn.181cfd8f0` (IPP kernel, `0x180001815`). Order 17 calls
  `fcn.181cfb130` (copy) and then `fcn.181cff62e`. Above 17 it calls `fcn.18000da80`.
- The wrappers are reached through the `.data` table at `0x1821ebe38`: the
  entries are `0x1640` (`0x1821ebe38`), `0x1900` (`0x1821ebe40`), `0x1c00`
  (`0x1821ebe48`), `0x7000` (`0x1821ec178`), `0x72e0` (`0x1821ec180`). With the
  thunk base `0x1821ebe78`, selector `-8` gives `0x1640`.
- **The scale lives in the spec.** `spec+0x10` is a float, passed as `xmm2`
  only when `spec+8 != 0`. That is the only place a forward scale can enter the
  reference transform. Its value is set at spec construction. **Not located.**

### What I tried that did not find the constructor

- Byte scan of `.text` + IPPCODE for `mov dword [R], 7` with `dword [R+4]` and
  `dword [R+8]` stores within ±60 insns: 0 hits.
- `movss [R+0x10], xmm` with `[R+4]`, `[R+8]`, `[R]` stores within ±80 insns: 42
  hits. Checked `0x180594d5b` (a cached-parameter node, ID `1`, callback via
  `r9`) and `0x180988de5` (a 4-wide linear transform). Neither is an FFT spec.
- IPP entry classification (`pdfj` on the 150 targets): kernels with
  `shufps/unpck/addsubps/mulps` (FFT codelets) at `0x181cfd8f0`, `0x181cff62e`,
  `0x181d01740`, `0x181d03480` and similar. Copy routines (`movups/movaps`) at
  `0x181cfab80`, `0x181cfb130`. Each `.text` wrapper calls the same
  copy–kernel–kernel–copy group, so the families are per-order variants.
- **No sqrt or magnitude routine** is called from the process path. The decompile
  (`_proc_88f4f0.c`) has no `SQRT` and no float literals; its constants are
  loaded from `.rdata`. The magnitude is either inside an IPP entry or
  computed in a `.text` helper I did not classify.

### Not established

- The FFT spec constructor, so the `spec+0x10` value and `spec+8` flag are unknown.
- Which IPP flag (`IPP_FFT_DIV_FWD_BY_N=1`, `DIV_INV_BY_N=2`, `DIV_BY_SQRTN=4`,
  `NODIV_BY_ANY=8`) the reference passes. Without the constructor the flag
  cannot be read.
- The window table and the log2 N per quality plan.

### Sensitivity if the reference differs by a constant factor `c`

Our raw `|X|` for a sqrt-Hann window (coherent gain `2/π`) gives
`m(A) ≈ A·N/π` for a sine of amplitude `A`. That is about 1303 at 0 dBFS for
`N = 4096`, and about 1.3 at -60 dBFS, which matches the level probe where the
soft reduction starts to shrink. If the reference scales by `c`, then the unit
floor `max(m,1)` and the `+1` in `ln(m²+1)` move to level `-20log10(c)` dB.

| reference scale `c` (N=4096) | `m` at 0 dBFS | `m` at -60 dBFS | effect |
|---|---:|---:|---|
| 1 (ours, raw) | 1303 | 1.3 | floors bite near -60 dBFS |
| 2/N ≈ 1/2048 | 0.64 | 0.0006 | unit floor active from about -66 dBFS up |
| 1/N = 1/4096 | 0.32 | 0.0003 | unit floor active at all levels; soft reduction collapses |
| 1/√N = 1/64 | 20 | 0.020 | floor moves about 36 dB up |

Hard mode is more sensitive: with `c = 1/N`, `ln(m²+1) ≈ m²` drops to about
0.1 nats at 0 dBFS, so the `+1` dominates. That changes the hard threshold by
more than an order of magnitude. Level invariance of the reference, if real,
would favour a scale that keeps `m ≫ 1` at ordinary levels, which argues
against `1/N`. That is an inference from the formulas, not from the binary.

### How to settle it

1. Find the spec constructor. It writes `spec[0]=7`, `spec[4]=order`,
   `spec[8]=flag`, and `spec[0x10]=float` together. Search the constructor
   call sites of `0x1640/0x1900/0x7000/0x72e0`'s callers, or the
   `0x1821ebe38`-table readers in the `.text` process path, rather than
   patterns on the wrapper.
2. Read the spec's `spec[0x10]` value at construction, or the IPP init flag
   (`ippsFFTInit_R_32f` flag argument) passed alongside it.
3. Only then compare with our `frame()`. If the value is `1/N` or `1/√N`, change
   the one line in `frame()` and rerun the level probe.

Scratch used (under `/tmp/opencode/rea/cls` and `/tmp/opencode/rea/fft`, text
dumps and scripts): `dump.txt`, `pdfj_all.txt`, `scan_init2.py`,
`scan_spec7.py`, `scan_spec_reg.py`. Move them to the dev drive if they are kept.
