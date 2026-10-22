# Kontakt 8 Replika core (NI::DSP::FXFDelayLight, object 0x14980): decompile spec

Status: partly portable. The per-sample chain is read end to end, in operation order, with its
constants. Not read: head ring addressing, filter families, effect objects, saturator coefficient
setup, most of setup and rebuild (see Gaps). Method: decompile text and `docs/re/kontakt-rdata/` TSVs
only. No binary, Ghidra, radare2, REA, Wine or oracle. Nothing measured against Kontakt.

Naming: functions by address (`141ab7480`). `C` = core object (wrapper `+0x1c0`). `Ctl(x)` = control
block at `C+x`; bare `Ctl(0x0)` means block `C+0x80`, the one the loop reads. `Ctl(0xc8)` is the block
the setter writes first. Offsets are bytes. Labels: CONFIRMED = read in code, constants decoded.
INFERRED = shape read, name or use guessed. UNKNOWN = not determined.

Corrections to `docs/re/engine/k-replika.md` (not applied there):
- TIME clamp is 4000 ms, not 8000 (`14756bdf8` is f64 4000.0).
- T4 index offset is +1200 (`144fc4b50`). The -60 is the index-18 scale (`144fc4d50`).
- Index 29 is confirmed as the saturation target (glided into the amount `+0x142ac`).
- Core vtable is `1479aaea8` (class `FXFDelayLight@DSP@NI`); `144e962d8` is the wrapper.

## Entry points

| Item | Address | Note |
|---|---|---|
| Core vtable | `1479aaea8` | Ctor `141a7f4d0` (1725 B); dtor `141a83a40`. |
| slot 3 | `141aa1580` | 2336 B, block setup `(rate, count)`: ring, high-rate flag, buffers. Partly read. |
| slot 7 (`+0x38`) | `141aa9480` | 3418 B, per-block process (the loop). Read. |
| slot 8 (`+0x40`) | `141ab4a10` | 858 B, update. Not read. |
| slot 12 (`+0x60`) | `141ac5980` | 3979 B, setter over indices 0..52. Read. |
| per-frame | `141ab7480` | 1537 B, lane banks, saturation glide, sub-effect gates. Read. |
| helpers | `141a9f4d0`, `14199ce60` | Bank phase tick (partly read); LCG noise and smoother on `C+0x9a0` (40 lines read). |
| helpers | `141ee2160`, `141aa44a0`, `140d947a0` | Line copy (372 B), saturator (157 B), two-state section (183 B). Read. |
| helpers | `141abd290`, `141acbfb0`, `141acc600` | Ducking read path (569 B), mode rebuild (898 B, mode-3 branch), bypass fade (404 B). |
| helpers | `141acd110`, `141ab7a90`, `141aa3ec0` | Head write, head read, stereo filter pair. Partly read. |
| T4 builder | `1404d14d0` | 214 B, dB-to-linear table at `14a6b2600`. Read. |

## Object state

### Mode, channels, line

| Off | Meaning | Ctor | Writer |
|---|---|---|---|
| +0x08 | byte: live flag. Setter broadcasts when set; epilogue runs 141a95830 and 141abd290 | 1 | ctor |
| +0x0c, +0x10 | int: channel-B source (0 or 2: buffer 1); base select (0 or 2: buffer 1). +0x10 is also the line copy count | 0 | not traced |
| +0x50 | float*: ring base (channel-B scratch) | - | setup |
| +0x80 | ptr: `Ctl(0x80)`, the block the loop reads | - | not traced (GAP-A) |
| +0x98, +0xb0, +0xc8, +0xe0 | ptr: other blocks. `+0xc8` is written first by the setter; `+0x98` is a gate block, zeroed when live | - | not traced |
| +0x600 | int: 1 or 2 feed the mid (x `+0x918`, `+0x91c`) into heads; 0, 3, 4 feed L/R | 0 | not traced |
| +0x604 | int: path. 0 normal; 0 and 4 run FX stage; 2 extra gain; 3 post filter; 4 tail test | 0 | not traced |
| +0x608 | byte: high-rate line flag (set by setup when rate > 48000) | 0 | setup |
| +0x60c, +0x610, +0x614 | float, float, int: rate in Hz (INFERRED), line block rate = rate/k, block size | 0 | setup |
| +0x618 / +0x718 | line objects: start / end, type 2 (copy) | - | setup |
| +0x818 / +0x820 | ptr: channel buffers 0 and 1, 80 B per frame each | 0 | setup |
| +0x6cc, +0x630, +0x634, +0x638, +0x6d0 | int / u32 / float: frame-count parameters (step 1) | - | not traced (GAP-L) |
| +0x860 | float: mode-3 time normalised, ms/2000 (flag clear) or ms/4000 (flag set) | - | rebuild |
| +0x93c | int: mode 0..3 (3 = time in ms) | 3 | ctor, idx 9 |
| +0x914, +0x915, +0x916 | bool: bypass-fade enables (idx 6, 7); candidate select (idx 26) | 0 | setter |
| +0x918, +0x91c | float: mid scales for paths 1, 2 | 0 | not traced |
| +0x920, +0x928, +0x930 | double: candidates, 1.0 each; +0x928 set by idx 25 | 1.0 | ctor, idx 25 |
| +0x938 | byte: 0 means ducking reads setter values; nonzero means smoothed copies, on smoother events | 0 | not traced |
| +0x940..+0x96c | LFO A: value +0x940, slope +0x944, sign +0x948, smoothed +0x94c, coef +0x950. LFO B: +0x954, +0x958, +0x95c, +0x960, +0x964 | signs 1.0, slopes 0 | ctor, loop |
| +0x968, +0x970, +0x978 | rotator: second output +0x968; state +0x970 (re), +0x978 (im) | 0 | loop |
| +0x980, +0x988 | double: rotator coefficients (cos, sin, INFERRED) from `1443c9bd0` | ctor | ctor |
| +0x998 | double: 1.0 | 1.0 | ctor |
| +0xad0 / +0xad8 | double: ducking position bounds, upper / lower | - | not traced (GAP-B) |

### Heads, filters, effects, saturator

| Off | Meaning |
|---|---|
| +0xac0, +0x6380, +0xbc40 | heads 1, 2, 3: write `141acd110`, read `141ab7a90`, gain `141abba60`, bypass `141abde30` |
| +0x11500, +0x12980 | filters F1, F2 (`141aa3ec0`); family not read (GAP-D) |
| +0x13f80, +0x13e80 | stereo effect objects; vtable +0x60 set param, +0x38 process one frame in place (GAP-E) |
| +0x11600, +0x12a80 | gate objects set by idx 51 |
| +0x14080 (L), +0x1410c (R) | saturators. Offsets from base B: +0x04..+0x10 coefficients c4, c8, c0c, c10 (GAP-F); +0x14 pre-tanh scale; +0x18 correction c18; +0x1c drive d (loop); +0x20 gain g (loop); section A at +0x24 (s0, s1, c2, c3, c4, out5 +0x38, out6 +0x3c, d +0x40, x +0x44); c50 at +0x50; section B at +0x58 (out6 +0x70) |
| +0x14198, +0x141cc | two-state sections for path 3 (out6 at +0x18) |

### Saturation, envelopes, smoothers, gates

| Off | Meaning |
|---|---|
| +0x142a8, +0x142ac, +0x142b0, +0x142b4 | glide coefficient; saturation amount a; target (idx 29); snap threshold |
| +0x142b8, +0x142b9 | byte: pending flags (refresh pow tables, clear) |
| +0x1409c/+0x14128 (drive), +0x140a0/+0x1412c (gain) | copies, L and R |
| +0x142c8, +0x142f8 | double: |tap| follower state, L and R. Coefficients +0x142e0 / +0x142e8 and +0x14310 / +0x14318 (attack, release) |
| +0x14328, +0x14358 | double: output-level follower state, L and R. Coefficients +0x14340 / +0x14348, +0x14370 / +0x14378 |
| +0x41, +0x42, +0x48, +0x1d5, +0x2c0 | byte/u64/int: active flag; hold enable; hold countdown (frames); enable and counter of the every-5-frame update |
| +0x13e00..+0x13e08 | index 19 curve parameters (float, uint, int) |
| +0x13e10 | double: gate target dB = -60 x v (idx 18) |
| +0x13e18 | double: gate threshold on |mid| = max(1 - v, 0.001) (idx 17) |
| +0x13e20 | double: gate retention toward target (writer not traced, GAP-G) |
| +0x13e28 | double: gate release factor per frame (idx 19) |
| +0x13e30 | double: pow base for idx 19 (ctor 0; GAP-G) |
| +0x13e38, +0x13e40 | double: gate envelope in dB (runtime); byte: last gate state |
| +0x14208 + 40k (k = 0..3) | smoother k: +0 coef, +8 current, +0x10 target, +0x18 snap threshold, +0x20 flag, +0x21 second flag. Targets fed from Ctl(0x2c) (k=0), Ctl(0x34) (k=1), Ctl(0x30) (k=2), Ctl(0x38) (k=3) |
| +0x14380..+0x14385 | bypass state; enables from idx 10 (v != 0), 11 (v != 0.5), 12 (v != 0.5), 15 (v != 0); flag blocking idx 9 and setting time factor (k = 0.5 or 1.0) |

### Ctor defaults (CONFIRMED)

The raw store at `C+0x840+4i` (i = 0..52) is zeroed. Set: `+0x8` = 1, `+0x93c` = 3, `+0x920/+0x928/+0x930`
= 1.0, LFO signs = 1.0, `+0x998` = 1.0, `+0x13e00..+0x13e40` = 0, `+0x600/+0x604/+0x608` = 0, `+0x61c` = 2.
Smoothers 1 and 3 targets are 0.5 and snapped. Saturation amount takes the zero target. No writer is
found for LFO slopes (`+0x944`, `+0x958`); they stay 0 (GAP-I).

## Per-sample flow

Block entry:

1. High-rate path. If `+0x608` is set, `count = ceil( ((n - A + 1)*B - C) / (D*B + E) )` with `n` the
   block frames, `A` = int `+0x6cc`, `B` = float `+0x638`, `C` = float `+0x6d0`, `D` = u32 `+0x630`,
   `E` = float `+0x634`. Ceiling taken by sign. CONFIRMED arithmetic; meaning INFERRED (variable-rate
   segment). Then the start line (`141ee2160` on `C+0x618`, copy) fills `+0x818`/`+0x820` for `count`
   frames. Source of that copy not established (GAP-H). If the flag is clear, the caller's buffers
   are used and count = `n`.
2. Buffer pairing. X0, X1 = input buffers (`+0x818`, `+0x820` if flag set, else caller's). Output 0
   is the first output pointer. Base = `+0x50` ring, or output 1 when `+0x10` is 0 or 2. Channel-B
   input is X1 when `+0x0c` is 0 or 2, else X0. CONFIRMED: every access is index-aligned; the loop
   has no fractional or offset read. Delay lives in the heads and the line (GAP-C).

Per frame i = 0 .. count-1:

3. Helper `141ab7480`: lane banks tick (lanes 0 and 1: phase += 1.0 per frame; output = phase x rate
   x scale + offset). Saturation amount glides (L8). If `C+0x115` is set, each nonzero value in the gate
   block (`Ctl(0x98)` at 0x10, 0x14, 0x18, 0x3c..0x5c) calls a sub-effect (e.g. 141abe6a0 on F1 and F2
   with `Ctl(0x80)+0x10`). Every 5 frames, param 0 of `C+0x11600` is set to lane 0 of the bank at `C+0x140`.
4. LFOs. Value += slope x sign; at +1 clamp to +1 and sign -1; at -1 clamp to -1 and sign +1. Output
   smoothed into `+0x94c` (A) or `+0x960` (B). Rotator: (re, im) rotated by (`+0x980`, `+0x988`); im to
   `+0x968`. No consumer in the loop (GAP-I).
5. m = lane0(`C+0x3c0`) x lane0(`C+0x300`) x lane0(`C+0x480`) x lane0(`C+0x540`). CONFIRMED arithmetic.
6. Taps. a = Ctl(0x0) x X0[i]; b = Ctl(0x0) x X1[i]. tapA = a x m x Ctl(0x68); tapB = b x m x Ctl(0x68).
   Followers update on |a| and |b| (meters only; INFERRED).
7. Saturation, path 0. When pending flags are set, refresh d = 10^a and g = 10^(-a/2). If a is nonzero,
   tapA = sat_L(tapA), tapB = sat_R(tapB). Then regardless of a: tapA = (1 - a) x Ctl(0x68) x dryA + a x tapA,
   dryA = Ctl(0x0) x X0[i] (no m); same for B with X1. For all paths, both taps are multiplied by m, so paths
   other than 0 keep m twice (CONFIRMED arithmetic; intent UNKNOWN).
8. Path 2 only: both taps x (2 x Ctl(0x4c) + 1).
9. Smoothers. Each takes its target (see table), snaps when |target - current| <= threshold (clearing the
   flag), else current = (1 - coef) x target + coef x current. If any flag is set and `C+0x938` is nonzero,
   run the ducking update `141abd290` (L15). With `C+0x938` zero, ducking runs only from setter calls.
10. Gate. mid = 0.5 x (tapA + tapB). If |mid| < `C+0x13e18`: env = env x `C+0x13e28`. Else:
    env = `C+0x13e10` + (env - `C+0x13e10`) x `C+0x13e20`. Store `C+0x13e40` = not below threshold. env is dB (L9).
11. Head inputs. Path 1 or 2: (mid x `C+0x918`, mid x `C+0x91c`). Else (tapA, tapB). Write head 1. Read
    head 1 to (L1, R1) and head 3 to (L3, R3). Path 0 or 4: (L3, R3) = 2 x effect(0.5 x (L3, R3)) via `C+0x13f80`.
12. Mix and filter. Mo_L = (L3 + L1) x m; Mo_R = (R3 + R1) x m. F1 in place. Write head 2 with F1 out. Read head 2
    to (L2, R2), x m. F2 in place. Write head 3 with F2 out. FEEDBACK PATH (L4): F2 output enters head 3,
    read at the start of the next frame. No separate feedback coefficient (INFERRED: strength is set by
    head gains, filters, effect).
13. OUT_L = F1_L x Ctl(0x60) + F2_L; OUT_R = F1_R x Ctl(0x60) + F2_R. No setter writes Ctl(0x60) (GAP-J).
14. g = 10^(env/20) from the T4 table (L10), same index for both channels. W_A = OUT_L x Ctl(0xc) x Ctl(0x24) x g;
    W_B = OUT_R x Ctl(0xc) x Ctl(0x28) x g. These replace tapA, tapB.
15. Path 3: each W runs through its two-state section (`+0x14198` for A, `+0x141cc` for B) and is replaced by
    that section's out6 (`+0x141b0`, `+0x141e4`). Then tapA x= m; tapB x= m (all paths).
16. Matrix. dryA = Ctl(0x0) x X0[i], dryB = Ctl(0x0) x X1[i]. mid = 0.5 x (tapA + tapB);
    side2 = (tapA - tapB) x Ctl(0x64). newA = (dryA x m x Ctl(0x8) x Ctl(0x1c) + side2 + mid) x m.
    newB = (mid - side2 + dryB x m x Ctl(0x8) x Ctl(0x20)) x m. Then newA, newB are multiplied by m and
    Ctl(0x4), by 0.5, through effect `C+0x13e80` (one frame, in place), then by 2. Output followers update
    (L13). CONFIRMED arithmetic.
17. After the frames: tail test. If `C+0x42` is zero: if `C+0x48` < count, clear `C+0x41` when path is not 4,
    or the output level is zero or below -100 dB; else `C+0x48` -= count. If the high-rate flag is set, the
    end line (`141ee2160` on `C+0x718`, count `n`) copies the buffers to the caller's output.

## Constants

f32 = constants.tsv `f32`; f64 = `f64`. Values not listed were used only as stated in the flow.

| Address | Value | Used by | Label |
|---|---|---|---|
| 14470bad4, 144714a54, 14470bad0 | 1.0, -1.0, 0.5 (f32) | taps, LFO clamp, setter halves, gate | CONFIRMED |
| 144714988, 144714994, 144714998 | 1.0, 2.0 (f32), 2.0 (f64) | smoother, FX pair, T4 and setter pow base | CONFIRMED |
| 144714a00, 144f38dbc | 10.0, -0.5 (f32) | powf base (drive, output exponent -a/2) | CONFIRMED |
| 144714b90, 144e91200 | abs masks (u32 0x7fffffff, u64 0x7fff...) | |x| | CONFIRMED |
| 14470bae8 | 0.5 (f64) | ctor smoother target | CONFIRMED |
| 1447148b8 | 0.02 (f32) | 20 ms ramp in 141ac5140 / 141ac54b0 (not read) | Lead |
| 144f39e1c, 14756bdf8 | 4000.0 (f32 and f64) | time law and clamp; ducking cap | CONFIRMED |
| 1447149e4, 1447149e8 | 4.0 (f32); 1000.0 (f64) | mode quantiser (int 4v); rebuild scale | CONFIRMED |
| 144f39ff8 | 0.00025 (f64) | ms to normalised (x 2 or x 1) | CONFIRMED |
| 144fc4d50 | -60.0 (f32) | idx 18 target dB | CONFIRMED |
| 1479ab5d4, 14471cb14 | 280.0, 20.0 (f32) | idx 25 slope, offset | CONFIRMED |
| 14769abe8 | 0.001 (f64) | idx 19 curve | CONFIRMED |
| 1479ab4d8, 1479ab4d0 | 79.7262740193227, 76.23945344297027 (f64) | pow-law slope and offset (idx 30, 35, 44) | CONFIRMED |
| 144f38cbc, 144f38910, 1447617a0 | 69.0 (f32); 1/12 (f64); 440.0 (f64) | MIDI offset, semitone exponent, reference | CONFIRMED |
| 144fc44fc, 144fc4b50 | 9.96578407, 1200.0 (f32) | T4 index scale, offset | CONFIRMED |
| 14a6b7100 | 2^20 (runtime) | T4 top entry (index >= 2400) | CONFIRMED (builder) |
| 14a6b2600 | 2^-20 (runtime) | T4 entry 0 | CONFIRMED (builder) |
| 144714938, 144761788 | 1/60, 20.0 (f64) | T4 builder slope, offset; dB factor | CONFIRMED |
| 1447377f0 | -100.0 (f64) | tail threshold, dB | CONFIRMED |
| 144f38790, 1447149c8, 144f38b08 | 1e-5, 10.0, 50.0 (f64) | ducking floor, path-2 floor, extra-gain numerator | CONFIRMED |
| 1479ab4e8, 1479ab508 | 125.0, 250.0 (f64) | ducking offset 125 - 250 x (1 - B) | CONFIRMED |
| 144762eb8, 1447617c8, 14471cb90 | 1/3, -0.5 (f64), -0.0 | ducking spread K; head-1 side scale; sign mask | CONFIRMED |
| 147559b18, 144f385c4 | 48000.0, 1/44100 (f32) | high-rate threshold; decimation k = trunc(rate/44100) | CONFIRMED |
| 144f38820, 1447149d0 | 0.8, 3.0 (f32) | noise amplitude, offset | CONFIRMED |
| 144714b70, 144714b74 | 1.0, 1.0 (f32) | bank lane increments 0, 1 | CONFIRMED |
| 144714b78, 144714b7c | not in TSV | bank lane increments 2, 3 | UNKNOWN (GAP-K) |
| 1447148a4 | 0.001 (f32) | idx 17 floor | CONFIRMED |
| 144f38764, 144f38814 | 0.3, 0.762405 (f32) | reset defaults idx 15, 0/1 | CONFIRMED (reset) |
| 14a6b4590, 14a6b4594 | runtime | idx 12 law term | UNKNOWN |

T4 law (CONFIRMED): entry k (0..2400) = 2^(k/60 - 20), linear interpolation. Index = trunc(9.96578 x dB + 1200).
Index < 0 gives 0; index >= 2400 gives 2^20. Net gain = 10^(dB/20).

Saturator (CONFIRMED arithmetic). d = 10^a, g = 10^(-a/2). Section B takes tanh(c14 x d x) and updates;
f = B.out6; P = c4 f^2 + c8 f^3 + c0c f^4 + c10 f^5; section A takes P and updates;
y = g x ( tanh(d x) + c18 x ( P - c50 x A.out5 - A.out6 ) ).

Two-state section (CONFIRMED arithmetic). d = x - s1. out5 = d c3 + s0 c2; out6 = c3 s0 + d c4 + s1;
s0' = 2 d c3 + 2 s0 c2 - s0; s1' = 2 s0 c3 + s1 + 2 d c4. Coefficients c2, c3, c4 set elsewhere (GAP-F).

## Index-to-field binding

Setter `141ac5980` stores the raw value at `C+0x840+4i`, then switches. "Bcast" = written to Ctl(0x80),
Ctl(0xb0), Ctl(0xc8), Ctl(0xe0), and 0 to Ctl(0x98), when live. Otherwise Ctl(0xc8) only. "Tie" = name
tie; no name appears in the decompile, so ties are at most INFERRED.

| Idx | Setter effect | Loop or helper read | Tie |
|---|---|---|---|
| 0, 1 | conv 141a90910 into Ctl(0xc8)+0x0, +0x4 (no broadcast) | Ctl(0)+0x0, +0x4 gains (GAP-A) | UNKNOWN |
| 2, 3, 5 | raw into Bcast+0x10, +0x14, +0x18 | gates for F1/F2 and 141ac9420 (zero when live) | UNKNOWN |
| 4 | (2(1-v), 1) if v > 0.5, else (1, 2v) into Ctl(0xc8)+0x8, +0xc | Ctl(0)+0x8, +0xc (GAP-A) | UNKNOWN |
| 6, 7 | bool +0x914, +0x915 (v >= 0.5); rebuild 141abd210, 141acc600 | bypass fade only | UNKNOWN |
| 8 | mode 3: ms = clamp(k x v x 4000, 0, 4000) into Ctl(0xc8)+0x2c (k = 0.5 or 1.0). Else table path (141a9d4a0, 9d7c0, 9d660) | Ctl(0)+0x2c; smoother 0 (GAP-A) | INFERRED (TIME) |
| 9 | mode = clamp(int 4v, 0..3) into +0x93c; rebuild 141acbfb0 on change; mode != 3 bypasses heads | head bypass | INFERRED (TYPE) |
| 10, 11 | raw into Ctl(0xc8)+0x30, +0x34; enables +0x14381 (v != 0), +0x14382 (v != 0.5) | Ctl(0)+0x30; smoother 2; Ctl(0)+0x34; smoother 1 | UNKNOWN |
| 12 | head gains via 141abba60; enable +0x14383 (v != 0.5) | head gains | UNKNOWN |
| 13, 14 | (2(1-v), 2v) into Ctl(0xc8)+0x1c/+0x20 and +0x24/+0x28 | dry pan pair; channel gains (GAP-A) | UNKNOWN |
| 15 | raw into Ctl(0xc8)+0x38; enable +0x14384 (v != 0) | Ctl(0)+0x38; smoother 3 | UNKNOWN |
| 16 | raw into Ctl(0xc8)+0x64 | Ctl(0)+0x64: side (width) scale (GAP-A) | INFERRED (WIDTH) |
| 17 | max(1 - v, 0.001) into +0x13e18 | gate threshold | CONFIRMED field; tie UNKNOWN |
| 18 | -60 x v into +0x13e10 | gate target | CONFIRMED field; tie UNKNOWN |
| 19 | +0x13e28 = base^(1/(lerp(+0x13e04, +0x13e08, v) x 0.001 x +0x13e00)) | gate release | CONFIRMED field; tie UNKNOWN |
| 25, 26 | +0x928 = 280 v + 20; flag +0x916 = (v >= 0.5) selects +0x920 or +0x928 for rebuild 141a9dbc0 | not read by loop | UNKNOWN |
| 28, 34, 43 | flags +0x829, +0x82b, +0x82a; calls 141aca670, 141abbf60, 141acb2b0 | head and gate state | UNKNOWN |
| 29 | target into +0x142b0; live: amount +0x142ac = v at once | saturation amount (L8) | CONFIRMED field; tie INFERRED (SATURATION) |
| 30, 35, 44 | pow law P(v) into Ctl(0xc8)+0x3c, +0x40, +0x54 (broadcast) | Ctl(0)+0x3c..0x54 via gates in 141ab7480 | CONFIRMED law; use UNKNOWN |
| 36, 40, 41, 45, 50 | raw into Ctl(0xc8)+0x44, +0x4c, +0x50, +0x58, +0x5c (broadcast) | gates; Ctl(0)+0x4c is path-2 gain | UNKNOWN |
| 39 | sqrt(v) into Ctl(0xc8)+0x48 (broadcast) | gates | CONFIRMED law |
| 31, 48, 49 | bank 1 and bank 2 outputs (lane 0 or 1 pointers) set; live zeroes some | overwritten each frame by bank tick | UNKNOWN |
| 32, 37, 42, 46 | 141ac44a0, 141abc0d0, 141acac40, 141acb240 on the three heads | heads | UNKNOWN |
| 51 | gate param 3 on +0x11600 (1 or 0) and +0x12a80 | sub-effect gates | UNKNOWN |
| 52 | raw into Ctl(0xc8)+0x68 (no broadcast) | Ctl(0)+0x68: tap gain (GAP-A) | UNKNOWN |
| 20-24, 27, 33, 38, 47 | raw only (no case) | none | - |

Read by the loop with no setter writer: Ctl(0)+0x60, `C+0x600`, `C+0x604`, `C+0x938`, frame-count
parameters, `C+0x48` (GAP-J, GAP-L).

Summary: 27 indices write a field the loop or direct helpers read. Of those, 2, 3, 5, 30, 35, 36, 39, 40,
41, 44, 45, 50 reach only through gates that are zero when live. The other 26 do not write a field the
loop reads directly.

## Edge cases

- Live clear: setter writes go to Ctl(0xc8) only; idx 29 queues a pending refresh.
- Mode 3 (default): idx 8 writes Ctl(0xc8)+0x2c. Ctl(0x80)+0x2c is only refreshed by the rebuild (on idx 9
  changes). Ducking with `+0x938` zero reads Ctl(0xc8) directly, so head positions follow the setter.
- Idx 9 clamps to 3, so TYPE 4 folds to mode 3 (INFERRED).
- Saturation amount 0: no saturation call; the crossfade still runs with the dry term only.
- Gate threshold minimum is 0.001. Head-3 side changes sign by sign-bit XOR.
- Path 4 alone compares the output level in the tail test; other paths clear the active flag at countdown end.
- Sample rate above 48000 uses the decimated line (block rate sr/k). Gate dB below about -120.4 gives gain 0.
- Smoother snap with threshold 0 fires only on exact equality.

## Gaps

| ID | What is missing | Address |
|---|---|---|
| GAP-A | Whether Ctl(0x80) aliases Ctl(0xc8). Idx 0, 1, 4, 8, 13, 14, 16, 52 write only Ctl(0xc8) but the loop reads Ctl(0x80). Top priority. | ctor 141a7f4d0, rebuild 141acbfb0, update 141ab4a10 |
| GAP-B | Writers of `C+0xad0`, `C+0xad8`, `+0x918`, `+0x91c`, `+0x938`. | not traced |
| GAP-C | Head ring addressing (write index from `+0x158/+0x15c`, `+0x190/+0x194`; ring length; read glide). | 141acd110, 141ab7a90 |
| GAP-D | Filter families and coefficients of F1, F2 (glide targets `+0x1408`, `+0x141c`). Low cut, high cut not identified. | 141aa3ec0 |
| GAP-E | Effect objects `C+0x13f80`, `C+0x13e80` (vtable +0x38, +0x60). | not read |
| GAP-F | Saturator coefficients c4..c10, c14, c18, c50 and section c2..c4. | not traced |
| GAP-G | Writers of `C+0x13e20` (gate retention) and `C+0x13e30` (pow base). | not traced |
| GAP-H | Start-line call `141ee2160(C+0x618, ...)`: second argument printed as double, callee reads a pointer array. Line types 1, 3-6 (`141ee7670`, `141ee8370`, `141ee7cd0`, `141ee79e0`, `141ee80a0`) not read. | 141ee2160, 141ee5da0 |
| GAP-I | Consumers of LFOs (`+0x94c`, `+0x960`), rotator (`+0x968`), and LFO slope writers (`+0x944`, `+0x958`). | 141ab7480 |
| GAP-J | Writer of Ctl(0x60) (OUT mix gain). | not found |
| GAP-K | Bank lane increments `144714b78`, `144714b7c` (not in constants.tsv). | constants.tsv |
| GAP-L | Writers of `C+0x600`, `C+0x604`, `C+0x630..+0x638`, `C+0x6cc`, `C+0x6d0`, `C+0x48`. | 141ac0000 ~4700-4970, 141aa1580 |
| GAP-M | Ducking body `141abd0b0` (section position and gain) and the rest of `141abd290`. | 141abd0b0 |
| GAP-N | Modes 0..2 rebuild, `141acc600` ramp length (`+0x62d8`), idx 8 table path, idx 0/1 conversion `141a90910`. | 141acbfb0, 141a9d4a0, 141a90910 |
| GAP-O | `14199ce60` beyond its first 40 lines (noise consumer). | 14199ce60 |
| GAP-P | Live epilogue `141a95830`, update `141ab4a10`, slot 1 `141a9e300`, ctor helpers `141a7d480`, `141a7ea50`, `141a7fb90`. | as named |
| GAP-Q | Values returned by `1443c9bd0` (rotator coefficients). | 1443c9bd0 |

## Confidence

Laws, one label each (35 lines). CONFIRMED 22, INFERRED 8, UNKNOWN 5.

- L1 Frame-count ceiling (step 1): CONFIRMED.
- L2 High-rate flag; decimation k = trunc(rate/44100): CONFIRMED.
- L3 Index-aligned tap addressing (step 2): CONFIRMED.
- L4 Feedback: F2 output into head 3, read next frame (step 12): CONFIRMED.
- L5 Tap followers: INFERRED (meters only).
- L6 Modulation product m (step 5): CONFIRMED.
- L7 Saturator law and gains (step 7): CONFIRMED.
- L8 Saturation glide (live at once; else coef +0x142a8, snap +0x142b4): CONFIRMED.
- L9 Gate envelope in dB; attack coefficient value UNKNOWN (GAP-G): CONFIRMED arithmetic.
- L10 T4 dB-to-linear table: CONFIRMED.
- L11 Two-state section and saturator chain: CONFIRMED.
- L12 Matrix: side scale (idx 16), dry pan pair (idx 4 x 13/14): CONFIRMED arithmetic.
- L13 Output followers and tail test (-100 dB): CONFIRMED arithmetic.
- L14 Smoother form: INFERRED (thresholds and coefficients UNKNOWN).
- L15 Ducking read path (constants decoded); head roles INFERRED: CONFIRMED arithmetic.
- L16 Time law, mode 3: ms = clamp(k v 4000, 0, 4000), k = 0.5 or 1; rebuild gives ms/2000 or ms/4000: CONFIRMED.
- L17 Pow law idx 30/35/44: 440 x 2^((79.726 v - 145.239)/12): CONFIRMED formula; use INFERRED.
- L18 Mode quantiser int(4v) clamped 0..3, default 3: CONFIRMED.
- L19 Enable flags (idx 10, 11, 12, 15) and bypass target (141acc600): CONFIRMED.
- L20 Live broadcast and gate zeroing: CONFIRMED.
- L21 Smoother targets from Ctl(0x2c, 0x30, 0x34, 0x38): CONFIRMED.
- L22 LCG noise form (uniform +/-0.8): INFERRED (consumer unknown).
- L23 Triangle LFO clamp and sign reflection: INFERRED (consumers unknown).
- L24 Rotator form: INFERRED (consumers unknown).
- L25 Lane banks, lanes 0 and 1: INFERRED (lanes 2, 3 unknown).
- L26 Every-5-frame update: INFERRED (purpose unknown).
- L27 Setter broadcast versus Ctl(0xc8)-only: CONFIRMED.
- L28 Hold countdown and tail flag: INFERRED (writers UNKNOWN).
- L29 Bypass fade target (141acc600): CONFIRMED.
- L30 Index 29 to saturation amount: CONFIRMED (field); name tie INFERRED.
- L31 Head roles and the second head parameter (-0.5C, C, -C): UNKNOWN.
- L32 Head ring addressing and read offsets: UNKNOWN (GAP-C).
- L33 Filter families F1, F2 (low cut, high cut): UNKNOWN (GAP-D).
- L34 Ping-pong, stereo, pan, LR offset flags: UNKNOWN (no consumer found).
- L35 Tape age, flutter, quality, BBD type, noise amount: UNKNOWN (no consumer found).

Overall: the per-sample chain (steps 3 to 17) is complete in operation order and constants. It is not
portable until GAP-A, GAP-C, GAP-D, GAP-E, GAP-F and GAP-H are read.
