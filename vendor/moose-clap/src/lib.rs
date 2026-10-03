//! CLAP format wrapper for the moose framework.
//!
//! Provides the [`export_clap!`] macro to expose any
//! `PluginExport` implementation as a CLAP plugin.

// Several `extern "C" fn`s in the CLAP vtable carry a `<P: PluginExport>`
// type parameter even though they don't use `P`. The vtable is built per-`P`
// inside the `export_clap!` macro and uniformity across the table simplifies
// the macro; removing `P` from individual entries would make the macro
// branch on which functions are generic.
#![allow(clippy::extra_unused_type_parameters)]
// CLAP event headers are 4-byte aligned but the extended event types
// (note, param_value, transport, …) require 8-byte alignment. The
// host guarantees the underlying buffer is allocated with the
// extended event's alignment; the `header.cast::<…>()` is reading
// through that promise.
#![allow(clippy::cast_ptr_alignment)]

#[doc(hidden)]
pub mod __macro_deps {
    pub use moose_core;
}

pub mod presets;

use std::ffi::{CStr, CString, c_char, c_void};
use std::marker::PhantomData;
use std::mem::transmute;
use std::path::Path;
use std::ptr;
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use clap_sys::audio_buffer::clap_audio_buffer;
use clap_sys::events::{
    CLAP_CORE_EVENT_SPACE_ID, CLAP_EVENT_DONT_RECORD, CLAP_EVENT_IS_LIVE, CLAP_EVENT_MIDI,
    CLAP_EVENT_MIDI_SYSEX, CLAP_EVENT_MIDI2, CLAP_EVENT_NOTE_CHOKE, CLAP_EVENT_NOTE_END,
    CLAP_EVENT_NOTE_EXPRESSION, CLAP_EVENT_NOTE_OFF, CLAP_EVENT_NOTE_ON,
    CLAP_EVENT_PARAM_GESTURE_BEGIN, CLAP_EVENT_PARAM_GESTURE_END, CLAP_EVENT_PARAM_MOD,
    CLAP_EVENT_PARAM_VALUE, CLAP_EVENT_TRANSPORT, CLAP_NOTE_EXPRESSION_BRIGHTNESS,
    CLAP_NOTE_EXPRESSION_EXPRESSION, CLAP_NOTE_EXPRESSION_PAN, CLAP_NOTE_EXPRESSION_PRESSURE,
    CLAP_NOTE_EXPRESSION_TUNING, CLAP_NOTE_EXPRESSION_VIBRATO, CLAP_NOTE_EXPRESSION_VOLUME,
    CLAP_TRANSPORT_HAS_BEATS_TIMELINE, CLAP_TRANSPORT_HAS_SECONDS_TIMELINE,
    CLAP_TRANSPORT_HAS_TEMPO, CLAP_TRANSPORT_HAS_TIME_SIGNATURE, CLAP_TRANSPORT_IS_LOOP_ACTIVE,
    CLAP_TRANSPORT_IS_PLAYING, CLAP_TRANSPORT_IS_RECORDING, clap_event_header, clap_event_midi,
    clap_event_midi_sysex, clap_event_midi2, clap_event_note, clap_event_note_expression,
    clap_event_param_gesture, clap_event_param_mod, clap_event_param_value, clap_event_transport,
    clap_input_events, clap_output_events,
};
use clap_sys::ext::audio_ports::{
    CLAP_AUDIO_PORT_IS_MAIN, CLAP_AUDIO_PORT_PREFERS_64BITS, CLAP_AUDIO_PORT_SUPPORTS_64BITS,
    CLAP_AUDIO_PORTS_RESCAN_NAMES, CLAP_EXT_AUDIO_PORTS, CLAP_PORT_MONO, CLAP_PORT_STEREO,
    clap_audio_port_info, clap_host_audio_ports, clap_plugin_audio_ports,
};
use clap_sys::ext::audio_ports_config::{
    CLAP_EXT_AUDIO_PORTS_CONFIG, clap_audio_ports_config, clap_plugin_audio_ports_config,
};
use clap_sys::ext::latency::{CLAP_EXT_LATENCY, clap_host_latency, clap_plugin_latency};
use clap_sys::ext::note_ports::{
    CLAP_EXT_NOTE_PORTS, CLAP_NOTE_DIALECT_CLAP, CLAP_NOTE_DIALECT_MIDI, CLAP_NOTE_DIALECT_MIDI2,
    CLAP_NOTE_DIALECT_MIDI_MPE, clap_note_port_info, clap_plugin_note_ports,
};
use clap_sys::ext::params::{
    CLAP_EXT_PARAMS, CLAP_PARAM_IS_AUTOMATABLE, CLAP_PARAM_IS_BYPASS, CLAP_PARAM_IS_ENUM,
    CLAP_PARAM_IS_HIDDEN, CLAP_PARAM_IS_MODULATABLE, CLAP_PARAM_IS_MODULATABLE_PER_NOTE_ID,
    CLAP_PARAM_IS_READONLY, CLAP_PARAM_IS_STEPPED, clap_param_info, clap_plugin_params,
};
use clap_sys::ext::params::{
    CLAP_PARAM_RESCAN_ALL, CLAP_PARAM_RESCAN_INFO, CLAP_PARAM_RESCAN_VALUES, clap_host_params,
};
use clap_sys::ext::preset_load::{
    CLAP_EXT_PRESET_LOAD, CLAP_EXT_PRESET_LOAD_COMPAT, clap_host_preset_load,
    clap_plugin_preset_load,
};
use clap_sys::ext::remote_controls::{
    CLAP_EXT_REMOTE_CONTROLS, CLAP_EXT_REMOTE_CONTROLS_COMPAT, CLAP_REMOTE_CONTROLS_COUNT,
    clap_host_remote_controls, clap_plugin_remote_controls, clap_remote_controls_page,
};
use clap_sys::ext::render::{
    CLAP_EXT_RENDER, CLAP_RENDER_OFFLINE, clap_plugin_render, clap_plugin_render_mode,
};
use clap_sys::ext::state::{CLAP_EXT_STATE, clap_plugin_state};
use clap_sys::ext::tail::{CLAP_EXT_TAIL, clap_host_tail, clap_plugin_tail};
use clap_sys::factory::preset_discovery::{
    CLAP_PRESET_DISCOVERY_LOCATION_FILE, clap_preset_discovery_location_kind,
};
use clap_sys::fixedpoint::{CLAP_BEATTIME_FACTOR, CLAP_SECTIME_FACTOR};
use clap_sys::host::clap_host;
use clap_sys::id::{CLAP_INVALID_ID, clap_id};
use clap_sys::plugin::{clap_plugin, clap_plugin_descriptor};
use clap_sys::plugin_features::{
    CLAP_PLUGIN_FEATURE_ANALYZER, CLAP_PLUGIN_FEATURE_AUDIO_EFFECT, CLAP_PLUGIN_FEATURE_INSTRUMENT,
    CLAP_PLUGIN_FEATURE_NOTE_EFFECT, CLAP_PLUGIN_FEATURE_SYNTHESIZER, CLAP_PLUGIN_FEATURE_UTILITY,
};
use clap_sys::process::{
    CLAP_PROCESS_CONTINUE, CLAP_PROCESS_CONTINUE_IF_NOT_QUIET, CLAP_PROCESS_ERROR,
    CLAP_PROCESS_SLEEP, CLAP_PROCESS_TAIL, clap_process,
};
use clap_sys::stream::{clap_istream, clap_ostream};
use clap_sys::string_sizes::{CLAP_NAME_SIZE, CLAP_PATH_SIZE};
use clap_sys::version::CLAP_VERSION;

use moose_core::TransportSlot;
use moose_core::buffer::AudioBuffer;
use moose_core::bus::{BusConfig, BusKind, ChannelConfig};
use moose_core::bus_routing::{BusActivation, BusRouting, bus_layouts_fit_routing};
use moose_core::cast::{len_u32, size_of_u32};
use moose_core::chunked_process::{ChunkedProcess, process_chunked_with_bus_routing};
use moose_core::config::{AudioConfig, ProcessMode};
use moose_core::editor::{
    ClosureBridge, Editor, EditorBuilder, PluginContext, RawWindowHandle, SendPtr, fit_logical_size,
};
use moose_core::events::{
    EVENT_LIST_PREALLOC, Event, EventBody, EventList, ExactEvent, ExactEventBody,
    ExactEventQualifiers, ExactEventRef, ExactNoteAddress, ExactNoteKind, LosslessEventRef,
    OutputEventStatus, PushError, RawMidi1, RawUmp, TransportInfo,
};
use moose_core::export::PluginExport;
use moose_core::info::{MidiDialect, PluginCategory, PluginInfo, resolve_name_override};
use moose_core::meters::MeterStore;
#[cfg(test)]
use moose_core::midi::per_note_bend_semitones;
use moose_core::midi::{
    PER_NOTE_VOLUME_MAX_GAIN, decode_short_message, denorm_7bit, downconvert_to_midi1,
    per_note_bend_from_semitones, pitch_bend_to_bytes,
};
use moose_core::plugin::PluginRuntime;
use moose_core::presets::parse_preset_file;
use moose_core::process::ProcessStatus;
use moose_core::rt::{RtSection, audit};
use moose_core::snapshot::SnapshotSlot;
use moose_core::state;
use moose_core::state::PluginFormat;
use moose_core::tasks::AnyTaskSpawner;
use moose_core::ump::{decode_ump_channel_voice_2, encode_ump_channel_voice_2};
use moose_core::wrapper::{
    PluginCell, SharedPlugin, copy_c_str, enter_plugin, run_audio_block_with,
    run_extern_callback_with, save_extra, shared_plugin,
};
use moose_core::{Float, Sample};
use moose_params::Params;
use moose_params::{ParamFlags, ParamInfo, ParamRange};

// ---------------------------------------------------------------------------
// GUI → host parameter change queue
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
enum GuiParamChange {
    GestureBegin(u32),
    Value(u32, f64), // (param_id, plain_value)
    GestureEnd(u32),
}

/// Wait-free bounded queue for GUI-initiated parameter changes.
///
/// GUI thread pushes, audio thread drains. Every push and pop is O(1)
/// and allocation-free, so the audio thread never blocks on the GUI
/// thread.
///
/// Capacity sized for the worst case "user wiggles every param at
/// once during MIDI-learn": ~64 widgets × (begin + value + end) per
/// gesture, with several blocks of headroom before the audio thread
/// next drains. Overflow drops the change - blocking or panicking
/// from the audio path is worse, and the host's next automation tick
/// recovers the lost values via the param tree. Per-instance memory
/// is `CAPACITY * sizeof(GuiParamChange)` (≈ 32 KB), one-time at
/// instance creation.
const GUI_QUEUE_CAPACITY: usize = 1024;
type GuiChangeQueue = crossbeam_queue::ArrayQueue<GuiParamChange>;

/// Bounded handoff slot for state loads. Capacity 1: presets don't
/// arrive faster than the audio thread completes a block, and on
/// overflow we want most-recent-wins (`force_push`) so a rapid
/// double-recall doesn't get the audio thread to apply a stale state
/// after the host already moved on.
type StateLoadQueue = crossbeam_queue::ArrayQueue<state::DeserializedState>;

// ---------------------------------------------------------------------------
// Internal wrapper struct held as plugin_data
// ---------------------------------------------------------------------------

struct ClapPluginData<P: PluginExport> {
    /// The user's plugin instance in the wrapper-standard ownership
    /// cell: the audio thread owns it per block, host lifecycle
    /// callbacks own it while processing is stopped, and the two never
    /// overlap. `state_save` and the editor's `get_state` read the
    /// lock-free snapshot instead, so they never touch it. See
    /// `moose_core::wrapper::SharedPlugin`.
    plugin: SharedPlugin<P>,
    /// Stable handle to the params Arc, set once at instance creation.
    /// Host-thread callbacks (`params_get_value`, `params_value_to_text`,
    /// `params_text_to_value`) read params through this handle so a
    /// param query never touches the plugin. Params are
    /// atomic-backed and `Sync`.
    params_arc: Arc<P::Params>,
    /// Shared meter storage, set once at instance creation. The
    /// editor's `get_meter` closure reads these atomic slots instead
    /// of the plugin instance.
    meter_store: Arc<MeterStore>,
    /// Lock-free custom-state slot the audio thread publishes into, read
    /// by `save_state` so a snapshot-capable plugin's save never touches
    /// the plugin. Cached here on the instance, like `params_arc`.
    snapshot: Arc<SnapshotSlot>,
    /// The plugin's background-task spawner (`None` unless it wired
    /// `tasks:`), cached at creation so the editor schedules without
    /// touching the plugin.
    task_spawner: Option<AnyTaskSpawner>,
    /// Lock-free editor factory, cached at creation. Building the editor
    /// through this never touches the plugin (`--shell` builds rebuild
    /// from the reloaded dylib, so GUI edits hot-reload).
    editor_builder: EditorBuilder<P::Params>,
    /// Atomic snapshots of the plugin's most recent `latency()` /
    /// `tail()`. Refreshed at `activate` / `reset` (from the real,
    /// resolved sample rate) and after every process block, so
    /// `latency_get` / `tail_get` read the value without touching
    /// `data.plugin`.
    latency_cache: AtomicU32,
    tail_cache: AtomicU32,
    /// Index into `P::bus_layouts()` of the port config the host last
    /// selected through `clap.audio-ports-config` (0 = the default first
    /// layout). Read by the audio-ports extension so a config switch is
    /// reflected in the ports the host sees. Set on the main thread while
    /// the plugin is deactivated; the audio scratch is pre-sized to the
    /// widest layout, so a wider selection never allocates in `process`.
    selected_config: AtomicU32,
    /// Audio + lifecycle-owned per-block scratch. Behind a `PluginCell` so
    /// every callback reaches it through a shared `&ClapPluginData` - never a
    /// whole-struct `&mut`, which would alias a concurrent host-thread `&`
    /// (param reads, GUI) and is UB under the aliasing model. The CLAP host
    /// contract serializes its owners: `process` and `reset` on the audio
    /// thread (`reset` is `[audio-thread & active]`, never concurrent with
    /// `process`), `activate` / `deactivate` on the main thread while
    /// stopped - so the cell is never held twice at once.
    audio: PluginCell<ClapAudio<P>>,
    /// Cached parameter infos (built once at init).
    param_infos: Vec<ParamInfo>,
    /// Visible CLAP indices; backing parameter IDs and storage stay fixed.
    exposed_param_indices: Mutex<Vec<usize>>,
    parameter_restart_pending: AtomicBool,
    /// Cached plugin info. Read by the chunker each block for
    /// `automation.min_subblock_samples` and otherwise unused; the
    /// rest of the wrapper consumes `PluginInfo` from the C ABI
    /// surface we build at registration.
    info: PluginInfo,
    /// Pre-hashed plugin ID for state serialization.
    plugin_id_hash: u64,
    /// Main/UI-thread-owned editor state, behind a `PluginCell` for the same
    /// reason as `audio`. Its owners are the GUI callbacks, all on the host
    /// main thread; `process` never touches it.
    gui: PluginCell<ClapGui>,
    /// Host pointer (for querying host extensions).
    host: *const clap_host,
    /// Host params extension (for `request_flush`).
    host_params: *const clap_host_params,
    /// Host latency extension (for `changed`). Null if the host doesn't
    /// expose `clap.latency`.
    host_latency: *const clap_host_latency,
    /// Host tail extension (for `changed`). Null if the host doesn't
    /// expose `clap.tail`.
    host_tail: *const clap_host_tail,
    /// Host audio-ports extension (for a names-only `rescan`). Null if the
    /// host doesn't expose `clap.audio-ports`.
    host_audio_ports: *const clap_host_audio_ports,
    /// Last `Params::output_port_names_revision` seen by `process`.
    port_names_revision: AtomicU64,
    /// Set on the audio thread when the port names changed; drained on
    /// the main thread, which asks the host to rescan the names.
    port_names_dirty: AtomicBool,
    /// Set on the audio thread when `latency()` changes; drained on the
    /// main thread, which notifies the host. Coalesces a burst of
    /// changes into one host notification per main-thread callback.
    latency_dirty: AtomicBool,
    /// Queue of GUI-initiated parameter changes to emit as output events.
    gui_changes: Arc<GuiChangeQueue>,
    /// Bounded SPSC handoff for state loads. Host (`state_load`) and
    /// editor (`set_state` callback) deserialize on their thread and
    /// push the result; the audio thread pops at the top of
    /// `clap_plugin_process` and calls [`state::apply_state`] under
    /// its exclusive `&mut plugin`. The queue is what makes the
    /// transfer race-free: no GUI-thread `&mut plugin` is ever
    /// constructed.
    pending_state: Arc<StateLoadQueue>,
    /// `true` between `activate` and `deactivate`. `state_load` reads it
    /// to decide whether the audio thread will drain `pending_state`: if
    /// inactive, no `process` runs, so it applies the custom-state blob
    /// synchronously instead of leaving it stranded in the queue (which
    /// would let a following `get_state` re-serialize stale extra
    /// state). Main-thread-only (`activate` / `deactivate` / `state_load`
    /// are serialized by the host).
    active: AtomicBool,
    /// Current render mode as a [`ProcessMode`] discriminant. The host
    /// sets it through the `clap.render` extension on the main thread;
    /// the audio thread reads it each block. Most hosts deactivate
    /// before switching to offline, so `activate` re-preps with the new
    /// mode; a mode change while active still reaches `process` through
    /// the per-block `ProcessContext`, but reallocation waits for the
    /// next `activate`.
    render_mode: AtomicU8,
    /// Flag: GUI changed params, need rescan on main thread.
    needs_rescan: Arc<AtomicBool>,
    /// Last `Params::parameter_presentation_revision` the host saw.
    presentation_revision: AtomicU64,
    /// Shared transport slot: audio thread writes each block, editor reads.
    transport_slot: Arc<TransportSlot>,
    /// Host-reported GUI scale (via `clap_plugin_gui::set_scale`).
    /// Sources of truth, by platform:
    /// - **macOS**: `gui_set_scale` refuses, so this stays unset
    ///   (`AppKit` handles backing scale through the parent `NSView`; we
    ///   report logical points and let the OS scale).
    /// - **Windows / Linux**: used at `gui_get_size` to convert
    ///   logical→physical. Unset, the editor's own window scale is used
    ///   instead (see `window_scale`): 1.0 in a non-DPI-aware host, the
    ///   monitor DPI or `Xft.dpi` otherwise. HiDPI-aware hosts call
    ///   `set_scale` before `gui_get_size`.
    ///
    /// f64 bits, `0` until the host calls `set_scale`; GUI-thread-only, but atomic so `gui_get_size`, `set_scale`,
    /// and the `gui_create` resize closure reach it through the shared
    /// `&ClapPluginData` without a `&mut *ctx` - and so it stays outside the
    /// `gui` cell, which the resize closure would otherwise re-enter.
    host_scale: AtomicU64,
    /// The open editor's own window scale (`Editor::window_scale`, f64 bits,
    /// `0` = none), refreshed by the size callbacks. Converts sizes while the
    /// host never called `set_scale`, so the reported size matches the child
    /// window the editor created at the monitor DPI / `Xft.dpi`.
    window_scale: AtomicU64,
    /// A resize the plugin requested through `PluginContext::request_resize`
    /// that came back into a GUI callback (`gui_set_size` / `gui_get_size`)
    /// synchronously - the host answering `request_host_resize` on the same
    /// thread - while the `gui` cell was already held. Stashed as physical
    /// `(w << 32) | h` (both non-zero, `0` = none) instead of re-entering the
    /// cell; `gui_get_size` applies it on the next size query. GUI thread only.
    pending_resize: AtomicU64,
    /// The immutable extension vtables this instance hands the host from
    /// `clap_plugin_get_extension`. Stored per-instance rather than in a
    /// generic function-local static: a static inside a generic fn is
    /// shared across every monomorphization, so a binary carrying two
    /// plugin types would reinterpret one type's vtables as the other's.
    /// The pointers returned point into this box and stay valid for the
    /// instance's lifetime, which is exactly how long the host uses them.
    extensions: Extensions<P>,
}

/// Audio + lifecycle-owned per-block scratch (see [`ClapPluginData::audio`]).
struct ClapAudio<P: PluginExport> {
    /// Re-usable event list for converting CLAP events each process call.
    event_list: EventList,
    /// Note lifecycle tracker backing wildcard release/expression matching and
    /// successful outbound `NOTE_END` cleanup. Audio-thread only.
    sounding_notes: SoundingNotes,
    /// Re-usable output event list for the process context.
    output_events: EventList,
    /// Per-sub-block scratch the chunker writes rebased events into
    /// while walking the audio block. Pre-allocated to the same
    /// capacity as `event_list` so steady-state operation stays
    /// allocation-free.
    sub_event_scratch: EventList,
    /// Param changes a stopped-transport `params_flush` applied, kept
    /// latest-wins per id and replayed at offset 0 of the next process
    /// block, so the plugin sees the host's explicit events (including
    /// same-value ones) rather than only a changed snapshot. Capacity is
    /// the param count, so it never grows.
    deferred_params: Vec<(u32, f64)>,
    /// Current sample rate.
    sample_rate: f64,
    /// Current max block size.
    max_block_size: usize,
    /// Persistent input/output channel-slice scratch reused across
    /// process callbacks so the audio thread doesn't allocate per
    /// block. The `'static` annotation is fictional - the slices
    /// actually point into the host's per-block buffers; each
    /// `process` call rebuilds them and clears them on exit so no
    /// dangling pointer lives between blocks.
    input_slices: Vec<&'static [<P as PluginRuntime>::Sample]>,
    output_slices: Vec<&'static mut [<P as PluginRuntime>::Sample]>,
    /// Per-channel input conversion scratch. A slot stays empty when
    /// the host wire for its port matches `P::Sample` (the slice in
    /// `input_slices` points straight into host memory); on a
    /// precision mismatch the channel is converted into the matching
    /// slot here and the slice points there.
    input_widen: Vec<Vec<<P as PluginRuntime>::Sample>>,
    /// Per-channel output conversion scratch. Same shape: only used
    /// on a precision mismatch, in which case the plugin writes here
    /// and the wrapper copies + converts back to the host's output
    /// pointers after `process()` returns.
    output_narrow: Vec<Vec<<P as PluginRuntime>::Sample>>,
    /// Cached pointers to host output channels, captured at slice
    /// build time so the post-`process` convert-back loop can copy
    /// without re-walking the CLAP bus structures. Each entry is
    /// tagged with the wire precision the host picked for its port.
    host_out_ptrs: Vec<HostOutPtr>,
    /// Selected layout's structural bus order and flattened ranges. Built at
    /// activate; CLAP exposes no truthful per-block connection state.
    bus_routing: BusRouting,
}

/// Main/UI-thread-owned editor state (see [`ClapPluginData::gui`]).
struct ClapGui {
    /// GUI editor (created by the plugin, if it implements `editor()`).
    editor: Option<Box<dyn Editor>>,
    /// Whether the GUI has been created via the gui extension.
    gui_created: bool,
}

impl<P: PluginExport> ClapPluginData<P> {
    /// The scale between the host's physical pixels and the editor's logical
    /// size: the host's `gui.set_scale` value, else the open editor's window
    /// scale, else `1.0`. Never both multiplied.
    fn host_scale(&self) -> f64 {
        self.reported_host_scale()
            .or_else(|| match self.window_scale.load(Ordering::Relaxed) {
                0 => None,
                bits => Some(f64::from_bits(bits)),
            })
            .unwrap_or(1.0)
    }

    /// Cache `editor`'s window scale for [`Self::host_scale`].
    fn note_window_scale(&self, editor: &dyn Editor) {
        let bits = editor
            .window_scale()
            .filter(|s| s.is_finite() && *s > 0.0)
            .map_or(0, f64::to_bits);
        self.window_scale.store(bits, Ordering::Relaxed);
    }

    /// The last scale the host passed to `gui.set_scale`, if it ever did.
    /// Bits `0` (`+0.0`, which `gui_set_scale` rejects) mean "never".
    fn reported_host_scale(&self) -> Option<f64> {
        match self.host_scale.load(Ordering::Relaxed) {
            0 => None,
            bits => Some(f64::from_bits(bits)),
        }
    }

    fn set_host_scale(&self, scale: f64) {
        self.host_scale.store(scale.to_bits(), Ordering::Relaxed);
    }
}

/// Host output channel pointer, tagged with the port's wire
/// precision (the host picks 32- or 64-bit per port; 64-bit only on
/// ports that advertised `CLAP_AUDIO_PORT_SUPPORTS_64BITS`).
#[derive(Copy, Clone)]
enum HostOutPtr {
    Null,
    F32(*mut f32),
    F64(*mut f64),
}

/// Build the plugin-facing input slice for one host channel of wire
/// precision `H`: zero-copy when `H` matches the plugin's sample
/// type `S`, otherwise converted into `scratch` (f64 round-trip,
/// lossless in the widening direction).
///
/// # Safety
/// A non-null `host_ptr` must address `num_frames` readable samples
/// that stay valid for the block; the returned slice's `'static`
/// lifetime is fictional (same convention as `input_slices`).
unsafe fn input_channel_slice<S: Sample, H: Sample>(
    host_ptr: *const H,
    num_frames: usize,
    scratch: &mut Vec<S>,
) -> &'static [S] {
    if host_ptr.is_null() {
        // Unconnected channel (an unrouted sidechain port). The plugin
        // negotiated it, so hand back block-length silence, not an empty
        // slice it would read out of range.
        scratch.clear();
        scratch.resize(num_frames, S::default());
        // SAFETY: `scratch` outlives the returned slice (same fictional
        // 'static convention as the non-null path).
        return unsafe { std::slice::from_raw_parts(scratch.as_ptr(), num_frames) };
    }
    unsafe {
        if S::IS_F64 == H::IS_F64 {
            // SAFETY: sealed traits - equal IS_F64 means S == H.
            return std::slice::from_raw_parts(host_ptr.cast::<S>(), num_frames);
        }
        scratch.clear();
        scratch.reserve(num_frames);
        let host = std::slice::from_raw_parts(host_ptr, num_frames);
        for &h in host {
            scratch.push(S::from_f64(h.to_f64()));
        }
        std::slice::from_raw_parts(scratch.as_ptr(), num_frames)
    }
}

/// Output twin of [`input_channel_slice`]: zero-copy into host
/// memory when precisions match, otherwise the plugin renders into
/// `scratch` and the post-`process` loop converts back to the host
/// pointer.
///
/// # Safety
/// Same contract as [`input_channel_slice`], with `num_frames`
/// writable samples.
unsafe fn output_channel_slice<S: Sample, H: Sample>(
    host_ptr: *mut H,
    num_frames: usize,
    scratch: &mut Vec<S>,
) -> &'static mut [S] {
    if host_ptr.is_null() {
        // Unconnected output channel: block-length discard scratch to
        // write into rather than an empty slice. The caller records
        // `HostOutPtr::Null`, so the copy-back skips this slot.
        scratch.clear();
        scratch.resize(num_frames, S::default());
        // SAFETY: `scratch` outlives the returned slice.
        return unsafe { std::slice::from_raw_parts_mut(scratch.as_mut_ptr(), num_frames) };
    }
    unsafe {
        if S::IS_F64 == H::IS_F64 {
            // SAFETY: sealed traits - equal IS_F64 means S == H.
            return std::slice::from_raw_parts_mut(host_ptr.cast::<S>(), num_frames);
        }
        scratch.clear();
        scratch.resize(num_frames, S::default());
        std::slice::from_raw_parts_mut(scratch.as_mut_ptr(), num_frames)
    }
}

// ---------------------------------------------------------------------------
// Descriptor management
// ---------------------------------------------------------------------------

/// Holds all the C strings and the descriptor itself. Lives for the process
/// lifetime via a `static` produced by the macro.
pub struct DescriptorHolder {
    pub descriptor: clap_plugin_descriptor,
    // Prevent dropping CStrings that the descriptor points into.
    _id: CString,
    _name: CString,
    _vendor: CString,
    _url: CString,
    _manual_url: Option<CString>,
    _support_url: Option<CString>,
    _version: CString,
    _description: Option<CString>,
    _features: Vec<*const c_char>,
    _features_storage: Vec<CString>,
}

unsafe impl Send for DescriptorHolder {}
unsafe impl Sync for DescriptorHolder {}

/// Plugin display-name in host browsers. Reads `moose.toml`'s
/// `clap_name` (baked into `PluginInfo` by `moose::plugin_info!`),
/// falling back to `PluginInfo::name`.
fn resolved_name(info: &PluginInfo) -> &'static str {
    resolve_name_override(info.clap_name, info.name)
}

impl DescriptorHolder {
    #[must_use]
    pub fn new(info: &PluginInfo) -> Self {
        let id = CString::new(info.clap_id).unwrap_or_default();
        let name = CString::new(resolved_name(info)).unwrap_or_default();
        let vendor = CString::new(info.vendor).unwrap_or_default();
        let url = CString::new(info.url).unwrap_or_default();
        let manual_url = info
            .clap_manual_url
            .and_then(|value| CString::new(value).ok());
        let support_url = info
            .clap_support_url
            .and_then(|value| CString::new(value).ok());
        let version = CString::new(info.version).unwrap_or_default();
        let description = info.description.and_then(|value| CString::new(value).ok());

        let default_features: &[&CStr] = match info.category {
            PluginCategory::Instrument => &[
                CLAP_PLUGIN_FEATURE_INSTRUMENT,
                CLAP_PLUGIN_FEATURE_SYNTHESIZER,
            ],
            PluginCategory::NoteEffect => &[CLAP_PLUGIN_FEATURE_NOTE_EFFECT],
            PluginCategory::Effect => &[CLAP_PLUGIN_FEATURE_AUDIO_EFFECT],
            // Analyzer / Tool still process audio (passthrough), so keep
            // AUDIO_EFFECT for insert menus, but lead with the specific
            // feature so a feature-filtered browser surfaces them.
            PluginCategory::Analyzer => &[
                CLAP_PLUGIN_FEATURE_ANALYZER,
                CLAP_PLUGIN_FEATURE_AUDIO_EFFECT,
            ],
            PluginCategory::Tool => &[
                CLAP_PLUGIN_FEATURE_UTILITY,
                CLAP_PLUGIN_FEATURE_AUDIO_EFFECT,
            ],
        };
        let configured_features = (!info.clap_features.is_empty()).then(|| {
            info.clap_features
                .iter()
                .map(|feature| CString::new(*feature))
                .collect::<Result<Vec<_>, _>>()
                .ok()
        });
        let features_storage = configured_features.flatten().unwrap_or_else(|| {
            default_features
                .iter()
                .map(|feature| (*feature).to_owned())
                .collect()
        });

        let mut features: Vec<*const c_char> =
            features_storage.iter().map(|f| f.as_ptr()).collect();
        features.push(ptr::null());

        let descriptor = clap_plugin_descriptor {
            clap_version: CLAP_VERSION,
            id: id.as_ptr(),
            name: name.as_ptr(),
            vendor: vendor.as_ptr(),
            url: url.as_ptr(),
            manual_url: manual_url
                .as_ref()
                .map_or(ptr::null(), |value| value.as_ptr()),
            support_url: support_url
                .as_ref()
                .map_or(url.as_ptr(), |value| value.as_ptr()),
            version: version.as_ptr(),
            description: description
                .as_ref()
                .map_or(ptr::null(), |value| value.as_ptr()),
            features: features.as_ptr(),
        };

        Self {
            descriptor,
            _id: id,
            _name: name,
            _vendor: vendor,
            _url: url,
            _manual_url: manual_url,
            _support_url: support_url,
            _version: version,
            _description: description,
            _features: features,
            _features_storage: features_storage,
        }
    }
}

// ---------------------------------------------------------------------------
// Helper: copy a Rust &str into a fixed-size [c_char; N] array
// ---------------------------------------------------------------------------

/// Copy `src` into the fixed-size C-string array `dst` (name / vendor /
/// module buffers), truncating on a UTF-8 char boundary and NUL-terminating.
/// Thin slice-typed front for [`copy_c_str`] so the truncation rule lives in
/// one place.
fn copy_str_to_buf(dst: &mut [c_char], src: &str) {
    debug_assert!(!dst.is_empty(), "copy_str_to_buf needs room for the NUL");
    // SAFETY: `dst` is a valid, non-empty slice of `dst.len()` `c_char`.
    unsafe {
        let _ = copy_c_str(dst.as_mut_ptr(), dst.len(), src);
    }
}

// ---------------------------------------------------------------------------
// Helper: get &mut ClapPluginData<P> from a *const clap_plugin
// ---------------------------------------------------------------------------

unsafe fn data_from_plugin<P: PluginExport>(
    plugin: *const clap_plugin,
) -> &'static ClapPluginData<P> {
    unsafe { &*(*plugin).plugin_data.cast::<ClapPluginData<P>>() }
}

// ---------------------------------------------------------------------------
// Plugin callbacks
//
// SAFETY for all unsafe extern "C" fn in this file:
// - `plugin` is the clap_plugin pointer returned by create_plugin_instance().
// - `(*plugin).plugin_data` is a Box::into_raw'd ClapPluginData<P>,
//   valid for the plugin's lifetime. The host guarantees it is not
//   freed until after clap_plugin.destroy() returns.
// - Audio-thread callbacks (process, start/stop_processing) have
//   exclusive access - the host never calls them concurrently.
// - Main-thread callbacks (init, destroy, activate, deactivate,
//   gui_*, params on main thread) are serialized by the host.
// - params_flush may be called from the audio thread while process
//   is not active, or from the main thread - never concurrently
//   with process().
// - Audio buffer pointers (inputs/outputs in clap_process) are
//   valid for the declared channel count × frame count. The host
//   guarantees non-aliasing between input and output buffers.
// ---------------------------------------------------------------------------

unsafe extern "C" fn clap_plugin_init<P: PluginExport>(plugin: *const clap_plugin) -> bool {
    // Author `init` runs here; firewall it so a panic can't unwind across
    // the C ABI (false = "init failed", the host then discards the plugin).
    run_extern_callback_with::<P, bool>("CLAP", "init", false, || unsafe {
        // `init` runs once on the main thread before the host publishes the
        // instance to any audio thread, so an exclusive `&mut` to the whole
        // struct is sound here - the write-once fields below (`param_infos`,
        // the host extension pointers) have no concurrent reader yet.
        let data = &mut *(*plugin).plugin_data.cast::<ClapPluginData<P>>();
        {
            let mut instance = enter_plugin(&data.plugin);
            instance.init();
            data.param_infos = instance.params().param_infos();
            data.exposed_param_indices = Mutex::new(parameter_indices(&data.param_infos, |id| {
                data.params_arc.parameter_presentation(id)
            }));
        }
        // Query host params extension for request_flush support
        if !data.host.is_null()
            && let Some(get_ext) = (*data.host).get_extension
        {
            let ext = get_ext(data.host, CLAP_EXT_PARAMS.as_ptr());
            if !ext.is_null() {
                data.host_params = ext.cast::<clap_host_params>();
            }
            let lat_ext = get_ext(data.host, CLAP_EXT_LATENCY.as_ptr());
            if !lat_ext.is_null() {
                data.host_latency = lat_ext.cast::<clap_host_latency>();
            }
            let tail_ext = get_ext(data.host, CLAP_EXT_TAIL.as_ptr());
            if !tail_ext.is_null() {
                data.host_tail = tail_ext.cast::<clap_host_tail>();
            }
            let ports_ext = get_ext(data.host, CLAP_EXT_AUDIO_PORTS.as_ptr());
            if !ports_ext.is_null() {
                data.host_audio_ports = ports_ext.cast::<clap_host_audio_ports>();
            }
        }
        true
    })
}

unsafe extern "C" fn clap_plugin_destroy<P: PluginExport>(plugin: *const clap_plugin) {
    unsafe {
        // Wrap the drop in `catch_unwind`. Dropping the
        // `ClapPluginData` cascades into the editor's `Drop`,
        // which tears down the wgpu surface / `NSView` /
        // baseview / runloop timers. A panic anywhere in that
        // chain propagates across this `extern "C"` boundary as
        // UB - hosts catch it as an Obj-C exception,
        // `objc_exception_rethrow` can't recover, and
        // `std::terminate` aborts the host (the REAPER / Cubase
        // SIGABRT pattern). Hosts destroy instances routinely
        // mid-session - removing a plugin from a track, closing a
        // project - not just at quit, so catching here keeps a
        // live host alive; the instance is being torn down anyway.
        let plugin_ptr = plugin.cast_mut();
        let data_ptr = (*plugin).plugin_data.cast::<ClapPluginData<P>>();
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            drop(Box::from_raw(data_ptr));
            drop(Box::from_raw(plugin_ptr));
        }));
    }
}

unsafe extern "C" fn clap_plugin_activate<P: PluginExport>(
    plugin: *const clap_plugin,
    sample_rate: f64,
    _min_frames_count: u32,
    max_frames_count: u32,
) -> bool {
    // Author `reset` runs here; firewall it (false = "activate failed").
    run_extern_callback_with::<P, bool>("CLAP", "activate", false, || unsafe {
        let data = data_from_plugin::<P>(plugin);
        let mut audio = data.audio.enter();
        audio.sample_rate = sample_rate;
        let max_block = max_frames_count as usize;
        audio.max_block_size = max_block;
        let layouts = P::bus_layouts();
        let selected = (data.selected_config.load(Ordering::Relaxed) as usize)
            .min(layouts.len().saturating_sub(1));
        audio.bus_routing = BusRouting::new();
        if let Some(layout) = layouts.get(selected) {
            for bus in &layout.inputs {
                if !audio.bus_routing.push_input(
                    if bus.enabled {
                        bus.channels.channel_count()
                    } else {
                        0
                    },
                    BusActivation::Unknown,
                ) {
                    return false;
                }
            }
            for bus in &layout.outputs {
                if !audio.bus_routing.push_output(
                    if bus.enabled {
                        bus.channels.channel_count()
                    } else {
                        0
                    },
                    BusActivation::Unknown,
                ) {
                    return false;
                }
            }
        }
        let mode = ProcessMode::from_u8(data.render_mode.load(Ordering::Relaxed));
        {
            let mut instance = enter_plugin(&data.plugin);
            instance.reset(&AudioConfig::new(sample_rate, max_block).with_process_mode(mode));
            // Refresh the caches from the freshly-reset instance so the
            // host reads the real latency/tail before the first block -
            // not the placeholder-rate value from create. No dirty flag:
            // this is the baseline the host reads at activate, not a
            // mid-session change needing changed() / request_restart.
            data.latency_cache
                .store(instance.latency(), Ordering::Relaxed);
            data.tail_cache.store(instance.tail(), Ordering::Relaxed);
        }

        // Pre-grow the widening / narrowing scratch so no host wire /
        // plugin precision combination allocates on the audio thread.
        // Without this, the first block after `activate` hits the global
        // allocator inside `clap_plugin_process` to grow the outer Vec and
        // each channel's inner Vec - a real RT hazard. The outer-Vec
        // capacity is already reserved in `create_plugin`; here we push the
        // inner per-channel `Vec<P::Sample>`s up to `max_block_size` frames
        // so the per-block `.clear() + .reserve()` path never allocates.
        //
        // Unconditional, both directions: an f64 plugin converts whenever a
        // host sends f32, and an f32 plugin converts if a (non-compliant)
        // host sends f64 to a port that never advertised
        // `SUPPORTS_64BITS`. Pre-growing for the f32 plugin keeps that
        // defensive path allocation-free too; its scratch just stays unused
        // on the common zero-copy path.
        {
            let scr = &mut *audio;
            let max_in = scr.input_widen.capacity();
            let max_out = scr.output_narrow.capacity();
            while scr.input_widen.len() < max_in {
                scr.input_widen.push(Vec::with_capacity(max_block));
            }
            for buf in &mut scr.input_widen {
                if buf.capacity() < max_block {
                    buf.reserve_exact(max_block - buf.capacity());
                }
            }
            while scr.output_narrow.len() < max_out {
                scr.output_narrow.push(Vec::with_capacity(max_block));
            }
            for buf in &mut scr.output_narrow {
                if buf.capacity() < max_block {
                    buf.reserve_exact(max_block - buf.capacity());
                }
            }
            while scr.host_out_ptrs.len() < max_out {
                scr.host_out_ptrs.push(HostOutPtr::Null);
            }
        }
        drop(audio);

        data.active.store(true, Ordering::Relaxed);
        true
    })
}

unsafe extern "C" fn clap_plugin_deactivate<P: PluginExport>(plugin: *const clap_plugin) {
    // Draining a stranded `pending_state` runs the author's `load_state`;
    // firewall it so a panic can't unwind across the C ABI.
    run_extern_callback_with::<P, ()>("CLAP", "deactivate", (), || unsafe {
        let data = data_from_plugin::<P>(plugin);
        data.active.store(false, Ordering::Relaxed);
        data.parameter_restart_pending
            .store(false, Ordering::Relaxed);
        // A `state_load` while active queues its blob for the audio thread
        // to drain at the top of the next block. If the host deactivates
        // before that block runs (sample-rate / buffer change, offline
        // reconfigure right after a recall), the blob is stranded: it would
        // resurrect stale state on the next activate, overwriting any newer
        // state applied while inactive. Drain it now and apply synchronously
        // (uncontended - no audio thread is processing) so nothing survives
        // deactivate.
        if let Some(deserialized) = data.pending_state.pop() {
            let mut instance = enter_plugin(&data.plugin);
            state::apply_state(&mut *instance, &deserialized);
            instance.republish_snapshot();
        }
        if refresh_parameter_list(data)
            && !data.host_params.is_null()
            && !data.host.is_null()
            && let Some(rescan) = (*data.host_params).rescan
        {
            rescan(data.host, CLAP_PARAM_RESCAN_ALL);
        }
    });
}

unsafe extern "C" fn clap_plugin_start_processing<P: PluginExport>(
    _plugin: *const clap_plugin,
) -> bool {
    true
}

unsafe extern "C" fn clap_plugin_stop_processing<P: PluginExport>(_plugin: *const clap_plugin) {
    // Nothing to do.
}

unsafe extern "C" fn clap_plugin_reset<P: PluginExport>(plugin: *const clap_plugin) {
    // Author `reset` runs here; firewall it so a panic can't unwind across
    // the C ABI.
    run_extern_callback_with::<P, ()>("CLAP", "reset", (), || unsafe {
        let data = data_from_plugin::<P>(plugin);
        let mut audio = data.audio.enter();
        audio.sounding_notes.clear_all();
        let mode = ProcessMode::from_u8(data.render_mode.load(Ordering::Relaxed));
        let mut instance = enter_plugin(&data.plugin);
        instance.reset(
            &AudioConfig::new(audio.sample_rate, audio.max_block_size).with_process_mode(mode),
        );
        // Same baseline refresh as `activate` (no dirty flag): keep the
        // caches consistent with the reset instance.
        data.latency_cache
            .store(instance.latency(), Ordering::Relaxed);
        data.tail_cache.store(instance.tail(), Ordering::Relaxed);
    });
}

unsafe extern "C" fn clap_plugin_on_main_thread<P: PluginExport>(plugin: *const clap_plugin) {
    unsafe {
        let data = data_from_plugin::<P>(plugin);
        // Runtime presentation metadata is re-read on RESCAN_INFO. A change
        // to the exposed list requests a restart and is published inactive.
        let revision = data.params_arc.parameter_presentation_revision();
        let info_changed = data.presentation_revision.swap(revision, Ordering::Relaxed) != revision;
        let list_changed = info_changed && refresh_parameter_list(data);
        let values_changed = data.needs_rescan.swap(false, Ordering::Relaxed);
        if (values_changed || info_changed)
            && !data.host_params.is_null()
            && !data.host.is_null()
            && let Some(rescan) = (*data.host_params).rescan
        {
            let mut flags = if list_changed {
                CLAP_PARAM_RESCAN_ALL
            } else {
                0
            };
            if values_changed {
                flags |= CLAP_PARAM_RESCAN_VALUES;
            }
            if info_changed {
                flags |= CLAP_PARAM_RESCAN_INFO;
            }
            rescan(data.host, flags);
        }

        // Output port names changed: a names-only rescan, which CLAP
        // allows while active. A host without `clap.audio-ports` (or one
        // that declines the flag) keeps the names it has until it asks
        // for the ports again.
        if data.port_names_dirty.swap(false, Ordering::Relaxed)
            && !data.host.is_null()
            && !data.host_audio_ports.is_null()
            && let Some(rescan) = (*data.host_audio_ports).rescan
        {
            let supported = (*data.host_audio_ports)
                .is_rescan_flag_supported
                .is_none_or(|f| f(data.host, CLAP_AUDIO_PORTS_RESCAN_NAMES));
            if supported {
                rescan(data.host, CLAP_AUDIO_PORTS_RESCAN_NAMES);
            }
        }

        // Latency changed on the audio thread: tell the host here, off
        // the audio thread. `changed()` re-reads our reported latency;
        // an active plugin additionally needs `request_restart` to apply
        // the new delay compensation.
        if data.latency_dirty.swap(false, Ordering::Relaxed) && !data.host.is_null() {
            if !data.host_latency.is_null()
                && let Some(changed) = (*data.host_latency).changed
            {
                changed(data.host);
            }
            if data.active.load(Ordering::Relaxed)
                && let Some(req_restart) = (*data.host).request_restart
            {
                req_restart(data.host);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Event conversion: CLAP input events -> EventList
// ---------------------------------------------------------------------------

/// 32-bit wire value -> CLAP `0..1` note-expression value.
#[cfg(test)]
fn unit_from_u32(v: u32) -> f64 {
    f64::from(v) / f64::from(u32::MAX)
}

/// CLAP `0..1` note-expression value -> 32-bit wire value.
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn u32_from_unit(v: f64) -> u32 {
    (v.clamp(0.0, 1.0) * f64::from(u32::MAX)).round() as u32
}

/// Map a moose per-note 2.0 event to a CLAP note expression
/// `(expression_id, channel, note, value)`. `TUNING` is in semitones;
/// the rest are `0..1`.
#[cfg(test)]
fn clap_note_expression_of(body: &EventBody) -> Option<(i32, u8, u8, f64)> {
    match *body {
        // Registered per-note controllers only: the predefined CLAP
        // expression ids carry the registered indices' semantics
        // (7 = volume, 74 = brightness, ...); an assignable index is
        // manufacturer-defined and must not alias onto them.
        EventBody::PerNoteCC {
            channel,
            note,
            cc,
            value,
            registered: true,
            ..
        } => {
            let id = match cc {
                7 => CLAP_NOTE_EXPRESSION_VOLUME,
                10 => CLAP_NOTE_EXPRESSION_PAN,
                1 => CLAP_NOTE_EXPRESSION_VIBRATO,
                11 => CLAP_NOTE_EXPRESSION_EXPRESSION,
                74 => CLAP_NOTE_EXPRESSION_BRIGHTNESS,
                _ => return None,
            };
            // CLAP `VOLUME` is plain linear gain `0..=4` (the wire's
            // quarter point is unity); the other ids are `0..=1`.
            let value = if id == CLAP_NOTE_EXPRESSION_VOLUME {
                PER_NOTE_VOLUME_MAX_GAIN * unit_from_u32(value)
            } else {
                unit_from_u32(value)
            };
            Some((id, channel, note, value))
        }
        EventBody::PerNotePitchBend {
            channel,
            note,
            value,
            ..
        } => Some((
            CLAP_NOTE_EXPRESSION_TUNING,
            channel,
            note,
            per_note_bend_semitones(value),
        )),
        EventBody::PolyPressure2 {
            channel,
            note,
            pressure,
            ..
        } => Some((
            CLAP_NOTE_EXPRESSION_PRESSURE,
            channel,
            note,
            unit_from_u32(pressure),
        )),
        _ => None,
    }
}

/// Resolve a CLAP note event's `(channel, note)` address. CLAP carries
/// both as `i16` where `-1` is a "match all" wildcard; moose's
/// `EventBody` speaks concrete MIDI addresses (`channel 0..=15`,
/// `note 0..=127`) and plugins index tables by them, so anything
/// outside the domain - wildcards included - is dropped rather than
/// delivered mislabeled. (Wildcard `NOTE_OFF` / `NOTE_CHOKE` don't take
/// this path; see [`SoundingNotes`].)
#[cfg(test)]
fn clap_note_address(ne: &clap_event_note) -> Option<(u8, u8)> {
    let channel = u8::try_from(ne.channel).ok().filter(|c| *c <= 15)?;
    let note = u8::try_from(ne.key).ok().filter(|k| *k <= 127)?;
    Some((channel, note))
}

/// One wildcardable axis of a CLAP note address: `-1` matches all,
/// a concrete in-domain value matches itself, out-of-domain junk
/// matches nothing (the event drops, same as the concrete path).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum NoteAxis {
    All,
    One(u8),
    Invalid,
}

impl NoteAxis {
    fn parse(raw: i16, domain: u8) -> Self {
        if raw == -1 {
            return Self::All;
        }
        match u8::try_from(raw) {
            Ok(v) if v < domain => Self::One(v),
            _ => Self::Invalid,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
struct Pck {
    port: u8,
    channel: u8,
    key: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct TrackedVoice {
    pck: Pck,
    note_id: i32,
    lifecycle: VoiceLifecycle,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum VoiceLifecycle {
    Active,
    Released,
    Choked,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum NoteIdAxis {
    All,
    One(i32),
    Invalid,
}

impl NoteIdAxis {
    fn parse(raw: i32) -> Self {
        match raw {
            -1 => Self::All,
            0.. => Self::One(raw),
            _ => Self::Invalid,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct VoicePattern {
    port: NoteAxis,
    channel: NoteAxis,
    key: NoteAxis,
    note_id: NoteIdAxis,
}

impl VoicePattern {
    fn parse(port: i16, channel: i16, key: i16, note_id: i32, port_count: u8) -> Self {
        Self {
            port: NoteAxis::parse(port, port_count),
            channel: NoteAxis::parse(channel, 16),
            key: NoteAxis::parse(key, 128),
            note_id: NoteIdAxis::parse(note_id),
        }
    }

    fn is_valid(self) -> bool {
        self.port != NoteAxis::Invalid
            && self.channel != NoteAxis::Invalid
            && self.key != NoteAxis::Invalid
            && self.note_id != NoteIdAxis::Invalid
    }

    fn concrete_pck(self) -> Option<Pck> {
        match (self.port, self.channel, self.key) {
            (NoteAxis::One(port), NoteAxis::One(channel), NoteAxis::One(key)) => {
                Some(Pck { port, channel, key })
            }
            _ => None,
        }
    }

    fn matches(self, voice: TrackedVoice) -> bool {
        fn axis_matches(axis: NoteAxis, value: u8) -> bool {
            matches!(axis, NoteAxis::All) || matches!(axis, NoteAxis::One(one) if one == value)
        }

        axis_matches(self.port, voice.pck.port)
            && axis_matches(self.channel, voice.pck.channel)
            && axis_matches(self.key, voice.pck.key)
            && (matches!(self.note_id, NoteIdAxis::All)
                || matches!(self.note_id, NoteIdAxis::One(one) if one == voice.note_id))
    }
}

/// Bounded, preallocated tracker for CLAP note identities. Every
/// `(port, channel, key, note_id)` voice is retained separately, including
/// stacked anonymous (`note_id == -1`) voices. Wildcard fanout is emitted only
/// while the tracker is complete; an overflow makes completeness sticky until
/// the next host reset so partial state can never invent semantic events.
struct SoundingNotes {
    voices: Vec<TrackedVoice>,
    complete: bool,
}

impl SoundingNotes {
    fn new(_midi_input_ports: u8) -> Self {
        Self {
            voices: Vec::with_capacity(EVENT_LIST_PREALLOC),
            complete: true,
        }
    }

    fn track(&mut self, voice: TrackedVoice) -> bool {
        if self.voices.len() >= self.voices.capacity() {
            self.complete = false;
            return false;
        }
        self.voices.push(voice);
        true
    }

    #[cfg(test)]
    fn set(&mut self, port: u8, channel: u8, key: u8) {
        let _ = self.track(TrackedVoice {
            pck: Pck { port, channel, key },
            note_id: -1,
            lifecycle: VoiceLifecycle::Active,
        });
    }

    #[cfg(test)]
    fn clear(&mut self, port: u8, channel: u8, key: u8) {
        self.voices
            .retain(|voice| voice.pck != Pck { port, channel, key });
    }

    fn clear_all(&mut self) {
        self.voices.clear();
        self.complete = true;
    }

    #[cfg(test)]
    fn drain_matching(
        &mut self,
        port: u8,
        channel: Option<u8>,
        key: Option<u8>,
        f: impl FnMut(u8, u8),
    ) {
        self.visit_matching(port, channel, key, true, f);
    }

    #[cfg(test)]
    fn for_each_matching(
        &mut self,
        port: u8,
        channel: Option<u8>,
        key: Option<u8>,
        f: impl FnMut(u8, u8),
    ) {
        self.visit_matching(port, channel, key, false, f);
    }

    #[cfg(test)]
    fn visit_matching(
        &mut self,
        port: u8,
        channel: Option<u8>,
        key: Option<u8>,
        clear: bool,
        mut f: impl FnMut(u8, u8),
    ) {
        let pattern = VoicePattern {
            port: NoteAxis::One(port),
            channel: channel.map_or(NoteAxis::All, NoteAxis::One),
            key: key.map_or(NoteAxis::All, NoteAxis::One),
            note_id: NoteIdAxis::All,
        };
        if clear {
            self.release_pattern(pattern, |pck| f(pck.channel, pck.key));
        } else {
            self.for_each_pattern(pattern, |pck| f(pck.channel, pck.key));
        }
    }

    fn release_pattern(&mut self, pattern: VoicePattern, mut f: impl FnMut(Pck)) {
        if !pattern.is_valid() {
            return;
        }
        let mut matched = [Pck::default(); EVENT_LIST_PREALLOC];
        let mut matched_len = 0;
        for voice in &mut self.voices {
            if voice.lifecycle == VoiceLifecycle::Active && pattern.matches(*voice) {
                if self.complete {
                    matched[matched_len] = voice.pck;
                    matched_len += 1;
                }
                voice.lifecycle = VoiceLifecycle::Released;
            }
        }
        if !self.complete {
            return;
        }
        emit_unique_pcks(&mut matched[..matched_len], &mut f);
    }

    fn choke_pattern(&mut self, pattern: VoicePattern) {
        if !pattern.is_valid() {
            return;
        }
        for voice in &mut self.voices {
            if voice.lifecycle != VoiceLifecycle::Choked && pattern.matches(*voice) {
                voice.lifecycle = VoiceLifecycle::Choked;
            }
        }
    }

    fn remove_pattern(&mut self, pattern: VoicePattern) {
        if pattern.is_valid() {
            self.voices.retain(|voice| !pattern.matches(*voice));
        }
    }

    fn for_each_pattern(&self, pattern: VoicePattern, mut f: impl FnMut(Pck)) {
        if !pattern.is_valid() || !self.complete {
            return;
        }
        let mut matched = [Pck::default(); EVENT_LIST_PREALLOC];
        let mut matched_len = 0;
        for voice in &self.voices {
            if voice.lifecycle != VoiceLifecycle::Choked && pattern.matches(*voice) {
                matched[matched_len] = voice.pck;
                matched_len += 1;
            }
        }
        emit_unique_pcks(&mut matched[..matched_len], &mut f);
    }
}

fn emit_unique_pcks(matched: &mut [Pck], mut f: impl FnMut(Pck)) {
    matched.sort_unstable();
    let mut previous = None;
    for pck in matched {
        if previous != Some(*pck) {
            f(*pck);
            previous = Some(*pck);
        }
    }
}

/// Decode a CLAP note expression into its moose per-note 2.0 event,
/// addressed to one concrete `(channel, note)` voice. The event's own
/// (possibly wildcard) axes are resolved by the caller - see
/// [`push_note_expressions`].
fn note_expression_body(
    ne: &clap_event_note_expression,
    channel: u8,
    note: u8,
) -> Option<EventBody> {
    let cc = match ne.expression_id {
        CLAP_NOTE_EXPRESSION_TUNING => {
            return Some(EventBody::PerNotePitchBend {
                group: 0,
                channel,
                note,
                value: per_note_bend_from_semitones(ne.value),
            });
        }
        CLAP_NOTE_EXPRESSION_VOLUME => 7,
        CLAP_NOTE_EXPRESSION_PAN => 10,
        CLAP_NOTE_EXPRESSION_VIBRATO => 1,
        CLAP_NOTE_EXPRESSION_EXPRESSION => 11,
        CLAP_NOTE_EXPRESSION_BRIGHTNESS => 74,
        CLAP_NOTE_EXPRESSION_PRESSURE => {
            return Some(EventBody::PolyPressure2 {
                group: 0,
                channel,
                note,
                pressure: u32_from_unit(ne.value),
            });
        }
        _ => return None,
    };
    // CLAP `VOLUME` arrives as plain linear gain `0..=4`; normalize
    // into the wire domain so unity gain lands on the quarter point.
    let value = if ne.expression_id == CLAP_NOTE_EXPRESSION_VOLUME {
        u32_from_unit(ne.value / PER_NOTE_VOLUME_MAX_GAIN)
    } else {
        u32_from_unit(ne.value)
    };
    Some(EventBody::PerNoteCC {
        group: 0,
        channel,
        note,
        cc,
        value,
        registered: true,
    })
}

/// Build a `TransportInfo` from a CLAP transport event/struct.
///
/// Same flag-driven decoding is needed in two places - the
/// `CLAP_EVENT_TRANSPORT` arm of `convert_input_events` (which sees a
/// `clap_event_transport` arriving as an input event mid-block) and
/// the per-process `clap_process::transport` field. Hosts deliver
/// transport state through whichever channel they prefer; the bit
/// layout is identical, so the decode is too.
// CLAP transport positions arrive as `i64` fixed-point counts that
// must be divided into `f64` seconds/beats; the `i64 as f64`
// narrowing is bounded in practice by song-length (well below 2^52).
#[allow(clippy::cast_precision_loss, clippy::cast_possible_truncation)]
fn build_transport_info(t: &clap_event_transport, sample_rate: f64) -> TransportInfo {
    let flags = t.flags;
    let beats_timeline = flags & CLAP_TRANSPORT_HAS_BEATS_TIMELINE != 0;
    let has_time_sig = flags & CLAP_TRANSPORT_HAS_TIME_SIGNATURE != 0;
    let position_seconds = if flags & CLAP_TRANSPORT_HAS_SECONDS_TIMELINE != 0 {
        t.song_pos_seconds as f64 / CLAP_SECTIME_FACTOR as f64
    } else {
        0.0
    };
    TransportInfo {
        playing: flags & CLAP_TRANSPORT_IS_PLAYING != 0,
        recording: flags & CLAP_TRANSPORT_IS_RECORDING != 0,
        tempo: if flags & CLAP_TRANSPORT_HAS_TEMPO != 0 {
            t.tempo
        } else {
            120.0
        },
        // CLAP delivers `tsig_num` / `tsig_denom` as `i16`; the
        // narrowing is bounded by the MIDI domain (≤ 255).
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        time_sig_num: if has_time_sig { t.tsig_num as u8 } else { 4 },
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        time_sig_den: if has_time_sig { t.tsig_denom as u8 } else { 4 },
        // CLAP transport carries no sample position; derive it from the
        // seconds timeline (rounded to the nearest sample), the same way
        // the host computes it. 0 when no seconds timeline is provided.
        position_samples: (position_seconds * sample_rate).round() as i64,
        position_seconds,
        position_beats: if beats_timeline {
            t.song_pos_beats as f64 / CLAP_BEATTIME_FACTOR as f64
        } else {
            0.0
        },
        bar_start_beats: if beats_timeline {
            t.bar_start as f64 / CLAP_BEATTIME_FACTOR as f64
        } else {
            0.0
        },
        loop_active: flags & CLAP_TRANSPORT_IS_LOOP_ACTIVE != 0,
        loop_start_beats: if beats_timeline {
            t.loop_start_beats as f64 / CLAP_BEATTIME_FACTOR as f64
        } else {
            0.0
        },
        loop_end_beats: if beats_timeline {
            t.loop_end_beats as f64 / CLAP_BEATTIME_FACTOR as f64
        } else {
            0.0
        },
    }
}

/// UMP packet width from the message-type nibble (MIDI 2.0 UMP table 1).
/// CLAP stores every packet in four words, so retaining this width is what
/// distinguishes meaningful words from padding without decoding the payload.
fn ump_word_count(word0: u32) -> u8 {
    const WORD_COUNTS: [u8; 16] = [1, 1, 1, 2, 2, 4, 1, 1, 2, 2, 2, 3, 3, 4, 4, 4];
    let message_type = usize::try_from((word0 >> 28) & 0x0F).unwrap_or(0);
    WORD_COUNTS[message_type]
}

fn exact_event_qualifiers(flags: u32) -> ExactEventQualifiers {
    ExactEventQualifiers {
        is_live: flags & CLAP_EVENT_IS_LIVE != 0,
        dont_record: flags & CLAP_EVENT_DONT_RECORD != 0,
    }
}

fn clap_exact_flags(qualifiers: ExactEventQualifiers) -> u32 {
    let mut flags = 0;
    if qualifiers.is_live {
        flags |= CLAP_EVENT_IS_LIVE;
    }
    if qualifiers.dont_record {
        flags |= CLAP_EVENT_DONT_RECORD;
    }
    flags
}

fn exact_note_event(sample_offset: u32, kind: ExactNoteKind, note: &clap_event_note) -> ExactEvent {
    ExactEvent::new(
        sample_offset,
        ExactEventBody::Note {
            kind,
            address: ExactNoteAddress::from_raw_signed(
                note.port_index,
                note.channel,
                note.key,
                note.note_id,
            ),
            velocity: note.velocity,
        },
    )
    .with_qualifiers(exact_event_qualifiers(note.header.flags))
}

fn exact_note_expression(
    sample_offset: u32,
    expression: &clap_event_note_expression,
) -> ExactEvent {
    ExactEvent::new(
        sample_offset,
        ExactEventBody::NoteExpression {
            expression_id: expression.expression_id,
            address: ExactNoteAddress::from_raw_signed(
                expression.port_index,
                expression.channel,
                expression.key,
                expression.note_id,
            ),
            value: expression.value,
        },
    )
    .with_qualifiers(exact_event_qualifiers(expression.header.flags))
}

/// Keep the exact lane authoritative without sacrificing the semantic view
/// when only one bounded lane is full. Returns whether the typed view became
/// visible, which lets state trackers avoid seeding semantic fanout from an
/// exact-only event. A partial-capacity result remains observable through
/// `EventList::overflow()`.
fn push_exact_with_fallback(
    event_list: &mut EventList,
    exact: ExactEvent,
    fallback: Event,
) -> bool {
    match event_list.try_push_with_exact(fallback, exact) {
        Ok(()) => true,
        Err(PushError::PoolFull | PushError::UnknownExactEvent | PushError::VoiceTrackerFull) => {
            false
        }
        Err(PushError::EventFull) => {
            let _ = event_list.try_push_exact(exact);
            false
        }
        Err(PushError::ExactEventFull) => event_list.try_push(fallback).is_ok(),
    }
}

type ClapTryPush =
    unsafe extern "C" fn(list: *const clap_output_events, event: *const clap_event_header) -> bool;

unsafe extern "C" fn accept_output_push(
    _list: *const clap_output_events,
    _event: *const clap_event_header,
) -> bool {
    true
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ExactEmit {
    Emitted,
    Unsupported,
    Invalid,
    QueueFull,
}

fn exact_push_result(pushed: bool) -> ExactEmit {
    if pushed {
        ExactEmit::Emitted
    } else {
        ExactEmit::QueueFull
    }
}

fn note_expression_value_is_valid(expression_id: i32, value: f64) -> bool {
    if !value.is_finite() {
        return false;
    }
    match expression_id {
        CLAP_NOTE_EXPRESSION_VOLUME => value > 0.0 && value <= PER_NOTE_VOLUME_MAX_GAIN,
        CLAP_NOTE_EXPRESSION_PAN
        | CLAP_NOTE_EXPRESSION_VIBRATO
        | CLAP_NOTE_EXPRESSION_EXPRESSION
        | CLAP_NOTE_EXPRESSION_BRIGHTNESS
        | CLAP_NOTE_EXPRESSION_PRESSURE => (0.0..=1.0).contains(&value),
        CLAP_NOTE_EXPRESSION_TUNING => (-120.0..=120.0).contains(&value),
        _ => false,
    }
}

fn midi1_message_len(status: u8) -> Option<usize> {
    if (0x80..=0xEF).contains(&status) {
        return Some(if matches!(status & 0xF0, 0xC0 | 0xD0) {
            2
        } else {
            3
        });
    }
    match status {
        0xF1 | 0xF3 => Some(2),
        0xF2 => Some(3),
        0xF6 | 0xF8 | 0xFA | 0xFB | 0xFC | 0xFE | 0xFF => Some(1),
        _ => None,
    }
}

fn raw_midi1_valid(message: RawMidi1) -> bool {
    let bytes = message.bytes();
    midi1_message_len(bytes[0]) == Some(bytes.len())
        && bytes.iter().skip(1).all(|byte| byte & 0x80 == 0)
}

fn typed_output_body_is_valid(body: &EventBody) -> bool {
    match *body {
        EventBody::NoteOn {
            group,
            channel,
            note,
            velocity,
        }
        | EventBody::NoteOff {
            group,
            channel,
            note,
            velocity,
        } => group == 0 && channel < 16 && note < 128 && velocity < 128,
        EventBody::Aftertouch {
            group,
            channel,
            note,
            pressure,
        } => group == 0 && channel < 16 && note < 128 && pressure < 128,
        EventBody::ChannelPressure {
            group,
            channel,
            pressure,
        } => group == 0 && channel < 16 && pressure < 128,
        EventBody::ControlChange {
            group,
            channel,
            cc,
            value,
        } => group == 0 && channel < 16 && cc < 128 && value < 128,
        EventBody::PitchBend {
            group,
            channel,
            value,
        } => group == 0 && channel < 16 && value < 16_384,
        EventBody::ProgramChange {
            group,
            channel,
            program,
        } => group == 0 && channel < 16 && program < 128,
        EventBody::NoteOn2 {
            group,
            channel,
            note,
            attribute_type,
            ..
        }
        | EventBody::NoteOff2 {
            group,
            channel,
            note,
            attribute_type,
            ..
        } => group < 16 && channel < 16 && note < 128 && attribute_type < 4,
        EventBody::PolyPressure2 {
            group,
            channel,
            note,
            ..
        }
        | EventBody::PerNotePitchBend {
            group,
            channel,
            note,
            ..
        }
        | EventBody::PerNoteCC {
            group,
            channel,
            note,
            ..
        } => group < 16 && channel < 16 && note < 128,
        EventBody::PerNoteManagement {
            group,
            channel,
            note,
            flags,
        } => group < 16 && channel < 16 && note < 128 && flags & !0b11 == 0,
        EventBody::ControlChange2 {
            group, channel, cc, ..
        } => group < 16 && channel < 16 && cc < 128,
        EventBody::ChannelPressure2 { group, channel, .. }
        | EventBody::PitchBend2 { group, channel, .. } => group < 16 && channel < 16,
        EventBody::ProgramChange2 {
            group,
            channel,
            program,
            bank,
        } => {
            group < 16
                && channel < 16
                && program < 128
                && bank.is_none_or(|(msb, lsb)| msb < 128 && lsb < 128)
        }
        EventBody::RegisteredController {
            group,
            channel,
            bank,
            index,
            ..
        }
        | EventBody::AssignableController {
            group,
            channel,
            bank,
            index,
            ..
        } => group < 16 && channel < 16 && bank < 128 && index < 128,
        EventBody::ParamChange { id, value } => id != CLAP_INVALID_ID && value.is_finite(),
        EventBody::ParamMod { id, note_id, value } => {
            id != CLAP_INVALID_ID && note_id >= -1 && value.is_finite()
        }
        EventBody::Transport(_) | EventBody::SysEx { .. } => true,
    }
}

fn is_portless_output_body(body: &EventBody) -> bool {
    matches!(
        body,
        EventBody::ParamChange { .. } | EventBody::ParamMod { .. } | EventBody::Transport(_)
    )
}

fn exact_note_fields(
    address: ExactNoteAddress,
    output_ports: u8,
    require_concrete_note_address: bool,
) -> Option<(i32, i16, i16, i16)> {
    if output_ports == 0
        || address.port.invalid_raw().is_some()
        || address.channel.invalid_raw().is_some()
        || address.key.invalid_raw().is_some()
        || address.note_id.invalid_raw().is_some()
    {
        return None;
    }

    let port = address.port.raw_i32();
    let channel = address.channel.raw_i32();
    let key = address.key.raw_i32();
    let note_id = address.note_id.raw_i32();
    if note_id < -1
        || (port != -1 && !(0..i32::from(output_ports)).contains(&port))
        || (channel != -1 && !(0..=15).contains(&channel))
        || (key != -1 && !(0..=127).contains(&key))
        || (require_concrete_note_address && (port == -1 || channel == -1 || key == -1))
    {
        return None;
    }

    Some((
        note_id,
        i16::try_from(port).ok()?,
        i16::try_from(channel).ok()?,
        i16::try_from(key).ok()?,
    ))
}

/// Emit an exact event when the configured CLAP port can legally carry it.
/// Unsupported and invalid representations fail closed, while queue exhaustion
/// stops the globally ordered replay.
unsafe fn emit_exact_clap(
    out_events: *const clap_output_events,
    try_push: ClapTryPush,
    event: ExactEventRef<'_>,
    info: &PluginInfo,
    frames_count: u32,
) -> ExactEmit {
    if !event.metadata().is_none() {
        return ExactEmit::Unsupported;
    }
    let sample_offset = event.sample_offset();
    if sample_offset >= frames_count {
        return ExactEmit::Invalid;
    }
    let flags = clap_exact_flags(event.qualifiers());
    match event.body() {
        ExactEventBody::Midi1 { port, message } => {
            if *port >= u16::from(info.midi_output_ports) || !raw_midi1_valid(*message) {
                return ExactEmit::Invalid;
            }
            let ev = clap_event_midi {
                header: clap_event_header {
                    size: size_of_u32::<clap_event_midi>(),
                    time: sample_offset,
                    space_id: CLAP_CORE_EVENT_SPACE_ID,
                    type_: CLAP_EVENT_MIDI,
                    flags,
                },
                port_index: *port,
                data: *message.storage(),
            };
            exact_push_result(unsafe { try_push(out_events, &raw const ev.header) })
        }
        ExactEventBody::Ump { port, packet } => {
            if *port >= u16::from(info.midi_output_ports)
                || packet.word_count() != usize::from(ump_word_count(packet.storage()[0]))
            {
                return ExactEmit::Invalid;
            }
            if info.midi_output_dialect != MidiDialect::Midi2 {
                return ExactEmit::Unsupported;
            }
            let ev = clap_event_midi2 {
                header: clap_event_header {
                    size: size_of_u32::<clap_event_midi2>(),
                    time: sample_offset,
                    space_id: CLAP_CORE_EVENT_SPACE_ID,
                    type_: CLAP_EVENT_MIDI2,
                    flags,
                },
                port_index: *port,
                data: *packet.storage(),
            };
            exact_push_result(unsafe { try_push(out_events, &raw const ev.header) })
        }
        ExactEventBody::SysEx { port } => {
            if *port >= u16::from(info.midi_output_ports) {
                return ExactEmit::Invalid;
            }
            let Some(bytes) = event.sysex_bytes_checked() else {
                return ExactEmit::Invalid;
            };
            let ev = clap_event_midi_sysex {
                header: clap_event_header {
                    size: size_of_u32::<clap_event_midi_sysex>(),
                    time: sample_offset,
                    space_id: CLAP_CORE_EVENT_SPACE_ID,
                    type_: CLAP_EVENT_MIDI_SYSEX,
                    flags,
                },
                port_index: *port,
                buffer: bytes.as_ptr(),
                size: len_u32(bytes.len()),
            };
            exact_push_result(unsafe { try_push(out_events, &raw const ev.header) })
        }
        ExactEventBody::Note {
            kind,
            address,
            velocity,
        } => {
            // Choke is host -> plugin; replaying it toward the host would
            // reverse a lifecycle command with no legal output meaning.
            if matches!(kind, ExactNoteKind::Choke) {
                return ExactEmit::Unsupported;
            }
            let type_ = match kind {
                ExactNoteKind::On => CLAP_EVENT_NOTE_ON,
                ExactNoteKind::Off => CLAP_EVENT_NOTE_OFF,
                ExactNoteKind::End => CLAP_EVENT_NOTE_END,
                _ => return ExactEmit::Unsupported,
            };
            if !velocity.is_finite() || !(0.0..=1.0).contains(velocity) {
                return ExactEmit::Invalid;
            }
            let port_count = if matches!(kind, ExactNoteKind::End) {
                info.midi_input_ports
            } else {
                info.midi_output_ports
            };
            let Some((note_id, port_index, channel, key)) = exact_note_fields(
                *address,
                port_count,
                matches!(kind, ExactNoteKind::On | ExactNoteKind::End),
            ) else {
                return ExactEmit::Invalid;
            };
            let ev = clap_event_note {
                header: clap_event_header {
                    size: size_of_u32::<clap_event_note>(),
                    time: sample_offset,
                    space_id: CLAP_CORE_EVENT_SPACE_ID,
                    type_,
                    flags,
                },
                note_id,
                port_index,
                channel,
                key,
                velocity: *velocity,
            };
            exact_push_result(unsafe { try_push(out_events, &raw const ev.header) })
        }
        ExactEventBody::NoteExpression {
            expression_id,
            address,
            value,
        } => {
            if !note_expression_value_is_valid(*expression_id, *value) {
                return ExactEmit::Invalid;
            }
            let Some((note_id, port_index, channel, key)) =
                exact_note_fields(*address, info.midi_output_ports, false)
            else {
                return ExactEmit::Invalid;
            };
            let ev = clap_event_note_expression {
                header: clap_event_header {
                    size: size_of_u32::<clap_event_note_expression>(),
                    time: sample_offset,
                    space_id: CLAP_CORE_EVENT_SPACE_ID,
                    type_: CLAP_EVENT_NOTE_EXPRESSION,
                    flags,
                },
                expression_id: *expression_id,
                note_id,
                port_index,
                channel,
                key,
                value: *value,
            };
            exact_push_result(unsafe { try_push(out_events, &raw const ev.header) })
        }
        _ => ExactEmit::Unsupported,
    }
}

fn preflight_clap_output(
    events: &EventList,
    info: &PluginInfo,
    param_infos: &[ParamInfo],
    frames_count: u32,
) -> OutputEventStatus {
    for replay in events.lossless_iter() {
        let event = match replay {
            LosslessEventRef::Exact(exact) => {
                let status = unsafe {
                    emit_exact_clap(
                        std::ptr::null(),
                        accept_output_push,
                        exact,
                        info,
                        frames_count,
                    )
                };
                match status {
                    ExactEmit::Emitted => continue,
                    ExactEmit::Unsupported => return OutputEventStatus::Unsupported,
                    ExactEmit::Invalid | ExactEmit::QueueFull => {
                        return OutputEventStatus::Invalid;
                    }
                }
            }
            LosslessEventRef::Typed(event) => event,
        };
        if event.sample_offset >= frames_count
            || !typed_output_body_is_valid(&event.body)
            || (!is_portless_output_body(&event.body)
                && (info.midi_output_ports == 0 || event.port >= info.midi_output_ports))
        {
            return OutputEventStatus::Invalid;
        }
        match &event.body {
            EventBody::ParamChange { id, .. } if !param_infos.iter().any(|info| info.id == *id) => {
                return OutputEventStatus::Invalid;
            }
            EventBody::SysEx { .. } => {
                if events.sysex_bytes_checked(&event.body).is_none() {
                    return OutputEventStatus::Invalid;
                }
            }
            body @ (EventBody::NoteOn2 { .. }
            | EventBody::NoteOff2 { .. }
            | EventBody::PolyPressure2 { .. }
            | EventBody::PerNoteCC { .. }
            | EventBody::PerNotePitchBend { .. }
            | EventBody::PerNoteManagement { .. }
            | EventBody::ControlChange2 { .. }
            | EventBody::ChannelPressure2 { .. }
            | EventBody::PitchBend2 { .. }
            | EventBody::ProgramChange2 { .. }
            | EventBody::RegisteredController { .. }
            | EventBody::AssignableController { .. }) => {
                if info.midi_output_dialect != MidiDialect::Midi2 {
                    return OutputEventStatus::Unsupported;
                }
                if encode_ump_channel_voice_2(body).is_none() {
                    return OutputEventStatus::Invalid;
                }
            }
            EventBody::ParamMod { .. } | EventBody::Transport(_) => {
                return OutputEventStatus::Unsupported;
            }
            _ => {}
        }
    }
    OutputEventStatus::Success
}

fn outbound_note_end_pattern(event: ExactEventRef<'_>, info: &PluginInfo) -> Option<VoicePattern> {
    let ExactEventBody::Note {
        kind: ExactNoteKind::End,
        address,
        ..
    } = event.body()
    else {
        return None;
    };
    let pattern = VoicePattern::parse(
        i16::try_from(address.port.value()?).ok()?,
        i16::from(address.channel.value()?),
        i16::from(address.key.value()?),
        address.note_id.raw_i32(),
        info.midi_input_ports,
    );
    (pattern.is_valid() && pattern.concrete_pck().is_some()).then_some(pattern)
}

/// Preserve one exact release and expose each newly released semantic PCK once.
/// Released voices remain tracked for note expressions until the plugin emits
/// `NOTE_END`; a repeated host release therefore has no semantic companion.
fn push_note_offs<P: PluginExport>(
    scr: &mut ClapAudio<P>,
    info: &PluginInfo,
    note_event: &clap_event_note,
    sample_offset: u32,
    exact: ExactEvent,
    velocity: u8,
) {
    let pattern = VoicePattern::parse(
        note_event.port_index,
        note_event.channel,
        note_event.key,
        note_event.note_id,
        info.midi_input_ports,
    );
    if !pattern.is_valid() {
        return;
    }

    let ClapAudio {
        sounding_notes,
        event_list,
        ..
    } = scr;
    let token = event_list.try_push_exact_token(exact);
    sounding_notes.release_pattern(pattern, |pck| {
        let event = Event::on_port(
            sample_offset,
            pck.port,
            EventBody::NoteOff {
                group: 0,
                channel: pck.channel,
                note: pck.key,
                velocity,
            },
        );
        if let Ok(token) = token {
            let _ = event_list.try_push_exact_companion(token, event);
        } else {
            let _ = event_list.try_push(event);
        }
    });
}

/// Preserve a host choke exactly and stop later semantic expression fanout for
/// the affected voices. Choke is not a MIDI note-off and has no typed fallback.
fn push_note_choke<P: PluginExport>(
    scr: &mut ClapAudio<P>,
    info: &PluginInfo,
    note_event: &clap_event_note,
    exact: ExactEvent,
) {
    let pattern = VoicePattern::parse(
        note_event.port_index,
        note_event.channel,
        note_event.key,
        note_event.note_id,
        info.midi_input_ports,
    );
    if !pattern.is_valid() {
        return;
    }
    let _ = scr.event_list.try_push_exact(exact);
    scr.sounding_notes.choke_pattern(pattern);
}

/// Deliver one exact note expression plus the linked semantic PCK views that
/// a typed plugin can consume.
fn push_note_expressions<P: PluginExport>(
    scr: &mut ClapAudio<P>,
    info: &PluginInfo,
    ne: &clap_event_note_expression,
    sample_offset: u32,
    exact: ExactEvent,
) {
    let pattern = VoicePattern::parse(
        ne.port_index,
        ne.channel,
        ne.key,
        ne.note_id,
        info.midi_input_ports,
    );
    if !pattern.is_valid() {
        return;
    }

    let semantic = |pck: Pck| {
        let decoded = note_expression_body(ne, pck.channel, pck.key)?;
        let body = if info.midi_input_dialect == MidiDialect::Midi2 {
            Some(decoded)
        } else {
            downconvert_to_midi1(&decoded)
        }?;
        Some(Event::on_port(sample_offset, pck.port, body))
    };

    let ClapAudio {
        sounding_notes,
        event_list,
        ..
    } = scr;
    let token = event_list.try_push_exact_token(exact);
    sounding_notes.for_each_pattern(pattern, |pck| {
        let Some(event) = semantic(pck) else {
            return;
        };
        if let Ok(token) = token {
            let _ = event_list.try_push_exact_companion(token, event);
        } else {
            let _ = event_list.try_push(event);
        }
    });
}

/// `sort` controls whether the resulting `event_list` gets a stable
/// sort by sample offset. `process` needs sorted events (the plugin
/// iterates them in time order); `params_flush` discards the events
/// after extracting param/GUI updates and doesn't care about order, so
/// it passes `false` to skip the sort. Process calls supply `frames_count`
/// so out-of-block host timestamps fail closed before state changes; flush has
/// no audio block and passes `None`.
fn clap_event_has_size<T>(header: &clap_event_header) -> bool {
    header.size >= size_of_u32::<T>()
}

fn input_note_pattern(
    note: &clap_event_note,
    info: &PluginInfo,
    require_concrete_pck: bool,
) -> Option<VoicePattern> {
    if info.midi_input_ports == 0
        || !note.velocity.is_finite()
        || !(0.0..=1.0).contains(&note.velocity)
    {
        return None;
    }
    let pattern = VoicePattern::parse(
        note.port_index,
        note.channel,
        note.key,
        note.note_id,
        info.midi_input_ports,
    );
    (pattern.is_valid() && (!require_concrete_pck || pattern.concrete_pck().is_some()))
        .then_some(pattern)
}

fn input_note_expression_pattern(
    expression: &clap_event_note_expression,
    info: &PluginInfo,
) -> Option<VoicePattern> {
    if info.midi_input_ports == 0
        || !note_expression_value_is_valid(expression.expression_id, expression.value)
    {
        return None;
    }
    let pattern = VoicePattern::parse(
        expression.port_index,
        expression.channel,
        expression.key,
        expression.note_id,
        info.midi_input_ports,
    );
    pattern.is_valid().then_some(pattern)
}

fn declared_input_port(port: u16, count: u8) -> Option<u8> {
    (port < u16::from(count)).then(|| u8::try_from(port).ok())?
}

/// Record a `params_flush` param change for replay in the next process
/// block, latest value per id. Bounded by the vec's capacity (the param
/// count), so an id the plugin doesn't declare can't grow it.
fn defer_param_change(deferred: &mut Vec<(u32, f64)>, id: u32, value: f64) {
    if let Some(slot) = deferred.iter_mut().find(|(d, _)| *d == id) {
        slot.1 = value;
    } else if deferred.len() < deferred.capacity() {
        deferred.push((id, value));
    }
}

/// Push the deferred flush changes at offset 0 ahead of the block's own
/// events, then forget them. A state load discards them: they predate
/// the recalled state, like the live param events it drops.
fn replay_deferred_params(
    list: &mut EventList,
    deferred: &mut Vec<(u32, f64)>,
    state_loaded: bool,
) {
    if !state_loaded {
        for &(id, value) in deferred.iter() {
            list.push(Event::new(0, EventBody::ParamChange { id, value }));
        }
    }
    deferred.clear();
}

#[allow(clippy::too_many_lines)]
unsafe fn convert_input_events<P: PluginExport>(
    scr: &mut ClapAudio<P>,
    info: &PluginInfo,
    in_events: *const clap_input_events,
    sort: bool,
    state_loaded: bool,
    frames_count: Option<u32>,
) {
    unsafe {
        scr.event_list.clear();
        // Process path only (`params_flush` passes no frame count):
        // stopped-transport param events precede this block's events.
        if frames_count.is_some() {
            replay_deferred_params(&mut scr.event_list, &mut scr.deferred_params, state_loaded);
        }

        if in_events.is_null() {
            return;
        }

        let Some(size_fn) = (*in_events).size else {
            return;
        };
        let Some(get_fn) = (*in_events).get else {
            return;
        };

        let count = size_fn(in_events);

        for i in 0..count {
            let header_ptr = get_fn(in_events, i);
            if header_ptr.is_null() {
                continue;
            }

            let header = &*header_ptr;
            if !clap_event_has_size::<clap_event_header>(header)
                || header.space_id != CLAP_CORE_EVENT_SPACE_ID
                || frames_count.is_some_and(|frames| header.time >= frames)
            {
                continue;
            }

            let sample_offset = header.time;
            // `NOTE_END` is plugin -> host only. An inbound event is a
            // protocol violation and must not alter plugin voice state.
            if header.type_ == CLAP_EVENT_NOTE_END {
                continue;
            }
            match header.type_ {
                CLAP_EVENT_NOTE_ON => {
                    if !clap_event_has_size::<clap_event_note>(header) {
                        continue;
                    }
                    let note_event = &*header_ptr.cast::<clap_event_note>();
                    let Some(pattern) = input_note_pattern(note_event, info, true) else {
                        continue;
                    };
                    let Some(pck) = pattern.concrete_pck() else {
                        continue;
                    };
                    let exact = exact_note_event(sample_offset, ExactNoteKind::On, note_event);
                    // CLAP's f64 velocity is a normalized [0, 1]; moose
                    // exposes it as a wire-native 7-bit value in the semantic
                    // view to match every other format. Track the voice only
                    // when that semantic onset is actually visible.
                    let typed_visible = push_exact_with_fallback(
                        &mut scr.event_list,
                        exact,
                        Event::on_port(
                            sample_offset,
                            pck.port,
                            EventBody::NoteOn {
                                group: 0,
                                channel: pck.channel,
                                note: pck.key,
                                velocity: denorm_7bit(f32::from_f64(note_event.velocity)),
                            },
                        ),
                    );
                    if typed_visible
                        && !scr.sounding_notes.track(TrackedVoice {
                            pck,
                            note_id: note_event.note_id,
                            lifecycle: VoiceLifecycle::Active,
                        })
                    {
                        scr.event_list.record_overflow(PushError::VoiceTrackerFull);
                    }
                }
                CLAP_EVENT_NOTE_OFF => {
                    if !clap_event_has_size::<clap_event_note>(header) {
                        continue;
                    }
                    let note_event = &*header_ptr.cast::<clap_event_note>();
                    if input_note_pattern(note_event, info, false).is_none() {
                        continue;
                    }
                    let velocity = denorm_7bit(f32::from_f64(note_event.velocity));
                    let exact = exact_note_event(sample_offset, ExactNoteKind::Off, note_event);
                    push_note_offs(scr, info, note_event, sample_offset, exact, velocity);
                }
                CLAP_EVENT_NOTE_CHOKE => {
                    if !clap_event_has_size::<clap_event_note>(header) {
                        continue;
                    }
                    let note_event = &*header_ptr.cast::<clap_event_note>();
                    if input_note_pattern(note_event, info, false).is_none() {
                        continue;
                    }
                    let exact = exact_note_event(sample_offset, ExactNoteKind::Choke, note_event);
                    push_note_choke(scr, info, note_event, exact);
                }
                CLAP_EVENT_PARAM_VALUE => {
                    // When a state load was applied at the head of
                    // this block, param-change events queued by the
                    // host predate that intent (typical case: a clip-
                    // edge re-trigger sends preset-B automation in
                    // the same block as the preset-A state recall).
                    // Drop them so the just-restored preset isn't
                    // partly overwritten by stale automation.
                    if state_loaded || !clap_event_has_size::<clap_event_param_value>(header) {
                        continue;
                    }
                    let param_event = &*header_ptr.cast::<clap_event_param_value>();
                    if param_event.param_id == CLAP_INVALID_ID || !param_event.value.is_finite() {
                        continue;
                    }
                    // `set_plain` is deferred to the per-sub-block
                    // `apply_pending_events` pass in
                    // `chunked_process::process_chunked` - that way
                    // the smoother sees `set_target` at the event's
                    // sample, not at the head of the audio block.
                    scr.event_list.push(Event::new(
                        sample_offset,
                        EventBody::ParamChange {
                            id: param_event.param_id,
                            value: param_event.value,
                        },
                    ));
                }
                CLAP_EVENT_PARAM_MOD => {
                    // Same rationale as PARAM_VALUE above: drop
                    // pre-state-load mod packets.
                    if state_loaded || !clap_event_has_size::<clap_event_param_mod>(header) {
                        continue;
                    }
                    let mod_event = &*header_ptr.cast::<clap_event_param_mod>();
                    if mod_event.param_id == CLAP_INVALID_ID
                        || mod_event.note_id < -1
                        || !mod_event.amount.is_finite()
                    {
                        continue;
                    }
                    scr.event_list.push(Event::new(
                        sample_offset,
                        EventBody::ParamMod {
                            id: mod_event.param_id,
                            note_id: mod_event.note_id,
                            value: mod_event.amount,
                        },
                    ));
                }
                CLAP_EVENT_TRANSPORT => {
                    if !clap_event_has_size::<clap_event_transport>(header) {
                        continue;
                    }
                    let transport = &*header_ptr.cast::<clap_event_transport>();
                    scr.event_list.push(Event::new(
                        sample_offset,
                        EventBody::Transport(build_transport_info(transport, scr.sample_rate)),
                    ));
                }
                CLAP_EVENT_MIDI => {
                    // CLAP carries MIDI 1.0 channel-voice messages as
                    // 3-byte packets. Demux back into the typed
                    // `EventBody` variants the plugin sees on every
                    // other format. Without this, hosts that route
                    // raw MIDI (`CLAP_NOTE_DIALECT_MIDI` ports)
                    // silently drop CC / PitchBend / Aftertouch /
                    // ChannelPressure / ProgramChange at the wrapper.
                    if !clap_event_has_size::<clap_event_midi>(header) {
                        continue;
                    }
                    let midi = &*header_ptr.cast::<clap_event_midi>();
                    if let Some(message) = RawMidi1::new(midi.data, 3) {
                        let exact = ExactEvent::new(
                            sample_offset,
                            ExactEventBody::Midi1 {
                                port: midi.port_index,
                                message,
                            },
                        )
                        .with_qualifiers(exact_event_qualifiers(midi.header.flags));
                        let status = midi.data[0] & 0xF0;
                        let semantic_is_valid = (0x80..=0xE0).contains(&status)
                            && midi.data[1] < 128
                            && (matches!(status, 0xC0 | 0xD0) || midi.data[2] < 128);
                        if let Some((port, body)) = declared_input_port(
                            midi.port_index,
                            info.midi_input_ports,
                        )
                        .zip(
                            semantic_is_valid
                                .then(|| {
                                    decode_short_message(midi.data[0], midi.data[1], midi.data[2])
                                })
                                .flatten(),
                        ) {
                            let _ = push_exact_with_fallback(
                                &mut scr.event_list,
                                exact,
                                Event::on_port(sample_offset, port, body),
                            );
                        } else {
                            let _ = scr.event_list.try_push_exact(exact);
                        }
                    }
                }
                CLAP_EVENT_MIDI_SYSEX => {
                    // CLAP delivers `SysEx` payloads as a pointer +
                    // length owned by the host for the duration of
                    // this `process()` call. Copy into our pool
                    // immediately - the bytes can't be assumed valid
                    // after we return. `push_sysex` is fail-closed
                    // when the pool is exhausted; we drop the
                    // message and keep going (a corrupt-by-split
                    // alternative is never the right answer for
                    // `SysEx`).
                    if !clap_event_has_size::<clap_event_midi_sysex>(header) {
                        continue;
                    }
                    let sysex = &*header_ptr.cast::<clap_event_midi_sysex>();
                    let Some(port) = declared_input_port(sysex.port_index, info.midi_input_ports)
                    else {
                        continue;
                    };
                    let bytes = if sysex.size == 0 {
                        &[][..]
                    } else if sysex.buffer.is_null() {
                        continue;
                    } else {
                        let Ok(size) = usize::try_from(sysex.size) else {
                            continue;
                        };
                        std::slice::from_raw_parts(sysex.buffer, size)
                    };
                    let exact = ExactEvent::new(
                        sample_offset,
                        ExactEventBody::SysEx {
                            port: sysex.port_index,
                        },
                    )
                    .with_qualifiers(exact_event_qualifiers(sysex.header.flags));
                    let _ = scr.event_list.try_push_sysex_with_exact_on_port(
                        sample_offset,
                        port,
                        bytes,
                        exact,
                    );
                }
                // Preserve every well-formed UMP exactly. Only a declared
                // MIDI 2.0 input port receives a typed semantic companion;
                // an ignored dialect never triggers a lossy down-conversion.
                CLAP_EVENT_MIDI2 => {
                    if !clap_event_has_size::<clap_event_midi2>(header) {
                        continue;
                    }
                    let midi2 = &*header_ptr.cast::<clap_event_midi2>();
                    if let Some(packet) = RawUmp::new(midi2.data, ump_word_count(midi2.data[0])) {
                        let exact = ExactEvent::new(
                            sample_offset,
                            ExactEventBody::Ump {
                                port: midi2.port_index,
                                packet,
                            },
                        )
                        .with_qualifiers(exact_event_qualifiers(midi2.header.flags));
                        if let Some((port, body)) =
                            declared_input_port(midi2.port_index, info.midi_input_ports)
                                .filter(|_| info.midi_input_dialect == MidiDialect::Midi2)
                                .zip(decode_ump_channel_voice_2(midi2.data))
                        {
                            let _ = push_exact_with_fallback(
                                &mut scr.event_list,
                                exact,
                                Event::on_port(sample_offset, port, body),
                            );
                        } else {
                            let _ = scr.event_list.try_push_exact(exact);
                        }
                    }
                }
                CLAP_EVENT_NOTE_EXPRESSION => {
                    // Per-note expression (hosts send these for MPE-style
                    // input). Decode to the 2.0 per-note event - fanned
                    // out across wildcard axes - with the down-converted
                    // channel form for plugins that didn't opt into 2.0.
                    if !clap_event_has_size::<clap_event_note_expression>(header) {
                        continue;
                    }
                    let ne = &*header_ptr.cast::<clap_event_note_expression>();
                    if input_note_expression_pattern(ne, info).is_none() {
                        continue;
                    }
                    let exact = exact_note_expression(sample_offset, ne);
                    push_note_expressions(scr, info, ne, sample_offset, exact);
                }
                _ => {
                    // Unsupported event type (system real-time, utility)
                    // - skip silently.
                }
            }
        }

        if sort {
            scr.event_list.ensure_sorted_by_offset();
        }
    }
}

// ---------------------------------------------------------------------------
// Flush GUI-initiated param changes to CLAP output events
// ---------------------------------------------------------------------------

unsafe fn flush_gui_changes<P: PluginExport>(
    data: &ClapPluginData<P>,
    out_events: *const clap_output_events,
) -> bool {
    unsafe {
        if out_events.is_null() {
            return true;
        }
        let Some(try_push) = (*out_events).try_push else {
            return true;
        };

        while let Some(change) = data.gui_changes.pop() {
            match change {
                GuiParamChange::GestureBegin(id) => {
                    let event = clap_event_param_gesture {
                        header: clap_event_header {
                            size: size_of_u32::<clap_event_param_gesture>(),
                            time: 0,
                            space_id: CLAP_CORE_EVENT_SPACE_ID,
                            type_: CLAP_EVENT_PARAM_GESTURE_BEGIN,
                            flags: CLAP_EVENT_IS_LIVE,
                        },
                        param_id: id,
                    };
                    if !try_push(out_events, &raw const event.header) {
                        return false;
                    }
                }
                GuiParamChange::Value(id, plain) => {
                    let event = clap_event_param_value {
                        header: clap_event_header {
                            size: size_of_u32::<clap_event_param_value>(),
                            time: 0,
                            space_id: CLAP_CORE_EVENT_SPACE_ID,
                            type_: CLAP_EVENT_PARAM_VALUE,
                            flags: CLAP_EVENT_IS_LIVE,
                        },
                        param_id: id,
                        cookie: ptr::null_mut(),
                        note_id: -1,
                        port_index: -1,
                        channel: -1,
                        key: -1,
                        value: plain,
                    };
                    if plain.is_finite() && !try_push(out_events, &raw const event.header) {
                        return false;
                    }
                }
                GuiParamChange::GestureEnd(id) => {
                    let event = clap_event_param_gesture {
                        header: clap_event_header {
                            size: size_of_u32::<clap_event_param_gesture>(),
                            time: 0,
                            space_id: CLAP_CORE_EVENT_SPACE_ID,
                            type_: CLAP_EVENT_PARAM_GESTURE_END,
                            flags: CLAP_EVENT_IS_LIVE,
                        },
                        param_id: id,
                    };
                    if !try_push(out_events, &raw const event.header) {
                        return false;
                    }
                }
            }
        }
        true
    }
}

// ---------------------------------------------------------------------------
// Process callback
// ---------------------------------------------------------------------------

#[allow(clippy::too_many_lines)]
unsafe extern "C" fn clap_plugin_process<P: PluginExport>(
    plugin: *const clap_plugin,
    process: *const clap_process,
) -> i32 {
    let result = run_audio_block_with::<P, i32>("CLAP", CLAP_PROCESS_ERROR, || unsafe {
        if process.is_null() {
            return CLAP_PROCESS_ERROR;
        }

        let proc = &*process;
        let data = data_from_plugin::<P>(plugin);
        let num_frames = proc.frames_count as usize;

        if num_frames == 0 {
            return CLAP_PROCESS_CONTINUE;
        }

        // Take ownership of the plugin for the whole block: an
        // uncontended `Acquire`, never a wait, since the host contract
        // keeps `process` from overlapping a lifecycle callback and host
        // saves read the snapshot instead of the plugin. Enter through a
        // local Arc clone so the guard doesn't pin a borrow of `data`
        // (later per-block work needs `&mut data`).
        let plugin_arc = Arc::clone(&data.plugin);
        let mut instance = enter_plugin(&plugin_arc);

        // Apply any state-load that the host or editor handed us
        // since the last block. Runs before per-block work so the
        // plugin sees consistent params for the entire block. The
        // single-slot queue means a rapid double-recall lands the
        // newest blob and the older one is dropped - preferred to
        // the audio thread chasing stale state across blocks.
        let state_loaded = data.pending_state.pop().is_some_and(|state| {
            state::apply_state(&mut *instance, &state);
            true
        });

        // Paranoid allocation check (the `rt-paranoid` feature): guard the
        // wrapper's per-block glue - event conversion, transport, process,
        // output encode, snapshot publish - as well as the plugin. Placed
        // after the state-load apply above, since `load_state` legitimately
        // allocates. No-op and zero-sized when the feature is off.
        let _rt = RtSection::enter();

        // The audio scratch lives behind an ownership cell reached through the
        // shared `&data`, so a concurrent host-thread `&data` (param reads,
        // GUI) can't alias it. One reborrow so the disjoint scratch fields
        // (event list, slices, output events, build scratch) can be borrowed
        // simultaneously - the guard's `Deref` can't split-borrow.
        let mut audio = data.audio.enter();
        let scr = &mut *audio;

        // Convert CLAP input events to our EventList - sort by
        // sample offset so the plugin sees them in time order.
        // `state_loaded` causes ParamValue/ParamMod events to be
        // dropped because they predate the state-load intent.
        convert_input_events::<P>(
            scr,
            &data.info,
            proc.in_events,
            true,
            state_loaded,
            Some(proc.frames_count),
        );

        // Build transport info from the CLAP transport event (or default).
        let transport = if proc.transport.is_null() {
            TransportInfo::default()
        } else {
            build_transport_info(&*proc.transport, scr.sample_rate)
        };

        // Build AudioBuffer from CLAP audio buffers.
        //
        // Three soundness considerations matching the format-wrapper
        // pattern in `RawBufferScratch::build`:
        //
        // 1. **No per-block heap allocation.** We reuse `data.input_slices`
        //    and `data.output_slices` (cleared each call) so the audio
        //    thread doesn't `Vec::new()` per process.
        // 2. **Channel indexing preserved.** A null channel pointer
        //    becomes an empty slice at the same flat-channel index
        //    rather than being dropped; densifying the channel list
        //    would silently re-map indices when only some channels
        //    were null.
        // 3. **No auto input→output copy.** Plugins that want
        //    pass-through must do `output.copy_from_slice(input)`
        //    themselves; auto-copying clobbers the previous-block tail
        //    that delay/reverb feedback paths read back from the output.
        debug_assert!(
            num_frames <= scr.max_block_size,
            "host violated CLAP contract: process() got {num_frames} frames \
             but activate() declared max {}",
            scr.max_block_size
        );

        // Build per-channel slices preserving channel index across
        // every bus. A null bus (neither data pointer set) emits
        // empty slices for each of its declared channels rather than
        // being skipped - skipping would shift downstream buses'
        // channel indices and silently re-route audio onto the wrong
        // bus for multi-bus plugins.
        //
        // The host picks the wire precision per port: `data64` is
        // only set on ports that advertised
        // `CLAP_AUDIO_PORT_SUPPORTS_64BITS` (f64 plugins). When the
        // wire matches `P::Sample`, slices point straight at host
        // memory (zero-copy); otherwise the channel is converted
        // through the matching `input_widen` / `output_narrow` slot
        // and copied back after `process()` returns.
        scr.input_slices.clear();
        // Reset each inner scratch buffer's length to 0 (preserves
        // its heap allocation), don't `.clear()` the outer
        // `Vec<Vec<_>>` - that would drop every inner Vec and force
        // the per-channel push below to re-allocate every block,
        // defeating the activate-time pre-grow.
        for buf in &mut scr.input_widen {
            buf.clear();
        }
        // The outer Vec is pre-sized in `clap_plugin_activate`; the
        // while-loops below only run as a fallback if the pre-grow
        // didn't cover the bus layout the host actually picked.
        // `Vec::new()` keeps the fallback allocation-free; the inner
        // scratch only allocates if that channel actually converts.
        let mut flat_in_idx = 0usize;
        let bus_routing = scr.bus_routing;
        for bus_idx in 0..proc.audio_inputs_count {
            let buf = &*proc.audio_inputs.add(bus_idx as usize);
            let bus_is_f64 = !buf.data64.is_null();
            for ch in 0..buf.channel_count {
                while scr.input_widen.len() <= flat_in_idx {
                    scr.input_widen.push(Vec::new());
                }
                let scratch = &mut scr.input_widen[flat_in_idx];
                let slice: &'static [P::Sample] = if bus_is_f64 {
                    let host_ptr: *const f64 = *buf.data64.add(ch as usize);
                    input_channel_slice::<P::Sample, f64>(host_ptr, num_frames, scratch)
                } else if buf.data32.is_null() {
                    // Whole input port disconnected (null channel-pointer
                    // array): feed this channel block-length silence.
                    input_channel_slice::<P::Sample, f32>(std::ptr::null(), num_frames, scratch)
                } else {
                    let host_ptr: *const f32 = *buf.data32.add(ch as usize);
                    input_channel_slice::<P::Sample, f32>(host_ptr, num_frames, scratch)
                };
                scr.input_slices.push(slice);
                flat_in_idx += 1;
            }
        }

        scr.output_slices.clear();
        // Same reasoning as `input_widen` above: clear each inner
        // scratch buffer in place so its heap allocation survives.
        for buf in &mut scr.output_narrow {
            buf.clear();
        }
        scr.host_out_ptrs.clear();
        let mut flat_out_idx = 0usize;
        for bus_idx in 0..proc.audio_outputs_count {
            let buf = &mut *proc.audio_outputs.add(bus_idx as usize);
            let bus_is_f64 = !buf.data64.is_null();
            for ch in 0..buf.channel_count {
                while scr.output_narrow.len() <= flat_out_idx {
                    scr.output_narrow.push(Vec::new());
                }
                let scratch = &mut scr.output_narrow[flat_out_idx];
                let slice: &'static mut [P::Sample] = if bus_is_f64 {
                    let host_ptr: *mut f64 = *buf.data64.add(ch as usize);
                    scr.host_out_ptrs.push(if host_ptr.is_null() {
                        HostOutPtr::Null
                    } else {
                        HostOutPtr::F64(host_ptr)
                    });
                    output_channel_slice::<P::Sample, f64>(host_ptr, num_frames, scratch)
                } else {
                    let host_ptr: *mut f32 = if buf.data32.is_null() {
                        std::ptr::null_mut()
                    } else {
                        *buf.data32.add(ch as usize)
                    };
                    scr.host_out_ptrs.push(if host_ptr.is_null() {
                        HostOutPtr::Null
                    } else {
                        HostOutPtr::F32(host_ptr)
                    });
                    output_channel_slice::<P::Sample, f32>(host_ptr, num_frames, scratch)
                };
                scr.output_slices.push(slice);
                flat_out_idx += 1;
            }
        }

        // Construct the AudioBuffer with a borrow scope tied to this
        // call only. Without the transmute, the borrow checker
        // propagates the `'static` lifetimes inside `input_slices`
        // out to the AudioBuffer's lifetime parameter - which would
        // pin the scratch mutably for the rest of the function. Same
        // pattern as `RawBufferScratch::build`.
        let scr_ptr: *mut ClapAudio<P> = scr;
        let s = &mut *scr_ptr;
        let mut audio_buffer =
            transmute::<AudioBuffer<'static, P::Sample>, AudioBuffer<'_, P::Sample>>(
                AudioBuffer::from_slices(&s.input_slices, &mut s.output_slices, num_frames),
            );

        scr.output_events.clear();
        scr.output_events.clear_overflow();

        // Publish transport to the editor slot before the plugin runs.
        data.transport_slot.write(&transport);

        // Sample-accurate parameter chunking: split the audio block
        // at every chunkable param-change / transport event, deferring
        // the `set_plain` apply to the sub-block boundary the event
        // sits on. On blocks with no chunkable events this runs
        // `plugin.process` exactly once - the splitting machinery is
        // inert. See `chunked_process` for the per-sub-block contract.
        let mut transport_snap = transport;
        let chunk_args = ChunkedProcess {
            events: &scr.event_list,
            sub_event_scratch: &mut scr.sub_event_scratch,
            transport: &mut transport_snap,
            sample_rate: scr.sample_rate,
            process_mode: ProcessMode::from_u8(data.render_mode.load(Ordering::Relaxed)),
            output_events: &mut scr.output_events,
            params_fn: None,
            meters_fn: None,
            param_infos: &data.param_infos,
            min_subblock_samples: data.info.automation.min_subblock_samples,
        };
        let status = process_chunked_with_bus_routing(
            &mut *instance,
            data.params_arc.as_ref() as &dyn Params,
            &mut audio_buffer,
            chunk_args,
            bus_routing,
        );

        // Convert + copy back to host outputs for every channel that
        // rendered into scratch (wire precision differed from
        // `P::Sample`). Zero-copy channels leave their scratch slot
        // empty and are skipped - the plugin already wrote host
        // memory. `zip` over the two vectors instead of indexing -
        // if either is shorter (it shouldn't be, but a future drift
        // would hit this), iteration stops at the min cleanly rather
        // than panicking on an out-of-bounds index.
        for (host_ptr, plugin) in scr.host_out_ptrs.iter().zip(scr.output_narrow.iter()) {
            if plugin.is_empty() {
                continue;
            }
            match *host_ptr {
                HostOutPtr::Null => {}
                HostOutPtr::F32(p) => {
                    let host = std::slice::from_raw_parts_mut(p, num_frames);
                    for (h, &s) in host.iter_mut().zip(plugin.iter()) {
                        *h = s.to_f32();
                    }
                }
                HostOutPtr::F64(p) => {
                    let host = std::slice::from_raw_parts_mut(p, num_frames);
                    for (h, &s) in host.iter_mut().zip(plugin.iter()) {
                        *h = s.to_f64();
                    }
                }
            }
        }

        // Refresh latency / tail caches so the host's main-thread
        // queries don't have to touch the plugin. On an actual
        // latency change, flag it and wake the main thread, which
        // notifies the host (`clap.latency` requires the call off the
        // audio thread).
        // Port names are read on the main thread; here only a revision
        // is compared (one atomic load) and the main thread woken.
        let names = data.params_arc.output_port_names_revision();
        if data.port_names_revision.swap(names, Ordering::Relaxed) != names
            && !data.port_names_dirty.swap(true, Ordering::Relaxed)
            && !data.host.is_null()
            && let Some(req_cb) = (*data.host).request_callback
        {
            req_cb(data.host);
        }
        let new_latency = instance.latency();
        if data.latency_cache.swap(new_latency, Ordering::Relaxed) != new_latency
            && !data.latency_dirty.swap(true, Ordering::Relaxed)
            && !data.host.is_null()
            && let Some(req_cb) = (*data.host).request_callback
        {
            req_cb(data.host);
        }
        // A shrinking reverb tail must reach the host so its bounce
        // end-time is right. Unlike latency (which needs a main-thread
        // restart), `clap.tail.changed` is an [audio-thread] callback, so
        // notify the host directly from here - no deferral.
        let new_tail = instance.tail();
        if data.tail_cache.swap(new_tail, Ordering::Relaxed) != new_tail
            && !data.host_tail.is_null()
            && let Some(changed) = (*data.host_tail).changed
        {
            changed(data.host);
        }

        // GUI delivery has its own queue semantics. The public status belongs
        // to the plugin output lane, so an unrelated GUI refusal cannot hide
        // a staging BufferFull root cause.
        let _ = flush_gui_changes::<P>(data, proc.out_events);
        let mut output_status = scr
            .output_events
            .overflow()
            .map_or(OutputEventStatus::Success, OutputEventStatus::BufferFull);
        if output_status == OutputEventStatus::Success && !scr.output_events.is_empty() {
            // CLAP requires globally time-sorted output. Preflight the exact
            // sequence the host will see before allowing the first push.
            scr.output_events.ensure_sorted_by_offset();
            output_status = preflight_clap_output(
                &scr.output_events,
                &data.info,
                &data.param_infos,
                proc.frames_count,
            );
        }

        // Forward plugin output events (MIDI output from instruments/effects)
        if output_status == OutputEventStatus::Success && !scr.output_events.is_empty() {
            let try_push = if proc.out_events.is_null() {
                None
            } else {
                (*proc.out_events).try_push
            };
            let Some(try_push) = try_push else {
                scr.output_events
                    .set_output_status(OutputEventStatus::Unsupported);
                scr.input_slices.clear();
                scr.output_slices.clear();
                return match status {
                    ProcessStatus::Normal => CLAP_PROCESS_CONTINUE,
                    ProcessStatus::Tail(0) => CLAP_PROCESS_SLEEP,
                    ProcessStatus::Tail(_) => CLAP_PROCESS_TAIL,
                    ProcessStatus::KeepAlive => CLAP_PROCESS_CONTINUE_IF_NOT_QUIET,
                };
            };
            // Exact payloads are authoritative. `lossless_iter` suppresses a
            // linked typed view, and an unsupported exact representation is
            // never replayed through a lossy compatibility conversion.
            macro_rules! push_output {
                ($queue:ident, $header:expr) => {
                    $queue = try_push(proc.out_events, $header);
                    if !$queue {
                        output_status = OutputEventStatus::HostQueueFull;
                    }
                };
            }
            'output_events: for replay in scr.output_events.lossless_iter() {
                let queue_open;
                let event = match replay {
                    LosslessEventRef::Typed(event) => event,
                    LosslessEventRef::Exact(exact) => {
                        let note_end = outbound_note_end_pattern(exact, &data.info);
                        match emit_exact_clap(
                            proc.out_events,
                            try_push,
                            exact,
                            &data.info,
                            proc.frames_count,
                        ) {
                            ExactEmit::Emitted => {
                                if let Some(pattern) = note_end {
                                    scr.sounding_notes.remove_pattern(pattern);
                                }
                                continue;
                            }
                            ExactEmit::Invalid => {
                                output_status = OutputEventStatus::Invalid;
                                break 'output_events;
                            }
                            ExactEmit::Unsupported => {
                                output_status = OutputEventStatus::Unsupported;
                                break 'output_events;
                            }
                            ExactEmit::QueueFull => {
                                output_status = OutputEventStatus::HostQueueFull;
                                break 'output_events;
                            }
                        }
                    }
                };
                if event.sample_offset >= proc.frames_count {
                    output_status = OutputEventStatus::Invalid;
                    break 'output_events;
                }
                if !typed_output_body_is_valid(&event.body) {
                    output_status = OutputEventStatus::Invalid;
                    break 'output_events;
                }
                if !is_portless_output_body(&event.body)
                    && (data.info.midi_output_ports == 0
                        || event.port >= data.info.midi_output_ports)
                {
                    output_status = OutputEventStatus::Invalid;
                    break 'output_events;
                }
                let out_port = event.port;
                match &event.body {
                    EventBody::NoteOn {
                        channel,
                        note,
                        velocity,
                        ..
                    } => {
                        let ev = clap_event_note {
                            header: clap_event_header {
                                size: size_of_u32::<clap_event_note>(),
                                time: event.sample_offset,
                                space_id: CLAP_CORE_EVENT_SPACE_ID,
                                type_: CLAP_EVENT_NOTE_ON,
                                flags: 0,
                            },
                            note_id: -1,
                            port_index: i16::from(out_port),
                            channel: i16::from(*channel),
                            key: i16::from(*note),
                            velocity: f64::from(*velocity) / 127.0,
                        };
                        push_output!(queue_open, &raw const ev.header);
                    }
                    EventBody::NoteOff {
                        channel,
                        note,
                        velocity,
                        ..
                    } => {
                        let ev = clap_event_note {
                            header: clap_event_header {
                                size: size_of_u32::<clap_event_note>(),
                                time: event.sample_offset,
                                space_id: CLAP_CORE_EVENT_SPACE_ID,
                                type_: CLAP_EVENT_NOTE_OFF,
                                flags: 0,
                            },
                            note_id: -1,
                            port_index: i16::from(out_port),
                            channel: i16::from(*channel),
                            key: i16::from(*note),
                            velocity: f64::from(*velocity) / 127.0,
                        };
                        push_output!(queue_open, &raw const ev.header);
                    }
                    // CLAP carries MIDI 1.0 control / channel events as
                    // `CLAP_EVENT_MIDI` 3-byte packets. The host
                    // demuxes them on the receiving side; we just
                    // build the standard MIDI status byte and pass
                    // the data bytes through.
                    EventBody::ControlChange {
                        channel, cc, value, ..
                    } => {
                        let ev = clap_event_midi {
                            header: clap_event_header {
                                size: size_of_u32::<clap_event_midi>(),
                                time: event.sample_offset,
                                space_id: CLAP_CORE_EVENT_SPACE_ID,
                                type_: CLAP_EVENT_MIDI,
                                flags: 0,
                            },
                            port_index: u16::from(out_port),
                            data: [0xB0 | *channel, *cc, *value],
                        };
                        push_output!(queue_open, &raw const ev.header);
                    }
                    EventBody::Aftertouch {
                        channel,
                        note,
                        pressure,
                        ..
                    } => {
                        let ev = clap_event_midi {
                            header: clap_event_header {
                                size: size_of_u32::<clap_event_midi>(),
                                time: event.sample_offset,
                                space_id: CLAP_CORE_EVENT_SPACE_ID,
                                type_: CLAP_EVENT_MIDI,
                                flags: 0,
                            },
                            port_index: u16::from(out_port),
                            data: [0xA0 | *channel, *note, *pressure],
                        };
                        push_output!(queue_open, &raw const ev.header);
                    }
                    EventBody::ChannelPressure {
                        channel, pressure, ..
                    } => {
                        let ev = clap_event_midi {
                            header: clap_event_header {
                                size: size_of_u32::<clap_event_midi>(),
                                time: event.sample_offset,
                                space_id: CLAP_CORE_EVENT_SPACE_ID,
                                type_: CLAP_EVENT_MIDI,
                                flags: 0,
                            },
                            port_index: u16::from(out_port),
                            data: [0xD0 | *channel, *pressure, 0],
                        };
                        push_output!(queue_open, &raw const ev.header);
                    }
                    EventBody::PitchBend { channel, value, .. } => {
                        let (lsb, msb) = pitch_bend_to_bytes(*value);
                        let ev = clap_event_midi {
                            header: clap_event_header {
                                size: size_of_u32::<clap_event_midi>(),
                                time: event.sample_offset,
                                space_id: CLAP_CORE_EVENT_SPACE_ID,
                                type_: CLAP_EVENT_MIDI,
                                flags: 0,
                            },
                            port_index: u16::from(out_port),
                            data: [0xE0 | *channel, lsb, msb],
                        };
                        push_output!(queue_open, &raw const ev.header);
                    }
                    EventBody::ProgramChange {
                        channel, program, ..
                    } => {
                        let ev = clap_event_midi {
                            header: clap_event_header {
                                size: size_of_u32::<clap_event_midi>(),
                                time: event.sample_offset,
                                space_id: CLAP_CORE_EVENT_SPACE_ID,
                                type_: CLAP_EVENT_MIDI,
                                flags: 0,
                            },
                            port_index: u16::from(out_port),
                            data: [0xC0 | *channel, *program, 0],
                        };
                        push_output!(queue_open, &raw const ev.header);
                    }
                    EventBody::ParamChange { id, value } => {
                        // CLAP params are global, not tied to a specific
                        // note/audio port - every key uses the `-1`
                        // wildcard so hosts that route automation by
                        // port_index match GUI-driven and process-driven
                        // param events on the same key. The
                        // `flush_gui_changes` path uses the same shape.
                        let ev = clap_event_param_value {
                            header: clap_event_header {
                                size: size_of_u32::<clap_event_param_value>(),
                                time: event.sample_offset,
                                space_id: CLAP_CORE_EVENT_SPACE_ID,
                                type_: CLAP_EVENT_PARAM_VALUE,
                                flags: 0,
                            },
                            param_id: *id,
                            cookie: ptr::null_mut(),
                            note_id: -1,
                            port_index: -1,
                            channel: -1,
                            key: -1,
                            value: *value,
                        };
                        push_output!(queue_open, &raw const ev.header);
                    }
                    EventBody::SysEx { .. } => {
                        // CLAP's contract for the output direction
                        // is narrower than the input's: the
                        // `buffer` pointer needs to stay valid for
                        // the duration of the `try_push` call only
                        // (the host is expected to copy before
                        // returning). Our pool is cleared at the
                        // *start* of the next block, so pointing
                        // the host at `pool_offset` satisfies that
                        // strictly - and any future host that
                        // defers the copy until later in
                        // `process()` is still fine because the
                        // pool stays valid through the whole block.
                        let Some(bytes) = scr.output_events.sysex_bytes_checked(&event.body) else {
                            output_status = OutputEventStatus::Invalid;
                            break 'output_events;
                        };
                        let ev = clap_event_midi_sysex {
                            header: clap_event_header {
                                size: size_of_u32::<clap_event_midi_sysex>(),
                                time: event.sample_offset,
                                space_id: CLAP_CORE_EVENT_SPACE_ID,
                                type_: CLAP_EVENT_MIDI_SYSEX,
                                flags: 0,
                            },
                            port_index: u16::from(out_port),
                            buffer: bytes.as_ptr(),
                            size: len_u32(bytes.len()),
                        };
                        push_output!(queue_open, &raw const ev.header);
                    }
                    // Typed MIDI 2.0 remains native UMP. The strict domain
                    // check above makes the shared encoder's masks inert, so
                    // group, attributes, banks, controllers, and wide values
                    // reach the host unchanged.
                    body @ (EventBody::NoteOn2 { .. }
                    | EventBody::NoteOff2 { .. }
                    | EventBody::PolyPressure2 { .. }
                    | EventBody::PerNoteCC { .. }
                    | EventBody::PerNotePitchBend { .. }
                    | EventBody::PerNoteManagement { .. }
                    | EventBody::ControlChange2 { .. }
                    | EventBody::ChannelPressure2 { .. }
                    | EventBody::PitchBend2 { .. }
                    | EventBody::ProgramChange2 { .. }
                    | EventBody::RegisteredController { .. }
                    | EventBody::AssignableController { .. }) => {
                        if data.info.midi_output_dialect != MidiDialect::Midi2 {
                            output_status = OutputEventStatus::Unsupported;
                            break 'output_events;
                        }
                        let Some(words) = encode_ump_channel_voice_2(body) else {
                            output_status = OutputEventStatus::Invalid;
                            break 'output_events;
                        };
                        let ev = clap_event_midi2 {
                            header: clap_event_header {
                                size: size_of_u32::<clap_event_midi2>(),
                                time: event.sample_offset,
                                space_id: CLAP_CORE_EVENT_SPACE_ID,
                                type_: CLAP_EVENT_MIDI2,
                                flags: 0,
                            },
                            port_index: u16::from(out_port),
                            data: words,
                        };
                        push_output!(queue_open, &raw const ev.header);
                    }
                    EventBody::ParamMod { .. } | EventBody::Transport(_) => {
                        output_status = OutputEventStatus::Unsupported;
                        break 'output_events;
                    }
                }
                if !queue_open {
                    break 'output_events;
                }
            }
        }
        scr.output_events.set_output_status(output_status);

        // Drop the channel-slice borrows we transmuted to `'static`
        // for the duration of this call. The host's `audio_inputs` /
        // `audio_outputs` buffers may be reused or freed once
        // `clap_plugin_process` returns; leaving the slice scratch
        // populated would pin dangling pointers across blocks. Clear
        // does NOT shrink capacity, so the next block reuses the
        // same allocation without touching the global allocator.
        scr.input_slices.clear();
        scr.output_slices.clear();

        match status {
            ProcessStatus::Normal => CLAP_PROCESS_CONTINUE,
            ProcessStatus::Tail(0) => CLAP_PROCESS_SLEEP,
            ProcessStatus::Tail(_) => CLAP_PROCESS_TAIL,
            ProcessStatus::KeepAlive => CLAP_PROCESS_CONTINUE_IF_NOT_QUIET,
        }
    });
    // A mid-process panic is caught by `run_audio_block_with`, which returns
    // the `CLAP_PROCESS_ERROR` fallback - but the plugin may have left the
    // output buffers half-written. Zero them so the host doesn't play stale
    // garbage for a block before it reacts to the error, matching VST3 / VST2
    // / AAX. (`CLAP_PROCESS_ERROR` is otherwise only the null-process guard,
    // where the zero is a no-op.)
    if result == CLAP_PROCESS_ERROR {
        unsafe { zero_clap_output_buffers(process) };
    }
    result
}

/// Zero every output-buffer channel in `process` (both `f32` and `f64` wires).
/// Used on the process error path so a caught panic doesn't leak stale samples
/// into the host's speakers.
///
/// # Safety
/// `process`, when non-null, must be a valid `clap_process` whose output
/// buffers are sized to `frames_count`.
unsafe fn zero_clap_output_buffers(process: *const clap_process) {
    if process.is_null() {
        return;
    }
    unsafe {
        let proc = &*process;
        let num_frames = proc.frames_count as usize;
        for bus_idx in 0..proc.audio_outputs_count {
            let buf = &*proc.audio_outputs.add(bus_idx as usize);
            let ch_count = buf.channel_count as usize;
            if !buf.data32.is_null() {
                for ch in 0..ch_count {
                    let ptr = *buf.data32.add(ch);
                    if !ptr.is_null() {
                        std::ptr::write_bytes(ptr, 0, num_frames);
                    }
                }
            } else if !buf.data64.is_null() {
                for ch in 0..ch_count {
                    let ptr = *buf.data64.add(ch);
                    if !ptr.is_null() {
                        std::ptr::write_bytes(ptr, 0, num_frames);
                    }
                }
            }
        }
    }
}

/// Test-only smoke helper for the `rt-paranoid` CI gate: drives a few
/// real CLAP `process` callbacks through this wrapper's per-block glue
/// (event conversion, transport, `process_chunked`, output narrow,
/// snapshot publish) via the full plugin vtable, and returns the
/// audio-thread allocation count of a steady-state block (0 = clean).
/// Small stereo buffers, no input events. Vacuously 0 unless the
/// `rt-paranoid` feature installs the checking allocator. Not public API.
#[doc(hidden)]
#[must_use]
pub fn rt_paranoid_smoke<P: PluginExport>() -> u32 {
    const FRAMES: u32 = 512;
    const CHANNELS: u32 = 2;

    // `clap_plugin_init` queries the params extension through the host's
    // `get_extension`, so that pointer must be a live stub; the rest of
    // the host callbacks go unused on this path.
    unsafe extern "C" fn no_extension(
        _host: *const clap_host,
        _id: *const c_char,
    ) -> *const c_void {
        ptr::null()
    }
    // No-op output-event sink. The wrapper null-checks `out_events` and
    // only calls `try_push` when the plugin emits events (this passthrough
    // does not), but a live sink keeps the harness honest if that changes.
    unsafe extern "C" fn no_push(
        _list: *const clap_output_events,
        _event: *const clap_event_header,
    ) -> bool {
        true
    }

    let frames = FRAMES as usize;
    // Leaked so their addresses outlive the plugin instance below.
    let descriptor: &'static clap_plugin_descriptor = Box::leak(Box::new(clap_plugin_descriptor {
        clap_version: CLAP_VERSION,
        id: ptr::null(),
        name: ptr::null(),
        vendor: ptr::null(),
        url: ptr::null(),
        manual_url: ptr::null(),
        support_url: ptr::null(),
        version: ptr::null(),
        description: ptr::null(),
        features: ptr::null(),
    }));
    let host: &'static clap_host = Box::leak(Box::new(clap_host {
        clap_version: CLAP_VERSION,
        host_data: ptr::null_mut(),
        name: ptr::null(),
        vendor: ptr::null(),
        url: ptr::null(),
        version: ptr::null(),
        get_extension: Some(no_extension),
        request_restart: None,
        request_process: None,
        request_callback: None,
    }));

    // SAFETY: constructs, drives, and destroys its own instance through
    // the plugin vtable. Every pointer handed to `process` (audio
    // buffers, the channel-pointer arrays, the output-event sink)
    // outlives each call, and the buffers are sized to `FRAMES`.
    unsafe {
        let plugin = create_plugin_instance::<P>(descriptor, host);
        let vtable = &*plugin;
        (vtable.init.unwrap())(plugin);
        (vtable.activate.unwrap())(plugin, 48_000.0, 1, FRAMES);
        (vtable.start_processing.unwrap())(plugin);

        // Non-zero constant input so a working passthrough-times-gain
        // renders non-zero output; the assert below fails loudly if a
        // regressed harness never actually ran `process`. CLAP's
        // `clap_audio_buffer::data32` is `*mut *mut f32`, so the input
        // storage is mutable too even though the wrapper only reads it.
        let mut in_left = vec![0.5f32; frames];
        let mut in_right = vec![0.5f32; frames];
        let mut out_left = vec![0f32; frames];
        let mut out_right = vec![0f32; frames];
        let mut in_ptrs: [*mut f32; 2] = [in_left.as_mut_ptr(), in_right.as_mut_ptr()];
        let mut out_ptrs: [*mut f32; 2] = [out_left.as_mut_ptr(), out_right.as_mut_ptr()];

        let input_bus = clap_audio_buffer {
            data32: in_ptrs.as_mut_ptr(),
            data64: ptr::null_mut(),
            channel_count: CHANNELS,
            latency: 0,
            constant_mask: 0,
        };
        let mut output_bus = clap_audio_buffer {
            data32: out_ptrs.as_mut_ptr(),
            data64: ptr::null_mut(),
            channel_count: CHANNELS,
            latency: 0,
            constant_mask: 0,
        };
        let sink = clap_output_events {
            ctx: ptr::null_mut(),
            try_push: Some(no_push),
        };
        let process = clap_process {
            steady_time: 0,
            frames_count: FRAMES,
            transport: ptr::null(),
            audio_inputs: &raw const input_bus,
            audio_outputs: &raw mut output_bus,
            audio_inputs_count: 1,
            audio_outputs_count: 1,
            in_events: ptr::null(),
            out_events: &raw const sink,
        };

        let mut count = 0;
        // A few blocks so any legitimate first-block warmup is behind us;
        // the last block is the steady-state measurement.
        for _ in 0..3 {
            let ((), n) = audit(|| {
                (vtable.process.unwrap())(plugin, &raw const process);
            });
            count = n;
        }

        (vtable.stop_processing.unwrap())(plugin);
        (vtable.deactivate.unwrap())(plugin);
        (vtable.destroy.unwrap())(plugin);

        // A no-op harness (process never ran) leaves the output silent;
        // a working passthrough-times-gain does not.
        debug_assert!(
            out_left
                .iter()
                .chain(out_right.iter())
                .any(|s| s.abs() > 0.0),
            "CLAP smoke harness produced silent output - process did not run"
        );
        count
    }
}

// ---------------------------------------------------------------------------
// Extension: params
// ---------------------------------------------------------------------------

fn parameter_indices(
    infos: &[ParamInfo],
    mut presentation: impl FnMut(u32) -> Option<moose_params::ParameterPresentation>,
) -> Vec<usize> {
    infos
        .iter()
        .enumerate()
        .filter_map(|(index, info)| {
            presentation(info.id)
                .is_none_or(|p| p.available)
                .then_some(index)
        })
        .collect()
}

// Main thread only. Active instances keep their published index list until
// the host restarts them; the parameter IDs and backing storage never change.
unsafe fn refresh_parameter_list<P: PluginExport>(data: &ClapPluginData<P>) -> bool {
    let next = parameter_indices(&data.param_infos, |id| {
        data.params_arc.parameter_presentation(id)
    });
    let mut indices = data
        .exposed_param_indices
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if *indices == next {
        return false;
    }
    if data.active.load(Ordering::Relaxed) {
        drop(indices);
        if !data.parameter_restart_pending.swap(true, Ordering::Relaxed)
            && !data.host.is_null()
            && let Some(restart) = unsafe { (*data.host).request_restart }
        {
            unsafe { restart(data.host) };
        }
        return false;
    }
    *indices = next;
    drop(indices);
    data.parameter_restart_pending
        .store(false, Ordering::Relaxed);
    if !data.host.is_null()
        && let Some(get_extension) = unsafe { (*data.host).get_extension }
    {
        let extension = unsafe { get_extension(data.host, CLAP_EXT_REMOTE_CONTROLS.as_ptr()) }
            .cast::<clap_host_remote_controls>();
        if !extension.is_null()
            && let Some(changed) = unsafe { (*extension).changed }
        {
            unsafe { changed(data.host) };
        }
    }
    true
}

unsafe extern "C" fn params_count<P: PluginExport>(plugin: *const clap_plugin) -> u32 {
    unsafe {
        let data = data_from_plugin::<P>(plugin);
        len_u32(
            data.exposed_param_indices
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .len(),
        )
    }
}

unsafe extern "C" fn params_get_info<P: PluginExport>(
    plugin: *const clap_plugin,
    param_index: u32,
    out: *mut clap_param_info,
) -> bool {
    unsafe {
        let data = data_from_plugin::<P>(plugin);
        let infos = &data.param_infos;

        let index = data
            .exposed_param_indices
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(param_index as usize)
            .copied();
        let Some(index) = index else { return false };
        let info = &infos[index];
        let out = &mut *out;

        out.id = info.id;
        out.cookie = ptr::null_mut();

        // Convert flags
        let mut flags: u32 = 0;
        if info.flags.contains(ParamFlags::AUTOMATABLE) {
            flags |= CLAP_PARAM_IS_AUTOMATABLE;
        }
        if info.flags.contains(ParamFlags::HIDDEN) {
            flags |= CLAP_PARAM_IS_HIDDEN;
        }
        if info.flags.contains(ParamFlags::READONLY) {
            flags |= CLAP_PARAM_IS_READONLY;
        }
        if info.flags.contains(ParamFlags::IS_BYPASS) {
            flags |= CLAP_PARAM_IS_BYPASS;
        }
        if info.flags.contains(ParamFlags::MODULATABLE) {
            flags |= CLAP_PARAM_IS_MODULATABLE;
        }
        if info.flags.contains(ParamFlags::MODULATABLE_PER_NOTE) {
            flags |= CLAP_PARAM_IS_MODULATABLE_PER_NOTE_ID;
        }
        match info.range.base() {
            ParamRange::Enum { .. } => {
                flags |= CLAP_PARAM_IS_STEPPED | CLAP_PARAM_IS_ENUM;
            }
            ParamRange::Discrete { .. } => {
                flags |= CLAP_PARAM_IS_STEPPED;
            }
            // CLAP's stepped flag means every plain value is an integer.
            // Keep fractional fixed-step ranges quantized in Moose, but
            // advertise them as continuous to CLAP hosts so the descriptor
            // stays valid and the plug-in remains loadable.
            ParamRange::Stepped { min, max, step }
                if min.is_finite()
                    && max.is_finite()
                    && step.is_finite()
                    && min.fract() == 0.0
                    && max.fract() == 0.0
                    && step.fract() == 0.0 =>
            {
                flags |= CLAP_PARAM_IS_STEPPED;
            }
            _ => {}
        }
        let presentation = data.params_arc.parameter_presentation(info.id);
        out.flags = presented_flags(flags, presentation.as_ref());

        out.min_value = info.range.min();
        out.max_value = info.range.max();
        out.default_value = info.default_plain;

        // Name
        out.name = [0; CLAP_NAME_SIZE];
        copy_str_to_buf(
            &mut out.name,
            presentation.as_ref().map_or(info.name, |p| p.name.as_str()),
        );

        // Module path (use group if non-empty)
        out.module = [0; CLAP_PATH_SIZE];
        copy_str_to_buf(
            &mut out.module,
            presentation
                .as_ref()
                .map_or(info.group, |p| p.group.as_str()),
        );

        true
    }
}

/// A runtime presentation's `hidden` overrides the static flag.
fn presented_flags(flags: u32, presentation: Option<&moose_params::ParameterPresentation>) -> u32 {
    match presentation {
        Some(p) if p.hidden => flags | CLAP_PARAM_IS_HIDDEN,
        Some(_) => flags & !CLAP_PARAM_IS_HIDDEN,
        None => flags,
    }
}

#[cfg(test)]
mod presentation_tests {
    use super::*;
    use moose_params::ParameterPresentation;

    #[test]
    fn presentation_hidden_overrides_static_flag() {
        let shown = ParameterPresentation {
            name: "A".into(),
            group: String::new(),
            hidden: false,
            available: true,
        };
        let hidden = ParameterPresentation {
            hidden: true,
            ..shown.clone()
        };
        let base = CLAP_PARAM_IS_AUTOMATABLE | CLAP_PARAM_IS_HIDDEN;
        assert_eq!(presented_flags(base, None), base);
        assert_eq!(
            presented_flags(base, Some(&shown)),
            CLAP_PARAM_IS_AUTOMATABLE
        );
        assert_eq!(
            presented_flags(CLAP_PARAM_IS_AUTOMATABLE, Some(&hidden)),
            base
        );
    }
}

unsafe extern "C" fn params_get_value<P: PluginExport>(
    plugin: *const clap_plugin,
    param_id: clap_id,
    out_value: *mut f64,
) -> bool {
    unsafe {
        let data = data_from_plugin::<P>(plugin);
        match data.params_arc.get_plain(param_id) {
            Some(v) => {
                *out_value = v;
                true
            }
            None => false,
        }
    }
}

unsafe extern "C" fn params_value_to_text<P: PluginExport>(
    plugin: *const clap_plugin,
    param_id: clap_id,
    value: f64,
    out_buffer: *mut c_char,
    out_buffer_capacity: u32,
) -> bool {
    // Author `format_value` can panic; firewall it (false = no display text).
    run_extern_callback_with::<P, bool>("CLAP", "value_to_text", false, || unsafe {
        // A zero capacity makes `cap - 1` underflow and a null `out_buffer`
        // plus non-zero capacity would still write the trailing NUL. Treat
        // both as "host wants nothing" and return.
        if out_buffer_capacity == 0 || out_buffer.is_null() {
            return false;
        }
        let data = data_from_plugin::<P>(plugin);
        match data.params_arc.format_value(param_id, value) {
            Some(text) => {
                let _ = copy_c_str(out_buffer, out_buffer_capacity as usize, &text);
                true
            }
            None => false,
        }
    })
}

unsafe extern "C" fn params_text_to_value<P: PluginExport>(
    plugin: *const clap_plugin,
    param_id: clap_id,
    param_value_text: *const c_char,
    out_value: *mut f64,
) -> bool {
    // Author `parse_value` can panic; firewall it (false = "not parsed").
    run_extern_callback_with::<P, bool>("CLAP", "text_to_value", false, || unsafe {
        if param_value_text.is_null() {
            return false;
        }
        let data = data_from_plugin::<P>(plugin);
        let Ok(text) = CStr::from_ptr(param_value_text).to_str() else {
            return false;
        };
        match data.params_arc.parse_value(param_id, text) {
            Some(v) => {
                *out_value = v;
                true
            }
            None => false,
        }
    })
}

unsafe extern "C" fn params_flush<P: PluginExport>(
    plugin: *const clap_plugin,
    in_events: *const clap_input_events,
    out_events: *const clap_output_events,
) {
    // Event conversion + `set_plain` run here; firewall the whole sweep so a
    // panic (a hostile-host event, a param edge) can't unwind across the C ABI
    // - matching the other `clap_plugin_params` callbacks.
    run_extern_callback_with::<P, ()>("CLAP", "params_flush", (), || unsafe {
        let data = data_from_plugin::<P>(plugin);
        // `flush` is never concurrent with `process` (CLAP contract), so it
        // owns the audio scratch through the cell for its event conversion.
        let mut audio = data.audio.enter();
        let scr = &mut *audio;
        // params_flush only forwards param values to the plugin and
        // sweeps GUI-driven changes outward; it doesn't iterate the
        // event list in time order, so skip the sort.
        // params_flush is a non-audio-thread param sweep; no state-load
        // race possible here, so the drain flag stays false.
        convert_input_events::<P>(scr, &data.info, in_events, false, false, None);
        // `flush` doesn't enter `process()`, so the chunker's deferred
        // `set_plain` apply pass never runs. Apply `ParamChange` events
        // synchronously here so host-thread param sweeps (preset
        // recalls, host automation while transport is stopped, the
        // clap-validator `state-reproducibility-flush` check) take
        // effect. Mirrors what the audio-thread chunker does inside
        // `apply_pending_events`.
        let params = data.params_arc.as_ref() as &dyn Params;
        for ev in scr.event_list.iter() {
            if let EventBody::ParamChange { id, value } = ev.body {
                params.set_plain(id, value);
                defer_param_change(&mut scr.deferred_params, id, value);
            }
        }
        let _ = flush_gui_changes::<P>(data, out_events);
    });
}

fn make_params_extension<P: PluginExport>() -> clap_plugin_params {
    clap_plugin_params {
        count: Some(params_count::<P>),
        get_info: Some(params_get_info::<P>),
        get_value: Some(params_get_value::<P>),
        value_to_text: Some(params_value_to_text::<P>),
        text_to_value: Some(params_text_to_value::<P>),
        flush: Some(params_flush::<P>),
    }
}

// ---------------------------------------------------------------------------
// Extension: remote_controls
// ---------------------------------------------------------------------------

/// Splits a `ParamInfo::group` into a CLAP remote-controls
/// `(section_name, page_name)` pair on its first `/`, mirroring the
/// convention the `params` extension's `module` path already uses
/// ("/" as a tree separator). `"EQ/Lo Shelf"` becomes section `"EQ"`,
/// page `"Lo Shelf"` - a host can offer one "EQ" button that cycles
/// through each band's page. A group with no `/` is its own single-page
/// section, e.g. `"Compressor"` stays as-is. Only the first slash
/// splits, so a page name may itself contain one.
fn split_group(group: &str) -> (&str, &str) {
    match group.split_once('/') {
        Some((section, page)) => (section, page),
        None => (group, group),
    }
}

/// Chunks a plugin's params into pages of at most
/// `CLAP_REMOTE_CONTROLS_COUNT` (8), one run of pages per distinct
/// non-empty `ParamInfo::group`. A remote-control slot is a scarce
/// performance control (8 per page), so a param is skipped when it has
/// no group (opt-out; the host's generic list covers it) or is `HIDDEN`
/// / `READONLY`. `group` has two consumers - the `params` extension's
/// `module` path (pure tree organization, sensible even for a hidden
/// param) and these slots - so grouping a hidden param for tidiness must
/// not silently burn one of the user's knobs on something they can't
/// see or move.
fn remote_control_pages<'a>(
    infos: impl IntoIterator<Item = &'a ParamInfo>,
) -> Vec<(&'a str, Vec<u32>)> {
    let mut pages: Vec<(&str, Vec<u32>)> = Vec::new();
    for info in infos {
        if info.group.is_empty()
            || info
                .flags
                .intersects(ParamFlags::HIDDEN | ParamFlags::READONLY)
        {
            continue;
        }
        let existing = pages
            .iter()
            .rposition(|page| page.0 == info.group && page.1.len() < CLAP_REMOTE_CONTROLS_COUNT);
        match existing {
            Some(idx) => pages[idx].1.push(info.id),
            None => pages.push((info.group, vec![info.id])),
        }
    }
    pages
}

/// Deterministic page id from the group name and its chunk index among
/// same-named pages, so a page's id survives an unrelated reorder of
/// `Params::param_infos()`. Hand-rolled FNV-1a rather than `std`'s
/// `DefaultHasher` - that one is keyed by `RandomState` and isn't
/// guaranteed stable across processes, which a host may use `page_id`
/// to remember a user's page choice across.
fn remote_controls_page_id(group: &str, chunk_index: usize) -> clap_id {
    let mut hash: u32 = 0x811c_9dc5;
    for &b in group.as_bytes() {
        hash ^= u32::from(b);
        hash = hash.wrapping_mul(0x0100_0193);
    }
    for &b in &chunk_index.to_le_bytes() {
        hash ^= u32::from(b);
        hash = hash.wrapping_mul(0x0100_0193);
    }
    hash
}

unsafe extern "C" fn remote_controls_count<P: PluginExport>(plugin: *const clap_plugin) -> u32 {
    unsafe {
        let data = data_from_plugin::<P>(plugin);
        let indices = data
            .exposed_param_indices
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        len_u32(remote_control_pages(indices.iter().map(|&i| &data.param_infos[i])).len())
    }
}

unsafe extern "C" fn remote_controls_get<P: PluginExport>(
    plugin: *const clap_plugin,
    page_index: u32,
    out: *mut clap_remote_controls_page,
) -> bool {
    unsafe {
        let data = data_from_plugin::<P>(plugin);
        let indices = data
            .exposed_param_indices
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let pages = remote_control_pages(indices.iter().map(|&i| &data.param_infos[i]));
        let page_index = page_index as usize;
        if page_index >= pages.len() {
            return false;
        }
        let group: &str = pages[page_index].0;
        let ids: &[u32] = &pages[page_index].1;

        // Chunk index among pages sharing this group name, so two pages
        // from the same >8-param group get distinct, stable ids.
        let chunk_index = pages[..page_index]
            .iter()
            .filter(|page| page.0 == group)
            .count();

        let (section_name, page_name) = split_group(group);

        let out = &mut *out;
        out.section_name = [0; CLAP_NAME_SIZE];
        copy_str_to_buf(&mut out.section_name, section_name);
        out.page_id = remote_controls_page_id(group, chunk_index);
        out.page_name = [0; CLAP_NAME_SIZE];
        copy_str_to_buf(&mut out.page_name, page_name);
        out.param_ids = [CLAP_INVALID_ID; CLAP_REMOTE_CONTROLS_COUNT];
        for (slot, &id) in out.param_ids.iter_mut().zip(ids.iter()) {
            *slot = id;
        }
        out.is_for_preset = false;

        true
    }
}

fn make_remote_controls_extension<P: PluginExport>() -> clap_plugin_remote_controls {
    clap_plugin_remote_controls {
        count: Some(remote_controls_count::<P>),
        get: Some(remote_controls_get::<P>),
    }
}

// ---------------------------------------------------------------------------
// Extension: state
// ---------------------------------------------------------------------------

unsafe extern "C" fn state_save<P: PluginExport>(
    plugin: *const clap_plugin,
    stream: *const clap_ostream,
) -> bool {
    // Guard the user's `save_state()` against panics so a stray
    // `unwrap` in custom-state code degrades to "save failed" instead
    // of aborting the host across this `extern "C"` boundary.
    run_extern_callback_with::<P, bool>("CLAP", "save_state", false, || unsafe {
        let data = data_from_plugin::<P>(plugin);
        // While inactive the audio thread never runs, so a state edit the
        // editor queued through `set_state` (or a load) sits undrained in
        // `pending_state` and would not reach the snapshot below - the save
        // would serialize the pre-edit state. Drain and apply it here on the
        // host thread, which owns the plugin while inactive, then republish
        // so the snapshot read reflects the edit. While active the audio
        // thread owns the plugin and drains the queue itself, so we must not
        // touch it here (read the snapshot only).
        if !data.active.load(Ordering::Relaxed)
            && let Some(deserialized) = data.pending_state.pop()
        {
            let mut instance = enter_plugin(&data.plugin);
            state::apply_state(&mut *instance, &deserialized);
            instance.republish_snapshot();
        }
        let (ids, values) = data.params_arc.collect_values();
        // Read the custom state from the lock-free snapshot the audio
        // thread publishes each block. Never touches the plugin, so it
        // can't stall a block in flight.
        let extra = save_extra(&data.snapshot);
        let persist = data.params_arc.serialize_persist();
        let blob = state::serialize_state(data.plugin_id_hash, &ids, &values, &extra, &persist);

        // Write to the CLAP output stream
        let Some(write_fn) = (*stream).write else {
            return false;
        };

        let mut offset = 0usize;
        while offset < blob.len() {
            let written = write_fn(
                stream,
                blob[offset..].as_ptr().cast::<c_void>(),
                (blob.len() - offset) as u64,
            );
            if written <= 0 {
                return false;
            }
            // `written > 0` checked above; on 32-bit targets the cast
            // narrows but blob.len() also fits in usize.
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let n = written as usize;
            offset += n;
        }

        true
    })
}

/// Largest state blob a host stream may hand us. Anything bigger is a
/// misbehaving stream (or an unrelated file), not a moose state.
const MAX_HOST_STATE_BYTES: usize = 32 * 1024 * 1024;

/// Drain a host `clap_istream` into one bounded blob. `read` fills the
/// buffer and returns the host's byte count: `0` is end of stream, a
/// negative count is a read error. A count larger than the buffer, or a
/// stream past [`MAX_HOST_STATE_BYTES`], fails the load before any
/// further allocation.
fn read_host_state(mut read: impl FnMut(&mut [u8]) -> i64) -> Option<Vec<u8>> {
    let mut blob = Vec::new();
    let mut buf = [0u8; 4096];
    loop {
        let n = usize::try_from(read(&mut buf)).ok()?;
        if n == 0 {
            return Some(blob);
        }
        if n > buf.len() || blob.len() + n > MAX_HOST_STATE_BYTES {
            return None;
        }
        blob.extend_from_slice(&buf[..n]);
    }
}

unsafe extern "C" fn state_load<P: PluginExport>(
    plugin: *const clap_plugin,
    stream: *const clap_istream,
) -> bool {
    // Guard the user's `load_state()` / custom deserialize against
    // panics so a malformed blob degrades to "load failed" instead of
    // aborting the host across this `extern "C"` boundary.
    run_extern_callback_with::<P, bool>("CLAP", "load_state", false, || unsafe {
        let data = data_from_plugin::<P>(plugin);

        let Some(read_fn) = (*stream).read else {
            return false;
        };

        let Some(blob) = read_host_state(|buf| {
            read_fn(stream, buf.as_mut_ptr().cast::<c_void>(), buf.len() as u64)
        }) else {
            return false;
        };

        if blob.is_empty() {
            return false;
        }

        // Not this plugin's envelope? Offer the bytes to the plugin's
        // `migrate_state` hook (legacy sessions from a pre-moose
        // build); `None` fails the load honestly.
        let Some(deserialized) = state::parse_or_migrate::<P>(
            &blob,
            data.plugin_id_hash,
            state::PluginFormat::Clap,
            None,
        ) else {
            return false;
        };

        // Apply params synchronously on the host thread (atomic-safe)
        // so host queries that read parameter values right after
        // `clap_plugin_state.load` see the restored values without
        // first running a process block - clap-validator reads back
        // immediately after a load round-trip.
        state::apply_params(&*data.params_arc, &deserialized);

        if data.active.load(Ordering::Relaxed) {
            // Active: the audio thread will drain `pending_state` at the
            // top of the next block and apply the custom-state blob
            // under its exclusive `&mut plugin`. `force_push` overwrites
            // any older pending blob - see the `pending_state` field
            // comment for why "newest wins" is right.
            let _ = data.pending_state.force_push(deserialized);
        } else {
            // Inactive: no `process` will run, so the queue would never
            // drain and the plugin's custom state would stay stale until
            // the next activate. Apply the full state (params + extra)
            // synchronously under exclusive plugin ownership - uncontended here
            // since no audio thread is processing.
            let mut instance = enter_plugin(&data.plugin);
            state::apply_state(&mut *instance, &deserialized);
            // No `process` will publish, so refresh the snapshot slot now
            // - a save that follows while still inactive reads live state.
            instance.republish_snapshot();
        }

        if let Some(ref mut editor) = data.gui.enter().editor {
            editor.state_changed();
        }
        request_value_rescan(data);

        true
    })
}

/// A recalled state may change every parameter behind the host's back, and
/// the host must hear it through `clap_host_params::rescan`. Coalesce onto
/// `on_main_thread` like editor-originated changes, so a host that loads
/// state from another thread still gets the rescan on the main thread.
unsafe fn request_value_rescan<P: PluginExport>(data: &ClapPluginData<P>) {
    unsafe {
        if !data.needs_rescan.swap(true, Ordering::Relaxed)
            && !data.host.is_null()
            && let Some(request_callback) = (*data.host).request_callback
        {
            request_callback(data.host);
        }
    }
}

fn make_state_extension<P: PluginExport>() -> clap_plugin_state {
    clap_plugin_state {
        save: Some(state_save::<P>),
        load: Some(state_load::<P>),
    }
}

// ---------------------------------------------------------------------------
// Extension: preset-load
// ---------------------------------------------------------------------------

/// `CLAP_EXT_PRESET_LOAD.from_location`: read a `.trucepreset` file
/// the discovery provider surfaced and apply its embedded state
/// envelope - the same host-thread-apply + audio-thread-handoff path
/// `state_load` takes, so preset recall and session restore are one
/// code path from here down.
unsafe extern "C" fn preset_load_from_location<P: PluginExport>(
    plugin: *const clap_plugin,
    location_kind: clap_preset_discovery_location_kind,
    location: *const c_char,
    load_key: *const c_char,
) -> bool {
    // Runs author `migrate_state`, and (on the inactive branch) a
    // synchronous `load_state` + `editor.state_changed()`. Firewall it like
    // `state_load` so a panic degrades to "load failed" instead of
    // unwinding across this `extern "C"` boundary and aborting the host
    // mid-preset-browse.
    run_extern_callback_with::<P, bool>("CLAP", "preset_load", false, || unsafe {
        if location_kind != CLAP_PRESET_DISCOVERY_LOCATION_FILE || location.is_null() {
            return false;
        }
        let Ok(path) = CStr::from_ptr(location).to_str() else {
            return false;
        };
        let data = data_from_plugin::<P>(plugin);

        // Same classification as a session load: an envelope under a
        // different plugin id (a pre-identity-change save) is offered
        // to the plugin's `migrate_state` hook instead of refused
        // outright. Discovery only lists same-id presets, but a host
        // can hand any path here (drag-drop, saved location).
        let Some(bytes) = std::fs::read(Path::new(path)).ok() else {
            return false;
        };
        let Some((_, blob)) = parse_preset_file(&bytes) else {
            return false;
        };
        let Some(deserialized) =
            state::parse_or_migrate::<P>(&blob, data.plugin_id_hash, PluginFormat::Clap, None)
        else {
            return false;
        };

        state::apply_params(&*data.params_arc, &deserialized);
        // Same active/inactive split as `state_load`: while inactive no
        // `process` drains `pending_state`, so a synchronous apply is what
        // keeps offline preset browsing from stranding the custom-state
        // blob (sample buffers, wavetables) on the queue.
        if data.active.load(Ordering::Relaxed) {
            let _ = data.pending_state.force_push(deserialized);
        } else {
            let mut instance = enter_plugin(&data.plugin);
            state::apply_state(&mut *instance, &deserialized);
            instance.republish_snapshot();
        }
        if let Some(ref mut editor) = data.gui.enter().editor {
            editor.state_changed();
        }
        request_value_rescan(data);

        // Tell the host the preset landed so it can update its
        // preset chrome (Bitwig's preset name display reads this).
        if !data.host.is_null()
            && let Some(get_ext) = (*data.host).get_extension
        {
            let host_ext =
                get_ext(data.host, CLAP_EXT_PRESET_LOAD.as_ptr()).cast::<clap_host_preset_load>();
            if !host_ext.is_null()
                && let Some(loaded) = (*host_ext).loaded
            {
                loaded(data.host, location_kind, location, load_key);
            }
        }
        true
    })
}

fn make_preset_load_extension<P: PluginExport>() -> clap_plugin_preset_load {
    clap_plugin_preset_load {
        from_location: Some(preset_load_from_location::<P>),
    }
}

// ---------------------------------------------------------------------------
// Extension: audio_ports
// ---------------------------------------------------------------------------

/// The `bus_layouts()` index the host selected through
/// `clap.audio-ports-config`, clamped in range. Falls back to 0 for a
/// single-layout plugin or a null handle.
unsafe fn selected_layout_index<P: PluginExport>(
    plugin: *const clap_plugin,
    layouts_len: usize,
) -> usize {
    if layouts_len <= 1 || plugin.is_null() {
        return 0;
    }
    let data = unsafe { data_from_plugin::<P>(plugin) };
    (data.selected_config.load(Ordering::Relaxed) as usize).min(layouts_len - 1)
}

/// The CLAP main-port-type string for a channel config (`null` for a
/// custom count, which CLAP leaves untyped).
fn clap_port_type_ptr(channels: ChannelConfig) -> *const c_char {
    match channels {
        ChannelConfig::Mono => CLAP_PORT_MONO.as_ptr(),
        ChannelConfig::Stereo => CLAP_PORT_STEREO.as_ptr(),
        ChannelConfig::Custom(_) => ptr::null(),
    }
}

unsafe extern "C" fn audio_ports_count<P: PluginExport>(
    plugin: *const clap_plugin,
    is_input: bool,
) -> u32 {
    let layouts = P::bus_layouts();
    let idx = unsafe { selected_layout_index::<P>(plugin, layouts.len()) };
    let Some(layout) = layouts.get(idx) else {
        return 0;
    };
    if is_input {
        len_u32(layout.inputs.iter().filter(|bus| bus.enabled).count())
    } else {
        len_u32(layout.outputs.iter().filter(|bus| bus.enabled).count())
    }
}

/// CLAP allows one main port per direction, at index 0. Take the role
/// from the bus kind so a layout whose first enabled bus is a sidechain
/// (its main bus disabled) doesn't advertise the sidechain as main.
fn audio_port_main_flag(index: u32, kind: BusKind) -> u32 {
    if index == 0 && kind == BusKind::Main {
        CLAP_AUDIO_PORT_IS_MAIN
    } else {
        0
    }
}

unsafe extern "C" fn audio_ports_get<P: PluginExport>(
    plugin: *const clap_plugin,
    index: u32,
    is_input: bool,
    info: *mut clap_audio_port_info,
) -> bool {
    unsafe {
        let layouts = P::bus_layouts();
        let idx = selected_layout_index::<P>(plugin, layouts.len());
        let Some(layout) = layouts.get(idx) else {
            return false;
        };

        let buses = if is_input {
            &layout.inputs
        } else {
            &layout.outputs
        };

        let Some(bus) = buses.iter().filter(|bus| bus.enabled).nth(index as usize) else {
            return false;
        };

        let out = &mut *info;
        out.id = index;
        out.name = [0; CLAP_NAME_SIZE];
        let named = if is_input {
            None
        } else {
            data_from_plugin::<P>(plugin).params_arc.output_port_name(index)
        };
        copy_str_to_buf(&mut out.name, named.as_deref().unwrap_or(bus.name));
        out.channel_count = bus.channels.channel_count();
        out.flags = audio_port_main_flag(index, bus.kind);
        // f64 plugins take the host's 64-bit wire directly (zero
        // copy, no precision loss at the boundary); the process loop
        // reads whichever of data32/data64 the host picked per port.
        if <P as PluginRuntime>::Sample::IS_F64 {
            out.flags |= CLAP_AUDIO_PORT_SUPPORTS_64BITS | CLAP_AUDIO_PORT_PREFERS_64BITS;
        }
        out.port_type = match bus.channels {
            ChannelConfig::Mono => CLAP_PORT_MONO.as_ptr(),
            ChannelConfig::Stereo => CLAP_PORT_STEREO.as_ptr(),
            ChannelConfig::Custom(_) => ptr::null(),
        };
        out.in_place_pair = CLAP_INVALID_ID;

        true
    }
}

fn make_audio_ports_extension<P: PluginExport>() -> clap_plugin_audio_ports {
    clap_plugin_audio_ports {
        count: Some(audio_ports_count::<P>),
        get: Some(audio_ports_get::<P>),
    }
}

// ---------------------------------------------------------------------------
// Extension: audio_ports_config (switch between the plugin's bus layouts)
// ---------------------------------------------------------------------------

unsafe extern "C" fn audio_ports_config_count<P: PluginExport>(_plugin: *const clap_plugin) -> u32 {
    len_u32(P::bus_layouts().len())
}

fn main_bus(buses: &[BusConfig]) -> Option<&BusConfig> {
    buses
        .iter()
        .find(|bus| bus.enabled)
        .filter(|bus| bus.kind == BusKind::Main)
}

unsafe extern "C" fn audio_ports_config_get<P: PluginExport>(
    _plugin: *const clap_plugin,
    index: u32,
    config: *mut clap_audio_ports_config,
) -> bool {
    unsafe {
        let layouts = P::bus_layouts();
        let Some(layout) = layouts.get(index as usize) else {
            return false;
        };
        let out = &mut *config;
        // Config id == index into `bus_layouts()`; `select` maps back.
        out.id = index;
        out.name = [0; CLAP_NAME_SIZE];
        let name = format!(
            "{} in / {} out",
            layout.total_input_channels(),
            layout.total_output_channels()
        );
        copy_str_to_buf(&mut out.name, &name);
        out.input_port_count = len_u32(layout.inputs.iter().filter(|bus| bus.enabled).count());
        out.output_port_count = len_u32(layout.outputs.iter().filter(|bus| bus.enabled).count());
        // The main port is the first enabled bus, and only if it is Main.
        let main_input = main_bus(&layout.inputs);
        out.has_main_input = main_input.is_some();
        out.main_input_channel_count = main_input.map_or(0, |b| b.channels.channel_count());
        out.main_input_port_type =
            main_input.map_or(ptr::null(), |b| clap_port_type_ptr(b.channels));
        let main_output = main_bus(&layout.outputs);
        out.has_main_output = main_output.is_some();
        out.main_output_channel_count = main_output.map_or(0, |b| b.channels.channel_count());
        out.main_output_port_type =
            main_output.map_or(ptr::null(), |b| clap_port_type_ptr(b.channels));
        true
    }
}

unsafe extern "C" fn audio_ports_config_select<P: PluginExport>(
    plugin: *const clap_plugin,
    config_id: clap_id,
) -> bool {
    unsafe {
        if (config_id as usize) >= P::bus_layouts().len() {
            return false;
        }
        // Called on the main thread while deactivated; the host rescans
        // `audio_ports` afterward, which now reports this layout.
        data_from_plugin::<P>(plugin)
            .selected_config
            .store(config_id, Ordering::Relaxed);
        true
    }
}

fn make_audio_ports_config_extension<P: PluginExport>() -> clap_plugin_audio_ports_config {
    clap_plugin_audio_ports_config {
        count: Some(audio_ports_config_count::<P>),
        get: Some(audio_ports_config_get::<P>),
        select: Some(audio_ports_config_select::<P>),
    }
}

// ---------------------------------------------------------------------------
// Extension: note_ports (only for instruments)
// ---------------------------------------------------------------------------

unsafe extern "C" fn note_ports_count<P: PluginExport>(
    _plugin: *const clap_plugin,
    is_input: bool,
) -> u32 {
    // Declare the plugin's MIDI ports for this direction. Count comes
    // from `PluginInfo` (category default of 0-or-1, raised by
    // `midi_input_ports` / `midi_output_ports`). A plain audio effect
    // declares neither direction.
    let info = P::info();
    let ports = if is_input {
        info.midi_input_ports
    } else {
        info.midi_output_ports
    };
    u32::from(ports)
}

unsafe extern "C" fn note_ports_get<P: PluginExport>(
    _plugin: *const clap_plugin,
    index: u32,
    is_input: bool,
    info: *mut clap_note_port_info,
) -> bool {
    unsafe {
        let ports = if is_input {
            u32::from(P::info().midi_input_ports)
        } else {
            u32::from(P::info().midi_output_ports)
        };
        if index >= ports {
            return false;
        }

        let out = &mut *info;
        // Port ids must be unique across the plugin. Fold the direction
        // bit and the index together: input k -> 2k, output k -> 2k+1.
        out.id = (index << 1) | u32::from(!is_input);
        // Notes go out as `CLAP_EVENT_NOTE`, but every other
        // channel-voice message and SysEx go out as `CLAP_EVENT_MIDI`
        // (raw MIDI dialect), so advertise both dialects or a
        // dialect-routing host drops the raw-MIDI events. Keep CLAP
        // preferred: our own note events are CLAP-dialect, and
        // preferring MIDI would also pull host input through the
        // raw-MIDI path.
        out.supported_dialects = CLAP_NOTE_DIALECT_CLAP | CLAP_NOTE_DIALECT_MIDI;
        out.preferred_dialect = CLAP_NOTE_DIALECT_CLAP;
        // Explicit consumer capability, independent of MIDI2 or current zone.
        // Output remains ordinary MIDI unless it has its own MPE contract.
        if is_input && cfg!(feature = "mpe-input") {
            out.supported_dialects |= CLAP_NOTE_DIALECT_MIDI_MPE;
        }
        // A port that opted into MIDI 2.0 also carries UMP: advertise
        // the MIDI2 dialect so the host routes `CLAP_EVENT_MIDI2` to us
        // (in) and reads it back (out). Still CLAP-preferred - MIDI2 is
        // the opt-in native path, not the default.
        let dialect = if is_input {
            P::info().midi_input_dialect
        } else {
            P::info().midi_output_dialect
        };
        if dialect == MidiDialect::Midi2 {
            out.supported_dialects |= CLAP_NOTE_DIALECT_MIDI2;
        }
        out.name = [0; CLAP_NAME_SIZE];
        let base = if is_input {
            "Note Input"
        } else {
            "Note Output"
        };
        // Number the ports only when there's more than one, so the
        // single-port case keeps its plain label. Not the audio thread -
        // allocation here is fine.
        let name = if ports > 1 {
            format!("{base} {}", index + 1)
        } else {
            base.to_string()
        };
        copy_str_to_buf(&mut out.name, &name);

        true
    }
}

fn make_note_ports_extension<P: PluginExport>() -> clap_plugin_note_ports {
    clap_plugin_note_ports {
        count: Some(note_ports_count::<P>),
        get: Some(note_ports_get::<P>),
    }
}

// ---------------------------------------------------------------------------
// GUI extension
// ---------------------------------------------------------------------------

#[cfg(target_os = "macos")]
use clap_sys::ext::gui::CLAP_WINDOW_API_COCOA;
#[cfg(target_os = "windows")]
use clap_sys::ext::gui::CLAP_WINDOW_API_WIN32;
#[cfg(target_os = "linux")]
use clap_sys::ext::gui::CLAP_WINDOW_API_X11;
use clap_sys::ext::gui::{
    CLAP_EXT_GUI, clap_gui_resize_hints, clap_host_gui, clap_plugin_gui, clap_window,
};

unsafe extern "C" fn gui_is_api_supported<P: PluginExport>(
    _plugin: *const clap_plugin,
    api: *const c_char,
    is_floating: bool,
) -> bool {
    unsafe { gui_api_supported(api, is_floating) }
}

unsafe fn gui_api_supported(api: *const c_char, is_floating: bool) -> bool {
    unsafe {
        if is_floating || api.is_null() {
            return false;
        }
        let api = CStr::from_ptr(api);
        #[cfg(target_os = "macos")]
        if api == CLAP_WINDOW_API_COCOA {
            return true;
        }
        #[cfg(target_os = "windows")]
        if api == CLAP_WINDOW_API_WIN32 {
            return true;
        }
        #[cfg(target_os = "linux")]
        if api == CLAP_WINDOW_API_X11 {
            return true;
        }
        false
    }
}

unsafe extern "C" fn gui_get_preferred_api<P: PluginExport>(
    _plugin: *const clap_plugin,
    api: *mut *const c_char,
    is_floating: *mut bool,
) -> bool {
    unsafe {
        #[cfg(target_os = "macos")]
        {
            *api = CLAP_WINDOW_API_COCOA.as_ptr();
            *is_floating = false;
            return true;
        }
        #[cfg(target_os = "windows")]
        {
            *api = CLAP_WINDOW_API_WIN32.as_ptr();
            *is_floating = false;
            return true;
        }
        #[cfg(target_os = "linux")]
        {
            *api = CLAP_WINDOW_API_X11.as_ptr();
            *is_floating = false;
            return true;
        }
        #[allow(unreachable_code)]
        false
    }
}

unsafe extern "C" fn gui_create<P: PluginExport>(
    plugin: *const clap_plugin,
    _api: *const c_char,
    _is_floating: bool,
) -> bool {
    // The editor builder is author code; firewall its construction so a
    // panic can't unwind across the C ABI (false = "no editor created").
    run_extern_callback_with::<P, bool>("CLAP", "gui_create", false, || unsafe {
        let data = data_from_plugin::<P>(plugin);
        let mut gui = data.gui.enter();
        if gui.gui_created {
            return true;
        }
        // Built through the cached lock-free editor factory, so opening
        // the GUI never stalls the audio thread (and `--shell` rebuilds
        // from the reloaded dylib).
        gui.editor = (data.editor_builder)(data.params_arc.clone());
        // Each `create` builds a fresh editor, but the host's scale lives
        // on the instance and outlives `destroy`. A host that sets it only
        // once, or calls `set_scale` before `create`, would leave the new
        // editor at 1.0 inside the host-scaled frame `gui_get_size`
        // reports, so replay it. VST3's `cb_gui_open` does the same.
        if let (Some(scale), Some(editor)) = (data.reported_host_scale(), gui.editor.as_mut()) {
            editor.set_scale_factor(scale);
        }
        gui.gui_created = gui.editor.is_some();
        gui.gui_created
    })
}

unsafe extern "C" fn gui_destroy<P: PluginExport>(plugin: *const clap_plugin) {
    unsafe {
        let data = data_from_plugin::<P>(plugin);
        // Take the editor out under the cell guard, then release the guard
        // before tearing it down so author teardown code can't re-enter the
        // cell (e.g. a stray `request_resize`).
        let editor = {
            let mut gui = data.gui.enter();
            gui.gui_created = false;
            gui.editor.take()
        };
        // Both `editor.close()` and the `Box<dyn Editor>` drop run author
        // teardown (wgpu surface drop, NSView teardown, baseview window
        // close). Firewall the whole teardown - a panic in either would
        // otherwise become an unhandled Obj-C exception and abort the host.
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
            if let Some(mut editor) = editor {
                editor.close();
            }
        }));
    }
}

unsafe extern "C" fn gui_set_scale<P: PluginExport>(
    plugin: *const clap_plugin,
    scale: f64,
) -> bool {
    // `Editor::set_scale_factor` is author code; firewall it so a panic
    // can't unwind across the C ABI (false = "scale not applied").
    run_extern_callback_with::<P, bool>("CLAP", "gui_set_scale", false, || unsafe {
        // macOS: the Cocoa GUI API is in logical points and AppKit applies
        // the backing scale, so refuse (the spec's answer for "the OS
        // handles scaling"). Accepting would divide host sizes by it.
        if cfg!(target_os = "macos") || !scale.is_finite() || scale <= 0.0 {
            return false;
        }
        let data = data_from_plugin::<P>(plugin);
        data.set_host_scale(scale);
        if let Some(ref mut editor) = data.gui.enter().editor {
            editor.set_scale_factor(scale);
        }
        true
    })
}

unsafe extern "C" fn gui_get_size<P: PluginExport>(
    plugin: *const clap_plugin,
    width: *mut u32,
    height: *mut u32,
) -> bool {
    // `Editor::size` is author code; firewall it so a panic can't unwind
    // across the C ABI (false = "size unavailable").
    run_extern_callback_with::<P, bool>("CLAP", "gui_get_size", false, || unsafe {
        let data = data_from_plugin::<P>(plugin);
        // Re-entered while an outer GUI callback holds the cell (mid-resize):
        // report the size the plugin just requested through `request_resize`,
        // leaving the host's value when there is none - never re-enter the cell.
        let Some(mut gui) = data.gui.try_enter() else {
            let packed = data.pending_resize.load(Ordering::Relaxed);
            if packed == 0 {
                return false;
            }
            #[allow(clippy::cast_possible_truncation)]
            {
                *width = (packed >> 32) as u32;
                *height = packed as u32;
            }
            return true;
        };
        // Apply a resize the plugin requested re-entrantly (stashed by
        // `gui_set_size` because the cell was busy) before reporting.
        let packed = data.pending_resize.swap(0, Ordering::Relaxed);
        if let Some(editor) = gui.editor.as_deref() {
            data.note_window_scale(editor);
        }
        if packed != 0
            && let Some(editor) = gui.editor.as_mut()
        {
            #[allow(clippy::cast_possible_truncation)]
            let (pw, ph) = ((packed >> 32) as u32, packed as u32);
            apply_physical_resize(editor.as_mut(), pw, ph, data.host_scale());
        }
        if let Some(ref editor) = gui.editor {
            let (w, h) = editor.size();
            // Like VST3, the CLAP spec describes gui size as pixels, but
            // macOS AppKit handles Retina backing automatically. On macOS
            // we report logical points and let the host / OS scale; on
            // Windows/Linux we multiply by the host-reported scale (default
            // 1.0 if the host never called `gui.set_scale`).
            #[cfg(target_os = "macos")]
            {
                *width = w;
                *height = h;
            }
            #[cfg(not(target_os = "macos"))]
            {
                // Round-to-nearest, not truncate - `(w * scale) as u32`
                // would round 199.9 → 199, drifting one pixel on
                // fractional scales. Matches VST3 / AAX / the
                // `to_physical_px` helper used elsewhere. Logical
                // pixel sizes are bounded by `u32::MAX / scale`; in
                // practice no editor exceeds 16384 logical pixels, so
                // the `f64 → u32` truncation/sign casts are safe.
                #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                {
                    let host_scale = data.host_scale();
                    *width = (f64::from(w) * host_scale).round() as u32;
                    *height = (f64::from(h) * host_scale).round() as u32;
                }
            }
            return true;
        }
        false
    })
}

unsafe extern "C" fn gui_can_resize<P: PluginExport>(plugin: *const clap_plugin) -> bool {
    // `Editor::can_resize` is author code; firewall it (false = "not resizable").
    run_extern_callback_with::<P, bool>("CLAP", "gui_can_resize", false, || unsafe {
        let data = data_from_plugin::<P>(plugin);
        data.gui
            .enter()
            .editor
            .as_ref()
            .is_some_and(|e| e.can_resize())
    })
}

unsafe extern "C" fn gui_set_parent<P: PluginExport>(
    plugin: *const clap_plugin,
    window: *const clap_window,
) -> bool {
    // `editor.open` is author GUI-construction code; firewall it so a panic
    // can't unwind across the C ABI (false = "parenting failed").
    run_extern_callback_with::<P, bool>("CLAP", "gui_set_parent", false, || unsafe {
        gui_set_parent_inner::<P>(plugin, window)
    })
}

unsafe fn gui_set_parent_inner<P: PluginExport>(
    plugin: *const clap_plugin,
    window: *const clap_window,
) -> bool {
    unsafe {
        let Some(parent_ptr) = gui_parent_pointer(window) else {
            return false;
        };
        let data = data_from_plugin::<P>(plugin);
        let mut gui = data.gui.enter();
        let Some(editor) = gui.editor.as_mut() else {
            return false;
        };

        let params = Arc::clone(&data.params_arc);
        let meter_store = Arc::clone(&data.meter_store);
        let snapshot = Arc::clone(&data.snapshot);
        let gui_changes = data.gui_changes.clone();
        let gui_changes2 = data.gui_changes.clone();
        let gui_changes3 = data.gui_changes.clone();
        let host = SendPtr::new(data.host);
        let host_params = SendPtr::new(data.host_params);
        let request_flush = move || {
            // `host_params` is null when the host omits the optional
            // `clap_host_params` extension (the spec marks it optional);
            // dereferencing it would crash hosts that don't implement
            // params.
            let hp = host_params.as_ptr();
            if hp.is_null() {
                return;
            }
            if let Some(f) = (*hp).request_flush {
                f(host.as_ptr());
            }
        };
        // `request_flush` is a `move ||` over only `Copy` captures, so the closure
        // itself is `Copy` and we re-bind rather than `.clone()`.
        let request_flush2 = request_flush;
        let request_flush3 = request_flush;
        let needs_rescan = data.needs_rescan.clone();
        let host_for_callback = SendPtr::new(data.host);
        let params_for_set = params.clone();
        let params_for_get = params.clone();
        let params_for_plain = params.clone();
        let params_for_fmt = params.clone();
        let params_for_ctx = params.clone();
        let task_spawner_for_ctx = data.task_spawner.clone();
        let pending_state_for_set = data.pending_state.clone();
        let transport_slot = data.transport_slot.clone();
        let context = PluginContext::from_closures(
            ClosureBridge {
                begin_edit: Box::new(move |id| {
                    // Push can fail only if the audio thread hasn't
                    // drained for `GUI_QUEUE_CAPACITY` gestures.
                    // request_flush below pokes the host to call
                    // back; needs_rescan in set_param ensures the
                    // param tree picks up the latest plain values
                    // even if a begin/end pair gets dropped.
                    let _ = gui_changes.push(GuiParamChange::GestureBegin(id));
                    request_flush();
                }),
                set_param: Box::new(move |id, value| {
                    let plain = params_for_set.set_normalized_returning_plain(id, value);
                    let _ = gui_changes2.push(GuiParamChange::Value(id, plain));
                    request_flush2();
                    // Symmetry with the host-pointer null guards at
                    // `:297, :365, :1404`. CLAP guarantees a valid
                    // host on init, but a host that creates a plugin
                    // without ever calling `clap_plugin_init`-style
                    // setup (rare validators) could leave `data.host`
                    // null; the deref below would crash inside the
                    // GUI thread.
                    let host_ptr = host_for_callback.as_ptr();
                    if !needs_rescan.swap(true, Ordering::Relaxed)
                        && !host_ptr.is_null()
                        && let Some(req_cb) = (*host_ptr).request_callback
                    {
                        req_cb(host_ptr);
                    }
                }),
                end_edit: Box::new(move |id| {
                    let _ = gui_changes3.push(GuiParamChange::GestureEnd(id));
                    request_flush3();
                }),
                request_resize: Box::new({
                    let host_ptr = SendPtr::new(data.host);
                    let plugin_ptr = SendPtr::new(plugin);
                    move |lw, lh| {
                        // Read `host_scale` live (it's atomic) so a
                        // `gui_set_scale` after the editor opened is reflected
                        // instead of using a stale snapshot.
                        let host_scale = data_from_plugin::<P>(plugin_ptr.as_ptr()).host_scale();
                        request_host_resize(host_ptr.as_ptr(), host_scale, lw, lh)
                    }
                }),
                get_param: Box::new(move |id| params_for_get.get_normalized(id).unwrap_or(0.0)),
                get_param_plain: Box::new(move |id| params_for_plain.get_plain(id).unwrap_or(0.0)),
                format_param: Box::new(move |id| {
                    let plain = params_for_fmt.get_plain(id).unwrap_or(0.0);
                    params_for_fmt
                        .format_value(id, plain)
                        .unwrap_or_else(|| format!("{plain:.1}"))
                }),
                get_meter: Box::new(move |id| meter_store.read(id)),
                get_state: Box::new(move || {
                    // Editor state read: lock-free, reads the snapshot the
                    // audio thread publishes each block. Never touches the
                    // plugin, so an editor read can't stall audio.
                    save_extra(&snapshot)
                }),
                set_state: Box::new(move |bytes| {
                    // The editor sends RAW custom-state bytes - exactly
                    // what `save_state()` emits and `get_state` above
                    // returns - NOT a full `serialize_state` envelope.
                    // Route them to the plugin's `load_state` through the
                    // handoff queue. No params ride along: the editor
                    // mutates params through `set_param`.
                    //
                    // Always enqueue, never apply here: this closure can run
                    // on the editor's own thread (baseview drives a separate
                    // thread on Linux), and the plugin cell must only ever be
                    // entered from the audio thread (active) or the host main
                    // thread (inactive) - never a third. The audio thread
                    // drains the queue while active; while inactive the main
                    // thread drains it in `state_save` / `deactivate`, so an
                    // edit made in that window is neither lost nor stranded.
                    let _ = pending_state_for_set.force_push(state::DeserializedState {
                        params: Vec::new(),
                        extra: Some(bytes),
                        persist: Vec::new(),
                    });
                }),
                transport: Box::new(move || transport_slot.read()),
            },
            params_for_ctx,
        )
        .with_tasks(task_spawner_for_ctx);

        #[cfg(target_os = "macos")]
        let handle = RawWindowHandle::AppKit(parent_ptr);
        #[cfg(target_os = "windows")]
        let handle = RawWindowHandle::Win32(parent_ptr);
        #[cfg(target_os = "linux")]
        let handle = RawWindowHandle::X11(parent_ptr as u64);

        editor.open(handle, context);
        // baseview attaches its child NSView at the parent's
        // `(0, 0)`. NSView's coordinate system is unflipped, so
        // `(0, 0)` is the parent's bottom-left - which renders the
        // editor anchored to the bottom-left of Reaper's plugin
        // area when Reaper opens the window at a size larger than
        // the editor's natural dimensions (the common case for
        // backends whose `editor.set_size` doesn't actually apply,
        // e.g. vizia: Reaper falls back to a default plugin-window
        // size and our child sits in the corner of all that extra
        // space). Re-anchor the child to the parent's top with a
        // fully-fixed autoresize mask (`0`) - the parent grows the
        // empty space below/right of the child rather than dragging
        // the child along. We re-pin per-frame from the editor
        // (`reanchor_to_superview_top` in `on_frame`) since
        // baseview's `setFrameSize:` notifications during embed can
        // override the origin we set here.
        #[cfg(target_os = "macos")]
        anchor_child_to_top(parent_ptr);
        true
    }
}

/// Validate the declared native representation before reading its union member
/// or entering the editor. The pointer is still host-owned, never dereferenced here.
unsafe fn gui_parent_pointer(window: *const clap_window) -> Option<*mut c_void> {
    unsafe {
        if window.is_null() || !gui_api_supported((*window).api, false) {
            return None;
        }
        #[cfg(target_os = "macos")]
        let parent = (*window).specific.cocoa;
        #[cfg(target_os = "windows")]
        let parent = (*window).specific.win32;
        #[cfg(target_os = "linux")]
        let parent = (*window).specific.ptr;
        (!parent.is_null()).then_some(parent)
    }
}

/// Move every direct subview of `parent` so its top edge sits at
/// the parent's top in unflipped Cocoa coordinates (where
/// `origin.y = 0` is the bottom edge). Pinning is via the
/// autoresize mask `NSViewMinYMargin | NSViewMaxXMargin`, which
/// keeps the gap between child-bottom and parent-bottom flexible
/// (so a taller parent grows the empty space below the child, not
/// above it) and the gap between child-right and parent-right
/// flexible (so a wider parent grows the empty space to the right
/// of the child). The child stays at its built size; `AppKit` just
/// repositions it as the parent resizes.
///
/// Mirrors `moose-lv2`'s `anchor_child_to_top` (the non-resizable
/// arm of `anchor_child_for_resize`). No `objc` dep needed in
/// moose-clap until the autoresize-mask install path comes back -
/// we use a local `class!`-free `msg_send!` via `objc2_app_kit`
/// types instead.
#[cfg(target_os = "macos")]
unsafe fn anchor_child_to_top(parent: *mut c_void) {
    use objc::runtime::Object;
    use objc::{msg_send, sel, sel_impl};
    // `MinYMargin | MaxXMargin`: bottom margin and right margin
    // elastic, view size + top/left margins fixed. As the parent
    // resizes, AppKit grows the empty space below/right of the
    // child rather than dragging the child along. Per-frame
    // `reanchor_to_superview_top` (from `on_frame`) catches any
    // host-driven setFrame that bypasses autoresize.
    const NSVIEW_MIN_Y_MARGIN: u64 = 8;
    const NSVIEW_MAX_X_MARGIN: u64 = 4;
    #[repr(C)]
    #[derive(Clone, Copy)]
    struct NsPoint {
        x: f64,
        y: f64,
    }
    #[repr(C)]
    #[derive(Clone, Copy)]
    struct NsSize {
        width: f64,
        height: f64,
    }
    #[repr(C)]
    #[derive(Clone, Copy)]
    struct NsRect {
        origin: NsPoint,
        size: NsSize,
    }
    if parent.is_null() {
        return;
    }
    let parent_obj = parent.cast::<Object>();
    let parent_frame: NsRect = msg_send![parent_obj, frame];
    let subviews: *mut Object = msg_send![parent_obj, subviews];
    if subviews.is_null() {
        return;
    }
    let count: usize = msg_send![subviews, count];
    for i in 0..count {
        let child: *mut Object = msg_send![subviews, objectAtIndex: i];
        if child.is_null() {
            continue;
        }
        let child_frame: NsRect = msg_send![child, frame];
        let new_origin = NsPoint {
            x: child_frame.origin.x,
            y: parent_frame.size.height - child_frame.size.height,
        };
        let _: () = msg_send![child, setFrameOrigin: new_origin];
        let _: () =
            msg_send![child, setAutoresizingMask: NSVIEW_MIN_Y_MARGIN | NSVIEW_MAX_X_MARGIN];
    }
}

unsafe extern "C" fn gui_show<P: PluginExport>(_plugin: *const clap_plugin) -> bool {
    true
}

unsafe extern "C" fn gui_hide<P: PluginExport>(_plugin: *const clap_plugin) -> bool {
    true
}

/// Reports horizontal/vertical/aspect-ratio constraints from the
/// editor. Returns `false` when the editor isn't resizable (Bitwig's
/// probe-call still gets a meaningful `false`, not a `None` slot).
unsafe extern "C" fn gui_get_resize_hints<P: PluginExport>(
    plugin: *const clap_plugin,
    hints: *mut clap_gui_resize_hints,
) -> bool {
    // `Editor::can_resize` / `aspect_ratio` are author code; firewall them
    // (false = "no hints").
    run_extern_callback_with::<P, bool>("CLAP", "gui_get_resize_hints", false, || unsafe {
        let data = data_from_plugin::<P>(plugin);
        let gui = data.gui.enter();
        let Some(editor) = gui.editor.as_ref() else {
            return false;
        };
        if !editor.can_resize() {
            return false;
        }
        if hints.is_null() {
            return false;
        }
        let (aw, ah) = editor.aspect_ratio().unwrap_or((0, 0));
        *hints = clap_gui_resize_hints {
            can_resize_horizontally: true,
            can_resize_vertically: true,
            preserve_aspect_ratio: editor.aspect_ratio().is_some(),
            aspect_ratio_width: aw,
            aspect_ratio_height: ah,
        };
        true
    })
}

/// Stub: floating windows aren't supported (`is_api_supported` rejects
/// `is_floating=true`); `set_transient` is meaningless when the GUI is
/// always embedded. Present as `Some` for the same Bitwig probe-quirk
/// described on `gui_get_resize_hints`.
unsafe extern "C" fn gui_set_transient<P: PluginExport>(
    _plugin: *const clap_plugin,
    _window: *const clap_window,
) -> bool {
    false
}

/// Stub: the editor renders its own title bar; we don't need the host's
/// hint. Present as `Some` for the same Bitwig probe-quirk described
/// on `gui_get_resize_hints`.
unsafe extern "C" fn gui_suggest_title<P: PluginExport>(
    _plugin: *const clap_plugin,
    _title: *const c_char,
) {
}

/// Ask the host to resize its window to a logical size via
/// `clap_host_gui.request_resize`. Returns false when the host
/// doesn't expose the gui extension. Logical-to-physical scaling
/// matches `gui_get_size`.
unsafe fn request_host_resize(host: *const clap_host, host_scale: f64, lw: u32, lh: u32) -> bool {
    unsafe {
        if host.is_null() {
            return false;
        }
        let Some(get_ext) = (*host).get_extension else {
            return false;
        };
        let host_gui_ptr = get_ext(host, CLAP_EXT_GUI.as_ptr()).cast::<clap_host_gui>();
        if host_gui_ptr.is_null() {
            return false;
        }
        let Some(req) = (*host_gui_ptr).request_resize else {
            return false;
        };
        let (pw, ph) = scale_logical_to_physical(lw, lh, host_scale);
        req(host, pw, ph)
    }
}

/// Host commits a new size. When the editor opts into resizing,
/// delegate to `Editor::set_size`; otherwise accept only the
/// editor's current fixed size (Bitwig probe-quirk - see
/// `gui_get_resize_hints` rationale).
unsafe extern "C" fn gui_set_size<P: PluginExport>(
    plugin: *const clap_plugin,
    width: u32,
    height: u32,
) -> bool {
    // `Editor::set_size` / `can_resize` / `size` are author code; firewall
    // them (false = "size rejected").
    run_extern_callback_with::<P, bool>("CLAP", "gui_set_size", false, || unsafe {
        let data = data_from_plugin::<P>(plugin);
        // A host answering a plugin `request_resize` (`request_host_resize`)
        // synchronously calls `set_size` back here while an outer GUI callback
        // (open / set_scale / a re-entrant set_size) still holds the cell.
        // `try_enter` avoids the same-thread aliasing re-entry: apply when
        // free, else stash for `gui_get_size` to apply on the next size query.
        let Some(mut gui) = data.gui.try_enter() else {
            data.pending_resize.store(
                (u64::from(width) << 32) | u64::from(height),
                Ordering::Relaxed,
            );
            return true;
        };
        let Some(editor) = gui.editor.as_mut() else {
            return false;
        };
        data.note_window_scale(editor.as_ref());
        let host_scale = data.host_scale();
        if editor.can_resize() {
            // Host passes physical points; moose works in logical.
            // Divide by the host-applied scale before handing to the
            // editor so `set_size` receives the same units `size()`
            // returns. Fit the largest on-ratio box *inside* the request:
            // a pure function of the request (no `current` dependence), so
            // it can't oscillate when a host re-asserts a pinned axis every
            // frame (Reaper caps the editor height at its FX-window content
            // height and reverts any grow past it).
            let req = scale_physical_to_logical(width, height, host_scale);
            let (lw, lh) = fit_logical_size(req.0, req.1, editor.as_ref());
            let accepted = editor.set_size(lw, lh);
            // The fit is `<=` the request, so this only ever asks the host to
            // shrink its window onto the on-ratio size (which hosts honour) -
            // never to grow past a pinned axis. Idempotent, so the host's
            // echoed `set_size` agrees and the exchange settles.
            let adopted = editor.size();
            drop(gui);
            if accepted && adopted != req {
                request_host_resize(data.host, host_scale, adopted.0, adopted.1);
            }
            accepted
        } else {
            // Drop the editor borrow before `gui_get_size` re-enters the cell.
            drop(gui);
            let mut current_w: u32 = 0;
            let mut current_h: u32 = 0;
            if !gui_get_size::<P>(plugin, &raw mut current_w, &raw mut current_h) {
                return false;
            }
            width == current_w && height == current_h
        }
    })
}

/// Host asks "what's the nearest size you can render at?". CLAP
/// defines this as "clamp to acceptable", not "reject if not exact".
/// Resizable editors clamp to `min_size` / `max_size` and apply the
/// `aspect_ratio` constraint; fixed-size editors snap to current.
unsafe extern "C" fn gui_adjust_size<P: PluginExport>(
    plugin: *const clap_plugin,
    width: *mut u32,
    height: *mut u32,
) -> bool {
    // `Editor::can_resize` / `size` are author code; firewall them
    // (false = "no adjustment").
    run_extern_callback_with::<P, bool>("CLAP", "gui_adjust_size", false, || unsafe {
        let data = data_from_plugin::<P>(plugin);
        // `try_enter`, not `enter`: a host may query the nearest valid size
        // while an outer GUI callback holds the cell mid resize round-trip
        // (same hazard the `gui_set_size` / `gui_get_size` siblings guard).
        // When busy, leave the host's requested `*width`/`*height` unchanged
        // rather than forming a second `&mut`.
        let Some(gui) = data.gui.try_enter() else {
            return false;
        };
        let Some(editor) = gui.editor.as_ref() else {
            return false;
        };
        data.note_window_scale(editor.as_ref());
        let host_scale = data.host_scale();
        if editor.can_resize() {
            let req = scale_physical_to_logical(*width, *height, host_scale);
            let (lw, lh) = fit_logical_size(req.0, req.1, editor.as_ref());
            // Convert clamped logical back to physical for the host.
            let (pw, ph) = scale_logical_to_physical(lw, lh, host_scale);
            *width = pw;
            *height = ph;
            true
        } else {
            // Drop the editor borrow before `gui_get_size` re-enters the cell.
            drop(gui);
            let mut current_w: u32 = 0;
            let mut current_h: u32 = 0;
            if !gui_get_size::<P>(plugin, &raw mut current_w, &raw mut current_h) {
                return false;
            }
            *width = current_w;
            *height = current_h;
            true
        }
    })
}

/// Convert physical points (what the host passes in resize APIs)
/// to logical points (what `Editor` works in). CLAP host scale is
/// 1.0 when the host hasn't called `set_scale`.
fn scale_physical_to_logical(pw: u32, ph: u32, host_scale: f64) -> (u32, u32) {
    if host_scale <= 0.0 || (host_scale - 1.0).abs() < f64::EPSILON {
        return (pw, ph);
    }
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let lw = (f64::from(pw) / host_scale).round() as u32;
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let lh = (f64::from(ph) / host_scale).round() as u32;
    (lw.max(1), lh.max(1))
}

/// Apply a host-supplied physical `(w, h)` to a resizable editor: convert to
/// logical points, fit to the editor's on-ratio box, and `set_size`. Shared by
/// `gui_set_size` (the host's committed size) and the deferred-resize drain in
/// `gui_get_size`.
fn apply_physical_resize(editor: &mut dyn Editor, w: u32, h: u32, host_scale: f64) {
    if editor.can_resize() {
        let (lw, lh) = scale_physical_to_logical(w, h, host_scale);
        let (cw, ch) = fit_logical_size(lw, lh, editor);
        editor.set_size(cw, ch);
    }
}

/// Inverse of `scale_physical_to_logical`.
fn scale_logical_to_physical(lw: u32, lh: u32, host_scale: f64) -> (u32, u32) {
    if host_scale <= 0.0 || (host_scale - 1.0).abs() < f64::EPSILON {
        return (lw, lh);
    }
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let pw = (f64::from(lw) * host_scale).round() as u32;
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let ph = (f64::from(lh) * host_scale).round() as u32;
    (pw.max(1), ph.max(1))
}

fn make_gui_extension<P: PluginExport>() -> clap_plugin_gui {
    clap_plugin_gui {
        is_api_supported: Some(gui_is_api_supported::<P>),
        get_preferred_api: Some(gui_get_preferred_api::<P>),
        create: Some(gui_create::<P>),
        destroy: Some(gui_destroy::<P>),
        set_scale: Some(gui_set_scale::<P>),
        get_size: Some(gui_get_size::<P>),
        can_resize: Some(gui_can_resize::<P>),
        get_resize_hints: Some(gui_get_resize_hints::<P>),
        adjust_size: Some(gui_adjust_size::<P>),
        set_size: Some(gui_set_size::<P>),
        set_parent: Some(gui_set_parent::<P>),
        set_transient: Some(gui_set_transient::<P>),
        suggest_title: Some(gui_suggest_title::<P>),
        show: Some(gui_show::<P>),
        hide: Some(gui_hide::<P>),
    }
}

// ---------------------------------------------------------------------------
// get_extension
// ---------------------------------------------------------------------------

/// Holds the static extension structs. One per monomorphization, which is fine
/// because we only have one plugin type per shared library.
struct Extensions<P: PluginExport> {
    params: clap_plugin_params,
    remote_controls: clap_plugin_remote_controls,
    state: clap_plugin_state,
    preset_load: clap_plugin_preset_load,
    audio_ports: clap_plugin_audio_ports,
    audio_ports_config: clap_plugin_audio_ports_config,
    note_ports: clap_plugin_note_ports,
    gui: clap_plugin_gui,
    latency: clap_plugin_latency,
    tail: clap_plugin_tail,
    render: clap_plugin_render,
    _phantom: PhantomData<P>,
}

unsafe extern "C" fn latency_get<P: PluginExport>(plugin: *const clap_plugin) -> u32 {
    unsafe {
        let data = data_from_plugin::<P>(plugin);
        data.latency_cache.load(Ordering::Relaxed)
    }
}

unsafe extern "C" fn tail_get<P: PluginExport>(plugin: *const clap_plugin) -> u32 {
    unsafe {
        let data = data_from_plugin::<P>(plugin);
        data.tail_cache.load(Ordering::Relaxed)
    }
}

/// `clap.render`: we have no hard realtime requirement - the plugin can
/// render offline. Returning `false` lets hosts drive an offline bounce.
unsafe extern "C" fn render_has_hard_realtime<P: PluginExport>(
    _plugin: *const clap_plugin,
) -> bool {
    false
}

/// `clap.render::set`: the host announces realtime vs offline. Store the
/// mode; `activate` re-preps with it (hosts deactivate before an offline
/// bounce), and each `process` block reads it for the per-block mode.
unsafe extern "C" fn render_set<P: PluginExport>(
    plugin: *const clap_plugin,
    mode: clap_plugin_render_mode,
) -> bool {
    unsafe {
        let data = data_from_plugin::<P>(plugin);
        let pm = if mode == CLAP_RENDER_OFFLINE {
            ProcessMode::Offline
        } else {
            ProcessMode::Realtime
        };
        data.render_mode.store(pm.as_u8(), Ordering::Relaxed);
        true
    }
}

impl<P: PluginExport> Extensions<P> {
    fn new() -> Self {
        Self {
            params: make_params_extension::<P>(),
            remote_controls: make_remote_controls_extension::<P>(),
            state: make_state_extension::<P>(),
            preset_load: make_preset_load_extension::<P>(),
            audio_ports: make_audio_ports_extension::<P>(),
            audio_ports_config: make_audio_ports_config_extension::<P>(),
            note_ports: make_note_ports_extension::<P>(),
            gui: make_gui_extension::<P>(),
            latency: clap_plugin_latency {
                get: Some(latency_get::<P>),
            },
            tail: clap_plugin_tail {
                get: Some(tail_get::<P>),
            },
            render: clap_plugin_render {
                has_hard_realtime_requirement: Some(render_has_hard_realtime::<P>),
                set: Some(render_set::<P>),
            },
            _phantom: PhantomData,
        }
    }
}

unsafe extern "C" fn clap_plugin_get_extension<P: PluginExport>(
    plugin: *const clap_plugin,
    id: *const c_char,
) -> *const c_void {
    unsafe {
        if id.is_null() {
            return ptr::null();
        }
        let ext_id = CStr::from_ptr(id);

        let extensions = &data_from_plugin::<P>(plugin).extensions;

        if ext_id == CLAP_EXT_PARAMS {
            return (&raw const extensions.params).cast::<c_void>();
        }
        if ext_id == CLAP_EXT_REMOTE_CONTROLS || ext_id == CLAP_EXT_REMOTE_CONTROLS_COMPAT {
            return (&raw const extensions.remote_controls).cast::<c_void>();
        }
        if ext_id == CLAP_EXT_STATE {
            return (&raw const extensions.state).cast::<c_void>();
        }
        if ext_id == CLAP_EXT_PRESET_LOAD || ext_id == CLAP_EXT_PRESET_LOAD_COMPAT {
            return (&raw const extensions.preset_load).cast::<c_void>();
        }
        if ext_id == CLAP_EXT_AUDIO_PORTS {
            return (&raw const extensions.audio_ports).cast::<c_void>();
        }
        // Only advertise config switching when there's more than one
        // layout to switch between; a single-layout plugin is fully
        // described by `audio_ports` alone.
        if ext_id == CLAP_EXT_AUDIO_PORTS_CONFIG && P::bus_layouts().len() > 1 {
            return (&raw const extensions.audio_ports_config).cast::<c_void>();
        }
        if ext_id == CLAP_EXT_NOTE_PORTS {
            return (&raw const extensions.note_ports).cast::<c_void>();
        }
        if ext_id == CLAP_EXT_GUI {
            return (&raw const extensions.gui).cast::<c_void>();
        }
        if ext_id == CLAP_EXT_LATENCY {
            return (&raw const extensions.latency).cast::<c_void>();
        }
        if ext_id == CLAP_EXT_TAIL {
            return (&raw const extensions.tail).cast::<c_void>();
        }
        if ext_id == CLAP_EXT_RENDER {
            return (&raw const extensions.render).cast::<c_void>();
        }
        ptr::null()
    }
}

// ---------------------------------------------------------------------------
// Factory: create_plugin
// ---------------------------------------------------------------------------

/// Create a `clap_plugin` instance for the given plugin type.
///
/// # Safety
/// Called by the host through the factory. The descriptor must remain valid
/// for the lifetime of the returned plugin.
#[must_use]
pub unsafe fn create_plugin_instance<P: PluginExport>(
    descriptor: *const clap_plugin_descriptor,
    host: *const clap_host,
) -> *const clap_plugin {
    // Author `create` runs here (its `init` is guarded separately in
    // `clap_plugin_init`). Guard the constructor too - the factory's
    // extern "C" caller is bare, so a panic would abort the host. A null
    // return tells the host construction failed.
    run_extern_callback_with::<P, *const clap_plugin>("CLAP", "create", ptr::null(), || {
        let layouts = P::bus_layouts();
        if !bus_layouts_fit_routing(&layouts) {
            eprintln!(
                "[moose CLAP] {} declares an audio-bus topology beyond BusRouting's limit of 32 \
                 buses per direction and 65,535 channels per bus - instance creation refused.",
                std::any::type_name::<P>(),
            );
            return ptr::null();
        }
        let instance = P::create();
        let info = P::info();
        let plugin_id_hash = state::hash_plugin_id(info.clap_id);
        let param_infos = instance.params().param_infos();
        let param_count = param_infos.len();
        let params_arc = instance.params_arc();
        let exposed_param_indices = Mutex::new(parameter_indices(&param_infos, |id| {
            params_arc.parameter_presentation(id)
        }));
        let meter_store = instance.meter_store();
        let snapshot = instance.snapshot_slot();
        let task_spawner = instance.task_spawner();
        let editor_builder = instance.editor_builder();
        let latency_cache = AtomicU32::new(instance.latency());
        let tail_cache = AtomicU32::new(instance.tail());

        // Pre-size the per-block channel-slice scratch from the worst-case
        // bus layout the plugin advertises. Without this, the first
        // `clap_plugin_process` call after activate hits the global
        // allocator on the audio thread for every channel push; this
        // amortizes the cost into instance creation, where it belongs.
        // Read before `info` is moved into the struct literal below.
        let midi_input_ports = info.midi_input_ports;
        let max_in = layouts
            .iter()
            .map(|l| l.total_input_channels() as usize)
            .max()
            .unwrap_or(0);
        let max_out = layouts
            .iter()
            .map(|l| l.total_output_channels() as usize)
            .max()
            .unwrap_or(0);

        let data = Box::new(ClapPluginData::<P> {
            plugin: shared_plugin(instance),
            params_arc,
            meter_store,
            snapshot,
            task_spawner,
            editor_builder,
            latency_cache,
            tail_cache,
            selected_config: AtomicU32::new(0),
            param_infos,
            exposed_param_indices,
            parameter_restart_pending: AtomicBool::new(false),
            info,
            plugin_id_hash,
            host,
            host_params: ptr::null(),
            host_latency: ptr::null(),
            host_tail: ptr::null(),
            host_audio_ports: ptr::null(),
            port_names_revision: AtomicU64::new(0),
            port_names_dirty: AtomicBool::new(false),
            latency_dirty: AtomicBool::new(false),
            gui_changes: Arc::new(GuiChangeQueue::new(GUI_QUEUE_CAPACITY)),
            pending_state: Arc::new(StateLoadQueue::new(1)),
            active: AtomicBool::new(false),
            render_mode: AtomicU8::new(ProcessMode::Realtime.as_u8()),
            needs_rescan: Arc::new(AtomicBool::new(false)),
            presentation_revision: AtomicU64::new(0),
            transport_slot: TransportSlot::new(),
            host_scale: AtomicU64::new(0),
            window_scale: AtomicU64::new(0),
            pending_resize: AtomicU64::new(0),
            extensions: Extensions::<P>::new(),
            audio: PluginCell::new(ClapAudio {
                event_list: EventList::with_capacity(EVENT_LIST_PREALLOC + param_count),
                sounding_notes: SoundingNotes::new(midi_input_ports),
                output_events: EventList::with_capacity(EVENT_LIST_PREALLOC),
                sub_event_scratch: EventList::with_capacity(EVENT_LIST_PREALLOC + param_count),
                deferred_params: Vec::with_capacity(param_count),
                sample_rate: 44100.0,
                max_block_size: 1024,
                input_slices: Vec::with_capacity(max_in),
                output_slices: Vec::with_capacity(max_out),
                input_widen: Vec::with_capacity(max_in),
                output_narrow: Vec::with_capacity(max_out),
                host_out_ptrs: Vec::with_capacity(max_out),
                bus_routing: BusRouting::new(),
            }),
            gui: PluginCell::new(ClapGui {
                editor: None,
                gui_created: false,
            }),
        });

        let clap = Box::new(clap_plugin {
            desc: descriptor,
            plugin_data: Box::into_raw(data).cast::<c_void>(),
            init: Some(clap_plugin_init::<P>),
            destroy: Some(clap_plugin_destroy::<P>),
            activate: Some(clap_plugin_activate::<P>),
            deactivate: Some(clap_plugin_deactivate::<P>),
            start_processing: Some(clap_plugin_start_processing::<P>),
            stop_processing: Some(clap_plugin_stop_processing::<P>),
            reset: Some(clap_plugin_reset::<P>),
            process: Some(clap_plugin_process::<P>),
            get_extension: Some(clap_plugin_get_extension::<P>),
            on_main_thread: Some(clap_plugin_on_main_thread::<P>),
        });

        Box::into_raw(clap).cast_const()
    })
}

// ---------------------------------------------------------------------------
// export_clap! macro
// ---------------------------------------------------------------------------

/// Export a CLAP plugin entry point.
///
/// Usage:
/// ```ignore
/// export_clap!(MyPlugin);
/// ```
///
/// Where `MyPlugin` implements `PluginExport`.
#[macro_export]
macro_rules! export_clap {
    ($plugin_type:ty) => {
        mod _clap_entry {
            use super::*;
            use std::ffi::{CStr, c_char, c_void};
            use std::path::Path;
            use std::ptr;
            use std::sync::OnceLock;

            use ::clap_sys::entry::clap_plugin_entry;
            use ::clap_sys::factory::plugin_factory::{
                CLAP_PLUGIN_FACTORY_ID, clap_plugin_factory,
            };
            use ::clap_sys::factory::preset_discovery::{
                CLAP_PRESET_DISCOVERY_FACTORY_ID, CLAP_PRESET_DISCOVERY_FACTORY_ID_COMPAT,
            };
            use ::clap_sys::host::clap_host;
            use ::clap_sys::plugin::{clap_plugin, clap_plugin_descriptor};
            use ::clap_sys::version::CLAP_VERSION;

            use ::moose_clap::__macro_deps::moose_core::plugin::PluginRuntime;
            use ::moose_clap::DescriptorHolder;

            static DESCRIPTOR: OnceLock<DescriptorHolder> = OnceLock::new();

            fn get_descriptor() -> &'static DescriptorHolder {
                DESCRIPTOR.get_or_init(|| {
                    let info = <$plugin_type as PluginRuntime>::info();
                    DescriptorHolder::new(&info)
                })
            }

            static FACTORY: clap_plugin_factory = clap_plugin_factory {
                get_plugin_count: Some(factory_get_plugin_count),
                get_plugin_descriptor: Some(factory_get_plugin_descriptor),
                create_plugin: Some(factory_create_plugin),
            };

            unsafe extern "C" fn factory_get_plugin_count(
                _factory: *const clap_plugin_factory,
            ) -> u32 {
                1
            }

            unsafe extern "C" fn factory_get_plugin_descriptor(
                _factory: *const clap_plugin_factory,
                index: u32,
            ) -> *const clap_plugin_descriptor {
                if index == 0 {
                    &get_descriptor().descriptor as *const clap_plugin_descriptor
                } else {
                    ptr::null()
                }
            }

            unsafe extern "C" fn factory_create_plugin(
                _factory: *const clap_plugin_factory,
                host: *const clap_host,
                plugin_id: *const c_char,
            ) -> *const clap_plugin {
                if plugin_id.is_null() {
                    return ptr::null();
                }
                let requested_id = CStr::from_ptr(plugin_id);
                let info = <$plugin_type as PluginRuntime>::info();
                let our_id = match std::ffi::CString::new(info.clap_id) {
                    Ok(s) => s,
                    Err(_) => return ptr::null(),
                };
                if requested_id != our_id.as_c_str() {
                    return ptr::null();
                }
                let descriptor = &get_descriptor().descriptor as *const clap_plugin_descriptor;
                ::moose_clap::create_plugin_instance::<$plugin_type>(descriptor, host)
            }

            unsafe extern "C" fn entry_init(plugin_path: *const c_char) -> bool {
                // Force descriptor initialization.
                let _ = get_descriptor();
                // Remember where the host loaded us from - the preset
                // discovery factory derives the factory-preset
                // location from it.
                ::moose_clap::presets::set_plugin_path(plugin_path);
                true
            }

            unsafe extern "C" fn entry_deinit() {}

            unsafe extern "C" fn entry_get_factory(factory_id: *const c_char) -> *const c_void {
                if factory_id.is_null() {
                    return ptr::null();
                }
                let id = CStr::from_ptr(factory_id);
                if id == CLAP_PLUGIN_FACTORY_ID {
                    return &FACTORY as *const clap_plugin_factory as *const c_void;
                }
                if id == CLAP_PRESET_DISCOVERY_FACTORY_ID
                    || id == CLAP_PRESET_DISCOVERY_FACTORY_ID_COMPAT
                {
                    return ::moose_clap::presets::discovery_factory::<$plugin_type>()
                        as *const c_void;
                }
                ptr::null()
            }

            #[unsafe(no_mangle)]
            #[allow(non_upper_case_globals)]
            pub static clap_entry: clap_plugin_entry = clap_plugin_entry {
                clap_version: CLAP_VERSION,
                init: Some(entry_init),
                deinit: Some(entry_deinit),
                get_factory: Some(entry_get_factory),
            };
        }
    };
}

#[cfg(test)]
mod gui_parent_tests {
    use super::*;
    use clap_sys::ext::gui::clap_window_handle;

    #[test]
    fn parent_api_is_validated_before_native_union_interpretation() {
        #[cfg(target_os = "macos")]
        let native = CLAP_WINDOW_API_COCOA;
        #[cfg(target_os = "windows")]
        let native = CLAP_WINDOW_API_WIN32;
        #[cfg(target_os = "linux")]
        let native = CLAP_WINDOW_API_X11;
        let mut token = 0u8;
        let parent = (&mut token as *mut u8).cast::<c_void>();
        let mut window = clap_window {
            api: native.as_ptr(),
            specific: clap_window_handle { ptr: parent },
        };
        // No native window or renderer is created: the exact parser used by
        // set_parent must return the unchanged host-owned pointer only for
        // the current platform's declared representation.
        unsafe {
            assert!(gui_api_supported(native.as_ptr(), false));
            assert!(!gui_api_supported(native.as_ptr(), true));
            assert_eq!(gui_parent_pointer(&window), Some(parent));
            for api in [c"cocoa", c"win32", c"x11", c"wayland", c"unknown", c"COCOA"] {
                window.api = api.as_ptr();
                assert_eq!(
                    gui_parent_pointer(&window),
                    (api == native).then_some(parent)
                );
            }
            window.api = ptr::null();
            assert_eq!(gui_parent_pointer(&window), None);
            assert!(!gui_api_supported(ptr::null(), false));
            window.api = native.as_ptr();
            window.specific = clap_window_handle {
                ptr: ptr::null_mut(),
            };
            assert_eq!(gui_parent_pointer(&window), None);
            assert_eq!(gui_parent_pointer(ptr::null()), None);
        }
    }
}

#[cfg(test)]
mod transport_tests {
    use super::{
        CLAP_SECTIME_FACTOR, CLAP_TRANSPORT_HAS_SECONDS_TIMELINE, build_transport_info,
        clap_event_transport,
    };

    #[test]
    fn position_samples_derived_from_seconds_timeline() {
        // SAFETY: clap_event_transport is a repr(C) POD; all-zero is a
        // valid value (no flags set, all positions 0).
        let mut t: clap_event_transport = unsafe { std::mem::zeroed() };
        t.flags = CLAP_TRANSPORT_HAS_SECONDS_TIMELINE;
        // 1.5 seconds, in CLAP's power-of-two fixed point (no float cast).
        t.song_pos_seconds = CLAP_SECTIME_FACTOR + CLAP_SECTIME_FACTOR / 2;
        let info = build_transport_info(&t, 48_000.0);
        assert_eq!(info.position_samples, 72_000); // 1.5 s * 48 kHz
        assert!((info.position_seconds - 1.5).abs() < 1e-9);
    }

    #[test]
    fn position_samples_zero_without_seconds_timeline() {
        // SAFETY: see above - zeroed POD, no seconds-timeline flag.
        let t: clap_event_transport = unsafe { std::mem::zeroed() };
        assert_eq!(build_transport_info(&t, 48_000.0).position_samples, 0);
    }
}

#[cfg(test)]
mod channel_slice_tests {
    use super::{input_channel_slice, output_channel_slice};

    /// An unconnected CLAP input port (null channel pointer) reaches the
    /// plugin as block-length silence, never the empty slice it would read
    /// out of range.
    #[test]
    fn null_input_channel_reads_block_length_silence() {
        let nf = 256usize;
        let mut scratch = Vec::<f32>::new();
        // SAFETY: the null pointer is the unconnected-port case under test;
        // `scratch` outlives the returned slice within this call.
        let slice = unsafe { input_channel_slice::<f32, f32>(std::ptr::null(), nf, &mut scratch) };
        assert_eq!(slice.len(), nf);
        assert!(slice.iter().all(|&s| s == 0.0));
    }

    /// A null output channel is a block-length discard buffer the plugin
    /// can write without an out-of-range panic.
    #[test]
    fn null_output_channel_is_block_length_discard() {
        let nf = 256usize;
        let mut scratch = Vec::<f32>::new();
        // SAFETY: the null pointer is the unconnected-port case under test.
        let slice =
            unsafe { output_channel_slice::<f32, f32>(std::ptr::null_mut(), nf, &mut scratch) };
        assert_eq!(slice.len(), nf);
        for s in slice.iter_mut() {
            *s = 1.0;
        }
    }
}

#[cfg(test)]
mod note_expression_tests {
    use super::{
        CLAP_NOTE_EXPRESSION_TUNING, CLAP_NOTE_EXPRESSION_VOLUME, clap_event_note_expression,
        clap_note_expression_of, note_expression_body,
    };
    use moose_core::events::EventBody;

    fn expression(id: i32, value: f64) -> clap_event_note_expression {
        // SAFETY: clap_event_note_expression is a repr(C) POD; all-zero
        // is valid (channel 0, key 0).
        let mut ne: clap_event_note_expression = unsafe { std::mem::zeroed() };
        ne.expression_id = id;
        ne.value = value;
        ne
    }

    #[test]
    fn volume_crosses_in_the_gain_domain() {
        // CLAP `VOLUME` is plain linear gain `0..=4`; wire full-scale
        // must land on gain 4 (+12 dB), unity on the quarter point.
        let (id, _, _, value) = clap_note_expression_of(&EventBody::PerNoteCC {
            group: 0,
            channel: 0,
            note: 60,
            cc: 7,
            value: u32::MAX,
            registered: true,
        })
        .expect("volume maps");
        assert_eq!(id, CLAP_NOTE_EXPRESSION_VOLUME);
        assert!((value - 4.0).abs() < 1e-9);

        // Host unity gain decodes to the wire's quarter point.
        let body = note_expression_body(&expression(CLAP_NOTE_EXPRESSION_VOLUME, 1.0), 0, 60)
            .expect("volume decodes");
        let EventBody::PerNoteCC { cc: 7, value, .. } = body else {
            panic!("expected volume PerNoteCC, got {body:?}");
        };
        assert!((f64::from(value) / f64::from(u32::MAX) - 0.25).abs() < 1e-9);
    }

    #[test]
    fn assignable_per_note_cc_is_not_an_expression() {
        // Only registered per-note indices carry the predefined
        // expression semantics; an assignable index 7 is not volume.
        assert!(
            clap_note_expression_of(&EventBody::PerNoteCC {
                group: 0,
                channel: 0,
                note: 60,
                cc: 7,
                value: u32::MAX,
                registered: false,
            })
            .is_none()
        );
    }

    #[test]
    fn tuning_full_scale_is_48_semitones() {
        let (_, _, _, semis) = clap_note_expression_of(&EventBody::PerNotePitchBend {
            group: 0,
            channel: 0,
            note: 60,
            value: u32::MAX,
        })
        .expect("tuning maps");
        assert!((semis - 48.0).abs() < 1e-6);

        // A host bend beyond the wire's range saturates.
        let body = note_expression_body(&expression(CLAP_NOTE_EXPRESSION_TUNING, 120.0), 0, 60)
            .expect("tuning decodes");
        assert!(matches!(
            body,
            EventBody::PerNotePitchBend {
                value: u32::MAX,
                ..
            }
        ));
    }
}

#[cfg(test)]
mod sounding_notes_tests {
    use super::{NoteAxis, SoundingNotes};

    #[test]
    fn wildcard_axis_parses_all_concrete_and_junk() {
        assert_eq!(NoteAxis::parse(-1, 16), NoteAxis::All); // wildcard
        assert_eq!(NoteAxis::parse(3, 16), NoteAxis::One(3)); // concrete
        assert_eq!(NoteAxis::parse(16, 16), NoteAxis::Invalid); // out of domain
        assert_eq!(NoteAxis::parse(-2, 128), NoteAxis::Invalid); // hostile
    }

    #[test]
    fn drain_matches_only_the_requested_axes() {
        let mut s = SoundingNotes::new(2);
        s.set(0, 0, 60);
        s.set(0, 3, 60);
        s.set(0, 3, 64);
        s.set(1, 3, 60); // other port: never touched below

        // Key wildcard, concrete channel: both channel-3 notes drain.
        let mut hits = Vec::new();
        s.drain_matching(0, Some(3), None, |ch, k| hits.push((ch, k)));
        assert_eq!(hits, [(3, 60), (3, 64)]);

        // Drained notes are cleared - a second wildcard finds nothing.
        hits.clear();
        s.drain_matching(0, Some(3), None, |ch, k| hits.push((ch, k)));
        assert!(hits.is_empty());

        // Channel wildcard, concrete key.
        s.set(0, 5, 60);
        hits.clear();
        s.drain_matching(0, None, Some(60), |ch, k| hits.push((ch, k)));
        assert_eq!(hits, [(0, 60), (5, 60)]);

        // Both wildcard: everything left on the port.
        s.set(0, 9, 1);
        hits.clear();
        s.drain_matching(0, None, None, |ch, k| hits.push((ch, k)));
        assert_eq!(hits, [(9, 1)]);

        // The other port's note survived it all.
        hits.clear();
        s.drain_matching(1, None, None, |ch, k| hits.push((ch, k)));
        assert_eq!(hits, [(3, 60)]);
    }

    #[test]
    fn concrete_off_clears_the_bit() {
        let mut s = SoundingNotes::new(1);
        s.set(0, 0, 60);
        s.clear(0, 0, 60);
        let mut hits = Vec::new();
        s.drain_matching(0, None, None, |ch, k| hits.push((ch, k)));
        assert!(hits.is_empty());
    }

    #[test]
    fn for_each_leaves_notes_sounding() {
        // Expression fan-out addresses voices without releasing them:
        // the same wildcard visit must keep finding the note.
        let mut s = SoundingNotes::new(1);
        s.set(0, 2, 60);
        s.set(0, 2, 64);
        for _ in 0..2 {
            let mut hits = Vec::new();
            s.for_each_matching(0, Some(2), None, |ch, k| hits.push((ch, k)));
            assert_eq!(hits, [(2, 60), (2, 64)]);
        }
    }
}

#[cfg(test)]
mod note_address_tests {
    use super::{clap_event_note, clap_note_address};

    fn note(channel: i16, key: i16) -> clap_event_note {
        // SAFETY: clap_event_note is a repr(C) POD; all-zero is valid.
        let mut ne: clap_event_note = unsafe { std::mem::zeroed() };
        ne.channel = channel;
        ne.key = key;
        ne
    }

    #[test]
    fn concrete_address_resolves() {
        assert_eq!(clap_note_address(&note(15, 127)), Some((15, 127)));
        assert_eq!(clap_note_address(&note(0, 0)), Some((0, 0)));
    }

    #[test]
    fn wildcard_and_out_of_range_are_dropped() {
        // `-1` is CLAP's "match all" wildcard; anything past the MIDI
        // domain is a hostile/buggy host. Neither may reach a plugin
        // that indexes tables by note/channel.
        assert_eq!(clap_note_address(&note(-1, 60)), None);
        assert_eq!(clap_note_address(&note(0, -1)), None);
        assert_eq!(clap_note_address(&note(16, 60)), None);
        assert_eq!(clap_note_address(&note(0, 128)), None);
    }
}

#[cfg(test)]
mod remote_controls_tests {
    use super::{
        ParamFlags, ParamInfo, ParamRange, parameter_indices, remote_control_pages,
        remote_controls_page_id, split_group,
    };
    use moose_params::{ParamUnit, ParamValueKind, ParameterPresentation};

    fn info(id: u32, group: &'static str) -> ParamInfo {
        ParamInfo {
            id,
            name: "p",
            short_name: "p",
            group,
            range: ParamRange::Linear { min: 0.0, max: 1.0 },
            default_plain: 0.0,
            flags: ParamFlags::empty(),
            unit: ParamUnit::None,
            kind: ParamValueKind::Float,
            midi_map: None,
            midi_channel: None,
        }
    }

    #[test]
    fn ungrouped_params_produce_no_page() {
        let infos = vec![info(0, ""), info(1, "")];
        assert!(remote_control_pages(&infos).is_empty());
    }

    #[test]
    fn unavailable_params_leave_stable_ids_out_of_clap_enumeration() {
        let infos = [info(10, "EQ"), info(11, "EQ"), info(12, "EQ")];
        let indices = parameter_indices(&infos, |id| {
            (id == 11).then(|| ParameterPresentation {
                available: false,
                ..ParameterPresentation::default()
            })
        });
        assert_eq!(indices, [0, 2]);
        assert_eq!(
            remote_control_pages(indices.iter().map(|&i| &infos[i])),
            [("EQ", vec![10, 12])]
        );
    }

    #[test]
    fn hidden_and_readonly_grouped_params_take_no_slot() {
        // A grouped-but-hidden / read-only param organizes the param
        // tree but isn't a performance control, so it must not burn one
        // of a page's eight slots.
        let mut hidden = info(0, "EQ");
        hidden.flags = ParamFlags::HIDDEN;
        let visible = info(1, "EQ");
        let mut readonly = info(2, "EQ");
        readonly.flags = ParamFlags::READONLY;
        let infos = [hidden, visible, readonly];
        let pages = remote_control_pages(&infos);
        assert_eq!(pages, vec![("EQ", vec![1])]);
    }

    #[test]
    fn group_over_eight_params_splits_into_two_pages() {
        let infos: Vec<ParamInfo> = (0..10).map(|id| info(id, "EQ")).collect();
        let pages = remote_control_pages(&infos);
        assert_eq!(pages.len(), 2);
        assert_eq!(pages[0].1.len(), 8);
        assert_eq!(pages[1].1.len(), 2);
        assert_eq!(pages[1].1, vec![8, 9]);
    }

    #[test]
    fn distinct_groups_get_distinct_pages() {
        let mut infos: Vec<ParamInfo> = (0..3).map(|id| info(id, "EQ")).collect();
        infos.extend((3..5).map(|id| info(id, "DYN")));
        let pages = remote_control_pages(&infos);
        assert_eq!(pages, vec![("EQ", vec![0, 1, 2]), ("DYN", vec![3, 4])]);
    }

    #[test]
    fn split_group_separates_section_from_page() {
        assert_eq!(split_group("EQ/Lo Shelf"), ("EQ", "Lo Shelf"));
        // Only the first '/' is a separator; a page name may itself
        // contain one without gaining a third level.
        assert_eq!(split_group("EQ/Lo Shelf/Extra"), ("EQ", "Lo Shelf/Extra"));
    }

    #[test]
    fn split_group_without_slash_is_its_own_section() {
        assert_eq!(split_group("Compressor"), ("Compressor", "Compressor"));
    }

    #[test]
    fn page_id_is_stable_and_distinguishes_chunks() {
        // Deterministic: identical inputs hash identically (a host
        // persists this across processes).
        assert_eq!(
            remote_controls_page_id("EQ", 0),
            remote_controls_page_id("EQ", 0)
        );
        // The two pages of a >8-param group get distinct ids.
        assert_ne!(
            remote_controls_page_id("EQ", 0),
            remote_controls_page_id("EQ", 1)
        );
        // Distinct group names get distinct ids.
        assert_ne!(
            remote_controls_page_id("EQ", 0),
            remote_controls_page_id("DYN", 0)
        );
    }
}

#[cfg(test)]
mod host_state_tests {
    use super::{MAX_HOST_STATE_BYTES, read_host_state};

    #[test]
    fn reads_until_end_of_stream() {
        let mut chunks = vec![3_i64, 2, 0].into_iter();
        let blob = read_host_state(|buf| {
            let n = chunks.next().unwrap();
            buf[..usize::try_from(n).unwrap()].fill(7);
            n
        });
        assert_eq!(blob, Some(vec![7; 5]));
    }

    #[test]
    fn rejects_read_errors_and_overlong_counts() {
        assert_eq!(read_host_state(|_| -1), None);
        // A host claiming more bytes than the buffer holds is lying.
        assert_eq!(
            read_host_state(|buf| i64::try_from(buf.len()).unwrap() + 1),
            None
        );
    }

    #[test]
    fn rejects_streams_past_the_cap() {
        let mut total = 0usize;
        let blob = read_host_state(|buf| {
            total += buf.len();
            assert!(total <= MAX_HOST_STATE_BYTES + buf.len(), "kept reading");
            i64::try_from(buf.len()).unwrap()
        });
        assert_eq!(blob, None);
    }
}

#[cfg(test)]
mod deferred_param_tests {
    use super::{Event, EventBody, EventList, defer_param_change, replay_deferred_params};

    fn change(offset: u32, id: u32, value: f64) -> Event {
        Event::new(offset, EventBody::ParamChange { id, value })
    }

    #[test]
    fn flushed_changes_are_latest_wins_and_bounded() {
        let mut deferred = Vec::with_capacity(2);
        defer_param_change(&mut deferred, 7, 0.2);
        defer_param_change(&mut deferred, 7, 0.6);
        defer_param_change(&mut deferred, 8, 0.1);
        defer_param_change(&mut deferred, 9, 0.9); // over capacity: dropped
        assert_eq!(deferred, vec![(7, 0.6), (8, 0.1)]);
    }

    #[test]
    fn replayed_changes_precede_the_block_and_sort_stays_stable() {
        let mut deferred = vec![(7, 0.6)];
        let mut list = EventList::with_capacity(8);
        replay_deferred_params(&mut list, &mut deferred, false);
        list.push(change(32, 7, 0.7));
        list.push(change(0, 8, 0.1));
        list.ensure_sorted_by_offset();
        let seen: Vec<_> = list
            .iter()
            .map(|e| match e.body {
                EventBody::ParamChange { id, value } => (e.sample_offset, id, value),
                _ => unreachable!(),
            })
            .collect();
        assert_eq!(seen, vec![(0, 7, 0.6), (0, 8, 0.1), (32, 7, 0.7)]);
        assert!(deferred.is_empty());
    }

    #[test]
    fn state_load_discards_deferred_changes() {
        let mut deferred = vec![(7, 0.6)];
        let mut list = EventList::with_capacity(8);
        replay_deferred_params(&mut list, &mut deferred, true);
        assert_eq!(list.iter().count(), 0);
        assert!(deferred.is_empty());
    }
}

#[cfg(test)]
mod bus_kind_tests {
    use super::{BusKind, CLAP_AUDIO_PORT_IS_MAIN, audio_port_main_flag};

    #[test]
    fn only_a_main_bus_at_index_zero_is_clap_main() {
        assert_eq!(
            audio_port_main_flag(0, BusKind::Main),
            CLAP_AUDIO_PORT_IS_MAIN
        );
        // Main bus disabled: the first enabled port is the sidechain.
        assert_eq!(audio_port_main_flag(0, BusKind::Sidechain), 0);
        assert_eq!(audio_port_main_flag(1, BusKind::Sidechain), 0);
    }
}
