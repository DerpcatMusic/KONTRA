# Phasis and Flair KISS mappings against Kontakt 8

Measured on real Kontakt 8 with `tools/kontakt-oracle/fxrig.py` (impulse stimulus, one note per
instance, the bypassed instance as the reference). KONTRA side: the same impulse through the
`config()` result of the mapping, rendered by the sampler-core test runtime, 16384 frames.
Compared as dB at ten probe frequencies (30 Hz to 15 kHz) relative to the dry impulse, plus the
window energy (E, dB relative to the dry impulse).

## Rig checks

- Positive control. Phasis MIX 0 against MIX 1 (AMOUNT 0, FEEDBACK 0, RATE 0): MIX 1 moves 500 Hz by
  -14.7 dB and 200 Hz by -2.4 dB, MIX 0 is flat. Flair FEEDBACK 0 against FEEDBACK 0.9: the tail
  after 20 ms is 0.000 against 0.052 (peak). Both parameters are live, not no-ops.
- Repeatability. Two full passes. Static settings (AMOUNT 0, RATE 0) repeat to 0.00 dB at every
  probe. Settings with a sweeping LFO (Phasis AMOUNT > 0 or RATE > 0; Flair AMOUNT 0.5) do not:
  spread up to 44 dB at a notch probe and 2.3 dB in E. Those rows carry no verdict.
- Onsets. The impulse arrives about 1.04 to 1.05 s after the MIDI note on this rig, and the
  arrival moves by up to 10 ms between sessions and by up to 2.5 ms between instances of one
  session. All delays below are therefore taken inside one instance (echo spacing, peak spacing),
  not from the note-on time.

## Phasis (legacy phaser, PR #669)

Mapping: AMOUNT -> sweep depth (`SweepLaw::Legacy`), FEEDBACK -> allpass feedback x 0.85, RATE ->
LFO speed, MIX -> wet/dry share. CENTER, SPREAD, NOTCHES, stereo, mod mix and the flags are not mapped.

Static grid (AMOUNT 0, RATE 0, FEEDBACK 0, MIX 1 unless stated). Kontakt K, KONTRA R, dB:

| case | source | 30 | 60 | 100 | 200 | 500 | 1k | 2k | 5k | 10k | 15k | E dB | max abs err |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| MIX 0 | K | -0.1 | 0.0 | 0.0 | 0.0 | 0.0 | 0.0 | 0.0 | 0.0 | 0.0 | 0.0 | -0.47 | 0.1 |
| | R | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0.00 | |
| MIX 0.25 | K | -0.2 | -0.3 | -0.7 | -2.2 | -0.3 | -1.4 | -2.1 | -0.4 | -0.1 | 0.0 | -0.79 | 4.6 |
| | R | -0.5 | -2.2 | -5.3 | -1.8 | -3.4 | -4.2 | -1.1 | -0.2 | 0.0 | 0.0 | -0.24 | |
| MIX 0.5 | K | -0.2 | -0.5 | -1.4 | -4.9 | -0.6 | -2.8 | -4.6 | -0.8 | -0.2 | -0.1 | -1.04 | 10.6 |
| | R | -0.7 | -3.2 | -12.0 | -2.7 | -5.7 | -7.5 | -1.5 | -0.2 | 0.0 | 0.0 | -0.32 | |
| MIX 0.75 | K | -0.3 | -0.6 | -1.8 | -8.0 | -0.8 | -3.9 | -7.4 | -1.0 | -0.2 | -0.1 | -1.19 | 6.3 |
| | R | -0.5 | -2.2 | -5.3 | -1.8 | -3.4 | -4.2 | -1.1 | -0.2 | 0.0 | 0.0 | -0.24 | |
| MIX 1 | K | -0.3 | -0.7 | -1.9 | -9.9 | -0.8 | -4.3 | -8.9 | -1.1 | -0.3 | -0.1 | -1.25 | 9.9 |
| | R | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0.00 | |
| FEEDBACK 0.5 | K | -0.7 | 0.7 | -3.0 | -15.4 | -1.1 | -9.3 | -13.1 | -1.4 | -2.2 | -2.6 | -3.13 | 15.0 |
| | R | 2.2 | -0.9 | -2.9 | -0.4 | -2.1 | -2.2 | 1.9 | 4.6 | 1.0 | -1.5 | 0.87 | |
| FEEDBACK 0.9 | K | -4.3 | 0.4 | -8.7 | -22.9 | -6.3 | -16.7 | -20.1 | -4.4 | -7.2 | -7.9 | -7.53 | 22.1 |
| | R | 2.7 | -2.2 | -4.7 | -1.5 | -3.8 | -3.8 | 2.0 | 10.6 | 0.4 | -3.1 | 3.82 | |

Verdict: out of tolerance for MIX and FEEDBACK (tolerance: 2 dB at each probe and 1 dB in E).
The KONTRA response at MIX 1 is flat (the allpass cascade has no notches by itself), while
Kontakt has deep notches at AMOUNT 0. The unmapped CENTER sets the notch positions: CENTER 0
puts the deepest notch at 100 Hz (-15.9 dB), CENTER 1 moves it to 2 to 15 kHz (-8.9 dB at 15 kHz).
NOTCHES 0 and 1e6 give identical output. The mismatch is therefore a missing CENTER law, not a
mix or feedback law error. AMOUNT and RATE: not verifiable on this rig (LFO-phase dependent).

Change pushed: none. The fix needs a CENTER-to-sweep-centre law fitted at three or more CENTER
values (0, 0.5, 1) and a notch count law; that is a design step, not a KISS tweak.

## Flair (legacy flanger, PR #670)

The PR's `Flair` arm delegated to the legacy flanger with `color: 2.5`. The legacy flanger arm
never reads `color` (its native base is 0 frames), and it passed `feedback` through as a constant
0. So the 2.5 ms base was not applied and FEEDBACK did nothing.

Static Kontakt measurements (AMOUNT 0, RATE 0, MIX 1):

- Echo train spacing (the base delay): 3.979, 7.792, 11.625, 15.438, 19.271 ms (FEEDBACK 0.5);
  3.958, 7.771, 11.604, 15.417, 19.250, 23.062 ms (FEEDBACK 0.9). Spacing 3.821 to 3.823 ms,
  that is 183.4 frames at 48 kHz. The base delay is **3.82 ms, not 2.5 ms** (1.3 ms short).
- Feedback loop gain, from successive echo peaks (noisy at low feedback): FEEDBACK 0.25 about
  0.36 to 0.5, 0.5 about 0.70, 0.9 about 0.94.
- Single-head: no tap at half the delay (1.9 ms). Second head disabled.
- An extra tap 8 frames (0.17 ms) after the arrival appears in every instance; its origin is not
  identified.
- Level at MIX 1: the wet energy is -3.5 dB against the dry impulse.

Fix pushed to fx/flair-map (PR #670), `crates/sampler-kontakt/src/effects/timemod.rs`, Flair arm:

- base delay 3.8125 ms, one head (`second_head: false`);
- feedback passed through as the loop gain `sqrt(FEEDBACK)`, fitted to the three measured
  gains above;
- dry and wet as before (`1 - MIX`, `MIX`).

KONTRA after the fix (impulse dump, frames from the input impulse): echo train at 184, 368, 552,
736 ... frames (3.83 ms spacing, within 0.6 frame of the Kontakt spacing). Loop gains 0.47, 0.67
and 0.90 for FEEDBACK 0.25, 0.5 and 0.9, against Kontakt 0.36 to 0.5, 0.70, 0.94. The regression
test `flair_maps_onto_the_legacy_flanger` asserts base 3.8125 ms, one head and the sqrt feedback.

Remaining error after the fix (MIX 1, AMOUNT 0): Kontakt's wet output has two equal taps (the
8-frame tap and the echo), so its energy is -3.5 dB and its magnitude has comb notches (up to
8.8 dB at 100 Hz and 8.4 dB at 2 kHz). KONTRA's single-tap wet is flat and 3 dB louder. Matching
this needs a direct-path option in the legacy kernel, which is a design step; not done here.

Not verified: AMOUNT (depth) and RATE (speed). Both need an LFO-phase-locked measurement; the
rig's repeatability fails for any sweeping setting. KONTRA's depth law (`excursion = AMOUNT * 75`)
is kept as is.

## Method

- Kontakt: `oracle-slot python3 tools/kontakt-oracle/fxrig.py SPEC.json OUT.wav`, chunked to
  four active instances plus the reference per session (the slot has five MIDI channels).
- KONTRA: a temporary `#[ignore]` test rendered each mapped `Config` with an impulse and wrote the
  left channel to a raw f32 file; the test was removed before commit. The regression test in
  `flair_map_tests` covers the fixed values.
