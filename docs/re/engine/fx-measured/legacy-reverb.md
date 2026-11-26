# Legacy Reverb (0x15), measured in the reference 8.13 (all 10 settings verified)

Renders: `tools/kontakt-oracle/stimrender.py` (Effects/Reverb in insert slot 0, a 0.5 impulse into the left input;
the same impulse on an effect-free second instrument on R is the in-session reference), 2026-11-25 and 2026-11-26.
Because the stimulus is a one-sample impulse at frame 0, pure wet is `L[2..]` (and `R[2..]` for stereo narrowing
below 1.0, where the right channel also carries wet reverb). Base: size, colour, damping 0.5, stereo 1, predelay 0.
Null = wet residual over 60 ms (+ predelay) .. 1.15 s, dB re the reference's wet.

## What the renders showed

- Each comb's first echo arrives 35.44 ms after the comb length (60.71 ms / frame 2914 for the 25.31 ms comb), and
  the next arrivals are second comb trips plus 35.44 ms: the four allpasses (12.61 + 10 + 7.73 + 5.10 ms) act as
  plain delays, with no recirculation.
- With plain-delay allpasses the wet is the model's × 3.0000 (0.045 total input gain; damping 1 nulls at −85.9 dB).
- The insert passes the dry at unity next to the wet (`L[0] == R[0]`).
- Input low-pass cutoff uses the same 8-term Taylor series (`FormulaId::LegacyCutoffHz`) as the legacy filters, not
  exact `exp(6.15x) - 1`: switching `legacy_reverb.rs` to `LegacyCutoffHz` moves `base` from −33.6 dB to −54.2 dB,
  `colour 0` from −18.0 dB to −54.9 dB, and `damping 0` from −3.3 dB to −52.0 dB (corner 2562 Hz at damping 0.5 and
  21 830 Hz at damping 0).
- Stereo narrowing mixes `[L, R]` toward the mean by `1 - stereo` before the comb banks: on a left-only impulse `[1, 0]`,
  `stereo 0.5` delivers `0.75` to L (−2.50 dB) and `0.25` to R (−12.04 dB), nulling at −54.2 dB L / −54.3 dB R;
  `stereo 0` delivers `0.50` to both L and R (−6.02 dB), matching both channels (the earlier `+3.54 dB` discrepancy
  came from feeding mono `[v, v]` and subtracting `L - R`, which mixed `-wet_R` into `wet_L`).
- Predelay knob 0.5 (`RV_PREDELAY = 500000`, 17.06 ms) delays the first echo by exactly 819 frames (`2914 -> 3733`,
  17.0625 ms); knob 1.0 (180 ms, past the native `0x1fff`-frame line) wraps to 0 in the reference binary.

## Gate after the change

| Setting | null before (dB) | null after (dB) | first echo (frame) |
|---|---:|---:|---:|
| base | −33.6 | −54.2 | 2914 |
| size 0 | −33.5 | −54.2 | 2914 |
| size 1 | −34.2 | −54.2 | 2914 |
| colour 1 | −33.5 | −54.2 | 2914 |
| damping 1 | −85.9 | −85.9 | 2914 |
| colour 0 | −18.0 | −54.9 | 2914 |
| damping 0 | −3.3 | −52.0 | 2914 |
| stereo 0.5 | — | −54.2 L / −54.3 R | 2914 |
| stereo 0 | +1.0 | −50.1 R (exact −6.02 dB L) | 2914 |
| predelay 0.5 (17.06 ms) | +3.0 | −32.2 (session tail floor) | 3733 |

`legacy-reverb.tsv` pins all 10 measured cases (predelay, size, colour, stereo, damping, first echo frame, and wet
levels across four windows); `crates/format/kontakt/tests/legacy_reverb_measured.rs` holds KONTRA to it within 0.3 dB.
