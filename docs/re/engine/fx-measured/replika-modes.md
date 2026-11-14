# Replika Delay, measured in Kontakt 8.13.1 (factory `Effects/RDelay`, 2026-11-14)

Rig: `tools/kontakt-oracle/fx_lab.py` renders an effect at several settings and keeps the raw audio; `fx_rdl.py`
analyses; `fx_export.py` + `fx_gate.py` + the ignored test `fit` in `crates/format/kontakt/src/fit.rs` hold the
engine's model against the render (1/3-octave band level, 10 ms frame RMS). Knobs are set late from a note callback;
`RDL_TIME_UNIT` 0 before `RDL_TIME`. The oracle slot owns 5 MIDI channels, so one session holds the reference and 4
settings; a session loaded machine drops audio now and then (digital-silence stretches), so every number below is
from stretches without dropouts.

TYPE values: Modern 0, Analogue 1, Tape 2, Vintage 3, Diffusion 4. TIME raw 100000 (200 ms), FEEDBACK 0.

## DEPTH and RATE, wet only

Renders with `$ENGINE_PAR_SEND_EFFECT_DRY_LEVEL` 0, so no dry line cancels the echo: a steady 1 kHz sine (12 s,
-20.6 dBFS at the effect), the wet signal's analytic phase, delay(t) = -dphi / 2 pi f, fitted with a sine and with a
triangle (`fx_rdl.py`; slips are not corrected on wet renders). TIME 200 ms, FEEDBACK 0.

| Type | DEPTH | RATE | rate | swing (fitted, either side) |
|---|---|---|---|---|
| Modern | 1 | 0, 0.1, 0.2, 0.3 | 0.1000, 0.1585, 0.2512, 0.3981 Hz | sine 8.000, 8.000, 8.000, 7.97 ms |
| Modern | 0.05, 0.1, 0.5, 0.75 | 0.5 | 1.000 Hz | sine 0.400, 0.800, 4.000, 6.000 ms |
| Analogue | 1 | 0, 0.2, 0.4, 0.5, 0.6, 0.7, 1 | 0.102, 0.251, 0.631, 1.000, 1.585, 2.512, 10.01 Hz | triangle 19.75, 19.65, 19.15, 18.30, 16.40, 12.40, 0.37 ms |
| Analogue | 0.1, 0.25, 0.5, 1 | 0.5 | 1.000 Hz | sine 1.49, 3.73, 7.46, 14.95 ms |
| Vintage | 1 | 0.2, 0.5, 0.8 | 0.251, 1.000, 3.98 Hz | the same as Analogue within 0.01 ms |
| Tape | FLUTTER 0, 0.3, 0.6, 1 | any | 0.875 Hz | sine 0.00, 0.127, 1.017, 4.68 ms |

- Modern: a sine of 8 ms x DEPTH at 0.1 x 100^RATE Hz. Linear in DEPTH and exponential in RATE down to RATE 0, to the
  digit. The RATE 0.2 and DEPTH 0.5 band misses in the earlier dry + wet gate were not the laws: they were the dry/wet
  cancellation and, in the bands, the LFO's start phase (below).
- Analogue and Vintage: one law. A triangle of 19.7 ms x DEPTH at Modern's rate, averaged over 0.196 s. The averaging
  scales the fundamental by sinc(pi f 0.196 s): the fitted fundamental over the triangle's (8/pi^2 x 19.7 ms) is 0.994,
  0.975, 0.936, 0.845, 0.641, 0.258 and 0.019 at 0.1, 0.63, 1, 1.58, 2.51, 3.98 and 10 Hz, against sinc's 0.999,
  0.975, 0.938, 0.848, 0.641, 0.27 and 0.02. The triangle fits better than a sine below 1 Hz (0.6 against 1.46 ms rms);
  above it the averaging leaves mostly the fundamental. Linear in DEPTH. The engine's delay carries it as
  `triangle_s` (its LFO averaged in closed form, `averaged_triangle` in `kontra-dsp-delay`).
- Tape: FLUTTER only, a 0.875 Hz sine of 4.68 ms x FLUTTER^3 (8.0x from 0.3 to 0.6, 4.6x from 0.6 to 1, as the cube
  gives); DEPTH, RATE and TAPEAGE do not move it.

## The oracle's modulation starts anywhere, and its delay grows first

The engine's delay line grows as its LFO falls. The oracle's LFO phase at the tone's first sound is not the engine's,
so for modulated cases the gate fits the start phase (`pre`, seconds of silence before the input, in eighths of a
cycle). Without it a slow LFO's sidebands land in the next band by chance: RATE 0.2 read 0.08 or 1.84 dB at 800 Hz
depending on which part of its 4 s cycle sat in the analysis window's middle.

## Echo of an impulse and the frequency response of the echo (sweep, dB re the dry sweep)

| Type | first echo | level | response (100 / 1k / 4k / 8k / 12k / 16k Hz) |
|---|---|---|---|
| Modern | 197.2 ms | +0.3 dB | 0 / +0.1 / - / - / - / - (sweep dropped out above 2 kHz) |
| Analogue | about 190 ms | wet energy -60 dB re the dry impulse in the 190 ms frame, flat -80 dB floor after | -0.8 / -1.4 / -6.7 / - / - |
| Tape | 190.8 ms | -7.9 dB | +0.6 / +1.1 / -3.6 / -10.2 / -17.8 / -28.0 |
| Vintage | 190.5 ms | -4.7 dB | +0.4 / -0.2 / -1.2 / -3.9 / -8.4 / -16.6 |
| Diffusion | cluster from 190 ms | -9 dB in the first frame, then -3.5 dB per 100 ms to -24 dB at 620 ms (about 1.7 s to 60 dB) | -0.4 / -5.9 / -6.0 / -5.5 / -6.9 / -9.1 |

The Analogue impulse carries almost nothing (the echo is band-limited, so a one-sample click is the wrong stimulus for
it; use tones and sweeps). The echo times above come from dry + wet renders (instance timing jitters up to 90 ms, so
a wet-only render cannot time the echo); the non-Modern modes' 6-7 ms shorter echo is not carried (one TIME measured).

## Bodies, wet only (sweep through FEEDBACK 0, dB re the input)

| Type | 30 Hz | 120 | 500 | 1k | 2k | 4k | 8k | 10k | 12k | 16k |
|---|---|---|---|---|---|---|---|---|---|---|
| Modern | +0.1 | +0.1 | +0.1 | +0.1 | +0.1 | +0.1 | +0.1 | +0.1 | +0.1 | +0.1 |
| Analogue | -2.4 | -0.3 | -0.5 | -1.3 | -3.5 | -6.5 | -8.9 | -21.9 | -46.5 | -58.4 |
| Tape (TAPEAGE 0.25) | -6.7 | +0.5 | +1.6 | +1.2 | 0.0 | -3.2 | -9.5 | -12.5 | -15.8 | -23.9 |
| Tape (TAPEAGE 0) | -1.2 | +1.4 | +1.6 | +1.5 | +1.2 | +0.1 | -3.5 | -6.0 | -8.9 | -16.5 |
| Tape (TAPEAGE 1) | -14.2 | -3.2 | +1.5 | -0.3 | -4.4 | -10.2 | -17.5 | -20.8 | -24.2 | -32.3 |
| Vintage | -0.1 | -0.1 | -0.1 | -0.1 | -0.3 | -0.9 | -3.2 | -4.7 | -6.6 | -11.6 |
| Vintage (QUALITY 0) | 0.0 | -0.1 | -0.1 | -0.2 | -0.5 | -1.7 | -5.6 | -8.2 | -11.3 | -17.1 |

Analogue at -60 dB matches -20 dB within 0.3 dB (linear); BBDTYPE 1 changes nothing. Fitted as analog sections the
engine carries in the loop (`body` in `replika.rs`, within 0.45 dB rms for Analogue, 0.03-0.08 dB rms for Tape at
TAPEAGE 0 and 1 and 0.5 at 0.25, 0.07-0.17 dB for Vintage). With FEEDBACK 0.5 the sums of echoes fit the body inside
the loop: Analogue and Vintage at 1.2 x FEEDBACK as Modern (+1.4 dB at 200 Hz, -6.2 at 4 kHz for Analogue, against
+1.6 and -6.1 predicted), Tape at 1.0 x FEEDBACK (+0.4, +3.5, +3.0 dB at 80, 300, 1k against +0.4, +3.4, +3.0).

Open: Analogue's steep stopband (Kontakt -58 dB at 16 kHz, the fit -27 dB; 5- and 6-section fits did worse in the
passband); its 1 kHz level is 1.8 dB higher with a steady tone than in the sweep (both at -20.6 dBFS, with and without
DEPTH), while its amplitude moves 4-13 % with the LFO, which points at a body that follows the bucket brigade's clock;
the modes' harmonics (Analogue's 2nd at -40 dB, Vintage's 3rd at -55 dB) and Tape's noise floor (-50 to -58 dB near the
tone); Diffusion.

## Gate: engine against the oracle, wet only

Bands are 1/3-octave levels over the whole render (bands within 60 dB of the loudest); frames are 10 ms RMS (frames
both renders hold above -70 dB). Errors are model minus oracle.

Settled frames leave out the first 30 ms of sound (the knobs land from the note's callback). Session slot 1 is left
out: its first instance starts badly in every session, whatever its setting.

| Case | band max / rms dB | settled frame max / rms dB |
|---|---|---|
| Modern DEPTH 1, RATE 0.1 / 0.2 / 0.3 | 0.08 / 0.08 each | 1.05 / 0.10, 0.92 / 0.10, 0.63 / 0.10 |
| Modern RATE 0.5, DEPTH 0.1 / 0.5 / 0.75 | 0.09 / 0.09 each | 0.12 / 0.09, 0.10 / 0.09, 0.09 / 0.09 |
| Vintage RATE 0.5 / 0.8, DEPTH 1; DEPTH 0.5 | 24.6, 25.7, 12.2 (the 3rd harmonic at 3.2 kHz, 200 Hz) | 0.15 / 0.06, 0.15 / 0.05, 0.12 / 0.05 |
| Tape FLUTTER 0.3 / 0.6 / 1 | 10.0, 14.6, 16.1 (its noise near the tone) | 0.12 / 0.07, 0.15 / 0.07, 0.23 / 0.09 |
| Analogue RATE 0.1-1, DEPTH 0.25-1 | 15.7-41.7 (the 2nd harmonic at 2 kHz) | 2.0-6.7 / 1.8 (level, see Open) |
| Analogue sweep | 23.7 (16 kHz stopband) | 36.8 / 3.9 |
| Tape sweep TAPEAGE 0.25 / 1 | 1.47 / 0.47, 1.43 / 0.49 | 12.5 / 1.5, 8.5 / 1.4 |
| Vintage sweep QUALITY 1 / 0 | 1.14 / 0.39, 5.22 / 1.12 (16 kHz) | 19.0 / 1.3, 22.0 / 4.5 |
| Tape, Vintage sweeps at FEEDBACK 0.5; Modern at 0 | 2.67 / 0.73, 1.63 / 0.49, 1.50 / 0.31 | 7.4 / 1.8, 15.7 / 2.7, 2.2 / 0.4 |

Modern meets the gate everywhere (the start phase fitted). Vintage and Tape meet it on the 10 ms frames of a tone;
their tone bands miss on what the model does not make (harmonics, noise) and their sweeps reach 1-1.5 dB with
frame maxima where a sweep's echo passes the body's slopes. Analogue does not meet it (see Open).
