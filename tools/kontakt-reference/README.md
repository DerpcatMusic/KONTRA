# Kontakt 8 reference harness

Measure Kontakt 8 (standalone under Wine) against KONTRA on the same MIDI scenario.
Outputs go to `~/.cache/kontra-reference/` (never the repo).

    tools/kontakt-reference/kontakt.sh start "/path/Instrument.nki"   # ~1 min, GUI-scripted
    tools/kontakt-reference/run.sh "/path/Instrument.nki" tools/kontakt-reference/scenarios/note.txt
    tools/kontakt-reference/kontakt.sh stop

- `scenario.py`: scenario text (`T on|off|note|cc|pedal|bend|pc ...`) to .mid.
- `record.sh`: `aplaymidi` into Kontakt, `pw-record` from the `kontra_ref` null sink (48 kHz float).
- `sampler-native render-kontakt-midi IN.nki OUT.wav SCEN.mid`: KONTRA side.
- `compare.py`: onset-aligned RMS/peak/centroid/pitch per hop, tail time, summary.

Caveats: Kontakt applies its default -6 dB instrument volume (expect ~5-6 dB lower than KONTRA
unless the instrument volume is set to 0 dB); capture is realtime, so onsets are aligned by
detection (~ms jitter); stop Kontakt with `kontakt.sh stop` and `pactl unload-module` the sink.
