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

Decay to sustain 0 and release from sustain 1 have the same shape. Above v ~ 0.39e6 times grow exponentially with v (about x3.8 per +157,480, i.e. ln T slope 8.5 per unit v/1e6; +78,740 doubles it); below that they flatten to millisecond floors. Level versus normalised time u = t / T(-40 dB) for release at v = 708,660: u .1 .74, .2 .55, .3 .45, .4 .33, .5 .22, .6 .16, .7 .11, .8 .07, .9 .03 (amplitude, 1.0 at u = 0); same within a few percent for v 551k-787k. The envelope shape is neither linear nor linear in dB: fast initial fall, long convex tail, ends at 0 at u about 1.2.
