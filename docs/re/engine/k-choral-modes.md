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

- The swing's law. `k-modfx-classic.md` gives amount `d = a²·0.99 + 0.01` and pitch ratio
  `2^(d·0.025)`, which at amount 1 is ±1.75%, i.e. about ±0.5 ms on a 30 ms delay. The measurement is
  four times that, so either the ratio is applied differently or the amount enters elsewhere. The
  documented law is **not** sufficient on its own.
- The LFO's shape and its per-voice phase.
- The six loop-filter constants after the pans, and whether the 0.99 seven in mode 3 are the one-pole
  coefficients they look like.
