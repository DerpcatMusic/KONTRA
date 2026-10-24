# Kontakt Compressor (Enhanced) attack and release (2026-10-24)

Rig: `fxrig.py burst` through `oracle-slot`. Tone burst at -12 dBFS, 1 kHz: 2.5 s loud, then 2.5 s at -50 dBFS (below the
threshold). Threshold and ratio are held at the burst chain's fixed settings (see `burst_curve` in `fxrig.py`); bypassed reference in the same session. Attack
and release are measured on 1 ms RMS bins, gain = active minus reference.

Definitions: attack = time from tone onset to 63% of the gain at 2 s; release = time from the end of the loud tone to
63% recovered (the 1/e time constant reading).

## Attack knob (`COMP_ATTACK`), release knob 0.3 (`COMP_DECAY`)

| attack knob | attack 63% (ms) | gain 0.5 s / 1.0 s / 2.0 s (dB) | release 63% (ms) | status |
|---|---|---|---|---|
| 0.00 | 1 | -13.16 / -13.16 / -13.16 | 144 | ok |
| 0.25 | 6 | -116.67 / -16.10 / -16.10 | - | glitch: -116.67 dB bin at 0.5 s |
| 0.50 | 27 | -17.67 / -17.67 / -17.67 | 167 | ok |
| 0.75 | 116 | -18.88 / -18.92 / -18.92 | 273 | ok |
| 1.00 | 482 | -12.87 / -17.90 / -19.91 | 1201 | not settled at 2 s (2 dB still moving) |

## Release knob (`COMP_DECAY`), attack knob 0.1

| release knob | release 63% (ms) | status |
|---|---|---|
| 0.00 | 46 | ok |
| 0.25 | - | glitch: -116.67 dB bin at 0.5 s (same artefact as attack 0.25) |
| 0.50 | 329 | ok |
| 0.75 | 868 | ok |
| 1.00 | 2287 | ok |

The -116.67 dB bin is a single 1 ms bin of near-silence at 0.5 s into the tone, in both sessions at knob 0.25, so it is
probably a Kontakt render dropout, not compressor behaviour. Rows with it are not used.

Files: `comp-burst-attack.tsv`, `comp-burst-release.tsv`, specs `comp-burst-release.spec.json`.

Not done: the attack and release laws are not yet compared with the `COMP_ATTACK` / `COMP_DECAY` tables
(`crates/sampler-ir/src/fx/tables.rs`); the 2 s steady check is only done at the attack knob 1.0 setting, where it fails.
Also: the first 11-instance session lost the sound for instances 6 and up (release grid), so the release grid came
from a separate 6-instance session.
