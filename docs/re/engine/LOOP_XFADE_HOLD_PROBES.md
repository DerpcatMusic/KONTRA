# Loop crossfade law and release-trigger hold probes (2026-10-24)

Oracle probes against real Kontakt 8 via `tools/oracle/oracle-slot` (plain slot, not `--exclusive`), compared
with KONTRA renders of the same MIDI (`tools/oracle/kontra_render.py`). Renders from one sampler-native build.

## 1. Loop crossfade law: linear (measured, no change)

Instrument: Audio Imperia CHORUS "09 Ensemble - Sustained Oh" (loop slot start 88800, end 242400, crossfade
19200 frames = 400 ms, length 153600). Ensemble legs are uncorrelated, so the two candidate laws differ clearly.
Note: key 60, velocity 100, held 14 s. Seam windows at 5.25, 8.45 and 11.65 s after slot start.

| | seam 0 | seam 1 | seam 2 |
|---|---|---|---|
| residual, Kontakt minus linear KONTRA | -69.5 dBFS | -70.3 dBFS | -29.2 dBFS (Kontakt dropout) |
| residual, Kontakt minus equal-power KONTRA | -40.3 dBFS | -40.2 dBFS | -27.4 dBFS |

Waveform correlation at sustain: 0.99997 (lag -17.2 ms). Kontakt's 50 ms envelope matches the linear render to
0.1 dB across the fade. Equal-power sits 2 to 3 dB above it mid-fade. **Kontakt's loop crossfade is the linear
complementary law already implemented (`LoopShape::Crossfade`).** No engine change.

Seam 2: Kontakt is near silent from about 11.2 s while both renders hold level. Not explained; it does not affect
the law, which is fixed on seams 0 and 1.

## 2. Release-trigger gain vs hold time (open: KONTRA plays nothing on note-off)

Instrument: Performance Samples Vista "3 Violins FFF Overlay" (100 gate-release zones, 24 release-counter refs).
Notes on keys 60, 64, 67, 60, holds 0.3, 1, 2, 4 s, 5 s apart.

- Kontakt plays a release tail after each note-off (-51 to -72 dB in the first 0.5 s after note-off; the tail is
  not a clean function of hold time in this capture).
- KONTRA renders exact digital silence in those windows. Its note-on sound is about 22 dB quieter than Kontakt's
  overall peak.
- Keys 60, 64, 67 are covered by gate-release zones in group "cl legreldyn1" (`start = RoundRobin(1)`,
  `monophonic = false`). The mismatch is likely the round-robin counter of that group under sparse notes, but
  this is not measured; the RR semantics of release groups are not settled from the binary.

Next step: a second probe with the same instrument and notes where the RR counter is advanced explicitly (several
notes before each release), plus a check of KONTRA's group RR state after one note. Do not change the release law
until that is measured.

## Pitfall found (fix in the oracle tooling, not here)

`capture.py --midi FILE` in single mode writes the MIDI on channel 1, but in an oracle slot the instrument listens
on the slot's channel block (`slot.channel_block()`). In slot 2 or 3 the capture is silent. Build custom MIDI on the
slot's first channel (see `tools/oracle/slot.py`) or run `oracle-slot bash -c 'python3 mk.py ... && capture.py ...'`.
