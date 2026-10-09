//! [`Core`] over `sampler-core`: one [`Runtime`] per rack part, fed through a
//! `sampler-midi` zone, mixed through the part's output tree.
//!
//! Wired: host notes (exact CLAP/VST3 ownership and NOTE_END, layered parts)
//! with per-note tuning, gain, pan, pressure and brightness; MIDI 1.0 and 2.0
//! channel voice packets (notes, sustain and sostenuto, controllers, pitch bend
//! with RPN sensitivity, channel and polyphonic pressure, all notes/sound off);
//! the mixer's part gain, pan, tune, mute, solo, output pair and aux send, pair
//! faders, peaks and the scope tap; every tree node's gain, pan, mute, solo and
//! output (its parent or a DAW pair). Loading: Kontakt instruments through
//! `sampler-kontakt` (cancelable, every group a mixer node), WAV files as one region.
//!
//! Kontakt samples stream: start data stays resident and the rest is read
//! from disk ahead of each voice.
//!
//! A part plays its MIDI on the zone's manager channel, or as an MPE lower
//! zone with member channels at its bend range.
//!
//! Not yet: per-note controllers and program changes without a selector (counted), a sample-rate change
//! without reloading.

use std::path::{Path, PathBuf};
use anyhow::{Context, ensure};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use sampler_core::{
    BusMix, ChannelAddress, ControlContext, ControlDefinition, ControlDomain, ControlValue, ControlWrite, Envelope, Expression, Frame, Input, Limits, NoteId, PAGE_FRAMES, Pcm, PlanControl, Playback, Prepared, Protocol,
    Region, Runtime, Stealing, StreamCache, Threads,
};
use sampler_ir as ir;
use sampler_midi::{ApplyError, Articulator, Intercept, Mpe, Packets, Zone};

use super::event::{Event, HostNote, NoteExpression};
use super::mix::{Mix, PartControls, Peaks, balance};
use super::report::{LoadReport, Missing, RuntimeProblems};
use super::tree::{self, MixNode, MixTree, NodeKind, NodeMix, NodeOutput};
use super::{
    BUSES, Block, BlockInfo, Core, CoreError, CoreLoader, Description, LoadFailure, LoadRequest, Loaded, ScriptUi, Stream, MAX_BLOCK, Progress,
    RACK_SLOTS, Rendered, Voices,
};

#[cfg(test)]
mod pressed_tests;
mod persistence;

/// Host notes tracked for ownership and NOTE_END across the rack.
const HELD: usize = 1024;
/// Notes a part holds at once, sounding or awaiting NOTE_END.
const NOTES: usize = 128;
/// [`Held::part`] of a note whose runtime was replaced: ends at the next block.
const ORPHAN: usize = usize::MAX;
/// Every input reaches a part's zone as MIDI 1.0 on its manager channel, so a
/// bend, pedal or controller on any channel reaches every note of the part.
const WIRE: ChannelAddress = ChannelAddress { protocol: Protocol::Midi1, port: 0, group: 0, channel: 0 };

/// One playable part: its runtime, the MIDI zone in front of it and its tree.
pub struct Part {
    runtime: Runtime,
    persistence: Option<persistence::Persistence>,
    tone: sampler_core::OutputLowPass,
    tone_history: [[[f64; 2]; 2]; BUSES + 1],
    pub(crate) epoch: u64,
    pub(crate) waveform_sources: std::collections::HashMap<u32, super::waveform::Source>,
    pub(crate) ui_controls: Option<ControlIngress>,
    pub(crate) engine_bindings: Arc<[sampler_core::EngineParameterBinding]>,
    editor_offsets: Option<Arc<[sampler_core::EngineParameterOffset]>>,
    mpe: Mpe,
    force_articulation_once: bool,
    tune: f32,
    /// Per tree node, its runtime bus (none for the root).
    buses: Box<[Option<usize>]>,
    tree: MixTree,
    /// Node settings as last set, after the root; preallocated to the tree.
    nodes: Vec<NodeMix>,
    audible: Box<[bool]>,
    /// DAW pairs some node plays to directly, as a bit set.
    direct: u32,
    problems: RuntimeProblems,
    pub(crate) fault_inbox: Arc<super::report::FaultInbox>,
    /// Velocity, channel, CC or program selecting articulations, when not keys.
    articulator: Option<Articulator>,
    /// The part's switching for each driver, from its instrument, and which
    /// [`PartControls::switching`] byte is applied.
    drivers: Vec<(sampler_core::Switching, Vec<sampler_core::Keyswitch>)>,
    switching: u8,
    user_route: Option<Arc<super::articulation::Routing>>,
    source_actions: Vec<Option<sampler_core::Switch>>,
    consumed_hosts: Vec<HostNote>,
    /// The instrument's own driver, for [`Self::drivers`].
    inherited: usize,
    /// The instrument's default articulation, when the runtime holds the
    /// articulation (it numbers that one 0).
    articulations: Option<usize>,
    /// Behavior-owned switching: each articulation's first switch key. The
    /// script holds the selection, so it is read from the key the driver tapped.
    tap_keys: Option<Vec<Option<u8>>>,
    /// Notes keep their member channel ([`PartControls::mpe`]).
    mpe_zone: bool,
    /// The bend range last sent to the zone, 0 for its default.
    bend_range: u8,
    /// Frames ahead of the clock that streamed voices read, if any stream.
    horizon: Option<u32>,
    /// Kept alive while the part plays; dropped with it, off the audio thread.
    _stream: Option<Arc<Stream>>,
    /// Grows the voice pool off the audio thread; stopped when the part drops.
    grower: Option<Grower>,
    /// The program's Lua scripts: they choose which of its oscillators play.
    script: Option<Box<ScriptDriver>>,
}

/// Bytes the system could give a new allocation without swapping, from
/// `/proc/meminfo`; `None` where that is unavailable.
fn mem_available() -> Option<usize> {
    let info = std::fs::read_to_string("/proc/meminfo").ok()?;
    let line = info.lines().find(|l| l.starts_with("MemAvailable:"))?;
    let kib: usize = line.split_whitespace().nth(1)?.parse().ok()?;
    kib.checked_mul(1024)
}

/// A growth may take at most this share of available memory.
const GROWTH_SHARE: usize = 4;

/// A thread that sleeps until the audio side reports a nearly full voice pool
/// (or a growth coming back), then doubles the pool while the new voices fit a
/// quarter of the memory available at that moment (and note capacity, sized
/// for `ceiling` voices, allows). Past that, only stealing is left.
struct Grower {
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Grower {
    fn start(
        runtime: &mut Runtime,
        mut control: PlanControl,
        ceiling: usize,
        per_voice: usize,
        note_ceiling: usize,
    ) -> std::io::Result<Self> {
        let stop = Arc::new(AtomicBool::new(false));
        let thread = std::thread::Builder::new().name("sampler-grow".into()).spawn({
            let stop = stop.clone();
            move || {
                while !stop.load(Ordering::Relaxed) {
                    let capacity = control.voice_capacity();
                    let notes = control.note_params_capacity();
                    if control.note_pressure() && notes < note_ceiling {
                        let next = (notes * 2).min(note_ceiling);
                        let fits = mem_available().is_none_or(|free|
                            (next - notes).saturating_mul(control.note_params_bytes()) <= free / GROWTH_SHARE);
                        if fits { let _ = control.grow_note_params(next); }
                    }
                    if control.voice_pressure() && capacity < ceiling {
                        let next = (capacity * 2).min(ceiling);
                        let fits = mem_available()
                            .is_none_or(|free| (next - capacity).saturating_mul(per_voice) <= free / GROWTH_SHARE);
                        if fits {
                            let _ = control.grow_voices(next);
                        }
                    }
                    std::thread::park();
                }
            }
        })?;
        runtime.set_growth_waker(thread.thread().clone());
        Ok(Self { stop, thread: Some(thread) })
    }
}

impl Drop for Grower {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            thread.thread().unpark();
            let _ = thread.join();
        }
    }
}

impl Part {
    pub(crate) fn new(runtime: Runtime, tree: MixTree) -> Result<Self, CoreError> {
        let core = |e: sampler_core::Error| CoreError::Invalid(format!("{e:?}"));
        let mpe = Mpe::new(&runtime, WIRE.port, WIRE.group, Zone::Lower, 15, NOTES).map_err(core)?;
        // Plans without articulations have nothing to drive.
        let articulator = runtime.performance(0).and_then(|p| Articulator::new(&runtime, p, WIRE.port)).ok();
        let count = tree.nodes.len();
        let engine_bindings = runtime.engine_parameter_bindings(runtime.active_plan()).map_err(core)?.into();
        let definitions = runtime.control_definitions(runtime.active_plan()).unwrap_or_default().to_vec();
        let plan = runtime.active_plan();
        let widgets = runtime.widget_definitions(plan).unwrap_or_default().to_vec();
        let widget_values = widgets.iter().filter(|w| !matches!(w.storage, sampler_core::WidgetStorage::Control(_))).filter_map(|w| widget_value(&runtime, plan, w).map(|value| (sampler_ui_ir::ControlId(w.id.0), value))).collect();
        let mut captures=std::collections::VecDeque::new();
        let mut capture=Vec::new();
        for widget in &widgets {
            let len=match widget.storage {sampler_core::WidgetStorage::Control(_)=>0,sampler_core::WidgetStorage::Cells{len,..}|sampler_core::WidgetStorage::Texts{len,..}=>len,sampler_core::WidgetStorage::FileSelection{..}=>1};
            for index in 0..len {
                capture.push(sampler_core::WidgetEdit{id:widget.id,index,value:sampler_core::WidgetValue::Integer(0),interaction:Default::default()});
                if capture.len()==sampler_core::WIDGET_EDIT_CAPACITY {captures.push_back(std::mem::take(&mut capture));}
            }
        }
        if !capture.is_empty() {captures.push_back(capture);}
        let revision=runtime.control_revision(plan).unwrap_or(0);
        let context = ControlContext { performance: runtime.performance(0).map_err(core)?, origin: WIRE, channels: 1 };
        let (runtime, client) = runtime.with_control_updates(256, sampler_core::WIDGET_EDIT_CAPACITY).map_err(core)?;
        let tone = sampler_core::OutputLowPass::new(runtime.sample_rate()).map_err(core)?;
        Ok(Self {
            epoch: 0,
            engine_bindings,
            waveform_sources: Default::default(),
            ui_controls: Some(ControlIngress { client, plan, context, definitions, widgets, widget_values, captures, capturing:false, revision, pending_widgets: Default::default(), pending: Default::default(), persistence: None }),
            runtime,
            tone,
            tone_history: [[[0.; 2]; 2]; BUSES + 1],
            editor_offsets:None,
            persistence: None,
            mpe,
            force_articulation_once: false,
            tune: 0.0,
            buses: (0..count).map(|n| n.checked_sub(1)).collect(),
            tree,
            nodes: vec![NodeMix::default(); count.saturating_sub(1)],
            audible: vec![true; count].into_boxed_slice(),
            direct: 0,
            problems: RuntimeProblems::default(),
            fault_inbox: Arc::new(super::report::FaultInbox::default()),
            articulator,
            drivers: Vec::new(),
            switching: 0,
            user_route: None,
            source_actions: Vec::new(),
            consumed_hosts: Vec::with_capacity(128),
            inherited: 0,
            articulations: None,
            tap_keys: None,
            mpe_zone: false,
            bend_range: 0,
            horizon: None,
            _stream: None,
            grower: None,
            script: None,
        })
    }

    // Mix changes frequently while dragging faders. Reuse the worker's
    // immutable sparse layer and rebind only when its contents actually change.
    fn apply_editor_offsets(&mut self,offsets:&Arc<[sampler_core::EngineParameterOffset]>) {
        if self.editor_offsets.as_deref() != Some(offsets.as_ref())
            && self.runtime.set_engine_offsets(offsets).is_err()
        {
            self.editor_offsets = None;
            return;
        }
        // Adopt the current Mix's owner before the shell retires the previous Mix.
        self.editor_offsets = Some(offsets.clone());
    }

    /// Follow the part's tuning, MPE and bend range settings.
    fn configure(&mut self, c: &PartControls, route: Option<&Arc<super::articulation::Routing>>) {
        if self.tune != c.tune && self.mpe.transpose(&mut self.runtime, f64::from(c.tune)).is_ok() {
            self.tune = c.tune;
        }
        if let Some(route) = route {
            if self.user_route.as_deref() != Some(route.as_ref()) {
                let _ = self.runtime.set_switching_table(route.switching.clone(), &route.keys);
                self.user_route = Some(route.clone());
            }
            self.switching = c.switching;
        } else if c.switching != self.switching && !self.drivers.is_empty() {
            self.switching = c.switching;
            // The instrument's own driver until the player remaps.
            let driver = if c.switching & 0x80 != 0 { usize::from(c.switching >> 1 & 7) } else { self.inherited };
            if let Some((switching, keys)) = self.drivers.get(driver) {
                let _ = self.runtime.set_switching_table(switching.clone(), keys);
            }
        }
        self.mpe_zone = c.mpe;
        if self.bend_range != c.bend_range {
            // 0 keeps what the instrument and the player's controller say.
            self.mpe.set_bend_range((c.bend_range > 0).then_some(c.bend_range));
            self.bend_range = c.bend_range;
        }
    }

    /// Keep one switching table per driver for `instrument`'s articulations,
    /// for a remap to swap in without lowering the plan again.
    fn set_drivers(&mut self, instrument: &ir::Instrument) {
        if instrument.articulations.is_empty() {
            return;
        }

        self.inherited = instrument.switching.driver as usize;
        let default = instrument.articulations.iter().position(|a| a.default).unwrap_or(0);
        self.source_actions = instrument.articulations.iter().enumerate().map(|(n, a)| {
            let id = if n == default { 0 } else if n < default { n as u32 + 1 } else { n as u32 };
            if instrument.switching.owner == ir::SwitchOwner::Native { Some(sampler_core::Switch::Articulation(id)) }
            else if let Some(&key) = a.switch_keys.first() { Some(sampler_core::Switch::Tap(key)) }
            else { a.control.map(|control| sampler_core::Switch::Control { id: control, articulation: id }) }
        }).collect();
        self.drivers.clear();
        for driver in [ir::Driver::Keys, ir::Driver::Velocity, ir::Driver::Channel, ir::Driver::Controller, ir::Driver::Program] {
            let switching = ir::Switching { driver, ..instrument.switching };
            let Ok((keys, switching)) = sampler_core::lower::switching(instrument, switching) else { break };
            self.drivers.push((switching, keys));
        }
    }

    /// Apply node settings to the runtime's buses.
    fn mix_nodes(&mut self, nodes: &[NodeMix]) {
        for (to, from) in self.nodes.iter_mut().zip(nodes) {
            *to = *from;
        }
        tree::audible(&self.tree, &self.nodes, &mut self.audible);
        self.direct = 0;
        for (n, mix) in self.nodes.iter().enumerate() {
            let Some(bus) = self.buses[n + 1] else { continue };
            let output = match mix.output {
                NodeOutput::Pair(pair) if usize::from(pair) < BUSES => {
                    self.direct |= 1 << pair;
                    Some(usize::from(pair))
                }
                _ => None,
            };
            let gain = tree::stereo_gain(mix, self.audible[n + 1]);
            let _ = self.runtime.set_bus_mix(bus, BusMix { gain, output });
        }
    }
}

/// A replaced part, dropped on a worker.
#[derive(Default)]
pub struct Retired(pub Option<Box<Part>>);

#[derive(Clone, Copy)]
struct Held {
    part: usize,
    note: HostNote,
    input: Input,
    id: NoteId,
}

pub struct V2Core {
    parts: Vec<Option<Box<Part>>>,
    performance: Option<[f64; 3]>,
    align: crate::timing::Align,
    holding: bool,
    aligned_buses: Box<[Block; BUSES]>,
    aligned_tap: Box<[f32;MAX_BLOCK]>,
    exact_work: Vec<HostNote>,
    rate: f64,
    mix: Mix,
    empty_editor_offsets: Arc<[sampler_core::EngineParameterOffset]>,
    held: Vec<Held>,
    /// Notes the ownership table had no room for.
    overflow: u64,
    buses: Box<[Block; BUSES]>,
    written: [bool; BUSES],
    signal_trace_active: bool,
    scratch: Box<[Frame; MAX_BLOCK]>,
    /// Nodes routed straight to a DAW pair, per pair.
    direct: Box<[[Frame; MAX_BLOCK]; BUSES]>,
    tap: Option<usize>,
    tapped: Box<[f32; MAX_BLOCK]>,
    peaks: Peaks,
}

/// Off-audio producer for the native admission/reply service; never retargeted on replacement.
pub(crate) struct ControlIngress {
    pub(crate) client: sampler_core::ControlClient,
    plan: sampler_core::PlanId,
    context: ControlContext,
    definitions: Vec<ControlDefinition>,
    widgets: Vec<sampler_core::WidgetDefinition>,
    widget_values: std::collections::BTreeMap<sampler_ui_ir::ControlId, sampler_ui_ir::Value>,
    pending_widgets: std::collections::BTreeMap<(sampler_ui_ir::ControlId, u32), (u64, sampler_ui_ir::Value)>,
    pending: std::collections::BTreeMap<sampler_ui_ir::ControlId, (u64, f64)>,
    captures:std::collections::VecDeque<Vec<sampler_core::WidgetEdit>>,
    capturing:bool,
    revision:u64,
    persistence: Option<Arc<persistence::Snapshot>>,
}

impl ControlIngress {
    pub(crate) fn save_script_state(&self) -> Option<String> { self.persistence.as_ref().map(|state|state.save()) }
    pub(crate) fn plan(&self) -> sampler_core::PlanId { self.plan }
    pub(crate) fn submit_host_parameter(&mut self, address: u16, value: f64) -> bool {
        if address >= super::HOST_AUTOMATION_SLOTS || !value.is_finite() || !(0.0..=1.0).contains(&value) { return false; }
        self.client.submit(sampler_core::ControlRequest { plan: self.plan, expected_revision: None,
            operation: sampler_core::ControlOperation::HostParameter(self.context, address, value) }).is_ok()
    }

    pub(crate) fn submit_ui_widgets(&mut self, source_slot: u8, widget: &sampler_ui_ir::Widget, edits: Vec<(u32, sampler_ui_ir::Value)>, interaction: sampler_core::WidgetInteraction) -> bool {
        let definition = self.widgets.iter().find(|w| w.source_slot == source_slot && Some(w.ui_id) == widget.source_id)
            .or_else(|| self.widgets.iter().find(|w| matches!(widget.binding, sampler_ui_ir::Binding::Control(id) if matches!(w.storage, sampler_core::WidgetStorage::Control(control) if control.0 == id.0))));
        let Some(definition) = definition else {
            if let sampler_ui_ir::Binding::Control(id) = widget.binding && edits.len() == 1 && edits[0].0 == 0 {
                return match edits[0].1 { sampler_ui_ir::Value::Integer(value) => self.submit(id, f64::from(value)), sampler_ui_ir::Value::Real(value) => self.submit(id, value), _ => false };
            }
            return false;
        };
        let mut native = Vec::with_capacity(edits.len());
        for (index, value) in edits {
            let value = match value {
                sampler_ui_ir::Value::Integer(value) => sampler_core::WidgetValue::Integer(i64::from(value)),
                sampler_ui_ir::Value::Real(value) if value.is_finite() => match definition.storage {
                    sampler_core::WidgetStorage::Control(id) if self.definitions.iter().any(|d| d.id == id && matches!(d.domain, ControlDomain::Integer { .. } | ControlDomain::Toggle)) => sampler_core::WidgetValue::Integer(value.round() as i64),
                    _ => sampler_core::WidgetValue::Real(value),
                },
                sampler_ui_ir::Value::DropPath { kind, path } => {
                    let Ok(path) = sampler_core::Text::try_new(&path) else { return false };
                    sampler_core::WidgetValue::DropPath { kind: match kind {
                        sampler_ui_ir::DropKind::Audio => sampler_core::WidgetDropKind::Audio,
                        sampler_ui_ir::DropKind::Midi => sampler_core::WidgetDropKind::Midi,
                        sampler_ui_ir::DropKind::Array => sampler_core::WidgetDropKind::Array,
                    }, path }
                }
                sampler_ui_ir::Value::Text(value) => { let text = sampler_core::Text::new(&value); if text.as_str() != value { return false; } sampler_core::WidgetValue::Text(text) },
                _ => return false,
            };
            native.push(sampler_core::WidgetEdit { id: definition.id, index, value, interaction });
        }
        self.submit_widgets(native)
    }

    pub(crate) fn submit(&mut self, id: sampler_ui_ir::ControlId, value: f64) -> bool {
        if !value.is_finite() { return false; }
        let Some(d) = self.definitions.iter().find(|d| d.id.0 == id.0) else { return false };
        let value = match d.domain {
            ControlDomain::Integer { min, max } if value.round() >= min as f64 && value.round() <= max as f64 => ControlValue::Integer(value.round() as i64),
            ControlDomain::Real { min, max } if (min..=max).contains(&value) => ControlValue::Real(value),
            ControlDomain::Toggle if value == 0. || value == 1. => ControlValue::Toggle(value == 1.),
            _ => return false,
        };
        if let Some(widget) = self.widgets.iter().find(|w| matches!(w.storage, sampler_core::WidgetStorage::Control(control) if control == d.id)) {
            return self.submit_widgets(vec![sampler_core::WidgetEdit { id: widget.id, index: 0, value: match value {
                ControlValue::Integer(v) => sampler_core::WidgetValue::Integer(v),
                ControlValue::Real(v) => sampler_core::WidgetValue::Real(v),
                ControlValue::Toggle(v) => sampler_core::WidgetValue::Integer(i64::from(v)),
            }, interaction: Default::default() }]);
        }
        let command = sampler_core::ControlRequest { plan: self.plan, expected_revision: None,
            operation: sampler_core::ControlOperation::Invoke(self.context, ControlWrite { id: d.id, value }) };
        match self.client.submit(command) {
            Ok(request) => { self.pending.insert(id, (request, number(value))); true }
            Err(_) => false,
        }
    }

    pub(crate) fn submit_widgets(&mut self, edits: Vec<sampler_core::WidgetEdit>) -> bool {
        let Some(first) = edits.first() else { return false };
        let Some(widget) = self.widgets.iter().find(|w| w.id == first.id) else { return false };
        if edits.len() > sampler_core::WIDGET_EDIT_CAPACITY || edits.iter().any(|e| e.id != first.id || ui_value(e.value).is_none()) { return false; }
        let id = sampler_ui_ir::ControlId(first.id.0);
        let preview: Vec<_> = edits.iter().filter(|e| !matches!(e.value, sampler_core::WidgetValue::DropPath { .. })).map(|e| (e.index, ui_value(e.value).unwrap())).collect();
        let scalar = match widget.storage { sampler_core::WidgetStorage::Control(control) => Some(sampler_ui_ir::ControlId(control.0)), _ => None };
        let command = sampler_core::ControlRequest { plan: self.plan, expected_revision: None,
            operation: sampler_core::ControlOperation::InvokeWidget(self.context, edits) };
        match self.client.submit(command) {
            Ok(request) => {
                for (index, value) in preview {
                    if let Some(control) = scalar {
                        let number = match value { sampler_ui_ir::Value::Integer(v) => f64::from(v), sampler_ui_ir::Value::Real(v) => v, _ => continue };
                        self.pending.insert(control, (request, number));
                    }
                    self.pending_widgets.insert((id, index), (request, value));
                }
                true
            }
            Err(_) => false,
        }
    }

    /// Existing 100 ms worker owns polling and recycles native capture buffers.
    pub(crate) fn refresh(&mut self) -> bool {
        let changed=self.settle();
        if !self.capturing && let Some(output)=self.captures.pop_front() {
            let request=sampler_core::ControlRequest {plan:self.plan,expected_revision:None,operation:sampler_core::ControlOperation::CaptureWidget(output)};
            match self.client.submit(request) {
                Ok(_)=>self.capturing=true,
                Err(rejected)=>if let sampler_core::ControlOperation::CaptureWidget(output)=rejected.command.operation {self.captures.push_front(output);},
            }
        }
        changed
    }

    pub(crate) fn settle(&mut self) -> bool {
        let mut changed = false;
        while let Some(reply) = self.client.reply() {
            if let Ok((_,revision))=reply.result {self.revision=revision;}
            if let sampler_core::ControlOperation::Invoke(_, write) = &reply.command.operation {
                let id = sampler_ui_ir::ControlId(write.id.0);
                if self.pending.get(&id).is_some_and(|(request, _)| *request == reply.request) {
                    self.pending.remove(&id);
                    changed = true;
                }
            }
            if let sampler_core::ControlOperation::InvokeWidget(_, edits) = &reply.command.operation {
                for edit in edits {
                    let id = sampler_ui_ir::ControlId(edit.id.0);
                    if self.pending_widgets.get(&(id, edit.index)).is_some_and(|(request, _)| *request == reply.request) { self.pending_widgets.remove(&(id, edit.index)); changed = true; }
                    if let Some(widget) = self.widgets.iter().find(|w| w.id == edit.id)
                        && let sampler_core::WidgetStorage::Control(control) = widget.storage {
                        let control = sampler_ui_ir::ControlId(control.0);
                        if self.pending.get(&control).is_some_and(|(request, _)| *request == reply.request) { self.pending.remove(&control); }
                    }
                    if reply.result.is_ok() { changed |= self.accept_value(edit); }
                }
            }
            if let sampler_core::ControlOperation::CaptureWidget(output)=reply.command.operation {
                self.capturing=false;
                if let Ok((count,_))=reply.result {for edit in output.iter().take(count) {changed|=self.accept_value(edit);}}
                self.captures.push_back(output);
            }
            if let Err(error) = reply.result {
                crate::diagnostics::event(crate::diagnostics::LogLevel::Warning, "ui", "control_rejected", serde_json::json!({"request": reply.request, "reason": format!("{error:?}")}));
            }
        }
        changed
    }

    fn accept_value(&mut self, edit: &sampler_core::WidgetEdit) -> bool {
        if matches!(edit.value, sampler_core::WidgetValue::DropPath { .. }) { return false; }
        let Some(value) = ui_value(edit.value) else { return false };
        let Some(current) = self.widget_values.get_mut(&sampler_ui_ir::ControlId(edit.id.0)) else { return false };
        match (current, value) {
            (sampler_ui_ir::Value::Integers(values), sampler_ui_ir::Value::Integer(value)) => values.get_mut(edit.index as usize).is_some_and(|old| { let changed = *old != value; *old = value; changed }),
            (sampler_ui_ir::Value::Reals(values), sampler_ui_ir::Value::Real(value)) => values.get_mut(edit.index as usize).is_some_and(|old| { let changed = *old != value; *old = value; changed }),
            (current, value) if edit.index == 0 => { let changed = *current != value; *current = value; changed },
            _ => false,
        }
    }

    pub(crate) fn values(&self, face: &sampler_ui_ir::Interface) -> std::collections::HashMap<sampler_ui_ir::WidgetRef, sampler_ui_ir::Value> {
        let mut values = std::collections::HashMap::new();
        for (n, widget) in face.widgets.iter().enumerate() {
            let sampler_ui_ir::Binding::Variable { script, name } = &widget.binding else { continue };
            let id = sampler_ui_ir::ControlId(sampler_ksp::derived_control_id(*script, name).0);
            let Some(mut current) = self.widget_values.get(&id).cloned() else { continue };
            for (&(_, index), (_, value)) in self.pending_widgets.range((id, 0)..=(id, u32::MAX)) {
                match (&mut current, value) {
                    (sampler_ui_ir::Value::Integers(values), sampler_ui_ir::Value::Integer(value)) => { if let Some(old) = values.get_mut(index as usize) { *old = *value; } }
                    (sampler_ui_ir::Value::Reals(values), sampler_ui_ir::Value::Real(value)) => { if let Some(old) = values.get_mut(index as usize) { *old = *value; } }
                    (current, value) if index == 0 => current.clone_from(value),
                    _ => {},
                }
            }
            values.insert(sampler_ui_ir::WidgetRef(n), current);
        }
        values
    }

    pub(crate) fn overlay(&mut self, values: &mut Vec<(sampler_ui_ir::ControlId, f64)>) {
        self.settle();
        for (id, value) in values { if let Some((_, pending)) = self.pending.get(id) { *value = *pending; } }
    }
}

fn ui_value(value: sampler_core::WidgetValue) -> Option<sampler_ui_ir::Value> {
    Some(match value {
        sampler_core::WidgetValue::Integer(value) => sampler_ui_ir::Value::Integer(value.try_into().ok()?),
        sampler_core::WidgetValue::Real(value) if value.is_finite() => sampler_ui_ir::Value::Real(value),
        sampler_core::WidgetValue::Text(value) => sampler_ui_ir::Value::Text(value.as_str().into()),
        sampler_core::WidgetValue::DropPath { kind, path } => sampler_ui_ir::Value::DropPath { kind: match kind {
            sampler_core::WidgetDropKind::Audio => sampler_ui_ir::DropKind::Audio,
            sampler_core::WidgetDropKind::Midi => sampler_ui_ir::DropKind::Midi,
            sampler_core::WidgetDropKind::Array => sampler_ui_ir::DropKind::Array,
        }, path: path.as_str().into() },
        _ => return None,
    })
}

fn widget_value(runtime: &Runtime, plan: sampler_core::PlanId, widget: &sampler_core::WidgetDefinition) -> Option<sampler_ui_ir::Value> {
    use sampler_core::{WidgetStorage, WidgetValue};
    Some(match widget.storage {
        WidgetStorage::Control(_) | WidgetStorage::Texts { .. } | WidgetStorage::FileSelection{..} => ui_value(runtime.widget_value(plan, widget.id, 0).ok()?)?,
        WidgetStorage::Cells { len, real: false, .. } => sampler_ui_ir::Value::Integers((0..len).map(|index| match runtime.widget_value(plan, widget.id, index).ok()? { WidgetValue::Integer(value) => value.try_into().ok(), _ => None }).collect::<Option<_>>()?),
        WidgetStorage::Cells { len, real: true, .. } => sampler_ui_ir::Value::Reals((0..len).map(|index| match runtime.widget_value(plan, widget.id, index).ok()? { WidgetValue::Real(value) => Some(value), _ => None }).collect::<Option<_>>()?),
    })
}

impl V2Core {
    pub(crate) fn release_all_notes(&mut self) {
        for (index, part) in self.parts.iter_mut().enumerate() {
            let Some(part) = part else { continue };
            // Lua note-off services use the same host identities as ordinary release.
            for held in self.held.iter().filter(|h| h.part == index) {
                if let Some(script) = part.script.as_mut() { let _ = script.note_off(&mut part.runtime, held.note.key); }
            }
            packet(part, index, &self.held, [0x20b0_4000, 0]); // MIDI 1 CC64 up
            packet(part, index, &self.held, [0x20b0_4200, 0]); // MIDI 1 CC66 up
            part.runtime.release_all_notes();
        }
    }
    pub(crate) fn pressed_keys(&self, keys: &mut [u8; 128]) -> bool {
        keys.fill(0);
        let mut loaded = false;
        for part in self.parts.iter().flatten() {
            loaded = true;
            part.runtime.pressed_keys(keys);
        }
        loaded
    }
    /// Called at the DAW event's sample boundary, on the runtime owner.
    pub(crate) fn host_parameter(&mut self, address: u16, value: f64) -> bool {
        if address >= super::HOST_AUTOMATION_SLOTS || !value.is_finite() || !(0.0..=1.0).contains(&value) { return false; }
        let mut accepted = true;
        for part in self.parts.iter_mut().flatten() {
            let rt = &mut part.runtime;
            let Ok(performance) = rt.performance(0) else { accepted = false; continue };
            let context = ControlContext { performance, origin: WIRE, channels: 1 };
            accepted &= rt.dispatch_host_parameter(context, address, value).is_ok();
        }
        accepted
    }

    pub(crate) fn widget_meter(&self, slot: usize, address: sampler_core::EngineMeterAddress) -> Option<f32> {
        let part = self.parts.get(slot)?.as_ref()?;
        part.runtime.engine_meter(part.runtime.active_plan(), address).ok()
    }

    pub(crate) fn epoch(&self, slot: usize) -> u64 { self.parts.get(slot).and_then(Option::as_ref).map_or(0, |p| p.epoch) }
    pub(crate) fn ui_revision(&self, slot: usize) -> u64 {
        self.parts.get(slot).and_then(Option::as_ref).and_then(|p| p.runtime.control_revision(p.runtime.active_plan()).ok()).unwrap_or(0)
    }
}

impl Default for V2Core {
    fn default() -> Self {
        Self::with_parts(RACK_SLOTS, 48000.0)
    }
}

#[cfg(feature = "shots")]
impl V2Core {
    pub fn scan_record_selections(&mut self, part: usize, enabled: bool) {
        if let Some(Some(p)) = self.parts.get_mut(part) { p.runtime.record_selections(enabled); }
    }
    pub fn scan_selections(&mut self, part: usize) -> Vec<sampler_core::SelectionRecord> {
        self.parts.get_mut(part).and_then(Option::as_mut).map(|p|p.runtime.take_selection_records()).unwrap_or_default()
    }
    #[cfg(test)]
    pub(crate) fn widget_gate_values(&self, part: usize) -> std::collections::BTreeMap<sampler_ui_ir::ControlId, sampler_ui_ir::Value> {
        let Some(Some(part)) = self.parts.get(part) else { return Default::default() };
        let plan = part.runtime.active_plan();
        part.runtime.widget_definitions(plan).unwrap_or_default().iter()
            .filter_map(|widget| widget_value(&part.runtime, plan, widget)
                .map(|value| (sampler_ui_ir::ControlId(widget.id.0), value))).collect()
    }
    pub fn scan_runtime_faults(&mut self,part:usize)->Vec<(usize,sampler_core::Outcome)> {
        let mut out=Vec::new();if let Some(Some(p))=self.parts.get_mut(part){p.runtime.flush_behaviors_at(|_,_,outcome,program|{if matches!(outcome,sampler_core::Outcome::Fault(_)|sampler_core::Outcome::FuelExhausted){out.push((program,outcome));}true});}out
    }
    pub fn scan_lua(&self, part: usize) -> Option<sampler_uvi::script::ScanFaults> {
        self.parts.get(part)?.as_ref()?.script.as_ref().map(|s| s.scan_faults())
    }
}

fn wire(key: u8, external_id: Option<i32>) -> Input {
    Input { protocol: WIRE.protocol, port: WIRE.port, group: WIRE.group, channel: WIRE.channel, key, external_id }
}

/// A host note's owner on the wire, distinct from MIDI notes by its ID; notes
/// without one (VST3) get a negative ID per channel. In an MPE zone it keeps
/// its member channel.
fn host_input(note: HostNote, mpe: bool) -> Input {
    let id = if note.id >= 0 { note.id } else { -1 - i32::from(note.channel) };
    Input { channel: if mpe { note.channel & 15 } else { WIRE.channel }, ..wire(note.key, Some(id)) }
}

fn peak(x: &[f32]) -> f32 {
    x.iter().fold(0.0, |p, x| p.max(x.abs()))
}

/// A MIDI 1.0 channel voice message into `part`'s zone, on its manager
/// channel, or in an MPE zone on its own.
fn wire_event(part: &mut Part, status: u8, a: u8, b: u8) {
    wire_packet(part, &[0x2000_0000 | u32::from(status) << 16 | u32::from(a & 127) << 8 | u32::from(b & 127)]);
}

/// A channel voice packet into `part`'s zone on its group, and on its manager
/// channel unless the zone is MPE.
fn wire_packet(part: &mut Part, words: &[u32]) {
    if !articulated(part, words) { return; }
    // Program selectors consume this above; the note adapter has no program sink.
    if words[0] & 0x00f0_0000 == 0x00c0_0000 {
        part.problems.ignored_input += 1;
        return;
    }
    let mut words = [words[0], words.get(1).copied().unwrap_or(0)];
    words[0] &= if part.mpe_zone { 0xf0ff_ffff } else { 0xf0f0_ffff };
    let words = &words[..if words[0] >> 28 == 4 { 2 } else { 1 }];
    if let Some(Ok(packet)) = Packets::new(words).next()
        && let Err(ApplyError::Core(sampler_core::Error::Capacity)) = part.mpe.apply(&mut part.runtime, packet)
    {
        part.problems.capacity_drops += 1;
    }
}

/// Run the articulation driver on a packet; false when it took the packet.
fn articulated(part: &mut Part, words: &[u32]) -> bool {
    if std::mem::take(&mut part.force_articulation_once) { return true; }
    let Some(articulator) = part.articulator.as_mut() else { return true };
    let Some(Ok(packet)) = Packets::new(words).next() else { return true };
    !matches!(articulator.intercept(&mut part.runtime, packet), Ok(Intercept::Consumed(_)))
}

/// Change one note's expression in place.
fn express(runtime: &mut Runtime, note: NoteId, change: impl FnOnce(&mut Expression)) {
    let Ok(owner) = runtime.expression_id(note) else { return };
    let Ok(mut expression) = runtime.expression(owner) else { return };
    change(&mut expression);
    expression.gain = expression.gain.clamp(0.0, sampler_core::MAX_EXPRESSION_GAIN);
    expression.pan = expression.pan.clamp(-1.0, 1.0);
    let _ = runtime.set_expressions(&[(owner, expression)]);
}

fn unit_scale(value: f64) -> u32 {
    (value.clamp(0.0, 1.0) * f64::from(u32::MAX)) as u32
}

/// `expression` on one held note; the notes a script played from it follow.
fn note_expression(part: &mut Part, id: NoteId, expression: NoteExpression) {
    let tune = f64::from(part.tune);
    express(&mut part.runtime, id, |e| match expression {
        NoteExpression::Tune(semitones) => e.pitch_semitones = semitones + tune,
        NoteExpression::Gain(gain) => e.gain = gain,
        NoteExpression::Pan(pan) => e.pan = pan,
        NoteExpression::Pressure(v) => e.pressure = unit_scale(v),
        NoteExpression::Brightness(v) => e.timbre = unit_scale(v),
    })
}

/// Controllers, bend, pressure and program changes reach the part's scripts
/// (they still reach the engine: a script's `postEvent` of one adds to it).
fn tell_script(part: &mut Part, kind: u32, status: u8, channel: u8, a: u8, b: u8, data: u32) {
    use sampler_uvi::scripted::HostInput as Input;
    let Part { script: Some(script), runtime, .. } = part else { return };
    let high = (data >> 25) as u8;
    let input = match (kind, status) {
        (2, 0xb0) => Input::Controller { cc: a, value: b, channel },
        (4, 0xb0) => Input::Controller { cc: a, value: high, channel },
        (2, 0xe0) => Input::Bend { value: (f64::from(u16::from(b) << 7 | u16::from(a)) - 8192.0) / 8192.0, channel },
        (4, 0xe0) => Input::Bend { value: f64::from(data) / 2_147_483_648.0 - 1.0, channel },
        (2, 0xd0) => Input::Touch { value: a, channel },
        (4, 0xd0) => Input::Touch { value: high, channel },
        (2, 0xa0) => Input::PolyTouch { key: a, value: b, channel },
        (4, 0xa0) => Input::PolyTouch { key: a, value: high, channel },
        (2 | 4, 0xc0) => Input::Program { value: a, channel },
        _ => return,
    };
    script.input(runtime, input);
}

/// A channel voice packet into one part, MIDI 2.0 values at full precision;
/// per-note messages reach held host notes.
fn packet(part: &mut Part, index: usize, held: &[Held], words: [u32; 2]) {
    let [word, data] = words;
    let (kind, status, a, b) = (word >> 28, (word >> 16) as u8 & 0xf0, (word >> 8) as u8 & 127, word as u8 & 127);
    let channel = (word >> 16) as u8 & 15;
    // Per-note messages reach held host notes on the key at full precision.
    let per_note = |part: &mut Part, expression: NoteExpression| {
        for h in held.iter().filter(|h| h.part == index && h.note.channel == channel && h.note.key == a) {
            note_expression(part, h.id, expression);
        }
    };
    tell_script(part, kind, status, channel, a, b, data);
    match (kind, status) {
        (2, 0xa0) => per_note(part, NoteExpression::Pressure(f64::from(b) / 127.0)),
        (4, 0xa0) => per_note(part, NoteExpression::Pressure(f64::from(data) / f64::from(u32::MAX))),
        // Per-note pitch bend: centre 2^31, ±48 semitones full scale.
        (4, 0x60) => per_note(part, NoteExpression::Tune((f64::from(data) / 2_147_483_648.0 - 1.0) * 48.0)),
        (2 | 4, 0xb0) if a == 120 => {
            let _ = part.runtime.all_sound_off(WIRE);
        }
        (2 | 4, 0xb0) if a == 123 => {
            let _ = part.runtime.all_notes_off(WIRE);
        }
        (2, 0x80 | 0x90 | 0xb0 | 0xc0 | 0xd0 | 0xe0) => wire_event(part, status | channel, a, b),
        // Notes, controllers, registered controllers (bend range), pressure, bend.
        (4, 0x80 | 0x90 | 0xb0 | 0xc0 | 0xd0 | 0xe0 | 0x20) => wire_packet(part, &[word, data]),
        _ => part.problems.ignored_input += 1,
    }
}

/// `event` into one part.
fn deliver(part: &mut Part, index: usize, held: &mut Vec<Held>, overflow: &mut u64, event: Event) {
    match event {
        Event::NoteOn { note, velocity, tune } => {
            if held.len() == HELD {
                *overflow += 1;
                return;
            }
            // The driver sees the note as MIDI on its own channel.
            let key = u32::from(note.key & 127) << 8 | (velocity.clamp(0.0, 1.0) * 127.0).round().max(1.0) as u32;
            if !articulated(part, &[0x2090_0000 | u32::from(note.channel & 15) << 16 | key]) {
                if part.consumed_hosts.len() < part.consumed_hosts.capacity() { part.consumed_hosts.push(note); }
                else { *overflow += 1; }
                return;
            }
            let input = host_input(note, part.mpe_zone);
            if let Some(script) = part.script.as_mut() {
                // The scripts play the part's notes: the physical one is silent.
                let velocity = velocity.clamp(0.0, 1.0);
                match part.runtime.note_on(input, note.key, velocity) {
                    Ok(id) => {
                        held.push(Held { part: index, note, input, id });
                        match script.note_on(&mut part.runtime, id, note.key, velocity) {
                            Err(sampler_core::Error::Capacity) => part.problems.capacity_drops += 1,
                            _ => {}
                        }
                    }
                    Err(sampler_core::Error::Capacity) => part.problems.capacity_drops += 1,
                    Err(_) => {}
                }
                return;
            }
            let id = match part.mpe.trigger(&mut part.runtime, input.channel, input, velocity.clamp(0.0, 1.0)) {
                Ok(id) => id,
                Err(ApplyError::Core(sampler_core::Error::Capacity)) => {
                    part.problems.capacity_drops += 1;
                    return;
                }
                Err(_) => return,
            };
            held.push(Held { part: index, note, input, id });
            if tune != 0.0 {
                express(&mut part.runtime, id, |e| e.pitch_semitones += tune);
            }
        }
        Event::NoteOff(pattern) | Event::Choke(pattern) => {
            // Release consumed presses by the exact host identity captured at onset,
            // even if the mapping or driver changed while that key was down.
            let mut at = 0;
            while at < part.consumed_hosts.len() {
                if pattern.matches(part.consumed_hosts[at]) {
                    let note = part.consumed_hosts.swap_remove(at);
                    let _ = articulated(part, &[0x2080_0000 | u32::from(note.channel & 15) << 16 | u32::from(note.key & 127) << 8]);
                } else { at += 1; }
            }
            for h in held.iter().filter(|h| h.part == index && pattern.matches(h.note)) {
                if let Some(script) = part.script.as_mut() {
                    let _ = script.note_off(&mut part.runtime, h.note.key);
                }
                let _ = part.runtime.note_off(h.input, None);
            }
        }
        Event::Expression(pattern, expression) => {
            for h in held.iter().filter(|h| h.part == index && pattern.matches(h.note)) {
                note_expression(part, h.id, expression);
            }
        }
        Event::Ump(words) => packet(part, index, held, words),
    }
}

impl V2Core {
    pub fn with_parts(parts: usize, sample_rate: f64) -> Self {
        let mut mix = Mix::default();
        mix.parts.resize(parts.max(mix.parts.len()), Default::default());
        mix.articulation_routes.resize(parts.max(mix.parts.len()), None);
        let empty_editor_offsets:Arc<[sampler_core::EngineParameterOffset]>=Arc::from([]);
        mix.editor_offsets.resize(parts.max(mix.parts.len()),empty_editor_offsets.clone());
        let mut peaks = Peaks::default();
        peaks.parts.resize(parts.max(peaks.parts.len()), [0.0; 2]);
        Self {
            parts: (0..parts).map(|_| None).collect(),
            performance: None,
            align: crate::timing::Align::with_slots(parts, mix.timing.clone()),
            holding: false,
            aligned_buses: Box::new([[[0.;MAX_BLOCK];2];BUSES]),
            aligned_tap: Box::new([0.;MAX_BLOCK]),
            exact_work: Vec::with_capacity(HELD),
            rate: sample_rate,
            mix,
            empty_editor_offsets,
            held: Vec::with_capacity(HELD),
            overflow: 0,
            buses: Box::new([[[0.0; MAX_BLOCK]; 2]; BUSES]),
            written: [false; BUSES],
            signal_trace_active: false,
            scratch: Box::new([[0.0; 2]; MAX_BLOCK]),
            direct: Box::new([[[0.0; 2]; MAX_BLOCK]; BUSES]),
            tap: None,
            tapped: Box::new([0.0; MAX_BLOCK]),
            peaks,
        }
    }

    /// Adopt larger worker-prepared storage, keeping every playing part.
    /// The replaced storage stays in `grown` to be dropped off audio.
    pub fn adopt(&mut self, grown: &mut Self) {
        self.align.adopt_parts(&mut grown.align);
        for (old, new) in self.parts.iter_mut().zip(&mut grown.parts) {
            std::mem::swap(old, new);
        }
        std::mem::swap(&mut self.parts, &mut grown.parts);
        for (old, new) in self.mix.parts.iter().zip(&mut grown.mix.parts) {
            *new = *old;
        }
        std::mem::swap(&mut self.mix.parts, &mut grown.mix.parts);
        for (old,new) in self.mix.editor_offsets.iter().zip(&mut grown.mix.editor_offsets) {*new=old.clone();}
        std::mem::swap(&mut self.mix.editor_offsets,&mut grown.mix.editor_offsets);
        // Port v1 PreparedGrowth::preserve!(routers): keep the larger storage.
        for (old,new) in self.mix.articulation_routes.iter_mut().zip(&mut grown.mix.articulation_routes) {std::mem::swap(old,new);}
        std::mem::swap(&mut self.mix.articulation_routes, &mut grown.mix.articulation_routes);
        for (old, new) in self.peaks.parts.iter().zip(&mut grown.peaks.parts) {
            *new = *old;
        }
        std::mem::swap(&mut self.peaks.parts, &mut grown.peaks.parts);
    }

    fn render_chunk(&mut self, frames: usize) -> Rendered<'_> {
        let n = frames.min(MAX_BLOCK);
        for bus in self.buses.iter_mut() {
            bus[0][..n].fill(0.0);
            bus[1][..n].fill(0.0);
        }
        self.written = [false; BUSES];
        self.signal_trace_active = false;
        self.tapped[..n].fill(0.0);
        let solo = self.mix.parts.iter().take(self.parts.len()).any(|c| c.solo);
        for (index, part) in self.parts.iter_mut().enumerate() {
            let Some(part) = part else { continue };
            // Native replies apply backpressure; bounded ingress is serviced before rendering.
            for _ in 0..256 {
                if !matches!(part.runtime.poll_control_update(), Ok(Some(_))) { break; }
            }
            let pairs = |direct: u32| (0..BUSES).filter(move |pair| direct & 1 << pair != 0);
            for pair in pairs(part.direct) {
                self.direct[pair][..n].fill([0.0; 2]);
            }
            // About 128 instructions per frame, so a long block keeps its script
            // throughput per second; never below the 64-frame measured 8192.
            let fuel = (n * 128).max(8192);
            if part.runtime.behavior_block_fuel() != fuel {
                part.runtime.set_behavior_block_fuel(fuel);
            }
            if let Some(script) = part.script.as_mut() {
                let _ = script.wake(&mut part.runtime);
                // What the scripts generated plays into the part.
                let mut midi = [None; 64];
                let mut n = 0;
                script.drain_midi(|out| {
                    if n < midi.len() {
                        midi[n] = Some(out);
                        n += 1;
                    }
                });
                for out in midi.iter().flatten() {
                    wire_event(part, out.status, out.a, out.b);
                }
            }
            if let Some(horizon) = part.horizon {
                // Pending pages play silent and count as underruns.
                if let Err(error) = part.runtime.service_streaming(horizon) {
                    record_stream_error(&mut part.problems, error);
                }
            }
            let out = &mut self.scratch[..n];
            let mut outs: [&mut [Frame]; BUSES] = self.direct.each_mut().map(|d| &mut d[..n]);
            if part.runtime.render_split(out, &mut outs).is_err() {
                if let Some(error) = part.runtime.take_stream_fault() {
                    part.problems.offline_failures += 1;
                    record_stream_error(&mut part.problems, error);
                }
                continue;
            }
            let cutoff = if part.runtime.has_input_tone() { 20_000. } else { self.performance.map_or(20_000., |p| p[2]) };
            let at = part.runtime.now().saturating_sub(n as u64);
            let _ = part.tone.process(out, &mut part.tone_history[BUSES], cutoff, at);
            for pair in pairs(part.direct) {
                let _ = part.tone.process(&mut self.direct[pair][..n], &mut part.tone_history[pair], cutoff, at);
            }
            if let Some((program, error)) = part.runtime.take_fault() {
                part.problems.fault_program = program as u64 + 1;
                part.problems.fault_error = sampler_core::Error::ALL.iter().position(|e| *e == error).unwrap_or(0) as u64;
            }
            if let Some(silent) = part.runtime.take_silent_note() {
                part.problems.silent_notes += 1;
                part.problems.silent = silent.pack();
            }
            let c = self.mix.parts[index];
            if part.runtime.signal_trace_enabled() {
                self.signal_trace_active = true;
                part.runtime.trace_host_frames(sampler_core::trace::HostStage::PartFader, out, balance(c.gain, c.pan), !(c.mute || solo && !c.solo), usize::from(c.output).min(BUSES - 1));
            }
            if c.mute || solo && !c.solo {
                continue;
            }
            // Nodes routed to their own pairs leave the instrument's fader.
            for pair in pairs(part.direct) {
                self.written[pair] = true;
                let [bl, br] = &mut self.buses[pair];
                for ((ol, or), [l, r]) in bl[..n].iter_mut().zip(&mut br[..n]).zip(self.direct[pair][..n].iter()) {
                    *ol += l;
                    *or += r;
                }
            }
            let [gl, gr] = balance(c.gain, c.pan);
            let mut level = [0f32; 2];
            for [l, r] in out.iter_mut() {
                *l *= gl;
                *r *= gr;
                level = [level[0].max(l.abs()), level[1].max(r.abs())];
            }
            let meter = &mut self.peaks.parts[index];
            *meter = [meter[0].max(level[0]), meter[1].max(level[1])];
            if level == [0.0; 2] {
                continue;
            }
            if self.tap == Some(index) {
                for (t, [l, r]) in self.tapped[..n].iter_mut().zip(out.iter()) {
                    *t = (l + r) * 0.5;
                }
            }
            let aux = usize::from(c.aux) < BUSES && c.aux != c.output && c.aux_gain != 0.0;
            if self.signal_trace_active && part.runtime.signal_trace_enabled() && aux {
                part.runtime.trace_host_frames(sampler_core::trace::HostStage::AuxSend, out, [c.aux_gain; 2], true, usize::from(c.aux));
            }
            for (bus, gain) in [(usize::from(c.output), 1.0), (usize::from(c.aux), c.aux_gain)].into_iter().take(1 + usize::from(aux)) {
                let bus = bus.min(BUSES - 1);
                self.written[bus] = true;
                let [bl, br] = &mut self.buses[bus];
                for ((ol, or), [l, r]) in bl[..n].iter_mut().zip(&mut br[..n]).zip(out.iter()) {
                    *ol += l * gain;
                    *or += r * gain;
                }
            }
        }
        let solo = self.mix.buses.iter().any(|c| c.solo);
        for (bus, c) in self.mix.buses.iter().enumerate().filter(|(bus, _)| self.written[*bus]) {
            let gains = if c.mute || solo && !c.solo { [0.0; 2] } else { balance(c.gain, c.pan) };
            if self.signal_trace_active {
                for (index, part) in self.parts.iter_mut().enumerate() {
                    let Some(part) = part else { continue };
                    let settings = self.mix.parts[index];
                    let routed = usize::from(settings.output).min(BUSES - 1) == bus
                        || usize::from(settings.aux) == bus && settings.aux_gain != 0.
                        || part.direct & (1 << bus) != 0;
                    if part.runtime.signal_trace_enabled() && routed {
                        part.runtime.trace_host_planar(sampler_core::trace::HostStage::RackBus(bus),
                            &self.buses[bus][0][..n], &self.buses[bus][1][..n], gains, None,
                            !(c.mute || solo && !c.solo), usize::from(c.port));
                    }
                }
            }
            let meter = &mut self.peaks.buses[bus];
            for ((signal, g), m) in self.buses[bus].iter_mut().zip(gains).zip(meter.iter_mut()) {
                if g != 1.0 {
                    signal[..n].iter_mut().for_each(|x| *x *= g);
                }
                *m = m.max(peak(&signal[..n]));
            }
        }
        for part in self.parts.iter_mut().flatten() {
            if let Some(state) = part.persistence.as_mut() { state.publish(&part.runtime); }
        }
        Rendered { buses: &self.buses, live: self.written }
    }

    /// Port v1 Align::release: stable original events, with their captured articulation.
    fn release_aligned(&mut self, now:u64) {
        for slot in 0..self.parts.len() {
            while let Some((event,row))=self.align.parts[slot].pop_due(now) {
                if row!=crate::timing::NO_ART && let Some(Some(p))=self.parts.get_mut(slot)
                    && let Some(Some(action))=p.source_actions.get(row)
                    && p.articulator.as_mut().is_some_and(|a|a.select(&mut p.runtime,*action).is_ok()) {
                    p.force_articulation_once=true;
                }
                self.deliver(slot,event);
                if let Some(Some(p))=self.parts.get_mut(slot) {p.force_articulation_once=false;}
            }
        }
    }
    fn flush_aligned(&mut self) {
        self.release_aligned(u64::MAX);
        for s in &mut self.align.parts {s.cancel();}
    }
    fn enqueue(&mut self,slot:usize,event:Event) {
        // Wildcards may also reach owners which predate alignment. Dispatch
        // only those exact tuples immediately; the scheduler retains its own holds.
        if let Event::NoteOff(pattern)|Event::Choke(pattern)|Event::Expression(pattern,_)=event
            && self.align.parts[slot].hosts().any(|n|pattern.matches(n)) {
            self.exact_work.clear();
            for h in self.held.iter().filter(|h|h.part==slot&&pattern.matches(h.note)) {
                if !self.align.parts[slot].hosts().any(|n|n==h.note) {self.exact_work.push(h.note);}
            }
            for at in 0..self.exact_work.len() {
                let note=self.exact_work[at];
                let p=super::event::HostPattern{port:i32::from(note.port),channel:i32::from(note.channel),key:i32::from(note.key),id:note.id,clap:note.clap};
                self.deliver(slot,match event{Event::NoteOff(_)=>Event::NoteOff(p),Event::Choke(_)=>Event::Choke(p),Event::Expression(_,x)=>Event::Expression(p,x),_=>unreachable!()});
            }
        }
        let Some(Some(p))=self.parts.get_mut(slot) else {return};
        let keys=p.user_route.as_ref().map(|r|r.keys.as_slice()).unwrap_or_else(|| {
            let driver=if p.switching&0x80!=0 {usize::from(p.switching>>1&7)}else{p.inherited};
            p.drivers.get(driver).map_or(&[],|(_,keys)|keys.as_slice())
        });
        let router=crate::timing::Router{switching:p.runtime.switching(),keys,actions:&p.source_actions,mpe:p.mpe_zone};
        let holds=self.align.plan.parts.get(slot).map_or(&self.align.empty,|h|h.as_ref());
        if let Some(event)=self.align.parts[slot].arrive(event,self.align.clock,holds,self.rate,&router) {self.deliver(slot,event);}
    }

    fn reaches(&self, part: usize, port: u8, channel: Option<u8>) -> bool {
        self.mix.parts.get(part).is_some_and(|c| {
            c.port == port && (c.mpe || c.channel < 0 || channel.is_none_or(|channel| c.channel == i16::from(channel)))
        })
    }

    fn deliver(&mut self, part: usize, event: Event) {
        let Some(Some(p)) = self.parts.get_mut(part) else { return };
        deliver(p, part, &mut self.held, &mut self.overflow, event);
    }
}

fn record_stream_error(problems: &mut RuntimeProblems, error: sampler_core::StreamError) {
    use sampler_core::StreamError;
    match error {
        StreamError::Capacity => problems.stream_capacity += 1,
        StreamError::Disconnected => problems.stream_disconnected += 1,
        StreamError::DecodeFailed(_) => problems.stream_failed += 1,
        _ => problems.stream_errors += 1,
    }
}

impl Core for V2Core {
    /// `None` empties the part.
    type Prepared = Option<Box<Part>>;
    type Retired = Retired;

    fn parts(&self) -> usize {
        self.parts.len()
    }

    fn sample_rate(&self) -> f64 {
        self.rate
    }

    fn reset(&mut self, sample_rate: f64) {
        // ponytail: parts keep their prepared rate; the shell reloads them on a rate change.
        self.rate = sample_rate;
        self.panic();
    }

    fn panic(&mut self) {
        for s in &mut self.align.parts {s.cancel();}
        for p in self.parts.iter_mut().flatten() {
            p.runtime.panic();
            p.tone_history.fill([[0.; 2]; 2]);
        }
    }

    fn install(&mut self, part: usize, mut prepared: Option<Box<Part>>) -> Retired {
        let Some(slot) = self.parts.get_mut(part) else { return Retired(prepared) };
        self.align.parts[part].cancel();
        for held in self.held.iter_mut().filter(|h| h.part == part) {
            held.part = ORPHAN;
        }
        if let (Some(p), Some(c)) = (prepared.as_mut(), self.mix.parts.get(part)) {
            if let Some([attack, release, cutoff]) = self.performance {
                let _ = p.runtime.set_fallback_envelope(attack, release);
                let _ = p.runtime.set_part_tone_cutoff(cutoff);
            }
            p.apply_editor_offsets(self.mix.editor_offsets.get(part).unwrap_or(&self.empty_editor_offsets));
            p.configure(c, self.mix.articulation_routes.get(part).and_then(Option::as_ref));
        }
        Retired(std::mem::replace(slot, prepared))
    }

    fn begin_block(&mut self, block: &BlockInfo) {
        let holding=self.align.holding(block.transport.playing);
        if self.holding&&!holding {self.flush_aligned();}
        self.holding=holding;
        for part in self.parts.iter_mut().flatten() { part.runtime.set_offline(block.offline); }
    }

    fn event(&mut self, port: u8, event: Event) {
        let channel = event.channel();
        for part in 0..self.parts.len() {
            if self.reaches(part, port, channel) {
                if self.holding {self.enqueue(part,event);} else {self.deliver(part,event);}
            }
        }
    }

    fn play(&mut self, part: usize, mut event: Event) {
        // Port v1 service_channel: the part's keyboard controls its MPE manager.
        if let Event::Ump(words)=&mut event
            && matches!(words[0]>>28,2|4)
            && let Some(Some(p))=self.parts.get(part)
            && p.mpe_zone {
            words[0]=(words[0]&!0x000f_0000)|u32::from(p.mpe.manager_channel())<<16;
        }
        self.deliver(part, event);
    }

    fn key_held(&self, channel: u8, key: u8) -> bool {
        self.held.iter().any(|h| h.part != ORPHAN && h.note.channel == channel && h.note.key == key) || self.holding&&self.align.host_key_held(channel,key)
    }

    fn render(&mut self, frames: usize) -> Rendered<'_> {
        let n=frames.min(MAX_BLOCK);
        if !self.holding {
            self.align.clock=self.align.clock.saturating_add(n as u64);
            return self.render_chunk(n);
        }
        let mut at=0;let mut live=[false;BUSES];
        for b in self.aligned_buses.iter_mut() {for c in b {c[..n].fill(0.);}}
        while at<n {
            self.release_aligned(self.align.clock);
            let until=self.align.next_due().map_or(n-at,|due|due.saturating_sub(self.align.clock).min((n-at)as u64)as usize);
            if until==0 {continue;}
            self.render_chunk(until);
            self.aligned_tap[at..at+until].copy_from_slice(&self.tapped[..until]);
            for (bus,on) in self.written.iter().copied().enumerate() {
                live[bus]|=on;
                for c in 0..2 {self.aligned_buses[bus][c][at..at+until].copy_from_slice(&self.buses[bus][c][..until]);}
            }
            at+=until;self.align.clock=self.align.clock.saturating_add(until as u64);
        }
        self.written=live;
        Rendered{buses:&self.aligned_buses,live}
    }

    fn trace_master(&mut self, gains: &[f32]) -> bool {
        if !self.signal_trace_active { return false }
        let buses=if self.holding {&self.aligned_buses} else {&self.buses};
        for (index, part) in self.parts.iter_mut().enumerate() {
            let Some(part) = part else { continue };
            if !part.runtime.signal_trace_enabled() { continue }
            let settings = self.mix.parts[index];
            for bus in 0..BUSES {
                let routed = usize::from(settings.output).min(BUSES - 1) == bus
                    || usize::from(settings.aux) == bus && settings.aux_gain != 0.
                    || part.direct & (1 << bus) != 0;
                if self.written[bus] && routed {
                    part.runtime.trace_host_planar(sampler_core::trace::HostStage::Master(bus),
                        &buses[bus][0][..gains.len()], &buses[bus][1][..gains.len()],
                        [1.; 2], Some(gains), true, usize::from(self.mix.buses[bus].port));
                }
            }
        }
        true
    }

    fn trace_output(&mut self, port: usize, frames: &[[f32; 2]], channels: u8) {
        if !self.signal_trace_active { return }
        for (index, part) in self.parts.iter_mut().enumerate() {
            let Some(part) = part else { continue };
            if !part.runtime.signal_trace_enabled() { continue }
            let settings = self.mix.parts[index];
            if (0..BUSES).any(|bus| self.written[bus] && usize::from(self.mix.buses[bus].port) == port
                && (usize::from(settings.output).min(BUSES - 1) == bus
                    || usize::from(settings.aux) == bus && settings.aux_gain != 0.
                    || part.direct & (1 << bus) != 0)) {
                part.runtime.trace_host_frames(sampler_core::trace::HostStage::Output(port, channels), frames, [1.; 2], true, port);
            }
        }
    }

    fn owns(&self, note: HostNote) -> bool {
        self.held.iter().any(|h| h.part != ORPHAN && h.note == note) || self.align.parts.iter().any(|s|s.hosts().any(|n|n==note))
    }

    fn end_block(&mut self, _frames: usize, end: &mut dyn FnMut(HostNote) -> bool) -> u64 {
        let Self { parts, held, align, .. } = self;
        // A layered note ends once its last part lets it go.
        let last = |held: &[Held], at: usize| held.iter().filter(|h| h.note == held[at].note).count() == 1;
        // Notes of replaced parts end now; their sound went with the part.
        let mut i = 0;
        while i < held.len() {
            if held[i].part != ORPHAN || align.host_note_waiting(held[i].note) {
                i += 1;
            } else if !held[i].note.clap || !last(held, i) || end(held[i].note) {
                let note=held[i].note;let final_owner=last(held,i);held.swap_remove(i);if final_owner {align.retire_host_note(note);}
            } else {
                return 1;
            }
        }
        let mut refused = 0;
        for (index, part) in parts.iter_mut().enumerate() {
            let Some(part) = part else { continue };
            part.runtime.flush_behaviors_at(|_, _, outcome, program| {
                if matches!(outcome, sampler_core::Outcome::Fault(_) | sampler_core::Outcome::FuelExhausted) {
                    part.fault_inbox.record(program, outcome);
                }
                if let sampler_core::Outcome::Fault(error) = outcome {
                    part.problems.fault_program = program as u64 + 1;
                    part.problems.fault_error = sampler_core::Error::ALL.iter().position(|e| *e == error).unwrap_or(0) as u64;
                }
                true
            });
            part.runtime.flush_ended(|input| {
                let Some(at) = held.iter().position(|h| h.part == index && h.input == input) else { return true };
                let note=held[at].note;
                if align.host_note_waiting(note) {return false;}
                if note.clap && last(held, at) && !end(note) {
                    refused = 1;
                    return false;
                }
                let final_owner=last(held,at);held.swap_remove(at);if final_owner {align.retire_host_note(note);}
                true
            });
            if refused > 0 {
                break;
            }
        }
        // Port v1 finish_host_notes: a queued root which failed admission
        // still ends exactly once after key-up and all delayed exact work.
        if refused==0 {
            let mut at=0;
            while let Some((note,down))=align.host_note_at(at) {
                if down||align.host_note_waiting(note)||held.iter().any(|h|h.note==note) {at+=1;continue;}
                if !note.clap||end(note) {align.retire_host_note(note);}else{refused=1;break;}
            }
        }
        refused
    }

    fn set_performance(&mut self, attack: f64, release: f64, cutoff: f64) {
        if !(0.0001..=5.).contains(&attack) || !(0.001..=10.).contains(&release)
            || !(20.0..=20_000.).contains(&cutoff) { return; }
        let next = Some([attack, release, cutoff]);
        if self.performance == next { return; }
        self.performance = next;
        for part in self.parts.iter_mut().flatten() {
            let _ = part.runtime.set_fallback_envelope(attack, release);
            let _ = part.runtime.set_part_tone_cutoff(cutoff);
        }
    }

    fn set_mix(&mut self, mix: &Mix) {
        self.align.plan=mix.timing.clone();
        // Field-wise so the parts vector keeps its audio-thread allocation.
        for (to, from) in self.mix.parts.iter_mut().zip(&mix.parts) {
            *to = *from;
        }
        self.mix.buses = mix.buses;
        for (slot,to) in self.mix.editor_offsets.iter_mut().enumerate() {*to=mix.editor_offsets.get(slot).unwrap_or(&self.empty_editor_offsets).clone();}
        for (to, from) in self.mix.articulation_routes.iter_mut().zip(mix.articulation_routes.iter().chain(std::iter::repeat(&None))) { *to = from.clone(); }
        for (index, (p, c)) in self.parts.iter_mut().zip(&self.mix.parts).enumerate() {
            let Some(p) = p else { continue };
            p.apply_editor_offsets(&self.mix.editor_offsets[index]);
            p.configure(c, mix.articulation_routes.get(index).and_then(Option::as_ref));
            p.mix_nodes(mix.nodes.get(index).map_or(&[], Vec::as_slice));
        }
    }

    fn bus_ports(&self) -> [u8; BUSES] {
        self.mix.buses.map(|b| b.port)
    }

    fn set_tap(&mut self, part: Option<usize>) {
        self.tap = part;
    }

    fn tapped(&self, frames: usize) -> Option<&[f32]> {
        self.tap.map(|_| if self.holding {&self.aligned_tap[..frames.min(MAX_BLOCK)]}else{&self.tapped[..frames.min(MAX_BLOCK)]})
    }

    fn peaks_mut(&mut self) -> &mut Peaks {
        &mut self.peaks
    }

    fn take_node_peaks(&mut self, part: usize, each: &mut dyn FnMut(usize, [f32; 2])) {
        let Some(Some(p)) = self.parts.get_mut(part) else { return };
        let nodes = &p.buses;
        p.runtime.take_bus_peaks(|bus, peak| {
            if let Some(node) = nodes.iter().position(|&b| b == Some(bus)) {
                each(node, peak);
            }
        });
    }

    fn take_effects(&mut self, part: usize, each: &mut dyn FnMut(usize, &sampler_core::Effect) -> bool) {
        let Some(Some(p)) = self.parts.get_mut(part) else { return };
        p.runtime.drain_effects(|e| e.instance.is_none_or(|i| each(usize::from(i.0), e)));
    }

    fn voice_taps(&self, part:usize)->[Option<sampler_core::VoiceTap>;16] {self.parts.get(part).and_then(Option::as_ref).map_or([None;16],|p|p.runtime.voice_taps())}

    fn voices(&self) -> Voices {
        let active = self.parts.iter().flatten().map(|p| p.runtime.voice_count()).sum();
        // Port from v1 0cb7a8a0:src/plugin.rs: rendered voices and IO/command loss.
        let audible = self.parts.iter().flatten().map(|p| p.runtime.audible_voice_count()).sum();
        let dropouts = self.parts.iter().flatten().fold(self.overflow.saturating_add(self.align.overflows()), |n, p| {
            n.saturating_add(p.runtime.stats().stream_underruns).saturating_add(p.problems.capacity_drops)
        });
        Voices { active, audible, dropouts }
    }

    fn problems(&self, part: usize) -> RuntimeProblems {
        let Some(Some(p)) = self.parts.get(part) else { return RuntimeProblems::default() };
        let stats = p.runtime.stats();
        let (lua_faults,lua_budgets) = p.script.as_ref().map_or((0,0), |s|s.ui().runtime_faults());
        RuntimeProblems {
            lua_faults,
            script_overruns: p.problems.script_overruns.saturating_add(lua_budgets),
            nonfinite: stats.nonfinite_frames,
            underruns: stats.stream_underruns,
            capacity_drops: p.problems.capacity_drops + stats.voice_drops,
            stolen_voices: p.runtime.steals(),
            refused_starts: stats.refused_starts,
            ..p.problems
        }
    }

    fn articulation(&self, part: usize) -> Option<usize> {
        let p = self.parts.get(part)?.as_ref()?;
        let default = p.articulations?;
        if let Some(id) = p.articulator.as_ref().and_then(Articulator::selected_articulation) {
            let id = id as usize;
            return Some(if id == 0 { default } else if id <= default { id - 1 } else { id });
        }
        if let Some(keys) = &p.tap_keys {
            // A script holds the selection: the last switch key, tapped or pressed.
            let tapped = p.articulator.as_ref().and_then(Articulator::selected);
            return Some(tapped.and_then(|k| keys.iter().position(|&f| f == Some(k))).unwrap_or(default));
        }
        let id = p.runtime.articulation(p.runtime.performance(0).ok()?).ok()? as usize;
        // Undo the runtime's numbering: the default is 0, those before it shift up.
        Some(match id {
            0 => default,
            id if id <= default => id - 1,
            id => id,
        })
    }

    fn clock(&self, part: usize) -> u64 {
        self.parts.get(part).and_then(Option::as_ref).map_or(0, |p| p.runtime.now())
    }

    fn latency(&self) -> u32 {
        self.align.plan.latency(self.rate)
    }

    fn select_articulation(&mut self, part: usize, articulation: usize) -> bool {
        let Some(Some(p)) = self.parts.get_mut(part) else { return false };
        let Some(Some(switch)) = p.source_actions.get(articulation) else { return false };
        let selected=p.articulator.as_mut().is_some_and(|a| a.select(&mut p.runtime, *switch).is_ok());
        if selected {self.align.parts[part].picked(articulation);}
        selected
    }

    fn set_control(&mut self, part: usize, control: sampler_ui_ir::ControlId, value: f64) -> bool {
        let Some(Some(p)) = self.parts.get_mut(part) else { return false };
        if let Some(script) = &mut p.script
            && script.ui().value(control).is_some() { return script.set_control(control, value) }
        let rt = &mut p.runtime;
        let (plan, id) = (rt.active_plan(), sampler_core::ControlId(control.0));
        let Ok(ControlDefinition { domain, .. }) = rt.control_definition(plan, id) else { return false };
        let value = match domain {
            ControlDomain::Integer { min, max } => ControlValue::Integer((value.round() as i64).clamp(min, max)),
            ControlDomain::Real { min, max } => ControlValue::Real(value.clamp(min, max)),
            ControlDomain::Toggle => ControlValue::Toggle(value >= 0.5),
        };
        let Ok(performance) = rt.performance(0) else { return false };
        let context = ControlContext { performance, origin: WIRE, channels: 1 };
        rt.invoke_control(context, plan, None, ControlWrite { id, value }).is_ok()
    }

    fn control_value(&self, part: usize, control: sampler_ui_ir::ControlId) -> Option<f64> {
        let p = self.parts.get(part)?.as_ref()?;
        if let Some(v) = p.script.as_ref().and_then(|s| s.ui().value(control)) { return Some(v) }
        let rt = &p.runtime;
        rt.control_base_value(rt.active_plan(), sampler_core::ControlId(control.0)).ok().map(number)
    }
}

fn number(value: ControlValue) -> f64 {
    match value {
        ControlValue::Integer(n) => n as f64,
        ControlValue::Real(r) => r,
        ControlValue::Toggle(on) => f64::from(u8::from(on)),
    }
}

/// Prepares [`V2Core`] parts from Kontakt instruments and WAV files.
#[derive(Default)]
pub struct V2Loader;

/// Free (available) and total RAM in bytes, from `/proc/meminfo`.
fn ram_free() -> Option<(usize, usize)> {
    let info = std::fs::read_to_string("/proc/meminfo").ok()?;
    let field = |name: &str| -> Option<usize> {
        let line = info.lines().find(|l| l.starts_with(name))?;
        let kib: usize = line[name.len()..].trim().trim_end_matches("kB").trim().parse().ok()?;
        Some(kib << 10)
    };
    Some((field("MemAvailable:")?, field("MemTotal:")?))
}

// Port from v1 bank::load_cancelable: reserve 1 GiB + one eighth of RAM.
fn stream_policy(request: &LoadRequest) -> sampler_kontakt::StreamPolicy {
    let resident_budget = (request.streaming == super::Streaming::RamOnly).then(|| {
        // ponytail: v1's /proc/meminfo probe; other systems keep its 8 GiB ceiling.
        ram_free().map_or(8 << 30, |(free, total)| free.saturating_sub((1 << 30) + total / 8))
    });
    sampler_kontakt::StreamPolicy {
        resident_budget,
        lazy: request.streaming == super::Streaming::Auto,
        head_budget: resident_budget.unwrap_or(8 << 20),
        block_frames: super::MAX_BLOCK,
        max_step: 16.0,
        ..Default::default()
    }
}

/// Voice-rendering threads per part: `KONTRA_THREADS` (`auto` or a count)
/// wins, then the player's setting; one (the audio thread alone) otherwise.
fn render_threads(request: &LoadRequest) -> Threads {
    match std::env::var("KONTRA_THREADS").as_deref() {
        Ok("auto") => Threads::Auto,
        Ok(n) => Threads::Fixed(n.parse().unwrap_or(1)),
        Err(_) => match request.threads {
            Some(super::ThreadChoice::Auto) => Threads::Auto,
            Some(super::ThreadChoice::Fixed(n)) => Threads::Fixed(n),
            None => Threads::Fixed(1),
        },
    }
}

/// Memory a part preallocates for per-voice state. Voices start sized to this,
/// not to a fixed polyphony, and the pool doubles off the audio thread (see
/// `Grower`) past three quarters full, up to `GROWTH` times as many if memory allows. A note is
/// refused only when that is exhausted too, and it is counted
/// (`RuntimeStats::voice_drops`).
const VOICE_BUDGET: usize = 256 << 20;
const MIN_VOICES: usize = 512;
// port from v1 0cb7a8a0:src/engine/mod.rs; growth remains off audio.
const MAX_VOICES: usize = 1024;
const GROWTH: usize = 8;
// port from v1 0cb7a8a0:src/ksp/runtime.rs EVENT_CAPACITY.
const INITIAL_NOTE_PARAMS: usize = 4096;
/// Per-voice bytes beyond the plan's state: voice slot, activity bit, parallel scratch.
const VOICE_OVERHEAD: usize = 4096;

/// Capacities of a part, sized for its plan's script state and voice cost, and
/// the voice count the pool may grow to. Event ownership is bounded by v1
/// capacity; multiple voices can still share each event.
fn limits(plan: &Prepared) -> (Limits, usize) {
    let voices = (VOICE_BUDGET / plan.voice_state_bytes().max(1)).clamp(MIN_VOICES, MAX_VOICES);
    let ceiling = voices * GROWTH;
    let notes = INITIAL_NOTE_PARAMS;
    let limits = Limits {
        families: (ceiling / 2).clamp(256, notes),
        decisions: (ceiling / 2).clamp(256, notes),
        ..Limits::for_plan(plan, notes, voices)
    };
    (limits, ceiling)
}

fn is_wav(path: &Path) -> bool {
    path.extension().is_some_and(|e| e.eq_ignore_ascii_case("wav"))
}

fn is_kontakt(path: &Path) -> bool {
    path.extension().is_some_and(|e| ["nki", "nkm", "nkb", "nksn"].iter().any(|k| e.eq_ignore_ascii_case(k)))
}

fn is_uvi(path: &Path) -> bool {
    path.ancestors().any(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("ufs")))
        || path.extension().is_some_and(|e| e.eq_ignore_ascii_case("uvip"))
}

fn unsupported(path: &Path) -> CoreError {
    if is_kontakt(path) || is_uvi(path) || is_wav(path) {
        CoreError::Invalid("unreadable instrument".into())
    } else {
        CoreError::Unsupported("translating this instrument format to sampler-core")
    }
}

fn insert_names(instrument: &ir::Instrument, chain: Option<ir::ChainRef>) -> Vec<String> {
    let Some(chain) = chain.and_then(|c| instrument.chains.get(c.0)) else { return Vec::new() };
    chain
        .pre_amplitude
        .iter()
        .chain(&chain.post_amplitude)
        .map(|p| match p {
            ir::Processor::Gain(_) => "Gain",
            ir::Processor::Gainer { .. } => "Gainer",
            ir::Processor::StereoModeller { .. } => "Stereo Modeller",
            ir::Processor::Pan(_) => "Pan",
            ir::Processor::LoFi { .. } => "LoFi",
            ir::Processor::StereoMatrix(_) => "Stereo",
            ir::Processor::Reverb(_) => "Reverb",
            ir::Processor::Compressor(_) => "Compressor",
            ir::Processor::Rectify(_) => "Rectify",
            ir::Processor::Daft(_) => "Daft",
            ir::Processor::LadderLP4(_) => "Ladder LP4",
            ir::Processor::SendReturnGate { .. } => "Send return gate",
            ir::Processor::Branch { .. } => "Branch",
            ir::Processor::Convolution { .. } => "Convolution",
            ir::Processor::Filter(_) => "Filter",
            ir::Processor::Delay { .. } => "Delay",
            ir::Processor::Mix { .. } => "Mix",
        })
        .map(String::from)
        .collect()
}

/// Give every group a bus of its own after the source's buses, so each is a
/// mixer node, and describe the result as the part's tree: node 0 is the
/// instrument, node `n > 0` is runtime bus `n - 1`.
fn nest(instrument: &mut ir::Instrument) -> MixTree {
    let node = |output: ir::Output| match output {
        ir::Output::Master => 0,
        ir::Output::Bus(bus) => bus.0 + 1,
    };
    let mut tree = MixTree::instrument(&instrument.name);
    for bus in &instrument.buses {
        tree.nodes.push(MixNode {
            name: bus.name.clone(),
            kind: NodeKind::Bus,
            parent: Some(node(bus.output)),
            inserts: insert_names(instrument, bus.chain),
            sends: bus.sends.iter().map(|s| (node(s.to), s.gain.linear() as f32)).collect(),
        });
    }
    // Microphone positions: a source bus the groups already play through, else
    // a bus made for the position; groups play through their mic.
    let mut mic_of = vec![None; instrument.groups.len()];
    for (name, groups) in super::mics::infer(instrument) {
        let existing = match instrument.groups[groups[0]].output {
            ir::Output::Bus(b) if instrument.buses[b.0].name == name => Some(b.0 + 1),
            _ => None,
        };
        let at = existing.unwrap_or_else(|| {
            let output = instrument.groups[groups[0]].output;
            instrument.buses.push(ir::Bus { name: name.clone(), chain: None, sends: Vec::new(), output, gain: ir::Gain::UNITY });
            tree.nodes.push(MixNode { name, kind: NodeKind::Mic, parent: Some(node(output)), inserts: Vec::new(), sends: Vec::new() });
            instrument.buses.len()
        });
        tree.nodes[at].kind = NodeKind::Mic;
        for g in groups {
            mic_of[g] = Some(at);
        }
    }
    for index in 0..instrument.groups.len() {
        let group = &instrument.groups[index];
        let name = if group.name.is_empty() { format!("Group {}", index + 1) } else { group.name.clone() };
        let output = match mic_of[index] {
            Some(at) => ir::Output::Bus(ir::BusRef(at - 1)),
            None => group.output,
        };
        tree.nodes.push(MixNode {
            name: name.clone(),
            kind: NodeKind::Group,
            parent: Some(node(output)),
            inserts: insert_names(instrument, group.chain),
            sends: Vec::new(),
        });
        let bus = ir::BusRef(instrument.buses.len());
        instrument.buses.push(ir::Bus { name, chain: None, sends: Vec::new(), output, gain: ir::Gain::UNITY });
        instrument.groups[index].output = ir::Output::Bus(bus);
        instrument.tap_group(index, bus);
        let post = instrument.groups[index].tap.as_ref().map_or(&[][..], |t| &t.post[..]);
        let fader = instrument.buses[bus.0].gain.linear();
        let sends = (instrument.buses[bus.0].sends.iter().enumerate())
            .map(|(n, s)| (node(s.to), (s.gain.linear() * if post.contains(&n) { fader } else { 1.0 }) as f32))
            .collect();
        tree.nodes.last_mut().expect("pushed above").sends = sends;
    }
    tree
}

/// A plan and, when its samples stream, the runtime's page cache.
type Plan = (Prepared, Option<StreamCache>, Option<ScriptDriver>);

/// A UVI program's Lua scripts, driving the part's runtime from their own thread.
pub type ScriptDriver = sampler_uvi::scripted::Driver<sampler_uvi::scripted::ScriptThread>;

/// The instrument a snapshot was saved from: `<name>.nki` somewhere under a
/// folder above the snapshot, named by its metadata or by the snapshot's folder.
fn snapshot_parent(snapshot: &Path, name: &str) -> Option<PathBuf> {
    fn find(dir: &Path, wanted: &[String], depth: usize) -> Option<PathBuf> {
        let mut folders = Vec::new();
        for entry in std::fs::read_dir(dir).ok()?.flatten() {
            let path = entry.path();
            if path.is_dir() {
                folders.push(path);
            } else if path.extension().is_some_and(|e| e.eq_ignore_ascii_case("nki"))
                && path.file_stem().is_some_and(|s| wanted.iter().any(|w| s.eq_ignore_ascii_case(w.as_str())))
            {
                return Some(path);
            }
        }
        if depth == 0 {
            return None;
        }
        folders.sort();
        folders.iter().find_map(|d| find(d, wanted, depth - 1))
    }
    let folder = snapshot.parent()?.file_name()?.to_string_lossy().into_owned();
    let wanted: Vec<String> = [name.to_owned(), folder].into_iter().filter(|n| !n.is_empty()).collect();
    snapshot.ancestors().skip(1).take(4).find_map(|dir| find(dir, &wanted, 3))
}

fn kontakt(
    request: &LoadRequest,
    progress: &mut dyn FnMut(Progress),
    canceled: &(dyn Fn() -> bool + Sync),
) -> Result<Loaded<Plan>, CoreError> {
    let load = |e: sampler_kontakt::LoadError| match e {
        sampler_kontakt::LoadError::Canceled => CoreError::Canceled,
        e => CoreError::Load((&e).into()),
    };
    let extension = |e: &str| request.path.extension().is_some_and(|x| x.eq_ignore_ascii_case(e));
    // A snapshot is the saved state of an instrument found beside it in the library.
    let snapshot = if let Some(snapshot) = &request.snapshot {
        let state = sampler_kontakt::read_snapshot(snapshot).map_err(load)?;
        let identity = snapshot_instrument(snapshot).and_then(|name| {
            ensure!(name == snapshot_base_name(&request.path)?, "Snapshot requires base instrument {name:?}");
            Ok(())
        });
        identity.map_err(|e| CoreError::Load(LoadFailure::message(e.to_string())))?;
        Some((request.path.clone(), state))
    } else if extension("nksn") {
        let state = sampler_kontakt::read_snapshot(&request.path).map_err(load)?;
        let parent = snapshot_parent(&request.path, &state.instrument).ok_or_else(|| {
            CoreError::Load(LoadFailure::message(format!("no instrument \"{}\" found for snapshot", state.instrument)))
        })?;
        Some((parent, state))
    } else {
        None
    };
    let path = snapshot.as_ref().map_or(&request.path, |(parent, _)| parent);
    let mut source = match &snapshot {
        Some((parent, state)) => sampler_kontakt::read_with_snapshot(parent, state),
        None if extension("nkm") => sampler_kontakt::read_program(path, request.program as usize),
        None => sampler_kontakt::read(path),
    }
    .map_err(load)?;
    let mut report = LoadReport::of(&source.instrument, &request.path, source.locations.len());
    let tree = nest(&mut source.instrument);
    let options = sampler_kontakt::Options {
        rate: request.sample_rate as u32,
        library: Some(path.clone()),
        mpe: request.mpe.then(|| sampler_core::lower::MpeDefaults::for_instrument(&source.instrument)),
        dynamics_start: request.dynamics_start,
        control_values: request.control_values.iter().filter(|(_, value)| value.is_finite()).map(|&(id, value)| (sampler_core::ControlId(id.0), value.round().clamp(f64::from(i32::MIN), f64::from(i32::MAX)) as i32)).collect(),
        ..Default::default()
    };
    let progress = |p: sampler_kontakt::Progress<'_>| {
        progress(Progress(match p {
            sampler_kontakt::Progress::Translated { .. } => 50,
            sampler_kontakt::Progress::Decoding { done, total, .. } => (100 + 800 * done / total.max(1)) as u16,
            sampler_kontakt::Progress::Lowering => 950,
        }))
    };
    if canceled() {
        return Err(CoreError::Canceled);
    }
    let policy = stream_policy(request);
    let streamed = sampler_kontakt::load_read_streamed_cancelable(source, &options, &policy, progress, canceled).map_err(load)?;
    let sampler_kontakt::Streamed { loaded, assets, cache, streamer, report: stream } = streamed;
    report.decoded.full_bytes = stream.full_bytes;
    report.decoded.dynamics = loaded.dynamics().iter().map(|&(cc, v)| (cc, (v * 127.).round().clamp(0., 127.) as u8)).collect();
    report.decoded.needs_controller = loaded.needs_controller();
    // Loading adds what it found unplayable (samples, keys) and scripts that failed.
    report.missing = loaded.instrument.unsupported.iter().map(Missing::from).collect();
    report.decoded.zones = loaded.instrument.zones.len();
    report.decoded.keys = super::report::key_bits(&loaded.instrument);
    report.decoded.samples = loaded.plan.sample_count();
    Ok(Loaded {
        part: (loaded.plan, Some(cache), None),
        tree,
        report,
        interfaces: loaded.interfaces,
        controls: Vec::new(),
        instrument: Some(Arc::new(loaded.instrument)),
        scripts: ScriptUi { views: loaded.scripts, resources: loaded.resources, ..Default::default() },
        stream: Some(Arc::new(Stream { streamer, assets, report: stream })),
    })
}

/// A UVI program (loose or in a bank): its layers become mixer nodes as groups do. Samples
/// stream from the bank or file; its Lua scripts run on their own thread.
fn uvi(request: &LoadRequest) -> Result<Loaded<Plan>, CoreError> {
    let load = |e: &dyn std::fmt::Display| CoreError::Load(LoadFailure::message(e));
    let span = sampler_kontakt::audit::Span::new("uvi_read_translate");
    let mut t = sampler_uvi::translate_path(&request.path).map_err(|e| load(&*e))?;
    drop(span);
    let span = sampler_kontakt::audit::Span::new("uvi_lua_init");
    let rate = request.sample_rate as u32;
    let config = sampler_uvi::script::Config::realtime();
    #[cfg(feature = "shots")]
    let config = sampler_uvi::script::Config { audit_seed: sampler_uvi::script::audit_seed().map_err(|e| load(&e))?, ..config };
    let attached = t.attach_script_with_ui_state(rate, config, request.uvi_state.clone()).map_err(|e| load(&e))?;
    drop(span);
    let mut report = LoadReport::of(&t.instrument, &request.path, t.locations.len());
    if let Some(a) = &attached { report.uvi_faults = a.driver.ui().fault_counts(); }
    let tree = nest(&mut t.instrument);
    let streamed = sampler_uvi::assemble_translated_streamed(t, rate, &stream_policy(request)).map_err(|e| load(&*e))?;
    let sampler_kontakt::Streamed { mut loaded, assets, cache, streamer, report: stream } = streamed;
    report.decoded.full_bytes = stream.full_bytes;
    let uvi_ui = attached.as_ref().map(|a| a.driver.ui().clone());
    let driver = attached.map(|a| {
        // Loading reports a script it has no frontend for; this one runs.
        loaded.instrument.unsupported.retain(|u| !(u.feature == "script" && u.value.contains("no frontend")));
        loaded.interfaces.push(a.interface);
        a.driver
    });
    report.missing = loaded.instrument.unsupported.iter().map(Missing::from).collect();
    report.decoded.zones = loaded.instrument.zones.len();
    report.decoded.keys = super::report::key_bits(&loaded.instrument);
    report.decoded.samples = loaded.plan.sample_count();
    Ok(Loaded {
        part: (loaded.plan, Some(cache), driver),
        tree,
        report,
        interfaces: loaded.interfaces,
        controls: uvi_ui.as_ref().map(|u| u.values()).unwrap_or_default(),
        instrument: Some(Arc::new(loaded.instrument)),
        scripts: ScriptUi { uvi: uvi_ui, views: loaded.scripts, resources: loaded.resources, ..Default::default() },
        stream: Some(Arc::new(Stream { streamer, assets, report: stream })),
    })
}

fn read_wav(path: &Path) -> Result<(u32, Box<[Frame]>), CoreError> {
    let mut reader = hound::WavReader::open(path).map_err(|e| CoreError::Load(LoadFailure::message(e)))?;
    let spec = reader.spec();
    let channels = usize::from(spec.channels);
    if channels == 0 {
        return Err(CoreError::Invalid("WAV without channels".into()));
    }
    let samples: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Float => reader.samples::<f32>().collect::<Result<_, _>>(),
        hound::SampleFormat::Int => {
            let scale = 1.0 / (1u64 << (spec.bits_per_sample - 1)) as f32;
            reader.samples::<i32>().map(|s| s.map(|s| s as f32 * scale)).collect::<Result<_, _>>()
        }
    }
    .map_err(|e| CoreError::Load(LoadFailure::message(e)))?;
    let frames = samples.chunks_exact(channels).map(|f| [f[0], f[channels.min(2) - 1]]).collect();
    Ok((spec.sample_rate, frames))
}

fn stem(path: &Path) -> String {
    path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default()
}

fn wav(request: &LoadRequest) -> Result<Loaded<Plan>, CoreError> {
    let core = |e: sampler_core::Error| CoreError::Invalid(format!("{e:?}"));
    let rate = request.sample_rate as u32;
    let (source_rate, frames) = read_wav(&request.path)?;
    let pcm = Pcm::new(source_rate, frames).map_err(core)?;
    let release = (0.05 * f64::from(rate)) as u32;
    let region = Region {
        sample: 0,
        // The resampler steps at most 16x up: four octaves above the root.
        key_low: 0,
        key_high: 108,
        root_key: Some(60),
        velocity_low: 0.0,
        velocity_high: 1.0,
        gain: 1.0,
        envelope: Envelope::new(0, 0, 0, 1.0, release).map_err(core)?,
        playback: Playback::default(),
    };
    let plan = Prepared::new(rate, vec![pcm], vec![region], 128).map_err(core)?
        .with_fallback_envelopes(vec![true]).map_err(core)?;
    let name = stem(&request.path);
    let mut report = LoadReport { name: name.clone(), path: request.path.display().to_string(), ..Default::default() };
    report.decoded.format = "WAV".into();
    report.decoded.zones = 1;
    report.decoded.samples = 1;
    report.decoded.keys = super::report::range_bits(0, 108);
    Ok(Loaded {
        part: (plan, None, None),
        tree: MixTree::instrument(&name),
        report,
        interfaces: Vec::new(),
        controls: Vec::new(),
        instrument: None,
        scripts: ScriptUi::default(),
        stream: None,
    })
}

/// Stack for the loader worker: lowering moves large plans by value and the
/// caller's thread (a UI or test thread) may only have 2 MB.
const LOADER_STACK: usize = 16 << 20;

impl V2Loader {
    fn prepare_on_worker(
        &self,
        request: &LoadRequest,
        progress: &mut dyn FnMut(Progress),
        canceled: &(dyn Fn() -> bool + Sync),
    ) -> Result<Loaded<Option<Box<Part>>>, CoreError> {
        let core = |e: sampler_core::Error| CoreError::Invalid(format!("{e:?}"));
        let Loaded { part: (prepared, cache, script), tree, mut report, interfaces, instrument, scripts, stream, .. } = if is_kontakt(&request.path) {
            kontakt(request, progress, canceled)?
        } else if is_uvi(&request.path) {
            uvi(request)?
        } else if is_wav(&request.path) {
            wav(request)?
        } else {
            return Err(unsupported(&request.path));
        };
        if canceled() {
            return Err(CoreError::Canceled);
        }
        let _span = sampler_kontakt::audit::Span::new("runtime_alloc_init");
        let mut timbre = None;
        if request.mpe {
            let defaults = match &instrument {
                Some(i) if is_kontakt(&request.path) => sampler_core::lower::MpeDefaults::for_instrument(i),
                _ => Default::default(),
            };
            if let sampler_core::lower::TimbreTarget::Controller(cc) = defaults.timbre {
                timbre = Some(cc);
            }
            report.decoded.mpe = super::report::mpe_summary(&defaults);
        }
        for binding in prepared.automation_bindings() {
            if let sampler_core::AutomationSource::HostParameter(address) = binding.source
                && address >= super::HOST_AUTOMATION_SLOTS {
                report.missing.push(super::report::Missing {
                    location: format!("script slot {} UI {}", binding.source_slot, binding.ui_id),
                    feature: "standalone-derived host automation capacity".into(), value: address.to_string(),
                    reason: super::report::MissingReason::NotModeled,
                });
            }
        }
        let mut controls: Vec<_> = prepared
            .controls()
            .iter()
            .map(|c| (sampler_ui_ir::ControlId(c.id.0), number(c.default)))
            .collect();
        if let Some(script) = &script { controls.extend(script.ui().values()); }
        let (limits, ceiling) = limits(&prepared);
        let per_voice = prepared.voice_state_bytes() + VOICE_OVERHEAD;
        report.decoded.script_callbacks = limits.behaviors;
        let voices = limits.voices;
        let mut waveform_sources = std::collections::HashMap::new();
        // Both Original waveform widgets and the chrome Mapping view use this worker.
        if let Some(inst) = &instrument {
            for (zone, id) in super::waveform::source_ids(inst).into_iter().enumerate() {
                // Lowering keeps one region per IR zone in order; IDs remain physical where present.
                if let Some(pcm) = prepared.region_asset(zone) {
                    waveform_sources.insert(id, super::waveform::Source {
                        pcm: pcm.clone(), stream: stream.as_ref().and_then(|stream| stream.streamer.source(pcm.asset_id())),
                    });
                }
            }
        }
        let (runtime, control) = Runtime::with_plan_updates_and_note_capacity(
            prepared, limits, 2, 1, limits.notes.min(INITIAL_NOTE_PARAMS),
        ).map_err(core)?;
        let mut runtime = runtime.with_threads(render_threads(request));
        #[cfg(feature = "shots")]
        if let Some(seed) = sampler_uvi::script::audit_seed().map_err(CoreError::Invalid)? {
            runtime.seed_random(u64::from(seed));
            runtime.reset_native_cycles(u64::from(seed));
        }
        // A source whose first window is not resident starts silent and fades in
        // rather than being refused NotReady.
        runtime.set_cold_starts(true);
        // A chord's script work spreads over blocks: 30 notes of a 14k-instruction
        // callback measured 8.0 ms in one block unlimited, 0.70 ms at this cap
        // (sampler-perf dense-strings, 64-frame blocks).
        // render() rescales it to 128 per frame for longer blocks.
        runtime.set_behavior_block_fuel(8192);
        let streams = cache.is_some();
        if let Some(cache) = cache {
            runtime = runtime.with_stream_cache(cache);
        }
        // Full polyphony steals (released, then quietest) rather than refusing notes.
        runtime.set_voice_stealing(Some(Stealing::for_limits(runtime.sample_rate(), voices))).map_err(core)?;
        if runtime.bus_count() + 1 != tree.nodes.len() && tree.nodes.len() > 1 {
            return Err(CoreError::Invalid(format!(
                "{} mixer nodes for {} runtime buses",
                tree.nodes.len() - 1,
                runtime.bus_count()
            )));
        }
        let grower = Grower::start(&mut runtime, control, ceiling, per_voice, limits.notes)
            .map_err(|e| CoreError::Invalid(e.to_string()))?;
        let mut part = Part::new(runtime, tree.clone())?;
        part.waveform_sources = waveform_sources;
        if request.mpe_upper {
            part.mpe = Mpe::new(&part.runtime, WIRE.port, WIRE.group, Zone::Upper, 15, NOTES).map_err(core)?;
        }
        part.mpe.set_timbre_controller(timbre);
        part.grower = Some(grower);
        part.script = script.map(Box::new);
        if let Some(inst) = instrument.as_deref() {
            part.set_drivers(inst);
        }
        let articulated = instrument.as_deref().filter(|i| !i.articulations.is_empty());
        part.articulations = articulated.map(|i| i.articulations.iter().position(|a| a.default).unwrap_or(0));
        part.tap_keys = articulated
            .filter(|i| i.switching.owner == ir::SwitchOwner::Behavior)
            .map(|i| i.articulations.iter().map(|a| a.switch_keys.first().copied()).collect());
        if streams && let Some(stream) = &stream {
            // Heads bound only starts; running voices request a page ahead.
            part.horizon = Some((stream.report.head_frames.max(PAGE_FRAMES) + MAX_BLOCK) as u32);
            part._stream = Some(stream.clone());
        }
        progress(Progress::DONE);
        Ok(Loaded { part: Some(Box::new(part)), tree, report, interfaces, controls, instrument, scripts, stream })
    }
}

impl CoreLoader for V2Loader {
    type Core = V2Core;

    /// Prepares on a worker with an explicit stack; progress is relayed to the
    /// caller's (non-`Send`) callback over a channel.
    fn prepare(
        &self,
        request: &LoadRequest,
        progress: &mut dyn FnMut(Progress),
        canceled: &(dyn Fn() -> bool + Sync),
    ) -> Result<Loaded<Option<Box<Part>>>, CoreError> {
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::scope(|scope| {
            let worker = std::thread::Builder::new()
                .name("sampler-load".into())
                .stack_size(LOADER_STACK)
                .spawn_scoped(scope, move || {
                    self.prepare_on_worker(request, &mut |p| drop(tx.send(p)), canceled)
                })
                .map_err(|e| CoreError::Invalid(e.to_string()))?;
            // The sender drops when the worker ends, closing the channel.
            for p in rx {
                progress(p);
            }
            worker.join().unwrap_or_else(|_| Err(CoreError::Invalid("loader panicked".into())))
        })
    }

    fn describe(&self, path: &Path, _program: u32) -> Result<Description, CoreError> {
        if is_kontakt(path) {
            let parent = path.extension().is_some_and(|e| e.eq_ignore_ascii_case("nksn")).then(|| {
                let state = sampler_kontakt::read_snapshot(path).ok()?;
                snapshot_parent(path, &state.instrument)
            });
            let path = parent.flatten().unwrap_or_else(|| path.to_path_buf());
            let instrument = sampler_kontakt::read(&path).map_err(|e| CoreError::Load(LoadFailure::message(e)))?.instrument;
            return Ok(Description {
                name: instrument.name.clone(),
                zones: instrument.zones.len(),
                scripts: instrument.behaviors.len(),
                missing: instrument.unsupported.iter().map(Missing::from).collect(),
            });
        }
        if !is_wav(path) {
            return Err(unsupported(path));
        }
        hound::WavReader::open(path).map_err(|e| CoreError::Load(LoadFailure::message(e)))?;
        Ok(Description { name: stem(path), zones: 1, scripts: 0, missing: Vec::new() })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sound::event::HostPattern;

    #[test]
    fn input_tone_core_skips_post_filter_for_main_and_direct_outputs() {
        let samples: Vec<_>=(0..4096).map(|n|[(n as f32*0.17).sin()*0.5;2]).collect();
        let region=Region {sample:0,key_low:0,key_high:127,root_key:None,velocity_low:0.,velocity_high:1.,gain:1.,envelope:Envelope::default(),playback:Playback::default()};
        let mut filtered=samples[..128].to_vec();
        sampler_core::OutputLowPass::new(48000).unwrap().process(&mut filtered,&mut [[0.;2];2],1000.,0).unwrap();
        for f in &mut filtered {*f=f.map(f32::abs);}
        for direct in [None,Some(0),Some(1),Some(2)] {
            let buses=(0..3).map(|b|sampler_core::Bus {processors:if b==1 {vec![sampler_core::Processor::Rectify(sampler_core::Rectifier::Full)]} else {vec![]}, sends:vec![sampler_core::BusSend {bus:(b<2).then_some(b+1),gain:1.}],tail_frames:0}).collect();
            let plan=Prepared::new(48000,vec![Pcm::new(48000,samples.clone().into_boxed_slice()).unwrap()],vec![region.clone()],128).unwrap().with_buses(buses,vec![Some(0)]).unwrap().with_input_bus(1).unwrap();
            let limits=Limits::for_plan(&plan,4,4);
            let mut tree=MixTree::instrument("Tone");
            for b in 0..3 {tree.nodes.push(super::super::tree::MixNode {name:format!("bus{b}"),kind:super::super::tree::NodeKind::Bus,parent:Some(if b==2 {0} else {b+2}),inserts:vec![],sends:vec![]});}
            let part=Box::new(Part::new(Runtime::new(plan,limits).unwrap(),tree).unwrap());
            let mut c=V2Core::with_parts(1,48000.);let mut mix=Mix::default();mix.nodes[0]=vec![Default::default();3];
            if let Some(bus)=direct {mix.nodes[0][bus].output=super::super::tree::NodeOutput::Pair(1);}
            c.install(0,Some(part));c.set_mix(&mix);c.set_performance(0.002,0.15,1000.);c.play(0,Event::midi1(0x90,60,127));
            let out=c.render(128);let pair=usize::from(direct.is_some());
            for n in 0..128 {for channel in 0..2 {let expected=if direct==Some(0) {samples[n][channel]} else {filtered[n][channel]};assert!((out.buses[pair][channel][n]-expected).abs()<1e-6,"Core Tone route {direct:?} must filter once");}}
            if direct.is_some() {assert!(out.buses[0].iter().flatten().all(|x|*x==0.));}
        }
    }

    #[test]
    fn v1_global_fallback_attack_and_release_reach_audio() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("fallback.wav");
        let spec = hound::WavSpec { channels: 1, sample_rate: 48000, bits_per_sample: 32, sample_format: hound::SampleFormat::Float };
        let mut wav = hound::WavWriter::create(&path, spec).unwrap();
        for _ in 0..48000 { wav.write_sample(0.5f32).unwrap(); }
        wav.finalize().unwrap();
        let mut core = V2Core::with_parts(1, 48000.);
        core.install(0, load(&path));
        core.set_performance(0.1, 0.2, 20_000.);
        core.play(0, Event::midi1(0x90, 60, 127));
        let first = core.render(128).buses[0][0][127];
        assert!(first > 0. && first < 0.02, "100ms fallback attack must start quietly, got {first}");
        for _ in 0..40 { core.render(128); }
        core.play(0, Event::midi1(0x80, 60, 0));
        for _ in 0..24 { core.render(128); }
        assert!(core.render(128).buses[0][0][127] > 0.2, "200ms fallback release outlasts the old 50ms WAV release");
        for _ in 0..60 { core.render(128); }
        assert_eq!(core.voices().active, 0);
    }

    #[test]
    fn v1_global_tone_filters_audio_and_default_bypasses() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("tone.wav"); sine(&path);
        let energy = |cutoff| {
            let mut core = V2Core::with_parts(1, 48000.);
            core.install(0, load(&path));
            core.set_performance(0.002, 0.15, cutoff);
            core.play(0, Event::midi1(0x90, 60, 127));
            for _ in 0..30 { core.render(128); }
            let out = core.render(128);
            out.buses[0][0][..128].iter().map(|x| f64::from(*x).powi(2)).sum::<f64>()
        };
        let dry = energy(20_000.);
        let wet = energy(20.);
        assert!(dry > 1. && wet < dry * 0.01, "Tone must filter the part output: dry={dry}, wet={wet}");
    }

    fn sine(path: &Path) {
        let spec = hound::WavSpec { channels: 1, sample_rate: 48000, bits_per_sample: 16, sample_format: hound::SampleFormat::Int };
        let mut w = hound::WavWriter::create(path, spec).unwrap();
        for i in 0..48000 {
            w.write_sample(((i as f32 * 440.0 / 48000.0 * std::f32::consts::TAU).sin() * 16000.0) as i16).unwrap();
        }
        w.finalize().unwrap();
    }

    fn loud(r: &Rendered<'_>, bus: usize, frames: usize) -> bool {
        r.live[bus] && r.buses[bus][0][..frames].iter().any(|x| x.abs() > 0.01)
    }

    fn on(note: HostNote) -> Event {
        Event::NoteOn { note, velocity: 100.0 / 127.0, tune: 0.0 }
    }

    fn load(path: &Path) -> Option<Box<Part>> {
        let request = LoadRequest { path: path.into(), sample_rate: 48000.0, ..Default::default() };
        V2Loader.prepare(&request, &mut |_| {}, &|| false).unwrap().part
    }

    #[test]
    fn host_signal_trace_records_aux_and_physical_mono_sum_on_the_routed_port() {
        let pcm = Pcm::new(48000, vec![[0.2, 0.4]; 512].into_boxed_slice()).unwrap();
        let region = Region {sample:0,key_low:60,key_high:60,root_key:Some(60),velocity_low:0.,velocity_high:1.,gain:1.,envelope:Envelope::default(),playback:Playback::default()};
        let plan = Prepared::new(48000,vec![pcm],vec![region],1).unwrap().with_signal_trace(4096).unwrap();
        let runtime = {let limits=limits(&plan).0; Runtime::new(plan, limits).unwrap()};
        let reader = runtime.signal_trace_reader().unwrap();
        let part = Box::new(Part::new(runtime,MixTree::instrument("fixture")).unwrap());
        let mut core = V2Core::with_parts(1,48000.);
        core.install(0,Some(part));
        let mut mix = Mix::default();
        mix.parts[0].gain = 0.5; mix.parts[0].output = 0; mix.parts[0].aux = 1; mix.parts[0].aux_gain = 0.25;
        mix.buses[0].gain = 0.5; mix.buses[0].port = 2; mix.buses[1].port = 2;
        core.set_mix(&mix);
        core.event(0,Event::NoteOn {note:HostNote {port:0,channel:0,key:60,id:7,clap:true},velocity:1.,tune:0.});
        let physical = {
            let rendered = core.render(64);
            std::array::from_fn::<_,64,_>(|i| {
                let sum = (rendered.buses[0][0][i]+rendered.buses[0][1][i]
                    +rendered.buses[1][0][i]+rendered.buses[1][1][i])*0.25;
                [sum,0.]
            })
        };
        assert!(core.trace_master(&[0.5;64]));
        core.trace_output(2,&physical,1);
        let rows = reader.drain();
        let aux = rows.iter().find(|r|reader.graph.nodes[r.node].kind=="host_aux_send").unwrap();
        assert!((aux.output.rms[0]-0.025).abs()<1e-6);
        let output = rows.iter().find(|r|reader.graph.nodes[r.node].kind=="host_output").unwrap();
        assert_eq!(output.identity.external_port,Some(2));
        assert_eq!(output.identity.output_channels,Some(1));
        assert!((output.output.rms[0]-0.05625).abs()<1e-6);
        assert_eq!(output.output.rms[1],0.);
    }

    #[test]
    fn offline_sound_seam_waits_for_delayed_storage_without_changing_pcm() {
        fn player(pcm: Pcm, cache: Option<StreamCache>) -> V2Core {
            let plan = Prepared::new(48000, vec![pcm], vec![Region {
                sample: 0, key_low: 0, key_high: 127, root_key: None,
                velocity_low: 0., velocity_high: 1., gain: 1.,
                envelope: Envelope::default(), playback: Playback::default(),
            }], 128).unwrap();
            let limits = Limits::for_plan(&plan, 128, 8);
            let mut runtime = Runtime::new(plan, limits).unwrap();
            runtime.set_cold_starts(true);
            let streamed = cache.is_some();
            if let Some(cache) = cache { runtime = runtime.with_stream_cache(cache); }
            let mut part = Part::new(runtime, MixTree::instrument("test")).unwrap();
            part.horizon = streamed.then_some(128);
            let mut core = V2Core::with_parts(1, 48000.);
            core.install(0, Some(Box::new(part)));
            core.begin_block(&BlockInfo { frames: 128, offline: true, ..Default::default() });
            core.event(0, on(HostNote { port: 0, channel: 0, key: 60, id: 1, clap: true }));
            core
        }
        let resident = Pcm::new(48000, vec![[0.25; 2]; PAGE_FRAMES].into_boxed_slice()).unwrap();
        let streamed = Pcm::headed(48000, PAGE_FRAMES, &[[0.25; 2]; 32]).unwrap();
        let (cache, mut worker) = StreamCache::new(2).unwrap();
        let ready = std::sync::Arc::new(AtomicBool::new(false));
        let done = ready.clone();
        let decoder = std::thread::spawn(move || {
            let mut job = loop {
                if let Some(job) = worker.next_job() { break job; }
                std::thread::sleep(std::time::Duration::from_micros(100));
            };
            std::thread::sleep(std::time::Duration::from_millis(15));
            job.frames_mut().fill([0.25; 2]);
            worker.complete(job, Ok(())).unwrap();
            while !done.load(Ordering::Relaxed) { std::thread::sleep(std::time::Duration::from_micros(100)); }
        });
        let mut expected = player(resident, None);
        let mut bounced = player(streamed, Some(cache));
        let expected = *expected.render(128).buses;
        let got = *bounced.render(128).buses;
        ready.store(true, Ordering::Relaxed);
        decoder.join().unwrap();
        assert!(got == expected, "offline must wait, preserving source phase and envelope");
        assert_eq!(bounced.problems(0).underruns, 0);
    }

    #[test]
    fn scripted_callbacks_retire_through_sound_seam_before_note_end() {
        use sampler_core::{Instruction, Program};
        let plan = Prepared::new(48000, vec![], vec![], 0).unwrap()
            .with_programs(vec![Program::new(vec![Instruction::End]).unwrap()], Some(0)).unwrap();
        let limits = Limits::for_plan(&plan, 128, 8);
        let runtime = Runtime::new(plan, limits).unwrap();
        let part = Part::new(runtime, MixTree::default()).unwrap();
        let mut core = V2Core::with_parts(1, 48000.);
        core.install(0, Some(Box::new(part)));
        let calls = crate::plugin::tests::allocations(|| {
        for id in 0..32 {
            let note = HostNote { port: 0, channel: 0, key: 60, id, clap: true };
            core.event(0, on(note));
            core.event(0, Event::NoteOff(HostPattern { port: -1, channel: -1, key: -1, id, clap: true }));
            core.render(64);
            assert_eq!(core.end_block(64, &mut |_| false), 1);
            let mut ended = 0;
            assert_eq!(core.end_block(64, &mut |n| { assert_eq!(n, note); ended += 1; true }), 0);
            assert_eq!(ended, 1);
            assert_eq!(core.parts[0].as_ref().unwrap().runtime.note_count(), 0);
        }
        });
        assert_eq!(calls, 0, "callback retirement allocated or freed");
    }

    #[test]
    fn host_note_plays_through_the_trait_and_ends_once() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sine.wav");
        sine(&path);
        let loader = V2Loader;
        assert_eq!(loader.describe(&path, 0).unwrap().name, "sine");
        let request = LoadRequest { path, sample_rate: 48000.0, ..Default::default() };
        let mut done = None;
        let loaded = loader.prepare(&request, &mut |p| done = Some(p), &|| false).unwrap();
        assert_eq!(done, Some(Progress::DONE));
        assert_eq!(loaded.tree.nodes.len(), 1);
        assert_eq!(loaded.report.decoded.format, "WAV");
        let part = loaded.part.as_ref().unwrap();
        assert!(part.runtime.voice_stealing().is_some(), "full polyphony steals");

        let mut core = V2Core::with_parts(2, 48000.0);
        assert!(core.install(0, loaded.part).0.is_none());
        let note = HostNote { port: 0, channel: 0, key: 60, id: 7, clap: true };
        core.begin_block(&BlockInfo { frames: 64, ..Default::default() });
        core.event(0, on(note));
        assert!(core.owns(note));
        assert!(core.key_held(0, 60));
        assert!(loud(&core.render(64), 0, 64));
        assert_eq!(core.voices().active, 1);
        assert_eq!(core.end_block(64, &mut |_| panic!("still held")), 0);

        core.event(0, Event::NoteOff(HostPattern { port: -1, channel: -1, key: 60, id: -1, clap: true }));
        let mut ended = Vec::new();
        for _ in 0..100 {
            core.render(64);
            core.end_block(64, &mut |n| {
                ended.push(n);
                true
            });
        }
        assert_eq!(ended, [note]);
        assert!(!core.owns(note));
        assert!(!loud(&core.render(64), 0, 64));
    }

    #[test]
    fn refused_note_end_is_retried_and_replaced_runtimes_orphan_their_notes() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.wav");
        sine(&path);
        let mut core = V2Core::with_parts(1, 48000.0);
        core.install(0, load(&path));
        let note = HostNote { port: 0, channel: 0, key: 64, id: 3, clap: true };
        core.event(0, on(note));
        assert!(core.install(0, load(&path)).0.is_some());
        assert!(!core.owns(note));
        assert_eq!(core.end_block(64, &mut |_| false), 1);
        let mut ended = Vec::new();
        assert_eq!(core.end_block(64, &mut |n| { ended.push(n); true }), 0);
        assert_eq!(ended, [note]);
    }

    #[test]
    fn pedals_bends_and_the_mix_reach_host_notes() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.wav");
        sine(&path);
        let mut core = V2Core::with_parts(1, 48000.0);
        core.install(0, load(&path));
        let note = HostNote { port: 0, channel: 3, key: 60, id: 5, clap: true };
        let off = Event::NoteOff(HostPattern { port: -1, channel: -1, key: -1, id: 5, clap: true });
        let expression = |core: &V2Core| {
            let part = core.parts[0].as_ref().unwrap();
            let h = core.held[0];
            part.runtime.expression(part.runtime.expression_id(h.id).unwrap()).unwrap()
        };
        core.event(0, Event::midi1(0xb3, 64, 127));
        core.event(0, on(note));
        core.event(0, Event::midi1(0xe3, 127, 127));
        assert!((expression(&core).pitch_semitones - 2.0).abs() < 1e-3, "bend on any channel moves the part");
        // MIDI 2.0 polyphonic pressure reaches the host note at full precision.
        core.event(0, Event::Ump([0x40a3_3c00, 0x8000_0000]));
        assert_eq!(expression(&core).pressure, 0x8000_0000);
        core.event(0, off);
        for _ in 0..20 {
            core.render(128);
            core.end_block(128, &mut |_| panic!("sustain holds the note"));
        }
        assert!(core.owns(note));

        let mut mix = Mix::default();
        mix.parts[0].pan = 1.0;
        mix.parts[0].tune = -12.0;
        mix.parts[0].aux = 2;
        mix.parts[0].aux_gain = 0.5;
        mix.buses[2].mute = true;
        core.set_mix(&mix);
        assert!((expression(&core).pitch_semitones - -10.0).abs() < 1e-3, "tune adds to the bend");
        let r = core.render(128);
        assert!(r.live[0] && r.live[2]);
        assert!(r.buses[0][0][..128].iter().all(|x| *x == 0.0), "panned hard right");
        assert!(r.buses[0][1][..128].iter().any(|x| x.abs() > 0.01));
        assert!(r.buses[2][1][..128].iter().all(|x| *x == 0.0), "aux bus muted");

        // MIDI 2.0 sustain off, narrowed to the zone's MIDI 1.0.
        core.event(0, Event::Ump([0x40b3_4000, 0]));
        let mut ended = Vec::new();
        for _ in 0..100 {
            core.render(128);
            core.end_block(128, &mut |n| { ended.push(n); true });
        }
        assert_eq!(ended, [note]);
        core.event(0, Event::Ump([0x40c3_0000, 0]));
        assert_eq!(core.problems(0).ignored_input, 1, "program change is counted, not dropped silently");
    }

    #[test]
    fn v1_upper_mpe_zone_uses_channel_sixteen_as_manager() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("upper.wav"); sine(&path);
        let request = LoadRequest { path, sample_rate: 48000.0, mpe: true, mpe_upper: true, ..Default::default() };
        let loaded = V2Loader.prepare(&request, &mut |_| {}, &|| false).unwrap();
        let mut core = V2Core::with_parts(1, 48000.0);
        let mut mix = Mix::default(); mix.parts[0].mpe = true; mix.parts[0].bend_range = 12;
        core.set_mix(&mix); core.install(0, loaded.part);
        core.event(0, on(HostNote { port: 0, channel: 1, key: 60, id: 1, clap: true }));
        core.event(0, on(HostNote { port: 0, channel: 2, key: 64, id: 2, clap: true }));
        core.event(0, Event::midi1(0xef, 127, 127));
        let part = core.parts[0].as_ref().unwrap();
        for held in &core.held[..2] {
            let expression = part.runtime.expression_id(held.id).unwrap();
            assert!(part.runtime.expression(expression).unwrap().pitch_semitones > 1.0, "upper manager bends every member");
        }
    }

    #[test]
    fn v1_upper_mpe_keyboard_controls_reach_every_member() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("upper.wav"); sine(&path);
        let request = LoadRequest { path, sample_rate: 48000.0, mpe: true, mpe_upper: true, ..Default::default() };
        let loaded = V2Loader.prepare(&request, &mut |_| {}, &|| false).unwrap();
        let mut core = V2Core::with_parts(1, 48000.0);
        let mut mix = Mix::default(); mix.parts[0].mpe = true; mix.parts[0].bend_range = 12;
        core.set_mix(&mix); core.install(0, loaded.part);
        for (channel, key) in [(1, 60), (2, 64)] {
            core.event(0, on(HostNote { port: 0, channel, key, id: i32::from(key), clap: true }));
        }
        for event in [Event::midi1(0xe0, 127, 127), Event::Ump([0x40e0_0000, u32::MAX])] {
            core.play(0, event);
            let part = core.parts[0].as_ref().unwrap();
            for held in &core.held[..2] {
                let expression = part.runtime.expression_id(held.id).unwrap();
                assert!(part.runtime.expression(expression).unwrap().pitch_semitones > 1.0,
                    "the keyboard bend uses the upper manager, as v1 service_channel did");
            }
            core.play(0, Event::midi1(0xe0, 0, 64));
        }
        core.event(0, Event::midi1(0xe1, 127, 127));
        let part = core.parts[0].as_ref().unwrap();
        let pitches:Vec<_>=core.held[..2].iter().map(|h| {
            part.runtime.expression(part.runtime.expression_id(h.id).unwrap()).unwrap().pitch_semitones
        }).collect();
        assert!(pitches[0]>1.0);assert_eq!(pitches[1],0.,"external member bend still affects only its own note");
    }

    #[test]
    fn mpe_member_channels_bend_their_own_notes() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.wav");
        sine(&path);
        let mut core = V2Core::with_parts(1, 48000.0);
        let mut mix = Mix::default();
        (mix.parts[0].mpe, mix.parts[0].channel, mix.parts[0].bend_range) = (true, 0, 12);
        core.set_mix(&mix);
        core.install(0, load(&path));
        let pitch = |core: &V2Core, at: usize| {
            let part = core.parts[0].as_ref().unwrap();
            let id = part.runtime.expression_id(core.held[at].id).unwrap();
            part.runtime.expression(id).unwrap().pitch_semitones
        };
        core.event(0, on(HostNote { port: 0, channel: 1, key: 60, id: 1, clap: true }));
        core.event(0, on(HostNote { port: 0, channel: 2, key: 64, id: 2, clap: true }));
        core.event(0, Event::midi1(0xe1, 127, 127));
        assert!((pitch(&core, 0) - 12.0).abs() < 0.01, "member bend at the part's range: {}", pitch(&core, 0));
        assert_eq!(pitch(&core, 1), 0.0, "another member's note stays");
        // The manager channel bends the whole zone, at its own range.
        core.event(0, Event::midi1(0xe0, 127, 127));
        assert!(pitch(&core, 1) > 1.0);
    }

    #[test]
    fn a_velocity_driver_selects_and_reports_the_articulation() {
        velocity_driver(ir::SwitchOwner::Native);
    }

    #[test]
    fn a_velocity_driver_reports_what_it_tapped_into_a_script_owned_switch() {
        velocity_driver(ir::SwitchOwner::Behavior);
    }

    fn velocity_driver(owner: ir::SwitchOwner) {
        // Key 60 in three articulations; keys 24..=26 switch; the second is the default.
        let zone = |asset, articulation| ir::Zone {
            keys: ir::KeyRange { low: 60, high: 60 },
            articulation: Some(ir::ArticulationRef(articulation)),
            ..ir::Zone::new(ir::AssetRef(asset))
        };
        let asset = |i| ir::Asset {
            location: ir::AssetLocation::Path(format!("{i}.wav")),
            encoding: ir::Encoding::Wav,
            root_key: None,
            loops: Vec::new(),
        };
        let mut instrument = ir::Instrument {
            assets: (0..3).map(asset).collect(),
            zones: (0..3).map(|a| zone(a, a)).collect(),
            articulations: (0..3u8)
                .map(|a| ir::Articulation { name: a.to_string(), switch_keys: vec![24 + a], default: a == 1, ..Default::default() })
                .collect(),
            ..Default::default()
        };
        instrument.assign_alternatives(32);
        let pcm = (0..3).map(|_| Pcm::new(48000, vec![[0.5; 2]; 4800].into_boxed_slice()).unwrap()).collect();
        let plan = sampler_kontakt::prepare(instrument.clone(), pcm, &Default::default()).unwrap().plan;
        let limits = limits(&plan).0;
        let mut part = Part::new(Runtime::new(plan, limits).unwrap(), MixTree::instrument("arts")).unwrap();
        part.articulations = Some(1);
        if owner == ir::SwitchOwner::Behavior {
            part.tap_keys = Some(vec![Some(24), Some(25), Some(26)]);
        }
        part.set_drivers(&instrument);
        let mut core = V2Core::with_parts(1, 48000.0);
        let mut mix = Mix::default();
        // Remap to velocity: the writer's Switching bits, driver in bits 1..4.
        mix.parts[0].switching = 0x80 | (ir::Driver::Velocity as u8) << 1;
        core.set_mix(&mix);
        core.install(0, Some(Box::new(part)));
        assert_eq!(core.articulation(0), Some(1), "the default plays first");
        // A script-owned switch would tag no zones, so that plan is built native
        // and only the readback is under test: it follows the last switch key.
        if owner == ir::SwitchOwner::Behavior {
            for key in [25u8, 24] {
                let note = HostNote { port: 0, channel: 0, key, id: i32::from(key), clap: true };
                core.event(0, Event::NoteOn { note, velocity: 0.5, tune: 0.0 });
                core.render(64);
                assert_eq!(core.articulation(0), Some(usize::from(key - 24)), "switch key {key}");
            }
            return;
        }
        // Velocities split 1..=127 in three by lowest switch key.
        for (velocity, articulation) in [(10.0, 0), (120.0, 2), (64.0, 1)] {
            let note = HostNote { port: 0, channel: 0, key: 60, id: velocity as i32, clap: true };
            core.event(0, Event::NoteOn { note, velocity: velocity / 127.0, tune: 0.0 });
            core.render(64);
            assert_eq!(core.articulation(0), Some(articulation), "velocity {velocity}");
        }
    }

    /// A real script-owned instrument (Afflatus Horns KS: its script reads the
    /// switch keys and selects the groups): the readback follows a switch key
    /// pressed directly and the key a velocity driver taps, and the selected
    /// articulation sounds. Set `KONTRA_KONTAKT_LIBRARIES` to run.
    #[test]
    fn a_script_owned_switch_reads_back_direct_presses_and_driver_taps() {
        let roots = std::env::var_os("KONTRA_KONTAKT_LIBRARIES").unwrap_or_default();
        let relative = "Afflatus Chapter II Brass/Instruments/1. Ensembles/Multi Instruments/2 Horns KS.nki";
        let Some(path) = std::env::split_paths(&roots).map(|r| r.join(relative)).find(|p| p.is_file()) else {
            eprintln!("skipped: {relative} is not installed");
            return;
        };
        let request = LoadRequest { path, sample_rate: 48000.0, ..Default::default() };
        let loaded = V2Loader.prepare(&request, &mut |_| {}, &|| false).unwrap();
        let instrument = loaded.instrument.clone().expect("a Kontakt instrument");
        assert_eq!(instrument.switching.owner, ir::SwitchOwner::Behavior, "script-owned");
        let keys: Vec<u8> = instrument.articulations.iter().filter_map(|a| a.switch_keys.first().copied()).collect();
        assert!(keys.len() >= 3, "{} switch keys", keys.len());
        let mut core = V2Core::with_parts(1, 48000.0);
        core.install(0, loaded.part);
        let sounds = |core: &mut V2Core| {
            let note = HostNote { port: 0, channel: 0, key: 60, id: 60, clap: true };
            core.event(0, on(note));
            let heard = (0..300).any(|_| {
                std::thread::sleep(std::time::Duration::from_millis(5));
                let r = core.render(128);
                r.live[0] && r.buses[0][0][..128].iter().any(|x| x.abs() > 1e-5)
            });
            core.event(0, Event::NoteOff(HostPattern { port: -1, channel: -1, key: i32::from(note.key), id: note.id, clap: true }));
            heard
        };
        // Direct presses: the keys driver (the instrument's own) selects.
        for (index, article) in instrument.articulations.iter().enumerate() {
            let Some(&key) = article.switch_keys.first() else { continue };
            let note = HostNote { port: 0, channel: 0, key, id: 1000 + i32::from(key), clap: true };
            core.event(0, on(note));
            core.render(128);
            core.event(0, Event::NoteOff(HostPattern { port: -1, channel: -1, key: i32::from(note.key), id: note.id, clap: true }));
            core.render(128);
            assert_eq!(core.articulation(0), Some(index), "pressed switch key {key}");
            assert!(sounds(&mut core), "articulation {index} sounds after its key");
        }
        // A velocity driver taps the keys into the script: low to high velocity
        // reaches different articulations, and each read back is one whose key
        // exists and whose zones sound.
        let mut mix = Mix::default();
        mix.parts[0].switching = 0x80 | (ir::Driver::Velocity as u8) << 1;
        core.set_mix(&mix);
        let mut seen = std::collections::BTreeSet::new();
        for velocity in [8.0, 30.0, 60.0, 90.0, 120.0] {
            let note = HostNote { port: 0, channel: 0, key: 60, id: velocity as i32, clap: true };
            core.event(0, Event::NoteOn { note, velocity: velocity / 127.0, tune: 0.0 });
            for _ in 0..8 {
                core.render(128);
            }
            core.event(0, Event::NoteOff(HostPattern { port: -1, channel: -1, key: i32::from(note.key), id: note.id, clap: true }));
            let index = core.articulation(0).expect("a readback");
            assert!(index < keys.len(), "velocity {velocity}: articulation {index}");
            seen.insert(index);
        }
        assert!(seen.len() >= 2, "velocity reached articulations {seen:?}");
    }

    #[test]
    fn v1_voice_telemetry_distinguishes_running_and_muted_voices() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.wav");
        sine(&path);
        let mut core = V2Core::with_parts(1, 48000.0);
        core.install(0, load(&path));
        let note = HostNote { port: 0, channel: 0, key: 60, id: 1, clap: true };
        let pattern = HostPattern { port: 0, channel: 0, key: 60, id: 1, clap: true };
        core.event(0, on(note));
        core.render(128);
        assert_eq!((core.voices().active, core.voices().audible), (1, 1));
        core.event(0, Event::Expression(pattern, NoteExpression::Gain(0.0)));
        core.render(128);
        assert_eq!((core.voices().active, core.voices().audible), (1, 0));
        core.event(0, Event::Expression(pattern, NoteExpression::Gain(1.0)));
        core.render(128);
        assert_eq!((core.voices().active, core.voices().audible), (1, 1));
        let part = core.parts[0].as_mut().unwrap();
        part.runtime.set_group_param(-1, sampler_core::ModTarget::Decibels, -1000.0, false).unwrap();
        core.render(128);
        assert_eq!((core.voices().active, core.voices().audible), (1, 0), "script-muted layer still runs");
        core.parts[0].as_mut().unwrap().runtime.set_group_param(-1, sampler_core::ModTarget::Decibels, 0.0, false).unwrap();
        core.render(128);
        assert_eq!((core.voices().active, core.voices().audible), (1, 1));
    }

    #[test]
    fn v1_voice_telemetry_counts_script_mutes_when_group_gain_lives_on_a_bus() {
        let mut instrument = ir::Instrument::default();
        instrument.groups.push(ir::Group::default());
        let mut zone = ir::Zone::new(ir::AssetRef(0));
        zone.group = Some(ir::GroupRef(0));
        zone.keys = ir::KeyRange { low: 60, high: 60 };
        zone.pitch = ir::KeyTracking::Tracked { root: 60 };
        instrument.zones.push(zone);
        instrument.assets.push(ir::Asset { location: ir::AssetLocation::Path("generated.wav".into()),
            encoding: ir::Encoding::Wav, root_key: None, loops: vec![] });
        let tree = nest(&mut instrument);
        let pcm = vec![Pcm::new(48000, vec![[0.5; 2]; 48000].into_boxed_slice()).unwrap()];
        let plan = sampler_kontakt::prepare(instrument, pcm, &Default::default()).unwrap().plan;
        let limits = limits(&plan).0;
        let part = Part::new(Runtime::new(plan, limits).unwrap(), tree).unwrap();
        let mut core = V2Core::with_parts(1, 48000.0);
        core.install(0, Some(Box::new(part)));
        core.event(0, on(HostNote { port: 0, channel: 0, key: 60, id: 1, clap: true }));
        core.render(128);
        assert_eq!((core.voices().active, core.voices().audible), (1, 1));
        core.parts[0].as_mut().unwrap().runtime.set_group_param(0, sampler_core::ModTarget::Decibels, -1000.0, false).unwrap();
        core.render(128);
        assert_eq!((core.voices().active, core.voices().audible), (1, 0));
    }

    #[test]
    fn v1_dropout_telemetry_includes_stream_and_lost_command_counts() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.wav");
        sine(&path);
        let mut core = V2Core::with_parts(1, 48000.0);
        core.install(0, load(&path));
        core.overflow = 2;
        core.parts[0].as_mut().unwrap().problems.capacity_drops = 3;
        assert_eq!(core.voices().dropouts, 5, "v1 sums lost commands and stream underruns");
    }

    #[test]
    fn midi2_per_note_pitch_bend_tunes_the_held_note() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.wav");
        sine(&path);
        let mut core = V2Core::with_parts(1, 48000.0);
        core.install(0, load(&path));
        core.event(0, on(HostNote { port: 0, channel: 0, key: 60, id: 1, clap: true }));
        core.event(0, Event::Ump([0x4060_3c00, 0xc000_0000]));
        let part = core.parts[0].as_ref().unwrap();
        let e = part.runtime.expression(part.runtime.expression_id(core.held[0].id).unwrap()).unwrap();
        assert!((e.pitch_semitones - 24.0).abs() < 1e-6, "three quarters of full scale is +24");
        assert_eq!(core.problems(0).ignored_input, 0);
        // CLAP gain expression up to +12 dB, unclamped.
        core.event(0, Event::Expression(super::super::event::HostPattern { port: -1, channel: -1, key: -1, id: -1, clap: true }, NoteExpression::Gain(3.0)));
        let gain = |core: &V2Core| {
            let part = core.parts[0].as_ref().unwrap();
            part.runtime.expression(part.runtime.expression_id(core.held[0].id).unwrap()).unwrap().gain
        };
        assert_eq!(gain(&core), 3.0);
        // A MIDI 2.0 channel bend keeps its 32 bits: a quarter up over ±2
        // semitones (the zone's pitch replaces the per-note bend).
        core.event(0, Event::Ump([0x40e0_0000, 0xa000_0000]));
        let part = core.parts[0].as_ref().unwrap();
        let e = part.runtime.expression(part.runtime.expression_id(core.held[0].id).unwrap()).unwrap();
        let bend = 2.0 * f64::from(0x2000_0000u32) / f64::from(0x7fff_ffffu32);
        assert!((e.pitch_semitones - bend).abs() < 1e-9, "{}", e.pitch_semitones);
        assert_eq!(core.problems(0).narrowed_input, 0);
    }

    #[test]
    fn groups_become_nodes_that_mix_and_route_to_their_own_pairs() {
        let mut instrument = ir::Instrument { name: "kit".into(), ..Default::default() };
        instrument.buses.push(ir::Bus { name: "room".into(), chain: None, sends: vec![], output: ir::Output::Master, gain: ir::Gain::UNITY });
        instrument.groups.push(ir::Group { name: "kick".into(), output: ir::Output::Bus(ir::BusRef(0)), ..Default::default() });
        instrument.groups.push(ir::Group::default());
        let tree = nest(&mut instrument);
        let names: Vec<_> = tree.nodes.iter().map(|n| (n.name.as_str(), n.parent)).collect();
        assert_eq!(names, [("kit", None), ("room", Some(0)), ("kick", Some(1)), ("Group 2", Some(0))]);
        assert_eq!(instrument.groups[0].output, ir::Output::Bus(ir::BusRef(1)));
        assert_eq!(instrument.buses[1].output, ir::Output::Bus(ir::BusRef(0)));

        // A one-group part: its group node plays to pair 3 instead of the instrument's pair 0.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.wav");
        sine(&path);
        let part = load(&path).unwrap();
        let plan = part.runtime.bus_count();
        assert_eq!(plan, 0, "a WAV part has no buses");
        let pcm = Pcm::new(48000, read_wav(&path).unwrap().1).unwrap();
        let region = Region {
            sample: 0, key_low: 0, key_high: 108, root_key: Some(60), velocity_low: 0.0, velocity_high: 1.0, gain: 1.0,
            envelope: Envelope::default(), playback: Playback::default(),
        };
        let bus = sampler_core::Bus { processors: vec![], sends: vec![sampler_core::BusSend { bus: None, gain: 1.0 }], tail_frames: 0 };
        let plan = Prepared::new(48000, vec![pcm], vec![region], 128).unwrap();
        let plan = plan.with_buses(vec![bus], vec![Some(0)]).unwrap();
        let mut tree = MixTree::instrument("one");
        tree.nodes.push(MixNode { name: "g".into(), kind: NodeKind::Group, parent: Some(0), inserts: vec![], sends: vec![] });
        let limits = limits(&plan).0;
        let part = Box::new(Part::new(Runtime::new(plan, limits).unwrap(), tree).unwrap());
        let mut core = V2Core::with_parts(1, 48000.0);
        core.install(0, Some(part));
        let mut mix = Mix::default();
        mix.nodes[0] = vec![NodeMix { output: NodeOutput::Pair(3), ..NodeMix::default() }];
        core.set_mix(&mix);
        core.event(0, on(HostNote { port: 0, channel: 0, key: 60, id: 1, clap: true }));
        let r = core.render(128);
        assert!(loud(&r, 3, 128) && !loud(&r, 0, 128), "the node left the instrument for pair 4");
        let mut nodes = Vec::new();
        core.take_node_peaks(0, &mut |node, peak| nodes.push((node, peak[0] > 0.0)));
        assert_eq!(nodes, [(1, true)], "the group node meters its signal");
        mix.nodes[0][0].mute = true;
        core.set_mix(&mix);
        assert!(!loud(&core.render(128), 3, 128), "muted node");
    }

    #[test]
    fn a_widget_edit_runs_the_scripts_ui_control_callback() {
        let source = "on init\n declare ui_knob $k(0, 100, 1)\n declare ui_knob $echo(0, 1000, 1)\nend on\n\
                      on ui_control($k)\n $echo := $k + 1\nend on\n";
        let script = sampler_ksp::compile(source, 48000, sampler_ksp::Limits::LIBRARY, &[]).unwrap();
        let id = |name: &str| {
            let c = script.controls().iter().find(|c| c.variable.ends_with(name)).unwrap();
            sampler_ui_ir::ControlId(c.definition.id.0)
        };
        let (k, echo) = (id("$k"), id("$echo"));
        let pcm = Pcm::new(48000, vec![[0.0; 2]; 512].into_boxed_slice()).unwrap();
        let region = Region {
            sample: 0, key_low: 60, key_high: 60, root_key: Some(60), velocity_low: 0.0, velocity_high: 1.0, gain: 1.0,
            envelope: Envelope::default(), playback: Playback::default(),
        };
        let plan = script.bind(Prepared::new(48000, vec![pcm], vec![region], 1).unwrap()).unwrap();
        let limits = limits(&plan).0;
        let part = Part::new(Runtime::new(plan, limits).unwrap(), MixTree::instrument("s")).unwrap();
        let mut core = V2Core::with_parts(1, 48000.0);
        core.install(0, Some(Box::new(part)));
        assert!(core.set_control(0, k, 41.6), "rounded into the knob's integer range");
        core.render(16);
        assert_eq!((core.control_value(0, k), core.control_value(0, echo)), (Some(42.0), Some(43.0)));
        assert!(core.set_control(0, k, 500.0));
        core.render(16);
        assert_eq!(core.control_value(0, k), Some(100.0), "clamped");
        assert!(!core.set_control(0, sampler_ui_ir::ControlId(7), 1.0), "no such control");
    }

    #[test]
    fn typed_capture_reads_callback_changes_and_rejected_edits_roll_back() {
        let source="on init declare ui_knob $k(0,100,1) declare ui_table %t[4](1,1,100) declare ui_xy ?xy[2] declare ui_text_edit @text end on on ui_control($k) %t[1] := $k @text := \"live callback\" ?xy[0] := 0.25 end on";
        let script=sampler_ksp::compile(source,48000,sampler_ksp::Limits::LIBRARY,&[]).unwrap();
        let face=script.ui(&|_|None).unwrap();
        let plan=script.bind(Prepared::new(48000,vec![],vec![],1).unwrap()).unwrap();
        let limits=limits(&plan).0;
        let mut part=Part::new(Runtime::new(plan,limits).unwrap(),MixTree::instrument("typed")).unwrap();
        let mut ingress=part.ui_controls.take().unwrap();
        let knob=sampler_ui_ir::ControlId(sampler_ksp::derived_control_id(0,"$k").0);
        assert!(ingress.submit(knob,25.));
        while part.runtime.poll_control_update().unwrap().is_some() {}
        part.runtime.render(&mut [[0.;2];16]).unwrap();
        ingress.settle();
        let revision=ingress.revision;
        assert!(!ingress.refresh(),"poll only queues capture");
        while part.runtime.poll_control_update().unwrap().is_some() {}
        assert!(ingress.settle(),"typed callback changes wake the view");
        assert!(ingress.revision>=revision);
        let values=ingress.values(&face);
        let find=|name:&str|sampler_ui_ir::WidgetRef(face.widgets.iter().position(|widget|widget.name==name).unwrap());
        let table=find("%t"); let text=find("@text"); let xy=find("?xy");
        assert_eq!(values[&table],sampler_ui_ir::Value::Integers(vec![0,25,0,0]));
        assert_eq!(values[&text],sampler_ui_ir::Value::Text("live callback".into()));
        assert!(matches!(&values[&xy],sampler_ui_ir::Value::Reals(values) if values[0]==0.25));
        assert!(ingress.submit_ui_widgets(0,&face.widgets[table.0],vec![(1,sampler_ui_ir::Value::Integer(999))],Default::default()));
        assert!(matches!(&ingress.values(&face)[&table],sampler_ui_ir::Value::Integers(values) if values[1]==999));
        while part.runtime.poll_control_update().unwrap().is_some() {}
        assert!(ingress.settle(),"rejected preview must wake rollback");
        assert_eq!(ingress.values(&face)[&table],values[&table]);
        assert!(!ingress.submit_ui_widgets(0,&face.widgets[text.0],vec![(0,sampler_ui_ir::Value::Text("x".repeat(8192)))],Default::default()));
        assert_eq!(ingress.values(&face)[&text],values[&text]);
    }

    #[test]
    fn script_ui_effects_reach_the_interface() {
        let source = "on init\n declare ui_knob $k(0, 100, 1)\n declare ui_label $l(1, 1)\n\
                      set_key_type(36, $NI_KEY_TYPE_CONTROL)\nend on\n\
                      on ui_control($k)\n set_control_par(get_ui_id($l), $CONTROL_PAR_HIDE, $HIDE_WHOLE_CONTROL)\n\
                      set_key_color(60, $KEY_COLOR_RED)\nend on\n";
        let script = sampler_ksp::compile(source, 48000, sampler_ksp::Limits::LIBRARY, &[]).unwrap();
        let k = script.controls().iter().find(|c| c.variable.ends_with("$k")).unwrap().definition.id.0;
        let mut ui = ScriptUi { views: vec![script.view()], resources: None, ..Default::default() };
        let before = ui.interfaces();
        assert!(ui.keys()[36].control && ui.keys()[60].color.is_none());
        let pcm = Pcm::new(48000, vec![[0.0; 2]; 512].into_boxed_slice()).unwrap();
        let region = Region {
            sample: 0, key_low: 60, key_high: 60, root_key: Some(60), velocity_low: 0.0, velocity_high: 1.0, gain: 1.0,
            envelope: Envelope::default(), playback: Playback::default(),
        };
        let plan = script.bind(Prepared::new(48000, vec![pcm], vec![region], 1).unwrap()).unwrap();
        let limits = limits(&plan).0;
        let part = Part::new(Runtime::new(plan, limits).unwrap(), MixTree::instrument("s")).unwrap();
        let mut core = V2Core::with_parts(1, 48000.0);
        core.install(0, Some(Box::new(part)));
        assert!(core.set_control(0, sampler_ui_ir::ControlId(k), 1.0));
        core.render(16);
        let mut applied = 0;
        core.take_effects(0, &mut |instance, effect| {
            applied += usize::from(ui.apply(instance, effect));
            true
        });
        assert_eq!(applied, 2);
        assert_ne!(ui.interfaces(), before, "the label is hidden");
        assert_eq!(ui.keys()[60].color, Some(0), "red");
    }

    #[test]
    fn w1_script_label_alias_reaches_the_interface() {
        let source = "on init\n declare ui_knob $k(0, 100, 1)\n declare ui_label $l(1, 1)\n\
                      set_key_type(36, $NI_KEY_TYPE_CONTROL)\nend on\n\
                      on ui_control($k)\n set_control_par(get_ui_id($l), $CONTROL_PAR_HIDE, $HIDE_WHOLE_CONTROL)\n\
                      set_knob_label($k, \"changed\")\nend on\n";
        let script = sampler_ksp::compile(source, 48000, sampler_ksp::Limits::LIBRARY, &[]).unwrap();
        let k = script.controls().iter().find(|c| c.variable.ends_with("$k")).unwrap().definition.id.0;
        let mut ui = ScriptUi { views: vec![script.view()], resources: None, ..Default::default() };
        let before = ui.interfaces();
        assert!(ui.keys()[36].control && ui.keys()[60].color.is_none());
        let pcm = Pcm::new(48000, vec![[0.0; 2]; 512].into_boxed_slice()).unwrap();
        let region = Region {
            sample: 0, key_low: 60, key_high: 60, root_key: Some(60), velocity_low: 0.0, velocity_high: 1.0, gain: 1.0,
            envelope: Envelope::default(), playback: Playback::default(),
        };
        let plan = script.bind(Prepared::new(48000, vec![pcm], vec![region], 1).unwrap()).unwrap();
        let limits = limits(&plan).0;
        let part = Part::new(Runtime::new(plan, limits).unwrap(), MixTree::instrument("s")).unwrap();
        let mut core = V2Core::with_parts(1, 48000.0);
        core.install(0, Some(Box::new(part)));
        assert!(core.set_control(0, sampler_ui_ir::ControlId(k), 1.0));
        core.render(16);
        let mut applied = 0;
        core.take_effects(0, &mut |instance, effect| {
            applied += usize::from(ui.apply(instance, effect));
            true
        });
        assert_eq!(applied, 2);
        assert_ne!(ui.interfaces(), before, "the label is hidden");
        assert_eq!(ui.interfaces()[0].widgets.iter().find(|w| w.name.ends_with("$k")).unwrap().value_text.as_deref(), Some("changed"));
    }

    #[test]
    fn script_capacity_grows_with_the_stages_a_key_passes() {
        let pcm = || Pcm::new(48000, vec![[0.0; 2]; 512].into_boxed_slice()).unwrap();
        let region = Region {
            sample: 0, key_low: 60, key_high: 60, root_key: Some(60), velocity_low: 0.0, velocity_high: 1.0, gain: 1.0,
            envelope: Envelope::default(), playback: Playback::default(),
        };
        let plain = Prepared::new(48000, vec![pcm()], vec![region.clone()], 1).unwrap();
        assert_eq!(Limits::script_capacity(&plain), 16);
        let script = |n: u8| {
            sampler_ksp::compile(&format!("on note\n play_note({}, 100, 0, -1)\nend on\n", 60 + n), 48000, sampler_ksp::Limits::LIBRARY, &[]).unwrap()
        };
        let one = sampler_ksp::bind_modules(vec![script(0)], Prepared::new(48000, vec![pcm()], vec![region.clone()], 1).unwrap()).unwrap();
        let four = sampler_ksp::bind_modules((0..4).map(script).collect(), Prepared::new(48000, vec![pcm()], vec![region], 1).unwrap()).unwrap();
        assert_eq!((Limits::script_capacity(&one), Limits::script_capacity(&four)), (4 * Limits::SCRIPT_KEYS + 1, 16 * Limits::SCRIPT_KEYS + 4));
        let limits = limits(&four).0;
        assert!(Runtime::new(four, limits).is_ok());
    }

    #[test]
    fn unknown_formats_are_explicitly_unsupported() {
        let request = LoadRequest { path: "x.exs".into(), sample_rate: 48000.0, ..Default::default() };
        let err = V2Loader.prepare(&request, &mut |_| {}, &|| false).err().unwrap();
        assert!(matches!(err, CoreError::Unsupported(_)), "{err}");
    }

    #[test]
    fn loader_starts_note_parameters_at_v1_event_capacity() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("note-capacity.wav");
        sine(&path);
        let part = load(&path).unwrap();
        assert_eq!(part.runtime.note_params_capacity(), 4096);
        assert_eq!(part.runtime.voice_capacity(), 1024);
    }

    #[test]
    fn production_ownership_limits_use_v1_event_capacity() {
        let plan = Prepared::new(48000, vec![], vec![], 1).unwrap();
        let (limits, ceiling) = limits(&plan);
        assert_eq!(limits.notes, 4096);
        assert_eq!(limits.expressions, 4096);
        assert_eq!(limits.families, 4096);
        assert_eq!(limits.decisions, 4096);
        assert_eq!(limits.voices, 1024);
        assert_eq!(ceiling, 8192);
    }

    /// Set `KONTRA_KONTAKT_LIBRARIES` to library roots to run; skips otherwise.
    /// Scripted instruments must sound: with their scripts on, a played note is
    /// audible within a few seconds. Set `KONTRA_KONTAKT_LIBRARIES` to run.
    #[test]
    fn real_scripted_instruments_are_not_silent() {
        let roots = std::env::var_os("KONTRA_KONTAKT_LIBRARIES").unwrap_or_default();
        let mut silent = Vec::new();
        for (relative, key) in [
            ("Performance Samples Vista/Instruments/Vista - 3 Cellos.nki", 48),
            ("Una Corda Library/Instruments/Una Corda Pure.nki", 60),
            ("Afflatus Chapter II Brass/Instruments/3. Curated Ensembles/Barbarian Brass.nki", 48),
            ("Pacific Ensemble Strings/Instruments/10 Cellos/Pacific - Ens Strings - 10 Cellos - Legato Sustains.nki", 48),
        ] {
            let Some(path) = std::env::split_paths(&roots).map(|r| r.join(relative)).find(|p| p.is_file()) else {
                eprintln!("skipped: {relative} is not installed");
                continue;
            };
            let request = LoadRequest { path, sample_rate: 48000.0, ..Default::default() };
            let loaded = V2Loader.prepare(&request, &mut |_| {}, &|| false).unwrap();
            let mut core = V2Core::with_parts(1, 48000.0);
            core.install(0, loaded.part);
            core.event(0, on(HostNote { port: 0, channel: 0, key, id: 1, clap: true }));
            // Streamed pages arrive from disk threads: give them real time.
            let heard = (0..300).any(|_| {
                std::thread::sleep(std::time::Duration::from_millis(5));
                // Quiet layers are still sound: dynamics start at the softest.
                let r = core.render(128);
                r.live[0] && r.buses[0][0][..128].iter().any(|x| x.abs() > 1e-5)
            });
            if !heard {
                silent.push(format!("{relative}: {} voices, {:?}, {} callbacks", core.voices().active, core.problems(0), loaded.report.decoded.script_callbacks));
            }
        }
        assert!(silent.is_empty(), "silent with scripts on: {silent:#?}");
    }

    /// The source's instrument and send buses are mixer nodes beside its groups.
    /// Set `KONTRA_KONTAKT_LIBRARIES` to library roots to run; skips otherwise.
    #[test]
    fn real_instrument_buses_become_mixer_nodes() {
        let relative = "ANALOG STRINGS/Instruments/ANALOG STRINGS.nki";
        let roots = std::env::var_os("KONTRA_KONTAKT_LIBRARIES").unwrap_or_default();
        let Some(path) = std::env::split_paths(&roots).map(|r| r.join(relative)).find(|p| p.is_file()) else {
            eprintln!("skipped: {relative} is not installed");
            return;
        };
        let loaded = V2Loader.prepare(&LoadRequest { path, sample_rate: 48000.0, ..Default::default() }, &mut |_| {}, &|| false).unwrap();
        let buses: Vec<_> = loaded.tree.nodes.iter().filter(|n| n.kind == NodeKind::Bus).map(|n| n.name.as_str()).collect();
        assert_eq!(buses, ["insert", "send 0", "send 1"]);
        let groups = loaded.tree.nodes.iter().filter(|n| n.kind == NodeKind::Group).count();
        assert!(groups > 100, "every group is a node too: {groups}");
        let mut core = V2Core::with_parts(1, 48000.0);
        core.install(0, loaded.part);
        // The part accepts a setting for every node.
        let mut mix = Mix::default();
        mix.nodes[0] = vec![NodeMix::default(); loaded.tree.nodes.len() - 1];
        core.set_mix(&mix);
    }

    /// Each program of a Kontakt multi loads as its own rack part.
    #[test]
    fn real_multi_programs_load_as_parts() {
        let relative = "Audio Imperia CHORUS/Multis/10 Chorus - Ensemble - Traditional Syllables.nkm";
        let roots = std::env::var_os("KONTRA_KONTAKT_LIBRARIES").unwrap_or_default();
        let Some(path) = std::env::split_paths(&roots).map(|r| r.join(relative)).find(|p| p.is_file()) else {
            eprintln!("skipped: {relative} is not installed");
            return;
        };
        let count = sampler_kontakt::read_multi(&path).unwrap().programs.len();
        assert!(count > 1, "a multi has several programs");
        let names: Vec<_> = (0..2)
            .map(|program| {
                let request = LoadRequest { path: path.clone(), program, sample_rate: 48000.0, ..Default::default() };
                V2Loader.prepare(&request, &mut |_| {}, &|| false).unwrap().instrument.unwrap().name.clone()
            })
            .collect();
        assert_ne!(names[0], names[1]);
    }

    /// A UVI bank program loads through the host and its layers are mixer nodes.
    #[test]
    fn real_uvi_layers_become_mixer_nodes() {
        let relative = "UVI/UVI - Augmented Orchestra v1.1.2-R2R/Augmented Orchestra.ufs";
        let roots = std::env::var_os("KONTRA_KONTAKT_LIBRARIES").unwrap_or_default();
        let roots = std::env::split_paths(&roots).map(|r| r.parent().unwrap_or(&r).to_path_buf());
        let Some(path) = roots.map(|r| r.join(relative)).find(|p| p.is_file()) else {
            eprintln!("skipped: {relative} is not installed");
            return;
        };
        let loaded = match V2Loader.prepare(&LoadRequest { path, sample_rate: 48000.0, ..Default::default() }, &mut |_| {}, &|| false) {
            Ok(loaded) => loaded,
            Err(e) => {
                eprintln!("skipped: {e:?}");
                return;
            }
        };
        let groups = loaded.tree.nodes.iter().filter(|n| n.kind == NodeKind::Group).count();
        assert!(groups > 0, "layers are nodes");
    }

    /// Peak of the last blocks after applying `mix` to a part with a held note.
    fn settled(core: &mut V2Core, mix: &Mix) -> f32 {
        core.set_mix(mix);
        let mut peak = 0.0f32;
        for block in 0..400 {
            std::thread::sleep(std::time::Duration::from_millis(2));
            let r = core.render(128);
            if block >= 390 {
                peak = r.buses.iter().flat_map(|b| b[0][..128].iter().chain(&b[1][..128])).fold(peak, |p, x| p.max(x.abs()));
            }
        }
        peak
    }

    /// Muting nodes silences the part and soloing one leaves only it. `kind`
    /// picks the nodes the test drives; the instrument must play at `keys`.
    fn mixer_nodes_pass_audio(path: std::path::PathBuf, kind: NodeKind) {
        let loaded = V2Loader.prepare(&LoadRequest { path, sample_rate: 48000.0, ..Default::default() }, &mut |_| {}, &|| false).unwrap();
        let nodes: Vec<usize> = (1..loaded.tree.nodes.len()).filter(|&n| loaded.tree.nodes[n].kind == kind).collect();
        let count = loaded.tree.nodes.len() - 1;
        // Try the middles of zones across the map until one key sounds.
        let zones = &loaded.instrument.as_ref().unwrap().zones;
        let mut keys: Vec<u8> = (0..8)
            .filter_map(|n| zones.get(n * zones.len() / 8))
            .map(|z| ((u16::from(z.keys.low) + u16::from(z.keys.high)) / 2) as u8)
            .collect();
        keys.dedup();
        let mut core = V2Core::with_parts(1, 48000.0);
        core.install(0, loaded.part);
        let mut heard = false;
        for (id, &key) in keys.iter().enumerate() {
            core.event(0, on(HostNote { port: 0, channel: 0, key, id: id as i32 + 1, clap: true }));
            heard = (0..150).any(|_| {
                std::thread::sleep(std::time::Duration::from_millis(5));
                let r = core.render(128);
                r.live[0] && r.buses[0][0][..128].iter().any(|x| x.abs() > 1e-5)
            });
            if heard {
                break;
            }
        }
        assert!(heard, "silent before any node is touched: keys {keys:?}, {:?}, samples {} zones {} missing {:?}", core.problems(0), loaded.report.decoded.samples, loaded.report.decoded.zones, loaded.report.missing.iter().take(6).collect::<Vec<_>>());
        assert!(nodes.len() >= 2, "{kind:?} nodes: {}", nodes.len());
        let mut mix = Mix::default();
        mix.nodes[0] = vec![NodeMix::default(); count];
        let open = settled(&mut core, &mix);
        assert!(open > 1e-6, "audible with every node open");
        for &n in &nodes {
            mix.nodes[0][n - 1].mute = true;
        }
        let muted = settled(&mut core, &mix);
        let loose: Vec<_> = (1..loaded.tree.nodes.len()).filter(|&n| loaded.tree.nodes[n].kind == NodeKind::Group && loaded.tree.nodes[n].parent.is_some_and(|p| !nodes.contains(&p) && kind == NodeKind::Mic)).map(|n| loaded.tree.nodes[n].name.clone()).collect();
        // An effect's tail may ring on after its input is muted.
        assert!(muted < open * 0.05 + 1e-7, "muting every {kind:?} node leaves {muted} of {open}; groups outside: {loose:?}");
        for &n in &nodes {
            mix.nodes[0][n - 1].mute = false;
        }
        // Soloing a node mutes the rest, so some solo is audible and the
        // soloed set is quieter than or equal to everything.
        let all = settled(&mut core, &mix);
        let mut heard_solo = false;
        for &n in &nodes {
            mix.nodes[0][n - 1].solo = true;
            let solo = settled(&mut core, &mix);
            mix.nodes[0][n - 1].solo = false;
            assert!(solo <= all * 1.01 + 1e-7, "solo {} louder than all: {solo} > {all}", loaded.tree.nodes[n].name);
            heard_solo |= solo > 1e-6;
        }
        assert!(heard_solo, "no {kind:?} node is audible alone");
        // A node with nothing soloed elsewhere: soloing one while every other
        // is muted keeps exactly that one.
        for &n in &nodes {
            mix.nodes[0][n - 1].mute = true;
        }
        mix.nodes[0][nodes[0] - 1].mute = false;
        mix.nodes[0][nodes[0] - 1].solo = true;
        let one = settled(&mut core, &mix);
        mix.nodes[0][nodes[0] - 1].solo = false;
        mix.nodes[0][nodes[0] - 1].mute = true;
        let none = settled(&mut core, &mix);
        assert!(none < open * 0.05 + 1e-7 && one >= none, "one {one}, none {none}");
    }

    #[test]
    fn real_snapshots_read_find_their_instrument_and_load() {
        let roots = std::env::var_os("KONTRA_KONTAKT_LIBRARIES").unwrap_or_default();
        let Some(library) = std::env::split_paths(&roots).map(|r| r.join("Una Corda Library")).find(|p| p.is_dir()) else {
            eprintln!("skipped: Una Corda Library is not installed");
            return;
        };
        let files: Vec<_> = walkdir::WalkDir::new(library.join("Snapshots"))
            .into_iter()
            .flatten()
            .filter(|e| e.path().extension().is_some_and(|x| x == "nksn"))
            .map(|e| e.into_path())
            .collect();
        assert!(!files.is_empty());
        let (mut script, mut groups, mut effects) = (0, 0, 0);
        for file in &files {
            let state = sampler_kontakt::read_snapshot(file).unwrap_or_else(|e| panic!("{}: {e}", file.display()));
            let parent = snapshot_parent(file, &state.instrument).unwrap_or_else(|| panic!("no instrument for {}", file.display()));
            let plain = sampler_kontakt::read(&parent).unwrap();
            let applied = sampler_kontakt::read_with_snapshot(&parent, &state).unwrap();
            let states = |k: &sampler_kontakt::Kontakt| k.instrument.behaviors.iter().map(|b| b.state.clone()).collect::<Vec<_>>();
            let mix = |k: &sampler_kontakt::Kontakt| format!("{:?}{:?}", k.instrument.buses, k.instrument.chains);
            let levels = |k: &sampler_kontakt::Kontakt| k.instrument.groups.iter().map(|g| (g.gain, g.pan, g.tune)).collect::<Vec<_>>();
            script += usize::from(states(&plain) != states(&applied));
            groups += usize::from(levels(&plain) != levels(&applied));
            effects += usize::from(mix(&plain) != mix(&applied));
        }
        eprintln!("{} snapshots: {script} change script state, {groups} group levels, {effects} effects", files.len());
        assert!(script > 0 && groups > 0 && effects > 0, "script {script} groups {groups} effects {effects} of {}", files.len());
        let request = LoadRequest { path: files[0].clone(), sample_rate: 48000.0, ..Default::default() };
        let loaded = V2Loader.prepare(&request, &mut |_| {}, &|| false).unwrap();
        assert!(loaded.instrument.is_some_and(|i| !i.zones.is_empty()));
    }

    #[test]
    fn real_kontakt_mic_nodes_pass_audio() {
        let relative = "Afflatus Chapter II Brass/Instruments/1. Ensembles/Single Instruments/2 Horns/2 Horns Staccato.nki";
        let roots = std::env::var_os("KONTRA_KONTAKT_LIBRARIES").unwrap_or_default();
        let Some(path) = std::env::split_paths(&roots).map(|r| r.join(relative)).find(|p| p.is_file()) else {
            eprintln!("skipped: {relative} is not installed");
            return;
        };
        mixer_nodes_pass_audio(path, NodeKind::Mic);
    }

    #[test]
    fn real_uvi_layer_nodes_pass_audio() {
        let relative = "UVI/UVI - Augmented Orchestra v1.1.2-R2R/Augmented Orchestra.ufs";
        let roots = std::env::var_os("KONTRA_KONTAKT_LIBRARIES").unwrap_or_default();
        let roots = std::env::split_paths(&roots).map(|r| r.parent().unwrap_or(&r).to_path_buf());
        let Some(path) = roots.map(|r| r.join(relative)).find(|p| p.is_file()) else {
            eprintln!("skipped: {relative} is not installed");
            return;
        };
        mixer_nodes_pass_audio(path, NodeKind::Group);
    }

    #[test]
    fn real_uvi_lua_program_sounds_through_the_trait() {
        let relative = "VWinds - Clarinets/VWinds-ContrabassClarinet_V2.ufs/Presets/Contrabass Clarinet.uvip";
        let roots = std::env::var_os("KONTRA_UVI_LIBRARIES").unwrap_or_default();
        let Some(path) = std::env::split_paths(&roots).map(|r| r.join(relative)).find(|p| p.to_string_lossy().contains(".ufs") && p.ancestors().any(|a| a.is_file())) else {
            eprintln!("skipped: {relative} is not installed");
            return;
        };
        let request = LoadRequest { path, sample_rate: 48000.0, ..Default::default() };
        let loaded = V2Loader.prepare(&request, &mut |_| (), &|| false).unwrap();
        assert!(
            !loaded.report.missing.iter().any(|m| m.value.contains("no frontend")),
            "the Lua script runs: {:?}",
            loaded.report.missing
        );
        let stream = loaded.stream.clone().expect("UVI samples stream");
        let (held, full) = (stream.resident_bytes(), loaded.report.decoded.full_bytes);
        assert!(held > 0 && held < full, "{held} of {full} bytes resident");
        let mut core = V2Core::with_parts(1, 48000.0);
        core.install(0, loaded.part);
        core.event(0, on(HostNote { port: 0, channel: 0, key: 36, id: 1, clap: true }));
        // The script runs on its own thread: its note arrives within a few blocks.
        let heard = (0..400).any(|_| {
            std::thread::sleep(std::time::Duration::from_millis(2));
            loud(&core.render(128), 0, 128)
        });
        assert!(heard, "the scripted program is silent: {:?} / {:?}", core.problems(0), core.voices());
    }

    /// Idle cost of a loaded part: blocks with no note playing. Prints the
    /// share of one core; `KONTRA_KONTAKT_LIBRARIES=… cargo test idle_cost -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn idle_cost_of_a_loaded_instrument() {
        let roots = std::env::var_os("KONTRA_KONTAKT_LIBRARIES").unwrap_or_default();
        for relative in [
            "Una Corda Library/Instruments/Una Corda Pure.nki",
            "Performance Samples Vista/Instruments/Vista - 3 Cellos.nki",
            "Afflatus Chapter II Brass/Instruments/3. Curated Ensembles/Barbarian Brass.nki",
        ] {
            let Some(path) = std::env::split_paths(&roots).map(|r| r.join(relative)).find(|p| p.is_file()) else { continue };
            let loaded = V2Loader.prepare(&LoadRequest { path, sample_rate: 48000.0, ..Default::default() }, &mut |_| {}, &|| false).unwrap();
            let mut core = V2Core::with_parts(1, 48000.0);
            core.install(0, loaded.part);
            std::thread::sleep(std::time::Duration::from_secs(2));
            let blocks = 48000 / 128 * 20;
            let start = std::time::Instant::now();
            for _ in 0..blocks {
                core.render(128);
            }
            let spent = start.elapsed().as_secs_f64();
            println!("IDLE {relative}: {:.4}% of a core ({:.1} us per 128-frame block)", spent / 20.0 * 100.0, spent / blocks as f64 * 1e6);
        }
    }

    #[test]
    fn real_uvi_lua_program_plays_without_audio_thread_allocation() {
        let relative = "VWinds - Clarinets/VWinds-ContrabassClarinet_V2.ufs/Presets/Contrabass Clarinet.uvip";
        let roots = std::env::var_os("KONTRA_UVI_LIBRARIES").unwrap_or_default();
        let Some(path) = std::env::split_paths(&roots).map(|r| r.join(relative)).find(|p| p.to_string_lossy().contains(".ufs") && p.ancestors().any(|a| a.is_file())) else {
            eprintln!("skipped: {relative} is not installed");
            return;
        };
        let request = LoadRequest { path, sample_rate: 48000.0, ..Default::default() };
        let loaded = V2Loader.prepare(&request, &mut |_| (), &|| false).unwrap();
        assert!(
            !loaded.report.missing.iter().any(|m| m.value.contains("no frontend")),
            "the Lua script runs: {:?}",
            loaded.report.missing
        );
        let mut core = V2Core::with_parts(1, 48000.0);
        core.install(0, loaded.part);
        core.event(0, on(HostNote { port: 0, channel: 0, key: 36, id: 1, clap: true }));
        // The script runs on its own thread: its note arrives within a few blocks.
        let heard = (0..400).any(|_| {
            std::thread::sleep(std::time::Duration::from_millis(2));
            loud(&core.render(128), 0, 128)
        });
        assert!(heard, "the scripted program is silent: {:?} / {:?}", core.problems(0), core.voices());
        // Warm: the first notes sized the driver's tables. Now play more
        // scripted notes and release them; the audio thread allocates nothing.
        core.event(0, Event::NoteOff(HostPattern { port: -1, channel: -1, key: 36, id: -1, clap: true }));
        (0..50).for_each(|_| _ = core.render(128));
        let allocations = crate::plugin::tests::allocations(|| {
            for (id, key) in [(2, 40), (3, 43), (4, 36)] {
                core.event(0, on(HostNote { port: 0, channel: 0, key, id, clap: true }));
                for _ in 0..60 {
                    std::thread::sleep(std::time::Duration::from_millis(1));
                    _ = core.render(128);
                }
                core.event(0, Event::NoteOff(HostPattern { port: -1, channel: -1, key: i32::from(key), id: -1, clap: true }));
            }
        });
        assert_eq!(allocations, 0, "the audio thread allocated or freed memory");
    }

    #[test]
    fn pitch_bend_reaches_notes_a_lua_script_played() {
        let relative = "VWinds - Clarinets/VWinds-ContrabassClarinet_V2.ufs/Presets/Contrabass Clarinet.uvip";
        let roots = std::env::var_os("KONTRA_UVI_LIBRARIES").unwrap_or_default();
        let Some(path) = std::env::split_paths(&roots).map(|r| r.join(relative)).find(|p| p.ancestors().any(|a| a.is_file())) else {
            eprintln!("skipped: {relative} is not installed");
            return;
        };
        let loaded = V2Loader.prepare(&LoadRequest { path, sample_rate: 48000.0, ..Default::default() }, &mut |_| (), &|| false).unwrap();
        let mut core = V2Core::with_parts(1, 48000.0);
        core.install(0, loaded.part);
        core.event(0, on(HostNote { port: 0, channel: 0, key: 40, id: 1, clap: true }));
        // Zero crossings of the left channel over `blocks` blocks, scripts given a moment each.
        let crossings = |core: &mut V2Core, blocks: usize| {
            let (mut n, mut last) = (0usize, 0.0f32);
            for _ in 0..blocks {
                std::thread::sleep(std::time::Duration::from_millis(2));
                let r = core.render(128);
                for x in &r.buses[0][0][..128] {
                    n += usize::from(last <= 0.0 && *x > 0.0);
                    last = *x;
                }
            }
            n
        };
        let heard = (0..400).any(|_| {
            std::thread::sleep(std::time::Duration::from_millis(2));
            loud(&core.render(128), 0, 128)
        });
        assert!(heard);
        let before = crossings(&mut core, 150);
        core.event(0, Event::Ump([0x40e0_0000, 0xffff_ffff]));
        let after = crossings(&mut core, 150);
        assert!(before > 20, "{before} crossings");
        // The default bend range is two semitones: about 12% higher.
        assert!(after as f64 > before as f64 * 1.05, "{before} crossings before the bend, {after} after");
    }

    #[test]
    fn real_kontakt_instrument_plays_through_the_trait() {
        let relative = "Una Corda Library/Instruments/Una Corda Pure.nki";
        let roots = std::env::var_os("KONTRA_KONTAKT_LIBRARIES").unwrap_or_default();
        let Some(path) = std::env::split_paths(&roots).map(|r| r.join(relative)).find(|p| p.is_file()) else {
            eprintln!("skipped: {relative} is not installed");
            return;
        };
        let description = V2Loader.describe(&path, 0).unwrap();
        assert!(description.zones > 0);
        let request = LoadRequest { path, sample_rate: 48000.0, ..Default::default() };
        let mut last = Progress(0);
        let loaded = V2Loader.prepare(&request, &mut |p| last = p, &|| false).unwrap();
        assert_eq!(last, Progress::DONE);
        assert!(loaded.tree.nodes.len() > 1, "groups are mixer nodes");
        assert!(loaded.report.decoded.zones > 0);
        let mut core = V2Core::with_parts(1, 48000.0);
        core.install(0, loaded.part);
        core.event(0, on(HostNote { port: 0, channel: 0, key: 60, id: 1, clap: true }));
        let heard = (0..40).any(|_| loud(&core.render(128), 0, 128));
        assert!(heard, "no output; missing: {:?}", loaded.report.missing);

        // Samples stream: only start data is resident, and an idle budget drops it.
        let stream = loaded.stream.expect("Kontakt samples stream");
        let (held, full) = (stream.resident_bytes(), loaded.report.decoded.full_bytes);
        assert!(held > 0 && held < full, "{held} of {full} bytes resident");
        core.event(0, Event::NoteOff(HostPattern { port: -1, channel: -1, key: 60, id: -1, clap: true }));
        (0..400).for_each(|_| _ = core.render(128));
        assert!(stream.trim(0, core.clock(0)) > 0, "idle start data is dropped");
        assert!(stream.resident_bytes() < held);
        // A purged sample's first start is refused and reloads it; later starts play.
        let again = (2..200).any(|id| {
            core.event(0, on(HostNote { port: 0, channel: 0, key: 60, id, clap: true }));
            std::thread::sleep(std::time::Duration::from_millis(5));
            (0..4).any(|_| loud(&core.render(128), 0, 128))
        });
        assert!(again, "the purged sample reloads");
    }
}

#[cfg(test)]
mod send_tests {
    use super::*;

    fn instrument(fader: f64) -> ir::Instrument {
        let mut i = ir::Instrument::default();
        i.buses.push(ir::Bus { name: "aux".into(), chain: None, sends: Vec::new(), output: ir::Output::Master, gain: ir::Gain::UNITY });
        let send = |gain, pre_fader| ir::GroupSend { to: ir::BusRef(0), gain: ir::Gain::Linear(gain), pre_fader };
        i.groups.push(ir::Group { gain: ir::Gain::Linear(fader), sends: vec![send(0.5, true), send(0.5, false)], ..Default::default() });
        i
    }

    fn tapped(fader: f64) -> (ir::Gain, Vec<f64>) {
        let mut i = instrument(fader);
        let tree = nest(&mut i);
        let bus = &i.buses[1];
        assert_eq!(i.groups[0].gain, ir::Gain::UNITY, "voices feed the tap unscaled");
        assert_eq!(tree.nodes[2].sends.len(), 2);
        (bus.gain, tree.nodes[2].sends.iter().map(|s| f64::from(s.1)).collect())
    }

    #[test]
    fn a_closed_fader_silences_the_output_but_not_the_pre_fader_send() {
        let (out, sends) = tapped(0.0);
        assert_eq!((out.linear(), sends), (0.0, vec![0.5, 0.0]));
        assert_eq!(instrument(0.0).groups[0].sends.len(), 2);
    }

    #[test]
    fn pre_and_post_sends_tap_either_side_of_a_minus_12_db_fader() {
        let fader = 10f64.powf(-12.0 / 20.0);
        let (out, sends) = tapped(fader);
        assert_eq!(out.linear(), fader);
        assert!((sends[0] - 0.5).abs() < 1e-6 && (sends[1] - 0.5 * fader).abs() < 1e-6, "{sends:?}");
    }

    #[test]
    fn lowering_hears_the_same_taps_without_a_host_mixer() {
        let routed = instrument(0.25).with_group_taps();
        assert_eq!(routed.buses[1].gain.linear(), 0.25);
        assert_eq!(routed.groups[0].output, ir::Output::Bus(ir::BusRef(1)));
    }
}

#[cfg(test)]
#[path = "keyswitch_tests.rs"]
mod keyswitch_tests;

#[cfg(test)]
#[path = "envelope_init_tests.rs"]
mod envelope_init_tests;

// Port from v1 0cb7a8a0:src/import.rs; v2 containers retain native metadata.
/// Container identity only, for the off-thread snapshot catalog. Snapshot
/// metadata names its base instrument, not the snapshot itself; the latter's
/// authored name is its file stem (as in `read_snapshot`).
pub(crate) fn snapshot_instrument(path: &Path) -> anyhow::Result<String> {
    std::panic::catch_unwind(|| {
        let c = sampler_kontakt::read_chunks(path)?;
        c.find_first(0x4f).context("Snapshot state missing")?;
        let names = ni_file::kontakt::objects::snapshot_metadata_names(
            c.find_first(0x51).context("Snapshot metadata missing")?,
        )?;
        Ok(snapshot_binding_name(&names, None)?.to_owned())
    }).map_err(|_| anyhow::anyhow!("Malformed snapshot metadata"))?
}

/// The same exact base identity used by snapshot validation, without loading
/// zones, samples, scripts or artwork.
pub(crate) fn snapshot_base_name(path: &Path) -> anyhow::Result<String> {
    std::panic::catch_unwind(|| {
        let c = sampler_kontakt::read_chunks(path)?;
        let program = ni_file::kontakt::objects::Program::try_from(c.find_first(0x28).context("Base program missing")?)?;
        Ok(program.params()?.name)
    }).map_err(|_| anyhow::anyhow!("Malformed base instrument metadata"))?
}

// A factory snapshot may retain Kontakt's generic template name. Only use
// its second embedded name when it also exactly names the supplied NKI file;
// the program name, group/source/slot identities are still validated below.
fn snapshot_binding_name<'a>(names: &'a (String, String), base: Option<&Path>) -> anyhow::Result<&'a str> {
    if names.0 != "Kontakt" || names.1.is_empty() { return Ok(&names.0); }
    if let Some(base) = base {
        ensure!(base.file_stem().and_then(|s| s.to_str()) == Some(names.1.as_str()),
            "Generic snapshot requires base filename {:?}", names.1);
    }
    Ok(&names.1)
}

#[cfg(test)]
mod editor_reload_tests {
    use super::*;
    use sampler_core::{EngineParameterAddress,EngineParameterLaw,EngineParameterBinding,EngineParameterOffset,ControlId};
    #[test]
    fn v1_editor_mix_before_load_and_reload_keeps_the_saved_offset() {
        let address=EngineParameterAddress{parameter:sampler_core::engine_parameter_id("ENGINE_PAR_CUTOFF").unwrap(),group:0,slot:3,generic:-1};
        let id=ControlId(10);let law=EngineParameterLaw::Exponential{low:10.,high:10000.};
        let make=|| {
            let plan=Prepared::new(48000,vec![],vec![],0).unwrap().with_controls(vec![ControlDefinition{id,domain:ControlDomain::Real{min:10.,max:10000.},default:ControlValue::Real(100.)}]).unwrap().with_engine_parameters(vec![EngineParameterBinding{address,control:id,law}],vec![]).unwrap();
            let limits=Limits::for_plan(&plan,128,16);Part::new(Runtime::new(plan,limits).unwrap(),MixTree::instrument("Reload")).unwrap()
        };
        let mut c=V2Core::with_parts(1,48000.);let mut mix=Mix::default();
        mix.editor_offsets=vec![Arc::from([EngineParameterOffset{address,offset:0.1}])];
        c.set_mix(&mix);
        for _ in 0..2 {
            let _old=c.install(0,Some(Box::new(make())));
            let rt=&c.parts[0].as_ref().unwrap().runtime;
            assert_eq!(rt.control_base_value(rt.active_plan(),id).unwrap(),ControlValue::Real(100.));
            assert_eq!(rt.control_value(rt.active_plan(),id).unwrap(),ControlValue::Real(law.decode(law.encode(100.)+100000)),"loading applies the already saved editor layer");
        }
    }
    #[test]
    fn equal_offset_arcs_follow_mix_ownership_and_failed_updates_drop_the_cache() {
        let address = EngineParameterAddress { parameter: sampler_core::engine_parameter_id("ENGINE_PAR_CUTOFF").unwrap(), group:0, slot:3, generic:-1 };
        let id = ControlId(10);
        let law = EngineParameterLaw::Exponential { low:10., high:10000. };
        let plan = Prepared::new(48000, vec![], vec![], 0).unwrap()
            .with_controls(vec![ControlDefinition { id, domain:ControlDomain::Real { min:10., max:10000. }, default:ControlValue::Real(100.) }]).unwrap()
            .with_engine_parameters(vec![EngineParameterBinding { address, control:id, law }], vec![]).unwrap();
        let limits = Limits::for_plan(&plan,128,16);
        let mut part = Part::new(Runtime::new(plan,limits).unwrap(),MixTree::instrument("Ownership")).unwrap();
        let offsets = |offset| Arc::<[EngineParameterOffset]>::from([EngineParameterOffset { address, offset }]);
        let a = offsets(0.1); let b = offsets(0.1); let c = offsets(0.2);
        part.apply_editor_offsets(&a);
        let plan = part.runtime.active_plan();
        let revision = part.runtime.control_revision(plan).unwrap();
        assert!(!Arc::ptr_eq(&a,&b));
        part.apply_editor_offsets(&b);
        assert!(Arc::ptr_eq(part.editor_offsets.as_ref().unwrap(),&b), "equal contents must adopt the latest worker-owned Mix Arc");
        assert_eq!(part.runtime.control_revision(plan).unwrap(),revision, "equal contents must not call the engine setter");
        part.apply_editor_offsets(&c);
        assert!(Arc::ptr_eq(part.editor_offsets.as_ref().unwrap(),&c));
        assert_eq!(part.runtime.control_value(plan,id).unwrap(),ControlValue::Real(law.decode(law.encode(100.)+200000)));
        let revision = part.runtime.control_revision(plan).unwrap();
        let invalid = offsets(f32::NAN);
        part.apply_editor_offsets(&invalid);
        assert!(part.editor_offsets.is_none(), "a failed update must release the older cache while the old Mix still retains it");
        assert_eq!(part.runtime.control_revision(plan).unwrap(),revision);
        part.apply_editor_offsets(&c);
        assert!(Arc::ptr_eq(part.editor_offsets.as_ref().unwrap(),&c), "valid retry must restore cache ownership");
        let d = offsets(0.3); part.apply_editor_offsets(&d);
        assert!(Arc::ptr_eq(part.editor_offsets.as_ref().unwrap(),&d));
        assert_eq!(part.runtime.control_value(plan,id).unwrap(),ControlValue::Real(law.decode(law.encode(100.)+300000)));
    }

}

#[cfg(test)]
mod timing_parity_tests {
    use super::*;
    fn aligned_core() -> V2Core {
        let pcm=Pcm::new(48000,vec![[0.25;2];48000].into_boxed_slice()).unwrap();
        let plan=Prepared::new(48000,vec![pcm],vec![Region{sample:0,key_low:0,key_high:127,root_key:None,velocity_low:0.,velocity_high:1.,gain:1.,envelope:Envelope::default(),playback:Playback::default()}],128).unwrap();
        let limits=Limits::for_plan(&plan,128,8);let rt=Runtime::new(plan,limits).unwrap();
        let mut c=V2Core::with_parts(1,48000.);c.install(0,Some(Box::new(Part::new(rt,MixTree::instrument("Timing")).unwrap())));
        let mut mix=Mix::default();mix.timing=Arc::new(crate::timing::Plan{on:true,latency_ms:10.,parts:vec![Arc::new(crate::timing::Holds::of(&crate::timing::Timing{override_ms:Some(0.),..Default::default()},&[],10.))],..Default::default()});
        c.set_mix(&mix);
        c
    }
    #[test]
    fn v1_auto_align_transport_flush_and_replacement_keep_exact_note_end() {
        let mut core = aligned_core();
        let note=HostNote{port:0,channel:0,key:60,id:23,clap:true};
        core.begin_block(&BlockInfo{frames:128,offline:true,..Default::default()});
        core.event(0,Event::NoteOn{note,velocity:0.8,tune:0.});
        let mut changed=core.mix.clone();
        changed.timing=Arc::new(crate::timing::Plan{on:false,..Default::default()});
        core.set_mix(&changed);
        core.begin_block(&BlockInfo{frames:128,offline:true,..Default::default()});
        assert!(core.render(1).buses[0][0][0]>0.1,"disabling alignment flushes an already accepted queued note");
        let _retired=core.install(0,None);
        let mut attempts=Vec::new();
        assert_eq!(core.end_block(1,&mut |n|{attempts.push(n);false}),1);
        assert_eq!(attempts,vec![note]);assert!(core.owns(note),"refused NOTE_END retains its exact owner");
        assert_eq!(core.end_block(1,&mut |n|{assert_eq!(n,note);true}),0);
        assert!(!core.owns(note));
        assert_eq!(core.end_block(1,&mut |_|panic!("duplicate NOTE_END")),0);
        let mut core=aligned_core();
        core.begin_block(&BlockInfo{frames:128,offline:true,..Default::default()});
        core.event(0,Event::NoteOn{note,velocity:0.8,tune:0.});
        let _retired=core.install(0,None);
        assert!(core.render(128).buses[0][0][..128].iter().all(|v|*v==0.),"replacement cancels queued audio");
        assert_eq!(core.end_block(128,&mut |_|false),1);
        assert!(core.owns(note));
        assert_eq!(core.end_block(128,&mut |n|{assert_eq!(n,note);true}),0);
        assert!(!core.owns(note));
    }
    #[test]
    fn v1_auto_align_master_trace_follows_the_assembled_buffer_and_gain_ramp() {
        let pcm=Pcm::new(48000,vec![[0.25;2];512].into_boxed_slice()).unwrap();
        let plan=Prepared::new(48000,vec![pcm],vec![Region{sample:0,key_low:60,key_high:60,root_key:Some(60),velocity_low:0.,velocity_high:1.,gain:1.,envelope:Envelope::default(),playback:Playback::default()}],128).unwrap().with_signal_trace(4096).unwrap();
        let limits=Limits::for_plan(&plan,128,8);let rt=Runtime::new(plan,limits).unwrap();
        let reader=rt.signal_trace_reader().unwrap();let mut core=aligned_core();
        let _old=core.install(0,Some(Box::new(Part::new(rt,MixTree::instrument("trace")).unwrap())));
        core.begin_block(&BlockInfo{frames:128,offline:true,..Default::default()});
        core.event(0,Event::NoteOn{note:HostNote{port:0,channel:0,key:60,id:31,clap:true},velocity:1.,tune:0.});
        for _ in 0..3 {let _=core.render(128);}
        let mut gains=[0.;128];gains[96..].fill(1.);
        let expected={let output=core.render(128);(output.buses[0][0].iter().zip(gains).map(|(x,g)|f64::from(*x*g).powi(2)).sum::<f64>()/128.).sqrt()};
        assert!(expected>0.1);assert!(core.trace_master(&gains));
        let rows=reader.drain();let masters:Vec<_>=rows.iter().filter(|r|reader.graph.nodes[r.node].kind=="host_master").collect();
        assert_eq!(masters.iter().map(|r|r.frames as usize).sum::<usize>(),128);
        let traced=(masters.iter().map(|r|r.output.rms[0].powi(2)*r.frames as f64).sum::<f64>()/128.).sqrt();
        assert!((traced-expected).abs()<1e-6,"trace must use the assembled aligned frame positions: {traced} vs {expected}");
    }
    #[test]
    fn v1_auto_align_first_plan_swap_never_frees_on_audio() {
        let mut core = V2Core::with_parts(1, 48000.);
        let mix = Mix::default();
        let calls = crate::plugin::tests::allocations(|| core.set_mix(&mix));
        assert_eq!(calls, 0, "initial timing plan must remain owned off audio");
    }
    #[test]
    fn v1_auto_align_reports_real_hold_and_preserves_host_ownership() {
        let mut c=aligned_core();c.begin_block(&BlockInfo{frames:128,offline:true,..Default::default()});
        assert_eq!(c.latency(),480,"the actual hold is reported to the host");
        let note=HostNote{port:0,channel:0,key:60,id:17,clap:true};c.event(0,Event::NoteOn{note,velocity:0.7654321,tune:0.123456789});
        assert!(c.owns(note),"queued host notes already own their exact tuple");assert_eq!(c.voices().active,0);
        for _ in 0..3{let r=c.render(128);assert!(r.buses[0][0][..128].iter().all(|v|*v==0.));}
        let r=c.render(96);assert!(r.buses[0][0][..96].iter().all(|v|*v==0.));
        let r=c.render(1);assert!(r.buses[0][0][0]>0.1,"audio starts exactly after 480 held frames");
    }
}

