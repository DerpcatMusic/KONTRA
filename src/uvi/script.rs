//! Lua 5.1 musical callbacks compiled to an offline engine-command stream.
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
    Function, HookTriggers, Lua, LuaOptions, MultiValue, StdLib, Table, Thread, Value, VmState,
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
    ReleaseLayer {
        id: u32,
        layer: NodeId,
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

fn frames(ms: f64) -> mlua::Result<u64> {
    if !ms.is_finite() || !(0. ..=60_000.).contains(&ms) {
        return Err(mlua::Error::runtime(
            "UVI time must be finite and between 0 and 60000 ms",
        ));
    }
    Ok((ms * RATE / 1000.).round() as u64)
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
        let (on, off) = (frames(on.parse()?)?, frames(off.parse()?)?);
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
    parent: Option<u32>,
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

struct Voice {
    note: Note,
    parent: Option<u32>,
    start: u64,
    end: Option<u64>,
    kill_at: Option<u64>,
    released: bool,
    canceled: bool,
}

#[derive(Default)]
struct State {
    now: u64,
    program_layers: Option<Vec<NodeId>>,
    tempo: f64,
    beat: f64,
    running_beat: f64,
    playing: bool,
    serial: u64,
    next_id: u32,
    current: Option<u32>,
    current_processor: Option<NodeId>,
    current_layer: Option<NodeId>,
    chain: Option<EventChain>,
    forwards: BTreeMap<(u64, u64), Forward>,
    initializing_chain: bool,
    tasks: BTreeMap<(u64, u64), Task>,
    voices: HashMap<u32, Voice>,
    held: HashSet<u32>,
    input_velocities: HashMap<u32, u8>,
    keys: HashMap<(u8, u8), VecDeque<u32>>,
    ccs: HashMap<u8, u8>,
    commands: Vec<Command>,
    logs: Vec<String>,
    log_bytes: usize,
    dropped_logs: usize,
}

impl State {
    fn note_held(&self, id: u32) -> bool {
        if self.input_velocities.contains_key(&id) {
            return self.held.contains(&id);
        }
        self.voices.get(&id).is_some_and(|voice| {
            !voice.released
                && !voice.canceled
                && voice.start <= self.now
                && voice.end.is_none_or(|end| self.now < end)
                && voice.kill_at.is_none_or(|end| self.now < end)
        })
    }
    fn set_time(&mut self, at: u64) {
        let beats = (at - self.now) as f64 / RATE * self.tempo / 60.;
        self.running_beat += beats;
        if self.playing {
            self.beat += beats;
        }
        self.now = at;
    }

    fn id(&mut self) -> mlua::Result<u32> {
        if self.next_id as usize >= LIMIT {
            return Err(mlua::Error::runtime("UVI event limit exceeded"));
        }
        self.next_id += 1;
        Ok(self.next_id)
    }

    fn emit(&mut self, frame: u64, action: Action) -> mlua::Result<()> {
        if self.commands.len() >= LIMIT {
            return Err(mlua::Error::runtime("UVI output command limit exceeded"));
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
        self.serial += 1;
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
        self.serial += 1;
        self.tasks.insert((frame, self.serial), task);
        Ok(())
    }

    fn release(&mut self, id: u32) -> mlua::Result<bool> {
        let now = self.now;
        let Some(voice) = self.voices.get_mut(&id).filter(|v| {
            !v.released
                && v.end.is_none_or(|end| now < end)
                && v.kill_at.is_none_or(|end| now < end)
        }) else {
            return Ok(false);
        };
        voice.released = true;
        voice.canceled = voice.start > self.now;
        self.emit(self.now, Action::Release(id))?;
        Ok(true)
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
        let duration_frames = frames(duration)?;
        let layer = match selector {
            Value::Nil | Value::Integer(0) => None,
            Value::Number(0.) => None,
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
            voice.kill_at = kill.then_some(self.now + duration_frames);
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

    fn post(&mut self, event: &Table, delta: f64) -> mlua::Result<Option<u32>> {
        let at = self.now + frames(delta)?;
        match event.get::<u32>("type")? {
            144 => {
                let raw_id = event.get::<Option<u32>>("id")?.unwrap_or(0);
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
                    offset_us: 0,
                };
                let follows = event.get::<Option<bool>>("_follows")?.unwrap_or(false);
                let previous = self.voices.get(&id);
                let previous_end = previous.and_then(|v| v.end);
                let newly_posted = previous.is_none();
                let parent = previous.and_then(|v| v.parent).or_else(|| {
                    follows
                        .then_some(self.current)
                        .flatten()
                        .filter(|&parent| parent != id)
                });
                let canceled = previous.is_some_and(|v| v.canceled)
                    || parent.is_some_and(|p| !self.note_held(p));
                let end = event
                    .get::<Option<f64>>("_duration")?
                    .map(frames)
                    .transpose()?
                    .map(|duration| at + duration)
                    .or(previous_end);
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
                    },
                );
                event.set("id", id)?;
                event.set("velocity", note.velocity)?;
                event.set("pan", note.pan)?;
                event.set("vol", note.volume)?;
                event.set("tune", note.tune)?;
                self.emit_event(at, Action::Start(note), event)?;
                if let Some(end) = end.filter(|_| newly_posted) {
                    self.emit_event(end, Action::Release(id), event)?;
                }
                Ok(Some(id))
            }
            128 => {
                let id = event.get::<u32>("id")?;
                if self.chain.is_some() {
                    self.emit_event(at, Action::Release(id), event)?;
                } else if at == self.now {
                    self.release(id)?;
                } else {
                    if let Some(voice) = self.voices.get_mut(&id) {
                        voice.end = Some(voice.end.map_or(at, |end| end.min(at)));
                    }
                    self.emit_event(at, Action::Release(id), event)?;
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
            Some(Value::Number(ms)) => frames(*ms)?,
            Some(Value::Integer(ms)) => frames(*ms as f64)?,
            _ => return Err(mlua::Error::runtime("UVI wait requires milliseconds")),
        };
        let mut s = state.borrow_mut();
        let wake = s.now + delay.max(1);
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
}

impl Runtime {
    fn new_chain(
        program: &Program,
        modules: BTreeMap<String, Vec<u8>>,
        resources: Option<host::Resources>,
    ) -> Result<Self> {
        let mut rt = Self::vm(Some(program), modules, resources)?;
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
        parent: Option<u32>,
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
                let event = event_snapshot(&self.lua, event)?;
                if event.get::<u32>("type")? == 128
                    && let Some(layer) = layer
                {
                    let id = event.get::<u32>("id")?;
                    let mut state = self.state.borrow_mut();
                    let now = state.now;
                    state.emit(now, Action::ReleaseLayer { id, layer })?;
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
                let result = state.post(&event, 0.);
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
        let mut rt = Self::vm(program, modules, resources)?;
        rt.initialize(source, name, program)?;
        Ok(rt)
    }

    fn vm(
        program: Option<&Program>,
        modules: BTreeMap<String, Vec<u8>>,
        resources: Option<host::Resources>,
    ) -> Result<Self> {
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
                let id = host.borrow_mut().post(&snapshot, delta)?;
                if let Some(id) = id {
                    e.set("id", id)?;
                }
                Ok(id)
            })?,
        )?;
        let host = state.clone();
        globals.set(
            "_release",
            lua.create_function(move |_, id: u32| host.borrow_mut().release(id))?,
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
            lua.create_function(move |_, ()| Ok(host.borrow().now as f64 * 1000. / RATE))?,
        )?;
        let clock = state.clone();
        globals.set(
            "getTempo",
            lua.create_function(move |_, ()| Ok(clock.borrow().tempo))?,
        )?;
        globals.set("getSamplingRate", lua.create_function(|_, ()| Ok(RATE))?)?;
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
                    .any(|((_, n), ids)| *n == note && !ids.is_empty())
                    || state
                        .voices
                        .iter()
                        .any(|(&id, voice)| voice.note.note == note && state.note_held(id)))
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
            lua.create_function(move |_, (id, ms): (u32, f64)| {
                let offset_us = frames(ms)? * 1_000_000 / RATE as u64;
                let mut s = host.borrow_mut();
                let now = s.now;
                let voice = s
                    .voices
                    .get_mut(&id)
                    .ok_or_else(|| mlua::Error::runtime("Unknown UVI voice"))?;
                if voice.start < now {
                    return Err(mlua::Error::runtime(
                        "Sample offset must be set before the voice starts",
                    ));
                }
                voice.note.offset_us = offset_us;
                for command in &mut s.commands {
                    if let Action::Start(note) = &mut command.action
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
            globals.set(name, lua.create_function(move |_, (id, value, relative, immediate): (u32, f64, Option<bool>, Option<bool>)| {
                if !value.is_finite() || immediate != Some(true) { return Err(mlua::Error::runtime("Nonfinite change or unsupported smoothed UVI voice change (use immediate=true)")); }
                let mut s = host.borrow_mut();
                let voice = s.voices.get(&id).ok_or_else(|| mlua::Error::runtime("Unknown UVI voice"))?;
                let start = voice.start;
                let mut n = voice.note.clone();
                let (mut gain, mut tune, mut pan) = (None, None, None);
                match field {
                    0 => { n.volume = if relative.unwrap_or(false) { n.volume * value as f32 } else { value as f32 }; gain = Some(n.volume); },
                    1 => { n.tune = if relative.unwrap_or(false) { n.tune + value } else { value }; tune = Some(n.tune); },
                    _ => { n.pan = if relative.unwrap_or(false) { n.pan + value as f32 } else { value as f32 }; pan = Some(n.pan); },
                }
                if !n.volume.is_finite() || n.volume < 0. || !(-1. ..=1.).contains(&n.pan) || n.tune.abs() > 120. { return Err(mlua::Error::runtime("UVI voice change exceeds playback range")); }
                let now = s.now;
                s.voices.get_mut(&id).unwrap().note = n.clone();
                if start > now {
                    for command in &mut s.commands { if let Action::Start(note) = &mut command.action && note.id == id { *note = n.clone(); } }
                    Ok(())
                } else { s.emit(now, Action::Change { id, gain, tune, pan }) }
            })?)?;
        }
        let fades = state.clone();
        globals.set(
            "fadeout",
            lua.create_function(
                move |_,
                      (id, ms, kill, reset, layer): (
                    u32,
                    f64,
                    Option<bool>,
                    Option<bool>,
                    Option<Value>,
                )| {
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
                move |_, (id, ms, reset, layer): (u32, f64, Option<bool>, Option<Value>)| {
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
                move |_, (id, target, ms, layer): (u32, f64, f64, Option<Value>)| {
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
                move |_, (id, start, target, ms, layer): (u32, f64, f64, f64, Option<Value>)| {
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
            Some(host::install(
                &lua,
                host::HostConfig {
                    program,
                    modules,
                    resources,
                    now: Rc::new(move || clock.borrow().now),
                    valid_voice: Some(Rc::new(move |id| id > 0 && id <= voices.borrow().next_id)),
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

    fn spawn(&mut self, f: Function, args: MultiValue, parent: Option<u32>) -> Result<()> {
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
                        event.set("id", note.id)?;
                        event.set("pan", note.pan)?;
                        event.set("vol", note.volume)?;
                        event.set("tune", note.tune)?;
                        Some(note.id)
                    }
                    Action::Release(id) => {
                        event.set("type", 128)?;
                        event.set("id", *id)?;
                        Some(*id)
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

    fn input(&mut self, input: Input) -> Result<()> {
        self.advance(input.frame)?;
        if let InputKind::Transport {
            playing,
            beat,
            tempo,
        } = input.kind
        {
            ensure!(
                beat.is_finite() && tempo.is_finite() && (1. ..=1000.).contains(&tempo),
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
                    let id = s.id()?;
                    s.held.insert(id);
                    s.input_velocities.insert(id, velocity);
                    s.keys.entry((channel, note)).or_default().push_back(id);
                    event.set("type", 144)?;
                    event.set("id", id)?;
                    event.set("note", note)?;
                    event.set("velocity", velocity)?;
                    event.set("channel", channel + 1)?;
                    event.set("vol", 1.)?;
                    event.set("pan", 0.)?;
                    event.set("tune", 0.)?;
                    ("onNote", Some(id))
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
                    s.held.remove(&id);
                    let children: Vec<_> = s
                        .voices
                        .iter()
                        .filter(|(_, v)| v.parent == Some(id))
                        .map(|(&id, _)| id)
                        .collect();
                    for child in children {
                        s.release(child)?;
                    }
                    event.set("type", 128)?;
                    event.set("id", id)?;
                    event.set("note", note)?;
                    event.set("velocity", s.input_velocities[&id])?;
                    event.set("channel", channel + 1)?;
                    event.set("vol", 1.)?;
                    event.set("pan", 0.)?;
                    event.set("tune", 0.)?;
                    ("onRelease", Some(id))
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
        let globals = self.lua.globals();
        let handler = globals
            .get::<Option<Function>>("onEvent")?
            .or(globals.get::<Option<Function>>(callback)?);
        if let Some(f) = handler {
            self.spawn(f, MultiValue::from_vec(vec![Value::Table(event)]), parent)?;
        } else {
            self.state.borrow_mut().post(&event, 0.)?;
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
    validate_inputs(inputs, until)?;
    collect(
        Runtime::new_chain(program, modules, resources)?,
        inputs,
        until,
    )
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
    ensure!(
        until <= 48000 * 60,
        "UVI offline timeline exceeds 60 seconds"
    );
    ensure!(
        inputs.len() <= LIMIT && inputs.windows(2).all(|w| w[0].frame <= w[1].frame),
        "UVI inputs must be bounded and sorted"
    );
    ensure!(
        inputs.last().is_none_or(|i| i.frame <= until),
        "UVI input exceeds render end"
    );
    for input in inputs {
        let valid = match input.kind {
            InputKind::NoteOn {
                channel,
                note,
                velocity,
            } => channel < 16 && note <= 127 && velocity <= 127,
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
        };
        ensure!(
            valid,
            "Invalid UVI MIDI/transport input at frame {}",
            input.frame
        );
    }
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
        assert!(
            result
                .commands
                .iter()
                .any(|c| c.frame == 480 && matches!(c.action, Action::Release(3)))
        );
        assert!(
            !result
                .commands
                .iter()
                .any(|c| c.frame < 2400 && matches!(c.action, Action::Release(2 | 4)))
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
