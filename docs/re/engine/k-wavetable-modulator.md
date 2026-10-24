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
| 0 | 0x141a3e9c1 | advances the pair: `a' = a + base`, `b' = 2·base + b`, each wrapped at `> 0.5`; returns shape(`a'·2π + π`) |
| 1 | 0x141a3e74e | phase += inc, wrap to [0,1); triangle `|p-0.5|*k - 1`; one-pole smoothed by `+0xc8` |
| 2 | 0x141a3e7d3 | sine-type shape via `0x141a3e5d0` on `x*c + d` (`x` from the phase pair); sign selects branch |
| 3 | 0x141a3e819 | sine-type, sign-selected output |
| 4 | 0x141a3e853 | sine-type on the phase pair |
| 5 | 0x141a3e891 | sign test on `+0xb8`, then `+0xc` path |
| 6 | 0x141a3e8af | sine-type on `+0xc` |
| 7 | 0x141a3e8e1 | advance via `0x141a3e6d0`; if `a < 0` return `\|shape(b·2π + π)\|` (abs mask `0x144714b90`), else 0 |
| 8 | 0x141a3e921 | advance; `v = shape(b·2π + π)`; if `a < 0` return `v²`; else take the zero-return tail at `0x141a3e883` |
| 9 | 0x141a3e95e | white noise: `0x141a3f230` steps the LCG `x = x·1103515245 + 12345` and returns `(float)(x >> 8?) · amp` (`amp` at sub-state `+0x4`); output is not reproducible across runs |
| 10 | 0x141a3e971 | Voss-style pink noise in `0x141a3f120`: row counter at `+0x84`, row mask at `+0x88`, row values in `+0x8..`, running sum at `+0x80`, output `sum · scale` (`scale` at `+0x8c`), LCG-fed rows |
| 11 | 0x141a3e984 | slewed random walk in `0x141a3f1b0`: LCG step, `v += step·0.05·amp` into `+0x90`, reflected at `±1` (`2 - v` above 1, `-2 - v` below -1) |
| 12 | 0x141a3e997 | sample and hold: calls the phase advance `0x141a3e6d0`; on a wrap (`al != 0`) draws a new white sample into `+0xc`; returns `+0xc` |

`0x141a3e5d0` is `shape(x)`, decoded (constants at `.rdata`, read with radare2):

- `y = x·(1/2π) − 0.25` (`0x144fd0d98` = 1/2π, `0x144e79a60` = 0.25; `x` arrives as `2π·p + π`, so `y = p + 0.25`).
- `r = round(y)` via `0x1443d107e` (inferred from use), `t = 0.5 − |2(y − r)|`, a triangle in [−0.5, 0.5].
- `shape = K1·t⁴ − K6·t⁶ − K2·t² + π` with `K1 = 0x144fd0da8` (2.535656), `K6 = 0x144fd0da0` (0.540347), `K2 = 0x144fd0db0` (5.166513), constant `0x1447149a0` (π).
- Checked in Python against the disassembly: it is not sin/cos. Values: p=0 → π, p=0.25 → 2, p=0.5 → π, p=0.125 → 2√2 (range [2, π]), so it is an unscaled rectified-style curve. Its consumers scale it; the scaling is not decoded.

## Still open

1. Modes 9–12 decoded as noise / random-walk / sample-and-hold (see table). The LCG output
   scaling (`>> 8`, `amp` at `+0x4`) is not checked. Noise modes can only be compared statistically.
   The output scaling of `shape` (how the consumer maps [2, π] to a wave) is not decoded.
2. The mixing kernel `0x140cd5af0` (entry decoded: rate from `[rdi+0x40/0x58/0x48]` into `0x141a3eac0`,
   then `0x141a3e720` gives the modulator `m`; the blend then calls `0x140ce8df0`, `0x140ce7ea0`,
   `0x140cd55c0`, `0x140ce6eb0` and `0x140567aa0` twice, not yet decoded.
   Further read (not decoded): state fields `+0x10000` (running accumulator, wrapped by `comiss` at 1 and
   reflected), `+0x10004`, `+0x10028` (gain), `+0x10100`/`+0x10104` (previous and current modulator),
   `+0x10030` (sub-state for `0x141a3e720`), `+0x80`/`+0x90` (`0x141a3e720` modes). A clamped index
   (`cvttss2si`, cap 0x95f = 2399) into a table at `0x14a6b2600` (8-byte entries, linear interpolation)
   appears in the kernel, plus helpers `0x1443d10a8` and `0x1443d109c` (wrap or fold). Pieces are
   interleaved; the full blend is a multi-step job, not a single formula.) (how amount, the modulator value and `+0x40`, `+0x58`,
   `+0x68`, `+0x70`, `+0x78` combine with the warped phase).
3. `0x1443d10cc` (exp-like, smoother coefficient) and `0x1443d107e` (round, assumed from use, not confirmed).
4. Oracle probe: set `ENGINE_PAR_WT_MOD_AMOUNT` from a KSP script and capture a wavetable sweep
   (`oracle-slot`), then compare with the decoded law.

Do not implement from this note alone. Every constant and mode above must be confirmed by the
oracle before it becomes engine behaviour.
