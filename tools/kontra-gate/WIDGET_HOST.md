# Native widget adapter

`widget-host.cpp` is W11's owned copy of e0e1d35f presented-host primitives.
The original `presented-host.cpp` and its pixel/RSS interfaces are unchanged.
This adapter supplies real OS input and an actual fresh CLAP instance; it does
not certify the widget acceptance axis without instrument-specific causal plans.

Build and run all compiler/host jobs through `~/.cache/kontakto-heavy`.
Do not set `CARGO_TARGET_DIR` or `RUSTC_WRAPPER`. The C++ build needs the official
CLAP headers and `-lX11 -lXtst -ldl -pthread`.

```
widget-host PLUGIN --template /dev/shm/private/template
widget-host PLUGIN /dev/shm/private/native-state --gestures
```

All stdin, stdout and stderr must be pipes or tmpfs files. The state input and
new template output must be on tmpfs. Set plugin settings/cache/log/report
locations to a private tmpfs directory, as the existing gate drivers do. A
state template belongs to that exact plugin. A driver may author its native
Selection envelope in RAM, using probe-only helpers; this introduces no v1
file compatibility into the product. Never rebuild or modify frozen v1.

Supply newline-terminated commands over stdin, numbered consecutively from 0:

```
0 move 100 200
1 wait 20
2 button 1 1
3 wait 50
4 move 100 180
5 wait 20
6 button 1 0
7 wait 50
8 checkpoint
9 save
10 reload
11 wait 100
12 checkpoint
13 quit
```

Operations are `move x y`, `button number down`, `key keysym down`, `wait ms`,
`midi port status data1 data2`, `checkpoint`, `save`, `reload`, and `quit`.
`down` is 0/1; buttons are 1..5 (including wheel button pulses). Keys are numeric
X11 keysyms, mapped through the active X11 keymap. Modifier presses/releases are
explicit real events. Coordinates are relative to the current mapped editor
child, translated to root for each move; out-of-child coordinates are rejected.
Waits preserve the plan's input cadence; include waits around click/key/drag
transitions so the GUI has time to observe each event. Limits: 256 bytes/line,
4096 commands, 10 seconds/wait, 300 seconds/plan, 64 MiB native state.

A `save` releases input and captures CLAP state in memory. `reload` requires a
prior save. It releases input, stops/joins processing, hides/destroys the GUI,
deactivates/destroys the plugin, resets host transients, creates/initializes a
fresh instance, loads that saved state, restarts audio, waits for readiness and
attaches a new GUI. Input is released on EOF, rejected plans and SIGTERM/SIGINT.

Checkpoint JSON on stdout contains raw CLAP parameter IDs/values and aggregate
audio measurements since the previous checkpoint. **Keep this stdout in RAM.**
Persist only counts, numeric audio metrics, target/value hashes and typed
failure reasons. Parameter changes alone are insufficient: causal acceptance
must identify the touched widget's owning script/engine parameter and relevant
sound change, while excluding unrelated background changes.

Optional `KONTRA_WIDGET_RGB_FD` names an inherited pipe/tmpfs descriptor. Every
checkpoint writes a 16-byte little-endian header (sequence, instance generation,
width, height as u32), followed by packed RGB8 pixels. Drain this descriptor
concurrently. Never persist raw pixels or use parent-window pixels as a
substitute for the actual editor child.

`load_ready` is true only when the plugin's existing `__kontra_clap_perf` export
confirms a loaded instrument. A plugin without that export can be exercised,
but asynchronous library readiness stays UNKNOWN. The export is an observation,
not a substitute for script/source association or an input callback.

The synthetic `widget-host-fixture.cpp` owns parameter 77 through real X11
button/key events and exposes instance count 78. `widget_host_tests.py` verifies
OS delivery, owner value and constant-mask audio, RAM RGB packets, fresh-instance
recall and input cleanup after invalid plans/signals. It also rejects plugin
destruction with a partially created GUI still alive. These are transport
fixtures, not library parity receipts. Full native widget, sound, automation,
MIDI-learn and frozen-v1 persistence gates remain separate.
