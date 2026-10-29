# Flair (0x5c) saved record and static laws, Kontakt 8.13.1

P6.21. **Blocked on transcribing the kernel** (2026-10-29): a measured stand-in does not fit once FEEDBACK is above 0; see Kernel.

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

## Feedback captures (repeated after the pause)

At pitch 60 the echoes are spaced at 91.7 frames, but they are not geometric:
- FEEDBACK 0.5: per-echo sums are 0.30, 0.52, 0.49, 0.36, 0.30, 0.31.
- FEEDBACK 0.9: the direct tap falls to about 0.13.
- INVERT flips the sign of every other echo (same magnitudes).

A comb `(a + b·z^-D)/(1 − g·z^-D)` fits with an rms of 0.18, so the static two-tap law does not
extend to feedback.

## Kernel (audio body 0x14195f600, read from the decompile)

Per sample:
1. The input passes a trapezoidal one-pole pair (state +0xeb0/+0xec0, coefficients
   +0xe90/+0xea0, taps +0xe30/+0xe40).
2. A 16-lane control ramp bank (+0x478/+0x460/+0x418/+0x400, gains +0x4a0..) updates. Its
   writers are 0x1419614e0 (11 lanes), 0x141961030, 0x141961f60 and 0x141960af0 (mode 2 = Scan,
   0x141961db0).
3. An LFO/phase stage (+0xfb8, the std::function at +0x1030) is smoothed into +0x3c0.
4. For each of 4 voices (stride 0x1d0, base +0x878):
   - **Feedback:** the input to the line is `k·x + 2u/√(u² + 1)`, with `u = ½·voice feedback`
     (k at +0x6b0, constants 0x144f39280 = 0.5 and 0x144f3a0b0 = 1).
   - **Interpolation ring:** an 8-slot ring, then a 3-term filter.
   - **Delay line:** it has three reads through the cubic interpolator 0x14195d9b0: `centre + m`,
     `centre − m` (m = depth × LFO, +0x1038) and `centre + offset` (Thru Zero).
5. **Normalisation:** the per-voice levels (+0x6d0) are inverted and divided by +0x1048. They are
   smoothed into +0x12dc, then (x − 1)·0.142141 + 1 (0x147699c40).
6. **Output:** two 2×2 cross matrices (+0x670..+0x6a8, scaled by +0x6b8) per voice pair, for width
   and cross-feedback.

**Next:** transcribe steps 4–6 together with the ramp writers. Check against these captures at a
low input level (the saturator) before porting.
