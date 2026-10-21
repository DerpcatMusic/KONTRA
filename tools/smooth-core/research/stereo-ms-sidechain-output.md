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

## 2. Sidechain (`+0xa5`): mux and listen — partly decoded

- Setters `0x89445b`, `0x89467e` write `+0xa5` (sidechain) and `+0xa6` (stereo mode).
- `0x890a00` does **not** read `+0xa5`. The corpus reader list is wrong for that function.
  The earlier notes say the detector source depends on whether the 4th argument is null
  (`0x890bee`). That is still the best evidence and is not decoded further.
- Not changed: the port analyses the sidechain input when `sidechain` is on and the main
  input otherwise. This is the reading the brief describes, and the port keeps it.
- Sidechain listen routing: open (see `engine-io.md`).
- Meter slot 13: the largest signed detector sample since the last frame (earlier
  verification, unchanged).

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
- **Smoothing (decoded, `output-stage-translation.md` "Solo path").** Per sample, R and k move
  toward their targets with one-pole rate `+0x38` while the rate is below 1, else snap. The rate
  is `1 - exp(-1/(0.03 fs))`, written by the constructor. The port's instant coefficient set
  does not match this, so solo switches differ from the reference until the port adopts it.
- **Resolved: `band+0xc` is the half bandwidth in octaves**, not 1/Q. The filter uses it as
  `hb = max(k, 0)/2` in the bandwidth law (`0x88e150`), and the setter fills it with
  `log2(e) asinh(1/(2Q))` (`params.rs`). So the reference's k is `(hi - lo)/g` from that width.
- **Correction: the solo shape is `+0x1c`, not `+0x18`.** The setter's shape index 26 writes
  `+0x124`. The filter reads `+0x128`, which index 27 (`selector`) writes. See the solo section.

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

## Round 2: the helper is bit-exact; where it applies

- **Bit-exact fixture.** `research/re-tools/emulate/mid_side_fixture.py` writes
  `src/reference/fixtures/mid_side.json` (52 cases: 24 encode, 24 decode, 4 copy). `mid_side_pair`
  reproduces every destination sample bit for bit (test `mid_side_helper_matches_the_emulated_helper_bit_for_bit`).
- **Encode site `0x8928a3` (flag 1)** is reached only when, in order: `+0xa6` (stereo M/S) is set
  (`0x892781`), the bypass byte `+0xa9` is clear (`0x892796`), and `[rbp+0x130]` is not negative
  (`0x8927a6`, the group index used as `r9 = idx << 5`). The shape the same block reads for the
  group is `+0x124` (`0x8927ba`), the setter's shape index 26, not `+0x128`. So the solo filter
  is the one place that reads the selector field.
- **Decode site `0x892d0c` (flag 0)** is in the output section, after the solo path, with its own
  gates (not decoded). A second decode call is at `0x89a5bc` (wrapper `0x89a4f0`, callers listed
  in the notes above).
- **Not verified:** which buffers the encode and decode act on. With M/S on and a band used, the
  trace (`0x8a3b20` entry hook) saw no helper call at all, so neither site was reached. The value
  of `[rbp+0x130]` was not read; the gate has to be set (or its writer found) before the stereo
  output can be checked.

## Tools

- `re-tools/emulate/stereo_pair_probe.py`: emulates `0x8a3b20` and checks the formulas.
  Run with `python3 -I` from the repo root.
- Pitfall: the IPP wrappers at `0x10a0`, `0x1070`, `0x11c0`, `0x15b0` dispatch through
  `[0x1821ed508]`, which is set by `0x181dae2a0`. Call that first in the emulator.
- Pitfall: the `0x1070` scalar is passed in `xmm1`, not `rdx`. Pass it by a register write.
