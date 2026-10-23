//! Control-side host for compiled KSP. One Luau VM per instrument, one coroutine
//! per running callback. `wait(us)` yields; the host resumes the coroutine at the
//! exact sample frame the interpreter would (`sampler-core` `wait_behavior`: the
//! micro-frame carry is kept per callback), so timing does not depend on how the
//! caller splits its render blocks.

use super::{Compiled, controls};
use crate::builtins::FIRST_UI_ID;
use mlua::{Function, Lua, MultiValue, Table, Thread, Value, VmState};
use sampler_script::{Budget, Jit, cache_dir, cached_bytecode, new_vm_with};
use std::{cell::RefCell, collections::{BTreeMap, HashMap}, rc::Rc};

/// Checkpoints one callback may spend before it is aborted (deterministic, not timed).
const CALLBACK_CHECKPOINTS: u64 = 1_000_000;
/// Luau memory cap for one instrument VM.
const MEMORY: usize = 64 << 20;
/// Note id of UI callbacks: they have no note, but `NV` still needs a table.
const NO_NOTE: u32 = u32::MAX;

struct Job {
    thread: Thread,
    /// Frames the waits so far overshot their exact time, in micro-frames (< 1e6).
    carry: u32,
    note: u32,
    /// A release callback: its note's polyphonic cells are freed once it finishes.
    ends_note: bool,
}

/// The interface fields the Luau path models, as `Initial::model.interface` holds them.
#[derive(Debug, Default, PartialEq)]
pub struct UiFields {
    pub width_px: Option<i32>,
    pub height_px: Option<i32>,
    pub height_grid: Option<i32>,
    pub skin_offset: Option<i32>,
    pub performance_view: bool,
    pub messages: Vec<String>,
}

pub struct Host {
    lua: Lua,
    rate: u32,
    cells: Table,
    texts: Table,
    callbacks: Table,
    /// `note` and `release` callbacks, looked up once.
    note_fn: Option<Function>,
    release_fn: Option<Function>,
    /// The chunk's pooled thread body (see `RUNNER` in `luau::compile`).
    runner: Function,
    /// Coroutines that finished their last callback, ready for the next job.
    free: Vec<Thread>,
    /// Note tables of ended notes, cleared and reused by the next note.
    free_notes: Vec<Table>,
    /// Note whose table is currently `NV` in the VM.
    selected: Option<u32>,
    select_note: Function,
    /// Polyphonic cells per note id.
    notes: BTreeMap<u32, Table>,
    note_cells: u16,
    now: u64,
    seq: u64,
    queue: BTreeMap<(u64, u64), Job>,
    engine: Rc<RefCell<BTreeMap<[i64; 4], i64>>>,
    controls: Rc<RefCell<controls::State>>,
    ui_meta: Rc<[controls::UiMeta]>,
    budget: Rc<Budget>,
}

impl Host {
    /// Load `compiled` for an instrument with `cells`/`texts` script cells and
    /// `note_cells` polyphonic cells per note. Every cell starts at zero or "".
    /// Interpreted VM; see [`Host::new_with`] for the native-code option.
    pub fn new(compiled: &Compiled, cells: u32, texts: u32, note_cells: u16, rate: u32) -> Result<Self, String> {
        Self::new_with(compiled, cells, texts, note_cells, rate, Jit::Interpreter)
    }

    /// As [`Host::new`] with an explicit codegen policy. Native code runs loop-heavy
    /// callbacks 2-3x faster, but codegen runs at load (about 7 ms per VM), so it is
    /// per instrument: [`Compiled::has_loops`] is the opt-in the caller should use.
    pub fn new_with(compiled: &Compiled, cells: u32, texts: u32, note_cells: u16, rate: u32, jit: Jit) -> Result<Self, String> {
        let lua = new_vm_with(MEMORY, jit).map_err(|e| e.to_string())?;
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
        let ui_state = Rc::new(RefCell::new(controls::State::new(&compiled.ui)));
        let ui_meta: Rc<[controls::UiMeta]> = compiled.ui.clone().into();
        controls::install(&lua, &host, &ui_state, &ui_meta)?;
        let cell_table = lua.create_table().map_err(|e| e.to_string())?;
        for i in 1..=i64::from(cells) {
            cell_table.raw_set(i, 0.0).map_err(|e| e.to_string())?;
        }
        let text_table = lua.create_table().map_err(|e| e.to_string())?;
        for i in 1..=i64::from(texts) {
            text_table.raw_set(i, "").map_err(|e| e.to_string())?;
        }
        // Bytecode from the content-hash cache (compiled once per source, reused by
        // every instrument with the same script); `source` stays the key.
        let bytes = cached_bytecode(cache_dir().as_deref(), compiled.source.as_bytes()).map_err(|e| e.to_string())?;
        let chunk = lua.load(&bytes[..]).into_function().map_err(|e| e.to_string())?;
        let callbacks: Table = chunk
            .call((cell_table.clone(), text_table.clone(), lua.create_table().map_err(|e| e.to_string())?, host))
            .map_err(|e| e.to_string())?;
        let select_note: Function = callbacks.get("select_note").map_err(|e| e.to_string())?;
        let runner: Function = callbacks.get("runner").map_err(|e| e.to_string())?;
        let note_fn = callbacks.get("note").map_err(|e| e.to_string())?;
        let release_fn = callbacks.get("release").map_err(|e| e.to_string())?;
        Ok(Self {
            lua,
            rate,
            cells: cell_table,
            texts: text_table,
            callbacks,
            note_fn,
            release_fn,
            runner,
            free: Vec::new(),
            free_notes: Vec::new(),
            selected: None,
            select_note,
            notes: BTreeMap::new(),
            note_cells,
            now: 0,
            seq: 0,
            queue: BTreeMap::new(),
            engine,
            controls: ui_state,
            ui_meta,
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

    /// The user changed UI control `ui` to `value`. The control takes the value (clamped), then
    /// the dispatcher starts `on ui_controls` (with `$UI_ID`), the control's `on ui_control`, and
    /// `on ui_update`, each up to its first wait, as the runtime does.
    pub fn ui_change(&mut self, ui: usize, value: i32) -> Result<(), String> {
        self.controls.borrow_mut().write_control(&self.ui_meta, ui, value);
        let id = i64::from(FIRST_UI_ID + ui as i32);
        for key in ["ui_controls".to_string(), format!("ui_{ui}"), "ui_update".to_string()] {
            let Some(func) = self.callbacks.get::<Option<Function>>(key.as_str()).map_err(|e| e.to_string())? else {
                continue;
            };
            // The runner passes `key` as the first argument: `$UI_ID` for `on ui_controls`.
            self.launch(func, false, NO_NOTE, id, 0.0)?;
        }
        Ok(())
    }

    /// Whether a resumption is queued (a callback is waiting on a clock).
    pub fn pending(&self) -> bool {
        !self.queue.is_empty()
    }

    /// A note started at the current frame: `on note` with the key and velocity.
    pub fn note_on(&mut self, id: u32, key: i64, velocity: f64) -> Result<(), String> {
        self.start(false, id, key, velocity)
    }

    /// A note ended at the current frame: `on release`.
    pub fn release(&mut self, id: u32, key: i64, velocity: f64) -> Result<(), String> {
        self.start(true, id, key, velocity)
    }

    fn start(&mut self, release: bool, note: u32, key: i64, velocity: f64) -> Result<(), String> {
        let func = if release { &self.release_fn } else { &self.note_fn };
        let Some(func) = func.clone() else {
            // Nothing to run, but a released note still frees its polyphonic cells.
            if release {
                self.end_note(note);
            }
            return Ok(());
        };
        self.launch(func, release, note, key, velocity)
    }

    /// Start `func` on a pooled coroutine at the current frame.
    fn launch(&mut self, func: Function, ends_note: bool, note: u32, key: i64, velocity: f64) -> Result<(), String> {
        let thread = match self.free.pop() {
            Some(thread) => thread,
            None => self.lua.create_thread(self.runner.clone()).map_err(|e| e.to_string())?,
        };
        let notes = self.note_table(note)?;
        let job = Job { thread, carry: 0, note, ends_note };
        let now = self.now;
        self.selected = Some(note);
        // First resume: the runner's arguments (callback, note table, key, velocity).
        self.step(job, now, Some((func, notes, key, velocity)))
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

    /// Resume `job` at frame `at`, then keep it queued for its next wait or return
    /// its thread to the pool when the callback is done.
    fn resume(&mut self, job: Job, at: u64) -> Result<(), String> {
        self.now = at;
        if self.selected != Some(job.note) {
            let notes = self.note_table(job.note)?;
            self.select_note.call::<()>(notes).map_err(|e| e.to_string())?;
            self.selected = Some(job.note);
        }
        self.step(job, at, None)
    }

    /// Run `job` until it waits (queued at its due frame) or finishes (thread pooled).
    fn step(&mut self, mut job: Job, at: u64, start: Option<(Function, Table, i64, f64)>) -> Result<(), String> {
        let mut first = start;
        loop {
            self.budget.refill(CALLBACK_CHECKPOINTS, None);
            let yielded: Option<i64> = match first.take() {
                Some((f, t, k, v)) => job.thread.resume((f, t, k, v)),
                None => job.thread.resume(()),
            }
            .map_err(|e| e.to_string())?;
            if yielded == Some(-1) {
                if job.ends_note {
                    self.end_note(job.note);
                }
                self.free.push(job.thread);
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

    /// Free a note's polyphonic cells: the table is cleared and pooled.
    fn end_note(&mut self, note: u32) {
        if let Some(t) = self.notes.remove(&note) {
            self.free_notes.push(t);
        }
        self.selected = None;
    }

    fn note_table(&mut self, note: u32) -> Result<Table, String> {
        if let Some(t) = self.notes.get(&note) {
            return Ok(t.clone());
        }
        let t = match self.free_notes.pop() {
            Some(t) => t,
            None => self.lua.create_table().map_err(|e| e.to_string())?,
        };
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

    /// Control values by UI index (`Initial::controls`).
    pub fn controls(&self) -> Vec<i32> {
        self.controls.borrow().controls.clone()
    }

    /// Integer `$CONTROL_PAR_*` mirror (`Initial::properties`).
    pub fn properties(&self) -> HashMap<(i32, i32), i32> {
        self.controls.borrow().properties.clone()
    }

    /// Text `$CONTROL_PAR_*` mirror (`Initial::text_properties`).
    pub fn text_properties(&self) -> HashMap<(i32, i32), String> {
        self.controls.borrow().text.clone()
    }

    /// PGS integer keys (`Initial::model.pgs`).
    pub fn pgs(&self) -> BTreeMap<String, Vec<i32>> {
        let mut out = BTreeMap::new();
        let Ok(table) = self.callbacks.get::<Table>("pgs") else { return out };
        for pair in table.pairs::<String, Table>() {
            let Ok((key, values)) = pair else { continue };
            let n = values.raw_get::<i64>("n").unwrap_or(0);
            let values = (1..=n).map(|i| values.raw_get::<f64>(i).unwrap_or(0.0) as i32).collect();
            out.insert(key, values);
        }
        out
    }

    /// PGS text keys (`Initial::model.pgs_text`).
    pub fn pgs_text(&self) -> BTreeMap<String, String> {
        let Ok(table) = self.callbacks.get::<Table>("pgs_text") else { return BTreeMap::new() };
        table.pairs::<String, String>().filter_map(Result::ok).collect()
    }

    /// The interface fields this path models (`Initial::model.interface`).
    pub fn interface(&self) -> UiFields {
        let mut out = UiFields::default();
        let Ok(model) = self.callbacks.get::<Table>("model") else { return out };
        if let Ok(ui) = model.get::<Table>("ui") {
            let int = |key: &str| ui.get::<Option<i64>>(key).ok().flatten().map(|n| n as i32);
            out.width_px = int("width_px");
            out.height_px = int("height_px");
            out.height_grid = int("height_grid");
            out.skin_offset = int("skin_offset");
            out.performance_view = ui.get::<bool>("performance_view").unwrap_or(false);
        }
        if let Ok(messages) = model.get::<Table>("messages") {
            out.messages = messages.sequence_values::<String>().filter_map(Result::ok).collect();
        }
        out
    }

    /// `set_script_title`'s last title (`Initial::model.interface.title`).
    pub fn title(&self) -> Option<String> {
        let model = self.callbacks.get::<Table>("model").ok()?;
        model.get::<Option<String>>("title").ok().flatten()
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
