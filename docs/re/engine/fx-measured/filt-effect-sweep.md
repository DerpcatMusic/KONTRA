# Filters/Effect native sweep (Kontakt 8, 2026-10-24)

Source: `fxrig.py sweep` on `Filters/Effect` (84 steps: cutoff grid at resonance 0, resonance 0.5 and 0.9 at cutoff 0.5 for
SV LP1/HP1/BP2/BP4/LP4/HP4/Notch4/LP6, and the legacy 1/4/6-pole types). Table: `filt-effect-sweep.tsv`
(bypass-referenced dB at 30..15000 Hz, LL pair), spec: `filt-effect-sweep.spec.json`.

Fixed (2026-10-24): the silent rows were the MIDI channel bug (fixed by oracle-slot / fxrig channel block). The 23 rows that read
-100 dB or lower were rerun (`filt-effect-sweep-rerun.spec.json`, `oracle-slot`) and replaced in `filt-effect-sweep.tsv`.
Their responses are plausible (for example SV HP4 cutoff 0.5 at resonance 0.5 has a peak near 1.5 kHz; SV LP4 cutoff 0.25 is -3.6 dB at
30 Hz and -52 dB at 15 kHz). The HP at cutoff 1.0 reads -124 dB below 1 kHz: a genuine high-pass at the top of the range, not a capture
fault.

Compared 2026-10-24 (SV rows, 56) against the Wine-oracle capture the model is gated on (`sv_response_vectors.tsv`,
same names, same probes):

- Agree within 0.01 dB: SV LP1, SV HP1, SV BP2 (6 of 7; the res 0.9 row differs), SV HP4 (6 of 7), SV BP4 at cutoff >= 0.5
  and res 0, SV LP4 and SV LP6 at cutoff >= 0.75 and res 0.
- Disagree by 7 to 172 dB at the probes: SV LP4 (5 of 7), SV LP6 (7 of 7), SV BP4 (4 of 7), SV NOTCH4 (4 of 7), and the
  res 0.5 / 0.9 rows of the 4-pole types. The disagreement includes original rows, not only the 23 reruns.
- The model passes its own gate on the old capture, so the sweep is the suspect side. Likely cause, not verified: the
  impulse window (`impulse_fft`, first sample above threshold after the onset) on steep responses with long group delay.
  Next step: re-extract from the raw WAVs with the peak-centred window the capture test uses (`cascade.rs`
  `sv_responses_match_the_kontakt_capture`), then compare again. The legacy POLE rows (LP1POLE, HP1POLE, LP4POLE, LP6POLE)
  have no class in the model yet and are not compared.

Parity rows are unchanged.
