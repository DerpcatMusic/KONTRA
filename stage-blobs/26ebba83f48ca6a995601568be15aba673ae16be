# Drive Mode 0, no-oversampling helper boundary

Measured 2026-10-04 against the unchanged official UVI Workstation 4.0.9 x64
executable (SHA256 `78729e96b752aea746280275072ad24cb4399a053739c49a161ff1fcfbf85721`)
and its earlier loaded-text snapshot (SHA256
`684a5557efed9e426a62cbba75c15cc727d764680e1660164b7c5ae88698fda1`).

[Official element controls](https://lua.uvi.net/_elements.html#drive) identify
DriveAmount 0..1, Mode 0..2, and Oversampling 0..4. The
[official Falcon description](https://www.uvi.net/falcon) describes three
saturation modes and internal oversampling up to 16x. This leaf only measures
Mode=0 and Oversampling=0; other modes and oversampling remain rejected.

Native registration reaches factory `0x1410fcde0`, constructor `0x1410fc480`,
DriveAmount setter `0x1410fcf70`, coefficient update `0x1410fbc00`, audio callback
`0x1410fd010`, and Mode-0 saturation helper `0x1410fdf90`. The no-oversampling,
static DriveAmount boundary has no audio delay or channel history. Its ordinary
rational curve uses float32 arithmetic:

```
a = DriveAmount * 0.95f32
ratio = a / (1 - a)
k = ratio + ratio
y = (x / (abs(x) * k + 1)) * (k + 1)
```

The implementation preserves the native operation ordering (divide before
doubling the coefficient), rather than reassociating the formula. Native SIMD
and scalar saturation instructions execute without replacement arithmetic
hooks in the fixture. A bypassed native outer buffer is unchanged.

Compiled production Rust, an independent float32 mathematical model, and the
native outer wrapper matched float32 output exactly in 1152 authored cases:
6 rates (8/32/44.1/48/96/192 kHz), 1/2/6/12 channels, DriveAmount
0/0.001/0.25/0.37540978/0.9/1, buffer lengths 1/7/32/65, and active/bypassed
buffers. Inputs include independent signed values from 1e-20 to 32, zero, and
unity. Direct native setter edits are included; the native internal-ramp-enable
flag is clear. This establishes the direct static setter boundary, not hosted
parameter smoothing or enabled internal ramps.

The native wrapper `0x140ecc2d0` executes with caller-owned effect/context/signal
metadata. The fixture does not initialize a complete host, property manager,
voice clone/cache, or oversampling state. Those boundaries, internal DriveAmount
ramps, connected controls and whole-program audio remain unverified. The Rust
leaf is exported for explicit helper use and **not registered with Program
playback**. The diagnostic and constructor gates preserve these limits.

The owned Augmented Orchestra Bartok program was decoded only in memory. Its
one Drive insert (node 56) is active with Mode=0, Oversampling=0,
DriveAmount=0.37540978 and no owned connections. Its separate DiodeClipper
insert (node 29) starts bypassed with Drive=3, Tone=20000, HighPass=1,
Asymmetry=0 and OutputGain=-3. This patch does not implement DiodeClipper.
No vendor preset, script, bank audio, source, activation patch, or account state
is included.
