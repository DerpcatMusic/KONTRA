# Kolor shaper cases 0, 1, 3, 4: dispatch findings

Status: **read, not implemented.** No Rust port and no kemu vectors yet. Scope: decompiled source in
`/data/dev/workspace/Research/KONTRA/source/kontakt8/` (`0141970000.c`, `0141940000.c`) and the
dispatch disassembly `docs/re/disasm/kolor-shaper-dispatch-1419749f0.txt`.

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
