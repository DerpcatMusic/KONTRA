# Legacy Chorus (0x11), measured in the reference 8.13

Renders: `tools/kontakt-oracle/stimrender.py` (Effects/Chorus in insert slot 0, `SEND_EFFECT_DRY_LEVEL = 0`,
`INSERT_EFFECT_OUTPUT_GAIN = 396850` [0 dB], impulse for static tap delays and 4 s 500 Hz `sine_steady` for LFO
modulation), 2026-11-26.

## What the renders showed

- At `depth = 0` (`d0_p0` and `d0_p100`), an impulse produces two identical heads at 10.0 ms (frame 480/481) and
  20.0 ms (frame 960/961), each smoothed by a one-pole with `c = 0.95` (`0.950, 0.0475, 0.00237, 0.00012`), matching
  `delays::CHORUS` (`time = 20.0 ms`, `second_head = true`, `CHORUS_SMOOTHING = 0.95`).
- Under `sine_steady` (500 Hz), demodulating the wet-to-dry complex ratio recovers the LFO rate and delay sweep:
  - `speed = 0.5` (`CH_SPEED = 500000`): `f_lfo = 0.641 Hz` (`0.05 · 160^0.5 = 0.632 Hz`, 1.3%).
  - `speed = 0.8` (`CH_SPEED = 800000`): `f_lfo = 2.907 Hz` (`0.05 · 160^0.8 = 2.897 Hz`, 0.3%).
  - `depth = 0.5`: delay swing `±0.764 ms` (`±36.7 frames`, vs `0.5 · 75 = 37.5 frames`, 2%).
  - `phase = 1.0` (`p100`): left channel matches `base` (`f_lfo = 0.641 Hz`, `±0.764 ms`), shifting the right channel
    by a quarter cycle.

## Gate

| Case | stimulus | measured | model |
|---|---|---|---|
| d0_p0 | impulse | taps @ 10.0 / 20.0 ms, peak 0.950, 2nd 0.0475 | exact |
| d0_p100 | impulse | taps @ 10.0 / 20.0 ms, peak 0.950, 2nd 0.0475 | exact |
| base | 500 Hz sine | `f_lfo = 0.641 Hz`, swing `±0.764 ms` | `0.632 Hz`, `±0.781 ms` |
| s80 | 500 Hz sine | `f_lfo = 2.907 Hz`, swing `±0.777 ms` | `2.897 Hz`, `±0.781 ms` |
| p100 | 500 Hz sine | `f_lfo = 0.641 Hz`, swing `±0.764 ms` | `0.632 Hz`, `±0.781 ms` |

`legacy-chorus.tsv` pins the measured static impulse and swept-sine metrics;
`crates/format/kontakt/tests/legacy_chorus_measured.rs` holds KONTRA's `delays::CHORUS` + `kontra-dsp-delay`
processor to it.
