# Kolor shaper cases 0, 1, 3, 4: dispatch findings

Status: **cc70 stage recovered and verified; cfb0 state update (c830) and cases 6-9 parity still open.**
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

## Still open

- **c830 (`14194c830`) state update.** cfb0 stores its 8-float result into `shaper[0..8]`. Black-box, that
  result is a dense mix of the state: lane j moves output i by about 0.2-0.36 when i = j and 0.01-0.07
  otherwise, and it depends on x non-linearly. Its helpers are not recovered: `1419466a0` (24-lane affine
  output, 365 B), `141943870` (x2), `141943490`, `141943550`. Until they are, the state after the first
  sample is unknown and cases 6-8 are exact for one sample only.
- **Case 9 (`1419748b0`)** is the inlined twin of cfb0 that writes state back. Not analysed beyond
  confirming the shape; the same c830 gap applies.
- **Parameter fill.** `P` (shaper+0x30) and `P2` (shaper+0x2c0) are written by the control lane. The
  kernel takes them as given.
- The Rust kernel is `#[allow(dead_code)]` and unwired: wiring it in before c830 would give wrong state.

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

## Helpers

- `14194cc70` (cc70), `141946590`, `14192d1b0`, `141946410`: read and verified, see the recovered section above.
  `14194cc70` uses the P-block offsets above, not `shaper+0x140` (that was the cfb0-relative view).
- `14194c830` (c830), `141979f60`, `141979bf0`: not read to completion; c830 is the open item above.
- `1419426e0` (253 B): std-array-style 1-element wrap (copies the input into a proxy). Not DSP.

## Open

- Case 0, 1/3, 4 output math depends on the unread helpers above. Read them with
  `radare2 -c 'pdf @ ...'` on `/data/dev/workspace/re-binaries/Kontakt8.exe` before writing any Rust.
- Kemu vectors for these cases need a warmed multi-block run via `kolor_kemu_probe.py`. The probe currently
  only drives the wrapper driver (`14197a2c0`), so the shaper state must be seeded through it.
