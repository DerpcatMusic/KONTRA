# Kontakt Compressor (Effects/Compressor, Enhanced) level-curve pilot (2026-10-24)

Rig: `fxrig.py levels` through `oracle-slot`: 1 kHz tone, 7 levels (-40..0 dBFS, 1 s each), bypassed reference in the same session.
Table: `comp-levels-pilot.tsv` (step, name, level, reference dB, output dB, delta dB). The earlier dropout at -18 dBFS was the
MIDI-channel bug; all seven levels now measure.

Result of the grid fit (Rust probe, `KontaktEnhanced`, threshold dB x saved ratio, delta RMS against the native deltas):

| threshold knob | ratio knob | best threshold dB | best saved ratio | RMS error |
|---|---|---|---|---|
| 0.50 | 0.50 | -52 | 0.75 | 2.6 dB |
| 0.25 | 0.50 | -56 | 0.15 | 1.0 dB |
| 0.75 | 0.25 | -30 | 0.70 | 0.5 dB |

Not a fit yet. The three settings want different ratio mappings (0.5 -> 0.75 and 0.25 -> 0.15), so the knob-to-ratio law of
the Enhanced law (`1/(r+0.13) - 0.884`) does not match this data. The threshold knob to dB map is not linear in these three
points (0.25 -> -56, 0.5 -> -52, 0.75 -> -30). The 0.25 s measurement window also sits on the attack/release settling. Next:
a threshold grid at fixed ratio (5 knobs), a ratio grid at fixed threshold (5 knobs), and the same levels after a 2 s tone.
Dynamics parity rows are unchanged.
