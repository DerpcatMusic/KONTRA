//! Script VM: bytecode, interpreter and callback coroutines (04-SCRIPT.md §2).
//!
//! Language- and format-free. A front end compiles to [`Program`]; the engine
//! implements [`HostApi`] once and drives callbacks as coroutines: `wait`
//! suspends one ([`Step::Wait`]) and the engine resumes it at the exact sample.
//! A resumed callback may be given fuel ([`Instance::set_fuel`]); otherwise there is no abort: a runaway `while` leaves at Kontakt's limit
//! and the callback continues; runtime errors are counted, never fatal.
//! Nothing allocates after [`Instance::new`].

mod action;
pub mod midi;
mod execute;
mod fuse;
mod host;
mod machine;
mod poly;
mod program;
mod pure;
mod text;

pub use action::{Action, event};
pub use fuse::fuse;
pub use host::{HostApi, HostCall, HostValue};
pub use machine::Vm;
use poly::{POLY_EVENTS, Poly, free_poly, take_poly};
pub use program::{Bin, Cmp, Op, Program, Returns, Src};
pub use text::{TEXT_CAPACITY, Text};

/// Iterations a callback's `while` loops run between waits before Kontakt
/// leaves the loop. Counted across nested loops, so an inner loop's exit
/// does not restart a runaway outer loop's count.
pub const LOOP_LIMIT: u32 = 10_000_001;
/// First iteration count at which a loop's state is compared; doubles after.
const LOOP_PROBE: u32 = 1024;

/// A front end that runs its own language (Lua) on the engine: the same
/// [`HostApi`], coroutine handles and [`Step`]s as [`Instance`].
pub trait Machine: Send {
    /// Takes a coroutine for the callback at `entry`, as [`Instance::start`].
    fn start(&mut self, entry: u32, context: u64) -> Option<CoId>;
    /// Runs it until it ends or waits, as [`Instance::run`].
    fn run(&mut self, co: CoId, host: &mut dyn HostApi) -> Step;
    fn cancel(&mut self, co: CoId);
    fn context(&self, co: CoId) -> Option<u64>;
    /// A callback the last run started beside itself (a front end's
    /// `spawn`), now waiting this many microseconds: the host schedules it
    /// like a wait. In the order they were started.
    fn spawned(&mut self) -> Option<(CoId, u64)>;
    fn faults(&self) -> Faults;
    /// A coroutine that moves control `id` ([`Foreign::controls`]) to
    /// `value` and runs its callback, as [`Machine::start`].
    fn control(&mut self, id: i32, value: f64, context: u64) -> Option<CoId>;
    /// A coroutine that moves control `id` to `saved`'s place in its range
    /// plus `ratio × m` (`m` 0..=1) and runs its callback when it changes.
    fn control_mod(
        &mut self,
        _id: i32,
        _saved: f64,
        _scale: f64,
        _m: f64,
        _context: u64,
    ) -> Option<CoId> {
        None
    }
    /// The editor moved widget `n` of the front end's own interface (one it
    /// builds itself, not from controls) to `value`: the next start of its
    /// widget entry applies it.
    fn stage_widget(&mut self, _n: u32, _value: f64, _cell: Option<u32>) {}
    /// Widget `n` of the front end's own interface as it stands now.
    fn widget(&self, _n: u32) -> Option<WidgetLook> {
        None
    }
    /// Copy widget `n`'s array cells into preallocated storage.
    fn widget_cells(&self, _n: u32, _out: &mut [f64]) -> bool {
        false
    }
    /// Widget `n`'s text into `out`; false when it has none.
    fn widget_text(&self, _n: u32, _out: &mut Text) -> bool {
        false
    }
    /// Menu widget `n`'s item `i` (from 0) into `out`; false past its last.
    fn widget_item(&self, _n: u32, _i: u32, _out: &mut Text) -> bool {
        false
    }
    /// The state the script saves for a project, written into `out` in the
    /// front end's own form; its length, or `None` when it saves none or
    /// it does not fit.
    fn save_state(&mut self, _out: &mut [u8]) -> Option<usize> {
        None
    }
    /// The oldest file the script saved for the host to write: its path,
    /// a newline, then its contents, into `out`; its length, or `None` when
    /// there is none (or it does not fit, and it is dropped).
    fn take_file(&mut self, _out: &mut [u8]) -> Option<usize> {
        None
    }
    /// The interface the front end builds itself, as it stands now, in its
    /// own type; not real-time (the editor reads it once, after init).
    fn own_page(&self) -> Option<Box<dyn std::any::Any + Send>> {
        None
    }
    /// How many times it ran a callback, each resume of one counted,
    /// wrapping: unchanged, its state is as it was.
    fn runs(&self) -> u32 {
        0
    }
}

/// What the editor redraws of a front end's own widget.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WidgetLook {
    pub value: f64,
    pub visible: bool,
    pub enabled: bool,
    /// A menu's item list revision: it moves when the script adds, renames
    /// or clears items.
    pub items: u32,
}

/// Compiled code a [`Machine`] runs; each engine makes its own machine.
pub trait Foreign: Send + Sync + std::fmt::Debug {
    /// Not real-time: builds the language state.
    fn machine(&self) -> Box<dyn Machine>;
    /// What a user can move ([`Machine::control`]), by name; its index is
    /// the control's id.
    fn controls(&self) -> &[String];
}

/// How a run ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    /// The callback finished; its coroutine is free.
    Done,
    /// Suspended for this many microseconds; resume with [`Instance::run`].
    Wait(u64),
    /// Out of fuel ([`Instance::set_fuel`]) at a loop turn: resume it later.
    Yield,
}

/// Runtime errors, counted. Kontakt logs these and continues; so does the VM.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Faults {
    pub divide_by_zero: u64,
    pub out_of_range: u64,
    pub stack_overflow: u64,
    pub texts_cut: u64,
    pub loop_limits: u64,
    /// Callbacks not started because every coroutine was busy.
    pub no_coroutine: u64,
    /// Ill-formed bytecode (a front-end bug): the callback ended.
    pub bad_code: u64,
    /// The first index out of range: the array's first cell, whether it
    /// holds texts, and the index, for diagnostics.
    pub first_out_of_range: Option<(u32, bool, i32)>,
    /// Where the first division by zero was, as a code position.
    pub first_divide_by_zero: Option<u32>,
}

/// Handle of a running or suspended callback: its slot and the slot's
/// generation, so a handle kept past its callback's end finds nothing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CoId {
    index: u16,
    generation: u16,
}

impl CoId {
    /// A handle for a [`Machine`]'s own coroutine slots.
    #[must_use]
    pub fn new(index: u16, generation: u16) -> Self {
        Self { index, generation }
    }

    #[must_use]
    pub fn index(self) -> u16 {
        self.index
    }

    #[must_use]
    pub fn generation(self) -> u16 {
        self.generation
    }
}

struct Coroutine {
    busy: bool,
    generation: u16,
    pc: u32,
    context: u64,
    /// Value stack: value `k` of `sp` in slot `k + 1` (see `execute`).
    stack: Box<[i64]>,
    sp: usize,
    texts: Vec<Text>,
    frames: Vec<u32>,
    /// Its note event's polyphonic cells, in [`Instance::polys`].
    poly: usize,
    loop_count: u32,
    /// Loop turns this run, whatever waits reset.
    turns: u32,
    /// Loop turns left before the run yields; 0 runs to its end or wait.
    fuel: u32,
    loop_hash: u64,
    /// A host call ran since the last stuck check: state outside the hash moved.
    hosted: bool,
    /// Counts waits and stops, so a resume queued for a stopped wait is stale.
    waits: u32,
    /// `stop_wait` with 1: every later wait passes at once.
    no_wait: bool,
}

/// One script's live state: cells, texts and its callback coroutines.
pub struct Instance {
    cells: Box<[i64]>,
    texts: Box<[Text]>,
    coroutines: Vec<Coroutine>,
    polys: Box<[Poly]>,
    starts: u64,
    faults: Faults,
    /// Counts what may have written a cell (a run, a host's write),
    /// wrapping: unchanged, every cell is as it was.
    changes: u32,
    last_turns: u32,
    fuel: u32,
}

impl Instance {
    /// Allocates cells and `coroutines` callback slots. Not real-time.
    #[must_use]
    pub fn new(program: &Program, coroutines: usize) -> Self {
        let stack = program.max_stack.max(16);
        let coroutines = (0..coroutines.max(1))
            .map(|_| Coroutine {
                busy: false,
                generation: 0,
                pc: 0,
                context: 0,
                stack: vec![0; stack + 1].into(),
                sp: 0,
                texts: Vec::with_capacity(stack),
                frames: Vec::with_capacity(program.max_depth.max(1)),
                poly: usize::MAX,
                loop_count: 0,
                turns: 0,
                fuel: 0,
                loop_hash: 0,
                hosted: false,
                waits: 0,
                no_wait: false,
            })
            .collect();
        let events = if program.poly_cells == 0 {
            0
        } else {
            POLY_EVENTS
        };
        let polys = (0..events)
            .map(|_| Poly {
                event: 0,
                cells: vec![0; program.poly_cells].into(),
                users: 0,
                used: 0,
            })
            .collect();
        Self {
            cells: vec![0; program.cells].into(),
            texts: vec![Text::EMPTY; program.text_cells].into(),
            coroutines,
            polys,
            starts: 0,
            faults: Faults::default(),
            changes: 0,
            last_turns: 0,
            fuel: 0,
        }
    }

    #[must_use]
    pub fn faults(&self) -> Faults {
        self.faults
    }

    #[must_use]
    pub fn cell(&self, n: usize) -> Option<i64> {
        self.cells.get(n).copied()
    }

    pub fn set_cell(&mut self, n: usize, value: i64) {
        if let Some(c) = self.cells.get_mut(n) {
            *c = value;
            self.changes = self.changes.wrapping_add(1);
        }
    }

    pub fn set_text(&mut self, n: usize, value: Text) {
        if let Some(t) = self.texts.get_mut(n) {
            *t = value;
            self.changes = self.changes.wrapping_add(1);
        }
    }

    #[must_use]
    pub fn text(&self, n: usize) -> Option<&Text> {
        self.texts.get(n)
    }

    /// The context a callback was started with.
    #[must_use]
    pub fn context(&self, co: CoId) -> Option<u64> {
        self.coroutines
            .get(usize::from(co.index))
            .filter(|c| c.busy && c.generation == co.generation)
            .map(|c| c.context)
    }

    /// Callbacks currently running or suspended.
    #[must_use]
    pub fn busy(&self) -> usize {
        self.coroutines.iter().filter(|c| c.busy).count()
    }

    /// Takes a free coroutine for a callback at `entry`. Run it with [`Instance::run`].
    pub fn start(&mut self, entry: u32, context: u64) -> Option<CoId> {
        let Some((i, co)) = self
            .coroutines
            .iter_mut()
            .enumerate()
            .find(|(_, c)| !c.busy)
        else {
            self.faults.no_coroutine += 1;
            return None;
        };
        let index = u16::try_from(i).ok()?;
        co.poly = take_poly(&mut self.polys, context as u32, &mut self.starts);
        co.busy = true;
        co.generation = co.generation.wrapping_add(1);
        co.pc = entry;
        co.context = context;
        co.sp = 0;
        co.texts.clear();
        co.frames.clear();
        co.loop_count = 0;
        co.loop_hash = 0;
        co.hosted = false;
        co.no_wait = false;
        Some(CoId {
            index,
            generation: co.generation,
        })
    }

    /// Ends a suspended callback without running it further (KSP `stop_wait` with kill).
    pub fn cancel(&mut self, co: CoId) {
        if let Some(c) = self
            .coroutines
            .get_mut(usize::from(co.index))
            .filter(|c| c.generation == co.generation)
        {
            if c.busy {
                free_poly(&mut self.polys, c.poly);
            }
            c.busy = false;
        }
    }

    /// The current wait of a busy callback: a resume carrying another is stale.
    #[must_use]
    pub fn wait_epoch(&self, co: CoId) -> Option<u32> {
        self.live(co).map(|c| c.waits)
    }

    /// The busy callback whose context `matches`.
    #[must_use]
    pub fn find(&self, matches: impl Fn(u64) -> bool) -> Option<CoId> {
        let (i, c) = self
            .coroutines
            .iter()
            .enumerate()
            .find(|(_, c)| c.busy && matches(c.context))?;
        Some(CoId {
            index: u16::try_from(i).ok()?,
            generation: c.generation,
        })
    }

    /// Ends `co`'s current wait (KSP `stop_wait`); with `rest` its later
    /// waits pass at once too. Returns the wait to resume it with.
    pub fn stop_wait(&mut self, co: CoId, rest: bool) -> Option<u32> {
        let c = self
            .coroutines
            .get_mut(usize::from(co.index))
            .filter(|c| c.busy && c.generation == co.generation)?;
        c.waits = c.waits.wrapping_add(1);
        c.no_wait |= rest;
        Some(c.waits)
    }

    fn live(&self, co: CoId) -> Option<&Coroutine> {
        self.coroutines
            .get(usize::from(co.index))
            .filter(|c| c.busy && c.generation == co.generation)
    }

    /// The next [`Instance::run`] yields after `turns` loop turns (0: never).
    pub fn set_fuel(&mut self, turns: u32) {
        self.fuel = turns;
    }

    /// Loop turns the last [`Vm::run`] made.
    #[must_use]
    pub fn last_turns(&self) -> u32 {
        self.last_turns
    }

    /// Runs a callback until it ends or waits.
    pub fn run(&mut self, program: &Program, co: CoId, host: &mut dyn HostApi) -> Step {
        let Self {
            cells,
            texts,
            coroutines,
            polys,
            faults,
            changes,
            last_turns,
            fuel,
            ..
        } = self;
        let Some(c) = coroutines
            .get_mut(usize::from(co.index))
            .filter(|c| c.busy && c.generation == co.generation)
        else {
            return Step::Done;
        };
        *changes = changes.wrapping_add(1);
        let poly = polys
            .get_mut(c.poly)
            .map_or(&mut [][..], |p| &mut p.cells[..]);
        c.turns = 0;
        c.fuel = std::mem::take(fuel);
        let step = execute::execute(program, cells, texts, poly, c, faults, host);
        *last_turns = c.turns;
        if step == Step::Done {
            c.busy = false;
            free_poly(polys, c.poly);
        }
        step
    }
}
