# Kontakt 8 Choral (0x5d): the four voice-bank mode tables

Recovered 2026-10-30 from the Kontakt 8.13.1 binary (`FileVersion 8.13.1`,
`SizeOfImage 0xb254000`) at the initialiser the docs call `MODES`, VA `0x14193cd70`.

`k-modfx-classic.md` described these as "four mode tables, seven voices each". They are not stored
data: `0x14193cd70` is inside `.text` and is a switch on `0x740(%rcx)` (the mode) that writes float
immediates into the bank at `0x6c8(%rcx)`. The switch resolves as:

| Mode | Block |
| --- | --- |
| 0 | `0x14193d854` |
| 1 | `0x14193d4b8` |
| 2 | `0x14193d130` |
| 3 | `0x14193cd9e` |
| 4 | `0x14193cd9e` (same as 3) |

Each record is seven delays (seconds), then seven gains, then pans and the loop filters.

## Mode 0 — `0x14193d854`

```
delays  0.003  0.003  0.007  0.007  0.011  0.011  0.013
gains   1.0    1.0    1.42857 1.43  1.7242 1.72765 0.7423
pan     +0x3c 0.5   +0x44 0.5   +0x4c 0.5
filter  +0x54..0x60 6.1   +0x64 0.1   +0x68 0.1
other   +0x6c 1.0   +0x80 0.01  +0x84 0.01   +0x8c 5000   +0x90 5000
```

## Mode 1 — `0x14193d4b8`

```
delays  0.007  0.007  0.007  0.007  0.013  0.013  0.029
gains   1.0    1.0    1.0    1.0    1.0    1.0    0.7423
pan     +0x3c 0.5   +0x40 0.3333 +0x44 0.8333 +0x48 0.6667 +0x4c 0.1667
filter  +0x54 4.1   +0x58 4.1   +0x5c 4.0   +0x60 4.0   +0x64 4.12  +0x68 4.1241
other   +0x6c 1.0   +0x70 1.0   +0x74 1.0
```

## Mode 2 — `0x14193d130`

```
delays  0.011  0.011  0.011  0.011  0.011  0.011  0.053
gains   1.0    1.0    1.0    1.0    1.0    1.0    0.7423
pan     +0x3c 0.5   +0x40 0.5   +0x48 0.25  +0x4c 0.75
filter  +0x54..0x68 6.1
other   +0x6c 1.0   +0x8c 12000  +0x90 12000 +0x94 150  +0x98 150  +0x9c 20000 +0xa0 20000
        +0xa4 50    +0xb0 1.0    +0xb4 1.0    +0xc0 1.0   +0xc4 0.1   +0xc8 0.1
```

## Mode 3 — `0x14193cd9e`

```
delays  0.007  0.007  0.011  0.011  0.013  0.013  0.029
gains   1.0    1.0    1.42857 1.42857 1.7242 1.7242 0.7423
pan     +0x44 0.25  +0x4c 0.25
filter  +0x54..0x68 6.1
other   +0x6c 1.0   +0x78 0.3   +0x7c 0.3   +0x80 0.6   +0x84 0.6
        +0x8c 20000 +0x90 20000 +0x94 20000 +0x98 20000 +0x9c 20000 +0xa0 20000
        +0xa4 50    +0xc0 1.0   +0xc8 0.2
        +0xe0..0xf8 0.99 (seven one-pole coefficients)
```

## Cross-check against measured impulse responses

`tools/re/choral_modes.py` sends an impulse through the emulated kernel with the modulation held
still. The taps land at:

| Mode | First non-zero frame | Time |
| --- | --- | --- |
| 0 | 373 | 7.8 ms |
| 1 | 875 | 18.2 ms |
| 2 | 1372 | 28.6 ms |
| 3 | 872 | 18.2 ms |

Ordering by largest delay agrees with the switch: mode 0 has the shortest bank (13 ms), modes 1 and 3
the middle (29 ms), mode 2 the longest (53 ms).

## Regenerating

```
objdump -d --start-address=0x14193cd70 --stop-address=0x14193dc10 "Kontakt 8.exe"
```

then read the `movl $0x…,(%rax)` / `movl $0x…,0xNN(%rax)` immediates inside each block as f32.

## Effective bank, read back from the emulator

The initialiser leaves some fields at the constructor's values, so the blocks above are not the whole
story. Reading the bank pointer (`[core+0x6c8]`) after the constructor and the mode setter gives what
the kernel actually sees (pans default to 0.0, and mode 4 is mode 3):

| Mode | delays (s) | gains | pans |
| --- | --- | --- | --- |
| 0 | .003 .003 .007 .007 .011 .011 .013 | 1, 1, 1.42857, 1.43, 1.7242, 1.72765, 0.7423 | 0, .5, 0, .5, 0, .5, 0 |
| 1 | .007 .007 .007 .007 .013 .013 .029 | 1 ×6, 0.7423 | 0, .5, .33333, .83333, .66666, .16666, 0 |
| 2 | .011 ×6, .053 | 1 ×6, 0.7423 | 0, .5, .5, 0, .25, .75, 0 |
| 3 | .007 .007 .011 .011 .013 .013 .029 | 1, 1, 1.42857, 1.42857, 1.7242, 1.7242, 0.7423 | 0, 0, 0, .25, 0, .25, 0 |
| 4 | same as 3 | same as 3 | same as 3 |

Loop-filter constants, per mode, the six floats after the pans: mode 0 `6.1 6.1 6.1 6.1 .1 .1`,
mode 1 `4.1 4.1 4.0 4.0 4.12 4.1241`, mode 2 and 3 `6.1 ×6`.

Read back with the emulator (`tools/re/choral_modes.py` uses the same core construction):

```
bank = e.read(core + 0x6c8, 'Q'); f = e.read(bank, 'f', 56)
```

## What is measured, and what is not

Measured on 8.13.1 through the emulator:

- **Voice delay** `base × (delay · 5 + 0.1)` s. Mode 0's first voice lands on frame 374 at delay 0.5
  and 734 at 1.0; the formula predicts 374.4 and 734.4.
- **Mode selection** `(int)(p · 4)` clamped to 0..=3, with 4 folding onto 3.
- **The LFO's swing at amount 1, rate 0.5, delay 0.5, mode 0**: an impulse train every 2000 frames
  puts the strongest echo between frames 1389 and 1594, i.e. 28.9 to 33.2 ms around a 30 ms centre,
  a swing of roughly ±7%.

Not measured, and therefore not implemented:

- The swing's law is **measured to be quadratic in amount**. Impulse train, rate 0.5, delay 0.5,
  mode 0, the voice whose base is 0.011 s (28.65 ms, 1375 frames at amount 0):

  | amount | echo range (frames) | swing | swing / base |
  | --- | --- | --- | --- |
  | 0.00 | 1375..1375 | 0 | 0 |
  | 0.25 | 1375..1387 | 12 | 0.87% |
  | 0.50 | 1378..1430 | 52 | 3.8% |
  | 0.75 | 1383..1492 | 109 | 7.9% |
  | 1.00 | 1389..1594 | 205 | 14.9% |

  The ratios track `a²` (0.0625, 0.25, 0.5625, 1.0) closely, so the peak-to-peak swing is about
  `0.15 · a² · base`. The documented pitch-ratio law (`2^(d·0.025)`, 1.75% at amount 1) is roughly
  eight times too small, so it is **not** the swing law and must not be used as one.
- The LFO's shape and its per-voice phase.
- The six loop-filter constants after the pans, and whether the 0.99 seven in mode 3 are the one-pole
  coefficients they look like.

## Kernel read (2026-10-30): the LFO, its phase, and the depth

Read from `0x14193be50` (process), `0x14193c7c0` (the modulation-matrix update, which is inert here:
`core+0x3d5` is 0, so it returns before writing anything), `0x14193bd90` (the per-voice LFO read),
`0x14193dc00`, `0x14193dd30`, `0x14193ddb0` (its setup) and the amount setter `0x14193a2f0`, then
confirmed by tracing the emulator (`Emu` hooks at `0x14193c431`, `0x14193c4b9`).

**The LFO is a triangle.** At rate 0.5, amount 0.6, mode 1, 48 kHz, the value handed to the voice
is `4·n·(hz/fs) − 1` to the last bit (`hz/fs` = 2.4479e-5) up to the peak at frame 20429, then the
mirror: `x = 4φ − 1` on the rising half, `x = 3 − 4φ` on the falling half.

**Rate law: `hz = 0.25 + 7.4 · rate³`** (Hz). `voice+(-0x220)` after prepare, 48 kHz:

| rate | 0 | 0.05 | 0.1 | 0.25 | 0.5 | 0.75 | 1 |
| --- | --- | --- | --- | --- | --- | --- | --- |
| Hz | 0.25 | 0.250925 | 0.2574 | 0.365625 | 1.175 | 3.371875 | 7.65 |

**Per-voice phase = the mode table's `pan` field.** `voice+(-0x158)` holds the offset the read adds
to the phase, and it is exactly the effective bank's pan for that voice: mode 1 `0, .5, .33333,
.83333, .66666, .16666, 0`; mode 0 `0, .5, 0, .5, 0, .5, 0`; mode 3 `0, 0, 0, .25, 0, .25, 0`. So the
`pan` column is the LFO's phase offset, not a stereo position.

**Depth law (supersedes the 0.15·a² fit below).** The amount setter stores
`m = 0.99·amount² + 0.01` at `voice+(-0x218)` and `1/(2^(0.025m) − 0.9999)` at `voice+0x58`;
`0x14193dc00` then stores

```
depth_samples = (fs / hz) · ½ · (2^(0.025·m) − 1)        at voice+(-0x210)
```

and the process adds `depth · s` to the base delay, where `s` is the smoothed LFO. Measured
(`voice+(-0x210)`, 48 kHz): 3.5398 / 25.4558 / 91.3451 / 201.6327 / 357.0320 samples at amount
0 / .25 / .5 / .75 / 1 (rate 0.5), and 611.4650 at amount .6, rate 0 (0.25 Hz). The impulse-train
table above under-samples a 1.175 Hz triangle, which is why it read only 205 frames at amount 1
where the depth field is 357 samples.

**The six loop-filter constants are per-voice coefficients.** `voice+(-0x1d8)` is the mode table's
filter value for that voice (mode 1: 4.1, 4.1, 4.0, 4.0, 4.12, 4.1241; 1.0 for voice 6, which the
table does not give one) and `voice+(-0x1d0)` is `1/`it. Two more per-voice smoother coefficients
sit at `voice+(-0x1f8)` and `voice+(-0x200)`, and a tap weight at `voice+(-0x1e0)` (which is what the
`cmpb -0x1a0(%rsi)` branch in the process tests):

| mode | `-0x1f8` | `-0x200` | `-0x1e0` |
| --- | --- | --- | --- |
| 0 | .2 .21 .21 .2 .4 .4 0 | .4 ×7 | 0 0 0 0 .01 .01 0 |
| 1 | .01 .01 .01 .013 .014 .012 0 | .3 .32 .32 .3 .33 .3 .4 | 1 ×6, 0 |
| 2 | .1 .1 .101 .101 .4 .4 0 | .2 .22 .22 .2 .2 .2 .99 | 0 ×7 |
| 3 | 0 .2 0 0 0 0 0 | .99 ×7 | 0 0 .3 .3 .6 .6 0 |

Which bank field or constructor table those come from is not identified yet.

**Feedback enters as a per-voice input gain**, `(0.999 · feedback^0.25)^(base_i/base_0)`
(`0x14193ae70`): at feedback 0.5 and mode 1 that is 0.840056 ×4, 0.723484 ×2, 0.485757.

**The process, per frame** (`0x14193be50`): for each voice — LFO read and glide (`0x14193bd90`), the
one-pole smoother, the fractional delay in samples, a **Catmull-Rom** read of the voice's ring (four
taps at `floor`, `floor±1`, `floor+2`, weights `p0 + ½f(p1−p−1) + f²(p−1−2.5p0+2p1−½p2) +
f³(1.5p0−1.5p1+½p2−½p−1)` with the ring running backwards in time) — then a seven-tap cyclic mix
(`0x141939270`) over `core+0x830` with the coefficients at `core+0x7c0` and the per-voice gains at
`core+0x748`/`core+0x780`, a global two-section tone filter (`core+0x1c20`..`0x1cd0`) and the
dry/wet mix at `core+0x1b88`/`core+0x1b90`. The ring is written at `widx` and `widx + size`
(double-buffered); the read pointer is `widx + 1`.

Not yet read, and the reason `Chorus::process` is still unimplemented: the per-voice smoothing and
filter networks behind `-0x1f8`/`-0x200`/`-0x1e0`, the pan rotation matrices at `core+0x8a0`, how the
seven-tap mix at `core+0x7c0` is applied per frame, and the global tone filter's design.

