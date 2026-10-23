//! Control-side host for compiled KSP. One Luau VM per instrument, one coroutine
//! per running callback. `wait(us)` yields; the host resumes the coroutine at the
//! exact sample frame the interpreter would (`sampler-core` `wait_behavior`: the
//! micro-frame carry is kept per callback), so timing does not depend on how the
//! caller splits its render blocks.

use super::Compiled;
use mlua::{Function, Lua, MultiValue, Table, Thread, Value, VmState};
use sampler_script::{Budget, new_vm};
use std::{cell::RefCell, collections::BTreeMap, rc::Rc};

/// Checkpoints one callback may spend before it is aborted (deterministic, not timed).
const CALLBACK_CHECKPOINTS: u64 = 1_000_000;
/// Luau memory cap for one instrument VM.
const MEMORY: usize = 64 << 20;

struct Job {
    thread: Thread,
    /// Frames the waits so far overshot their exact time, in micro-frames (< 1e6).
    carry: u32,
    note: u32,
    /// A release callback: its note's polyphonic cells are freed once it finishes.
    ends_note: bool,
    /// Event arguments, passed on the first resume only.
    args: Vec<Value>,
}

pub struct Host {
    lua: Lua,
    rate: u32,
    cells: Table,
    texts: Table,
    callbacks: Table,
    select_note: Function,
    /// Polyphonic cells per note id.
    notes: BTreeMap<u32, Table>,
    note_cells: u16,
    now: u64,
    seq: u64,
    queue: BTreeMap<(u64, u64), Job>,
    engine: Rc<RefCell<BTreeMap<[i64; 4], i64>>>,
    messages: Rc<RefCell<Vec<String>>>,
    budget: Rc<Budget>,
}

impl Host {
    /// Load `compiled` for an instrument with `cells`/`texts` script cells and
    /// `note_cells` polyphonic cells per note. Every cell starts at zero or "".
    pub fn new(compiled: &Compiled, cells: u32, texts: u32, note_cells: u16, rate: u32) -> Result<Self, String> {
        let lua = new_vm(MEMORY).map_err(|e| e.to_string())?;
        let budget = Rc::new(Budget::new(CALLBACK_CHECKPOINTS, None));
        let hook = Rc::clone(&budget);
        lua.set_interrupt(move |_| {
            hook.checkpoint()?;
            Ok(VmState::Continue)
        });
        let engine = Rc::new(RefCell::new(BTreeMap::new()));
        let host = lua.create_table().map_err(|e| e.to_string())?;
        let store = Rc::clone(&engine);
        host.set(
            "set_engine_par",
            lua.create_function(move |_, (p, v, g, s, generic): (i64, i64, i64, i64, i64)| {
                store.borrow_mut().insert([p, g, s, generic], v);
                // -1 = no async operation pending (interpreter parity).
                Ok(-1_i64)
            })
            .map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?;
        let store = Rc::clone(&engine);
        host.set(
            "get_engine_par",
            lua.create_function(move |_, (p, g, s, generic): (i64, i64, i64, i64)| {
                Ok(store.borrow().get(&[p, g, s, generic]).copied().unwrap_or(0))
            })
            .map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?;
        let messages = Rc::new(RefCell::new(Vec::new()));
        let store = Rc::clone(&messages);
        host.set(
            "message",
            lua.create_function(move |_, text: String| {
                // The interpreter keeps the first 1000 messages (`eval.rs`, Message).
                let mut store = store.borrow_mut();
                if store.len() < 1000 {
                    store.push(text);
                }
                Ok(())
            })
            .map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?;
        let cell_table = lua.create_table().map_err(|e| e.to_string())?;
        for i in 1..=i64::from(cells) {
            cell_table.raw_set(i, 0.0).map_err(|e| e.to_string())?;
        }
        let text_table = lua.create_table().map_err(|e| e.to_string())?;
        for i in 1..=i64::from(texts) {
            text_table.raw_set(i, "").map_err(|e| e.to_string())?;
        }
        let chunk = lua.load(&compiled.source).set_name("ksp").into_function().map_err(|e| e.to_string())?;
        let callbacks: Table = chunk
            .call((cell_table.clone(), text_table.clone(), lua.create_table().map_err(|e| e.to_string())?, host))
            .map_err(|e| e.to_string())?;
        let select_note: Function = callbacks.get("select_note").map_err(|e| e.to_string())?;
        Ok(Self {
            lua,
            rate,
            cells: cell_table,
            texts: text_table,
            callbacks,
            select_note,
            notes: BTreeMap::new(),
            note_cells,
            now: 0,
            seq: 0,
            queue: BTreeMap::new(),
            engine,
            messages,
            budget,
        })
    }

    /// Current frame.
    pub fn now(&self) -> u64 {
        self.now
    }

    /// Run `on init` to completion. Waits are no-ops there (interpreter parity).
    pub fn init(&mut self) -> Result<(), String> {
        let init: Option<Function> = self.callbacks.get("init").map_err(|e| e.to_string())?;
        let Some(init) = init else { return Ok(()) };
        self.budget.refill(CALLBACK_CHECKPOINTS, None);
        init.call::<()>(()).map_err(|e| e.to_string())
    }

    /// A note started at the current frame: `on note` with the key and velocity.
    pub fn note_on(&mut self, id: u32, key: i64, velocity: f64) -> Result<(), String> {
        self.start("note", id, vec![Value::Integer(key), Value::Number(velocity)])
    }

    /// A note ended at the current frame: `on release`.
    pub fn release(&mut self, id: u32, key: i64, velocity: f64) -> Result<(), String> {
        self.start("release", id, vec![Value::Integer(key), Value::Number(velocity)])
    }

    fn start(&mut self, key: &str, note: u32, args: Vec<Value>) -> Result<(), String> {
        let Some(func) = self.callbacks.get::<Option<Function>>(key).map_err(|e| e.to_string())? else {
            return Ok(());
        };
        let thread = self.lua.create_thread(func).map_err(|e| e.to_string())?;
        let job = Job { thread, carry: 0, note, ends_note: key == "release", args };
        let now = self.now;
        self.resume(job, now)
    }

    /// Render frames `[now, end)`: run every resumption due at a frame index below `end`,
    /// in (frame, start order). A wait of N frames from frame `t` resumes at index `t + N`.
    pub fn advance_to(&mut self, end: u64) -> Result<(), String> {
        while let Some((&(due, _), _)) = self.queue.first_key_value() {
            if due >= end {
                break;
            }
            let (_, job) = self.queue.pop_first().expect("checked above");
            self.resume(job, due)?;
        }
        self.now = end;
        Ok(())
    }

    /// Resume `job` at frame `at`, then keep it queued for its next wait or drop it when done.
    fn resume(&mut self, mut job: Job, at: u64) -> Result<(), String> {
        self.now = at;
        let notes = self.note_table(job.note)?;
        self.select_note.call::<()>(notes).map_err(|e| e.to_string())?;
        loop {
            self.budget.refill(CALLBACK_CHECKPOINTS, None);
            let args = MultiValue::from_vec(std::mem::take(&mut job.args));
            let yielded: Option<i64> = job.thread.resume(args).map_err(|e| e.to_string())?;
            if !matches!(job.thread.status(), mlua::thread::ThreadStatus::Resumable) {
                if job.ends_note {
                    self.notes.remove(&job.note);
                }
                return Ok(());
            }
            let micros = yielded.unwrap_or(0).max(0);
            let frames = frames_for(micros as u64, self.rate, &mut job.carry);
            if frames == 0 {
                continue;
            }
            self.seq += 1;
            let due = at + frames;
            self.queue.insert((due, self.seq), job);
            return Ok(());
        }
    }

    fn note_table(&mut self, note: u32) -> Result<Table, String> {
        if let Some(t) = self.notes.get(&note) {
            return Ok(t.clone());
        }
        let t = self.lua.create_table().map_err(|e| e.to_string())?;
        for i in 1..=i64::from(self.note_cells) {
            t.raw_set(i, 0.0).map_err(|e| e.to_string())?;
        }
        self.notes.insert(note, t.clone());
        Ok(t)
    }

    /// Script cells as the interpreter stores them: integers, reals as `f64`.
    pub fn cells(&self, count: u32) -> Vec<f64> {
        (1..=i64::from(count)).map(|i| self.cells.raw_get::<f64>(i).unwrap_or(0.0)).collect()
    }

    /// Script text cells.
    pub fn texts(&self, count: u32) -> Vec<String> {
        (1..=i64::from(count)).map(|i| self.texts.raw_get::<String>(i).unwrap_or_default()).collect()
    }

    /// `message()` texts, in call order.
    pub fn messages(&self) -> Vec<String> {
        self.messages.borrow().clone()
    }

    /// `set_engine_par` writes, keyed `[parameter, group, slot, generic]`.
    pub fn engine(&self) -> BTreeMap<[i64; 4], i64> {
        self.engine.borrow().clone()
    }
}

/// Frames a `wait(micros)` takes, as `sampler-core` computes them (`Instruction::WaitMicros`).
pub fn frames_for(micros: u64, rate: u32, carry: &mut u32) -> u64 {
    let exact = u128::from(micros) * u128::from(rate);
    let credit = u128::from(*carry);
    let (frames, left) = if exact >= 1_000_000 {
        (exact.div_ceil(1_000_000), credit)
    } else if exact <= credit {
        (0, credit - exact)
    } else {
        (1, 1_000_000 - (exact - credit))
    };
    *carry = left as u32;
    frames as u64
}
