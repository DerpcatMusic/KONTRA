# Polyphonic variable read outside a note callback (Kontakt 8.13.1)

Question: what does real Kontakt return when a `declare polyphonic $v` (or `%arr[n]`) is read
outside `on note` / `on release`, e.g. in `on ui_control`, `on controller`, `on listener`?
KONTRA returns 0 with a warning (`crates/sampler-ksp/src/lower.rs:1093`). Kontakt's own warning
text is `polyphonic variable read outside a note callback is N`; 271 corpus instruments emit it.

## Status: partially measured, blocked

Measured (native oracle, `tools/kontakt-oracle/dump.py`, Kontakt 8.13.1 under Wine, Xvfb :78):

| Scenario | Probe | Kontakt value |
|---|---|---|
| `on init` read, no note played yet (`declare polyphonic $v`, `$out := $v`) | `probes/ksp_polyphonic_init_read.lua` (`init_read_poly_no_note`) | **0** |
| `on init` write `$v := 5`, then read in init | same (`init_write_then_read_poly`) | **0**: the write outside a note callback is not kept |
| `on init` read of plain `declare $out := -1` set to 1 (control) | same (`init_plain_expr`) | decodes as 1 (-149.38 dB) |
| `on listener` (1000 ms timer) sets a plain variable (control) | same (`listener_plain`) | **listener never fires** (0 dB, unchanged) |

Decode for the rows above: the readback is `get_instrument_volume` (dB) after
`set_engine_par(VOLUME, E)`. Calibration in the same family of runs: E = 1000 → -167.38 dB
(value 0), E = 2000 → -149.38 dB (value 1), E = 65000 → -58.98, E = 128000 → -41.38.

**Not measured: any scenario after notes.** Kontakt runs no timers, notes or `on note`
callbacks unless its audio thread runs. `dump.py` runs without an audio device, so the listener
(and therefore `on ui_control` / `on controller` reads after notes) never executes. The
existing `render.py` path starts audio, but each launch needs GUI clicks in
Options > Audio (the device choice does not persist, per its docstring), and it reads back audio,
not state. `ksp_polyphonic_read.lua` is the audio-dependent probe (note at tick 1, read at tick 2
or 3); it is committed and stays blocked until a device that persists exists or `render.py`
gains a state readback.

Still open, and the questions the probe answers once audio runs:
- no note yet, read in `on ui_control` / `on controller` / `on listener`: 0 or something else?
- after one note (held / released), two notes of different pitch (held / released).

## Implications for KONTRA (no semantics change yet)

- The no-note init read is 0, which matches KONTRA's current behaviour
  (`crates/sampler-ksp/tests/controllers.rs`, `polyphonic_read_in_init_before_any_note_is_zero`).
- KONTRA emits no warning for an `on init` read of a polyphonic variable; the reads in
  ui/controller/listener warn. Whether Kontakt warns in init is not measured (its log is not
  captured headlessly).
- The 271-instrument corpus case (value N, not 0) is unresolved: the N is only meaningful after
  notes, which are unmeasured. Do not change `lower.rs` until the after-note rows exist.

## Probe pitfalls found while building this (keep for next time)

- KSP `declare` must sit inside `on init` (`on init declare $x end on`); a top-level `declare` or
  an undeclared variable fails to compile, and `set_instrument_script_source` still returns OK
  because it only reports the Lua call. Check the script's effect through a readback.
- `declare $x := 0` inside init is accepted; `:= -1` on a negative literal is fine too.
- `wineserver -k` kills the shared Wine prefix, including the yabridge plugin hosts used by other
  sessions. Do not run it; use `wineserver -w` or leave Wine alone.
- Only one Kontakt at a time: another session's Kontakt blocks `dump.py` with
  "another Kontakt is running".

Fixture: `docs/re/engine/ksp-polyphonic-read-init.jsonl` (raw output of the init probe).
