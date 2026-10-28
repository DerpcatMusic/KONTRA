//! UVIScript (Luau) on the engine: a [`Machine`] whose callbacks call the
//! same [`HostApi`] as KSP (04-SCRIPT.md §4). The UVI API lives in
//! `prelude.lua` on one host function, `__host(action, ...)`, which exists
//! only while a callback runs (an `mlua` scope borrowing the host), so no
//! unsafe code holds the engine.
//!
//! ponytail: the Luau state allocates from the system heap on the audio
//! thread; a fixed arena allocator sized at prepare is the next step
//! (04-SCRIPT.md §4, "Real-time safety").

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use kontra_vm::{Action, CoId, Faults, Foreign, HostApi, HostCall, HostValue, Machine, Step, Text};
use mlua::thread::ThreadStatus;
use mlua::{Function, Lua, LuaOptions, StdLib, Table, Thread, Value, Variadic};

const PRELUDE: &str = include_str!("prelude.lua");
/// Luau heap per script.
const MEMORY: usize = 64 << 20;
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
    let modules: Arc<HashMap<String, String>> = Arc::new(
        modules
            .iter()
            .map(|(name, text)| (module_name(name), text.clone()))
            .collect(),
    );
    let saved: Arc<Vec<(String, String)>> = Arc::new(
        saved
            .iter()
            .filter_map(|s| s.split_once('='))
            .map(|(name, value)| (name.into(), value.into()))
            .collect(),
    );
    let lua = state(source, &modules, &saved).map_err(|e| e.to_string())?;
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
    })
}

impl Foreign for Compiled {
    fn machine(&self) -> Box<dyn Machine> {
        let machine = state(&self.source, &self.modules, &self.saved).map(|lua| LuaMachine {
            lua: Some(lua),
            slots: (0..COROUTINES).map(|_| Slot::default()).collect(),
            spawned: Vec::with_capacity(COROUTINES),
            faults: Faults::default(),
        });
        // `compile` loaded the same source, so this fails only on memory.
        Box::new(machine.unwrap_or_else(|_| LuaMachine::empty()))
    }
}

/// `require("a.b")` and a member `a/b.lua` name the same module.
fn module_name(name: &str) -> String {
    let name = name.trim_end_matches(".lua").replace(['/', '\\'], ".");
    name.rsplit('.').next().unwrap_or(&name).to_lowercase()
}

/// A sandboxed state with the prelude, `require` and `source` loaded.
fn state(
    source: &str,
    modules: &Arc<HashMap<String, String>>,
    saved: &[(String, String)],
) -> mlua::Result<Lua> {
    let libs = StdLib::TABLE | StdLib::STRING | StdLib::MATH | StdLib::COROUTINE | StdLib::BIT;
    let lua = Lua::new_with(libs, LuaOptions::new())?;
    lua.set_memory_limit(MEMORY)?;
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
        let text = found
            .get(&key)
            .ok_or_else(|| mlua::Error::runtime(format!("module {name} not found")))?;
        let value: Value = lua.load(text.as_str()).set_name(&name).call(())?;
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
    Ok(lua)
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
    slots: Vec<Slot>,
    spawned: Vec<(CoId, u64)>,
    faults: Faults,
}

impl LuaMachine {
    fn empty() -> Self {
        Self {
            lua: None,
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
        let checkpoints = AtomicU32::new(0);
        lua.set_interrupt(move |_| {
            if checkpoints.fetch_add(1, Ordering::Relaxed) > CHECKPOINTS {
                return Err(mlua::Error::runtime("callback ran too long"));
            }
            Ok(mlua::VmState::Continue)
        });
        let resumed = lua.scope(|scope| {
            let call = scope.create_function_mut(|_, args: Variadic<Value>| {
                Ok(host_call(host, context, &args))
            })?;
            lua.globals().set("__host", call)?;
            let result = thread.resume::<Value>(());
            lua.globals().set("__host", Value::Nil)?;
            result
        });
        lua.remove_interrupt();
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
        self.spawned.pop()
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

    /// Two layers; the second's keygroups made engine groups 3 and 4.
    const TREE: &str = "return {params={DisplayName='Horn'},db=0,pan=0,layers={
        {params={DisplayName='FX',Gain='1'},keygroups={{params={},group=0}}},
        {params={DisplayName='Sustain',Gain='0'},sendPre=1,sendPost=0,
         keygroups={{params={Gain='1'},group=3},{params={},group=nil},{params={},group=4}}}}}";

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
            [
                vec![7, 9, 0, 0x3FFF_FFFF],
                vec![7, 9, 1, 3],
                vec![7, 9, 1, 4]
            ]
        );
        assert!(
            log.0
                .iter()
                .any(|(a, ints)| *a == Action::ChangeVol && ints.first() == Some(&7))
        );
    }

    #[test]
    fn a_layers_gain_sets_its_groups_with_its_aux_send() {
        // Gain 0 plus a unity pre-fader send is unity; muting silences it.
        let log = run("", Callbacks::INIT);
        let volume = |log: &Log, group: i64| {
            log.0
                .iter()
                .rev()
                .find(|(a, ints)| *a == Action::GroupVolume && ints.first() == Some(&group))
                .map(|(_, ints)| ints.get(1).copied())
        };
        assert_eq!(volume(&log, 3), Some(Some(0)));
        let muted = "Program.layers[2]:setParameter('Mute', true)";
        assert_eq!(
            volume(&run(muted, Callbacks::INIT), 4),
            Some(Some(-200_000))
        );
    }
}
