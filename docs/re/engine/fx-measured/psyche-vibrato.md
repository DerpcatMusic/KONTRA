# Psyche Delay and Vibrato/Chorus, measured in Kontakt 8.13.1 (2026-11-14)

Both load from a script by type (`$EFFECT_TYPE_PSYCHEDELAY` 49, `$EFFECT_TYPE_VIBRATO_CHORUS` 55) and read from saved
records (modules 0x63 and 0x69, v0x10). Rig: `tools/kontakt-oracle/fx_lab.py` (knobs set from the note callback,
0.7 s before the stimulus), `fx_export.py` + `fx_gate.py` + the ignored `fit` test. The earlier "VC parameters are
inert" finding (`vibrato-chorus.md`) was the init-time set: set from a note callback every VC knob moves.

## Saved records (factory `.nkp` presets, decoded; fresh read-back `BuildScratch/fx5-fresh-defaults.txt`)

| Module | Bytes | Layout |
|---|---|---|
| Psyche 0x63 | 40 | f32 TIME (ms, or a synced count) @0, FEEDBACK @4, DETUNE @8, PITCH @12, LR_OFFSET @16, CROSS_FEEDBACK @20; bytes REVERSE_STEREO @24, DETUNE_STEREO @25, REVERSE @26; f32 unit beats @27 (−1 free); f32 @31, f32 @35, byte @39 (not heard) |
| Vibrato/Chorus 0x69 | 25 | i32 DEPTH @0 (0..5), i32 COLOR @4 (0..2), f32 BLEND @8, RATE @12, WIDTH @16, MIX @20; byte @24 (1 everywhere) |

Fresh: Psyche TIME 500 ms (.25), FEEDBACK .25, DETUNE/PITCH/LR_OFFSET/CROSS .5, switches off, dry level 0 dB, output
0 dB. Vibrato/Chorus DEPTH 0, COLOR 0, BLEND 0, RATE .6, WIDTH 0, MIX 1, dry level off.

## Psyche Delay: laws

The kernel read from the binary for v2 (process [ref], kernel [ref], apply [ref]), then checked
against the oracle; it is now the engine's generic pitch-shifting echo (`ir::Shifter`, `kontra-dsp-delay`
`shifter.rs`), the Kontakt laws in `kontra-kontakt-effects` `psyche.rs`.

- Two heads per side glide through the line at speed `r` (pitch ratio; negative backwards), crossfaded on a parabola
  whose phase steps `4q/rate` (`q = max(|1−r|·500 ms / D, ½)`); a head restarts when its fade passes 0, at
  `D − (|1−r|·500/q)·sign(1−r)` ms, so its grain is centred on D. The fades sum to exactly 1.
- TIME is ms (`2000·x`, 5 ms floor); the right side runs `D·(1 − LR_OFFSET/2)`. Impulse, FEEDBACK 0: echo at
  500.0 ms (TIME .25), 200 / 1000 ms at .1 / .5.
- Repeats scale by `0.98·FEEDBACK`; CROSS is the other side's share of each line's feedback. Fresh: the second left
  echo −18.3 dB (0.5·0.245 → −18.2), the right side's first echo 375 ms after the left's (0.75·D).
- PITCH `round(24x − 12)` semitones (PITCH 1: a 1 kHz tone comes back at 2000 Hz, level −0.02 dB); DETUNE `x − ½`
  semitones (PITCH .75 DETUNE .7: 1431.1 Hz against 1430.6 predicted), on the left only with DETUNE_STEREO.
- REVERSE reverses both sides; REVERSE_STEREO flips the right side's direction against the left's.
- A head keeps its place when TIME changes until it restarts (up to one grain, 2 s at pitch 0): set TIME .1 0.7 s
  before an impulse, the old 500 ms echo is still heard at −6.2 dB. The engine does the same.
- Tempo sync (unit beats @27) plays as count × beats, unmeasured, and is reported.

### Gate, wet only (best fitted start phase, from 0.5 s into the sound)

| Case | band max / rms dB | frame max / rms dB |
|---|---|---|
| tone, FEEDBACK 0 | 7.35 / 4.84 (skirts at −43..−59 dB) | 0.69 / 0.02 |
| tone, PITCH 1 | 0.16 / 0.16 | 29.3 / 1.72 (*) |
| tone, PITCH .75 DETUNE .7 | 8.51 / 5.14 (grain sidebands −45..−56 dB) | 8.15 / 0.99 |
| tone, REVERSE | 0.16 / 0.16 | 21.6 / 1.72 (*) |
| sweep, fresh | 1.11 / 0.34 | 7.61 / 1.01 |
| sweep, FEEDBACK .5 CROSS 0 | 1.11 / 0.26 | 3.36 / 0.38 |
| sweep, FEEDBACK .5 PITCH .25 | 2.29 / 1.18 | 11.5 / 2.02 |
| sweep, REVERSE + REVERSE_STEREO | 3.48 / 1.11 | 28.8 / 2.78 |

(*) The bypassed reference (the model's input) holds a capture phase slip of π at 5.29 s (and 0.58 rad at 8.5 s);
two heads either side of it cancel for one crossfade, the oracle instance has no slip there. The frames outside it
hold 0.1 dB. The main tone and every band within 40 dB of the loudest meet the 1 dB gate; misses are grain sidebands
and sweep bands whose level hangs on where the grain boundaries fall.

## Vibrato/Chorus: laws (1 kHz tone through the delay's phase, and a sweep)

Carried as `ir::Delay` (a sine-swept head, a body section, `direct` for the input inside the effect), Kontakt laws in
`kontra-kontakt-effects` `vibrato.rs`.

| DEPTH | 0 | 1 | 2 | 3 | 4 | 5 |
|---|---|---|---|---|---|---|
| sweep, ms peak to peak | 0.63 | 0.56 | 0.82 | 1.10 | 1.35 | 1.78 |
| 1 kHz level re input, dB | −0.72 | −1.24 | −0.95 | −2.49 | −3.09 | −2.96 |
| amplitude ripple, dB | 4.3 | 4.3 | 4.3 | 4.2 | 5.5 | 8.7 |

- RATE: 0.833, 2.167, 6.0, 17.0 Hz at 0, .3, .6, .9 → `0.814·28.7^x` Hz. The sweep is near a sine (3rd harmonic
  4% at DEPTH 2); its depth does not follow RATE.
- COLOR 0 response (sweep, DEPTH 0): −1 dB to 250 Hz, −1.2..−3 dB to 4 kHz, −7 dB above 6 kHz: a second-order high
  shelf, 3.82 kHz, Q 0.63, −6.4 dB, and −1.25 dB (0.74 dB rms, 1.65 dB max).
- MIX 0: the input alone (+0.17 dB). BLEND 1: the dry signal at the vibrato's level (a 12 Hz beat, −3.35 dB).
  BLEND .5: −2.93 dB.
- WIDTH 1 (DEPTH 2): L/R correlation 0.57 (1.0 at WIDTH 0).

Not modelled: COLOR 1 and 2 (relative to COLOR 0: +5 dB at 635 Hz and −2 dB at 1 kHz; +4.5 dB at 250 Hz and 1 kHz,
+3 dB above; they play COLOR 0 and are reported), the 4..9 dB amplitude ripple (the scanner's line), the delay's
centre (1 ms taken), BLEND between its ends (linear taken), WIDTH's law (the right sweep a quarter cycle ahead at
WIDTH 1 taken). Superseded: the binary core below replaces these fits, and its gate follows it.

## The core (binary, 2026-11-15)

`ni::effektrig::dsp::hammondvibratochorus::HammondVibratoChorus` (ctor
[ref], process [ref], switches [ref], COLOR [ref],
line design [ref] per channel, constants read with
`tools/re/pe_consts.py`):

- The switches: DEPTH `int(6x)` (0..5), COLOR `int(3x)` (0..2), for `x` below 1 (the indices a
  script writes land there; `x = 1` would give 6 and 3, which no saved or written index reaches), a byte
  switch at `x ≥ 0.5` (the record's trailing byte).
- RATE (lambda [ref]): `6·2^(5x − 3)` Hz (6.0 Hz at 0.6, as measured).
- One continuous lambda is `π·x` (WIDTH's phase, taken).
- The line is a wave-digital LC ladder (series and parallel adaptors,
  inductor ports `2L·fs` with L 0.5 H, capacitor ports `T/2C` with C 4 nF,
  1 nF the last): 18 sections for DEPTH I..III, 36
 for IV..VI. Its first nodes carry resistive dividers to
  ground (27k+68k, 56k+150k, 39k+150k, 33k+180k, 18k+180k, 12k+180k, the
  first at the input); the source resistance is `11000·p₀²` Ω. A section's
  delay is √(LC) = 44.7 µs and its cutoff 2/√(LC) = 7.1 kHz.
- Process: the channels' sum ×½ goes through two identical
  trapezoidal state-variable filters at 7100 Hz, k √2; the lowpass feeds
  the line, ½ × the highpass is added to the pickup's output; MIX (+0x58)
  blends that against each channel's input.
- The pickup: its phase steps `2π·rate/fs`, the right one
  ahead by WIDTH's `π·x`; 16 stations per turn (×8/π), through the
  triangle 0..8..1 into DEPTH's row of nine taps (ctor 0x664..0x738):
  I `0 1 2 3 4 5 6 7 8`, II `0 1 2 4 6 8 10 11 12`, III `0 1 3 6 9 12 15 17
  18`, IV `0 1 3 7 12 17 21 23 24`, V `0 1 4 9 15 21 26 29 30`, VI `0 2 6
  12 20 28 30 34 36`; neighbouring stations crossfade linearly.
- COLOR sets the ladder's terminating resistance, both channels:
  `15000·10^(4.5u − 2.25)` Ω, with `u = 0.5` at COLOR 0 (15 kΩ),
  `0.3667(1 − p₀)` at COLOR 1 and `0.3667p₀ + 0.6333` at COLOR 2; `p₀` is
  the core's first continuous parameter (+0x40, clamped 1e-6..1), fresh at
  1e-6 and not tied to an ENGINE_PAR: 3.8 kΩ and 60 kΩ. A mismatched load
  reflects along the line: COLOR 1 and 2's resonances.
- Ported as `ir::Scanner` (`kontra-dsp-delay` `scanner.rs`: the same circuit,
  trapezoidal nodal solve, which is what the bilinear WDF computes), laws in
  `kontra-kontakt-effects` `vibrato.rs`. BLEND is not in this process; it is
  taken as the line's input mixed into the pickup's signal, half each at 1
  (the chorus measured above). The DEPTH gain table and the fitted body are
  gone: the line makes both.

Gate of the fitted delay (2026-11-15, `sweep`, DEPTH 2, RATE .6; band rms / max, frame rms, dB): COLOR 0 1.24 /
2.7, 2.09; COLOR 1 1.93 / 3.9 (worst 635 Hz), 3.57; COLOR 2 3.13 / 10.2 (worst 317 Hz), 3.97; COLOR 1 at BLEND 1
2.36 / 7.0, 5.90.

Gate of the ported core (2026-11-15, same stimulus, one oracle session; band rms / max, frame rms, dB):

| Case | band rms / max | frame rms |
|---|---|---|
| DEPTH III, COLOR 0 | 0.59 / 1.2 | 2.02 |
| DEPTH III, COLOR 1 | 0.98 / 1.9 | 3.59 |
| DEPTH III, COLOR 2 | 0.87 / 2.2 | 4.48 |
| DEPTH III, COLOR 1, BLEND 1 | 2.23 / 9.3 (8 kHz) | 4.96 |
| DEPTH I, COLOR 0 | 0.95 / 1.5 | 1.84 |
| DEPTH VI, COLOR 0 | 0.58 / 1.5 | 2.33 |
| DEPTH III, WIDTH 1 | 0.71 / 2.0 | 2.02 |
| DEPTH IV, COLOR 2, RATE .3 | 2.28 / 6.8 (2.5 kHz) | 5.27 |

Every COLOR passes the band gate the fit missed. Frame errors follow the pickup's start phase (the model's starts
at 0, Kontakt's where its last turn left it). BLEND 1 misses about the split (−3 dB at 6.3 kHz, −9.5 at 8 kHz):
the line's input and ½ the high band cancel about 7.1 kHz in the model, not in Kontakt, so BLEND is not that mix;
its law is open (not in the core's process). DEPTH IV at RATE .3: a 2 Hz pickup over a 5 s sweep gives each band
one place in the turn, so its band error hangs on the start phase.
