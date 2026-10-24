# Tape Saturator: live-control smoothing and Quality (measured through Kontakt 8)

Measured on the real Kontakt 8 oracle (`oracle-slot`, Wine), with `tools/kontakt-oracle/fxrig.py tone` (new in this
change: live CC20 steps and a spectrum mode). Raw data: `tape-saturator/raw/`. Specs: `tape-saturator/spec-*.json`.
Reproduce a fit without Kontakt: `fxrig.py tone SPEC OUT_PREFIX OUT.wav` (analysis only when the WAV exists).

## Rig and method

- Effect: `Effects/Tape Saturator` (INIT preset), one effect per instrument, 48 kHz, PipeWire capture.
- Stimulus: a steady sine in a generated zone (1 kHz at −18 dBFS for gain, 40 Hz at −12 dBFS for warmth, 8 kHz at
  −12 dBFS for roll-off, 9.5 kHz at −6 dBFS for Quality). Each instance plays in its own slot, one per MIDI channel.
- Reference: instance 0 is bypassed (`EFFECT_BYPASS=1`). Gain = active minus reference, in dB, per 1 ms bin
  (`bin_ms` = one tone period for 40 Hz and 25 ms; warmth uses a Hilbert envelope).
- Live step: note-on at the slot start; `CC20` at 1.5 s; the KSP `on controller` writes the `post` sets with
  `set_engine_par`. Pre-state set in `on init`. No-change instance (`ctrl`) and bypass instance were run in the same
  sessions as controls.
- Fit: one-pole `a + b·exp(−(t−t0)/τ)` with the start time `t0` free (±20 ms around the scheduled CC), least squares
  on dB and on linear gain; a linear ramp fit (`ramp`, same free `t0`) and a model-free 10–90 % rise time are
  reported too. The 63 % time equals τ for a one-pole.

Why `t0` is free: each CC20 has its own MIDI timing jitter of a few ms (aplaymidi). A calibration from a
bypass instance in the same session does not share that jitter, and the first attempt that used it gave τ = 11.07
and 14.50 ms for the same gain instance in two sessions (27 % apart). The free-`t0` fit gives 10.70 and 10.70 ms
for the same spec. The first method was dropped, not patched.

## Constants (one-pole τ = 63 % time, dB domain; linear-domain value in brackets)

| Control (step) | Stimulus | τ (ms), mean of n | rise 10–90 (ms) | Best shape (residual) | Fits |
|---|---|---|---|---|---|
| Gain `TP_GAIN` 0.2 → 0.8 | 1 kHz, −18 dBFS | **10.70** (10.86) n=6, sd 0.20 | 22.0 | linear ramp **27–28 ms** (lin. 0.0008 vs one-pole 0.0062) | 3 sessions |
| Warmth `TP_WARMTH` 0 → 1 | 40 Hz, −12 dBFS | **11.84** (11.79) n=6, sd 0.28 | 24.3 | linear ramp **30–31 ms** (lin. 0.0053 vs one-pole 0.0136) | 2 sessions |
| Roll-off `TP_HF_ROLLOFF` 0 → 1 | 8 kHz, −12 dBFS | **6.94** (7.17) n=6, sd 0.00 | 15.1 | one-pole **6.94 ms** (lin. 0.0005 vs ramp 0.0023) | 2 sessions |

Plateau changes measured (positive controls): gain −0.98 → −4.72 dB (Δ −3.74 dB); warmth −3.72 → +2.06 dB at 40 Hz
(Δ +5.78 dB); roll-off −1.10 → +0.46 dB at 8 kHz (Δ +1.56 dB). The no-change control is flat (`ctrl`, `gain_a`:
residual 0.000 dB). The bypass step changes the output by about 1 dB (weak check, see below).

Reading the table: the requested one-pole model gives the 63 % time, but for gain and warmth the shape is a linear
ramp about 2.6× the one-pole τ long, and the linear ramp's residual is far below the one-pole's in the linear domain
(gain 0.0008 vs 0.0062). A one-pole with τ 10.7 ms is therefore an approximation of gain; a 28 ms linear ramp fits the
measured shape. Roll-off is a one-pole at 6.9 ms.

## Repeatability

Same spec, sessions compared (tolerance: levels ±0.05 dB, τ and rise ±10 %, t0 ±3 ms):

- Gain (`spec-gain-step.json`): three sessions, plateau −4.72 dB and pre −0.98 dB in all three; τ session means
  10.86, 10.70, 10.55 ms (spread 3 %); rise 22.0 ms in all. **Pass.**
- Warmth: two sessions, τ means 11.90 and 11.79 ms (spread 0.9 %). **Pass.**
- Roll-off: two sessions, τ 6.94 ms in all six fits. **Pass.**
- Quality (`spec-quality-A`, `-B`): the 0.5 Q0 point repeats in both sessions: alias −93.79 dB and 2nd harmonic
  −101.25 dB relative to the fundamental, identical to 0.01 dB. **Pass.**

Rig problems (reported, not hidden): two of the seven timing sessions have a 0.1–0.3 s digital-silence dropout in
one instance (gain run 1 and run 3, instance `gain_b`, about 3.9–4.1 s, after the fit window). Excluded below −60 dB
(column `dropout_bins`). The dropouts do not change τ here, but they happen by chance and they are not explained.
The first method's disagreement is described above.

## Quality (oversampling)

Factor from source: `crates/sampler-core/src/dsp/color/tape.rs` lines 1–6 and the `factor` match: 2× at 48 kHz with
Quality off, 4× with Quality on (not measured here; the output cannot show the factor directly).

Measured on a 9.5 kHz sine. The 3rd harmonic (28.5 kHz) folds to 19.5 kHz; that bin is the alias. The bypass
reference is at −175 dB, so the alias bin is real signal, not noise floor.

| Drive `TP_GAIN` | Quality | fundamental (dBFS) | 2nd harmonic (dB rel.) | alias 19.5 kHz (dB rel.) |
|---|---|---|---|---|
| 0.2 | off | −17.2 | −120.5 | −115.2 |
| 0.5 | off | −22.4 | −101.3 | **−93.8** (repeated) |
| 0.5 | on | −22.4 | −95.6 | **−82.3** |
| 0.8 | off | −43.1 | −75.9 | −76.2 |
| 0.8 | on | −36.2 | −94.2 | −78.0 |
| 1.0 | off | −45.9 | −99.4 | −94.1 |
| 1.0 | on | −45.8 | −94.2 | −78.8 |

Finding: Quality on does not reduce the alias bin here. It raises it by 11.5 dB at drive 0.5 and by 15 dB at drive
1.0, and about 2 dB lower at 0.8 (the only point where the drive pushes into the table, with harmonics −76 dB). The 2nd
harmonic is about 5 dB higher with Quality on. The fundamental is unchanged at drive 0.5 and 1.0 but 7 dB louder
with Quality on at 0.8 (−36.2 vs −43.1 dBFS), so Quality also changes the level at that drive. So Quality changes the nonlinear
response (more 2nd harmonic and different alias content) and does not behave like a clean 4× oversampling reduction
of aliasing at this stimulus. A single 9.5 kHz tone at one level is a narrow test; the static core's nonlinear
response is not compared to these numbers in this change.

Drive is non-monotonic: fundamental output falls by about 23 dB from `TP_GAIN` 0.5 to 1.0, while 2nd harmonic at 1.0
returns to −99 dB. This needs a separate check (possibly the law of the parameter at the top of its range) before any
drive-dependent claim. It affects the Quality numbers at 1.0, so those are reported with that caveat.

## Instruments used

Not achieved. The Conflux and Morphology Evolved libraries are on disk at `/data/main/Libraries/Kontakt/` (one
`Instruments/*.nki` in each), but the effect type of an insert is stored as a numeric id, not as a name, so grepping
the `.nki` and `.nksn` files for "Tape" finds nothing usable. `tools/kontakt-oracle/dump.lua` does not list insert
effects, and I did not verify a Lua API for them, so no library instrument was loaded. The measurements use the
Tape Saturator on generated instruments, which is the same effect and preset those libraries use. The seven library
instruments and their effect settings are unmeasured.

## Not measured

- The seven library instruments (above).
- Gain and roll-off steps in the opposite direction; levels other than −18 dBFS (gain) and −12 dBFS (roll-off, warmth);
  other frequencies for roll-off and warmth.
- The CC20-to-audio latency in absolute terms (t0 is relative to the scheduled CC; its per-instance offset is ~14 ms and
  repeats within 1–2 ms).
- Quality at other frequencies and levels; the 4× factor (from source only).
- The nonlinear response of the static core against these numbers (this change is measurement only).
- The bypass switch: a weak check (≈1 dB change) that shows the CC edge is a single bin; not a full calibration.

## Consequences for the static core

Not changed in this change (the live-control diagnostic stays). The measured constants are candidates for the KISS
ramp: gain ~28 ms linear ramp (or τ 10.7 ms one-pole if a one-pole is preferred), warmth ~31 ms linear ramp (τ 11.8 ms),
roll-off τ 6.9 ms one-pole. Quality changes the nonlinear response and needs a separate DSP decision before it is
modelled.
