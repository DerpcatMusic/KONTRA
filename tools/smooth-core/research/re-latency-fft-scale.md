# soothe3 re-pass: latency fields and FFT forward scale

Status: **FFT spec constructor located and decoded. Forward scale for the complex
path is unscaled (IPP flag 8). Latency per-mode numbers: not recovered.**
No SMOOTH code changed by this pass; the evidence does not yet pick the detector's
magnitude path (see "Open").

Image: `/tmp/opencode/rea/soothe3-restored.bin`, base `0x180000000`.
Corpus: `/data/dev/workspace/RunScratch/smooth/re-corpus/corpus/fn/<rva>.c`.
Scripts (not in git): `/data/dev/workspace/RunScratch/smooth/re-latency/`
(`xrefs_fn.py call|ref LO HI` walks `manifest.tsv` per function; `disasm_va.py VA N`).

## 1. FFT spec constructor — located

`fcn.180007c40` (rva `0x7c40`), reached only through the dispatch thunk family at
`0x1800012b0` (table `0x1821ec1b8`, entry 0). Signature (Windows x64):
`init(out**, order, flag, hint, …)`, `order` ≤ 0x1d, `flag` = arg3.

| flag (arg3) | IPP name | spec+8 (scaled?) | spec+0x10 (forward scale) |
|---:|---|---|---|
| 8 | `IPP_FFT_NODIV_BY_ANY` | 0 (unscaled) | — |
| 4 | `IPP_FFT_DIV_BY_SQRTN` | 1 | `1.0 / sqrt(N)` (`0x181e17610` double 1.0 ÷ `0x181d89c10` sqrtsd) |
| 1 | `IPP_FFT_DIV_FWD_BY_N` | 1 | `1.0f / N` (`0x181e17618` float 1.0) |
| 2 | `IPP_FFT_DIV_INV_BY_N` | 0 | — (1/N kept for inverse at spec+0x10) |

The spec layout: `spec[0]=7`, `spec[1]=order`, `spec[2]=scaled flag`, `spec[3]=inverse-scale flag`,
`spec[4]` (+0x10) float scale, `spec[5]` (+0x14 from arg4 hint), `spec[6]`, buffers from `+0x30`.
This matches the wrapper `fcn.180001640` (`spec+8` selects `table_scaled`/`table_unscaled`).

## 2. Who builds the complex forward spec — flag 8

`fcn.180617f70` (rva `0x617f70`) is the FFT-object constructor. Its body:

```c
puVar5 = alloc(0x20);  /* buffer header */        *(arg1+8) = puVar5;
puVar5 = alloc();      *puVar5 = 8;  /* flag */  *arg1 = puVar5;   // puVar2 = *arg1 (uint*)
*(*(arg1+8)+0x18) = N;                           // N = requested length, rounded to 8
func_0x180001130(uVar4, *puVar2, 1, …);          // GetSize with flag 8
func_0x1800012b0(puVar2 + 2, uVar4, *puVar2, 1, …);  // init: spec written at (*arg1)+8
```

Because `puVar2` is `uint *`, `puVar2 + 2` is byte offset +8: the spec pointer sits at
`(*arg1)+8`, which is exactly the argument `*(X)+8` the per-frame code passes to
the FFT wrappers. Flag 8 means **no forward scale in the complex path**.

Callers of the constructor (`fcn.18061b870`, `size` = arg2, objects at `+0x68` etc.):
`0x89aed0`, `0x89c480` (`arg1+0x68`, size `arg1+0x38`), `0x89fa30`, `0x8a4a90`.

## 3. The per-frame FFT chain (`fcn.180619fc0`)

Two objects are built per block (lines 306–310 of the annotated decompile):

```c
fcn.180617f70(iStack_a60 + 0x70, N1);   // N1 = 2*(bins1-1), uVar17
*(iStack_a60+0xa8) = N2;                // N2 = 2*(bins2-1), uVar35 (= iVar9)
fcn.180617f70(iStack_a60 + 0x90, N2);
```

Ordered calls (the decompile, not yet mapped to plan semantics):

1. `0x180001280(buf, spec(+0x90), tmp)` — **real** forward through `0x7000`/`0x72e0`
   (the real pack/unpack wrappers around the unscaled complex kernel).
2. `0x180001220(1.0f / N2, buf, N2)` — **explicit software scale 1/N2**.
   `1.0f` is `*0x18209d2b4`; the divisor is `*(obj+0xa8) = N2`.
3. `0x1800010a0`, `0x1800010d0` — real-spectrum fold-up.
4. `0x180001010(buf2, buf1, spec(+0x70), tmp)` — **complex** forward, flag 8, **unscaled**.
5. `0x1806181b0`, `0x180001190` — not yet identified (candidate: spectral multiply and
   inverse; `fStack_aa0 = -22.0`, `fStack_a98 = 19.8` are set nearby).

So the reference's spectrum is scaled by **1/N2 once** on the real leg and not at all
on the complex leg. Which of these reaches the detector's `m` is not yet decided.

## 4. Consequences for SMOOTH

- `magnitude-scale.md` said "no scale connected". That is now partly answered: the
  complex forward is unscaled, the real leg is scaled by `1/N2` in software. Raw `|X|`
  (SMOOTH's current `norm()`) matches the unscaled complex path only.
- If the detector consumes the scaled real leg, `m ∝ 1/N` and the `max(m,1)` / `+1`
  floors move far down (see the table in `magnitude-scale.md`); the soft-mode level
  invariance would then need the floors re-derived. If it consumes the complex leg
  after the product, raw `|X|` is right.
- **Sample-rate dependence (rt-audit: depth 10 gives -3.5 dB at 44.1 kHz, -0.45 dB at
  192 kHz):** SMOOTH's FFT length is `m*2` with `m = rate/44100*2048`, so `N` scales with
  rate (4096 → 17840). Raw `|X|` grows with `N` (about `N·A·CG/2`), so the absolute
  floors bite at different levels per rate. A `1/N` normalisation would remove that
  dependence. Whether the reference has the same dependence therefore hinges on the
  open item below; it is not settled here.

## 5. Latency — partial

- `getLatency` `0x18088f610`:
  `if (*(X+0x237)==0 && *(X+0xab)==0) return H(X+0x5f8)*[X+0x2c] + [X+0x278]; else return [X+0x278];`
- `H` = `fcn.180894d20`, `size 40`:
  `if (*(*a + 0x58) == 0) return [a+0x2c78] + [a+0x30];` (`a` = `X+0x5f8`)
  `else return ([a+0x2c8] + [a+0x2f10]) / 2 + [a+0x2c78] + [a+0x30];` (the `/2` and the
  two counts come from the decompile; `[a+0x2c0]`, `[a+0x2f08]` are the loop counts).
- The plan constructors `0x63ceb0` and `0x66fda0` do **not** compute `[0x278]`: they copy
  the struct from their stack arguments (`in_stack_00000040[3]` → `+0x278`,
  `in_stack_00000090[0]` → `+0x30`). The values come from `0x63ab82` (caller) and
  `0x66e500` (`Footer::slot3`), which build those tables from the object `R14` and float
  constants `0x18209d600`, `0x18209da10`, `0x18209d6c4`.
- **Not recovered:** the actual numbers per quality, size and low-latency mode. They
  depend on `R14` state written at runtime by the host, so a static read cannot give
  them. The next step is Unicorn emulation of `0x63ab82` up to the call at `0x63b9cf`
  with the object zeroed and the float constants set, then reading `[+0x278]`, `[+0x30]`
  and `[+0x2c78]`.

## Open

1. Which buffer does the detector read after step 4 (`0x180001010`) and step 1? Trace
   `0x1806181b0` and `0x180001190`, and the detector's `ln()` input (`0x1801c84d0` with a
   broadcast constant).
2. Emulate `0x63ab82` to get `[0x278]`/`[0x30]`/`[0x2c78]` for the default plan; then the
   reported latency = `H*1 + [0x278]` in the non-low-latency branch.
3. Confirm the N2 path is the sidechain/analysis leg, not the audio path.

Until 1 and 2 are closed, SMOOTH keeps `norm()` and `latency = n-1 (+m/2)`. The impulse
delay test in `dsp.rs` (per plan) stays as the PDC check.

## 6. Detector data flow, pass 2 (2026-10-21)

Per channel, `fcn.180619fc0` (lines 415–450 of the annotated decompile):

1. Real leg: `0x180001280(pad, out, spec_N2)` — real forward, N2 = 2·(bins2−1),
   then `0x180001220(1.0f/N2, out, N2)` — **explicit 1/N2 on the real leg**.
2. Fold: `0x1800010a0`, `0x1800010d0` on the N2 spectrum.
3. Complex leg: `0x180001010(out2, fold, spec_N1)` — **unscaled** complex forward,
   N1 = 2·(bins1−1). (Argument order: first arg is src, second dst, as in step 1.)
4. Power: `fcn.1806181b0(dst, complexN1, N1)` = `re²+im²` per bin, no scale
   (checked in the pseudocode: `pfVar24[k] = re*re + im*im`).
5. `0x180001190(dst, idx, per_channel_ptr, N1 as float)` — **not identified**. It may
   take sqrt/log or just move bins. This decides whether the array is |Y| or |Y|².
6. Resample to 1024 points (`iStack_574 = 0x400`), **Catmull–Rom cubic** at the
   fractional bin `t = (f[k]/f[N1−1])·(N1−1)`:
   `p = 0.5·[2b + (c−a)t + (2a−5b+4c−d)t² + (−a+3b−3c+d)t³]`, with the constants
   `0.5` (`0x18209d1c0`), `2` (`0x18209d4a0`), `3` (`0x18209d560`), `4`
   (`0x18209d5d0`), `5` (`0x18209d5ec`) read from `.rdata` (`re-latency-readconst.py`).
   A pure interpolation: no scale is introduced here.
7. Grid: the 1024 log-spaced values come from `0x18209d258 = 1/1023` (linear from
   `func_181cbdf7b()` to another call, `fVar36`/`fVar40`).
8. Smoothing: `fcn.18061c3f0(buf, state, α, α)` with `α = 1 − f(0x620·0x18209d984)`
   (`0x18209d984 = −0.05`). Attack/release one-pole on the log state.
9. Detector target: `ln` on the 1024 array (`0x1801c84d0`), weighted by
   `pfVar26[k]²` in the soft branch (`fVar40/fVar48`); floors `1e-5`
   (`0x18209d010`), `0.1` (`0x18209d0c8`), `0.01` (`0x18209d060`).

Constants read from the binary (`.rdata`, f32): `1.0`, `0.5`, `1/38 (0x18209d088)`,
`1e-5`, `2`, `3`, `4`, `5`, `20`, `19.8`, `−20`, `−22`, `0.05`, `−0.05`, `1/1023`,
`9.22e18` (int-conversion sentinel).

### What this changes for SMOOTH

- The detector does **not** consume SMOOTH's single STFT magnitude. It consumes the
  output of two transforms: a real N2 forward (with 1/N2) and an unscaled complex N1
  forward over the folded spectrum, then |·|² and a 1024-point log-frequency resample.
  The two plan sizes (N1 from table `0x770`, N2 from `0x788`) are different objects.
- So the reference's m is **not** `norm()` of an N-point FFT, and the sample-rate
  dependence of the reference is not determined by SMOOTH's `N = 2m` argument alone.
  The rate dependence depends on how bins1/bins2 scale with the rate. Not yet read.

### Still open (exact)

- Step 5 `0x180001190`: is there a sqrt, a log, or a copy? Decides |Y| vs |Y|².
- Steps 1–3 fold: `0x1800010a0/d0` arithmetic.
- How `bins1`, `bins2` (tables `0x770/0x788`) depend on quality, size and rate.
- Latency (section 5) is untouched by this pass.
