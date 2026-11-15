# Bite and Freak, measured in Kontakt 8.13.1 (2026-11-14)

Both models take their laws from the native cores as v2 ported them
(`sampler-core/src/dsp/color/{bite,freak,hilbert}.rs` at e80a9f22b^, each
checked there against vectors of the emulated native core), with the
renders below as the check that they are Kontakt's. Stimuli from
`fx_lab.py` (session slot 1 left out); every setting sets all controls
explicitly. A script-loaded fresh unit is not the factory INIT preset: the
INIT Bite turned a sine into a ±0.2 square (saturated), and the INIT
Freak runs its carrier at 2.5 Hz where a fresh unit's FREQUENCY 0.513572 is
0.1 Hz.

## Bite (module 0x66) → `ir::Crush` with an `ir::Converter`

Engine: `crates/dsp/drive/src/converter.rs` (the reducer's modelled mode;
Lo-Fi keeps the raw hold and quantizer). Laws: `crates/format/kontakt-effects/src/bite.rs`.

| Control | Law | Render check |
|---|---|---|
| FREQUENCY | clock `100·441^x` Hz (pitch index `527.08x + 1598.63`) | FREQ 0.5 −5.9 dB at 1 kHz, 0.2 −5.7 dB at 159 Hz: pre+post at half of 2.1 kHz / 338 Hz |
| hold | cubic Hermite reads, 4-tap band-limited steps, ×2 halfband ≤ 48 kHz | fresh droop −0.9/−1.4/−3.8/−6.8 dB at 8/10/16/20 kHz = sinc² at 44.1 kHz (−0.95/−1.49/−3.9/−6.3) |
| PREFILTER, POSTFILTER | four-pole Butterworth (TPT SVF pair); half the clock at 0.5, 50 Hz at 0, 22.05 kHz at 1 | PRE 0 / POST 0: −3.1 dB at 50 Hz, −8.4 at 63, −23.5 at 100 (4th order); POST 0 floors at −40 dB (not carried) |
| BITS | `2 + ⌊15x⌋` bits, step `2^(1−bits)` on the companded scale, round to nearest | EXPAND 1 steps .145/.0835/.024 at BITS 0/.1/.25 (input × bitgain), 2.8e-3/1.7e-3/5.4e-4 at .4/.5/.6 from noise power |
| EXPAND | μ-law `μ = 2^(8 − 7.9x) − 1` (255 at 0, 0.072 at 1) | EXPAND 0, BITS 0.1: levels .004/.021/.086/.337 (logarithmic); no effect at 16 bits |
| DC_QUANT | mid-rise steps (no zero) | BITS 0 + DC: −0.08 → ~1.0 levels, silence buzzes |
| SATURATE | gain `+24x` dB into `y·(1 − (0.05 + 0.2x)·|y|)`, trim `−8x` dB | fresh: −0.17/−0.43/−0.84 dB at 0.32/0.53/0.94 input; 1: output ≈ 0.233·tanh(21x) |
| CRUNCH | `18x·log10(step)` dB in, the same out | not rendered |
| JITTER, DITHER | `0.9x²` of a period per capture; `x²` steps of Gaussian dither | not rendered |
| HPF | one-pole 5 / 100 / 200 Hz | HPF 1: −3.1 dB at 100 Hz; HPF 2: −3.1 at 200, −18 at 25 |
| MIX | wet `2m²` below 0.5, `4m − 2m² − 1` above; dry `1 − wet` | MIX 0.5: −6.0 dB above the clock |

The engine reads the newest four inputs where the core reads three to six
frames back, and delays the dry 8 frames instead of 11: the same output 3
core frames sooner. This keeps the reducer within the per-voice unit size
(voices copy their units without allocating).

Also from the core: fewer than 9 bits scale the output by 0.6..0.95, and
silence (follower ≤ 1e-4, 5/100 ms) holds the last sample.

## Freak (module 0x68) → `ir::RingMod`, Oscillator mode

Engine: `crates/dsp/spatial/src/ringmod.rs`. Laws: `crates/format/kontakt-effects/src/freak.rs`.

| Control | Law | Render check |
|---|---|---|
| FREQUENCY | `range·(2x − 1)³`, range 5000 Hz (WIDE_RANGE) or 200 | TYPE 1 on 1 kHz: 1040, 1625, 6000 Hz at 0.6, 0.75, 1 |
| TYPE | `t = 2x − 1`; below 0 a carrier bias `t²` scaled `1 − t²/2` (AM at 0); above, the quadrature branch at `t²` (a single sideband at 1) | INIT preset (2.5 Hz): TYPE 0 a full-depth tremolo at 2.5 Hz, 0.5 an \|cos\| envelope at 5 Hz, 1 a flat envelope |
| HARMONICS | diode law `k = 13.5x + 0.5`, normalized `√(k²/2 + 1)/(2k²)` | not rendered |
| STEREO | carrier offset `±(2x − 1)/8` cycle per side | INIT 0.5: L = R |
| ANTIFOLD | input one-pole high-pass, pitch index `5x·(17.31·ln(|f| + 200) − 36.38) + 1381.88` (8.2 Hz at 0) | not rendered |
| GATE | wet scaled down below a 1e-5 follower (5/100 ms) | not rendered |
| MIX | equal power, `√m` wet, `√(1 − m)` dry | not rendered |

The Hilbert pair is Niemitalo's fixed polyphase IIR, not the native
per-rate elliptic design: they differ in phase only.

Not carried, reported by the translator: the Radio mode (MODE 0: fuzz,
band-pass, noise and demodulator chain; TUNING, CARRIER, WIDTH, DEMOD) and
Sidechain mode (MODE 2: follower, CONTOUR, RELEASE, BP_FILTER, BP_FREQ),
and FEEDBACK (the core's implicit feedback loop). Their scripts' moves have
no rows, so they report as unknown parameters.

## Saved records

Field order from v2's readers (Bite apply 0x140aba100: HPF i32, ten
floats, DC_QUANT byte; Freak apply 0x140aba860: thirteen floats, five
bytes, the mode i32). Byte offsets assume packed fields and are not yet
byte-diffed against a probe save.

## Gate

The fit gate (`fx_export.py`, `fx_gate.py`, `fit` test) was not run.
Under the 2026-11-15 rule (read first, no per-parameter sweeps) the models
come from the core laws above, and the renders here only confirm them.
