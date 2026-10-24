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
