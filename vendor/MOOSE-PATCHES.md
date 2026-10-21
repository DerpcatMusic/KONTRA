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

## Parameter presentation reaches CLAP hosts while processing

`Params::parameter_presentation_revision` was compared only in
`on_main_thread`, which nothing scheduled when the revision moved, so a CLAP
host saw new parameter names only after some unrelated main-thread callback.
`process` now compares it (one atomic load) and, once per change, calls
`clap_host::request_callback`; `on_main_thread` rescans as before. KONTRA
names its host automation slots after the controls they drive this way.
Value text follows the presentation too: a presentation change adds
`CLAP_PARAM_RESCAN_TEXT` to the rescan, and the VST3 shim raises
`kParamValuesChanged` (1 << 0) with `kParamTitlesChanged`.

`ParameterPresentation::stepped` marks a parameter whose static range holds
only integer steps at runtime (KONTRA: a slot bound to a switch). CLAP may
change `CLAP_PARAM_IS_STEPPED` only via `RESCAN_ALL` while deactivated, so it
joins the exposed index list: a change while active requests a restart and
is published in `deactivate`. VST3 keeps the static `stepCount`: changing it
needs `kReloadComponent`, which hosts handle as a full reload.

## Floating CLAP GUI on Linux

`gui_create` used to ignore its `api` and `is_floating` arguments, so a host
that asked for an unsupported window (`wayland`, or a floating window the
plug-in never offered) got `true` and an editor no `set_parent` or `show` could
ever open: no display at all. It now refuses anything `is_api_supported` would
refuse.

Linux also offers X11 floating, for a host that cannot embed (native Wayland
has no client-side embedding). `is_api_supported("x11", true)` is `true`; the
editor's window is its own top-level, opened by `show` and closed by `hide`
(the editor keeps its state and reopens). `set_parent` is refused for a
floating GUI. Moose-core has no floating `RawWindowHandle`, so `X11(0)` is the
request for one: `moose-mui` reads it in `MuiEditor::open` and opens through
`mui-baseview::open_floating_threaded`, a baseview window with no parent.
Upstream: a `RawWindowHandle::Floating` variant would replace the sentinel.

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
checks do not turn native renderer failure into a verified successful opening. On Linux,
CLAP `set_parent` answers with the window's own report (`window_scale().is_some()`): a
parent that is not a window (an X `BadWindow` inside baseview) used to return `true` with no
editor, a display that silently never appears. VST3 `gui_open` is unchanged.

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

## Restored state retires off the audio thread

CLAP and VST3 share `moose-state-queue.rs`: the latest pending restore stays in
one slot; the audio consumer applies it by reference and transfers ownership
to two retirement slots. Host/editor publication, inactive drains and main
callbacks destroy consumed blobs. A full retirement queue leaves the pending
restore for the next callback rather than freeing it on audio. The restore
scope also retires ownership if authored `load_state` panics.

CLAP requests its existing main-thread callback after applying a restore.
VST3 reuses the shim's main-thread restart drain (including its macOS wake),
without changing the Rust/C++ ABI. On Linux with the editor closed, a host
that does not poll parameters can retain at most two consumed blobs until
the next recall, main-thread poll or deactivation. The realtime guard now
starts before deferred restore application; custom `load_state` allocations
are observable as realtime violations. Persist parsing remains on the host.

The shared large-state fixture fails on the previous handoff with three audio
frees and passes with zero; order, concurrent recalls, newest-wins and panic
retirement have targeted tests in both wrapper crates. This is ownership
validation, not a native DAW recall or whole-process C++ allocation claim.

## Zero-frame process applies parameter events

`clap_plugin_process` returned before reading the event list when
`frames_count == 0`, so a host that sends parameter changes in a zero-frame
call (a flush spelled as a process) had them dropped. The zero-frame path now
applies `ParamChange` events the way `params_flush` does (the same
`convert_input_events` and `set_plain` sweep) and returns `CONTINUE`. No audio
is rendered. Checked by a CLAP host-matrix test (`edge_flush`). Upstream: additive; worth offering to MOOSE as is.

## Latency changes ask for a restart, not `changed()`

When the engine's latency changed on the audio thread, the main-thread callback
called `clap_host_latency.changed()` and then `request_restart` for an active
plugin. CLAP allows `changed()` only inside `clap_plugin.activate`
(`clap-validator`: "Must only be called within 'clap_plugin::activate'", seen by
`param-fuzz-basic`, `param-fuzz-bounds` and `param-fuzz-sample-accurate`). The
callback now only requests a restart; the host reactivates, and `activate`
refreshes the latency it reads. The `clap.latency` host pointer is no longer
captured. Upstream: worth offering to MOOSE as is.

## X11 keyboard keymap is released with the window

`XkbcommonState::new` (`moose-baseview`, `src/wrappers/xkbcommon.rs`) builds a
keymap with `xkb_x11_keymap_new_from_device` for every X11 window, and the
state takes its own reference to it. Nothing released the keymap, and the
keymap holds a reference to the context, so each editor open leaked one
keymap and one context: 54 kB of malloc memory per open in a live-allocation
trace (71 kB/cycle by mallinfo2 in the soak). The keymap is now unref'd once
the state holds it; dropping the state frees both. Checked by
a 200-cycle GUI open/close soak, which
measured 71.4 kB/cycle before and 0.3 to 0.5 kB/cycle after (200 cycles). Upstream:
worth offering to MOOSE as is.
