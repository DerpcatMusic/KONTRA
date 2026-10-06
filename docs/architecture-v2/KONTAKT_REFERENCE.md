# Kontakt 8 reference: documented laws and measured laws

Sources: Kontakt 8 Portable 8.13.1 bundled manuals (".../Portapotty/Kontakt 8/Documentation/"). Page numbers
are PDF page indices of the shipped files (printed page numbers are 8 lower for KONTAKT_Manual). KM = KONTAKT_Manual.pdf,
KSP = KSP_Manual.pdf. Measured items come from the black-box harness in `tools/kontakt-reference/` (Kontakt standalone
under Wine, recorded from a null sink).

## 1. Documented (manual)

| Topic | Statement | Source |
|---|---|---|
| Loop Count | Count = repeats of the loop region before playback proceeds to the following sample material. **0 loops the region indefinitely; playback never reaches any following material or later loops.** | KM p190 |
| Loop modes | Until End: forward, loop keeps playing through the release phase. Until End <->: ping-pong, also through release. Until Release: forward while key held, on release resumes normal playback from the current position. Until Release <->: same, alternating. Loop Tune applies to all passes after the first loop jump. | KM p190 |
| Filter slopes | cutoff = -3 dB point. N poles = 6N dB/octave: LP1 -6, LP2 -12, LP3 -18, LP4 -24, LP6 -36. The manual does not state cutoff range, Q law or gain compensation. | KM p224-p230 |
| Filter families | SV: clean standard. Ladder: ladder circuit, has High Quality (oversampling) and Gain (post-filter gain / soft saturation). Daft: 2-pole, Gain. Monark: 4-pole ladder with saturating Gain, Feedback A/B, FM, self-oscillates. AR: resonance adapts to input level (lower at high level). Pro-53: "similar to 4-pole, different signature". Legacy: older algorithm, same pole counts, Legacy Ladder is 4-pole -24. | KM p225-p230 |
| Modulation assignment | Controls: Intensity, Invert, Lag (ms time constant, smoothing; pitch assignments default 250), Modulation Shaper (128-entry table of the transfer curve, X = source value, Y = output, or curve segments editor like the flex envelope). | KM p308-p309 |
| Bipolar intensity | `$ENGINE_PAR_MOD_TARGET_MP_INTENSITY`: no modulation at 500000, max at 1000000, max inverted at 0. `$ENGINE_PAR_MOD_TARGET_INTENSITY` is positive range only, negative needs Invert. | KSP p342 |
| set_engine_par range | Value is 0..1000000 except switches (0/1) and stepped params. `FLEXENV_STAGE_TIME`: 1000000 is one second. Flex stages 3..31, loop start 1..30, loop end 2..31. | KSP p116, p342-p343 |
| AHDSR | Attack curve parameter: 0 linear, negative concave, positive convex (attack phase only). Hold is a fixed time at max. Sustain holds while key held. Release goes from the sustain level to zero. Retrigger off: envelope keeps its position until the last note is released. AHD only: one-shot always completes. | KM p310-p311 |
| DBD | Rises/falls from zero to the break level over D1, then returns to zero over D2. Negative break falls then rises. Easy mode: break fixed at 0, only D1. | KM p311 |
| Flexible envelope | Up to 32 breakpoints (min 4). Segment curve value 0.5 linear, higher convex, lower concave. SLD mode shifts following points, FIX does not. Sustain section between the orange markers: two points only -> freezes at the second level while held; extra points inside -> loops that section while held. | KM p311-p312 |
| Release trigger | Group samples fire on note-off. `t(ms)` counts down from its value from note-on, frozen at note-off, and is a modulation source (note duration). **Monophonic: a repeated note cuts any still-sounding previous release samples so only one plays at a time.** A looped release sample cannot be stopped from outside; needs a volume envelope. | KM p205 |
| Voice groups | Modes Kill Any/Oldest/Newest/Highest/Lowest; Pref.Rel prefers keeping already-released notes; FadeTime is the fade of a sacrificed voice (may briefly exceed the max); Exclusive Groups cut all sounding samples of the other voice groups in the same exclusive group. | KM p165, p114 |
| play_note | `play_note(note, vel, sample-offset-us, duration-us)`: note-on followed by note-off. Duration -1: releasing the triggering note stops the sample. **Duration 0: the entire sample is played; looped samples would play indefinitely.** Returns the event id. | KSP p90 |
| note_off | Equivalent to releasing the key: always triggers `on release`, jumps to the volume envelope release. Optional time offset (us) overrides a play_note duration (works on events with predefined duration, new in K8). Differs from `fade_out()`, which works on voice level. | KSP p88-p89 |
| Instrument volume | Default of volume slider is -6 dB or 0 dB depending on the Options dialog (new instruments). | KM p106 |
| Not stated | Zone crossfade curve, pan law, velocity-to-volume curve, filter cutoff Hz range, resonance law, shaper segment curvature numerics, group volume/zone volume combination. These stay measurement-only. | |

## 2. Measured (harness)

All through Kontakt standalone, group insert filter on a noise sample, spectra from recordings.

- Cutoff law (SV LP2): `f = 25 * 800^x` Hz for engine value x in 0..1 (typed Hz fields accepted).
- SV LP2 at resonance 0: prewarped bilinear 2-pole, Q = 0.5, `|H|^2 = 1/(1+u^2)^2`, `u = tan(pi f/fs)/tan(pi fc/fs)`.
- Resonance: `k = 1/Q = (2 - 0.013)(1 - r)^3.1 + 0.013`; gain compensation falls to -6 dB at full resonance.
- Lowpass family measured response: SV LP1 and Legacy LP1 are 1-pole without resonance. Two-pole SV/AR/Ladder/Daft/Monark LP2 behave as two cascaded one-pole sections (-3 dB at 0.647 fc). Ladder LP3: three cascaded one-poles (0.513 fc). LP4 types: four cascaded (0.439 fc). SV LP6: six cascaded (0.352 fc). Legacy LP2/LP4: Butterworth. Pro 53 passband +3.3 dB. Monark has passband loss.
- Loops: a 48 s Vista note stays level; self-similarity period ~2.31 s, same as KONTRA. Consistent with the documented Count 0 meaning.
- Retrigger pedal on Vista 3 Cellos: Kontakt ends all notes (quiet release tail, silent from 2.7 s through a 40 s tail), including those created by `play_note` with duration 0 inside the script. KONTRA cut at 1.4 s with script Fault(InvalidInput).
- Loudness KONTRA minus Kontakt (raw peak): Vista Cellos +3.4 dB, Vista Basses +3.2, Una Corda -3.0/-3.2/-1.0 (vel 64/100/127), Barbarian Brass -4.3/-4.4. No consistent offset.
- Pedal suite (8 pedals.rs scenarios) recorded for Vista 3 Cellos, Una Corda Pure, ANALOG STRINGS: Kontakt ends every note (cellos release 2-3 s, Una ~5 s, ANALOG STRINGS ~12 s). KONTRA renders silent after ~1.5 s (cellos Fault(InvalidInput), Una Fault(Capacity) with stuck voices in later scenarios, ANALOG STRINGS renders silence).
