//! VST3 format wrapper for moose.
//!
//! Uses a C++ shim that implements the real VST3 COM interfaces
//! with correct vtable layout. All plugin logic is delegated to
//! Rust via C FFI callbacks.

pub mod ffi;

use std::collections::HashSet;
use std::ffi::{CStr, CString};
use std::os::raw::c_char;
use std::slice;

use moose_core::TransportSlot;
use moose_core::buffer::RawBufferScratch;
use moose_core::bus::{BusConfig, BusKind, BusLayout};
use moose_core::bus_routing::{
    BusActivation, BusRouting, MAX_AUDIO_BUSES, bus_layouts_fit_routing,
};
use moose_core::cast::{len_u32, sample_pos_i64};
use moose_core::chunked_process::{ChunkedProcess, process_chunked_with_bus_routing};
use moose_core::config::{AudioConfig, ProcessMode};
use moose_core::editor::EditorBuilder;
use moose_core::editor::{
    ClosureBridge, Editor, PluginContext, RawWindowHandle, SendPtr, clamp_logical_size,
    fit_logical_size,
};
use moose_core::events::{
    EVENT_LIST_PREALLOC, Event, EventBody, EventList, ExactAddress, ExactEvent, ExactEventBody,
    ExactEventMetadata, ExactEventQualifiers, ExactNoteAddress, ExactNoteKind, LosslessEventCursor,
    LosslessEventRef, OutputEventStatus, PushError, TransportInfo, Vst3EventMetadata,
};
use moose_core::export::PluginExport;
use moose_core::info::{PluginCategory, PluginInfo, resolve_name_override};
use moose_core::meters::MeterStore;
use moose_core::midi::{
    denorm_7bit, denorm_pitch_bend, per_note_bend_semitones, pitch_bend_to_bytes,
};
#[cfg(test)]
use moose_core::midi::{downconvert_to_midi1, per_note_bend_from_semitones};
use moose_core::plugin::PluginRuntime;
use moose_core::rt::{RtSection, audit};
use moose_core::snapshot::SnapshotSlot;
use moose_core::state;
use moose_core::tasks::AnyTaskSpawner;
use moose_core::wrapper::{
    ParamCStrings, PluginCell, SharedPlugin, copy_c_str, default_io_channels, enter_plugin,
    find_bus_layout, log_missing_bus_layout, run_audio_block, run_extern_callback_with,
    run_register, save_extra, shared_plugin,
};
use moose_params::MidiSource;
use moose_params::sample::{Float, Sample};
use moose_params::{ParamFlags, ParamInfo, ParamRange, Params};

use ffi::{Vst3Callbacks, Vst3NativeEvent, Vst3ParamDescriptor, Vst3PluginDescriptor};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};

// ---------------------------------------------------------------------------
// Instance wrapper
// ---------------------------------------------------------------------------

/// Bounded handoff slot for state loads. Capacity 1: presets don't
/// arrive faster than the audio thread completes a block, and on
/// overflow we want most-recent-wins (`force_push`) so a rapid
/// double-recall doesn't get the audio thread to apply a stale state
/// after the host already moved on.
type StateLoadQueue = crossbeam_queue::ArrayQueue<state::DeserializedState>;

/// VST3 `RestartFlags::kLatencyChanged`. The audio thread flags this
/// through `moose_vst3_mark_restart` (an atomic bit-set, no host call);
/// the shim drains it via `IComponentHandler::restartComponent` on the
/// next host main-thread callback, so the actual notification lands on
/// the UI thread - never the render thread or a moose-owned thread.
const K_LATENCY_CHANGED: i32 = 8;

struct Vst3Instance<P: PluginExport> {
    /// The plugin in the wrapper-standard ownership cell: the audio
    /// thread owns it per block, host lifecycle callbacks own it while
    /// processing is stopped, and the two never overlap. `cb_state_save`
    /// and the editor's `get_state` read the lock-free snapshot instead,
    /// so they never touch it. See `moose_core::wrapper::SharedPlugin`.
    plugin: SharedPlugin<P>,
    /// Stable handle to the params Arc, set once at instance creation.
    /// Host-thread callbacks (`cb_param_*`) read params through this
    /// handle so a param query never touches the plugin.
    /// Params are atomic-backed and `Sync`.
    params_arc: Arc<P::Params>,
    /// Shared meter storage, set once at instance creation. The
    /// editor's `get_meter` closure reads these atomic slots instead
    /// of the plugin instance.
    meter_store: Arc<MeterStore>,
    /// Lock-free custom-state slot the audio thread publishes
    /// into, read by `save_state` so a snapshot-capable plugin's
    /// save never touches the plugin. Cached on the instance.
    snapshot: Arc<SnapshotSlot>,
    /// Background-task spawner (`None` unless the plugin wired `tasks:`),
    /// cached at creation so the editor schedules without touching the plugin.
    task_spawner: Option<AnyTaskSpawner>,
    /// Lock-free editor factory, cached at creation - building
    /// the editor never touches the plugin (`--shell` rebuilds
    /// from the reloaded dylib, so GUI edits hot-reload).
    editor_builder: EditorBuilder<P::Params>,
    /// Full param-info cache for the chunker's `is_chunked(id)`
    /// lookup. Built once at `cb_create`; static for the life of
    /// the instance.
    param_infos: Vec<ParamInfo>,
    /// `min_subblock_samples` from `moose.toml`'s `[automation]`
    /// table. Read at instance construction and passed to
    /// `chunked_process::process_chunked` every block.
    min_subblock_samples: u32,
    plugin_id_hash: u64,
    /// `true` between `setActive(true)` and `setActive(false)`.
    /// `cb_state_load` and `cb_state_save` read it to decide whether the
    /// audio thread will drain `pending_state`: if inactive, no
    /// `cb_process` runs, so the host thread applies the custom-state
    /// blob synchronously rather than leaving it stranded (which would
    /// let a following `getState` re-serialize stale extra state).
    /// Written only from `cb_set_active` (main thread); unlike
    /// `prepared`, it tracks deactivation too.
    active: AtomicBool,
    /// Cached `(id, range)` pairs sorted by id. Built once in
    /// `cb_create` from `params().param_infos()`. Hosts call
    /// `cb_param_normalize` / `cb_param_denormalize` extremely often
    /// while reading automation; rebuilding the full `Vec<ParamInfo>`
    /// per call would heap-allocate on a tight host read path. Ranges
    /// are static for the life of the plugin instance, so caching is
    /// safe.
    param_ranges: Vec<(u32, ParamRange)>,
    /// Precomputed MIDI-controller bindings, sorted by param id, for the
    /// audio-thread bridge in `process_block`. Only params with a
    /// `midi_map` appear, so it's empty for the common no-mapping plugin
    /// and the per-change lookup short-circuits (`binary_search` on an
    /// empty slice is `O(1)`).
    midi_maps: Vec<(u32, MidiMap)>,
    /// Shared transport slot: audio thread writes each block, editor reads.
    transport_slot: Arc<TransportSlot>,
    /// Bounded SPSC handoff for state loads. Host (`cb_state_load`)
    /// and editor (`set_state` callback) deserialize on their thread
    /// and push the result; the audio thread pops at the top of
    /// `cb_process` and calls [`state::apply_state`]
    /// under its exclusive `&mut plugin`. While inactive no `cb_process`
    /// runs, so the host thread drains it in `cb_state_save` /
    /// `cb_set_active` - the editor pushes unconditionally rather than
    /// entering the cell from its own (possibly third) thread.
    pending_state: Arc<StateLoadQueue>,
    /// Atomic snapshots of the plugin's most recent `latency()` /
    /// `tail()` reports. Updated by the audio thread (or `cb_reset`)
    /// so host-thread callbacks (`cb_get_latency`, `cb_get_tail`) read
    /// the value without forming a `&Inst.plugin` reference. Initial
    /// value is whatever the plugin reports immediately after `init()`.
    latency_cache: AtomicU32,
    tail_cache: AtomicU32,
    /// Collision-free hidden MIDI proxy IDs in logical
    /// `(port, channel, controller)` order. Allocated deterministically
    /// from the top of the host-safe parameter domain while skipping
    /// every real parameter ID.
    midi_proxy_ids: Vec<u32>,
    /// Last-seen values of the hidden MIDI proxy params (f64 bits), in
    /// the same logical order as `midi_proxy_ids`. Empty when the plugin
    /// doesn't accept MIDI input. Written by `cb_param_set_value` and
    /// read by `cb_param_get_value` - both host-thread; atomic for
    /// interior mutability through the shared `&Inst` those callbacks
    /// hold.
    midi_proxy_values: Vec<AtomicU64>,
    /// Content scale from `setContentScaleFactor` (f64 bits, `0` until the
    /// host sends one; ignored on macOS); converts the editor's logical size
    /// to physical pixels for `getSize`.
    /// GUI-thread-only, but atomic so the host-thread GUI callbacks and the
    /// `request_resize` closure reach it through the shared `&Inst` without a
    /// `&mut *ctx` - and so it stays outside the `gui` cell, which the
    /// resize closure would otherwise re-enter while `cb_gui_open` holds it.
    host_scale: AtomicU64,
    /// The open editor's own window scale (`Editor::window_scale`, f64 bits,
    /// `0` = none), refreshed by the size callbacks. Converts sizes while the
    /// host never sent a content scale, so `getSize` matches the child window
    /// the editor created at the monitor DPI / `Xft.dpi`.
    window_scale: AtomicU64,
    /// Editor resize the plugin requested through `PluginContext::request_resize`
    /// that landed back in a GUI callback (`onSize` → `cb_gui_set_size`)
    /// synchronously while the `gui` cell was already held - the wrapper stashes
    /// the physical `(w << 32) | h` here (both non-zero, `0` = none) instead of
    /// re-entering the cell, and `cb_gui_get_size` applies it on the next size
    /// query. GUI thread only; atomic for interior mutability through `&Inst`.
    pending_resize: AtomicU64,
    /// Audio + lifecycle-owned per-block scratch. Behind a `PluginCell` so
    /// every callback reaches it through a shared `&Vst3Instance` - never a
    /// whole-struct `&mut *ctx`, which would alias a concurrent host-thread
    /// `&*ctx` (param reads, GUI) and is UB under the aliasing model. The
    /// VST3 host contract serializes its owners (process, reset, setup,
    /// activate, and the per-block sysex / output-event callbacks that run
    /// within the process cycle), so the cell is never held twice at once.
    audio: PluginCell<Vst3Scratch<P>>,
    /// Main/UI-thread-owned editor state, behind a `PluginCell` for the same
    /// reason. Its owners - the GUI callbacks and `cb_state_load`'s
    /// editor-notify - all run on the host main thread; `cb_process` never
    /// touches it, so it can't overlap the audio thread.
    gui: PluginCell<Vst3Gui>,
}

impl<P: PluginExport> Vst3Instance<P> {
    /// The scale between the host's physical pixels and the editor's logical
    /// size: the host's content scale, else the open editor's window scale,
    /// else `1.0`. Never both multiplied.
    fn host_scale(&self) -> f64 {
        self.reported_host_scale()
            .or_else(|| match self.window_scale.load(Ordering::Relaxed) {
                0 => None,
                bits => Some(f64::from_bits(bits)),
            })
            .unwrap_or(1.0)
    }

    /// The last content scale the host sent, if it ever did.
    fn reported_host_scale(&self) -> Option<f64> {
        match self.host_scale.load(Ordering::Relaxed) {
            0 => None,
            bits => Some(f64::from_bits(bits)),
        }
    }

    /// Cache `editor`'s window scale for [`Self::host_scale`].
    fn note_window_scale(&self, editor: &dyn Editor) {
        let bits = editor
            .window_scale()
            .filter(|s| s.is_finite() && *s > 0.0)
            .map_or(0, f64::to_bits);
        self.window_scale.store(bits, Ordering::Relaxed);
    }

    fn set_host_scale(&self, scale: f64) {
        self.host_scale.store(scale.to_bits(), Ordering::Relaxed);
    }
}

/// Audio + lifecycle-owned per-block scratch (see [`Vst3Instance::audio`]).
struct Vst3Scratch<P: PluginExport> {
    event_list: EventList,
    input_num_frames: u32,
    output_events: EventList,
    /// Per-sub-block scratch for `chunked_process::process_chunked`.
    sub_event_scratch: EventList,
    sample_rate: f64,
    /// Max block size declared by the host in `setupProcessing`; used to
    /// debug-assert `cb_process` never exceeds the sized block.
    max_block_size: usize,
    /// `true` once `cb_reset` ran (host `setActive(true)`); `cb_process`
    /// early-returns and zeros outputs until then.
    prepared: bool,
    /// Reused per-block scratch for `RawBufferScratch::build`, parameterized
    /// by `P::Sample` (widening path for `prelude64` plugins).
    scratch: RawBufferScratch<<P as PluginRuntime>::Sample>,
    output_cursor: LosslessEventCursor,
    output_preflight_status: u32,
    output_note_ids: OutputNoteIds,
    pending_output_mutation: PendingOutputMutation,
}

/// Main/UI-thread-owned editor state (see [`Vst3Instance::gui`]).
struct Vst3Gui {
    editor: Option<Box<dyn Editor>>,
}

const VST3_NOTE_ID_LOWER: i32 = -10_000;
const VST3_NOTE_ID_UPPER: i32 = -1_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum OutputSourceIdentity {
    Exact { bus: u16, note_id: i32 },
    Anonymous,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum OutputVoiceLifecycle {
    Active,
    Released,
}

#[derive(Clone, Copy, Debug)]
struct OutputNoteSlot {
    used: bool,
    source: OutputSourceIdentity,
    vst3_note_id: i32,
    bus: u8,
    channel: u8,
    pitch: u8,
    lifecycle: OutputVoiceLifecycle,
    age: u64,
}

impl OutputNoteSlot {
    const FREE: Self = Self {
        used: false,
        source: OutputSourceIdentity::Anonymous,
        vst3_note_id: VST3_NOTE_ID_LOWER,
        bus: 0,
        channel: 0,
        pitch: 0,
        lifecycle: OutputVoiceLifecycle::Released,
        age: 0,
    };
}

#[derive(Clone, Copy)]
struct OutputNoteIds {
    slots: [OutputNoteSlot; EVENT_LIST_PREALLOC],
    next_id: i32,
    next_age: u64,
}

#[derive(Clone, Copy, Debug)]
enum PendingOutputMutation {
    None,
    Insert {
        index: usize,
        slot: OutputNoteSlot,
        next_id: i32,
    },
    Release {
        index: usize,
    },
}

impl OutputNoteIds {
    fn new() -> Self {
        Self {
            slots: [OutputNoteSlot::FREE; EVENT_LIST_PREALLOC],
            next_id: VST3_NOTE_ID_LOWER,
            next_age: 0,
        }
    }

    fn clear(&mut self) {
        self.slots = [OutputNoteSlot::FREE; EVENT_LIST_PREALLOC];
        self.next_id = VST3_NOTE_ID_LOWER;
        self.next_age = 0;
    }

    fn propose_note_on(
        &self,
        source: OutputSourceIdentity,
        bus: u8,
        channel: u8,
        pitch: u8,
    ) -> Option<(i32, PendingOutputMutation)> {
        if source != OutputSourceIdentity::Anonymous
            && self.slots.iter().any(|slot| {
                slot.used && slot.source == source && slot.lifecycle == OutputVoiceLifecycle::Active
            })
        {
            return None;
        }

        let index = self
            .slots
            .iter()
            .position(|slot| {
                source != OutputSourceIdentity::Anonymous
                    && slot.used
                    && slot.source == source
                    && slot.lifecycle == OutputVoiceLifecycle::Released
            })
            .or_else(|| self.slots.iter().position(|slot| !slot.used))
            .or_else(|| {
                self.slots
                    .iter()
                    .enumerate()
                    .filter(|(_, slot)| slot.lifecycle == OutputVoiceLifecycle::Released)
                    .min_by_key(|(_, slot)| slot.age)
                    .map(|(index, _)| index)
            })?;
        let (vst3_note_id, next_id) = self.available_id(index)?;
        let slot = OutputNoteSlot {
            used: true,
            source,
            vst3_note_id,
            bus,
            channel,
            pitch,
            lifecycle: OutputVoiceLifecycle::Active,
            age: self.next_age,
        };
        Some((
            vst3_note_id,
            PendingOutputMutation::Insert {
                index,
                slot,
                next_id,
            },
        ))
    }

    fn propose_note_off(
        &self,
        source: OutputSourceIdentity,
        bus: u8,
        channel: u8,
        pitch: u8,
    ) -> Option<(i32, PendingOutputMutation)> {
        let index = if source == OutputSourceIdentity::Anonymous {
            self.unique_pck(bus, channel, pitch, true)?
        } else {
            self.slots.iter().position(|slot| {
                slot.used && slot.source == source && slot.lifecycle == OutputVoiceLifecycle::Active
            })?
        };
        Some((
            self.slots[index].vst3_note_id,
            PendingOutputMutation::Release { index },
        ))
    }

    fn note_id_for_expression(
        &self,
        source: OutputSourceIdentity,
        bus: u8,
        channel: u8,
        pitch: u8,
    ) -> Option<i32> {
        let index = if source == OutputSourceIdentity::Anonymous {
            self.unique_pck(bus, channel, pitch, false)?
        } else {
            self.slots
                .iter()
                .position(|slot| slot.used && slot.source == source)?
        };
        Some(self.slots[index].vst3_note_id)
    }

    fn unique_pck(&self, bus: u8, channel: u8, pitch: u8, active_only: bool) -> Option<usize> {
        let mut matches = self.slots.iter().enumerate().filter(|(_, slot)| {
            slot.used
                && slot.bus == bus
                && slot.channel == channel
                && slot.pitch == pitch
                && (!active_only || slot.lifecycle == OutputVoiceLifecycle::Active)
        });
        let (index, _) = matches.next()?;
        matches.next().is_none().then_some(index)
    }

    fn available_id(&self, replacing: usize) -> Option<(i32, i32)> {
        let span = i64::from(VST3_NOTE_ID_UPPER) - i64::from(VST3_NOTE_ID_LOWER) + 1;
        for offset in 0..span {
            let candidate = i64::from(self.next_id) + offset;
            let candidate = if candidate > i64::from(VST3_NOTE_ID_UPPER) {
                candidate - span
            } else {
                candidate
            };
            let candidate = i32::try_from(candidate).ok()?;
            if !self.slots.iter().enumerate().any(|(index, slot)| {
                index != replacing && slot.used && slot.vst3_note_id == candidate
            }) {
                let next_id = if candidate == VST3_NOTE_ID_UPPER {
                    VST3_NOTE_ID_LOWER
                } else {
                    candidate + 1
                };
                return Some((candidate, next_id));
            }
        }
        None
    }

    fn commit(&mut self, mutation: PendingOutputMutation) {
        match mutation {
            PendingOutputMutation::None => {}
            PendingOutputMutation::Insert {
                index,
                slot,
                next_id,
            } => {
                self.slots[index] = slot;
                self.next_id = next_id;
                self.next_age = self.next_age.wrapping_add(1);
            }
            PendingOutputMutation::Release { index } => {
                if let Some(slot) = self.slots.get_mut(index) {
                    slot.lifecycle = OutputVoiceLifecycle::Released;
                }
            }
        }
    }
}

#[cfg(test)]
#[repr(C)]
struct Vst3MidiEvent {
    sample_offset: u32,
    status: u8,
    data1: u8,
    data2: u8,
    port: u8,
    note_id: i32,
    ne_value: f64,
}

#[cfg(test)]
struct NoteIdMap {
    slots: [TestNoteIdSlot; Self::CAPACITY],
    cursor: usize,
}

#[cfg(test)]
#[derive(Clone, Copy)]
struct TestNoteIdSlot {
    note_id: i32,
    port: u8,
    channel: u8,
    note: u8,
}

#[cfg(test)]
impl NoteIdMap {
    const CAPACITY: usize = 128;
    const FREE: TestNoteIdSlot = TestNoteIdSlot {
        note_id: -1,
        port: 0,
        channel: 0,
        note: 0,
    };

    fn new() -> Self {
        Self {
            slots: [Self::FREE; Self::CAPACITY],
            cursor: 0,
        }
    }

    fn insert(&mut self, port: u8, note_id: i32, channel: u8, note: u8) {
        if note_id < 0 {
            return;
        }
        let index = self
            .slots
            .iter()
            .position(|slot| slot.note_id == note_id && slot.port == port)
            .or_else(|| self.slots.iter().position(|slot| slot.note_id < 0))
            .unwrap_or_else(|| {
                let index = self.cursor;
                self.cursor = (index + 1) % Self::CAPACITY;
                index
            });
        self.slots[index] = TestNoteIdSlot {
            note_id,
            port,
            channel,
            note,
        };
    }

    fn lookup(&self, port: u8, note_id: i32) -> Option<(u8, u8)> {
        self.slots
            .iter()
            .find(|slot| note_id >= 0 && slot.note_id == note_id && slot.port == port)
            .map(|slot| (slot.channel, slot.note))
    }

    fn clear(&mut self) {
        self.slots = [Self::FREE; Self::CAPACITY];
        self.cursor = 0;
    }
}

// ---------------------------------------------------------------------------
// C callback implementations
//
// SAFETY for all unsafe extern "C" fn below:
// - `ctx` is a *mut c_void created by Box::into_raw in cb_create().
//   Valid until cb_destroy() (called exactly once by the C++ shim).
// - The C++ shim (MooseComponent) owns the Rust context and
//   guarantees exclusive access: process() on the audio thread,
//   all other callbacks on the main thread, never concurrent.
// - Audio buffer pointers come from the VST3 host via ProcessData
//   and are valid for the declared channel count × numSamples.
// - Parameter IDs and values come from IParamValueQueue and are
//   guaranteed valid by the VST3 host.
// ---------------------------------------------------------------------------

unsafe extern "C" fn cb_create<P: PluginExport>() -> *mut std::ffi::c_void {
    // Author `create` / `init` run here; a panic must not cross the FFI
    // boundary. A null return tells the host construction failed.
    run_extern_callback_with::<P, *mut std::ffi::c_void>(
        "vst3",
        "create",
        std::ptr::null_mut(),
        || {
            let mut plugin = P::create();
            plugin.init();
            let info = P::info();
            let param_infos: Vec<ParamInfo> = plugin.params().param_infos();
            let mut param_ranges: Vec<(u32, ParamRange)> =
                param_infos.iter().map(|i| (i.id, i.range)).collect();
            // Sort by id so `binary_search_by_key` works in the hot lookups.
            param_ranges.sort_by_key(|(id, _)| *id);
            // Precompute the MIDI-controller bindings, sorted by id, so the
            // audio thread bridges mapped controllers without a linear scan.
            let mut midi_maps: Vec<(u32, MidiMap)> = param_infos
                .iter()
                .filter_map(|i| MidiMap::from_param(i).map(|m| (i.id, m)))
                .collect();
            midi_maps.sort_by_key(|(id, _)| *id);
            let params_arc = plugin.params_arc();
            let meter_store = plugin.meter_store();
            let snapshot = plugin.snapshot_slot();
            let task_spawner = plugin.task_spawner();
            let editor_builder = plugin.editor_builder();
            let latency_cache = AtomicU32::new(plugin.latency());
            let tail_cache = AtomicU32::new(plugin.tail());
            let midi_proxy_ids = allocate_midi_proxy_ids(&param_infos, midi_proxy_len::<P>());
            let midi_proxy_values = (0..midi_proxy_ids.len())
                .map(|i| {
                    // Bounded by the proxy count.
                    #[allow(clippy::cast_possible_truncation)]
                    let controller = (i as u32) % MIDI_PROXY_PER_CHANNEL;
                    AtomicU64::new(midi_proxy_default(controller).to_bits())
                })
                .collect();
            let instance = Box::new(Vst3Instance::<P> {
                plugin: shared_plugin(plugin),
                params_arc,
                meter_store,
                snapshot,
                task_spawner,
                editor_builder,
                param_infos,
                min_subblock_samples: info.automation.min_subblock_samples,
                plugin_id_hash: state::shared_plugin_state_hash(&info),
                active: AtomicBool::new(false),
                param_ranges,
                midi_maps,
                transport_slot: TransportSlot::new(),
                pending_state: Arc::new(StateLoadQueue::new(1)),
                latency_cache,
                tail_cache,
                midi_proxy_ids,
                midi_proxy_values,
                host_scale: AtomicU64::new(0),
                window_scale: AtomicU64::new(0),
                pending_resize: AtomicU64::new(0),
                audio: PluginCell::new(Vst3Scratch {
                    event_list: EventList::with_capacity(EVENT_LIST_PREALLOC),
                    input_num_frames: 0,
                    output_events: EventList::with_capacity(EVENT_LIST_PREALLOC),
                    sub_event_scratch: EventList::with_capacity(EVENT_LIST_PREALLOC),
                    sample_rate: 44100.0,
                    // 8192 covers the largest block sizes mainstream DAWs /
                    // validators use (Reaper / pluginval <= 4096); a non-zero
                    // default keeps the process-before-activate path from
                    // tripping the contract assert.
                    max_block_size: 8192,
                    prepared: false,
                    scratch: RawBufferScratch::default(),
                    output_cursor: LosslessEventCursor::default(),
                    output_preflight_status: VST3_EVENT_END,
                    output_note_ids: OutputNoteIds::new(),
                    pending_output_mutation: PendingOutputMutation::None,
                }),
                gui: PluginCell::new(Vst3Gui { editor: None }),
            });
            let raw = Box::into_raw(instance);
            raw.cast::<std::ffi::c_void>()
        },
    )
}

unsafe extern "C" fn cb_destroy<P: PluginExport>(ctx: *mut std::ffi::c_void) {
    // Dropping the instance cascades into the editor's `Drop` (wgpu surface
    // / NSView / baseview / runloop-timer teardown). A panic there would
    // unwind across this `extern "C"` boundary and abort the host. Hosts
    // destroy instances routinely mid-session - removing a plugin from a
    // track, closing a project - not just at quit, so swallowing the panic
    // keeps a live host alive; the instance is being torn down regardless.
    run_extern_callback_with::<P, ()>("vst3", "destroy", (), || unsafe {
        if !ctx.is_null() {
            drop(Box::from_raw(ctx.cast::<Vst3Instance<P>>()));
        }
    });
}

/// Map a VST3 `ProcessModes` value (`kRealtime` 0, `kPrefetch` 1,
/// `kOffline` 2) to a moose [`ProcessMode`]. Unknown values fall back
/// to `Realtime`.
fn vst3_process_mode(mode: i32) -> ProcessMode {
    match mode {
        1 => ProcessMode::Buffered,
        2 => ProcessMode::Offline,
        _ => ProcessMode::Realtime,
    }
}

unsafe extern "C" fn cb_reset<P: PluginExport>(
    ctx: *mut std::ffi::c_void,
    sample_rate: f64,
    max_frames: u32,
    process_mode: i32,
) {
    // Author `reset` can panic (allocation, DSP prep); firewall it so the
    // panic can't unwind across the C ABI and abort the host.
    run_extern_callback_with::<P, ()>("vst3", "reset", (), || unsafe {
        let inst = &*ctx.cast::<Vst3Instance<P>>();
        let mut audio = inst.audio.enter();
        // Clamp host-supplied max_frames up to a sane minimum: hosts that
        // don't honor their own setupProcessing contract can pass 0 here,
        // which would size plugin-internal delay lines to zero and blow up
        // on the first non-zero process() call.
        let max_frames = (max_frames as usize).max(1024);
        audio.sample_rate = sample_rate;
        audio.max_block_size = max_frames;
        // Grow per-block scratch to cover the *widest* declared layout and
        // this block size before the first process() call so the audio
        // thread stays alloc-free. VST3 negotiates layouts at runtime
        // (setBusArrangements), so a later, wider layout than the default
        // must not grow the per-channel Vecs on the audio thread - size to
        // the same structural width the shim gathers, including disabled
        // optional buses that retain null/silence positions in the flat
        // array. Active-only totals would leave `build` allocating here on
        // the audio thread when every declared optional bus is disabled.
        let (num_in, num_out) = max_layout_channels(&P::bus_layouts());
        audio
            .scratch
            .ensure_capacity(num_in as usize, num_out as usize, max_frames);
        {
            let mut plugin = enter_plugin(&inst.plugin);
            let config = AudioConfig::new(sample_rate, max_frames)
                .with_process_mode(vst3_process_mode(process_mode));
            plugin.reset(&config);
            inst.latency_cache
                .store(plugin.latency(), Ordering::Relaxed);
            inst.tail_cache.store(plugin.tail(), Ordering::Relaxed);
        }
        audio.output_note_ids.clear();
        audio.pending_output_mutation = PendingOutputMutation::None;
        audio.prepared = true;
    });
}

/// `IComponent::setActive`. Tracks activation so `cb_state_load` knows
/// whether the audio thread will drain the pending-state queue.
unsafe extern "C" fn cb_set_active<P: PluginExport>(ctx: *mut std::ffi::c_void, active: i32) {
    // The deactivation drain runs author `load_state` via `apply_state`;
    // firewall it so a panic can't unwind across the C ABI.
    run_extern_callback_with::<P, ()>("vst3", "set_active", (), || unsafe {
        let inst = &*ctx.cast::<Vst3Instance<P>>();
        inst.active.store(active != 0, Ordering::Relaxed);
        if active == 0 {
            // Deactivating: the audio thread stops draining `pending_state`,
            // so a load queued (by the host or the editor) while active would
            // strand and a following `getState` would re-serialize stale extra
            // state - silently losing the load. Apply it now under the plugin
            // lock (uncontended: the host serializes lifecycle calls with
            // `process`) and refresh the snapshot.
            if let Some(state) = inst.pending_state.pop() {
                let mut plugin = enter_plugin(&inst.plugin);
                state::apply_state(&mut *plugin, &state);
                plugin.republish_snapshot();
            }
        }
    });
}

/// The `bus_layouts()` index matching `(in_ch, out_ch)`, or `-1`. The
/// shim's `setBusArrangements` calls this to accept any declared layout.
/// Static per plugin type - no instance context.
unsafe extern "C" fn cb_match_bus_layout<P: PluginExport>(in_ch: u32, out_ch: u32) -> i32 {
    find_bus_layout::<P>(in_ch, out_ch).map_or(-1, |i| i32::try_from(i).unwrap_or(-1))
}

/// Per-bus channel width of a declared layout, for the shim's per-bus
/// `getBusInfo` / `getBusArrangement` and its process-time channel gather.
/// `0` for any out-of-range index.
unsafe extern "C" fn cb_layout_bus_channels<P: PluginExport>(
    layout_index: u32,
    is_output: i32,
    bus_index: u32,
) -> u32 {
    let (Ok(li), Ok(bi)) = (usize::try_from(layout_index), usize::try_from(bus_index)) else {
        return 0;
    };
    let layouts = P::bus_layouts();
    let Some(layout) = layouts.get(li) else {
        return 0;
    };
    let buses = if is_output != 0 {
        &layout.outputs
    } else {
        &layout.inputs
    };
    buses.get(bi).map_or(0, |b| b.channels.channel_count())
}

/// Match a host-proposed per-bus arrangement (arrays of per-bus channel
/// counts) to a declared `bus_layouts()` index, or `-1`. A layout matches
/// when its per-bus widths equal the host's, direction by direction - so
/// a sidechain bus is matched on its own width, not summed into the main.
unsafe extern "C" fn cb_match_bus_layout_perbus<P: PluginExport>(
    in_channels: *const u32,
    num_in: u32,
    out_channels: *const u32,
    num_out: u32,
) -> i32 {
    // SAFETY: the shim passes arrays of the lengths it declares, or null
    // with length 0 for a bus-less direction.
    let ins = unsafe { slice_or_empty(in_channels, num_in) };
    let outs = unsafe { slice_or_empty(out_channels, num_out) };
    let widths_match = |buses: &[BusConfig], want: &[u32]| {
        buses.len() == want.len()
            && buses
                .iter()
                .zip(want)
                .all(|(b, &w)| b.channels.channel_count() == w)
    };
    P::bus_layouts()
        .iter()
        .position(|l| widths_match(&l.inputs, ins) && widths_match(&l.outputs, outs))
        .map_or(-1, |i| i32::try_from(i).unwrap_or(-1))
}

/// `(num_input_buses, num_output_buses, input_kinds_ptr, output_kinds_ptr)`
/// for the descriptor, from the plugin's first declared layout. The
/// kind-byte arrays are leaked to `'static`.
fn descriptor_buses<P: PluginExport>() -> (u32, u32, *const u8, *const u8) {
    let first = P::bus_layouts().into_iter().next().unwrap_or_default();
    let ins = leak_bus_kinds(&first.inputs);
    let outs = leak_bus_kinds(&first.outputs);
    (
        u32::try_from(ins.len()).unwrap_or(0),
        u32::try_from(outs.len()).unwrap_or(0),
        ins.as_ptr(),
        outs.as_ptr(),
    )
}

/// Leak the per-bus kind bytes of a bus list (`0` = Main, `1` = Sidechain)
/// to `'static` for the descriptor's raw pointer.
fn leak_bus_kinds(buses: &[BusConfig]) -> &'static [u8] {
    Box::leak(
        buses
            .iter()
            .map(|b| u8::from(b.kind == BusKind::Sidechain))
            .collect::<Vec<u8>>()
            .into_boxed_slice(),
    )
}

/// True when every declared layout shares the first layout's bus topology:
/// the same input and output bus COUNT and the same per-bus KIND (main vs
/// sidechain) in order. VST3 fixes one bus topology per plugin - only
/// channel widths are negotiated - so [`descriptor_buses`] takes the bus
/// count + kinds from layout 0 and [`cb_match_bus_layout_perbus`] requires
/// the host arrangement to have exactly that many buses. A layout with a
/// different bus count or kind can therefore never be matched (it's dead),
/// or, if it leads the list, silently drops a bus the others declare.
fn vst3_topology_consistent(layouts: &[BusLayout]) -> bool {
    let kinds = |buses: &[BusConfig]| buses.iter().map(|b| b.kind).collect::<Vec<BusKind>>();
    let Some(first) = layouts.first() else {
        return true;
    };
    let (first_in, first_out) = (kinds(&first.inputs), kinds(&first.outputs));
    layouts
        .iter()
        .all(|l| kinds(&l.inputs) == first_in && kinds(&l.outputs) == first_out)
}

/// Build a slice from a `(ptr, len)` the C++ shim handed us, or an empty
/// slice when the pointer is null (a direction with no buses).
unsafe fn slice_or_empty<'a>(ptr: *const u32, len: u32) -> &'a [u32] {
    match usize::try_from(len) {
        Ok(n) if !ptr.is_null() && n > 0 => unsafe { std::slice::from_raw_parts(ptr, n) },
        _ => &[],
    }
}

const VST3_EVENT_NOTE_ON: u32 = 0;
const VST3_EVENT_NOTE_OFF: u32 = 1;
const VST3_EVENT_DATA: u32 = 2;
const VST3_EVENT_POLY_PRESSURE: u32 = 3;
const VST3_EVENT_NOTE_EXPRESSION: u32 = 4;
const VST3_EVENT_LEGACY_MIDI_CC_OUT: u32 = 65_535;

const VST3_EVENT_END: u32 = 0;
const VST3_EVENT_EMITTED: u32 = 1;
const VST3_EVENT_UNSUPPORTED: u32 = 2;
const VST3_EVENT_INVALID: u32 = 3;
const VST3_EVENT_QUEUE_FULL: u32 = 4;
const VST3_EVENT_IS_LIVE: u32 = 1;

unsafe extern "C" fn cb_begin_input_events<P: PluginExport>(
    ctx: *mut std::ffi::c_void,
    num_frames: u32,
) {
    unsafe {
        let inst = &*ctx.cast::<Vst3Instance<P>>();
        let mut audio = inst.audio.enter();
        audio.event_list.clear();
        audio.input_num_frames = num_frames;
    }
}

unsafe extern "C" fn cb_push_input_event<P: PluginExport>(
    ctx: *mut std::ffi::c_void,
    event: *const Vst3NativeEvent,
) -> u32 {
    unsafe {
        if event.is_null() {
            return VST3_EVENT_INVALID;
        }
        let inst = &*ctx.cast::<Vst3Instance<P>>();
        let mut audio = inst.audio.enter();
        push_vst3_input_event::<P>(&mut audio, &*event)
    }
}

fn push_vst3_input_event<P: PluginExport>(
    audio: &mut Vst3Scratch<P>,
    event: &Vst3NativeEvent,
) -> u32 {
    let Ok(sample_offset) = u32::try_from(event.sample_offset) else {
        return VST3_EVENT_INVALID;
    };
    let Ok(port) = u8::try_from(event.bus_index) else {
        return VST3_EVENT_INVALID;
    };
    if sample_offset >= audio.input_num_frames || port >= P::info().midi_input_ports {
        return VST3_EVENT_INVALID;
    }
    let Some((qualifiers, metadata)) = vst3_exact_provenance(event) else {
        return VST3_EVENT_INVALID;
    };

    let result = match event.kind {
        VST3_EVENT_NOTE_ON | VST3_EVENT_NOTE_OFF => {
            let Some((channel, pitch)) = valid_note_axes(event.channel, event.pitch) else {
                return VST3_EVENT_INVALID;
            };
            if !valid_unit_f32(event.velocity) || !event.tuning.is_finite() {
                return VST3_EVENT_INVALID;
            }
            let (kind, length) = if event.kind == VST3_EVENT_NOTE_ON {
                if event.length < 0 {
                    return VST3_EVENT_INVALID;
                }
                (ExactNoteKind::On, Some(event.length))
            } else {
                (ExactNoteKind::Off, None)
            };
            let address = ExactNoteAddress::from_vst3_signed(
                event.bus_index,
                event.channel,
                event.pitch,
                event.note_id,
            );
            let exact = ExactEvent::new(
                sample_offset,
                ExactEventBody::DetailedNote {
                    kind,
                    address,
                    velocity: event.velocity,
                    tuning: event.tuning,
                    length,
                },
            )
            .with_qualifiers(qualifiers)
            .with_metadata(ExactEventMetadata::Vst3(metadata));
            let typed = faithful_typed_note(event, channel, pitch, kind)
                .map(|body| Event::on_port(sample_offset, port, body));
            match typed {
                Some(typed) => audio.event_list.try_push_with_exact(typed, exact),
                None => audio.event_list.try_push_exact(exact),
            }
        }
        VST3_EVENT_POLY_PRESSURE => {
            let Some((channel, pitch)) = valid_note_axes(event.channel, event.pitch) else {
                return VST3_EVENT_INVALID;
            };
            if !valid_unit_f32(event.velocity) {
                return VST3_EVENT_INVALID;
            }
            let address = ExactNoteAddress::from_vst3_signed(
                event.bus_index,
                event.channel,
                event.pitch,
                event.note_id,
            );
            let exact = ExactEvent::new(
                sample_offset,
                ExactEventBody::DetailedPolyPressure {
                    address,
                    pressure: event.velocity,
                },
            )
            .with_qualifiers(qualifiers)
            .with_metadata(ExactEventMetadata::Vst3(metadata));
            let typed = (event.note_id == -1)
                .then(|| normalized_u7_exact(event.velocity))
                .flatten()
                .map(|pressure| {
                    Event::on_port(
                        sample_offset,
                        port,
                        EventBody::Aftertouch {
                            group: 0,
                            channel,
                            note: pitch,
                            pressure,
                        },
                    )
                });
            match typed {
                Some(typed) => audio.event_list.try_push_with_exact(typed, exact),
                None => audio.event_list.try_push_exact(exact),
            }
        }
        VST3_EVENT_NOTE_EXPRESSION => {
            if !valid_unit_f64(event.value) || event.note_id == -1 {
                return VST3_EVENT_INVALID;
            }
            let exact = ExactEvent::new(
                sample_offset,
                ExactEventBody::NormalizedNoteExpression {
                    expression_id: event.type_id,
                    address: ExactNoteAddress::from_vst3_signed(
                        event.bus_index,
                        -1,
                        -1,
                        event.note_id,
                    ),
                    value: event.value,
                },
            )
            .with_qualifiers(qualifiers)
            .with_metadata(ExactEventMetadata::Vst3(metadata));
            audio.event_list.try_push_exact(exact)
        }
        VST3_EVENT_DATA => {
            if event.type_id != 0 || (event.len > 0 && event.bytes.is_null()) {
                return VST3_EVENT_INVALID;
            }
            let bytes = if event.len == 0 {
                &[][..]
            } else {
                unsafe { std::slice::from_raw_parts(event.bytes, event.len as usize) }
            };
            let exact = ExactEvent::new(
                sample_offset,
                ExactEventBody::SysEx {
                    port: u16::from(port),
                },
            )
            .with_qualifiers(qualifiers)
            .with_metadata(ExactEventMetadata::Vst3(metadata));
            audio
                .event_list
                .try_push_sysex_with_exact_on_port(sample_offset, port, bytes, exact)
        }
        _ => return VST3_EVENT_UNSUPPORTED,
    };

    match result {
        Ok(()) => VST3_EVENT_EMITTED,
        Err(
            PushError::EventFull
            | PushError::ExactEventFull
            | PushError::VoiceTrackerFull
            | PushError::PoolFull,
        ) => VST3_EVENT_QUEUE_FULL,
        Err(PushError::UnknownExactEvent) => VST3_EVENT_INVALID,
    }
}

fn vst3_exact_provenance(
    event: &Vst3NativeEvent,
) -> Option<(ExactEventQualifiers, Vst3EventMetadata)> {
    let raw_flags = u16::try_from(event.flags).ok()?;
    let metadata = Vst3EventMetadata::new(event.ppq_position, raw_flags)?;
    let qualifiers = ExactEventQualifiers {
        is_live: event.flags & VST3_EVENT_IS_LIVE != 0,
        dont_record: false,
    };
    Some((qualifiers, metadata))
}

fn valid_note_axes(channel: i16, pitch: i16) -> Option<(u8, u8)> {
    let channel = u8::try_from(channel).ok().filter(|value| *value < 16)?;
    let pitch = u8::try_from(pitch).ok().filter(|value| *value < 128)?;
    Some((channel, pitch))
}

fn valid_unit_f32(value: f32) -> bool {
    value.is_finite() && (0.0..=1.0).contains(&value)
}

fn valid_unit_f64(value: f64) -> bool {
    value.is_finite() && (0.0..=1.0).contains(&value)
}

#[allow(clippy::float_cmp)]
fn normalized_u7_exact(value: f32) -> Option<u8> {
    let scaled = value * 127.0;
    let rounded = scaled.round();
    if rounded != scaled {
        return None;
    }
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let wire = rounded as u8;
    (f32::from(wire) / 127.0 == value).then_some(wire)
}

fn faithful_typed_note(
    event: &Vst3NativeEvent,
    channel: u8,
    pitch: u8,
    kind: ExactNoteKind,
) -> Option<EventBody> {
    if event.note_id != -1 || event.tuning != 0.0 || event.length != 0 {
        return None;
    }
    let velocity = normalized_u7_exact(event.velocity)?;
    match kind {
        ExactNoteKind::On if velocity > 0 => Some(EventBody::NoteOn {
            group: 0,
            channel,
            note: pitch,
            velocity,
        }),
        ExactNoteKind::Off => Some(EventBody::NoteOff {
            group: 0,
            channel,
            note: pitch,
            velocity,
        }),
        _ => None,
    }
}

unsafe extern "C" fn cb_process<P: PluginExport>(
    ctx: *mut std::ffi::c_void,
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
    transport_ptr: *const ffi::Vst3Transport,
    param_changes: *const ffi::Vst3ParamChange,
    num_param_changes: u32,
    process_mode: i32,
) -> u32 {
    // SAFETY: forwarded - the shim's contract is the same.
    unsafe {
        process_block::<P, f32>(
            ctx,
            inputs,
            outputs,
            num_input_channels,
            num_output_channels,
            input_bus_channels,
            num_input_buses,
            input_bus_active,
            output_bus_channels,
            num_output_buses,
            output_bus_active,
            num_frames,
            transport_ptr,
            param_changes,
            num_param_changes,
            process_mode,
        )
    }
}

/// 64-bit wire twin of [`cb_process`]. The shim routes here when the
/// host negotiated `kSample64` in `setupProcessing` (only offered for
/// `f64` plugins), so an `f64` plugin reads and writes host memory
/// directly with no widen/narrow pass.
unsafe extern "C" fn cb_process_f64<P: PluginExport>(
    ctx: *mut std::ffi::c_void,
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
    transport_ptr: *const ffi::Vst3Transport,
    param_changes: *const ffi::Vst3ParamChange,
    num_param_changes: u32,
    process_mode: i32,
) -> u32 {
    // SAFETY: forwarded - the shim's contract is the same.
    unsafe {
        process_block::<P, f64>(
            ctx,
            inputs,
            outputs,
            num_input_channels,
            num_output_channels,
            input_bus_channels,
            num_input_buses,
            input_bus_active,
            output_bus_channels,
            num_output_buses,
            output_bus_active,
            num_frames,
            transport_ptr,
            param_changes,
            num_param_changes,
            process_mode,
        )
    }
}

/// Shared body of [`cb_process`] / [`cb_process_f64`], generic over
/// the host wire precision `H`. `RawBufferScratch` zero-copies when
/// `H` matches the plugin's `Sample` and converts through scratch
/// otherwise, so both wires work for both plugin precisions.
// The parameter list mirrors the C ABI callback signature 1:1.
#[allow(clippy::too_many_lines, clippy::too_many_arguments)]
unsafe fn process_block<P: PluginExport, H: Sample>(
    ctx: *mut std::ffi::c_void,
    inputs: *const *const H,
    outputs: *mut *mut H,
    num_input_channels: u32,
    num_output_channels: u32,
    input_bus_channels: *const u32,
    num_input_buses: u32,
    input_bus_active: u32,
    output_bus_channels: *const u32,
    num_output_buses: u32,
    output_bus_active: u32,
    num_frames: u32,
    transport_ptr: *const ffi::Vst3Transport,
    param_changes: *const ffi::Vst3ParamChange,
    num_param_changes: u32,
    process_mode: i32,
) -> u32 {
    let nf = num_frames as usize;
    let ok = run_audio_block::<P>("VST3", || unsafe {
        // Shared `&Vst3Instance` (never a whole-struct `&mut`) - the audio
        // scratch is reached through its ownership cell, so a concurrent
        // host-thread `&*ctx` (param reads, GUI) can't alias us.
        let inst = &*ctx.cast::<Vst3Instance<P>>();
        let mut audio = inst.audio.enter();
        let num_frames = nf;

        // Host called process() before setActive(true) - the plugin
        // hasn't been told its sample rate / max block size yet, so
        // running DSP would feed garbage out of un-snapped smoothers.
        // Zero outputs and bail.
        if !audio.prepared {
            for ch in 0..num_output_channels as usize {
                let ptr = *outputs.add(ch);
                if !ptr.is_null() {
                    std::ptr::write_bytes(ptr, 0, num_frames);
                }
            }
            audio.event_list.clear();
            audio.output_events.clear();
            audio.output_events.clear_overflow();
            return;
        }

        // Take ownership of the plugin for the whole block: an
        // uncontended `Acquire`, never a wait, since the host contract
        // keeps `process` from overlapping a lifecycle callback and host
        // saves read the snapshot instead of the plugin.
        let mut plugin = enter_plugin(&inst.plugin);

        // Apply any pending state-load before per-block work so the
        // plugin sees consistent params and extra state for the
        // entire block. See `pending_state` field comment for the
        // queue-overflow policy.
        if let Some(state) = inst.pending_state.pop() {
            state::apply_state(&mut *plugin, &state);
        }

        // Paranoid allocation check (the `rt-paranoid` feature): guard the
        // wrapper's per-block glue - event conversion, transport, process,
        // output encode, snapshot publish - as well as the plugin. Placed
        // after the state-load apply above, since `load_state` legitimately
        // allocates. No-op and zero-sized when the feature is off.
        let _rt = RtSection::enter();

        // One reborrow of the scratch so the disjoint fields below (event
        // list, sub-block scratch, output events, build scratch) can be
        // borrowed simultaneously - guard `Deref` can't split-borrow.
        let scr = &mut *audio;

        debug_assert_eq!(scr.input_num_frames as usize, num_frames);

        // Build AudioBuffer from raw pointers. Uses the per-instance
        // `scratch` so the audio thread doesn't heap-allocate.
        debug_assert!(
            num_frames <= scr.max_block_size,
            "host violated VST3 contract: process() got {num_frames} frames \
             but setupProcessing declared max {}",
            scr.max_block_size
        );
        let mut audio_buffer = scr.scratch.build(
            inputs,
            outputs,
            num_input_channels,
            num_output_channels,
            len_u32(num_frames),
            P::supports_in_place(),
        );
        let mut bus_routing = BusRouting::new();
        if !input_bus_channels.is_null() {
            debug_assert!((num_input_buses as usize) <= MAX_AUDIO_BUSES);
            for (index, &channels) in
                slice::from_raw_parts(input_bus_channels, num_input_buses as usize)
                    .iter()
                    .enumerate()
            {
                debug_assert!(bus_routing.push_input(
                    channels,
                    if input_bus_active & (1_u32 << index) == 0 {
                        BusActivation::Inactive
                    } else {
                        BusActivation::Active
                    },
                ));
            }
        }
        if !output_bus_channels.is_null() {
            debug_assert!((num_output_buses as usize) <= MAX_AUDIO_BUSES);
            for (index, &channels) in
                slice::from_raw_parts(output_bus_channels, num_output_buses as usize)
                    .iter()
                    .enumerate()
            {
                debug_assert!(bus_routing.push_output(
                    channels,
                    if output_bus_active & (1_u32 << index) == 0 {
                        BusActivation::Inactive
                    } else {
                        BusActivation::Active
                    },
                ));
            }
        }

        // Queue sample-accurate parameter changes. `set_plain` is
        // deferred to the chunker's per-sub-block apply pass so
        // smoothers see `set_target` at the event's sample rather
        // than at the head of the audio block.
        // The C++ shim sends plain (denormalized) values.
        if !param_changes.is_null() && num_param_changes > 0 {
            let changes = slice::from_raw_parts(param_changes, num_param_changes as usize);
            for pc in changes {
                // VST3 delivers sampleOffset as int32; per-block
                // offsets are non-negative and bounded by block size.
                #[allow(clippy::cast_sign_loss)]
                let sample_offset = pc.sample_offset as u32;
                // Unbound MIDI controllers arrive on the hidden proxy
                // ids: decode straight to the event. No `ParamChange`
                // and no `Params` write - a proxy is not a plugin
                // parameter. The shim's denormalize is identity for
                // proxy ids, so `pc.value` is the host's raw `0..=1`.
                // The id carries the event bus it was mapped for, so
                // multi-port plugins keep controllers per port.
                if let Some((port, channel, controller)) =
                    allocated_midi_proxy_decode(&inst.midi_proxy_ids, pc.id)
                {
                    #[allow(clippy::cast_possible_truncation)]
                    let normalized = pc.value.clamp(0.0, 1.0) as f32;
                    scr.event_list.push(Event::on_port(
                        sample_offset,
                        port,
                        midi_proxy_event(channel, controller, normalized),
                    ));
                    continue;
                }
                // MIDI-mapped controllers (pitch bend, CC, pressure,
                // program) arrive here as parameter changes because
                // VST3 has no native input event for them. Bridge them
                // back into the MIDI event the plugin expects, in
                // addition to the plain `ParamChange` so the bound
                // parameter still tracks the controller.
                //
                // The bridged event is port 0: an explicit `midi_map`
                // binds one plugin parameter across every bus, so the
                // host delivers a bus-less parameter change with no
                // originating port to recover - unlike the per-bus
                // proxy ids decoded above.
                if let Ok(idx) = inst.midi_maps.binary_search_by_key(&pc.id, |(id, _)| *id) {
                    scr.event_list.push(Event {
                        sample_offset,
                        port: 0,
                        body: midi_event_from_map(&inst.midi_maps[idx].1, pc.value),
                    });
                }
                scr.event_list.push(Event {
                    sample_offset,
                    port: 0,
                    body: EventBody::ParamChange {
                        id: pc.id,
                        value: pc.value,
                    },
                });
            }
        }
        // Single stable sort across the merged MIDI + param-change
        // streams. Stable sort preserves the within-group order each
        // section already pushed in.
        scr.event_list.ensure_sorted_by_offset();

        let transport = if transport_ptr.is_null() {
            TransportInfo::default()
        } else {
            let t = &*transport_ptr;
            TransportInfo {
                playing: t.playing != 0,
                recording: t.recording != 0,
                tempo: t.tempo,
                // VST3 hosts deliver `i32` time-signature fields; the
                // u8 narrowing is bounded by the MIDI domain (≤ 255).
                #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                time_sig_num: t.time_sig_num as u8,
                #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                time_sig_den: t.time_sig_den as u8,
                position_samples: sample_pos_i64(t.position_samples),
                // Derived from samples so a plugin reading
                // `position_seconds` gets the same value on every format
                // (CLAP populates it directly). Guard the pre-reset zero SR.
                position_seconds: if scr.sample_rate > 0.0 {
                    t.position_samples / scr.sample_rate
                } else {
                    0.0
                },
                position_beats: t.position_beats,
                bar_start_beats: t.bar_start_beats,
                loop_active: t.cycle_active != 0,
                loop_start_beats: t.cycle_start_beats,
                loop_end_beats: t.cycle_end_beats,
            }
        };

        scr.output_events.clear();
        scr.output_events.clear_overflow();
        inst.transport_slot.write(&transport);

        let mut transport_snap = transport;
        let chunk_args = ChunkedProcess {
            events: &scr.event_list,
            sub_event_scratch: &mut scr.sub_event_scratch,
            transport: &mut transport_snap,
            sample_rate: scr.sample_rate,
            process_mode: vst3_process_mode(process_mode),
            output_events: &mut scr.output_events,
            params_fn: None,
            meters_fn: None,
            param_infos: &inst.param_infos,
            min_subblock_samples: inst.min_subblock_samples,
        };
        process_chunked_with_bus_routing(
            &mut *plugin,
            inst.params_arc.as_ref() as &dyn Params,
            &mut audio_buffer,
            chunk_args,
            bus_routing,
        );
        scr.output_events.ensure_sorted_by_offset();
        // End the `audio_buffer` borrow before reaching back into scratch.
        let _ = audio_buffer;
        // For `f64` plugins the scratch holds the rendered output -
        // copy + narrow it back to the host's `f32` pointers here.
        // No-op for `f32` plugins (output already pointed at the
        // host buffer).
        scr.scratch
            .finish_widening(outputs, num_output_channels, len_u32(num_frames));

        // Refresh latency / tail caches so the host's main-thread
        // queries don't have to touch the plugin. On an actual
        // latency change, flag a restart: `mark_restart` only sets an
        // atomic bit (RT-safe), and the shim calls `restartComponent` on
        // the next host main-thread callback.
        let new_latency = plugin.latency();
        if inst.latency_cache.swap(new_latency, Ordering::Relaxed) != new_latency {
            // `ctx` is the shim's live component key for this instance;
            // `mark_restart` only sets a bit on its atomic (RT-safe).
            ffi::moose_vst3_mark_restart(ctx, K_LATENCY_CHANGED);
        }
        inst.tail_cache.store(plugin.tail(), Ordering::Relaxed);
    });
    if !ok {
        // Panic in plugin.process() - zero outputs so the host
        // doesn't keep playing whatever stale samples were in the
        // buffer when DSP died.
        unsafe {
            for ch in 0..num_output_channels as usize {
                let ptr = *outputs.add(ch);
                if !ptr.is_null() {
                    std::ptr::write_bytes(ptr, 0, nf);
                }
            }
        }
    }
    u32::from(ok)
}

/// Test-only smoke helper for the `rt-paranoid` CI gate: drives a few
/// real process blocks through this wrapper's per-block glue via the
/// shared `process_block` body (with null events / transport / param
/// changes and small stereo buffers), returning the steady-state
/// audio-thread allocation count (0 = clean). Vacuously 0 unless the
/// `rt-paranoid` feature installs the checking allocator. Not public API.
#[doc(hidden)]
#[must_use]
pub fn rt_paranoid_smoke<P: PluginExport>() -> u32 {
    const FRAMES: u32 = 512;
    const CH: u32 = 2;
    let frames = FRAMES as usize;
    // SAFETY: constructs, drives, and destroys its own instance; all
    // pointers below outlive each `process_block` call, buffers sized to
    // `FRAMES`, and the event / transport / param pointers are null
    // (which `process_block` tolerates).
    unsafe {
        let ctx = cb_create::<P>();
        cb_reset::<P>(ctx, 48_000.0, FRAMES, 0);

        // Non-zero input so the sanity check below can confirm the block
        // actually processed (a no-op harness would leave zeros).
        let in_left = vec![0.5f32; frames];
        let in_right = vec![0.5f32; frames];
        let mut out_left = vec![0f32; frames];
        let mut out_right = vec![0f32; frames];
        let in_ptrs: [*const f32; 2] = [in_left.as_ptr(), in_right.as_ptr()];
        let mut out_ptrs: [*mut f32; 2] = [out_left.as_mut_ptr(), out_right.as_mut_ptr()];
        let bus_channels = [CH];

        let mut count = 0;
        for _ in 0..3 {
            let ((), n) = audit(|| {
                cb_begin_input_events::<P>(ctx, FRAMES);
                process_block::<P, f32>(
                    ctx,
                    in_ptrs.as_ptr(),
                    out_ptrs.as_mut_ptr(),
                    CH,
                    CH,
                    bus_channels.as_ptr(),
                    1,
                    1,
                    bus_channels.as_ptr(),
                    1,
                    1,
                    FRAMES,
                    std::ptr::null(),
                    std::ptr::null(),
                    0,
                    0,
                );
            });
            count = n;
        }

        assert!(
            out_left.iter().any(|s| s.abs() > 0.0),
            "vst3 smoke: process did not run (output stayed zero)"
        );
        cb_destroy::<P>(ctx);
        count
    }
}

unsafe extern "C" fn cb_param_count<P: PluginExport>(ctx: *mut std::ffi::c_void) -> u32 {
    unsafe {
        // Read the cached `param_ranges.len()` rather than walking the
        // `Params` impl. The cache is built once at instantiation
        // (`Vst3Instance::new`) and never grows; trait dispatch was
        // free per-call but consistent with the cache-first pattern
        // the rest of the file uses.
        let inst = &*ctx.cast::<Vst3Instance<P>>();
        len_u32(inst.param_ranges.len() + inst.midi_proxy_values.len())
    }
}

unsafe extern "C" fn cb_param_get_value<P: PluginExport>(
    ctx: *mut std::ffi::c_void,
    id: u32,
) -> f64 {
    unsafe {
        let inst = &*ctx.cast::<Vst3Instance<P>>();
        if let Some(index) = allocated_midi_proxy_index(&inst.midi_proxy_ids, id)
            && let Some(slot) = inst.midi_proxy_values.get(index)
        {
            return f64::from_bits(slot.load(Ordering::Relaxed));
        }
        inst.params_arc.get_plain(id).unwrap_or(0.0)
    }
}

unsafe extern "C" fn cb_param_set_value<P: PluginExport>(
    ctx: *mut std::ffi::c_void,
    id: u32,
    value: f64,
) {
    unsafe {
        let inst = &*ctx.cast::<Vst3Instance<P>>();
        if let Some(index) = allocated_midi_proxy_index(&inst.midi_proxy_ids, id)
            && let Some(slot) = inst.midi_proxy_values.get(index)
        {
            slot.store(value.clamp(0.0, 1.0).to_bits(), Ordering::Relaxed);
            return;
        }
        inst.params_arc.set_plain(id, value);
    }
}

unsafe extern "C" fn cb_param_presentation<P: PluginExport>(
    ctx: *mut std::ffi::c_void,
    id: u32,
    out: *mut c_char,
    capacity: u32,
) -> i32 {
    run_extern_callback_with::<P, i32>("vst3", "parameter_presentation", -1, || unsafe {
        if ctx.is_null() || out.is_null() || capacity == 0 {
            return -1;
        }
        let inst = &*ctx.cast::<Vst3Instance<P>>();
        let Some(presentation) = inst.params_arc.parameter_presentation(id) else {
            return -1;
        };
        let _ = copy_c_str(out, capacity as usize, &presentation.name);
        i32::from(presentation.hidden)
    })
}

unsafe extern "C" fn cb_output_bus_name<P: PluginExport>(
    ctx: *mut std::ffi::c_void,
    layout: u32,
    index: u32,
    out: *mut c_char,
    capacity: u32,
) -> i32 {
    run_extern_callback_with::<P, i32>("vst3", "output_port_name", 0, || unsafe {
        if out.is_null() || capacity == 0 {
            return 0;
        }
        let named = if ctx.is_null() {
            None
        } else {
            (*ctx.cast::<Vst3Instance<P>>()).params_arc.output_port_name(index)
        };
        let name = named.or_else(|| {
            let layouts = P::bus_layouts();
            let bus = layouts.get(layout as usize)?.outputs.get(index as usize)?;
            Some(bus.name.to_owned())
        });
        let Some(name) = name else {
            return 0;
        };
        let _ = copy_c_str(out, capacity as usize, &name);
        1
    })
}

unsafe extern "C" fn cb_output_bus_names_revision<P: PluginExport>(
    ctx: *mut std::ffi::c_void,
) -> u64 {
    run_extern_callback_with::<P, u64>("vst3", "output_port_names_revision", 0, || unsafe {
        if ctx.is_null() {
            return 0;
        }
        (*ctx.cast::<Vst3Instance<P>>())
            .params_arc
            .output_port_names_revision()
    })
}

unsafe extern "C" fn cb_param_presentation_revision<P: PluginExport>(
    ctx: *mut std::ffi::c_void,
) -> u64 {
    run_extern_callback_with::<P, u64>("vst3", "parameter_presentation_revision", 0, || unsafe {
        if ctx.is_null() {
            return 0;
        }
        (*ctx.cast::<Vst3Instance<P>>())
            .params_arc
            .parameter_presentation_revision()
    })
}

/// Whether `id` is a `CHUNKED` param. The shim keys its block-rate
/// pre-commit on this: chunked params are committed per-offset by
/// `process_chunked`, so the shim must not pre-write their end value.
unsafe extern "C" fn cb_param_is_chunked<P: PluginExport>(
    ctx: *mut std::ffi::c_void,
    id: u32,
) -> i32 {
    unsafe {
        let inst = &*ctx.cast::<Vst3Instance<P>>();
        let chunked = inst
            .param_infos
            .iter()
            .find(|info| info.id == id)
            .is_some_and(|info| info.flags.contains(ParamFlags::CHUNKED));
        i32::from(chunked)
    }
}

unsafe extern "C" fn cb_param_normalize<P: PluginExport>(
    ctx: *mut std::ffi::c_void,
    id: u32,
    plain: f64,
) -> f64 {
    unsafe {
        let inst = &*ctx.cast::<Vst3Instance<P>>();
        match inst.param_ranges.binary_search_by_key(&id, |(i, _)| *i) {
            Ok(idx) => inst.param_ranges[idx].1.normalize(plain),
            Err(_) => plain,
        }
    }
}

unsafe extern "C" fn cb_param_denormalize<P: PluginExport>(
    ctx: *mut std::ffi::c_void,
    id: u32,
    normalized: f64,
) -> f64 {
    unsafe {
        let inst = &*ctx.cast::<Vst3Instance<P>>();
        match inst.param_ranges.binary_search_by_key(&id, |(i, _)| *i) {
            Ok(idx) => inst.param_ranges[idx].1.denormalize(normalized),
            Err(_) => normalized,
        }
    }
}

unsafe extern "C" fn cb_param_format<P: PluginExport>(
    ctx: *mut std::ffi::c_void,
    id: u32,
    value: f64,
    out: *mut c_char,
    out_len: u32,
) -> u32 {
    // The author's `format_value` can panic (an `unwrap` on a host value
    // outside the declared domain); firewall it so that can't abort the
    // host. On panic the host sees an empty display string.
    run_extern_callback_with::<P, u32>("vst3", "format_value", 0, || unsafe {
        // `out_len == 0` would underflow on `out_len as usize - 1`
        // and let `copy_nonoverlapping` write the full formatted
        // string into a buffer the host claimed had zero capacity.
        // Treat zero capacity as "host wants nothing" and return.
        if out_len == 0 || out.is_null() {
            return 0;
        }
        let inst = &*ctx.cast::<Vst3Instance<P>>();
        match inst.params_arc.format_value(id, value) {
            Some(text) => len_u32(copy_c_str(out, out_len as usize, &text)),
            None => 0,
        }
    })
}

/// Parse host text entry for a MIDI-CC proxy param: a plain number,
/// clamped to the proxy's normalized `0..=1` domain.
fn parse_midi_proxy_text(text: &str) -> Option<f64> {
    let v = text.trim().parse::<f64>().ok()?;
    v.is_finite().then(|| v.clamp(0.0, 1.0))
}

unsafe extern "C" fn cb_param_parse<P: PluginExport>(
    ctx: *mut std::ffi::c_void,
    id: u32,
    text: *const c_char,
    out_plain: *mut f64,
) -> i32 {
    // Author `parse_value` can panic; firewall it (0 = "not parsed").
    run_extern_callback_with::<P, i32>("vst3", "parse_value", 0, || unsafe {
        if text.is_null() || out_plain.is_null() {
            return 0;
        }
        let Ok(text) = CStr::from_ptr(text).to_str() else {
            return 0;
        };
        let inst = &*ctx.cast::<Vst3Instance<P>>();
        // MIDI-CC proxy params have no `Params` entry; their display
        // text is the shim's `%.2f` fallback of the normalized value.
        if allocated_midi_proxy_index(&inst.midi_proxy_ids, id).is_some() {
            return match parse_midi_proxy_text(text) {
                Some(v) => {
                    *out_plain = v;
                    1
                }
                None => 0,
            };
        }
        match inst.params_arc.parse_value(id, text) {
            Some(v) => {
                *out_plain = v;
                1
            }
            None => 0,
        }
    })
}

unsafe extern "C" fn cb_state_save<P: PluginExport>(
    ctx: *mut std::ffi::c_void,
    out_data: *mut *mut u8,
    out_len: *mut u32,
) {
    // Pre-zero the out pointers so a panic anywhere in the body below
    // leaves the host seeing an empty blob rather than a stale buffer
    // pointer paired with whatever length was last written. The body
    // overwrites these on the happy path.
    unsafe {
        *out_data = std::ptr::null_mut();
        *out_len = 0;
    }
    run_extern_callback_with::<P, ()>("vst3", "save_state", (), || unsafe {
        let inst = &*ctx.cast::<Vst3Instance<P>>();
        // While inactive the audio thread never runs, so a state edit the
        // editor queued through `set_state` (or a load) sits undrained in
        // `pending_state` and would not reach the snapshot below - the save
        // would serialize the pre-edit state. Drain and apply it here on the
        // host thread, which owns the plugin while inactive, then republish
        // so the snapshot read reflects the edit. While active the audio
        // thread owns the plugin and drains the queue itself, so we must not
        // touch it here (read the snapshot only).
        if !inst.active.load(Ordering::Relaxed)
            && let Some(deserialized) = inst.pending_state.pop()
        {
            let mut plugin = enter_plugin(&inst.plugin);
            state::apply_state(&mut *plugin, &deserialized);
            plugin.republish_snapshot();
        }
        let (ids, values) = inst.params_arc.collect_values();
        // Read the custom state from the lock-free snapshot the audio
        // thread publishes each block. Never touches the plugin, so it
        // can't stall a block in flight.
        //
        // Allocator pin: this wrapper allocates with `libc_malloc` and
        // the C++ shim frees with `libc::free`. The Rust global
        // allocator must not appear on either side. (VST2 uses the
        // Rust global allocator for both save + free; do not cross
        // wires when refactoring `_save_state` paths together.)
        let extra = save_extra(&inst.snapshot);
        let persist = inst.params_arc.serialize_persist();
        let blob = state::serialize_state(inst.plugin_id_hash, &ids, &values, &extra, &persist);
        let len = blob.len();
        let ptr = libc_malloc(len).cast::<u8>();
        if ptr.is_null() {
            // malloc failed - `*out_data` is already null and
            // `*out_len` already 0 from the pre-zero above; nothing
            // to do on this branch except return.
            return;
        }
        std::ptr::copy_nonoverlapping(blob.as_ptr(), ptr, len);
        *out_data = ptr;
        *out_len = len_u32(len);
    });
}

unsafe extern "C" fn cb_state_load<P: PluginExport>(
    ctx: *mut std::ffi::c_void,
    data: *const u8,
    len: u32,
) -> i32 {
    run_extern_callback_with::<P, i32>("vst3", "load_state", 0, || unsafe {
        let inst = &*ctx.cast::<Vst3Instance<P>>();
        // `slice::from_raw_parts(null, n)` for `n > 0` is UB. Treat
        // `(null, *)` and `(_, 0)` the same as "host gave us nothing".
        if data.is_null() || len == 0 {
            return 0;
        }
        let blob = slice::from_raw_parts(data, len as usize);
        // Not this plugin's envelope? Offer the bytes to the plugin's
        // `migrate_state` hook (legacy sessions from a pre-moose
        // build); `None` fails the load honestly.
        let Some(deserialized) = state::parse_or_migrate::<P>(
            blob,
            inst.plugin_id_hash,
            state::PluginFormat::Vst3,
            None,
        ) else {
            return 0;
        };
        // Apply params synchronously on the host thread (atomic-safe)
        // so host-side queries that read parameter values right
        // after `setState` see the restored values without first
        // running a process block. pluginval / DAW preset reload
        // both observe this.
        state::apply_params(&*inst.params_arc, &deserialized);
        if inst.active.load(Ordering::Relaxed) {
            // Active: the audio thread drains `pending_state` at the top
            // of the next block and applies the custom-state blob under
            // its exclusive `&mut plugin`. `force_push` overwrites any
            // older pending blob - see the `pending_state` field comment
            // for why newest-wins is right.
            let _ = inst.pending_state.force_push(deserialized);
        } else {
            // Inactive: no `cb_process` will run, so apply the full
            // state (params + extra) synchronously under the plugin
            // lock - uncontended here since no audio thread is
            // processing. Otherwise a `getState` before the next
            // activate would re-serialize stale custom state.
            let mut plugin = enter_plugin(&inst.plugin);
            state::apply_state(&mut *plugin, &deserialized);
            // No `cb_process` will publish, so refresh the snapshot slot
            // now - a `getState` while still inactive reads live state.
            plugin.republish_snapshot();
        }
        // `try_enter`, not `enter`: a host can deliver `setState`
        // synchronously from a component-handler callback while an outer
        // GUI callback (e.g. `cb_gui_open`, which holds the cell across the
        // author's `editor.open`) still holds the cell. Re-entering with
        // `enter` would hand out a second aliasing `&mut` in release. When
        // busy, skip the notify: the editor reads live param/state values as
        // it finishes opening, so it renders the freshly-loaded state anyway.
        if let Some(mut gui) = inst.gui.try_enter()
            && let Some(ref mut editor) = gui.editor
        {
            editor.state_changed();
        }
        1
    })
}

unsafe extern "C" fn cb_state_free(data: *mut u8, _len: u32) {
    unsafe {
        if !data.is_null() {
            libc_free(data.cast::<std::ffi::c_void>());
        }
    }
}

unsafe extern "C" {
    fn malloc(size: usize) -> *mut std::ffi::c_void;
    fn free(ptr: *mut std::ffi::c_void);
}
unsafe fn libc_malloc(size: usize) -> *mut std::ffi::c_void {
    unsafe { malloc(size) }
}
unsafe fn libc_free(ptr: *mut std::ffi::c_void) {
    unsafe { free(ptr) }
}

// ---------------------------------------------------------------------------
// Latency + tail callbacks
// ---------------------------------------------------------------------------

unsafe extern "C" fn cb_get_latency<P: PluginExport>(ctx: *mut std::ffi::c_void) -> u32 {
    unsafe {
        let inst = &*ctx.cast::<Vst3Instance<P>>();
        inst.latency_cache.load(Ordering::Relaxed)
    }
}

unsafe extern "C" fn cb_get_tail<P: PluginExport>(ctx: *mut std::ffi::c_void) -> u32 {
    unsafe {
        let inst = &*ctx.cast::<Vst3Instance<P>>();
        inst.tail_cache.load(Ordering::Relaxed)
    }
}

// ---------------------------------------------------------------------------
// Output event callbacks
// ---------------------------------------------------------------------------

#[cfg(test)]
fn try_encode_vst3_midi(event: &Event) -> Option<Vst3MidiEvent> {
    let body = match event.body {
        body if note_expression_of(&body).is_some() => return None,
        other => downconvert_to_midi1(&other).unwrap_or(other),
    };
    let (status, data1, data2) = match body {
        EventBody::NoteOn {
            channel,
            note,
            velocity,
            ..
        } => (0x90 | (channel & 0x0F), note, velocity),
        EventBody::NoteOff {
            channel,
            note,
            velocity,
            ..
        } => (0x80 | (channel & 0x0F), note, velocity),
        EventBody::ControlChange {
            channel, cc, value, ..
        } => (0xB0 | (channel & 0x0F), cc, value),
        EventBody::Aftertouch {
            channel,
            note,
            pressure,
            ..
        } => (0xA0 | (channel & 0x0F), note, pressure),
        EventBody::ChannelPressure {
            channel, pressure, ..
        } => (0xD0 | (channel & 0x0F), pressure, 0),
        EventBody::PitchBend { channel, value, .. } => {
            let (lsb, msb) = pitch_bend_to_bytes(value);
            (0xE0 | (channel & 0x0F), lsb, msb)
        }
        EventBody::ProgramChange {
            channel, program, ..
        } => (0xC0 | (channel & 0x0F), program, 0),
        _ => return None,
    };
    Some(Vst3MidiEvent {
        sample_offset: event.sample_offset,
        status,
        data1,
        data2,
        port: event.port,
        note_id: -1,
        ne_value: 0.0,
    })
}

/// VST3 has no native CC / pitch-bend / channel-pressure / program
/// input event. Hosts route those MIDI messages to a parameter the
/// plugin advertises through `IMidiMapping` (see
/// `cb_midi_mapping_get_param_id`) and deliver them as parameter
/// changes. When `info` carries such a binding, turn the parameter
/// change back into the MIDI event the plugin expects, so event-based
/// plugins behave the same here as on AU / CLAP / LV2 (which hand the
/// plugin raw MIDI). Returns `None` for unmapped parameters - the
/// caller still emits the plain `ParamChange`.
///
/// `plain` is the denormalized value the shim already produced; we
/// re-normalize through the parameter's range so the MIDI-domain
/// mapping is independent of how the binding parameter declares its
/// range.
//
// `norm as f32` is a lossless-enough narrowing of a clamped `0..=1`
// value; the MIDI encoders take `f32`.
#[allow(clippy::cast_possible_truncation)]
/// A parameter's precomputed MIDI-controller binding. Built once per
/// instance for every param that declares a `midi_map`, so the audio
/// thread can bridge a mapped controller change to its `EventBody`
/// through a binary search instead of a linear `ParamInfo` scan.
#[derive(Clone, Copy)]
struct MidiMap {
    source: MidiSource,
    channel: u8,
    range: ParamRange,
}

impl MidiMap {
    /// The binding `info` declares, or `None` when it has no `midi_map`.
    fn from_param(info: &ParamInfo) -> Option<Self> {
        Some(Self {
            source: info.midi_map?,
            channel: info.midi_channel.unwrap_or(0),
            range: info.range,
        })
    }
}

/// Bridge a MIDI-mapped parameter change back into the `EventBody` the
/// plugin expects. VST3 has no native input event for channel MIDI, so
/// the host delivers it as a parameter change on the mapped id.
// `normalize` yields a `0.0..=1.0` value; the MIDI encoders take `f32`.
#[allow(clippy::cast_possible_truncation)]
fn midi_event_from_map(map: &MidiMap, plain: f64) -> EventBody {
    let channel = map.channel;
    let norm = map.range.normalize(plain) as f32; // 0.0..=1.0
    match map.source {
        // Host-normalized `0..1` is the pitch-wheel position (0 = full
        // down, 0.5 = center, 1 = full up); shift to `[-1, 1]` for the
        // 14-bit encoder.
        MidiSource::PitchBend => EventBody::PitchBend {
            group: 0,
            channel,
            value: denorm_pitch_bend(norm * 2.0 - 1.0),
        },
        MidiSource::Cc(cc) => EventBody::ControlChange {
            group: 0,
            channel,
            cc,
            value: denorm_7bit(norm),
        },
        MidiSource::ChannelPressure => EventBody::ChannelPressure {
            group: 0,
            channel,
            pressure: denorm_7bit(norm),
        },
        MidiSource::ProgramChange => EventBody::ProgramChange {
            group: 0,
            channel,
            program: denorm_7bit(norm),
        },
    }
}

// ---------------------------------------------------------------------------
// MIDI input proxy parameters
//
// VST3 has no input events for channel-level MIDI; hosts deliver CC /
// pitch bend / channel pressure only to a parameter advertised through
// `IMidiMapping`. Explicit `midi_map` bindings cover parameters the
// plugin *wants* as parameters; these hidden proxies cover everything
// else, so event-consuming plugins hear the same MIDI on VST3 as on
// AU / CLAP / LV2 / VST2. Proxies are not real parameters: never
// serialized into state, never visible to `Params`, only an
// `IMidiMapping` target that turns back into the matching `EventBody`.
// ---------------------------------------------------------------------------

/// Controllers per channel: CC 0..=127, 128 = channel pressure,
/// 129 = pitch bend. Program change (VST3 controller 130) is
/// deliberately not proxied - `kIsProgramChange` parameters interact
/// with unit/program-list metadata, so it stays explicit-binding-only.
const MIDI_PROXY_PER_CHANNEL: u32 = 130;
/// One event-input bus's worth of proxies (16 channels). A plugin
/// gets one bank per declared MIDI input port, so controllers keep
/// their bus attribution - the host queries `IMidiMapping` per bus
/// and a shared id would merge every bus's values into one parameter
/// queue before moose ever saw them.
const MIDI_PROXY_BANK: u32 = 16 * MIDI_PROXY_PER_CHANNEL;
const MIDI_PROXY_PRESSURE: u32 = 128;
const MIDI_PROXY_PITCH_BEND: u32 = 129;

/// Allocate every hidden proxy from the top of the VST3-safe 31-bit
/// domain, skipping real parameter IDs without changing them. The
/// descending order is stable for a given real-ID set and doubles as
/// the logical proxy index for allocation-free binary-search decoding.
fn allocate_midi_proxy_ids(real_params: &[ParamInfo], count: usize) -> Vec<u32> {
    let real_ids: HashSet<u32> = real_params.iter().map(|info| info.id).collect();
    let mut ids = Vec::with_capacity(count);
    let mut candidate = Some(moose_params::PARAM_ID_MAX);
    while ids.len() < count {
        let id = candidate.expect("VST3 parameter ID domain exhausted by MIDI proxies");
        candidate = id.checked_sub(1);
        if !real_ids.contains(&id) {
            ids.push(id);
        }
    }
    ids
}

fn allocated_midi_proxy_id(ids: &[u32], port: u8, channel: u8, controller: u32) -> Option<u32> {
    let index = u32::from(port) * MIDI_PROXY_BANK
        + u32::from(channel.min(15)) * MIDI_PROXY_PER_CHANNEL
        + controller;
    ids.get(index as usize).copied()
}

fn allocated_midi_proxy_index(ids: &[u32], id: u32) -> Option<usize> {
    ids.binary_search_by(|candidate| candidate.cmp(&id).reverse())
        .ok()
}

/// `(port, channel, controller)` for an allocated proxy id, `None`
/// for every real parameter ID and other host input.
fn allocated_midi_proxy_decode(ids: &[u32], id: u32) -> Option<(u8, u8, u32)> {
    let index = u32::try_from(allocated_midi_proxy_index(ids, id)?).ok()?;
    #[allow(clippy::cast_possible_truncation)]
    Some((
        (index / MIDI_PROXY_BANK) as u8,
        (index % MIDI_PROXY_BANK / MIDI_PROXY_PER_CHANNEL) as u8,
        index % MIDI_PROXY_PER_CHANNEL,
    ))
}

// Keep the historical arithmetic available to the existing unit
// checks; production proxy IDs use the collision-safe allocator above.
#[cfg(test)]
const MIDI_PROXY_ID_BASE: u32 = moose_params::METER_ID_BASE + 0x1_0000;

#[cfg(test)]
fn midi_proxy_id(port: u8, channel: u8, controller: u32) -> u32 {
    MIDI_PROXY_ID_BASE
        + u32::from(port) * MIDI_PROXY_BANK
        + u32::from(channel.min(15)) * MIDI_PROXY_PER_CHANNEL
        + controller
}

#[cfg(test)]
fn midi_proxy_decode(id: u32) -> Option<(u8, u8, u32)> {
    let rel = id.checked_sub(MIDI_PROXY_ID_BASE)?;
    if rel >= 256 * MIDI_PROXY_BANK {
        return None;
    }
    #[allow(clippy::cast_possible_truncation)]
    Some((
        (rel / MIDI_PROXY_BANK) as u8,
        (rel % MIDI_PROXY_BANK / MIDI_PROXY_PER_CHANNEL) as u8,
        rel % MIDI_PROXY_PER_CHANNEL,
    ))
}

/// Wheel-centred default for pitch bend, zero for everything else.
fn midi_proxy_default(controller: u32) -> f64 {
    if controller == MIDI_PROXY_PITCH_BEND {
        0.5
    } else {
        0.0
    }
}

/// The MIDI event a proxy change decodes to. `normalized` is the
/// host's `0..=1` wheel/controller position (the shim's denormalize is
/// identity for proxy ids, so the plain value passes through).
fn midi_proxy_event(channel: u8, controller: u32, normalized: f32) -> EventBody {
    match controller {
        MIDI_PROXY_PITCH_BEND => EventBody::PitchBend {
            group: 0,
            channel,
            value: denorm_pitch_bend(normalized * 2.0 - 1.0),
        },
        MIDI_PROXY_PRESSURE => EventBody::ChannelPressure {
            group: 0,
            channel,
            pressure: denorm_7bit(normalized),
        },
        cc => EventBody::ControlChange {
            group: 0,
            channel,
            // Bounded to 0..=127 by `midi_proxy_decode`.
            cc: u8::try_from(cc).unwrap_or(0) & 0x7F,
            value: denorm_7bit(normalized),
        },
    }
}

/// Proxy count for this plugin: the full bank when it accepts MIDI
/// input, zero otherwise (no surface change for non-MIDI plugins).
fn midi_proxy_len<P: PluginExport>() -> usize {
    if P::info().accepts_midi_in {
        MIDI_PROXY_BANK as usize * usize::from(P::info().midi_input_ports)
    } else {
        0
    }
}

enum Vst3EncodeResult {
    Emitted(Vst3NativeEvent, PendingOutputMutation),
    Unsupported,
    Invalid,
}

unsafe extern "C" fn cb_begin_output_events<P: PluginExport>(ctx: *mut std::ffi::c_void) {
    unsafe {
        let inst = &*ctx.cast::<Vst3Instance<P>>();
        let mut audio = inst.audio.enter();
        audio.output_cursor = LosslessEventCursor::default();
        audio.output_preflight_status = if audio.output_events.overflow().is_some() {
            VST3_EVENT_QUEUE_FULL
        } else {
            let mut note_ids = audio.output_note_ids;
            audio
                .output_events
                .lossless_iter()
                .find_map(|event| {
                    if let LosslessEventRef::Typed(Event {
                        sample_offset,
                        body: EventBody::ParamChange { id, value },
                        ..
                    }) = event
                    {
                        let valid = *sample_offset < audio.input_num_frames
                            && value.is_finite()
                            && inst.param_infos.iter().any(|info| info.id == *id);
                        return (!valid).then_some(VST3_EVENT_INVALID);
                    }
                    match encode_vst3_output_event::<P>(
                        event,
                        &audio.output_events,
                        &note_ids,
                        audio.input_num_frames,
                    ) {
                        Vst3EncodeResult::Emitted(_, mutation) => {
                            note_ids.commit(mutation);
                            None
                        }
                        Vst3EncodeResult::Unsupported => Some(VST3_EVENT_UNSUPPORTED),
                        Vst3EncodeResult::Invalid => Some(VST3_EVENT_INVALID),
                    }
                })
                .unwrap_or(VST3_EVENT_END)
        };
        audio.pending_output_mutation = PendingOutputMutation::None;
    }
}

unsafe extern "C" fn cb_next_output_event<P: PluginExport>(
    ctx: *mut std::ffi::c_void,
    out: *mut Vst3NativeEvent,
) -> u32 {
    unsafe {
        if out.is_null() {
            return VST3_EVENT_INVALID;
        }
        let inst = &*ctx.cast::<Vst3Instance<P>>();
        let mut audio = inst.audio.enter();
        audio.pending_output_mutation = PendingOutputMutation::None;
        let scr = &mut *audio;
        if scr.output_preflight_status != VST3_EVENT_END {
            return std::mem::replace(&mut scr.output_preflight_status, VST3_EVENT_END);
        }
        let event = loop {
            let Some(event) = scr.output_events.lossless_next(&mut scr.output_cursor) else {
                return VST3_EVENT_END;
            };
            if matches!(
                event,
                LosslessEventRef::Typed(Event {
                    body: EventBody::ParamChange { .. },
                    ..
                })
            ) {
                continue;
            }
            break event;
        };
        match encode_vst3_output_event::<P>(
            event,
            &scr.output_events,
            &scr.output_note_ids,
            scr.input_num_frames,
        ) {
            Vst3EncodeResult::Emitted(native, mutation) => {
                scr.pending_output_mutation = mutation;
                out.write(native);
                VST3_EVENT_EMITTED
            }
            Vst3EncodeResult::Unsupported => VST3_EVENT_UNSUPPORTED,
            Vst3EncodeResult::Invalid => VST3_EVENT_INVALID,
        }
    }
}

unsafe extern "C" fn cb_finish_output_events<P: PluginExport>(
    ctx: *mut std::ffi::c_void,
    status: u32,
) {
    unsafe {
        let inst = &*ctx.cast::<Vst3Instance<P>>();
        let mut audio = inst.audio.enter();
        let status = audio.output_events.overflow().map_or_else(
            || match status {
                VST3_EVENT_END | VST3_EVENT_EMITTED => OutputEventStatus::Success,
                VST3_EVENT_UNSUPPORTED => OutputEventStatus::Unsupported,
                VST3_EVENT_QUEUE_FULL => OutputEventStatus::HostQueueFull,
                _ => OutputEventStatus::Invalid,
            },
            OutputEventStatus::BufferFull,
        );
        audio.output_events.set_output_status(status);
    }
}

unsafe extern "C" fn cb_commit_output_event<P: PluginExport>(ctx: *mut std::ffi::c_void) {
    unsafe {
        let inst = &*ctx.cast::<Vst3Instance<P>>();
        let mut audio = inst.audio.enter();
        let mutation = std::mem::replace(
            &mut audio.pending_output_mutation,
            PendingOutputMutation::None,
        );
        audio.output_note_ids.commit(mutation);
    }
}

fn encode_vst3_output_event<P: PluginExport>(
    event: LosslessEventRef<'_>,
    events: &EventList,
    note_ids: &OutputNoteIds,
    num_frames: u32,
) -> Vst3EncodeResult {
    let sample_offset = match event {
        LosslessEventRef::Typed(event) => event.sample_offset,
        LosslessEventRef::Exact(event) => event.sample_offset(),
    };
    if sample_offset >= num_frames {
        return Vst3EncodeResult::Invalid;
    }

    match event {
        LosslessEventRef::Typed(event) => encode_typed_vst3_output::<P>(event, events, note_ids),
        LosslessEventRef::Exact(event) => encode_exact_vst3_output::<P>(event, note_ids),
    }
}

#[allow(clippy::float_cmp)]
fn encode_exact_vst3_output<P: PluginExport>(
    exact: moose_core::ExactEventRef<'_>,
    note_ids: &OutputNoteIds,
) -> Vst3EncodeResult {
    let (flags, ppq_position) = match exact.metadata() {
        ExactEventMetadata::None => (
            u32::from(exact.qualifiers().is_live) * VST3_EVENT_IS_LIVE,
            0.0,
        ),
        ExactEventMetadata::Vst3(metadata) if metadata.ppq_position().is_finite() => {
            (u32::from(metadata.raw_flags()), metadata.ppq_position())
        }
        ExactEventMetadata::Vst3(_) => return Vst3EncodeResult::Invalid,
        _ => return Vst3EncodeResult::Unsupported,
    };
    let encoded = match *exact.body() {
        ExactEventBody::DetailedNote {
            kind,
            address,
            velocity,
            tuning,
            length,
        } => encode_detailed_note::<P>(
            exact.sample_offset(),
            flags,
            kind,
            address,
            velocity,
            tuning,
            length,
            note_ids,
        ),
        ExactEventBody::Note {
            kind,
            address,
            velocity,
        } => {
            #[allow(clippy::cast_possible_truncation)]
            let native_velocity = velocity as f32;
            if f64::from(native_velocity) != velocity {
                return Vst3EncodeResult::Invalid;
            }
            encode_detailed_note::<P>(
                exact.sample_offset(),
                flags,
                kind,
                address,
                native_velocity,
                0.0,
                (kind == ExactNoteKind::On).then_some(0),
                note_ids,
            )
        }
        ExactEventBody::DetailedPolyPressure { address, pressure } => {
            let Some((bus, channel, pitch, source)) = concrete_output_address::<P>(address) else {
                return Vst3EncodeResult::Invalid;
            };
            if !valid_unit_f32(pressure) {
                return Vst3EncodeResult::Invalid;
            }
            let note_id = match source {
                OutputSourceIdentity::Exact { .. } => {
                    let Some(note_id) =
                        note_ids.note_id_for_expression(source, bus, channel, pitch)
                    else {
                        return Vst3EncodeResult::Unsupported;
                    };
                    note_id
                }
                OutputSourceIdentity::Anonymous => -1,
            };
            Vst3EncodeResult::Emitted(
                Vst3NativeEvent {
                    kind: VST3_EVENT_POLY_PRESSURE,
                    sample_offset: i32::try_from(exact.sample_offset()).unwrap_or(i32::MAX),
                    bus_index: i32::from(bus),
                    flags,
                    channel: i16::from(channel),
                    pitch: i16::from(pitch),
                    note_id,
                    velocity: pressure,
                    ..Vst3NativeEvent::default()
                },
                PendingOutputMutation::None,
            )
        }
        ExactEventBody::NormalizedNoteExpression {
            expression_id,
            address,
            value,
        } => {
            if !valid_unit_f64(value) {
                return Vst3EncodeResult::Invalid;
            }
            let Some((bus, source)) = output_expression_address::<P>(address) else {
                return Vst3EncodeResult::Invalid;
            };
            let Some(note_id) = note_ids.note_id_for_expression(source, bus, 0, 0) else {
                return Vst3EncodeResult::Unsupported;
            };
            Vst3EncodeResult::Emitted(
                Vst3NativeEvent {
                    kind: VST3_EVENT_NOTE_EXPRESSION,
                    sample_offset: i32::try_from(exact.sample_offset()).unwrap_or(i32::MAX),
                    bus_index: i32::from(bus),
                    flags,
                    note_id,
                    type_id: expression_id,
                    value,
                    ..Vst3NativeEvent::default()
                },
                PendingOutputMutation::None,
            )
        }
        ExactEventBody::SysEx { port } => {
            let Ok(bus) = u8::try_from(port) else {
                return Vst3EncodeResult::Invalid;
            };
            if bus >= P::info().midi_output_ports {
                return Vst3EncodeResult::Invalid;
            }
            let Some(bytes) = exact.sysex_bytes_checked() else {
                return Vst3EncodeResult::Invalid;
            };
            Vst3EncodeResult::Emitted(
                sysex_native_event(exact.sample_offset(), bus, flags, bytes),
                PendingOutputMutation::None,
            )
        }
        // Raw MIDI 1.0 and UMP, legacy note expression, and every exact
        // variant without a native VST3 representation stay unsupported.
        _ => Vst3EncodeResult::Unsupported,
    };
    match encoded {
        Vst3EncodeResult::Emitted(mut native, mutation) => {
            native.flags = flags;
            native.ppq_position = ppq_position;
            Vst3EncodeResult::Emitted(native, mutation)
        }
        other => other,
    }
}

#[allow(clippy::too_many_arguments)]
fn encode_detailed_note<P: PluginExport>(
    sample_offset: u32,
    flags: u32,
    kind: ExactNoteKind,
    address: ExactNoteAddress,
    velocity: f32,
    tuning: f32,
    length: Option<i32>,
    note_ids: &OutputNoteIds,
) -> Vst3EncodeResult {
    let Some((bus, channel, pitch, source)) = concrete_output_address::<P>(address) else {
        return Vst3EncodeResult::Invalid;
    };
    if !valid_unit_f32(velocity) || !tuning.is_finite() {
        return Vst3EncodeResult::Invalid;
    }
    let (kind, length, note_id, mutation) = match kind {
        ExactNoteKind::On => {
            let Some(length) = length.filter(|length| *length >= 0) else {
                return Vst3EncodeResult::Invalid;
            };
            let Some((note_id, mutation)) = note_ids.propose_note_on(source, bus, channel, pitch)
            else {
                return Vst3EncodeResult::Unsupported;
            };
            (VST3_EVENT_NOTE_ON, length, note_id, mutation)
        }
        ExactNoteKind::Off => {
            if length.is_some() {
                return Vst3EncodeResult::Invalid;
            }
            let Some((note_id, mutation)) = note_ids.propose_note_off(source, bus, channel, pitch)
            else {
                return Vst3EncodeResult::Unsupported;
            };
            (VST3_EVENT_NOTE_OFF, 0, note_id, mutation)
        }
        // Choke, End, and future note kinds have no native VST3 event.
        _ => return Vst3EncodeResult::Unsupported,
    };
    Vst3EncodeResult::Emitted(
        Vst3NativeEvent {
            kind,
            sample_offset: i32::try_from(sample_offset).unwrap_or(i32::MAX),
            bus_index: i32::from(bus),
            flags,
            channel: i16::from(channel),
            pitch: i16::from(pitch),
            note_id,
            length,
            velocity,
            tuning,
            ..Vst3NativeEvent::default()
        },
        mutation,
    )
}

fn concrete_output_address<P: PluginExport>(
    address: ExactNoteAddress,
) -> Option<(u8, u8, u8, OutputSourceIdentity)> {
    let bus = u8::try_from(address.port.value()?).ok()?;
    if bus >= P::info().midi_output_ports {
        return None;
    }
    let channel = address.channel.value()?;
    let pitch = address.key.value()?;
    if channel >= 16 || pitch >= 128 {
        return None;
    }
    let source = match address.note_id {
        ExactAddress::Wildcard => OutputSourceIdentity::Anonymous,
        ExactAddress::Value(note_id) => OutputSourceIdentity::Exact {
            bus: u16::from(bus),
            note_id,
        },
        ExactAddress::InvalidRaw(_) => return None,
    };
    Some((bus, channel, pitch, source))
}

fn output_expression_address<P: PluginExport>(
    address: ExactNoteAddress,
) -> Option<(u8, OutputSourceIdentity)> {
    let bus = u8::try_from(address.port.value()?).ok()?;
    if bus >= P::info().midi_output_ports {
        return None;
    }
    let note_id = address.note_id.value()?;
    Some((
        bus,
        OutputSourceIdentity::Exact {
            bus: u16::from(bus),
            note_id,
        },
    ))
}

fn encode_typed_vst3_output<P: PluginExport>(
    event: &Event,
    events: &EventList,
    note_ids: &OutputNoteIds,
) -> Vst3EncodeResult {
    if event.port >= P::info().midi_output_ports {
        return Vst3EncodeResult::Invalid;
    }
    let bus = event.port;
    let mut native = Vst3NativeEvent {
        sample_offset: i32::try_from(event.sample_offset).unwrap_or(i32::MAX),
        bus_index: i32::from(bus),
        ..Vst3NativeEvent::default()
    };
    let mutation = match event.body {
        EventBody::NoteOn {
            group: 0,
            channel,
            note,
            velocity,
        } if channel < 16 && note < 128 && velocity < 128 => {
            let Some((note_id, mutation)) =
                note_ids.propose_note_on(OutputSourceIdentity::Anonymous, bus, channel, note)
            else {
                return Vst3EncodeResult::Unsupported;
            };
            native.kind = VST3_EVENT_NOTE_ON;
            native.channel = i16::from(channel);
            native.pitch = i16::from(note);
            native.note_id = note_id;
            native.velocity = f32::from(velocity) / 127.0;
            mutation
        }
        EventBody::NoteOff {
            group: 0,
            channel,
            note,
            velocity,
        } if channel < 16 && note < 128 && velocity < 128 => {
            let Some((note_id, mutation)) =
                note_ids.propose_note_off(OutputSourceIdentity::Anonymous, bus, channel, note)
            else {
                return Vst3EncodeResult::Unsupported;
            };
            native.kind = VST3_EVENT_NOTE_OFF;
            native.channel = i16::from(channel);
            native.pitch = i16::from(note);
            native.note_id = note_id;
            native.velocity = f32::from(velocity) / 127.0;
            mutation
        }
        EventBody::Aftertouch {
            group: 0,
            channel,
            note,
            pressure,
        } if channel < 16 && note < 128 && pressure < 128 => {
            native.kind = VST3_EVENT_POLY_PRESSURE;
            native.channel = i16::from(channel);
            native.pitch = i16::from(note);
            native.note_id = note_ids
                .note_id_for_expression(OutputSourceIdentity::Anonymous, bus, channel, note)
                .unwrap_or(-1);
            native.velocity = f32::from(pressure) / 127.0;
            PendingOutputMutation::None
        }
        EventBody::PerNotePitchBend {
            group: 0,
            channel,
            note,
            value,
        } if channel < 16 && note < 128 => {
            let Some(note_id) = note_ids.note_id_for_expression(
                OutputSourceIdentity::Anonymous,
                bus,
                channel,
                note,
            ) else {
                return Vst3EncodeResult::Unsupported;
            };
            native.kind = VST3_EVENT_NOTE_EXPRESSION;
            native.note_id = note_id;
            native.type_id = 2;
            native.value = wire_to_vst3_tuning(value);
            PendingOutputMutation::None
        }
        EventBody::ControlChange {
            group: 0,
            channel,
            cc,
            value,
        } if channel < 16 && cc < 128 && value < 128 => {
            native.kind = VST3_EVENT_LEGACY_MIDI_CC_OUT;
            native.channel = i16::from(channel);
            native.type_id = u32::from(cc);
            native.data1 = value;
            PendingOutputMutation::None
        }
        EventBody::ChannelPressure {
            group: 0,
            channel,
            pressure,
        } if channel < 16 && pressure < 128 => {
            native.kind = VST3_EVENT_LEGACY_MIDI_CC_OUT;
            native.channel = i16::from(channel);
            native.type_id = 128;
            native.data1 = pressure;
            PendingOutputMutation::None
        }
        EventBody::PitchBend {
            group: 0,
            channel,
            value,
        } if channel < 16 && value < 16_384 => {
            let (lsb, msb) = pitch_bend_to_bytes(value);
            native.kind = VST3_EVENT_LEGACY_MIDI_CC_OUT;
            native.channel = i16::from(channel);
            native.type_id = 129;
            native.data1 = lsb;
            native.data2 = msb;
            PendingOutputMutation::None
        }
        EventBody::ProgramChange {
            group: 0,
            channel,
            program,
        } if channel < 16 && program < 128 => {
            native.kind = VST3_EVENT_LEGACY_MIDI_CC_OUT;
            native.channel = i16::from(channel);
            native.type_id = 130;
            native.data1 = program;
            PendingOutputMutation::None
        }
        EventBody::SysEx { .. } => {
            let Some(bytes) = events.sysex_bytes_checked(&event.body) else {
                return Vst3EncodeResult::Invalid;
            };
            return Vst3EncodeResult::Emitted(
                sysex_native_event(event.sample_offset, bus, 0, bytes),
                PendingOutputMutation::None,
            );
        }
        _ => return Vst3EncodeResult::Unsupported,
    };
    Vst3EncodeResult::Emitted(native, mutation)
}

fn sysex_native_event(sample_offset: u32, bus: u8, flags: u32, bytes: &[u8]) -> Vst3NativeEvent {
    Vst3NativeEvent {
        kind: VST3_EVENT_DATA,
        sample_offset: i32::try_from(sample_offset).unwrap_or(i32::MAX),
        bus_index: i32::from(bus),
        flags,
        bytes: bytes.as_ptr(),
        len: len_u32(bytes.len()),
        ..Vst3NativeEvent::default()
    }
}

/// Map a moose per-note MIDI 2.0 event to a VST3 note-expression tuple
/// `(type_id, note_id, value)`. VST3 has no UMP; per-note richness rides
/// `INoteExpressionController` value events keyed by `note_id`. We key
/// `note_id` deterministically as `(channel << 7) | note` - the shim
/// stamps the plugin's `NoteOn` with the same id, so notes and their
/// expression correlate without any per-instance tracking state. Returns
/// `None` for per-note controllers VST3 has no predefined type for; the
/// value is normalized `0..=1` (VST3's `NoteExpressionValue` domain).
#[cfg(test)]
fn note_expression_of(body: &EventBody) -> Option<(u32, i32, f64)> {
    // Predefined VST3 NoteExpressionTypeIDs (reverse of the input map):
    // Volume=0, Pan=1, Tuning=2, Vibrato=3, Expression=4, Brightness=5.
    let (type_id, channel, note, value) = match *body {
        // Registered per-note controllers only: the predefined VST3
        // expression types carry the registered indices' semantics;
        // an assignable index is manufacturer-defined and must not
        // alias onto them.
        EventBody::PerNoteCC {
            channel,
            note,
            cc,
            value,
            registered: true,
            ..
        } => {
            let type_id = match cc {
                7 => 0,
                10 => 1,
                1 => 3,
                11 => 4,
                74 => 5,
                _ => return None,
            };
            (type_id, channel, note, u32_to_unit(value))
        }
        EventBody::PerNotePitchBend {
            channel,
            note,
            value,
            ..
        } => (2, channel, note, wire_to_vst3_tuning(value)),
        _ => return None,
    };
    Some((type_id, vst3_note_id(channel, note), value))
}

/// Deterministic VST3 `noteId` for a moose note: `(channel << 7) | note`.
/// The C++ shim stamps every emitted note-on/off with the same formula,
/// so a plug-in's note-expression events address the live note without
/// any shared correlation state.
#[cfg(test)]
fn vst3_note_id(channel: u8, note: u8) -> i32 {
    (i32::from(channel & 0x0F) << 7) | i32::from(note & 0x7F)
}

/// Normalize a wire-native 32-bit per-note value into VST3's `0..=1`
/// `NoteExpressionValue` domain.
#[cfg(test)]
fn u32_to_unit(v: u32) -> f64 {
    f64::from(v) / f64::from(u32::MAX)
}

/// VST3's tuning note-expression span: normalized `0..=1` covers
/// `-120..=+120` semitones (`plain = 240 * (norm - 0.5)` per the SDK).
const VST3_TUNING_SPAN_SEMITONES: f64 = 240.0;

/// VST3 tuning norm (`0..=1`, ±120 st) -> wire per-note bend. The wire
/// full-scale is ±48 st, so a wider host bend saturates.
#[cfg(test)]
fn vst3_tuning_to_wire(norm: f64) -> u32 {
    per_note_bend_from_semitones((norm - 0.5) * VST3_TUNING_SPAN_SEMITONES)
}

/// Wire per-note bend (±48 st full-scale) -> VST3 tuning norm
/// (`0..=1`, ±120 st), so the same event bends identically on every
/// semitone-denominated host domain.
fn wire_to_vst3_tuning(v: u32) -> f64 {
    0.5 + per_note_bend_semitones(v) / VST3_TUNING_SPAN_SEMITONES
}

/// Inverse of [`u32_to_unit`]: widen a VST3 `NoteExpressionValue` into
/// the wire-native 32-bit per-note domain. Hosts are supposed to stay
/// in `0..=1`, but the value crosses an FFI boundary - clamp first.
#[cfg(test)]
fn unit_to_u32(v: f64) -> u32 {
    // Clamped to `0..=u32::MAX` before the cast, so no truncation or
    // sign loss is possible.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let scaled = (v.clamp(0.0, 1.0) * f64::from(u32::MAX)).round() as u32;
    scaled
}

unsafe extern "C" fn cb_get_output_param_count<P: PluginExport>(ctx: *mut std::ffi::c_void) -> u32 {
    unsafe {
        let inst = &*ctx.cast::<Vst3Instance<P>>();
        let audio = inst.audio.enter();
        len_u32(
            audio
                .output_events
                .iter()
                .filter(|e| matches!(e.body, EventBody::ParamChange { .. }))
                .count(),
        )
    }
}

unsafe extern "C" fn cb_get_output_param<P: PluginExport>(
    ctx: *mut std::ffi::c_void,
    index: u32,
    out_id: *mut u32,
    out_sample_offset: *mut i32,
    out_value: *mut f64,
) {
    unsafe {
        let inst = &*ctx.cast::<Vst3Instance<P>>();
        let audio = inst.audio.enter();
        if let Some(event) = audio
            .output_events
            .iter()
            .filter(|e| matches!(e.body, EventBody::ParamChange { .. }))
            .nth(index as usize)
            && let EventBody::ParamChange { id, value } = event.body
        {
            // VST3 output param queues carry normalized values; the
            // plugin emits plain. Fall back to the plain value if the
            // id has no descriptor (shouldn't happen for real params).
            let normalized = inst
                .param_infos
                .iter()
                .find(|i| i.id == id)
                .map_or(value, |i| i.range.normalize(value));
            *out_id = id;
            *out_sample_offset = i32::try_from(event.sample_offset).unwrap_or(0);
            *out_value = normalized;
        }
    }
}

// ---------------------------------------------------------------------------
// GUI callbacks
// ---------------------------------------------------------------------------

unsafe extern "C" fn cb_gui_has_editor<P: PluginExport>(ctx: *mut std::ffi::c_void) -> i32 {
    // The editor builder is author code; firewall its lazy construction so
    // a panic there can't unwind across the C ABI (0 = "no editor").
    run_extern_callback_with::<P, i32>("vst3", "gui_has_editor", 0, || unsafe {
        if ctx.is_null() {
            return 0;
        }
        let inst = &*ctx.cast::<Vst3Instance<P>>();
        let mut gui = inst.gui.enter();
        if gui.editor.is_none() {
            // Built from the lock-free param store the wrapper already
            // holds outside the plugin, so opening the GUI never
            // stalls the audio thread.
            gui.editor = (inst.editor_builder)(inst.params_arc.clone());
            // Replay a content scale the host reported before the editor
            // existed (a valid VST3 ordering - `setContentScaleFactor`
            // can precede the editor object). Only a scale the host really
            // sent: replaying a default would pin the editor to it instead
            // of the OS scale. macOS never stores one (AppKit drives Retina).
            if let (Some(scale), Some(editor)) = (inst.reported_host_scale(), gui.editor.as_mut()) {
                editor.set_scale_factor(scale);
            }
        }
        i32::from(gui.editor.is_some())
    })
}

unsafe extern "C" fn cb_gui_get_size<P: PluginExport>(
    ctx: *mut std::ffi::c_void,
    w: *mut u32,
    h: *mut u32,
) {
    // `Editor::size` is author code; firewall it so a panic can't unwind
    // across the C ABI.
    run_extern_callback_with::<P, ()>("vst3", "gui_get_size", (), || unsafe {
        let inst = &*ctx.cast::<Vst3Instance<P>>();
        // A host can query the size synchronously while an outer GUI callback
        // holds the cell (mid-resize). If so, report the size the plugin just
        // requested through `request_resize` (still pending), leaving the
        // host's value untouched when there is none - never re-enter the cell.
        let Some(mut gui) = inst.gui.try_enter() else {
            let packed = inst.pending_resize.load(Ordering::Relaxed);
            if packed != 0 {
                #[allow(clippy::cast_possible_truncation)]
                {
                    *w = (packed >> 32) as u32;
                    *h = packed as u32;
                }
            }
            return;
        };
        // Apply a resize the plugin requested re-entrantly (stashed by
        // `cb_gui_set_size` because the cell was busy) before reporting.
        let packed = inst.pending_resize.swap(0, Ordering::Relaxed);
        if let Some(editor) = gui.editor.as_deref() {
            inst.note_window_scale(editor);
        }
        if packed != 0
            && let Some(editor) = gui.editor.as_mut()
        {
            #[allow(clippy::cast_possible_truncation)]
            let (pw, ph) = ((packed >> 32) as u32, packed as u32);
            apply_physical_resize(editor.as_mut(), pw, ph, inst.host_scale());
        }
        if let Some(ref editor) = gui.editor {
            let (ew, eh) = editor.size();
            // VST3 `ViewRect` is documented as "in pixels". That's literally
            // true on Windows/Linux, where hosts expect physical pixels and
            // may drive the scale via `IPlugViewContentScaleSupport`. On
            // macOS, AppKit handles the Retina backing automatically and
            // hosts expect logical points - scaling here would double the
            // window on Retina displays.
            #[cfg(target_os = "macos")]
            {
                *w = ew;
                *h = eh;
            }
            #[cfg(not(target_os = "macos"))]
            {
                // Round-to-nearest, not truncate - `(w * scale) as u32`
                // would round 199.9 → 199, drifting one pixel on
                // fractional scales. Matches the CLAP / AAX / `to_physical_px`
                // helper used elsewhere. Logical pixel sizes are bounded
                // by `u32::MAX / scale`; in practice no editor exceeds
                // 16384 logical pixels, so the `f64 → u32` truncation
                // and sign casts are safe.
                #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                {
                    let host_scale = inst.host_scale();
                    *w = (f64::from(ew) * host_scale).round() as u32;
                    *h = (f64::from(eh) * host_scale).round() as u32;
                }
            }
        }
    });
}

unsafe extern "C" fn cb_gui_set_content_scale<P: PluginExport>(
    ctx: *mut std::ffi::c_void,
    scale: f64,
) {
    // `Editor::set_scale_factor` is author code; firewall it.
    run_extern_callback_with::<P, ()>("vst3", "gui_set_content_scale", (), || unsafe {
        // macOS: `ViewRect` is in logical points and AppKit applies the
        // backing scale; a stored scale would only mis-convert host sizes.
        if cfg!(target_os = "macos") || ctx.is_null() || !scale.is_finite() || scale <= 0.0 {
            return;
        }
        // Clamp to the same range the GUI cluster's `EditorScale`
        // cell uses. A buggy host passing `f64::MAX` would otherwise
        // propagate to the editor and overflow when the editor
        // multiplies its logical size to physical pixels.
        let scale = scale.clamp(0.25, 8.0);
        let inst = &*ctx.cast::<Vst3Instance<P>>();
        let changed = inst.reported_host_scale() != Some(scale);
        inst.set_host_scale(scale);
        // `try_enter`, not `enter`: a host can deliver `setContentScaleFactor`
        // synchronously while an outer GUI callback holds the cell - on
        // Windows, `editor.open` (held across `cb_gui_open`) pumps messages and
        // the host may re-enter here. Re-entering with `enter` would hand out a
        // second aliasing `&mut` in release. When busy, skip the editor call:
        // `host_scale` is already persisted above, and `cb_gui_open` re-syncs
        // the editor's scale from it once `open` returns.
        let Some(mut gui) = inst.gui.try_enter() else {
            return;
        };
        let Some(editor) = gui.editor.as_mut() else {
            return;
        };
        editor.set_scale_factor(scale);
        // The view's physical size follows the new scale; tell the host
        // (`IPlugFrame::resizeView`) so its frame matches the child. Not a
        // no-op re-send: skipped when the scale didn't change.
        let (lw, lh) = editor.size();
        drop(gui);
        if changed && lw > 0 && lh > 0 {
            let (pw, ph) = logical_to_phys(lw, lh, scale);
            ffi::moose_vst3_request_resize(ctx, pw, ph);
        }
    });
}

/// `IPlugView::canResize` callback. Returns 1 / 0 mapping to
/// `kResultOk` / `kResultFalse` on the shim side.
unsafe extern "C" fn cb_gui_can_resize<P: PluginExport>(ctx: *mut std::ffi::c_void) -> i32 {
    // `Editor::can_resize` is author code; firewall it (0 = "not resizable").
    run_extern_callback_with::<P, i32>("vst3", "gui_can_resize", 0, || unsafe {
        if ctx.is_null() {
            return 0;
        }
        let inst = &*ctx.cast::<Vst3Instance<P>>();
        i32::from(
            inst.gui
                .enter()
                .editor
                .as_ref()
                .is_some_and(|e| e.can_resize()),
        )
    })
}

/// `IPlugView::checkSizeConstraint` callback. Clamps the
/// requested physical width / height in place against the
/// editor's `min_size` / `max_size` / `aspect_ratio`. For
/// fixed-size editors snaps to the editor's current size (JUCE's
/// Ableton-Live workaround pattern).
unsafe extern "C" fn cb_gui_check_size_constraint<P: PluginExport>(
    ctx: *mut std::ffi::c_void,
    w: *mut u32,
    h: *mut u32,
) {
    // `Editor::can_resize` / `size` are author code; firewall them.
    run_extern_callback_with::<P, ()>("vst3", "gui_check_size_constraint", (), || unsafe {
        if ctx.is_null() || w.is_null() || h.is_null() {
            return;
        }
        let inst = &*ctx.cast::<Vst3Instance<P>>();
        // `try_enter`, not `enter`: a host answering a plugin `request_resize`
        // calls `resizeView` -> `checkSizeConstraint` (then `onSize`)
        // synchronously on this thread, re-entering the cell an outer editor
        // callback still holds. When busy, leave the host's requested `*w`/`*h`
        // unchanged (skip the constraint pass this frame) rather than aliasing -
        // matching the `cb_gui_set_size` / `cb_gui_get_size` siblings.
        let Some(gui) = inst.gui.try_enter() else {
            return;
        };
        let Some(ref editor) = gui.editor else {
            return;
        };
        inst.note_window_scale(editor.as_ref());
        let host_scale = inst.host_scale();
        if editor.can_resize() {
            // Physical -> logical, fit, logical -> physical. Fit the largest
            // on-ratio box *inside* the requested cursor box (never larger on
            // either axis). VST3 hosts drive the drag from the raw cursor and
            // re-assert it every frame, so any size we return that exceeds the
            // cursor (a single-edge "grow the other axis") is honoured for one
            // frame then bounced - the window judders. A size <= the cursor
            // is a fixed point the host converges on.
            let (lw, lh) = phys_to_logical(*w, *h, host_scale);
            let (fw, fh) = fit_logical_size(lw, lh, editor.as_ref());
            let (pw, ph) = logical_to_phys(fw, fh, host_scale);
            *w = pw;
            *h = ph;
        } else {
            // Snap to current size; host-side Live quirk handled
            // identically by JUCE.
            let (cw, ch) = editor.size();
            let (pw, ph) = logical_to_phys(cw, ch, host_scale);
            *w = pw;
            *h = ph;
        }
    });
}

/// `IPlugView::onSize` callback. Host committed a new size; delegate
/// to `Editor::set_size` after scaling physical -> logical. The editor
/// *fills* the committed window (min/max clamp only) rather than
/// re-fitting onto the aspect ratio - that shaping happened earlier in
/// `checkSizeConstraint`, the host's drag-negotiation point, and
/// flooring it again here would leave a 1px letterbox line at the
/// bottom. `onSize` must not request a resize: VST3 forbids
/// `IPlugFrame::resizeView` from inside `onSize`, and a reentrant call
/// judders the drag.
unsafe extern "C" fn cb_gui_set_size<P: PluginExport>(ctx: *mut std::ffi::c_void, w: u32, h: u32) {
    // `Editor::set_size` / `can_resize` are author code; firewall them.
    run_extern_callback_with::<P, ()>("vst3", "gui_set_size", (), || unsafe {
        if ctx.is_null() || w == 0 || h == 0 {
            return;
        }
        let inst = &*ctx.cast::<Vst3Instance<P>>();
        // A host answering a plugin `request_resize` (resizeView) synchronously
        // lands `onSize` here while an outer GUI callback still holds the cell.
        // `try_enter` avoids the same-thread aliasing re-entry: apply when free,
        // else stash for `cb_gui_get_size` to apply on the next size query.
        match inst.gui.try_enter() {
            Some(mut gui) => {
                if let Some(editor) = gui.editor.as_mut() {
                    inst.note_window_scale(editor.as_ref());
                    apply_physical_resize(editor.as_mut(), w, h, inst.host_scale());
                }
            }
            None => {
                inst.pending_resize
                    .store((u64::from(w) << 32) | u64::from(h), Ordering::Relaxed);
            }
        }
    });
}

/// `IMidiMapping::getMidiControllerAssignment` callback. Resolves the
/// host's controller query to a bound parameter or the instance's
/// collision-free hidden proxy ID.
//
// `controller as u8` is guarded by the `0..=127` match arm; `channel`
// goes through `try_from` so a negative never wraps.
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
unsafe extern "C" fn cb_midi_mapping_get_param_id<P: PluginExport>(
    ctx: *mut std::ffi::c_void,
    bus_index: i32,
    channel: i16,
    controller: i16,
    out_param_id: *mut u32,
) -> i32 {
    if ctx.is_null() {
        return 0;
    }
    let inst = unsafe { &*ctx.cast::<Vst3Instance<P>>() };
    // VST3 `ControllerNumbers`: 0..=127 are CCs; the extended values
    // mirror the output path's encoding (`ivstmidicontrollers.h`).
    let source = match controller {
        0..=127 => MidiSource::Cc(controller as u8),
        128 => MidiSource::ChannelPressure, // kAfterTouch
        129 => MidiSource::PitchBend,
        130 => MidiSource::ProgramChange, // kCtrlProgramChange
        _ => return 0,                    // kResultFalse
    };
    let channel = u8::try_from(channel).unwrap_or(0);
    // Returns a hit-flag (1/0); the shim maps it to kResultOk /
    // kResultFalse for the VST3 boundary. Explicit bindings win on
    // every bus - a bound parameter is one value, so per-port
    // separation doesn't apply to it. Everything unbound falls
    // through to the hidden proxy bank for the queried bus, keeping
    // controllers attributed per port (program change excepted -
    // not proxied).
    if let Some(id) = moose_params::map_source_to_param(&inst.param_infos, channel, source) {
        unsafe { out_param_id.write(id) };
        return 1;
    }
    let Ok(port) = u8::try_from(bus_index) else {
        return 0;
    };
    if P::info().accepts_midi_in
        && port < P::info().midi_input_ports
        && channel < 16
        && let Ok(controller) = u32::try_from(controller)
        && controller < MIDI_PROXY_PER_CHANNEL
        && let Some(id) = allocated_midi_proxy_id(&inst.midi_proxy_ids, port, channel, controller)
    {
        unsafe { out_param_id.write(id) };
        return 1;
    }
    0
}

/// Convert physical pixels (what the VST3 host speaks) to logical
/// points (what `Editor` works in). Identity when `host_scale` is
/// 1.0 or invalid.
fn phys_to_logical(pw: u32, ph: u32, host_scale: f64) -> (u32, u32) {
    if host_scale <= 0.0 || (host_scale - 1.0).abs() < f64::EPSILON {
        return (pw, ph);
    }
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let lw = (f64::from(pw) / host_scale).round() as u32;
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let lh = (f64::from(ph) / host_scale).round() as u32;
    (lw.max(1), lh.max(1))
}

/// Inverse of `phys_to_logical`.
fn logical_to_phys(lw: u32, lh: u32, host_scale: f64) -> (u32, u32) {
    if host_scale <= 0.0 || (host_scale - 1.0).abs() < f64::EPSILON {
        return (lw, lh);
    }
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let pw = (f64::from(lw) * host_scale).round() as u32;
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let ph = (f64::from(lh) * host_scale).round() as u32;
    (pw.max(1), ph.max(1))
}

/// Apply a host-supplied physical `(w, h)` to a resizable editor: convert to
/// logical points, clamp to the editor's min/max, and `set_size`. Shared by
/// `cb_gui_set_size` (the host's `onSize`) and the deferred-resize drain in
/// `cb_gui_get_size`.
fn apply_physical_resize(editor: &mut dyn Editor, w: u32, h: u32, host_scale: f64) {
    if editor.can_resize() {
        let (lw, lh) = phys_to_logical(w, h, host_scale);
        let (cw, ch) = clamp_logical_size(lw, lh, editor);
        editor.set_size(cw, ch);
    }
}

unsafe extern "C" fn cb_gui_open<P: PluginExport>(
    ctx: *mut std::ffi::c_void,
    parent: *mut std::ffi::c_void,
) {
    // `editor.open` runs author GUI-construction code that can panic;
    // firewall it so the panic can't unwind across the C ABI.
    run_extern_callback_with::<P, ()>("vst3", "gui_open", (), || unsafe {
        let inst = &*ctx.cast::<Vst3Instance<P>>();
        let mut gui = inst.gui.enter();
        if let Some(ref mut editor) = gui.editor {
            // Re-sync the editor's content scale to the host's current
            // value right before opening. `host_scale` lives on the
            // persistent instance and can already be set by the time the
            // host attaches (REAPER on Linux reports it via
            // `setContentScaleFactor` around `getSize`, *before* this
            // `open`), but the editor's own copy only got the value if a
            // `setContentScaleFactor` / `has_editor` replay happened to
            // land after the editor object existed and before now. When
            // it didn't - editor built while `host_scale` was still 1.0,
            // host bumped it to 2.0, then attached - the editor would open
            // pinned to 1.0 and render a half-size view in the host's 2x
            // frame (with 2x-off click targets). Applying it here makes
            // the open-time scale authoritative regardless of callback
            // ordering. Skip macOS: Retina is driven through AppKit there
            // and `host_scale` stays 1.0, so pinning would force 1x.
            #[cfg(not(target_os = "macos"))]
            editor.set_scale_factor(inst.host_scale());
            let params = Arc::clone(&inst.params_arc);
            let meter_store = Arc::clone(&inst.meter_store);
            let snapshot = Arc::clone(&inst.snapshot);
            let ctx_raw = SendPtr::new(ctx);
            let params_for_set = params.clone();
            let params_for_get = params.clone();
            let params_for_plain = params.clone();
            let params_for_fmt = params.clone();
            let params_for_ctx = params.clone();
            let task_spawner_for_ctx = inst.task_spawner.clone();
            let pending_state_for_set = inst.pending_state.clone();
            let transport_slot = inst.transport_slot.clone();
            let context = PluginContext::from_closures(
                ClosureBridge {
                    begin_edit: Box::new(move |id| {
                        ffi::moose_vst3_begin_edit(ctx_raw.as_ptr().cast_mut(), id);
                    }),
                    set_param: Box::new(move |id, value| {
                        // Single trait dispatch: same value-then-readback
                        // pattern collapsed via the trait helper. The
                        // post-clamp normalized value is what the host
                        // expects for `IComponentHandler::performEdit`.
                        let norm = params_for_set.set_normalized_returning_normalized(id, value);
                        ffi::moose_vst3_perform_edit(ctx_raw.as_ptr().cast_mut(), id, norm);
                    }),
                    end_edit: Box::new(move |id| {
                        ffi::moose_vst3_end_edit(ctx_raw.as_ptr().cast_mut(), id);
                    }),
                    request_resize: Box::new(move |w, h| {
                        // SAFETY: `ctx_raw` is the live
                        // `Vst3Instance` pointer the shim holds in
                        // its ctx -> MooseComponent table. The
                        // closure runs on the GUI thread, same as
                        // `cb_gui_set_content_scale` which is the
                        // only writer of `host_scale`. Routing
                        // through the shim's component (rather
                        // than holding a plug view pointer) avoids
                        // UAF across host editor recreations.
                        let host_scale = (*ctx_raw.as_ptr().cast::<Vst3Instance<P>>()).host_scale();
                        // VST3 hosts speak physical points;
                        // `Editor` speaks logical.
                        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                        let pw = (f64::from(w) * host_scale).round() as u32;
                        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                        let ph = (f64::from(h) * host_scale).round() as u32;
                        ffi::moose_vst3_request_resize(ctx_raw.as_ptr().cast_mut(), pw, ph) != 0
                    }),
                    get_param: Box::new(move |id| params_for_get.get_normalized(id).unwrap_or(0.0)),
                    get_param_plain: Box::new(move |id| {
                        params_for_plain.get_plain(id).unwrap_or(0.0)
                    }),
                    format_param: Box::new(move |id| {
                        let plain = params_for_fmt.get_plain(id).unwrap_or(0.0);
                        params_for_fmt
                            .format_value(id, plain)
                            .unwrap_or_else(|| format!("{plain:.1}"))
                    }),
                    get_meter: Box::new(move |id| meter_store.read(id)),
                    get_state: Box::new(move || {
                        // Editor state read: lock-free, reads the snapshot
                        // the audio thread publishes each block. Never
                        // touches the plugin, so an editor read can't
                        // stall audio.
                        save_extra(&snapshot)
                    }),
                    set_state: Box::new(move |bytes| {
                        // The editor sends RAW custom-state bytes -
                        // exactly what `save_state()` emits and
                        // `get_state` above returns - NOT a full
                        // `serialize_state` envelope. No params ride
                        // along: the editor mutates params through
                        // `set_param`.
                        //
                        // Always enqueue, never apply here: this closure can
                        // run on the editor's own thread (baseview drives a
                        // separate thread on Linux), and the plugin cell must
                        // only ever be entered from the audio thread (active)
                        // or the host main thread (inactive) - never a third.
                        // The audio thread drains the queue while active; while
                        // inactive the main thread drains it in `cb_state_save`
                        // / `cb_set_active`, so an edit made in that window is
                        // neither lost nor stranded.
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
            let handle = RawWindowHandle::AppKit(parent);
            #[cfg(target_os = "windows")]
            let handle = RawWindowHandle::Win32(parent);
            #[cfg(target_os = "linux")]
            let handle = RawWindowHandle::X11(parent as u64);

            editor.open(handle, context);
            // Re-sync the scale after `open`: on Windows `open` pumps
            // messages, so a re-entrant `setContentScaleFactor` may have
            // updated `host_scale` while `cb_gui_set_content_scale` had to skip
            // the editor (the cell was held here). Apply the authoritative
            // value now that the editor exists. macOS drives Retina through
            // AppKit, so `host_scale` stays 1.0 and pinning would force 1x.
            #[cfg(not(target_os = "macos"))]
            editor.set_scale_factor(inst.host_scale());
        }
    });
}

unsafe extern "C" fn cb_gui_close<P: PluginExport>(ctx: *mut std::ffi::c_void) {
    // `editor.close` runs author teardown code that can panic; firewall it.
    run_extern_callback_with::<P, ()>("vst3", "gui_close", (), || unsafe {
        let inst = &*ctx.cast::<Vst3Instance<P>>();
        if let Some(ref mut editor) = inst.gui.enter().editor {
            editor.close();
        }
    });
}

// ---------------------------------------------------------------------------
// Registration
// ---------------------------------------------------------------------------

/// Plugin display-name surfaced as `PClassInfo::name`. Reads
/// `moose.toml`'s `vst3_name` (baked into `PluginInfo` by
/// `moose::plugin_info!`), falling back to `PluginInfo::name`.
fn resolved_plugin_name(info: &PluginInfo) -> &'static str {
    resolve_name_override(info.vst3_name, info.name)
}

/// Per-direction channel-pointer capacity of the shim's `process()`
/// arrays (`kMaxProcChannels` in `vst3_shim.cpp`). That path is the
/// audio callback, so the arrays are fixed rather than heap-sized; a
/// plugin whose widest declared layout exceeds this can't be rendered
/// without silently truncating channels. Registration rejects it here
/// rather than advertise a layout the audio path can't honor. Bus
/// *counts* have no such limit - `setBusArrangements()` sizes its
/// per-bus arrays dynamically.
const VST3_MAX_CHANNELS_PER_DIRECTION: u32 = 32;

/// Largest structural input and output widths across all declared layouts.
///
/// Disabled optional buses still occupy fixed positions in the shim's
/// process arrays, so their declared channels count toward this bound.
fn max_layout_channels(layouts: &[BusLayout]) -> (u32, u32) {
    layouts.iter().fold((0, 0), |(mi, mo), l| {
        let sum = |buses: &[BusConfig]| {
            buses.iter().fold(0_u32, |channels, bus| {
                channels.saturating_add(bus.channels.channel_count())
            })
        };
        (mi.max(sum(&l.inputs)), mo.max(sum(&l.outputs)))
    })
}

pub fn register_vst3<P: PluginExport>() {
    // Called from the export macro's `extern "C" fn init()` static
    // initializer. Catch any panic so it doesn't cross the FFI
    // boundary and abort the host process.
    run_register::<P>("VST3", || {
        let Some((num_inputs, num_outputs)) = default_io_channels::<P>() else {
            log_missing_bus_layout::<P>("VST3");
            return;
        };
        let layouts = P::bus_layouts();
        if !bus_layouts_fit_routing(&layouts) {
            eprintln!(
                "[moose VST3] {} declares an audio-bus topology beyond BusRouting's limit of 32 \
                 buses per direction and 65,535 channels per bus - plugin will not register.",
                std::any::type_name::<P>(),
            );
            return;
        }
        let (max_in, max_out) = max_layout_channels(&layouts);
        if max_in > VST3_MAX_CHANNELS_PER_DIRECTION || max_out > VST3_MAX_CHANNELS_PER_DIRECTION {
            eprintln!(
                "[moose VST3] {} declares up to {max_in} input / {max_out} output channels, \
                 exceeding the shim's {VST3_MAX_CHANNELS_PER_DIRECTION}-channel-per-direction \
                 process limit - plugin will not register.",
                std::any::type_name::<P>(),
            );
            return;
        }
        if !vst3_topology_consistent(&layouts) {
            eprintln!(
                "[moose VST3] {} declares bus layouts that differ in input/output bus count or \
                 kind. VST3 fixes one bus topology per plugin - only channel widths may vary \
                 across layouts, not the number of buses or their main/sidechain kind. Give every \
                 layout the same bus structure. Plugin will not register.",
                std::any::type_name::<P>(),
            );
            return;
        }
        register_vst3_inner::<P>(num_inputs, num_outputs);
    });
}

// VST3 `ParameterInfo::ParameterFlags`. Only these bits are defined by
// the SDK (3.7); the previous code set a reserved `1 << 8`, which no host
// interprets, and left the read-only / hidden / list bits unmapped.
const VST3_PARAM_CAN_AUTOMATE: i32 = 1 << 0;
const VST3_PARAM_IS_READ_ONLY: i32 = 1 << 1;
const VST3_PARAM_IS_LIST: i32 = 1 << 3;
/// `kIsHidden` (SDK 3.7+): the parameter is not shown in generic editors
/// or automation pickers. Hosts still write to hidden ids through
/// `IParameterChanges`, which is how the MIDI proxy bank receives its
/// `IMidiMapping`-resolved controllers.
const VST3_PARAM_IS_HIDDEN: i32 = 1 << 4;
const VST3_PARAM_IS_BYPASS: i32 = 1 << 16;

// Assembles the descriptor, param descriptors, and callback table in one
// linear pass; splitting it further would scatter the one-time registration
// wiring across helpers that each read once.
#[allow(clippy::too_many_lines)]
fn register_vst3_inner<P: PluginExport>(num_inputs: u32, num_outputs: u32) {
    let info = P::info();
    // Static metadata path: derive emits a `LazyLock`-cached
    // `Vec<ParamInfo>` so registration skips the
    // `Self::create().params().param_infos()` walk and the plugin
    // construction it implies. Hand-written `PluginExport` impls
    // without a `Params::param_infos_static` override fall back to
    // the historical runtime path inside `PluginExport`'s default
    // impl.
    let param_infos = P::param_infos_static();
    let midi_proxy_ids = allocate_midi_proxy_ids(&param_infos, midi_proxy_len::<P>());

    let mut param_descs: Vec<Vst3ParamDescriptor> = Vec::with_capacity(param_infos.len());
    for pi in &param_infos {
        let cs = ParamCStrings::from_info(pi);

        let mut flags: i32 = 0;
        if pi.flags.contains(ParamFlags::AUTOMATABLE) {
            flags |= VST3_PARAM_CAN_AUTOMATE;
        }
        if pi.flags.contains(ParamFlags::READONLY) {
            flags |= VST3_PARAM_IS_READ_ONLY;
        }
        if pi.flags.contains(ParamFlags::HIDDEN) {
            flags |= VST3_PARAM_IS_HIDDEN;
        }
        if pi.flags.contains(ParamFlags::IS_BYPASS) {
            flags |= VST3_PARAM_IS_BYPASS;
        }
        // An enum is a named, indexed value list: `kIsList` makes the host
        // render a value dropdown (populated via `getParamStringByValue`)
        // instead of a knob. Discreteness for int / discrete-float params
        // is carried by the `step_count` descriptor field below, which is
        // what VST3 reads for step navigation - there's no separate flag.
        if matches!(pi.range, ParamRange::Enum { .. }) {
            flags |= VST3_PARAM_IS_LIST;
        }
        let step_count = pi.range.step_count();

        param_descs.push(Vst3ParamDescriptor {
            id: pi.id,
            name: cs.name.into_raw(),
            short_name: cs.short_name.into_raw(),
            units: cs.unit.into_raw(),
            min: pi.range.min(),
            max: pi.range.max(),
            default_normalized: pi.range.normalize(pi.default_plain),
            // Param step counts come from `IntParam`/`EnumParam` ranges,
            // bounded well below i32::MAX in practice.
            #[allow(clippy::cast_possible_wrap)]
            step_count: step_count.map_or(0, |n| n.get() as i32),
            flags,
            group: cs.group.into_raw(),
        });
    }

    // Hidden MIDI input proxies (see the MIDI-proxy block above):
    // appended *after* the real params so the shim's index-based
    // structures (unit table, ParameterInfo enumeration) keep their
    // positions. One bank per declared MIDI input port so multi-port
    // plugins keep controllers attributed per bus. `kIsHidden` (with
    // `kCanAutomate` clear) keeps the bank out of generic editors and
    // automation pickers - flags 0 alone left thousands of "MIDI Ch N
    // CC M" rows listed; hosts still deliver `IMidiMapping`-resolved
    // changes to hidden ids. Identity 0..=1 range, grouped under a
    // "MIDI" unit. The CStrings intentionally leak - registration
    // runs once per process, matching the real params' `into_raw`
    // pattern.
    if info.accepts_midi_in {
        let empty_units = || CString::default().into_raw();
        for port in 0..info.midi_input_ports {
            // Single-port plugins keep the unprefixed names hosts
            // already display; only multi-port names carry the bus.
            let (name_prefix, short_prefix) = if info.midi_input_ports > 1 {
                (format!("MIDI In {} ", port + 1), format!("I{}", port + 1))
            } else {
                (String::from("MIDI "), String::new())
            };
            for channel in 0u8..16 {
                for controller in 0..MIDI_PROXY_PER_CHANNEL {
                    let (name, short) = match controller {
                        MIDI_PROXY_PITCH_BEND => (
                            format!("{name_prefix}Ch {} Pitch Bend", channel + 1),
                            format!("{short_prefix}M{}PB", channel + 1),
                        ),
                        MIDI_PROXY_PRESSURE => (
                            format!("{name_prefix}Ch {} Pressure", channel + 1),
                            format!("{short_prefix}M{}Pr", channel + 1),
                        ),
                        cc => (
                            format!("{name_prefix}Ch {} CC {cc}", channel + 1),
                            format!("{short_prefix}M{}C{cc}", channel + 1),
                        ),
                    };
                    param_descs.push(Vst3ParamDescriptor {
                        id: allocated_midi_proxy_id(&midi_proxy_ids, port, channel, controller)
                            .expect("VST3 MIDI proxy allocation is complete"),
                        name: CString::new(name).unwrap_or_default().into_raw(),
                        short_name: CString::new(short).unwrap_or_default().into_raw(),
                        units: empty_units(),
                        min: 0.0,
                        max: 1.0,
                        default_normalized: midi_proxy_default(controller),
                        step_count: 0,
                        flags: VST3_PARAM_IS_HIDDEN,
                        group: CString::new("MIDI").unwrap_or_default().into_raw(),
                    });
                }
            }
        }
    }

    let name = CString::new(resolved_plugin_name(&info)).unwrap_or_default();
    let vendor = CString::new(info.vendor).unwrap_or_default();
    let url = CString::new(info.url).unwrap_or_default();
    let version = CString::new(info.version).unwrap_or_default();
    let category = CString::new("Audio Module Class").unwrap_or_default();
    // VST3 "Plugin Type Categories": Cubase (and other VST3 hosts)
    // route plugins into submenus based on a `<primary>|<secondary>`
    // pair from the SDK's published vocabulary. `Fx` alone advertises
    // the plug-in as "an effect of unspecified kind" and falls back
    // to the "Other" bucket; a secondary token like `Delay`, `Reverb`,
    // `EQ`, `Modulation`, etc. routes to the matching submenu.
    //
    // The Analyzer / NoteEffect / Tool categories already carry their
    // own implicit secondary token (`Fx|Analyzer`, `Fx|Event`,
    // `Fx|Tools`). For instruments and generic effects, the secondary
    // is opt-in via `moose.toml`'s `vst3_subcategory`. When unset the
    // wrapper ships the bare primary so the plug-in still loads, just
    // unbucketed.
    let subcategory_str = match (info.category, info.vst3_subcategory) {
        (PluginCategory::Instrument, Some(sub)) => format!("Instrument|{sub}"),
        (PluginCategory::Instrument, None) => "Instrument|Synth".to_string(),
        (PluginCategory::Effect, Some(sub)) => format!("Fx|{sub}"),
        (PluginCategory::Effect, None) => "Fx".to_string(),
        (PluginCategory::NoteEffect, _) => "Fx|Event".to_string(),
        (PluginCategory::Analyzer, _) => "Fx|Analyzer".to_string(),
        (PluginCategory::Tool, _) => "Fx|Tools".to_string(),
    };
    let subcategories = CString::new(subcategory_str).unwrap_or_default();

    // MIDI port counts are decided once on `PluginInfo` (category
    // default, overridable via `midi_input` / `midi_output` /
    // `midi_input_ports` / `midi_output_ports` in moose.toml). The shim
    // advertises this many event buses per direction.
    let midi_output_ports = i32::from(info.midi_output_ports);
    let midi_input_ports = i32::from(info.midi_input_ports);

    // Per-bus structure from the first declared layout (bus count + kind
    // are consistent across a plugin's layouts; only widths vary).
    let (num_input_buses, num_output_buses, input_bus_kinds, output_bus_kinds) =
        descriptor_buses::<P>();

    let descriptor = Box::leak(Box::new(Vst3PluginDescriptor {
        name: name.into_raw(),
        vendor: vendor.into_raw(),
        url: url.into_raw(),
        email: std::ptr::null(),
        version: version.into_raw(),
        cid: state::resolve_vst3_cid(P::vst3_class_id(), info.vst3_id),
        category: category.into_raw(),
        subcategories: subcategories.into_raw(),
        num_inputs,
        num_outputs,
        num_input_buses,
        num_output_buses,
        input_bus_kinds,
        output_bus_kinds,
        midi_output_ports,
        midi_input_ports,
        supports_f64: i32::from(<P as PluginRuntime>::Sample::IS_F64),
        // Widest output width across all layouts, so the shim's per-channel
        // output discard scratch has a distinct block for every channel of
        // any negotiable layout (see the field's `ffi` doc).
        max_outputs: max_layout_channels(&P::bus_layouts()).1,
    }));

    let callbacks = Box::leak(Box::new(Vst3Callbacks {
        create: cb_create::<P>,
        destroy: cb_destroy::<P>,
        reset: cb_reset::<P>,
        process: cb_process::<P>,
        process_f64: cb_process_f64::<P>,
        param_count: cb_param_count::<P>,
        param_get_value: cb_param_get_value::<P>,
        param_set_value: cb_param_set_value::<P>,
        param_normalize: cb_param_normalize::<P>,
        param_denormalize: cb_param_denormalize::<P>,
        param_format: cb_param_format::<P>,
        param_parse: cb_param_parse::<P>,
        state_save: cb_state_save::<P>,
        state_load: cb_state_load::<P>,
        state_free: cb_state_free,
        get_latency: cb_get_latency::<P>,
        get_tail: cb_get_tail::<P>,
        begin_input_events: cb_begin_input_events::<P>,
        push_input_event: cb_push_input_event::<P>,
        begin_output_events: cb_begin_output_events::<P>,
        next_output_event: cb_next_output_event::<P>,
        commit_output_event: cb_commit_output_event::<P>,
        finish_output_events: cb_finish_output_events::<P>,
        gui_has_editor: cb_gui_has_editor::<P>,
        gui_get_size: cb_gui_get_size::<P>,
        gui_open: cb_gui_open::<P>,
        gui_close: cb_gui_close::<P>,
        gui_set_content_scale: cb_gui_set_content_scale::<P>,
        gui_can_resize: cb_gui_can_resize::<P>,
        gui_check_size_constraint: cb_gui_check_size_constraint::<P>,
        gui_set_size: cb_gui_set_size::<P>,
        midi_mapping_get_param_id: cb_midi_mapping_get_param_id::<P>,
        get_output_param_count: cb_get_output_param_count::<P>,
        get_output_param: cb_get_output_param::<P>,
        set_active: cb_set_active::<P>,
        match_bus_layout: cb_match_bus_layout::<P>,
        layout_bus_channels: cb_layout_bus_channels::<P>,
        match_bus_layout_perbus: cb_match_bus_layout_perbus::<P>,
        param_is_chunked: cb_param_is_chunked::<P>,
        param_presentation: cb_param_presentation::<P>,
        param_presentation_revision: cb_param_presentation_revision::<P>,
        output_bus_name: cb_output_bus_name::<P>,
        output_bus_names_revision: cb_output_bus_names_revision::<P>,
    }));

    // Unify with the `Box::leak(Box::new(...))` shape above so every
    // descriptor handed to `moose_vst3_register` lives behind the
    // same kind of leaked allocation. `Vec::leak` produces a
    // `&'static mut [T]` from a heap reallocation that may differ in
    // capacity from len; converting through `into_boxed_slice()`
    // first trims to exact len and lets us route through `Box::leak`
    // alongside `descriptor` and `callbacks`.
    let param_descs: &'static [Vst3ParamDescriptor] = Box::leak(param_descs.into_boxed_slice());

    unsafe {
        ffi::moose_vst3_register(
            std::ptr::from_ref::<Vst3PluginDescriptor>(descriptor),
            std::ptr::from_ref::<Vst3Callbacks>(callbacks),
            param_descs.as_ptr(),
            len_u32(param_descs.len()),
        );
    }
}

// ---------------------------------------------------------------------------
// export_vst3! macro
// ---------------------------------------------------------------------------

#[macro_export]
macro_rules! export_vst3 {
    ($plugin_type:ty) => {
        mod _vst3_entry {
            use super::*;

            #[unsafe(no_mangle)]
            pub extern "C" fn moose_vst3_init() {
                ::moose_vst3::register_vst3::<$plugin_type>();
            }

            #[unsafe(no_mangle)]
            #[allow(non_snake_case)]
            pub unsafe extern "C" fn GetPluginFactory() -> *mut ::std::ffi::c_void {
                // Lazy init: register on first call
                static INIT: ::std::sync::Once = ::std::sync::Once::new();
                INIT.call_once(|| {
                    moose_vst3_init();
                });
                ::moose_vst3::ffi::moose_vst3_get_factory()
            }

            #[cfg(target_os = "macos")]
            #[unsafe(no_mangle)]
            #[allow(non_snake_case)]
            pub extern "system" fn BundleEntry(_: *mut ::std::ffi::c_void) -> bool {
                true
            }

            #[cfg(target_os = "macos")]
            #[unsafe(no_mangle)]
            pub extern "system" fn bundleEntry(_: *mut ::std::ffi::c_void) -> bool {
                true
            }

            #[cfg(target_os = "macos")]
            #[unsafe(no_mangle)]
            #[allow(non_snake_case)]
            pub extern "system" fn BundleExit() -> bool {
                true
            }

            #[cfg(target_os = "macos")]
            #[unsafe(no_mangle)]
            pub extern "system" fn bundleExit() -> bool {
                true
            }

            #[cfg(target_os = "linux")]
            #[unsafe(no_mangle)]
            #[allow(non_snake_case)]
            pub extern "system" fn ModuleEntry(_: *mut ::std::ffi::c_void) -> bool {
                true
            }

            #[cfg(target_os = "linux")]
            #[unsafe(no_mangle)]
            #[allow(non_snake_case)]
            pub extern "system" fn ModuleExit() -> bool {
                true
            }

            #[cfg(target_os = "windows")]
            #[unsafe(no_mangle)]
            #[allow(non_snake_case)]
            pub extern "system" fn InitDll() -> bool {
                true
            }

            #[cfg(target_os = "windows")]
            #[unsafe(no_mangle)]
            #[allow(non_snake_case)]
            pub extern "system" fn ExitDll() -> bool {
                true
            }
        }
    };
}

#[cfg(test)]
mod midi_proxy_tests {
    use super::{
        MIDI_PROXY_ID_BASE, MIDI_PROXY_PER_CHANNEL, MIDI_PROXY_PITCH_BEND, MIDI_PROXY_PRESSURE,
        midi_proxy_decode, midi_proxy_default, midi_proxy_event, midi_proxy_id,
    };
    use moose_core::events::EventBody;

    #[test]
    fn id_round_trips_across_the_banks() {
        // Every (port, channel, controller) triple survives the trip -
        // multi-timbral hosts rely on the port dimension to keep each
        // bus's controllers separate.
        for port in [0u8, 1, 3, 255] {
            for channel in 0u8..16 {
                for controller in 0..MIDI_PROXY_PER_CHANNEL {
                    let id = midi_proxy_id(port, channel, controller);
                    assert_eq!(midi_proxy_decode(id), Some((port, channel, controller)));
                }
            }
        }
    }

    #[test]
    fn ports_get_distinct_ids() {
        // The whole point of per-port banks: the same (channel, cc)
        // on two buses must be two parameter queues host-side.
        assert_ne!(midi_proxy_id(0, 4, 74), midi_proxy_id(1, 4, 74));
    }

    #[test]
    fn real_param_ids_never_decode() {
        // Hash ids live below METER_ID_BASE, meters just above it -
        // both far under the proxy base.
        const _: () = assert!(MIDI_PROXY_ID_BASE > moose_params::METER_ID_BASE);
        assert_eq!(midi_proxy_decode(0), None);
        assert_eq!(midi_proxy_decode(moose_params::METER_ID_BASE), None);
        assert_eq!(midi_proxy_decode(MIDI_PROXY_ID_BASE - 1), None);
        // One past the last bank is out again.
        assert_eq!(
            midi_proxy_decode(midi_proxy_id(255, 15, MIDI_PROXY_PER_CHANNEL - 1) + 1),
            None
        );
    }

    #[test]
    fn pitch_bend_endpoints_and_center() {
        let bend = |norm: f32| match midi_proxy_event(3, MIDI_PROXY_PITCH_BEND, norm) {
            EventBody::PitchBend { channel, value, .. } => {
                assert_eq!(channel, 3);
                value
            }
            other => panic!("expected PitchBend, got {other:?}"),
        };
        assert_eq!(bend(0.0), 0);
        assert_eq!(bend(0.5), 8192);
        assert_eq!(bend(1.0), 16383);
    }

    #[test]
    fn cc_and_pressure_decode_to_their_events() {
        match midi_proxy_event(0, 74, 1.0) {
            EventBody::ControlChange { cc, value, .. } => {
                assert_eq!(cc, 74);
                assert_eq!(value, 127);
            }
            other => panic!("expected ControlChange, got {other:?}"),
        }
        match midi_proxy_event(9, MIDI_PROXY_PRESSURE, 0.0) {
            EventBody::ChannelPressure {
                channel, pressure, ..
            } => {
                assert_eq!(channel, 9);
                assert_eq!(pressure, 0);
            }
            other => panic!("expected ChannelPressure, got {other:?}"),
        }
    }

    #[test]
    fn defaults_center_only_the_wheel() {
        assert!((midi_proxy_default(MIDI_PROXY_PITCH_BEND) - 0.5).abs() < f64::EPSILON);
        assert!(midi_proxy_default(0).abs() < f64::EPSILON);
        assert!(midi_proxy_default(MIDI_PROXY_PRESSURE).abs() < f64::EPSILON);
    }
}

#[cfg(test)]
mod channel_limit_tests {
    use super::{VST3_MAX_CHANNELS_PER_DIRECTION, max_layout_channels, vst3_topology_consistent};
    use moose_core::bus::{BusLayout, ChannelConfig};

    /// Layouts differing only in channel *width* (the point of multi-layout)
    /// share one bus topology and are accepted.
    #[test]
    fn topology_consistent_when_only_widths_vary() {
        let layouts = [
            BusLayout::stereo().with_sidechain_input("Sidechain", ChannelConfig::Stereo),
            BusLayout::mono().with_sidechain_input("Sidechain", ChannelConfig::Mono),
        ];
        assert!(vst3_topology_consistent(&layouts));
    }

    /// A layout with a different input bus *count* (the report's case: a
    /// mono main with no sidechain alongside a main+sidechain layout) can
    /// never be matched, so it's rejected at registration.
    #[test]
    fn topology_inconsistent_when_bus_count_differs() {
        let layouts = [
            BusLayout::stereo().with_sidechain_input("Sidechain", ChannelConfig::Stereo),
            BusLayout::mono(),
        ];
        assert!(!vst3_topology_consistent(&layouts));
    }

    #[test]
    fn topology_single_or_empty_is_consistent() {
        assert!(vst3_topology_consistent(&[BusLayout::stereo()]));
        assert!(vst3_topology_consistent(&[]));
    }

    #[test]
    fn widest_layout_wins() {
        let layouts = [BusLayout::stereo(), BusLayout::mono()];
        assert_eq!(max_layout_channels(&layouts), (2, 2));
    }

    #[test]
    fn sums_channels_across_buses() {
        // Main stereo + stereo sidechain = 4 input channels, 2 output.
        let layouts =
            [BusLayout::stereo().with_sidechain_input("Sidechain", ChannelConfig::Stereo)];
        assert_eq!(max_layout_channels(&layouts), (4, 2));
    }

    #[test]
    fn many_mono_buses_stay_within_limit() {
        // 17 mono input buses = 17 channels, under the 32-channel cap - so a
        // high bus count alone never trips the guard, and (with the dynamic
        // setBusArrangements arrays) such a plugin negotiates all its buses.
        let mut layout = BusLayout::new()
            .with_input("Main", ChannelConfig::Mono)
            .with_output("Main", ChannelConfig::Mono);
        for _ in 1..17 {
            layout = layout.with_sidechain_input("Aux", ChannelConfig::Mono);
        }
        let (max_in, _) = max_layout_channels(&[layout]);
        assert_eq!(max_in, 17);
        assert!(max_in <= VST3_MAX_CHANNELS_PER_DIRECTION);
    }

    #[test]
    fn oversized_layout_exceeds_limit() {
        // 17 stereo buses = 34 channels > 32: registration rejects this.
        let mut layout = BusLayout::new()
            .with_input("Main", ChannelConfig::Stereo)
            .with_output("Main", ChannelConfig::Stereo);
        for _ in 1..17 {
            layout = layout.with_sidechain_input("Aux", ChannelConfig::Stereo);
        }
        let (max_in, _) = max_layout_channels(&[layout]);
        assert_eq!(max_in, 34);
        assert!(max_in > VST3_MAX_CHANNELS_PER_DIRECTION);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use moose_core::events::EventBody;
    use std::ffi::c_void;

    #[test]
    fn midi_proxy_text_parses_the_fallback_display() {
        assert_eq!(parse_midi_proxy_text("0.50"), Some(0.5));
        assert_eq!(parse_midi_proxy_text(" 2 "), Some(1.0));
        assert_eq!(parse_midi_proxy_text("-1"), Some(0.0));
        assert_eq!(parse_midi_proxy_text("NaN"), None);
        assert_eq!(parse_midi_proxy_text("loud"), None);
    }

    unsafe extern "C" {
        fn moose_vst3_read_state_stream(stream: *mut c_void, out_len: *mut i32) -> *mut u8;
        fn moose_vst3_write_state_stream(stream: *mut c_void, data: *const u8, len: u32) -> i32;
    }

    /// A fake `IBStream`: the vtable pointer first, then a script of
    /// `(tresult, byte count)` replies the shim sees per read/write.
    #[repr(C)]
    struct FakeStream {
        vtbl: *const FakeVtbl,
        replies: Vec<(i32, i32)>,
        calls: usize,
    }

    unsafe extern "C" fn fake_io(s: *mut c_void, _buf: *mut c_void, n: i32, out: *mut i32) -> i32 {
        let s = unsafe { &mut *s.cast::<FakeStream>() };
        let (r, count) = s.replies.get(s.calls).copied().unwrap_or((0, 0));
        s.calls += 1;
        unsafe { *out = if count == i32::MAX { n } else { count } };
        r
    }

    type IoFn = unsafe extern "C" fn(*mut c_void, *mut c_void, i32, *mut i32) -> i32;

    /// `IBStream` vtable layout: 3 `FUnknown` slots, read, write, seek, tell.
    #[repr(C)]
    struct FakeVtbl {
        _unknown: [usize; 3],
        read: IoFn,
        write: IoFn,
        _seek_tell: [usize; 2],
    }

    static FAKE_VTBL: FakeVtbl = FakeVtbl {
        _unknown: [0; 3],
        read: fake_io,
        write: fake_io,
        _seek_tell: [0; 2],
    };

    fn fake(replies: &[(i32, i32)]) -> FakeStream {
        FakeStream {
            vtbl: &raw const FAKE_VTBL,
            replies: replies.to_vec(),
            calls: 0,
        }
    }

    fn read(replies: &[(i32, i32)]) -> Option<i32> {
        let mut s = fake(replies);
        let mut len = 0;
        let data = unsafe { moose_vst3_read_state_stream((&raw mut s).cast(), &raw mut len) };
        if data.is_null() {
            return None;
        }
        unsafe { libc_free(data.cast()) };
        Some(len)
    }

    #[test]
    fn state_read_is_bounded() {
        assert_eq!(read(&[(0, 4096), (0, 10), (0, 0)]), Some(4106));
        // Empty stream, negative or overlong counts fail the load.
        assert_eq!(read(&[(0, 0)]), None);
        assert_eq!(read(&[(0, -1)]), None);
        assert_eq!(read(&[(0, 4097)]), None);
        // A stream that never ends stops at the 32 MiB cap.
        assert_eq!(read(&vec![(0, i32::MAX); 8200]), None);
    }

    #[test]
    fn state_write_loops_over_partial_writes() {
        let blob = [7u8; 100];
        let write = |replies: &[(i32, i32)]| {
            let mut s = fake(replies);
            let ok =
                unsafe { moose_vst3_write_state_stream((&raw mut s).cast(), blob.as_ptr(), 100) };
            (ok, s.calls)
        };
        assert_eq!(write(&[(0, 60), (0, 40)]), (1, 2));
        // No progress, or a count past the request, fails the save.
        assert_eq!(write(&[(0, 60), (0, 0)]).0, 0);
        assert_eq!(write(&[(0, 101)]).0, 0);
    }
    use moose_params::{MidiSource, ParamFlags, ParamUnit, ParamValueKind};

    fn info(range: ParamRange, midi_map: Option<MidiSource>) -> ParamInfo {
        ParamInfo {
            id: 1,
            name: "p",
            short_name: "p",
            group: "",
            range,
            default_plain: 0.0,
            flags: ParamFlags::AUTOMATABLE,
            unit: ParamUnit::None,
            kind: ParamValueKind::Float,
            midi_map,
            midi_channel: None,
        }
    }

    /// Bridge a param change the way `process_block` does: only a param
    /// with a `midi_map` produces an event.
    fn bridge(info: &ParamInfo, plain: f64) -> Option<EventBody> {
        MidiMap::from_param(info).map(|m| midi_event_from_map(&m, plain))
    }

    #[test]
    fn unmapped_param_does_not_bridge() {
        let i = info(ParamRange::Linear { min: 0.0, max: 1.0 }, None);
        assert!(bridge(&i, 0.5).is_none());
    }

    #[test]
    fn midi_map_cache_holds_only_mapped_ids_and_binary_searches() {
        // The `process_block` fast path: build the sorted cache the way
        // `cb_create` does, then look up by id. Unmapped params are
        // absent, so their ids (and unknown ids) miss.
        let range = ParamRange::Linear {
            min: 0.0,
            max: 127.0,
        };
        let mut mapped_cc = info(range, Some(MidiSource::Cc(74)));
        mapped_cc.id = 5;
        let mut unmapped = info(range, None);
        unmapped.id = 2;
        let mut mapped_bend = info(range, Some(MidiSource::PitchBend));
        mapped_bend.id = 9;

        let mut cache: Vec<(u32, MidiMap)> = [&mapped_cc, &unmapped, &mapped_bend]
            .into_iter()
            .filter_map(|i| MidiMap::from_param(i).map(|m| (i.id, m)))
            .collect();
        cache.sort_by_key(|(id, _)| *id);

        assert_eq!(cache.iter().map(|(id, _)| *id).collect::<Vec<_>>(), [5, 9]);

        let find = |id: u32| cache.binary_search_by_key(&id, |(i, _)| *i);
        // The mapped CC bridges to a ControlChange on its number.
        let idx = find(5).expect("mapped id 5 present");
        assert!(matches!(
            midi_event_from_map(&cache[idx].1, 127.0),
            EventBody::ControlChange { cc: 74, .. }
        ));
        assert!(find(9).is_ok(), "mapped id 9 present");
        assert!(find(2).is_err(), "unmapped id absent");
        assert!(find(999).is_err(), "unknown id absent");
    }

    #[test]
    fn note_expression_maps_per_note_cc_and_bend() {
        // Volume CC (7) -> VST3 type 0; noteId = (channel<<7)|note.
        let (type_id, note_id, value) = note_expression_of(&EventBody::PerNoteCC {
            group: 0,
            channel: 2,
            note: 60,
            cc: 7,
            value: u32::MAX,
            registered: true,
        })
        .expect("volume maps");
        assert_eq!(type_id, 0);
        assert_eq!(note_id, vst3_note_id(2, 60));
        assert!((value - 1.0).abs() < 1e-9);

        // Pitch bend -> tuning (type 2), center value ~0.5.
        let (type_id, _, value) = note_expression_of(&EventBody::PerNotePitchBend {
            group: 0,
            channel: 0,
            note: 64,
            value: 0x8000_0000,
        })
        .expect("bend maps");
        assert_eq!(type_id, 2);
        assert!((value - 0.5).abs() < 1e-3);

        // Full-scale wire bend is ±48 st; VST3's tuning norm spans
        // ±120 st, so it must land at 0.5 + 48/240 = 0.7, not 1.0.
        let (_, _, value) = note_expression_of(&EventBody::PerNotePitchBend {
            group: 0,
            channel: 0,
            note: 64,
            value: u32::MAX,
        })
        .expect("bend maps");
        assert!((value - 0.7).abs() < 1e-6);

        // A CC with no predefined VST3 note-expression type is skipped.
        assert!(
            note_expression_of(&EventBody::PerNoteCC {
                group: 0,
                channel: 0,
                note: 60,
                cc: 20,
                value: 0,
                registered: true,
            })
            .is_none()
        );
    }

    #[test]
    fn assignable_per_note_cc_is_not_an_expression() {
        // Only registered per-note indices carry the predefined
        // expression semantics; an assignable index 7 is not volume.
        assert!(
            note_expression_of(&EventBody::PerNoteCC {
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
    fn note_id_is_deterministic() {
        assert_eq!(vst3_note_id(0, 0), 0);
        assert_eq!(vst3_note_id(2, 60), 0x013C); // (2 << 7) | 60
        assert_eq!(vst3_note_id(15, 127), 0x07FF); // (15 << 7) | 127
    }

    #[test]
    fn midi_event_layout_matches_shim() {
        // The C++ shim static_asserts the same shape; a drift on either
        // side fails its build or this test.
        assert_eq!(std::mem::size_of::<Vst3MidiEvent>(), 24);
        assert_eq!(std::mem::align_of::<Vst3MidiEvent>(), 8);
    }

    #[test]
    fn note_id_map_scopes_ids_per_port() {
        let mut map = NoteIdMap::new();
        // Host counters are arbitrary - nothing like the pitch - and
        // scoped per event bus: the same id on two buses is two
        // distinct voices.
        map.insert(0, 90210, 3, 64);
        map.insert(1, 90210, 5, 72);
        assert_eq!(map.lookup(0, 90210), Some((3, 64)));
        assert_eq!(map.lookup(1, 90210), Some((5, 72)));
        assert_eq!(map.lookup(2, 90210), None);
        assert_eq!(map.lookup(0, 64), None); // pitch is not a key
        // Unassigned ids never enter the map.
        map.insert(0, -1, 0, 60);
        assert_eq!(map.lookup(0, -1), None);
        // A reset drops every correlation.
        map.clear();
        assert_eq!(map.lookup(0, 90210), None);
        assert_eq!(map.lookup(1, 90210), None);
    }

    #[test]
    fn note_id_map_overflow_overwrites_oldest() {
        let mut map = NoteIdMap::new();
        for i in 0..NoteIdMap::CAPACITY {
            // Bounded by CAPACITY = 128, fits in both domains.
            #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
            map.insert(0, 1000 + i as i32, 0, i as u8);
        }
        // Full map: the next insert takes the round-robin slot rather
        // than being dropped, and the newest entry resolves.
        map.insert(0, 5000, 1, 72);
        assert_eq!(map.lookup(0, 5000), Some((1, 72)));
        // Entries outlive their note-off by design, so a full map of
        // released voices still can't wedge it: stale ids fall to the
        // round-robin overwrite.
        for i in 0..NoteIdMap::CAPACITY {
            #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
            map.insert(0, 6000 + i as i32, 2, i as u8);
        }
        assert_eq!(map.lookup(0, 1000), None);
    }

    #[test]
    fn tuning_norm_round_trips_and_saturates() {
        // Center and mid-range survive the domain re-scale both ways.
        assert_eq!(vst3_tuning_to_wire(0.5), 0x8000_0000);
        assert!((wire_to_vst3_tuning(0x8000_0000) - 0.5).abs() < 1e-9);
        let wire = vst3_tuning_to_wire(0.55); // +12 st
        assert!((wire_to_vst3_tuning(wire) - 0.55).abs() < 1e-6);
        // A host bend past the wire's ±48 st saturates.
        assert_eq!(vst3_tuning_to_wire(1.0), u32::MAX);
        assert_eq!(vst3_tuning_to_wire(0.0), 0);
    }

    #[test]
    fn unmapped_per_note_cc_degrades_to_channel_cc() {
        // No predefined VST3 expression type for cc 20 - it must fall
        // through to the 1.0 downconvert as a channel CC (matching
        // CLAP), not vanish.
        let event = Event::new(
            0,
            EventBody::PerNoteCC {
                group: 0,
                channel: 3,
                note: 60,
                cc: 20,
                value: u32::MAX,
                registered: true,
            },
        );
        let packet = try_encode_vst3_midi(&event).expect("degrades to channel CC");
        assert_eq!(packet.status, 0xB3);
        assert_eq!(packet.data1, 20);
        assert_eq!(packet.data2, 127);

        // Mapped per-note events ride note expression instead - the
        // MIDI encoder must skip them or they'd double-emit.
        let mapped = Event::new(
            0,
            EventBody::PerNoteCC {
                group: 0,
                channel: 0,
                note: 60,
                cc: 7,
                value: 0,
                registered: true,
            },
        );
        assert!(try_encode_vst3_midi(&mapped).is_none());
        let bend = Event::new(
            0,
            EventBody::PerNotePitchBend {
                group: 0,
                channel: 0,
                note: 60,
                value: 0,
            },
        );
        assert!(try_encode_vst3_midi(&bend).is_none());
    }

    #[test]
    fn unit_conversion_round_trips_full_precision() {
        // A centered tuning value must survive the crossing exactly -
        // the old 7-bit path decoded 0.5 as ~0.496 (about a semitone
        // flat over the +/-120 st tuning domain).
        let center = unit_to_u32(0.5);
        assert!((u32_to_unit(center) - 0.5).abs() < 1e-9);
        assert_eq!(unit_to_u32(0.0), 0);
        assert_eq!(unit_to_u32(1.0), u32::MAX);
        // FFI hygiene: out-of-domain hosts get clamped, not wrapped.
        assert_eq!(unit_to_u32(-0.25), 0);
        assert_eq!(unit_to_u32(1.5), u32::MAX);
        assert_eq!(unit_to_u32(f64::NAN), 0);
    }

    #[test]
    fn output_encode_carries_port() {
        // The plug-in stamps an outbound event's MIDI port; the shim
        // reads it back off `Vst3MidiEvent::port` to pick the event bus.
        let event = Event::on_port(
            5,
            2,
            EventBody::NoteOn {
                group: 0,
                channel: 0,
                note: 60,
                velocity: 100,
            },
        );
        let packet = try_encode_vst3_midi(&event).expect("note-on encodes");
        assert_eq!(packet.port, 2);
    }

    #[test]
    fn pitch_bend_maps_wheel_position_to_14bit() {
        // The synth's binding range: -1..1, where the host's
        // normalized 0/0.5/1 wheel positions land on plain -1/0/1.
        let i = info(
            ParamRange::Linear {
                min: -1.0,
                max: 1.0,
            },
            Some(MidiSource::PitchBend),
        );

        // Center wheel -> 8192.
        assert!(matches!(
            bridge(&i, 0.0),
            Some(EventBody::PitchBend { value: 8192, .. })
        ));
        // Full down -> 0, full up -> 16383.
        assert!(matches!(
            bridge(&i, -1.0),
            Some(EventBody::PitchBend { value: 0, .. })
        ));
        assert!(matches!(
            bridge(&i, 1.0),
            Some(EventBody::PitchBend { value: 16383, .. })
        ));
    }

    #[test]
    fn cc_and_pressure_and_program_map_to_7bit() {
        let cc = info(
            ParamRange::Linear { min: 0.0, max: 1.0 },
            Some(MidiSource::Cc(74)),
        );
        assert!(matches!(
            bridge(&cc, 1.0),
            Some(EventBody::ControlChange {
                cc: 74,
                value: 127,
                ..
            })
        ));

        let pressure = info(
            ParamRange::Linear { min: 0.0, max: 1.0 },
            Some(MidiSource::ChannelPressure),
        );
        assert!(matches!(
            bridge(&pressure, 0.0),
            Some(EventBody::ChannelPressure { pressure: 0, .. })
        ));

        let program = info(
            ParamRange::Linear { min: 0.0, max: 1.0 },
            Some(MidiSource::ProgramChange),
        );
        assert!(matches!(
            bridge(&program, 1.0),
            Some(EventBody::ProgramChange { program: 127, .. })
        ));
    }
}
