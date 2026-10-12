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

## 3. Group insert filter kind ids (nki slot kind -> Kontakt GUI module)

Read by opening the instrument, selecting the group (0-based list position in the Group Editor) and reading the slot tile / module panel.

| kind | GUI module | where | stored values |
|---|---|---|---|
| 3 | Legacy HP1 | Vista - 3 Violins FFF Overlay, g16 slot 5 | cutoff 0.0 stored, GUI shows 36.1 Hz (modulated/script-set) |
| 52 | SV LP2 | Una Corda Cotton, g110 slot 0 | cutoff 0.293 |
| 54 | SV HP2 | Una Corda Cotton, g103 slot 2 | cutoff 0.578 |
| 55 | SV LP4 | Areia - 01 Pads, g0 slot 3 | cutoff 1.0 |
| 57 | SV HP4 | Areia - 01 Pads, g0 slot 5 | cutoff 0.0 |
| 90 | Formant I | ANALOG STRINGS, g9 slot 1 | Talk/Sharp/Size 50 % (stored 0.5/0.5) |
| 106 | AR LP2/4 | ANALOG STRINGS, g9 slot 0 | cutoff 0.5135 -> GUI 603.1 Hz, reso 0.5946 -> 59.5 % |

Note: SV cutoff law `25*800^x` predicts 774 Hz for x = 0.5135, the AR LP2/4 module shows 603.1 Hz (modulators on Cutoff in that group may add an offset).

## 4. Group solo levels (Kontakt, allow_group handler, vel 100, held 3 s)

Vista - 3 Cellos key 48 (CC1 = 127, CC11 = 127): g37 -46.6 pk / -60.8 rms, g36 -50.7 / -65.6, g33 -50.9 / -64.9, g5/g4/g1 silent. Barbarian Brass key 55: g8 -29.2 / -43.1, g32 -26.0 / -39.7, g52 -26.2 / -44.9, g0/g4 silent (g0 trace -94). Group ids 0-based.

## 5. AHDSR attack (Kontakt, noise sample, linear curve, KSP set_engine_par on group 0 mod slot 0)

Attack time law (v = engine value 0..1,000,000). Full attack is about 2 x T50 (time to 50% of sustain):

| v | T50 | T25 | T75 | full attack |
|---|---|---|---|---|
| 78,740 | 6 ms | 4 | 10 | ~12 ms |
| 236,220 | 13 ms | 9 | 18 | ~26 ms |
| 393,700 | 39 ms | 19 | 54 | ~78 ms |
| 551,180 | 118 ms | 43 | 189 | ~0.24 s |
| 629,920 | ~0.27 s | | | ~0.54 s |
| 708,660 | 0.554 s | 0.280 | 0.882 | ~1.1 s |
| 866,140 | 2.362 s | 1.162 | 3.559 | ~4.7 s |
| 999,998 | 5.082 s | 2.553 | 7.616 | ~10.2 s (sample-limited) |

Attack curve (ENGINE_PAR_ATK_CURVE, c = (v-500000)/500000) at attack v=629,920 (T ~0.545 s). Level a(u) at fraction u of the attack time; total time is independent of the curve:

| c | .1 | .2 | .3 | .4 | .5 | .6 | .7 | .8 | .9 |
|---|---|---|---|---|---|---|---|---|---|
| -1.000 | 0.000 | 0.000 | 0.001 | 0.002 | 0.006 | 0.016 | 0.046 | 0.120 | 0.322 |
| -0.748 | 0.001 | 0.004 | 0.009 | 0.018 | 0.032 | 0.067 | 0.129 | 0.238 | 0.502 |
| -0.496 | 0.016 | 0.035 | 0.066 | 0.108 | 0.159 | 0.236 | 0.336 | 0.486 | 0.740 |
| -0.244 | 0.074 | 0.142 | 0.239 | 0.327 | 0.399 | 0.507 | 0.597 | 0.702 | 0.893 |
| +0.008 | 0.097 | 0.196 | 0.297 | 0.370 | 0.507 | 0.574 | 0.699 | 0.793 | 0.890 |
| +0.260 | 0.141 | 0.264 | 0.379 | 0.457 | 0.591 | 0.696 | 0.771 | 0.824 | 0.958 |
| +0.512 | 0.309 | 0.519 | 0.682 | 0.740 | 0.836 | 0.919 | 0.942 | 0.951 | 0.998 |
| +0.764 | 0.677 | 0.860 | 0.962 | 0.941 | 0.945 | 1.031 | 1.002 | 0.974 | 1.047 |
| +1.000 | ~1 throughout (90% within ~11 ms) | | | | | | | | |

Concave side fits a = (e^{ku}-1)/(e^k-1) with k ~ 10 (c=-1), 6.0 (-0.748), 3.15 (-0.496), 0.72 (-0.244). Positive side is far more extreme than negative.

## 6. Vista 3 Cellos CC100 (dynamics) sweep, solo of one group (key 48 vel 100, CC1 = CC11 = 127)

RMS dBFS over 0.5-2 s after onset (lead removed), Kontakt, group solo via allow_group. With no CC100 sent first, behaviour equals CC100 = 0. Instrument slider and group volume are 0.0 dB.

| CC100 | g37 | g36 |
|---|---|---|
| 0 | -57.3 | -62.3 |
| 8 | -51.4 | -54.4 |
| 16 | -47.7 | -50.2 |
| 24 | -45.0 | -47.4 |
| 32 | -42.9 | -45.2 |
| 40 | -41.0 | -45.6 |
| 48 | -39.4 | -46.7 |
| 56 | -38.1 | -48.3 |
| 64 | -38.4 | -50.6 |
| 72 | -39.1 | -54.6 |
| 80 | -40.4 | -65.3 |
| 88 | -43.0 | silent |
| 96 | -49.5 | silent |
| 104, 127 | silent | silent |

KONTRA minus Kontakt for g37: about +3.4 to +4.2 dB on the rising side (CC100 0-56), +2.2 at 64, +0.8 at 72, -0.7 at 80, -2.3 at 88, -4.1 at 96. A constant offset on the rising side plus a falling side where Kontakt decays more slowly than KONTRA's linear segment: the curved shaper segment matters.

## 7. Una Corda Cotton notes (key 60)

- Instrument slider reads 0.0 dB; DRY_G1 group volume is -6.0 dB. Group ids are 0-based, 4 columns row-major in the Group Editor list (113 groups; g98 = Depth, g94 = RESONANCE f, g88 = SSR, g107 = Flageolet 1, g110 = Reverse).
- Group solos vel 64/100/127 (peak, rms dBFS): g39 -30.9/-20.7/-14.9 and -52.0/-42.5/-36.0; g94 flat -17.7 / -26.8; g98 flat -30.6 / -43.8 with Una's scripts active, but silent with scripts bypassed (script driven); g107 silent.
- g98 Amplifier mod "Velocity to Volume" shaper reads as an exponential segment y = (e^{kx}-1)/(e^k-1), k about 1.79, stored curvature 0.118. Sampled: u .25 -> .12, .5 -> .30, .75 -> .58, .93 -> .86.

## 8. AR LP2/4 (kind 106) cutoff law

Measured on a noise instrument with an AR LP2/4 group insert and no modulators, engine value set with `set_engine_par($ENGINE_PAR_CUTOFF, v, 0, 0, -1)` and the displayed Hz read from the module:

| x (v/1e6) | 0 | 0.1 | 0.25 | 0.5 | 0.5135 | 0.75 | 0.9 | 1.0 |
|---|---|---|---|---|---|---|---|---|
| Hz | 8.2 | 18.9 | 66.4 | 538.6 | 603.1 | 4.4k | 15.4k | 35.5k |

Pure exponential: `f = 8.2 * 4329^x` Hz (ln slope 8.35-8.40 per unit x between every pair of points). The stored 0.5135 of ANALOG STRINGS group 9 gives 603 Hz, matching the earlier GUI read, so no modulator offset was involved. (Compare SV LP2: `25 * 800^x`.)

## 9. AHDSR decay and release times (Kontakt, noise sample, engine value v = CC*7874)

Same noise instrument. Times in seconds from note-off (release, sustain 1.0) or from the attack peak (decay, sustain 0, attack 0) for the level to fall by N dB:

| v | kind | -3 | -6 | -10 | -20 | -30 | -40 | -50 |
|---|---|---|---|---|---|---|---|---|
| 236,220 | release | 0.009 | 0.012 | 0.014 | 0.019 | 0.022 | 0.025 | 0.026 |
| 393,700 | release | 0.016 | 0.026 | 0.037 | 0.061 | 0.076 | 0.084 | 0.088 |
| 551,180 | release | 0.051 | 0.098 | 0.143 | 0.254 | 0.320 | 0.352 | 0.364 |
| 708,660 | release | 0.174 | 0.374 | 0.599 | 1.093 | 1.387 | 1.529 | 1.580 |
| 787,400 | release | 0.382 | 0.758 | 1.267 | 2.285 | 2.926 | 3.213 | 3.317 |
| 236,220 | decay | 0.002 | 0.004 | 0.006 | 0.011 | 0.014 | 0.015 | |
| 393,700 | decay | 0.009 | 0.018 | 0.030 | 0.054 | 0.068 | 0.074 | |
| 551,180 | decay | 0.033 | 0.081 | 0.133 | 0.243 | 0.307 | 0.340 | |
| 708,660 | decay | 0.126 | 0.291 | 0.532 | 1.023 | 1.359 | 1.502 | |
| 787,400 | decay | 0.195 | 0.587 | 1.117 | 2.146 | 2.860 | 3.172 | |

Decay to sustain 0 and release from sustain 1 have the same shape. Above v ~ 0.39e6 times grow exponentially with v (about x3.8 per +157,480, i.e. ln T slope 8.5 per unit v/1e6; +78,740 doubles it); below that they flatten to millisecond floors. Level versus normalised time u = t / T(-40 dB) for release at v = 708,660: u .1 .74, .2 .55, .3 .45, .4 .33, .5 .22, .6 .16, .7 .11, .8 .07, .9 .03 (amplitude, 1.0 at u = 0); same within a few percent for v 551k-787k. The envelope shape is neither linear nor linear in dB: fast initial fall, long convex tail.

## 10. Flexible envelope segment curve (STAGE_SLOPE) and curvature-to-k map

Measured on a 1 s segment from level 0 to 1 (flex envelope on Volume, `scenarios/flex.ksp`), slope s = CC*7874/1e6 (0.5 linear, as in the manual). Fraction of the final level versus time, fitted with the normalised exponential `a(u) = (e^{ku}-1)/(e^k-1)` for concave (s < 0.5, slow start) and the mirror `(1-e^{-ku})/(1-e^{-k})` for convex (s > 0.5), u = t / 1 s:

| s - 0.5 | k | k / abs(s - 0.5) | rms error |
|---|---|---|---|
| -0.375 | 6.97 | 18.6 | 0.012 |
| -0.250 | 3.72 | 14.9 | 0.019 |
| -0.120 | 1.67 | 13.9 | 0.013 |
| +0.005 | 0.34 | | 0.021 |
| +0.130 | 2.21 | 17.0 | 0.027 |
| +0.255 | 4.05 | 15.9 | 0.027 |
| +0.380 | 7.22 | 19.0 | 0.026 |

So k is about K * abs(s - 0.5) with K between about 14 and 19 (slightly increasing with abs(s - 0.5); the Vista g37 shaper sweep and Una g98 fit K 15-18). Slope value 0 (CC 0) acts as linear, not as the most concave curve. The positive (convex) side sets the final level quickly: s - 0.5 = +0.38 reaches 90% at 0.28 s. Level reaches the target by the end of the stage.

## 11. Zone crossfade (fade-in) law

Measured on a one-zone noise instrument by setting `$ZONE_PAR_LOW_VELO` / `$ZONE_PAR_FADE_LOW_VELO` / `$ZONE_PAR_LOW_KEY` / `$ZONE_PAR_FADE_LOW_KEY` from `on init` (`set_snapshot_type(3)`), RMS of each note divided by the same note with no fade (dB), `tools/kontakt-reference/zone_rms.py`.

Linear amplitude ramp, in integer steps, with a nonzero first step: for a note value v inside the fade-in (low edge L, fade length F, applies to velocity and key alike):

    gain(v) = (v - L + 1) / (F + 1)   for L <= v <= L + F,   1 above

- L = 1, F = 100 (velocity): measured gain v/101 at v = 4, 8, 16, 24, 32, 40, 48, 56, 64, 80 within 0.02 dB.
- L = 30, F = 60 (velocity): gain (v-29)/61 at v = 32, 40, 48, 56, 64, 80 (measured 0.049, 0.180, 0.311, 0.443, 0.573, 0.837 vs 0.049, 0.180, 0.311, 0.443, 0.574, 0.836).
- L = 24, F = 36 (key, vel 100): gain (k-23)/37 at k = 24, 30, 36, 48, 60 (measured 0.027, 0.188, 0.351, 0.677, 1.0); k = 42 and 54 read 0.503 and 0.810 vs 0.514 and 0.838 (pitch-shifted noise, about 0.2-0.3 dB level wobble).
- The ramp is a linear amplitude gain, not equal-power and not dB-linear; the gain is not 0 at the low edge (first step is 1/(F+1)). Notes below L do not play. The high-edge fade-out is the mirror: with H = 100, F = 60 (velocity) gain = (H - v + 1)/(F + 1) = (101 - v)/61, measured 1.0 up to v = 40, then 0.868, 0.738, 0.607, 0.339 at v = 48, 56, 64, 80 (law: 0.869, 0.738, 0.607, 0.344). Notes above H do not play.

## 12. Loudness metric, and Vista g36/g37 left/right split

- The peaks and RMS in sections 2, 4 and 6 come from `compare.wav`, which returns the mono mix (L+R)/2. For uncorrelated channels that reads about 3 dB under the per-channel number. Sections added after this one use `loud_lr.py` / `lr_report.py` (per channel, combined = sqrt((L^2+R^2)/2), max-channel peak).
- Vista g36 and g37 solo (key 48, vel 100, CC1 = CC11 = 127, solo via `allow_group` replacing the Vista script, instrument and group volume 0 dB): the right channel is louder than the left by a constant offset at every CC100.
  - g36: L - R = -5.0 dB at every CC100 from 0 to 80. Example CC100 40: L -45.0, R -39.9, combined -41.8 dBFS RMS. CC100 88 and above is silent.
  - g37: L - R = -4.4 dB (-4.6 at CC100 0-8). Example CC100 56: L -37.0, R -32.6, combined -34.3. CC100 96 is still audible at combined -45.7.
  - The shape over CC100 is the same on both channels, so the split is a constant gain difference per group, not a CC-dependent pan.
- Run through `scenarios/vista_g36_lr.txt` (CC20 selects the solo group: 36 or 37) and `lr_report.py`.

## 13. Full-note levels per channel (real instruments, Kontakt scripts ON, fresh load, no controller sent)

Peak over 0.3-3 s after onset (3 s held notes), RMS sqrt((L^2+R^2)/2) over 0.5-2 s, dBFS, instrument volume 0.0 dB. `monopk` is the (L+R)/2 peak that sections 2/4 report.

| Instrument, note | L pk | R pk | max(L,R) pk | mono pk | RMS (L,R) |
|---|---|---|---|---|---|
| Vista 3 Cellos key 48 vel 100 | -44.5 | -38.6 | -38.6 | -45.0 | -52.5 |
| Una Corda Cotton key 60 vel 64 | -27.1 | -24.7 | -24.7 | -25.8 | -53.5 |
| Una Corda Cotton key 60 vel 100 | -17.5 | -14.7 | -14.7 | -16.1 | -44.1 |
| Una Corda Cotton key 60 vel 127 | -11.3 | -8.6 | -8.6 | -9.9 | -40.3 |
| Barbarian Brass key 55 vel 100 | -19.1 | -15.9 | -15.9 | -20.1 | -27.3 |

Vista with no CC sent is far below its CC1 = CC11 = 127 solo levels (section 6): the power-on dynamics state is quiet. Tools: `loud_lr.py`.

## 14. Power-on controller state (fresh load, before any controller message)

- KSP read-back: a script in an empty slot showing `%CC[n]` in labels on the first note (before any CC sent) reads 0 for CC1, CC2, CC4, CC7, CC10, CC11, CC64, pitch bend (`%CC[$VCC_PITCH_BEND]`, centred = 0 in KSP) and channel pressure (`%CC[$VCC_MONO_AT]`). So the engine array has no non-zero power-on value, including CC11 and CC7. `%CC[]` cannot be read in `on init` (compile error); it is readable in `on note`. Not tested: the value after reloading the instrument in the same session.
- That 0 is not what the modulators see. Barbarian Brass (key 55 vel 100, scripts on, max-channel peak / RMS (L,R) dBFS):
  - CC11 never sent: -15.9 / -27.3. CC11 = 127: -15.9 / -27.3 (identical). CC11 = 0: silent (-98.8 / -126.5). So an unsent CC11 behaves as full (127), not 0.
  - CC1 never sent (CC11 = 127): -15.9 / -27.3. CC1 = 0: -27.6 / -38.8; 32: -22.4 / -32.9; 48: -16.9 / -27.2; 64: -13.1 / -23.6; 80: -14.0 / -24.9; 96: -13.0 / -24.7; 127: -7.7 / -18.8. An unsent CC1 reads like CC1 about 48 (the instrument script sets its own default), not like 0 or 127.
  - The curve over CC1 is not monotonic (script and layer crossfades), so "about 48" is the nearest sampled match, not a measured default.
- Vista 3 Cellos group g37 (solo, CC100 = 56): level is identical (L -37.0, R -32.4 dBFS) with no CC1/CC11 sent, with CC11 = 127 only, with CC1 = 0 and with CC1 = 127; g37 does not depend on CC1 or CC11. CC100 unsent equals CC100 = 0 (section 6).
- Full Vista with its own script and no CC sent is -52.5 dBFS RMS (section 13): the instrument's default dynamics is low.

## 15. Instrument volume, reload persistence, Una without scripts, key crossfade

- Barbarian Brass instrument volume slider (read on a 2560x1440 desktop, `KONTAKT_RES=2560x1440`): 0.0 dB; the four mic faders also 0.0 dB. So the section 13/14 levels have no hidden instrument gain.
- Reload: controller state survives removing and reloading an instrument in the same Kontakt session. After CC1=100, CC11=50, CC7=90, CC64=127, bend +0.5 (4096) and an instrument reload, `%CC` still reads 1:100 7:90 11:50 64:127 pb:4096. The section 14 zeros apply only to a fresh Kontakt process.
- Una Corda Cotton with all four scripts bypassed (key 60 vel 100): unsent, CC80 = 0 and CC80 = 127 are identical: max-channel peak -3.8 (L -5.8, R -3.8), RMS -24.3. CC80 does nothing without the scripts.
- Key crossfade fade-out (`scenarios/key_fade.ksp` on the noise instrument made by `noise_instrument.sh`; `ZONE_PAR_HIGH_KEY` 72, `ZONE_PAR_FADE_HIGH_KEY` 36, vel 100, attack/release 0). Zone key range applies (key 73 silent). Level, dB re key 60: 61 -1.8, 62 -2.5, 63 -3.3, 64 -4.3, 65 -5.3, 66 -6.4, 67 -7.8, 68 -9.4, 69 -11.3, 70 -13.9, 71 -17.3, 72 -22.2; key 48 +4.8; keys 30 and 36 equal (about +9, full level). Matches linear amplitude gain (H-k+1)/(F+1) for k in [H-F, H] (72 vs 60: -22.3 dB, 48 vs 60: +5.7, 66 vs 60: -5.3; measured within about 1 dB), unity below H-F, silent above H.
- Pitfall: the sample is not cut at note-off and keys below 60 pitch it down (longer than 20 s), so tails pollute later notes. Measure with 21 s spacing and only keys >= 60 before the key under test, or one note per recording.
- Not measured: module-parameter modulation (needs GUI-built filter plus modulator).

## 16. Barbarian Brass without its script

Barbarian Brass has a single script (slot 1, "Barbarian Brass 1.00"). Bypassed, key 55 vel 100 (`scenarios/barb_bypass_cc1.txt`; max-channel peak / RMS(L,R) dBFS): unsent -3.4 / -19.3; CC1=0 -3.9 / -18.3; CC1=48 -3.9 / -17.9; CC1=127 -2.9 / -15.6; CC11=0 silent. So without the script CC1 moves the level by under 4 dB (script on: about 20 dB, section 14), an unsent CC1 is not distinguishable from 0 or 48, and CC11 still gates the volume (unsent = full). The "unsent CC1 behaves like about 48" of section 14 therefore comes from the script (its layer logic and initial state), not from NKI modulator bytes. The bypassed instrument is also about 8 dB louder (-19.3 vs -27.3 RMS): the script attenuates or mutes layers.

## 17. Filter cutoff modulation (velocity, LFO, envelope)

Rig: `noise_instrument.sh` loads the noise sample, then Group Editor > Group Insert FX slot 1 > Filters > Lowpass > SV LP2 (reso 0), `Mod` > Add Modulator > target Cutoff. `scenarios/cutoff_mod.nki` is the patch with the SV LP2 plus an AHDSR envelope on cutoff (references D:\kontra_ref_src\noise.wav). Each case is recorded twice, filter bypassed (reference) and active, and `cutoff_report.py WAV REF SPACING LABELS [WIN]` divides the 1/6-octave band spectra: fc is the -3 dB point of the measured response (plateau from 150-300 Hz). Scenarios: `cutoff_vel.txt` (velocities 127/100/64/32/16/1, 21 s apart), `cutoff_one.txt` (one 9 s note, time curves in 0.5 s windows).

- Calibration: with no modulation the SV LP2 reads fc(-3 dB) = 0.65 x knob (knob 6000 -> 3.9 kHz, 800 -> 0.52 kHz). All modulation numbers below are octaves relative to that.
- All cutoff modulators act in octaves, linearly in the modulator value: shift = 10 octaves x amount x modulator (the cutoff knob range 20 Hz-20 kHz is 10 octaves; 100 % amount = the full range), clamped at the range ends.
- Velocity (unipolar, 0..1 = vel/127), amount 44.4 %, knob 800: v1 650 Hz, v32 1094, v64 2373, v100 5640, v127 11343 Hz = +0.0/+1.07/+2.19/+3.44/+4.45 octaves over 520 Hz, i.e. 9.6-10.0 oct per 100 %. Amount 100 %: v16 +1.17, v32 +2.22 octaves, v64 and above beyond 16 kHz. Linear in velocity, no curve with the default shape.
- LFO (Rectangle 0.25 Hz, bipolar, starts high; amount 17.4 %, knob 4000, retrigger on): high state 9070 Hz, low state 843 Hz against 2600 Hz neutral = +1.80 / -1.63 octaves (prediction +-1.74). The first half period (0-2 s) is the high state. Amount 100 %: high above 16 kHz, low about 35 dB down (cutoff pinned near 20 Hz): the swing is clamped to the knob range.
- Envelope (AHDSR, unipolar, amount 34 %, knob 1000, attack 2000 ms, hold 0, decay 2000 ms, sustain -12 dB, curve 0): fc 855 / 1477 / 2585 / 4724 Hz at 0.25 / 0.75 / 1.25 / 1.75 s = env 0.12 / 0.35 / 0.59 / 0.84 (linear attack, octaves = 10 x 0.34 x env); peak at 2 s about 3.4 octaves; sustain 1150 Hz = 0.82 oct = env 0.24, so sustain -12 dB is the linear amplitude 0.25. The decay is exponential, not linear: env - sustain falls 0.51, 0.23, 0.085, 0.02 at 2.25 / 2.75 / 3.25 / 3.75 s (about x0.4 per 0.5 s, 97 % done within the 2000 ms decay time).
- Caveat: the SV LP2 cutoff relation (0.65 x knob) was fitted from one filter type; the ratios, not the absolute -3 dB points, are the law. Measurements are +-0.1 octave (band resolution).

## 18. Vista 3 Cellos solos g1, g4, g5, g33, g36 (silent in section 4 only because they are quiet)

`scenarios/vista_solo_g1_4_5_33_36.txt` with the solo script of section 4 (`scenarios/solo_group.ksp`, replaces the Vista script in slot 1 through Apply from Clipboard; slot 2 cleared), CC1 = CC11 = 127, key 48 vel 100 held 3 s, 6 s apart; `lr_report.py` per channel, RMS over 0.5-2 s after onset, dBFS:

| group | CC100 = 0 (L / R) | CC100 = 64 (L / R) | CC100 = 127 |
|---|---|---|---|
| g1 | -68.8 / -68.7 | -52.8 / -52.1 | silent |
| g4 | -59.9 / -61.7 | -48.3 / -50.1 | silent |
| g5 | -55.5 / -55.8 | -36.6 / -37.0 | silent |
| g33 | -75.4 / -73.9 | -56.5 / -54.7 | silent |
| g36 (control) | -61.8 / -56.5 | -50.1 / -44.8 | silent |

So g1/g4/g5 are not gated: they sound at CC100 0 and 64, but 16-35 dB under g37 (-37 R at CC100 56, section 12), so section 4 (taken at an unspecified CC100) read them as silent. All five groups are silent at CC100 = 127, as g36 (section 12: silent from 88). Left-right differences are constant per group (g1 0, g4 +1.8, g5 +0.4, g33 -1.5, g36 -5.3 dB) as in section 12; the 64 level is 12-19 dB above the 0 level.

## 19. Una Corda Cotton g39 and g94 solos (all four scripts bypassed)

Scenario: `tools/kontakt-reference/scenarios/una_solo_g39_g94.txt` plus `solo_group.ksp` in free slot 5 (MAIN, RESONANCE, RELEASE, REPEDAL all bypassed). Key 60, 3 s notes, 6 s apart; CC20 selects the group. dBFS, RMS 0.5-2 s, peak 0.3-3 s after onset.

| group | vel | L rms | R rms | L peak | R peak |
|---|---|---|---|---|---|
| g39 | 64 | -54.5 | -51.6 | -26.4 | -24.0 |
| g39 | 100 | -45.3 | -42.5 | -17.2 | -14.4 |
| g39 | 127 | -39.9 | -37.4 | -9.5 | -6.8 |
| g94 | 64 | -25.1 | -25.3 | -14.6 | -15.8 |
| g94 | 100 | -25.2 | -25.3 | -14.6 | -15.8 |
| g94 | 127 | -25.2 | -25.3 | -14.6 | -15.8 |

- g39 is the velocity-sensitive note group: about 9 dB per step from 64 to 100 and 5.4 dB from 100 to 127; R is 2.4-2.8 dB louder than L.
- g94 is velocity-independent (identical to 0.1 dB at 64, 100 and 127), nearly centred and 20 dB louder in RMS than g39 at vel 64 (a sustained layer, not a struck note).

### 19a. Re-record under REFERENCE_PROTOCOL confirms section 19

Fresh process, calibration PASS before and after, master 0.00 dB, load 3-5, scripts 1-4 bypassed, `solo_group.ksp`, key 60, vel 100, 3 s, CC7=127 explicit. Peak over the note, RMS 0.5-2 s after onset, dBFS, two repeats identical to 0.1 dB (`note_levels.py`):

| case | L pk | R pk | L rms | R rms |
|---|---|---|---|---|
| g39 | -17.3 | -14.4 | -45.4 | -42.5 |
| g94 | -14.7 | -15.8 | -25.2 | -25.3 |
| g39 CC7=64 | -35.2 | -32.3 | -63.2 | -60.3 |
| g39 three-note sequence v64 / v100 / v127 (6 s apart) | -26.5 / -17.3 / -9.5 | -24.0 / -14.4 / -6.8 | -54.5 / -45.3 / -39.7 | -51.6 / -42.4 / -37.2 |

- These equal section 19 to 0.1-0.2 dB, so section 19 stands, and multi-note recordings in one file work.
- CC7=64 is -17.9 dB against CC7=127 on both channels (the cubic CC7 law); CC7 overrides the saved volume. CC7 unsent and CC7=127 are the same on Una (its saved instrument volume is 0 dB).
- An earlier version of this section (L=R, "second note +7.7 dB", "12 s WAV for a 20 s file") was an analysis bug: `compare.wav()` returns the L/R mean, and my ad-hoc script reshaped that mono signal as stereo, halving the time axis and pairing adjacent samples. The recordings were fine (24 s file for 20 s MIDI + tail). Use `note_levels.py`, which reads both channels.

## 20. Stereo Modeller law

Rig: `noise_instrument.sh stereo` (independent noise A in L, B in R) as a group Insert FX, then `sm_sweep.sh TAG FIELDX FIELDY VALUE...`, which types each value, records a 3.5 s key-60 note at vel 100 and runs `matrix_report.py` (least-squares 2x2 fit out = M [A,B], lag, residual). Base gain g = 0.3838 (-8.3 dB at vel 100); residual -119 dB, so the module is a pure memoryless matrix (unity at defaults).

**Spread s (percent / 100)**
- s > 0: M = g [[1+s, -s], [-s, 1+s]]. Mid gain stays 1, side gain is 1+2s. Clamped at 100% (150 and 200 equal 100).
  - 25%: diag 0.4798, off -0.0960. 50%: 0.5757 / -0.1919. 100%: 0.7677 / -0.3838.
- s < 0: standard M/S width w = 1+s, diag = g(1+w)/2, off = g(1-w)/2.
  - -25%: 0.3359 / 0.0480. -50%: 0.2879 / 0.0960. -75%: 0.2399 / 0.1439. -100%: 0.1919 / 0.1919 (mono).
  - One early -50% run read 0.1949 / 0.0653; it did not reproduce.

**Pan p (-1..1)**: linear balance. The opposite channel is scaled by 1-|p|, the same side is unchanged. -100: R=0; -50: R x0.5 (0.1919); -25: R x0.75 (0.2879); +25, +50, +100 mirror on L.

**Output (dB)**: plain linear gain 10^(dB/20): +6 dB gives x2.0, -6 dB gives x0.5.

**Pseudo Stereo on (pan centre, stereo input)**: not a memoryless matrix (residual -3.1 dB, lag 60 samples, L about 0.0018, R 0.3838). Not resolved; needs a mono-input test.

## 21. Reverb (Send FX "Reverb", Mode Room and Hall)

Where: Instrument Send FX slot > Reverb > Reverb (not Group Insert FX; the other entries there are Convolution, Plate Reverb, Raum, Legacy Reverb and were not measured). Panel: Mode, Predelay, Size, Time, Damping, Diffusion, Mod, Stereo, Low Shelf, High Cut, Return. Defaults: Room, 0 ms, 50%, 3.2k ms, 50%, 50%, 50%, 100%, 0 dB, 21k Hz, Return 0.0 (send level at default).

Rig: `noise_instrument.sh burst` (50 ms noise burst at 0.1 s, 15 s file), reverb in Send FX slot 1, `scenarios/reverb_burst.txt` (one 14 s key-60 note). `dry.wav` is the same scenario with the reverb bypassed; `reverb_report.py WET DRY` subtracts it (wet = WET - DRY, aligned, lag 0) and prints pre-delay (first 5 ms window within 30 dB of the wet peak), broadband RT60 (Schroeder integral, -5..-35 dB fit, extrapolated to -60) and RT60 per band (smooth 1-octave-wide Gaussian bands at 250, 1k, 4k and 8k Hz). `rv_sweep.sh TAG FX FY VALUE...` types the values and prints the report; fields (Room, scrolled): Predelay (860,768), Size (994,768), Time (1140,768), Damping (1270,768), Diffusion (1408,768), Mod (855,831), Stereo (996,831), Low Shelf (1138,831), High Cut (1275,831).

**Time -> decay.** Measured RT60 is 0.81-0.83 x the displayed Time, linear over 0.8-20 s (Room, damping 50%):

| Time ms | 800 | 1000 | 1500 | 2000 | 3000 | 3200 | 4000 | 5000 | 10000 | 20000 |
|---|---|---|---|---|---|---|---|---|---|---|
| RT60 s | 0.66 | 0.84 | 1.22 | 1.63 | 2.42 | 2.60 | 3.27 | 4.08 | 8.09 | 16.17 |

Range about 800 ms to 20 s (typed 100 and 500 are rejected and leave the old value). The decay is exponential (straight in dB). Hall has the same law (1000 -> 0.86, 3200 -> 2.61, 5000 -> 4.09).

**Damping** changes only the high-frequency decay (3.2 s Time; 250/1k bands stay 2.9 s):

| damping % | 0 | 25 | 50 | 75 | 100 |
|---|---|---|---|---|---|
| 4 kHz RT60 s | 2.60 | 2.40 | 2.30 | 2.19 | 2.05 |
| 8 kHz RT60 s | 2.11 | 1.78 | 1.63 | 1.47 | 1.33 |

(Even damping 0 is shorter than mid at 8 kHz: 0.72 x.) At the default the band RT60 relative to 1 kHz is 4 kHz 0.79, 8 kHz 0.56, and the 250 Hz band is about 1.02 x.

**Predelay**: wet starts at predelay + about 5-10 ms (20 -> 25, 50 -> 55, 100 -> 100, 250 -> 240 ms). Range 0-250 ms (500 rejected). RT60 unchanged. Hall starts with no offset (0 ms at 0, 95 ms at 100).

**Size**: RT60 does not change (2.59-2.62 s for 0-100%). It changes early-reflection level and density: wet peak -27.6 dB at 0, -30.7 at 50, -32.6 at 100 (Hall -32.8, -33.8, -35.7), and first arrival 5 ms at size 0 versus 10 ms.

**Diffusion**: RT60 unchanged; only the first-arrival time (15 ms at 0, 10 at 50, 5 at 100) and the early texture change.

**Stereo**: wet L/R correlation (0.5-2 s) is 1.00 at 0%, 0.59 at 50%, -0.05 at 100%. Wet level rises with it (-34.3, -33.7, -31.5 dB).

**High Cut** lowers wet level and leaves the decay: wet peak -37.0 dB (1 kHz), -34.6 (4k), -32.1 (10k), -32.2 (21k). **Low Shelf** (+-12 dB) changes the 250 Hz band by about 0.1 s of RT60 and the wet peak by under 0.4 dB (a shelf on the wet low end; level effect not separated). **Mod** and **Return** not measured.

Note on the first-sight default: the Time display shows "3.2k ms" and the measured default RT60 is 2.59-2.62 s, matching 3200 typed.

## 22. Volume envelope: hold and decay time laws (GUI ms to seconds)

Rig: `noise_instrument.sh noise` (continuous noise), Group Editor, scrolled to the Modulation > Volume AHDSR row. Defaults on a new sample instrument: Curve -33%, Attack 0, Hold 0, Decay 500 ms, Sustain 0 dB, Release 300 ms, mode AHDSR (the "AHD Only" button off). Sustain set to -24 dB so the decay is visible. `scenarios/ahd_note.txt` (one 6 s key-60 note), `env_sweep.sh TAG FX FY VALUE...` (fields at y 818: Curve 700, Attack 860, Hold 998, Decay 1135, Sustain 1265, Release 1410) and `env_ahd.py` (time after the plateau at which the 10 ms RMS falls 1/3/6/10/20 dB).

**Hold**: the level stays flat for the displayed time, then the decay starts. Time to -3 dB minus the hold-0 value (0.072 s, which is the decay's own 3 dB point):

| Hold ms | 100 | 500 | 1000 | 3000 |
|---|---|---|---|---|
| T(-3 dB) - 0.072 s | 0.084 | 0.480 | 0.985 | 2.95 |

So hold = the displayed ms, linear, within about 2% (10 ms measurement window).

**Decay**: with sustain -24 dB the level falls linearly in dB and reaches the sustain level at exactly the displayed Decay time. Time to -20 dB (fraction 20/24 = 0.833 of the decay): D=100 ms: 0.094 s; 250: 0.216; 500: 0.427; 1000: 0.844; 2000: 1.647 (0.84-0.94 x D). Decay 500 ms: -1/-3/-6/-10/-20 dB at 0.031/0.072/0.130/0.216/0.427 s, i.e. about 46 dB/s (24 dB over 0.5 s). So the decay is exponential in amplitude (a straight line in dB). The time to reach the sustain level scales linearly with the Decay setting (Decay 2000: 20 dB at 1.647 s, 12 dB/s), and the slope in dB/s is depth / D.

The Curve field (-100, -33, 0, 50, 100%) did not change the decay timings at all; it shapes only the attack segment.

Release and attack time laws are in sections 5 and 9 (engine value -> seconds). Not yet cross-checked: the engine value (set_engine_par) against the GUI ms display for hold and decay; read the ms display after setting the engine value in a KSP script if that mapping is needed.

## 23. Unsent CC1 (power-on value) on Barbarian Brass and Vista 3 Cellos

`session_cc1.sh`, quiet load (3-6.5), calibration PASS before and after, two fresh-process repeats identical to 0.1 dB. Scripts on, vel 100, 3 s notes 6 s apart, all controllers unsent for note 1, then only CC1 sent. The state check was skipped for these two GUIs (header does not match the 1600x1000 goldens; logged in each `.log`). L/R peak (whole note) and RMS (0.5-2 s), dBFS.

| CC1 | Barbarian key 55 L pk / R pk | L rms / R rms | Vista Cellos key 48 L pk / R pk | L rms / R rms |
|---|---|---|---|---|
| unsent | -19.1 / -15.9 | -30.0 / -25.7 | -44.5 / -38.6 | -55.7 / -50.7 |
| 0 | -31.5 / -27.6 | -41.2 / -36.8 | -44.5 / -38.6 | -55.7 / -50.7 |
| 32 | -27.1 / -22.4 | -36.4 / -30.5 | -29.2 / -23.3 | -40.5 / -35.5 |
| 48 | -22.1 / -16.9 | -31.1 / -24.8 | -26.8 / -21.8 | -38.5 / -33.3 |
| 64 | -18.0 / -13.1 | -27.3 / -21.1 | -25.5 / -22.2 | -37.3 / -32.3 |
| 96 | -17.4 / -13.1 | -27.4 / -22.7 | -21.9 / -18.6 | -33.8 / -29.6 |
| 127 | -10.3 / -7.7 | -21.4 / -17.1 | -18.9 / -13.6 | -28.7 / -24.8 |

- Unsent CC1 is stored per instrument, not an engine-wide constant: Vista Cellos unsent equals CC1=0 exactly; Barbarian unsent sits between CC1=32 and 64 (rms interpolates to about 46-53, peak to about 52-60, so roughly 50).

## 24. Una Corda Cotton g39 with the instrument scripts ON; group 39 post-amp Inverter

- `session_una_on.sh`: scripts 1-4 left ON, only `solo_group.ksp` added in slot 5; quiet load, calibration PASS before and after, two repeats identical to 0.1 dB. g39 v100 L/R peak -17.3/-14.4, rms -45.4/-42.5; g94 v100 peak -14.7/-15.8, rms -25.2/-25.3; three-note g39 v64/v100/v127 identical to section 19a. So bypassing the four scripts changes nothing for these notes.
- GUI read (Group Editor, group DRY_C3 = "Group 40 / 113", scripts on, nothing touched): Post-Amp FX has 2 slots. Slot 1 Inverter: bypassed (red Byp), L/R Swap ON, Output 0.0 dB. Slot 2 Inverter: active, Phase Invert off, L/R Swap off, **Output +6.0 dB**. Group volume -6.0 dB, pan C.
- Group selection tip: the Group Editor "Group n / 113" dropdown is a long popup; open it, scroll with the mouse wheel and click the entry (keyboard Down stops after ~13 items).

## 25. Gainer smoothing (instrument insert slot 1, freshly added module)

- `session_gain.sh` + `gain_step.py`: 9 s held noise note (key 60, vel 100, explicit CC 1/7/10/11/64), Gain field typed and committed with Return at about +4.6 s, quiet load (3-5), calibration PASS before and after, two repeats per step.
- A freshly added Gainer is a 50/50 dry/wet mix: plateau after typed -24 dB is -5.5 dB (= 0.5 + 0.5*10^(-24/20)), after -6 dB it is -2.5 dB (= 0.5 + 0.5*0.501). Fitted mix m = 0.50 on all four recordings (0.499-0.507). Typed 0 dB is unity. KONTRA must apply `(1-m) + m*g` with the module's own dry/wet, not `g`.
- Smoothing shape (4 ms RMS windows of the noise, noise floor about 3% rms of the pre level, so shapes are only weakly separated): a one-pole in linear amplitude gives the same time constant at both step sizes: -24 dB step tau = 45 and 44 ms; -6 dB step tau = 43 and 49 ms. Linear-amplitude ramp: 111/111 vs 100/136 ms; linear-in-dB ramp 150/155 vs 98/133 ms; one-pole-in-dB 95/89 vs 50/58 ms: all inconsistent between step sizes, so one-pole in linear gain, tau about 45 ms (reaches 63% in 45 ms, 95% in 135 ms), is the best fit. The ramp is applied to the wet gain g only (the dry half is constant).
- Caveat: the step is a GUI parameter commit, so any host-side parameter smoothing is included; script- or automation-driven steps were not measured.

## 26. ANALOG STRINGS compressor (instrument insert slot 2), on vs bypassed

Classic mode, threshold -14.2 dB, ratio 1:2.0, attack 26 ms, release 200.5 ms, output +9.0 dB, stereo link on.
Notes: key C4/E4/G4, vel 100, held 3 s, CC1=0, CC7=127, CC10=64, CC11=127. dBFS; peak over the note, RMS over 1..3 s. Two independent launches (a,b / x,y). The instrument is randomised, so repeats agree only within 0.02-0.5 dB RMS (peaks within about 2 dB).

Compressor ON (RMS L/R, a then b): C4 -17.84/-14.94, -18.11/-14.55. E4 -18.11/-14.05, -18.10/-14.43. G4 -19.30/-16.35, -19.32/-15.85.
Bypassed (RMS L/R, x then y): C4 -26.54/-23.56, -26.36/-22.83. E4 -26.79/-22.72, -26.95/-22.59. G4 -27.96/-24.13, -27.91/-24.19.
Peaks ON: C4 -4.2/-4.4, -5.9/-2.0; E4 -6.0/-2.3, -5.8/-2.4; G4 -6.3/-4.2, -6.6/-5.0. Bypassed: C4 -12.6/-11.8, -13.9/-10.4; E4 -14.1/-10.1, -14.5/-9.6; G4 -15.9/-12.3, -14.8/-11.0.
Net: about +8.7 dB RMS (8.4-9.0) from the compressor, i.e. the +9.0 dB output with only about 0.3 dB of gain reduction.

Same note twice in one recording (C4 at 0.5 s and 6.5 s) matches within 0.05 dB RMS. The earlier "second note 7.7 dB louder" did not reproduce; multi-note recordings are full length (record.sh now refuses short captures).

## 27. Inverter output gain (stereo noise, key 60 vel 100)

| Output | RMS L/R | peak L/R |
|---|---|---|
| no FX control, and 0 dB | -22.5/-22.4 | -9.6/-9.5 |
| -6 dB | -28.5/-28.4 | -15.6/-15.5 |
| +6 dB | -16.5/-16.4 | -3.6/-3.5 |

Output is a plain dB gain; same law in instrument insert and group insert (inst run calibrated before and after; grp post-calibration not confirmed).

## 28. Reverb displayed values (GUI display strings)

Normalised x = 0, .25, .5, .75, 1.
Time 500 / 1.3k / 3.2k / 8.0k / 20.2k ms (about 500*40.4^x). Size, Damping, Diffusion, Stereo, Mod 0/25/50/75/100 %. Predelay 0/62.5/125/187.5/250 ms. High Cut 21.0k/16.2k/11.5k/6.8k/2.0k Hz (linear, decreasing). Low Shelf -0/-3/-6/-9/-12 dB.
