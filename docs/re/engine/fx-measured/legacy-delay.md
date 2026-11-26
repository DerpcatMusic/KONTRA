# Legacy Delay (0x10), measured in the reference 8.13

Renders: `tools/kontakt-oracle/stimrender.py` (Effects/Delay in insert slot 0, `SEND_EFFECT_DRY_LEVEL = 0`,
`INSERT_EFFECT_OUTPUT_GAIN = 396850` [0 dB], a 0.5 impulse into the left input with dry reference on R), 2026-11-26.
Base: `DL_TIME = 500000` (140.5417 ms / 6746 frames at 48 kHz), `DL_FEEDBACK = 500000` (0.5), `DL_PAN = 0`,
`DL_DAMPING = 500000` (0.5).

## What the renders showed

- Input gain and loop feedback both scale by `g = 0.98 · feedback`: at `damp = 0`, `feedback = 0.5`, echoes arrive at
  exact multiples of 6746 frames with peak amplitudes `0.49000`, `0.24010 = 0.49²`, `0.11765 = 0.49³`, `0.05765 = 0.49⁴`.
- One-pole damping coefficient in the read path is `c = 1 - 0.7 · damping`: at `damping = 0.5`, first echo peak is
  `0.49 · 0.65 = 0.31850`; at `damping = 1.0`, `0.49 · 0.30 = 0.14700` followed by exact geometric tail `0.147 · 0.7^k`.
- At `pan = 1.0`, even echoes cross to the right channel (`e2 = 0` on L) and odd echoes return to L (`e1 = 0.31850`,
  `e3 = 0.03231`), matching the cross-feedback matrix `[[1 - pan, pan], [pan, 1 - pan]] · g`.
- At `feedback = 0.8`, `damping = 0.5`, first echo peak is `0.98 · 0.8 · 0.65 = 0.50960`, second `0.25969`.

## Gate

| Setting | delay (frames) | 1st echo peak | null vs reference (dB) |
|---|---:|---:|---:|
| base | 6746 | 0.31850 | −113.55 |
| time 0.25 | 1421 | 0.31850 | −113.55 |
| damp 0 | 6746 | 0.49000 | −113.5 |
| damp 1 | 6746 | 0.14700 | bit-exact (`0.14700, 0.10290, 0.07203, ...`) |
| pan 1 | 6746 | 0.31850 (even echoes cross to R) | −102.01 |
| feedback 0.8 | 6746 | 0.50960 | −88.79 |

`legacy-delay.tsv` pins all six measured cases; `crates/format/kontakt/tests/legacy_delay_measured.rs` holds KONTRA's
`delays::delay` + `kontra-dsp-delay` processor to exact echo frames, peak amplitudes, and RMS levels within 0.2 dB.
