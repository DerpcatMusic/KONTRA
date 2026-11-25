# Legacy Reverb (0x15), measured in the reference 8.13 (partial: 6 of 9 settings null below −33 dB)

Renders: `tools/kontakt-oracle/stimrender.py` (Effects/Reverb in insert slot 0, a 0.5 impulse into the left input;
the same impulse on an effect-free second instrument on R is the in-session reference), one setting per session,
2026-11-25. Wet = L − R. Base: size, colour, damping 0.5, stereo 1, predelay 0. KONTRA: the `legacy_reverb` case of
the fit rig (`crates/format/kontakt/src/fit.rs`), the same input. Null = wet residual over 60 ms .. 1.25 s, dB
re the reference's wet.

## What the renders showed

- Each comb's first echo arrives 35.44 ms after the comb length (60.71 ms for the 25.31 ms comb), and the next
  arrivals are second comb trips plus 35.44 ms: the four allpasses (12.61 + 10 + 7.73 + 5.10 ms) act as plain
  delays, with no recirculation. The model ran them at gain 0.5, which put the first echo at 25.29 ms.
- With plain-delay allpasses the wet is the model's × 3.0000 (damping 1: a least-squares gain of 3.0000 and a
  −88.6 dB null). The 2.975 that settings with an open corner fit is that gain plus the corner residue below.
- The insert passes the dry at unity next to the wet (L at the impulse equals R).

## Gate after the change

| Setting | null before | null after | wet level after |
|---|---:|---:|---:|
| base | −0.7 | −33.6 | +0.07 |
| size 0 | −0.7 | −33.5 | +0.07 |
| size 1 | −0.7 | −34.2 | +0.06 |
| colour 1 | −0.7 | −33.5 | +0.07 |
| damping 1 | −0.7 | −85.9 | +0.00 |
| colour 0 | −0.7 | −18.0 | +0.07 |
| damping 0 | −0.7 | −3.3 | −0.00 |
| stereo 0 | −0.7 | +1.0 | +3.54 |
| predelay 0.5 | −0.7 | +3.0 | +0.20 |

`legacy-reverb.tsv` pins the passing five (first echo frame, wet level of four windows);
`crates/format/kontakt/tests/legacy_reverb_measured.rs` holds KONTRA to it within 0.3 dB.

## Still open

- Predelay: knob 0.5 (17.1 ms on the display) delays the first echo 59.7 ms, not 17.1.
- Stereo 0: the wet is 3.5 dB louder than the reference's; the narrowing law is not `(1 − stereo)` toward the mean.
- Colour 0 (comb one-pole keeping 0.65) nulls at −18 dB only; damping 0 (the corner at its top) at −3.3 dB.
