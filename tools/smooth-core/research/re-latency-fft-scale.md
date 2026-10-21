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
5. `0x180001190(dst, idx, per_channel_ptr, N1 as float)` — **identified in pass 3 as an
   elementwise multiply** (see section 7). The array is `|Y|²` times a weight, not `|Y|`.
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

## 7. Pass 3 (2026-10-21): thunks decoded, magnitude is power times weight

Method: thunk jump tables read from the PE (`r2 pxq 8`, slot 0 = runtime selector 0; the
selector is written as 0 or 0x14 by `fcn.1802fb880`, and every IPP variant of one op computes the
same arithmetic). Each thunk is an argument-check wrapper that tail-calls the op.

| thunk | slot 0 target | op (VERIFIED from disassembly) |
|---|---|---|
| `0x180001010` | `0x180001640` | complex FFT wrapper (spec ID 7 check, section 1) |
| `0x180001190` | `0x1800049e0` -> `0x1800278c0` | `dst[i] = a[i] * b[i]`, `len` in r9d (`ippsMul_32f` shape) |
| `0x1800010a0` | `0x180002ae0` -> `0x181d52e00` | copy, `count = len*4` bytes (rcx = dst, rdx = src) |
| `0x1800010d0` | `0x180002e40` -> `0x181d53d90` | zero-fill, `count = len*4` bytes (rcx = dst) |

`0x1806181b0` (fcn, between the complex FFT and the multiply): `P[k] = re^2 + im^2`, no sqrt, no scale
(VERIFIED from the pass-2 pseudocode; no sqrt routine is called).

### The detector's operand at the multiply (fn619fc0.c line 450)

```c
fcn.1806181b0(iVar32 /*P*/, iVar30 /*complex Y, N1*/, uStack_a40 /*= bins1*/);
func_0x000180001190(iVar32 /*a = P*/,
                    auStack_a28 /*b = iStack_998 + uVar10*bins1*4*/,
                    *(auStack_560 + uVar10*8 - 8) /*dst, per group*/,
                    fStack_aa0 /*= fStack_164 = uStack_a40 = bins1*/);
```

- `iStack_998 = *(obj+0x7a0)`, the weight table W1 of `bins1 × groups` floats (checked at line 245:
  `(obj+0x7a8 − obj+0x7a0)>>2 == bins1 * groups`). VERIFIED.
- `*(auStack_560 + 8g − 8)` is the g-th output row (buffer slices of `bins1` floats, line 333).
- So per group g and bin k: **`D_g[k] = P[k] · W1_g[k]`**, then Catmull-Rom onto 1024 log bins.
  The detector therefore consumes a **weighted power**, never a plain `|X|` or `|Y|`.

### Scale consequence (INFERRED until the fold indices are pinned)

- Real leg: `X = DFT_N2(pad)` unnormalised, then `X / N2` (line 416, explicit `1.0f / N2`).
- Fold: copy + zero-fill of length-`N2` spectrum rows into a length-`N1` buffer (lines 428, 438).
  The exact index ranges depend on `uVar35>>1` and `uStack_a10`; not yet read.
- Complex leg: `Y = DFT_N1(fold)`, unscaled.
- Therefore `P ∝ (N1/N2)² · (fold energy)`. If `N1` and `N2` are fixed per quality (not per rate),
  the reference's absolute level has **no 1/N or 1/√N normalisation at all** and is set by `W1`.
  Whether `N1/N2` depends on the sample rate is the open point (see the table in `magnitude-scale.md`).

### What this changes for SMOOTH (no SMOOTH code changed in this pass)

1. `frame()` takes `norm(X_k)` of one N-point STFT. The reference takes `P_k` of a
   two-resolution chain, then a per-group weight. These are different quantities: `norm` is the
   square root of the power, so SMOOTH's soft target `-ln(max(m,1))` and hard target
   `-(0.25 + d^4/4) ln(m^2+1)` are evaluated on a different variable than the reference's.
2. The 1024-point resample and `W1`/`W2` are the structural pieces missing from the port.
3. The rate-dependence test (depth 10 at 44.1 kHz vs 192 kHz) cannot be matched until the
   reference's `N1/N2` and `W1` are read as functions of rate. Multiplier `*(arg2+0x630)` on the
   detector bounds (caller `fcn.1806b96f0`, corpus `006b96f0.c`) is the likely rate factor. Not read yet.

### Still open

- Fold index ranges (`uVar35`, `uStack_a10`, `iVar16`) and therefore the N1/N2 relation.
- Builders of the object vectors at `+0x770/+0x788` (bin frequencies) and `+0x7a0/+0x7b8`
  (weights). The corpus hit at `00670bb0.c` writes `+0x778` on a different object; ignore it.
- Latency (section 5) and the rate factor `*(arg2+0x630)`.

## 8. Latency correction (pass 3, Unicorn, low fidelity): no numbers recovered

Corrections to section 5 (the earlier attribution was wrong):
- `getLatency` `0x18088f610` and `H` `0x180894d20` formulas are verified by disassembly, as in section 5.
  `H` is about 0x103 bytes, not 40.
- The writes to `+0x278` and `+0x30` come from `fcn.18066fda0` (`0x18066fda0`), not from
  `fcn.18063ceb0`. `ceb0` writes only `arg2+0x2c = 1`.
- `fda0` is called inside `Footer::slot3` (`0x18066e500`) at `0x18066f32f` and `0x18066fc00`.
  `ceb0` is called at `0x18063b9cf`, inside `fcn.18063ab31`, which is not on the `+0x278` path.
- Emulating `Footer::slot3` with zeroed state and with the four `r8` floats varied gave
  `+0x278 = 0` and `+0x30 = 0` in every case. The outputs are artifacts of stubbed null-vtable
  objects (`this+0x160/0xc10/0x16c0/0x1cc0/0x22c0/0x2a40`), so no quality, size, low-latency or
  rate table exists yet. Latency numbers remain **not recovered**.
- The `fda0` destination is a frame local, not `X`. Whether it aliases `X+0x5f8` is unverified.
- Open: the writers of `X+0x278` (258 store sites in `.text`), the inputs of `H`, and the
  constants at `0x18209d600`, `0x18209da10`, `0x18209d6c4`.

## 9. Bin counts, weights and the "rate factor" (pass 3, static): corrections

Agent-reported static reads (line refs into the annotated decompile and corpus; not re-verified line by line):
- N1 = 2(bins1-1), N2 = 2(bins2-1). bins1 and bins2 are the float counts of two vectors on the
  detector state S = `*(arg2+0x648)`: S+0x770 and S+0x788 (`fn619fc0.c:183-188`). Both are filled by
  a record dispatcher `fcn.180621d50` (`corpus .../00621d50.c`): record 0 -> S+0x770, 1 -> S+0x788,
  2 -> S+0x7a0 (W1), 3 -> S+0x7b8 (W2). Names from `index/annotated/0088fa90.c:119-122`
  (reduction_curve_freq_points_lo/hi). So bins are **host-supplied data, not a function of quality,
  size or sample rate** at this level.
- No caller of `fcn.180621d50` was found (no direct call, rip-relative ref or absolute pointer), so
  the per-preset values are unknown. Settling it needs a breakpoint or a dump of default state.
- Groups = `fcn.18056cad0(**(S+0x4a0))` (`:155`), body unread.
- **The "rate factor" `*(arg2+0x630)` is NOT a DSP rate factor.** It is `M` from the caller
  `fcn.1806b96f0` (`006b96f0.c:5-8`), default 1.0 (`006f3260.c:254`), updated per UI tick from
  `PluginUI::slot12` (RVA 0x569f60, not decoded). It scales the widget rect (left, top, right,
  bottom) passed as the detector's frequency bounds. INFERRED: a display-coordinate scale.
  Section 6's "likely rate factor" should be discarded.
- Sample rate enters a different object. `fcn.18089c480` (`0089c480.c:134-146`) clamps R >= 44100 and
  sets m = trunc(R/44100 * size), N = 2m, with size 0x800 from `00896ad0.c:28`. For size 0x800 that gives
  4096 (44.1k), 4456 (48k), 8192 (88.2k), 8916 (96k), 16384 (176.4k), 17832 (192k). The 17840 in
  section 4 is off by 8 (the agent's arithmetic check, not re-run here).
- Caller correction: `fcn.180617f70` (the FFT-object ctor) is called from `0x619fc0`
  (`0x61a5b1`, `0x61a5c4`) and `0x61b870` (`0x61b894`). The 0x89xxxx/0x8axxxx sites in section 2 call
  `0x61b870`, not `0x617f70` directly.

### Consequence for the port

The reference's absolute level is not a function of SMOOTH's sample-rate-scaled N. Its detector uses
N1/N2 from host-filled bin vectors and a per-group weight W1 from the same host records. The
44.1 vs 192 kHz depth difference (rt-audit) therefore has to come from the bin/weight data or from the
analysis sub-object, not from `M`. Not resolved; the next step is the dispatcher caller.

### Open

1. Caller of `fcn.180621d50`: breakpoint at RVA 0x621d50 during preset or default-state load.
2. Default bin counts: dump the default record payloads (bytes/4 = bins).
3. Decode `PluginUI::slot12` (0x569f60) only if the display scale matters for the detector.
4. Writer of PluginUI+0xd0 (divisor for M).

## 10. Route to the detector's bin vectors (pass 4, static, 2026-10-21)

Section 9 said no caller of `fcn.180621d50` (0x621d50) exists. That is wrong: the call is
an indirect one, through a this-adjusting thunk and a vtable. Verified with
`re-tools/refs.py` (rel32, rip-relative, abs64 and rva32 forms, plus vtable and RTTI decode):

| step | address | fact (checked) |
|---|---|---|
| 1 | `0x62a710` | thunk `add rcx, 8; jmp 0x180621d50` (rel32 hit at 0x62a714) |
| 2 | vtable `0x2073998`, slot 2 | `0x20739a8` holds `0x18062a710`. RTTI: `_Func_impl_no_alloc<lambda_1 in MainComponent::update(Context, Input, Output)>` |
| 3 | `0x6210d0` = `MainComponent::slot2` | size 3191. Sets `uStack_108 = 0x182073998` (the lambda vptr), then `func_0x18033cd50(*(arg3+0x100), &visitor)` |
| 4 | `0x621d50` | record handler reached from the lambda's invoke. Writes S+0x770/0x788/0x7a0/0x7b8 (section 9) |

- `MainComponent` is the root class: vtable `0x2073b90` (slot 2 = `0x6210d0`, RTTI `.?AUMainComponent@@`).
  Its vptr is stored at `0x61d94f` and `0x620f0b` (refs tool, rip-relative).
  `MainComponent::update` is the function whose lambda is the visitor.
- `0x6210d0` runs on `*(arg2+0x648) = S` (`iVar1`). It writes `S+0x800/0x804/0x76c` (time or block
  counters) and calls the weight/bin helpers `0x6293b0`, `0x563970`, `0x56cad0`, `0x629480` and `0x6fc030`.
  Those helpers have not been read.
- A second table reference: `.data` RVA `0x241cb04` holds 32-bit `0x621d50`. The table at
  `0x241cad4..0x241cb34` alternates MainComponent RVAs (`0x620e70`, `0x6210d0`, `0x621d47`, `0x621f18`,
  `0x622890`, ...) with data pointers (`0x20fc7a0`, `0x21355d0`). Shape: visitor or dispatch table.
  NOT decoded: the table's index and its user are open.

### Where the defaults come from (what this pass could not reach)

- `0x88fa90` (size 3022) registers the DSP's key names through a host vtable:
  `(**(host+0x18))(host, id, name, flag)` for ids 0..0xd (`reduction_curve_freq_points_lo/hi`,
  `reduction_curve_magnitudes_lo/hi`, `offset_curve_freq_points`, `detail/attack/release_offset_curve`,
  `max_cut_per_channel`, `band_solo_channels`, `input_channel_layout`, `samplerate`, `sidechain_input_level`).
  Values come back through `(**(host+0x20))(host, id, out, flag)`.
  The bin counts are therefore host state, read with these getters.
- `0x88fa90` has one caller, `0x88f430` (size 171). That function builds the DSP object: its vtable is
  filled with `0x88f4e0, 0x88f4f0 (process, section 6), 0x88f500, 0x88f530, 0x88f610 (getLatency), 0x88f660`.
- `0x88f430`'s caller is `0x88e3a0`. Its decompile is nonsense (a `do { } while` on the return address
  and a computed call `(*(r15+0x70e))()`). It has **zero** static callers. The construction path is
  obfuscated or reached only through a computed dispatch. Static reading stops here.
- The host object that implements `+0x18/+0x20` is not identified. Its default values (the
  reduction and offset curve points, the sample-rate table, and the layout) are the missing ground truth.

### Consequence for the oracle (task's "wall")

The emulated oracle can reach the bin fill and the per-block analysis (sections 6-7) by calling
`0x6210d0` (or `0x621d50`) directly with a synthetic visitor and a synthetic `S`. It cannot yet produce
per-preset bin counts or weights. The next static step is to identify the host class whose
vtable slots `+0x18/+0x20` implement the getter (its RTTI name, then its default-value table). If that
fails, the fallback is a breakpoint-style trace in the emulator on `0x88f430` with a fake host whose
getter records every call.

### Still open (pass 4)

1. Host class of `+0x18/+0x20` and its default data.
2. `0x88e3a0`: what calls it (computed-call dispatcher, or a patched-in path).
3. The index of the `.data` RVA table at `0x241cad4` and its user.
4. Helpers `0x6293b0`, `0x629480`, `0x56cad0` and `0x6fc030` on the S-side of `0x6210d0`.

## 11. Fake-host run of the constructor (pass 5, Unicorn, 2026-10-21)

Harness: `re-tools/emulate/fake_host.py` (uses `pe_emu.py`). Evidence scope: isolated emulation with a
synthetic host. Nothing here is a plug-in default.

### Verified facts

- **Host layout (corrects section 10's "vtable" wording).** The constructor's calls are
  `call qword ptr [r15+0x18]` and `call qword ptr [r15+0x20]` with `rcx = host` (asm at
  `0x18088fb67` ... `0x18088fc80`, `r15 = rdx = host`). The host object holds the function pointers
  directly at +0x18 (register) and +0x20 (get); there is no vtable indirection at these sites.
- **Signatures.** Constructor `0x18088fa90(obj, host, cfg, xmm3 = samplerate)`. Builder
  `0x18088f430(host, cfg, xmm2 = samplerate)` allocates `0xc1a8` bytes and calls the constructor
  with the same arguments. The builder's wrapper vtable slots are `0x88f4e0, 0x88f4f0, 0x88f500,
  0x88f530, 0x88f610, 0x88f660`.
- **Registrations.** The emulated run makes 13 register calls: ids 0-10, 12 and 13. **Id 11 is not
  registered.** The getter is called once, for id 0xd (`sidechain_input_level`).
- **Sample rate.** `xmm3` is stored at `obj+0x28`, and `max(rate, *0x18209d450)` seeds the 16 group
  records at `obj+0xb7a8` (stride `0xf` floats). Their defaults before that are `48000.0` (rate),
  `1000.0`, `20.0`, `30.0`. The `48000` is a constructor default overwritten by the argument. So the
  rate-dependent part is the argument, not a table.
- **Config enums.** `cfg[0]` (qword at arg3+0) and `cfg[8]` feed `fcn.18088acb0`, a switch with cases
  1..0x14 that returns the address of a static object. Values 0 and out-of-range fault in the jump
  table. Values 1, 2, 5, 9 and 20 are accepted. `cfg[16]` is the `int` read by `fcn.18056cad0`.
- **TLS.** The constructor reads `gs:[0x58]` (TLS array). The harness now installs a 256-slot TLS
  array. Without it the run faults at address 0.

### Blocker (not solved)

With cfg = (1, 1, 1) the constructor reaches `fcn.18088c950` (`0x88c950`, size 297). It calls
`func_0x1806baab0` on sub-vectors at `arg1+0x18/0x30/0x48/0x60/0x78` and then fills
`count = *(arg1+0x50)` entries from the `+0x48` buffer. The buffer is null (`mov [rcx], 1.0` at
`0x18088cce5` writes to address 0). So the caller of `0x88c950` sets a count without allocating the
vector. Who sizes those vectors (the `0x896ad0`/`0x896eb0` filter-bank builders, lines `0x88ff1e`,
`0x89666b`, `0x89f65d` in the stack walk) is the next question. The `N1/N2` and `W1/W2` vectors of
section 9 are not yet reached, so no bin counts, weights or latency numbers came out of this run.

### Not done in this pass

- No oracle (sine/impulse/noise) runs, so no reported latency, impulse delay or level numbers.
- No change to `dsp.rs`, no test, no install, no CPU or pluginval check.
- `pe_emu.py` fix: the XMM argument path passed `bytes` to `reg_write`. It now passes an integer.

## 12. Constructor runs to completion under emulation (pass 6, Unicorn, 2026-10-21)

Harness: `re-tools/emulate/fake_host.py` plus two new modules, `cpu.py` (CPUID profile) and
`libm.py` (math imports). Evidence scope: isolated emulation with a synthetic host and an assumed
CPU. Nothing here is a plug-in default or a measured value.

### Blockers found and fixed (each traced by reading the code, not by guessing)

1. **Analysis mode never set.** Builder `0x89c480` allocates its bin/filter vectors only when the
   global mode `0x26d7bb8` is 4 (path `0x8ce260`) or 1 (path `0x8debf0`). The writer is `0x30d310`:
   mode = `flag ? 4 : 1` where `flag` is bit 0 of the cpuid leaf-1 ECX word (`0x26e00ec`, written
   by the detection routine `0x88c5f0`). Under emulation the mode stayed 0, the allocation was
   skipped, and `0x88cb10` wrote `1.0` through a null buffer (`0x18088cce5`, the reported fault).
   Fix: `cpu.py` answers cpuid with a modern-Intel profile (SSE3 set, AVX2), and `set_cpu_mode`
   runs the real writer `0x30d310` with the flag set. **Not done:** the detection routine
   `0x88c5f0` itself. Its record loop (cpuid leaves 0x80000000..) writes through a std::vector
   whose storage is never set up and keeps overwriting the loop bound at `0x26e00a4` (open; not
   needed for the constructor).
2. **`pow` returned garbage.** The block-size function `0x181cbdb35` is a jump through slot
   `0x181db0888`, which is `IAT!pow`. The default import stub returns 0 in RAX and leaves XMM0 as
   it was, so every block size was wrong (2048 bins gave 32 blocks, and the consumer `0x8ac980`
   then read index 32 and faulted). Fix: `libm.py` implements the math imports the path uses
   (pow, exp, exp2, log, log2, log10, sin, cos, tan, acos, asinh, ceil, floor, round, fmod, hypot,
   ldexp, lround, the f variants, fmaf, _dclass, _dsign, _fdclass). Other value-returning imports
   still use the default stub (string, time, and window APIs); the constructor path does not reach
   them.

Lesson for the harness: an import with a float result must never use the default stub. The
default-stub log (`emu.events`) should be checked for any import that returns a value.

### Result (constructor, per sample rate, `python3 -I fake_host.py`)

| rate | `obj+0x2c` (groups) | `obj+0x248` (vec count) | `obj+0x28` (rate) |
|---|---|---|---|
| 44100 | 1 | 48 | 44100.0 |
| 48000 | 1 | 48 | 48000.0 |
| 96000 | 2 | 96 | 96000.0 |
| 192000 | 4 | 192 | 192000.0 |

- Groups scale with the rate: 1, 1, 2, 4. This is the first emulated sample-rate dependence (the
  depth question in section 5). It is the count of groups, not a level.
- Group-0 record at `obj+0xb7a8` (16 floats, stride 0xf): entries 0..13 agree across rates
  (for example entry 5 = 4.477, entry 7 = 1.875, entry 13 = 2.969). Entries 1 and 14/15 change with
  the rate (1: 7.173 / 7.232 / 7.732 / 8.465; 15: 0.784 / 0.776 / 0.714 / 0.651).
- **Open anomaly:** entry 14 is 2.7e22 at 48 kHz and -0.0 at 192 kHz (0 at 44.1 and 96 kHz).
  Not explained. Suspect an uninitialized read in the constructor or a `pow` result on a sentinel
  input. Do not use entry 14 until it is traced.
- `obj+0xbf28` coefficients change with the rate (0.000756 at 44.1k, 0.000174 at 192k in entry 2),
  consistent with a 1/rate-scaled coefficient.

### Not done in this pass

- No process path run (0x890a00 / 0x180619fc0), so no latency, impulse delay or level numbers.
- No oracle, no `oracle.py` or JSON output, no Rust change to `dsp.rs`, no plugin change, no test,
  no install, no pluginval, no CPU measurement.
- The mode and CPU features are an assumption (reference-class CPU). The other mode (1) has not been
  emulated at all.

### Latency from the emulated getter (pass 6, constructor state only)

`0x18088f610` (getLatency) is `groups(+0x2c) * f(this+0x5f8) + base(+0x278)`, or `base` alone when
bytes +0x237 or +0xab are set (both 0 here). `f` = `fcn.180894d20`, read through the emulated call.

| rate | getLatency | groups | base (+0x278) | f |
|---|---|---|---|---|
| 44100 | 2304 | 1 | 0 | 2304 |
| 48000 | 2304 | 1 | 0 | 2304 |
| 96000 | 4704 | 2 | 96 | 2304 |
| 192000 | 9408 | 4 | 192 | 2304 |

- `f` = 2304 at every rate, and the config enums (cfg[0] in {1,2,5,9,20}, cfg[8] in {1,2,5,9,20})
  do not change latency at 48 kHz. Latency is therefore a function of the sample rate only, through
  groups and base, in this state.
- `base` is 0 at 44.1 and 48 kHz and `rate/1000` at 96 and 192 kHz (96 and 192). One pair of points
  each. Not a rule yet: needs 88.2 kHz and a second size.
- Current SMOOTH (`src/dsp.rs:62-83`) reports `n - 1 (+ m/2 linear phase)` with n = 2048, which is
  2047 at the default (plus m/2 when linear phase is on). The reference state gives 2304 at 44.1/48
  kHz. Mismatch of 257 samples at the default. Medium confidence: the value rests on the CPU profile
  and the libm stubs, and the process path has not been run to confirm the impulse delay.

## 13. Sample-rate table and entry 14 (pass 7, Unicorn, constructor state)

Harness: `re-tools/emulate/oracle.py` (writes `oracle/ctor_latency_mode{4,1}.json`).
Evidence scope: constructor state, fake host, reference-class CPU profile. No process path run.

- `getLatency` takes `rcx = &holder` (it reads `X = [rcx]`). Passing the object
  directly gives `0x40000000`; section 12's table was produced with the correct
  convention and still stands.
- Rates 44.1, 48, 64, 88.2, 96, 110.25, 132.3, 176.4, 192 kHz give
  `groups = max(1, trunc(rate/44100))` and `vec_count = 48*groups`.
- `H = 2304` at every rate. `getLatency = groups*2304 + base`, with
  `base = 48*groups` for groups >= 2 and `0` for groups = 1 (0, 96, 144, 192).
  The base rule is fitted on four group counts; it is not read from code.
  Section 12's "base = rate/1000" was a coincidence of the 96 and 192 points.
- Analysis mode 1 (CPU flag clear) gives the same latency as mode 4 (checked at 44.1, 96, 192 kHz).
- **Entry 14 resolved:** the constructor writes a double (8 bytes at `obj+0xb7e0`),
  not a float. `rate * double` = 33.32-33.33 at every rate, so the stored value is
  about `33.3/rate`, a rate-dependent coefficient. The earlier 2.7e22 / -0.0 came from
  reading that double as a float. Do not use the float view.

### Still open (pass 7)

- The process path (`0x890a00`, `0x180619fc0`) is not run. The FFT wrappers do not compute
  a DFT under emulation (no twiddle tables), so impulse delay and level need numpy hooks
  verified at small sizes first.
- Config enums (quality, size, low-latency) are not varied in the oracle.
- `0x88c5f0` (CPU detection) is not run; mode is set through the writer `0x30d310`.
- No SMOOTH change in this pass: reported latency (`n - 1 = 2047`) is not replaced until the
  real impulse delay is measured, because PDC must match the delay the plugin produces.

## 14. FFT primitives verified under emulation; three labels corrected (pass 8, 2026-10-21)

Harness: `re-tools/emulate/fft_verify.py` writes `oracle/fft_verify.json`. Evidence scope: the
image's own kernels run under isolated emulation (reference-class CPU profile, libm stubs) and
compared with numpy on random input. No plug-in data, no process path.

**No numpy replacement is needed for the wrappers.** The emulated kernels match numpy to about
1e-7 relative at every order 1..12 (worst 1.9e-7). The earlier "no DFT under emulation" was a
harness bug: the init thunk `0x12b0` (→ `0x7c40`) takes six Win64 arguments, `(out**, order,
flag, hint, scratch, twiddle scratch)`. A null scratch returns `0xfffffff8`, and the twiddle
scratch is needed from order 11.

| thunk (slot 0 target) | verified semantics | argument order |
|---|---|---|
| `0x1010` → `0x1640` | **real forward, unscaled** (spec flag 8). N reals → packed `N+2` floats `[X0.re, 0, X1.re, X1.im, ..., X_{N/2}.re, 0]` | `(src, dst, spec, tmp)` |
| `0x1280` → `0x7000` | **real inverse, unscaled**. Packed `N+2` → `N·x` (no 1/N) | `(src, dst, spec, tmp)` |
| `0x1220` → `0x5e80` | in-place scale, `xmm0` = factor | `(_, buf, N)` |
| `0x10a0` → `0x2ae0` | copy `len` floats | `(src, dst, len)` |
| `0x10d0` → `0x2e40` | zero-fill `len` floats | `(dst, len)` |
| `0x1190` → `0x49e0` | `dst[i] = a[i] * b[i]` | `(a, b, dst, len)` |
| `0x6181b0` (not a thunk) | **complex magnitude** `|Y_k| = sqrt(re² + im²)` over `bins` interleaved pairs | `(dst, src, bins)` |

Corrections to earlier sections (the code, not the labels, is authoritative):
1. **`0x1280` is an inverse, not a real forward.** Sections 3, 6 and the README said "real
   forward". Line 415 of `fn619fc0.c` calls it on `(x, 0)` pairs built from the frame, so it
   computes a cosine transform of the frame.
2. **`0x1010` is the real forward (N reals in), and it is not "complex" in the notes' sense.**
   Line 447 calls it on the folded buffer, then `0x6181b0`.
3. **`0x6181b0` returns the magnitude, not the power.** Section 7 said "no sqrt, P = re²+im²".
   The emulated kernel returns `|Y|` (basis test: input 2.0 gives 2.0; `3+4i` gives 5). The
   detector's operand is `|Y_k| · W1_k` (line 450), not `|Y|²`. This changes the magnitude
   scale and the floors in `magnitude-scale.md`.

### Detector chain, now read with verified semantics (fn619fc0.c lines 395–450)

```
frame -> (x,0) pairs -> 0x1280 inverse, N2 (unscaled) -> 1/N2 (0x1220)
      -> copy/zero fold (0x10a0/0x10d0)  [the lifter: keep one half, zero the rest]
      -> 0x1010 forward real, N1 (unscaled) -> 0x6181b0 |Y| over bins1
      -> 0x1190 multiply by W1 row g -> Catmull-Rom to 1024 log bins -> attack/release -> ln
```

The inverse-then-forward pair with a fold between them is a cepstral (liftered log-spectrum)
envelope. That is the shape of the reference's analysis, not a plain STFT magnitude. The input
buffer `pad` (the source of `iVar28`) and the values written before line 395 are not decoded yet,
so the input to this chain is still unknown.

### Blocker (not solved): detector state

Running `0x619fc0` with a synthetic state (constructor object at `obj`, bins at `+0x770/+0x788`
as ascending Hz, weights at `+0x7a0/+0x7b8` set to 1) faults at each host-built structure it
reads:
- `func_0x180563970` is a `std::map` lookup on `S+0x538/0x550` (MSVC layout). Sentinel node
  pointers move the fault deeper (to `0x18056d1a1`).
- Further unpopulated fields follow. The host fills these in `MainComponent::update`
  (section 10), which is not run here.

So the process path (`0x619fc0`, `0x890a00`) cannot give an impulse delay, output level or the
1024-point curve yet. The fix is to run the host-side fill, not more guessing of fields.

### Consequences for this pass

- Latency: unchanged (section 13's constructor-state table). Real delay is still unmeasured, so
  PDC cannot change (plan rule).
- Magnitude scale: the reference uses `|Y|·W`, and `N2`/`N1` scaling is `1/N2` on the inverse
  leg only. SMOOTH's `norm(X_k)` is not this, but a port needs the fold indices and the
  input buffer first. Not changed here.
- Next: decode the code before line 395 (what `pad` holds) and the host-side fill of the
  detector state. Without them, step 4 has no reference to match.

## 15. Detector runs to completion on a stand-in state (pass 9, Unicorn, 2026-10-21)

Harness: `re-tools/emulate/detector_run.py`. Evidence scope: isolated emulation with a synthetic
host and stand-in state. Nothing here is a plug-in default, and no parity is claimed.

### Fault chain fixed, in the order it was hit

1. **Bin and weight fill** through the record dispatcher `0x621d50`: type 0 -> bins1 (S+0x770),
   1 -> bins2 (S+0x788), 2 -> W1 (S+0x7a0), 3 -> W2 (S+0x7b8). Payload = float array, size in bytes at
   record+0x30, data pointer at record+0x28, type int at record+0. Values used: 1025 linear bin
   frequencies (step fs/(2*1024)) and unity weights. Not the plug-in's values.
2. **Map lookups** `0x563970(tree = *(S+t), key = *(S+k))`: when the tree's flag byte (+0x48) is 0 the
   function is a no-op. Stand-in: empty flag-clear trees at S+0x550, S+0x4b0, S+0x570. Their real
   contents are host state and are unknown.
3. **Result object**: `*(S+0x540)` must be a valid object (read at +0x2c/+0x3c by `0x17bb1e0`).
4. **Group count**: `**(S+0x4a0)` is the int 1 (cell pointed to by S+0x4a0).
5. **Flag byte**: `**(S+0x560)` is 0.
6. **Frame (detector arg1) is a channel list**, not a sample struct: `+0x38` = qword pointers to
   channel sample buffers, `+0x40` = channel count, `+0x48` = samples per channel N. With count 0 the
   copy loop at `0x619fc0+0x52..0x76` takes `[rbp+0x880] = 0` as a buffer pointer and the fold writes
   through address 0 (the fault seen first).

### Output and the blocker

- The detector returns (`0x20033fb8`), and writes 2022 floats into the host block that `*arg3`
  points to, at offsets 0x68..0x1ffe. Values are 0.0 at the lowest indices and 1.0 above them.
- **Input independence:** silence, a 1 kHz sine at 0.9, and white noise at 0.9 give the same
  output. So with this state the detector does not respond to its input. Until that is explained,
  the output is not a reduction curve and no oracle can be drawn from it.
- Likely gates (not yet read): the depth and detail settings in the host block (`*arg3` floats at
  indices 0x398, 0x97, 0x3cb, 0x3ac, 0x0, 0xeb4 read by the entry checks), and the attack/release
  states in S. Their defaults are the missing host state.

### Consequences

- No oracle JSON in this pass. Impulse delay, output level and the 1024-point reduction curve are
  still unmeasured. Latency stays `n - 1` in SMOOTH; the PDC rule waits for a measured delay.
- Step (3), the port, is blocked on two unverified inputs: the fold indices (the lifter; section 14 says
  "keep one half, zero the rest", an interpretation, not decoded) and the host default block. A port
  now would invent those constants, so it was not started.
- Next: decode the entry checks in `0x619fc0` (lines 98-135 of the annotated file) to find which host
  floats gate the output; then decode the fold indices from the code between lines 395 and 450.

## 16. Why the detector ignores its input: the FFT kernels never see the samples (pass 10, Unicorn, 2026-10-21)

Harness: `re-tools/emulate/detector_run.py` (fixed) and `re-tools/emulate/detector_probe.py` (new).
Evidence scope: isolated emulation, synthetic host, stand-in state. Nothing here is a plug-in default.

### Corrections to section 15

- **Frame object too small.** `build_frame` allocated `arg1` as 0x100 bytes, and the detector
  writes its working block at `arg1+0x120..0x2190`. The fix is a 0x4000-byte frame.
- **Samples were never written.** `build_frame` allocated the channel buffer but did not fill it.
  The section 15 run was therefore on silence. Fixed: `build_frame(emu, samples)`.
- **The output is the host block, not `arg1`.** `*arg3 + 0x68 .. 0x1ffe` (2022 floats) is the output
  and it is written by the detector at `0x61adff` and `0x61af58` (weighted-average and interpolation
  stores). The block at `arg1+0x120` holds work values (constant 242.526 at index 10 onward here) and is
  not the curve.
- **Caller and frame.** `ReductionCurve::slot2` (`0x6b96f0`) passes `*(this+0x288)` as `arg1`, the engine
  as `arg2`, the host block as `arg3`, and `(origin, extent)` as the rect (`x0,y0` then `w,h`). The
  harness rect convention matches. `this+0x288` is written by `0x3829b0` (`003829b0.c:100`, which the
  name hint calls `juce::TimeSliceThread::slot1`; the class is unclear and was not checked further).
  That writes the object's vtable-bearing pointer, so it is a candidate producer, not confirmed.

### Verified in this pass

1. **The output is a function of the host weight tables only.** With a flat weight W on both
   vectors, the output is `W` scaled and clamped (W = 1e-4 -> 0..1, W = 1 -> 1.0, W = 100 -> up to
   10000). A sine at amplitude 0.9 and silence give identical output at every W tested.
2. **The inverse, forward and magnitude kernels never receive the samples.** `detector_probe.py`
   hooks the entries of `0x180007000` (inverse), `0x180001640` (forward) and `0x1806181b0`
   (magnitude). Their inputs are `[1, 0, 1, 0, ...]` and `[1, 0, 0, 0 ...]`, built from the weight
   vectors, for silence and for the sine, at group count 1 and 2.
3. **The sample buffer is read**, at `0x61ac90` and `0x61ad4c..0x61ad7a` (about 1024 reads per group),
   but those reads do not reach the output in this state.
4. **No data-dependent branch on the signal path.** The executed-instruction sets for silence and a
   0.9 sine are identical (7164 addresses each, zero difference).
5. **Saturation.** Amplitudes 0, 1e-6 .. 0.9 all give the same output at group count 1. The values
   written to the host block at `0x61adff` and `0x61af58` are exactly 1.0 for silence and sine.
6. **Group count changes the output, not the dependence.** `S+0x4a0` (`**` gives the count) at 2
   gives values up to about 2e4, still amplitude-independent. The stand-in count of 1 selects the
   `cStack_aa8` branch (`uStack_a38 == 1`), which skips the frame branch.
7. **Gate constants (read from the image).** `cStack_aa6` is set when a host float is below its
   threshold: `host[0x398] < 0.05`, `host[0x97] > 0.5`, `host[0x3cb] > 0.5`, `host[0x3ac] < 0.01`,
   `host[0] < 0.1` (`K1..K4` at `0x18209d0a0`, `0x18209d1c0`, `0x18209d060`, `0x18209d0c8`). An all-zero
   host sets it. Opening those five host values does not change the amplitude dependence, so
   `cStack_aa6` is not the gate that closes the output. `cStack_aa7` is `**(S+0x560)`, and `0` here.
8. **Clamp range.** `log10` of the weights is multiplied by 20 and clamped to `[-22, 19.8]` dB (the
   constants `0x18209da48 = -22`, `0x18209d6c8 = 20`, `0x18209d6c4 = 19.8`, `0x18209d088 = 1/38`).
   With W = 1 the dB term is 0 and the output saturates at 1.0.

### What this means for the blocker

The detector is not gated by a host float in this state. It is fed the weight tables instead of the
frame, because the frame channel list arrives in a state we have not built. The sample pointer is in
`arg1+0x38`, and the FFT input path is built from `S+0x7a0/0x7b8` (`uStack_9c8`), so a stand-in
with unity weights will always produce a constant output. The real input comes from the audio-thread
process path that fills `ReductionCurve+0x288`. That path (`0x890a00` per the section 6 notes) is
not emulated.

### Still open

1. The producer of the channel list at `this+0x288` (process path `0x890a00` and its callers). Run it
   from its entry with a real `AudioBuffer`-like host object, then check that the kernel inputs depend
   on the sine.
2. The source of the group count `S+0x4a0` and the real weight vectors. Both are host state (section
   10). At 48 kHz the constructor gives 1 group, which would select the frame-free branch. Either the
   host sets a different count at 48 kHz, or the frame path is reached some other way.
3. Steps 2 and 3 of the plan (fold indices, oracle) wait on (1). No oracle JSON was written in this pass.
4. No SMOOTH change: `dsp.rs` is untouched, so no build, pluginval or CLAP run was needed for this pass.

## 17. Process block 0x890a00: entry, staging, gates and the emulated oracle (pass 11, Unicorn, 2026-10-21)

Harness: `re-tools/emulate/process_lib.py` (session, host, probes, fault report), `process_oracle.py`
(oracle), `detector_reach.py` (reachability), `disasm_rva.py`. Outputs: `oracle/process_block_<rate>.json`.
Evidence scope: isolated emulation, synthetic host, stand-in state (2 channels, block N = 512, every
band and output parameter at the constructor's zero state). Nothing here is a plug-in default or parity.
Labels: **verified** = checked by emulation or by the bytes; **read** = from the code, one input inferred;
**inferred** = not checked.

### 17.1 Corrections to earlier sections

- **The entry is 0x890a00.** The corpus splits one function at 0x890c92 (rows 0x890a00 size 658 and
  0x890c92 size 12101). The real function is 0x890a00..0x893bd7 (12759 bytes); 0x890c8c falls through
  into 0x890c92. **verified** (disassembly).
- **The constructor was run with zero host values (sections 11-16).** The constructor reads host id 10
  (`input_channel_layout`) and id 12 (`samplerate`) as floats and sizes its three ring structs from them
  (ring setup in `0x88fa90`, decompile `0088fa90.c` ~l.403-420: channels from the layout, length =
  groups << 12). With zeros the rings had no channels. Fixed in `fake_host.run_ctor` (defaults: layout
  2.0, samplerate = rate). Latency is unchanged: 2304 / 2304 / 4704 / 9408 at 44.1 / 48 / 96 / 192 kHz.
  **verified**. The detector stand-in of sections 15-16 changed shape: it is now 1.0 for silence, 0.001 and
  0.9 (it was a 0-to-1 ramp). Input independence still holds. **verified** (`detector_run.py`).

### 17.2 Entry, calling convention and host

- Wrapper: 0x58 bytes built by the factory `0x88f430` (called from `0x88e3a0`, which has no recorded
  callers, so the host that builds the wrapper is not identified). `[+0]` = DSP object. Slots: `+0x08`
  setParam (thunk `0x88f4e0` -> `0x894370`, a 143-case switch); `+0x10` process (thunk `0x88f4f0`:
  `mov rcx,[rcx]; jmp 0x890a00`); `+0x18` prepare (`0x88f500`: `0x894fc0(obj, dl=1)`, then `[obj+0x4a]=1`);
  `+0x20` teardown (`0x88f530`); `+0x28` getLatency (`0x88f610`); `+0x50` flag setter (`0x88f660`:
  which 1/2/3 -> `[obj+0x235]`, `+0x236`, `+0x237`; value = r8 > 0). **verified** (disassembly).
- `process(this = rcx, out = rdx, in = r8, side = r9, N = [S0+0x28])`. Tables at rdx and r8 each have
  `[this+0x10]` pointers. Staged frames have `[this+0x14]` channels and are built by `0x894b00` (a struct
  constructor: channel pointers, `+0x80` channels, `+0x84` N, `+0x88` N*4). `side == 0` stages the frame
  from the r8 table, otherwise from the r9 table. **verified** (branch at `0x890bee`; `0x894b00`).
- Output is written in place into the rdx table; the input is the r8 table. **verified** (impulse on r8
  -> output on rdx at sample 2304; impulse on rdx -> zero output).
- `obj[0]` is the host. Host `+0x20` is get/set `(host, id, out)`; host `+0x28` is a bool query
  `(host, id)` (`0x892dc4`, id 9). Registered ids: 0-8 reduction-curve, offset-curve, detail, attack,
  release and max-cut; 9 `band_solo_channels`; 10 `input_channel_layout`; 12 `samplerate`;
  13 `sidechain_input_level` (written back by the block near `0x893928`). **verified** (registration
  strings, call sites).
- Channel counts: `[obj+0x10]` = `0x56cad0(cfg+0)`, `[obj+0x14]` = `0x56cad0(cfg+8)`. Layout index 2 ->
  2 channels (index 1 -> 1). **verified** (emulated `0x56cad0`).

### 17.3 Per-block structure (read, partly verified)

- Entry copies both pointer tables into locals (`0x890a00`..`0x890b9f`) and stages the frame at
  `0x890bee`..`0x890c0a`. The body (`0x890c92`..) loops over hops with per-channel offsets
  (`iStack_3370`, decompile ~l.1880-1925).
- Working state is in the three ring structs at `obj+0xbff8`, `+0xc088`, `+0xc118`: `[+0x80]` write
  position (advances by N per block, **verified**), `[+0x88]` length in floats, `[+0x8c]` channels.
- Kernels: `0x8973f0` copies a frame into the ring through `0x89ba70` (which splits the ring at the write
  position, **read**); the copy wrapper `0x10a0` returns at once for a null destination (`0x2aed`..`0x2af0`,
  **verified**); the add kernel at `0x5e9a0` is `dst += src` (**verified**).

### 17.4 Gates (verified)

- Mode: `0x893de0` (the analysis helper, called from the body's `*(this+0x2c) < 2` branch, decompile
  ~l.2286) reads the analysis-mode global `0x26d7bb8` (= 4 after the constructor). Mode 4 -> `0x8a3a90`,
  mode 1 -> `0x8add30`. **verified** (`0x893e52` probe, eax = 4).
- Zero-latency gate: the byte `S0+0x28` (the fifth argument slot, reused as a flag) is set when
  `[obj+0x237]` or `[obj+0xab]` is nonzero (`0x891758..0x891775`; `r14b = [obj+0x237]` at `0x890d49`).
  `0x8a3a90` tests it: nonzero -> convolver chain `0x8acc30 -> 0x8b1630 -> 0x8dec70`; zero -> `0x894d20`
  and `0x8ad640`. **verified** (probes and executed-PC sets, both modes).
- getLatency (`0x88f610`): base alone when `[obj+0x237]` or `[obj+0xab]` is nonzero; otherwise
  `groups * f + base` with `f = 0x894d20(obj+0x5f8) = 2304`. **verified**.
- The M/S helper `0x8a3b20` is not executed in either mode (stereo-mode byte `+0xa6` is 0 in the
  stand-in). **verified**.
- `[obj+0xab]` is written by the setter `0x894370` case 1, a boolean `value >= const(0x18209d1c0)`.
  **read**. Which host value sets `+0xab` or `+0x237` in the plug-in default is **open**.

### 17.5 Oracle (`oracle/process_block_<rate>.json`; 26 blocks of 512; impulse in block 0)

| rate | mode | getLatency | impulse delay (ch0 / ch1) | nonzero output samples | noise cross-correlation lag |
|---|---|---|---|---|---|
| 44.1 kHz | 0 (default stand-in) | 2304 | 2304 / 2304 | 1 | not run |
| 44.1 kHz | 1 (zero-latency) | 0 | 0 / 0 | 1 | not run |
| 48 kHz | 0 | 2304 | 2304 / 2304 | 1 | 2304 |
| 48 kHz | 1 | 0 | 0 / 0 | 1 | 0 |
| 96 kHz | 0 | 4704 | 4704 / - | 1 | not run |
| 96 kHz | 1 | 96 | 96 / - | 1 | not run |
| 192 kHz | 0 | 9408 | 9408 / - | 1 | not run |
| 192 kHz | 1 | 192 | 192 / - | 1 | not run |

- Impulse delay equals getLatency at every rate and in both modes. **verified** (the JSON check in
  `process_oracle.py` asserts it).
- Level (48 kHz, 1 kHz sine at -40, -20, -6 dBFS, both modes): 0.000 dB at every level, residual against
  the delayed input at most 6e-7 relative. Level-independent, but this is the identity of the stand-in
  (all bands at zero state): the path passes the signal and exercises no gain. **verified (identity only)**.
- Output is signal-dependent (the delayed input), as expected for a pass-through. **verified**.

### 17.6 Display-only claim for 0x619fc0

- `0x619fc0` is not executed in four blocks at 48 kHz, in either mode (`detector_reach.py`: zero PCs in its
  6046-byte span). **verified, for this stand-in state.** This supports the claim; it is not final, because
  the stand-in never leaves the identity band state.
- The process path does execute the shared magnitude kernel `0x6181b0` (|Y|), eight calls per block, from
  `0x8e13a8` inside the helper `0x8e0858` (17290 bytes). That helper also calls `logf`, `powf` and `expf`
  (for example `logf(0.001)`, `logf(2.083)`, `powf(10, -3)`, `expf(-1.0996)`). It is the likely
  magnitude-to-gain mapping and is entered four times per block from an unresolved caller (indirect or
  tail call). **verified (calls); role inferred.** The brief's `-ln(max(m,1))` form was not seen among
  the calls, and the formulas were not decoded.
- `0x6181b0` also has a caller in the detector (`0x619fc0`, one site), so the magnitude kernel is shared.
  The claim above is about the detector function only.

### 17.7 Iteration log (fault-driven, 4 iterations, 3 fixes)

1. Fault: null indirect call at `0x892dd3` (`call [host+0x28]`). Fix: host bool slot `+0x28` (stand-in 0).
2. Block 0 silent and block 1 faulted with a write to 0x800 (N*4 past null) in the copy kernel `0x1d52e00`
   via `0x8973f0`. Root: the constructor's host gets 10 and 12 returned 0, so the rings had no channels.
   Fix: typed host values before construction (`fake_host.run_ctor`).
3. Impulse absent from the output in both tables. Cause: the harness put the input in the rdx table.
   Fix: input in r8, output read from rdx. The impulse then appears at 2304.
4. Zero-latency gate: no fault, set by a flag byte; probed and used as mode 1.

The block completes for 26 blocks at all four rates in both modes. Iteration cap (40) not reached.

### 17.8 Open

- The band and output parameters. The 143-case setter `0x894370` writes the DSP parameters, and the
  host defaults were not decoded. The stand-in is the identity, so depth, sharpness, selectivity, output
  gain, dry/wet and the magnitude-to-gain mapping are unmeasured. Next: map the switch cases to fields and
  set a non-zero band state, then repeat the level test.
- Which host value selects zero-latency mode (`+0x237` or `+0xab`) in the plug-in default. The SMOOTH
  latency question (n - 1 = 2047, section 5) depends on it: mode 0 gives 2304 at 44.1 and 48 kHz, mode 1
  gives 0. The PDC rule stays open.
- The caller of the factory `0x88e3a0`, and of the helper `0x8e0858`.
- Not done: no SMOOTH source change, no PDC change, no plug-in parity, no native host or pluginval run.

## 18. Plan geometry in Rust (translate-process pass, 2026-10-21)

- `src/reference_plan.rs` holds the verified geometry: `groups = max(1, trunc(rate/44100))`,
  `vec_count = 48*groups`, `base = 48*groups` for groups >= 2 else 0, and
  `latency = groups*2304 + base`, or `base` alone when the zero-latency byte is set.
- Checked against all 20 oracle rows (`ctor_latency_mode{4,1}.json`, `process_block_<rate>.json`):
  the constructor rows and the process-block impulse delays agree. **verified** (python cross-check and
  the two unit tests in `reference_plan.rs`).
- **Not wired into `Engine`.** The engine still delays by its own `n - 1` (4095 at 44.1 kHz normal),
  so reporting 2304 now would move PDC away from the audio. Wiring waits for the engine to produce the
  reference delay.
- Still open, unchanged from section 17.8: the 0x890a00 analysis and output math, the 143-case setter
  0x894370, the gain call site (0x8e0858, other branch), and the zero-latency byte's source in the default.

## 19. Staging, ring and identity output in Rust (translate-analysis pass, 2026-10-21)

- `src/reference/analysis.rs`: `align16` (the 0x8ad640 workspace round-up, verified arithmetic), `Ring`
  (fields `+0x80` write, `+0x88` length, `+0x8c` channels; stage with wrap, masked advance from 0x893de0),
  the `BinGain` boundary for 0x8e0858 (trait, closures accepted), and `IdentityDelay` (the output path with
  the reported latency as its delay).
- Differential fixtures: `re-tools/emulate/process_fixture.py` writes `oracle/process_fixture_<rate>_m<mode>.json`
  (seeded uniform noise in, emulated output out, 26 blocks of 512, identity band state). Checked in Python:
  `max |y[n] - x[n-L]|` is at most 3.3e-7 at every rate and both modes. **verified (emulation, identity state only).**
  The Rust test `emulated_identity_fixtures` reproduces these to 1e-6.
- **Not translated:** the window, the hop loop body (0x890c92..), the FFT calls and the overlap-add. The
  per-hop math sits in 0x8acc30 / 0x8b1630 / 0x8dec70 (latency > 0 path, from 0x8a3a90) and in
  0x8b61a0 / 0x8b7410 (called from 0x8ad640, the default path). These were not decoded in this pass.
  `IdentityDelay` is a stand-in for that output path, not a translation of it: its ring read rule
  (`start - latency`) is inferred from the delay, not from the code.
- Ring length `groups << 12` is from section 17.1 (read), not re-checked here.

## 20. Normal path call tree (translate-analysis pass 2, Unicorn trace, 2026-10-21)

Tool: `re-tools/emulate/call_trace.py` (commit 57e4d2a59). It hooks every `call` site of the text range
(decoded per corpus row: a linear sweep stops at the first data island) and logs callee, the four Win64
register arguments, the two stack arguments and, with `--peek N`, the first N floats behind each pointer
argument before and after the call. Run: `python -I call_trace.py --rate 48000 --block 2 [--peek 4]`.
Evidence scope: identity band state, mode 0 (default stand-in), synthetic host, seeded noise input.

### 20.1 Shape of one 512-sample block (48 kHz, mode 0), verified by the trace

- Entry 0x890a00 -> 0x893de0 (mode gate) -> 0x8a3a90 -> 0x894d20 (latency term, 2304) -> **0x8ad640**.
- 0x8ad640 does the workspace bump (align16, verified arithmetic), then calls exactly two workers:
  **0x8b61a0** (analysis side, arg `obj+0x2c50`) and **0x8b7410** (synthesis side, arg `obj+8`).
- 0x8b61a0 has one direct child that matters, **0x8c5f30** (the hop loop). 0x8b7410 has **0x8c68a0**
  (its hop loop) plus the ring work at 0x899ab0 / 0x899cd0 (three and one calls per block).
- **Hop size 128** (`r8 = 0x80` into 0x89b760 and every 0x10a0 copy), **FFT length 256** (`0x1cfd8f0` with
  `r8 = 0x100`; per-hop frame 0x100 floats). The hop loop runs **4 times per 512-sample block**
  (each of 0x8d9230 / 0x8dad80 appears once per hop). **verified (call counts and arguments).**
- Per block, call counts: `0x1010` (real forward, 24 calls), `0x1280` (real inverse, 24), `0x1190`
  (elementwise multiply, 16, each reaching `0x278c0`), `0x6181b0` (|Y|, 8, from `0x8e13a3`), `0x10a0`
  copy (52), `0x10d0` zero fill (58 across the block). **verified.**
- Call sites inside the hop loop of 0x8c5f30 (one hop, both channels):
  memcpy of 16 bytes from a double parameter block (`0x17bb8d6` = IAT memcpy), `0x8e0390` x2 (input
  staging per channel: `0x89b760` then copy `0x10a0` of 128 floats), `0x8d9230` (window/fold stage: calls
  `0x8d98a0`, which reaches `0x8e04c0` = gain helper, and `0x8e7000`, `0x8e7150`), `0x8dad80` (copy of 128
  floats into the output side, through `0x89b760` and `0x10a0`).
- Forward/inverse in the analysis worker: `0x8e11bf` calls `0x1010` (r8 = spec `0x40339f00`, r9 = work);
  `0x8e675d` calls `0x1010` and `0x8e6f05` calls `0x1280`. `0x8e6a23` calls `0x1190` (multiply, dst rcx,
  a = rdx = window-like table `0x404c1c80`, b = r8 = frame, n = 0x100). **verified (argument pattern);
  roles inferred from order.**
- The thunks `0x1010`, `0x1280`, `0x10a0`, `0x1190`, `0x10d0`, `0x1220`, `0x1100`, `0x1340`, `0x1400`,
  `0x1580`, `0x11f0` are all jump-table dispatchers (`movsxd` of a runtime selector, `jmp [table+sel*8]`).
  **verified (disassembly).**

### 20.2 Values seen at the first hop (block 2, peek 4)

- `0x8c5f30` reads double-precision parameters from `rcx`/`rdx` (pairs `[x, 0]` are doubles, not floats).
  These are not audio; the parameter-block role is open.
- The audio staged for the first hop starts `[-0.2716, 0.2371, -0.1762, -0.1141]` (channel 0), matching the
  seeded noise, so the 128-sample copy in `0x8e03fa` is the input hop. **verified (values).**

### 20.3 Not done (what remains for the normal path)

- No Rust translation of the hop math in this pass. The stages above are mapped, not decoded.
- Still to decode, each as its own function with a fixture: `0x8c5f30` (hop loop body, parameters),
  `0x8e0390` / `0x89b760` / `0x89b930` (staging and copy), `0x8d9230` / `0x8d98a0` / `0x8e7000` / `0x8e7150`
  (window and fold), `0x8dad80` / `0x8c68a0` / `0x8db030` (overlap-add and output side), `0x8e67a0` and
  `0x8e53c0` (ring split and output read), `0x899ab0` / `0x899cd0` (ring read), the window table at
  `0x404c1c80` (its constructor formula is not yet read), and the FFT scale (1/N, software).
- Zero-latency path (0x8acc30, 0x8b1630, 0x8dec70): not traced in this pass.
- The yield of the 2304-sample delay per stage is not yet attributed to a read offset in the ring.
