//! UVIScript (Luau) on the engine: a [`Machine`] whose callbacks call the
//! same [`HostApi`] as KSP (04-SCRIPT.md §4). The UVI API lives in
//! `prelude.lua` on one host function, `__host(action, ...)`, which exists
//! only while a callback runs (an `mlua` scope borrowing the host), so no
//! unsafe code holds the engine.
//!
//! The Luau heap lives in [`alloc`]'s arena: mlua 0.12 allocates through the
//! Rust global allocator, so this crate installs [`alloc::Routed`] as that
//! allocator. A binary linking this crate takes it; a second
//! `#[global_allocator]` fails to link.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

use kontra_vm::{Action, CoId, Faults, Foreign, HostApi, HostCall, HostValue, Machine, Step, Text};
use mlua::thread::ThreadStatus;
use mlua::{Function, Lua, LuaOptions, StdLib, Table, Thread, Value, Variadic};

pub mod alloc;
mod interrupt;
mod literal;

#[global_allocator]
static ROUTED: alloc::Routed = alloc::Routed;

const PRELUDE: &str = include_str!("prelude.lua");
/// Luau heap per script. Augmented Orchestra's scripts, once they see their
/// modulation connections, build about 88 MB (V Strings Pizzicato 2: 61 MB
/// without them). ponytail: a cap, not a GC budget; past the spike's 0.5 ms
/// cycle boundary (spikes/luau-alloc-mlua.md) either way.
const MEMORY: usize = 128 << 20;
/// Free arena kept after a load for its callbacks: at least this, or half
/// the heap the load built (spikes/luau-alloc-mlua.md, "Per-script arena rule").
const HEADROOM: usize = 8 << 20;
/// Interrupt checkpoints (calls and loop back-edges) one run may take before
/// it is stopped: a runaway loop must not hold the audio thread.
const CHECKPOINTS: u32 = 10_000_000;
/// Callbacks that may run or wait at once.
const COROUTINES: usize = 64;

/// Which UVI callbacks a script defines, by the engine's entry number.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Callbacks {
    pub init: bool,
    pub note: bool,
    pub release: bool,
    pub controller: bool,
}

impl Callbacks {
    pub const INIT: u32 = 0;
    pub const NOTE: u32 = 1;
    pub const RELEASE: u32 = 2;
    pub const CONTROLLER: u32 = 3;
}

/// A script that loaded once: its source, the modules `require` finds, and
/// the callbacks it defines.
#[derive(Debug)]
pub struct Compiled {
    source: String,
    modules: Arc<HashMap<String, String>>,
    saved: Arc<Vec<(String, String)>>,
    pub callbacks: Callbacks,
    /// The state that load built, for the first machine (a big program's
    /// state is tens of MB and a second of load).
    built: Mutex<Option<(Lua, Arc<AtomicU32>)>>,
}

/// Loads `source` once, with `modules` (name, source) for `require` and the
/// widgets' `saved` values (`name=value`), to learn its callbacks. Each
/// engine then builds its own state from it.
///
/// # Errors
/// The script's syntax or load-time error, as Luau reports it.
pub fn compile(
    source: &str,
    modules: &[(String, String)],
    saved: &[String],
) -> Result<Compiled, String> {
    let (modules, saved) = inputs(modules, saved);
    let load = alloc::Scope::load();
    let built = state(source, &modules, &saved).map_err(|e| e.to_string());
    drop(load);
    let (lua, checkpoints) = built?;
    let defined = |name: &str| matches!(lua.globals().get::<Value>(name), Ok(Value::Function(_)));
    let callbacks = Callbacks {
        // Always: init pushes the levels the program tree sets.
        init: true,
        note: defined("onNote"),
        release: defined("onRelease"),
        controller: defined("onController"),
    };
    Ok(Compiled {
        source: source.into(),
        modules,
        saved,
        callbacks,
        built: Mutex::new(Some((lua, checkpoints))),
    })
}

/// A sample the script puts on an oscillator as the program loads: the
/// engine group of the oscillator's keygroup, its slot there, the path the
/// script names, and the base note it set.
#[derive(Clone, Debug, PartialEq)]
pub struct SampleLoad {
    pub group: u32,
    pub slot: u16,
    pub path: String,
    pub root: Option<u8>,
}

/// The modules by the name `require` finds them, and the saved widget
/// values split at `=`.
type Inputs = (Arc<HashMap<String, String>>, Arc<Vec<(String, String)>>);

fn inputs(modules: &[(String, String)], saved: &[String]) -> Inputs {
    let modules = modules
        .iter()
        .map(|(name, text)| (module_name(name), text.clone()))
        .collect();
    let saved = saved
        .iter()
        .filter_map(|s| s.split_once('='))
        .map(|(name, value)| (name.into(), value.into()))
        .collect();
    (Arc::new(modules), Arc::new(saved))
}

/// Runs a program's load once, off the audio thread (init, then `onLoad`),
/// and returns the samples it loads; arguments as [`compile`]. A script that
/// fails part way returns what it loaded before.
#[must_use]
pub fn sample_loads(
    source: &str,
    modules: &[(String, String)],
    saved: &[String],
) -> Vec<SampleLoad> {
    let (modules, saved) = inputs(modules, saved);
    let Ok((lua, checkpoints)) = state(source, &modules, &saved) else {
        return Vec::new();
    };
    checkpoints.store(0, Ordering::Relaxed);
    let Ok(loads) = lua
        .globals()
        .get::<Function>("__record")
        .and_then(|record| record.call::<Table>(()))
    else {
        return Vec::new();
    };
    loads
        .sequence_values::<Table>()
        .filter_map(|load| {
            let load = load.ok()?;
            let int = |i: usize| number(&load.get::<Value>(i).ok()?);
            Some(SampleLoad {
                group: u32::try_from(int(1)? as i64).ok()?,
                slot: u16::try_from(int(2)? as i64).ok()?,
                path: load.get::<String>(3).ok()?,
                root: int(4).and_then(|n| u8::try_from(n as i64).ok()),
            })
        })
        .collect()
}

impl Foreign for Compiled {
    fn machine(&self) -> Box<dyn Machine> {
        let load = alloc::Scope::load();
        let built = self.built.lock().ok().and_then(|mut b| b.take());
        let machine = built
            .map_or_else(|| state(&self.source, &self.modules, &self.saved), Ok)
            .map(|(lua, checkpoints)| LuaMachine {
                lua: Some(lua),
                checkpoints,
                slots: (0..COROUTINES).map(|_| Slot::default()).collect(),
                spawned: Vec::with_capacity(COROUTINES),
                faults: Faults::default(),
            });
        drop(load);
        if let Ok(m) = &machine
            && let Some(lua) = &m.lua
        {
            alloc::reserve(HEADROOM.max(lua.used_memory() / 2));
        }
        // `compile` loaded the same source, so this fails only on memory.
        Box::new(machine.unwrap_or_else(|_| LuaMachine::empty()))
    }
}

/// `require("a.b")` and a member `a/b.lua` name the same module.
fn module_name(name: &str) -> String {
    let name = name.trim_end_matches(".lua").replace(['/', '\\'], ".");
    name.rsplit('.').next().unwrap_or(&name).to_lowercase()
}

/// A sandboxed state with the prelude, `require` and `source` loaded, and
/// the count its interrupt stops at [`CHECKPOINTS`].
fn state(
    source: &str,
    modules: &Arc<HashMap<String, String>>,
    saved: &[(String, String)],
) -> mlua::Result<(Lua, Arc<AtomicU32>)> {
    let libs = StdLib::TABLE | StdLib::STRING | StdLib::MATH | StdLib::COROUTINE | StdLib::BIT;
    let lua = Lua::new_with(libs, LuaOptions::new())?;
    lua.set_memory_limit(MEMORY)?;
    // Luau seeds `math.random` from the clock; a fixed seed makes a program
    // play the same on every load, as KSP's `random` does (Behaviour's rng),
    // so renders can be compared.
    lua.load("math.randomseed(0x5EED)").exec()?;
    // Installed once and bounding the load too: the caller zeroes the count
    // before each run, so a runaway loop stops the run, not the host.
    let checkpoints = Arc::new(AtomicU32::new(0));
    let count = Arc::clone(&checkpoints);
    lua.set_interrupt(move |_| {
        if count.fetch_add(interrupt::STRIDE, Ordering::Relaxed) > CHECKPOINTS {
            return Err(mlua::Error::runtime("script ran too long"));
        }
        Ok(mlua::VmState::Continue)
    });
    interrupt::install(&lua)?;
    let actions = lua.create_table()?;
    for &action in Action::all() {
        actions.set(format!("{action:?}"), action.id())?;
    }
    let globals = lua.globals();
    globals.set("Action", actions)?;
    globals.set("__spawned", lua.create_table()?)?;
    globals.set("__saved", lua.create_table_from(saved.iter().cloned())?)?;
    let found = Arc::clone(modules);
    let loaded = lua.create_table()?;
    let require = lua.create_function(move |lua, name: String| {
        let key = module_name(&name);
        if let Ok(value) = loaded.get::<Value>(key.as_str())
            && !value.is_nil()
        {
            return Ok(value);
        }
        // `uvi.*` modules are built in: the prelude defines their globals.
        if name.starts_with("uvi.") && !found.contains_key(&key) {
            return Ok(Value::Boolean(true));
        }
        let text = found
            .get(&key)
            .ok_or_else(|| mlua::Error::runtime(format!("module {name} not found")))?;
        let value = match literal::read(lua, text) {
            Some(value) => value,
            None => lua.load(text.as_str()).set_name(&name).call(())?,
        };
        let value = if value.is_nil() {
            Value::Boolean(true)
        } else {
            value
        };
        loaded.set(key, value.clone())?;
        Ok(value)
    })?;
    globals.set("require", require)?;
    lua.load(PRELUDE).set_name("prelude").exec()?;
    lua.load(source).set_name("script").exec()?;
    globals.get::<Function>("__restore")?.call::<()>(())?;
    Ok((lua, checkpoints))
}

#[derive(Default)]
struct Slot {
    thread: Option<Thread>,
    generation: u16,
    context: u64,
}

struct LuaMachine {
    /// `None` when the state could not be built: every callback is refused.
    lua: Option<Lua>,
    /// The interrupt's count for the current run.
    checkpoints: Arc<AtomicU32>,
    slots: Vec<Slot>,
    spawned: Vec<(CoId, u64)>,
    faults: Faults,
}

impl LuaMachine {
    fn empty() -> Self {
        Self {
            lua: None,
            checkpoints: Arc::default(),
            slots: Vec::new(),
            spawned: Vec::new(),
            faults: Faults::default(),
        }
    }

    /// Puts `thread` in a free slot.
    fn take(&mut self, thread: Thread, context: u64) -> Option<CoId> {
        let Some((i, slot)) = self
            .slots
            .iter_mut()
            .enumerate()
            .find(|(_, s)| s.thread.is_none())
        else {
            self.faults.no_coroutine += 1;
            return None;
        };
        let index = u16::try_from(i).ok()?;
        slot.generation = slot.generation.wrapping_add(1);
        slot.thread = Some(thread);
        slot.context = context;
        Some(CoId::new(index, slot.generation))
    }

    fn slot(&mut self, co: CoId) -> Option<&mut Slot> {
        self.slots
            .get_mut(usize::from(co.index()))
            .filter(|s| s.generation == co.generation() && s.thread.is_some())
    }

    /// Moves the coroutines `spawn` queued during the run into slots.
    fn collect_spawned(&mut self, lua: &Lua, context: u64) {
        let Ok(queued) = lua.globals().get::<Table>("__spawned") else {
            return;
        };
        let pairs: Vec<(Thread, f64)> = queued
            .sequence_values::<Table>()
            .flatten()
            .filter_map(|pair| Some((pair.get(1).ok()?, pair.get(2).ok()?)))
            .collect();
        let _ = queued.clear();
        for (thread, micros) in pairs {
            if let Some(co) = self.take(thread, context) {
                self.spawned.push((co, micros.max(0.0) as u64));
            }
        }
    }
}

impl Machine for LuaMachine {
    fn start(&mut self, entry: u32, context: u64) -> Option<CoId> {
        let _arena = alloc::Scope::audio();
        let lua = self.lua.as_ref()?;
        let callback = lua
            .globals()
            .get::<Table>("__entries")
            .and_then(|t| t.get::<Function>(entry))
            .ok()?;
        let thread = lua.create_thread(callback).ok()?;
        self.take(thread, context)
    }

    fn run(&mut self, co: CoId, host: &mut dyn HostApi) -> Step {
        let _arena = alloc::Scope::audio();
        let Some(lua) = self.lua.clone() else {
            return Step::Done;
        };
        let Some(slot) = self.slot(co) else {
            return Step::Done;
        };
        let context = slot.context;
        let Some(thread) = slot.thread.clone() else {
            return Step::Done;
        };
        self.checkpoints.store(0, Ordering::Relaxed);
        let resumed = lua.scope(|scope| {
            let call = scope.create_function_mut(|_, args: Variadic<Value>| {
                Ok(host_call(host, context, &args))
            })?;
            lua.globals().set("__host", call)?;
            let result = thread.resume::<Value>(());
            lua.globals().set("__host", Value::Nil)?;
            result
        });
        self.collect_spawned(&lua, context);
        let step = match (resumed, thread.status()) {
            (Ok(waited), ThreadStatus::Resumable) => {
                Step::Wait(number(&waited).unwrap_or(0.0).max(0.0) as u64)
            }
            (Ok(_), _) => Step::Done,
            (Err(_), _) => {
                self.faults.bad_code += 1;
                Step::Done
            }
        };
        if step == Step::Done
            && let Some(slot) = self.slot(co)
        {
            slot.thread = None;
        }
        step
    }

    fn cancel(&mut self, co: CoId) {
        let _arena = alloc::Scope::audio();
        if let Some(slot) = self.slot(co) {
            slot.thread = None;
        }
    }

    fn context(&self, co: CoId) -> Option<u64> {
        self.slots
            .get(usize::from(co.index()))
            .filter(|s| s.generation == co.generation() && s.thread.is_some())
            .map(|s| s.context)
    }

    fn spawned(&mut self) -> Option<(CoId, u64)> {
        // In spawn order; at most `COROUTINES` entries.
        (!self.spawned.is_empty()).then(|| self.spawned.remove(0))
    }

    fn faults(&self) -> Faults {
        self.faults
    }
}

/// A Lua number, whichever way the state holds it.
fn number(v: &Value) -> Option<f64> {
    match v {
        Value::Integer(i) => Some(*i as f64),
        Value::Number(n) => Some(*n),
        _ => None,
    }
}

/// `__host(action, ...)`: numbers become int arguments, strings text ones.
fn host_call(host: &mut dyn HostApi, context: u64, args: &[Value]) -> Value {
    let Some(action) = args
        .first()
        .and_then(number)
        .and_then(|id| Action::from_id(id as u16))
    else {
        return Value::Nil;
    };
    let mut ints = [0i64; 8];
    let mut texts = [Text::EMPTY; 4];
    let (mut n_int, mut n_text) = (0, 0);
    for value in args.iter().skip(1) {
        match value {
            Value::String(s) => {
                if let Some(t) = texts.get_mut(n_text) {
                    *t = Text::from_str_cut(&s.to_string_lossy()).0;
                    n_text += 1;
                }
            }
            other => {
                if let Some(i) = ints.get_mut(n_int) {
                    *i = match other {
                        Value::Boolean(b) => i64::from(*b),
                        _ => number(other).unwrap_or(0.0) as i64,
                    };
                    n_int += 1;
                }
            }
        }
    }
    let mut text_out = Text::EMPTY;
    let value = host.call(HostCall {
        action,
        context,
        ints: ints.get(..n_int).unwrap_or_default(),
        texts: texts.get(..n_text).unwrap_or_default(),
        cells: &mut [],
        text_out: &mut text_out,
    });
    match value {
        HostValue::Nothing => Value::Nil,
        HostValue::Int(i) => Value::Number(f64::from(i)),
        HostValue::Real(r) => Value::Number(r),
        HostValue::Text => Value::Nil,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Log(Vec<(Action, Vec<i64>)>);

    impl HostApi for Log {
        fn call(&mut self, call: HostCall<'_>) -> HostValue {
            self.0.push((call.action, call.ints.to_vec()));
            HostValue::Int(7)
        }
    }

    #[test]
    fn a_note_callback_reaches_the_host() {
        let compiled =
            compile("function onNote(e) playNote(e.note + 7, 100) end", &[], &[]).unwrap();
        assert!(compiled.callbacks.note);
        let mut m = compiled.machine();
        let co = m.start(Callbacks::NOTE, 0).expect("start");
        let mut log = Log(Vec::new());
        assert_eq!(m.run(co, &mut log), Step::Done);
        assert_eq!(m.faults().bad_code, 0);
        let actions: Vec<Action> = log.0.iter().map(|c| c.0).collect();
        assert!(actions.contains(&Action::IgnoreEvent), "{actions:?}");
        assert_eq!(log.0.last().map(|c| c.1.clone()), Some(vec![14, 100, 0, 0]));
    }

    #[test]
    fn a_runaway_top_level_loop_fails_the_load() {
        assert!(compile("while true do end", &[], &[]).is_err());
        assert!(sample_loads("while true do end", &[], &[]).is_empty());
    }

    #[test]
    fn a_callback_allocates_from_the_arena_without_spilling() {
        let source = "function onNote(e) local t = {} for i = 1, 20000 do t[i] = {i} end end";
        let mut m = compile(source, &[], &[]).unwrap().machine();
        let before = alloc::spills();
        let co = m.start(Callbacks::NOTE, 0).expect("start");
        assert_eq!(m.run(co, &mut Log(Vec::new())), Step::Done);
        assert_eq!(m.faults().bad_code, 0);
        assert_eq!(alloc::spills(), before);
    }

    /// Two layers; the second's keygroups made engine groups 3 and 4.
    const TREE: &str = "return {params={DisplayName='Horn'},db=0,pan=0,layers={
        {params={DisplayName='FX',Gain='1'},bus=1,keygroups={{params={},group=0}},
         busRouters={{params={Name='Send0',Gain='0.5'}}}},
        {params={DisplayName='Sustain',Gain='0'},bus=2,
         keygroups={{params={Gain='1'},group=3},{params={},group=nil},{params={},group=4}}}},
        auxs={{params={Name='Aux0',Gain='0.5'},bus=3}}}";

    fn run(source: &str, entry: u32) -> Log {
        let modules = [("__program.lua".to_string(), TREE.to_string())];
        let compiled = compile(source, &modules, &[]).unwrap();
        let mut m = compiled.machine();
        let co = m.start(entry, 0).expect("start");
        let mut log = Log(Vec::new());
        assert_eq!(m.run(co, &mut log), Step::Done);
        assert_eq!(m.faults().bad_code, 0);
        log
    }

    #[test]
    fn play_note_on_a_layer_allows_only_that_layers_groups() {
        let source = "assert(Program:getParameter('DisplayName') == 'Horn')
            function onNote(e) playNote{e.note, 90, 0, layer = findLayer('Sustain'), vol = 0.5} end";
        let log = run(source, Callbacks::NOTE);
        let groups: Vec<Vec<i64>> = log
            .0
            .iter()
            .filter(|(a, _)| *a == Action::SetEventParArr)
            .map(|(_, ints)| ints.clone())
            .collect();
        assert_eq!(
            groups,
            [vec![7, 10, 0, -1], vec![7, 10, 1, 3], vec![7, 10, 1, 4]]
        );
        assert!(
            log.0
                .iter()
                .any(|(a, ints)| *a == Action::ChangeVol && ints.first() == Some(&7))
        );
    }

    #[test]
    fn the_load_pass_runs_onload_with_the_saved_state_and_records_its_samples() {
        let tree = "return {params={},db=0,pan=0,layers={{params={},keygroups={
            {params={},group=7,oscillators={{params={BaseNote='24'}},{params={}}}}}}}}";
        let state = "return {{name='Crescendo'}}";
        let source = "function onLoad(data)
            local osc = Program.layers[1].keygroups[1].oscillators[2]
            osc:setParameter('BaseNote', 36)
            wait(50)
            loadSample(osc, data[1].name .. '.wav')
        end";
        let modules = [
            ("__program.lua".to_string(), tree.to_string()),
            ("__state.lua".to_string(), state.to_string()),
        ];
        let load = SampleLoad {
            group: 7,
            slot: 1,
            path: "Crescendo.wav".into(),
            root: Some(36),
        };
        assert_eq!(sample_loads(source, &modules, &[]), vec![load]);
    }

    #[test]
    fn elements_keep_their_shape_when_their_containers_are_made_on_use() {
        // AO's _Engine/Part sets `layer.sends[i]`; an empty container is one table.
        let source = "local fx, sustain = Program.layers[1], Program.layers[2]
            assert(type(fx) == 'userdata' and fx.sends == fx.busRouters)
            fx.sends[1]:setParameter('Gain', 0.25)
            assert(fx.busRouters[1]:getParameter('Gain') == 0.25 and fx.busRouters[1].parent == fx)
            assert(#sustain.sends == 0 and #sustain.inserts == 0)
            assert(sustain.inserts == sustain.inserts and sustain:noSuchMethod() == nil)
            local list = sustain:getParameterConnections('Gain')
            assert(#list == 0 and list[1]:getParameter('Ratio') == 0)
            assert(sustain:getParameterConnections('Gain') == list)";
        run(source, Callbacks::INIT);
    }

    #[test]
    fn a_layers_fader_and_an_auxs_gain_are_bus_volumes() {
        // The fader leaves the groups at their own gain; muting silences them.
        let log = run("", Callbacks::INIT);
        let set = |log: &Log, action: Action, target: i64| {
            log.0
                .iter()
                .rev()
                .find(|(a, ints)| *a == action && ints.first() == Some(&target))
                .map(|(_, ints)| ints.get(1).copied())
        };
        let volume = |log: &Log, group: i64| set(log, Action::GroupVolume, group);
        assert_eq!(volume(&log, 3), Some(Some(0)));
        assert_eq!(set(&log, Action::BusVolume, 2), Some(Some(-200_000)));
        assert_eq!(set(&log, Action::BusVolume, 3), Some(Some(-6_021)));
        let muted = "Program.layers[2]:setParameter('Mute', true)";
        assert_eq!(
            volume(&run(muted, Callbacks::INIT), 4),
            Some(Some(-200_000))
        );
    }

    #[test]
    fn a_saved_widget_restored_after_init_sets_a_connections_route() {
        let tree = "return {params={},db=0,pan=0,layers={{params={},keygroups={
            {params={},group=0,links={2,1}}}}},
            connections={{'@MIDI CC 1','Gain',0,43,1},{'$Program/LFO 1','Pan',1,42,-0.5}}}";
        let source = "local kg = Program.layers[1].keygroups[1]
            local pans = kg:getParameterConnections('Pan')
            assert(#pans == 1 and pans[1]:getParameter('Source') == '$Program/LFO 1')
            assert(#kg:getParameterConnections('Gain') == 1)
            local ratio
            for _, d in ipairs(pans[1].parameterDefinitions) do
                if d.name == 'Ratio' then ratio = d.id end
            end
            -- Built at its default; its saved 0.25 comes back after the body.
            local amount = Knob('Amount', 0, -1, 1)
            amount.changed = function(k) pans[1]:setParameter(ratio, k.value) end
            amount:changed()
            assert(amount.value == 0)
            assert(pans[1]:getParameter('Ratio') == 0)";
        let modules = [("__program.lua".to_string(), tree.to_string())];
        let saved = ["Amount=0.25".to_string()];
        let mut m = compile(source, &modules, &saved).unwrap().machine();
        let co = m.start(Callbacks::INIT, 0).expect("start");
        let mut log = Log(Vec::new());
        assert_eq!(m.run(co, &mut log), Step::Done);
        assert_eq!(m.faults().bad_code, 0);
        // −0.5 a unit of ratio at the saved 0.25: the default 0 it was built
        // at was set before init too, so only the last amount reaches the host.
        let amounts: Vec<_> = (log.0.iter())
            .filter(|(a, _)| *a == Action::RouteAmount)
            .map(|(_, ints)| ints.clone())
            .collect();
        assert_eq!(amounts, [vec![42, -125_000]]);
    }

    #[test]
    fn a_connections_bypass_takes_its_route_amount_away_and_gives_it_back() {
        // A place holder (key −1) ahead of a bypassed route keeps the order.
        let tree = "return {params={},db=0,pan=0,layers={{params={},keygroups={
            {params={},group=0,links={1,2}}}}},
            connections={{'@VoiceParam Velocity','Freq',0.5,-1,1,false},{'@MIDI CC 1','Freq',0.5,7,1,true}}}";
        let source = "local c = Program.layers[1].keygroups[1]:getParameterConnections('Freq')
            assert(c[2]:getParameter('Source') == '@MIDI CC 1' and c[2]:getParameter('Bypass'))
            c[1]:setParameter('Bypass', true)
            c[2]:setParameter('Ratio', 0.25)
            c[2]:setParameter('Bypass', false)
            c[2]:setParameter('Bypass', true)";
        let modules = [("__program.lua".to_string(), tree.to_string())];
        let mut m = compile(source, &modules, &[]).unwrap().machine();
        let co = m.start(Callbacks::INIT, 0).expect("start");
        let mut log = Log(Vec::new());
        assert_eq!(m.run(co, &mut log), Step::Done);
        assert_eq!(m.faults().bad_code, 0);
        let amounts: Vec<_> = (log.0.iter())
            .filter(|(a, _)| *a == Action::RouteAmount)
            .map(|(_, ints)| ints.clone())
            .collect();
        // Set while loading: the route's last amount, bypassed, arrives at init.
        assert_eq!(amounts, [vec![7, 0]]);
        let live = "function onInit() c[2]:setParameter('Bypass', false) c[2]:setParameter('Bypass', true) end";
        let source = format!("{source}\n{live}");
        let mut m = compile(&source, &modules, &[]).unwrap().machine();
        let co = m.start(Callbacks::INIT, 0).expect("start");
        let mut log = Log(Vec::new());
        assert_eq!(m.run(co, &mut log), Step::Done);
        let amounts: Vec<_> = (log.0.iter())
            .filter(|(a, _)| *a == Action::RouteAmount)
            .map(|(_, ints)| ints.clone())
            .collect();
        assert_eq!(amounts, [vec![7, 0], vec![7, 250_000], vec![7, 0]]);
    }

    #[test]
    fn math_random_draws_the_same_on_every_load() {
        let draw = || {
            let modules = Arc::new(HashMap::new());
            let (lua, _) = state("", &modules, &[]).unwrap();
            lua.load("return math.random(1, 1000000), math.random()")
                .eval::<(i64, f64)>()
                .unwrap()
        };
        assert_eq!(draw(), draw());
    }
}
