# Kontakt Compressor (Effects/Compressor, Enhanced) level-curve pilot (2026-10-24)

Rig: `fxrig.py levels` (level-stepped 1 kHz tone, -40..0 dBFS, 1 s per level; bypassed reference in the same session).
Table: `comp-levels-pilot.tsv` (step, name, input dBFS, reference dB, output dB, delta dB).

Status: NOT a usable static curve yet.
- The stimulus plays (checked: segment RMS -43/-33/-27/-21/-15/-9/-3 dB as written).
- The -18 dBFS segment reads silent (-240 dB) in the reference in all three settings: a capture problem not yet explained.
- Deltas are not monotone in level (for example thr 0.5 / ratio 0.5: -58 dB at -40 dBFS, -16 at -30, -14 at -24, -12 at -12, -16 at 0),
  and with threshold 0.25 the output sits near -56 dB at every level. The 0.25 s measurement window is probably still inside the
  attack/settling of this preset, and the threshold/ratio knob-to-dB laws are not calibrated yet.
Next: measure the steady state after a 2 s tone, check the -18 dBFS dropout, and calibrate THRESHOLD and RATIO knob laws against
the level sweep before any fit. Dynamics parity rows are unchanged.
