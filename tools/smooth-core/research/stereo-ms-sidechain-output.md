# Stereo, sidechain, solo and output: findings (soothe3)

Static analysis of `/tmp/opencode/rea/soothe3-restored.bin` (PE, base `0x180000000`), plus
isolated emulation under Unicorn (`re-tools/emulate/stereo_pair_probe.py`). Addresses are
VA; RVA = VA − `0x180000000`. The corpus pseudocode is `RunScratch/smooth/re-corpus`.

Status: **verified** = read from the image and checked by emulation; **read** = the form is
read, one input is inferred; **open** = not settled.

## 1. Mid/side (`+0xa6`): the encode and decode gains are 0.5 and 1.0 — verified

Earlier notes said the sum and difference in the helper were dead stores and that no
`1/sqrt(2)` appeared. Both were wrong. The writes do reach the output, because the IPP
wrappers take `(src, dst, n)` for copy (`0x10a0`), not `(dst, src, n)`. Found by emulation.

**Helper `0x8a3b20`** (`re-corpus/index/annotated`, `callers: 7`). Arguments:
`arg1` = destination channel-pointer table (`+0x84` = frame length N), `arg2` = source pair
table, `arg3` = list of groups (stride `0x18`: `+8` = channel index, `+0x10` = mode),
`arg4` = scratch arena (`+0` = base, `+0x18` = cursor), 5th arg = gain flag.

```c
// per group, channels (ch, ch+1) of the source pair; mode 0 = stereo pair
A = src[ch] + src[ch+1];          // 0x1800015b0 (IPP add)
B = src[ch+1] - src[ch];          // 0x1800011c0 (IPP sub, args reversed)  -> B = L - R
if (flag == 0) { dst[ch] = A;  dst[ch+1] = B; }        // 0x1800010a0 = copy(src, dst, n)
else           { dst[ch] = 0.5*A; dst[ch+1] = 0.5*B; } // 0x180001070 = mulC(src, xmm1=0.5, dst, n)
// mode 1 (single channel): dst[ch] = src[ch]           // copy
```

Emulated (`stereo_pair_probe.py`, N = 32, random input): `dst_L = g(L+R)` and
`dst_R = g(L−R)` with error below `1e-7`, for both flags. Mode 1 copies one channel.

**Call sites** (the 5th argument is `byte [rsp+0x20]` set just before the call):

| VA | containing pdata fn (corpus) | flag | gain | role |
|---|---|---|---|---|
| `0x1808928a3` | `0x180890c92` (process, M/S branch) | 1 | 0.5 | encode, analysis input |
| `0x18089a5bc` | `0x18089a4f0` wrapper, called from `0x8b1630`, `0x8b3be0`, `0x8c7210`, `0x8c97c0` | 0 | 1.0 | decode |

Encode and decode invert each other, so `M = (L+R)/2`, `S = (L−R)/2`, `L = M+S`, `R = M−S`.
The round trip is exact. No `1/sqrt(2)` constant is used on this path.

**Read, not verified:** the decode site is applied to the wet (processed) M/S pair before
the output. It is the inverse of the encode, and the encode feeds the analysis path.

**Port change (this branch):** `MS_ENCODE_GAIN = 0.5` on the analysis input, decode is
unity on the wet path. Test `mid_side_encodes_at_half_and_round_trips_at_unity`.
The detector therefore reads M at `(L+R)/2`, which is 3 dB below the old value.

**Link and focus in M/S:** focus weights are `2(1−f)` and `2f`, clamped to `[0,1]`
(`0x180890c92`, the `*0x18209d4a0 = 2` multiply). The port's `focus_weights` matches.
Link (`stereo_pair`) applies per bin before M/S is taken and is not M/S-specific. The
reference's order was not checked.

## 2. Sidechain (`+0xa5`): mux, listen and meter — verified (emulated)

Evidence: `re-tools/emulate/sidechain_fixture.py`, `src/reference/sidechain.rs`.

- **Mux (`0x890bee`, verified in the asm).** `r11 = side pointer`. Non-null: the analysis frame is
  staged from the side table (`0x894b00` into `rbp+0x29d0`). Null: staged from the main table
  (`rbp+0x2530`). Both are copied to `rbp+0xe00`. The block never reads `+0xa5` (no displacement
  `0xa5` in `0x890a00..0x893bd7`). Emulated: sidechain flag 0 and 1 give bit-identical outputs and
  meters. The earlier "reads of `+0xa5`" in `re-corpus-findings.md` were the setter.
- **Listen (`+0xa7`, verified emulated).** Read into `[rsp+0x33]` at `0x890a7d`, tested at `0x892e43`.
  With listen on and a side pair the output is `side[k - 2304] * g_out[k]`: the side delayed by the
  block latency and multiplied by the out-trim ramp, stepped on the absolute sample index (error
  2e-6 in the Python model; the Rust check is 1e-6 relative). Mix, wet and dry gains do not apply.
  With no side pointer the output is zero. Bypass with listen: the output is the delayed main input
  (ratio 1.000000 for `k >= 2304`), so bypass wins; the first 2304 samples in that case are not
  analysed.
- **Meter (host id 13, `0x893928`, verified emulated).** One value per 512-sample sub-block, so
  `ceil(N/512)` values. Value = max over channels of `|side|` over the sub-block (`0x1800013d0`
  is `ippsMaxAbs_32f`). **Corrects the earlier "signed max"**: the emulated block returns the
  absolute peak (a negative sample of -0.68 gives 0.68). No side pointer or a silent side: 0.
  Independent of the main input. Unsmoothed, linear.
- **Listen gain targets.** The out-trim record is the same as the output stage (`0xbf58`). The
  ramp runs over the whole block, so at the first output sample the gain has already moved (0.6018
  at `k = 0` for -6 dB, not 1).

## 3. Band solo (param 91): the shape-to-filter table is verified

`0x89a1a0` (per-sample solo filter) and `0x89bf10` (per-band coefficient store). The
shape index is `*(band+0x1c)`, in the port's `Shape` order: 0 LowCut, 1 LowShelf, 2 Bell,
3 Bandpass, 4 BandReject, 5 Tilt, 6 HighShelf, 7 HighCut.

| shapes (index) | `0x89bf10` mode | k store (`+0x18` slot) | port before | port after |
|---|---|---|---|---|
| {2, 3} Bell, Bandpass | 2 band-pass | `max(0, band+0xc)` | mode 2, k = 1/Q(width) | unchanged |
| {1, 7} LowShelf, HighCut | 0 low-pass | `1.0` | mode 0, k = 1/Q | mode 0, k = 1 |
| {0, 6} LowCut, HighShelf | 1 high-pass | `1.0` | mode 1, k = 1/Q | mode 1, k = 1 |
| {4} BandReject | 3 notch | `max(0, band+0xc)` | mode 3, k = 1/Q | unchanged |
| {5} Tilt | none (copy) | — | mode 2 (filtered) | **main signal passed through** |

- **Tilt (fixed).** `0x89a1a0` takes the copy branch for shape 5. The copy wrapper is
  `copy(src = rcx, dst = rdx)`, and the call is `copy(arg3[k], arg2[k], N)`. Here `arg3` is
  the input and `arg2` the solo output. So the solo stream is the unfiltered main signal.
  The old note calling this "inverted" came from the same argument-order mistake.
- **k for LP/HP (fixed).** The `0x89bf10` store sets the coefficient to the double `1.0`
  (`0x3ff0000000000000`) for modes 0 and 1. The port used `1/Q` there.
- **Open.** `0x89a1a0` does not write the filter state directly. It smooths each coefficient
  toward its target with one-pole rate `*(record+0x38)` (`s += (target − s)·g`). The port
  sets coefficients at once when the band changes. A solo switch can therefore click
  where the reference fades. The rate and its source were not decoded.
- **Open.** What `band+0xc` is. The port derives k for modes 2 and 3 from the stored width.
  The reference uses `band+0xc`, which may already be 1/Q, but this was not confirmed.

## 4. Output stage: formulas and the 30 ms ramp — verified, unchanged

Already in `dsp-remaining.md` items 4 and 1 (the ramp form, the three gain records, bypass
and delta targets). Checked against the code this round, with no differences:

- Ramp step `g += (t − g)·a` (`0x899ab0`). Coefficient `a = 1 − exp(−1/(τ·fs))`, τ = 30 ms
  (the constant `−33.3333 = −1/0.03` at `0x18209da5c`).
- Targets: normal `dry = 1−mix`, `wet = wet_gain·mix`, `out = out_gain`. Bypass
  `dry = 1`, `wet = 0`, `out = 1`. Delta `dry = wet_gain·mix`, `wet = −wet_gain·mix`, so the
  output is `wet_gain·mix·(dry−wet)`.
- Open: `fs` as the exponent's X is inferred, from the constructor `0x88fa90` whose caller
  (`0x88f430`) is flattened.

## 5. Channel handling (23 layouts, 143 parameters)

- The helper's group model is a stereo pair (mode 0, consumes `ch` and `ch+1`) or one channel
  (mode 1). M/S needs a pair, so it is gated on two active lanes. The port does this
  (`active_channels == 2`), and mono bypasses M/S.
- The mode branch has no channel-count test in the reference (earlier notes). Whether a
  mono group is built is decided upstream, and that is not decoded.
- Layouts above stereo: not decoded. The port's engine has one or two lanes. Not implemented.

## Corrections to earlier notes

- `dsp-remaining.md` section 2 and `re-corpus-findings.md` section 2 said the sum and
  difference were dead stores and that no `1/sqrt(2)` could be found. Both are superseded by
  section 1 above.
- `dsp-remaining.md` section 1 said the shape-5 copy looked inverted. It is not (section 3).

## Tools

- `re-tools/emulate/stereo_pair_probe.py`: emulates `0x8a3b20` and checks the formulas.
  Run with `python3 -I` from the repo root.
- Pitfall: the IPP wrappers at `0x10a0`, `0x1070`, `0x11c0`, `0x15b0` dispatch through
  `[0x1821ed508]`, which is set by `0x181dae2a0`. Call that first in the emulator.
- Pitfall: the `0x1070` scalar is passed in `xmm1`, not `rdx`. Pass it by a register write.

## 8. Setter case 0x5b (band solo) and layouts — verified (emulated)

- **Case `0x5b` (`0x894545`).** `r9 = roundf(clamp(v, 0, 8))` (libm at `0x1817bb92a`, then
  `cvttss2si`), `eax = min(r9 - 1, 7)` with a **signed** compare (`cmovg`). So value 0 gives -1
  (no solo), 0.4 gives -1, 0.5 gives 0, 8 gives 7, 9 gives 7. **The decompile's "value 0 -> index 7"
  is wrong**: the asm uses a signed compare, and the parameter table's `uint(idx-1,max7)` wording
  is not what the code does. Table: `solo_band_index` in `src/reference/sidechain.rs`.
- **Band fields.** The band record is `obj + 0x10c + 0x20*band`, with the generic handler `0x89466e`
  (`band = (id - 0x13) / 9`, field = remainder). Remainder 6 (`+0x120`) is the **band balance**
  (`Band k focus`, id `0x19 + 9*(k-1)`). Port `focus` is that balance (`plugin/src/lib.rs:840`).
- **Focus snippet (`0x892b34..0x892bf7`, verified).** `f = [r9 + r15 + 0x120]`, applied when the group
  has two channels (`[rdi+0x10] == 2`). `gL = clamp((1-f)*2)`, `gR = clamp(f*2)`, each applied with
  `ippsMulC_32f_I` (`0x180001220`) to the group's channel pair (`[rsp+0x40+8*ch]`, `+8`). Bit-exact
  on the isolated snippet for 17 balances, including clamps. A zero gain writes `+0.0`.
  **Not verified:** which band `r9` selects outside solo (`[rbp-8]` is set only on the solo path, so
  the non-solo value is open), and which buffers the group loop feeds (the analysis frame or the
  output). The group list is empty in the stand-in (see below), so the loop does not run.
- **Harness blocker.** `[obj+0x18]` (the group list) is empty after construction. The layout builder
  `0x88acb0` returns a function-local static whose guard does not initialise under emulation. So the
  per-group loop and the analysis (`0x619fc0`, per the notes) never run in the process harness.
  Fixing this unblocks the focus path end to end, and the link and M/S order.
- **Layouts (`0x56cad0`, verified).** Index to channel count, 23 entries:
  `0,1,2,3,4,4,5,6,6,7,7,7,8,8,8,10,9,10,12,14,16,14,16` (index 2 is stereo). The group tables for
  these layouts (`0x88acb0`, 23 static switch cases of `(kind, count)` pairs) are not decoded.
