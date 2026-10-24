# Kontakt wavetable modulator (WT_MOD_*): RE progress

Status: partial. Source: `Kontakt8.exe` (`/data/dev/workspace/re-binaries`, sha256
`0fe6356e...`, image base 0x140000000), disassembled with radare2 (`pd`, `pxw`). REA/Ghidra
deep analysis of this binary times out at the 1200 s startup deadline (JVM, 2 GB heap), so it is
not used.

## Consumer and layout

- `0x140cd5af0`: per-sample kernel that combines the modulator with the wavetable phase. It calls
  the modulator state update `0x141a3e720` (modulator out), the wavetable phase-form routine
  `0x140567aa0` (already ported, `sampler-core/src/wavetable.rs::warp`), and helpers `0x141a3eac0`
  and `0x141a3e720`. Its mixing formula is not decoded yet.
- Modulator state (struct offsets from the state pointer `rbx`):
  - `+0x08`, `+0x0c`: phase pair (`+0x08` base phase, `+0x0c` doubled-rate phase) written by `0x141a3e6d0`.
  - `+0xb8`: phase accumulator in [0, 1) (mode 1 wraps both ways).
  - `+0xbc`: phase increment, set by the setter `0x141a3eac0` from the frequency and sample rate.
  - `+0xc4`: one-pole smoother state; `+0xc8`: smoother coefficient `a`, output `s' = t + a (s - t)`.
- Setter `0x141a3eac0`: increment = `freq / rate` (`divss`); smoother coefficient from `0x1443d10cc`
  (an `exp`-like call, not yet identified).
- `0x141a3e6d0`: advances a bipolar phase pair; wraps only the upper side (`> 0.5` subtract 1).

## Mode dispatch (`0x141a3e720`, selector `[rbx+0x20]`, 13 entries, jump table at 0x141a3ea2c)

| mode | case target | decoded |
|---|---|---|
| 0 | 0x141a3e9c1 | not decoded |
| 1 | 0x141a3e74e | phase += inc, wrap to [0,1); triangle `|p-0.5|*k - 1`; one-pole smoothed by `+0xc8` |
| 2 | 0x141a3e7d3 | sine-type shape via `0x141a3e5d0` on `x*c + d` (`x` from the phase pair); sign selects branch |
| 3 | 0x141a3e819 | sine-type, sign-selected output |
| 4 | 0x141a3e853 | sine-type on the phase pair |
| 5 | 0x141a3e891 | sign test on `+0xb8`, then `+0xc` path |
| 6 | 0x141a3e8af | sine-type on `+0xc` |
| 7 | 0x141a3e8e1 | not decoded |
| 8 | 0x141a3e921 | not decoded |
| 9 | 0x141a3e95e | not decoded |
| 10 | 0x141a3e971 | not decoded |
| 11 | 0x141a3e984 | not decoded |
| 12 | 0x141a3e997 | not decoded |

`0x141a3e5d0` is a polynomial shape: `u = 0.5 - |x - round(x)| * 2` style with a squared and a cubic
term (decode incomplete). Its constants are in `.rdata` at 0x144fd0d98, 0x144e79a60, 0x144fd0da8,
0x144fd0da0, 0x144fd0db0, 0x1447149a0.

## Still open

1. Modes 0 and 7–12 bodies, and the exact polynomial in `0x141a3e5d0`.
2. The mixing kernel `0x140cd5af0` (how amount, the modulator value and `+0x40`, `+0x58`,
   `+0x68`, `+0x70`, `+0x78` combine with the warped phase).
3. `0x1443d10cc` and `0x1443d107e` (exp and floor/round helpers).
4. Oracle probe: set `ENGINE_PAR_WT_MOD_AMOUNT` from a KSP script and capture a wavetable sweep
   (`oracle-slot`), then compare with the decoded law.

Do not implement from this note alone. Every constant and mode above must be confirmed by the
oracle before it becomes engine behaviour.
