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
    processor: Option<NodeId>,
    layer: Option<NodeId>,
}

struct Forward {
    event: Table,
    action: Action,
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
    current_processor: Option<NodeId>,
    current_layer: Option<NodeId>,
    chain: Option<EventChain>,
    forwards: BTreeMap<(u64, u64), Forward>,
    initializing_chain: bool,
    tasks: BTreeMap<(u64, u64), Task>,
    voices: HashMap<u32, Voice>,
    next_trigger: u64,
    triggers: HashMap<u64, HeldTrigger>,
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
        self.post(lua, &event, 0.)?;
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
                let velocity = event.get::<Option<u8>>("velocity")?.unwrap_or(100);
                let channel = event.get::<Option<u8>>("channel")?.unwrap_or(1);
                let volume = event.get::<Option<f32>>("vol")?.unwrap_or(1.);
                let pan = event.get::<Option<f32>>("pan")?.unwrap_or(0.);
                let tune = event.get::<Option<f64>>("tune")?.unwrap_or(0.);
                if note > 127
                    || !(1..=127).contains(&velocity)
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
    let previous_processor = state.borrow().current_processor;
    let previous_layer = state.borrow().current_layer;
    state.borrow_mut().current = task.parent;
    state.borrow_mut().current_processor = task.processor;
    state.borrow_mut().current_layer = task.layer;
    let result = task.thread.resume::<MultiValue>(task.args);
    state.borrow_mut().current = previous;
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
    resumes: Rc<Cell<usize>>,
    depth: Rc<Cell<usize>>,
    host: Option<host::Host>,
    environments: BTreeMap<NodeId, Table>,
    gc_drains: u8,
}

impl Runtime {
    fn new_chain(
        program: &Program,
        modules: BTreeMap<String, Vec<u8>>,
        resources: Option<host::Resources>,
        sample_rate: u32,
    ) -> Result<Self> {
        let mut rt = Self::vm(Some(program), modules, resources, sample_rate)?;
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
            rt.fuel.set(FUEL);
            rt.lua
                .load(source)
                .set_name(format!("UVI ScriptProcessor node {processor}"))
                .set_environment(environment.clone())
                .exec()
                .context("UVI scoped Lua initialization")?;
            ensure!(rt.fuel.get() > 0, "UVI instruction budget exceeded");
            rt.scope(processor);
            let states = program
                .nodes
                .iter()
                .filter(|n| n.parent == Some(processor) && n.kind == "state")
                .collect::<Vec<_>>();
            ensure!(
                states.len() <= 1,
                "Multiple UVI saved states on one processor"
            );
            if let Some(saved) = states.first() {
                let value: serde_json::Value =
                    serde_json::from_str(&saved.text).context("Invalid UVI saved JSON state")?;
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
            rt.fuel.set(FUEL);
            host::restore_widgets_scoped(&rt.lua, program, processor, &environment)?;
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
                            .receive(&self.lua, event, Some(processor))?;
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
                    if let Some(f) = environment
                        .get::<Option<Function>>("onEvent")?
                        .or(environment.get::<Option<Function>>(callback)?)
                    {
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
                                processor: Some(processor),
                                layer,
                            },
                        )?;
                        continue;
                    }
                }
                self.deliver(event, Some(processor), layer, parent, selected)?;
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
                    state.release_note(now, id, note, channel - 1, Some(layer))?;
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
                state.current = parent;
                let result = state.post(&self.lua, &event, 0.);
                state.current = previous;
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
        let lua = Lua::new_with(
            StdLib::TABLE | StdLib::STRING | StdLib::MATH,
            LuaOptions::default(),
        )?;
        lua.set_memory_limit(32 << 20)?;
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
                    Err(mlua::Error::runtime(format!(
                        "UVI instruction budget exceeded at Lua line {:?}",
                        hook_line.get()
                    )))
                } else {
                    Ok(VmState::Continue)
                }
            },
        )?;
        let state = Rc::new(RefCell::new(State {
            tempo: TEMPO,
            sample_rate,
            program_layers: program.map(|p| p.layers.clone()),
            ..State::default()
        }));
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
        globals.set(
            "_check",
            lua.create_function(move |_, ()| {
                if check_fuel.get() == 0 {
                    Err(mlua::Error::runtime(format!(
                        "UVI instruction budget exceeded at Lua line {:?}",
                        check_line.get()
                    )))
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
                        events.raw_set(id, posted)?;
                    } else {
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
                state.schedule(
                    now,
                    Task {
                        thread: lua.create_thread(f)?,
                        args,
                        parent: None,
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
                resume_task(
                    Task {
                        thread: lua.create_thread(f)?,
                        args,
                        parent: None,
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
        Ok(Self {
            lua,
            state,
            fuel,
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
            rt.fuel.set(FUEL);
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
        state.schedule(
            now,
            Task {
                thread,
                args,
                parent,
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
            self.fuel.set(FUEL);
            if let Some(task) = task {
                resume_task(task, &self.state, &self.fuel, &self.resumes, &self.depth)
                    .context("UVI musical callback")?;
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
                self.deliver(&event, forward.after, forward.layer, parent, selected)?;
            }
        }
        let mut state = self.state.borrow_mut();
        state.current = None;
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
                let (_, events) = scope?;
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
                let (_, events) = scope?;
                let retired = events
                    .clone()
                    .pairs::<u32, Table>()
                    .map(|entry| entry.map(|(id, _)| id))
                    .collect::<mlua::Result<Vec<_>>>()?;
                for id in retired.into_iter().filter(|id| !retained.contains(id)) {
                    events.raw_set(id, Value::Nil)?;
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
        Ok(())
    }

    fn input(&mut self, input: Input) -> Result<()> {
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
                    let Some(id) = s
                        .keys
                        .get_mut(&(channel, note))
                        .and_then(VecDeque::pop_front)
                    else {
                        return Ok(());
                    };
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
            self.deliver(&event, None, None, parent, None)?;
            return self.advance(input.frame);
        }
        let parent = self.state.borrow_mut().receive(&self.lua, &event, None)?;
        let globals = self.lua.globals();
        let handler = globals
            .get::<Option<Function>>("onEvent")?
            .or(globals.get::<Option<Function>>(callback)?);
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
}

impl Session {
    pub fn new_program_chain(
        program: &Program,
        modules: BTreeMap<String, Vec<u8>>,
        resources: Option<host::Resources>,
        sample_rate: u32,
    ) -> Result<Self> {
        Ok(Self {
            runtime: Runtime::new_chain(program, modules, resources, sample_rate)?,
        })
    }

    pub fn sample_rate(&self) -> u32 {
        self.runtime.state.borrow().sample_rate
    }
    pub fn current_frame(&self) -> u64 {
        self.runtime.state.borrow().now
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
        let (mut commands, future): (Vec<_>, Vec<_>) = std::mem::take(&mut state.commands)
            .into_iter()
            .partition(|command| command.frame <= until);
        state.commands = future;
        commands.retain(|command| !matches!(&command.action, Action::Start(note) if state.voices.get(&note.id).is_some_and(|v| v.canceled)));
        commands.sort_by_key(|command| command.frame);
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
        commands,
        host_commands,
        logs: std::mem::take(&mut state.logs),
        dropped_logs: state.dropped_logs,
    })
}

#[cfg(test)]
mod tests {
    use super::super::program::parse_program;
    use super::*;

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
}
