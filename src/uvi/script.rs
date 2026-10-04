//! Lua 5.1 musical callbacks compiled to an engine-command stream.
//! This VM allocates and must never run in a plugin audio callback.
//! Callback ranges and cooperative scheduling follow UVI's public API:
//! https://lua.uvi.net/group___event_callbacks.html
//! https://lua.uvi.net/_time_intro.html

use super::{
    host,
    program::{NodeId, Program},
};
use anyhow::{Context, Result, ensure};
use mlua::thread::ThreadStatus;
use mlua::{
    Function, HookTriggers, Lua, LuaOptions, MultiValue, StdLib, Table, Thread, UserData, Value,
    VmState,
};
use serde::{Deserialize, Serialize};
use std::{
    cell::{Cell, RefCell},
    collections::{BTreeMap, HashMap, HashSet, VecDeque},
    rc::Rc,
};

// Explicit offline context: 48 kHz, initially stopped at beat zero / 120 BPM.
const RATE: f64 = 48000.;
const TEMPO: f64 = 120.;
const LIMIT: usize = 65_536;
// Bound retirement latency even when allocation-driven GC completes between drains.
const GC_MAX_DRAINS: u8 = 64;
pub const HOST_ROOT_CAPACITY: usize = 4096;
/// UVI backend ancestry within one activation. The core adapter owns any
/// corresponding DAW note identity; this token does not replace that registry.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct HostRoot {
    pub epoch: u64,
    pub generation: u64,
    pub token: u64,
}
/// A root has no held input, pending rooted work or descendant DSP voice at
/// this exclusive rendered boundary. Shared effect tails may still continue.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HostCompletion {
    pub root: HostRoot,
    pub frame: u64,
}
#[derive(Clone, Copy)]
struct RootInput {
    id: u32,
    channel: u8,
    note: u8,
    held: bool,
    choked: bool,
    ended: Option<u64>,
}

// A measured sequencer callback uses 2.011M instructions for finite UI updates.
// Leave headroom while bounding every outer resume, including nested run/pcall.
const FUEL: u32 = 4_000_000;

#[derive(Debug, Clone, Serialize)]
pub struct Note {
    pub id: u32,
    pub note: u8,
    pub velocity: u8,
    /// Engine channels are zero-based; Lua channels are one-based.
    pub channel: u8,
    pub dim1: usize,
    pub dim2: Option<u32>,
    /// Native Program layer node IDs; None selects all, an empty list selects none.
    pub layers: Option<Vec<NodeId>>,
    /// One-based oscillator index within each matching keygroup.
    pub oscillator: Option<usize>,
    pub volume: f32,
    pub pan: f32,
    pub tune: f64,
    pub offset_us: u64,
}

#[derive(Debug, Clone, Serialize)]
pub enum Action {
    /// Backend-only immediate stop; activation-local ancestry is in command_roots.
    ChokeRoot,
    Start(Note),
    Release(u32),
    /// Key release forwarded through the issuing processor's downstream chain.
    ReleaseNote {
        id: u32,
        note: u8,
        channel: u8,
        layer: Option<NodeId>,
    },
    Controller {
        channel: u8,
        controller: u8,
        value: u8,
    },
    /// Omni controller change within the current Program, on every MIDI channel.
    ControllerAll {
        controller: u8,
        value: u8,
    },
    PitchBend {
        channel: u8,
        bend: f64,
    },
    AfterTouch {
        channel: u8,
        value: u8,
    },
    PolyAfterTouch {
        channel: u8,
        note: u8,
        value: u8,
    },
    PolyAfterTouchAll {
        note: u8,
        value: u8,
    },
    Transport {
        playing: bool,
        beat: f64,
        tempo: f64,
    },
    /// Independent linear fade gain, multiplied with the ordinary voice gain.
    Fade {
        id: u32,
        start: Option<f32>,
        target: f32,
        duration_frames: u64,
        kill: bool,
        layer: Option<NodeId>,
    },
    Change {
        id: u32,
        gain: Option<f32>,
        tune: Option<f64>,
        pan: Option<f32>,
        /// Apply raw values independently to each live voice (gain multiplies).
        relative: bool,
        layer: Option<NodeId>,
    },
}

#[derive(Debug, Clone, Serialize)]
pub struct Command {
    pub frame: u64,
    pub action: Action,
}

#[derive(Debug, Clone, Copy, Deserialize)]
pub enum InputKind {
    NoteOn {
        channel: u8,
        note: u8,
        velocity: u8,
    },
    NoteOff {
        channel: u8,
        note: u8,
    },
    PitchBend {
        channel: u8,
        bend: f64,
    },
    AfterTouch {
        channel: u8,
        value: u8,
    },
    PolyAfterTouch {
        channel: u8,
        note: u8,
        value: u8,
    },
    Transport {
        playing: bool,
        beat: f64,
        tempo: f64,
    },
    Controller {
        channel: u8,
        controller: u8,
        value: u8,
    },
}

#[derive(Debug, Clone, Copy, Deserialize)]
pub struct Input {
    pub frame: u64,
    pub kind: InputKind,
}

#[derive(Debug, Clone, Copy)]
pub enum HostedInput {
    On {
        root: HostRoot,
        input: Input,
    },
    Off {
        root: HostRoot,
        frame: u64,
    },
    /// Stop this backend root without synthesizing a Lua release callback.
    Choke {
        root: HostRoot,
        frame: u64,
    },
    /// Non-note input in the same wire order as owned On/Off messages.
    /// Notes must use On/Off so they cannot bypass the activation ledger.
    Event(Input),
}
impl HostedInput {
    pub fn frame(&self) -> u64 {
        match self {
            Self::On { input, .. } | Self::Event(input) => input.frame,
            Self::Off { frame, .. } | Self::Choke { frame, .. } => *frame,
        }
    }
}

/// Immutable opaque Lua handle; command streams retain the internal integer ID.
struct VoiceId(u32);
impl UserData for VoiceId {}

pub(crate) fn voice_id(value: Value) -> mlua::Result<u32> {
    match value {
        Value::UserData(handle) if handle.is::<VoiceId>() => Ok(handle.borrow::<VoiceId>()?.0),
        _ => Err(mlua::Error::runtime(
            "UVI voiceId must be an issued voice handle",
        )),
    }
}

// Lua owns the cache, rather than a Rust Vec of references that can exhaust
// mlua's reference stack. Reachable handles preserve equality and table keys.
pub(crate) fn voice_handle(lua: &Lua, id: u32) -> mlua::Result<Value> {
    const CACHE: &str = "kontakto.uvi.voice_ids";
    let cache = match lua.named_registry_value::<Option<Table>>(CACHE)? {
        Some(cache) => cache,
        None => {
            let cache = lua.create_table()?;
            cache.set_metatable(Some(lua.create_table_from([("__mode", "v")])?))?;
            lua.set_named_registry_value(CACHE, cache.clone())?;
            cache
        }
    };
    let handle = cache.raw_get::<Value>(id)?;
    if !matches!(handle, Value::Nil) {
        return Ok(handle);
    }
    let handle = Value::UserData(lua.create_userdata(VoiceId(id))?);
    cache.raw_set(id, handle.clone())?;
    Ok(handle)
}

// A ScriptProcessor registers its output posts, not its incoming callbacks.
// Native releaseVoice consumes the last posted event for this scope and ID,
// then dispatches its key release through the remaining event processors.
fn posted_events(lua: &Lua, processor: Option<NodeId>) -> mlua::Result<Table> {
    const CACHE: &str = "kontakto.uvi.posted_events";
    let scopes = match lua.named_registry_value::<Option<Table>>(CACHE)? {
        Some(scopes) => scopes,
        None => {
            let scopes = lua.create_table()?;
            lua.set_named_registry_value(CACHE, scopes.clone())?;
            scopes
        }
    };
    let key = processor.map_or(0, |id| id + 1);
    if let Some(events) = scopes.raw_get::<Option<Table>>(key)? {
        return Ok(events);
    }
    let events = lua.create_table()?;
    scopes.raw_set(key, events.clone())?;
    Ok(events)
}

fn frames_at_rate(ms: f64, sample_rate: u32) -> mlua::Result<u64> {
    if !ms.is_finite() || !(0. ..=60_000.).contains(&ms) {
        return Err(mlua::Error::runtime(
            "UVI time must be finite and between 0 and 60000 ms",
        ));
    }
    Ok((ms * sample_rate as f64 / 1000.).round() as u64)
}

fn validate_sample_rate(sample_rate: u32) -> Result<()> {
    ensure!(
        (8000..=192000).contains(&sample_rate),
        "UVI sample rate must be between 8000 and 192000 Hz"
    );
    Ok(())
}

// Native postEvent truncates and clamps virtual CC fields, unlike MIDI input.
fn controller_byte(value: Value) -> u8 {
    match value {
        Value::Integer(n) => n.clamp(0, 127) as u8,
        Value::Number(n) if n.is_finite() => n.clamp(0., 127.) as u8,
        _ => 0,
    }
}

fn controller_channel(value: Value) -> mlua::Result<u8> {
    let channel = match value {
        Value::Nil => 0.,
        Value::Integer(n) => n as f64,
        Value::Number(n) => n,
        _ => {
            return Err(mlua::Error::runtime(
                "UVI controller channel must be numeric",
            ));
        }
    };
    if !(0. ..=16.).contains(&channel) {
        return Err(mlua::Error::runtime(
            "UVI controller channel must be in 0..16",
        ));
    }
    Ok(channel as u8)
}

fn event_snapshot(lua: &Lua, event: &Table) -> mlua::Result<Table> {
    let snapshot = lua.create_table()?;
    for name in [
        "type",
        "id",
        "note",
        "velocity",
        "channel",
        "controller",
        "value",
        "bend",
        "layer",
        "oscIndex",
        "vol",
        "pan",
        "tune",
        "dim1",
        "dim2",
        "input",
        "slice",
        "_follows",
        "_duration",
    ] {
        snapshot.raw_set(name, event.get::<Value>(name)?)?;
    }
    Ok(snapshot)
}

pub fn parse_notes(list: &str) -> Result<Vec<Input>> {
    parse_notes_at_rate(list, RATE as u32)
}

pub fn parse_notes_at_rate(list: &str, sample_rate: u32) -> Result<Vec<Input>> {
    validate_sample_rate(sample_rate)?;
    let mut inputs = Vec::new();
    for text in list.split(',') {
        ensure!(inputs.len() < LIMIT, "Too many UVI input notes");
        let (text, velocity) = text
            .rsplit_once(':')
            .map_or((text, Ok(100)), |(s, v)| (s, v.parse::<u8>()));
        let (note, time) = text
            .split_once('@')
            .context("Expected note@on_ms-off_ms:velocity")?;
        let (on, off) = time.split_once('-').context("Expected on_ms-off_ms")?;
        let (note, velocity) = (note.parse::<u8>()?, velocity?);
        ensure!(
            note < 128 && (1..=127).contains(&velocity),
            "Invalid MIDI note/velocity"
        );
        let (on, off) = (
            frames_at_rate(on.parse()?, sample_rate)?,
            frames_at_rate(off.parse()?, sample_rate)?,
        );
        ensure!(off > on, "Note-off must follow note-on");
        inputs.push(Input {
            frame: on,
            kind: InputKind::NoteOn {
                channel: 0,
                note,
                velocity,
            },
        });
        inputs.push(Input {
            frame: off,
            kind: InputKind::NoteOff { channel: 0, note },
        });
    }
    // Preserve authored order within each class; release before attack at a tie.
    inputs.sort_by_key(|i| (i.frame, matches!(i.kind, InputKind::NoteOn { .. })));
    Ok(inputs)
}

struct Task {
    thread: Thread,
    args: MultiValue,
    parent: Option<u64>,
    root: Option<HostRoot>,
    processor: Option<NodeId>,
    layer: Option<NodeId>,
}

struct Forward {
    event: Table,
    action: Action,
    root: Option<HostRoot>,
    after: Option<NodeId>,
    layer: Option<NodeId>,
}

#[derive(Default)]
struct EventChain {
    program: Vec<NodeId>,
    layers: Vec<(NodeId, Vec<NodeId>)>,
    owners: HashMap<NodeId, NodeId>,
}

impl EventChain {
    fn new(program: &Program) -> Result<Self> {
        let mut chain = Self {
            layers: program.layers.iter().map(|&id| (id, Vec::new())).collect(),
            ..Self::default()
        };
        for (id, node) in program
            .nodes
            .iter()
            .enumerate()
            .filter(|(_, n)| n.kind == "ScriptProcessor")
        {
            let parent = node
                .parent
                .context("UVI ScriptProcessor has no EventProcessors owner")?;
            ensure!(
                program.nodes[parent].kind == "EventProcessors",
                "UVI ScriptProcessor must belong to EventProcessors"
            );
            let owner = program.nodes[parent]
                .parent
                .context("UVI EventProcessors has no scope")?;
            if owner == program.root {
                chain.program.push(id);
            } else if let Some((_, processors)) =
                chain.layers.iter_mut().find(|(layer, _)| *layer == owner)
            {
                processors.push(id);
            } else {
                anyhow::bail!(
                    "Unsupported UVI ScriptProcessor scope {}",
                    program.nodes[owner].kind
                );
            }
            chain.owners.insert(id, owner);
        }
        Ok(chain)
    }

    fn successors(
        &self,
        after: Option<NodeId>,
        layer: Option<NodeId>,
        selected: Option<&[NodeId]>,
    ) -> Vec<(Option<NodeId>, Option<NodeId>)> {
        let processors = layer
            .and_then(|layer| {
                self.layers
                    .iter()
                    .find(|(id, _)| *id == layer)
                    .map(|(_, list)| list)
            })
            .unwrap_or(&self.program);
        let position = after
            .and_then(|id| processors.iter().position(|&p| p == id))
            .map_or(0, |position| position + 1);
        if let Some(&processor) = processors.get(position) {
            return vec![(Some(processor), layer)];
        }
        if layer.is_some() {
            return vec![(None, layer)];
        }
        self.layers
            .iter()
            .filter(|(id, _)| selected.is_none_or(|ids| ids.contains(id)))
            .map(|(id, processors)| (processors.first().copied(), Some(*id)))
            .collect()
    }
}

struct HeldTrigger {
    held: bool,
}

struct Voice {
    note: Note,
    parent: Option<u64>,
    start: u64,
    end: Option<u64>,
    kill_at: Option<u64>,
    released: bool,
    canceled: bool,
    creator: Option<NodeId>,
    creator_layer: Option<NodeId>,
    // Earliest emitted start per scope, retained when command chunks are drained.
    starts: HashMap<Option<NodeId>, u64>,
}

#[derive(Default)]
struct State {
    module_sources: Option<Rc<BTreeMap<String, Vec<u8>>>>,
    now: u64,
    sample_rate: u32,
    program_layers: Option<Vec<NodeId>>,
    tempo: f64,
    beat: f64,
    running_beat: f64,
    playing: bool,
    serial: u64,
    next_id: u32,
    current: Option<u64>,
    current_root: Option<HostRoot>,
    root_activation: Option<(u64, u64)>,
    last_root: u64,
    roots: HashMap<HostRoot, RootInput>,
    posted_roots: HashMap<(Option<NodeId>, u32), Option<HostRoot>>,
    command_roots: Vec<Option<HostRoot>>,
    current_processor: Option<NodeId>,
    current_layer: Option<NodeId>,
    chain: Option<EventChain>,
    forwards: BTreeMap<(u64, u64), Forward>,
    initializing_chain: bool,
    tasks: BTreeMap<(u64, u64), Task>,
    voices: HashMap<u32, Voice>,
    next_trigger: u64,
    triggers: HashMap<u64, HeldTrigger>,
    // Hosted-only ancestry; an empty legacy map allocates nothing.
    trigger_roots: HashMap<u64, HostRoot>,
    trigger_queues: HashMap<(Option<NodeId>, u32, u8), VecDeque<u64>>,
    input_velocities: HashMap<u32, u8>,
    keys: HashMap<(u8, u8), VecDeque<u32>>,
    ccs: HashMap<u8, u8>,
    commands: Vec<Command>,
    logs: Vec<String>,
    log_bytes: usize,
    dropped_logs: usize,
}

impl State {
    fn live_root(&self, root: Option<HostRoot>) -> Option<HostRoot> {
        root.filter(|root| {
            self.roots
                .get(root)
                .is_some_and(|owner| owner.ended.is_none() && !owner.choked)
        })
    }
    fn note_held(&self, trigger: u64) -> bool {
        self.triggers
            .get(&trigger)
            .is_some_and(|trigger| trigger.held)
    }
    fn receive(
        &mut self,
        lua: &Lua,
        event: &Table,
        processor: Option<NodeId>,
        root: Option<HostRoot>,
    ) -> mlua::Result<Option<u64>> {
        let kind = event.get::<u32>("type")?;
        if kind != 144 && kind != 128 {
            return Ok(None);
        }
        let id = voice_id(event.get::<Value>("id")?)?;
        let note = event.get::<u8>("note")?;
        if kind == 144 {
            if self.triggers.len() >= LIMIT {
                return Err(mlua::Error::runtime("UVI trigger limit exceeded"));
            }
            self.next_trigger = self
                .next_trigger
                .checked_add(1)
                .ok_or_else(|| mlua::Error::runtime("UVI trigger ID space exhausted"))?;
            let trigger = self.next_trigger;
            self.triggers.insert(trigger, HeldTrigger { held: true });
            if let Some(root) = self.live_root(root) {
                self.trigger_roots.insert(trigger, root);
            }
            self.trigger_queues
                .entry((processor, id, note))
                .or_default()
                .push_back(trigger);
            return Ok(Some(trigger));
        }
        let trigger = self
            .trigger_queues
            .get_mut(&(processor, id, note))
            .and_then(VecDeque::pop_back);
        if let Some(trigger) = trigger {
            self.triggers.get_mut(&trigger).unwrap().held = false;
            let children = self
                .voices
                .iter()
                .filter(|(_, v)| v.parent == Some(trigger))
                .map(|(&id, v)| (id, v.creator, v.creator_layer))
                .collect::<Vec<_>>();
            let previous_processor = self.current_processor;
            let previous_layer = self.current_layer;
            for (child, creator, creator_layer) in children {
                self.current_processor = creator;
                self.current_layer = creator_layer;
                let result = self.release(lua, child);
                self.current_processor = previous_processor;
                self.current_layer = previous_layer;
                result?;
            }
        }
        Ok(trigger)
    }
    fn set_time(&mut self, at: u64) {
        let beats = (at - self.now) as f64 / self.sample_rate as f64 * self.tempo / 60.;
        self.running_beat += beats;
        if self.playing {
            self.beat += beats;
        }
        self.now = at;
    }

    fn id(&mut self) -> mlua::Result<u32> {
        self.next_id = self
            .next_id
            .checked_add(1)
            .ok_or_else(|| mlua::Error::runtime("UVI voice ID space exhausted"))?;
        Ok(self.next_id)
    }

    fn emit(&mut self, frame: u64, action: Action) -> mlua::Result<()> {
        if self.commands.len() >= LIMIT {
            return Err(mlua::Error::runtime("UVI output command limit exceeded"));
        }
        if let Action::Start(note) = &action
            && let Some(voice) = self.voices.get_mut(&note.id)
        {
            let mut scopes = vec![None];
            scopes.extend(
                note.layers
                    .as_ref()
                    .map_or_else(
                        || self.program_layers.clone().unwrap_or_default(),
                        Clone::clone,
                    )
                    .into_iter()
                    .map(Some),
            );
            for scope in scopes {
                voice
                    .starts
                    .entry(scope)
                    .and_modify(|at| *at = (*at).min(frame))
                    .or_insert(frame);
            }
        }
        self.commands.push(Command { frame, action });
        if self.root_activation.is_some() {
            self.command_roots.push(self.live_root(self.current_root));
        }
        Ok(())
    }

    fn emit_event(&mut self, frame: u64, action: Action, event: &Table) -> mlua::Result<()> {
        if self.chain.is_none() {
            return self.emit(frame, action);
        }
        if self.tasks.len() + self.forwards.len() >= 1024 {
            return Err(mlua::Error::runtime("UVI pending callback limit exceeded"));
        }
        self.serial = self
            .serial
            .checked_add(1)
            .ok_or_else(|| mlua::Error::runtime("UVI scheduler ID space exhausted"))?;
        self.forwards.insert(
            (frame, self.serial),
            Forward {
                event: event.clone(),
                action,
                root: self.live_root(self.current_root),
                after: self.current_processor,
                layer: self.current_layer,
            },
        );
        Ok(())
    }

    fn schedule(&mut self, frame: u64, task: Task) -> mlua::Result<()> {
        if self.tasks.len() + self.forwards.len() >= 1024 {
            return Err(mlua::Error::runtime("UVI pending callback limit exceeded"));
        }
        self.serial = self
            .serial
            .checked_add(1)
            .ok_or_else(|| mlua::Error::runtime("UVI scheduler ID space exhausted"))?;
        self.tasks.insert((frame, self.serial), task);
        Ok(())
    }

    fn release(&mut self, lua: &Lua, id: u32) -> mlua::Result<bool> {
        let events = posted_events(lua, self.current_processor)?;
        let Some(posted) = events.raw_get::<Option<Table>>(id)? else {
            return Ok(false);
        };
        events.raw_set(id, Value::Nil)?;
        let root = self
            .posted_roots
            .remove(&(self.current_processor, id))
            .flatten();
        if posted
            .raw_get::<Option<u64>>("_expires")?
            .is_some_and(|end| self.now >= end)
        {
            return Ok(false);
        }
        let event = event_snapshot(lua, &posted)?;
        event.set("id", voice_handle(lua, id)?)?;
        event.set("type", 128)?;
        event.raw_set("_duration", Value::Nil)?;
        event.raw_set("_follows", Value::Nil)?;
        let previous = self.current_root;
        self.current_root = self.live_root(root);
        let result = self.post(lua, &event, 0.);
        self.current_root = previous;
        result?;
        Ok(true)
    }

    fn release_note(
        &mut self,
        at: u64,
        id: u32,
        note: u8,
        channel: u8,
        layer: Option<NodeId>,
    ) -> mlua::Result<()> {
        if let Some(voice) = self.voices.get_mut(&id)
            && voice.note.note == note
            && voice.start <= at
            && layer.is_none_or(|layer| {
                voice
                    .note
                    .layers
                    .as_ref()
                    .is_none_or(|layers| layers.contains(&layer))
            })
        {
            if at == self.now {
                voice.released = true;
            } else {
                voice.end = Some(voice.end.map_or(at, |end| end.min(at)));
            }
        }
        self.emit(
            at,
            Action::ReleaseNote {
                id,
                note,
                channel,
                layer,
            },
        )
    }

    fn layers(&self, selector: Value) -> mlua::Result<Option<Vec<NodeId>>> {
        if matches!(selector, Value::Nil) {
            return Ok(None);
        }
        let layers = self.program_layers.as_ref().ok_or_else(|| {
            mlua::Error::runtime("UVI layer routing is unavailable in a standalone mapping")
        })?;
        let resolve = |value| -> mlua::Result<NodeId> {
            let index = positive_index(value, "layer", layers.len())?;
            Ok(layers[index - 1])
        };
        let mut selected = Vec::new();
        match selector {
            Value::Table(list) => {
                let len = list.raw_len();
                if len > LIMIT {
                    return Err(mlua::Error::runtime("UVI layer selection exceeds limit"));
                }
                let mut keys = 0;
                for pair in list.pairs::<Value, Value>() {
                    let (key, _) = pair?;
                    positive_index(key, "layer list key", len)?;
                    keys += 1;
                }
                if keys != len {
                    return Err(mlua::Error::runtime(
                        "UVI layers require a contiguous list of indices",
                    ));
                }
                let mut seen = HashSet::new();
                for i in 1..=len {
                    let id = resolve(list.raw_get::<Value>(i)?)?;
                    if seen.insert(id) {
                        selected.push(id);
                    }
                }
            }
            value => selected.push(resolve(value)?),
        }
        Ok(Some(selected))
    }

    fn fade(
        &mut self,
        id: u32,
        start: Option<f64>,
        target: f64,
        duration: f64,
        kill: bool,
        selector: Value,
    ) -> mlua::Result<()> {
        if !target.is_finite() || start.is_some_and(|v| !v.is_finite()) {
            return Err(mlua::Error::runtime("Nonfinite UVI fade gain"));
        }
        let duration_frames = frames_at_rate(duration, self.sample_rate)?;
        let layer = match selector {
            Value::Nil | Value::Integer(0) => self.current_layer,
            Value::Number(0.) => self.current_layer,
            value => {
                let layers = self.program_layers.as_ref().ok_or_else(|| {
                    mlua::Error::runtime(
                        "UVI layer fade routing is unavailable in a standalone mapping",
                    )
                })?;
                Some(layers[positive_index(value, "fade layer", layers.len())? - 1])
            }
        };
        let voice = self
            .voices
            .get_mut(&id)
            .ok_or_else(|| mlua::Error::runtime("Unknown UVI fade voice"))?;
        if voice.canceled || voice.kill_at.is_some_and(|end| self.now >= end) {
            return Ok(());
        }
        if layer.is_none() {
            voice.kill_at = if kill {
                Some(
                    self.now
                        .checked_add(duration_frames)
                        .ok_or_else(|| mlua::Error::runtime("UVI timeline overflow"))?,
                )
            } else {
                None
            };
        }
        self.emit(
            self.now,
            Action::Fade {
                id,
                start: start.map(|v| v.clamp(0., 1.) as f32),
                target: target.clamp(0., 1.) as f32,
                duration_frames,
                kill,
                layer,
            },
        )
    }

    fn post(&mut self, lua: &Lua, event: &Table, delta: f64) -> mlua::Result<Option<u32>> {
        let at = self
            .now
            .checked_add(frames_at_rate(delta, self.sample_rate)?)
            .ok_or_else(|| mlua::Error::runtime("UVI timeline overflow"))?;
        match event.get::<u32>("type")? {
            144 => {
                let raw_id = match event.get::<Value>("id")? {
                    Value::Nil => 0,
                    value => voice_id(value)?,
                };
                let id = if raw_id > 0 && raw_id <= self.next_id {
                    raw_id
                } else {
                    self.id()?
                };
                let note = event.get::<u8>("note")?;
                let velocity = match event.get::<Value>("velocity")? {
                    Value::Nil => 100.,
                    Value::Integer(value) => value as f64,
                    Value::Number(value) => value,
                    _ => return Err(mlua::Error::runtime("Invalid UVI note velocity")),
                };
                // Native NoteOn decoding truncates, narrows to signed 32 bits, then clamps.
                let velocity = (velocity as i64 as i32).clamp(0, 127) as u8;
                let channel = event.get::<Option<u8>>("channel")?.unwrap_or(1);
                let volume = event.get::<Option<f32>>("vol")?.unwrap_or(1.);
                let pan = event.get::<Option<f32>>("pan")?.unwrap_or(0.);
                let tune = event.get::<Option<f64>>("tune")?.unwrap_or(0.);
                if note > 127
                    || !(1..=16).contains(&channel)
                    || !volume.is_finite()
                    || volume < 0.
                    || !pan.is_finite()
                    || !(-1. ..=1.).contains(&pan)
                    || !tune.is_finite()
                    || tune.abs() > 120.
                {
                    return Err(mlua::Error::runtime("Invalid UVI note fields"));
                }
                for unsupported in ["input", "slice"] {
                    if !matches!(event.get::<Value>(unsupported)?, Value::Nil) {
                        return Err(mlua::Error::runtime(format!(
                            "UVI {unsupported} routing is unavailable in a standalone mapping"
                        )));
                    }
                }
                let layers = self.layers(event.get::<Value>("layer")?)?;
                let oscillator = match event.get::<Value>("oscIndex")? {
                    Value::Nil => None,
                    selector => {
                        if self.program_layers.is_none() {
                            return Err(mlua::Error::runtime(
                                "UVI oscIndex routing is unavailable in a standalone mapping",
                            ));
                        }
                        Some(positive_index(selector, "oscIndex", LIMIT)?)
                    }
                };
                let dim1 = event.get::<Option<i64>>("dim1")?.unwrap_or(0).max(0) as usize;
                let dim2 = event
                    .get::<Option<i64>>("dim2")?
                    .map(|v| v.clamp(0, u32::MAX as i64) as u32);
                if self.program_layers.is_some() && (dim1 != 0 || dim2.is_some()) {
                    return Err(mlua::Error::runtime(
                        "Native UVI Program dimension dispatch requires a SampleMappingOscillator",
                    ));
                }
                let note = Note {
                    id,
                    note,
                    velocity,
                    channel: channel - 1,
                    dim1,
                    dim2,
                    layers,
                    oscillator,
                    volume,
                    pan,
                    tune,
                    offset_us: event.raw_get::<Option<u64>>("_offset_us")?.unwrap_or(0),
                };
                let follows = event.get::<Option<bool>>("_follows")?.unwrap_or(false);
                let previous = self.voices.get(&id);
                let previous_end = previous.and_then(|v| v.end);
                let newly_posted = previous.is_none();
                let parent = previous
                    .and_then(|v| v.parent)
                    .or_else(|| follows.then_some(self.current).flatten());
                let creator = previous.map_or(self.current_processor, |v| v.creator);
                let creator_layer = previous.map_or(self.current_layer, |v| v.creator_layer);
                let canceled = previous.is_some_and(|v| v.canceled)
                    || parent.is_some_and(|p| !self.note_held(p));
                let end = event
                    .get::<Option<f64>>("_duration")?
                    .map(|ms| {
                        frames_at_rate(ms, self.sample_rate).and_then(|duration| {
                            at.checked_add(duration)
                                .ok_or_else(|| mlua::Error::runtime("UVI timeline overflow"))
                        })
                    })
                    .transpose()?
                    .or(previous_end);
                let starts = previous.map(|v| v.starts.clone()).unwrap_or_default();
                if newly_posted && self.voices.len() >= LIMIT {
                    return Err(mlua::Error::runtime("UVI retained voice limit exceeded"));
                }
                self.voices.insert(
                    id,
                    Voice {
                        note: note.clone(),
                        parent,
                        start: at,
                        end,
                        kill_at: None,
                        released: canceled,
                        canceled,
                        creator,
                        creator_layer,
                        starts,
                    },
                );
                event.set("id", voice_handle(lua, id)?)?;
                event.set("velocity", note.velocity)?;
                event.set("pan", note.pan)?;
                event.set("vol", note.volume)?;
                event.set("tune", note.tune)?;
                let release_note = note.note;
                let release_channel = note.channel;
                self.emit_event(at, Action::Start(note), event)?;
                if let Some(end) = end.filter(|_| newly_posted) {
                    self.emit_event(
                        end,
                        Action::ReleaseNote {
                            id,
                            note: release_note,
                            channel: release_channel,
                            layer: None,
                        },
                        event,
                    )?;
                }
                Ok(Some(id))
            }
            128 => {
                let id = voice_id(event.get::<Value>("id")?)?;
                let note = event.get::<u8>("note")?;
                let channel = event.get::<Option<u8>>("channel")?.unwrap_or(1);
                if note > 127 || !(1..=16).contains(&channel) {
                    return Err(mlua::Error::runtime("Invalid UVI release fields"));
                }
                if self.chain.is_some() {
                    self.emit_event(
                        at,
                        Action::ReleaseNote {
                            id,
                            note,
                            channel: channel - 1,
                            layer: None,
                        },
                        event,
                    )?;
                } else {
                    self.release_note(at, id, note, channel - 1, None)?;
                }
                Ok(Some(id))
            }
            176 => {
                let channel = controller_channel(event.get::<Value>("channel")?)?;
                let controller = controller_byte(event.get::<Value>("controller")?);
                let value = controller_byte(event.get::<Value>("value")?);
                let action = if channel == 0 {
                    Action::ControllerAll { controller, value }
                } else {
                    Action::Controller {
                        channel: channel - 1,
                        controller,
                        value,
                    }
                };
                self.emit_event(at, action, event)?;
                Ok(None)
            }
            160 => {
                let channel = controller_channel(event.get::<Value>("channel")?)?;
                let note = controller_byte(event.get::<Value>("note")?);
                let value = controller_byte(event.get::<Value>("value")?);
                let action = if channel == 0 {
                    Action::PolyAfterTouchAll { note, value }
                } else {
                    Action::PolyAfterTouch {
                        channel: channel - 1,
                        note,
                        value,
                    }
                };
                self.emit_event(at, action, event)?;
                Ok(None)
            }
            224 => {
                let channel = event.get::<Option<u8>>("channel")?.unwrap_or(1);
                let bend = event.get::<f64>("bend")?;
                if !(1..=16).contains(&channel) || !bend.is_finite() || !(-1. ..=1.).contains(&bend)
                {
                    return Err(mlua::Error::runtime("Invalid UVI pitch bend fields"));
                }
                self.emit_event(
                    at,
                    Action::PitchBend {
                        channel: channel - 1,
                        bend,
                    },
                    event,
                )?;
                Ok(None)
            }
            208 => {
                let channel = event.get::<Option<u8>>("channel")?.unwrap_or(1);
                let value = event.get::<u8>("value")?;
                if !(1..=16).contains(&channel) || value > 127 {
                    return Err(mlua::Error::runtime("Invalid UVI aftertouch fields"));
                }
                self.emit_event(
                    at,
                    Action::AfterTouch {
                        channel: channel - 1,
                        value,
                    },
                    event,
                )?;
                Ok(None)
            }
            kind => Err(mlua::Error::runtime(format!(
                "Unsupported UVI event type {kind}"
            ))),
        }
    }
}

const BOOTSTRAP: &str = r#"
local yield, nativePcall, nativeXpcall, nativeGC = coroutine.yield, pcall, xpcall, collectgarbage
local check, post, release, start, immediate = _check, _post, _release, _spawn, _run
function pcall(f, ...)
    check()
    local result = {nativePcall(f, ...)}
    check()
    return unpack(result, 1, table.maxn(result))
end
function xpcall(f, handler)
    check()
    local result = {nativeXpcall(f, handler)}
    check()
    return unpack(result, 1, table.maxn(result))
end
-- Real Lua 5.1 collection/query in the allocating offline VM, never the audio callback.
function collectgarbage(option)
    check()
    if option == nil then option = 'collect' end
    assert(option == 'collect' or option == 'count', 'Unsupported UVI offline GC operation')
    local result = nativeGC(option)
    check()
    return result
end
function wait(ms) return yield(ms) end
function waitBeat(beats) return wait(beat2ms(beats)) end
function spawn(f, ...) return start(f, ...) end
function run(f, ...) return immediate(f, ...) end
function postEvent(e, delta) return post(e, delta or 0) end
local function midi(kind, value, channel, input, controller)
    assert(input == nil, 'MIDI input routing is unavailable')
    local e = {type=kind,channel=channel,value=value,controller=controller}
    if kind == Event.PitchBend then e.bend=value; e.value=nil end
    return post(e, 0)
end
function controlChange(cc, value, channel, input) return midi(Event.ControlChange,value,channel,input,cc) end
function pitchBend(bend, channel, input) return midi(Event.PitchBend,bend,channel,input) end
function afterTouch(value, channel, input) return midi(Event.AfterTouch,value,channel,input) end
function polyAfterTouch(value, note, channel, input)
    assert(input == nil, 'MIDI input routing is unavailable')
    return post({type=Event.PolyAfterTouch,note=note,value=value,channel=channel},0)
end
function releaseVoice(id) return release(id) end
function playNote(note, velocity, duration, layer, channel, input, vol, pan, tune, slice, oscIndex)
    local e
    if type(note) == 'table' then
        e = {}
        local names = {'note','velocity','duration','layer','channel','input','vol','pan','tune','slice','oscIndex'}
        for i, name in ipairs(names) do e[name] = note[name]; if e[name] == nil then e[name] = note[i] end end
    else
        e = {note=note, velocity=velocity, duration=duration, layer=layer, channel=channel,
             input=input, vol=vol, pan=pan, tune=tune, slice=slice, oscIndex=oscIndex}
    end
    e.type = Event.NoteOn
    if e.duration == nil then e.duration = -1 end
    assert(type(e.duration) == 'number' and e.duration >= -1, 'invalid note duration')
    if e.duration == -1 then e._follows = true end
    if e.duration > 0 then e._duration = e.duration end
    return post(e, 0)
end
function beat2ms(beats) return beats * 60000 / getTempo() end
function ms2beat(ms) return ms * getTempo() / 60000 end
function getBeatDuration() return beat2ms(1) end
function getBarDuration() return beat2ms(4) end
function getTimeSignature() return 4, 4 end
function waitForRelease() while isNoteHeld() do wait(1) end end
function table.copy(t) local r = {}; for k,v in pairs(t) do r[k] = v end; return r end
coroutine, dofile, loadfile, load, loadstring, newproxy = nil, nil, nil, nil, nil, nil
_check, _post, _release, _spawn, _run = nil, nil, nil, nil, nil
"#;

fn positive_index(value: Value, field: &str, max: usize) -> mlua::Result<usize> {
    let number = match value {
        Value::Integer(n) => n as f64,
        Value::Number(n) => n,
        _ => {
            return Err(mlua::Error::runtime(format!(
                "UVI {field} requires a one-based integer index"
            )));
        }
    };
    if !number.is_finite() || number.fract() != 0. || number < 1. || number > max as f64 {
        return Err(mlua::Error::runtime(format!(
            "UVI {field} index is out of range"
        )));
    }
    Ok(number as usize)
}

// Both immediate run() and scheduled callbacks use this one resume path.
// Nested run() shares the outer instruction budget and loses note context.
fn resume_task(
    task: Task,
    state: &Rc<RefCell<State>>,
    fuel: &Cell<u32>,
    resumes: &Cell<usize>,
    depth: &Cell<usize>,
) -> mlua::Result<()> {
    if depth.get() >= 64 {
        return Err(mlua::Error::runtime("UVI run nesting limit exceeded"));
    }
    if resumes.get() >= LIMIT {
        return Err(mlua::Error::runtime("UVI coroutine resume budget exceeded"));
    }
    resumes.set(resumes.get() + 1);
    depth.set(depth.get() + 1);
    let previous = state.borrow().current;
    let previous_root = state.borrow().current_root;
    let previous_processor = state.borrow().current_processor;
    let previous_layer = state.borrow().current_layer;
    state.borrow_mut().current = task.parent;
    let root = state.borrow().live_root(task.root);
    state.borrow_mut().current_root = root;
    state.borrow_mut().current_processor = task.processor;
    state.borrow_mut().current_layer = task.layer;
    let result = task.thread.resume::<MultiValue>(task.args).map_err(|cause| {
        let state = state.borrow();
        super::lua_failure::capture(
            cause, &task.thread, task.processor, state.now, state.module_sources.as_deref(),
        )
    });
    state.borrow_mut().current = previous;
    state.borrow_mut().current_root = previous_root;
    state.borrow_mut().current_processor = previous_processor;
    state.borrow_mut().current_layer = previous_layer;
    depth.set(depth.get() - 1);
    if fuel.get() == 0 {
        return Err(result
            .err()
            .unwrap_or_else(|| mlua::Error::runtime("UVI instruction budget exceeded")));
    }
    let yielded = result?;
    if task.thread.status() == ThreadStatus::Resumable {
        let delay = match yielded.front() {
            Some(Value::Number(ms)) => frames_at_rate(*ms, state.borrow().sample_rate)?,
            Some(Value::Integer(ms)) => frames_at_rate(*ms as f64, state.borrow().sample_rate)?,
            _ => return Err(mlua::Error::runtime("UVI wait requires milliseconds")),
        };
        let mut s = state.borrow_mut();
        let wake = s
            .now
            .checked_add(delay.max(1))
            .ok_or_else(|| mlua::Error::runtime("UVI timeline overflow"))?;
        s.schedule(
            wake,
            Task {
                thread: task.thread,
                args: MultiValue::new(),
                parent: task.parent,
                root: task.root,
                processor: task.processor,
                layer: task.layer,
            },
        )?;
    }
    Ok(())
}

// Observed <state> is JSON for onSave's supported nested table values.
// This decodes data only: saved strings are never evaluated as Lua source.
pub(crate) fn saved_value(
    lua: &Lua,
    value: &serde_json::Value,
    depth: usize,
    count: &mut usize,
) -> Result<Value> {
    *count += 1;
    ensure!(
        depth <= 64 && *count <= LIMIT,
        "UVI saved state exceeds structure limit"
    );
    Ok(match value {
        serde_json::Value::Bool(b) => Value::Boolean(*b),
        serde_json::Value::Number(n) => {
            let n = n
                .as_f64()
                .context("UVI saved state number is unsupported")?;
            ensure!(n.is_finite(), "Nonfinite UVI saved state number");
            Value::Number(n)
        }
        serde_json::Value::String(s) => Value::String(lua.create_string(s)?),
        serde_json::Value::Array(values) => {
            let table = lua.create_table()?;
            for (i, value) in values.iter().enumerate() {
                table.set(i + 1, saved_value(lua, value, depth + 1, count)?)?;
            }
            Value::Table(table)
        }
        serde_json::Value::Object(values) => {
            let table = lua.create_table()?;
            for (key, value) in values {
                table.set(key.as_str(), saved_value(lua, value, depth + 1, count)?)?;
            }
            Value::Table(table)
        }
        // Native JSON conversion represents null as nil: object members vanish,
        // while later numeric array indices retain their original positions.
        serde_json::Value::Null => Value::Nil,
    })
}

struct Runtime {
    lua: Lua,
    state: Rc<RefCell<State>>,
    fuel: Rc<Cell<u32>>,
    fuel_line: Rc<Cell<Option<usize>>>,
    budget_failure: Rc<RefCell<Option<mlua::Error>>>,
    resumes: Rc<Cell<usize>>,
    depth: Rc<Cell<usize>>,
    host: Option<host::Host>,
    environments: BTreeMap<NodeId, Table>,
    gc_drains: u8,
}

impl Runtime {
    fn reset_budget(&self) {
        // onSave can fail without retiring a Session. First-cause context must
        // live for exactly one fuel allocation, including its _check rethrows.
        self.fuel_line.set(None);
        *self.budget_failure.borrow_mut() = None;
        self.fuel.set(FUEL);
    }

    fn new_chain(
        program: &Program,
        modules: BTreeMap<String, Vec<u8>>,
        resources: Option<host::Resources>,
        sample_rate: u32,
        saved: Option<&super::state::SavedState>,
    ) -> Result<Self> {
        if let Some(saved) = saved {
            saved.validate(program)?;
        }
        let mut rt = Self::vm(Some(program), modules, resources, sample_rate)?;
        if let Some(saved) = saved {
            saved.preload(&rt.lua, rt.host.as_ref().unwrap())?;
        }
        rt.state.borrow_mut().chain = Some(EventChain::new(program)?);
        let processors = program
            .nodes
            .iter()
            .enumerate()
            .filter(|(_, n)| n.kind == "ScriptProcessor")
            .map(|(id, _)| id)
            .collect::<Vec<_>>();
        ensure!(
            processors.len() <= 64,
            "UVI event chain exceeds 64 ScriptProcessors"
        );
        let mut source_bytes = 0usize;
        let mut sources = Vec::new();
        rt.state.borrow_mut().initializing_chain = true;
        for &processor in &processors {
            let source = program
                .nodes
                .iter()
                .filter(|n| n.parent == Some(processor) && n.kind == "script")
                .collect::<Vec<_>>();
            ensure!(
                source.len() <= 1,
                "Multiple UVI script sources on one processor"
            );
            let source = source.first().map_or("", |n| n.text.as_str());
            source_bytes += source.len();
            ensure!(
                source_bytes <= 2 << 20 && !source.as_bytes().starts_with(b"\x1bLua"),
                "UVI event-chain sources exceed source bounds"
            );
            let environment = rt
                .host
                .as_ref()
                .unwrap()
                .script_environment(&rt.lua, program, processor)?;
            rt.environments.insert(processor, environment.clone());
            sources.push((processor, source));
        }
        for (processor, source) in sources {
            let environment = rt.environments[&processor].clone();
            rt.scope(processor);
            rt.reset_budget();
            rt.lua
                .load(source)
                .set_name(format!("UVI ScriptProcessor node {processor}"))
                .set_environment(environment.clone())
                .exec()
                .map_err(|cause| super::lua_failure::initialization(
                    cause, Some(processor), rt.state.borrow().now,
                    &format!("UVI ScriptProcessor node {processor}"),
                ))
                .context("UVI scoped Lua initialization")?;
            ensure!(rt.fuel.get() > 0, "UVI instruction budget exceeded");
            rt.scope(processor);
            let restored = saved
                .and_then(|state| state.processor(processor))
                .map(|text| host::parse_state(text.as_bytes()))
                .transpose()?;
            let states = program
                .nodes
                .iter()
                .filter(|n| n.parent == Some(processor) && n.kind == "state")
                .collect::<Vec<_>>();
            ensure!(
                states.len() <= 1,
                "Multiple UVI saved states on one processor"
            );
            let value = if let Some((_, value)) = &restored {
                value.clone()
            } else {
                states
                    .first()
                    .map(|saved| {
                        serde_json::from_str(&saved.text).context("Invalid UVI saved JSON state")
                    })
                    .transpose()?
            };
            if let Some(value) = value {
                ensure!(
                    value.is_object() || value.is_array() || value.is_null(),
                    "UVI saved script state must be a table or nil"
                );
                let value = saved_value(&rt.lua, &value, 0, &mut 0)?;
                if let Some(f) = environment.get::<Option<Function>>("onLoad")? {
                    rt.spawn(f, MultiValue::from_vec(vec![value]), None)?;
                    rt.advance(0)?;
                }
            }
            rt.scope(processor);
            rt.reset_budget();
            if let Some((program, _)) = &restored {
                host::restore_saved_widgets(&rt.lua, program, &environment)?;
            } else {
                host::restore_widgets_scoped(&rt.lua, program, processor, &environment)?;
            }
            ensure!(rt.fuel.get() > 0, "UVI instruction budget exceeded");
            rt.advance(0)?;
        }
        for processor in processors {
            let environment = rt.environments[&processor].clone();
            if rt.processor_enabled(processor) {
                rt.scope(processor);
                if let Some(f) = environment.get::<Option<Function>>("onInit")? {
                    rt.spawn(f, MultiValue::new(), None)?;
                    rt.advance(0)?;
                }
            }
        }
        rt.state.borrow_mut().initializing_chain = false;
        rt.advance(0)?;
        Ok(rt)
    }

    fn scope(&mut self, processor: NodeId) {
        let mut state = self.state.borrow_mut();
        state.current_processor = Some(processor);
        let owner = state
            .chain
            .as_ref()
            .and_then(|chain| chain.owners.get(&processor))
            .copied();
        state.current_layer = owner.filter(|owner| {
            state
                .program_layers
                .as_ref()
                .is_some_and(|layers| layers.contains(owner))
        });
    }

    fn processor_enabled(&self, processor: NodeId) -> bool {
        !self.host.as_ref().unwrap().parameters.borrow()[processor]
            .get("Bypass")
            .is_some_and(|v| {
                matches!(v, host::ParameterValue::Boolean(true))
                    || matches!(v, host::ParameterValue::Number(n) if *n != 0.)
            })
    }

    fn deliver(
        &mut self,
        event: &Table,
        after: Option<NodeId>,
        layer: Option<NodeId>,
        parent: Option<u64>,
        selected: Option<&[NodeId]>,
        root: Option<HostRoot>,
    ) -> Result<()> {
        let targets = self
            .state
            .borrow()
            .chain
            .as_ref()
            .unwrap()
            .successors(after, layer, selected);
        for (processor, layer) in targets {
            if let Some(processor) = processor {
                if self.processor_enabled(processor) {
                    let parent =
                        self.state
                            .borrow_mut()
                            .receive(&self.lua, event, Some(processor), root)?;
                    let environment = self.environments[&processor].clone();
                    let callback = match event.get::<u32>("type")? {
                        144 => "onNote",
                        128 => "onRelease",
                        176 => "onController",
                        224 => "onPitchBend",
                        208 => "onAfterTouch",
                        160 => "onPolyAfterTouch",
                        _ => anyhow::bail!("Unsupported UVI chain event type"),
                    };
                    let handler = match environment.get::<Option<Function>>("onEvent")? {
                        Some(handler) => Some(handler),
                        None => environment.get::<Option<Function>>(callback)?,
                    };
                    if let Some(f) = handler {
                        let event = event_snapshot(&self.lua, event)?;
                        let thread = self.lua.create_thread(f)?;
                        let mut state = self.state.borrow_mut();
                        let now = state.now;
                        state.schedule(
                            now,
                            Task {
                                thread,
                                args: MultiValue::from_vec(vec![Value::Table(event)]),
                                parent,
                                root,
                                processor: Some(processor),
                                layer,
                            },
                        )?;
                        continue;
                    }
                }
                self.deliver(event, Some(processor), layer, parent, selected, root)?;
            } else {
                // Automatic Layer dispatch retains offsets. An authored postEvent
                // callback snapshots only documented event fields, resetting it.
                let offset = event.raw_get::<Option<u64>>("_offset_us")?;
                let event = event_snapshot(&self.lua, event)?;
                if let Some(offset) = offset {
                    event.raw_set("_offset_us", offset)?;
                }
                if event.get::<u32>("type")? == 128
                    && let Some(layer) = layer
                {
                    let id = voice_id(event.get::<Value>("id")?)?;
                    let note = event.get::<u8>("note")?;
                    let channel = event.get::<Option<u8>>("channel")?.unwrap_or(1);
                    ensure!(
                        note <= 127 && (1..=16).contains(&channel),
                        "Invalid UVI release fields"
                    );
                    let mut state = self.state.borrow_mut();
                    let now = state.now;
                    let previous_root = state.current_root;
                    state.current_root = state.live_root(root);
                    let result = state.release_note(now, id, note, channel - 1, Some(layer));
                    state.current_root = previous_root;
                    result?;
                    continue;
                }
                if event.get::<u32>("type")? == 144
                    && let Some(layer) = layer
                {
                    let index = self
                        .state
                        .borrow()
                        .program_layers
                        .as_ref()
                        .unwrap()
                        .iter()
                        .position(|&id| id == layer)
                        .unwrap()
                        + 1;
                    event.set("layer", index)?;
                }
                let mut state = self.state.borrow_mut();
                let chain = state.chain.take();
                let previous = state.current;
                let previous_root = state.current_root;
                state.current = parent;
                state.current_root = state.live_root(root);
                let result = state.post(&self.lua, &event, 0.);
                state.current = previous;
                state.current_root = previous_root;
                state.chain = chain;
                result?;
            }
        }
        Ok(())
    }

    fn new(
        source: &str,
        name: &str,
        program: Option<&Program>,
        modules: BTreeMap<String, Vec<u8>>,
        resources: Option<host::Resources>,
    ) -> Result<Self> {
        let mut rt = Self::vm(program, modules, resources, RATE as u32)?;
        rt.initialize(source, name, program)?;
        Ok(rt)
    }

    fn vm(
        program: Option<&Program>,
        modules: BTreeMap<String, Vec<u8>>,
        resources: Option<host::Resources>,
        sample_rate: u32,
    ) -> Result<Self> {
        validate_sample_rate(sample_rate)?;
        // Lua 5.1 math.random/randomseed use the platform CRT, including its
        // shared seed state. Linux fixed-seed replays establish local regression
        // parity only: the native MSVC stream differs and its full context/
        // seeding lifecycle remains unmeasured.
        // See docs/UVI_LUA_RANDOM_EVIDENCE.md before changing RNG or seeding.
        let lua = Lua::new_with(
            StdLib::TABLE | StdLib::STRING | StdLib::MATH,
            LuaOptions::default(),
        )?;
        lua.set_memory_limit(32 << 20)?;
        let state = Rc::new(RefCell::new(State {
            tempo: TEMPO,
            sample_rate,
            program_layers: program.map(|p| p.layers.clone()),
            ..State::default()
        }));
        let budget_failure = Rc::new(RefCell::new(None::<mlua::Error>));
        let hook_failure = budget_failure.clone();
        let hook_state = state.clone();
        let fuel = Rc::new(Cell::new(FUEL));
        let fuel_line = Rc::new(Cell::new(None));
        let hook_line = fuel_line.clone();
        let hook_fuel = fuel.clone();
        lua.set_global_hook(
            HookTriggers::new().every_nth_instruction(1000),
            move |_, debug| {
                let remaining = hook_fuel.get().saturating_sub(1000);
                hook_fuel.set(remaining);
                if remaining == 0 {
                    if hook_line.get().is_none() {
                        hook_line.set(debug.current_line());
                    }
                    // Keep this budget cycle's first structured cause when the
                    // existing pcall/xpcall _check guard rethrows it.
                    let mut retained = hook_failure.borrow_mut();
                    let failure = retained.get_or_insert_with(|| {
                        let cause = mlua::Error::runtime(format!(
                            "UVI instruction budget exceeded at Lua line {:?}", hook_line.get(),
                        ));
                        match hook_state.try_borrow() {
                            Ok(state) => super::lua_failure::budget(
                                cause, debug, state.current_processor, state.now,
                                state.module_sources.as_deref(),
                            ),
                            Err(_) => cause, // Keep the original error if scope cannot be read.
                        }
                    });
                    Err(failure.clone())
                } else {
                    Ok(VmState::Continue)
                }
            },
        )?;
        let resumes = Rc::new(Cell::new(0));
        let depth = Rc::new(Cell::new(0));
        let globals = lua.globals();
        let stringify = globals.get::<Function>("tostring")?;
        let logger = state.clone();
        globals.set(
            "print",
            lua.create_function(move |_, args: MultiValue| {
                let mut line = String::new();
                for (index, value) in args.into_iter().enumerate() {
                    let text = stringify.call::<mlua::LuaString>(value)?;
                    if index > 0 {
                        line.push('\t');
                    }
                    // Check the Lua string length before copying it into Rust.
                    if line.len() + text.as_bytes().len() > 32 << 10 {
                        logger.borrow_mut().dropped_logs += 1;
                        return Ok(());
                    }
                    line.push_str(&text.to_string_lossy());
                }
                let mut s = logger.borrow_mut();
                if s.log_bytes + line.len() < 32 << 10 {
                    s.log_bytes += line.len() + 1;
                    s.logs.push(line);
                } else {
                    s.dropped_logs += 1;
                }
                Ok(())
            })?,
        )?;
        let check_fuel = fuel.clone();
        let check_line = fuel_line.clone();
        let check_failure = budget_failure.clone();
        globals.set(
            "_check",
            lua.create_function(move |_, ()| {
                if check_fuel.get() == 0 {
                    Err(check_failure.borrow().clone().unwrap_or_else(|| {
                        mlua::Error::runtime(format!(
                            "UVI instruction budget exceeded at Lua line {:?}", check_line.get(),
                        ))
                    }))
                } else {
                    Ok(())
                }
            })?,
        )?;
        let host = state.clone();
        globals.set(
            "_post",
            lua.create_function(move |lua, (e, delta): (Table, f64)| {
                let snapshot = event_snapshot(lua, &e)?;
                let mut state = host.borrow_mut();
                let id = state.post(lua, &snapshot, delta)?;
                if let Some(id) = id {
                    e.set("id", voice_handle(lua, id)?)?;
                    let events = posted_events(lua, state.current_processor)?;
                    if snapshot.get::<u32>("type")? == 144 {
                        let posted = event_snapshot(lua, &snapshot)?;
                        // Internal registrations must not pin their own opaque handle.
                        // A reachable handle, physical key or parent dependency keeps
                        // this registration alive; release reconstructs event.id.
                        posted.raw_set("id", Value::Nil)?;
                        if let Some(duration) = snapshot.raw_get::<Option<f64>>("_duration")? {
                            let delay = frames_at_rate(delta, state.sample_rate)?;
                            let duration = frames_at_rate(duration, state.sample_rate)?;
                            let expires = state
                                .now
                                .checked_add(delay)
                                .and_then(|at| at.checked_add(duration))
                                .ok_or_else(|| mlua::Error::runtime("UVI timeline overflow"))?;
                            posted.raw_set("_expires", expires)?;
                        }
                        if state.root_activation.is_some() {
                            let key = (state.current_processor, id);
                            if !state.posted_roots.contains_key(&key)
                                && state.posted_roots.len() >= LIMIT
                            {
                                return Err(mlua::Error::runtime(
                                    "UVI posted root metadata limit exceeded",
                                ));
                            }
                            let root = state.live_root(state.current_root);
                            state.posted_roots.insert(key, root);
                        }
                        events.raw_set(id, posted)?;
                    } else {
                        let key = (state.current_processor, id);
                        state.posted_roots.remove(&key);
                        events.raw_set(id, Value::Nil)?;
                    }
                }
                id.map(|id| voice_handle(lua, id)).transpose()
            })?,
        )?;
        let mint = state.clone();
        globals.set(
            "__nextVoiceId",
            lua.create_function(move |lua, args: MultiValue| {
                if !args.is_empty() {
                    return Err(mlua::Error::runtime("UVI __nextVoiceId takes no arguments"));
                }
                voice_handle(lua, mint.borrow_mut().id()?)
            })?,
        )?;
        let host = state.clone();
        globals.set(
            "_release",
            lua.create_function(move |lua, id: Value| {
                host.borrow_mut().release(lua, voice_id(id)?)
            })?,
        )?;
        let host = state.clone();
        globals.set(
            "_spawn",
            lua.create_function(move |lua, mut args: MultiValue| {
                let Some(Value::Function(f)) = args.pop_front() else {
                    return Err(mlua::Error::runtime("spawn requires a function"));
                };
                let mut state = host.borrow_mut();
                let now = state.now;
                let processor = state.current_processor;
                let layer = state.current_layer;
                let root = state.live_root(state.current_root);
                state.schedule(
                    now,
                    Task {
                        thread: lua.create_thread(f)?,
                        args,
                        parent: None,
                        root,
                        processor,
                        layer,
                    },
                )
            })?,
        )?;
        let run_state = state.clone();
        let run_fuel = fuel.clone();
        let run_resumes = resumes.clone();
        let run_depth = depth.clone();
        globals.set(
            "_run",
            lua.create_function(move |lua, mut args: MultiValue| {
                let Some(Value::Function(f)) = args.pop_front() else {
                    return Err(mlua::Error::runtime("run requires a function"));
                };
                let processor = run_state.borrow().current_processor;
                let layer = run_state.borrow().current_layer;
                let root = {
                    let state = run_state.borrow();
                    state.live_root(state.current_root)
                };
                resume_task(
                    Task {
                        thread: lua.create_thread(f)?,
                        args,
                        parent: None,
                        root,
                        processor,
                        layer,
                    },
                    &run_state,
                    &run_fuel,
                    &run_resumes,
                    &run_depth,
                )
            })?,
        )?;
        let host = state.clone();
        globals.set(
            "getTime",
            lua.create_function(move |_, ()| {
                let state = host.borrow();
                Ok(state.now as f64 * 1000. / state.sample_rate as f64)
            })?,
        )?;
        let clock = state.clone();
        globals.set(
            "getTempo",
            lua.create_function(move |_, ()| Ok(clock.borrow().tempo))?,
        )?;
        globals.set(
            "getSamplingRate",
            lua.create_function(move |_, ()| Ok(sample_rate))?,
        )?;
        let clock = state.clone();
        globals.set(
            "getBeatTime",
            lua.create_function(move |_, ()| Ok(clock.borrow().beat))?,
        )?;
        let clock = state.clone();
        globals.set(
            "getRunningBeatTime",
            lua.create_function(move |_, ()| Ok(clock.borrow().running_beat))?,
        )?;
        let host = state.clone();
        globals.set(
            "isNoteHeld",
            lua.create_function(move |_, ()| {
                let s = host.borrow();
                Ok(s.current.is_some_and(|id| s.note_held(id)))
            })?,
        )?;
        let host = state.clone();
        globals.set(
            "isKeyDown",
            lua.create_function(move |_, note: u8| {
                let state = host.borrow();
                Ok(state
                    .keys
                    .iter()
                    .any(|((_, key), ids)| *key == note && !ids.is_empty())
                    || state.voices.values().any(|voice| {
                        voice.note.note == note
                            && !voice.released
                            && !voice.canceled
                            && voice.start <= state.now
                            && voice.end.is_none_or(|end| state.now < end)
                            && voice.kill_at.is_none_or(|end| state.now < end)
                    }))
            })?,
        )?;
        let host = state.clone();
        globals.set(
            "getCC",
            lua.create_function(move |_, cc: u8| {
                Ok(host.borrow().ccs.get(&cc).copied().unwrap_or(0))
            })?,
        )?;
        globals.set(
            "Event",
            lua.create_table_from([
                ("NoteOn", 144),
                ("NoteOff", 128),
                ("ControlChange", 176),
                ("PitchBend", 224),
                ("AfterTouch", 208),
                ("PolyAfterTouch", 160),
            ])?,
        )?;
        let host = state.clone();
        globals.set(
            "setSampleOffset",
            lua.create_function(move |_, (id, ms): (Value, f64)| {
                let id = voice_id(id)?;
                let mut s = host.borrow_mut();
                frames_at_rate(ms, s.sample_rate)?;
                // Offsets are source time, independent of output rate and pitch.
                let offset_us = (ms * 1000.).round() as u64;
                let now = s.now;
                if id == 0 || id > s.next_id {
                    return Err(mlua::Error::runtime("Unknown UVI voice"));
                }
                // Only same-frame pending launches below receive this change.
                // A later delayed post sharing the ID does not mask an earlier
                // immediate post; active and future-only setters remain no-ops.
                for command in &mut s.commands {
                    if let Action::Start(note) = &mut command.action
                        && note.id == id
                        && command.frame == now
                    {
                        note.offset_us = offset_us;
                    }
                }
                let processor = s.current_processor;
                for ((frame, _), forward) in &mut s.forwards {
                    if *frame == now
                        && forward.after == processor
                        && let Action::Start(note) = &mut forward.action
                        && note.id == id
                    {
                        note.offset_us = offset_us;
                    }
                }
                Ok(())
            })?,
        )?;
        for (name, field) in [("changeVolume", 0), ("changeTune", 1), ("changePan", 2)] {
            let host = state.clone();
            globals.set(name, lua.create_function(move |_, (id, value, relative, immediate): (Value, f64, Option<bool>, Option<bool>)| {
                let id = voice_id(id)?;
                if !value.is_finite() || immediate != Some(true) { return Err(mlua::Error::runtime("Nonfinite change or unsupported smoothed UVI voice change (use immediate=true)")); }
                let mut s = host.borrow_mut();
                let Some(_) = s.voices.get(&id) else {
                    if field == 1 && id > 0 && id <= s.next_id { return Ok(()); }
                    return Err(mlua::Error::runtime("Unknown UVI voice"));
                };
                let now=s.now;let layer=s.current_layer;
                if !s.voices[&id].starts.get(&layer).is_some_and(|at| *at<=now) { return Ok(()); }
                let (mut gain, mut tune, mut pan) = (None, None, None);
                match field {
                    0 => { gain = Some(value as f32); },
                    1 => { tune = Some(value); },
                    _ => { pan = Some(value as f32); },
                }
                if gain.is_some_and(|n|!n.is_finite()||n<0.) || pan.is_some_and(|n|!(-1. ..=1.).contains(&n)) || tune.is_some_and(|n|n.abs()>120.) { return Err(mlua::Error::runtime("UVI voice change exceeds playback range")); }
                s.emit(now, Action::Change { id, gain, tune, pan, relative:relative.unwrap_or(false), layer })
            })?)?;
        }
        let fades = state.clone();
        globals.set(
            "fadeout",
            lua.create_function(
                move |_,
                      (id, ms, kill, reset, layer): (
                    Value,
                    f64,
                    Option<bool>,
                    Option<bool>,
                    Option<Value>,
                )| {
                    let id = voice_id(id)?;
                    fades.borrow_mut().fade(
                        id,
                        reset.unwrap_or(false).then_some(1.),
                        0.,
                        ms,
                        kill.unwrap_or(false),
                        layer.unwrap_or(Value::Nil),
                    )
                },
            )?,
        )?;
        let fades = state.clone();
        globals.set(
            "fadein",
            lua.create_function(
                move |_, (id, ms, reset, layer): (Value, f64, Option<bool>, Option<Value>)| {
                    let id = voice_id(id)?;
                    fades.borrow_mut().fade(
                        id,
                        reset.unwrap_or(false).then_some(0.),
                        1.,
                        ms,
                        false,
                        layer.unwrap_or(Value::Nil),
                    )
                },
            )?,
        )?;
        let fades = state.clone();
        globals.set(
            "fade",
            lua.create_function(
                move |_, (id, target, ms, layer): (Value, f64, f64, Option<Value>)| {
                    let id = voice_id(id)?;
                    fades.borrow_mut().fade(
                        id,
                        None,
                        target,
                        ms,
                        false,
                        layer.unwrap_or(Value::Nil),
                    )
                },
            )?,
        )?;
        let fades = state.clone();
        globals.set(
            "fade2",
            lua.create_function(
                move |_, (id, start, target, ms, layer): (Value, f64, f64, f64, Option<Value>)| {
                    let id = voice_id(id)?;
                    fades.borrow_mut().fade(
                        id,
                        Some(start),
                        target,
                        ms,
                        false,
                        layer.unwrap_or(Value::Nil),
                    )
                },
            )?,
        )?;
        lua.load(BOOTSTRAP).set_name("kontra-uvi-host").exec()?;
        let object_host = if program.is_some() {
            let clock = state.clone();
            let voices = state.clone();
            let scope = state.clone();
            Some(host::install(
                &lua,
                host::HostConfig {
                    program,
                    modules,
                    resources,
                    now: Rc::new(move || clock.borrow().now),
                    valid_voice: Some(Rc::new(move |id| id > 0 && id <= voices.borrow().next_id)),
                    layer_scope: Some(Rc::new(move || scope.borrow().current_layer)),
                },
            )?)
        } else {
            None
        };
        if let Some(host) = &object_host {
            state.borrow_mut().module_sources = Some(host.modules.clone());
        }
        Ok(Self {
            lua,
            state,
            fuel,
            fuel_line,
            budget_failure,
            resumes,
            depth,
            host: object_host,
            environments: BTreeMap::new(),
            gc_drains: 0,
        })
    }

    fn initialize(&mut self, source: &str, name: &str, program: Option<&Program>) -> Result<()> {
        ensure!(source.len() <= 2 << 20, "UVI Lua exceeds 2 MiB limit");
        ensure!(
            !source.as_bytes().starts_with(b"\x1bLua"),
            "Only UVI Lua source text is accepted"
        );
        self.lua
            .load(source)
            .set_name(name)
            .exec()
            .map_err(|cause| super::lua_failure::initialization(cause, None, self.state.borrow().now, name))
            .context("UVI Lua initialization")?;
        ensure!(self.fuel.get() > 0, "UVI instruction budget exceeded");
        let rt = self;
        // Native UVI first-load observation: onLoad sees constructor values,
        // persisted widget changes run next, then onInit sees restored values.
        if let Some(program) = program {
            let states: Vec<_> = program
                .nodes
                .iter()
                .filter(|n| {
                    n.kind == "state"
                        && n.parent
                            .is_some_and(|p| program.nodes[p].kind == "ScriptProcessor")
                })
                .collect();
            ensure!(
                states.len() <= 1,
                "Multiple UVI saved script states require explicit processor selection"
            );
            if let Some(state) = states.first() {
                let saved: serde_json::Value =
                    serde_json::from_str(&state.text).context("Invalid UVI saved JSON state")?;
                ensure!(
                    saved.is_object() || saved.is_array() || saved.is_null(),
                    "UVI saved script state must be a table or nil"
                );
                let mut count = 0;
                let value = saved_value(&rt.lua, &saved, 0, &mut count)?;
                if let Some(f) = rt.lua.globals().get::<Option<Function>>("onLoad")? {
                    rt.spawn(f, MultiValue::from_vec(vec![value]), None)?;
                    rt.advance(0)?;
                }
            }
            rt.reset_budget();
            host::restore_widgets(&rt.lua, program)?;
            ensure!(rt.fuel.get() > 0, "UVI instruction budget exceeded");
            rt.advance(0)?;
        }
        if let Some(f) = rt.lua.globals().get::<Option<Function>>("onInit")? {
            rt.spawn(f, MultiValue::new(), None)?;
        }
        rt.advance(0)?;
        Ok(())
    }

    fn spawn(&mut self, f: Function, args: MultiValue, parent: Option<u64>) -> Result<()> {
        let thread = self.lua.create_thread(f)?;
        let mut state = self.state.borrow_mut();
        let now = state.now;
        let processor = state.current_processor;
        let layer = state.current_layer;
        let root = state.live_root(state.current_root);
        state.schedule(
            now,
            Task {
                thread,
                args,
                parent,
                root,
                processor,
                layer,
            },
        )?;
        Ok(())
    }

    fn advance(&mut self, until: u64) -> Result<()> {
        ensure!(
            until >= self.state.borrow().now,
            "UVI timeline must advance monotonically"
        );
        loop {
            let (task, forward) = {
                let mut state = self.state.borrow_mut();
                let task_key = state.tasks.first_key_value().map(|(key, _)| *key);
                let forward_key = (!state.initializing_chain)
                    .then(|| state.forwards.first_key_value().map(|(key, _)| *key))
                    .flatten();
                let next = task_key.into_iter().chain(forward_key).min();
                if next.is_none_or(|key| key.0 > until) {
                    state.set_time(until);
                    break;
                }
                let next = next.unwrap();
                state.set_time(next.0);
                if forward_key == Some(next) {
                    (None, Some(state.forwards.pop_first().unwrap().1))
                } else {
                    (Some(state.tasks.pop_first().unwrap().1), None)
                }
            };
            self.reset_budget();
            if let Some(task) = task {
                let processor = task.processor;
                let frame = self.state.borrow().now;
                resume_task(task, &self.state, &self.fuel, &self.resumes, &self.depth)
                    .with_context(|| match processor {
                        Some(processor) => format!(
                            "UVI ScriptProcessor node {processor} callback at frame {frame}"
                        ),
                        None => format!("UVI musical callback at frame {frame}"),
                    })?;
            }
            if let Some(forward) = forward {
                let event = event_snapshot(&self.lua, &forward.event)?;
                event.raw_set("_duration", Value::Nil)?;
                event.raw_set("_follows", Value::Nil)?;
                let parent = match &forward.action {
                    Action::Start(note) => {
                        event.set("id", voice_handle(&self.lua, note.id)?)?;
                        event.set("pan", note.pan)?;
                        event.set("vol", note.volume)?;
                        event.set("tune", note.tune)?;
                        event.raw_set("_offset_us", note.offset_us)?;
                        None
                    }
                    Action::ReleaseNote {
                        id, note, channel, ..
                    } => {
                        event.set("type", 128)?;
                        event.set("id", voice_handle(&self.lua, *id)?)?;
                        event.set("note", *note)?;
                        event.set("channel", *channel + 1)?;
                        None
                    }
                    Action::Controller {
                        channel,
                        controller,
                        value,
                    } => {
                        event.set("channel", *channel + 1)?;
                        event.set("controller", *controller)?;
                        event.set("value", *value)?;
                        None
                    }
                    Action::ControllerAll { controller, value } => {
                        event.set("channel", Value::Nil)?;
                        event.set("controller", *controller)?;
                        event.set("value", *value)?;
                        None
                    }
                    Action::PolyAfterTouch {
                        channel,
                        note,
                        value,
                    } => {
                        event.set("channel", *channel + 1)?;
                        event.set("note", *note)?;
                        event.set("value", *value)?;
                        None
                    }
                    Action::PolyAfterTouchAll { note, value } => {
                        event.set("channel", Value::Nil)?;
                        event.set("note", *note)?;
                        event.set("value", *value)?;
                        None
                    }
                    _ => None,
                };
                let selected = match &forward.action {
                    Action::Start(note) => note.layers.as_deref(),
                    _ => None,
                };
                self.deliver(
                    &event,
                    forward.after,
                    forward.layer,
                    parent,
                    selected,
                    forward.root,
                )?;
            }
        }
        let mut state = self.state.borrow_mut();
        state.current = None;
        state.current_root = None;
        state.current_processor = None;
        state.current_layer = None;
        Ok(())
    }

    // Only unreachable handles may retire voice metadata. A released audio tail can
    // still receive changes while a script retains its opaque handle.
    fn prune(&mut self) -> Result<()> {
        let now = self.state.borrow().now;
        if let Some(scopes) = self
            .lua
            .named_registry_value::<Option<Table>>("kontakto.uvi.posted_events")?
        {
            for scope in scopes.pairs::<u64, Table>() {
                let (scope, events) = scope?;
                let expired = events
                    .clone()
                    .pairs::<u32, Table>()
                    .filter_map(|entry| {
                        let (id, event) = match entry {
                            Ok(entry) => entry,
                            Err(error) => return Some(Err(error)),
                        };
                        match event.raw_get::<Option<u64>>("_expires") {
                            Ok(Some(end)) if end <= now => Some(Ok(id)),
                            Ok(_) => None,
                            Err(error) => Some(Err(error)),
                        }
                    })
                    .collect::<mlua::Result<Vec<_>>>()?;
                for id in expired {
                    events.raw_set(id, Value::Nil)?;
                    self.state.borrow_mut().posted_roots.remove(&(
                        if scope == 0 {
                            None
                        } else {
                            Some((scope - 1) as usize)
                        },
                        id,
                    ));
                }
            }
        }
        self.gc_drains += 1;
        let pressure = {
            let mut state = self.state.borrow_mut();
            state.keys.retain(|_, ids| !ids.is_empty());
            state.voices.len() >= LIMIT / 2 || state.triggers.len() >= LIMIT / 2
        };
        if pressure || self.gc_drains >= GC_MAX_DRAINS {
            self.lua.gc_collect()?;
        } else if !self.lua.gc_step()? {
            return Ok(());
        }
        self.gc_drains = 0;
        // Inspect weak userdata only after a collection finishes: mlua values
        // temporarily root them, which could otherwise keep dead handles alive.
        // ponytail: native GC steps can include atomic work; this VM stays off RT.
        let mut retained = HashSet::new();
        if let Some(cache) = self
            .lua
            .named_registry_value::<Option<Table>>("kontakto.uvi.voice_ids")?
        {
            for entry in cache.pairs::<u32, Value>() {
                let (id, handle) = entry?;
                if !matches!(handle, Value::Nil) {
                    retained.insert(id);
                }
            }
        }
        let mut state = self.state.borrow_mut();
        retained.extend(state.keys.values().flatten().copied());
        for command in &state.commands {
            match &command.action {
                Action::Start(note) => {
                    retained.insert(note.id);
                }
                Action::Release(id)
                | Action::ReleaseNote { id, .. }
                | Action::Fade { id, .. }
                | Action::Change { id, .. } => {
                    retained.insert(*id);
                }
                _ => {}
            }
        }
        if let Some(host) = &self.host {
            for command in host.commands.borrow().iter() {
                if let host::Action::ScriptModulation {
                    voice: Some(id), ..
                } = command.action
                {
                    retained.insert(id);
                }
            }
        }
        let held = state
            .triggers
            .iter()
            .filter_map(|(&id, trigger)| trigger.held.then_some(id))
            .collect::<HashSet<_>>();
        state.voices.retain(|id, voice| {
            retained.contains(id) || voice.parent.is_some_and(|parent| held.contains(&parent))
        });
        retained.extend(state.voices.keys().copied());
        if let Some(scopes) = self
            .lua
            .named_registry_value::<Option<Table>>("kontakto.uvi.posted_events")?
        {
            for scope in scopes.pairs::<u64, Table>() {
                let (scope, events) = scope?;
                let retired = events
                    .clone()
                    .pairs::<u32, Table>()
                    .map(|entry| entry.map(|(id, _)| id))
                    .collect::<mlua::Result<Vec<_>>>()?;
                for id in retired.into_iter().filter(|id| !retained.contains(id)) {
                    events.raw_set(id, Value::Nil)?;
                    state.posted_roots.remove(&(
                        if scope == 0 {
                            None
                        } else {
                            Some((scope - 1) as usize)
                        },
                        id,
                    ));
                }
            }
        }
        let mut parents = state
            .voices
            .values()
            .filter_map(|v| v.parent)
            .collect::<HashSet<_>>();
        parents.extend(state.tasks.values().filter_map(|task| task.parent));
        if let Some(parent) = state.current {
            parents.insert(parent);
        }
        // A finished newer callback still shadows an older held callback. Keep
        // the whole registration stack while its ID or any parent is reachable.
        state.trigger_queues.retain(|(_, id, _), queue| {
            !queue.is_empty()
                && (retained.contains(id) || queue.iter().any(|id| parents.contains(id)))
        });
        parents.extend(state.trigger_queues.values().flatten().copied());
        state.triggers.retain(|id, _| parents.contains(id));
        state.trigger_roots.retain(|id, _| parents.contains(id));
        Ok(())
    }

    fn input(&mut self, input: Input) -> Result<()> {
        self.input_rooted(input, None)
    }
    fn input_rooted(&mut self, input: Input, mut root: Option<HostRoot>) -> Result<()> {
        validate_input(&input)?;
        self.advance(input.frame)?;
        if let InputKind::Transport {
            playing,
            beat,
            tempo,
        } = input.kind
        {
            ensure!(
                beat.is_finite() && tempo.is_finite() && tempo > 0.,
                "Invalid UVI transport input"
            );
            let mut s = self.state.borrow_mut();
            let playing_changed = s.playing != playing;
            s.playing = playing;
            s.beat = beat;
            s.tempo = tempo;
            s.emit(
                input.frame,
                Action::Transport {
                    playing,
                    beat,
                    tempo,
                },
            )?;
            drop(s);
            if !playing_changed {
                return self.advance(input.frame);
            }
            if self.state.borrow().chain.is_some() {
                let processors = self.environments.keys().copied().collect::<Vec<_>>();
                for processor in processors {
                    if self.processor_enabled(processor)
                        && let Some(f) =
                            self.environments[&processor].get::<Option<Function>>("onTransport")?
                    {
                        self.scope(processor);
                        self.spawn(f, MultiValue::from_vec(vec![Value::Boolean(playing)]), None)?;
                    }
                }
            } else if let Some(f) = self.lua.globals().get::<Option<Function>>("onTransport")? {
                self.spawn(f, MultiValue::from_vec(vec![Value::Boolean(playing)]), None)?;
            }
            return self.advance(input.frame);
        }
        let event = self.lua.create_table()?;
        let (callback, parent) = {
            let mut s = self.state.borrow_mut();
            match input.kind {
                InputKind::NoteOn {
                    channel,
                    note,
                    velocity,
                } => {
                    ensure!(
                        channel < 16 && note < 128 && (1..=127).contains(&velocity),
                        "Invalid UVI MIDI input"
                    );
                    ensure!(
                        s.input_velocities.len() < LIMIT,
                        "UVI held input limit exceeded"
                    );
                    let id = s.id()?;
                    if let Some(root) = root {
                        s.last_root = root.token;
                        s.roots.insert(
                            root,
                            RootInput {
                                id,
                                channel,
                                note,
                                held: true,
                                choked: false,
                                ended: None,
                            },
                        );
                    }
                    s.input_velocities.insert(id, velocity);
                    s.keys.entry((channel, note)).or_default().push_back(id);
                    event.set("type", 144)?;
                    event.set("id", voice_handle(&self.lua, id)?)?;
                    event.set("note", note)?;
                    event.set("velocity", velocity)?;
                    event.set("channel", channel + 1)?;
                    event.set("vol", 1.)?;
                    event.set("pan", 0.)?;
                    event.set("tune", 0.)?;
                    ("onNote", None)
                }
                InputKind::NoteOff { channel, note } => {
                    ensure!(channel < 16 && note < 128, "Invalid UVI MIDI input");
                    let id = if let Some(root) = root {
                        let selected = s.roots[&root].id;
                        let queue = s
                            .keys
                            .get_mut(&(channel, note))
                            .context("Missing hosted key queue")?;
                        let index = queue
                            .iter()
                            .position(|id| *id == selected)
                            .context("Missing hosted key")?;
                        queue.remove(index).unwrap()
                    } else {
                        let Some(id) = s
                            .keys
                            .get_mut(&(channel, note))
                            .and_then(VecDeque::pop_front)
                        else {
                            return Ok(());
                        };
                        id
                    };
                    if let Some((token, owner)) =
                        s.roots.iter_mut().find(|(_, owner)| owner.id == id)
                    {
                        owner.held = false;
                        root = Some(*token);
                    }
                    event.set("type", 128)?;
                    event.set("id", voice_handle(&self.lua, id)?)?;
                    event.set("note", note)?;
                    event.set(
                        "velocity",
                        s.input_velocities
                            .remove(&id)
                            .context("Missing UVI input velocity")?,
                    )?;
                    event.set("channel", channel + 1)?;
                    event.set("vol", 1.)?;
                    event.set("pan", 0.)?;
                    event.set("tune", 0.)?;
                    ("onRelease", None)
                }
                InputKind::PitchBend { channel, bend } => {
                    ensure!(
                        channel < 16 && bend.is_finite() && (-1. ..=1.).contains(&bend),
                        "Invalid UVI pitch bend input"
                    );
                    event.set("type", 224)?;
                    event.set("channel", channel + 1)?;
                    event.set("bend", bend)?;
                    ("onPitchBend", None)
                }
                InputKind::AfterTouch { channel, value } => {
                    ensure!(channel < 16 && value < 128, "Invalid UVI aftertouch input");
                    event.set("type", 208)?;
                    event.set("channel", channel + 1)?;
                    event.set("value", value)?;
                    ("onAfterTouch", None)
                }
                InputKind::PolyAfterTouch {
                    channel,
                    note,
                    value,
                } => {
                    ensure!(
                        channel < 16 && note < 128 && value < 128,
                        "Invalid UVI polyphonic aftertouch input"
                    );
                    event.set("type", 160)?;
                    event.set("channel", channel + 1)?;
                    event.set("note", note)?;
                    event.set("value", value)?;
                    ("onPolyAfterTouch", None)
                }
                InputKind::Transport { .. } => unreachable!(),
                InputKind::Controller {
                    channel,
                    controller,
                    value,
                } => {
                    ensure!(
                        channel < 16 && controller < 128 && value < 128,
                        "Invalid UVI MIDI input"
                    );
                    s.ccs.insert(controller, value);
                    event.set("type", 176)?;
                    event.set("channel", channel + 1)?;
                    event.set("controller", controller)?;
                    event.set("value", value)?;
                    ("onController", None)
                }
            }
        };
        if self.state.borrow().chain.is_some() {
            self.deliver(&event, None, None, parent, None, root)?;
            return self.advance(input.frame);
        }
        let parent = self
            .state
            .borrow_mut()
            .receive(&self.lua, &event, None, root)?;
        let globals = self.lua.globals();
        let handler = match globals.get::<Option<Function>>("onEvent")? {
            Some(handler) => Some(handler),
            None => globals.get::<Option<Function>>(callback)?,
        };
        if let Some(f) = handler {
            self.spawn(f, MultiValue::from_vec(vec![Value::Table(event)]), parent)?;
        } else {
            self.state.borrow_mut().post(&self.lua, &event, 0.)?;
        }
        self.advance(input.frame)
    }
}

#[derive(Debug, Serialize)]
pub struct Processed {
    pub commands: Vec<Command>,
    #[serde(skip)]
    pub command_roots: Vec<Option<HostRoot>>,
    pub host_commands: Vec<host::Command>,
    /// Local diagnostics may contain proprietary/private script data.
    #[serde(skip)]
    pub logs: Vec<String>,
    pub dropped_logs: usize,
}

/// Admission failures are nonfatal to playback. Execution failures can leave
/// state changed and require the owning Player to stop. Underlying diagnostics
/// can contain private script data; Debug/Display expose only the error class.
pub enum UiEditError {
    Rejected(anyhow::Error),
    Execution(anyhow::Error),
}

impl std::fmt::Debug for UiEditError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Rejected(_) => "UiEditError::Rejected",
            Self::Execution(_) => "UiEditError::Execution",
        })
    }
}
impl std::fmt::Display for UiEditError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Rejected(_) => "UVI UI edit rejected",
            Self::Execution(_) => "UVI UI edit execution failed",
        })
    }
}
impl std::error::Error for UiEditError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(match self {
            Self::Rejected(error) | Self::Execution(error) => error.as_ref(),
        })
    }
}

/// Persistent allocating Lua session, owned by a control/worker thread.
/// Inputs and advances use absolute frames at the declared sample rate. Drain
/// after each chunk to reset output/log/resume budgets and retire unreachable IDs.
/// Never call this VM from a plugin audio callback.
pub struct Session {
    runtime: Runtime,
    program_fingerprint: [u8; 32],
}

impl Session {
    pub fn new_program_chain(
        program: &Program,
        modules: BTreeMap<String, Vec<u8>>,
        resources: Option<host::Resources>,
        sample_rate: u32,
    ) -> Result<Self> {
        Self::new_program_chain_with_state(program, modules, resources, sample_rate, None)
    }

    pub fn new_program_chain_with_state(
        program: &Program,
        modules: BTreeMap<String, Vec<u8>>,
        resources: Option<host::Resources>,
        sample_rate: u32,
        saved: Option<&super::state::SavedState>,
    ) -> Result<Self> {
        Ok(Self {
            runtime: Runtime::new_chain(program, modules, resources, sample_rate, saved)?,
            program_fingerprint: super::state::fingerprint(program)?,
        })
    }

    pub fn new_hosted_program_chain(
        program: &Program,
        modules: BTreeMap<String, Vec<u8>>,
        resources: Option<host::Resources>,
        sample_rate: u32,
        epoch: u64,
        generation: u64,
    ) -> Result<Self> {
        Self::new_hosted_program_chain_with_state(
            program,
            modules,
            resources,
            sample_rate,
            epoch,
            generation,
            None,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn new_hosted_program_chain_with_state(
        program: &Program,
        modules: BTreeMap<String, Vec<u8>>,
        resources: Option<host::Resources>,
        sample_rate: u32,
        epoch: u64,
        generation: u64,
        saved: Option<&super::state::SavedState>,
    ) -> Result<Self> {
        ensure!(epoch > 0 && generation > 0, "Invalid hosted activation");
        let session =
            Self::new_program_chain_with_state(program, modules, resources, sample_rate, saved)?;
        {
            let mut state = session.runtime.state.borrow_mut();
            let initialized_commands = state.commands.len();
            debug_assert!(state.command_roots.is_empty());
            state.command_roots.resize(initialized_commands, None);
            state.root_activation = Some((epoch, generation));
        }
        Ok(session)
    }
    /// Read-only admission for an entire bounded packet, before callbacks or
    /// clock mutation. Lua cannot change this ledger; only host admission and
    /// post-render completion acknowledgement add or remove its roots.
    pub fn validate_hosted_inputs(&self, inputs: &[HostedInput]) -> Result<()> {
        ensure!(
            inputs.len() <= HOST_ROOT_CAPACITY,
            "Hosted input packet exceeds capacity"
        );
        let state = self.runtime.state.borrow();
        let mut admitted = HashSet::new();
        let mut count = state.roots.len();
        let mut last_token = state.last_root;
        let mut last_frame = state.now;
        for event in inputs {
            ensure!(event.frame() >= last_frame, "Hosted input precedes clock");
            last_frame = event.frame();
            let root = match event {
                HostedInput::On { root, .. }
                | HostedInput::Off { root, .. }
                | HostedInput::Choke { root, .. } => *root,
                HostedInput::Event(input) => {
                    validate_input(input)?;
                    ensure!(
                        !matches!(
                            input.kind,
                            InputKind::NoteOn { .. } | InputKind::NoteOff { .. }
                        ),
                        "Hosted notes require an owned On/Off message"
                    );
                    continue;
                }
            };
            ensure!(
                state.root_activation == Some((root.epoch, root.generation)) && root.token > 0,
                "Stale hosted activation"
            );
            match event {
                HostedInput::On { input, .. } => {
                    validate_input(input)?;
                    ensure!(
                        matches!(input.kind, InputKind::NoteOn { .. }),
                        "Invalid hosted note on"
                    );
                    ensure!(root.token > last_token, "Stale or duplicate hosted root");
                    ensure!(count < HOST_ROOT_CAPACITY, "Hosted root ledger full");
                    admitted.insert(root.token);
                    last_token = root.token;
                    count += 1;
                }
                HostedInput::Off { .. } => {
                    ensure!(
                        admitted.contains(&root.token)
                            || state
                                .roots
                                .get(&root)
                                .is_some_and(|owner| owner.ended.is_none()),
                        "Unknown or complete hosted root"
                    );
                }
                HostedInput::Choke { .. } => {
                    ensure!(root.token <= last_token, "Unknown hosted choke root");
                }
                HostedInput::Event(_) => unreachable!("non-note input was validated above"),
            }
        }
        Ok(())
    }
    pub fn host_note_on(&mut self, root: HostRoot, input: Input) -> Result<()> {
        validate_input(&input)?;
        ensure!(
            matches!(input.kind, InputKind::NoteOn { .. }) && input.frame >= self.current_frame(),
            "Invalid hosted note on"
        );
        {
            let state = self.runtime.state.borrow();
            ensure!(
                state.root_activation == Some((root.epoch, root.generation))
                    && root.token > state.last_root,
                "Stale or duplicate hosted root"
            );
            ensure!(
                state.roots.len() < HOST_ROOT_CAPACITY,
                "Hosted root ledger full"
            );
        }
        self.runtime.input_rooted(input, Some(root))
    }
    pub fn host_note_off(&mut self, root: HostRoot, frame: u64) -> Result<()> {
        ensure!(
            frame >= self.current_frame(),
            "Hosted note off precedes clock"
        );
        let owner = *self
            .runtime
            .state
            .borrow()
            .roots
            .get(&root)
            .context("Unknown hosted root")?;
        ensure!(owner.ended.is_none(), "Hosted root already complete");
        if !owner.held {
            return self.advance(frame);
        }
        self.runtime.input_rooted(
            Input {
                frame,
                kind: InputKind::NoteOff {
                    channel: owner.channel,
                    note: owner.note,
                },
            },
            Some(root),
        )
    }
    /// Cancel rooted work without onRelease. A historical root may already have
    /// transferred its completion to the worker queue; repeating its choke is
    /// then a no-op. The adapter supplies genuine previously admitted tokens.
    pub fn host_note_choke(&mut self, root: HostRoot, frame: u64) -> Result<()> {
        ensure!(frame >= self.current_frame(), "Hosted choke precedes clock");
        {
            let state = self.runtime.state.borrow();
            ensure!(
                state.root_activation == Some((root.epoch, root.generation))
                    && root.token > 0
                    && root.token <= state.last_root,
                "Unknown hosted choke root"
            );
        }
        self.advance(frame)?;
        let mut state = self.runtime.state.borrow_mut();
        let Some(owner) = state.roots.get(&root).copied() else {
            return Ok(());
        };
        if owner.ended.is_some() || owner.choked {
            return Ok(());
        }
        state.tasks.retain(|_, task| task.root != Some(root));
        state
            .forwards
            .retain(|_, forward| forward.root != Some(root));
        let commands = std::mem::take(&mut state.commands);
        let roots = std::mem::take(&mut state.command_roots);
        (state.commands, state.command_roots) = commands
            .into_iter()
            .zip(roots)
            .filter(|(command, ancestry)| command.frame < frame || *ancestry != Some(root))
            .unzip();
        if let Some(queue) = state.keys.get_mut(&(owner.channel, owner.note)) {
            queue.retain(|id| *id != owner.id);
        }
        state.input_velocities.remove(&owner.id);
        let triggers = state
            .trigger_roots
            .iter()
            .filter_map(|(id, ancestry)| (*ancestry == root).then_some(*id))
            .collect::<HashSet<_>>();
        for id in &triggers {
            if let Some(trigger) = state.triggers.get_mut(id) {
                trigger.held = false;
            }
        }
        for queue in state.trigger_queues.values_mut() {
            queue.retain(|trigger| !triggers.contains(trigger));
        }
        let registrations = state
            .posted_roots
            .iter()
            .filter_map(|(key, ancestry)| (*ancestry == Some(root)).then_some(*key))
            .collect::<Vec<_>>();
        for (processor, id) in registrations {
            posted_events(&self.runtime.lua, processor)?.raw_set(id, Value::Nil)?;
            state.posted_roots.remove(&(processor, id));
        }
        let previous = state.current_root;
        state.current_root = Some(root);
        let result = state.emit(frame, Action::ChokeRoot);
        state.current_root = previous;
        result?;
        let owner = state.roots.get_mut(&root).unwrap();
        owner.held = false;
        owner.choked = true;
        Ok(())
    }
    /// Called only after drain commands were rendered. Live instances include
    /// their sustain/envelope/per-voice processor lifetime, never shared FX.
    pub fn complete_host_roots(
        &mut self,
        rendered_until: u64,
        sounding: &[HostRoot],
    ) -> Result<Vec<HostCompletion>> {
        ensure!(
            sounding.len() <= HOST_ROOT_CAPACITY,
            "Hosted census exceeds voice capacity"
        );
        ensure!(
            rendered_until
                == self
                    .current_frame()
                    .checked_add(1)
                    .context("Root boundary overflow")?,
            "Hosted census clock mismatch"
        );
        let mut state = self.runtime.state.borrow_mut();
        let mut pinned = sounding.iter().copied().collect::<HashSet<_>>();
        pinned.extend(state.tasks.values().filter_map(|task| task.root));
        pinned.extend(state.forwards.values().filter_map(|forward| forward.root));
        pinned.extend(state.command_roots.iter().flatten().copied());
        for (root, owner) in &mut state.roots {
            if owner.ended.is_none() && !owner.held && !pinned.contains(root) {
                owner.ended = Some(rendered_until);
            }
        }
        let mut completed = state
            .roots
            .iter()
            .filter_map(|(root, owner)| {
                owner
                    .ended
                    .map(|frame| HostCompletion { root: *root, frame })
            })
            .collect::<Vec<_>>();
        completed.sort_by_key(|completion| (completion.frame, completion.root.token));
        Ok(completed)
    }
    /// Transfer source ownership only after a bounded completion queue accepted
    /// these tokens. Queue-full retry preserves both the ledger and end boundary.
    pub fn acknowledge_host_completions(&mut self, roots: &[HostRoot]) -> Result<()> {
        ensure!(
            roots.len() <= HOST_ROOT_CAPACITY,
            "Hosted completion acknowledgement exceeds capacity"
        );
        let mut state = self.runtime.state.borrow_mut();
        let mut unique = HashSet::new();
        ensure!(
            roots.iter().all(|root| unique.insert(*root)
                && state
                    .roots
                    .get(root)
                    .is_some_and(|owner| owner.ended.is_some())),
            "Duplicate, unknown or active hosted completion"
        );
        for root in roots {
            state.roots.remove(root);
        }
        Ok(())
    }
    pub fn sample_rate(&self) -> u32 {
        self.runtime.state.borrow().sample_rate
    }
    pub fn current_frame(&self) -> u64 {
        self.runtime.state.borrow().now
    }

    /// Read final original-node parameters without invoking Lua or draining
    /// commands. Constructor and save admission belong to the owning Player.
    pub(crate) fn validate_final_pan_laws(&self) -> Result<()> {
        super::playback::validate_final_pan_laws(
            self.runtime.host.as_ref().context("UVI Program host is unavailable")?,
        )
    }

    /// Allocating worker only. onSave may execute Lua; commands stay on the
    /// native frame boundary for the next normal Player drain/render.
    pub fn saved_state(&mut self, frame: u64) -> Result<super::state::SavedState> {
        self.runtime.advance(frame)?;
        let processors = self
            .runtime
            .environments
            .keys()
            .copied()
            .collect::<Vec<_>>();
        let mut documents = BTreeMap::new();
        let mut bytes = 0usize;
        let previous = {
            let mut state = self.runtime.state.borrow_mut();
            let previous = (
                state.current,
                state.current_root,
                state.current_processor,
                state.current_layer,
            );
            state.current = None;
            state.current_root = None;
            previous
        };
        let result = (|| {
            for processor in processors {
                self.runtime.scope(processor);
                self.runtime.reset_budget();
                let document = host::save_state(&self.runtime.environments[&processor])?;
                ensure!(
                    self.runtime.fuel.get() > 0,
                    "UVI instruction budget exceeded"
                );
                bytes = bytes
                    .checked_add(document.len())
                    .context("UVI state size overflow")?;
                ensure!(
                    bytes <= super::state::MAX_STATE_BYTES,
                    "UVI saved state exceeds 2 MiB"
                );
                documents.insert(processor, String::from_utf8(document)?);
            }
            super::state::SavedState::new(
                self.program_fingerprint,
                documents,
                self.runtime.host.as_ref().unwrap(),
            )
        })();
        let mut state = self.runtime.state.borrow_mut();
        (
            state.current,
            state.current_root,
            state.current_processor,
            state.current_layer,
        ) = previous;
        result
    }

    /// Copy one initialized processor's UI on the VM's owning thread.
    /// Reading does not run widget callbacks or advance the musical clock.
    /// The result owns its data and can be sent to a separate UI thread.
    pub fn ui_snapshot(&self, processor: NodeId) -> Result<host::UiSnapshot> {
        let environment = self
            .runtime
            .environments
            .get(&processor)
            .context("UVI script processor is not initialized")?;
        Ok(host::snapshot_ui(processor, environment)?)
    }

    /// Run one GUI edit on this session's owning control thread at an absolute
    /// frame. Initial admission precedes clock mutation; pending callbacks can
    /// change control state, so admission is repeated at the execution frame.
    /// Changed callbacks use the existing scoped scheduler and may yield. Drain
    /// outputs normally; no callbacks or Lua handles belong on the GUI thread.
    pub fn edit_ui(
        &mut self,
        edit: &host::UiEdit,
        frame: u64,
    ) -> std::result::Result<(), UiEditError> {
        if frame < self.current_frame() {
            return Err(UiEditError::Rejected(anyhow::anyhow!(
                "UVI timeline must advance monotonically"
            )));
        }
        let environment = self
            .runtime
            .environments
            .get(&edit.processor)
            .context("UVI script processor is not initialized")
            .map_err(UiEditError::Rejected)?
            .clone();
        host::prepare_ui_edit(&self.runtime.lua, &environment, edit)
            .map_err(|error| UiEditError::Rejected(error.into()))?;
        self.runtime
            .advance(frame)
            .map_err(UiEditError::Execution)?;
        let (setter, args) = host::prepare_ui_edit(&self.runtime.lua, &environment, edit)
            .map_err(|error| UiEditError::Rejected(error.into()))?;
        self.runtime.scope(edit.processor);
        let scheduled = self.runtime.spawn(setter, args, None);
        {
            let mut state = self.runtime.state.borrow_mut();
            state.current_processor = None;
            state.current_layer = None;
        }
        scheduled.map_err(UiEditError::Execution)?;
        self.runtime.advance(frame).map_err(UiEditError::Execution)
    }

    pub fn input(&mut self, input: Input) -> Result<()> {
        self.runtime.input(input)
    }

    pub fn advance(&mut self, until: u64) -> Result<()> {
        self.runtime.advance(until)
    }

    /// Validate the complete chunk before running any callbacks.
    pub fn process(&mut self, inputs: &[Input], until: u64) -> Result<Processed> {
        validate_sequence(inputs, until)?;
        ensure!(
            until >= self.current_frame()
                && inputs
                    .first()
                    .is_none_or(|input| input.frame >= self.current_frame()),
            "UVI timeline must advance monotonically"
        );
        for &input in inputs {
            self.input(input)?;
        }
        self.advance(until)?;
        self.drain()
    }

    /// Return commands through the current frame, preserving all future work.
    pub fn drain(&mut self) -> Result<Processed> {
        self.runtime.prune()?;
        let mut state = self.runtime.state.borrow_mut();
        let until = state.now;
        let commands = std::mem::take(&mut state.commands);
        let (commands, command_roots) = if state.root_activation.is_some() {
            let roots = std::mem::take(&mut state.command_roots);
            assert_eq!(commands.len(), roots.len());
            let (mut ready, future): (Vec<_>, Vec<_>) = commands
                .into_iter()
                .zip(roots)
                .partition(|(command, _)| command.frame <= until);
            (state.commands, state.command_roots) = future.into_iter().unzip();
            ready.retain(|(command,_)| !matches!(&command.action, Action::Start(note) if state.voices.get(&note.id).is_some_and(|v| v.canceled)));
            ready.sort_by_key(|(command, _)| command.frame);
            ready.into_iter().unzip()
        } else {
            debug_assert!(state.command_roots.is_empty());
            let (mut ready, future): (Vec<_>, Vec<_>) = commands
                .into_iter()
                .partition(|command| command.frame <= until);
            state.commands = future;
            ready.retain(|command| !matches!(&command.action, Action::Start(note) if state.voices.get(&note.id).is_some_and(|v| v.canceled)));
            ready.sort_by_key(|command| command.frame);
            (ready, Vec::new())
        };
        let mut host_commands = Vec::new();
        if let Some(host) = &self.runtime.host {
            let mut pending = host.commands.borrow_mut();
            let (ready, future): (Vec<_>, Vec<_>) = std::mem::take(&mut *pending)
                .into_iter()
                .partition(|command| command.frame <= until);
            *pending = future;
            host_commands = ready;
            host_commands.sort_by_key(|command| command.frame);
        }
        let logs = std::mem::take(&mut state.logs);
        state.log_bytes = 0;
        let dropped_logs = std::mem::take(&mut state.dropped_logs);
        self.runtime.resumes.set(0);
        Ok(Processed {
            commands,
            command_roots,
            host_commands,
            logs,
            dropped_logs,
        })
    }
}

pub fn process(source: &str, name: &str, inputs: &[Input], until: u64) -> Result<Vec<Command>> {
    Ok(process_inner(source, name, None, BTreeMap::new(), None, inputs, until)?.commands)
}

pub fn process_program(
    source: &str,
    name: &str,
    program: &Program,
    modules: BTreeMap<String, Vec<u8>>,
    inputs: &[Input],
    until: u64,
) -> Result<Processed> {
    process_program_with_resources(source, name, program, modules, None, inputs, until)
}

pub fn process_program_with_resources(
    source: &str,
    name: &str,
    program: &Program,
    modules: BTreeMap<String, Vec<u8>>,
    resources: Option<host::Resources>,
    inputs: &[Input],
    until: u64,
) -> Result<Processed> {
    process_inner(
        source,
        name,
        Some(program),
        modules,
        resources,
        inputs,
        until,
    )
}

/// Native EventProcessors in Program order, then each selected Layer's order.
/// Sources and globals remain isolated; graph identity, clock and limits are shared.
pub fn process_program_chain(
    program: &Program,
    modules: BTreeMap<String, Vec<u8>>,
    resources: Option<host::Resources>,
    inputs: &[Input],
    until: u64,
) -> Result<Processed> {
    process_program_chain_at_rate(program, modules, resources, inputs, until, RATE as u32)
}

pub fn process_program_chain_at_rate(
    program: &Program,
    modules: BTreeMap<String, Vec<u8>>,
    resources: Option<host::Resources>,
    inputs: &[Input],
    until: u64,
    sample_rate: u32,
) -> Result<Processed> {
    validate_inputs_at_rate(inputs, until, sample_rate)?;
    Session::new_program_chain(program, modules, resources, sample_rate)?.process(inputs, until)
}

fn process_inner(
    source: &str,
    name: &str,
    program: Option<&Program>,
    modules: BTreeMap<String, Vec<u8>>,
    resources: Option<host::Resources>,
    inputs: &[Input],
    until: u64,
) -> Result<Processed> {
    validate_inputs(inputs, until)?;
    collect(
        Runtime::new(source, name, program, modules, resources)?,
        inputs,
        until,
    )
}

fn validate_inputs(inputs: &[Input], until: u64) -> Result<()> {
    validate_inputs_at_rate(inputs, until, RATE as u32)
}

pub fn validate_inputs_at_rate(inputs: &[Input], until: u64, sample_rate: u32) -> Result<()> {
    validate_sample_rate(sample_rate)?;
    ensure!(
        until <= sample_rate as u64 * 60,
        "UVI offline timeline exceeds 60 seconds"
    );
    validate_sequence(inputs, until)
}

pub(crate) fn validate_sequence(inputs: &[Input], until: u64) -> Result<()> {
    ensure!(
        inputs.len() <= LIMIT && inputs.windows(2).all(|w| w[0].frame <= w[1].frame),
        "UVI inputs must be bounded and sorted"
    );
    ensure!(
        inputs.last().is_none_or(|i| i.frame <= until),
        "UVI input exceeds render end"
    );
    for input in inputs {
        validate_input(input)?;
    }
    Ok(())
}

/// Shared allocation-free ingress check for the VM and realtime worker port.
pub(crate) fn input_is_valid(input: &Input) -> bool {
    match input.kind {
        InputKind::NoteOn {
            channel,
            note,
            velocity,
        } => channel < 16 && note <= 127 && (1..=127).contains(&velocity),
        InputKind::NoteOff { channel, note } => channel < 16 && note <= 127,
        InputKind::Controller {
            channel,
            controller,
            value,
        } => channel < 16 && controller <= 127 && value <= 127,
        InputKind::PitchBend { channel, bend } => {
            channel < 16 && bend.is_finite() && (-1. ..=1.).contains(&bend)
        }
        InputKind::AfterTouch { channel, value } => channel < 16 && value <= 127,
        InputKind::PolyAfterTouch {
            channel,
            note,
            value,
        } => channel < 16 && note <= 127 && value <= 127,
        InputKind::Transport { beat, tempo, .. } => {
            beat.is_finite() && tempo.is_finite() && tempo > 0.
        }
    }
}

fn validate_input(input: &Input) -> Result<()> {
    ensure!(
        input_is_valid(input),
        "Invalid UVI MIDI/transport input at frame {}",
        input.frame
    );
    Ok(())
}

fn collect(mut rt: Runtime, inputs: &[Input], until: u64) -> Result<Processed> {
    for &input in inputs {
        rt.input(input)?;
    }
    rt.advance(until)?;
    let mut state = rt.state.borrow_mut();
    let mut commands = std::mem::take(&mut state.commands);
    commands.retain(|c| {
        c.frame <= until
            && match &c.action {
                Action::Start(note) => !state.voices.get(&note.id).is_some_and(|v| v.canceled),
                _ => true,
            }
    });
    commands.sort_by_key(|c| c.frame);
    let mut host_commands = rt
        .host
        .as_ref()
        .map_or_else(Vec::new, |h| std::mem::take(&mut *h.commands.borrow_mut()));
    host_commands.retain(|c| c.frame <= until);
    host_commands.sort_by_key(|c| c.frame);
    Ok(Processed {
        command_roots: Vec::new(),
        commands,
        host_commands,
        logs: std::mem::take(&mut state.logs),
        dropped_logs: state.dropped_logs,
    })
}

#[cfg(test)]
mod tests {
    fn callback_dispatch_fixture(scoped: bool, source: &str, inputs: &[Input], until: u64) -> Result<Vec<Command>> {
        if scoped {
            let program = parse_program(&format!(r#"<Program><EventProcessors><ScriptProcessor><script><![CDATA[{source}]]></script></ScriptProcessor></EventProcessors><Layers><Layer/></Layers></Program>"#))?;
            Ok(process_program_chain(&program, BTreeMap::new(), None, inputs, until)?.commands)
        } else {
            process(source, "authored-callback-selection", inputs, until)
        }
    }

    #[test]
    fn winning_on_event_does_not_convert_unused_specialized_callbacks() {
        let source = r#"
          onNote=17;onController={}
          function onEvent(e)
            if e.type==Event.NoteOn then e.note=e.note+1;wait(1) end
            postEvent(e)
          end
        "#;
        let inputs = [audio_on(0), Input { frame: 48, kind: InputKind::Controller {
            channel: 0, controller: 1, value: 70,
        }}];
        for scoped in [false, true] {
            let commands = callback_dispatch_fixture(scoped, source, &inputs, 48).unwrap();
            assert_eq!(commands.len(), 2);
            assert!(matches!(&commands[0], Command {frame:48, action:Action::Start(note)} if note.note==61));
            assert!(matches!(&commands[1], Command {frame:48, action:Action::Controller {channel:0, controller:1, value:70}}));
        }
    }

    #[test]
    fn selected_specialized_callback_type_errors_and_absent_forwarding_are_preserved() {
        let controller = Input { frame: 0, kind: InputKind::Controller {
            channel: 0, controller: 1, value: 70,
        }};
        for scoped in [false, true] {
            assert!(callback_dispatch_fixture(scoped, "onNote=17", &[audio_on(0)], 0).is_err());
            assert!(callback_dispatch_fixture(scoped, "onController={}", &[controller], 0).is_err());
            assert!(callback_dispatch_fixture(scoped, "onEvent=17;function onNote(e)postEvent(e)end", &[audio_on(0)], 0).is_err());
            let fallback = callback_dispatch_fixture(scoped,
                "function onNote(e)e.note=e.note+2;postEvent(e)end", &[audio_on(0)], 0).unwrap();
            assert_eq!(fallback.len(), 1);
            assert!(matches!(&fallback[0].action, Action::Start(note) if note.note==62));
            let automatic = callback_dispatch_fixture(scoped, "", &[audio_on(0), controller], 0).unwrap();
            assert_eq!(automatic.len(), 2);
            assert!(matches!(&automatic[0].action, Action::Start(note) if note.note==60));
            assert!(matches!(&automatic[1].action, Action::Controller {channel:0, controller:1, value:70}));
        }
    }

    #[test]
    fn final_pan_law_admission_preserves_transient_repairs_and_restore_on_init() {
        let program = super::super::program::parse_program(r#"<Program><EventProcessors><ScriptProcessor><script><![CDATA[
          function onInit()Program:setParameter('PanLaw',2);Program:setParameter('PanLaw',0)end
          function onLoad(data)assert(Program:getParameter('PanLaw')==2)end
          function onSave()Program:setParameter('PanLaw',2);return {}end
        ]]></script></ScriptProcessor></EventProcessors></Program>"#).unwrap();
        let mut session = Session::new_program_chain(&program, BTreeMap::new(), None, 48000).unwrap();
        session.validate_final_pan_laws().unwrap();
        let commands = session.drain().unwrap().host_commands;
        let laws = commands.iter().filter_map(|command| match &command.action {
            host::Action::Parameter { parameter, value: host::ParameterValue::Number(value), .. }
                if parameter == "PanLaw" => Some(*value),
            _ => None,
        }).collect::<Vec<_>>();
        assert_eq!(laws, vec![2., 0.], "final inspection does not drain or reorder temporary writes");
        let saved = session.saved_state(0).unwrap();
        assert!(session.validate_final_pan_laws().is_err(), "retained final law2 is not renderer-admissible");
        // Decode/type/fingerprint and restoration prefix still admit the payload;
        // authored onLoad sees2 and onInit repairs it before final admission.
        let saved = super::super::state::SavedState::decode(&saved.encode().unwrap()).unwrap();
        let restored = Session::new_program_chain_with_state(
            &program, BTreeMap::new(), None, 48000, Some(&saved),
        ).unwrap();
        restored.validate_final_pan_laws().unwrap();
    }

    #[test]
    fn final_pan_law_admission_checks_only_original_renderer_owner_kinds() {
        for target in ["Program", "Program.layers[1]", "Program.layers[1].keygroups[1]"] {
            let xml = format!(r#"<Program><Layers><Layer><Keygroups><Keygroup/></Keygroups></Layer></Layers><EventProcessors><ScriptProcessor><script>{target}:setParameter('PanLaw',2)</script></ScriptProcessor></EventProcessors></Program>"#);
            let program = super::super::program::parse_program(&xml).unwrap();
            let session = Session::new_program_chain(&program, BTreeMap::new(), None, 48000).unwrap();
            let kind = match target {
                "Program" => "Program",
                "Program.layers[1]" => "Layer",
                _ => "Keygroup",
            };
            let node = program.nodes.iter().position(|node| node.kind == kind).unwrap();
            let error = session.validate_final_pan_laws().unwrap_err();
            assert!(format!("{error:#}").contains(&format!("{kind} node {node} parameter PanLaw=2")),
                "retained2 on {target} identifies the original renderer owner");
        }
        let program = super::super::program::parse_program(r#"<Program PanLaw="1"><Layers><Layer PanLaw="0"><Keygroups><Keygroup PanLaw="1"><Oscillators><SamplePlayer SamplePath="authored.wav" PanLaw="2"/></Oscillators></Keygroup></Keygroups></Layer></Layers></Program>"#).unwrap();
        let session = Session::new_program_chain(&program, BTreeMap::new(), None, 48000).unwrap();
        session.validate_final_pan_laws().unwrap();
        let program = super::super::program::parse_program(r#"<Program PanLaw="private_pan_value_marker"/>"#).unwrap();
        let session = Session::new_program_chain(&program, BTreeMap::new(), None, 48000).unwrap();
        let error = format!("{:#}", session.validate_final_pan_laws().unwrap_err());
        assert!(error.contains(&format!("Program node {} parameter PanLaw", program.root)));
        assert!(!error.contains("private_pan_value_marker"), "numeric parse diagnostics do not publish retained text");
    }

    use super::super::program::parse_program;
    use super::*;

    fn audio_on(frame: u64) -> Input {
        Input {
            frame,
            kind: InputKind::NoteOn {
                channel: 0,
                note: 60,
                velocity: 100,
            },
        }
    }

    fn audio_root(token: u64) -> HostRoot {
        HostRoot {
            epoch: 7,
            generation: 9,
            token,
        }
    }

    #[test]
    fn generated_velocity_uses_native_signed_integer_clamp() {
        let velocities = "-1000,-1.9,0,.9,1.9,126.9,127.9,128,1000,-2147483649,-2147483648,2147483647,2147483648,4294967295,4294967296,4294967297,9007199254740991,9007199254740992,-9223372036854775808,9223372036854774784,9223372036854775808,0/0,math.huge,-math.huge";
        let expected = [0,0,0,0,1,126,127,127,127,127,0,127,0,0,0,1,0,0,0,0,0,0,0,0];
        for api in ["playNote(60,v,0)", "playNote{60,v,0}", "postEvent{type=Event.NoteOn,note=60,velocity=v}"] {
            let source = format!("function onInit() for _,v in ipairs({{{velocities}}}) do {api} end end");
            let out = process(&source, "authored-generated-velocity", &[], 0).unwrap();
            let actual: Vec<_> = out.iter().filter_map(|c| match &c.action {
                Action::Start(n) => Some(n.velocity),
                _ => None,
            }).collect();
            assert_eq!(actual, expected, "{api}");
        }
        for value in ["true", "'64'", "{}", "function()end"] {
            let source = format!("function onInit() playNote(60,{value},0) end");
            assert!(process(&source, "authored-invalid-velocity", &[], 0).is_err());
        }
    }

    fn audio_completion_program(source: &str) -> Program {
        parse_program(&format!("<Program Gain='1'><Layers><Layer Gain='1'><EventProcessors><ScriptProcessor><script><![CDATA[{source}]]></script></ScriptProcessor></EventProcessors><Keygroups><Keygroup Gain='1'><Oscillators><SamplePlayer Gain='1' SamplePath='old.wav' NoteTracking='0'/></Oscillators></Keygroup></Keygroups><Inserts><Convolver Dry='1' Wet='0'/></Inserts></Layer></Layers></Program>")).unwrap()
    }

    fn audio_completion_resources() -> host::Resources {
        Rc::new(|request| match request {
            host::ResourceRequest::ReadAudio { path, .. } if path == "new.wav" => {
                Ok(host::ResourceResponse::Audio(host::ResourceInfo {
                    name: path.clone(),
                    rate: 48_000,
                    channels: 1,
                    frames: 256,
                }))
            }
            _ => Err(mlua::Error::runtime("Authored missing audio")),
        })
    }

    #[test]
    fn audio_resource_completions_are_deferred_yieldable_and_include_failures() {
        for (api, target) in [
            (
                "loadSample",
                "Program.layers[1].keygroups[1].oscillators[1]",
            ),
            ("loadImpulse", "Program.layers[1].inserts[1]"),
        ] {
            for (path, success) in [("new.wav", true), ("missing.wav", false)] {
                let source = format!(
                    r#"
                    history=''
                    spawn=function()error('authored global must not replace completion scheduler')end
                    function onInit()
                        history='A'
                        local task={api}({target},'{path}',function(t)
                            assert(t.finished and t.state=='finished' and t.success=={success})
                            assert(not isNoteHeld() and this.parent.type=='Layer')
                            assert(history=='AB');history=history..'C'
                            wait(1);history=history..'D';print(history)
                            playNote(60,100,2)
                        end)
                        assert(history=='A' and task.success=={success})
                        assert(task.id==1 and task.progress==1)
                        if not task.success then assert(string.find(task.error,'Authored missing audio'))end
                        history=history..'B'
                    end
                "#
                );
                let program = audio_completion_program(&source);
                let mut session = Session::new_program_chain(
                    &program,
                    BTreeMap::new(),
                    Some(audio_completion_resources()),
                    48_000,
                )
                .unwrap();
                let initial = session.drain().unwrap();
                assert!(initial.commands.is_empty() && initial.logs.is_empty());
                assert_eq!(initial.host_commands.len(), usize::from(success));
                session.advance(47).unwrap();
                assert!(session.drain().unwrap().commands.is_empty());
                session.advance(48).unwrap();
                let done = session.drain().unwrap();
                assert_eq!(done.logs, ["ABCD"]);
                assert!(
                    matches!(&done.commands[0], Command { frame:48, action:Action::Start(note) } if note.layers==Some(vec![program.nodes.iter().position(|node|node.kind=="Layer").unwrap()]))
                );
            }
        }
    }

    #[test]
    fn audio_resource_failure_preserves_playable_pcm_and_existing_metadata() {
        use super::super::{playback::Renderer, sample::Sample, storage::Storage};
        use std::{collections::HashMap, sync::Arc};
        let old = Arc::new(Sample {
            rate: 48_000,
            channels: 1,
            frames: 256,
            interleaved: Storage::from_f32(vec![0.25; 256]).unwrap(),
            loops: vec![],
            unity_note: None,
            wavetable_cycle_frames: None,
            wavetable_image: false,
            riff_metadata: vec![],
        });
        let source = r#"
            function onNote(e)
                local oscillator=Program.layers[1].keygroups[1].oscillators[1]
                local before=oscillator.sampleInfo
                local failed=loadSample(oscillator,'missing.wav',function(t)
                    assert(not t.success and t.error);print('failed completion')
                end)
                assert(not failed.success and oscillator.sampleInfo==before)
                assert(oscillator:getParameter('SamplePath')=='old.wav')
                postEvent(e)
            end
        "#;
        let program = audio_completion_program(source);
        let mut session = Session::new_program_chain(
            &program,
            BTreeMap::new(),
            Some(audio_completion_resources()),
            48_000,
        )
        .unwrap();
        let result = session.process(&[audio_on(0)], 3).unwrap();
        assert_eq!(result.logs, ["failed completion"]);
        assert!(result.host_commands.is_empty());
        let mut renderer = Renderer::new(
            &program,
            HashMap::from([("old.wav".into(), old.clone())]),
            48_000,
        )
        .unwrap();
        let pcm = renderer
            .render(&result.commands, &result.host_commands, 4)
            .unwrap();
        assert!(pcm.iter().all(|frame| frame[0] > 0.));
        let control = audio_completion_program("function onNote(e)postEvent(e)end");
        let mut control_session =
            Session::new_program_chain(&control, BTreeMap::new(), None, 48_000).unwrap();
        let control_result = control_session.process(&[audio_on(0)], 3).unwrap();
        let mut control_renderer =
            Renderer::new(&control, HashMap::from([("old.wav".into(), old)]), 48_000).unwrap();
        assert_eq!(
            pcm,
            control_renderer
                .render(&control_result.commands, &[], 4)
                .unwrap()
        );
        println!("Authored failed-load PCM equals retained-source control: {pcm:?}");
    }

    #[test]
    fn audio_resource_success_swaps_future_voices_without_replacing_active_pcm() {
        use super::super::{playback::Renderer, sample::Sample, storage::Storage};
        use std::{collections::HashMap, sync::Arc};
        let sample = |value| {
            Arc::new(Sample {
                rate: 48_000,
                channels: 1,
                frames: 256,
                interleaved: Storage::from_f32(vec![value; 256]).unwrap(),
                loops: vec![],
                unity_note: None,
                wavetable_cycle_frames: None,
                wavetable_image: false,
                riff_metadata: vec![],
            })
        };
        let program = audio_completion_program(
            r#"
            function onNote(e)
                if e.note==61 then
                    local oscillator=Program.layers[1].keygroups[1].oscillators[1]
                    local task=loadSample(oscillator,'new.wav',function(t)
                        assert(t.success);print('completed after return')
                    end)
                    assert(task.success and oscillator.sampleInfo.name=='new.wav')
                    print('returned')
                end
                postEvent(e)
            end
        "#,
        );
        let mut session = Session::new_program_chain(
            &program,
            BTreeMap::new(),
            Some(audio_completion_resources()),
            48_000,
        )
        .unwrap();
        let mut second = audio_on(4);
        second.kind = InputKind::NoteOn {
            channel: 0,
            note: 61,
            velocity: 100,
        };
        let result = session.process(&[audio_on(0), second], 7).unwrap();
        assert_eq!(result.logs, ["returned", "completed after return"]);
        assert_eq!(result.host_commands.len(), 1);
        assert_eq!(result.host_commands[0].frame, 4);
        let mut renderer = Renderer::new(
            &program,
            HashMap::from([("old.wav".into(), sample(0.25))]),
            48_000,
        )
        .unwrap();
        renderer
            .install_prepared_samples(HashMap::from([("new.wav".into(), sample(0.5))]))
            .unwrap();
        let pcm = renderer
            .render(&result.commands, &result.host_commands, 8)
            .unwrap();
        assert!(pcm[0][0] > 0.);
        assert!(pcm[..4].iter().all(|frame| *frame == pcm[0]));
        assert!(
            pcm[4..]
                .iter()
                .all(|frame| *frame == [3. * pcm[0][0], 3. * pcm[0][1]])
        );
        println!(
            "Authored successful sample-swap PCM: {pcm:?}; callbacks: {:?}",
            result.logs
        );
    }

    #[test]
    fn audio_resource_failures_without_callbacks_reach_bounded_desktop_diagnostics() {
        let reason = format!("Authored audio journal marker {}", "é".repeat(1000));
        let full = reason.clone();
        let resources: host::Resources = Rc::new(move |_| Err(mlua::Error::runtime(full.clone())));
        let program = audio_completion_program(
            r#"
            print=function()error('authored override must not hide resource failures')end
            function onInit()
                local oscillator=Program.layers[1].keygroups[1].oscillators[1]
                local impulse=Program.layers[1].inserts[1]
                for _,task in ipairs{loadSample(oscillator,'missing.wav'),loadImpulse(impulse,'missing.wav')}do
                    assert(task.finished and not task.success and string.len(task.error)>2000)
                    assert(task.error=='runtime error: Authored audio journal marker '..string.rep('é',1000))
                end
            end
        "#,
        );
        let mut session =
            Session::new_program_chain(&program, BTreeMap::new(), Some(resources), 48_000).unwrap();
        let result = session.drain().unwrap();
        assert!(
            result.commands.is_empty() && result.host_commands.is_empty() && result.logs.is_empty()
        );
        let snapshot = crate::diagnostics::snapshot();
        let failures = snapshot
            .events
            .iter()
            .filter(|event| {
                event.code.as_deref() == Some("resource_task_failed")
                    && event.reason.as_deref().is_some_and(|text| {
                        text.starts_with("runtime error: Authored audio journal marker ")
                    })
            })
            .collect::<Vec<_>>();
        assert_eq!(failures.len(), 2);
        for (event, kind) in failures.into_iter().zip(["Sample", "Impulse"]) {
            assert_eq!(event.module, "uvi.host");
            assert_eq!(event.level, crate::diagnostics::LogLevel::Warning);
            assert_eq!(event.details["kind"], kind);
            assert_eq!(event.details["frame"], 0);
            assert!(event.details["node"].as_u64().is_some());
            assert_eq!(event.details["reason_truncated"], true);
            let bounded = event.reason.as_deref().unwrap();
            assert_eq!(bounded.chars().count(), 512);
            assert!(("runtime error: ".to_owned() + &reason).starts_with(bounded));
            assert_eq!(event.details.as_object().unwrap().len(), 5);
        }
    }

    #[test]
    fn audio_resource_completion_descendants_follow_host_root_choke() {
        let program = audio_completion_program(
            r#"
            function onNote(e)
                loadSample(Program.layers[1].keygroups[1].oscillators[1],'missing.wav',function(t)
                    assert(not t.success and not isNoteHeld());print('completion')
                    wait(1);playNote(62,100,1)
                end)
                postEvent(e)
            end
            function onRelease(e)postEvent(e)end
        "#,
        );
        let mut session = Session::new_hosted_program_chain(
            &program,
            BTreeMap::new(),
            Some(audio_completion_resources()),
            48_000,
            7,
            9,
        )
        .unwrap();
        session.host_note_on(audio_root(1), audio_on(0)).unwrap();
        assert_eq!(session.drain().unwrap().logs, ["completion"]);
        session.host_note_choke(audio_root(1), 1).unwrap();
        session.drain().unwrap();
        session.advance(48).unwrap();
        let out = session.drain().unwrap();
        assert!(out.commands.is_empty() && out.logs.is_empty());
        session.host_note_on(audio_root(2), audio_on(49)).unwrap();
        session.drain().unwrap();
        session.advance(97).unwrap();
        let out = session.drain().unwrap();
        assert!(
            matches!(&out.commands[0],Command {frame:97,action:Action::Start(note)} if note.note==62)
        );
        assert_eq!(out.command_roots[0], Some(audio_root(2)));
    }

    #[test]
    fn audio_resource_completion_errors_propagate_on_resume() {
        let program = audio_completion_program(
            r#"
            function onInit()
                loadSample(Program.layers[1].keygroups[1].oscillators[1],'new.wav',function(t)
                    assert(t.success);wait(1);error('authored completion failure')
                end)
            end
        "#,
        );
        let mut session = Session::new_program_chain(
            &program,
            BTreeMap::new(),
            Some(audio_completion_resources()),
            48_000,
        )
        .unwrap();
        assert_eq!(session.drain().unwrap().host_commands.len(), 1);
        let error = session.advance(48).unwrap_err();
        assert!(format!("{error:#}").contains("authored completion failure"));
    }

    fn session_program(source: &str) -> Program {
        parse_program(&format!("<Program Gain='0.5'><EventProcessors><ScriptProcessor><script><![CDATA[{source}]]></script></ScriptProcessor></EventProcessors><Layers><Layer/></Layers></Program>")).unwrap()
    }

    #[test]
    fn session_restores_parameter_widgets_beside_stateless_buttons_and_roundtrips_state() {
        let program=parse_program(r#"<Program><EventProcessors><ScriptProcessor click="1" panel="0.9" gain="0.75" enabled="1"><ScriptData curve="0,250000 0,500000"/><script><![CDATA[
          calls=0
          panel=Panel('panel');button=panel:Button('click')
          button.persistent=true
          button.changed=function()error('stateless Button must not be restored')end
          gain=panel:Knob{'gain',0,0,1}
          gain.changed=function(...)assert(select('#',...)==1);calls=calls+1 end
          curve=panel:Table{'curve',2,0,0,1}
          curve.changed=function(self,index)assert(index==1 or index==2);calls=calls+1 end
          enabled=panel:OnOffButton{'enabled',false}
          enabled.changed=function(self,mods)assert(type(mods)=='userdata');calls=calls+1 end
          function onSave()return {authored=true}end
          function onLoad(data)assert(data.authored)end
          function onInit()
            assert(button.value==nil and button.setValue==nil and calls==4)
            assert(gain.value==0.75 and enabled.value and curve:getValue(1)==0.25 and curve:getValue(2)==0.5)
            assert(saveState('authored-state.xml').success)
            gain:setValue(0.125,false);enabled:setValue(false,false);curve:setValue(2,0.125,false)
            assert(loadState('authored-state.xml').success)
            assert(calls==7 and gain.value==0.75 and enabled.value and curve:getValue(2)==0.5)
            assert(button.value==nil and button.setValue==nil)
          end
        ]]></script></ScriptProcessor></EventProcessors><Layers><Layer/></Layers></Program>"#).unwrap();
        let saved = Rc::new(RefCell::new(None::<Vec<u8>>));
        let storage = saved.clone();
        let resources = Some(Rc::new(
            move |request: &host::ResourceRequest| -> mlua::Result<host::ResourceResponse> {
                match request {
                    host::ResourceRequest::WriteState { path, bytes }
                        if path == "authored-state.xml" =>
                    {
                        *storage.borrow_mut() = Some(bytes.clone());
                        Ok(host::ResourceResponse::Saved)
                    }
                    host::ResourceRequest::ReadState { path } if path == "authored-state.xml" => {
                        Ok(host::ResourceResponse::Bytes(
                            storage
                                .borrow()
                                .clone()
                                .ok_or_else(|| mlua::Error::runtime("Authored state not saved"))?,
                        ))
                    }
                    _ => Err(mlua::Error::runtime("Unapproved authored capability")),
                }
            },
        ) as host::Resources);
        let mut session =
            Session::new_program_chain(&program, BTreeMap::new(), resources, 48_000).unwrap();
        let processor = program
            .nodes
            .iter()
            .position(|n| n.kind == "ScriptProcessor")
            .unwrap();
        let snapshot = session.ui_snapshot(processor).unwrap();
        assert!(
            snapshot.widgets[1].kind == host::UiKind::Button && snapshot.widgets[1].value.is_none()
        );
        assert!(matches!(snapshot.widgets[2].value,Some(host::UiValue::Number(n))if n==0.75));
        assert!(session.drain().unwrap().host_commands.is_empty());
        let bytes = saved.borrow();
        let document =
            roxmltree::Document::parse(std::str::from_utf8(bytes.as_ref().unwrap()).unwrap())
                .unwrap();
        let scalar = document
            .descendants()
            .find(|node| node.has_tag_name("ScriptProcessor"))
            .unwrap();
        assert!(scalar.attribute("click").is_none() && scalar.attribute("panel").is_none());
        assert!(
            scalar.attribute("gain") == Some("0.75") && scalar.attribute("enabled") == Some("1")
        );
        assert!(
            document
                .descendants()
                .find(|node| node.has_tag_name("ScriptData"))
                .unwrap()
                .attribute("curve")
                .is_some()
        );
    }

    #[test]
    fn session_ui_edits_are_scoped_scheduled_and_prevalidated() {
        let program=parse_program(r#"<Program><EventProcessors><ScriptProcessor><script><![CDATA[
          n=Knob('same',0.25,0,1)
          n.changed=function(...)
            assert(select('#',...)==1);local self=...
            Program.layers[1]:setParameter('Gain',self.value)
            sendScriptModulation(1,self.value,0);wait(10);playNote(60,90,5)
          end
        ]]></script></ScriptProcessor></EventProcessors><Layers><Layer Gain="1"><EventProcessors><ScriptProcessor><script><![CDATA[
          n=Knob('same',0.25,0,1)
          n.changed=function(...)
            assert(select('#',...)==1);local self=...
            sendScriptModulation(1,self.value,0);wait(10);playNote(72,90,5)
          end
        ]]></script></ScriptProcessor></EventProcessors></Layer></Layers></Program>"#).unwrap();
        let processors = program
            .nodes
            .iter()
            .enumerate()
            .filter(|(_, n)| n.kind == "ScriptProcessor")
            .map(|(id, _)| id)
            .collect::<Vec<_>>();
        let mut session =
            Session::new_program_chain(&program, BTreeMap::new(), None, 48_000).unwrap();
        let request = |processor, widget, value| host::UiEdit {
            processor,
            widget,
            value,
            modifiers: host::UiModifiers::default(),
        };
        for bad in [
            request(processors[0], 0, host::UiEditValue::Number(0.5)),
            request(processors[0], 1, host::UiEditValue::Boolean(true)),
            request(processors[0], 1, host::UiEditValue::Number(2.0)),
            request(program.root, 1, host::UiEditValue::Number(0.5)),
        ] {
            assert!(matches!(
                session.edit_ui(&bad, 100),
                Err(UiEditError::Rejected(_))
            ));
            assert_eq!(session.current_frame(), 0);
        }
        assert!(session.drain().unwrap().host_commands.is_empty());
        let edit = request(processors[0], 1, host::UiEditValue::Number(0.75));
        session.edit_ui(&edit, 100).unwrap();
        assert!(
            matches!(session.ui_snapshot(processors[0]).unwrap().widgets[0].value,Some(host::UiValue::Number(n))if n==0.75)
        );
        assert!(
            matches!(session.ui_snapshot(processors[1]).unwrap().widgets[0].value,Some(host::UiValue::Number(n))if n==0.25)
        );
        let first = session.drain().unwrap();
        assert!(first.commands.is_empty());
        assert!(first.host_commands.iter().any(|command|command.frame==100 && matches!(command.action,host::Action::ScriptModulation{target,layer:None,..}if target==0.75)));
        session.edit_ui(&edit, 100).unwrap();
        assert!(session.drain().unwrap().host_commands.is_empty());
        assert!(session.edit_ui(&edit, 99).is_err());
        assert_eq!(session.current_frame(), 100);
        session
            .edit_ui(
                &request(processors[1], 1, host::UiEditValue::Number(0.5)),
                100,
            )
            .unwrap();
        let second = session.drain().unwrap();
        assert!(second.host_commands.iter().any(|command|command.frame==100 && matches!(command.action,host::Action::ScriptModulation{target,layer:Some(layer),..}if target==0.5 && layer==program.layers[0])));
        session.advance(580).unwrap();
        let resumed = session.drain().unwrap();
        let notes = resumed
            .commands
            .iter()
            .filter_map(|command| match &command.action {
                Action::Start(note) => Some((command.frame, note.note, note.layers.clone())),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert!(
            notes
                .iter()
                .any(|(frame, key, _)| *frame == 580 && *key == 60)
        );
        assert!(notes.iter().any(|(frame, key, layers)| *frame == 580
            && *key == 72
            && *layers == Some(vec![program.layers[0]])));
    }

    #[test]
    fn session_ui_edit_revalidates_after_pending_work_and_reports_callback_errors() {
        let program = session_program(
            r#"
          n=Knob('gain',0.25,0,1)
          n.changed=function()error('authored callback failure')end
          function onInit()wait(10);n:setRange(0,0.5)end
        "#,
        );
        let processor = program
            .nodes
            .iter()
            .position(|n| n.kind == "ScriptProcessor")
            .unwrap();
        let mut session =
            Session::new_program_chain(&program, BTreeMap::new(), None, 48_000).unwrap();
        let mut request = host::UiEdit {
            processor,
            widget: 1,
            value: host::UiEditValue::Number(0.75),
            modifiers: host::UiModifiers::default(),
        };
        assert!(matches!(
            session.edit_ui(&request, 480),
            Err(UiEditError::Rejected(_))
        ));
        assert_eq!(session.current_frame(), 480);
        assert!(
            matches!(session.ui_snapshot(processor).unwrap().widgets[0].value,Some(host::UiValue::Number(n))if n==0.25)
        );
        assert!(session.drain().unwrap().host_commands.is_empty());
        request.value = host::UiEditValue::Number(0.5);
        let failure = session.edit_ui(&request, 480).unwrap_err();
        assert!(matches!(failure, UiEditError::Execution(_)));
        assert!(!format!("{failure:?} {failure}").contains("authored callback failure"));
        // Native setter updates the value before calling changed; callback errors
        // are reported, not hidden behind a fake successful transaction.
        assert!(
            matches!(session.ui_snapshot(processor).unwrap().widgets[0].value,Some(host::UiValue::Number(n))if n==0.5)
        );
        let program = session_program(
            "n=Knob('gain',0.25,0,1);function onInit()wait(10);error('authored pending failure')end",
        );
        let mut session =
            Session::new_program_chain(&program, BTreeMap::new(), None, 48_000).unwrap();
        assert!(matches!(
            session.edit_ui(&request, 480),
            Err(UiEditError::Execution(_))
        ));
        assert!(
            matches!(session.ui_snapshot(processor).unwrap().widgets[0].value,Some(host::UiValue::Number(n))if n==0.25)
        );
    }

    #[test]
    fn session_uses_actual_rate_and_resumes_across_chunks() {
        for rate in [44100, 96000] {
            let program = session_program(&format!(
                r#"
                function onInit()
                    assert(getSamplingRate()=={rate})
                    local id=postEvent({{type=Event.NoteOn,note=60,velocity=100,_duration=200}},100)
                    setSampleOffset(id,10)
                    changeVolume(id,0.1,false,true)
                    wait(100)
                    assert(math.abs(getTime()-100)<1e-8)
                    assert(math.abs(getRunningBeatTime()-0.2)<1e-8)
                    assert(math.abs(getBeatTime()-4.2)<1e-8)
                    changeVolume(id,0.5,false,true)
                    wait(100)
                    assert(math.abs(getTime()-200)<1e-8)
                    Program:setParameter('Gain',0.75)
                end
            "#
            ));
            let mut session =
                Session::new_program_chain(&program, BTreeMap::new(), None, rate).unwrap();
            session
                .input(Input {
                    frame: 0,
                    kind: InputKind::Transport {
                        playing: true,
                        beat: 4.,
                        tempo: 120.,
                    },
                })
                .unwrap();
            let first = session.drain().unwrap();
            assert_eq!(first.commands.len(), 1);
            assert!(matches!(first.commands[0].action, Action::Transport { .. }));
            let at100 = session.process(&[], rate as u64 / 10).unwrap();
            assert_eq!(at100.commands.len(), 2);
            assert!(matches!(&at100.commands[0].action, Action::Start(note) if note.offset_us==0));
            assert!(matches!(
                at100.commands[1].action,
                Action::Change {
                    gain: Some(0.5),
                    ..
                }
            ));
            assert!(
                at100
                    .commands
                    .iter()
                    .all(|command| command.frame == rate as u64 / 10)
            );
            let at200 = session.process(&[], rate as u64 / 5).unwrap();
            assert!(at200.commands.is_empty());
            assert_eq!(at200.host_commands.len(), 1);
            assert_eq!(at200.host_commands[0].frame, rate as u64 / 5);
            let at300 = session.process(&[], rate as u64 * 3 / 10).unwrap();
            assert_eq!(at300.commands.len(), 1);
            assert!(matches!(
                at300.commands[0].action,
                Action::ReleaseNote { .. }
            ));
            assert_eq!(at300.commands[0].frame, rate as u64 * 3 / 10);
            let notes = parse_notes_at_rate("60@100-200:100", rate).unwrap();
            assert_eq!(notes[0].frame, rate as u64 / 10);
            assert_eq!(notes[1].frame, rate as u64 / 5);
        }
    }

    #[test]
    fn session_immediate_offset_survives_leaf_dispatch_and_resets_on_authored_post() {
        for rate in [44100, 96000] {
            let source = "function onInit()local minted=__nextVoiceId();setSampleOffset(minted,20);local id=postEvent{type=Event.NoteOn,note=60,velocity=100};setSampleOffset(id,10.5)end";
            let program = session_program(source);
            let mut session =
                Session::new_program_chain(&program, BTreeMap::new(), None, rate).unwrap();
            let out = session.drain().unwrap();
            assert_eq!(out.commands.len(), 1);
            assert!(
                matches!(&out.commands[0].action, Action::Start(note) if note.offset_us==10500)
            );
            let program = parse_program(&format!("<Program><EventProcessors><ScriptProcessor><script><![CDATA[{source}]]></script></ScriptProcessor><ScriptProcessor><script><![CDATA[function onNote(e)postEvent(e);postEvent(table.copy(e))end]]></script></ScriptProcessor></EventProcessors><Layers><Layer/></Layers></Program>")).unwrap();
            let mut session =
                Session::new_program_chain(&program, BTreeMap::new(), None, rate).unwrap();
            let out = session.drain().unwrap();
            assert_eq!(out.commands.len(), 2);
            assert!(out.commands.iter().all(
                |command| matches!(&command.action, Action::Start(note) if note.offset_us==0)
            ));
        }
    }

    #[test]
    fn session_offset_targets_already_pending_same_id_posts_without_inheritance() {
        for (body, expected) in [
            (
                "local id=postEvent(e);setSampleOffset(id,10);postEvent(table.copy(e))",
                [10000, 0],
            ),
            (
                "local id=postEvent(e);postEvent(table.copy(e));setSampleOffset(id,20)",
                [20000, 20000],
            ),
        ] {
            let program = session_program(&format!(
                "function onInit()local e={{type=Event.NoteOn,note=60,velocity=100}};{body};wait(10);setSampleOffset(e.id,30)end"
            ));
            let mut session =
                Session::new_program_chain(&program, BTreeMap::new(), None, 48000).unwrap();
            let out = session.drain().unwrap();
            assert_eq!(out.commands.len(), 2);
            for (command, expected) in out.commands.iter().zip(expected) {
                assert!(matches!(&command.action, Action::Start(note) if note.offset_us==expected));
            }
            assert!(session.process(&[], 480).unwrap().commands.is_empty());
        }
    }

    #[test]
    fn session_resets_offset_on_delayed_copied_duplicate_posts() {
        let program = parse_program(r#"<Program><EventProcessors><ScriptProcessor><script><![CDATA[
            function onInit()
                local e={type=Event.NoteOn,note=60,velocity=100}
                local id=postEvent(e,100);setSampleOffset(id,10)
                local copy=table.copy(e);copy.note=61;postEvent(copy,100)
            end
        ]]></script></ScriptProcessor></EventProcessors><Layers><Layer><EventProcessors><ScriptProcessor><script><![CDATA[
            function onNote(e)postEvent(e);postEvent(table.copy(e))end
        ]]></script></ScriptProcessor></EventProcessors></Layer></Layers></Program>"#).unwrap();
        let mut session =
            Session::new_program_chain(&program, BTreeMap::new(), None, 44100).unwrap();
        assert!(session.drain().unwrap().commands.is_empty());
        let out = session.process(&[], 4410).unwrap();
        assert_eq!(out.commands.len(), 4);
        assert!(out.commands.iter().all(|command| matches!(&command.action, Action::Start(note) if note.id==1 && note.offset_us==0)));
    }

    #[test]
    fn session_retains_tail_handles_and_runs_beyond_offline_limit() {
        let program = session_program(
            r#"
            local id
            function onInit() id=playNote(60,100,1) end
            function onController(e)
                assert(getTime()>60000)
                changeTune(id,7,false,true)
                changeVolume(id,0.3,false,true)
                fadeout(id,10,false)
            end
        "#,
        );
        let mut session =
            Session::new_program_chain(&program, BTreeMap::new(), None, 96000).unwrap();
        let initial = session.process(&[], 192).unwrap();
        assert_eq!(initial.commands.len(), 2);
        assert!(matches!(initial.commands[0].action, Action::Start(_)));
        assert!(matches!(
            initial.commands[1].action,
            Action::ReleaseNote { .. }
        ));
        for _ in 0..GC_MAX_DRAINS * 2 {
            session.drain().unwrap();
        }
        let later = session
            .process(
                &[Input {
                    frame: 96000 * 61,
                    kind: InputKind::Controller {
                        channel: 0,
                        controller: 1,
                        value: 2,
                    },
                }],
                96000 * 61,
            )
            .unwrap();
        assert_eq!(later.commands.len(), 3);
        assert!(matches!(
            later.commands[0].action,
            Action::Change { tune: Some(7.), .. }
        ));
        assert!(matches!(
            later.commands[1].action,
            Action::Change {
                gain: Some(0.3),
                ..
            }
        ));
        assert!(matches!(
            later.commands[2].action,
            Action::Fade {
                duration_frames: 960,
                ..
            }
        ));
        assert_eq!(session.runtime.state.borrow().voices.len(), 1);
    }

    #[test]
    fn session_drain_preserves_future_commands_and_resets_logs() {
        let program = session_program(
            r#"
            function onInit()
                print(string.rep('x',40000))
                print('first')
                wait(10)
                print('second')
                print(string.rep('x',40000))
            end
        "#,
        );
        let mut session =
            Session::new_program_chain(&program, BTreeMap::new(), None, 48000).unwrap();
        session
            .runtime
            .state
            .borrow_mut()
            .emit(
                480,
                Action::Controller {
                    channel: 0,
                    controller: 1,
                    value: 2,
                },
            )
            .unwrap();
        session
            .runtime
            .host
            .as_ref()
            .unwrap()
            .commands
            .borrow_mut()
            .push(host::Command {
                frame: 480,
                action: host::Action::Parameter {
                    node: program.root,
                    parameter: "Gain".into(),
                    value: host::ParameterValue::Number(0.5),
                },
            });
        let first = session.drain().unwrap();
        assert!(first.commands.is_empty() && first.host_commands.is_empty());
        assert_eq!(first.logs, ["first"]);
        assert_eq!(first.dropped_logs, 1);
        let second = session.process(&[], 480).unwrap();
        assert_eq!(second.commands.len(), 1);
        assert_eq!(second.host_commands.len(), 1);
        assert_eq!(second.logs, ["second"]);
        assert_eq!(second.dropped_logs, 1);
        let third = session.drain().unwrap();
        assert!(
            third.commands.is_empty() && third.host_commands.is_empty() && third.logs.is_empty()
        );
        assert_eq!(third.dropped_logs, 0);
    }

    #[test]
    fn yielded_callback_error_retains_processor_frame_and_lua_line() {
        let program = session_program("function onNote(e)\n wait(1)\n error('authored callback marker')\nend");
        let processor = program.nodes.iter().position(|n| n.kind == "ScriptProcessor").unwrap();
        let mut session = Session::new_program_chain(&program, BTreeMap::new(), None, 48000).unwrap();
        session.input(Input {
            frame: 7,
            kind: InputKind::NoteOn { channel: 0, note: 60, velocity: 100 },
        }).unwrap();
        let error = session.advance(55).unwrap_err();
        let message = format!("{error:#}");
        assert!(message.contains(&format!("UVI ScriptProcessor node {processor} callback at frame 55")), "{message}");
        assert!(message.contains(":3:"), "{message}");
        assert!(message.contains("authored callback marker"), "{message}");
    }
    #[test]
    fn session_rejects_invalid_chunks_before_mutation() {
        let program = session_program("function onController(e) print('called');postEvent(e) end");
        assert!(Session::new_program_chain(&program, BTreeMap::new(), None, 7999).is_err());
        assert!(Session::new_program_chain(&program, BTreeMap::new(), None, 192001).is_err());
        assert!(parse_notes_at_rate("60@0-100:100", 0).is_err());
        let mut session =
            Session::new_program_chain(&program, BTreeMap::new(), None, 48000).unwrap();
        let cc = |frame, value| Input {
            frame,
            kind: InputKind::Controller {
                channel: 0,
                controller: 1,
                value,
            },
        };
        assert!(session.process(&[cc(20, 1), cc(10, 2)], 30).is_err());
        assert!(session.process(&[cc(10, 1), cc(20, 255)], 30).is_err());
        assert_eq!(session.current_frame(), 0);
        assert!(session.runtime.state.borrow().ccs.is_empty());
        session.advance(100).unwrap();
        assert!(session.input(cc(99, 2)).is_err());
        assert!(session.input(cc(200, 255)).is_err());
        assert!(session.advance(99).is_err());
        assert_eq!(session.current_frame(), 100);
        assert!(session.drain().unwrap().logs.is_empty());
        session.runtime.state.borrow_mut().input_velocities =
            (1..=LIMIT as u32).map(|id| (id, 100)).collect();
        let before_id = session.runtime.state.borrow().next_id;
        assert!(
            session
                .input(Input {
                    frame: 100,
                    kind: InputKind::NoteOn {
                        channel: 0,
                        note: 60,
                        velocity: 100
                    }
                })
                .is_err()
        );
        assert_eq!(session.runtime.state.borrow().next_id, before_id);
        assert!(session.runtime.state.borrow().keys.is_empty());
    }

    #[test]
    fn session_preserves_finished_lifo_shadow_registrations() {
        let program = parse_program(r#"<Program><EventProcessors><ScriptProcessor><script><![CDATA[
            function onInit()
                local e={type=Event.NoteOn,id=__nextVoiceId(),note=60,velocity=100,tune=0}
                postEvent(e);e.tune=12;postEvent(e)
                wait(10);e.type=Event.NoteOff;postEvent(e)
                wait(140);postEvent(e)
            end
        ]]></script></ScriptProcessor></EventProcessors><Layers><Layer><EventProcessors><ScriptProcessor><script><![CDATA[
            function onNote(e)
                postEvent(e)
                if e.tune==0 then
                    wait(100);assert(isNoteHeld())
                    wait(100);assert(not isNoteHeld());print('released old callback')
                end
            end
        ]]></script></ScriptProcessor></EventProcessors></Layer></Layers></Program>"#).unwrap();
        let mut session =
            Session::new_program_chain(&program, BTreeMap::new(), None, 48000).unwrap();
        assert_eq!(session.drain().unwrap().commands.len(), 2);
        assert_eq!(session.process(&[], 960).unwrap().commands.len(), 1);
        assert!(session.process(&[], 4800).unwrap().commands.is_empty());
        assert_eq!(session.process(&[], 7200).unwrap().commands.len(), 1);
        assert_eq!(
            session.process(&[], 9600).unwrap().logs,
            ["released old callback"]
        );
    }

    #[test]
    fn session_long_event_stream_keeps_pending_metadata_bounded() {
        let program = parse_program(r#"<Program><EventProcessors><ScriptProcessor><script><![CDATA[
            function onNote(e)postEvent(e)end function onRelease(e)postEvent(e)end
        ]]></script></ScriptProcessor></EventProcessors><Layers><Layer><EventProcessors><ScriptProcessor><script><![CDATA[
            function onNote(e)postEvent(e)end
        ]]></script></ScriptProcessor></EventProcessors></Layer></Layers></Program>"#).unwrap();
        let mut session =
            Session::new_program_chain(&program, BTreeMap::new(), None, 48000).unwrap();
        let mut frame = 0;
        for chunk in 0..550 {
            let mut inputs = Vec::new();
            for _ in 0..128 {
                inputs.push(Input {
                    frame,
                    kind: InputKind::NoteOn {
                        channel: 0,
                        note: 60,
                        velocity: 100,
                    },
                });
                inputs.push(Input {
                    frame: frame + 1,
                    kind: InputKind::NoteOff {
                        channel: 0,
                        note: 60,
                    },
                });
                frame += 2;
            }
            let out = session
                .process(&inputs, frame)
                .unwrap_or_else(|error| panic!("chunk {chunk}: {error:#}"));
            assert_eq!(out.commands.len(), 256);
            let state = session.runtime.state.borrow();
            assert!(state.voices.len() <= 16_384 && state.triggers.len() <= 32_768);
            assert!(state.trigger_queues.len() <= 32_768);
            assert!(state.keys.is_empty() && state.input_velocities.is_empty());
        }
        assert!(session.runtime.state.borrow().next_id as usize > LIMIT);
        assert!(session.runtime.state.borrow().next_trigger as usize > LIMIT);
        for _ in 0..GC_MAX_DRAINS * 2 {
            session.drain().unwrap();
        }
        assert!(session.runtime.state.borrow().voices.is_empty());
        assert!(session.runtime.state.borrow().triggers.is_empty());
        let scopes = session
            .runtime
            .lua
            .named_registry_value::<Table>("kontakto.uvi.posted_events")
            .unwrap();
        for scope in scopes.pairs::<u64, Table>() {
            assert_eq!(scope.unwrap().1.pairs::<u32, Table>().count(), 0);
        }
    }

    #[test]
    fn session_gc_preserves_retained_identity_and_retires_dropped_handles() {
        let program = session_program(
            r#"
            local kept, keys
            function onInit()
                kept=playNote(60,100,1)
                keys={[kept]=true}
            end
            function onController(e)
                if e.value==1 then
                    assert(keys[kept])
                    changeTune(kept,7,false,true)
                else
                    kept=nil
                    keys=nil
                end
            end
        "#,
        );
        let mut session =
            Session::new_program_chain(&program, BTreeMap::new(), None, 48000).unwrap();
        assert_eq!(session.process(&[], 96).unwrap().commands.len(), 2);
        for _ in 0..GC_MAX_DRAINS * 2 {
            session.drain().unwrap();
        }
        assert_eq!(session.runtime.state.borrow().voices.len(), 1);
        let cache = session
            .runtime
            .lua
            .named_registry_value::<Table>("kontakto.uvi.voice_ids")
            .unwrap();
        assert!(!matches!(cache.raw_get::<Value>(1).unwrap(), Value::Nil));
        assert!(matches!(
            session
                .process(
                    &[Input {
                        frame: 96,
                        kind: InputKind::Controller {
                            channel: 0,
                            controller: 1,
                            value: 1
                        },
                    }],
                    96
                )
                .unwrap()
                .commands[0]
                .action,
            Action::Change { tune: Some(7.), .. }
        ));
        session
            .process(
                &[Input {
                    frame: 96,
                    kind: InputKind::Controller {
                        channel: 0,
                        controller: 1,
                        value: 2,
                    },
                }],
                96,
            )
            .unwrap();
        for _ in 0..GC_MAX_DRAINS * 2 {
            session.drain().unwrap();
        }
        assert!(matches!(cache.raw_get::<Value>(1).unwrap(), Value::Nil));
        let state = session.runtime.state.borrow();
        assert!(state.voices.is_empty() && state.triggers.is_empty());
    }

    #[test]
    fn authored_json_inputs_validate_before_loading_scripts() {
        let inputs: Vec<Input> = serde_json::from_str(
            r#"[
          {"frame":0,"kind":{"NoteOn":{"channel":15,"note":127,"velocity":127}}},
          {"frame":1,"kind":{"PitchBend":{"channel":15,"bend":-1.0}}},
          {"frame":2,"kind":{"Transport":{"playing":true,"beat":-4.0,"tempo":0.1}}}
        ]"#,
        )
        .unwrap();
        assert!(validate_inputs(&inputs, 2).is_ok());
        let invalid = [
            InputKind::NoteOn {
                channel: 0,
                note: 60,
                velocity: 0,
            },
            InputKind::NoteOn {
                channel: 16,
                note: 60,
                velocity: 100,
            },
            InputKind::NoteOn {
                channel: 0,
                note: 60,
                velocity: 128,
            },
            InputKind::NoteOff {
                channel: 0,
                note: 128,
            },
            InputKind::Controller {
                channel: 0,
                controller: 128,
                value: 0,
            },
            InputKind::Controller {
                channel: 0,
                controller: 1,
                value: 128,
            },
            InputKind::PitchBend {
                channel: 0,
                bend: f64::NAN,
            },
            InputKind::PitchBend {
                channel: 0,
                bend: 1.01,
            },
            InputKind::AfterTouch {
                channel: 0,
                value: 128,
            },
            InputKind::PolyAfterTouch {
                channel: 0,
                note: 128,
                value: 0,
            },
            InputKind::PolyAfterTouch {
                channel: 16,
                note: 60,
                value: 0,
            },
            InputKind::Transport {
                playing: true,
                beat: f64::INFINITY,
                tempo: 120.,
            },
            InputKind::Transport {
                playing: true,
                beat: 0.,
                tempo: 0.,
            },
            InputKind::Transport {
                playing: true,
                beat: 0.,
                tempo: f64::NAN,
            },
        ];
        for kind in invalid {
            let error = process(
                "error('source must not load')",
                "authored-boundary",
                &[Input { frame: 0, kind }],
                0,
            )
            .unwrap_err();
            assert!(
                error
                    .to_string()
                    .contains("Invalid UVI MIDI/transport input")
            );
        }
        assert!(
            serde_json::from_str::<Vec<Input>>(
                r#"[{"frame":0,"kind":{"NoteOn":{"channel":256,"note":60,"velocity":100}}}]"#
            )
            .is_err()
        );
    }

    #[test]
    fn authored_duplicate_callbacks_hold_latest_independently_of_audio_fifo() {
        let program = parse_program(
            r#"<Program><EventProcessors><ScriptProcessor><script><![CDATA[
          function onInit()
            local e={type=Event.NoteOn,id=__nextVoiceId(),note=60,velocity=100,tune=0}
            postEvent(e);e.tune=12;postEvent(e);wait(100)
            e.type=Event.NoteOff;postEvent(e)
          end
        ]]></script></ScriptProcessor></EventProcessors><Layers><Layer><EventProcessors>
          <ScriptProcessor><script><![CDATA[
            function onNote(e)
              local tune=e.tune;postEvent(e);wait(80);assert(isNoteHeld())
              wait(40);assert(isNoteHeld()==(tune==0))
            end
          ]]></script></ScriptProcessor>
        </EventProcessors></Layer></Layers></Program>"#,
        )
        .unwrap();
        let out = process_program_chain(&program, BTreeMap::new(), None, &[], 7200).unwrap();
        assert_eq!(out.commands.len(), 3);
        assert!(matches!(&out.commands[0].action,Action::Start(n) if n.tune==0.));
        assert!(matches!(&out.commands[1].action,Action::Start(n) if n.tune==12.));
        assert!(matches!(
            out.commands[2].action,
            Action::ReleaseNote {
                id: 1,
                note: 60,
                layer: Some(_),
                ..
            }
        ));
        assert_eq!(out.commands[2].frame, 4800);
    }

    #[test]
    fn authored_release_dispatch_respects_script_scope_and_queued_notes() {
        let program = parse_program(
            r#"<Program><EventProcessors><ScriptProcessor><script><![CDATA[
          function onNote(e)
            assert(releaseVoice(e.id)==false);postEvent(e);wait(10)
            assert(releaseVoice(e.id)==true and releaseVoice(e.id)==false)
            assert(isNoteHeld())
          end
          function onRelease(e)end
        ]]></script></ScriptProcessor></EventProcessors><Layers>
          <Layer><EventProcessors><ScriptProcessor><script><![CDATA[
            function onNote(e)assert(isNoteHeld());postEvent(e);wait(30);assert(not isNoteHeld())end
            function onRelease(e)assert(e.note==60 and not isNoteHeld())end
          ]]></script></ScriptProcessor></EventProcessors></Layer>
          <Layer><EventProcessors><ScriptProcessor><script><![CDATA[
            function onNote(e)assert(isNoteHeld());e.note=72;postEvent(e);wait(30);assert(not isNoteHeld())end
            function onRelease(e)assert(e.note==60 and not isNoteHeld());e.note=72;postEvent(e)end
          ]]></script></ScriptProcessor></EventProcessors></Layer>
        </Layers></Program>"#,
        )
        .unwrap();
        let out = process_program_chain(
            &program,
            BTreeMap::new(),
            None,
            &parse_notes("60@0-100:100").unwrap(),
            4800,
        )
        .unwrap();
        assert_eq!(out.commands.len(), 3);
        assert!(
            matches!(out.commands[2].action, Action::ReleaseNote { id:1,note:72,layer:Some(layer),.. } if layer==program.layers[1])
        );
        assert_eq!(out.commands[2].frame, 480);
        let source = r#"
          function onInit()
            local id=postEvent({type=Event.NoteOn,note=60,velocity=100},100)
            wait(20);assert(releaseVoice(id)==true and releaseVoice(id)==false)
          end
        "#;
        let out = process(source, "authored-queued-note", &[], 9600).unwrap();
        assert_eq!(out.len(), 2);
        assert!(matches!(
            out[0].action,
            Action::ReleaseNote { note: 60, .. }
        ));
        assert_eq!(out[0].frame, 960);
        assert!(matches!(out[1].action, Action::Start(_)));
        assert_eq!(out[1].frame, 4800);
        let source = r#"
          local id
          function onNote(e)id=e.id;e.note=72;postEvent(e)end
          function onRelease(e)
            assert(releaseVoice(__nextVoiceId())==false)
            postEvent(e);wait(20);assert(releaseVoice(id)==false)
          end
        "#;
        let out = process(
            source,
            "authored-wrong-key-release",
            &parse_notes("60@0-100:100").unwrap(),
            6000,
        )
        .unwrap();
        assert_eq!(out.len(), 2);
        assert!(matches!(&out[0].action,Action::Start(n) if n.note==72));
        assert!(matches!(
            out[1].action,
            Action::ReleaseNote { note: 60, .. }
        ));
        let program = parse_program(
            r#"<Program><EventProcessors><ScriptProcessor><script><![CDATA[
          function onInit()
            local id=playNote(72,100,100);wait(200);assert(releaseVoice(id)==false)
          end
        ]]></script></ScriptProcessor></EventProcessors><Layers><Layer><EventProcessors>
          <ScriptProcessor><script><![CDATA[
            function onNote(e)postEvent(e);wait(200);assert(releaseVoice(e.id)==true)end
            function onRelease(e)assert(e.note==72)end
          ]]></script></ScriptProcessor>
        </EventProcessors></Layer></Layers></Program>"#,
        )
        .unwrap();
        let out = process_program_chain(&program, BTreeMap::new(), None, &[], 14400).unwrap();
        assert_eq!(out.commands.len(), 2);
        assert!(matches!(
            out.commands[1].action,
            Action::ReleaseNote { note: 72, .. }
        ));
        assert_eq!(out.commands[1].frame, 9600);
    }

    #[test]
    fn authored_duplicate_controls_preserve_raw_values_and_issuing_scope() {
        let inputs = vec![Input {
            frame: 0,
            kind: InputKind::NoteOn {
                channel: 0,
                note: 60,
                velocity: 100,
            },
        }];
        let source = r#"
          function onNote(e)
            e.tune=0;e.vol=0.25;e.pan=-0.5;postEvent(e)
            e.tune=12;e.vol=1;e.pan=0.5;postEvent(e);wait(2)
            changeVolume(e.id,0.5,true,true)
            changeTune(e.id,12,true,true)
            changePan(e.id,0.25,true,true)
            changeVolume(e.id,0.5,false,true)
            changeTune(e.id,12,false,true)
            changePan(e.id,0.25,false,true)
          end
        "#;
        let out = process(source, "authored-relative-controls", &inputs, 96).unwrap();
        assert_eq!(out.len(), 8);
        assert!(
            matches!(&out[0].action,Action::Start(n) if n.volume==0.25 && n.tune==0. && n.pan==-0.5)
        );
        assert!(
            matches!(&out[1].action,Action::Start(n) if n.volume==1. && n.tune==12. && n.pan==0.5)
        );
        for (index, command) in out[2..].iter().enumerate() {
            let Action::Change {
                gain,
                tune,
                pan,
                relative,
                layer,
                ..
            } = &command.action
            else {
                panic!("Expected a voice control");
            };
            assert_eq!(*relative, index < 3);
            assert_eq!(*layer, None);
            assert_eq!(command.frame, 96);
            assert_eq!(
                (*gain, *tune, *pan),
                match index % 3 {
                    0 => (Some(0.5), None, None),
                    1 => (None, Some(12.), None),
                    _ => (None, None, Some(0.25)),
                }
            );
        }
        let program=parse_program(r#"<Program><EventProcessors><ScriptProcessor><script><![CDATA[
          function onInit()sendScriptModulation(1,0.1,0)end
        ]]></script></ScriptProcessor></EventProcessors><Layers>
          <Layer><EventProcessors><ScriptProcessor><script><![CDATA[
            function onNote(e)sendScriptModulation(1,0.2,0,e.id);sendScriptModulation2(1,0.1,0.3,0);postEvent(e);wait(1);changeVolume(e.id,0.5,true,true)end
          ]]></script></ScriptProcessor></EventProcessors></Layer>
          <Layer><EventProcessors><ScriptProcessor><script><![CDATA[
            function onNote(e)sendScriptModulation(1,0.4,0,e.id);sendScriptModulation2(1,0.1,0.5,0);postEvent(e)end
          ]]></script></ScriptProcessor></EventProcessors></Layer>
        </Layers></Program>"#).unwrap();
        let out = process_program_chain(&program, BTreeMap::new(), None, &inputs, 96).unwrap();
        assert_eq!(out.host_commands.len(), 5);
        for (index, command) in out.host_commands.iter().enumerate() {
            let host::Action::ScriptModulation { layer, voice, .. } = &command.action else {
                panic!("Expected script modulation");
            };
            assert_eq!(
                *layer,
                if index == 0 {
                    None
                } else {
                    Some(program.layers[(index - 1) / 2])
                }
            );
            assert_eq!(*voice, if index % 2 == 1 { Some(1) } else { None });
        }
        assert!(out.commands.iter().any(|c|matches!(c.action,Action::Change{relative:true,layer:Some(layer),..} if layer==program.layers[0])));
    }

    #[test]
    fn authored_voice_handles_preserve_identity_and_key_release() {
        let program = parse_program("<Program><EventProcessors><ScriptProcessor Name='Authored'/></EventProcessors></Program>").unwrap();
        let source = r#"
          local minted,spare=__nextVoiceId(),__nextVoiceId()
          local keys={[minted]='minted'};local physical
          function onInit()
            assert(type(minted)=='userdata' and minted~=spare and keys[minted]=='minted')
            for i=1,9000 do assert(type(__nextVoiceId())=='userdata')end;collectgarbage('collect');assert(keys[minted]=='minted')
            assert(not pcall(function()__nextVoiceId(nil)end))
            assert(not pcall(function()__nextVoiceId(1,2)end))
            assert(not pcall(function()releaseVoice(1)end))
            assert(releaseVoice(minted)==false)
            changeTune(minted,12,false,true)
            sendScriptModulation(1,0.5,0,minted)
          end
          function onNote(e)
            physical=e.id;assert(type(physical)=='userdata' and physical~=minted)
            e.id=minted;assert(postEvent(e)==minted and e.id==minted and keys[e.id]=='minted')
            e.note=72;assert(postEvent(e)==minted);wait(2)
            assert(releaseVoice(minted)==true and releaseVoice(minted)==false)
          end
          function onRelease(e)assert(e.id==physical and e.note==60);postEvent(e)end
        "#;
        let out = process_program(
            source,
            "authored-handles",
            &program,
            BTreeMap::new(),
            &parse_notes("60@0-100:100").unwrap(),
            4800,
        )
        .unwrap();
        assert_eq!(out.commands.len(), 4);
        assert!(
            matches!(&out.commands[0].action, Action::Start(n) if n.id==1 && n.note==60 && n.tune==0.)
        );
        assert!(matches!(&out.commands[1].action, Action::Start(n) if n.id==1 && n.note==72));
        assert!(matches!(
            out.commands[2].action,
            Action::ReleaseNote {
                id: 1,
                note: 72,
                layer: None,
                ..
            }
        ));
        assert!(matches!(
            out.commands[3].action,
            Action::ReleaseNote {
                id: 9003,
                note: 60,
                channel: 0,
                layer: None
            }
        ));
        assert!(matches!(
            out.host_commands[0].action,
            host::Action::ScriptModulation { voice: Some(1), .. }
        ));
    }

    #[test]
    fn authored_async_updater_waits_and_coalesces_in_the_real_scheduler() {
        let program = parse_program("<Program><EventProcessors><ScriptProcessor Name='Authored'/></EventProcessors></Program>").unwrap();
        let source = r#"
          require('uvi.AsyncUpdater');local times={20,50,115};local count=0
          local u=AsyncUpdater(function(...)count=count+1;assert(select('#',...)==0 and getTime()==times[count])end)
          function onInit()assert(getTime()==0);u:trigger(20);assert(getTime()==20 and count==1);u:trigger(30);assert(getTime()==50 and count==2)end
          function onNote(e)assert(getTime()==0);u:trigger(10);assert(getTime()==0 and count==0);postEvent(e)end
          function onRelease(e)assert(getTime()==100);u:trigger(15);assert(getTime()==115 and count==3);postEvent(e)end
        "#;
        let out = process_program(
            source,
            "authored-updater",
            &program,
            BTreeMap::new(),
            &parse_notes("60@0-100:100").unwrap(),
            10000,
        )
        .unwrap();
        assert_eq!(out.commands.len(), 2);
        assert_eq!(out.commands[0].frame, 0);
        assert_eq!(out.commands[1].frame, 5520);
    }

    #[test]
    fn authored_program_callbacks_keep_timing_context_state_and_budgets() {
        let program = parse_program(r#"<Program Name="P" Gain="0.4"><EventProcessors>
          <ScriptProcessor Name="Script" n="0.8"><state>{"saved":true,"array":[9]}</state></ScriptProcessor>
        </EventProcessors></Program>"#).unwrap();
        let source = r#"
          local order=''
          print('private authored diagnostic',123)
          n=Knob{'n',0,0,1};n.changed=function(self)assert(order=='loaded');order='restored';Program:setParameter('Gain',self.value)end
          function onInit() assert(order=='restored' and math.abs(n.value-0.8)<1e-7 and require('fixture')==7);order='init';Program:setParameter('Gain',0.6) end
          function onLoad(data) assert(order=='' and n.value==0 and data.saved and data.array[1]==9);order='loaded';Program:setParameter('Gain',0.7) end
          function onTransport(playing)
            assert(type(playing)=='boolean' and getSamplingRate()==48000)
            if playing then assert(getBeatTime()==4 and getRunningBeatTime()==0)
            else assert(getBeatTime()==12 and math.abs(getRunningBeatTime()-0.04)<1e-9 and getTempo()==90) end
          end
          function onNote(e)
            assert(order=='init' and isNoteHeld());postEvent(e)
            local sequence='A'
            spawn(function()assert(sequence=='ABCD' and not isNoteHeld());playNote(e.note+2,e.velocity,50)end)
            run(function()
              assert(sequence=='A' and not isNoteHeld());sequence=sequence..'B'
              playNote(e.note+1,e.velocity,50);wait(2);assert(not isNoteHeld())
            end)
            assert(sequence=='AB' and isNoteHeld());sequence=sequence..'C'
            playNote(e.note+3,e.velocity);sequence=sequence..'D'
          end
          function onRelease(e) postEvent(e) end
          function onPitchBend(e) assert(e.channel==2 and e.bend==0.5);postEvent(e) end
          function onAfterTouch(e) assert(e.channel==2 and e.value==99);postEvent(e) end
          function onPolyAfterTouch(e) assert(e.channel==2 and e.note==60 and e.value==86);postEvent(e) end
        "#;
        let inputs = [
            Input {
                frame: 0,
                kind: InputKind::Transport {
                    playing: true,
                    beat: 4.,
                    tempo: 120.,
                },
            },
            Input {
                frame: 0,
                kind: InputKind::NoteOn {
                    channel: 1,
                    note: 60,
                    velocity: 100,
                },
            },
            Input {
                frame: 120,
                kind: InputKind::PitchBend {
                    channel: 1,
                    bend: 0.5,
                },
            },
            Input {
                frame: 240,
                kind: InputKind::AfterTouch {
                    channel: 1,
                    value: 99,
                },
            },
            Input {
                frame: 300,
                kind: InputKind::PolyAfterTouch {
                    channel: 1,
                    note: 60,
                    value: 86,
                },
            },
            Input {
                frame: 480,
                kind: InputKind::NoteOff {
                    channel: 1,
                    note: 60,
                },
            },
            Input {
                frame: 960,
                kind: InputKind::Transport {
                    playing: false,
                    beat: 12.,
                    tempo: 90.,
                },
            },
        ];
        let result = process_program(
            source,
            "authored.lua",
            &program,
            BTreeMap::from([("fixture".into(), b"return 7".to_vec())]),
            &inputs,
            3000,
        )
        .unwrap();
        assert_eq!(result.host_commands.len(), 3);
        assert_eq!(result.logs, ["private authored diagnostic\t123"]);
        assert!(
            !serde_json::to_string(&result)
                .unwrap()
                .contains("private authored diagnostic")
        );
        assert!(
            matches!(&result.host_commands[0].action, host::Action::Parameter { value: host::ParameterValue::Number(n), .. } if *n==0.7)
        );
        assert!(
            matches!(&result.host_commands[2].action, host::Action::Parameter { value: host::ParameterValue::Number(n), .. } if *n==0.6)
        );
        let starts: Vec<_> = result
            .commands
            .iter()
            .filter_map(|c| match &c.action {
                Action::Start(n) => Some((n.id, n.note)),
                _ => None,
            })
            .collect();
        assert_eq!(starts, [(1, 60), (2, 61), (3, 63), (4, 62)]);
        assert!(result.commands.iter().any(|c| c.frame == 480
            && matches!(
                c.action,
                Action::ReleaseNote {
                    id: 3,
                    note: 63,
                    layer: None,
                    ..
                }
            )));
        assert!(
            !result.commands.iter().any(
                |c| c.frame < 2400 && matches!(c.action, Action::ReleaseNote { id: 2 | 4, .. })
            )
        );
        assert!(result.commands.iter().any(|c| matches!(
            c.action,
            Action::PitchBend {
                channel: 1,
                bend: 0.5
            }
        )));
        assert!(result.commands.iter().any(|c| matches!(
            c.action,
            Action::AfterTouch {
                channel: 1,
                value: 99
            }
        )));
        assert!(result.commands.iter().any(|c| matches!(
            c.action,
            Action::PolyAfterTouch {
                channel: 1,
                note: 60,
                value: 86
            }
        )));
        let pressure = process(
            "polyAfterTouch(139,60,1);polyAfterTouch(12.75,128,0)",
            "authored-poly.lua",
            &[],
            0,
        )
        .unwrap();
        assert!(matches!(
            pressure[0].action,
            Action::PolyAfterTouch {
                channel: 0,
                note: 60,
                value: 127
            }
        ));
        assert!(matches!(
            pressure[1].action,
            Action::PolyAfterTouchAll {
                note: 127,
                value: 12
            }
        ));
        let controls = process(
            r#"controlChange(107,139,1);controlChange(108,12.75,1.5)
               postEvent{type=Event.ControlChange,controller=256,value=-1}
               controlChange(110,true,1);controlChange(111,'12.75',1)
               controlChange(112,0/0,1);controlChange(113,1/0,1)"#,
            "authored-controller.lua",
            &[],
            0,
        )
        .unwrap();
        assert_eq!(controls.len(), 7);
        assert!(matches!(
            controls[0].action,
            Action::Controller {
                channel: 0,
                controller: 107,
                value: 127
            }
        ));
        assert!(matches!(
            controls[1].action,
            Action::Controller {
                channel: 0,
                controller: 108,
                value: 12
            }
        ));
        assert!(matches!(
            controls[2].action,
            Action::ControllerAll {
                controller: 127,
                value: 0
            }
        ));
        assert!(
            controls[3..]
                .iter()
                .all(|c| matches!(c.action, Action::Controller { value: 0, .. }))
        );
        let broadcasts = process(
            "for i=1,5000 do controlChange(1,64,0) end",
            "authored-controller-bounds.lua",
            &[],
            0,
        )
        .unwrap();
        assert_eq!(broadcasts.len(), 5000);
        assert!(broadcasts.iter().all(|c| matches!(
            c.action,
            Action::ControllerAll {
                controller: 1,
                value: 64
            }
        )));
        let future_voice = process_program(
            r#"function onInit()
              local id=postEvent({type=Event.NoteOn,note=60,velocity=100},500)
              sendScriptModulation(101,0.5,0,id)
              assert(not pcall(function()sendScriptModulation(101,0.5,0,99999)end))
            end"#,
            "authored-issued-voice.lua",
            &program,
            BTreeMap::new(),
            &[],
            24000,
        )
        .unwrap();
        assert_eq!(future_voice.host_commands.len(), 1);
        assert!(matches!(
            future_voice.host_commands[0].action,
            host::Action::ScriptModulation { voice: Some(1), .. }
        ));
        assert!(
            future_voice
                .commands
                .iter()
                .any(|c| c.frame == 24000 && matches!(c.action, Action::Start(_)))
        );
        let incoming_voice = process_program(
            r#"function onNote(e) sendScriptModulation(1,0.25,0,e.id);postEvent(e) end
               function onRelease(e) sendScriptModulation(1,0.5,0,e.id);postEvent(e) end"#,
            "authored-incoming-voice.lua",
            &program,
            BTreeMap::new(),
            &parse_notes("60@0-500:100").unwrap(),
            24000,
        )
        .unwrap();
        assert_eq!(incoming_voice.host_commands.len(), 2);
        assert!(incoming_voice.host_commands.iter().all(|c| matches!(
            c.action,
            host::Action::ScriptModulation { voice: Some(1), .. }
        )));
        let chain = parse_program(r#"<Program Gain="1"><EventProcessors>
          <ScriptProcessor><state>{}</state><script><![CDATA[
            sentinel=91;local m=require('scopeFixture');assert(m==require('scopeFixture') and m.count==1)
            Program:setParameter('Gain',0.1)
            function onLoad(s)assert(Program:getParameter('Gain')==0.1)end
            function onInit()assert(Program:getParameter('Gain')==0.2)end
            function onNote(e)
              assert(e.pan==0 and e.vol==1 and e.tune==0 and e.velocity==100)
              Program:setParameter('Gain',0.1);e.note=e.note+1;postEvent(e);postEvent(e)
              e.note=10;playNote(72,100,100,2);playNote(74,100,-1,1);Program:setParameter('Gain',0.2)
            end
          ]]></script></ScriptProcessor>
          <ScriptProcessor Bypass="1" n="0.8"><state>{"saved":true}</state><script><![CDATA[
            local order='';n=Knob{'n',0,0,1}
            function onLoad(s)assert(s.saved and n.value==0);order='loaded'end
            n.changed=function()assert(order=='loaded');order='restored'end
            function onInit()error('bypassed init')end
            function onNote(e)error('bypassed event')end
          ]]></script></ScriptProcessor>
          <ScriptProcessor><state>{}</state><script><![CDATA[
            assert(sentinel==nil and _G.sentinel==nil);local m=require('scopeFixture');assert(m.count==1 and m==require('scopeFixture'))
            Program:setParameter('Gain',0.2)
            function onLoad(s)assert(Program:getParameter('Gain')==0.2)end
            function onEvent(e)
              assert(e.pan==0 and e.vol==1 and e.tune==0 and e.velocity==100)
              if e.type==Event.NoteOn then assert(Program:getParameter('Gain')==0.2);e.note=e.note+1 end
              postEvent(e)
            end
            function onNote(e)error('onEvent must take precedence')end
          ]]></script></ScriptProcessor>
        </EventProcessors><Layers>
          <Layer Name="L1"><EventProcessors><ScriptProcessor><script><![CDATA[
            function onNote(e)e.note=e.note+1;postEvent(e)end
          ]]></script></ScriptProcessor></EventProcessors></Layer>
          <Layer Name="L2"><EventProcessors><ScriptProcessor><script><![CDATA[
            function onNote(e)e.note=e.note+2;postEvent(e)end
          ]]></script></ScriptProcessor></EventProcessors></Layer>
        </Layers></Program>"#).unwrap();
        let chained = process_program_chain(
            &chain,
            BTreeMap::from([(
                "scopeFixture".into(),
                b"scopeCounter=(scopeCounter or 0)+1;return {count=scopeCounter}".to_vec(),
            )]),
            None,
            &parse_notes("60@0-500:100").unwrap(),
            24000,
        )
        .unwrap();
        let notes = chained
            .commands
            .iter()
            .filter_map(|c| match &c.action {
                Action::Start(n) => Some((n.id, n.note, n.layers.clone())),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(
            notes,
            [
                (1, 63, Some(vec![chain.layers[0]])),
                (1, 64, Some(vec![chain.layers[1]])),
                (1, 63, Some(vec![chain.layers[0]])),
                (1, 64, Some(vec![chain.layers[1]])),
                (2, 75, Some(vec![chain.layers[1]])),
                (3, 76, Some(vec![chain.layers[0]]))
            ]
        );
        for channel in ["17", "-1", "-0.5", "16.5", "true", "'2'", "0/0"] {
            assert!(
                process(
                    &format!("controlChange(1,12,{channel})"),
                    "bad-controller.lua",
                    &[],
                    0
                )
                .is_err()
            );
        }
        for input in [
            InputKind::PitchBend {
                channel: 0,
                bend: f64::NAN,
            },
            InputKind::PitchBend {
                channel: 0,
                bend: 1.1,
            },
            InputKind::AfterTouch {
                channel: 0,
                value: 128,
            },
            InputKind::PolyAfterTouch {
                channel: 0,
                note: 128,
                value: 0,
            },
            InputKind::Transport {
                playing: true,
                beat: 0.,
                tempo: 0.,
            },
        ] {
            assert!(
                process(
                    "",
                    "bad.lua",
                    &[Input {
                        frame: 0,
                        kind: input
                    }],
                    0
                )
                .is_err()
            );
        }
        for source in [
            "pcall(function()run(function()while true do end end)end)",
            "function nested()run(nested)end;nested()",
            "local n=0;while n<10000 do run(function()for i=1,1000 do n=n+0 end end);n=n+1 end",
        ] {
            assert!(process(source, "budget.lua", &[], 0).is_err());
        }
        let malformed=parse_program("<Program><EventProcessors><ScriptProcessor><state>not-json</state></ScriptProcessor></EventProcessors></Program>").unwrap();
        assert!(
            process_program(
                "function onLoad(s)end",
                "state.lua",
                &malformed,
                BTreeMap::new(),
                &[],
                0
            )
            .is_err()
        );
    }
    #[test]
    fn transport_snapshots_update_timing_but_only_play_stop_calls_on_transport() {
        let p = session_program(
            r#"
          local changes=0
          function onTransport(playing) changes=changes+1 end
          function onNote(e)
            if e.note==60 then assert(changes==0 and getBeatTime()==4 and getTempo()==120)
            elseif e.note==61 then assert(changes==1 and getBeatTime()==8)
            elseif e.note==62 then assert(changes==1 and getBeatTime()==42 and getTempo()==90)
            elseif e.note==63 then assert(changes==2 and getBeatTime()==43)
            else assert(changes==2 and getBeatTime()==44 and getTempo()==100) end
          end
        "#,
        );
        let mut s = Session::new_program_chain(&p, BTreeMap::new(), None, 48000).unwrap();
        for (frame, playing, beat, tempo, note) in [
            (0, false, 4., 120., 60),
            (1, true, 8., 120., 61),
            (2, true, 42., 90., 62),
            (3, false, 43., 90., 63),
            (4, false, 44., 100., 64),
        ] {
            s.input(Input {
                frame,
                kind: InputKind::Transport {
                    playing,
                    beat,
                    tempo,
                },
            })
            .unwrap();
            s.input(Input {
                frame,
                kind: InputKind::NoteOn {
                    channel: 0,
                    note,
                    velocity: 100,
                },
            })
            .unwrap();
        }
        let out = s.drain().unwrap();
        assert_eq!(
            out.commands
                .iter()
                .filter(|c| matches!(c.action, Action::Transport { .. }))
                .count(),
            5
        );
    }
}

#[cfg(test)]
mod hosted_tests {
    use super::*;
    fn done(s: &mut Session, frame: u64, sounding: &[HostRoot]) -> Result<Vec<HostRoot>> {
        let completed = s.complete_host_roots(frame, sounding)?;
        let roots = completed.iter().map(|c| c.root).collect::<Vec<_>>();
        s.acknowledge_host_completions(&roots)?;
        Ok(roots)
    }
    fn root(token: u64) -> HostRoot {
        HostRoot {
            epoch: 7,
            generation: 9,
            token,
        }
    }
    fn program(source: &str) -> Program {
        super::super::program::parse_program(&format!("<Program><EventProcessors><ScriptProcessor><script><![CDATA[{source}]]></script></ScriptProcessor></EventProcessors><Layers><Layer/></Layers></Program>")).unwrap()
    }
    fn session(source: &str) -> Session {
        Session::new_hosted_program_chain(&program(source), BTreeMap::new(), None, 48000, 7, 9)
            .unwrap()
    }
    fn on(frame: u64, velocity: u8) -> Input {
        Input {
            frame,
            kind: InputKind::NoteOn {
                channel: 0,
                note: 60,
                velocity,
            },
        }
    }
    fn ids(out: &Processed) -> Vec<u32> {
        out.commands
            .iter()
            .filter_map(|c| match &c.action {
                Action::Start(n) => Some(n.id),
                Action::ReleaseNote { id, .. } => Some(*id),
                _ => None,
            })
            .collect()
    }
    #[test]
    fn hosted_choke_cancels_waiting_and_delayed_descendants_without_release() {
        let mut s = session(
            r#"
            function onNote(e)
                saved=e
                postEvent(e)
                postEvent(e,100)
                spawn(function() wait(120);playNote(72,100,50) end)
                wait(150);playNote(74,100,50)
            end
            function onRelease(e) error('choke synthesized release') end
            function onController(e) releaseVoice(saved.id);postEvent(e) end
        "#,
        );
        s.host_note_on(root(1), on(0, 100)).unwrap();
        s.host_note_choke(root(1), 48).unwrap();
        s.host_note_choke(root(1), 48).unwrap();
        s.host_note_off(root(1), 48).unwrap();
        s.advance(12000).unwrap();
        let out = s.drain().unwrap();
        assert_eq!(out.commands.len(), 2);
        assert!(matches!(out.commands[0].action, Action::Start(_)));
        assert!(matches!(out.commands[1].action, Action::ChokeRoot));
        assert_eq!(out.command_roots, [Some(root(1)), Some(root(1))]);
        assert_eq!(done(&mut s, 12001, &[]).unwrap(), [root(1)]);
        // A queued acknowledgement may precede host consumption; repeated stop is harmless.
        s.host_note_choke(root(1), 12001).unwrap();
        s.input(Input {
            frame: 12001,
            kind: InputKind::Controller {
                channel: 0,
                controller: 1,
                value: 2,
            },
        })
        .unwrap();
        let out = s.drain().unwrap();
        assert_eq!(out.commands.len(), 1);
        assert!(matches!(out.commands[0].action, Action::Controller { .. }));
        s.host_note_on(root(2), on(12001, 90)).unwrap();
        assert!(
            s.drain()
                .unwrap()
                .commands
                .iter()
                .any(|c| matches!(c.action, Action::Start(_)))
        );
    }
    #[test]
    fn hosted_choke_cancels_layer_forward_and_retained_release_but_fresh_post_is_detached() {
        let p=super::super::program::parse_program(r#"<Program><EventProcessors><ScriptProcessor><script><![CDATA[
            local saved
            function onNote(e) saved=saved or e;postEvent(e);postEvent(e,100) end
            function onRelease(e) error('choke synthesized program release') end
            function onController(e)
                if e.value==1 then releaseVoice(saved.id)
                else postEvent({type=Event.NoteOn,id=saved.id,note=70,velocity=100}) end
            end
        ]]></script></ScriptProcessor></EventProcessors><Layers><Layer><EventProcessors><ScriptProcessor><script><![CDATA[
            function onNote(e)postEvent(e);wait(150);playNote(72,100,10)end
            function onRelease(e)error('choke synthesized layer release')end
        ]]></script></ScriptProcessor></EventProcessors></Layer></Layers></Program>"#).unwrap();
        let mut s =
            Session::new_hosted_program_chain(&p, BTreeMap::new(), None, 48000, 7, 9).unwrap();
        s.host_note_on(root(1), on(0, 100)).unwrap();
        assert!(!s.runtime.state.borrow().forwards.is_empty());
        s.host_note_choke(root(1), 48).unwrap();
        assert!(s.runtime.state.borrow().forwards.is_empty());
        s.input(Input {
            frame: 48,
            kind: InputKind::Controller {
                channel: 0,
                controller: 1,
                value: 1,
            },
        })
        .unwrap();
        s.advance(8000).unwrap();
        let out = s.drain().unwrap();
        assert_eq!(
            out.commands
                .iter()
                .filter(|c| matches!(c.action, Action::Start(_)))
                .count(),
            1
        );
        assert_eq!(
            out.commands
                .iter()
                .filter(|c| matches!(c.action, Action::ChokeRoot))
                .count(),
            1
        );
        assert_eq!(done(&mut s, 8001, &[]).unwrap(), [root(1)]);
        // An explicit new post with the stored opaque ID cannot revive sealed ancestry.
        s.input(Input {
            frame: 8001,
            kind: InputKind::Controller {
                channel: 0,
                controller: 1,
                value: 2,
            },
        })
        .unwrap();
        let out = s.drain().unwrap();
        assert!(
            out.commands
                .iter()
                .any(|c| matches!(&c.action,Action::Start(n) if n.note==70))
        );
        assert!(out.command_roots.iter().all(Option::is_none));
        // Stable same-frame wire order admits a fresh root after On -> Choke.
        s.host_note_on(root(2), on(8001, 100)).unwrap();
        s.host_note_choke(root(2), 8001).unwrap();
        s.host_note_on(root(3), on(8001, 100)).unwrap();
        let out = s.drain().unwrap();
        assert!(matches!(out.commands[0].action, Action::ChokeRoot));
        assert_eq!(out.command_roots[0], Some(root(2)));
        assert!(
            out.commands
                .iter()
                .zip(&out.command_roots)
                .any(|(c, r)| matches!(c.action, Action::Start(_)) && *r == Some(root(3)))
        );
        assert!(
            !out.commands
                .iter()
                .zip(&out.command_roots)
                .any(|(c, r)| matches!(c.action, Action::Start(_)) && *r == Some(root(2)))
        );
    }
    #[test]
    fn hosted_choke_keeps_another_roots_same_id_registration_and_gate() {
        let mut s = session(
            r#"
            function onNote(e) if saved==nil then saved=e end postEvent(saved) end
            function onController(e) releaseVoice(saved.id) end
            function onRelease(e) postEvent(e) end
        "#,
        );
        s.host_note_on(root(1), on(0, 100)).unwrap();
        s.host_note_on(root(2), on(0, 90)).unwrap();
        s.host_note_choke(root(1), 1).unwrap();
        {
            let state = s.runtime.state.borrow();
            assert!(state.posted_roots.values().any(|r| *r == Some(root(2))));
            assert!(
                state
                    .trigger_roots
                    .iter()
                    .any(|(id, r)| *r == root(2) && state.triggers[id].held)
            );
            assert!(
                state
                    .trigger_roots
                    .iter()
                    .filter(|(_, r)| **r == root(1))
                    .all(|(id, _)| !state.triggers[id].held)
            );
        }
        s.input(Input {
            frame: 2,
            kind: InputKind::Controller {
                channel: 0,
                controller: 1,
                value: 2,
            },
        })
        .unwrap();
        let out = s.drain().unwrap();
        assert!(out.commands.iter().zip(&out.command_roots).any(|(c,r)|
            matches!(c.action,Action::ReleaseNote{..})&&*r==Some(root(2))));
        assert_eq!(done(&mut s, 3, &[root(2)]).unwrap(), [root(1)]);
    }
    #[test]
    fn hosted_choke_retains_full_ledger_until_queued_completion_ack() {
        let mut s = session("function onNote(e)end");
        for token in 1..=HOST_ROOT_CAPACITY as u64 {
            s.host_note_on(root(token), on(0, 100)).unwrap();
        }
        s.host_note_choke(root(1), 1).unwrap();
        s.drain().unwrap();
        assert_eq!(
            s.complete_host_roots(2, &[]).unwrap(),
            [HostCompletion {
                root: root(1),
                frame: 2
            }]
        );
        let events = [
            HostedInput::Choke {
                root: root(1),
                frame: 100,
            },
            HostedInput::On {
                root: root(HOST_ROOT_CAPACITY as u64 + 1),
                input: on(101, 100),
            },
        ];
        assert!(s.validate_hosted_inputs(&events).is_err());
        assert_eq!(s.current_frame(), 1);
        assert_eq!(s.runtime.state.borrow().roots.len(), HOST_ROOT_CAPACITY);
        s.host_note_choke(root(1), 2).unwrap();
        assert!(s.drain().unwrap().commands.is_empty());
        assert_eq!(s.complete_host_roots(3, &[]).unwrap()[0].frame, 2);
        s.acknowledge_host_completions(&[root(1)]).unwrap();
        s.host_note_choke(root(1), 3).unwrap();
        s.host_note_on(root(HOST_ROOT_CAPACITY as u64 + 1), on(3, 100))
            .unwrap();
    }
    #[test]
    fn hosted_choke_batch_admission_rejects_suffix_before_mutation() {
        let mut s = session("function onNote(e)postEvent(e)end");
        let prefix = HostedInput::On {
            root: root(1),
            input: on(10, 100),
        };
        for bad in [
            HostedInput::Choke {
                root: root(2),
                frame: 11,
            },
            HostedInput::Choke {
                root: HostRoot {
                    epoch: 8,
                    ..root(1)
                },
                frame: 11,
            },
            HostedInput::Choke {
                root: root(0),
                frame: 11,
            },
        ] {
            assert!(s.validate_hosted_inputs(&[prefix, bad]).is_err());
            assert_eq!(s.current_frame(), 0);
            assert!(s.runtime.state.borrow().roots.is_empty());
        }
        let events = [
            prefix,
            HostedInput::Choke {
                root: root(1),
                frame: 11,
            },
            HostedInput::Off {
                root: root(1),
                frame: 11,
            },
        ];
        s.validate_hosted_inputs(&events).unwrap();
        s.host_note_on(root(1), on(10, 100)).unwrap();
        s.host_note_choke(root(1), 11).unwrap();
        s.host_note_off(root(1), 11).unwrap();
        assert_eq!(s.runtime.state.borrow().roots.len(), 1);
        assert!(s.runtime.state.borrow().roots[&root(1)].choked);
    }
    #[test]
    fn hosted_packet_admission_rejects_invalid_suffix_without_mutation() {
        let mut s =
            session("function onNote(e)postEvent(e)end function onRelease(e)postEvent(e)end");
        s.host_note_on(root(1), on(0, 100)).unwrap();
        s.host_note_off(root(1), 1).unwrap();
        s.drain().unwrap();
        assert_eq!(s.complete_host_roots(2, &[]).unwrap().len(), 1);
        let prefix = HostedInput::On {
            root: root(2),
            input: on(10, 100),
        };
        let before = {
            let state = s.runtime.state.borrow();
            (
                state.now,
                state.next_id,
                state.last_root,
                state.roots.len(),
                state.commands.len(),
                state.tasks.len(),
                state.forwards.len(),
            )
        };
        for suffix in [
            HostedInput::On {
                root: root(2),
                input: on(11, 100),
            },
            HostedInput::On {
                root: root(1),
                input: on(11, 100),
            },
            HostedInput::On {
                root: HostRoot {
                    epoch: 8,
                    ..root(3)
                },
                input: on(11, 100),
            },
            HostedInput::On {
                root: HostRoot {
                    generation: 10,
                    ..root(3)
                },
                input: on(11, 100),
            },
            HostedInput::On {
                root: root(0),
                input: on(11, 100),
            },
            HostedInput::Off {
                root: root(99),
                frame: 11,
            },
            HostedInput::Off {
                root: root(1),
                frame: 11,
            },
            HostedInput::Off {
                root: HostRoot {
                    epoch: 8,
                    ..root(2)
                },
                frame: 11,
            },
            HostedInput::Off {
                root: root(2),
                frame: 9,
            },
            HostedInput::On {
                root: root(3),
                input: Input {
                    frame: 11,
                    kind: InputKind::NoteOff {
                        channel: 0,
                        note: 60,
                    },
                },
            },
            HostedInput::On {
                root: root(3),
                input: on(11, 255),
            },
            HostedInput::Event(on(11, 100)),
            HostedInput::Event(Input {
                frame: 11,
                kind: InputKind::NoteOff {
                    channel: 0,
                    note: 60,
                },
            }),
            HostedInput::Event(Input {
                frame: 11,
                kind: InputKind::PitchBend {
                    channel: 0,
                    bend: f64::NAN,
                },
            }),
            HostedInput::Event(Input {
                frame: 11,
                kind: InputKind::Controller {
                    channel: 0,
                    controller: 64,
                    value: 255,
                },
            }),
        ] {
            assert!(s.validate_hosted_inputs(&[prefix, suffix]).is_err());
            let state = s.runtime.state.borrow();
            assert_eq!(
                before,
                (
                    state.now,
                    state.next_id,
                    state.last_root,
                    state.roots.len(),
                    state.commands.len(),
                    state.tasks.len(),
                    state.forwards.len()
                )
            );
        }
        let valid = [
            prefix,
            HostedInput::Off {
                root: root(2),
                frame: 11,
            },
            HostedInput::Off {
                root: root(2),
                frame: 12,
            },
        ];
        s.validate_hosted_inputs(&valid).unwrap();
        assert_eq!(s.current_frame(), 1);
        for event in valid {
            match event {
                HostedInput::On { root, input } => s.host_note_on(root, input).unwrap(),
                HostedInput::Off { root, frame } => s.host_note_off(root, frame).unwrap(),
                HostedInput::Choke { root, frame } => s.host_note_choke(root, frame).unwrap(),
                HostedInput::Event(input) => s.input(input).unwrap(),
            }
        }
        s.drain().unwrap();
        assert_eq!(
            s.complete_host_roots(13, &[]).unwrap(),
            [
                HostCompletion {
                    root: root(1),
                    frame: 2
                },
                HostCompletion {
                    root: root(2),
                    frame: 13
                }
            ]
        );
    }
    #[test]
    fn hosted_ordered_controls_keep_same_frame_note_gate_order() {
        for pedal_first in [false, true] {
            let expected = if pedal_first { 127 } else { 0 };
            let mut s = session(&format!(
                "function onNote(e)postEvent(e)end function onController(e)postEvent(e)end function onRelease(e)assert(getCC(64)=={expected});postEvent(e)end"
            ));
            let pedal = HostedInput::Event(Input {
                frame: 1,
                kind: InputKind::Controller {
                    channel: 0,
                    controller: 64,
                    value: 127,
                },
            });
            let off = HostedInput::Off {
                root: root(1),
                frame: 1,
            };
            let packet = [
                HostedInput::On {
                    root: root(1),
                    input: on(0, 100),
                },
                if pedal_first { pedal } else { off },
                if pedal_first { off } else { pedal },
            ];
            s.validate_hosted_inputs(&packet).unwrap();
            for event in packet {
                match event {
                    HostedInput::On { root, input } => s.host_note_on(root, input).unwrap(),
                    HostedInput::Off { root, frame } => s.host_note_off(root, frame).unwrap(),
                    HostedInput::Choke { root, frame } => s.host_note_choke(root, frame).unwrap(),
                    HostedInput::Event(input) => s.input(input).unwrap(),
                }
            }
            let commands = s.drain().unwrap().commands;
            assert_eq!(commands.len(), 3);
            assert!(matches!(commands[0].action, Action::Start(_)));
            let gate = if pedal_first { 2 } else { 1 };
            let control = if pedal_first { 1 } else { 2 };
            assert!(matches!(commands[gate].action, Action::ReleaseNote { .. }));
            assert!(matches!(
                commands[control].action,
                Action::Controller {
                    controller: 64,
                    value: 127,
                    ..
                }
            ));
        }
    }
    #[test]
    fn hosted_ordered_bend_and_cc_callbacks_precede_same_frame_on() {
        let mut s = session(
            "local cc,bend=0,0 function onController(e)cc=e.value;postEvent(e)end function onPitchBend(e)bend=e.bend;postEvent(e)end function onNote(e)assert(cc==64 and bend==.5);postEvent(e)end",
        );
        let packet = [
            HostedInput::Event(Input {
                frame: 0,
                kind: InputKind::Controller {
                    channel: 0,
                    controller: 1,
                    value: 64,
                },
            }),
            HostedInput::Event(Input {
                frame: 0,
                kind: InputKind::PitchBend {
                    channel: 0,
                    bend: 0.5,
                },
            }),
            HostedInput::On {
                root: root(1),
                input: on(0, 100),
            },
        ];
        s.validate_hosted_inputs(&packet).unwrap();
        for event in packet {
            match event {
                HostedInput::On { root, input } => s.host_note_on(root, input).unwrap(),
                HostedInput::Off { root, frame } => s.host_note_off(root, frame).unwrap(),
                HostedInput::Choke { root, frame } => s.host_note_choke(root, frame).unwrap(),
                HostedInput::Event(input) => s.input(input).unwrap(),
            }
        }
        let output = s.drain().unwrap();
        assert!(matches!(
            output.commands[0].action,
            Action::Controller {
                controller: 1,
                value: 64,
                ..
            }
        ));
        assert!(matches!(
            output.commands[1].action,
            Action::PitchBend { bend: 0.5, .. }
        ));
        assert!(matches!(output.commands[2].action, Action::Start(_)));
        assert_eq!(output.command_roots, [None, None, Some(root(1))]);
    }
    #[test]
    fn hosted_packet_admission_counts_pending_completions_and_same_batch_off() {
        let mut s = session("function onNote(e)end");
        for token in 1..HOST_ROOT_CAPACITY as u64 {
            s.host_note_on(root(token), on(0, 100)).unwrap();
        }
        s.host_note_off(root(1), 1).unwrap();
        s.drain().unwrap();
        assert_eq!(s.complete_host_roots(2, &[]).unwrap().len(), 1);
        let next = HOST_ROOT_CAPACITY as u64;
        let packet = [
            HostedInput::On {
                root: root(next),
                input: on(10, 100),
            },
            HostedInput::Off {
                root: root(next),
                frame: 11,
            },
            HostedInput::On {
                root: root(next + 1),
                input: on(12, 100),
            },
        ];
        assert!(s.validate_hosted_inputs(&packet).is_err());
        assert_eq!(s.current_frame(), 1);
        assert_eq!(s.runtime.state.borrow().last_root, next - 1);
        assert_eq!(s.runtime.state.borrow().roots.len(), HOST_ROOT_CAPACITY - 1);
        s.validate_hosted_inputs(&packet[..2]).unwrap();
        let oversized = vec![
            HostedInput::Off {
                root: root(2),
                frame: 10
            };
            HOST_ROOT_CAPACITY + 1
        ];
        assert!(s.validate_hosted_inputs(&oversized).is_err());
        s.acknowledge_host_completions(&[root(1)]).unwrap();
        s.validate_hosted_inputs(&packet).unwrap();
        assert_eq!(s.current_frame(), 1);
        assert_eq!(s.runtime.state.borrow().roots.len(), HOST_ROOT_CAPACITY - 2);
    }
    #[test]
    fn hosted_initialization_commands_are_backfilled_once_before_rooted_input() {
        let source = "function onInit()postEvent{type=Event.NoteOn,note=61,velocity=90};wait(5);postEvent{type=Event.NoteOn,note=62,velocity=80}end function onNote(e)postEvent(e)end";
        let mut s = session(source);
        {
            let state = s.runtime.state.borrow();
            assert_eq!(state.commands.len(), 1);
            assert_eq!(state.command_roots, [None]);
        }
        s.host_note_on(root(1), on(0, 100)).unwrap();
        let out = s.drain().unwrap();
        assert_eq!(ids(&out), [1, 2]);
        assert_eq!(out.command_roots, [None, Some(root(1))]);
        assert!(s.drain().unwrap().command_roots.is_empty());
        s.advance(240).unwrap();
        let out = s.drain().unwrap();
        assert_eq!(ids(&out), [3]);
        assert_eq!(out.command_roots, [None]);
        s.host_note_on(root(2), on(240, 100)).unwrap();
        assert_eq!(s.drain().unwrap().command_roots, [Some(root(2))]);
    }
    #[test]
    fn legacy_commands_and_posted_registrations_allocate_no_root_storage() {
        let p = program(
            "function onInit()postEvent{type=Event.NoteOn,note=61,velocity=90}end function onNote(e)postEvent(e);postEvent(table.copy(e),5)end function onRelease(e)postEvent(e)end",
        );
        let mut s = Session::new_program_chain(&p, BTreeMap::new(), None, 48000).unwrap();
        for frame in 0..32 {
            s.input(on(frame, 100)).unwrap();
            let state = s.runtime.state.borrow();
            assert!(state.root_activation.is_none());
            assert_eq!(state.trigger_roots.capacity(), 0);
            assert!(state.command_roots.is_empty());
            assert_eq!(state.command_roots.capacity(), 0);
            assert!(state.posted_roots.is_empty());
            assert_eq!(state.posted_roots.capacity(), 0);
        }
        let out = s.drain().unwrap();
        assert!(!out.commands.is_empty());
        assert_eq!(out.command_roots.capacity(), 0);
        s.advance(300).unwrap();
        let out = s.drain().unwrap();
        assert_eq!(out.commands.len(), 32);
        assert_eq!(out.command_roots.capacity(), 0);
        let offline = process_program_chain(&p, BTreeMap::new(), None, &[on(0, 100)], 300).unwrap();
        assert!(!offline.commands.is_empty());
        assert_eq!(offline.command_roots.capacity(), 0);
    }
    #[test]
    fn hosted_completion_acknowledgements_validate_entire_batch_before_removal() {
        let mut s = session("function onNote(e)end");
        s.host_note_on(root(1), on(0, 100)).unwrap();
        s.host_note_on(root(2), on(0, 100)).unwrap();
        s.host_note_off(root(1), 1).unwrap();
        s.drain().unwrap();
        let expected = [HostCompletion {
            root: root(1),
            frame: 2,
        }];
        assert_eq!(s.complete_host_roots(2, &[]).unwrap(), expected);
        for rejected in [
            vec![root(1), root(1)],
            vec![root(1), root(2)],
            vec![
                root(1),
                HostRoot {
                    epoch: 8,
                    ..root(1)
                },
            ],
            vec![root(1); HOST_ROOT_CAPACITY + 1],
        ] {
            assert!(s.acknowledge_host_completions(&rejected).is_err());
            assert_eq!(s.current_frame(), 1);
            assert_eq!(s.runtime.state.borrow().roots.len(), 2);
            assert_eq!(s.complete_host_roots(2, &[]).unwrap(), expected);
        }
        assert!(s.host_note_off(root(1), 100).is_err());
        assert_eq!(s.current_frame(), 1);
        s.acknowledge_host_completions(&[root(1)]).unwrap();
        assert!(s.acknowledge_host_completions(&[root(1)]).is_err());
        assert_eq!(s.current_frame(), 1);
        assert_eq!(s.runtime.state.borrow().roots.len(), 1);
    }
    #[test]
    fn hosted_exact_same_key_overlap_preserves_legacy_fifo() {
        let mut s =
            session("function onNote(e)postEvent(e)end function onRelease(e)postEvent(e)end");
        s.host_note_on(root(1), on(0, 100)).unwrap();
        s.host_note_on(root(2), on(0, 80)).unwrap();
        s.host_note_off(root(2), 1).unwrap();
        s.input(on(1, 90)).unwrap();
        s.input(Input {
            frame: 2,
            kind: InputKind::NoteOff {
                channel: 0,
                note: 60,
            },
        })
        .unwrap();
        let out = s.drain().unwrap();
        assert_eq!(ids(&out), [1, 2, 2, 3, 1]);
        assert_eq!(
            out.command_roots,
            [
                Some(root(1)),
                Some(root(2)),
                Some(root(2)),
                None,
                Some(root(1))
            ]
        );
        assert_eq!(done(&mut s, 3, &[root(1)]).unwrap(), [root(2)]);
        assert_eq!(done(&mut s, 3, &[]).unwrap(), [root(1)]);
        assert!(done(&mut s, 3, &[]).unwrap().is_empty());
    }
    #[test]
    fn hosted_consumed_note_is_pinned_until_physical_release() {
        let mut s = session("function onNote(e)end");
        s.host_note_on(root(1), on(0, 100)).unwrap();
        assert!(s.drain().unwrap().commands.is_empty());
        assert!(done(&mut s, 1, &[]).unwrap().is_empty());
        s.host_note_off(root(1), 2).unwrap();
        let out = s.drain().unwrap();
        assert_eq!(out.command_roots, [Some(root(1))]);
        assert_eq!(done(&mut s, 3, &[]).unwrap(), [root(1)]);
        assert!(done(&mut s, 3, &[]).unwrap().is_empty());
    }
    #[test]
    fn hosted_delayed_and_run_spawn_descendants_pin_without_held_parent() {
        let mut s = session(
            r#"
            function onNote(e)
                postEvent(e,100)
                spawn(function() assert(not isNoteHeld());wait(150);playNote(62,100,10);wait(10);fadeout(e.id,0,false) end)
                run(function() assert(not isNoteHeld());wait(120);playNote(61,100,10) end)
            end
            function onRelease(e)postEvent(e)end
        "#,
        );
        s.host_note_on(root(1), on(0, 100)).unwrap();
        s.host_note_off(root(1), 48).unwrap();
        s.drain().unwrap();
        assert!(done(&mut s, 49, &[]).unwrap().is_empty());
        s.advance(4800).unwrap();
        let out = s.drain().unwrap();
        assert!(
            out.commands
                .iter()
                .any(|c| matches!(&c.action,Action::Start(n) if n.note==60))
        );
        assert!(out.command_roots.iter().all(|r| *r == Some(root(1))));
        assert!(done(&mut s, 4801, &[]).unwrap().is_empty());
        s.advance(6250).unwrap();
        let out = s.drain().unwrap();
        assert!(
            out.commands
                .iter()
                .any(|c| matches!(&c.action,Action::Start(n) if n.note==61))
        );
        assert!(out.command_roots.iter().all(|r| *r == Some(root(1))));
        assert!(done(&mut s, 6251, &[]).unwrap().is_empty());
        s.advance(7700).unwrap();
        let out = s.drain().unwrap();
        assert!(
            out.commands
                .iter()
                .any(|c| matches!(&c.action,Action::Start(n) if n.note==62))
        );
        assert!(out.command_roots.iter().all(|r| *r == Some(root(1))));
        assert!(done(&mut s, 7701, &[root(1)]).unwrap().is_empty());
        assert_eq!(done(&mut s, 7701, &[]).unwrap(), [root(1)]);
    }
    #[test]
    fn hosted_scoped_release_hint_survives_ui_context_without_root() {
        let p=super::super::program::parse_program(r#"<Program><EventProcessors><ScriptProcessor><script><![CDATA[
            local id
            function onNote(e)id=e.id;postEvent(e);postEvent(table.copy(e))end
            function onRelease(e)end
            function onInit() release=Button("Release");release.changed=function()assert(releaseVoice(id));fadeout(id,0,false)end end
        ]]></script></ScriptProcessor></EventProcessors><Layers><Layer><EventProcessors><ScriptProcessor><script><![CDATA[
            function onNote(e)postEvent(e)end
            function onRelease(e)playNote(72,100,10)end
        ]]></script></ScriptProcessor></EventProcessors></Layer></Layers></Program>"#).unwrap();
        let mut s =
            Session::new_hosted_program_chain(&p, BTreeMap::new(), None, 48000, 7, 9).unwrap();
        s.host_note_on(root(1), on(0, 100)).unwrap();
        s.host_note_off(root(1), 1).unwrap();
        let out = s.drain().unwrap();
        assert_eq!(
            out.commands
                .iter()
                .filter(|c| matches!(c.action, Action::Start(_)))
                .count(),
            2
        );
        assert!(done(&mut s, 2, &[root(1)]).unwrap().is_empty());
        let processor = s.runtime.environments.keys().copied().min().unwrap();
        s.edit_ui(
            &host::UiEdit {
                processor,
                widget: 1,
                value: host::UiEditValue::Push,
                modifiers: host::UiModifiers::default(),
            },
            2,
        )
        .unwrap();
        let out = s.drain().unwrap();
        let generated = out
            .commands
            .iter()
            .position(|c| matches!(&c.action,Action::Start(n) if n.note==72))
            .unwrap();
        assert_eq!(out.command_roots[generated], Some(root(1)));
        assert!(done(&mut s, 3, &[]).unwrap().is_empty());
        s.advance(500).unwrap();
        s.drain().unwrap();
        assert_eq!(done(&mut s, 501, &[]).unwrap(), [root(1)]);
    }
    #[test]
    fn hosted_closed_retained_handle_and_registration_cannot_revive_root() {
        let p=super::super::program::parse_program(r#"<Program><EventProcessors><ScriptProcessor><script><![CDATA[
            local saved
            function onNote(e)if not saved then saved=e.id end;postEvent(e)end
            function onRelease(e)fadeout(e.id,0,false)end
            function onController(e)
                if e.value==1 then assert(releaseVoice(saved))
                else postEvent({type=Event.NoteOn,id=saved,note=70,velocity=100}) end
            end
        ]]></script></ScriptProcessor></EventProcessors><Layers><Layer><EventProcessors><ScriptProcessor><script><![CDATA[
            function onNote(e)postEvent(e)end
            function onRelease(e)playNote(72,100,1)end
        ]]></script></ScriptProcessor></EventProcessors></Layer></Layers></Program>"#).unwrap();
        let mut s =
            Session::new_hosted_program_chain(&p, BTreeMap::new(), None, 48000, 7, 9).unwrap();
        s.host_note_on(root(1), on(0, 100)).unwrap();
        s.host_note_off(root(1), 1).unwrap();
        s.advance(50).unwrap();
        s.drain().unwrap();
        assert_eq!(done(&mut s, 51, &[]).unwrap(), [root(1)]);
        s.host_note_on(root(2), on(51, 100)).unwrap();
        s.drain().unwrap();
        s.input(Input {
            frame: 52,
            kind: InputKind::Controller {
                channel: 0,
                controller: 1,
                value: 1,
            },
        })
        .unwrap();
        let out = s.drain().unwrap();
        assert!(out.command_roots.iter().all(Option::is_none));
        s.input(Input {
            frame: 53,
            kind: InputKind::Controller {
                channel: 0,
                controller: 1,
                value: 2,
            },
        })
        .unwrap();
        let out = s.drain().unwrap();
        assert!(out.command_roots.iter().all(Option::is_none));
        assert!(
            out.commands
                .iter()
                .any(|c| matches!(&c.action,Action::Start(n) if n.id==1 && n.note==70))
        );
        assert!(s.runtime.state.borrow().roots.contains_key(&root(2)));
    }
    #[test]
    fn hosted_stale_duplicate_and_capacity_reject_before_clock_mutation() {
        let mut s = session("function onNote(e)end");
        for bad in [
            HostRoot {
                epoch: 8,
                ..root(1)
            },
            HostRoot {
                generation: 10,
                ..root(1)
            },
            root(0),
        ] {
            assert!(s.host_note_on(bad, on(100, 100)).is_err());
            assert_eq!(s.current_frame(), 0);
            assert_eq!(s.runtime.state.borrow().next_id, 0);
        }
        for id in 1..=HOST_ROOT_CAPACITY as u64 {
            s.host_note_on(root(id), on(0, 100)).unwrap();
        }
        assert!(
            s.host_note_on(root(HOST_ROOT_CAPACITY as u64 + 1), on(100, 100))
                .is_err()
        );
        assert!(s.host_note_on(root(1), on(100, 100)).is_err());
        assert_eq!(s.current_frame(), 0);
        s.host_note_off(root(1), 1).unwrap();
        s.drain().unwrap();
        assert_eq!(done(&mut s, 2, &[]).unwrap(), [root(1)]);
        assert!(s.host_note_off(root(1), 100).is_err());
        assert_eq!(s.current_frame(), 1);
        s.host_note_on(root(HOST_ROOT_CAPACITY as u64 + 1), on(2, 100))
            .unwrap();
        assert_eq!(s.runtime.state.borrow().roots.len(), HOST_ROOT_CAPACITY);
    }
    #[test]
    fn hosted_same_opaque_id_keeps_distinct_per_post_root_ancestry() {
        let mut s = session(
            "local shared;function onNote(e)if shared then e.id=shared else shared=e.id end;postEvent(e);postEvent(table.copy(e))end function onRelease(e)postEvent(e)end",
        );
        s.host_note_on(root(1), on(0, 100)).unwrap();
        s.host_note_on(root(2), on(0, 80)).unwrap();
        let out = s.drain().unwrap();
        assert_eq!(ids(&out), [1, 1, 1, 1]);
        assert_eq!(
            out.command_roots,
            [Some(root(1)), Some(root(1)), Some(root(2)), Some(root(2))]
        );
        s.host_note_off(root(1), 1).unwrap();
        s.host_note_off(root(2), 1).unwrap();
        s.drain().unwrap();
        assert!(done(&mut s, 2, &[root(1), root(2)]).unwrap().is_empty());
        assert_eq!(done(&mut s, 2, &[root(2)]).unwrap(), [root(1)]);
        assert_eq!(done(&mut s, 2, &[]).unwrap(), [root(2)]);
    }
    #[test]
    fn hosted_completion_backpressure_preserves_capacity_boundary_and_closed_policy() {
        let mut s = session(
            "local id;function onNote(e)if not id then id=e.id end end function onController(e)postEvent({type=Event.NoteOn,id=id,note=70,velocity=100})end",
        );
        for id in 1..=HOST_ROOT_CAPACITY as u64 {
            s.host_note_on(root(id), on(0, 100)).unwrap();
        }
        s.host_note_off(root(1), 1).unwrap();
        s.drain().unwrap();
        let expected = HostCompletion {
            root: root(1),
            frame: 2,
        };
        assert_eq!(s.complete_host_roots(2, &[]).unwrap(), [expected]);
        assert!(
            s.host_note_on(root(HOST_ROOT_CAPACITY as u64 + 1), on(100, 100))
                .is_err()
        );
        assert!(s.acknowledge_host_completions(&[root(2)]).is_err());
        assert_eq!(s.current_frame(), 1);
        s.input(Input {
            frame: 3,
            kind: InputKind::Controller {
                channel: 0,
                controller: 1,
                value: 1,
            },
        })
        .unwrap();
        let out = s.drain().unwrap();
        assert!(out.command_roots.iter().all(Option::is_none));
        assert_eq!(s.complete_host_roots(4, &[]).unwrap(), [expected]);
        assert_eq!(s.runtime.state.borrow().roots.len(), HOST_ROOT_CAPACITY);
        s.acknowledge_host_completions(&[root(1)]).unwrap();
        assert!(s.complete_host_roots(4, &[]).unwrap().is_empty());
        s.host_note_on(root(HOST_ROOT_CAPACITY as u64 + 1), on(4, 100))
            .unwrap();
        assert_eq!(s.runtime.state.borrow().roots.len(), HOST_ROOT_CAPACITY);
    }
}
