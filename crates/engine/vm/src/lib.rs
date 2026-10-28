//! Script VM: bytecode, interpreter and callback coroutines (04-SCRIPT.md §2).
//!
//! Language- and format-free. A front end compiles to [`Program`]; the engine
//! implements [`HostApi`] once and drives callbacks as coroutines: `wait`
//! suspends one ([`Step::Wait`]) and the engine resumes it at the exact sample.
//! There is no fuel and no abort: a runaway `while` leaves at Kontakt's limit
//! and the callback continues; runtime errors are counted, never fatal.
//! Nothing allocates after [`Instance::new`].

mod action;
mod program;
mod pure;
mod text;

use std::hash::{Hash, Hasher};

pub use action::{Action, event};
pub use program::{Cmp, Op, Program, Returns};
pub use text::{TEXT_CAPACITY, Text};

/// Iterations a callback's `while` loops run between waits before Kontakt
/// leaves the loop. Counted across nested loops, so an inner loop's exit
/// does not restart a runaway outer loop's count.
pub const LOOP_LIMIT: u32 = 10_000_001;
/// First iteration count at which a loop's state is compared; doubles after.
const LOOP_PROBE: u32 = 1024;

/// One host call: the action, its arguments and the instance cells (for
/// actions that take arrays by reference as `base, len` pairs).
pub struct HostCall<'a> {
    pub action: Action,
    /// Opaque context the engine gave the callback ([`Instance::start`]), e.g. its event.
    pub context: u64,
    pub ints: &'a [i64],
    pub texts: &'a [Text],
    pub cells: &'a mut [i64],
    /// Where a builtin that returns text writes it (then returns [`HostValue::Text`]).
    pub text_out: &'a mut Text,
}

/// What a host call returns. Must match the call's [`Returns`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum HostValue {
    Nothing,
    Int(i32),
    Real(f64),
    /// The text is in [`HostCall::text_out`].
    Text,
}

/// The engine side of every non-pure action, implemented once (`kontra-engine`).
pub trait HostApi {
    fn call(&mut self, call: HostCall<'_>) -> HostValue;
}

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
}

/// Compiled code a [`Machine`] runs; each engine makes its own machine.
pub trait Foreign: Send + Sync + std::fmt::Debug {
    /// Not real-time: builds the language state.
    fn machine(&self) -> Box<dyn Machine>;
}

/// How a run ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    /// The callback finished; its coroutine is free.
    Done,
    /// Suspended for this many microseconds; resume with [`Instance::run`].
    Wait(u64),
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
    stack: Vec<i64>,
    texts: Vec<Text>,
    frames: Vec<u32>,
    poly: Box<[i64]>,
    loop_count: u32,
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
    faults: Faults,
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
                stack: Vec::with_capacity(stack),
                texts: Vec::with_capacity(stack),
                frames: Vec::with_capacity(program.max_depth.max(1)),
                poly: vec![0; program.poly_cells].into(),
                loop_count: 0,
                loop_hash: 0,
                hosted: false,
                waits: 0,
                no_wait: false,
            })
            .collect();
        Self {
            cells: vec![0; program.cells].into(),
            texts: vec![Text::EMPTY; program.text_cells].into(),
            coroutines,
            faults: Faults::default(),
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
        }
    }

    pub fn set_text(&mut self, n: usize, value: Text) {
        if let Some(t) = self.texts.get_mut(n) {
            *t = value;
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
        co.busy = true;
        co.generation = co.generation.wrapping_add(1);
        co.pc = entry;
        co.context = context;
        co.stack.clear();
        co.texts.clear();
        co.frames.clear();
        co.poly.fill(0);
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

    /// Runs a callback until it ends or waits.
    pub fn run(&mut self, program: &Program, co: CoId, host: &mut dyn HostApi) -> Step {
        let Self {
            cells,
            texts,
            coroutines,
            faults,
        } = self;
        let Some(c) = coroutines
            .get_mut(usize::from(co.index))
            .filter(|c| c.busy && c.generation == co.generation)
        else {
            return Step::Done;
        };
        let step = execute(program, cells, texts, c, faults, host);
        if step == Step::Done {
            c.busy = false;
        }
        step
    }
}

fn execute(
    program: &Program,
    cells: &mut [i64],
    texts: &mut [Text],
    c: &mut Coroutine,
    faults: &mut Faults,
    host: &mut dyn HostApi,
) -> Step {
    macro_rules! pop {
        () => {
            match c.stack.pop() {
                Some(v) => v,
                None => {
                    faults.bad_code += 1;
                    return Step::Done;
                }
            }
        };
    }
    macro_rules! push {
        ($v:expr) => {{
            let v: i64 = $v;
            if c.stack.len() == c.stack.capacity() {
                faults.stack_overflow += 1;
                return Step::Done;
            }
            c.stack.push(v);
        }};
    }
    macro_rules! pop_text {
        () => {
            match c.texts.pop() {
                Some(v) => v,
                None => {
                    faults.bad_code += 1;
                    return Step::Done;
                }
            }
        };
    }
    macro_rules! push_text {
        ($v:expr) => {{
            if c.texts.len() == c.texts.capacity() {
                faults.stack_overflow += 1;
                return Step::Done;
            }
            c.texts.push($v);
        }};
    }
    let int = |v: i64| v as i32;
    let real = |v: i64| f64::from_bits(v as u64);
    let from_real = |v: f64| v.to_bits() as i64;

    loop {
        let Some(&op) = program.code.get(c.pc as usize) else {
            faults.bad_code += 1;
            return Step::Done;
        };
        c.pc += 1;
        match op {
            Op::Int(v) => push!(i64::from(v)),
            Op::Real(v) => push!(from_real(v)),
            Op::Text(n) => {
                let t = program
                    .texts
                    .get(n as usize)
                    .copied()
                    .unwrap_or(Text::EMPTY);
                push_text!(t);
            }
            Op::Pop => {
                pop!();
            }
            Op::Dup => {
                let v = pop!();
                push!(v);
                push!(v);
            }
            Op::Load(n) => push!(cells.get(n as usize).copied().unwrap_or(0)),
            Op::Store(n) => {
                let v = pop!();
                if let Some(cell) = cells.get_mut(n as usize) {
                    *cell = v;
                }
            }
            Op::LoadAt { base, len } => {
                let i = int(pop!());
                match element(base, len, i) {
                    Some(at) => push!(cells.get(at).copied().unwrap_or(0)),
                    None => {
                        faults.out_of_range += 1;
                        push!(0);
                    }
                }
            }
            Op::StoreAt { base, len } => {
                let v = pop!();
                let i = int(pop!());
                match element(base, len, i).and_then(|at| cells.get_mut(at)) {
                    Some(cell) => *cell = v,
                    None => faults.out_of_range += 1,
                }
            }
            Op::LoadPoly(n) => push!(c.poly.get(usize::from(n)).copied().unwrap_or(0)),
            Op::StorePoly(n) => {
                let v = pop!();
                if let Some(cell) = c.poly.get_mut(usize::from(n)) {
                    *cell = v;
                }
            }
            Op::TLoad(n) => push_text!(texts.get(n as usize).copied().unwrap_or(Text::EMPTY)),
            Op::TStore(n) => {
                let t = pop_text!();
                if let Some(cell) = texts.get_mut(n as usize) {
                    *cell = t;
                }
            }
            Op::TLoadAt { base, len } => {
                let i = int(pop!());
                match element(base, len, i) {
                    Some(at) => push_text!(texts.get(at).copied().unwrap_or(Text::EMPTY)),
                    None => {
                        faults.out_of_range += 1;
                        push_text!(Text::EMPTY);
                    }
                }
            }
            Op::TStoreAt { base, len } => {
                let t = pop_text!();
                let i = int(pop!());
                match element(base, len, i).and_then(|at| texts.get_mut(at)) {
                    Some(cell) => *cell = t,
                    None => faults.out_of_range += 1,
                }
            }
            Op::TFromInt => {
                let mut t = Text::EMPTY;
                t.push_fmt(format_args!("{}", int(pop!())));
                push_text!(t);
            }
            Op::TFromReal => {
                let mut t = Text::EMPTY;
                t.push_fmt(format_args!("{:.6}", real(pop!())));
                push_text!(t);
            }
            Op::TConcat => {
                let b = pop_text!();
                let mut a = pop_text!();
                if !a.push(b.as_str()) {
                    faults.texts_cut += 1;
                }
                push_text!(a);
            }
            Op::TEq => {
                let b = pop_text!();
                let a = pop_text!();
                push!(i64::from(a == b));
            }
            Op::IAdd
            | Op::ISub
            | Op::IMul
            | Op::IDiv
            | Op::IMod
            | Op::IAnd
            | Op::IOr
            | Op::IXor
            | Op::IShl
            | Op::IShr => {
                let b = int(pop!());
                let a = int(pop!());
                let v = match op {
                    Op::IAdd => a.wrapping_add(b),
                    Op::ISub => a.wrapping_sub(b),
                    Op::IMul => a.wrapping_mul(b),
                    Op::IDiv | Op::IMod if b == 0 => {
                        faults.divide_by_zero += 1;
                        0
                    }
                    Op::IDiv => a.wrapping_div(b),
                    Op::IMod => a.wrapping_rem(b),
                    Op::IAnd => a & b,
                    Op::IOr => a | b,
                    Op::IXor => a ^ b,
                    Op::IShl => a.wrapping_shl(b as u32),
                    _ => a.wrapping_shr(b as u32),
                };
                push!(i64::from(v));
            }
            Op::INeg => {
                let a = int(pop!());
                push!(i64::from(a.wrapping_neg()));
            }
            Op::IBitNot => {
                let a = int(pop!());
                push!(i64::from(!a));
            }
            Op::FAdd | Op::FSub | Op::FMul | Op::FDiv => {
                let b = real(pop!());
                let a = real(pop!());
                let v = match op {
                    Op::FAdd => a + b,
                    Op::FSub => a - b,
                    Op::FMul => a * b,
                    _ => a / b,
                };
                push!(from_real(v));
            }
            Op::FNeg => {
                let a = real(pop!());
                push!(from_real(-a));
            }
            Op::ICmp(cmp) => {
                let b = int(pop!());
                let a = int(pop!());
                push!(i64::from(compare(cmp, a.cmp(&b))));
            }
            Op::FCmp(cmp) => {
                let b = real(pop!());
                let a = real(pop!());
                let ordering = a.partial_cmp(&b);
                push!(i64::from(
                    ordering.is_some_and(|o| compare(cmp, o))
                        || (cmp == Cmp::Ne && ordering.is_none())
                ));
            }
            Op::Not => {
                let a = pop!();
                push!(i64::from(a == 0));
            }
            Op::IntToReal => {
                let a = int(pop!());
                push!(from_real(f64::from(a)));
            }
            Op::RealToInt => {
                let a = real(pop!());
                push!(i64::from(a as i32));
            }
            Op::Jump(to) => c.pc = to,
            Op::JumpIfZero(to) => {
                if pop!() == 0 {
                    c.pc = to;
                }
            }
            Op::LoopCheck { exit } => {
                c.loop_count += 1;
                let mut stuck = false;
                if c.loop_count >= LOOP_PROBE && c.loop_count.is_power_of_two() {
                    let hash = state_hash(cells, texts, c);
                    stuck = !std::mem::take(&mut c.hosted) && hash == c.loop_hash;
                    c.loop_hash = hash;
                }
                // The limit holds for the rest of the run (every loop exits
                // until the next wait); a stuck loop frees only itself.
                if c.loop_count >= LOOP_LIMIT {
                    faults.loop_limits += 1;
                    c.pc = exit;
                } else if stuck {
                    faults.loop_limits += 1;
                    c.loop_count = 0;
                    c.pc = exit;
                }
            }
            Op::LoopExit => c.loop_hash = 0,
            Op::Call(to) => {
                if c.frames.len() == c.frames.capacity() {
                    faults.stack_overflow += 1;
                    return Step::Done;
                }
                c.frames.push(c.pc);
                c.pc = to;
            }
            Op::Ret => match c.frames.pop() {
                Some(back) => c.pc = back,
                None => return Step::Done,
            },
            Op::Exit => return Step::Done,
            Op::Wait => {
                let micros = pop!();
                c.loop_count = 0;
                if !c.no_wait {
                    c.waits = c.waits.wrapping_add(1);
                    return Step::Wait(u64::try_from(micros).unwrap_or(0));
                }
            }
            Op::Host {
                action,
                ints,
                texts: text_args,
                returns,
            } => {
                let (ints, text_args) = (usize::from(ints), usize::from(text_args));
                if c.stack.len() < ints || c.texts.len() < text_args {
                    faults.bad_code += 1;
                    return Step::Done;
                }
                let mut text_out = Text::EMPTY;
                let int_from = c.stack.len() - ints;
                let text_from = c.texts.len() - text_args;
                let args = c.stack.get(int_from..).unwrap_or_default();
                let value = match pure::eval(action, args, cells) {
                    Some(value) => value,
                    None => {
                        // A message changes nothing the loop can read.
                        c.hosted |= action != Action::Message;
                        host.call(HostCall {
                            action,
                            context: c.context,
                            ints: args,
                            texts: c.texts.get(text_from..).unwrap_or_default(),
                            cells,
                            text_out: &mut text_out,
                        })
                    }
                };
                c.stack.truncate(int_from);
                c.texts.truncate(text_from);
                match (returns, value) {
                    (Returns::Nothing, _) => {}
                    (Returns::Int, HostValue::Int(v)) => push!(i64::from(v)),
                    (Returns::Real, HostValue::Real(v)) => push!(from_real(v)),
                    (Returns::Text, HostValue::Text) => push_text!(text_out),
                    (Returns::Int, _) => push!(0),
                    (Returns::Real, _) => push!(from_real(0.0)),
                    (Returns::Text, _) => push_text!(Text::EMPTY),
                }
            }
        }
    }
}

fn element(base: u32, len: u32, index: i32) -> Option<usize> {
    let i = u32::try_from(index).ok().filter(|&i| i < len)?;
    Some(base as usize + i as usize)
}

fn compare(cmp: Cmp, o: std::cmp::Ordering) -> bool {
    use std::cmp::Ordering::{Equal, Greater, Less};
    match cmp {
        Cmp::Eq => o == Equal,
        Cmp::Ne => o != Equal,
        Cmp::Lt => o == Less,
        Cmp::Le => o != Greater,
        Cmp::Gt => o == Greater,
        Cmp::Ge => o != Less,
    }
}

/// State a loop iteration can change: if equal at two checkpoints, the loop
/// can never exit and the limit applies now instead of after ten million turns.
fn state_hash(cells: &[i64], texts: &[Text], c: &Coroutine) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    cells.hash(&mut h);
    c.poly.hash(&mut h);
    c.stack.hash(&mut h);
    for t in texts.iter().chain(&c.texts) {
        t.as_str().hash(&mut h);
    }
    h.finish()
}
