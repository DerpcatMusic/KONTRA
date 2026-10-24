# Dolce onset vs sample start range (2026-10-24)

Question: does Kontakt 8 start a zone with `sample_start_mod_range` later per note (random delay), and
how much of the ~40 ms Kontakt-minus-KONTRA onset gap is capture latency rather than the start law?

## Result

- **Capture latency is about 34 ms.** Trills zones with `start_range` 0 and no start route play the same
  waveform in both (waveform r 0.97-0.99). Kontakt lags KONTRA by +32.8, +33.1, +34.6 ms (Trills CC1 0 notes,
  `xcorr.py`). This is the audio path, not the start law.
- **Dolce does not exercise the random start-delay law.** Every Dolce zone (`start_range` 3841 frames = 80 ms)
  has a SampleStart route (depth 1.0) driven by Controller 113, which is the instrument's "S. Start" slider
  (script `512e38b38165e870.ksp`, default 500000 of 1000000 = 0.5 x range = 40 ms, a latency compensation
  control). For a program with a SampleStart route, `random_delay` returns 0 (`voice_mod.rs`: `random_delay`
  checks `programs[..].start`). KONTRA's delay trace on the 32-note render: all 64 admitted voices, delay = 0.
- **After latency removal the two sides agree within about 5 ms** (median, 32 notes at key 60, vel 112):

| 32 notes, onset after note-on at -20 dB below sustain | min | median | max | span |
|---|---|---|---|---|
| Kontakt 8 (capture, latency included) | 69 | 93 | 101 | 32 |
| KONTRA (`origin/main` 20c22981b) | 36 | 53.5 | 58 | 22 |
| Difference of medians, minus 34 ms latency | | 39.5 - 34 = **+5.5** | | |

- The 8-note captures agree with this (median residual about 5 ms). The span is about 10 ms wider in
  Kontakt. Round-robin samples have different attacks, and the detector is threshold dependent, so per-note
  matching is not meaningful. Per-note start draws differ between engines anyway.

## Why no start-law change

- The residual after latency (5.5 ms) is within threshold noise. The 10 ms span difference is not a start-law
  signal, because Dolce's start is route-driven and KONTRA's random law is off for this program. Changing the
  unrouted-start scale on Dolce data would change nothing measured here.
- Correction to the earlier version of this doc: Dolce was described as a ready test case for the random
  start-delay law. It is not. It exercises the route law (CC113), which is already matched.

## Remaining test for the random law

Trills `start_range` 7680 (160 ms) zones have no SampleStart route (488 zones). They are release layers
(groups 16-31, sample starts 375966), so a note-on test at key 60 does not hit them. A release-trigger capture
(note-off at a known time) is the next measurement of the unrouted random law. Not done here.

## Method (scripts in ~/.cache/kontra-ksp/onset/)

- `onset.py WAV NOTE_ON[,..] --pre S --thr DB`: first 5 ms RMS reaching sustain (200-400 ms after note-on)
  minus DB. Threshold-dependent; use it for distribution checks, not per-note fits.
- `xcorr.py KONTAKT.wav KONTRA.wav NOTE_ON[,..] --pre S`: per-note waveform lag (+-120 ms) and correlation.
  Valid only where both sides use the same sample with the same start (r near 1).
- Captures: `tools/oracle/capture.py --midi` through `oracle-slot`; renders: `tools/oracle/kontra_render.py`
  (32-note render was run directly, `render-kontakt-midi`, because the wrapper reported 0/1 on the long file).
- The Trills latency is from existing CC1 captures (`~/.cache/kontra-ksp/cc1v2`).

## Not done

- Release-trigger capture for the unrouted random law (see above).
- Velocity dependence; all data is vel 112.
