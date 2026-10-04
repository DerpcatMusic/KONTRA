# Flanger scalar helper boundary

Measured 2026-10-04 against the unchanged official UVI Workstation 4.0.9 x64
executable (SHA256 `78729e96b752aea746280275072ad24cb4399a053739c49a161ff1fcfbf85721`)
and its earlier loaded-text snapshot (SHA256
`684a5557efed9e426a62cbba75c15cc727d764680e1660164b7c5ae88698fda1`).

[Official element parameters](https://lua.uvi.net/_elements.html#flanger) and the
[Falcon manual, Flanger](https://uvi.s3.us-east-1.amazonaws.com/UVIFC/falcon_2026_manual.pdf)
identify Speed, Feedback, DelayTime, Depth, Mix, SyncToHost and Bypass. The scalar
helper covers the five direct signal fields. Native observation establishes a
32-bit triangle clock, 20 ms scaling of the direct delay/depth fields, cubic delay
reads, independent channel rings, feedback and an additive wet signal over unity
dry input. Signal reset clears delay memory and preserves oscillator phase.

The Rust leaf matched float32 output exactly in 46 authored native comparisons:
24 cold signal cases at 8/32/44.1/48/96/192 kHz with zero-depth, observed preset
settings and large delay/depth; 2/6/12-channel fragmented cases at 48 kHz; one
131,072-frame 48 kHz clock case; all five warm direct field transitions at
32/48/96 kHz; and three warm signal resets. The source-direct test retains an
authored native impulse and checks split buffers, reset, independent channels,
invalid controls and unmeasured-rate rejection. These are scoped measurements,
not exhaustive parameter or sample-rate proofs.

The isolated Unicorn oracle executes the original native callback and triangle
table initialization instructions. Scratch allocation and scalar zero-fill for
reset are authored hooks; SIMD dispatch globals are absent from the snapshot.
The reader executable is unchanged. Hosted parameter timing, connected-control
clocks, tempo sync, bypass lifecycle and whole-program audio remain unverified.
The leaf is **not registered with Program playback** and removes no preflight
blockers. Its diagnostic and rate gate preserve that distinction.

The owned Augmented Orchestra Bartok program was decoded only in memory. Its
four Flanger inserts all start bypassed, with DelayTime=0.2, Depth=0.50335938 and
SyncToHost=0. Speed is 0.39889875 or 0.50171787, Feedback is 0.39999998 or
0.41013733, and Mix is 0.89997077 or 1. There are 20 connections each to Speed,
Feedback and Mix. Initial bypass cannot waive live connections or later activation.
No vendor preset, script, bank audio, source, namespace, key, activation patch or
account state is included in this change.
