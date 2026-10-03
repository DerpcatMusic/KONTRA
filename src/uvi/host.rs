//! Offline Falcon object/UI state. No VM or GUI work belongs on the audio thread.
//!
//! Parameter and widget signatures: https://lua.uvi.net/class_element.html,
//! https://lua.uvi.net/class_unit.html, https://lua.uvi.net/class_table.html,
//! https://lua.uvi.net/group___voice.html and https://lua.uvi.net/group___async.html.
//! XML field names were observed in privately held presets; no preset source is
//! included here. Retaining a command does not establish DSP support for it.
//! Table boundary behavior was measured against official UVI Workstation 4.0.9 using
//! original synthetic probes (ignored writes/default reads outside 1..N).
//! Original native probes also establish Unit enum values and that parameter
//! writes with a mismatched scalar type are ignored without conversion.
//! Numeric widgets retain float32 values, truncate integers, allow programmatic
//! values beyond their display range, and notify only when a value changes.
//! The exposed Lua API revision (23) was measured with an original UVI Workstation 4.0.9
//! probe; unknown operations still fail explicitly instead of claiming support.

use super::program::{NodeId, Program};
use mlua::{
    AnyUserData, Function, Lua, MetaMethod, MultiValue, Table, UserData, UserDataMethods, Value,
};
use serde::Serialize;
use std::{
    cell::{Cell, RefCell},
    collections::{BTreeMap, HashMap, HashSet},
    rc::Rc,
};

const LIMIT: usize = 65_536;
const SOURCE_LIMIT: usize = 2 << 20;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub enum ParameterValue {
    Number(f64),
    Boolean(bool),
    Text(String),
}

impl ParameterValue {
    fn from_lua(value: Value) -> mlua::Result<Self> {
        match value {
            Value::Number(n) if n.is_finite() => Ok(Self::Number(n)),
            Value::Integer(n) => Ok(Self::Number(n as f64)),
            Value::Boolean(b) => Ok(Self::Boolean(b)),
            Value::String(s) => Ok(Self::Text(s.to_str()?.to_owned())),
            _ => Err(mlua::Error::runtime(
                "UVI parameters require finite numbers, booleans or strings",
            )),
        }
    }

    fn to_lua(&self, lua: &Lua) -> mlua::Result<Value> {
        Ok(match self {
            Self::Number(n) => Value::Number(*n),
            Self::Boolean(b) => Value::Boolean(*b),
            Self::Text(s) => Value::String(lua.create_string(s)?),
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub enum ResourceKind {
    Sample,
    Impulse,
}

#[derive(Debug, Clone)]
pub struct ResourceInfo {
    pub name: String,
    pub rate: u32,
    pub channels: usize,
    pub frames: usize,
}

/// Requests go only to an explicitly approved Library/private-state facility.
#[derive(Debug, Clone)]
pub enum ResourceRequest {
    ReadAudio {
        kind: ResourceKind,
        path: String,
    },
    ReadData {
        path: String,
    },
    ReadState {
        path: String,
    },
    WriteState {
        path: String,
        bytes: Vec<u8>,
    },
    Browse {
        mode: String,
        title: String,
        initial: String,
        patterns: String,
    },
}

#[derive(Debug, Clone)]
pub enum ResourceResponse {
    /// The caller must decode and retain the actual audio before returning this.
    Audio(ResourceInfo),
    Bytes(Vec<u8>),
    Saved,
    Selected(Option<String>),
}

pub type Resources = Rc<dyn Fn(&ResourceRequest) -> mlua::Result<ResourceResponse>>;

#[derive(Debug, Clone, Serialize)]
pub enum Action {
    Parameter {
        node: NodeId,
        parameter: String,
        value: ParameterValue,
    },
    ScriptModulation {
        id: u8,
        start: Option<f64>,
        target: f64,
        ramp_ms: f64,
        voice: Option<u32>,
    },
    LoadResource {
        node: NodeId,
        kind: ResourceKind,
        path: String,
    },
}

#[derive(Debug, Clone, Serialize)]
pub struct Command {
    pub frame: u64,
    pub action: Action,
}

pub struct HostConfig<'a> {
    pub program: Option<&'a Program>,
    /// Only sources resolved from approved resources by the caller. No disk search.
    pub modules: BTreeMap<String, Vec<u8>>,
    pub now: Rc<dyn Fn() -> u64>,
    pub resources: Option<Resources>,
    /// Runtime-issued IDs, including voices posted for a future frame.
    pub valid_voice: Option<Rc<dyn Fn(u32) -> bool>>,
}

#[derive(Clone)]
pub struct Host {
    pub commands: Rc<RefCell<Vec<Command>>>,
    pub parameters: Rc<RefCell<Vec<BTreeMap<String, ParameterValue>>>>,
    objects: Table,
    identities: Rc<RefCell<HashMap<usize, NodeId>>>,
    types: Rc<Vec<String>>,
    modules: Rc<BTreeMap<String, Vec<u8>>>,
    resources: Option<Resources>,
    now: Rc<dyn Fn() -> u64>,
    task_ids: Rc<Cell<u32>>,
}

impl Host {
    /// Validate actual Lua table identity rather than a mutable script field.
    pub fn object_id(&self, object: &Table) -> mlua::Result<NodeId> {
        node_id(object, &self.identities)
    }

    /// Run every processor in its own environment while sharing one engine graph.
    /// The runtime's base globals must contain only common APIs, not user chunks.
    pub fn script_environment(
        &self,
        lua: &Lua,
        program: &Program,
        processor: NodeId,
    ) -> mlua::Result<Table> {
        if self.types.get(processor).map(String::as_str) != Some("ScriptProcessor")
            || program.nodes.get(processor).map(|n| n.kind.as_str()) != Some("ScriptProcessor")
        {
            return Err(mlua::Error::runtime("Invalid UVI ScriptProcessor scope"));
        }
        let environment = lua.create_table()?;
        let metatable = lua.create_table()?;
        metatable.set("__index", lua.globals())?;
        environment.set_metatable(Some(metatable))?;
        environment.set("_G", environment.clone())?;
        // Standard-library tables are mutable globals too; keep helper additions
        // local to their processor while reusing the native library functions.
        for name in ["table", "string", "math", "Event"] {
            if let Some(source) = lua.globals().get::<Option<Table>>(name)? {
                let local = lua.create_table()?;
                for pair in source.pairs::<Value, Value>() {
                    let (key, value) = pair?;
                    local.raw_set(key, value)?;
                }
                environment.set(name, local)?;
            }
        }
        environment.set("this", self.objects.raw_get::<Table>(processor + 1)?)?;
        environment.set("Program", self.objects.raw_get::<Table>(program.root + 1)?)?;
        install_class(lua, &environment)?;
        install_modules(lua, self.modules.clone(), &environment)?;
        install_resources(
            lua,
            self,
            self.now.clone(),
            self.resources.clone(),
            &environment,
        )?;
        install_ui(lua, &environment)?;
        Ok(environment)
    }
}

fn emit(
    commands: &RefCell<Vec<Command>>,
    now: &dyn Fn() -> u64,
    action: Action,
) -> mlua::Result<()> {
    let mut commands = commands.borrow_mut();
    if commands.len() >= LIMIT {
        return Err(mlua::Error::runtime("UVI host command limit exceeded"));
    }
    commands.push(Command {
        frame: now(),
        action,
    });
    Ok(())
}

fn attribute(name: &str, value: &str) -> ParameterValue {
    if matches!(
        name,
        "Name" | "DisplayName" | "SamplePath" | "OutputName" | "Source" | "Destination" | "Mapper"
    ) || name.ends_with("Path")
    {
        return ParameterValue::Text(value.to_owned());
    }
    // XML encodes these documented Boolean parameters as 0/1.
    let boolean = matches!(
        name,
        "Bypass"
            | "BypassInsertFX"
            | "Bipolar"
            | "Inverted"
            | "SyncToHost"
            | "NormalizePower"
            | "Enabled"
            | "Reverse"
            | "SamplePurged"
            | "AllowStreaming"
            | "Streaming"
            | "Mute"
            | "Solo"
            | "MidiMute"
            | "PreFader"
            | "PreInsert"
    ) || name
        .strip_prefix("Enabled")
        .is_some_and(|s| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()));
    if boolean && matches!(value, "0" | "1") {
        return ParameterValue::Boolean(value == "1");
    }
    value
        .parse::<f64>()
        .ok()
        .filter(|n| n.is_finite())
        .map_or_else(
            || ParameterValue::Text(value.to_owned()),
            ParameterValue::Number,
        )
}

fn collections() -> [(&'static str, &'static str); 10] {
    [
        ("layers", "Layers"),
        ("keygroups", "Keygroups"),
        ("oscillators", "Oscillators"),
        ("inserts", "Inserts"),
        ("auxs", "Auxs"),
        ("modulations", "ControlSignalSources"),
        ("eventProcessors", "EventProcessors"),
        ("sends", "BusRouters"),
        ("mappers", "Mappers"),
        ("chains", "Chains"),
    ]
}

/// Must be installed before running the instrument's source.
pub fn install(lua: &Lua, config: HostConfig<'_>) -> mlua::Result<Host> {
    let HostConfig {
        program,
        modules,
        now,
        resources,
        valid_voice,
    } = config;
    lua.globals().set("__API_VERSION__", 23)?;
    let state = Rc::new(RefCell::new(program.map_or_else(Vec::new, |p| {
        p.nodes
            .iter()
            .map(|n| {
                n.attributes
                    .iter()
                    .map(|(k, v)| (k.clone(), attribute(k, v)))
                    .collect()
            })
            .collect()
    })));
    let host = Host {
        parameters: state.clone(),
        commands: Rc::new(RefCell::new(Vec::new())),
        objects: lua.create_table()?,
        identities: Rc::new(RefCell::new(HashMap::new())),
        types: Rc::new(program.map_or_else(Vec::new, |p| {
            p.nodes
                .iter()
                .map(|n| n.kind.clone())
                .chain(["Part".to_owned(), "Synth".to_owned()])
                .collect()
        })),
        modules: Rc::new(modules),
        resources: resources.clone(),
        now: now.clone(),
        task_ids: Rc::new(Cell::new(0)),
    };
    if let Some(program) = program {
        // One Lua-managed inventory retains every node, including XML wrappers.
        // Keeping one Rust Table per node exhausts MLua's auxiliary reference stack.
        let objects = host.objects.clone();
        for id in 0..program.nodes.len() {
            let object = lua.create_table()?;
            host.identities
                .borrow_mut()
                .insert(object.to_pointer() as usize, id);
            objects.raw_set(id + 1, object)?;
        }
        // Parent/children refer to semantic nodes, skipping XML collection wrappers.
        let wrappers = collections().map(|(_, xml)| xml);
        let mut children_by_node = vec![Vec::new(); program.nodes.len()];
        for (id, node) in program.nodes.iter().enumerate() {
            if let Some(parent) = node.parent {
                children_by_node[parent].push(id);
            }
        }
        let mut connections_by_owner = vec![Vec::new(); program.nodes.len()];
        for connection in &program.connections {
            connections_by_owner[connection.owner]
                .push((connection.destination.clone(), connection.node));
        }
        let methods = lua.create_table()?;
        let params = state.clone();
        let ids = host.identities.clone();
        methods.set(
            "getParameter",
            lua.create_function(move |lua, (object, name): (Table, String)| {
                let id = node_id(&object, &ids)?;
                params.borrow()[id]
                    .get(&name)
                    .ok_or_else(|| {
                        mlua::Error::runtime(format!(
                            "Unknown or unretained UVI parameter {name} on node {id}"
                        ))
                    })?
                    .to_lua(lua)
            })?,
        )?;
        let params = state.clone();
        let ids = host.identities.clone();
        methods.set(
            "hasParameter",
            lua.create_function(move |_, (object, name): (Table, String)| {
                let id = node_id(&object, &ids)?;
                Ok(params.borrow()[id].contains_key(&name))
            })?,
        )?;
        let params = state.clone();
        let commands = host.commands.clone();
        let clock = now.clone();
        let ids = host.identities.clone();
        methods.set(
            "setParameter",
            lua.create_function(move |_, (object, name, value): (Table, String, Value)| {
                let id = node_id(&object, &ids)?;
                let value = ParameterValue::from_lua(value)?;
                let mut params = params.borrow_mut();
                let old = params[id].get(&name).ok_or_else(|| {
                    mlua::Error::runtime(format!(
                        "Unknown or unretained UVI parameter {name} on node {id}"
                    ))
                })?;
                if std::mem::discriminant(old) != std::mem::discriminant(&value) {
                    return Ok(());
                }
                emit(
                    &commands,
                    &*clock,
                    Action::Parameter {
                        node: id,
                        parameter: name.clone(),
                        value: value.clone(),
                    },
                )?;
                params[id].insert(name, value);
                Ok(())
            })?,
        )?;
        let targets = connections_by_owner;
        let connection_objects = objects.clone();
        let ids = host.identities.clone();
        methods.set(
            "getParameterConnections",
            lua.create_function(move |lua, (object, name): (Table, String)| {
                let id = node_id(&object, &ids)?;
                let list = lua.create_table()?;
                for (i, (_, target)) in targets[id]
                    .iter()
                    .filter(|(destination, _)| destination == &name)
                    .enumerate()
                {
                    list.set(i + 1, connection_objects.raw_get::<Table>(*target + 1)?)?;
                }
                Ok(list)
            })?,
        )?;
        let object_metatable = lua.create_table()?;
        object_metatable.set("__index", methods)?;
        for (id, node) in program.nodes.iter().enumerate() {
            let object = objects.raw_get::<Table>(id + 1)?;
            object.set("_nodeId", id)?;
            object.set("type", node.kind.as_str())?;
            object.set("name", node.name.as_deref().unwrap_or(node.kind.as_str()))?;
            object.set(
                "displayName",
                node.attributes
                    .get("DisplayName")
                    .or(node.name.as_ref())
                    .map(String::as_str)
                    .unwrap_or(node.kind.as_str()),
            )?;
            let mut parent = node.parent;
            while parent.is_some_and(|p| {
                wrappers.contains(&program.nodes[p].kind.as_str())
                    || program.nodes[p].kind == "Connections"
            }) {
                parent = parent.and_then(|p| program.nodes[p].parent);
            }
            if let Some(parent) = parent {
                object.set("parent", objects.raw_get::<Table>(parent + 1)?)?;
            }
            let children = lua.create_table()?;
            let synth_children = lua.create_table()?;
            let mut child_index = 0;
            for (field, wrapper) in collections() {
                if !matches!(
                    node.kind.as_str(),
                    "Program" | "Layer" | "Keygroup" | "AuxEffect" | "EffectRack"
                ) && !children_by_node[id]
                    .iter()
                    .any(|&child| program.nodes[child].kind == wrapper)
                {
                    continue;
                }
                let list = lua.create_table()?;
                let named = lua.create_table()?;
                let mut index = 0;
                for &wrapper_id in children_by_node[id].iter().filter(|&&child| {
                    program.nodes[child].kind == wrapper
                        || (field == "auxs"
                            && node.kind == "EffectRack"
                            && program.nodes[child].kind == "Chains")
                }) {
                    for &child_id in &children_by_node[wrapper_id] {
                        let child = &program.nodes[child_id];
                        index += 1;
                        list.set(index, objects.raw_get::<Table>(child_id + 1)?)?;
                        if let Some(name) = &child.name {
                            children.set(name.as_str(), objects.raw_get::<Table>(child_id + 1)?)?;
                            if field == "modulations" {
                                named
                                    .set(name.as_str(), objects.raw_get::<Table>(child_id + 1)?)?;
                            }
                        }
                        if matches!(field, "layers" | "keygroups") {
                            child_index += 1;
                            synth_children
                                .set(child_index, objects.raw_get::<Table>(child_id + 1)?)?;
                        }
                    }
                }
                if field == "modulations" {
                    object.set("mods", list.clone())?;
                    object.set(field, named)?;
                } else {
                    object.set(field, list)?;
                }
            }
            object.set("children", children)?;
            if matches!(node.kind.as_str(), "Program" | "Layer" | "Keygroup") {
                object.set("synthChildren", synth_children)?;
            }
            // Attributes are authoritative. The only omitted default supplied here
            // is the documented common processor Bypass=false.
            if !wrappers.contains(&node.kind.as_str()) && node.kind != "Connections" {
                state.borrow_mut()[id]
                    .entry("Bypass".into())
                    .or_insert(ParameterValue::Boolean(false));
            }
            object.set_metatable(Some(object_metatable.clone()))?;
        }
        install_context(lua, &objects.raw_get::<Table>(program.root + 1)?, &host)?;
        if let Some((id, _)) = program
            .nodes
            .iter()
            .enumerate()
            .find(|(_, n)| n.kind == "ScriptProcessor")
        {
            lua.globals()
                .set("this", objects.raw_get::<Table>(id + 1)?)?;
        }
        let layer_names = program
            .layers
            .iter()
            .map(|&id| {
                program.nodes[id]
                    .attributes
                    .get("DisplayName")
                    .or(program.nodes[id].name.as_ref())
                    .cloned()
            })
            .collect::<Vec<_>>();
        lua.globals().set(
            "findLayer",
            lua.create_function(move |_, name: String| {
                Ok(layer_names
                    .iter()
                    .position(|n| n.as_ref() == Some(&name))
                    .map(|index| index + 1))
            })?,
        )?;
        lua.globals()
            .set("Program", objects.raw_get::<Table>(program.root + 1)?)?;
    }
    install_class(lua, &lua.globals())?;
    install_modules(lua, host.modules.clone(), &lua.globals())?;
    install_modulation(lua, &host, now.clone(), valid_voice)?;
    install_resources(lua, &host, now, resources, &lua.globals())?;
    install_ui(lua, &lua.globals())?;
    Ok(host)
}

fn install_ui(lua: &Lua, environment: &Table) -> mlua::Result<()> {
    environment.set(
        "_uvi_float",
        lua.create_function(|_, value: f64| {
            let value = value as f32;
            if !value.is_finite() {
                return Err(mlua::Error::runtime(
                    "UVI widget value exceeds float32 range",
                ));
            }
            Ok(f64::from(value))
        })?,
    )?;
    lua.load(UI)
        .set_name("UVI offline UI state")
        .set_environment(environment.clone())
        .exec()
}

fn node_id(object: &Table, identities: &RefCell<HashMap<usize, NodeId>>) -> mlua::Result<NodeId> {
    identities
        .borrow()
        .get(&(object.to_pointer() as usize))
        .copied()
        .ok_or_else(|| mlua::Error::runtime("Table is not a UVI Program object"))
}

/// Restore observed ScriptProcessor scalars and ScriptData Table cells after
/// constructors execute. Native restoration visits Tables first, then scalar
/// controls, in constructor order, notifying immediately after each changed cell.
pub fn restore_widgets(lua: &Lua, program: &Program) -> mlua::Result<usize> {
    restore_widgets_in(lua, program, None, &lua.globals())
}

pub fn restore_widgets_scoped(
    lua: &Lua,
    program: &Program,
    processor: NodeId,
    environment: &Table,
) -> mlua::Result<usize> {
    restore_widgets_in(lua, program, Some(processor), environment)
}

fn restore_widgets_in(
    lua: &Lua,
    program: &Program,
    processor: Option<NodeId>,
    environment: &Table,
) -> mlua::Result<usize> {
    let processors = program
        .nodes
        .iter()
        .enumerate()
        .filter(|(id, n)| n.kind == "ScriptProcessor" && processor.is_none_or(|p| p == *id))
        .collect::<Vec<_>>();
    if processors.is_empty() {
        if processor.is_some() {
            return Err(mlua::Error::runtime("Invalid UVI widget scope"));
        }
        return Ok(0);
    }
    if processors.len() != 1 {
        return Err(mlua::Error::runtime(
            "Multiple UVI ScriptProcessor widget namespaces are not supported",
        ));
    }
    let (processor_id, processor) = processors[0];
    let data = program
        .nodes
        .iter()
        .filter(|n| n.parent == Some(processor_id) && n.kind == "ScriptData")
        .collect::<Vec<_>>();
    if data.len() > 1 {
        return Err(mlua::Error::runtime(
            "Multiple UVI ScriptData widget stores are not supported",
        ));
    }
    let ui = environment.get::<Table>("UVI_UI_STATE")?;
    let order = ui.get::<Table>("order")?;
    let mut restored = 0;
    for tables in [true, false] {
        for widget in order.sequence_values::<Table>() {
            let widget = widget?;
            if (widget.get::<String>("kind")? == "Table") != tables {
                continue;
            }
            if !widget.get::<bool>("persistent")? {
                continue;
            }
            let name = widget.get::<String>("name")?;
            let kind = widget.get::<String>("kind")?;
            let set = widget.get::<Function>("setValue")?;
            if kind == "Table" {
                let Some(text) = data.first().and_then(|n| n.attributes.get(&name)) else {
                    continue;
                };
                // Observed Falcon persistence uses a decimal comma inside each
                // whitespace-separated value, without indices or pair delimiters.
                let values = text
                    .split_whitespace()
                    .map(|token| {
                        token
                            .replace(',', ".")
                            .parse::<f64>()
                            .ok()
                            .filter(|n| n.is_finite())
                            .ok_or_else(|| {
                                mlua::Error::runtime(format!(
                                    "Malformed UVI Table persistence for {name}"
                                ))
                            })
                    })
                    .collect::<mlua::Result<Vec<_>>>()?;
                if values.len() != widget.get::<usize>("length")? {
                    return Err(mlua::Error::runtime(format!(
                        "UVI Table persistence length differs for {name}"
                    )));
                }
                for (index, value) in values.into_iter().enumerate() {
                    set.call::<()>((widget.clone(), index + 1, value))?;
                }
                restored += 1;
            } else if let Some(text) = processor.attributes.get(&name) {
                let value = match kind.as_str() {
                    "Button" | "OnOffButton" => match text.as_str() {
                        "0" => Value::Boolean(false),
                        "1" => Value::Boolean(true),
                        _ => {
                            return Err(mlua::Error::runtime(format!(
                                "Malformed UVI Boolean persistence for {name}"
                            )));
                        }
                    },
                    "Knob" | "Slider" | "NumBox" | "Menu" => attribute(&name, text).to_lua(lua)?,
                    _ => continue,
                };
                set.call::<()>((widget.clone(), value))?;
                restored += 1;
            }
        }
    }
    Ok(restored)
}

fn install_context(lua: &Lua, program: &Table, host: &Host) -> mlua::Result<()> {
    // A standalone Program load creates an Omni Part in a fresh Synth. These
    // defaults are playback context, not parent data recovered from the preset.
    let part = lua.create_table()?;
    let synth = lua.create_table()?;
    let base = host.parameters.borrow().len();
    for (offset, object, kind) in [(0, &part, "Part"), (1, &synth, "Synth")] {
        let id = base + offset;
        host.objects.raw_set(id + 1, object.clone())?;
        let mut defaults = BTreeMap::from([
            ("Gain".into(), ParameterValue::Number(1.)),
            ("Pan".into(), ParameterValue::Number(0.)),
            ("Bypass".into(), ParameterValue::Boolean(false)),
        ]);
        if kind == "Part" {
            defaults.insert("MidiChannel".into(), ParameterValue::Number(-1.));
            defaults.insert("MidiInput".into(), ParameterValue::Number(-1.));
        }
        host.parameters.borrow_mut().push(defaults);
        host.identities
            .borrow_mut()
            .insert(object.to_pointer() as usize, id);
        object.set("_nodeId", id)?;
        object.set("type", kind)?;
        object.set("name", kind)?;
        for (field, _) in collections() {
            object.set(field, lua.create_table()?)?;
        }
        object.set("children", lua.create_table()?)?;
        object.set("mods", lua.create_table()?)?;
        object.set("synthChildren", lua.create_table()?)?;
        object.set_metatable(program.metatable())?;
        object.set(
            "getParameterConnections",
            lua.create_function(|lua, (_self, _name): (Table, String)| lua.create_table())?,
        )?;
    }
    program.set("parent", part.clone())?;
    part.set("parent", synth.clone())?;
    part.set("program", program.clone())?;
    part.get::<Table>("synthChildren")?
        .set(1, program.clone())?;
    synth.get::<Table>("synthChildren")?.set(1, part.clone())?;
    lua.globals().set("Part", part)?;
    Ok(())
}

struct ScriptClass;
struct ScriptInstance;

impl UserData for ScriptClass {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        for operator in [MetaMethod::Eq, MetaMethod::ToString] {
            methods.add_meta_function(operator, |_, _: MultiValue| -> mlua::Result<Value> {
                Err(mlua::Error::runtime("Unsupported UVI class operator"))
            });
        }
        methods.add_meta_function(
            MetaMethod::Index,
            |_, (class, key): (AnyUserData, String)| class.user_value::<Table>()?.get::<Value>(key),
        );
        methods.add_meta_function(
            MetaMethod::NewIndex,
            |_, (class, key, value): (AnyUserData, String, Value)| {
                class.user_value::<Table>()?.set(key, value)
            },
        );
        methods.add_meta_function(MetaMethod::Call, |lua, mut args: MultiValue| {
            let Some(Value::UserData(class)) = args.pop_front() else {
                return Err(mlua::Error::runtime("Invalid UVI class constructor"));
            };
            let members = class.user_value::<Table>()?;
            let init = members.get::<Function>("__init")?;
            let instance = lua.create_userdata(ScriptInstance)?;
            let state = lua.create_table()?;
            state.set("members", members)?;
            state.set("fields", lua.create_table()?)?;
            instance.set_user_value(state)?;
            args.push_front(Value::UserData(instance.clone()));
            init.call::<()>(args)?;
            Ok(instance)
        });
    }
}

impl UserData for ScriptInstance {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        for operator in [MetaMethod::Eq, MetaMethod::ToString] {
            methods.add_meta_function(operator, |_, _: MultiValue| -> mlua::Result<Value> {
                Err(mlua::Error::runtime("Unsupported UVI class operator"))
            });
        }
        methods.add_meta_function(
            MetaMethod::Index,
            |_, (instance, key): (AnyUserData, String)| {
                let state = instance.user_value::<Table>()?;
                let value = state.get::<Table>("fields")?.get::<Value>(key.as_str())?;
                if matches!(value, Value::Nil) {
                    state.get::<Table>("members")?.get::<Value>(key)
                } else {
                    Ok(value)
                }
            },
        );
        methods.add_meta_function(
            MetaMethod::NewIndex,
            |_, (instance, key, value): (AnyUserData, String, Value)| {
                instance
                    .user_value::<Table>()?
                    .get::<Table>("fields")?
                    .set(key, value)
            },
        );
    }
}

// Native class() publishes userdata immediately and returns an optional-base
// builder. Inheritance copies existing members but requires its own __init.
fn install_class(lua: &Lua, environment: &Table) -> mlua::Result<()> {
    let scope = environment.clone();
    environment.set(
        "class",
        lua.create_function(move |lua, name: String| {
            if name.is_empty() || name.len() > 256 || name.contains('\0') {
                return Err(mlua::Error::runtime("Invalid UVI class name"));
            }
            let class = lua.create_userdata(ScriptClass)?;
            class.set_user_value(lua.create_table()?)?;
            scope.set(name, class.clone())?;
            lua.create_function(move |_, base: AnyUserData| {
                if !base.is::<ScriptClass>() {
                    return Err(mlua::Error::runtime("Invalid UVI base class"));
                }
                let members = class.user_value::<Table>()?;
                for pair in base.user_value::<Table>()?.pairs::<String, Value>() {
                    let (key, value) = pair?;
                    if key != "__init" {
                        members.set(key, value)?;
                    }
                }
                Ok(())
            })
        })?,
    )
}

struct AsyncUpdaterFactory;
struct AsyncUpdater;

// Native trigger waits in its caller and coalesces while busy, including callback
// reentry. The Lua closure can yield without crossing a Rust method call boundary.
// User values retain callbacks in Lua rather than permanent Rust reference slots.
impl UserData for AsyncUpdaterFactory {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_meta_function(
            MetaMethod::Call,
            |lua, (factory, callback): (AnyUserData, Function)| {
                let environment = factory.user_value::<Table>()?;
                let trigger = lua
                    .load(
                        "local pending=false;return function(self,ms)\
                         if pending then return end;pending=true;wait(ms);\
                         self.callback();pending=false end",
                    )
                    .set_name("UVI offline AsyncUpdater")
                    .set_environment(environment)
                    .eval::<Function>()?;
                let state = lua.create_table()?;
                state.set("callback", callback)?;
                state.set("trigger", trigger)?;
                let updater = lua.create_userdata(AsyncUpdater)?;
                updater.set_user_value(state)?;
                Ok(updater)
            },
        );
    }
}

impl UserData for AsyncUpdater {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_meta_function(
            MetaMethod::Index,
            |_, (updater, key): (AnyUserData, String)| {
                updater.user_value::<Table>()?.get::<Value>(key)
            },
        );
        methods.add_meta_function(
            MetaMethod::NewIndex,
            |_, (updater, key, callback): (AnyUserData, String, Function)| {
                if key != "callback" {
                    return Err(mlua::Error::runtime("Unknown UVI AsyncUpdater property"));
                }
                updater.user_value::<Table>()?.set(key, callback)
            },
        );
    }
}

fn install_modules(
    lua: &Lua,
    modules: Rc<BTreeMap<String, Vec<u8>>>,
    environment: &Table,
) -> mlua::Result<()> {
    let mut total = 0usize;
    for (name, source) in modules.iter() {
        total = total.saturating_add(source.len());
        if name.is_empty()
            || name.len() > 256
            || source.len() > SOURCE_LIMIT
            || total > 16 << 20
            || source.starts_with(b"\x1bLua")
            || std::str::from_utf8(source).is_err()
        {
            return Err(mlua::Error::runtime(
                "Invalid or oversized UVI embedded Lua module",
            ));
        }
    }
    let cache = lua.create_table()?;
    let loading = Rc::new(RefCell::new(HashSet::<String>::new()));
    let module_environment = environment.clone();
    environment.set(
        "require",
        lua.create_function(move |lua, name: String| {
            if name.is_empty() || name.len() > 256 || name.contains('\0') {
                return Err(mlua::Error::runtime("Invalid UVI embedded module name"));
            }
            let cached = cache.get::<Value>(name.as_str())?;
            if !matches!(cached, Value::Nil | Value::Boolean(false)) {
                return Ok(cached);
            }
            if name == "uvi.AsyncUpdater" && !modules.contains_key(&name) {
                let factory = lua.create_userdata(AsyncUpdaterFactory)?;
                factory.set_user_value(module_environment.clone())?;
                module_environment.set("AsyncUpdater", factory)?;
                cache.set(name, true)?;
                return Ok(Value::Boolean(true));
            }
            let source = if let Some(source) = modules.get(&name) {
                source.as_slice()
            } else if name == "uvi.ChordRec" {
                CHORD_REC.as_bytes()
            } else {
                // Embedded processors may omit their resource folder. Resolve only
                // approved bank names, accepting duplicate aliases of the same bytes.
                let relative = name.replace(['/', '\\'], ".");
                let suffix = format!(".{relative}");
                let mut matches = modules
                    .iter()
                    .filter(|(key, _)| key.as_str() == relative || key.ends_with(&suffix));
                let (_, source) = matches.next().ok_or_else(|| {
                    mlua::Error::runtime(format!("UVI embedded module {name:?} is not approved"))
                })?;
                if matches.any(|(_, candidate)| candidate != source) {
                    return Err(mlua::Error::runtime(format!(
                        "UVI embedded module {name:?} is ambiguous"
                    )));
                }
                source.as_slice()
            };
            if !loading.borrow_mut().insert(name.clone()) {
                return Err(mlua::Error::runtime(format!(
                    "UVI module load cycle at {name:?}"
                )));
            }
            let result = lua
                .load(source)
                .set_name(format!("embedded module {name}"))
                .set_environment(module_environment.clone())
                .call::<Value>(name.clone());
            loading.borrow_mut().remove(&name);
            let result = result?;
            let result = if matches!(result, Value::Nil) {
                Value::Boolean(true)
            } else {
                result
            };
            cache.set(name, result.clone())?;
            Ok(result)
        })?,
    )?;
    Ok(())
}

fn install_modulation(
    lua: &Lua,
    host: &Host,
    now: Rc<dyn Fn() -> u64>,
    valid_voice: Option<Rc<dyn Fn(u32) -> bool>>,
) -> mlua::Result<()> {
    for (name, explicit_start) in [
        ("sendScriptModulation", false),
        ("sendScriptModulation2", true),
    ] {
        let commands = host.commands.clone();
        let clock = now.clone();
        let valid_voice = valid_voice.clone();
        lua.globals().set(
            name,
            lua.create_function(move |_, mut args: MultiValue| {
                let id = match args.pop_front() {
                    Some(Value::Integer(n)) if (0..128).contains(&n) => n as u8,
                    Some(Value::Number(n)) if n.fract() == 0. && (0. ..128.).contains(&n) => {
                        n as u8
                    }
                    _ => {
                        return Err(mlua::Error::runtime(
                            "UVI script modulation id must be 0..127",
                        ));
                    }
                };
                let number = |value: Option<Value>, default: Option<f64>| -> mlua::Result<f64> {
                    match value {
                        Some(Value::Number(n)) if n.is_finite() => Ok(n),
                        Some(Value::Integer(n)) => Ok(n as f64),
                        None | Some(Value::Nil) if default.is_some() => Ok(default.unwrap()),
                        _ => Err(mlua::Error::runtime(
                            "UVI modulation requires finite numbers",
                        )),
                    }
                };
                let start = if explicit_start {
                    Some(number(args.pop_front(), None)?)
                } else {
                    None
                };
                let target = number(args.pop_front(), None)?;
                let ramp_ms = number(args.pop_front(), Some(20.))?;
                let voice = match args.pop_front() {
                    None | Some(Value::Nil) => None,
                    Some(value) => Some(super::script::voice_id(value)?),
                };
                if !(-1. ..=1.).contains(&target)
                    || start.is_some_and(|s| !(-1. ..=1.).contains(&s))
                    || !(0. ..=60000.).contains(&ramp_ms)
                    || !args.is_empty()
                {
                    return Err(mlua::Error::runtime(
                        "UVI modulation value/ramp outside supported range",
                    ));
                }
                if voice.is_some_and(|id| !valid_voice.as_ref().is_some_and(|valid| valid(id))) {
                    return Err(mlua::Error::runtime("Unknown UVI modulation voice id"));
                }
                emit(
                    &commands,
                    &*clock,
                    Action::ScriptModulation {
                        id,
                        start,
                        target,
                        ramp_ms,
                        voice,
                    },
                )
            })?,
        )?;
    }
    Ok(())
}

fn resource_path(path: &str) -> mlua::Result<()> {
    if path.is_empty() || path.len() > 4096 || path.contains('\0') {
        return Err(mlua::Error::runtime("Invalid UVI resource path"));
    }
    Ok(())
}

fn resource_read(
    resources: &Option<Resources>,
    request: &ResourceRequest,
) -> mlua::Result<ResourceResponse> {
    resources
        .as_ref()
        .ok_or_else(|| mlua::Error::runtime("UVI resource capability is not configured"))?(
        request
    )
}

fn task(lua: &Lua, ids: &Cell<u32>, error: Option<&mlua::Error>) -> mlua::Result<Table> {
    let id = ids
        .get()
        .checked_add(1)
        .filter(|id| *id <= LIMIT as u32)
        .ok_or_else(|| mlua::Error::runtime("UVI resource task limit exceeded"))?;
    ids.set(id);
    let task = lua.create_table()?;
    task.set("id", id)?;
    task.set("finished", true)?;
    task.set("progress", 1.)?;
    task.set("state", "finished")?;
    task.set("success", error.is_none())?;
    if let Some(error) = error {
        task.set("error", error.to_string())?;
    }
    Ok(task)
}

fn json_data(lua: &Lua, bytes: &[u8]) -> mlua::Result<Value> {
    if bytes.len() > SOURCE_LIMIT {
        return Err(mlua::Error::runtime("UVI resource data exceeds 2 MiB"));
    }
    let value: serde_json::Value = serde_json::from_slice(bytes).map_err(mlua::Error::external)?;
    super::script::saved_value(lua, &value, 0, &mut 0).map_err(mlua::Error::external)
}

/// Native loadState restores controls/callbacks, then onLoad; it does not rerun onInit.
fn read_state(lua: &Lua, bytes: &[u8]) -> mlua::Result<(Program, Option<Value>)> {
    if bytes.len() > SOURCE_LIMIT {
        return Err(mlua::Error::runtime("UVI state exceeds 2 MiB"));
    }
    let source = std::str::from_utf8(bytes).map_err(mlua::Error::external)?;
    let document = roxmltree::Document::parse_with_options(
        source,
        roxmltree::ParsingOptions {
            allow_dtd: false,
            nodes_limit: LIMIT as u32,
            ..Default::default()
        },
    )
    .map_err(mlua::Error::external)?;
    let root = document.root_element();
    let processors = root
        .children()
        .filter(|n| n.is_element())
        .collect::<Vec<_>>();
    if root.tag_name().name() != "UVI4"
        || processors.len() != 1
        || processors[0].tag_name().name() != "ScriptProcessor"
    {
        return Err(mlua::Error::runtime(
            "Unsupported UVI script state document",
        ));
    }
    let processor = processors[0];
    if processor.descendants().filter(|n| n.is_element()).any(|n| {
        !matches!(
            n.tag_name().name(),
            "ScriptProcessor" | "ScriptData" | "state"
        )
    }) {
        return Err(mlua::Error::runtime(
            "UVI script state contains unsupported elements",
        ));
    }
    let states = processor
        .children()
        .filter(|n| n.has_tag_name("state"))
        .collect::<Vec<_>>();
    if states.len() > 1 {
        return Err(mlua::Error::runtime("Multiple UVI script states"));
    }
    let saved = states
        .first()
        .map(|n| json_data(lua, n.text().unwrap_or("").as_bytes()))
        .transpose()?;
    if saved
        .as_ref()
        .is_some_and(|s| !matches!(s, Value::Table(_) | Value::Nil))
    {
        return Err(mlua::Error::runtime(
            "UVI script state must decode to a table or nil",
        ));
    }
    let wrapped = format!("<Program>{}</Program>", &source[processor.range()]);
    let program = super::program::parse_program(&wrapped).map_err(mlua::Error::external)?;
    Ok((program, saved))
}

fn restore_state(
    lua: &Lua,
    program: &Program,
    saved: Option<Value>,
    environment: &Table,
) -> mlua::Result<()> {
    restore_widgets_in(lua, program, None, environment)?;
    if let (Some(saved), Some(callback)) = (saved, environment.get::<Option<Function>>("onLoad")?) {
        callback.call::<()>(saved)?;
    }
    Ok(())
}

fn json_value(
    value: Value,
    depth: usize,
    count: &mut usize,
    parents: &mut HashSet<usize>,
) -> mlua::Result<serde_json::Value> {
    *count += 1;
    if depth > 64 || *count > LIMIT {
        return Err(mlua::Error::runtime("UVI state exceeds structure limit"));
    }
    Ok(match value {
        Value::Nil => serde_json::Value::Null,
        Value::Boolean(b) => b.into(),
        Value::Integer(n) => n.into(),
        Value::Number(n) => serde_json::Number::from_f64(n)
            .map(serde_json::Value::Number)
            .ok_or_else(|| mlua::Error::runtime("Nonfinite UVI state number"))?,
        Value::String(s) => s.to_str()?.as_ref().into(),
        Value::Table(t) => {
            let id = t.to_pointer() as usize;
            if !parents.insert(id) {
                return Err(mlua::Error::runtime("Circular UVI state table"));
            }
            let mut indices = BTreeMap::new();
            let mut names = serde_json::Map::new();
            for pair in t.pairs::<Value, Value>() {
                let (key, value) = pair?;
                let value = json_value(value, depth + 1, count, parents)?;
                match key {
                    Value::String(s) => {
                        names.insert(s.to_str()?.to_owned(), value);
                    }
                    Value::Integer(n) if n > 0 => {
                        indices.insert(n as usize, value);
                    }
                    Value::Number(n) if n.fract() == 0. && n > 0. && n <= LIMIT as f64 => {
                        indices.insert(n as usize, value);
                    }
                    _ => return Err(mlua::Error::runtime("Unsupported UVI state table key")),
                }
            }
            parents.remove(&id);
            if indices.is_empty() {
                serde_json::Value::Object(names)
            } else if names.is_empty() && indices.keys().copied().eq(1..=indices.len()) {
                serde_json::Value::Array(indices.into_values().collect())
            } else {
                return Err(mlua::Error::runtime(
                    "UVI state table must be a contiguous array or dictionary",
                ));
            }
        }
        _ => return Err(mlua::Error::runtime("Unsupported UVI saved state value")),
    })
}

fn xml_text(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

// Original synthetic native saveState probes establish this document shape.
fn save_state(environment: &Table) -> mlua::Result<Vec<u8>> {
    let mut scalars = String::new();
    let mut tables = String::new();
    let order = environment
        .get::<Table>("UVI_UI_STATE")?
        .get::<Table>("order")?;
    for widget in order.sequence_values::<Table>() {
        let widget = widget?;
        if !widget.get::<bool>("persistent")? {
            continue;
        }
        let name = widget.get::<String>("name")?;
        let kind = widget.get::<String>("kind")?;
        let (destination, value) = if kind == "Table" {
            let get = widget.get::<Function>("getValue")?;
            let values = (1..=widget.get::<usize>("length")?)
                .map(|i| {
                    get.call::<f64>((widget.clone(), i))
                        .map(|v| format!("{v:.6}"))
                })
                .collect::<mlua::Result<Vec<_>>>()?;
            (&mut tables, values.join(" "))
        } else if matches!(
            kind.as_str(),
            "Knob" | "Slider" | "NumBox" | "Menu" | "Button" | "OnOffButton"
        ) {
            let value = match ParameterValue::from_lua(widget.get::<Value>("value")?)? {
                ParameterValue::Number(n) => n.to_string(),
                ParameterValue::Boolean(b) => if b { "1" } else { "0" }.to_owned(),
                ParameterValue::Text(_) => {
                    return Err(mlua::Error::runtime(
                        "Unsupported UVI persistent widget value",
                    ));
                }
            };
            (&mut scalars, value)
        } else {
            continue;
        };
        destination.push_str(&format!(" {}=\"{}\"", name, xml_text(&value)));
        if scalars.len() + tables.len() > SOURCE_LIMIT {
            return Err(mlua::Error::runtime("UVI state exceeds 2 MiB"));
        }
    }
    let saved = if let Some(callback) = environment.get::<Option<Function>>("onSave")? {
        let value = callback.call::<Value>(())?;
        if !matches!(value, Value::Table(_) | Value::Nil) {
            return Err(mlua::Error::runtime(
                "UVI script state saving requires an onSave table or nil",
            ));
        }
        let value = json_value(value, 0, &mut 0, &mut HashSet::new())?;
        format!(
            "<state>{}</state>",
            xml_text(&serde_json::to_string(&value).map_err(mlua::Error::external)?)
        )
    } else {
        String::new()
    };
    let source = format!(
        "<UVI4><ScriptProcessor API_version=\"13\"{scalars}><ScriptData{tables}/>{saved}</ScriptProcessor></UVI4>"
    );
    if source.len() > SOURCE_LIMIT {
        return Err(mlua::Error::runtime("UVI state exceeds 2 MiB"));
    }
    roxmltree::Document::parse(&source).map_err(mlua::Error::external)?;
    Ok(source.into_bytes())
}

fn install_resources(
    lua: &Lua,
    host: &Host,
    now: Rc<dyn Fn() -> u64>,
    resources: Option<Resources>,
    environment: &Table,
) -> mlua::Result<()> {
    let task_ids = host.task_ids.clone();
    let target_types = host.types.clone();
    for (name, kind) in [
        ("loadSample", ResourceKind::Sample),
        ("loadImpulse", ResourceKind::Impulse),
    ] {
        let commands = host.commands.clone();
        let clock = now.clone();
        let parameters = host.parameters.clone();
        let ids = host.identities.clone();
        let target_types = target_types.clone();
        let resources = resources.clone();
        let task_ids = task_ids.clone();
        environment.set(
            name,
            lua.create_function(
                move |lua, (object, path, callback): (Table, String, Option<Function>)| {
                    let node = node_id(&object, &ids)?;
                    resource_path(&path)?;
                    let target_type = target_types
                        .get(node)
                        .ok_or_else(|| mlua::Error::runtime("Unknown UVI resource target"))?;
                    if (kind == ResourceKind::Sample && target_type != "SamplePlayer")
                        || (kind == ResourceKind::Impulse
                            && !matches!(target_type.as_str(), "Convolver" | "SampledReverb"))
                    {
                        return Err(mlua::Error::runtime(
                            "UVI resource target has incompatible processor type",
                        ));
                    }
                    // Retain even failed requests so a renderer cannot silently discard a required load.
                    emit(
                        &commands,
                        &*clock,
                        Action::LoadResource {
                            node,
                            kind,
                            path: path.clone(),
                        },
                    )?;
                    let result = (|| {
                        let ResourceResponse::Audio(info) = resource_read(
                            &resources,
                            &ResourceRequest::ReadAudio {
                                kind,
                                path: path.clone(),
                            },
                        )?
                        else {
                            return Err(mlua::Error::runtime(
                                "UVI audio capability returned an incompatible response",
                            ));
                        };
                        if info.rate == 0
                            || info.channels == 0
                            || info.channels > 64
                            || info.name.len() > 4096
                            || info.frames == 0
                        {
                            return Err(mlua::Error::runtime("Invalid UVI decoded audio metadata"));
                        }
                        let sample = lua.create_table_from([
                            ("name", Value::String(lua.create_string(&info.name)?)),
                            ("samplerate", Value::Number(info.rate as f64)),
                            ("channels", Value::Number(info.channels as f64)),
                            (
                                "duration",
                                Value::Number(info.frames as f64 * 1000. / info.rate as f64),
                            ),
                        ])?;
                        if kind == ResourceKind::Sample {
                            object.set("sampleInfo", sample)?;
                        }
                        parameters.borrow_mut()[node]
                            .insert("SamplePath".into(), ParameterValue::Text(path.clone()));
                        Ok(())
                    })();
                    let task = task(lua, &task_ids, result.as_ref().err())?;
                    if let (Ok(()), Some(callback)) = (result, callback) {
                        callback.call::<()>(task.clone())?;
                    }
                    Ok(task)
                },
            )?,
        )?;
    }
    for (name, text) in [("loadData", false), ("loadTextData", true)] {
        let resources = resources.clone();
        let task_ids = task_ids.clone();
        environment.set(
            name,
            lua.create_function(move |lua, (path, callback): (String, Option<Function>)| {
                resource_path(&path)?;
                let mut raw = None;
                let result = (|| {
                    let ResourceResponse::Bytes(bytes) =
                        resource_read(&resources, &ResourceRequest::ReadData { path })?
                    else {
                        return Err(mlua::Error::runtime(
                            "UVI data capability returned an incompatible response",
                        ));
                    };
                    if bytes.len() > SOURCE_LIMIT {
                        return Err(mlua::Error::runtime("UVI resource data exceeds 2 MiB"));
                    }
                    let data = lua.create_string(&bytes)?;
                    raw = Some(data.clone());
                    if text {
                        Ok(Value::String(data))
                    } else {
                        json_data(lua, &bytes)
                    }
                })();
                let task = task(lua, &task_ids, result.as_ref().err())?;
                if let Some(raw) = raw {
                    task.set("data", raw)?;
                    // Native success tracks reading, even when JSON decoding fails.
                    task.set("success", true)?;
                }
                if let (Ok(data), Some(callback)) = (result, callback) {
                    callback.call::<()>(data)?;
                }
                Ok(task)
            })?,
        )?;
    }
    {
        let resources = resources.clone();
        let task_ids = task_ids.clone();
        let state_environment = environment.clone();
        environment.set(
            "loadState",
            lua.create_function(move |lua, (path, callback): (String, Option<Function>)| {
                resource_path(&path)?;
                // Read failures are task failures; Lua restoration callback errors propagate.
                let result = resource_read(&resources, &ResourceRequest::ReadState { path })
                    .and_then(|response| match response {
                        ResourceResponse::Bytes(bytes) => read_state(lua, &bytes),
                        _ => Err(mlua::Error::runtime(
                            "UVI state capability returned an incompatible response",
                        )),
                    });
                let task = task(lua, &task_ids, result.as_ref().err())?;
                if let Ok((program, saved)) = result {
                    restore_state(lua, &program, saved, &state_environment)?;
                    if let Some(callback) = callback {
                        callback.call::<()>(task.clone())?;
                    }
                }
                Ok(task)
            })?,
        )?;
    }
    {
        let resources = resources.clone();
        let task_ids = task_ids.clone();
        let state_environment = environment.clone();
        environment.set(
            "saveState",
            lua.create_function(move |lua, (path, callback): (String, Option<Function>)| {
                resource_path(&path)?;
                let result = (|| {
                    let bytes = save_state(&state_environment)?;
                    match resource_read(&resources, &ResourceRequest::WriteState { path, bytes })? {
                        ResourceResponse::Saved => Ok(()),
                        _ => Err(mlua::Error::runtime(
                            "UVI state capability returned an incompatible response",
                        )),
                    }
                })();
                let task = task(lua, &task_ids, result.as_ref().err())?;
                if let (Ok(()), Some(callback)) = (result, callback) {
                    callback.call::<()>(task.clone())?;
                }
                Ok(task)
            })?,
        )?;
    }
    environment.set(
        "browseForFile",
        lua.create_function(
            move |lua,
                  (mode, title, initial, patterns, callback): (
                String,
                String,
                String,
                String,
                Option<Function>,
            )| {
                if !matches!(mode.as_str(), "open" | "save")
                    || [&title, &initial, &patterns]
                        .iter()
                        .any(|s| s.len() > 4096 || s.contains('\0'))
                {
                    return Err(mlua::Error::runtime("Invalid UVI file browsing request"));
                }
                let result = resource_read(
                    &resources,
                    &ResourceRequest::Browse {
                        mode,
                        title,
                        initial,
                        patterns,
                    },
                );
                let error = match &result {
                    Ok(ResourceResponse::Selected(_)) => None,
                    Ok(_) => Some(mlua::Error::runtime(
                        "UVI browse capability returned an incompatible response",
                    )),
                    Err(e) => Some(e.clone()),
                };
                let task = task(lua, &task_ids, error.as_ref())?;
                if let Ok(ResourceResponse::Selected(selected)) = result {
                    task.set("result", selected.clone().unwrap_or_default())?;
                    if selected.is_none() {
                        task.set("success", false)?;
                        task.set("state", "cancelled")?;
                    }
                }
                if let Some(callback) = callback {
                    callback.call::<()>(task.clone())?;
                }
                Ok(task)
            },
        )?,
    )?;
    Ok(())
}

// Original implementation calibrated with authored pitch-set probes against the
// official player. Numeric masks encode standard intervals above a candidate root.
// The native module installs a global and returns nil (require caches true).
const CHORD_REC: &str = r#"
local kinds={
  [5]="sus2",[9]="m",[11]="maddb9",[13]="madd9",[17]="M",[19]="Maddb9",
  [21]="Madd9",[25]="Madd#9",[33]="sus4",[37]="sus2sus4",[41]="mbb5",[65]="5-",
  [73]="dim",[81]="Mb5",[129]="5",[133]="sus2",[137]="m",[139]="maddb9",
  [141]="madd9",[145]="M",[147]="Maddb9",[149]="Madd9",[153]="Madd#9",[161]="sus4",
  [165]="sus2sus4",[261]="sus2#5",[273]="aug",[525]="m6/9",[533]="6/9",[585]="dim7",
  [649]="m6",[653]="m6/9",[657]="6",[661]="6/9",[1029]="7sus2no5",[1033]="m7",
  [1035]="m7b9",[1037]="m9",[1041]="7",[1043]="7b9",[1045]="9",[1049]="7#9",
  [1057]="7sus4no5",[1061]="7sus2sus4no5",[1069]="m9/11",[1077]="11",[1097]="m7b5",[1101]="m9b5",
  [1105]="7b5",[1109]="9b5",[1157]="7sus2",[1161]="m7",[1163]="m7b9",[1165]="m9",
  [1169]="7",[1171]="7b9",[1173]="9",[1177]="7#9",[1185]="7sus4",[1189]="7sus2sus4",
  [1193]="m7/11",[1197]="m11",[1205]="11",[1225]="m7/#11",[1289]="m7#5",[1297]="7#5",
  [1301]="9#5",[1581]="m13",[1589]="13",[1709]="m13",[1717]="13",[2053]="M7sus2",
  [2057]="mM7",[2061]="mM9",[2065]="M7",[2067]="M7b9",[2069]="M9",[2073]="M7#9",
  [2081]="M7sus4no5",[2085]="M7sus2sus4no5",[2089]="mM7bb5",[2093]="mM11",[2101]="M11",[2121]="mM7b5",
  [2129]="M7b5",[2133]="M#11",[2181]="M7sus2",[2185]="mM7",[2189]="mM9",[2193]="M7",
  [2195]="M7b9",[2197]="M9",[2201]="M7#9",[2209]="M7sus4",[2213]="M7sus2sus4",[2221]="mM11",
  [2229]="M11",[2261]="M#11",[2313]="mM7#5",[2321]="M7#5",[2325]="M9#5",[2605]="mM13",
  [2613]="M13",[2733]="mM13",[2741]="M13",
}
ChordRec={}
function ChordRec.getChroma(root,notes)
  local present={}
  for _,pitch in ipairs(notes)do present[(pitch-root)%12]=true end
  local chroma={}
  for interval=0,11 do chroma[interval+1]=present[interval]and 1 or 0 end
  return chroma
end
function ChordRec.getChromaString(chroma)return table.concat(chroma)end
function ChordRec.chordKind(notes)
  local bass=notes[1]%12
  for _,note in ipairs(notes)do
    local root=note%12
    local chroma=ChordRec.getChroma(root,notes)
    local mask=0
    for interval=0,11 do mask=mask+chroma[interval+1]*2^interval end
    local kind=kinds[mask]
    if kind then return root,kind,bass end
  end
end
"#;

// Original, state-only widget implementation using native Lua tables. Unit is
// formatting metadata: it never rescales the engine parameter or stored value.
const UI: &str = r#"
Unit={Generic=0,Percent=1,PercentNormalized=2,Seconds=3,MilliSeconds=5,Hertz=7,Decibels=9,UviFilter=10,LinearGain=11,Pan=12,Megabyte=13,SemiTones=14,Cents=15,MidiKey=16}
-- Native enum IDs; mappers describe visual position, never stored-value scaling.
Mapper={Linear=0,Exponential=1,QuinticRoot=2,QuarticRoot=3,CubeRoot=4,SquareRoot=5,Quadratic=6,Cubic=7,Quartic=8,Quintic=9}
local float32=_uvi_float;_uvi_float=nil
local widgets, order, root = {}, {}, {width=0,height=0}
local methods={}
local values={Table=true,Menu=true,Knob=true,Slider=true,NumBox=true,Button=true,OnOffButton=true}
local function integer(v)if v<0 then return math.ceil(v)end;return math.floor(v)end
local function geometry(p,k,v)
  p[k]=v
  if k=='bounds' then p.x=v[1];p.y=v[2];p.width=v[3];p.height=v[4]
  elseif k=='size' then p.width=v[1];p.height=v[2]
  elseif k=='position' or k=='pos' then p.x=v[1];p.y=v[2] end
end
function methods:setValue(a,b,c)
  local p=self._state
  if not values[p.kind] then error('This UVI widget has no value control')end
  local call=true
  local index=nil
  if p.kind=='Table' then
    index=a; if type(index)~='number' or index~=index or index==math.huge or index==-math.huge then error('Invalid UVI Table index') end;index=integer(index)
    if index<1 or index>p.length then return end
    if type(b)~='number' or b~=b or b==math.huge or b==-math.huge then error('UVI Table value must be finite') end
    local v=b; if p.integer then v=integer(v) end
    v=float32(v);local old=p.values[index];p.values[index]=v;call=c~=false and old~=v
  else
    if p.kind=='OnOffButton' or p.kind=='Button' then if type(a)~='boolean' then error('UVI button value must be boolean') end
    elseif type(a)~='number' or a~=a or a==math.huge or a==-math.huge then error('UVI control value must be finite') end
    local v=a
    if type(v)=='number' then if p.integer then v=integer(v) end;v=float32(v) end
    local old=p.value;p.value=v;call=b~=false and old~=v
  end
  if call and type(p.changed)=='function' then p.changed(self,index) end
end
function methods:getValue(index)
  local p=self._state
  if not values[p.kind] then error('This UVI widget has no value control')end
  if p.kind=='Table' then if type(index)~='number' or index~=index or index==math.huge or index==-math.huge then error('Invalid UVI Table index') end return p.values[integer(index)] or p.default end
  return p.value
end
function methods:getText(index) local p=self._state;if p.kind~='Menu' then error('Only UVI Menu has getText')end;if type(index)~='number' then error('UVI Menu getText requires an index')end;return p.items[integer(index)] or '' end
function methods:clear()local p=self._state;if p.kind~='Menu' then error('Only UVI Menu can clear items')end;p.items={};p.max=0 end
function methods:addItem(text)local p=self._state;if p.kind~='Menu' or type(text)~='string' then error('UVI Menu item must be text')end;table.insert(p.items,text);p.max=#p.items;return #p.items end
function methods:setItem(index,text)
  local p=self._state; if p.kind~='Menu' or type(index)~='number' or type(text)~='string' then error('Invalid UVI Menu item') end
  index=integer(index);if index<1 or index>#p.items then error('Invalid UVI Menu item')end;p.items[index]=text
end
function methods:setRange(min,max) if type(min)~='number' or type(max)~='number' or min>max then error('Invalid UVI widget range') end self._state.min=min; self._state.max=max end
function methods:setStripImage(image,numImages,orientation) self._state.stripImage={image,numImages,orientation} end
local function construct(kind,...)
  local args={...}; local p={kind=kind,enabled=true,visible=true,alpha=1,persistent=true,children={},x=0,y=0,width=0,height=0}
  if type(args[1])=='table' then for k,v in pairs(args[1]) do p[k]=v end; args=args[1] end
  if kind=='XY' then
    p.paramX=p.paramX or args[1];p.paramY=p.paramY or args[2]
    if type(p.paramX)~='string' or type(p.paramY)~='string' then error('UVI XY requires two parameter names')end
    p.name=p.name or ('XY_'..p.paramX..'_'..p.paramY)
  end
  p.name=p.name or args[1];if p.name==nil and (kind=='Panel' or kind=='Viewport')then p.name=''end
  if type(p.name)~='string' then error('UVI widget requires a name') end
  p.displayName=p.displayName or p.name; p.tooltip=p.tooltip or p.name
  p.integer=p.integer or false
  if kind=='WaveView' then p.sample=p.sample or '' end
  if kind=='Table' then p.length=p.length or args[2] or 16; p.default=p.default or args[3] or 0; p.min=p.min or args[4] or 0; p.max=p.max or args[5] or 1; p.integer=p.integer or args[6] or false; p.values={}; if p.length<1 or p.length>65536 or p.length%1~=0 then error('Invalid UVI Table length') end; for i=1,p.length do p.values[i]=p.default end
  elseif kind=='Menu' then p.items=p.items or args[2] or {}; p.min=1; p.max=#p.items; p.value=p.value or p.selected or args[3] or 1; p.integer=true
  elseif kind=='Button' or kind=='OnOffButton' then if p.value==nil then p.value=args[2] or false end
  elseif kind=='Knob' or kind=='Slider' or kind=='NumBox' then p.min=p.min or args[3] or 0; p.max=p.max or args[4] or 1; p.value=p.value or args[2] or 0; p.integer=p.integer or args[5] or false
  end
  if p.min then p.min=float32(p.min);p.max=float32(p.max)end
  if kind=='Table' then p.default=float32(p.default);for i=1,p.length do p.values[i]=p.default end
  elseif kind=='Knob' or kind=='Slider' or kind=='NumBox' then p.default=float32(p.default or args[2] or 0);p.value=float32(p.value)end
  if p.size then geometry(p,'size',p.size)end;if p.position then geometry(p,'position',p.position)end;if p.pos then geometry(p,'pos',p.pos)end;if p.bounds then geometry(p,'bounds',p.bounds)end
  local widget=setmetatable({_state=p},{__index=function(t,k)
    if p.kind=='Menu' then if k=='selected' then return p.value elseif k=='text' or k=='selectedText' then return p.items[p.value] or '' elseif k=='length' then return #p.items end end
    if k=='size' then return {p.width,p.height} elseif k=='position' or k=='pos' then return {p.x,p.y} elseif k=='bounds' then return {p.x,p.y,p.width,p.height}end
    return methods[k] or p[k]
  end,__newindex=function(t,k,v) if k=='value' or (p.kind=='Menu' and k=='selected') then methods.setValue(t,v) else geometry(p,k,v) end end})
  if p.parent then table.insert(p.parent._state.children,widget) end
  widgets[p.name]=widget
  table.insert(order,widget)
  return widget
end
for _,kind in ipairs{'Panel','Viewport','Label','Image','WaveView','AudioMeter','XY','Menu','Table','Slider','Knob','NumBox','Button','OnOffButton'} do
  _G[kind]=function(...) return construct(kind,...) end
  methods[kind]=function(parent,...) if parent.kind~='Panel' and parent.kind~='Viewport' then error('Only UVI containers can create child widgets')end;local widget=construct(kind,...);if not widget.parent then widget.parent=parent;table.insert(parent._state.children,widget)end;return widget end
end
function setSize(w,h) root.width=w;root.height=h end
function setHeight(h) root.height=h end
function setBackground(path) root.background=path end
function setBackgroundColour(colour) root.backgroundColour=colour end
function makePerformanceView() root.performanceView=true end
function setKeyColour(note,colour) root.keyColours=root.keyColours or {};root.keyColours[note]=colour end
function resetKeyColour(note) if root.keyColours then root.keyColours[note]=nil end end
function setKeySwitches(notes) root.keySwitches=notes end
UVI_UI_STATE={widgets=widgets,order=order,root=root}
"#;

#[cfg(test)]
mod tests {
    use super::super::program::parse_program;
    use super::*;
    use mlua::{LuaOptions, StdLib};
    fn vm() -> Lua {
        let lua = Lua::new_with(
            StdLib::TABLE | StdLib::STRING | StdLib::MATH,
            LuaOptions::default(),
        )
        .unwrap();
        lua.set_memory_limit(32 << 20).unwrap();
        lua
    }
    #[test]
    fn host_resolves_only_unambiguous_approved_relative_modules() {
        let lua = vm();
        let counter =
            b"local name=...;counter=(counter or 0)+1;return {name=name,count=counter}".to_vec();
        let modules = BTreeMap::from([
            (
                "Scripts.MIDI Scripts._Folder.Counter".into(),
                counter.clone(),
            ),
            ("ApprovedAlias._Folder.Counter".into(), counter),
            ("First._Conflict.Main".into(), b"return 'first'".to_vec()),
            ("Second._Conflict.Main".into(), b"return 'second'".to_vec()),
            ("_Conflict/Main".into(), b"return 'exact'".to_vec()),
            (
                "Scripts._Loop.Main".into(),
                b"return require('_Loop/Main')".to_vec(),
            ),
        ]);
        install(
            &lua,
            HostConfig {
                program: None,
                modules,
                now: Rc::new(|| 0),
                resources: None,
                valid_voice: None,
            },
        )
        .unwrap();
        lua.load(r#"
          local m=require('_Folder/Counter')
          assert(m==require('_Folder/Counter') and m.count==1 and m.name=='_Folder/Counter')
          assert(require('_Conflict/Main')=='exact')
          local ok,err=pcall(require,'_Conflict.Main');assert(not ok and string.find(tostring(err),'ambiguous'))
          assert(not pcall(require,'_Loop/Main'))
          assert(not pcall(require,'outside/Counter'))
          assert(not pcall(require,''))
          assert(not pcall(require,string.rep('x',257)))
          assert(require('uvi.ChordRec')==true)
          local extend=class'AuthoredBase';assert(type(extend)=='function' and type(AuthoredBase)=='userdata')
          AuthoredBase.static=7
          function AuthoredBase:__init(x)self.x=x end
          function AuthoredBase:sum()return self.x+AuthoredBase.static end
          local a=AuthoredBase(4);a.y=9
          assert(type(a)=='userdata' and a.x==4 and a.y==9 and a:sum()==11 and a.static==7 and a.missing==nil and a==a)
          assert(not pcall(function()return a==AuthoredBase(4)end))
          assert(not pcall(function()return tostring(a)end))
          assert(class'AuthoredChild'(AuthoredBase)==nil)
          assert(not pcall(function()AuthoredChild(3)end))
          function AuthoredChild:__init(x)self.x=x*2 end
          local child=AuthoredChild(5);assert(child.x==10 and child:sum()==17 and child.static==7)
          AuthoredBase.static=10;function AuthoredBase:sum()return 1000 end
          assert(child:sum()==20 and child.static==7)
          local rec=ChordRec;assert(require('uvi.ChordRec')==true and rec==ChordRec)
          local root,kind,bass=rec.chordKind{60,64,67};assert(root==0 and kind=='M' and bass==0)
          root,kind,bass=rec.chordKind{64,67,72};assert(root==0 and kind=='M' and bass==4)
          root,kind,bass=rec.chordKind{69,60,64,67};assert(root==9 and kind=='m7' and bass==9)
          root,kind,bass=rec.chordKind{64,60,67,69};assert(root==0 and kind=='6' and bass==4)
          root,kind,bass=rec.chordKind{64,69,60,67};assert(root==9 and kind=='m7' and bass==4)
          root,kind,bass=rec.chordKind{60.5,64.5,67.5};assert(root==0.5 and kind=='M' and bass==0.5)
          assert(rec.chordKind{60,61}==nil and rec.chordKind{60}==nil)
          assert(rec.getChromaString(rec.getChroma(0,{60,64,67}))=='100010010000')
          assert(not pcall(rec.chordKind,{}))
          assert(require('uvi.AsyncUpdater')==true and type(AsyncUpdater)=='userdata')
          local factory=AsyncUpdater;require('uvi.AsyncUpdater');assert(AsyncUpdater==factory)
          local delays={};local count=0;local u
          wait=function(ms)table.insert(delays,ms);u:trigger(99)end
          u=AsyncUpdater(function(...)assert(select('#',...)==0);count=count+1;u:trigger(0)end)
          assert(type(u)=='userdata' and type(u.trigger)=='function' and type(u.callback)=='function' and u.pending==nil and u.cancel==nil)
          u:trigger(20);assert(count==1 and #delays==1 and delays[1]==20)
          u:trigger(30);assert(count==2 and #delays==2 and delays[2]==30)
          u.callback=function()count=count+10 end;u:trigger(0);assert(count==12)
        "#).exec().unwrap();
    }
    #[test]
    fn host_graph_modules_units_controls_and_timed_mutations() {
        let program=parse_program(r#"<Program Name="P" Gain="0.8"><Layers><Layer Name="L" Gain="0.6"><Keygroups><Keygroup Name="K" Gain="0.4"><Oscillators><SamplePlayer Name="S" BaseNote="60" SamplePath="synthetic.wav"/></Oscillators><Connections><SignalConnection Name="C" Destination="Gain" Source="X" Ratio="0.2"/></Connections></Keygroup></Keygroups></Layer></Layers></Program>"#).unwrap();
        let lua = vm();
        let clock = Rc::new(std::cell::Cell::new(17));
        let now = clock.clone();
        let mut modules = BTreeMap::new();
        modules.insert(
            "counter".into(),
            b"counter=(counter or 0)+1;return {value=counter}".to_vec(),
        );
        modules.insert("loop".into(), b"return require('loop')".to_vec());
        let host = install(
            &lua,
            HostConfig {
                program: Some(&program),
                modules,
                now: Rc::new(move || now.get()),
                resources: None,
                valid_voice: Some(Rc::new(|id| id == 1)),
            },
        )
        .unwrap();
        lua.globals()
            .set(
                "authoredVoice",
                super::super::script::voice_handle(&lua, 1).unwrap(),
            )
            .unwrap();
        lua.load(r#"
          assert(__API_VERSION__==23)
          assert(Mapper.Linear==0 and Mapper.Exponential==1 and Mapper.Quintic==9)
          local unnamed=Panel{};local view=unnamed:Viewport{}
          assert(unnamed.name=='' and view.name=='' and view.parent==unnamed and unnamed.children[1]==view)
          assert(not pcall(function()Knob{}end))
          local x=Knob{'axisX',0.25,0,1};local y=Knob{'axisY',0.75,-1,1}
          local xy=view:XY{'axisX','axisY',bounds={2,3,128,64}}
          assert(xy.name=='XY_axisX_axisY' and xy.paramX=='axisX' and xy.paramY=='axisY' and xy.parent==view)
          assert(xy.x==2 and xy.y==3 and xy.width==128 and xy.height==64 and xy.value==nil)
          local axisCalls=0;local xyCalls=0;x.changed=function()axisCalls=axisCalls+1 end;xy.changed=function()xyCalls=xyCalls+1 end
          x:setValue(0.5);assert(axisCalls==1 and xyCalls==0 and UVI_UI_STATE.widgets[xy.paramX]==x)
          xy.paramX='axisY';assert(xy.paramX=='axisY' and xy.name=='XY_axisX_axisY')
          assert(not pcall(function()xy:setValue(0.2,0.3)end))
          assert(not pcall(function()XY{'axisX'}end))
          local wave=WaveView{'authored-wave',size={128,64},hiWaveColour='#FF0000'};wave.visible=false
          assert(wave.kind=='WaveView' and wave.sample=='' and wave.visible==false and wave.width==128 and wave.height==64)
          assert(UVI_UI_STATE.widgets['authored-wave']==wave and wave.hiWaveColour=='#FF0000')
          assert(not pcall(function()wave:setValue(0.5)end))
          local l=Program.layers[1];local k=l.keygroups[1]
          assert(l==Program.children.L and k.parent==l)
          assert(k:getParameter('Gain')==0.4 and k.oscillators[1]:getParameter('BaseNote')==60)
          k._nodeId=0 -- object identity stays bound to the original graph node
          k:setParameter('Gain',0.7);assert(k:getParameter('Gain')==0.7)
          assert(not pcall(function()k.setParameter({_nodeId=0},'Gain',0.2)end))
          local c=k:getParameterConnections('Gain')[1];c:setParameter('Ratio',0.3)
          assert(c:getParameter('Ratio')==0.3)
          assert(not pcall(function() return k:getParameter('Invented') end))
          k:setParameter('Gain',true);assert(k:getParameter('Gain')==0.7)
          assert(not pcall(function() k:setParameter('Gain',{}) end))
          assert(require('counter')==require('counter') and counter==1)
          assert(not pcall(function()require('loop')end))
          assert(not pcall(function()require('disk/module')end))
          local knob=Knob{'n',0.25,0,1,unit=Unit.PercentNormalized}
          local called=0;knob.changed=function(self)called=called+1;k:setParameter('Gain',self.value)end
          knob:setValue(0.6,false);assert(called==0 and math.abs(knob.value-0.6)<1e-6)
          knob:setValue(0.6);assert(called==0)
          knob.value=0.9;assert(called==1 and math.abs(k:getParameter('Gain')-0.9)<1e-6)
          assert(Unit.MilliSeconds==5 and Unit.SemiTones==14 and Unit.PercentNormalized==2)
          local ms=Knob{'ms',1500,0,2000,unit=Unit.MilliSeconds};assert(ms.value==1500)
          local pct=Knob{'pct',75,0,100,unit=Unit.Percent};assert(pct.value==75)
          local t=Table{'t',3,0,0,1};t.changed=function(self,index)called=called+index end
          t:setValue(2,0.4);assert(math.abs(t:getValue(2)-0.4)<1e-6 and called==3)
          t:setValue(0,1);t:setValue(4,1);assert(t:getValue(0)==0 and t:getValue(4)==0 and called==3)
          local ints=Knob{'ints',0,-5,5,true};ints:setValue(-1.8,false);assert(ints.value==-1);ints:setValue(6,false);assert(ints.value==6)
          local defaults=Table{'defaults',2,0.25,0,1};defaults:setValue(1.8,2,false);assert(defaults:getValue(1)==2 and defaults:getValue(0.8)==0.25)
          assert(type(authoredVoice)=='userdata')
          sendScriptModulation(3,0.4,100,authoredVoice)
          assert(not pcall(function()sendScriptModulation(3,0.4,100,1)end))
          assert(not pcall(function()sendScriptModulation(3,0.4,100,99999)end))
          assert(not pcall(function()sendScriptModulation2(3,0.1,0.4,100,99999)end))
          sendScriptModulation2(4,0.1,0.2,0,authoredVoice)
        "#).exec().unwrap();
        let commands = host.commands.borrow();
        assert_eq!(commands.len(), 5);
        assert!(commands.iter().all(|c| c.frame == 17));
        assert!(
            matches!(&commands[1].action,Action::Parameter{parameter,value:ParameterValue::Number(n),..} if parameter=="Ratio" && *n==0.3)
        );
        assert!(matches!(
            &commands[3].action,
            Action::ScriptModulation {
                ramp_ms: 100.,
                voice: Some(1),
                ..
            }
        ));
        assert!(matches!(
            commands[4].action,
            Action::ScriptModulation {
                start: Some(0.1),
                target: 0.2,
                voice: Some(1),
                ..
            }
        ));
    }

    #[test]
    fn host_restores_original_values_and_reports_deferred_resource_failure() {
        let program=parse_program(r#"<Program Name="P"><EventProcessors><ScriptProcessor Name="Script" n="0.8" enabled="1"><ScriptData t="0,100000 0,750000"/></ScriptProcessor></EventProcessors><Layers><Layer Name="L"><Keygroups><Keygroup Name="K"><Oscillators><SamplePlayer Name="S" SamplePath="synthetic.wav"/></Oscillators></Keygroup></Keygroups></Layer></Layers></Program>"#).unwrap();
        let lua = vm();
        let host = install(
            &lua,
            HostConfig {
                program: Some(&program),
                modules: BTreeMap::new(),
                now: Rc::new(|| 31),
                resources: None,
                valid_voice: None,
            },
        )
        .unwrap();
        lua.load(r#"
          called=0
          n=Knob{'n',0,0,1};n.changed=function(self)called=called+1;assert(math.abs(self.value-0.8)<1e-6 and t:getValue(2)==0.75)end
          t=Table{'t',2,0,0,1};t.changed=function(self,index)called=called+1 end
          enabled=OnOffButton{'enabled',false}
          loadSample(Program.layers[1].keygroups[1].oscillators[1],'approved-resource',function(task)assert(task.finished and not task.success)end)
        "#).exec().unwrap();
        assert_eq!(restore_widgets(&lua, &program).unwrap(), 3);
        lua.load("assert(called==3 and enabled.value==true and math.abs(t:getValue(1)-0.1)<1e-6)")
            .exec()
            .unwrap();
        let commands = host.commands.borrow();
        assert_eq!(commands.len(), 1);
        assert!(
            matches!(&commands[0],Command{frame:31,action:Action::LoadResource{kind:ResourceKind::Sample,path,..}} if path=="approved-resource")
        );
    }
    #[test]
    fn host_unchanged_persisted_widget_does_not_invoke_callback() {
        let program=parse_program(r#"<Program><EventProcessors><ScriptProcessor n="0.6"><ScriptData t="0,250000 0,750000"/></ScriptProcessor></EventProcessors></Program>"#).unwrap();
        let lua = vm();
        install(
            &lua,
            HostConfig {
                program: Some(&program),
                modules: BTreeMap::new(),
                now: Rc::new(|| 0),
                resources: None,
                valid_voice: None,
            },
        )
        .unwrap();
        lua.load(
            r#"
          n=Knob{'n',0.6,0,1};n.changed=function()error('unchanged callback')end
          t=Table{'t',2,0.25,0,1};t:setValue(2,0.75,false)
          t.changed=function()error('unchanged table callback')end
        "#,
        )
        .exec()
        .unwrap();
        assert_eq!(restore_widgets(&lua, &program).unwrap(), 2);
    }

    #[test]
    fn host_approved_capabilities_preserve_data_state_and_audio_results() {
        let program = parse_program(r#"<Program><Layers><Layer><Keygroups><Keygroup><Oscillators><SamplePlayer SamplePath="old.wav"/></Oscillators></Keygroup></Keygroups></Layer></Layers></Program>"#).unwrap();
        let lua = vm();
        let requests = Rc::new(RefCell::new(Vec::new()));
        let recorded = requests.clone();
        let saved = Rc::new(RefCell::new(Vec::new()));
        let storage = saved.clone();
        let host = install(
            &lua,
            HostConfig {
                program: Some(&program),
                modules: BTreeMap::new(),
                now: Rc::new(|| 42),
                valid_voice: None,
                resources: Some(Rc::new(move |request| {
                    recorded.borrow_mut().push(request.clone());
                    Ok(match request {
                        ResourceRequest::ReadAudio { path, .. } if path == "owned.wav" => {
                            ResourceResponse::Audio(ResourceInfo {
                                name: "owned.wav".into(),
                                rate: 48_000,
                                channels: 2,
                                frames: 24_000,
                            })
                        }
                        ResourceRequest::ReadData { path } if path == "owned.json" => {
                            ResourceResponse::Bytes(
                                br#"{"name":"authored","values":[2,3]}"#.to_vec(),
                            )
                        }
                        ResourceRequest::ReadData { path } if path == "invalid.json" => {
                            ResourceResponse::Bytes(b"invalid".to_vec())
                        }
                        ResourceRequest::ReadData { path } if path == "null.json" => {
                            ResourceResponse::Bytes(
                                br#"{"missing":null,"values":[1,null,3]}"#.to_vec(),
                            )
                        }
                        ResourceRequest::ReadState { path } if path == "invalid.state" => {
                            ResourceResponse::Bytes(b"<bad/>".to_vec())
                        }
                        ResourceRequest::WriteState { path, bytes } if path == "owned.state" => {
                            *storage.borrow_mut() = bytes.clone();
                            ResourceResponse::Saved
                        }
                        ResourceRequest::ReadState { path } if path == "owned.state" => {
                            ResourceResponse::Bytes(storage.borrow().clone())
                        }
                        ResourceRequest::Browse { .. } => ResourceResponse::Selected(None),
                        _ => return Err(mlua::Error::runtime("Resource is not approved")),
                    })
                })),
            },
        )
        .unwrap();
        lua.load(r#"
            local oscillator=Program.layers[1].keygroups[1].oscillators[1]
            Program.layers[1].type='SamplePlayer'
            assert(not pcall(function()loadSample(Program.layers[1],'owned.wav')end))
            oscillator.type='Layer' -- target type, like node identity, stays bound to the graph
            local task=loadSample(oscillator,'owned.wav',function(t)assert(t.success and t.finished and t.state=='finished')end)
            assert(task.id==1 and oscillator.sampleInfo.duration==500 and oscillator.sampleInfo.samplerate==48000)
            assert(oscillator:getParameter('SamplePath')=='owned.wav')
            local bad=loadSample(oscillator,'unapproved.wav',function()error('failed load must not call completion')end);assert(not bad.success and oscillator:getParameter('SamplePath')=='owned.wav')
            local json=loadData('owned.json',function(data)assert(data.name=='authored' and data.values[2]==3)end)
            assert(json.success and string.find(json.data,'authored'))
            local badJSON=loadData('invalid.json',function()error('failed JSON decoding must not call completion')end);assert(badJSON.success and badJSON.data=='invalid' and badJSON.error)
            local nullCalled=false
            loadData('null.json',function(data)nullCalled=true;assert(data.missing==nil and data.values[2]==nil and data.values[3]==3)end)
            assert(nullCalled)
            local badState=loadState('invalid.state',function()error('invalid state must not call completion')end);assert(not badState.success)
            k=Knob{'k',0.8,0,1};t=Table{'t',2,0.3,0,1};events={}
            function onSave()return{foo=7,values={1,2},text='<&'}end
            saveState('owned.state',function(task)assert(task.success)end)
            k:setValue(0.2,false);t:setValue(1,0.1,false);t:setValue(2,0.1,false)
            k.changed=function()table.insert(events,'changed')end
            t.changed=function(_,i)table.insert(events,'table'..i)end
            function onLoad(data)assert(data.foo==7 and data.values[2]==2 and data.text=='<&');assert(math.abs(k.value-0.8)<1e-6);table.insert(events,'load')end
            function onInit()error('loadState must not run onInit')end
            loadState('owned.state',function(task)assert(task.success);table.insert(events,'done')end)
            assert(table.concat(events,',')=='table1,table2,changed,load,done')
            local cancelled=browseForFile('open','Choose','','*.wav');assert(not cancelled.success and cancelled.state=='cancelled' and cancelled.result=='')
            k.changed=function()error('callback error propagates')end;k:setValue(0.1,false)
            assert(not pcall(function()loadState('owned.state')end))
        "#).exec().unwrap();
        assert_eq!(host.commands.borrow().len(), 2);
        assert!(host
            .commands
            .borrow()
            .iter()
            .all(|command| command.frame == 42));
        assert_eq!(requests.borrow().len(), 10);
        let text = String::from_utf8(saved.borrow().clone()).unwrap();
        assert!(text.contains("<UVI4><ScriptProcessor") && text.contains("<ScriptData t="));
    }

    #[test]
    fn host_menu_mutations_and_default_capability_denial_are_explicit() {
        let lua = vm();
        install(
            &lua,
            HostConfig {
                program: None,
                modules: BTreeMap::new(),
                now: Rc::new(|| 0),
                resources: None,
                valid_voice: None,
            },
        )
        .unwrap();
        lua.load(r#"
            local menu=Menu{'menu',{'first','second'}};local changed=0;menu.changed=function()changed=changed+1 end
            assert(menu:getText(1.8)=='first' and menu:getText(-1)=='')
            assert(not pcall(function()menu:getText()end))
            menu:clear();assert(menu.length==0 and menu.value==1 and menu.text=='')
            assert(menu:addItem('new')==1 and menu:addItem('other')==2)
            menu:setItem(1.8,'renamed');assert(menu:getText(1)=='renamed' and changed==0)
            assert(not pcall(function()menu:setItem(0,'bad')end))
            local data=loadData('unapproved',function()error('denied data must not call completion')end);assert(not data.success and data.error)
            local state=loadState('unapproved');assert(not state.success)
            local save=saveState('unapproved');assert(not save.success)
            local browse=browseForFile('open','Choose','','');assert(not browse.success and browse.error)
            sendScriptModulation(0,0.1)
            assert(not pcall(function()sendScriptModulation(0,0.1,20,1)end))
        "#).exec().unwrap();
    }

    #[test]
    fn host_persistence_notifies_tables_first_and_stops_at_callback_errors() {
        let program = parse_program(r#"<Program><EventProcessors><ScriptProcessor z="0.8" a="0.9"><ScriptData ta="0.1 0.75" tz="0.2 0.65"/></ScriptProcessor></EventProcessors></Program>"#).unwrap();
        let lua = vm();
        install(
            &lua,
            HostConfig {
                program: Some(&program),
                modules: BTreeMap::new(),
                now: Rc::new(|| 0),
                resources: None,
                valid_voice: None,
            },
        )
        .unwrap();
        lua.load(r#"
          z=Knob{'z',0,0,1};a=Knob{'a',0,0,1};tz=Table{'tz',2,0,0,1};ta=Table{'ta',2,0,0,1};events={}
          tz.changed=function(_,i)assert(z.value==0 and a.value==0);if i==1 then assert(tz:getValue(2)==0)end;table.insert(events,'tz'..i)end
          ta.changed=function(_,i)assert(z.value==0 and a.value==0);table.insert(events,'ta'..i)end
          z.changed=function()assert(a.value==0);table.insert(events,'z')end
          a.changed=function()table.insert(events,'a')end
        "#).exec().unwrap();
        assert_eq!(restore_widgets(&lua, &program).unwrap(), 4);
        lua.load(
            r#"
          assert(table.concat(events,',')=='tz1,tz2,ta1,ta2,z,a')
          z:setValue(0,false);a:setValue(0,false);tz:setValue(1,0,false);tz:setValue(2,0,false)
          tz.changed=function()error('stop here')end
        "#,
        )
        .exec()
        .unwrap();
        assert!(restore_widgets(&lua, &program).is_err());
        lua.load("assert(z.value==0 and a.value==0 and tz:getValue(2)==0)")
            .exec()
            .unwrap();
    }

    #[test]
    fn host_large_graph_keeps_objects_in_lua_without_exhausting_reference_stack() {
        let mut xml = String::from(
            r#"<Program><Layers><Layer Name="L"><Keygroups><Keygroup Name="K" Gain="1"><Connections>"#,
        );
        for i in 0..9000 {
            xml.push_str(&format!(
                r#"<SignalConnection Name="C{i}" Source="X" Destination="Gain" Ratio="0.1"/>"#
            ));
        }
        xml.push_str("</Connections></Keygroup></Keygroups></Layer></Layers></Program>");
        let program = parse_program(&xml).unwrap();
        assert!(program.nodes.len() > 7996);
        let lua = vm();
        let host = install(
            &lua,
            HostConfig {
                program: Some(&program),
                modules: BTreeMap::new(),
                now: Rc::new(|| 23),
                resources: None,
                valid_voice: None,
            },
        )
        .unwrap();
        lua.gc_collect().unwrap();
        lua.load(
            r#"
          local k=Program.layers[1].keygroups[1]
          local connections=k:getParameterConnections('Gain')
          assert(#connections==9000 and connections[9000].name=='C8999')
          assert(connections[9000].parent==k and k.parent==Program.layers[1])
          connections[9000]:setParameter('Ratio',0.2)
          assert(k:getParameterConnections('Gain')[9000]==connections[9000])
        "#,
        )
        .exec()
        .unwrap();
        assert!(
            matches!(host.commands.borrow()[0], Command {frame: 23,action: Action::Parameter {node, ..}} if node > 7996)
        );
        // Exhaustion of the configured Lua heap remains a recoverable Lua error.
        let small = vm();
        small.set_memory_limit(128 << 10).unwrap();
        assert!(install(
            &small,
            HostConfig {
                program: Some(&program),
                modules: BTreeMap::new(),
                now: Rc::new(|| 0),
                resources: None,
                valid_voice: None,
            }
        )
        .is_err());
    }

    #[test]
    fn host_scopes_isolate_modules_widgets_and_state_while_sharing_engine_objects() {
        let program = parse_program(r#"<Program><EventProcessors><ScriptProcessor Name="A" n="0.75"/></EventProcessors><Layers><Layer Name="L" Gain="1"><EventProcessors><ScriptProcessor Name="B" n="0.5"/></EventProcessors></Layer></Layers></Program>"#).unwrap();
        let processors = program
            .nodes
            .iter()
            .enumerate()
            .filter(|(_, n)| n.kind == "ScriptProcessor")
            .map(|(id, _)| id)
            .collect::<Vec<_>>();
        let lua = vm();
        let storage = Rc::new(RefCell::new(BTreeMap::new()));
        let files = storage.clone();
        let host = install(
            &lua,
            HostConfig {
                program: Some(&program),
                modules: BTreeMap::from([(
                    "scope".into(),
                    b"moduleCount=(moduleCount or 0)+1;return {owner=this.name,count=moduleCount}"
                        .to_vec(),
                )]),
                now: Rc::new(|| 11),
                valid_voice: None,
                resources: Some(Rc::new(move |request| {
                    Ok(match request {
                        ResourceRequest::WriteState { path, bytes } => {
                            files.borrow_mut().insert(path.clone(), bytes.clone());
                            ResourceResponse::Saved
                        }
                        ResourceRequest::ReadState { path } => ResourceResponse::Bytes(
                            files
                                .borrow()
                                .get(path)
                                .cloned()
                                .ok_or_else(|| mlua::Error::runtime("Unapproved state"))?,
                        ),
                        _ => return Err(mlua::Error::runtime("Unapproved resource")),
                    })
                })),
            },
        )
        .unwrap();
        let a = host
            .script_environment(&lua, &program, processors[0])
            .unwrap();
        let b = host
            .script_environment(&lua, &program, processors[1])
            .unwrap();
        lua.load(r#"
          assert(sentinel==nil and _G.sentinel==nil);sentinel='A';table.scopeSentinel='A'
          local first=require('scope');assert(first==require('scope') and first.owner=='A' and first.count==1)
          require('uvi.ChordRec');ChordRec.scopeSentinel='A'
          class'ScopeClass';function ScopeClass:__init()self.owner='A'end
          n=Knob{'n',0.1,0,1};n.changed=function()Program.layers[1]:setParameter('Gain',n.value)end
          function onSave()return{owner='A'}end
          function onLoad(data)assert(data.owner=='A');loaded='A'end
        "#).set_environment(a.clone()).exec().unwrap();
        lua.load(r#"
          assert(sentinel==nil and _G.sentinel==nil and table.scopeSentinel==nil);sentinel='B'
          local first=require('scope');assert(first==require('scope') and first.owner=='B' and first.count==1)
          require('uvi.ChordRec');assert(ChordRec.scopeSentinel==nil)
          assert(ScopeClass==nil);class'ScopeClass';function ScopeClass:__init()self.owner='B'end
          assert(this.parent==Program.layers[1])
          n=Knob{'n',0.2,0,1};n.changed=function()Program.layers[1]:setParameter('Gain',n.value)end
          function onSave()return{owner='B'}end
          function onLoad(data)assert(data.owner=='B');loaded='B'end
        "#).set_environment(b.clone()).exec().unwrap();
        assert_eq!(
            a.get::<Table>("Program").unwrap(),
            b.get::<Table>("Program").unwrap()
        );
        assert_eq!(
            host.object_id(&a.get::<Table>("this").unwrap()).unwrap(),
            processors[0]
        );
        assert_eq!(
            host.object_id(&b.get::<Table>("this").unwrap()).unwrap(),
            processors[1]
        );
        restore_widgets_scoped(&lua, &program, processors[0], &a).unwrap();
        lua.load(
            "assert(Program.layers[1]:getParameter('Gain')==0.75 and math.abs(n.value-0.2)<1e-6)",
        )
        .set_environment(b.clone())
        .exec()
        .unwrap();
        restore_widgets_scoped(&lua, &program, processors[1], &b).unwrap();
        lua.load("assert(Program.layers[1]:getParameter('Gain')==0.5 and n.value==0.75)")
            .set_environment(a.clone())
            .exec()
            .unwrap();
        lua.load("local task=saveState('A.state');assert(task.success);n:setValue(0.3,false);assert(loadState('A.state').success and loaded=='A' and n.value==0.75)").set_environment(a.clone()).exec().unwrap();
        lua.load("assert(loaded==nil and n.value==0.5);assert(saveState('B.state').success);n:setValue(0.4,false);assert(loadState('B.state').success and loaded=='B' and n.value==0.5)").set_environment(b.clone()).exec().unwrap();
        assert!(lua.globals().get::<Value>("sentinel").unwrap().is_nil());
        assert!(lua.globals().get::<Value>("onSave").unwrap().is_nil());
        assert_eq!(storage.borrow().len(), 2);
        assert!(restore_widgets(&lua, &program).is_err());
        assert!(host
            .script_environment(&lua, &program, program.root)
            .is_err());
        assert!(host
            .commands
            .borrow()
            .iter()
            .all(|command| command.frame == 11));
    }
}
