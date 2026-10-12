# MOOSE patches

`moose-params`, `moose-derive`, `moose-clap` and `moose-vst3` are vendored
from moose `bffa467` (the rev in `Cargo.toml`) and patched in through
`[patch."https://github.com/Matari-Audio/moose"]`. Their `Cargo.toml`s
spell out what the moose workspace used to inherit; the sources differ only
by the patches below. `moose-mui` is vendored for its own reasons (see its
README).

## Output port names that follow the plugin

MOOSE named audio ports from the static `BusLayout` only. KONTRA names each
host output after what is routed to it ("Harp", "Harp Close", "st.5"), so
the names must change at runtime and the host must hear of it.

Same idiom as `Params::parameter_presentation` / `_revision`:

- `moose-params`: `Params::output_port_name(&self, index) -> Option<String>`
  (main thread; `None` keeps the layout name) and
  `Params::output_port_names_revision(&self) -> u64` (lock-free, bumped on
  every change). Both default to "no override".
- `moose-derive`: `#[params(output_port_name = "method",
  output_port_names_revision = "method")]` forward to inherent methods.
- `moose-clap`: `clap_plugin_audio_ports::get` reports the runtime name for
  outputs. `process` compares the revision (one atomic load) and, on a
  change, flags it and calls `clap_host::request_callback`; `on_main_thread`
  then calls `clap_host_audio_ports::rescan(CLAP_AUDIO_PORTS_RESCAN_NAMES)`,
  which the spec allows while active, if the host has `clap.audio-ports` and
  does not decline the flag in `is_rescan_flag_supported`. Nothing is
  rescanned from the audio thread.
- `moose-vst3`: two callbacks appended to `Vst3Callbacks` (Rust and shim
  in the same order): `output_bus_name` and `output_bus_names_revision`.
  `getBusInfo` titles output buses with the runtime name, else the layout's
  bus name (before, every output bus was "Output" or "Aux Output").
  `flushPendingRestart`, which already runs on host main-thread callbacks
  and the shim's run-loop timer, polls the revision and raises
  `restartComponent(kIoTitlesChanged)` (1 << 7).

Debouncing is the plugin's job: KONTRA bumps the revision only after the
names have held for 500 ms (`src/routing.rs`, `PortNames`).

A host that ignores the notification keeps the names it read when it last
enumerated the ports; they are current whenever it asks again.

Upstream: this is additive and default-off; worth offering to MOOSE as is.

## Native editor parent contracts

VST3 attachment now rejects null parents, unsupported or null platform types,
and missing open callbacks before changing attachment ownership. CLAP parent
assignment checks the window and API before interpreting the native-handle
union; null and unsupported representations never enter the editor. Valid
NSView, HWND and X11 behavior is preserved. The actual-shim `editor_attach.cpp`
gate covers rejected attachment state and valid attach/remove/reopen; the CLAP
unit fixture covers the native union parser used by `set_parent`.

These checks enforce the [VST3 platform pointer contract](https://steinbergmedia.github.io/vst3_doc/base/group__platformUIType.html)
and prevent malformed native arguments from reaching AppKit or other window
APIs. They do not identify the cause of any reported Mac DAW crash. The existing
`Editor::open` and VST3 `gui_open` return no success value, so these argument
checks do not turn native renderer failure into a verified successful opening.

## Explicit CLAP MPE input capability

The vendored `moose-clap` has a default-off `mpe-input` feature. KONTRA
opts in because its configured MPE zones process member and manager MIDI
expression. `clap.note-ports` then includes `CLAP_NOTE_DIALECT_MIDI_MPE`
for input ports only, alongside CLAP, MIDI and the existing MIDI2 opt-in.
The preferred dialect stays CLAP; output ports do not gain an MPE claim.
This advertises supported processing, not a default zone or automatic zone
negotiation. It does not infer MPE capability from MIDI2.

The [CLAP 1.2.2 note-port contract](https://github.com/free-audio/clap/blob/1.2.2/include/clap/ext/note-ports.h)
defines MIDI_MPE as raw MIDI with polyphonic expression. The real exported
factory/state/process gate is `tools/test_native_clap.py ... --require-mpe`;
it checks every input and output port and plays an original tone through
configured lower-zone member/manager bend and pedal routing.
