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

## Saturation and noise, wet only (1 kHz ramp to full scale and a steady -20.6 dBFS tone, FEEDBACK 0)

Each mode bends what it writes into its line; `fx_curve.py` fits that curve to the ramp (rms error 0.0009, 0.0001 and
0.0012 of full scale for Analogue, Vintage, Tape), and the engine's delay carries it as `shape` on the line's input,
the body's 1 kHz gain divided out.

| Type | curve at -1 / +1 | 2nd / 3rd harmonic, ramp at -22 dB / -2 dB | noise re the tone, 100-700 Hz, at -19 / -13 / -7 / -2 dB |
|---|---|---|---|
| Analogue | -0.97 / +0.85 (asymmetric) | -41 / -32, -63 / -28 dB | -60, -63, -66, -70 |
| Vintage | -0.51 / +0.51 | none, -58 / -16 dB | none (below -92) |
| Tape (TAPEAGE 0.25) | -0.72 / +0.72 | -80 / -83, -69 / -26 dB | -56, -55, -56, -58 |

- The noise is heard only under sound: the tail after a tone holds -124 dB (Analogue) and -102 dB (Tape). Analogue's
  grows as the square root of the level, as a bucket brigade's compander's does; Tape's with the level. Both are white
  where they are written, the body shapes them (Analogue's falls about 40 dB from 1 kHz to 12-20 kHz). The engine writes white
  noise of `level x rms^power` of the line's input over 10 ms: Analogue 1.84e-3, power 1/2; Tape 8.2e-3, power 1.
- RDL_NOISE and RDL_SATURATION at 0 move none of it (Analogue and Tape render as the preset within 1.4 dB at every
  harmonic and the noise).
- Analogue plays a steady 1 kHz tone 0.4 dB over its input, against -1.3 dB at 1 kHz in the sweep (both -20.6 dBFS, no
  drift over 12 s, the same at TIME 100, 200 and 400 ms within 0.6 dB), so no time-invariant body plays both; the curve
  follows the tone (its small-signal slope 1.19). TIME does not move the steep cut much (-8.2 to -13.5 dB at 10 kHz at
  400 and 100 ms), so the body does not follow the bucket brigade's clock.

### Analogue: steady tones against the sweep (Kontakt 8.13.1, 2026-11-15)

`fx_lab.py` `levels` (1 kHz, -48 to -8 dB peak) and `steps`/`stepslo` (0.5 s tones from 63 Hz to 16 kHz, 40 dB apart: about -20 and
-60 dB at the effect), FEEDBACK 0, DEPTH 0, wet only:

- Level does not move it at 1 kHz: -0.05, -0.06, -0.07 and -0.11 dB re the input at -48, -38, -26 and -20 dB peak
  (two of seven steps lost to capture gaps). So the 1.5 dB tone/sweep split is steady against moving, not loud against
  quiet.
- Steady tones, dB re the input at the low and high level:

  | Hz | 63 | 1k | 2k | 4k | 6k | 8k | 10k | 12k | 14k | 16k |
  |---|---|---|---|---|---|---|---|---|---|---|
  | -60 dB | -0.6 | -0.1 | -1.0 | +0.7 | +4.8 | +7.2 | -21.6 | -27.6 | -74.7 | -87.0 |
  | -20 dB | 0 | 0 | 0 | +3 | +8 | +11 | -20 | -25 | -71 | -91 |

  Each step is at its level from its first 10 ms frame (no build-up); the sweep at -20 dB reads -6.5, -8.9 and -21.9
  dB at 4, 8 and 10 kHz. So above 2 kHz a steady tone plays up to 20 dB over the same frequency swept, louder the louder
  it is. Vintage and Tape steps match their sweeps, so it is Analogue's own (its compander: the noise grows as
  sqrt(level)). Not modelled: one time-invariant body cannot play both, and which a held note hears (the steps) against a
  moving one (the sweep) needs a probe that separates them (two tones, or a slow and a fast sweep).

### Analogue from the binary (2026-11-15)

TYPE index 0.3 runs head mode 1 with write lane mode 3 (`141aca670`, `141aca4a0`, design `141abbc90` / `141ac3fd0`,
ported bit-exact in v2 `sampler-core/src/dsp/replika/{mode1,design}.rs` at e80a9f22b^; see k-replika-gaps.md GAP-C):

- Write side (`141aa3c60`): a feedback compressor, gain `10^(−e/20)` from the smoothed (`exp(−75π/fs)`, 4.2 ms)
  dB envelope `e` of its own output `|g·x| + 1e-4`: 2:1, the line holds `√level`. Then MT dither (0.01), the
  shaper (in 0.6, ADAA 0.2, out 1.6) and two TPT low-passes: 8 kHz damping 0.5 (+6 dB peak) and 7 kHz damping 1.3.
- Read side (`141aa3e00`): a band-pass at 150 Hz, 8 octaves wide (damping 15.94, times its damping: unity at the
  centre, −3 dB at 9.4 Hz and 2.4 kHz), times `10^(e/20)` of the smoothed (`exp(−50π/fs)`, 6.4 ms) dB envelope of
  the line's output: a 1:2 expander.
- The compander explains the noise: what the line adds is expanded with the signal, so it grows as √level and
  closes in silence (the −124 dB tail).
- The design once (`|BP|·|H_write|`) predicts the sweep at 4, 8, 10 kHz: −3.8, −8.5, −17.7 dB against −6.5, −8.9,
  −21.9 measured. A compander whose expander hears the write filter predicts it squared for a held tone (−1.8, −6.1,
  −22.8 at 4, 8, 10 kHz); Kontakt's steps read +3, +11, −20 at −20 dB. So neither is the steady-tone excess, which
  grows with level and was read broadband: the bucket brigade's images (its clock) are the candidate. The fitted
  body stays (0.45 dB rms on the sweep, better than the design's sections) until the probe (two tones; a slow
  against a fast sweep) separates images from gain.

Probe (2026-11-15, `pairsweeps`, TIME 200 ms, FEEDBACK 0, wet; dB re the input): a 0.5 s and a 12 s sweep agree
within 0.5 dB to 8 kHz (fast/slow: −1.3/−1.4 at 1 kHz, −7.1/−6.6 at 4 kHz, −10.9/−9.1 at 8 kHz, −27.8/−25.4 at
10 kHz), so speed does not move it. With 1 kHz and 7.3 kHz together, 7.3 kHz plays −8.1 dB, as the sweep: the
steps' +11 dB at 8 kHz does not reproduce, and the fitted body stands. 1 kHz plays +1.6 dB beside its partner
against −1.4 dB alone (the compander: about 3 dB of level interaction, not modelled), with intermodulation at
−38 dB (±100 Hz about 1 kHz) and −45 dB (7.3 kHz ± 1 kHz). Modern, the control, holds everything within 0.2 dB
and nothing above −68 dB.

Open: Analogue's sweep (1.5 dB over it everywhere, from the tone/sweep split above) and its stopband (Kontakt -46 dB at
12 kHz and -58 dB at 16 kHz, the body -27 dB at 16 kHz); Tape's sidebands within 100 Hz of a tone, 11 dB over its noise
floor (a random flutter); Diffusion.

## Gate: engine against the oracle, wet only

Bands are 1/3-octave levels over the whole render (bands within 60 dB of the loudest); frames are 10 ms RMS (frames
both renders hold above -70 dB). Errors are model minus oracle.

Settled frames leave out the first 30 ms of sound (the knobs land from the note's callback). Session slot 1 is left
out: its first instance starts badly in every session, whatever its setting.

| Case | band max / rms dB | settled frame max / rms dB |
|---|---|---|
| Modern DEPTH 1, RATE 0.1 / 0.2 / 0.3 | 0.08 / 0.08 each | 1.05 / 0.10, 0.92 / 0.10, 0.63 / 0.10 |
| Modern RATE 0.5, DEPTH 0.1 / 0.5 / 0.75 | 0.09 / 0.09 each | 0.12 / 0.09, 0.10 / 0.09, 0.09 / 0.09 |
| Vintage tone, DEPTH 0 (was 24.6 at DEPTH 1 without the curve) | 1.32 / 0.94 | 0.05 / 0.05 |
| Tape tone, DEPTH 0 (was 10.0-16.1 without the noise) | 3.01 / 2.13 (sidebands within 100 Hz) | 0.06 / 0.03 |
| Analogue tone, DEPTH 0 (was 15.7-41.7 and 2.0-6.7 / 1.8 frames) | 1.72 / 0.97 | 0.18 / 0.14 |
| Ramps to full scale: Analogue / Vintage / Tape | 32.7 (above 10 kHz, the stopband) / 13.2, 0.69 / 0.26, 1.99 / 0.67 | 10.1 / 0.72, 4.5 / 0.41, 35.9 / 1.65 |
| Analogue sweeps at TIME 100 / 400 / 200 ms | 26.5, 23.6, 20.5 (16 kHz) | 37-60 / 4.1-8.9 |
| Analogue sweep | 23.7 (16 kHz stopband) | 36.8 / 3.9 |
| Tape sweep TAPEAGE 0.25 / 1 | 1.47 / 0.47, 1.43 / 0.49 | 12.5 / 1.5, 8.5 / 1.4 |
| Vintage sweep QUALITY 1 / 0 | 1.14 / 0.39, 5.22 / 1.12 (16 kHz) | 19.0 / 1.3, 22.0 / 4.5 |
| Tape, Vintage sweeps at FEEDBACK 0.5; Modern at 0 | 2.67 / 0.73, 1.63 / 0.49, 1.50 / 0.31 | 7.4 / 1.8, 15.7 / 2.7, 2.2 / 0.4 |

Modern meets the gate everywhere (the start phase fitted). Vintage, Tape and Analogue meet it on the 10 ms frames of a
tone; with their curves and noise Vintage's tone bands meet it too (1.3 dB at most), Analogue's reach 1.7 dB and
Tape's 3.0 dB (its sidebands). The sweeps reach 1-1.5 dB (Tape, Vintage) with frame maxima where a sweep's echo passes
the body's slopes; Analogue's sweeps do not meet it (see Open).
