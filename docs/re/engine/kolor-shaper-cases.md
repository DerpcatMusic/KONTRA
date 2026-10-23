# Kolor shaper cases 0, 1, 3, 4: dispatch findings

Status: **cc70 stage recovered and verified; case 4 ported (output 0.0 natively, see below); c830 (state update) and cases 0, 1, 3, 6-9 recovered and verified as one kernel (`Shaper6_9`); not yet wired into the dispatch (control lane).**
Scope: decompiled source in `/data/dev/workspace/Research/KONTRA/source/kontakt8/` (`0141970000.c`,
`0141940000.c`), the dispatch disassembly `docs/re/disasm/kolor-shaper-dispatch-1419749f0.txt`, and the raw
disassembly of the shaper functions in `docs/re/disasm/kolor-*.txt` (radare2 `pdf`, read by hand, no decompile).

## Recovered: cc70 and the cfb0 sample (cases 6-8)

`cfb0` (`14194cfb0`) takes the shaper and `&x`. Verified against the binary: its returned `out[0]` is the
`cc70` scalar with `P = shaper+0x30`, `V = [x, s0..s7]` where `s = shaper[0..8]`. Max error 2.5e-8
(`tools/re/kolor_shaper_6_9_vectors.py`, `cfb0_verify` run).

With `M1 = P[0x000..0x120]`, `b1 = P[0x120..0x140]`, `M2 = P[0x140..0x240]`, `off = P[0x240..0x260]`,
`w = P[0x260..0x280]`, `c = P[0x280]`, matrices column-major (`M[c*8 + r]`):

```
A = M1 V + b1          (141946590, 9 -> 8 lanes; 141932e70 is the generic matmul it calls)
Q = M2 A + off         (14192d1b0, 8x8 matmul on A; off added in cc70)
X = Q / sqrt(Q^2 + 1) + A   (cc70 lanes; rsqrtps + one Newton step, masked for Q^2+1 = inf)
y = w . X + c          (141946410, single-float dot + bias)
```

The Rust kernel is `crates/sampler-core/src/dsp/color/kolor/shaper_6_9.rs` (`Cc70::sample`). Tests:
`cc70_matches_native` (24 random blocks) and `cfb0_sample_matches_native` (12 random shapers), both at 1e-5.
Fixtures: `tools/re/fixtures/kolor_shaper_6_9_cc70.json`, `..._cfb0.json`, plus per-mode wrapper drives
`kolor_shaper_{6,7,8,9}_wrapper.json` from `kolor_kemu_probe.build`/`block`.

## c830: the state update (recovered, GRU)

`14194c830` is a GRU step: input size 1, hidden size 8. Recovered from the decompile
(`/data/dev/workspace/Research/KONTRA/source/kontakt8/0141940000.c`, `FUN_14194c830`) and the raw
disassembly (`docs/re/disasm/kolor-shaper-14194c830.txt`), then checked against the emulator.

With `P2 = shaper+0x2c0` (0x108 floats), `s` the state (`shaper[0..8]`), `x` the input:

```
A  = M s + b                      M 24x8 column-major at P2[0x30..0xf0], b = P2[0xf0..0x108]
r  = sigmoid(x p[i]   + p[0x18+i] + A[i])       i = 0..8     (141943870 = 0.5 tanh(0.5 v) + 0.5)
z  = sigmoid(x p[8+i] + p[0x20+i] + A[8+i])     i = 0..8
n  = tanh(r * A[16+i] + x p[0x10+i] + p[0x28+i])              (ptanh, 141943490)
s'[i] = (1 - z) n + z s[i]
```

Helpers, all read: `1419466a0` (the 24-row `M s + b`, via the generic GEMM `141932e70`), `141943870`
(8-lane sigmoid), `141943490` / `141943550` (4-lane ptanh; `141943550` returns the same result in xmm0),
`141932e70` (generic GEMM, used with rows = 24, inner = 8, column-major).

Verified: `141943490` and `141943870` against native over a sweep (max 1.4e-7 and 1.2e-7);
`c830` alone against the formulas above (max 7.6e-8). Fixture: `kolor_shaper_6_9_c830.json`.

The C830 block `P2` is written by the control lane. The kernel takes it as given.

## Case 9 (1419748b0): same math, other calling convention

`1419748b0` reads x through `ctx+0x10` (a pointer to a pointer to x), takes the shaper in `rdx`, and
returns y in xmm0. Its body is the cfb0 sequence: cc70 with `P = shaper+0x30` (y, old state), then c830
with `P2 = shaper+0x2c0`, then the 8-float result written back to `shaper[0..8]`. Verified against native
over warmed 12-sample runs (`kolor_shaper_6_9_sequence.json`, `cases9`).

## Kernel status

`sampler-core/src/dsp/color/kolor/shaper_6_9.rs`: `Shaper6_9` (state carried, `load_params` for control
writes). Tests: `ptanh_and_sigmoid_match_native`, `cc70_matches_native`, `cfb0_sample_matches_native`,
`c830_step_matches_native`, `shaper_sequence_matches_native` (cfb0 and case 9, steady and moving params).
All at 1e-5. The module stays `#[allow(dead_code)]` and unwired: the dispatch (`1419749f0`, control lane)
is not connected to it yet.

## Open

- Wiring cases 6-9 into the dispatch (control lane; `kolor/mod.rs`).
- Case 4 (`14197a150`) is resolved on main (see the case 4 sections below). Cases 0, 1, 3 are ported (see above).
- Parameter fill of `P` and `P2` (control lane).

## Dispatch (1419749f0, `FUN_1419749f0`)

- Selector: `switch((char)param_2[0xde])`, i.e. the signed byte at shaper+0x6f0. Cases 0..9.
- Cases 2 and 5 are stateless (`14197adb0`, `14197ae80`) and belong to the control lane.
- Cases 6, 7, 8 call `14194cfb0`; case 9 calls `1419748b0`. Both are outside this lane.

## Per-case body (verified from decompile)

| Case | Body | Notes |
|------|------|-------|
| 0 | input x = `*param_1[2]` → 9-float `V = [x, s0..s7]` (from `shaper[0..8]`, read in full) → `14194cc70(shaper+0x30, y, V)` → `14194c830(shaper+0x2c0, ..., x)` → store the 8-float result back into shaper+0..0x1c; returns `y` (xmm0, the cc70 output) | Verified from the disassembly (`docs/re/disasm/kolor-shaper-dispatch-1419749f0.txt`, 141974a2e..141974b3d). Same as cfb0 (cases 6-8). |
| 1 | `1419426e0` wraps x into a 1-element array → `141974860` pushes an 8-float record into a ring (slot stride 9 floats) → `cc70` on the ring as above | Same code path as 3. |
| 3 | identical to case 1 | Confirmed by reading both arms. |
| 4 | x → `14197a150(shaper, out, &x)`, which calls `141979f60(shaper+0x20, ...)` then `141979bf0(shaper+0x208, ...)` and stores 4 floats back at shaper+0..0xc | Not yet read below these two helpers. |

## Cases 0, 1, 3: ported (2026-10-23)

Read from the dispatch disassembly (`141974a2e` case 0, `141974b75`/`141974c02` cases 1/3) and checked against
the emulator. All three compute the cfb0 sequence, so they share `Shaper6_9` (`shaper_0_1_3.rs`: `Shaper0`,
`Shaper1_3` aliases).

- Case 0: `V = [x, s0..s7]` in a stack buffer (`[rsp+0x68]`, state copied to `[rsp+0x6c..0x8c]`); `cc70` writes
  `y` at `[rsp+0x20]`; `c830` (P2 = shaper+0x2c0, x at `[rsp+0x30]`) writes the new state to `[rbp-0x70]`; the
  state is stored to shaper[0..8]; `y` is returned in xmm0.
- Case 1/3: `1419426e0` makes a 1-element view of the data buffer `[rbp-0x30]` and writes `x` at float 0.
  `141974860` pushes `shaper[0..8]` at float 1..8 (ring: `off = 0 + cap(1)`, count 1 then 0, stride 9). So
  the buffer is `V = [x, s]`, and `cc70` reads it through `*V`. `c830` and the state store are as in case 0;
  return is the cc70 `y` at `[rsp+0x28]`.
- The ring state (count, offset, cap) is not read by `cc70` (checked: the native vectors match with it ignored).

Fixtures: `tools/re/kolor_shaper_0_1_3_vectors.py` drives the dispatch with the selector byte at shaper+0x6f0 (0,
1, 3), 12 samples per warmed run, steady and moving P/P2 at samples 3, 6, 9. Output: `kolor_shaper_0_sequence.json`,
`kolor_shaper_1_3_sequence.json`. Tests: `case0_sequence_matches_native`, `case1_3_sequence_matches_native` at 1e-5.

Not measured: which modes select cases 0, 1, 3 in the wrapper (only the dispatch is driven, not `14197a2c0`).

## Helpers

- `14194cc70` (cc70), `141946590`, `14192d1b0`, `141946410`: read and verified, see the recovered section above.
  `14194cc70` uses the P-block offsets above, not `shaper+0x140` (that was the cfb0-relative view).
- `14194c830` (c830): recovered, see above. `141979f60`, `141979bf0`: case 4 chain, not read to completion.
- `1419426e0` (253 B): std-array-style 1-element wrap (copies the input into a proxy). Not DSP.

## Open (case 0, 1/3, 4, and the kemu vectors for them)

- Case 0, 1/3, 4 output math depends on the unread helpers above (`141979f60`, `141979bf0`, `141978300`,
  `141974f10`). Read them with `radare2 -c 'pdf @ ...'` on `/data/dev/workspace/re-binaries/Kontakt8.exe`
  before writing any Rust.
- Kemu vectors for these cases need a warmed multi-block run via `kolor_kemu_probe.py`. The probe currently
  only drives the wrapper driver (`14197a2c0`), so the shaper state must be seeded through it.

## Case 4 chain, read from raw disassembly (2026-10-23)

Raw disassembly cache: `docs/re/disasm/kolor-shaper-<addr>.txt` (`r2 -e scr.color=0 -c "s 0x<addr>; af; pdf"` on
`re-binaries/Kontakt8.exe`). Scope of this pass: call graph, wrapper I/O, and the tanh core. No Rust yet.

Corrections to the table above:

- Case 4 wrapper `14197a150` writes **7 floats** back, not 4. Stores `[rbx+0..0xf]` (xmm, 4 floats),
  `[rbx+0x10..0x17]` (2 floats), `[rbx+0x18]` (1 float), i.e. shaper state `s[0..7]`. Input `x = *arg3`.
  Before the call it builds a 1-element temp `B = {x}` and a 7-float copy of `shaper[0..7]` at `var_35h`.
- Call order: `14197a150` → `141979f60(this=shaper+0x20, out=rdi, in=&B)` → `141979bf0(this=shaper+0x290, out, in=..., state=shaper)`.
- `141979f60` (489 B): calls `141977650(this+0x100 ...)` (8-lane copy-add, 1-element input expanded through
  `141932e70`), `14192d1b0` (wrapper over `141932e70`), then `141946410` (dot-product helper, 8-lane
  `a·b` horizontal-add), with `addps`/`rsqrtps` normalisation. Returns through `rdi`.
- `141979bf0` (879 B) calls `141978300` (153 B, not read), `141974f10` (x2, 81 B, not read), `141943490` (4-lane tanh, below),
  and `14192d0e0` (scalar tanh, below) three times.

### Tanh core: `14192d0e0` (scalar) and `141943490` (4-lane)

This is **Eigen `ptanh_float`**, confirmed by its constants (float bit patterns decoded from the binary):

| symbol | value | role |
|---|---|---|
| clamp hi/lo | ±7.905311107635498 (`0x40fcf84f`) | input clamp |
| abs mask | `0x7fffffff` | |
| small-x threshold | 0.0004 (`0x39d1b717`) | if `\|x\| < 0.0004` return x |
| num a13..a7 | -2.7607683663038313e-16, 2.0001879384549948e-13, -8.604671836165423e-11, 5.122297253024044e-08, 1.4857223504805006e-05, 6.372619536705315e-04, 4.893524572253227e-03 | `p = x*(((((a13*x2+a12)*x2+a11)*x2+a10)*x2+a9)*x2+a8)*x2+a7)` |
| den b3..b0 | 1.1982583600911312e-06, 1.1853470641653985e-04, 2.2684347350150347e-03, 4.8935250379145145e-03 | `q = ((b3*x2+b2)*x2+b1)*x2+b0` |

`tanh(x) = clamp(x) * P(x2) / Q(x2)` with `x2 = clamp(x)^2`, and `x` itself when `|x| < 0.0004`.
The scalar version (`14192d0e0`) does the same with one `divss`. Note: the `a11` constant is stored negated
and applied with `subss`.

Still open: `141932e70` (generic 8-lane helper used by `141977650`, `14192d1b0`, `141946590`), `141978300`,
`141974f10`, the state layout behind `shaper+0x20/0x100/0x120/0x240/0x290`, and the warmed-up kemu fixtures.
Nothing in this section has been run under unicorn or compared to native output yet.

## Case 4: resolved (2026-10-23)

Output is **0.0 for every input** in the shipped binary. Measured, not inferred:

- The output is `w . v + b` (`141946410`, a dot product of the 8-lane state `v` with 8 weights at
  shaper+0x260 plus a bias at shaper+0x280). Those 9 floats are zero after every static init we run, and no
  decompiled code writes them. `w` and `b` are loaded from `DAT_14a64ba20`, `ba30`, `ba40`, which sit past the raw
  `.data` bytes (`0xa64ba20` vs raw end `0xa588000`), so the loader zero-fills them.
- No knob change writes those bytes (knobs 0-3 and 5-12 stepped; knob 4 is the mode and reseeds the state only,
  knob 13 untested). No per-block write touches them. Output over 1709 calls
  (inputs to 3e38, random, long run) is exactly 0. State stays finite (|s| <= 0.51).
- The recurrent 7-float state (`14197a150` write-back) is real and evolves, but nothing reads it into the output,
  so it is not ported. The Rust case returns `0.0` and its tests check it against the native vectors.

**Caveat:** the claim depends on no init we did not run writing `ba20..ba40`. Nothing in the decompile does, and
the PE has no `.CRT` init table to check. If the output ever goes non-zero natively, the native-vector test fails.

Static init needed to populate the shaper template (for the state path, if ever ported): `1404AC840`,
`1404ADFA0`, `1404AE740`, `1404AEBA0` (`1404AEED0` ran too, no change to the output weights). `141976810` builds
the template and `141974510` copies it into the shaper. Without these the 8x8 matrix at shaper+0x20 and the bias
at +0x120 are zero.
Measured: `u = bias + c*x` with `c` at shaper+0x20 (8 floats) and bias at shaper+0x120 matches native to 4 decimals.
Recurrent part (`141979bf0`, `141978300`, `141974f10`, `14192d1b0`) still unread for the state.


## Case 4 output statics: writer search (2026-10-23)

Checked against the cached decompile `/data/dev/workspace/Research/KONTRA/source/kontakt8/` (1113 files), by
`rg` on `DAT_14a64ba20|ba30|ba40` and on `14a64ba` (direct and `uRam…` forms):

- `DAT_14a64ba20`, `_DAT_14a64ba30`, `DAT_14a64ba40` are read in one place only: `0141970000.c:5622-5627`, the
  BEffectKolor constructor, which copies them into the object (`param_1[0x4c]`, `[0x4e]`, `[0x50]`). The same
  constructor copies the template block at `&DAT_14a64ba50` (line 5585).
- No decompiled file writes to them. `01404a0000.c:7905-7913` writes `DAT_14a64ba14`, `ba04`, `ba0c` at init,
  which are the neighbouring words and are not read by case 4.
- Caveat: a write through a computed pointer (for example `(&DAT_14a64ba00)[i]`) would not match these
  patterns. The grep excludes only direct and named-symbol writes. The emulator run (1709 calls, output exactly 0)
  is the stronger evidence.

Conclusion (unchanged): case 4 outputs the zero statics, so the Rust case returning `0.0` stands.

## Control tick and per-mode seed (decompile read, 2026-10-23)

Read from `0141970000.c` (not yet ported):

- `FUN_14197ab90` (control tick, line 9030): a 2-step countdown at object `+0x510/+0x518` (calls `14193fab0` or
  `1419421c0`). When `+0x7f1` is set and `+0x7e8` is 0, it runs `14197acc0(mode)` if `+0x7f4` is set, then sets
  `+0x7e8 = 1.0`, and sets `+0x7f0` when `+0x7e4` differs from `DAT_14470bad4` (1.0).
- `FUN_14197acc0(mode)` (line 9092): stores `mode` at `+0x7dc`, clears `+0x7f4`, calls
  `141974d90(this+0x800 + mode*0xe00)` and `141974d90(this+0xf00 + mode*0xe00)`. Mode 4 also zeroes
  `+0x942c/+0x9418` and `+0x945c/+0x9448` and reloads its state. The `0xe00` stride is two `0x700` objects
  (k = 0, 1), which matches the `(mode*2+k)*0x700` note in `kolor/mod.rs`.
- `FUN_14197adb0` is the case-2 body read in `shaper_2_5.rs`.

`141974d90` (134 B, decompile `0141970000.c:3869`) is **not a template load**. It switches on the selector byte
at `shaper+0x6f0` and only clears shaper state:

- default (cases 0, 1, 3, 6-9): zero `shaper[0..8]` (8 floats, the recurrent `s`).
- case 4: zero `shaper[0..7]` (7 floats: 2 qwords, 1 dword, then the 4-byte tail at +0x18).
- cases 2, 5: nothing (stateless, matches `shaper_2_5.rs`).

Call sites: `14197acc0(mode)` (`0141970000.c:9092`) runs it on `this+0x800+mode*0xe00` and `this+0xf00+mode*0xe00`
(the two `0x700` objects, k = 0, 1). Mode 4 additionally zeroes `+0x942c/+0x9418` and `+0x945c/+0x9448`. This
is decompile-only: not yet checked against the emulator. The per-mode *parameter* fill (P, P2) is still open.

Still open for the live path: `14193fab0` / `1419421c0`, the per-knob notify lambdas that fill `P` and `P2`, and the
mode-to-case table (which selector byte each mode writes at `shaper+0x6f0`). The control tick, the P/P2 fill and
the mode table are needed before a Kolor `Processor` can be attached to the IR; none is in the tree yet.

## Mode table and control tick: resolved (step 3, 2026-10-23)

**Mode -> selector (resolved, kemu-verified).** `14197a2c0` and `141977d20` compute
`mode = clamp((int)(knob4 * 10.0), 0, 9)` (`DAT_144714a00` = 10.0, `141977d20` clamp at 0 and 9). The
block process uses object `n = mode*2 + k` at `core + 0x800 + n*0x700`. Its selector byte (`+0x6f0`) is
**the mode itself**: the constructor writes `0,0,1,1,...,9,9` for `n = 0..19` (the `*(undefined1*)(...+0xde) = s`
writes in `141972fa0`, `1419732b0`, `1419735c0`, `1419738d0`, `141973be0`, `141973ef0`, `141974200`,
`141974510`, plus `0xa9e/0xb7e/0x55e/0x63e` for modes 5 and 2). Probe `tools/re/kolor_mode_table_probe.py`
reads the bytes under kemu after every mode and observes the dispatch case that runs (`case0` ... `case9`;
modes 6-8 run `14194cfb0`, told apart by the byte). Table: `control.rs::shaper_selector`.
`mode_from_knob` is unit-tested with the `mode/9` round trip that `apply` uses.

**Control tick `14197ab90` (ported, native-verified).** `control.rs::Control::tick`. The fields and their
offsets are read from the decompile. The probe `tools/re/kolor_control_tick_probe.py` writes 400 random
control states, runs the tick natively, and records the fields after it and the notify and seed calls.
`tick_matches_native` checks every field, the phase and the notify/seed counts.

Two facts the port depends on, found by the probe:
- `+0x7f0` is a 2-byte word. Its low byte is a flag (`14197a2c0` tests it and writes `0x100`). Its high byte
  is `+0x7f1`, the pending apply byte. The tick writes the word as `1` after clearing pending. A probe
  that wrote the two as separate fields clobbered the pending byte, so they are separate `u8`s in the port.
- Notify A (`14193fab0`) sets phase 4 and tail-calls notify B (`1419421c0`). B sets the phase from
  data: `if (sum <= 0) phase = 4 else { phase = 2; byte this+0x95 = 1 }` (`this` = core+0x480) (`0141940000.c` `1419421c0`, end of
  function). The port takes the notify results through the `Notifier` trait. The test replays the native
  phase. So the countdown, the flags and the seed are ported and checked, and the phase rule
  is not (it depends on the P data, which is step 2).

**Step 2 (P/P2 fill): ported and native-verified (`crates/sampler-core/src/dsp/color/kolor/notify.rs`).**
Notify B, per lane (16 floats per block): X := S, P2 := P, Z := 0, D := X - P; with `a = float(|P| bits)`
and `e = float(|D| bits)` (integer magnitudes, not reinterpreted), `T = 0` if a = 0 else `float((int)a+1) - a`;
`e < T` snaps (D := 0, P := X, P2 := X); `sum = sum of e`; `sum <= 0` gives phase 4, else phase 2, byte +0x95 := 1,
+0x98 := +0x9c. Since T = 1 for every integer P below 2^24, the snap only zeroes lanes with a zero delta.
Vectors: `tools/re/fixtures/kolor_notify_b.json` (160 cases, 16 per mode, `tools/re/kolor_notify_b_probe.py`).
Notify A (`14193fab0`) copies X into P and P2, zeroes D and Z, sets phase 4 and +0x95 := 0, then tail-calls B.
Still open: who writes the source block S (`this[9]`, core+0x4c8) on the live path. The notify test does not need it.

Historical notes (before step 2): `14193fab0` (notify A, `this` = core+0x480) copies 0x40-byte source blocks (`this[6]`) into
`this[0]` and `this[0xc]`, zeroes `this[3]` and `this[0xf]`, sets the phase (`this+0x90` = core+0x510) to 4,
clears the byte `this+0x95`, then tail-calls B. The source block (who writes `param_1[6]`) is not traced.
B (`1419421c0`, 656 B) is the SIMD routine that computes the activity sum above and the P2 data. Not
ported. The tick port needs no P data, but the phase after a B call does.

**Per-mode gains (read, not yet used).** `14197a2c0` multiplies the pre-shaper input by
`DAT_14769aed0[mode]` and the post-shaper output by `DAT_14769aea8[mode]` (`pefile`, 10 floats each):
pre = 26.32905, 0.836556, 11.220184, 0.022209, 10.035909, 7.079458, 8.291311, 0.33225, 14.77084, 12.438722;
post = 0.084792, -0.364524, 0.562341, 0.527867, 0.121059, 1.0, 0.189637, 0.33553, -0.148212, -0.12086.
The pre-gain is multiplied with `param_7d4` and `param_7c0` (not decoded here).

**Not done:** P/P2 fill (step 2), wiring the tick into the live path, and the Processor (step 4). The
parity row stays `partial`; no mode flips.
