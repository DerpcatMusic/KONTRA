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

`crates/format/kontakt-effects/src/supergt.rs`: `(C/T) tanh(s T x / C)` with ceiling C and slope s per SAT_MODE and
SATURATION, fitted per ramp (`fx_curve.py`, then tanh, 0.2-2 % of full scale):

| SAT_MODE | SATURATION 0 | 0.25 | 0.5 | 1 |
|---|---|---|---|---|
| Mild | C 0.652, s 0.95 | 0.677, 1.22 | 0.249, 1.64 | 0.114, 1.82 |
| Moderate | (Mild's) | 0.415, 1.38 | 0.107, 2.10 | 0.0515, 1.58 |
| Hot | (Mild's) | 0.170, 1.06 | 0.0649, 1.43 | 0.0203, 0.65 |

TRIM is a gain T of 58.4 dB x (TRIM - 0.5) in and out (TRIM 1 brings Moderate's 0.107 to 0.0037; TRIM 0 leaves a slope
of 1.9 to a hard clip at 1.0, not carried). CHARACTER moves a tone whose dB is linear in it (0.5 sits midway in two
modes): a fixed base (+0.55 dB, +0.8 dB at 67 Hz, +5 dB above 16.6 kHz) and per CHAR_MODE a low shelf, a peak and a high
shelf. Where SAT_MODE, CHAR_MODE and HPF_MODE are saved is not known: scripts set them; a saved instrument plays Mild.

| Case | band max / rms dB | settled frame max / rms dB |
|---|---|---|
| sweeps at -60 dB, CHARACTER 0, 1 in each CHAR_MODE, 0.5 | 0.53-1.69 / 0.42-0.81 | 13.7-30.2 / 0.95-1.83 |
| sweeps, HPF_MODE 1 / 2 (not carried) | 1.27, 2.77 | 20.2 / 1.47, 16.9 / 2.73 |
| ramps, every SAT_MODE at SATURATION 0.25-1, TRIM 0 / 1 | 4.9-40 (harmonics) | 13.8-56 / 1.0-4.0 |

The tone meets the band gate within 1.7 dB; the saturator's frames are 1-2.3 dB off (tanh's knee against the
measured curves; Hot at SATURATION 1 4 dB), so the next step is the measured curves as tables, blended as Tube's are.

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
