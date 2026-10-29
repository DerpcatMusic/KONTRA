# Flair (0x5c) saved record and static laws, Kontakt 8.13.1

Work in progress for P6.21. The measurements were paused on 2026-10-29 (machine-wide pause).

## Saved record (v0x51, 66 bytes)

Decoded from INIT and one-parameter saves (oracle, `set_engine_par` then `save_instrument`).

| Offset | Field | INIT |
|---|---|---|
| 0 | AMOUNT f32 | 1.0 |
| 4 | PITCH f32, n/96 (KSP takes the note 0..96; values above 96 are ignored) | 0.5 (48) |
| 8 | RATE f32 | 0.3606 (0.1 Hz) |
| 12 | WIDTH f32 | 0 |
| 16 | MIX f32 | 0.5 |
| 20 | VOICES f32 | 0 |
| 24 | FEEDBACK f32 | 0.4 |
| 28 | DAMPING f32 | 0 |
| 32 | DETUNE f32 | 0 |
| 36 | OFFSET f32 | 0 |
| 40 | INVERT_PHASE byte | 0 |
| 41 | MODE i32 (0 Standard, 1 Thru Zero, 2 Scan) | 0 |
| 45 | SCANMODE i32 (0 Triangle, 1 Saw Up, 2 Saw Down) | 0 |
| 49 | CHORD i32 0..23 (KSP takes the integer) | 0 |
| 53 | rate sync unit f32, −1 free | −1 |
| 57, 61, 65 | unidentified (0, 1.0, byte 0) | |

## Display laws (binary, display function 0x14195dbf0)

- **RATE:** Hz = 440·2^((138x − 195)/12), about 0.005..16.4 Hz (0x14195d910).
- **PITCH:** note = round(96x).
- **Chords 0..23:** Major, Major7, Minor, Minor7, Add2, Sus2, Sus4, N.3rd, Dom7, Quartal, Maj Min,
  Cluster, Whole, Min3rd, Maj3rd, Fourth, Tritone, Fifth, Octave, Harm, Even, Odd, Inharm, Unison.
  The intervals are not yet read.

## Static audio (Standard, AMOUNT 0, MIX 1, impulse rig, left channel)

- **Fitted response:** wet = 0.5·x + 0.5·x(t − D). The direct tap and the delayed tap are equal.
- **D** is one period of 440·2^((n − 57)/12) Hz, i.e. the note n+12. Fitted D:
  - pitch 48: 182.97 frames (C4 is 183.5);
  - pitch 60: 91.23 frames (C5 is 91.7);
  - pitch 72: 45.37 frames (C6 is 45.9).

  The fitted D runs about 0.5 frame short; this needs a recheck.
- **FEEDBACK 0.5:** echoes every about 91.7 frames at pitch 60. The gain law is not fitted yet.
- **WIDTH 1 (AMOUNT 0):** both taps scale to 0.354 on the left (−3 dB, a pan law).
- **Not yet measured:** the right channel, and the FEEDBACK 0.9, DAMPING and MIX 0.5 runs (that
  capture failed). Still to come:
  - INVERT and VOICES (captured in r2, not yet fitted);
  - Thru Zero, Scan, DETUNE, OFFSET;
  - modulation (AMOUNT, RATE).
