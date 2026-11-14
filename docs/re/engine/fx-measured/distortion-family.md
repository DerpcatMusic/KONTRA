# Distortion family, measured in Kontakt 8.13.1 (2026-11-14)

Stimulus: a 1 kHz sine whose amplitude rises linearly over 5 s (`fx_lab.py ... ramp`), so each cycle's peak is a point
of the static curve; the bypassed instance is the input. Peaks are per 2 ms window; the first 2 % are skipped (ramp
start). The oracle drops audio now and then; windows reading zero are dropped.

## Distortion (`Effects/Distortion`)

### Tube, refitted (Kontakt 8.13.1 is the reference)

`tools/kontakt-oracle/fx_curve.py` fits the curve itself from the ramp: per two-cycle window the instance's highest and
lowest values are the curve at +a and -a less its mean over the cycle (the blocker after it takes the mean out), all
linear in the curve's values at 97 points over -1..1, solved at once (error rms 0.0006-0.0019 of full scale). DRIVE 0
passes the input (within 0.002). The engine reads these curves (`crates/dsp/drive/src/tube.rs`), blended linearly
between the measured drives.

| DRIVE | +0.12 | +0.25 | +0.5 | +1.0 | -0.12 | -0.25 | -0.5 | -1.0 |
|---|---|---|---|---|---|---|---|---|
| 0.25 | +0.128 | +0.254 | +0.504 | +1.011 | -0.069 | -0.190 | -0.501 | -0.728 |
| 0.5 | +0.281 | +0.388 | +0.509 | +0.733 | -0.041 | -0.174 | -0.496 | -0.557 |
| 0.75 | +0.354 | +0.444 | +0.495 | +0.542 | -0.055 | -0.196 | -0.508 | -0.557 |
| 1 | +0.425 | +0.489 | +0.530 | +0.553 | -0.070 | -0.225 | -0.499 | -0.523 |

The positive half saturates harder with DRIVE; the negative half stays thin below 0.25 and holds near -0.55. The old
formula (v2/v1's) reached 1.0 on the positive half at full input.

The DC blocker after it is a Butterworth high-pass at 36.8 Hz, not v2's 14.96 Hz: the Tube 0.5 sweep sat 7.0, 4.6, 2.4
and 1.0 dB under the 14.96 Hz model at 25, 31, 40 and 50 Hz, which a 36.0-37.1 Hz Butterworth gives at each.

### Gate (settled frames leave out the first 30 ms; session slot 1 left out)

| Case | band max / rms dB | settled frame max / rms dB |
|---|---|---|
| Tube sweep, DAMPING 0.5 / 1 | 0.79 / 0.35 each | 5.7 / 0.66, 7.3 / 0.77 |
| Trans sweep, DAMPING 1 | 2.13 / 0.57 (25 Hz) | 11.0 / 0.89 |
| Tube ramp, DRIVE 0.25 / 0.5 / 0.75 | 10.0, 12.6, 8.6 (between harmonics) | 4.8 / 0.30, 3.4 / 0.32, 6.3 / 0.43 |
| Trans ramp, DRIVE 0 / 0.75 / 1 | 5.0, 13.7, 13.6 (2 kHz) | 3.2 / 0.24, 3.3 / 0.68, 5.2 / 1.46 |

The sweeps meet the 1 dB band gate for Tube; the ramps' frames hold the static curve (0.3-0.4 dB rms), while their bands
miss on the distortion products' levels between and on the harmonics, which the engine's 2x oversampling and the
native resampler shape differently. Trans was not refitted (its DRIVE 0.75 and 1 are 0.7-1.5 dB off per frame).

## DStortion (`Effects/DStortion`)

Modelled from the native kernel's structure (`crates/format/kontakt-effects/src/dstortion.rs`: shelf, treble and bass
one-poles, the 256-point clip table, the VOLUME/TONE network) with two measured corrections:

- The table is read at twice the kernel's gain and its output halved: at saturation the first model sat a flat 6.1 dB
  above Kontakt at every VOLUME, with the small-signal level right (0.3 dB at -57 dB input).
- The native 4x resampler's roll-off, the same at every knob setting: a fixed 22 kHz, Q 0.28 low-pass (within 0.5 dB to
  12.8 kHz, 1.1 dB at 16 kHz).

| Case | band max / rms dB | settled frame max / rms dB |
|---|---|---|
| sweeps at -60 dB, TONE 0 / 1, BASS 0 / 1, MID 0 / 1, TREBLE 0 / 1, VOLUME 1, DRIVE 0 | 1.05-1.15 / 0.21-0.32 | 1.4-15.7 / 0.28-0.75 |
| sweeps at -20 dB, DRIVE 1, TONE 0 / 1 | 0.85, 1.05, 5.13 (25 Hz) / 0.17-1.26 | 2.0-15.4 / 0.12-1.46 |
| ramps, DRIVE 0 / 0.5 / 1, TONE 0, VOLUME 1 | 0.4-18 (between harmonics) | 0.02-16.6 / 0.01-0.74 |

The sweeps meet the band gate to 1.2 dB (the 16 kHz band); the ramps' frames hold the clip curve (0.01-0.17 dB rms in
all but one case); their bands miss between the harmonics.

## Supercharger GT (`Effects/Supercharger GT`), modelled

### Where the modes are saved

Three oracle saves each of one instrument that differ only in SAT_MODE, CHAR_MODE or HPF_MODE (0, 1, 2, set by
`set_engine_par` and `save_instrument`, `probe_fx.py` with `PROBE_KEEP`), byte-diffed. The v0x51 record is 45 bytes:
floats TRIM, SATURATION, COMPRESS, CHARACTER, MIX, ATTACK, RELEASE at 0..24, then i32 HPF_MODE at 28, SAT_MODE at 32 and
CHAR_MODE at 36, each saved as `2 - mode`; an i32 2 at 40 and a byte 0 at 44 did not move. The INIT record is TRIM 0.5,
SATURATION 0, COMPRESS 0, CHARACTER 0, MIX 1, ATTACK 0.3667, RELEASE 0.4444, every mode 0.

### The saturator, measured curves

`crates/format/kontakt-effects/src/supergt.rs`: per SAT_MODE the curve at SATURATION 0, 0.25, 0.5, 0.75 and 1, each
fitted by `fx_curve.py` to a 1 kHz ramp to full scale (rms error 0.0001 of full scale or less, odd within 0.003),
stored as its positive half at the saturator's input (the base tone's 1 kHz gain, 1.0695, divided out), blended linearly
between settings (`DriveCurve::Blend`). TRIM is a gain T of 58.4 dB x (TRIM - 0.5) in and out. Hot at SATURATION 0 is
not Mild's (ceiling 0.32 against 0.63), as the first pass had assumed. MIX crossfades the dry input back in (the slot's
dry level).

| SAT_MODE | ceiling at SATURATION 0 / 0.25 / 0.5 / 0.75 / 1 |
|---|---|
| Mild | 0.63 / 0.69 / 0.25 / 0.17 / 0.11 |
| Moderate | 0.62 / 0.41 / 0.11 / 0.077 / 0.054 |
| Hot | 0.32 / 0.18 / 0.065 / 0.038 / 0.020 |

The tone: a fixed base (+0.55 dB, +0.8 dB at 67 Hz, +5 dB above 16.6 kHz) and per CHAR_MODE a low shelf, a peak and a
high shelf at CHARACTER 1, whose dB is linear in CHARACTER (0.5 sits midway in two modes).

### What a static curve cannot be (measured, not carried)

- At SATURATION 0 (every mode), and at 0.25 in Mild and Hot, part of the loss is a clean gain: at full input Kontakt's
  3rd harmonic is -58 to -77 dB under the tone where the curve gives -20 to -39 dB. From 0.5 up the harmonics match
  within 1 dB (3rd to 9th).
- COMPRESS is a compressor, not a curve: at 0.5 the output holds -20 to -19 dB for a 1 kHz tone rising 18 dB, the
  3rd harmonic at -62 to -64 dB; at 1, -21 dB with the 3rd at -57 to -39 dB.
- HPF_MODE is level-dependent, so it sits in that compressor's detector: against mode 0 the level falls by 1.5 dB
  (mode 1) and 3.0 dB (mode 2) on a -20.6 dBFS sweep but rises by 2.0 and 1.5 dB on a -60 dB sweep, flat to within
  0.4 dB from 63 Hz to 10 kHz.
- After a loud input stops, Kontakt's output holds near the curve's ceiling for 30 ms and falls to -50 dB over the
  next 100 ms (Hot, SATURATION 1); something before its saturator rings, which the model's base tone does not.

The translator reports a record with COMPRESS above 0 or HPF_MODE not 0 (`Supercharger GT compressor`).

### Gate (ramps to full scale, every SAT_MODE x SATURATION; session slot 1 left out)

| Case | band max / rms dB | frames under sound, p90 / rms dB | settled frames incl. the tail, max / rms dB |
|---|---|---|---|
| Mild 0 / 0.25 / 0.5 / 0.75 / 1 | 37.6, 22.1, 17.2, 18.7, 18.9 / 4.6-10.9 | 0.70, 0.66, 0.28, 0.21, 0.19 / 0.14-0.46 | 16-31 / 0.89-1.90 |
| Moderate 0 / 0.25 / 0.5 / 0.75 / 1 | 23.8, 11.0, 3.7, 10.0, 14.2 / 1.2-6.9 | 0.68, 0.45, 0.14, 0.40, 1.01 / 0.10-0.99 | 17-37 / 0.91-2.56 |
| Hot 0 / 0.25 / 0.5 / 0.75 / 1 | 24.9, 27.1, 23.6, 27.8, 35.0 / 8.9-12.2 | 1.14, 1.17, 0.72, 0.64, 0.94 / 0.80-1.58 | 18-58 / 1.42-4.20 |
| sweep -20.6 dBFS / -60 dB, HPF_MODE 0 | 0.19 / 0.05, 3.12 / 0.66 | 0.23, 3.67 | 5.5 / 0.42, 26 / 2.1 |

Frames under sound (from 30 ms after the input starts to where it ends) meet the 1 dB gate in 12 of 15 settings
(Hot 0 and 0.25 and Moderate 1 reach 1.0-1.2 dB at p90); the band misses are the harmonics above (the clean gain at
SATURATION 0) and the ramp's high harmonics, and the frame maxima are the tail after the input stops.

### Peaks from the first pass, SATURATION 0.5 unless noted

| setting | peak at input 0.05 / 0.2 / 0.5 / 0.9 |
|---|---|
| SAT_MODE 0 | 0.079 / 0.213 / 0.249 / 0.252 |
| SAT_MODE 1 | 0.082 / 0.106 / 0.106 / 0.109 |
| SAT_MODE 2 | 0.053 / 0.064 / 0.065 / 0.065 |
| SATURATION 1, SAT_MODE 2 | 0.019 / 0.020 / 0.020 / 0.021 |
| SATURATION 0 | 0.052 / 0.199 / 0.418 / 0.563 |
| SAT_MODE 1, CHAR_MODE 1 | 0.079 / 0.106 / 0.106 / 0.107 |
| SAT_MODE 1, CHARACTER 1 | 0.062 / 0.088 / 0.091 / 0.110 |
| SAT_MODE 1, CHARACTER 1, CHAR_MODE 2 | 0.088 / 0.163 / 0.198 / 0.210 |

Mild, Moderate and Hot saturation are three ceilings (0.25, 0.11, 0.065 at SATURATION 0.5) that fall as SATURATION
rises.
