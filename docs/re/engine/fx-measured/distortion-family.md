# Distortion family, measured in Kontakt 8.13.1 (2026-11-14)

Stimulus: a 1 kHz sine whose amplitude rises linearly over 5 s (`fx_lab.py ... ramp`), so each cycle's peak is a point
of the static curve; the bypassed instance is the input. Peaks are per 2 ms window; the first 2 % are skipped (ramp
start). The oracle drops audio now and then; windows reading zero are dropped.

## Distortion (`Effects/Distortion`), the engine's model against the oracle

Level error = engine peak over oracle peak, dB, at input peak 0.05 / 0.1 / 0.2 / 0.4 / 0.6 / 0.8 (DAMPING 0).

| type, DRIVE | 0.05 | 0.1 | 0.2 | 0.4 | 0.6 | 0.8 | bands max / rms | frames rms |
|---|---|---|---|---|---|---|---|---|
| Tube 0.5 | +0.8 | +1.5 | +2.4 | +0.7 | +1.6 | +3.8 | 7.1 / 4.0 | 6.4 |
| Tube 1 | +1.2 | +2.6 | +4.0 | +2.0 | +3.3 | +5.0 | 6.8 / 4.3 | 4.3 |
| Tube 0.25 | +0.1 | +0.1 | -0.0 | -0.2 | -0.4 | -0.7 | 20.7 / 5.5 | 1.9 |
| Trans 0.5 | +0.5 | -0.1 | -0.1 | +0.0 | +0.0 | -0.0 | 12.7 / 5.6 | 1.9 |
| Trans 1 | +4.6 | +2.2 | -1.9 | +0.5 | +0.2 | +0.1 | 13.6 / 7.0 | 2.4 |

(Band errors on a pure tone are dominated by its harmonics' levels, so they say how the curve's distortion products
differ: the level of the tone itself is the table's first columns.) Trans matches the oracle within 0.5 dB above 0.1
input at DRIVE 0.25 and 0.5; Tube at DRIVE 0.5 and up is 1-5 dB too loud: the model's positive branch reaches 1.0 at full
input where Kontakt 8.13.1 reaches 0.50-0.65. The engine's Tube curve was checked against an older (v1) vector, not
against this version. Oracle positive and negative branch peaks, input a to output +/-:

- Tube 0.25: 0.12: +0.097/-0.079, 0.23: +0.203/-0.191, 0.35: +0.321/-0.332
- Tube 0.5: 0.05: +0.098/-0.053, 0.2: +0.249/-0.214, 0.4: +0.383/-0.457, 0.7: +0.547/-0.573, 0.9: +0.629/-0.603
- Tube 1: 0.05: +0.185/-0.107, 0.2: +0.325/-0.307, 0.4: +0.409/-0.515, 0.7: +0.486/-0.572, 0.9: +0.502/-0.574

The small-signal positive branch at DRIVE 1 goes as 0.83 sqrt(x) (0.05: 0.185, 0.1: 0.262). No model is refitted yet.

Frequency response from a log sweep (DAMPING 0, sweep -12 dB): Tube 0.5 +2.8 dB flat 100 Hz-8 kHz then -27 dB at
12 kHz; Tube 1 +7.3 dB flat; Trans 0.5 -0.1 dB flat; Trans 1 -1.4 dB flat (level dependent, the curve's gain).

## DStortion (`Effects/DStortion`)

Output saturates at a fixed ceiling whatever the input: peak 0.022-0.024 for DRIVE 0, 0.25, 0.5 and 1 at every input
from 0.05 up (VOLUME at its INIT value); the ceiling is 0.029 at TONE 0, 0.088 at TONE 1, 0.027 at VOLUME 0.5 and 0.022 at
MONO 1. A hard clip into a volume stage. Sweep responses are the clipped signal's and move only at 50 Hz with
BASS (-10.3 dB at 0, -5.2 at 1) and +2 to +3 dB above 1 kHz with TONE 0.5; MID and TREBLE did not move the clipped
sweep. No model.

## Supercharger GT (`Effects/Supercharger GT`), SATURATION 0.5 unless noted

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
rises. The ceiling falls with the setting, so the saturator is a drive into a limiter-like clip followed by make-up that
the setting shrinks; no model.
