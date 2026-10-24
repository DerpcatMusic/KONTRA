# Sampler-mode oracle verification, second pass (2026-10-24)

Scope: gate-release level vs hold time; round-robin group start; the census's most-used modes without oracle
evidence. One oracle-slot capture each, plus KONTRA renders of the same MIDI (`tools/oracle/kontra_render.py`).
Per-slot MIDI must use the slot's channel block (`slot.channel_block()`); `mkmidi` in the session scratch does this.

## 1. Gate-release level vs hold time: blocked by script-owned release triggers

- Pacific 12 Violas Marcatos (360 gate-release zones, 30 release-counter refs) and Vista 3 Violins FFF Overlay
  both have one script with `owns_release_triggers = true` (new `ScriptView::owns_release_triggers`).
- A 40-file sample found 16 instruments with gate-release zones; every one with release-counter refs owns release
  triggers through its script. Native release-trigger groups are muted by design in that case
  (`run_release` → `musical = false`), so KONTRA's native release level is not on the output path here.
- Kontakt capture (Pacific Marcatos, holds 0.3 / 1 / 2 / 4 s): release-window level -36.8 / -38.6 / -36.7 / -42.5 dB.
  KONTRA: exact silence in every release window. The hold dependence in Kontakt is about 6 dB at 4 s, not a clean
  linear law; it is measured here and not fitted.
- Next: the script's `on release` (wave74, "scripts on = silent within 80 ms of note-off").

## 2. Round robin: Una Corda Cotton, key 96, nine repeated notes

- The RR tests are `RoundRobin(3)` on "The Pianist" zones (keys 86 to 99), cycle length 3, four zones per key.
  No script owns release triggers on this instrument.
- KONTRA: all nine notes sound, identical level (-34.39 dB) and centroid (1439 Hz), so the three RR positions
  select the same sample on this key.
- Kontakt: notes 0 to 4 sound with the same fingerprint (-34.39 dB, about 1437 Hz); notes 5 to 8 are exact digital
  silence. That is a mismatch: Kontakt stops sounding after five repeated notes, KONTRA does not.
- Cause not established. The instrument has four scripts (`scripts 4`); their `on note` / `on release` logic and any
  group counters are the first suspects. The RR counter itself is not separated from the scripts by this capture.

## 3. Not done

- Third mode (until-release loops, 420 zones in one library): not captured; the scan of 60 files was stopped.
- Sample-start offset, reverse and one-shot were not counted by the census, so they were not chosen.

## 4. Una Corda Cotton key 96: follow-up (static read and one capture)

- Static read of the script's `on note` (No Dampers killer after 10 s, repetition killer with `$NUM_REP=4`,
  `fade_out(..., 150000, 1)`, `play_note` start offsets) and `on release` (`ignore_event`, `note_off` loop).
  Checked and ruled out as the cause: `$ENGINE_UPTIME` units (KONTRA `ReadClock micros:1000` = ms, matches KSP);
  `fade_out` time units and stop flag (`lower.rs` FadeOut lowering); `%StartOffset` and `%FadeInTabelle` index
  ranges for key 96 (in bounds).
- One capture (`oracle-slot`, `capture.py --midi`, six key-96 notes, 2 s apart, 1.5 s long, 16 s window):
  Kontakt plays all six notes (onsets at about 1.0, 3.0, ... 11.0 s, -32.6 dB peaks, same decay per note).
  Nothing went silent, so this capture does not reproduce the nine-note silence seen earlier.
- Not done: KSP variable dump of `NotePointer` / `NumNotes` per note (no dump tool hook used), and a matching KONTRA
  render. The earlier silence probably needs held or overlapping notes (the 10 s killer and repetition killer only
  act once slots are full and older than 10 s). Next step if picked up again: capture with notes held 12 s each, and
  diff against KONTRA's per-note slot choice.
- Verdict: cause not found; no engine change; time-boxed and moved on.
