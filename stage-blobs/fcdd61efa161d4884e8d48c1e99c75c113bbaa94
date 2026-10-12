//! C ABI types for the Rust / C++ VST3 shim boundary.

use std::ffi::c_void;
use std::mem::{align_of, offset_of, size_of};
use std::os::raw::c_char;

/// Plugin descriptor passed from Rust to the C++ shim.
#[repr(C)]
pub struct Vst3PluginDescriptor {
    pub name: *const c_char,
    pub vendor: *const c_char,
    pub url: *const c_char,
    pub email: *const c_char,
    pub version: *const c_char,
    /// VST3 class ID (16 bytes)
    pub cid: [u8; 16],
    /// "Audio Module Class" for processors
    pub category: *const c_char,
    /// Subcategories like "Fx" or "Instrument|Synth"
    pub subcategories: *const c_char,
    /// Summed channel counts across all buses of the first layout. Kept
    /// for the "does this side have any audio at all" guards and scratch
    /// sizing; the per-bus arrays below drive bus reporting and routing.
    pub num_inputs: u32,
    pub num_outputs: u32,
    /// Number of audio input / output buses (from the first declared
    /// layout). The bus *structure* - how many buses and each one's
    /// `BusKind` - is consistent across a plugin's layouts; only channel
    /// widths vary. The shim reports one VST3 bus per entry, so a
    /// sidechain input surfaces as its own `kBusType_Aux` bus instead of
    /// being summed into the main bus.
    pub num_input_buses: u32,
    pub num_output_buses: u32,
    /// Per-bus role, one byte per bus (`0` = Main, `1` = Sidechain/Aux),
    /// arrays of length `num_input_buses` / `num_output_buses`. The shim
    /// maps `1` to `kBusType_Aux` in `getBusInfo`.
    pub input_bus_kinds: *const u8,
    pub output_bus_kinds: *const u8,
    /// Number of MIDI output ports (event output buses). `0` disables
    /// output events entirely - the host never allocates
    /// `ProcessData::outputEvents` and the drain loop after `process()`
    /// is a no-op. The shim advertises this many `kEvent | kOutput`
    /// buses; the plugin routes each event to a bus via `Event::port`.
    pub midi_output_ports: i32,
    /// Number of MIDI input ports (event input buses). `0` means the
    /// plugin takes no MIDI (decoupled from `num_inputs` so an audio
    /// effect can also take MIDI). The shim advertises this many
    /// `kEvent | kInput` buses and stamps each event's `Event::port`
    /// from the bus it arrived on.
    pub midi_input_ports: i32,
    /// Non-zero when the plugin's `Sample` is `f64`. The shim then
    /// answers `canProcessSampleSize(kSample64)` with `kResultOk`,
    /// accepts a 64-bit `setupProcessing`, and routes blocks through
    /// `process_f64` so the plugin reads/writes host memory directly
    /// with no precision conversion.
    pub supports_f64: i32,
    /// Widest total output-channel count across *all* declared layouts.
    /// The shim sizes its per-channel output discard scratch to this so a
    /// negotiated layout wider than the first can't alias two output
    /// channels onto one discard block (which would be aliased `&mut`s).
    pub max_outputs: u32,
}

/// Parameter descriptor.
#[repr(C)]
pub struct Vst3ParamDescriptor {
    pub id: u32,
    pub name: *const c_char,
    pub short_name: *const c_char,
    pub units: *const c_char,
    pub min: f64,
    pub max: f64,
    pub default_normalized: f64,
    pub step_count: i32,
    pub flags: i32,
    pub group: *const c_char,
}

/// Flat, pointer-free except for borrowed `SysEx` bytes, representation of one
/// VST3 native event. The shim traverses the host's `IEventList` in place and
/// hands events to Rust one at a time, avoiding a fixed-size staging array.
#[repr(C)]
#[derive(Copy, Clone, Default)]
pub struct Vst3NativeEvent {
    pub kind: u32,
    pub sample_offset: i32,
    pub bus_index: i32,
    pub flags: u32,
    pub channel: i16,
    pub pitch: i16,
    pub note_id: i32,
    /// Note-expression type ID, `SysEx` data type, or legacy controller ID.
    pub type_id: u32,
    pub length: i32,
    pub velocity: f32,
    pub tuning: f32,
    pub value: f64,
    pub ppq_position: f64,
    pub bytes: *const u8,
    pub len: u32,
    pub data1: u8,
    pub data2: u8,
}

const _: () = {
    assert!(size_of::<Vst3NativeEvent>() == 72);
    assert!(align_of::<Vst3NativeEvent>() == 8);
    assert!(offset_of!(Vst3NativeEvent, kind) == 0);
    assert!(offset_of!(Vst3NativeEvent, sample_offset) == 4);
    assert!(offset_of!(Vst3NativeEvent, bus_index) == 8);
    assert!(offset_of!(Vst3NativeEvent, flags) == 12);
    assert!(offset_of!(Vst3NativeEvent, channel) == 16);
    assert!(offset_of!(Vst3NativeEvent, pitch) == 18);
    assert!(offset_of!(Vst3NativeEvent, note_id) == 20);
    assert!(offset_of!(Vst3NativeEvent, type_id) == 24);
    assert!(offset_of!(Vst3NativeEvent, length) == 28);
    assert!(offset_of!(Vst3NativeEvent, velocity) == 32);
    assert!(offset_of!(Vst3NativeEvent, tuning) == 36);
    assert!(offset_of!(Vst3NativeEvent, value) == 40);
    assert!(offset_of!(Vst3NativeEvent, ppq_position) == 48);
    assert!(offset_of!(Vst3NativeEvent, bytes) == 56);
    assert!(offset_of!(Vst3NativeEvent, len) == 64);
    assert!(offset_of!(Vst3NativeEvent, data1) == 68);
    assert!(offset_of!(Vst3NativeEvent, data2) == 69);
};

/// Transport info passed from the C++ shim to Rust.
#[repr(C)]
pub struct Vst3Transport {
    pub playing: i32,
    pub recording: i32,
    pub tempo: f64,
    pub time_sig_num: i32,
    pub time_sig_den: i32,
    pub position_samples: f64,
    pub position_beats: f64,
    pub bar_start_beats: f64,
    pub cycle_start_beats: f64,
    pub cycle_end_beats: f64,
    pub cycle_active: i32,
}

/// Parameter change event with sample offset (for sample-accurate automation).
#[repr(C)]
pub struct Vst3ParamChange {
    pub id: u32,
    pub sample_offset: i32,
    pub value: f64, // plain value (already denormalized)
}

/// Callbacks from the C++ shim into Rust.
#[repr(C)]
pub struct Vst3Callbacks {
    pub create: unsafe extern "C" fn() -> *mut c_void,
    pub destroy: unsafe extern "C" fn(ctx: *mut c_void),
    /// `process_mode` is the VST3 `ProcessSetup::processMode`
    /// (`kRealtime` / `kPrefetch` / `kOffline`).
    pub reset: unsafe extern "C" fn(
        ctx: *mut c_void,
        sample_rate: f64,
        max_frames: u32,
        process_mode: i32,
    ),
    pub process: unsafe extern "C" fn(
        ctx: *mut c_void,
        inputs: *const *const f32,
        outputs: *mut *mut f32,
        num_input_channels: u32,
        num_output_channels: u32,
        input_bus_channels: *const u32,
        num_input_buses: u32,
        input_bus_active: u32,
        output_bus_channels: *const u32,
        num_output_buses: u32,
        output_bus_active: u32,
        num_frames: u32,
        transport: *const Vst3Transport,
        param_changes: *const Vst3ParamChange,
        num_param_changes: u32,
        // VST3 `ProcessData::processMode` for this block.
        process_mode: i32,
    ) -> u32,
    /// 64-bit twin of `process`. The shim calls exactly one of the
    /// two per block, chosen by the sample size the host negotiated
    /// in `setupProcessing` (only offered when
    /// `Vst3PluginDescriptor::supports_f64` is set).
    pub process_f64: unsafe extern "C" fn(
        ctx: *mut c_void,
        inputs: *const *const f64,
        outputs: *mut *mut f64,
        num_input_channels: u32,
        num_output_channels: u32,
        input_bus_channels: *const u32,
        num_input_buses: u32,
        input_bus_active: u32,
        output_bus_channels: *const u32,
        num_output_buses: u32,
        output_bus_active: u32,
        num_frames: u32,
        transport: *const Vst3Transport,
        param_changes: *const Vst3ParamChange,
        num_param_changes: u32,
        process_mode: i32,
    ) -> u32,
    pub param_count: unsafe extern "C" fn(ctx: *mut c_void) -> u32,
    pub param_get_value: unsafe extern "C" fn(ctx: *mut c_void, id: u32) -> f64,
    pub param_set_value: unsafe extern "C" fn(ctx: *mut c_void, id: u32, value: f64),
    pub param_normalize: unsafe extern "C" fn(ctx: *mut c_void, id: u32, plain: f64) -> f64,
    pub param_denormalize: unsafe extern "C" fn(ctx: *mut c_void, id: u32, normalized: f64) -> f64,
    pub param_format: unsafe extern "C" fn(
        ctx: *mut c_void,
        id: u32,
        value: f64,
        out: *mut c_char,
        out_len: u32,
    ) -> u32,
    /// Parse host-entered UTF-8 `text` into a plain param value. Returns
    /// `1` and writes `out_plain` on success, `0` when the text can't be
    /// parsed (the shim then reports `kResultFalse` to the host).
    pub param_parse: unsafe extern "C" fn(
        ctx: *mut c_void,
        id: u32,
        text: *const c_char,
        out_plain: *mut f64,
    ) -> i32,
    pub state_save:
        unsafe extern "C" fn(ctx: *mut c_void, out_data: *mut *mut u8, out_len: *mut u32),
    /// Returns `1` when the blob was accepted (moose envelope, or
    /// `migrate_state` translated it), `0` when the load failed -
    /// the shim's `setState` forwards that as `kResultFalse`.
    pub state_load: unsafe extern "C" fn(ctx: *mut c_void, data: *const u8, len: u32) -> i32,
    pub state_free: unsafe extern "C" fn(data: *mut u8, len: u32),
    // Latency + tail
    pub get_latency: unsafe extern "C" fn(ctx: *mut c_void) -> u32,
    pub get_tail: unsafe extern "C" fn(ctx: *mut c_void) -> u32,
    /// Reset the bounded input queue for one block. Native events then arrive
    /// through `push_input_event` in the host's original `IEventList` order.
    pub begin_input_events: unsafe extern "C" fn(ctx: *mut c_void, num_frames: u32),
    /// Returns 1=emitted, 2=unsupported, 3=invalid, 4=queue full. Queue-full
    /// tells the shim to stop traversing so the retained prefix stays ordered.
    pub push_input_event:
        unsafe extern "C" fn(ctx: *mut c_void, event: *const Vst3NativeEvent) -> u32,
    /// Begin one globally ordered output traversal over the lossless event
    /// view. `next_output_event` uses the same result codes plus 0=end.
    pub begin_output_events: unsafe extern "C" fn(ctx: *mut c_void),
    pub next_output_event: unsafe extern "C" fn(ctx: *mut c_void, out: *mut Vst3NativeEvent) -> u32,
    /// Commit the pending note-ID lifecycle mutation only after the host
    /// accepted the event through `IEventList::addEvent`.
    pub commit_output_event: unsafe extern "C" fn(ctx: *mut c_void),
    /// Publish the completed host drain result for the next process block.
    pub finish_output_events: unsafe extern "C" fn(ctx: *mut c_void, status: u32),
    // GUI
    pub gui_has_editor: unsafe extern "C" fn(ctx: *mut c_void) -> i32,
    pub gui_get_size: unsafe extern "C" fn(ctx: *mut c_void, w: *mut u32, h: *mut u32),
    pub gui_open: unsafe extern "C" fn(ctx: *mut c_void, parent: *mut c_void),
    pub gui_close: unsafe extern "C" fn(ctx: *mut c_void),
    /// Host-driven DPI notification via `IPlugViewContentScaleSupport::
    /// setContentScaleFactor`. Passed to the Rust side so the instance
    /// can remember the host scale (used for physical-pixel size
    /// reporting on Windows/Linux) and forward it to the editor.
    pub gui_set_content_scale: unsafe extern "C" fn(ctx: *mut c_void, scale: f64),
    /// `IPlugView::canResize`. Returns `1` for `kResultTrue` if the
    /// editor advertised `can_resize() == true`, else `0`.
    pub gui_can_resize: unsafe extern "C" fn(ctx: *mut c_void) -> i32,
    /// `IPlugView::checkSizeConstraint`. Clamps the requested
    /// physical width / height in place to the editor's
    /// min / max / aspect constraints. Returns `1` for `kResultOk`.
    /// The Ableton-Live behaviour (host calls this even when
    /// `canResize` is false) is handled host-side here: for fixed
    /// editors we snap to the editor's current size and still
    /// return `kResultOk`.
    pub gui_check_size_constraint: unsafe extern "C" fn(ctx: *mut c_void, w: *mut u32, h: *mut u32),
    /// `IPlugView::onSize`. Host commits a new size; delegate to
    /// `Editor::set_size`. Width / height are in physical pixels;
    /// the Rust side scales to logical using the cached host
    /// content-scale before handing to the editor.
    pub gui_set_size: unsafe extern "C" fn(ctx: *mut c_void, w: u32, h: u32),
    /// `IMidiMapping::getMidiControllerAssignment`. Given the event-input
    /// bus, MIDI channel (0..=15), and a VST3 `ControllerNumbers` value
    /// (`0..=127` CC, `kAfterTouch`/`kPitchBend`/`kCtrlProgramChange`),
    /// resolve the bound parameter from the static `midi_map` metadata.
    /// Writes the id via `out_param_id` and returns `1` (`kResultOk`)
    /// on a hit, `0` (`kResultFalse`) otherwise.
    pub midi_mapping_get_param_id: unsafe extern "C" fn(
        ctx: *mut c_void,
        bus_index: i32,
        channel: i16,
        controller: i16,
        out_param_id: *mut u32,
    ) -> i32,
    /// Process-emitted parameter output. The shim drains these after
    /// `process` into the host's `outputParameterChanges` queue, so
    /// hosts see parameter changes the plugin makes during processing.
    /// `value` is normalized `[0,1]`; `sample_offset` is block-relative.
    pub get_output_param_count: unsafe extern "C" fn(ctx: *mut c_void) -> u32,
    pub get_output_param: unsafe extern "C" fn(
        ctx: *mut c_void,
        index: u32,
        out_id: *mut u32,
        out_sample_offset: *mut i32,
        out_value: *mut f64,
    ),
    /// `IComponent::setActive`. `active != 0` between activate and
    /// deactivate. Lets `cb_state_load` tell whether the audio thread
    /// will drain the pending-state queue (active) or whether it must
    /// apply the custom-state blob synchronously (inactive).
    pub set_active: unsafe extern "C" fn(ctx: *mut c_void, active: i32),
    /// Return the `bus_layouts()` index whose main-bus channel counts are
    /// `(in_ch, out_ch)`, or `-1`. Static per plugin type (no `ctx`); the
    /// shim's `setBusArrangements` uses it to accept alternate layouts.
    pub match_bus_layout: unsafe extern "C" fn(in_ch: u32, out_ch: u32) -> i32,
    /// Per-bus channel width of a declared layout. `layout_index` indexes
    /// `bus_layouts()`, `is_output != 0` selects the output direction,
    /// `bus_index` the bus within that direction. Returns the channel
    /// count, or `0` when out of range. The shim reports `getBusInfo` /
    /// `getBusArrangement` per bus from this and sizes its process gather.
    pub layout_bus_channels:
        unsafe extern "C" fn(layout_index: u32, is_output: i32, bus_index: u32) -> u32,
    /// Match a host-proposed *per-bus* arrangement to a declared layout.
    /// `in_channels` / `out_channels` are arrays of per-bus channel counts
    /// (length `num_in` / `num_out`). Returns the matching `bus_layouts()`
    /// index, or `-1`. Replaces the summed `match_bus_layout` for
    /// `setBusArrangements` so a sidechain bus is matched independently.
    pub match_bus_layout_perbus: unsafe extern "C" fn(
        in_channels: *const u32,
        num_in: u32,
        out_channels: *const u32,
        num_out: u32,
    ) -> i32,
    /// Is a parameter flagged `ParamFlags::CHUNKED` (sample-accurate
    /// automation)? Non-zero when the plugin requests sub-block splitting
    /// on this id, so the shim defers its per-offset commits to
    /// `process_chunked` rather than pre-writing the block's end value at
    /// collection time - which would make a mid-block step audible from
    /// sample 0.
    pub param_is_chunked: unsafe extern "C" fn(ctx: *mut c_void, id: u32) -> i32,
    /// `Params::parameter_presentation`: writes the runtime title into
    /// `name` and returns 0 / 1 (hidden), or -1 for "no override".
    pub param_presentation:
        unsafe extern "C" fn(ctx: *mut c_void, id: u32, name: *mut c_char, capacity: u32) -> i32,
    /// `Params::parameter_presentation_revision`.
    pub param_presentation_revision: unsafe extern "C" fn(ctx: *mut c_void) -> u64,
    /// Name of output audio bus `index` of layout `layout`: the plugin's
    /// `Params::output_port_name`, else the layout's bus name. Writes a
    /// NUL-terminated title and returns 1, or 0 for none.
    pub output_bus_name: unsafe extern "C" fn(
        ctx: *mut c_void,
        layout: u32,
        index: u32,
        name: *mut c_char,
        capacity: u32,
    ) -> i32,
    /// `Params::output_port_names_revision`.
    pub output_bus_names_revision: unsafe extern "C" fn(ctx: *mut c_void) -> u64,
}

unsafe extern "C" {
    /// Register the plugin with the VST3 shim.
    pub fn moose_vst3_register(
        descriptor: *const Vst3PluginDescriptor,
        callbacks: *const Vst3Callbacks,
        param_descriptors: *const Vst3ParamDescriptor,
        num_params: u32,
    );

    /// Get the VST3 factory COM object. Called by `GetPluginFactory`.
    pub fn moose_vst3_get_factory() -> *mut std::ffi::c_void;

    /// Notify host: begin editing a parameter (mouse-down).
    pub fn moose_vst3_begin_edit(ctx: *mut std::ffi::c_void, id: u32);

    /// Notify host: parameter value changed during a gesture.
    pub fn moose_vst3_perform_edit(ctx: *mut std::ffi::c_void, id: u32, normalized: f64);

    /// Notify host: end editing a parameter (mouse-up).
    pub fn moose_vst3_end_edit(ctx: *mut std::ffi::c_void, id: u32);

    /// Plugin -> host resize request. Looks up the owning
    /// component via the ctx mapping, walks to the live plug view's
    /// stored `IPlugFrame*`, and calls `IPlugFrame::resizeView`.
    /// Returns `1` on success; `0` when no live view / frame is
    /// available (e.g. editor closed) or when the host refused.
    /// Routes through the component (not the plug view) so that
    /// view re-creation between calls (Cubase theme change, Live
    /// dock/undock) doesn't leave a stale pointer in the closure.
    pub fn moose_vst3_request_resize(ctx: *mut std::ffi::c_void, w: u32, h: u32) -> i32;

    /// Flag a pending `IComponentHandler::restartComponent`. `flags` is a
    /// VST3 `RestartFlags` bitmask - `kLatencyChanged` (8) for a latency
    /// change. Only sets bits on an atomic, so it is safe to call from the
    /// audio thread; the shim drains them via `restartComponent` on the
    /// next host main-thread callback (param read / edit gesture), keeping
    /// the actual host call on the UI thread.
    pub fn moose_vst3_mark_restart(ctx: *mut std::ffi::c_void, flags: i32);
}
