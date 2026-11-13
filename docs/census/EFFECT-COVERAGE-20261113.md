# Effect and filter law coverage (2026-11-13)

What every effect and filter kind in the corpus rests on: a **probe** (a Kontakt or UVI render
of a controlled patch against ours), a **binary** read (constants and code read from the
executable), a **library** render (a real instrument scored against the oracle), or nothing
(**unverified**).

## Kontakt: all 682 `.nki` on disk

`kontra-inspect load` on every instrument (8 libraries), active (unbypassed) effects counted per
instrument. Script-loaded effects (Conflux's Raum, Una Corda's Space presets) are not in a load.

| Instruments | Libs | Kind (Kontakt module) | Status | Evidence |
|---:|---:|---|---|---|
| 677 | 8 | Send Levels | library | Afflatus sends into convolution score within 1 dB on the scoreboard; the isolated probe added no send return in Kontakt, so it is inconclusive |
| 365 | 4 | Convolution | binary + library | Auto Gain 0x1419b2bc0; size linear (Una Corda's label); Una Corda Space −45.5 dB ours vs −46.2 dB |
| 361 | 7 | Stereo Modeller | **probe** | five presets (INIT, Narrow −100/−50, Wide +50/+100): every L/R gain within 0.02 dB |
| 224 | 8 | 1/2/3-band EQ | **probe** + binary | EQ1 Peak, EQ2 Hi-Fi, EQ3 Old Radio: octave bands 63 Hz–8 kHz within 0.2 dB, 16 kHz within 0.7 dB; level within 0.46 dB |
| 134 | 4 | Gainer | unverified, inaudible | every instance stores 1.0011 (+0.01 dB) |
| 30 | 4 | Inverter | trivial | a sign or channel swap |
| 20 | 3 | 1-pole high-pass, 43.5 Hz | probe | `coverage_probe.py hp1`: corner at 0.825 of the cascade law (35.6 Hz at 0); within 0.4 dB at cutoff 0, .1, .25, .5, .75 |
| ≤ 6 each | 1–2 | Soft/Tube shaper, ladder and SV filters, Limiter, Compressor, Delay, Crush, Formant | binary (kontakt-effects crate cites addresses) | |

Probe tool: `tools/kontakt-oracle/coverage_probe.py stereo|spectral OUT_DIR`. The first instance
of a kind in a session ramps in Kontakt, so each set starts with a throwaway one.

## UVI: Augmented Orchestra (620 programs) and V Winds (40 programs, 25 banks)

Active instances in the decoded program XML.

| Instances | Effect | Status | Evidence |
|---:|---|---|---|
| 240,560 / 3,122 | OnePole (AO / VW) | binary | coefficient `exp(−2π·min(f·2^((n−60)·kt/12), fs/2)/fs)`, 0x1412b4710 |
| 79,675 | XpanderFilter | binary, Q open | Freq 20..20000 Hz (0x14258b220), Drive `10^(dB·0.05)` (0x141cfa9e8); Q is stored raw (+0x3c), its ladder law not traced |
| 3,508 | GainMatrix (VW) | binary | linear cells |
| 2,977 | ThreeBandShelves | binary + math | equals the first-order bilinear shelf to 0.01 dB |
| 1,784 | CombFilter | binary | |
| 1,234 | SparkVerb | binary | −600 dB per pass, 1/340 s per metre, 2π·1200 mod scale |
| 1,000+ | Gain | linear | |
| 620 | Maximizer | binary, fixed in #1503 | falls to a third in Release (ln(1/3) at 0x141e64248) |
| ≈ 900 | Convolver / SampledReverb | binary | raw wet 0.1, Time window, Width, NormalizePower |

## Open

- Kontakt factory impulse responses (`/presets/Effects/Convolution/K4IR.nkx/...`) do not resolve:
  the resolver knows no factory root. No library instrument uses one (0 of 682).
- XpanderFilter Q → resonance: mapped as far as the node. The bindings sit at 0x1412be77e: Freq
  +0x38, Q +0x3c (raw), Drive +0x4c, mode +0x48, each setting the dirty byte +0xf0. The
  clone at 0x1412cb2e0 (vtable 0x141e79158) gives the defaults: 1000 Hz, Q 0, mode 3, Drive
  1.0, key centre 60. Each channel has a 36-byte state in the +0x90 vector, plus 2× up/down
  half-band oversamplers (+0xb0/+0xd0, designed at 0x14163cc40 with cutoff
  min(20000/fs, 0.48)). The ladder kernel that reads Q is in neither of the node's vtables
  and stays open. Next: the callers of the +0x90 vector's element.
- Send Levels in isolation: a probe with a send-slot effect whose return is audible.
