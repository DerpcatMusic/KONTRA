# Kontakt 8 modulation FX: Flair, Phasis, Vibrato/Chorus (unit doc)

Status: **not ported.** No Rust DSP law for these three modules is recovered
yet, so nothing is added to `sampler-core` and no parity row changes status.
They stay unsupported and report `NotModeled` (`unproved` in
`crates/sampler-kontakt/src/effects/timemod.rs`). Native vectors cannot be
written until the laws are recovered and an oracle capture exists.

Companion docs: [k-modfx-classic.md](k-modfx-classic.md) (module list, saved
fields, measurements) and [k-modfx-classic-bodies.md](k-modfx-classic-bodies.md)
(audio body locations). This doc adds what the decompile text shows for the
kernels and the parameter writer. Addresses are image VAs.

Decompile source: `/data/dev/workspace/Research/KONTRA/source/kontakt8/`
(`READY` present). Files are 64 KiB buckets; headers `// ==== <addr> <name> size=N`.
Functions marked **read** were read in full in this unit; the rest are only
located.

## Flair (0x5c)

- Audio body `FUN_14195f600` (3417 B, `0141950000.c`): **located**, only its
  opening read. The body runs the optional pre-stage, then a per-sample loop.
- Pre-stage: if byte `+0x1280` is set, `FUN_14195db30(this+0x1050, n)` returns
  the loop count and `FUN_141ee2160` runs on the same sub-object. Otherwise the
  count is `n`. (`FUN_14195db30` 137 B is now read, see the Phasis section;
  `FUN_141ee2160` 372 B: entry read, mode bodies not.)
- Per-sample two-channel recursion (**read**): input from the two channel
  pointers, state pairs at `+0xeb0/+0xeb8` and `+0xec0/+0xec8`, coefficients at
  `+0xe90/+0xe98` (feedback-like) and `+0xea0/+0xea8` (gains), mix taps at
  `+0xe30/+0xe38` and `+0xe40/+0xe48`. The two channels are processed as a pair,
  with the same trapezoidal-style doubling as Phasis.
- Four-lane ramp bank (**read**): gate byte `+0x495`. Phase `+0x478` advances
  by lane increments from the constant `1.0` (4 x f32, `0x144714b70`), base
  `+0x460`, weight `+0x418`, scalar gain `+0x4a0`, lane gains `+0x4a4/+0x4a8/+0x4ac`.
  Output goes to `+0x400`. This is a per-sample counter ramp, as the earlier note said.
- Still missing: the voice loop (`FUN_14195d9b0`, 347 B, located only), the
  pitch note writer (`+0x40`), the start-phase writer, and the resampler body
  (`FUN_141ee2160`, shared with Phasis and Vibrato).
- Random start phase (from the ctor, per earlier note, not re-read here): a
  process-global MINSTD (`x * 48271 mod 2^31-1`, seeded from `random_device`,
  0 mapped to 1). A golden test must inject the phase.

## Phasis (0x5b)

Status: **not ported, no law validated.** The per-block and per-sample order,
the 2x2 stage cascade, the output stage, the parameter writer and the runtime
tables that the writer reads are now read. The law itself is still open (see
"Still open"). Nothing here is a parity claim, and there is no native vector.

### Functions read in this unit

File `0141970000.c` unless noted. "In full" means the whole body was read.

| Function | Size | Role | Read |
| --- | --- | --- | --- |
| `FUN_14197cfe0` | 1857 B | audio body, one block | in full |
| `FUN_14197dad0` | 788 B | 2x2 stage cascade and output mix | in full |
| `FUN_14197bc10` | 444 B | cascade seed and coefficient-vector tail | in full |
| `FUN_14197d910` | 436 B | parameter writer | in full |
| `FUN_14197d740` | 461 B | pending slot-bit lists, flag gate | in full |
| `FUN_14197cd00` | 721 B | per-block bit gating, per-slot resample | in full |
| `FUN_14197e430` | 644 B | per-block notch-record handler | in full |
| `FUN_14197e6e0` | 995 B | per-sample coefficient set | in full |
| `FUN_14197eaf0` | 652 B | flag group `0x333`: log/exp cutoff mapping | in full |
| `FUN_14197ed80` | 492 B | stage count, mix weight, tangent prewarp | in full |
| `FUN_14197ef70` | 347 B | flag group `0x666`: ramp endpoints, power of the stage value | in full |
| `FUN_14197aff0` | 2539 B | constructor: defaults, random start phase | in full |
| `FUN_14197df70` | 1201 B | one-time initializer of the Phasis tables | in full |
| `FUN_14197bbd0` | 47 B | LFO shape thunk (see the LFO step) | in full |
| `FUN_14195db30` | 137 B | pre-stage frame count (shared with Flair) | in full |
| `FUN_1419622b0` | 185 B | per-sample LFO object update (`+0x640`) | in full |
| `FUN_141ee2160` | 372 B | resampler entry and mode dispatch | entry only |
| `FUN_141963f10`, `f20`, `f30`, `FUN_14197ba60` | 15 B each | 2-lane product, difference, sum, divide | in full |
| `FUN_141970b50` | 1533 B | prepare writer for `+0x828..+0xa88` | tail only |

### Per-block order (`FUN_14197cfe0`)

1. Atomically take the dirty mask at `+0x9b0` (set it to 0). For each set bit
   `i`, call the parameter writer with `i` and the float at `+0x9b4 + 4*i`.
2. Take the two channel buffer pointers from the first member of the two
   channel objects at `+0x5c8` and `+0x5e0`.
3. `m = FUN_14195db30(+0x3c8, n)`: the internal frame count, where `n` is the
   host frame count.
4. `FUN_141ee2160(+0x3c8, ...)` upsamples the input into the channel buffers.
5. `FUN_14197cd00(this, *(+0x10), ...)` applies the per-bit gating and the
   per-slot resample, then calls `FUN_14197e430`.
6. Per-sample loop over the `m` internal frames (next section).
7. `FUN_141ee2160(+0x4c8, ...)` downsamples the output back to `n`.

### Per-sample loop (`FUN_14197cfe0`, order verified)

a. Advance a 4-lane ramp at `+0xb78` and an 8-lane ramp at `+0xa78` by 1.0 per
   frame. The 4 lanes of `+0xb00` are `base(+0xb60) + gain(+0xb18) * phase(+0xb78)
   * scale(+0xba0..bac)`. The 8 lanes of `+0xa00` are
   `base(+0xa60) + phase(+0xa78) * scale(+0xaa0..aac) * gain(+0xa18)`.
b. Copy the eight lanes of `+0xa00` into the slot records at `+0xc18 + 0x138*k`
   (`k = 0..7`). For each slot in the mask `+0xc0c`, the value is
   `buf * scale(+0xc1c) + offset(+0xc18)`, clamped to `[min(+0xc20), max(+0xc24)]`
   and stored back. `buf` is the slot's source buffer at `+0xc38`.
c. If `+0x750 & 0x666`, run `FUN_14197ef70`. If `+0x750 & 0x333`, run
   `FUN_14197eaf0`. Both are gated on parameter changes.
d. If the byte at `+0x670` is set, run `FUN_1419622b0(+0x640)`.
e. LFO input: `x = phase(+0x680) + offset(+0x6b8)`, subtract 1.0 once if
   `x >= 1.0`, then `lfo = x - trunc(x + 0.5)`: a centred sawtooth on
   `[-0.5, 0.5)`. `FUN_14197bbd0` computes exactly this. The `std::function`
   at `+0x6f8` wraps the lambda, so this is medium confidence (identified by the
   call shape, not by the vtable slot).
f. Phase advance: `chunk = min(+0x6a0, +0x6b0)`, `+0x6a0 -= chunk`,
   `phase = phase + chunk * rate(+0x6a8) + offset(+0x688)`, then wrap once into
   `[0, 1)`.
g. Four lanes from the LFO value `s` and slot 4 (`+0x10f8`, call it `d`):
   `[s + d, s - d, s, s]`. The lanes are then folded (sign-bit masks, a 2^23
   round-to-integer trick, `minps`/`maxps` clamps), passed through `sqrtps` and a
   division, and stored at `+0x730..+0x748`. The fold is read, not reduced to a
   closed form.
h. Four-lane smoother (`+0x620..+0x62c`, gains `+0x630..+0x63c`): with
   `t_k = (lane_k + 1) * 0.5`, `v_k += c_k * (t_k - v_k)`. Read.
i. `FUN_14197e6e0(this, lanes)` once per frame: the coefficient set (next section).
j. `FUN_14197dad0(this, wet, dry)`: the cascade (next section). `dry` is the
   channel input pair and `wet` the cascade output pair.
k. Output stage, per channel, with gain `g` at `+0x980/+0x988` and state at
   `+0x9a0/+0x9a8`: `v = (x - s) * g`, `lp = v + s`, `s' = lp + v`, and the output
   is `x - lp`. This is a trapezoidal one-pole used as a high-pass. The arithmetic
   is read. The gain `g` is written by `FUN_141970b50` and `FUN_14197c780`
   (see "Still open").
l. Write the output pair to the channel buffers and clear the low nibble of
   `+0x750`.

### Stage cascade (`FUN_14197dad0`, `FUN_14197bc10`, read)

- Stage count `N` is the int at `+0x754`. Stage `s` state is 0x20 bytes at
  `+0x7a0 + 0x20*s`: `+0` ch1 s0, `+8` ch2 s0, `+0x10` ch1 s1, `+0x18` ch2 s1.
  The tail block at `+0x7a0 + 0x20*N` uses the same layout.
- Coefficients: `k1 = +0x770`, `k2 = +0x778`, `h1 = +0x780`, `h2 = +0x788`,
  `cross1 = +0x790`, `cross2 = +0x798`.
- Per stage, per channel (ch1 shown; ch2 uses `k2`, `h2`, `cross2` and the
  `+0x8` lanes):
  - `v = ((x * cross1 - s1) * k1 + s0) * h1`
  - next stage input `x' = x - v`. The decompiler's `CONCAT44` in this subtraction
    rebuilds the same double, so it is a no-op and the stage output is `x - v`.
    Resolved, high confidence.
  - `s0' = 2v - s0`, `s1' = s1 + 2 * k1 * v`
- Seed (`FUN_14197bc10`): the two 2-lane state vectors start from `[1.0, 1.0]`
  (`0x144f3a0b0`, `0x144f3a0b8`) and `[0, 0]`.
- Tail, in `FUN_14197dad0` (read): one more kernel block at index `N` takes its
  input scaled by `+0x758`. Its output is `(0x75c * (scaled - v) + 0x760 * x)`
  times the lane-0 ramp at `+0xb00`, averaged with the initial stage input
  (`0.5` at `0x144f39280/88`). `FUN_14197bc10` runs a similar tail on the 2-lane
  coefficient vectors (not an audio output). Read; the sign and weight meanings
  are not closed.
- `FUN_14197dad0` ends with a dry/wet lerp at weight `+0xfc0` and a normalization
  `1 / sqrt(w/(1-s) + (1-w))` with `s = +0xc18`. Read; meaning open.

### Coefficient set (`FUN_14197e6e0`, per frame, read)

Writes `+0x730..+0x748` (four-lane intermediates), `+0x920/+0x928`,
`+0x930/+0x938`, `+0x940..+0x958`, `+0x770/+0x778` (k), `+0x780/+0x788` (h) and
`+0x790/+0x798` (cross). Inputs: the four lanes from the body, the slot values,
`+0x5f8..+0x60c` (ramp values from `FUN_14197ef70`), `+0x714`, `+0x720`,
`+0x724`, `+0x72c`, `+0xe88`, `+0x1368`, `+0x14a0`. The math is sqrt, division,
polynomial `exp2`-style terms (`(i + 0x7f) << 23` float construction), and
tangent-like rational terms. Structure read. Closed form per coefficient: open.

### Parameter writer and runtime tables (`FUN_14197d910`, `FUN_14197df70`, read)

- Writer. The param row is 12 bytes at `0x14a64efc0 + 12*p`: `int slot`,
  `float offset`, `float slope`. If `slot != -1`, the value
  `param * slope + offset` is handled by mode:
  - mode byte `+0xc00 == 0`: the value goes to the slot array `+0xa48[slot]`,
    and the dirty flags `+0xa90` and `+0xa94` are set.
  - otherwise: `+0xa18[slot]` is zeroed, the value goes to `+0xa30[slot]`,
    `+0xa48[slot]`, `+0xa00[slot]` and `+0xa60[slot]`.
  - in both modes, if `DAT_14a64f088[slot] != 0`, the bit
    `DAT_14a64f088[slot] << s` is OR-ed into `*(+0xac0)`, where `s` is the low 5
    bits of the int at `0x14a1821f8 + 12*k` and `k` is the uint at
    `0x14a182150 + 4*mode`.
  If `slot == -1`, the notch branch runs. The notch row `p` at
  `0x14a64ee80 + 20*p` gives `count` and `mult`, and for flag 0 the index is
  `idx = clamp(trunc((count+1) * param), 0, count)`. The per-bit record at
  `+0x15d8 + 0x14*slot` gets `base + mult*idx` and a fraction, and dirty bit
  `+0xc10` is set. Flag 1 uses a threshold at 0.5 instead. No written row takes
  this branch (see "Still open").
- Tables are written at runtime. `FUN_14197df70` is a one-time initializer,
  guarded by the byte at `0x14a64ee70`. It writes the param, notch, slot-bit and
  route tables as immediates. The bytes in the PE at these addresses are
  pre-init and do not decode under the layout: there is no `-1` sentinel and the
  float fields are denormal. A read-only PE dump was taken for this unit; its raw
  values are not kept in the repo and are not used.
- Layout, from the writes:
  - param rows: `0x14a64efc0 + 12*p`, `p = 0..15`. The 16 rows end at
    `0x14a64f080`, where the flag word `DAT_14a64f080` starts.
  - notch rows: `0x14a64ee80 + 20*p`, `p = 0..15`; `+0` int slot, `+4` byte flag,
    `+8` int base, `+12` int count, `+16` int mult. The 16 rows end at
    `0x14a64efc0`, where the param rows start.
  - slot-bit table `0x14a64f088` (ints, slots 0..7); flag word `0x14a64f0a8`
    (OR-ed with 4, 2, 8); list table `0x14a64f0b0` (ints, indices 1..3 used).
  - route table `0x14a182160` (8-byte `{int flag, int target}`), valid for bits
    0..11. From bit 12 the entries alias the float constants at `0x14a1821c0`.
  - mode table `0x14a182150` (uint) and mode records `0x14a1821f8` (12 bytes):
    not written at runtime; the file bytes decode to small integers.
- Param rows written by `FUN_14197df70` (decoded, high confidence):

| id | row | slot | offset | slope |
| --- | --- | --- | --- | --- |
| 3 | `0x14a64efe4` | 0 | 0 | `0x144737744` = 0.95 |
| 4 | `0x14a64eff0` | 1 | 0 | `0x14470bad4` = 1.0 |
| 5 | `0x14a64effc` | 2 | 0 | 1.0 |
| 6 | `0x14a64f008` | 3 | 0 | 1.0 |
| 7 | `0x14a64f014` | 4 | -0.25 | `0x14470bad0` = 0.5 |
| 8 | `0x14a64f020` | 5 | 0 | 1.0 |
| 9 | `0x14a64f02c` | 6 | 0 | 1.0 |
| 15 | `0x14a64f074` | 7 | 0 | 1.0 |

  Rows 0, 1, 2 and 10..14 are not written. Their file bytes are not used.
- Notch rows written by `FUN_14197df70` (flag and unwritten fields marked):

| id | row | slot | flag | base | count | mult |
| --- | --- | --- | --- | --- | --- | --- |
| 0 | `0x14a64ee80` | 2 | 1 | 0 | 1 | 1 |
| 1 | `0x14a64ee94` | 1 | not written | 2 | 10 | 1 |
| 2 | `0x14a64eea8` | 0 | 1 | 1 | 1 | -2 |
| 10 | `0x14a64ef48` | 3 | 1 | 0 | 1 | 1 |
| 11 | `0x14a64ef5c` | 4 | not written | 1 | 63 | 1 |
| 12 | `0x14a64ef70` | 5 | not written | 0 | 6 | 1 |
| 13 | `0x14a64ef84` | 6 | not written | 0 | 4 | 1 |
| 14 | `0x14a64ef98` | 7 | 1 | -1 | 1 | 2 |

  Rows 3..9 and 15 are not written.
- Pending lists. `FUN_14197df70` fills the vector at `0x14a182220` (begin/end
  pointers `0x14a182220/228`) with `{5}`, and the vector at `0x14a182238` with
  `{1, 2, 3}`. `FUN_14197d740` turns each pending index `k` into a bit: the
  first list writes into `+0xac0` with `DAT_14a64f088`, the second into `+0xbc0`
  with `DAT_14a64f0b0` (values 4, 2, 8 at indices 1, 2, 3). Each bit is `value << 4`.
  It is set when the lane `+0xa18[k]` (first list) or `+0xb18[k]` (second list) is
  nonzero, and cleared when it is zero.

### LFO and state initialization (`FUN_14197aff0`, `FUN_1419622b0`, read)

- Start phase: each construction takes one MINSTD step on the process seed
  `0x14a64b6f0`. The seed is set once from `std::random_device` (0 mapped to 1),
  multiplier 48271, modulus 2^31-1. The phase is `(seed - 1) * 2^-31`
  (`0x144f385ac`), stored at `+0x678` and `+0x680`. A golden test must inject it.
- Defaults from the ctor: `+0x688 = 0`, `+0x690 = 0`, `+0x6a0 = 0`,
  `+0x6a8 = 1.0`, `+0x6b0 = 1.0`. `+0x6f8` holds the `std::function` for the lambda.
- `FUN_1419622b0` (read): object at `+0x640`, with int fields at `+0x640`,
  `+0x644`, `+0x648`, double `+0x650`, and double `+0x690` (scale). When the
  index at `+0x658` is 0 it writes the double at `+0x660` as
  `1 / (240 * int(+0x644) / (f(int(+0x640)) * int(+0x648) * dbl(+0x650)))`, where
  `f` is `FUN_1419a0200` (not read) and `240` is `0x144f38b48`. It then takes
  `v` from the table at `+0x660 + 8*index`. If `0 <= v <= 1/(+0x690)`, it writes
  `+0x698 = v` and `+0x688 = (+0x690) * v`. `+0x688` is the phase offset used in
  the LFO step. Read; the meaning of `f` and the table row are open.

### Constants (values from a read-only PE dump)

| Address | Type | Value | Use |
| --- | --- | --- | --- |
| `0x14470bad0` | f32 | 0.5 | notch flag-1 threshold in the writer and `FUN_14197e430` |
| `0x14470bad4` | f32 | 1.0 | identity |
| `0x14470bae8` | f64 | 0.5 | LFO centred-sawtooth offset |
| `0x144714988` | f64 | 1.0 | phase wrap threshold |
| `0x14470bb18` | f64 | -1.0 | phase wrap step |
| `0x144714b70..7c` | f32 x4 | 1.0 | ramp lane increments |
| `0x144714b90` | u32 | `0x7fffffff` | abs mask |
| `0x144714ba0..ac`, `0x14471cb90..bac` | u32 | sign and all-ones lanes | fold masks |
| `0x144f3a0b0`, `0x144f3a0b8` | f64 | 1.0 | cascade seed |
| `0x144f39280`, `0x144f39288` | f64 | 0.5 | cascade output average |
| `0x144f39250..5c` | f32 x4 | 0.5 | unipolar map `(x + 1) * 0.5` |
| `0x144f392f0..fc` | f32 x4 | 2^23 | round-to-integer trick |
| `0x144f392a0..ac` | f32 x4 | 2.0 | lane scale |
| `0x144f39230..3c` | f32 x4 | 1/6 | series coefficient |
| `0x144737724` | f32 | 1/12 | note to octave |
| `0x144f38cbc` | f32 | 69.0 | MIDI note A4 |
| `0x144f38cf8` | f32 | 440.0 | A4 in Hz |
| `0x144f385ac` | f32 | 2^-31 | MINSTD scale |
| `0x144f38b48` | f64 | 240.0 | `FUN_1419622b0` |
| `0x144f38804` | f32 | 0.7 | `FUN_14197ef70` power exponent |
| `0x1447148e4` | f32 | 0.25 | `FUN_14197ed80` tangent argument |
| `0x144714994` | f32 | 2.0 | `FUN_14197ed80` |
| `0x144fc4984` | f32 | 90.0 | `FUN_14197ed80` |
| `0x144714a00` | f32 | 10.0 | `FUN_14197ed80` |
| `0x144fc4bf8` | f32 | 5000.0 | `FUN_14197eaf0` clamp |
| `0x144714a54` | f32 | -1.0 | lane threshold in `FUN_14197e6e0` |
| `0x144737744` | f32 | 0.95 | param row id 3 slope |
| `0x14a1821c8` / `0x14a1821e0` | f32 | 30 / -30 | ramp range for `+0x5f8` (low / high) |
| `0x14a1821cc` / `0x14a1821e4` | f32 | 120 / 190 | ramp range for `+0x5fc` |
| `0x14a1821d0` / `0x14a1821e8` | f32 | 56 / 121 | ramp range for `+0x600` |
| `0x14a1821d4` / `0x14a1821ec` | f32 | 0.9 / 0.9 | ramp range for `+0x604` |
| `0x14a1821d8` / `0x14a1821f0` | f32 | -120 / -120 | ramp range for `+0x608` |
| `0x14a1821dc` / `0x14a1821f4` | f32 | 20 / 111 | ramp range for `+0x60c` |

The ramp range values are the two ends of a linear map by lane 2 of the buffer
at `+0xb00`, read in `FUN_14197ef70`.

The `FUN_14197e6e0` polynomial and `exp2` constants are at `0x147697ad0..adc`,
`0x147698050..07c`, `0x14769c080..17c` (about 70 f32 values). They are recorded
by range only; each one needs its role traced before use.

### Still open (blocks a faithful port)

1. Oversampler. `FUN_141ee2160` dispatches on the object mode. Mode 2 copies the
   input. Modes 1, 3, 4, 5, 6 call `FUN_141ee7670`, `FUN_141ee8370`,
   `FUN_141ee7cd0`, `FUN_141ee79e0`, `FUN_141ee80a0`, and a zero count calls
   `FUN_141eebcb0`. None of these are read. The mode field of `+0x3c8` and
   `+0x4c8` is 0 at construction, with a count field of 2 at `+0x3cc` and
   `+0x4cc`. The mode the body uses is not traced.
2. Prepare and reset helpers. `FUN_14197c780` (1034 B) writes the gains
   `+0x980/+0x988` at its line 10498 and the stage defaults. `FUN_14197c640`
   (221 B) zeroes the output state `+0x9a0/+0x9a8` and the LFO state
   `+0x680/+0x6a0`, sets `+0x6a8 = 1.0`, and writes the smoother state `+0x620`.
   `FUN_14197cb90` (367 B) touches the flag words `0x14a64f080` and
   `0x14a64f0a8`. `FUN_14197ded0` (146 B) is called from `FUN_14197e430`. None
   of these are read.
3. `FUN_141970b50` (1533 B) writes the prepare coefficients `+0x828..+0xa88` from
   one float argument, including `+0x930`, `+0x940`, `+0x980`, `+0x988`. Its
   caller is not located. Only the tail is read.
4. Stage count. The target stage count comes from the int at `+0x15f8` minus 1
   (`+0x15f8` is the record index that `FUN_14197e430` writes for route bit 1),
   unless flag `0x40` is set. In that case it comes from the lane-1 ramp through a
   piecewise integer table at `0x14769bf50` (partly read). The law is not closed.
5. Notch branch activation. No written param row has `slot == -1`, so the notch
   branch of the writer is unreachable with the rows `FUN_14197df70` writes.
   Rows 0, 1, 2 and 10..14 are unwritten, so the branch may be fed by a writer
   not yet found.
6. Saved field to engine id. The TSV
   (`docs/re/kontakt-bindings/fx_raum_delight_psyche_vibrato_phasis_replika.tsv`)
   gives the saved offsets and the measured laws for `ENGINE_PAR_PHASIS_RATE`
   (+8), `RATE_UNIT` (+8, +12, +13), `CENTER` (+12), `SPREAD` (+16),
   `FEEDBACK` (+20), `MIX` (+24), `NOTCHES` (+28, a table law), `AMOUNT` (+32),
   `STEREO` (+36), `MOD_MIX` (+40), `ULTRA` (+44), `INVERT_PHASE` (+45) and
   `INVERT_MOD_MIX` (+46). The engine ids of these names are not mapped to
   writer rows 0..15.
7. CRT math. `tanf`, `logf`, `powf`, `sqrtps`, `divps` and `sqrt` appear in
   `FUN_14197ed80`, `FUN_14197eaf0`, `FUN_14197ef70`, `FUN_14197e6e0` and
   `FUN_14197dad0`. A Rust port must choose f32 or f64 and the libm results. There
   is no native oracle.

## Vibrato/Chorus (0x69, `hammondvibratochorus::HammondVibratoChorus`)

- Wrapper `FUN_14099eea0` (340 B) and core `FUN_14196d230` (1133 B): located,
  not read in this unit. Entry points and field offsets are in
  [k-modfx-classic-bodies.md](k-modfx-classic-bodies.md) and are unchanged.
- LC ladder bodies `FUN_14196b9e0` (3055 B) and `FUN_14196c600` (3074 B),
  `FUN_141969370` (613 B): located, not read. The "wave-digital LC ladder"
  label is still unverified.
- Nothing from this module was re-read or confirmed in this unit.
- Vibrato/Chorus is not related to Phasis; do not reuse the notch-table writer
  for it.

## Gap table

| Module | Status | Recovered in this unit | Missing | Blocker |
| --- | --- | --- | --- | --- |
| Flair (0x5c) | unsupported | per-sample two-channel recursion, four-lane ramp bank and gate (read) | voice loop `FUN_14195d9b0`, resampler `FUN_141ee2160` modes, pitch note `+0x40`, start-phase writer | ~5 KB of helpers unread; no audio vectors; random start phase must be injected |
| Phasis (0x5b) | unsupported | per-block and per-sample order, 2x2 stage cascade, output stage, parameter writer, runtime table layout and decoded rows (read) | oversampler modes, prepare/reset helpers (`FUN_141970b50`, `FUN_14197c780`, `FUN_14197c640`), stage-count law, notch activation, saved-field id map, closed-form coefficient set | oversampler and helpers unread; binding to writer rows 0..15 open; no native vector |
| Vibrato/Chorus (0x69) | unsupported | none new | ladder equations, core body, wrapper | all bodies unread here |

## Engine status (unchanged)

- No change to `crates/sampler-core/src/dsp/**` or `crates/sampler-ir/src/fx/**`.
- Parity rows for Flair, Phasis and Vibrato/Chorus stay `missing`.
- The existing Phasis allpass cascade in `SweepLaw::Sweep` must not be reused as
  Phasis: the recovered kernel is a 2x2 state-space stage with a per-stage
  coefficient helper, which is a different law.
- Phasis is not added to `sampler-core` in this unit. The stage kernel and output
  stage are recovered, but their coefficient inputs (the `FUN_14197e6e0` set and
  the four-lane shaping) are not closed, and the oversampler and the stage count
  are unread. A core added now would be guessed constants, so it waits.

## Next steps (in order)

1. Phasis: read the oversampler modes and trace the mode writers of `+0x3c8` and
   `+0x4c8` (`FUN_141ee2160` callers, `FUN_141ee19a0`, `FUN_141ee86c0`).
2. Phasis: read `FUN_14197c780`, `FUN_14197c640` and `FUN_14197cb90`, and locate
   the caller of `FUN_141970b50`. They set the output gain, the LFO reset and the
   prepare coefficients.
3. Phasis: map the saved fields to the writer rows by resolving the `ENGINE_PAR`
   index of each Phasis parameter in the wrapper that writes `+0x9b4`. Reduce the
   four-lane shaping and `FUN_14197e6e0` to closed form per coefficient.
4. Flair: voice loop `FUN_14195d9b0`, resampler `FUN_141ee2160` modes, then the
   pitch note writer.
5. Vibrato: the LC ladder entry points and the core `FUN_14196d230`.
6. Only then add a shared config-driven core in `sampler-core` with a recorded
   vector per module. Do not add per-product copies.

## Confidence

- Stage recurrence (`x - v`, `s0' = 2v - s0`, `s1' = s1 + 2kv`): high (two functions read in full).
- Per-block and per-sample order: medium-high (body read in full; helpers listed above).
- Output stage (trapezoidal high-pass): high for the arithmetic; medium for the gain source `+0x980/+0x988`.
- LFO shape (centred sawtooth): medium (thunk identified by call shape).
- Coefficient set `FUN_14197e6e0` and four-lane shaping: medium for structure, low for closed form.
- Runtime table values written by `FUN_14197df70`: high (immediates). Unwritten fields: unknown.
- Flair ramp bank and gate: high for the operations, medium for the increment semantics (taken from the constant `1.0` as the earlier note states).
- All laws: none validated. No parity claim.
