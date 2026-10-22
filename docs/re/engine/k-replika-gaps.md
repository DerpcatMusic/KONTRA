# Kontakt 8 Replika core: gap closure (GAP-A, C, D, E, F, H)

Follows `docs/re/engine/k-replika-core.md`. Method: decompile text in
`Research/KONTRA/source/kontakt8/` and `docs/re/kontakt-rdata/` TSVs, read by line range; constants
checked by arithmetic only. No binary, Ghidra, radare2, REA, Wine or Kontakt; nothing measured.

Notation. `C` = core object. Offsets are bytes from `C` unless marked `H` (head), `F` (filter),
`B` (saturator), `L` (limiter). f32/f64 = decoded constant value. Triple = three 8-byte pointers
(begin, end, cap) at `C+0x80`, `+0x98`, `+0xb0`, `+0xc8`, `+0xe0`, `+0xf8`; each points to one
112-byte float block. Blocks are P0..P5 in that order.

Portable: partly. Blockers are listed under Open items.

---

## GAP-A: does Ctl(0x80) alias Ctl(0xc8)?

**Answer.** No alias. `C+0x80` (P0) is the block the loop reads; `C+0xc8` (P3) is the block the
setter writes first. Controls reach the loop by two routes:
- Live (`C+0x8` nonzero): the setter writes P0 and the epilogue copies P3 over P0 at once.
- Not live: the setter writes P3 and sets a pending flag. The next update copies P3 into a target
  P2 and P0 glides to P2 per frame, ending in a snap. Indices 0, 1, 4, 8, 13, 14, 16, 52 write only
  P3 when not live, so they reach the loop only through this ramp.

**Evidence.**
- Allocation. Constructor `141a7f4d0` calls `141a7d480` with `C+0x10`, which allocates six 0x70-byte
  blocks and stores the triple pointers at `C+0x80`, `+0x98`, `+0xb0`, `+0xc8`, `+0xe0`, `+0xf8`.
  The constructor stores none of them directly.
- Loop `141aa9480` reads the pointer at `C+0x80` (five sites) and P0's floats. It writes no pointer.
- Setter `141ac5980`. Non-live cases write P3, then set `C+0x114 = 1` and `C+0x110 = 1` (pending
  apply): shared tail `141ac6880` (verified for 0, 1, 2 and 34) or inline. Live cases also write
  P0, P2 (`+0xb0`), P4 (`+0xe0`), and zero P1 (`+0x98`) gate fields. If `C+0x8` is set after the
  tail, the setter calls epilogue `141a95830` and `141abd290`.
- Epilogue `141a95830` calls `141abbac0` first. It copies P3 to P2 (112 bytes), then P2 to P0 and
  P4, zeroes P1 and P5, sets `C+0x110 = 4`, `C+0x115 = 0`, and calls `141acbcd0`. The delta is
  zero, so no ramp starts.
- Update `141ab4a10` (slot 8): if `C+0x114` is set, clear it and call `141acbcd0`. Else if
  `C+0x110` is nonzero, decrement counter `C+0x118`; at zero call commit `141a956d0`.
- Apply `141acbcd0`: P2 = P3; P4 = P0; P1 = P2 - P0 per float; P5 = 0. Each float whose masked
  delta is at or below a ULP-style threshold (constants `144f39330`..`3c`, `144f38ef0`..`ef8`)
  snaps: P0 = P4 = P2, P1 = 0. Then `C+0x118 = C+0x11c`, `C+0x115 = 1`, and `C+0x110 = 2` if any
  delta remains, else 4.
- Commit `141a956d0` (counter zero): P0 = P2, P4 = P2, P1 = 0, P5 = 0, `C+0x110 = 4`,
  `C+0x115 = 0`, then `141acbcd0`, which re-arms if P3 still differs from P0.
- Per frame: `141ab7480` calls `141a9f4d0` on the triple. It ticks P5 floats 0..3 by
  f32 `144714b70` = 1.0, `144714b74` = 1.0 (lanes 2, 3: `144714b78`, `144714b7c`, not in the TSV),
  then for k = 0..3: `P0[k] = P5[k] * s_k * P1[k] + P4[k]`. `s_0..s_3` are the floats at `C+0x120`,
  `+0x124`, `+0x128`, `+0x12c`.
- Ramp setup in block setup `141aa1580` (decompile lines 939-944): `C+0x11c = max(1,
  trunc(f32[C+0x610] * f32[147697658] / n + 0.5))`, `n` an integer divisor set earlier in that
  function; `s_k = 1 / (C+0x11c * n)` for all k. Rounding constant f32 `14470bad0` = 0.5.
- Other writers. Rebuild `141acbfb0` (mode change) writes P0, P2, P3, P4 directly (immediate). Bypass
  fade `141acc600` writes P3 `+0x60` only. `141aca670` and `141abd290` read P3 only.

**Label.** CONFIRMED: no alias, copy routes, counter and slope, commit and re-arm.
INFERRED: `n` = frames per update step; 147697658 not decoded. UNKNOWN: lanes 2-3 increments.

**Port consequence.** Keep P3, P2, P0, P4, P1, P5, the state words `C+0x110`, `+0x114`, `+0x115`,
`+0x118`, `+0x11c`, and the four slopes. Per frame: phase tick, then glide P0. A control change
reaches the loop after one or more updates. Blocked by lanes 2-3, and by confirming `n` and 147697658.

---

## GAP-C: head ring addressing and read glide

**Answer.** Each head has an L and an R ring, each with a write index and a length. The index
advances once per frame after the read and wraps at the length. Mode 0 (`H+0xc0 == 0`) reads at
`A + offset` with a modulated delay. Active heads are set to mode 1 (`H+0xc0 = 1`, written by
`141ab4e30` in its active branch), and the playing reader is `141ab8c60`. Its kernels are not read.

**Rings and indices (CONFIRMED).**
- L ring: buffer `**(H+0x150)`, write index A_L `H+0x158`, length Len_L `H+0x15c` (both int).
  R ring: buffer `**(H+0x188)`, A_R `H+0x190`, Len_R `H+0x194`.
- Write `141acd110`: if hold byte `H+0x6c` is set and fade byte `H+0x5824` clear, store inputs at
  `H+0x348`/`H+0x34c` and write nothing. If `H+0x5824` is set, store and scale by
  `(1 - f32[H+0x5808])`. Else by mode `H+0xc0`: 0 = as is; 1 = `141aa3c60` (state `H+0x2dc0` L,
  `H+0x4240` R); 3 = one-pole `141aa31f0` (state `H+0x3d0`, `H+0x430`; coefficients `H+0x3c4`,
  `H+0x424`); 2 = `14199c870` (`H+0x480`, `H+0x1920`) with gain `H+0x84` and the pair at `C+0x9a0`.
- Slot for write index A: A if 0 <= A < Len; A - Len if Len <= A < 2 Len; A + Len if A < 0. The
  store happens before the read in the same frame.
- Read `141ab7a90`: advances three glides (`H+0x5808`, `H+0x5828`, `H+0x5848`) that set gains
  `H+0x70`, `H+0x74`, then calls `141ab82e0` (mode 0) or `141ab8c60` (other modes). The index
  advance in `141ab82e0`: A += 1, wrap to 0 at Len.

**Mode 0 read law (CONFIRMED arithmetic).**
- Taps: L at `H+0x138` (+0x20 = A_L, +0x24 = Len_L); R at `H+0x170` (+0x20 = A_R, +0x24 = Len_R).
  Tap call `141ab7e00(tap, p)`: p' = A + p; subtract Len if p' >= Len; add Len if p' < 0; clamp to
  `[0, Len-1]`. The fraction p - floor(p) is used (exact integers return at once). The blend of the
  two neighbours is lost in the decompile text, so the kernel is UNKNOWN.
- `offL = min(0, s * k * T + b) + baseL`; `offR = min(0, s * k * (g * U + (1 - g) * T) + b) + baseR`.
  - `T` = f32 `C+0x968` (rotator output; the head's table pointer `H+0x5878`); `U` = f64 `C+0x970`
    (rotator state); `g` = f32 `H+0x57e8` (glide fraction).
  - `s` = f64 `H+0x98` (chosen in `141ab4e30` from the floats `H+0xa0..0xac`); `k` = f32 `H+0xb0`.
  - `b` = f32 `H+0xb4` = -trunc(f32[H+0xa0] * f32[H+0xb0] + 2.0) (sign bit XORed with `144714ba0`).
  - Bases: targets `H+0xe8` = -round(f64[H+0xc8]) (L) and `H+0xf0` = -round(f64[H+0xd0]) (R),
    copied to the active bases `H+0xd8`, `H+0xe0` when a crossfade completes.
- Base-change crossfade: level `f` = f64 `H+0x5788` starts at 1 and decays by (1 - 1/len) per frame,
  len = f64 `H+0x57a8`. Output = f * old + (1 - f) * new.
- Ring length in `141ab4e30`: `T_ring` = f32 `H+0xa4` if int `H+0x80` > 0, else round(f32 `H+0x44`);
  `n_ring = round(T_ring) * int H+0xbc`, written to `H+0x158` and `H+0x220` (an index start value).
  If n_ring < 1, `H+0x15c = 0`. Else the head is active and the write of Len_L is not in the first
  40 lines of that branch (UNKNOWN).
- Time source: the floats `H+0xa0`, `+0xa4`, `+0xa8`, `+0xac`; their writer is not traced. Core
  `C+0x860` (mode-3 time) is not read by any head reader found.

**Mode 1 and 3 read (`141ab8c60`; role INFERRED, arithmetic CONFIRMED).**
- Modulation: `(B*g + (1-g)*A) * s * k` with A = f32 `C+0x940` (LFO A), B = f32 `C+0x954`
  (LFO B), `s = f64[H+0x98] * 0.1` (f64 `144714950` = 0.1).
- Speed ratios `H+0x58 = f64[H+0x48] / f64[H+0xc8]` and `H+0x60 = f64[H+0x48] / f64[H+0xd0]`,
  clamped by f64 `1479ab4c0` = 50.5 (INFERRED).
- Tap kernel by `H+0x270` (L), `H+0x340` (R): 0 = `141ab8a00`, 1 = `141ab87d0`, 2 = `141ab7fe0`,
  other = 0. Not read.
- Mode 2: position from the `C+0x9a0` pair, scaled by `s * 0.1`, limited by f64 `1447149a8` = 4.0.

**Label.** CONFIRMED: rings, indices, wrap, write slot, mode-0 offset, bases, crossfade. INFERRED:
mode 1/3 role and speed ratios. UNKNOWN: interpolation (`141ab7e00` blend, kernels `141ab8a00`,
`141ab87d0`, `141ab7fe0`), active-branch length writer, head time-field writer.

**Port consequence.** Mode 0 ports as read, with an interpolation the port must choose and label.
Do not treat mode 0 as the playing mode. Blocked by the length writer, the time-field writer and
the mode-1 kernels.

---

## GAP-D: filter family, low cut and high cut

**Answer.** Each filter (F1 = `C+0x11500`, F2 = `C+0x12980`) holds, per channel, a lowpass-type TPT
state-variable section (`F+0x1c` L, `F+0x5c` R) followed by a highpass-type section (`F+0x90` L,
`F+0xc4` R). Both filters get the same updates from the same two setter values.
- LOWCUT = setter index 2: highpass section, `f = 10^(2v + 1)` Hz for v in [0, 1] (10 Hz to 1 kHz,
  log; powf base f32 `144714a00` = 10.0). Enable `v != 0`; neutral at v = 0.
- HIGHCUT = setter index 3: lowpass-type mix section, `f = 200 + 19800 v` Hz (200 Hz to 20 kHz,
  linear; f32 `144f38ce4` = 200.0, f64 `1479ab650` = 19800.0). Enable `v != 1`; neutral at v = 1.
- Names are INFERRED from these sweep shapes (the binary has none). The wrapper-to-core index map
  is not confirmed here.

**Section (`140d947a0`, CONFIRMED arithmetic).** Layout from base B: s0 `+0`, s1 `+4`, c2 `+8`,
c3 `+0xc`, c4 `+0x10`, out5 `+0x14`, out6 `+0x18`, d `+0x1c`, x `+0x20`, g `+0x28`, R `+0x2c`.
- d = x - s1; out5 = d*c3 + s0*c2; out6 = c3*s0 + d*c4 + s1.
- s0' = 2 d c3 + 2 s0 c2 - s0; s1' = 2 s0 c3 + s1 + 2 d c4.
- w = pi * f / fs (f64 `1447149a0` = pi); g0 = w + w^3/3 + 2 w^5/15 (f32 `144760108` = 1/3,
  `144fcfce0` = 2/15), i.e. tan(w) to fifth order.
- Lowpass-type (`1419529e0`): g = g0 * Q, Q = f32 `1479ab25c` = 0.7498942; R = 5/3 (bit pattern
  `14473775c` = 0x3fd55555).
- Highpass-type (`141abe6a0`): g = g0; R = `F+0xbc` = 2.0, written in setup by `141ac97e0` (the zero branch there is dead for the constant 2.0).
- Both: c2 = 1/(g^2 + R g + 1), c3 = g c2, c4 = g c3.

**Chain (`141aa3ec0`, per channel, CONFIRMED arithmetic).**
1. Lowpass-type section on x. Mix with glide m (`F+0x1404`): y1 = m (0.31622776 x + 0.41018924 out5 +
   0.68377221 out6) + (1 - m) x. Mix words f32 `3ea1e89b` (= 10^-0.5), `3ed20453`, `3f2f0bb2`
   (= 1 - 10^-0.5), written by `141abe820` into `F+0x10`, `+0x14`, `+0x18`.
2. Highpass-type section on y1: y2 = (y1 - R out5' - out6') h + (1 - h) y1, with h = glide 2
   (`F+0x1418`), out5' = `F+0xa4`, out6' = `F+0xa8`, R = `F+0xbc`.
3. If byte `F+0x4` is set, the object at `F+0x100` processes the pair in place (class and flag not traced).
4. Glide 3 (flag `F+0x1444`): out = (1 - g3) out + g3 x_in.

**Glides.**
- Glide 1 (m): target `F+0x1408` = (v3 != 1.0) (`141abe820`). Coefficient `F+0x1400` =
  `exp(ln(0.05) / (0.025 fs))` (`141ac97e0`; f64 `147698b10` = ln 0.05 = -2.9957; f32 `144f38678`
  = 0.025), i.e. 5% per 25 ms. Snap `F+0x140c` = 0.001.
- Glide 2 (h): target `F+0x141c` = (v2 != 0) (`141abe6a0`). Coefficient `F+0x1414` = exp(ln(0.05) / (0.025 fs)),
  the same as glide 1 (`141ac97e0` stores the `exp` result; an earlier reading of it as the raw
  -2.9957 was wrong). Snap `F+0x1420` = 0.001.
- Glide 3: length `F+0x143c` = int(0.025 fs). Snap `F+0x1434` = 0.001. Step `F+0x1430` writer UNKNOWN.
- `F+0x0` = fs (`141ac97e0` stores it), set in setup before use.

**Label.** CONFIRMED: section algebra, both sweep laws, constants, mix, chain order, glide targets,
glide 1 and glide 2 coefficients, highpass R, snaps. INFERRED: LOWCUT/HIGHCUT names; out6 as lowpass.
UNKNOWN: glide 3 step, `F+0x100` class and flag, the writer of the enable byte `F+0x5`.

**Port consequence.** A TPT SVF pair with the sweep laws and fixed mix ports directly. Blocked by
glide 3 step and the `F+0x100` object (its flag is `F+0x4`). The saturator R terms are GAP-F.

**Implemented.** `crates/sampler-core/src/dsp/tpt_svf.rs` (`Section`, `Cut`, `prewarp`) with the
sweep laws in `sampler-ir` (`log_sweep_hz`, `linear_sweep_hz`). The in-loop hook is
`Feedback::cut` in `sampler-ir`, run in `dsp/timemod/mod.rs` on the recirculated signal before the
matrix. `sampler-kontakt` `replika_cut` maps LOWCUT and HIGHCUT into it. Not ported: the glides (static
targets), glide 3, the `F+0x100` object, and F1's output term `OUT = F1 x Ctl(0x60) + F2`.

---

## GAP-E: stereo effect objects C+0x13f80 and C+0x13e80

**Answer.** Both are `NI::DSP::Limiter`. Constructor `141adc050` installs vtable `14769c220`
(xref `vtable_xrefs.tsv`) and builds both objects. Slots used by the core:
- +0x38 (index 7): `141ae3ba0` (842 B), per-frame stereo process.
- +0x40 (index 8): `141ae60f0` (9 B), update (called from `141ab4a10`).
- +0x60 (index 12): `141aea740` (453 B), set parameter (index, value).
- +0x10 (index 4): `141ae1d10` (30 B), called at two core sites (lines 9435-9436); role not read.
Use (core doc steps 11, 16): `C+0x13f80` on L3/R3 for paths 0 and 4; `C+0x13e80` on the matrix
outputs. Both at half scale. The loop call sites were not re-located here.

**Parameters (`141aea740`, CONFIRMED).** v = the float argument.
- index 0: `L+0xa0 = v`; gain `G0 = 10^(40 (1 - v) / 20)` through table T4 (index
  `1200 + 398.631348 (1 - v)`, 9.96578 per dB), stored at `L+0x70` and copied to `L+0x50` when
  `L+0x44` is set. Range 1 to 100. Sets dirty flags.
- index 1: `L+0xa4 = v`; `tau = max(6.0 v, 0.001)` (f32 `144f38bd0` = 6.0, `1447148a4` = 0.001) at
  `L+0xb0`; `L+0xd0 = 1 - 1/(tau * L.f[+0x40])`. `L+0x40` writer UNKNOWN. No dirty flags.
- index 2: `L+0xa8 = v`; gain `G2 = 10^(-40 (1 - v) / 20)` (index `1200 - 398.631348 (1 - v)`) at
  `L+0x74`. Range 0.01 to 1. Sets dirty flags.
- index 3: `L+0xac = (v > 0.5)` (INFERRED: link or side select). No dirty flags.
- index 4: `L+0xad = (v >= 0.5)` (INFERRED: mode). No dirty flags. The core resets it to 0.
- Dirty flags are `L+0x90 = 1` and `L+0x80 = 1`. If `L+0x44` is set, gains are copied to the
  working values in every case.

**Process (`141ae3ba0`; arithmetic CONFIRMED, roles INFERRED).** Per frame:
- Four ramp accumulators: `L+0x50 += L+0x60`, `L+0x54 += L+0x64`, `L+0x58 += L+0x68`,
  `L+0x5c += L+0x6c`.
- Detector: the larger of two channel candidates (magnitude times gain, plus f32 `144f385a8` =
  1e-12), mapped to dB by `8.6858902 * ln(level)` (f32 `1479ab570`).
- Gain computer: dB envelope state `L+0xc8` (attack `L+0xcc`, release `L+0xd0`, chosen by
  comparison); second dB state `L+0xbc` (`L+0xc0`, `L+0xc4`); gain returns to linear through T4.
- Output clamped to `[-2.5, 2.5]` (f32 `144fc40e0` = 2.5, `145494e5c` = -2.5).

**Label.** CONFIRMED: class, slots, five parameter laws, T4 use, clamp. INFERRED: detector and gain
roles. UNKNOWN: `L+0x40`, update thunk body, other parameter writers besides index 4.

**Port consequence.** A stereo dB-domain peak limiter, five parameters. An exact port needs the rest
of `141ae3ba0` (attack/release selection, link, side) and the `L+0x40` writer.

**Implemented.** `crates/sampler-core/src/dsp/db_limiter.rs` (`DbLimiter`, not wired): the five
parameter laws, the detector, the attack, the second dB state, the mode, the gain and the output
clamp. Attack, second state and `L+0x40` were closed in this pass (see "Closed in this pass").

---

## GAP-F: saturator coefficients and section coefficients

**Answer.** c4, c8, c0c and c10 (`B+0x4`, `+0x8`, `+0xc`, `+0x10`) are set once, in slot-1 init
`141a9e300`, through `141abe910` with fixed exponents. Index 29 does not write them. Index 29
(amount a) writes the target `C+0x142b0` and, live, the current amount `C+0x142ac`; then `141accd10`
sets drive `d = 10^a` (`B+0x1c`) and gain `g = 10^(-a/2)` (`B+0x20`).

**Evidence (CONFIRMED).**
- Index 29: live writes `C+0x142ac = v` at once; else pending flag `C+0x142b8`. Glide per frame with
  coefficient `C+0x142a8`; snap `C+0x142b4` = f32 `3a83126f` (0.001, set in setup).
- `141abe910(B, p2, p3, p4, p5, p6)` with K = f32 `1447148c0` = 0.05:
  - raw base = 10^(p2 K) + 3 * 10^(p4 K) + 5 * 10^(p6 K) (`1447149d0` = 3.0; `1447149f0` = 5.0);
  - raw c4 = 2 * 10^(p3 K) + 8 * 10^(p5 K) (`1447149f8` = 8.0);
  - raw c8 = -4 * 10^(p4 K) - 20 * 10^(p6 K) (`144f38e04` = -4.0; `14471cb14` = 20.0);
  - raw c0c = -8 * 10^(p5 K) (`144fc4d20` = -8.0); raw c10 = 16 * 10^(p6 K) (`144714a10` = 16.0);
  - each raw c divided by the raw base.
- Slot 1 (`141a9e300`, lines 9815-9829) calls `141abe910` for B_L and B_R with (p2..p6) = (0, -40,
  -45, -49, -40) (f32 `144f38e20` = -40.0, `1479ab744` = -45.0, `1479ab748` = -49.0). The same slot
  sets c14 and c18 of both sides to 1.0.
- Constructor `141a82070` sets c14 = c18 = d = g = 1.0, zeroes c8, c0c, c10; it does not set c4.
- Section coefficients: setup `141aa1580` (lines 1110-1111) calls `141ac9c50` with the block rate,
  which runs `1419529e0` (Q = 1.0) on section B (`B+0x58`, f32 `1479ab60c` = 2205.0 Hz; g `B+0x80`,
  R `B+0x84` UNKNOWN; c2..c4 at `B+0x60`, `+0x64`, `+0x68`) and section A (`B+0x24`, f32
  `1447149f0` = 5.0 Hz; g `B+0x4c`, R = c50 at `B+0x50` UNKNOWN; c2..c4 at `B+0x2c`, `+0x30`, `+0x34`).

**Values (arithmetic CONFIRMED).** Raw base 1.0668702. c4 = 0.0453523, c8 = -0.2085480,
c0c = -0.0266059, c10 = 0.1499714. P(f) = c4 f^2 + c8 f^3 + c0c f^4 + c10 f^5.

**Output (`141aa44a0`, CONFIRMED arithmetic).** With x' = d x: section B takes tanh(c14 x'), its
out6 (`B+0x70`) is f; P(f) feeds section A (input `B+0x44`); y = g (tanh(x') + c18 (P - c50 A.out5 -
A.out6)), with A.out5 at `B+0x38` and A.out6 at `B+0x3c`.

**Label.** CONFIRMED: polynomial and constants, d and g laws, section frequencies and layout, output
law, slot-1 and setup calls. UNKNOWN: R of section A (c50) and section B (`B+0x84`), `C+0x142a8` writer.

**Port consequence.** The polynomial is a constant table: port the four numbers. Amount is a glided
drive and gain. c50 and `B+0x84` are closed in this pass (R = 2.0), so the saturator ports.

---

## GAP-H: start call 141ee2160 and the line types

**Answer.** `141ee2160` takes a line object, a source pointer array, a frame count, a destination
pointer array, a count and a channel count. The decompile prints the second argument as a double;
the callee reads it as a pointer array. The line type word `+0x0` selects behaviour. Type 2 is a
per-channel copy (CONFIRMED). Types 1 and 3-6 are sample kernels (not read in full).

**Callee (`141ee2160`, 372 B, CONFIRMED).** Arguments:
1. line object (int*): type `+0x0`; channel count `+0x8`; source-pointer array pointer `+0x38`;
   destination-pointer array pointer `+0x50`; table-pointer array pointer `+0x68` (from 24-byte
   entries at `+0x98`); frame count `+0x30`; count `+0x34`.
2. source pointer array (one 8-byte pointer per channel).
3. frame count n (uint); copy length n floats per channel.
4. destination pointer array (one per channel).
5. count (int).
6. channel count (uint).
- Type 2 (copy): for each channel i, `memcpy(dest_i, src_i, n * 4)`.
- Other types: fill the three arrays, set `+0x2c` = channel count, `+0x30` = n, `+0x34` = count, then
  dispatch: type 1 to `141ee7670` (879 B), 3 to `141ee8370` (839 B), 4 to `141ee7cd0` (975 B), 5 to
  `141ee79e0` (752 B), 6 to `141ee80a0` (715 B; INFERRED from the header list). Zero channels go to
  `141eebcb0`. Guard: nothing when n + count == 0.

**Kernels 1, 3-6 (INFERRED).** Per-channel sample loops with a fractional position (f32 `+0xb8`),
a mode flag (`+0xb4`), frame counters and table pointers: resampling or ring-read kernels.

**Core start call (`141aa9480`, high-rate path).** Line `C+0x618` (type 2), channel count `C+0x10`.
Arguments: (line, second, n, `C+0x818`, count, `C+0x10`).
- Second argument: the caller's input pointer array (its third argument). The decompile prints
  `(double) f32[C+0x6d0]`, which only feeds the count formula. Evidence: the caller assigns its
  third argument to the input-array variable before the call and reads the first input from it; the
  end call (line 5694) passes a pointer array in the same slot. INFERRED.
- Copy length is n (block frames), passed before the loop variable is reassigned to count. So the
  start copy moves n frames per channel into `C+0x818`/`C+0x820`, and the loop reads `count` frames.
  The core doc's "fills count frames" should read n frames.
- End call (`C+0x718`, line 5694): copies `count` frames from `C+0x818` to the caller's output (type 2).

**Label.** CONFIRMED: argument roles, type 2 copy and length, dispatch targets, end copy. INFERRED:
second-argument identity, kernel roles, type 6 mapping. UNKNOWN: kernel bodies, the type-word writer
for `C+0x618` and `C+0x718`.

**Port consequence.** If both line objects are type 2 on every path, the high-rate segment is a
copy with the count formula and ports directly. Kernels matter only for other line objects
(`C+0x6f28`, `C+0x7028` in `141ac0000`), out of scope here.

---

## Open items (before this pass; closed items are listed in "Closed in this pass")

1. GAP-A: lanes 2-3 phase increments (`144714b78`, `144714b7c`); 147697658; `n`.
2. GAP-C: writer of `H+0x15c` and `H+0x194` on the active branch; head time field; kernels
   `141ab8a00`, `141ab87d0`, `141ab7fe0`; blend in `141ab7e00`.
3. GAP-D: glide 3 step (`F+0x1430`); `F+0x100` class and flag; writer of `F+0x5`.
4. GAP-E: `L+0x40` writer; rest of `141ae3ba0`; other parameter writers.
5. GAP-F: R of section A (`B+0x50`) and section B (`B+0x84`); `C+0x142a8` writer.
6. GAP-H: type 2 on `C+0x618`/`C+0x718` on all paths; second argument.

Core portable: partly. Blocked by the section R terms (GAP-D, GAP-F), the mode-1 head reader and
time source (GAP-C), and the ramp scale (GAP-A). The limiter (GAP-E) and the saturator polynomial
(GAP-F) port once their listed writers are confirmed.

---

## Closed in this pass

Method: the decompile text and TSV rows as before, plus one read-only `pefile` read of 8 `.rdata`
bytes at `144714b78` (the extractor skips `_UNK_`-prefixed references, so `constants.tsv` has no
row for it). No Ghidra, radare2, REA, Wine or Kontakt. The sampler-core tests were run; sampler-kontakt
was not changed and not built.

| Gap | Status | Law and address |
|---|---|---|
| GAP-A | Open | Lanes 2-3 closed (GAP-K). `147697658` = 0.02 (f32 `3ca3d70b`), so the ramp length is `max(1, trunc(rate x 0.02 / n + 0.5))` (`141aa1580`; the limiter uses the same law, `141ae28c0`). Open: `n`, the integer divisor set earlier in `141aa1580`. |
| GAP-B | Open | Not traced in this pass: writers of `C+0xad0`, `C+0xad8`, `+0x918`, `+0x91c`, `+0x938`. |
| GAP-C | Open | Mode-1 selector closed: index 28 (`141aca670`, `0141ac0000.c`; `141aca4a0` writes `H+0xc0 = k`; see `k-replika-binding.md`). Write filter `141aa3c60` (403 B; state `H+0x2dc0` L / `H+0x4240` R, 0x1480 B; returns `+0xa4`, the lp of the second section, equal to xmm0 on every kemu call) PORTED, NOT WIRED (piece 3a: `replika/mode1.rs` `WriteFilter`; vectors `replika-native-mode1-write-{l,r}.json` from the real lane state after index 28 = 0.3 on noise; max abs error 6.6e-7 (L) and 7.2e-7 (R), test tolerance 1e-5). Exact-order ports that make this possible: the trig of `14199ae00` (sin) and `14199b1d0` (cos, sine fold) from the disassembly (`disasm/replika-mode1-listings.txt`, constants `disasm/replika-mode1-constants.txt`), vectors `replika-native-trig.json` (2048 lanes, bit-exact); `tpt_svf::Section::tick` = `140d947a0` in its native operation order (the core's replika vectors are now exact too); the T4 table entries are f32 of the exact `2^(k/60-20)` (checked, 2400 entries), `t4` now matches the native index semantics. Helpers, 4 lanes each: `14199ae00` xmm0 = sin(v_i), xmm1 = |v_i|; `14199b1d0` xmm0 = cos(v_i), xmm1 = sin(|v_i| - k pi/2) with k = round(2|v_i|/pi) (the decompile's RAX lanes are xmm0 lanes 0-1); `14199c5b0` = `t^2/2 - sum w_i cos(c_i t) - k` for |t| <= 1 (sum order `(P0+P2)+(P1+P3)`), `t + m` / `m - t` outside; `141aa31f0` = ADAA of the shaper (direct form below the 1e-4 step). Large-argument branches: `14199ae00` at |v| >= 25966 (`1476a3760`), `14199b1d0` at |v| >= 18838 (`1476a3750`), a 2/pi bit-table reduction (`1476a3390`), not ported and not reached by the write path. MT `1410ec0c0` is standard MT19937 (the decompile's pre-shifted masks equal 0x9d2c5680 and 0xefc60000; mask word `+0x1444` = 0xffffffff), reused as `timemod::mt::Mt`; the ring is the 1248-entry double buffer at `+0xc4`, read by the vector loader. Design chain PORTED (`replika/design.rs`): `141abbc90` sets the lane and post coefficients through `141ac3fd0` (lane, four mode branches), `1419529e0` (g = tangent series of pi f / fs), `14199dbf0` (k = r - 1/r, r = 2^(bw/2)) and `141ac96c0` (k = 2 - 2 arg). Bit-exact (tolerance 0 checked) against `vectors/replika-native-mode1-design.json` (kemu via `tools/re/replika_mode1_design_kemu.py`, rates 44.1/48/96 kHz, modes 0..3) and the core-run mode-3 vectors. Lane envelope exp(-75 pi / fs) (`1479ab720`), post envelope exp(-50 pi / fs) (`1479ab718`), post gain = the damping k. Index 28 = 0.3 (k = 1) gives lane mode 3 (`lane+0x1454`, `H+0x8c`), so the write vectors' "mode 1" label names the kernel index; the k-to-mode map sits in `141aca4a0` and is not traced. Not ported: the resets `141abaab0` at `H+0x1a8` and `H+0x278` (flag `+0xc8` = 1 for modes 1..3, else 2). Open: the lane MT seed (its init is not read); `141aca4a0` also calls `141abd190` (115 B; per mode, `+0x50` = a product of the doubles at `+0`, `+0x38`, `+0x40` for k = 1..3, then clamp at 1.0 and the wrap flags at `+0x57c8..+0x57e0`) and `141ac4510` (105 B; `H+0x98` = double of the float at `+0xa0 + 4k`; mode-1 time field `H+0xa4`, writer not traced); post filter `141aa3e00` (189 B) CLOSED (piece 3a: `replika/mode1.rs`, unit vectors `replika-native-mode1-postfilter-{l,r}.json`, max abs error 7.5e-8 and 5.2e-8); kernel 1 `141ab87d0` (549 B) with `14199dcc0` (182 B); read position, speed and wrap in `141ab8c60` (`H+0x58` / `H+0x60`, mask `H+0x1c0`); delay positions `141abd0b0` (214 B) and `141abd290` (569 B, GAP-M). Kernels `141ab8a00` (530 B, kernel 0) and `141ab7fe0` (754 B, kernel 2) are void in the text. Reference vectors: `vectors/replika-native-mode1-*.json`. Piece 3b, parts PORTED, NOT WIRED: read tap `141ab87d0` + `14199dcc0` (`replika/read.rs`, vectors `replika-native-tap-read.json`, 1e-5, PR #335); deposit `141accfb0` (`replika/deposit.rs`, `replika-native-deposit.json`, relative 1e-5, PR #336); delay leaf `141abd0b0` (`replika/delay.rs`, `replika-native-delay-pos.json`, relative 1e-5, PR #338). Still open for 3b: the driver `141ab8c60` (1240 B: sets the tap positions `pos` / `x`, glide, mode-2 branch, flags `H+0x270` / `H+0x340`), the caller `141abd290` (569 B, GAP-M: constants `144f38790`, `14756bdf8`, `1479ab4e8`, `1479ab508`, `144762eb8`, `1447617c8`, the live/stored source flag `+0x938`), the mode writer `141aca4a0` (k-to-mode map, 460 B) and selector `141aca670` (1200 B, index 28). Reset `141abaab0` is read (sets `+0x84`, `+0x8c` to zero and `+0x90`, `+0x98`, `+0xa0` to 1.0). Index 28 stays unwired until the driver and the selector are ported. |
| GAP-D | Open | Unchanged: glide 3 step, `F+0x100` object, writer of `F+0x5`. |
| GAP-E | Closed | Rate setup `141ae28c0` (limiter slot 3, `0141ae0000.c:2118`): `L+0x40` = rate. Attack `L+0xcc` = 1 - 200/rate (`144f38ce4` = 200, u32 `43480000`). Second dB state `L+0xbc`: rise `L+0xc0` = 1 - (1/15)/rate (`144f386a8`, u32 `3d888889`), fall `L+0xc4` = 1 - (1/3)/rate (`144760108`); update in `141ae3ba0`. Gain = -max(0, env, hold) dB. Index 4 sets `L+0xad` (mode): 1 = the first envelope rises at once, else attack. Ported in `db_limiter.rs`, with the output clamp kept. Open: release before the index-1 setter (default `L+0xb0`, so release starts at 0 in the port); ramp lengths `L+0x88` and `+0x8c` (not applied); rebuild `141ae6c60` (writes `+0xd0` and `+0xc0`, reached from `141ae9e40`, which is not a slot of this vtable; not applied). |
| GAP-F | Closed for R and the polynomial | c50 (R of section A, `B+0x50`) and `B+0x84` (R of section B) = 2.0: `141ac9c50` (`0141ac0000.c:6018`) writes `0x40000000` when `144714994` is nonzero (u32 `40000000`). Sections at 2205 Hz (`1479ab60c`) and 5 Hz (`1447149f0`), Q = 1 (`14470bad4`). Polynomial: `141abe910` with K = 0.05 (`1447148c0`, u32 `3d4ccccd`), exponents (0, -40, -45, -49, -40) from slot 1 `141a9e300`, divided by the raw base 1.0668702: c4 = 0.0453523, c8 = -0.2085480, c0c = -0.0266059, c10 = 0.1499714. Output `141aa44a0`: g (tanh(d x) + (P - c50 A.out5 - A.out6) c18), with c14 = c18 = 1, d = 10^a, g = 10^(-a/2). Ported as `tpt_svf::Saturator` (not wired). Open: writer of `C+0x142a8` (amount glide coefficient). |
| GAP-H | Open | Unchanged. |
| GAP-K | Closed | Lanes 2 and 3: `144714b78` and `144714b7c` = 1.0 (f32 `3f800000`, read from the PE `.rdata`). Lanes 0 and 1 = 1.0 (`144714b70`; its u64 `3f8000003f800000` covers `144714b74` too). Each frame adds 1.0 to all four lane phases (`141a9f4d0`). |
| GAP-N | Open | Not read in this pass: `141acbfb0` (mode rebuild), `141a9d4a0` / `141a9d660` / `141a9d7c0` (index 8 table path), `141a90910` (index 0/1 conversion), `141acc600` (ramp length `+0x62d8`). |
| GAP-O | Open | `14199ce60` (543 B) beyond its first 40 lines, not read. |
| GAP-G, I, J, L, M, P, Q | Open | Out of scope this pass; addresses as in `k-replika-core.md`. |

### Name binding (task 1)

Not confirmed as a table. Findings:
- The setter path `140754060` (4291 B, `0140750000.c:3475`) is a cereal serializer: its arguments are
  `StaticObject<cereal::detail::Versions>`, and `uVar16 = uVar15 | 0x145f0` (`:3891-3921`) is a
  variant tag. It does not bind indices.
- The 53 indices are the slots of the Replika wrapper's stream array at `+0x145f0` (8-byte pointers).
  The per-quantum reader `1408f98d0` loops over 0x35 entries (`01408f0000.c:6097`); the ctor
  `1407c7d40` zeroes them (`01407c0000.c:4522`). No store into that array is in the decompile text, so
  the host writer that maps ENGINE_PAR ids to slots is not located.
- `crates/sampler-core/src/engine_par_ids.tsv` `internal_id` cannot bind by itself: TIME (`0x2c0`) and
  SATURATION (`0x2c4`) are not a fixed offset from indices 8 and 29, and `set_engine_par` tests `0x414`
  as the step-modulator count (`0140780000.c:6105`), while the table gives `0x414` to RDL_WIDTH.
- Named now (6 of 53): index 8 = TIME (medium: TSV linear 2000 ms at raw 1e6 equals `141ac5980`
  case 8 at k = 0.5, 4000 x 0.5 v); index 9 = TYPE (inferred: `trunc(4v)` clamped 0..3 against TSV
  TYPE 0..4); indices 2 and 3 = LOWCUT and HIGHCUT (inferred from the GAP-D sweep shapes); index 29 =
  SATURATION (inferred amount target); index 16 = WIDTH (inferred side scale). 47 unnamed. The Kontakt
  mapping is not changed: nothing reached confirmed.

### Tests (sampler-core, `build-slot cargo test -p sampler-core --lib <filter> -j 4`)

- `limiter`: 11 pass, including `rate_setup_coefficients_match_the_decoded_constants`,
  `attack_coefficient_sets_the_first_frame_gain`, `instant_mode_reaches_the_peak_on_the_first_frame`,
  `second_db_state_sets_the_gain_when_above_the_envelope`, and the output clamp test.
- `svf`: 12 pass, including `saturator_polynomial_is_the_normalised_exponent_sum`,
  `saturator_sections_are_r_two_at_their_decoded_cutoffs` and `saturator_dc_output_is_g_times_tanh_of_the_drive`.
- `saturator`: 8 pass (the three new ones above, plus the existing color saturator tests).

---

## Loop pass

Method: decompile text only (`0141aa0000.c`, `0141ac0000.c`, `0141ab0000.c`, `0141a70000.c`), plus the
`tpt_svf` and `timemod` code already in the tree. No binary, Ghidra, radare2, REA, Wine or Kontakt; nothing
measured against Kontakt. No native vector.

### Wired

1. **Saturator (SATURATION, setter index 29). CONFIRMED law, tie INFERRED.**
   - Setter `141ac5980` case 0x1d (`0141ac0000.c` line 3654): the target goes to `C+0x142b0`; live (`C+0x8`
     set, the ctor default 1) writes the amount `C+0x142ac = v` at once and calls `141accd10`.
   - Placement: loop path 0 (`C+0x604 == 0`, the ctor default; no writer found, GAP-L). `0141aa0000.c`
     5452-5474: when `a != 0`, the tap runs through `141aa44a0` (`tpt_svf::Saturator`), then the tap is
     `(1 - a) dry + a sat(tap)`. Drive `d = 10^a` (`powf` base f32 `144714a00`), gain `g = 10^(-a/2)`.
   - Mapping: `Line::saturation` (sampler-ir), `dsp/timemod` runs the stage on the line input before the
     write. Generic lines only (`Native::None`). The four state frames live in the slab, reset when the ring
     is empty, flushed at block end.
   - Approximation: the decompile's tap carries the lane product `m` and the tap gain `Ctl(0x68)`, and the
     dry term does not. Neither is modelled, so the dry term is the line input. At `a = 0` the stage is
     skipped, which is exact for this port. The decompile's `a = 0` crossfade differs by `m x Ctl(0x68)`.

### Still open

2. **Output term `OUT = F1 x Ctl(0x60) + F2`** (`141aa9480` step 13). Not wired.
   - SUPERSEDED by "Heads and feedback pass" below: the default is 1.0 (slot-1 init `141a9e300`), and the
     bypass fade `141acc600` writes `P3 + 0x60`. The paragraph below (default 0, no writer) is wrong.
   - `Ctl(0x60)` = block `P0 + 0x60`. Default **0**: the block allocator `141a7d480` zeroes every 0x70-byte
     block, and nothing writes the offset. GAP-J stays open. The setter switch `141ac5980` has no store at
     that offset (checked: its only `+0x60` references are virtual calls on sub-objects). The bypass fade
     `141acc600` writes sub-object targets (`C+0x62cc`, `+0x1144c`, `+0x1292c`), not `Ctl(0x60)`. The
     earlier note that it writes `P3 + 0x60` is not supported by the text.
   - F2 is the cut after head 2 (step 12: F1 out is written to head 2, read back, then filtered by F2). Head 2
     is a separate read (GAP-C: ring addressing, head time field), which the generic line does not have.
     So F2 has no faithful position. The output stays the head sum. At the default weight the decompile
     output is F2 of the head 2 read, which is the head sum only when the cut is neutral.
   - LOWCUT/HIGHCUT are the cut stages (GAP-D). They stay on the recirculated signal, inert while the matrix
     is zero, so the output is unchanged by them.

3. **Feedback (FEEDBACK). Not wired, kept at 0.**
   - No index is tied to FEEDBACK (name binding still open, see "Name binding" above).
   - The recirculating path is F2 out, written to head 3, read next frame (`0141aa0000.c` step 12). Head 3's
     gain comes from the head gains (setter index 12 through `141abba60`, tie UNKNOWN, GAP-C).
   - The matrix writes (step 16) use `Ctl(0x8) x Ctl(0x1c)` (dry), `Ctl(0x64)` (side, idx 16), and `Ctl(0x4)`
     (output). None multiplies the recirculated signal, so no feedback scalar is confirmed.
   - Test (c), the echo scale, is not written: the law it would check is not confirmed.
   - Open address: `141aa9480` step 12, `141abba60`, GAP-C.

4. **Ducking (DUCKING_SENSE, DUCKING_RELEASE, DUCKING_AMOUNT).** Closed in piece 2 (see "Piece 2: ducking" at the end).

### Unchanged opens (addresses)

- GAP-A: ramp length `n` in `141aa1580`.
- GAP-C: head ring writer `H+0x15c`, mode-1 kernels `141ab8c60` / `141ab8a00` / `141ab87d0` / `141ab7fe0`,
  blend `141ab7e00`.
- GAP-D: glide 3 step `F+0x1430`, `F+0x100` class and flag.
- GAP-F: the amount glide coefficient `C+0x142a8` (live path does not use it).
- GAP-I: LFO consumers `+0x94c` / `+0x960` in `141ab7480`.
- GAP-L: writers of `C+0x600` / `C+0x604` / `C+0x630..+0x638` / `C+0x6cc` / `C+0x6d0` (path 0 assumed).

### Mapping (sampler-kontakt `replika_config`)

- SATURATION: mapped (no note). Outside 0..1 (or non-finite): InvalidValue, mapping returns None.
- LOWCUT, HIGHCUT: still UnknownLawOmitted (law confirmed, binding not, GAP-D). The open address is in the note.
- FEEDBACK: still UnknownLawOmitted, open at `141aa9480` step 12 (note text).
- DUCKING_*: wired in piece 2 (the gate; the saved-field tie is INFERRED).
- depth, rate, modulation: UnknownLawOmitted, open at GAP-I (note text).

### Tests

- `dsp::tpt_svf::replika_saturator_full_amount_is_ten_and_minus_half_power_of_drive`: DC limit at a = 1 is
  `10^-0.5 tanh(10 x 0.1)`.
- `dsp::timemod::replika_zero_saturation_passes_the_delayed_input_exactly`: saturation 0 returns the delayed
  impulse exactly (no stage).
- `dsp::timemod::replika_full_saturation_delivers_gain_times_tanh_of_drive_at_dc`: line output at DC is
  `10^-0.5 tanh(1)`.
- `effects::timemod::replika_saturation_maps_into_the_line_input` (sampler-kontakt): maps 0.5, rejects 1.5.
- `replika_time_law_is_documented_and_the_rest_is_reported`: note count 28 -> 27 (saturation is mapped).
- Existing `loop_cut_acts_only_through_the_feedback_matrix` (zero matrix bit-identical): passes.

---

## Heads and feedback pass

Scope: GAP-C (head addressing and read), GAP-J (writer of Ctl(0x60)), the output term, and the feedback law
(`141aa9480` step 12, head gain `141abba60`). Method: decompile text (`0141aa0000.c`, `0141ab0000.c`,
`0141ac0000.c`, `0141a90000.c`) only; no binary, Ghidra, radare2, REA, Wine or Kontakt. Nothing measured
against Kontakt. No native vector. Ported code is unwired (`crates/sampler-core/src/dsp/replika_head.rs`).

### Summary

| Item | Status | Address |
|---|---|---|
| GAP-C mode-0 write slot | Closed (ported, unwired) | `141acd110`, `0141ac0000.c:8436-8470` |
| GAP-C mode-0 tap position (wrap, clamp) | Closed (ported, unwired); sample blend open | `141ab7e00`, `0141ab0000.c:4522-4560` |
| GAP-C mode-0 left offset `min(0, s k T + b) + base` | Closed (ported, unwired) | `141ab82e0`, `0141ab0000.c:4640-4700` |
| GAP-C sample blend of the tap read | Open | `141ab7e00` return value is not in the text |
| GAP-C mode-1 selector | Closed (emulated) | index 28: `141aca670` (`0141ac0000.c`) computes k = min(4, trunc(5 v)); `141aca4a0` writes `H+0xc0 = k`; `tools/re/replika_mode1_kemu.py` |
| GAP-C mode-1 kernels (playing reader) | Open | `141ab8a00` (4875), `141ab7fe0` (4562): void in text; `141ab87d0` (4810) needs `14199dcc0` (unread) |
| GAP-C mode-1 mask and length | Open | `141ab4e30` active branch (2551-2767) writes `H+0x1c0` from a table at `H+0x1003a8`; no Len_L/Len_R write there |
| GAP-C head time field `H+0xa0..0xac` | Open | writer not traced |
| GAP-J writer of Ctl(0x60) | Closed (writers found; default 1.0) | slot-1 init `141a9e300` (`0141a90000.c:9830-9834`); bypass fade `141acc600` (`0141ac0000.c:8022`) |
| Output term `OUT = F1 Ctl(0x60) + F2` | Open (formula ported, not wired) | `141aa9480` step 13; needs heads 2-3 |
| Feedback scalar | Open | `141aa9480` step 12; head 3 gain `141ac5980` case 0xc, `141abba60` |
| FEEDBACK index binding | Open | name binding (above) |

### GAP-C: head addressing and read (partly closed)

Closed (CONFIRMED by decompile arithmetic):
- Write slot (`141acd110`, mode 0). Slot = `a` for `0 <= a < len`, `a - len` for `a >= len`, `a + len` for
  `a < 0`. Port: `replika_head::write_slot`.
- Tap position (`141ab7e00`). `x = a + p`; subtract `len` once if `x >= len`; add `len` if `x < 0`; clamp to
  `[0, len - 1]`. The function returns a value in the decompiled caller (`141ab82e0` stores it as the tap), but
  the decompile text of `141ab7e00` ends in a bare `return`. Port: `replika_head::tap_position`.
- Mode-0 left offset (`141ab82e0`, `fVar10` path). `min(0, s k T + b) + base`, with `s` = f64 `H+0x98`
  (cast to f32), `k` = f32 `H+0xb0`, `T` = f32 `C+0x968`, `b` = `H+0xb4`, `base` = f64 `H+0xd8`. The right
  channel uses `g U + (1 - g) T` (`U` = f64 `C+0x970`, `g` = `H+0x57e8`). Port: `replika_head::mode0_offset`.
- Bias `b` (`H+0xb4`): INFERRED `-trunc(a k + 2.0)` (`a` = f32 `H+0xa0`, 2.0 = f32 `144714994`). The text
  shows `(uint)(...) ^ DAT_144714ba0`; the int-to-float step is not in the text, so the sign-XOR reading is
  the inferred one. Port: `replika_head::head_bias`.

Open:
- Sample blend of the tap read. The blend is the value `141ab7e00` returns. Its text lost the return; the
  mode-0 read has no sample value in this port.
- Mode-1 reader `141ab8c60` (`0141ab0000.c:4937-5100`). It is the playing reader (`H+0xc0 = 1`, set by
  `141ab4e30`). Its selection reads `H+0x270` (L) and `H+0x340` (R): 0 = `141ab8a00`, 1 = `141ab87d0`,
  2 = `141ab7fe0`, other = 0. `141ab8a00` (530 B, 4875) and `141ab7fe0` (754 B, 4562) have a `void`
  signature in the text but their callers use the return value; the return is not recoverable. `141ab87d0`
  (549 B, 4810) returns the linear blend of `buf[i]` and `buf[i+1]` after a catch-up pass of a two-state
  section (the `c2 = A`, `c3 = k`, `c4 = B` form of the saturator section, CONFIRMED by arithmetic); it calls
  `14199dcc0`, which is not read. The selector is index 28 (closed; see `k-replika-binding.md`, Mode-1 selector).
- Active branch of `141ab4e30` (`0141ab0000.c:2551-2767`). The n_ring write (`H+0x158`, `H+0x220`) and the
  `n_ring < 1` zeroing of `H+0x15c` are confirmed. In the active branch the function writes `H+0x1c0`
  (the mode-1 wrap mask) from a 32-entry int table at `H+0x1003a8`, indexed by `H+0x1a8` (clamped to 0..31),
  and a pending-update list at `H+0x40046c`. These offsets are far outside the head layout (heads are about
  `0x58c0` apart), so the object they belong to is unresolved. The function does not write `H+0x15c` or
  `H+0x194` on the active branch. The one candidate found, `141ac8a40` (`0141ac0000.c:5164`), writes `+0x15c`
  on the object at its `+0x38`, which is not identified as a head. Open.
- Head time field `H+0xa0..0xac`: writer not traced.

### GAP-J: writer of Ctl(0x60) (closed; the earlier default-0 note is wrong)

Two writers, both on the block pointers of the core (`C+0x80` P0, `C+0x98` P1, `C+0xb0` P2, `C+0xc8` P3,
`C+0xe0` P4), offset `+0x60`:
- Slot-1 init `141a9e300` (`0141a90000.c:9606-9834`). After `141a998a0` zeroes the triple (line 9624), it
  writes P1 = 0 (`0141a90000.c:9830`), then P2, P3, P0 and P4 = `0x3f800000` = 1.0 (lines 9831-9834).
  Confirmed by the offsets: `param_1[0x10]` = `C+0x80`, `[0x13]` = `C+0x98`, `[0x16]` = `C+0xb0`,
  `[0x19]` = `C+0xc8`, `[0x1c]` = `C+0xe0`.
- Bypass fade `141acc600` (`0141ac0000.c:7942-8025`). Line 8022-8023: `*(float*)(*(longlong*)(C+0xc8) + 0x60) =
  fVar4`, where `fVar4 = 1.0` if any sub-effect enable is set (`C+0x14381`, `+0x14382`, `+0x14383`, `+0x14384`,
  `+0x914`, `+0x915` are not all zero) and 0.0 otherwise. It then sets the pending flags `C+0x114 = 1` and
  `C+0x110 = 1`, so the change is glided (GAP-A), not stepped. Its callers are the enable cases of the setter
  (indices 6, 7, 10, 11, 12, 15; e.g. `0141ac0000.c:3444`, `3453`, `3524-3608`).
- Setter `141ac5980` has no store at `+0x60` of any block (the index table in `k-replika-core.md` has no
  `+0x60` entry). The copies in the epilogue (`141abbac0`) and the commit (`141a956d0`) move the value without
  changing it.

Effect: Ctl(0x60) is 1.0 at init and whenever an enable is set. At reset, index 15 has its default 0.3
(`144f38764`), so the enable `C+0x14384` is set and Ctl(0x60) = 1.0. The enables are indices 6, 7, 10, 11,
12, 15, and their saved-field binding is open (name binding). The port therefore keeps Ctl(0x60) at its
slot-1 default 1.0 (`replika_head::OUT_GAIN_DEFAULT`), and does not switch it.

### Output term `OUT = F1 Ctl(0x60) + F2` (open; formula ported)

- `replika_head::output_term(f1, f2, ctl60) = f1 ctl60 + f2`. At the default it is `F1 + F2` (bit for bit, test
  `output_term_at_default_is_f1_plus_f2`). This is NOT bit-identical to the current unwired output, which has
  no F1 or F2 term; F1 is included at its default gain of 1.0. Only Ctl(0x60) = 0 gives F2 alone.
- Not wired: F1 is the cut on the head sum (`Mo = (L3 + L1) m`, step 12), and F2 is the same cut on the head 2
  read of F1's output. Head 2 needs the mode-1 reader (GAP-C open), so F2 has no faithful position. The
  current loop's cut sits on the recirculated signal, which is zero while the feedback matrix is zero.

### Feedback law (open)

- Structure (`141aa9480` step 12, CONFIRMED): F2's output is written to head 3 and read at the start of the
  next frame. No separate feedback coefficient appears in step 12.
- Head 3's gain is set by setter index 12 (`141ac5980` case 0xc, `0141ac0000.c:3547-3584`): at v = 0.5 all three
  head gains are 1.0. For v > 0.5, head 1 = 1, head 2 = `max(2(1 - v), f9)`, head 3 = 1 / head 2. For v < 0.5
  the head 1 and head 3 gains use `max(2v, f9)` and head 2 = 1 / f9. The floor `f9` = f32 `14a6b4594 x 1479ab230
  + 14a6b4590` is not a closed value: the two words 14a6b4590 and 14a6b4594 are unbacked in rdata
  (`constants.tsv` `.data(bss-or-unbacked)`), set elsewhere and not traced; 1479ab230 is an rdata constant
  (f32 0.650085, 8-byte read 1.7285e-4). So the law is not a closed scalar. The write goes
  through `141abba60` (`0141ab0000.c:6522-6537`): target `H+0x584c`, step `(v - cur) / len` (`H+0x5850`,
  `len` = `H+0x5858`), and output `H+0x74`, which `141ab7a90` multiplies into the read.
- FEEDBACK index: not bound (name binding section above). Without it the saved FEEDBACK control cannot be
  wired.
- Result: feedback stays at zero (`UnknownLawOmitted`), and test (c) is not written.

### LOWCUT and HIGHCUT

- Still `UnknownLawOmitted`. The note now says the open reason: the binding (GAP-D), and that F1 and F2 need
  heads 2-3 (GAP-C).
- Audible on the output: NO. The port's cut is applied to the recirculated signal before the feedback
  matrix, and the matrix is zero, so the cut never reaches the output. The decompile places F1 on the output
  path and F2 after head 2; neither position exists in the port.

### Mapping (sampler-kontakt `replika_config`)

- No control moved to mapped. Notes updated: `feedback` (141aa9480 step 12, index 12 floor, FEEDBACK
  binding), `lowcut` and `highcut` (binding, heads 2-3).
- `replika_time_ms` clamp corrected to 4000 (`DAT_14756bdf8`, as `k-replika-core.md` says). Output is unchanged
  for v in 0..=1 (the largest value is 2000 ms).

### Tests

- `dsp::replika_head::tests::mode0_head_offset_is_the_clipped_bias_law` (test a): `b = -trunc(10 x 1.5 + 2) = -17`;
  offsets -17, -15.5 and 0 (positive excursion clipped).
- `dsp::replika_head::tests::output_term_at_default_is_f1_plus_f2` (test b): at `Ctl(0x60) = 1.0` the term is
  `F1 + F2` bit for bit; at 0.0 it is `F2`.
- `dsp::replika_head::tests::write_slot_and_tap_position_wrap_the_ring`: wrap and clamp of the ring laws.
- Test (c), the feedback echo scale, is not written (the feedback law is open).
- Existing tests unchanged and passing: `loop_cut_acts_only_through_the_feedback_matrix`, the saturator-at-zero
  path (`replika_zero_saturation_passes_the_delayed_input_exactly`), and the svf, timemod and sampler-kontakt
  replika filters.

## Piece 1: core port (`engine/replika-port`)

Ported in `crates/sampler-core/src/dsp/replika/` (`ReplikaCore`, `ReplikaState`) and wired into `TimeMod` as
`Native::Replika(ReplikaControls)` (sampler-ir). The rings and the packed state are slab entries
(`storage_len` + `STATE_SLOTS`; 29 words, `ReplikaState::load`/`store`); `DbLimiter` exposes its two dB states for
the packing. `replika_config` maps TIME (8), FEEDBACK (5), LOWCUT (2), HIGHCUT (3) and SATURATION (29). Their notes
are removed; the other controls keep their notes (24 in total).

Evidence. `dsp::replika::tests` run the seven emulated native vectors (kemu output, not a Kontakt render) in 32-frame
quanta. Max abs error per case: default 2.384e-7, time-v0p01 2.384e-7, time-v0p03 2.384e-7, feedback-v0p8 2.384e-7,
saturation-v0p5 1.192e-7, lowcut-v0p5 2.384e-7, highcut-v0p5 1.192e-7. Tolerance 1e-5; no frame exceeds it.
Also: the slab path equals the bare core frame for frame (`timemod::tests::replika_stage_equals_the_bare_core`), and a
packed state resumes the run exactly (`replika::tests::packed_state_resumes_the_run_exactly`).

Laws added in this piece:
- Input and output level (indices 0 and 1): `idx = trunc(v x 499.980408 + 819.609009)`, T4 linear interpolation, then
  `(T4 - C) x D` with C = 0.0123456742 and D = 1.00311076. The output stage is from the disassembly of `141a90910`;
  C and D are fitted to the native measurements (not in `constants.tsv`; residual about 1e-7).
- Feedback: 1.2 x index 5 on the head 2 and head 3 outputs (`1479ab410`).
- Head 3 path at half scale (`0141aa0000.c` 5561-5578); matrix limiter x 0.5 before and x 2 after.
- Head offsets `[[0, 360], [540, -180], [-180, 540]]` with bias 2: delays 1742/2102, 2282/1562, 1562/2282 at TIME 0.02.
  The offsets are verified by the vectors at TIME 0.01, 0.02 and 0.03 only.

Open after piece 1:
- Ducking (GAP-B, GAP-M): closed in piece 2 (the gate is ported; GAP-M is the head update, not the ducker; see below).
- Mode-1 head kernels (GAP-C): open. The selector is closed (index 28); the chain is in the GAP-C table row.
- Gate closed branch (|mid| >= threshold, GAP-G): the threshold is 1.0 at reset and no vector reaches it.
- Head gain index 12 (floor `f9` unbacked): not wired.
- LFO and rotator (depth, rate, modulation): head read depth is 0 in the vectors, so they are inert; lane products `m`
  are 1 in every vector.
- Ctl(0x60) is fixed at 1.0 (GAP-J).
- Not wired from Kontakt (reset values): input levels (0, 1), DRY_WET (4), PAN_DRY (13), PAN_WET (14), DELAY_OFFSET (15),
  WIDTH (16), TAP_GAIN (52).
- The slot's `dry` level does not reach native Replika output (other natives do the same); the core's DRY_WET is wet only
  at reset. This differs from the generic line, which mixed `dry x input`. Needs a Kontakt check if a slot dry must sum in.
- Feedback tail not reported to the engine (`tail_ms` is 0 for natives in sampler-ir).
- Ring is 2.5 s per head (2^17 frames at 48 kHz); the native rings are 2^19. Output is equal while the longest read
  (2000 ms + 542 frames) fits.

---

## Piece 2: ducking (`engine/replika-ducking`)

Method: decompile text (`0141aa0000.c`, `0141ab0000.c`, `0141ac0000.c`, `0141a90000.c`), `constants.tsv`, and the
kemu emulation of the core (`tools/re/replika_ducking_kemu.py`). Numbers and the native vectors are in
`k-replika-binding.md` ("Ducking (piece 2)").

### Ducker (the gate of `141aa9480`), CONFIRMED arithmetic and emulated

- Detector: instantaneous |mid|, mid = 0.5 (tapA + tapB) after saturation (`0141aa0000.c` 5551-5552). No follower.
- Threshold `+0x13e18` = max(1 - v17, 0.001) (setter case 0x11, `0141ac0000.c` 3614-3618).
- Target `+0x13e10` = -60 v18 dB (case 0x12, 3621).
- Closed branch (|mid| < threshold): env = env x `+0x13e28`, the release factor of case 0x13 (3624-3631):
  0.05^(1 / ((200 + 7300 v19) ms x 0.001 x rate)). Open branch: env = (env - target) x `+0x13e20` + target, where
  `+0x13e20` = 0.05^(333.33331750085034 / rate) (setup 1103), an attack of about 3 ms to 95 %, not controlled.
- Gain: W x 10^(env/20) through the T4 index env x 9.96578407 + 1200 (5598-5625), on both wet taps, before the matrix.
- Labels: arithmetic and emulation CONFIRMED; names (sense, amount, release) INFERRED.
- Port: `crates/sampler-core/src/dsp/replika/mod.rs` (`Gate`, env packed in the slab as word 30), wired from
  `sampler-kontakt` `replika_config` and `sampler-ir` `ReplikaControls`.

### Status

| Item | Status | Address |
| --- | --- | --- |
| GAP-M, ducking body `141abd0b0` / `141abd290` | Closed as not the ducker: `141abd290` is the head delay-position update (TIME from smoother 0, idx 11, 10, 15 as spread and offset) that calls `141abd0b0` for heads 1-3 (`+0xc8`/`+0xd0` read offsets, `+0x58`/`+0x60` speed ratios). It also sets limiter C+0x13f80 parameter 1 to max(max(d5, d8), 1/12) (`0141ab0000.c` 7750). That limiter link is OPEN. | `0141ab0000.c` 7561-7765 |
| GAP-B `+0xad0`, `+0xad8` (bounds read in `141abd290`) | OPEN. No writer of a double there: the offsets hold an int table (`141a80000.c` 31-32 writes int 0xb at `+0xad8`; `0141ab0000.c` 9482 writes uints at `+0xad8 + 4k`). The double reads alias that table. | `0141ab0000.c` 7710-7711 |
| GAP-B `+0x938` (byte, smoothed copies flag) | Closed: `141aca670` sets it to 1 when path (`+0x604`) is not 0 or 4 and `+0x939` is clear (`0141ac0000.c` 6452); 0 on path 0; `141abb2c0` writes 0 (`0141ab0000.c` 6348, 6368). Read in the loop (5534) and `141a9dbc0` (`0141a90000.c` 9033). | |
| GAP-B `+0x918`, `+0x91c` (mid scales, paths 1 and 2) | Closed as writers: `141ac9420` (`0141ac0000.c` 5684-5735). Not used on path 0. | |
| Tap followers `+0x142c8` (L), `+0x142f8` (R) | Open, no reader. Updated each frame: attack coefficient exp(-10000 / rate) (0.1 ms, `DAT_1479ab738` = -10000), release exp(-6.6667 / rate) (150 ms, `1479ab6e8`), state read only by its own update (`0141aa0000.c` 5433-5448; coefficients set in setup 990-1022). Not ported. | `0141aa0000.c` 5433-5448 |
| Release factor written before prepare | Open (order). The setter 19 computes its factor from the block rate and base `+0x13e30` (0 until setup). A write before prepare gives factor 0 (instant release). The harness routes 19 after prepare every quantum; the port uses the prepared rate. | `0141aa1580` 1100-1105; `0141ac0000.c` 3624-3631 |
| Saved DUCKING_* to index tie | INFERRED (SENSE 29 -> 17, RELEASE 30 -> 19, AMOUNT 31 -> 18). No decompile text binds the wrapper slots. | field table in `docs/re/kontakt-bindings/fx_raum_delight_psyche_vibrato_phasis_replika.tsv` |

### Tests

- `dsp::replika::tests::ducking_matches_native_vectors_at_amount_zero_and_half`: `ducking-amount0` (1.19e-7),
  `ducking-amount0p5` (7.45e-8), `ducking-amount0p5-release1` (1.12e-8). Max abs error over both channels.
- `effects::timemod::tests::replika_ducking_maps_into_the_core`: fields 29, 30, 31 reach indices 17, 19, 18; 1.5 is rejected.
- Existing vectors (default, time, feedback, saturation, lowcut, highcut) unchanged: at amount 0 the gain is exactly 1.

