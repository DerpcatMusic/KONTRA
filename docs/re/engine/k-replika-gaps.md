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
- Highpass-type (`141abe6a0`): g = g0; R = `F+0xbc` (writer UNKNOWN).
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
- Glide 2 (h): target `F+0x141c` = (v2 != 0) (`141abe6a0`). Coefficient `F+0x1414` is written by
  `141ac97e0` as the raw f64 -2.9957, not an exponential. As a one-pole coefficient that diverges.
  ANOMALY: treat as UNKNOWN until read again. Snap `F+0x1420` = 0.001.
- Glide 3: length `F+0x143c` = int(0.025 fs). Snap `F+0x1434` = 0.001. Step `F+0x1430` writer UNKNOWN.
- `F+0x0` = fs (`141ac97e0` stores it), set in setup before use.

**Label.** CONFIRMED: section algebra, both sweep laws, constants, mix, chain order, glide targets,
glide 1 coefficient and snaps. INFERRED: LOWCUT/HIGHCUT names; out6 as lowpass. UNKNOWN: highpass R,
glide 2 coefficient (anomaly), glide 3 step, `F+0x100` class and flag.

**Port consequence.** A TPT SVF pair with the sweep laws and fixed mix ports directly. Blocked by
the highpass R (`F+0xbc`; saturator R terms too, GAP-F), glide 2 coefficient, glide 3 step, and the
`F+0x100` object.

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
drive and gain. Exact port blocked by c50 and `B+0x84` (section damping).

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

## Open items

1. GAP-A: lanes 2-3 phase increments (`144714b78`, `144714b7c`); 147697658; `n`.
2. GAP-C: writer of `H+0x15c` and `H+0x194` on the active branch; head time field; kernels
   `141ab8a00`, `141ab87d0`, `141ab7fe0`; blend in `141ab7e00`.
3. GAP-D: highpass R (`F+0xbc`); glide 2 coefficient (`F+0x1414`, anomaly); glide 3 step
   (`F+0x1430`); `F+0x100` class and flag.
4. GAP-E: `L+0x40` writer; rest of `141ae3ba0`; other parameter writers.
5. GAP-F: R of section A (`B+0x50`) and section B (`B+0x84`); `C+0x142a8` writer.
6. GAP-H: type 2 on `C+0x618`/`C+0x718` on all paths; second argument.

Core portable: partly. Blocked by the section R terms (GAP-D, GAP-F), the mode-1 head reader and
time source (GAP-C), and the ramp scale (GAP-A). The limiter (GAP-E) and the saturator polynomial
(GAP-F) port once their listed writers are confirmed.
