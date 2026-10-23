# Kontakt Raum vs KONTRA: impulse-response decay (2026-10-24)

Method: `tools/kontakt-oracle/stimrender.py` with the `impulse8` stimulus (8 s, one 0.5 impulse), Kontakt 8.13.1
under `flock /tmp/kontra-oracle.lock`. Ours: `sampler-core` `dsp/space` test `raum_ir_dump`
(`RAUM_CORE`, `RAUM_IR_OUT`), same 20-value core as `sampler-kontakt` `read_raum`. Decay is a Schroeder EDC
from 20 ms after onset; T20 = -5..-25 dB, T30 = -5..-35 dB, both doubled/tripled as usual.

Kontakt `RAUM_*` engine parameters written with `set_engine_par` did not change the output, so each state is a
factory preset file (nkp), decoded with `nkp_decode dump`.

## Results (broadband, T30)

| Preset | Type | Decay knob | Kontakt T30 | Ours T30 | Dev |
|---|---|---|---|---|---|
| INIT Raum | Airy | 0.639 | 4.25 s | 4.23 s | -0.4 % |
| Small / Boxed In | Grounded | 0.245 | 1.16 s | 1.23 s | +6.8 % |

Octave bands (T20) for INIT (Airy): 60-250 Hz +2.3 %, 250-1k -0.5 %, 1-4k -1.7 %, 4-16k -2.1 %. All within the
+-10 % time-constant band.

Boxed In (Grounded) mid bands match (250-1k -2 %, 1-4k 0 %). Low (60-250 Hz) and high (4-16k) bands do not:
Kontakt's 0.1 -> 1 s slope is 28 dB/s (LF) and 21 dB/s (HF) against ours 43 dB/s and 43 dB/s. Its tails reach
the capture's noise floor (about -120 dBFS), so the T20 numbers below -110 dB are not reliable. The Grounded
mismatch is a candidate for the low-shelf (0.249), high-cut (0.126) or damping (0.4) laws. It is NOT isolated yet:
no capture varies one control at a time.

## Regression

`raum_airy_init_decay_matches_kontakt_capture` pins the INIT broadband T30 to 4.25 s +-10 %.

## Open

1. Isolate low-shelf, high-cut and damping on Grounded (needs factory presets that differ in one control, or a
   working set_engine_par path for RAUM_*).
2. Arcadia Dream Hall (Airy, decay 0.599, expected T30 about 4.1 s) and Big Roon (Grounded, decay 0.220, expected
   about 1.0 s) were queued behind other oracle sessions and are not in this note.
3. Cosmic (type 2) is not measured: Cosmic Keys has decay 0.805, which is beyond an 8 s window.
