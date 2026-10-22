# Kolor shaper cases 0, 1, 3, 4: dispatch findings

Status: **case 4 resolved** (ported as `sampler-core/src/dsp/color/kolor/shaper_4.rs`, native vectors
`tools/re/fixtures/kolor_shaper_4_{block,sweep}.json`, harness `tools/re/kolor_shaper4_kemu.py`).
Cases 0, 1, 3 still read-only. Scope: decompiled source in
`/data/dev/workspace/Research/KONTRA/source/kontakt8/` (`0141970000.c`, `0141940000.c`) and the
dispatch disassembly `docs/re/disasm/kolor-shaper-dispatch-1419749f0.txt`.

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

## Dispatch (1419749f0, `FUN_1419749f0`)

- Selector: `switch((char)param_2[0xde])`, i.e. the signed byte at shaper+0x6f0. Cases 0..9.
- Cases 2 and 5 are stateless (`14197adb0`, `14197ae80`) and belong to the control lane.
- Cases 6, 7, 8 call `14194cfb0`; case 9 calls `1419748b0`. Both are outside this lane.

## Per-case body (verified from decompile)

| Case | Body | Notes |
|------|------|-------|
| 0 | input x = `*param_1[2]` → 4-slot state vector `[x, s0..s6]` built from shaper fields → `14194cc70(shaper+0x30, out, vec)` → `14194c830(shaper+0x2c0, ...)` → store the 8-float result back into shaper+0..0x1c | Two-stage: first `cc70`, then `c830`. |
| 1 | `1419426e0` wraps x into a 1-element array → `141974860` pushes an 8-float record into a ring (slot stride 9 floats) → `cc70` on the ring as above | Same code path as 3. |
| 3 | identical to case 1 | Confirmed by reading both arms. |
| 4 | x → `14197a150(shaper, out, &x)`, which calls `141979f60(shaper+0x20, ...)` then `141979bf0(shaper+0x208, ...)` and stores 4 floats back at shaper+0..0xc | Not yet read below these two helpers. |

## Helpers (roles only, bodies unread)

- `14194cc70` (450 B): builds 8 lanes from `shaper+0x140` via `141946590` and `14192d1b0`, adds offsets at
  `shaper+0x240..0x25c`, applies a per-lane rsqrt-normalised gain, then calls `141946410(shaper+0x260, ...)`.
  Uses SSE `rsqrtps`; the gain path has an `x < eps || x == eps` guard per lane.
- `14194c830` (501 B), `141946590`, `14192d1b0`, `141946410`, `141979f60`, `141979bf0`: not read.
- `1419426e0` (253 B): std-array-style 1-element wrap (copies the input into a proxy). Not DSP.

## Open

- Case 0, 1/3, 4 output math depends on the unread helpers above. Read them with
  `radare2 -c 'pdf @ ...'` on `/data/dev/workspace/re-binaries/Kontakt8.exe` before writing any Rust.
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
