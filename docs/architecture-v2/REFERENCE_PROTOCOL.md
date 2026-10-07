# Kontakt reference recording protocol

Every Kontakt vs KONTRA comparison runs at matched levels and parameters. The tools in `tools/kontakt-reference/` enforce the items below; `record.sh` refuses to run when a gate fails.

## What is pinned

| Item | Value | Enforced by |
|---|---|---|
| Kontakt master volume, master tune | 0.00 dB, 440.00 Hz | `kontakt.sh state` (golden-image compare of the Master Editor, run before every recording) |
| Instrument volume, pan, tune | 0 dB (slider position follows CC7), Center, 0.00 st | `state` (golden images; the volume slider is accepted only in the default or CC7=127 position) |
| Output channel and bus faders | whole-chain gain verified, not read separately | `calibrate` (below); any fader off unity fails it |
| Sample rate | 48 kHz: null sink 48 kHz float, `pw-record --rate 48000`, source files 48 kHz | `calibrate.py` asserts both files; `record.sh` logs the WAV header rate |
| Controller state | CC 1, 7, 10, 11, 64 sent explicitly before the first note of every channel | `scenario.py` refuses to compile otherwise; a test about unsent state must say `# protocol: unsent REASON`, recorded in `OUT.mid.proto` |
| MIDI | the same `.mid` for Kontakt and KONTRA | `record.sh` logs the file's SHA-256 |
| Routing | Kontakt linked only to the `kontra_ref` null sink | `kontakt.sh route` (every recording) |
| Metric | per-channel peak and RMS (and combined sqrt((L^2+R^2)/2)) | `lr_report.py`, `loud_lr.py` |

Every recording leaves `OUT.wav.log` with the time, MIDI SHA-256, controller-state mode, WAV sample rate, the calibration result and the state values verified.

The default explicit state is CC1=0, CC7=127, CC10=64, CC11=127, CC64=0. Use another value only when that value is the subject of the test, and put it in the scenario.

## Calibration (start of every session)

`kontakt.sh start /mnt/MAIN_STORAGE/kontra_ref_src/calibration.nki` (copy of `scenarios/calibration.nki` plus `noise.wav` there; the NKI references `D:\kontra_ref_src\noise.wav`), then `kontakt.sh calibrate`. It plays the bare noise instrument (no envelope or filter change, no script) at key 60, velocity 127 with the default explicit state, records it, aligns it to the source file by cross-correlation and fits the per-channel gain. Pass: both channels within 0.1 dB of 0 dB (measured -0.09 dB L, -0.02 dB R; the 0.07 dB difference is CC10=64 not being exactly centre) and the residual below -140 dB. On a pass it writes a stamp that is tied to the PipeWire sink and wineserver; `record.sh` refuses to run without a matching stamp. On a failure the session aborts. Restarting Kontakt on the same sink and wineserver keeps the stamp (the stamp covers the PipeWire and Wine chain, not the instrument); the state check runs before every recording.

## Findings from the calibration (2026-10-07)

All numbers are gains relative to the sample file, bare noise instrument, per channel fit, master 0.00 dB unless stated.

- **Master volume is 1:1 in dB**: typed -6, -3, +3, +6 dB gave -6.1, -3.1, +2.9, +5.96 dB. Master 0 dB is unity.
- **CC7 never sent: exactly -6.0206 dB (0.5x amplitude), L = R.** This is Kontakt's state after any restart. Every Kontakt recording made before this protocol (CC1 and CC11 only) carries this 0.5x. KONTRA must model the 0.5x default or the scenarios must send CC7 explicitly.
- **CC7 law**: amplitude = (cc/127)^3. CC7=127 0 dB; 100 -6.23 dB; 64 -17.95 dB; 32 -36.0 dB; 0 silent. CC7 moves the instrument volume slider.
- **CC state persists inside the Kontakt process across recordings.** A recording that sends nothing inherits the previous one's CC7/CC10; this is why explicit state is mandatory, and why a stamp-less "unsent" test has to run right after a restart.
- **Velocity law** (vel 127 = 0 dB; CC7=127; mono noise sample; default Velocity-to-Volume): vel 1 -12.9, 8 -12.0, 16 -11.0, 32 -9.1, 48 -7.4, 64 -5.7, 80 -4.1, 100 -2.30, 110 -1.42, 127 0 dB. Rough fit amp = 0.22 + 0.78 (v/127)^1.4.
- **Pan**: CC10=64 gives L -0.09 / R -0.02 dB for a mono centre sample (0.07 dB imbalance); unsent CC10 gives L = R.
- Earlier sections of KONTAKT_REFERENCE.md that quote absolute dBFS were recorded with CC7 unsent: add 6.02 dB to compare with CC7=127. Ratios, spectra and timings are unaffected.

## Not automated (manual, logged)

The volume of Output channels and instrument buses is not read; the calibration gain covers them. The instrument Velocity-to-Volume amount, group volumes and zone volumes are instrument content, not setup; read them from the Group and Mapping Editors and put them in the report (see section 19 for Una Cotton).

## Host load

Kontakt under Wine drops or underruns voices when the host is busy (observed at load average 11-18: silent notes, wrong sustain). `record.sh` refuses to send MIDI while the 1-minute load average is >= 4 (`KONTRA_MAX_LOAD`), and logs `load1_start` and `load1_end` in each recording's `.log`. Recordings made above that load are invalid. Never use `wineserver -k` on ~/.wine (shared with other sessions); kill only the PIDs you started.
