# Replika Delay, measured in Kontakt 8.13.1 (factory `Effects/RDelay`, 2026-11-14)

Rig: `tools/kontakt-oracle/fx_lab.py` renders an effect at several settings and keeps the raw audio; `fx_rdl.py`
analyses; `fx_export.py` + `fx_gate.py` + the ignored test `fit` in `crates/format/kontakt/src/fit.rs` hold the
engine's model against the render (1/3-octave band level, 10 ms frame RMS). Knobs are set late from a note callback;
`RDL_TIME_UNIT` 0 before `RDL_TIME`. The oracle slot owns 5 MIDI channels, so one session holds the reference and 4
settings; a session loaded machine drops audio now and then (digital-silence stretches), so every number below is
from stretches without dropouts.

TYPE values: Modern 0, Analogue 1, Tape 2, Vintage 3, Diffusion 4. TIME raw 100000 (200 ms), FEEDBACK 0.

## DEPTH and RATE (the user's method: a steady 1 kHz sine, the dry line cut from the spectrum, the wet signal's analytic
phase, delay(t) = -dphi / 2 pi f)

| Type | DEPTH | RATE | delay swing p-p | rate |
|---|---|---|---|---|
| Modern | 0.25, 0.5, 1 | 0.5 | 4.00, 8.00, 16.00 ms | 1.000 Hz |
| Modern | 1 | 0.2, 0.5, 0.8 | 16.06, 16.00, (12.0, unwrap) ms | 0.25, 1.000, 3.98 Hz |
| Analogue | 0.25, 0.5, 1 | 0.5 | 7.9, 15.8, 31.6 ms | 1.000 Hz |
| Analogue | 1 | 0.2, 0.5, 0.8 | 37.6, 31.6, 8.6 ms | 0.25, 1.000, 3.98 Hz |
| Vintage | same as Analogue to 0.01 ms | | | |
| Tape | 0.5, 1 | any | 0.57 ms, not moved by DEPTH or RATE | 0.88 Hz (flutter) |
| Diffusion | 0.5, 1 | any | no single tap (smeared) | |

Modern: a sine, +-8 ms x DEPTH, 0.1 x 100^RATE Hz. Wired (`replika.rs`). Analogue and Vintage swing +-15.8 ms at 1 Hz
and 4.3 ms at 4 Hz (a constant +-10 % pitch above 1 Hz, capped near 19 ms below), non-sinusoidal (third harmonic
0.18-0.47); not wired, because their echo (below) is not modelled either.

## The oracle's modulation starts anywhere, and its delay grows first

The Modern LFO phase at the tone's first sound was the same at the same rate (0.397 s of LFO at 1 Hz) and the engine's
delay line grows as its LFO falls, so the gate fits the start phase (`pre`, seconds) per case.

## Echo of an impulse and the frequency response of the echo (sweep, dB re the dry sweep)

| Type | first echo | level | response (100 / 1k / 4k / 8k / 12k / 16k Hz) |
|---|---|---|---|
| Modern | 197.2 ms | +0.3 dB | 0 / +0.1 / - / - / - / - (sweep dropped out above 2 kHz) |
| Analogue | about 190 ms | wet energy -60 dB re the dry impulse in the 190 ms frame, flat -80 dB floor after | -0.8 / -1.4 / -6.7 / - / - |
| Tape | 190.8 ms | -7.9 dB | +0.6 / +1.1 / -3.6 / -10.2 / -17.8 / -28.0 |
| Vintage | 190.5 ms | -4.7 dB | +0.4 / -0.2 / -1.2 / -3.9 / -8.4 / -16.6 |
| Diffusion | cluster from 190 ms | -9 dB in the first frame, then -3.5 dB per 100 ms to -24 dB at 620 ms (about 1.7 s to 60 dB) | -0.4 / -5.9 / -6.0 / -5.5 / -6.9 / -9.1 |

The Analogue impulse carries almost nothing (the echo is band-limited, so a one-sample click is the wrong stimulus for
it; use tones and sweeps). Not modelled: Analogue, Tape, Vintage, Diffusion bodies.

## Gate: engine against the oracle (1 kHz tone, dry + wet, Modern, TIME 200 ms, FEEDBACK 0)

Bands are 1/3-octave levels over the whole render (bands within 60 dB of the loudest); frames are 10 ms RMS (frames
both renders hold above -70 dB). Errors are model minus oracle.

| DEPTH, RATE | band max / rms dB | frame max / rms dB |
|---|---|---|
| 1, 0.2 (0.25 Hz) | 4.85 / 1.99 | 13.77 / 2.72 |
| 1, 0.5 (1 Hz) | 0.11 / 0.06 | 4.93 / 1.25 |
| 1, 0.8 (3.98 Hz) | 0.82 / 0.48 | 4.56 / 0.58 |
| 0.25, 0.5 | 0.14 / 0.06 | 11.75 / 2.62 |
| 0.5, 0.5 | 2.47 / 1.86 | 5.35 / 1.81 |

The 1 dB gate is met for the bands at DEPTH 1 and RATE 0.5-0.8 and DEPTH 0.25; not for the slow rate and DEPTH 0.5.
The frame maxima sit in dry/wet cancellations of a 1 kHz tone, where a phase error of a few degrees moves the level
by several dB. The modulation's measured delay swing, pitch deviation and wet amplitude agree with the oracle's to the
digit (model 16.00 ms and +-55 Hz against 16.00 ms and +-55 Hz at RATE 0.5).
