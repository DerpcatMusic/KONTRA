# Kontakt 8 Raum Cosmic (type 2): closing the open laws

Date: 2026-10-22. Method: decompile export only (`0141980000.c`, `0141970000.c`,
`0141ad0000.c`, `0141960000.c`), PE constant cache (`constants.tsv`), vtable cache
(`vtables.tsv.gz`, `vtable_xrefs.tsv`). No Kontakt run, no native vector. Cosmic
has no native vector, so every law below is pinned by decompile code and constants
only. Addresses are hex. `Raum+0xNN` = byte offset in the Raum object.
Labels: CONFIRMED (read in code, constant decoded), INFERRED, UNKNOWN.

## 0. Corrections to the earlier notes

1. **141966ad0 is not the b80 feedback loop.** It sits in the vtable of class
   `Freak` (vtable 147699d88, slot 19, the only vtable that references it). Its
   f64 fields and its d38/d3c guards belong to Freak. CONFIRMED.
2. **b80 (Raum+0xb80) is `NI::DSP::Limiter`.** The Raum ctor 14197f620 calls the
   Limiter ctor 141adc050 on `param_1[0x170]` (= +0xb80). CONFIRMED.
3. **Cosmic's odd polynomial lives in the Cosmic core 14198c980** (Raum+0xe10
   base). The copy in the outer 1419878b0 (lines 5669-5730) is a Galois phasor
   branch gated by +0x2424, not Cosmic. CONFIRMED.
4. **The post loop multiplies the buffer by the transition gain g again.** So the
   Cosmic output into the post loop is g^4 C, not g^3 C. Grounded/Airy get g^2
   (not g^1). CONFIRMED in code; linearity of the kernels INFERRED.
5. **dd4/dd8 (Raum+0xdd4/0xdd8) are predelay feedback samples**, not a b80 path.

## 1. Predelay feedback

Verdict: structure and gains CONFIRMED. The one open point is which setter
parameter sits at +0x8c (name INFERRED).

- Predelay rings: L ring pointer +0xae0, write index +0xad8, mask +0xb00. R ring
  pointer +0xb10, write index +0xb08, mask +0xb30. Core loop 141981200
  (0141980000.c 558-800). CONFIRMED.
- Write, per sample, before the read (0141980000.c ~689-693):
  `bufL[w] = de4 * xL + dd4`, `bufR[w] = de4 * xR + dd8`.
  dd4/dd8 are the values stored at the end of the previous sample. CONFIRMED.
- de4 = (Raum+0xc0)^2, written by the lambda body 14197fe60
  (0141970000.c ~13144). CONFIRMED. (Role "input gain before predelay": INFERRED.)
- Read: after the index advances, `tapOld = buf[w - int(ac8)]`,
  `tapNew = buf[w - int(acc)]`, and output `y = (1-p)*tapNew + p*tapOld`,
  p = +0xb38. CONFIRMED structure.
- Fade: p (+0xb38) starts at 1 and steps by -1/F per sample (+0xb40). The fade
  ends when |p - p_reset| <= 0.001 (+0xb44) or when the counter (+0xb4c = N) runs out.
  p_reset = +0xb3c = 0 (set in the set-up, and p is reset to it at the end).
  F = clamp(500*N/sr, 2, 100) * sr / 1000 samples, that is F = clamp(N/2, 2 ms, 100 ms).
  Constants: 500 at 144f38d04, 2 at 144714998, 100 at 144737790, 0.001 at
  1447148a4. Fade set-up is 141983000 (0141980000.c 1842). CONFIRMED arithmetic;
  "ms" unit INFERRED.
- ac8 (+0xac8) becomes acc (+0xacc) when the fade completes. CONFIRMED.
- Target N: `N = round(max(2, sr * s))`, s = predelay seconds, from 141982110
  (0141980000.c 1205). sr = param block [0]: INFERRED.
- Predelay field: case 1 of the Raum setter 141982830 stores +0xde8 = f(v) via
  141982050: for v <= 0.5, `f = 0.08*(2v)^2`; for v > 0.5, `f = 1.92*(2v-1)^2 + 0.08`.
  Constants 14769c650 = 0.08, 14769c658 = 1.92. CONFIRMED. With the sync flag off
  (+0xdec, case 2), s = f = +0xde8. With it on, s = min(240 n / (d(df8) * m * bpm), 20)
  with n = +0xdf0 (case 3), m = +0xdf4 from a 5-entry table at 14769c378 (case 4),
  d = 1419a0200 of +0xdf8 (case 5), bpm = +0xe00 (f64). Constants 240 at 144f38b48,
  20 at 14471cb14. CONFIRMED formula from 141982f00; the division table is UNKNOWN.
- Feedback amount dd0 = `sqrt(clamp(F, 0, 1))`, F = Raum+0x8c. Written by the
  lambda body 14197fd50 (0141970000.c 13044), which setParameterHandlers
  (141982c70, 1676) installs at +0x248. CONFIRMED law. The identity of the
  parameter that writes +0x8c: INFERRED (feedback). The write path from the
  notify is UNKNOWN.
- Feedback gate 9e8: written in 141982f00 (0141980000.c 1794-1841). 9e8 = 0 when
  s < 1e-4 (constant 144f385e0), else 1. Calls from 141982830 cases 1-5. CONFIRMED.
- Smoothed gate 9e4 follows 9e8 with coefficient 9e0 = exp(-ln 20 / (0.05 * p2)).
  -ln 20 at 147698b10 (f64 -2.99573), 0.05 at 1447148c0. p2 is the prepare argument,
  INFERRED = sr. Snap to target when |9e8 - 9e4| <= 0.001 (0141980000.c 653-657). CONFIRMED.
- Feedback path, per sample, L lane (0141980000.c 779-788):
  `fbL = 0.707 * 9e4 * dd0 * LIM( (1/0.707) * 0.5 * S_L )`,
  S_L = sum of the two states of the L-lane network (+0xd54..+0xd60). The R lane
  is the same with the R-lane network (+0xd78..+0xd84), fbR = dd8.
  Constants: 0.707 at 14769c648; 1/0.707 = 1.41442716 at 144fcdc14; 0.5 at 14470bad0.
  The product of the two scalings is 0.5000, so the loop gain is 0.5 * dd0 * 9e4 * LIM.
  CONFIRMED arithmetic; lane-network topology INFERRED.
- The limiter LIM is the Limiter (section 2). Its gain table is not in this note.

**Port consequence:** Cosmic needs a one-sample feedback delay on each channel
into the predelay write. Feedback = 0.5 * sqrt(clamp(F,0,1)) * gate * LIM(...),
with the gate from `predelay >= 1e-4` and a 50 ms smoothing of the gate. The
predelay read needs the linear fade with F = clamp(N/2, 2 ms, 100 ms). Do not
rely on the findings' "+0xdd4/0xdd8 is b80" line.

## 2. b80 identity and feedback

Verdict: **b80 is the NI::DSP::Limiter.** The candidate 141966ad0 is rejected.

- Raum ctor 14197f620 (0141970000.c 12694-12806) calls ctor 141adc050
  (0141ad0000.c, 110 B) on `param_1[0x170]` = Raum+0xb80. The Limiter vtable is
  14769c220. CONFIRMED.
- Limiter vtable slots (from vtables.tsv.gz):
  - 0x18 -> 141ae28c0 (208 B), prepare with (sr, block). Called from 141980ac0 (prepare).
  - 0x20 -> 141ae1d10 (30 B). Called from 141980a70.
  - 0x28 -> 141ae2a50 (30 B). Called from 141980f40.
  - 0x30 -> 1404f99a0 (3 B, stub). Called from 141980930.
  - 0x38 -> **141ae3ba0 (842 B), process, in place on 2 lanes, count.**
    Called from 141981200 at 0141980000.c 779-780. The call is unconditional in the
    per-sample loop, so every algorithm (Grounded, Airy, Cosmic) passes it. CONFIRMED.
  - 0x60 -> 141aea740 (453 B), parameter setter with (id, value). Called from
    141982f00 with (1, max(v, 1/12)). The 1/12 floor is at 144737724. CONFIRMED value;
    the meaning of parameter 1 is UNKNOWN.
- 141966ad0 (0141960000.c 6088, 5251 B): Freak vtable slot 19 only. It has no direct
  call site in the export and no vtable at slot 0x38 references it. CONFIRMED rejection.
  Caveat: vtables.tsv covers RTTI classes only.
- Odd polynomial (Cosmic LFO shaping), coefficients from constants.tsv:
  - C1 = 0.7594793 (1476a1730, u32 3f426d3d)
  - C2 = 0.1265799 (1476a1700, u32 3e019e29)
  - C3 = 0.0569610 (1476a16e0, u32 3d694fe3)
  - C4 = 0.0339053 (1476a16d0, u32 3d0ae050)
  - C5 = 0.0230745 (1476a16b0, u32 3cbd06a6)
  - P(x) = C1 x + C2 x^3 + C3 x^5 + C4 x^7 + C5 x^9. CONFIRMED.
  - Used in 14198c980 (0141980000.c 8641-8762): the stage delay ratio is
    B + P(U) or B - P(U) per stage (with U = Re * A, A = depth). CONFIRMED.
  - Used again in 1419878b0 lines 5725-5728 on the Galois phasors (+0x2380.., gated
    by +0x2424). Not Cosmic. CONFIRMED.
- Cosmic takes the Limiter branch: yes. The Limiter call is not gated on
  algorithm. CONFIRMED.

**Port consequence:** Port the Limiter (141ae3ba0) as the feedback limiter, not a
generic clamp. The gap-table entry "+-2.5 clamp" is only a stand-in. Its gain table
is still needed and is not in this note.

## 3. Post-filter laws (shared post loop)

Verdict: the filter types and coefficient laws are **the same for Cosmic and
Grounded/Airy**. The post loop (141981200 from the label after the kernel call,
0141980000.c 800-895) runs for every algorithm; Cosmic only skips the kernel call.

Per-sample order, CONFIRMED:
1. buf = buf * g (g ring at +0x9c8). Extra factor, see section 4.
2. Stage A (per channel, L state +0xd00..+0xd0c, R state +0xd30..+0xd3c): a
   TPT state-variable low shelf. Output `x + (A-1) * (s+LP)/2 + 2 q (sqrt(A)-1) * v`,
   where v is the TPT band-pass term and s the old integrator state.
   - A = 10^(-24 x / 20) with x = smoothed control a00[0]. Constants: -24 at
     144f38e18, 1/(20 log10 2) = 0.166096 at 14769b2c8 (the code uses 2^n times a
     degree-2 poly in the fraction; 0.70588 = 12/17 at 1476988d0, 0.23529 = 4/17 at
     147698840). CONFIRMED.
   - Corner f = (200 + 200 max(0, x - 0.5)) / A^(1/4), constants 200 at 144f38ce4 and
     1/A^(1/4) from sqrt(sqrt(A)). CONFIRMED arithmetic.
   - g = tan-polynomial: g = y + y^3/3 + 2 y^5/15 with y = f * pre. Constants 1/3 at
     144760108 and 2/15 at 144fcfce0. The prewarp `pre` is at word +0x10 of the block
     (INFERRED pi/sr). CONFIRMED polynomial.
   - h = 1/(1 + 2 q g + g^2), q = block word +0x18 (preset, writer UNKNOWN).
     CONFIRMED structure.
   - Control a00[0] = low shelf, INFERRED from the parameter order and the shape.
3. Stage B: the same for R (state +0xd30..).
4. Stage C (L) and stage D (R): Butterworth low-pass, 2nd order, R = sqrt(2)
   (constant 144fcfd80 = 1.41421354). Cutoff f = 1000 + 23324.1 x^2 Hz for x <= 0.95,
   else 22050 + 1764000 (x - 0.95) Hz (constants 144714a40 = 1000, 144fc4c14 = 23324.1,
   144737744 = 0.95, 144fc4c0c = 22050, 144fc4c34 = 1764000). x = a00[1], INFERRED
   "high cut". Same tan-polynomial and h law. CONFIRMED arithmetic.
5. Mix: out = dry * de0 + wet * ddc (section 4). CONFIRMED.

Differences from Grounded/Airy: none in the code. The filters are called from the
shared loop with `a18` dirty flags only. The only algorithm difference is the core
signal that enters the post loop (section 5).

**Port consequence:** the Rust Cosmic path can reuse the shared post loop unchanged.
Port the tan-polynomial (not exact tan) and the 2^n poly shelf gain if bit-level
fidelity matters. Preset `q` (block +0x18) and the pre-warp word need a writer
before this is complete.

## 4. Wet and dry law, and the g factor

Verdict: **out = dry * de0 + wet * ddc is CONFIRMED.** The g verification is CONFIRMED
in code; steady state g = 1 is CONFIRMED for the normal path.

- de0 and ddc come from 141982250 (0141980000.c 1275-1317), reading a00[2] = mix, clamped to [0,1]:
  - mix <= 0.5: dry = 1, wet = (2 mix)^(3/2).
  - mix = 0.5: dry = 1, wet = 1.
  - mix > 0.5: dry = 2 - 2 mix, wet = 1.
  Constants 2.0 at 144714994, 0.5 at 14470bad0. CONFIRMED.
- dry = copy of the input made by memmove at the start of 141981200
  (into +0x980 and +0x998). wet = post-loop output. CONFIRMED.
- g = transition gain at +0x9b4, smoothed to target +0x9b8 with coefficient +0x9b0,
  snapped within +0x9bc = 0.001. Target 9b8 = 1.0 at prepare (141980ac0, 0141980000.c 253)
  and in 141980f40 (498-499, forced to 1.0 every block). Target set to 0 at 141980f40
  520/539 (conditions not traced), 1419830e0 (1906) and 141983140 (1957). **Steady state
  g = 1.0** (the note is right for the normal path). CONFIRMED.
- Cosmic store: chain input = g * pred; the chain output is multiplied by g, then
  stored with another g, so the stored value is g^3 C(pred). CONFIRMED.
- Post loop: multiplies that buffer by g again, so the post input is g^4 C for Cosmic.
  The note's g^3 is therefore one factor short (section 0.4).
- Grounded/Airy: stored = g * pred; the kernel output is assumed linear, so kernel
  = g * K(pred), and the post loop gives g^2 K(pred). Linearity INFERRED.
- Chain linearity: the stage equations in the findings (Z y_prev + g0 x) are linear.
  INFERRED from the findings, not re-read here.
- Repo port: `design/raum.rs` ports g (`Transition`: one-pole `a = exp(-ln 20/(0.05 sr))`,
  snap at 0.001, g^4 Cosmic, g^2 Grounded/Airy; steady g = 1 is bit-identical).
- Open trigger: the `9b8 = 0` writes (141983140, 1419830e0, 141980700) take the fade path
  only while `0x5c8 == 0`; 141980f40 (520/539) is dead because it sets `0x5c8 = 1` first.
  Who leaves `0x5c8` at 0 during playback is unread (`set_transition_target(0)` is the input).

**Port consequence:** at g = 1 the factors vanish. For fade-in and fade-out
transitions, the Rust port must apply g at the same four points (chain input,
chain output, store, post input) or the transition shape will differ.

## 5. Cosmic vs Grounded/Airy: shared and different

| Item | Cosmic (9f4 = 2) | Grounded (0) / Airy (1) | Shared |
|---|---|---|---|
| Predelay write, read, fade (section 1) | yes | yes | shared |
| Feedback gain dd0, gate 9e8/9e4, limiter (section 2) | yes | yes | shared |
| Core | 14198c980 (chain, flag 0 at Cosmic+0x1188; FDN flag 1) on g*pred | kernel object at Raum+0x1fc0 (algo 0) or +0x4400 (algo 1), vtable slot 0x38 | no |
| LFO and polynomial | Cosmic LFO (14198e840 writes Cosmic+0x1110..0x1190), polynomial P | none | no |
| Store factor | g^3 C | g * K, then g | no |
| Post loop (section 3) | yes | yes | shared |
| Mix (section 4) | yes | yes | shared |

Algorithm select: 9f4 = clamp(trunc(3 * Raum+0x5f4), 0, 2) (setter 141980f40, 0141980000.c
~510-520). CONFIRMED. The Raum ctor default is 3, which skips the core and runs only the post loop.

## 6. Remaining UNKNOWNs

- Which parameter writes Raum+0x8c (F, feedback law). INFERRED: feedback.
- Writer of the SVF preset q (block +0x18 = Raum+0xcf8) and of the pre-warp words
  (Raum+0xcf0 and +0xd8c). INFERRED pi/sr for the pre-warp.
- Meaning of the limiter parameter 1 (value max(s, 1/12)); gain table of 141ae3ba0.
- p2 (prepare argument) = sr. INFERRED.
- Conditions of the 9b8 = 0 writes (141980f40 520/539; 1419830e0 1906; 141983140 1957).
- Linearity of the kernels (Grounded/Airy g^2 claim).
- Cosmic chain and LFO constants beyond those in the findings (not re-checked here).
- No native vector; every law above is decompile-only.
