//! The Falcon/UVI Lua script runtime (design: `docs/architecture-v2/UVI_LUA.md`).
//!
//! A [`ScriptHost`] runs a program's scripts in a sandboxed Luau state on a
//! control thread, never on the audio thread. It is fed note events with a
//! time and answers with timed [`Command`]s; `wait`/`spawn` are coroutines
//! resumed by [`ScriptHost::advance`], so timing follows the host's clock.
//! Whatever the host does not model is inert and reported, not silent.

use mlua::{
    Function, Lua, LuaOptions, MultiValue, StdLib, Table, Thread, Value, Variadic, VmState,
};
use std::{
    cell::{Cell, RefCell},
    collections::BTreeMap,
    rc::Rc,
    time::{Duration, Instant},
};

mod ui;

const PRELUDE: &str = include_str!("script_prelude.lua");
/// Instructions between two budget checks of the VM hook.

/// Where `require` finds a module: a bank's script members.
pub trait Files {
    fn script(&self, module: &str) -> Option<String>;
}

impl<T: Files> Files for std::rc::Rc<T> {
    fn script(&self, module: &str) -> Option<String> {
        (**self).script(module)
    }
}

/// A bank's Lua members, by path.
#[derive(Default)]
pub struct Scripts {
    files: Vec<(String, String)>,
}

impl Scripts {
    pub fn insert(&mut self, path: &str, source: String) {
        self.files
            .push((path.to_lowercase().replace('\\', "/"), source));
    }
}

impl Files for Scripts {
    /// `require 'a/b'` finds the member `.../a/b.lua`; the shortest path wins.
    fn script(&self, module: &str) -> Option<String> {
        let wanted = format!("{}.lua", module.to_lowercase().replace('\\', "/"));
        let tail = format!("/{wanted}");
        self.files
            .iter()
            .filter(|(path, _)| *path == wanted || path.ends_with(&tail))
            .min_by_key(|(path, _)| path.len())
            .map(|(_, source)| source.clone())
    }
}

impl Files for () {
    fn script(&self, _: &str) -> Option<String> {
        None
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Config {
    /// Time one callback may run before it is aborted.
    pub callback: Duration,
    /// Time loading the scripts (their data tables) may take.
    pub load: Duration,
    /// Bytes the Lua state may allocate.
    pub memory: usize,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            callback: Duration::from_millis(200),
            load: Duration::from_secs(20),
            memory: 1536 << 20,
        }
    }
}

/// One note a script asked for. Times are milliseconds on the host's clock.
#[derive(Clone, Debug, PartialEq)]
pub struct Play {
    /// The voice id `playNote` returned to the script.
    pub id: u64,
    pub at_ms: f64,
    pub key: u8,
    pub velocity: u8,
    /// Milliseconds until its own release; `Some(0.0)` sends only the note-on
    /// (the script releases it); `None` (-1, or unset) follows the originating
    /// note.
    pub duration_ms: Option<f64>,
    /// 1-based layers it may sound in; empty is all of them.
    pub layers: Vec<u32>,
    /// 1-based oscillator within each keygroup.
    pub osc: Option<u32>,
    pub vol: f64,
    pub pan: f64,
    pub tune: f64,
    /// The script event that caused it, if any.
    pub parent: Option<u64>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Command {
    Play(Play),
    Release {
        id: u64,
        at_ms: f64,
    },
    /// `sendScriptModulation(id, value, glide_ms, voice)`: set (gliding to)
    /// "Script Event Modulation `id`" for one voice, or all when `voice` is None.
    Modulation {
        id: u16,
        value: f64,
        glide_ms: f64,
        voice: Option<u64>,
        at_ms: f64,
    },
}

/// What the host left inert or could not run: feature, one example, count.
#[derive(Clone, Debug, PartialEq)]
pub struct Finding {
    pub feature: String,
    pub value: String,
    pub count: usize,
}

struct Waiting {
    thread: Thread,
    due: f64,
    seq: u64,
    note: Option<u64>,
    release: bool,
}

struct Shared {
    now: Cell<f64>,
    ids: Cell<u64>,
    seq: Cell<u64>,
    /// When the running callback is aborted.
    deadline: Cell<Option<Instant>>,
    current: Cell<Option<u64>>,
    commands: RefCell<Vec<Command>>,
    findings: RefCell<BTreeMap<String, Finding>>,
    waiting: RefCell<Vec<Waiting>>,
    deferred: RefCell<Vec<(Thread, MultiValue, Option<u64>)>>,
    params: RefCell<Vec<Vec<(String, String)>>>,
    /// The preset's saved widget values and table data (ScriptProcessor
    /// attributes and ScriptData), by widget name.
    saved: RefCell<BTreeMap<String, String>>,
    files: Box<dyn Files>,
    config: Config,
}

impl Shared {
    fn find(&self, feature: &str, value: &str) {
        let mut findings = self.findings.borrow_mut();
        match findings.get_mut(feature) {
            Some(f) => f.count += 1,
            None => {
                if findings.len() < 2000 {
                    findings.insert(
                        feature.to_owned(),
                        Finding {
                            feature: feature.to_owned(),
                            value: value.to_owned(),
                            count: 1,
                        },
                    );
                }
            }
        }
    }

    /// Start a time budget for the code about to run.
    fn arm(&self, budget: Duration) {
        self.deadline.set(Some(Instant::now() + budget));
    }

    fn next_id(&self) -> u64 {
        self.ids.set(self.ids.get() + 1);
        self.ids.get()
    }

    fn command(&self, command: Command) {
        let mut commands = self.commands.borrow_mut();
        if commands.len() < 1 << 16 {
            commands.push(command);
        } else {
            drop(commands);
            self.find("command queue full, commands dropped", "");
        }
    }
}

pub struct ScriptHost {
    lua: Lua,
    shared: Rc<Shared>,
}

fn number(value: &Value) -> Option<f64> {
    match value {
        Value::Number(n) => Some(*n),
        Value::Integer(n) => Some(*n as f64),
        _ => None,
    }
}

fn field(table: &Table, name: &str) -> Option<f64> {
    table.get::<Value>(name).ok().as_ref().and_then(number)
}

fn lua_error(e: mlua::Error) -> String {
    // The message plus the innermost frames (file:line), not the whole trace.
    let text = e.to_string();
    let mut lines = text.lines();
    let mut out = lines.next().unwrap_or("").to_owned();
    for frame in lines
        .filter(|l| l.contains(".lua") || l.contains("[string"))
        .take(4)
    {
        out.push_str(" < ");
        out.push_str(
            frame
                .trim()
                .split(':')
                .take(2)
                .collect::<Vec<_>>()
                .join(":")
                .as_str(),
        );
    }
    out
}

/// The elements of a program the scripts can reach (`Program.layers[i]`...).
struct Tree {
    params: Vec<Vec<(String, String)>>,
}

fn element(
    lua: &Lua,
    tree: &mut Tree,
    node: roxmltree::Node,
    parent: Option<&Table>,
) -> mlua::Result<Table> {
    let table = lua.create_table()?;
    let id = tree.params.len();
    tree.params.push(
        node.attributes()
            .map(|a| (a.name().to_owned(), a.value().to_owned()))
            .collect(),
    );
    table.raw_set("__id", id)?;
    table.raw_set("type", node.tag_name().name())?;
    table.raw_set("name", node.attribute("Name").unwrap_or_default())?;
    table.raw_set("bypass", node.attribute("Bypass") == Some("1"))?;
    if let Some(parent) = parent {
        table.raw_set("parent", parent.clone())?;
    }
    let class: Table = lua.globals().raw_get("__element_mt")?;
    table.set_metatable(Some(class))?;
    for container in node.children().filter(|n| n.is_element()) {
        let field = match container.tag_name().name() {
            "Layers" => "layers",
            "Keygroups" => "keygroups",
            "Oscillators" => "oscillators",
            "Inserts" => "inserts",
            "Auxs" | "Chains" => "auxs",
            "BusRouters" => "sends",
            "ControlSignalSources" => "modulations",
            _ => continue,
        };
        let list = lua.create_table()?;
        list.set_metatable(Some(lua.globals().raw_get("__list_mt")?))?;
        for child in container.children().filter(|n| n.is_element()) {
            list.raw_push(element(lua, tree, child, Some(&table))?)?;
        }
        table.raw_set(field, list)?;
    }
    Ok(table)
}

impl ScriptHost {
    /// Run the scripts of the program `xml` (its `ScriptProcessor`s). Fails when
    /// the sandbox cannot be built or a script does not load.
    pub fn new(xml: &str, files: impl Files + 'static, config: Config) -> Result<Self, String> {
        let options = roxmltree::ParsingOptions {
            nodes_limit: 4_000_000,
            ..Default::default()
        };
        let doc =
            roxmltree::Document::parse_with_options(xml, options).map_err(|e| e.to_string())?;
        let lua = Lua::new_with(
            StdLib::TABLE | StdLib::STRING | StdLib::MATH | StdLib::COROUTINE,
            LuaOptions::new(),
        )
        .map_err(lua_error)?;
        lua.set_memory_limit(config.memory).map_err(lua_error)?;
        let shared = Rc::new(Shared {
            now: Cell::new(0.0),
            ids: Cell::new(0),
            seq: Cell::new(0),
            deadline: Cell::new(Some(Instant::now() + config.load)),
            current: Cell::new(None),
            commands: RefCell::new(Vec::new()),
            findings: RefCell::new(BTreeMap::new()),
            waiting: RefCell::new(Vec::new()),
            deferred: RefCell::new(Vec::new()),
            params: RefCell::new(Vec::new()),
            saved: RefCell::new(BTreeMap::new()),
            files: Box::new(files),
            config,
        });
        let host = Self { lua, shared };
        host.install().map_err(lua_error)?;
        host.build_program(&doc).map_err(lua_error)?;
        host.load_scripts(&doc)?;
        Ok(host)
    }

    fn build_program(&self, doc: &roxmltree::Document) -> mlua::Result<()> {
        let program = doc
            .descendants()
            .find(|n| n.has_tag_name("Program"))
            .ok_or_else(|| mlua::Error::runtime("no Program"))?;
        for processor in program
            .descendants()
            .filter(|n| n.has_tag_name("ScriptProcessor"))
        {
            let mut saved = self.shared.saved.borrow_mut();
            for node in std::iter::once(processor).chain(
                processor
                    .children()
                    .filter(|c| c.has_tag_name("ScriptData")),
            ) {
                for a in node.attributes() {
                    saved.insert(a.name().to_owned(), a.value().to_owned());
                }
            }
        }
        let mut tree = Tree { params: Vec::new() };
        let root = element(&self.lua, &mut tree, program, None)?;
        // The part the program sits in (MidiChannel, MidiInput...): inert.
        let part = self.lua.create_table()?;
        part.raw_set("__id", tree.params.len())?;
        tree.params.push(Vec::new());
        part.raw_set("type", "Part")?;
        part.raw_set("name", "")?;
        part.set_metatable(Some(self.lua.globals().raw_get("__element_mt")?))?;
        root.raw_set("parent", part)?;
        *self.shared.params.borrow_mut() = tree.params;
        self.lua.globals().raw_set("Program", root)
    }

    fn install(&self) -> mlua::Result<()> {
        let lua = &self.lua;
        // Only table/string/math/coroutine are loaded (no io, os, debug,
        // package); `lua.sandbox` would give every coroutine its own proxy
        // environment, which breaks the globals the prelude and scripts share.
        let globals = lua.globals();
        let shared = &self.shared;

        // Luau calls the interrupt at calls and loop back-edges: a callback
        // that outlives its time budget is aborted.
        let budget = shared.clone();
        lua.set_interrupt(move |_| {
            if budget.deadline.get().is_some_and(|d| Instant::now() > d) {
                return Err(mlua::Error::runtime("time budget exceeded"));
            }
            Ok(VmState::Continue)
        });

        // Natives the prelude wraps (`__native`) and the engine API (globals).
        let native = lua.create_table()?;
        let s = shared.clone();
        native.set(
            "saved",
            lua.create_function(move |_, name: String| Ok(s.saved.borrow().get(&name).cloned()))?,
        )?;
        let s = shared.clone();
        native.set(
            "paramNames",
            lua.create_function(move |lua, id: usize| {
                let names = lua.create_table()?;
                if let Some(p) = s.params.borrow().get(id) {
                    for (k, _) in p {
                        names.raw_push(k.as_str())?;
                    }
                }
                Ok(names)
            })?,
        )?;
        let s = shared.clone();
        native.set(
            "param",
            lua.create_function(move |lua, (id, name): (usize, String)| {
                let found = s
                    .params
                    .borrow()
                    .get(id)
                    .and_then(|p| p.iter().find(|(k, _)| *k == name))
                    .map(|(_, v)| v.clone());
                Ok(match found {
                    None => Value::Nil,
                    Some(v) => match v.parse::<f64>() {
                        Ok(n) => Value::Number(n),
                        Err(_) => Value::String(lua.create_string(&v)?),
                    },
                })
            })?,
        )?;
        let s = shared.clone();
        native.set(
            "defglobal",
            // The main environment: a coroutine's own is a throwaway proxy.
            lua.create_function({
                let env = globals.clone();
                move |_, (name, value): (String, Value)| env.raw_set(name, value)
            })?,
        )?;
        native.set("nextId", lua.create_function(move |_, ()| Ok(s.next_id()))?)?;
        let s = shared.clone();
        native.set(
            "source",
            lua.create_function(move |_, name: String| Ok(s.files.script(&name)))?,
        )?;
        native.set(
            "compile",
            lua.create_function(|lua, (source, name): (mlua::LuaString, String)| {
                match lua
                    .load(source.as_bytes().as_ref())
                    .set_name(name)
                    .into_function()
                {
                    Ok(f) => Ok((Some(f), None)),
                    Err(e) => Ok((None, Some(lua_error(e)))),
                }
            })?,
        )?;
        globals.raw_set("__native", native)?;
        let s = shared.clone();
        globals.raw_set(
            "__report",
            lua.create_function(move |_, (feature, value): (String, String)| {
                s.find(&format!("lua {feature}"), &value);
                Ok(())
            })?,
        )?;

        let s = shared.clone();
        globals.raw_set(
            "getTime",
            lua.create_function(move |_, ()| Ok(s.now.get()))?,
        )?;
        let s = shared.clone();
        globals.raw_set(
            "playNote",
            lua.create_function(move |_, args: Variadic<Value>| {
                let play = parse_play(&s, &args);
                let id = play.id;
                s.command(Command::Play(play));
                Ok(id)
            })?,
        )?;
        let s = shared.clone();
        globals.raw_set(
            "releaseVoice",
            lua.create_function(move |_, id: f64| {
                s.command(Command::Release {
                    id: id as u64,
                    at_ms: s.now.get(),
                });
                Ok(true)
            })?,
        )?;
        let s = shared.clone();
        globals.raw_set(
            "sendScriptModulation",
            lua.create_function(
                move |_, (id, value, glide, voice): (f64, f64, Option<f64>, Option<f64>)| {
                    s.command(Command::Modulation {
                        id: id.clamp(0.0, f64::from(u16::MAX)) as u16,
                        value,
                        glide_ms: glide.unwrap_or(0.0).max(0.0),
                        voice: voice.map(|v| v as u64),
                        at_ms: s.now.get(),
                    });
                    Ok(())
                },
            )?,
        )?;
        let s = shared.clone();
        globals.raw_set(
            "spawn",
            lua.create_function(move |lua, (f, args): (Function, MultiValue)| {
                let thread = lua.create_thread(f)?;
                s.deferred
                    .borrow_mut()
                    .push((thread, args, s.current.get()));
                Ok(())
            })?,
        )?;
        let s = shared.clone();
        globals.raw_set(
            "run",
            lua.create_function(move |lua, (f, args): (Function, MultiValue)| {
                let thread = lua.create_thread(f)?;
                resume(&s, thread, args, s.current.get());
                Ok(())
            })?,
        )?;
        lua.load(PRELUDE).set_name("prelude").exec()
    }

    fn load_scripts(&self, doc: &roxmltree::Document) -> Result<(), String> {
        for script in doc.descendants().filter(|n| n.has_tag_name("script")) {
            let text: String = script.text().unwrap_or_default().to_owned();
            if text.trim().is_empty() {
                continue;
            }
            self.shared.arm(self.shared.config.load);
            let function = self
                .lua
                .load(&text)
                .set_name("script")
                .into_function()
                .map_err(lua_error)?;
            let thread = self.lua.create_thread(function).map_err(lua_error)?;
            resume(&self.shared, thread, MultiValue::new(), None);
            self.cycle();
            self.call("onInit", None);
        }
        Ok(())
    }

    /// Start spawned threads until none remain.
    fn cycle(&self) {
        loop {
            let next = self.shared.deferred.borrow_mut().pop();
            // Spawned threads start in spawn order.
            let Some(first) = next else { break };
            let mut batch = vec![first];
            batch.append(&mut self.shared.deferred.borrow_mut());
            batch.reverse();
            for (thread, args, note) in batch {
                resume(&self.shared, thread, args, note);
            }
        }
    }

    /// Whether the scripts handle note-ons themselves (the original attack is
    /// then theirs to replay).
    pub fn handles_notes(&self) -> bool {
        matches!(
            self.lua.globals().raw_get::<Value>("onNote"),
            Ok(Value::Function(_))
        )
    }

    fn call(&self, name: &str, event: Option<Table>) {
        let Ok(Value::Function(f)) = self.lua.globals().raw_get::<Value>(name) else {
            return;
        };
        self.shared.arm(self.shared.config.callback);
        let Ok(thread) = self.lua.create_thread(f) else {
            return;
        };
        let note = event
            .as_ref()
            .and_then(|e| field(e, "id"))
            .map(|id| id as u64);
        let args: MultiValue = event.map(Value::Table).into_iter().collect();
        resume(&self.shared, thread, args, note);
        self.cycle();
    }

    fn event(&self, kind: i32, fields: &[(&str, f64)]) -> mlua::Result<Table> {
        let table = self.lua.create_table()?;
        table.set("type", kind)?;
        for (name, value) in fields {
            table.set(*name, *value)?;
        }
        Ok(table)
    }

    /// A note-on the host received, as the script's `onNote(e)`.
    pub fn note_on(&mut self, id: u64, key: u8, velocity: u8, channel: u8) {
        let e = self.event(
            1,
            &[
                ("id", id as f64),
                ("note", f64::from(key)),
                ("velocity", f64::from(velocity)),
                ("channel", f64::from(channel) + 1.0),
            ],
        );
        match e {
            Ok(e) => self.call("onNote", Some(e)),
            Err(e) => self.shared.find("onNote event", &lua_error(e)),
        }
    }

    /// A note-off for the note-on `id`: `onRelease(e)`, and wakes `waitForRelease`.
    pub fn note_off(&mut self, id: u64, key: u8, velocity: u8, channel: u8) {
        let woken: Vec<Waiting> = {
            let mut waiting = self.shared.waiting.borrow_mut();
            let (woken, rest): (Vec<_>, Vec<_>) = std::mem::take(&mut *waiting)
                .into_iter()
                .partition(|w| w.release && w.note == Some(id));
            *waiting = rest;
            woken
        };
        for w in woken {
            self.shared.arm(self.shared.config.callback);
            resume(&self.shared, w.thread, MultiValue::new(), w.note);
        }
        let e = self.event(
            2,
            &[
                ("id", id as f64),
                ("note", f64::from(key)),
                ("velocity", f64::from(velocity)),
                ("channel", f64::from(channel) + 1.0),
            ],
        );
        match e {
            Ok(e) => self.call("onRelease", Some(e)),
            Err(e) => self.shared.find("onRelease event", &lua_error(e)),
        }
        self.cycle();
    }

    pub fn controller(&mut self, controller: u8, value: u8, channel: u8) {
        let e = self.event(
            3,
            &[
                ("controller", f64::from(controller)),
                ("value", f64::from(value)),
                ("channel", f64::from(channel) + 1.0),
            ],
        );
        match e {
            Ok(e) => self.call("onController", Some(e)),
            Err(e) => self.shared.find("onController event", &lua_error(e)),
        }
    }

    /// Move the clock to `now_ms`, resuming every thread whose wait ends by
    /// then, in time order. Commands carry the time they were issued.
    pub fn advance(&mut self, now_ms: f64) {
        loop {
            let next = {
                let mut waiting = self.shared.waiting.borrow_mut();
                let index = waiting
                    .iter()
                    .enumerate()
                    .filter(|(_, w)| !w.release && w.due <= now_ms)
                    .min_by(|(_, a), (_, b)| a.due.total_cmp(&b.due).then(a.seq.cmp(&b.seq)))
                    .map(|(i, _)| i);
                index.map(|i| waiting.swap_remove(i))
            };
            let Some(w) = next else { break };
            self.shared.now.set(w.due.max(self.shared.now.get()));
            self.shared.arm(self.shared.config.callback);
            resume(&self.shared, w.thread, MultiValue::new(), w.note);
            self.cycle();
        }
        self.shared.now.set(now_ms.max(self.shared.now.get()));
    }

    /// Set the clock without resuming anything (a starting point).
    pub fn set_time(&mut self, now_ms: f64) {
        self.shared.now.set(now_ms);
    }

    /// The commands issued since the last call.
    pub fn take_commands(&mut self) -> Vec<Command> {
        std::mem::take(&mut *self.shared.commands.borrow_mut())
    }

    /// The earliest time a waiting thread resumes, if any.
    pub fn next_due(&self) -> Option<f64> {
        self.shared
            .waiting
            .borrow()
            .iter()
            .filter(|w| !w.release)
            .map(|w| w.due)
            .min_by(f64::total_cmp)
    }

    /// Everything inert or failed so far.
    pub fn findings(&self) -> Vec<Finding> {
        self.shared.findings.borrow().values().cloned().collect()
    }

    pub fn memory(&self) -> usize {
        self.lua.used_memory()
    }
}

fn resume(shared: &Rc<Shared>, thread: Thread, args: MultiValue, note: Option<u64>) {
    let before = shared.current.replace(note);
    let result = thread.resume::<MultiValue>(args);
    shared.current.set(before);
    match result {
        Err(e) => shared.find("lua error", &lua_error(e)),
        Ok(values) => {
            if !thread.is_resumable() {
                return;
            }
            let (due, release) = match values.front() {
                Some(Value::String(s)) if s.as_bytes().as_ref() == b"release" => (0.0, true),
                Some(v) => (shared.now.get() + number(v).unwrap_or(0.0).max(0.0), false),
                None => (shared.now.get(), false),
            };
            shared.seq.set(shared.seq.get() + 1);
            shared.waiting.borrow_mut().push(Waiting {
                thread,
                due,
                seq: shared.seq.get(),
                note,
                release,
            });
        }
    }
}

fn parse_play(shared: &Shared, args: &[Value]) -> Play {
    let mut values: [Option<Value>; 11] = Default::default();
    match args {
        [Value::Table(t)] => {
            for (i, name) in [
                "note", "velocity", "duration", "layer", "channel", "input", "vol", "pan", "tune",
                "slice", "oscIndex",
            ]
            .iter()
            .enumerate()
            {
                values[i] = t
                    .get::<Value>(*name)
                    .ok()
                    .filter(|v| !v.is_nil())
                    .or_else(|| {
                        if i < 3 {
                            t.get::<Value>(i as i64 + 1).ok().filter(|v| !v.is_nil())
                        } else {
                            None
                        }
                    });
            }
        }
        _ => {
            for (i, v) in args.iter().take(11).enumerate() {
                if !v.is_nil() {
                    values[i] = Some(v.clone());
                }
            }
        }
    }
    let num = |i: usize| values[i].as_ref().and_then(number);
    let layers = match &values[3] {
        Some(Value::Table(t)) => t
            .sequence_values::<f64>()
            .filter_map(Result::ok)
            .map(|n| n as u32)
            .collect(),
        Some(v) => number(v).map(|n| vec![n as u32]).unwrap_or_default(),
        None => Vec::new(),
    };
    if values[4].is_some() || values[5].is_some() || values[9].is_some() {
        shared.find("lua playNote channel/input/slice", "");
    }
    // lua.uvi.net: > 0 releases after that long, -1 with the originating note,
    // 0 sends only the note-on (the script ends it with releaseVoice).
    let duration = num(2).filter(|d| *d >= 0.0);
    Play {
        id: shared.next_id(),
        at_ms: shared.now.get(),
        key: num(0).unwrap_or(60.0).clamp(0.0, 127.0) as u8,
        velocity: num(1).unwrap_or(100.0).clamp(1.0, 127.0) as u8,
        duration_ms: duration,
        layers,
        osc: num(10).map(|n| n as u32),
        vol: num(6).unwrap_or(1.0),
        pan: num(7).unwrap_or(0.0),
        tune: num(8).unwrap_or(0.0),
        parent: shared.current.get(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn host(script: &str) -> ScriptHost {
        let xml = format!(
            "<UVI4><Program Name='P'><Layers><Layer Name='A'><Keygroups><Keygroup Name='K'>\
             <Oscillators><SamplePlayer Name='o1'/><SamplePlayer Name='o2'/></Oscillators>\
             </Keygroup></Keygroups></Layer></Layers>\
             <EventProcessors><ScriptProcessor Name='S'><script><![CDATA[{script}]]></script>\
             </ScriptProcessor></EventProcessors></Program></UVI4>"
        );
        ScriptHost::new(&xml, (), Config::default()).unwrap()
    }

    #[test]
    fn note_on_plays_the_oscillator_the_script_picks() {
        let mut h = host(
            "local n = 0\n\
             function onNote(e) n = n + 1; playNote(e.note, e.velocity, -1, 1, nil, nil, 1, 0, 0, nil, n) end",
        );
        assert!(h.handles_notes());
        h.note_on(1, 60, 100, 0);
        h.note_on(2, 62, 90, 0);
        let plays: Vec<_> = h
            .take_commands()
            .into_iter()
            .map(|c| match c {
                Command::Play(p) => (p.key, p.velocity, p.osc, p.layers, p.duration_ms, p.parent),
                other => panic!("{other:?}"),
            })
            .collect();
        assert_eq!(
            plays,
            [
                (60, 100, Some(1), vec![1], None, Some(1)),
                (62, 90, Some(2), vec![1], None, Some(2))
            ]
        );
        assert!(h.findings().is_empty(), "{:?}", h.findings());
    }

    #[test]
    fn wait_and_spawn_follow_the_hosts_clock() {
        let mut h = host(
            "function onNote(e)\n\
               spawn(function() wait(100); playNote(e.note + 12, 80, 50) end)\n\
               playNote{ note = e.note, velocity = 70, oscIndex = 2 }\n\
             end",
        );
        h.note_on(7, 60, 100, 0);
        let first = h.take_commands();
        assert_eq!(first.len(), 1);
        assert!(matches!(&first[0], Command::Play(p) if p.osc == Some(2) && p.velocity == 70));
        assert_eq!(h.next_due(), Some(100.0));
        h.advance(99.0);
        assert!(h.take_commands().is_empty());
        h.advance(250.0);
        let later = h.take_commands();
        assert!(
            matches!(&later[..], [Command::Play(p)] if p.key == 72 && p.at_ms == 100.0 && p.duration_ms == Some(50.0))
        );
    }

    #[test]
    fn wait_for_release_resumes_on_note_off_and_release_is_called() {
        let mut h = host(
            "function onNote(e) spawn(function() waitForRelease(); releaseVoice(e.id + 100) end) end\n\
             function onRelease(e) playNote(e.note, 1) end",
        );
        h.note_on(3, 60, 100, 0);
        assert!(h.take_commands().is_empty());
        h.note_off(3, 60, 0, 0);
        let c = h.take_commands();
        assert!(matches!(&c[0], Command::Release { id: 103, .. }));
        assert!(matches!(&c[1], Command::Play(p) if p.velocity == 1));
    }

    #[test]
    fn runaway_scripts_are_aborted_and_reported() {
        let mut h = host("function onNote(e) while true do end end");
        h.note_on(1, 60, 100, 0);
        let findings = h.findings();
        assert!(
            findings
                .iter()
                .any(|f| f.feature == "lua error" && f.value.contains("time budget")),
            "{findings:?}"
        );
        // The host is still usable.
        h.note_on(2, 60, 100, 0);
    }

    #[test]
    fn classes_tables_and_playnote_tables_work() {
        let mut h = host(
            "class 'A'\nfunction A:__init(x) self.x = x end\n\
             class 'B'(A)\nlocal b = B(7)\nassert(b.x == 7)\n\
             local t = Table{'t', 4, 1, 0, 9, true}\n\
             t.changed = function(self, i) lastIndex = i end\n\
             t:setValue(2, 5)\nassert(lastIndex == 2 and t:getValue(2) == 5)\n\
             assert(type(Program.layers[1]) == 'userdata')\n\
             function onNote(e) playNote{e.note, 90, 0, layer=1} end",
        );
        h.note_on(1, 62, 100, 0);
        let c = h.take_commands();
        assert!(
            matches!(&c[..], [Command::Play(p)] if p.key == 62 && p.velocity == 90 && p.duration_ms == Some(0.0)),
            "{c:?} {:?}",
            h.findings()
        );
    }

    #[test]
    fn send_script_modulation_becomes_a_command() {
        let mut h = host("function onNote(e) sendScriptModulation(9, 0.4, 1000, nil) end");
        h.note_on(1, 60, 100, 0);
        let c = h.take_commands();
        assert!(
            matches!(&c[..], [Command::Modulation { id: 9, glide_ms, voice: None, .. }] if *glide_ms == 1000.0),
            "{c:?}"
        );
    }

    #[test]
    fn widgets_export_to_the_ui_ir() {
        let h = host(
            "setSize(400, 200)\n\
             local p = Panel('main')\n\
             p:Knob('gain', 0.5, 0, 1)\n\
             p:Menu{name='mode', items={'a','b'}}\n\
             p:OnOffButton('on', true)",
        );
        let ui = h.interface();
        assert_eq!(ui.source, sampler_ui_ir::Source::FalconLua);
        assert!(ui.widgets.len() >= 4, "{}", ui.widgets.len());
        assert_eq!(ui.pages.len(), 1);
    }

    #[test]
    fn unknown_api_and_ui_are_inert_and_reported_once() {
        let mut h = host(
            "local p = Panel('main')\n\
             local k = p:Knob('gain', 0.5, 0, 1)\n\
             k.changed = function(self) playNote(61, 100) end\n\
             k:setValue(0.75)\n\
             Mystery.thing:go(1, 2)\n\
             Program.layers[1].keygroups[1].oscillators[2]:setParameter('Gain', 0.5)\n\
             assert(Program.layers[1].keygroups[1].oscillators[1].name == 'o1')\n\
             assert(k.value == 0.75)",
        );
        let features: Vec<_> = h.findings().into_iter().map(|f| f.feature).collect();
        assert!(features.contains(&"lua global".to_owned()), "{features:?}");
        assert!(!features.contains(&"lua error".to_owned()), "{features:?}");
        assert!(
            features.contains(&"lua setParameter".to_owned()),
            "{features:?}"
        );
        assert!(matches!(&h.take_commands()[..], [Command::Play(p)] if p.key == 61));
    }

    #[test]
    fn the_sandbox_has_no_io_os_or_package_access() {
        let mut h = host("function onNote(e) playNote(io == nil and 1 or 2, 1) end");
        h.note_on(1, 60, 100, 0);
        // `io` is an unknown global: an inert stub, never the real library.
        let c = h.take_commands();
        assert!(matches!(&c[0], Command::Play(p) if p.key == 2));
        assert!(h.findings().iter().any(|f| f.value == "io"));
    }
}
