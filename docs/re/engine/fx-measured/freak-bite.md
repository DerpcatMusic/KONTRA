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

## Freak (module 0x68) → `ir::RingMod`, all three modes

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

Radio, Sidechain and FEEDBACK, from the core's setters and process
(through v2's port); not yet rendered against Kontakt:

| Control | Law |
|---|---|
| FEEDBACK | `a = 0.9x²` of the wet output, low-passed at 15 kHz and high-passed at 20 Hz, taken off the input, solved implicitly (≤ 4 passes, 1e-4); the shifted branch's level `q·√(1 − a²) + 1 − q` (Oscillator, Sidechain) |
| MODE 2 Sidechain | carrier = the input's analytic pair, its quadrature branch inverted; CONTOUR blends it to its envelope (mono magnitude), whose attack and release are `0.005 + 0.195c`, `0.01 + 9.99c` s, `c = RELEASE³`; the TYPE bias follows the envelope; no input high-pass |
| BP_FILTER, BP_FREQ | 3.5-octave band-pass on the modulator at pitch index `455x + 1551.88` |
| MODE 0 Radio | the input × `3·HARMONICS + 1`, plus the TYPE bias (× the input's level with DEMOD), transmitted on a sine at pitch index `200·CARRIER + 1881.88`; noise (`0.299a² + 0.001`, 5 kHz low-pass) added, then received |
| TUNING, CARRIER | receiver band-pass at pitch index `5·(a − b + 2b·TUNING) + 1381.88`, `a = 40·CARRIER + 100`, `b = 24 − 12·CARRIER` (the carrier at 0.5) |
| WIDTH | band-pass `0.1 + 1.9x` octaves, makeup `1/(0.556v + 0.444)` below one octave |
| DEMOD | off: ADAA rectifier, 50 Hz high-pass, 4-pole Butterworth at half the tuned pitch; on: the band-pass × a sine at the tuned pitch |
| level | received × `2/√(3·HARMONICS + 1)`, then `0.707/(e + 0.4)` of a 2/200 ms follower |

## Saved records

Checked against Kontakt 8's own readers (reader_schemas.tsv plus the
decompile) and the member offsets in
docs/re/kontakt-bindings/fx_bite_dirt_freak_wowflutter_twindelay.tsv:

- Bite, reader [ref], schema `IFx10B`: HPF i32 at 0, ten floats at
  4..40 (FREQ, BITS, JITTER, CRUNCH, DITHER, EXPAND, SAT, PRE, POST, MIX),
  DC_QUANT byte at 44.
- Freak, reader [ref], schema `Fx13Bx5I`: thirteen floats at 0..48
  (member offset − 8: TYPE, HARMONICS, FEEDBACK, MIX, TUNING, CARRIER,
  WIDTH, FREQUENCY, ANTIFOLD, STEREO, CONTOUR, BP_FREQ, RELEASE), five
  bytes at 52..56 (DEMOD, GATE, WIDE_RANGE, BP_FILTER, then one with no
  ENGINE_PAR), MODE i32 at 57, stored raw 0/1/2.

The record is packed: no padding between the byte run and the mode.

## Gate (Kontakt 8.13.1, 2026-11-15)

One oracle session, `fx_lab.py` `sweep` (5 s log sweep, every control set), `fx_export.py` + `fit` + `fx_gate.py`;
dB, model minus oracle, 1/3-octave band rms / max, 10 ms frame rms:

| Case | band rms / max | frame rms | |
|---|---|---|---|
| Freak Oscillator, FREQ .75, TYPE .5 | 1.09 / 4.6 | 2.26 | passes |
| Freak Oscillator, TYPE 1, FEEDBACK .7 | 2.30 / 8.9 | 1.57 | worst at 31 Hz |
| Freak Radio, CARRIER .5, FEEDBACK .3 | 4.91 / 16.9 | 4.74 | worst at 16 kHz (the receiver's top) |
| Freak Sidechain, TYPE 1, CONTOUR 0 | 7.51 / 16.6 | 8.28 | fails: level and lows (25 Hz) |
| Bite FREQ .5, BITS .5 | 0.16 / 0.5 | 2.55 | passes |
| Bite FREQ .2 | 12.0 / 32.0 | 2.20 | fails above the clock (1.6 kHz): images |
| Bite SATURATE .7 | 1.11 / 1.1 | 1.12 | passes |
| Bite BITS .3 | 0.17 / 0.6 | 0.25 | passes |

Fresh read-back of a script-loaded Freak: every control at the core's init values used in `FRESH` (FEEDBACK,
DEMOD, BP_FILTER 0; TUNING, WIDTH, CONTOUR, BP_FREQ, RELEASE 0.5; CARRIER 1; MODE 1).

### The three fails, against the native cores (2026-11-15)

The models were run sample for sample against v2's emulator-checked native cores (Freak's and Bite's, compiled
from `e80a9f22b^` with stubs, on the same stimuli and the translator's laws): Freak Oscillator 0.0 dB everywhere,
Freak Sidechain (TYPE 1, CONTOUR 0) 0.0 dB across the sweep, Freak Radio (CARRIER .5, FEEDBACK .3) within 0.3 dB,
Bite FREQ .2 within 0.6 dB (bands 200..6400 Hz: 0, 0, 0, 0, −0.3, −3.4). The laws also match the core ctors (clock,
bits, the corners at clock/2, compander, mix, HPF). So the gaps above are not in the cores the models port: they
come in upstream, in the BParFX wrappers' parameter path or the level the core sees (Sidechain follows the input
square-law, so a level offset doubles in dB). Bite's apply feeds cores 2, 5, 3, 4, 6, 8, 0, 1, 9,
11, 7 through [ref](obj, k)` and HPF as `int·0.5`; that value path and the oversampling wrapper
([ref], flag +0x57c) are the next reads. No model was changed without them (that would be fitting).
