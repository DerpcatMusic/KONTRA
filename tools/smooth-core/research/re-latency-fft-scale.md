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
