# MOOSE patches

`moose-params`, `moose-derive`, `moose-clap` and `moose-vst3` are vendored
from moose `bffa467` (the rev in `Cargo.toml`) and patched in through
`[patch."https://github.com/Matari-Audio/moose"]`. Their `Cargo.toml`s
spell out what the moose workspace used to inherit; the sources differ only
by the patch below. `moose-mui` is vendored for its own reasons (see its
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
