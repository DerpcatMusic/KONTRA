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
The reader executable is unchanged. These scalar fixtures do not establish hosted parameter timing, connected-control
clocks, tempo sync, bypass lifecycle or whole-program audio.
The leaf is **not registered with Program playback** and removes no preflight
blockers. Its diagnostic and rate gate preserve that distinction.

The owned Augmented Orchestra Bartok program was decoded only in memory. Its
four Flanger inserts all start bypassed, with DelayTime=0.2, Depth=0.50335938 and
SyncToHost=0. Speed is 0.39889875 or 0.50171787, Feedback is 0.39999998 or
0.41013733, and Mix is 0.89997077 or 1. There are 20 connections each to Speed,
Feedback and Mix. Initial bypass cannot waive live connections or later activation.
No vendor preset, script, bank audio, source, namespace, key, activation patch or
account state is included in this change.


## Explicit physical points, tempo sync and bypass (still unadmitted)

The follow-up executes native outer wrapper `0x140ecc2d0`, generic parameter
manager `0x14134a480`, native Flanger property callbacks and the signal kernel.
It reuses the MS20/Comb caller-owned descriptor and parameter-manager fixture
instead of replacing native clock arithmetic. Metadata, allocation and the
thread stack-limit record are authored; this remains an isolated native
instruction fixture, not a hosted DLL or complete preset render.

The production `process_control_points` dispatcher consumes one already-prepared
physical Speed/Feedback/DelayTime/Depth/Mix tuple per 32 frames. It never smooths
or combines connections. Active controls are held for each interval. When
bypassed, it selects the last interval's controls, passes audio through, and
freezes ring memory, write heads and oscillator phase. Resuming preserves that
state. The sync flag and tempo are supplied as caller-owned effect/context fields.
Native synced processing uses the float32 law `(1/60 / Speed) * tempo` for oscillator
frequency. Live SyncToHost edits additionally remap Speed; that property-edit
lifecycle is not implemented by this dispatcher. The dispatcher is gated to 32/44.1/48/96 kHz and tempo 60–300 BPM.

Compiled Rust matches native float32 audio exactly in 80 outer cases: mono and
stereo at those four rates, authored warm Speed/Feedback/Mix changes, buffer
lengths 257/65/129, sync off or sync at 60/120/180/300 BPM, and middle-buffer
bypass/resume. Comparisons consume the measured native physical points; they
prove point dispatch and DSP rather than independent control generation. Dry
samples are independently compared to input; bypass snapshots prove phase,
ring and head freeze.

An independent fixture reuses the measured generic smoother law in normalized
units: float32 `1-pow(0.33f32,100/rate)` per sample and
`1-pow(0.33f32,3200/rate)` per full interval. Speed's linear physical mapping
uses 0.01–10, while Feedback and Mix use 0–1. It emits the prior future value,
then advances by a full interval or the actual partial-buffer sample count;
padded endpoint advancement is separate. The independent physical points and
persistent normalized future match native bits in all 96 comparisons for
Speed/Feedback/Mix at the four rates and lengths 1/17/31/32/33/65/129/256.
This clock is measured but not implemented by the DSP leaf.

`process_frames` accepts the shared `dsp::Frame`/`MAX_CHANNELS` representation.
Its 32-frame planar bridge uses fixed stack storage and preserves inactive
channels. An allocation-counted 12-channel 257-frame check records zero heap
allocations across shared-frame processing and synced point/bypass dispatch.
The direct source check covers frame/planar equivalence and malformed point
batches in addition to the earlier scalar checks.

The shared native static connection-mixer measurements are a separate boundary;
they do not establish this preset's connected graph or its control timing.
Connected modulation, full host lifecycle, larger channel counts through the
outer wrapper, other rates/tempos and whole-program audio remain unverified.
All four actual Bartok Flanger inserts therefore remain preflight blockers.
