//! The interpreter loop. Callbacks are coroutines: all suspendable state lives in a
//! `Thread` (pc, call stack, callback context); operand stacks are shared because a
//! callback can only suspend between statements, when they are empty.

use super::builtins::{Ret, SysVar};
use super::calls;
use super::compile::{Callback, InitData, Op, Program, Ty, VarId};
use super::engine::KspEngine;
use super::idiom::Operand;
use super::runtime::Env;
use super::ui::Ui;

pub const MAX_CALL_DEPTH: usize = 64;

/// A runtime error that aborts the current callback. Static text only: faults can
/// happen on the audio thread.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Fault(pub &'static str);

pub type Exec<T> = Result<T, Fault>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Yield {
    Done,
    /// Suspended until the absolute sample time.
    Wait(u64),
    /// The instruction budget ran out mid-callback.
    OutOfFuel,
}

/// What a builtin asks of the interpreter.
pub enum Step {
    Next,
    Wait(u64),
    Exit,
}

/// Which callback a thread runs, for `$NI_CALLBACK_TYPE` and event forwarding.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Cb(Callback),
    UiControl,
}

/// Event handed to the next slot when the callback first yields.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Forward {
    None,
    Note,
    Release,
    Controller,
    PolyAt,
    Rpn { nrpn: bool },
}

#[derive(Clone, Copy, Debug)]
pub struct Ctx {
    pub slot: u8,
    pub kind: Kind,
    pub event: i32,
    pub callback_id: i32,
    /// Row in the slot's polyphonic storage.
    pub poly_row: u32,
    pub cc: i32,
    pub value: i32,
    pub note: i32,
    pub signal: i32,
    pub async_id: i32,
    pub async_status: i32,
    pub ignore_controller: bool,
    pub forward: Forward,
}

impl Ctx {
    pub fn new(slot: u8, kind: Kind) -> Self {
        Self {
            slot,
            kind,
            event: 0,
            callback_id: 0,
            poly_row: 0,
            cc: 0,
            value: 0,
            note: 0,
            signal: 0,
            async_id: 0,
            async_status: 0,
            ignore_controller: false,
            forward: Forward::None,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Thread {
    pub pc: u32,
    pub depth: u8,
    /// Bumped whenever a pending timer for this thread becomes stale.
    pub generation: u32,
    pub live: bool,
    pub waiting: bool,
    /// Instructions run since the callback started or last waited.
    pub spent: u64,
    pub calls: [u32; MAX_CALL_DEPTH],
    pub ctx: Ctx,
}

impl Default for Thread {
    fn default() -> Self {
        Self {
            pc: 0,
            depth: 0,
            generation: 0,
            live: false,
            waiting: false,
            spent: 0,
            calls: [0; MAX_CALL_DEPTH],
            ctx: Ctx::new(0, Kind::Cb(Callback::Init)),
        }
    }
}

/// A stack of reusable strings: popped entries keep their capacity.
#[derive(Default)]
pub struct StrStack {
    items: Vec<String>,
    len: usize,
}

impl StrStack {
    pub fn push(&mut self) -> &mut String {
        if self.len == self.items.len() {
            self.items.push(String::with_capacity(64));
        }
        let s = &mut self.items[self.len];
        s.clear();
        self.len += 1;
        s
    }

    pub fn push_str(&mut self, text: &str) {
        self.push().push_str(text);
    }

    /// Remove the top entry; the returned text stays valid until the next push.
    pub fn pop(&mut self) -> &str {
        self.len -= 1;
        &self.items[self.len]
    }

    pub fn top(&mut self) -> &mut String {
        &mut self.items[self.len - 1]
    }

    /// Pop two entries and compare them.
    pub fn pop_equal(&mut self) -> bool {
        self.len -= 2;
        self.items[self.len] == self.items[self.len + 1]
    }

    /// Pop the top entry and append it to the one below.
    pub fn concat(&mut self) {
        self.len -= 1;
        let (below, top) = self.items.split_at_mut(self.len);
        below[self.len - 1].push_str(&top[0]);
    }

    pub fn clear(&mut self) {
        self.len = 0;
    }
}

#[derive(Default)]
pub struct Stacks {
    pub ints: Vec<i32>,
    pub reals: Vec<f64>,
    pub strs: StrStack,
    pub refs: Vec<u32>,
}

impl Stacks {
    pub fn with_capacity(n: usize) -> Self {
        let mut s = Self {
            ints: Vec::with_capacity(n),
            reals: Vec::with_capacity(n),
            strs: StrStack::default(),
            refs: Vec::with_capacity(n),
        };
        for _ in 0..16 {
            s.strs.push();
        }
        s.strs.clear();
        s
    }

    pub fn clear(&mut self) {
        self.ints.clear();
        self.reals.clear();
        self.strs.clear();
        self.refs.clear();
    }

    pub fn int(&mut self) -> i32 {
        self.ints.pop().expect("compiler balanced the int stack")
    }

    pub fn real(&mut self) -> f64 {
        self.reals.pop().expect("compiler balanced the real stack")
    }

    pub fn var(&mut self) -> VarId {
        self.refs.pop().expect("compiler balanced the ref stack")
    }
}

/// Typed script memory: scalars and arrays share one vector per type.
pub struct Memory {
    pub ints: Vec<i32>,
    pub reals: Vec<f64>,
    pub strs: Vec<String>,
    pub poly: Vec<i32>,
}

/// Per-slot mutable state.
pub struct SlotState {
    pub index: u8,
    pub mem: Memory,
    pub ui: Ui,
    /// Host PGS key index per string-pool entry, resolved on first use.
    pub pgs_keys: Vec<u32>,
    pub persistent: Vec<VarId>,
    pub listener: Listener,
    pub error: Option<String>,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Listener {
    pub timer_us: i32,
    pub beats: i32,
    pub transport: bool,
    /// Bumped when the timer changes so stale timer entries are dropped.
    pub generation: u32,
}

/// Polyphonic rows: one per event slot plus a scratch row for other callbacks.
pub const POLY_ROWS: u32 = super::runtime::EVENT_CAPACITY as u32 + 1;

impl SlotState {
    pub fn new(index: u8, p: &Program) -> Self {
        Self {
            index,
            mem: Memory {
                ints: vec![0; p.ints as usize],
                reals: vec![0.0; p.real_slots as usize],
                strs: vec![String::new(); p.strs as usize],
                poly: vec![0; (p.poly * POLY_ROWS) as usize],
            },
            ui: Ui::new(p.vars.len()),
            pgs_keys: vec![u32::MAX; p.strings.len()],
            persistent: Vec::new(),
            listener: Listener::default(),
            error: None,
        }
    }
}

pub struct Machine<'a> {
    pub prog: &'a Program,
    pub slot: &'a mut SlotState,
    pub env: &'a mut Env,
    pub stk: &'a mut Stacks,
    pub engine: &'a mut dyn KspEngine,
    pub t: &'a mut Thread,
}

fn bool_int(b: bool) -> i32 {
    b as i32
}

/// Memory index of an array element. Like Kontakt, an out-of-bounds access is
/// reported but not fatal: reads yield zero/empty and writes are dropped.
fn element(m: &mut Machine, v: VarId, index: i32) -> Option<usize> {
    let var = &m.prog.vars[v as usize];
    match u32::try_from(index) {
        Ok(i) if i < var.len.unwrap_or(1) => Some((var.slot + i) as usize),
        _ => {
            m.env.fault(
                m.slot.index,
                m.t.pc,
                "Array index out of bounds (read 0, write ignored)",
            );
            None
        }
    }
}

fn finite(x: f64) -> Exec<f64> {
    if x.is_finite() {
        Ok(x)
    } else {
        Err(Fault("Nonfinite real result"))
    }
}

/// Run until the callback finishes, suspends, faults or exhausts `fuel`.
pub fn exec(m: &mut Machine, fuel: &mut u64) -> Exec<Yield> {
    // Locals, not the caller's memory: `run` inlines and keeps both in registers.
    let (mut pc, mut left) = (m.t.pc as usize, *fuel);
    let result = run(m, &mut pc, &mut left);
    m.t.pc = pc as u32;
    *fuel = left;
    if result.is_err() {
        m.stk.clear();
    }
    result
}

#[inline(always)]
fn run(m: &mut Machine, pc: &mut usize, fuel: &mut u64) -> Exec<Yield> {
    let code = &m.prog.code;
    loop {
        // Fuel is only checked at loop back-edges and calls: those are statement
        // boundaries, so a preempted thread never leaves operands on the shared stacks.
        *fuel = fuel.saturating_sub(1);
        let op = code[*pc];
        *pc += 1;
        let s = &mut *m.stk;
        let mem = &mut m.slot.mem;
        macro_rules! int2 {
            (|$a:ident, $b:ident| $e:expr) => {{
                let $b = s.int();
                let $a = s.int();
                s.ints.push($e);
            }};
        }
        macro_rules! real2 {
            (|$a:ident, $b:ident| $e:expr) => {{
                let $b = s.real();
                let $a = s.real();
                s.reals.push(finite($e)?);
            }};
        }
        macro_rules! cmp_real {
            (|$a:ident, $b:ident| $e:expr) => {{
                let $b = s.real();
                let $a = s.real();
                s.ints.push(bool_int($e));
            }};
        }
        match op {
            Op::PushI(n) => s.ints.push(n),
            Op::PushR(i) => s.reals.push(m.prog.reals[i as usize]),
            Op::PushS(i) => s.strs.push_str(&m.prog.strings[i as usize]),
            Op::LdI(i) => s.ints.push(mem.ints[i as usize]),
            Op::StI(i) => mem.ints[i as usize] = s.int(),
            Op::LdR(i) => s.reals.push(mem.reals[i as usize]),
            Op::StR(i) => mem.reals[i as usize] = s.real(),
            Op::LdS(i) => s.strs.push_str(&mem.strs[i as usize]),
            Op::StS(i) => {
                let dst = &mut mem.strs[i as usize];
                dst.clear();
                dst.push_str(s.strs.pop());
            }
            Op::LdPoly(i) => {
                let row = m.t.ctx.poly_row * m.prog.poly;
                s.ints.push(mem.poly[(row + i) as usize]);
            }
            Op::StPoly(i) => {
                let row = m.t.ctx.poly_row * m.prog.poly;
                mem.poly[(row + i) as usize] = s.int();
            }
            Op::LdIA(v) => {
                let i = s.int();
                let x = element(m, v, i).map_or(0, |e| m.slot.mem.ints[e]);
                m.stk.ints.push(x);
            }
            Op::StIA(v) => {
                let value = s.int();
                let i = s.int();
                if let Some(e) = element(m, v, i) {
                    m.slot.mem.ints[e] = value;
                }
            }
            Op::LdRA(v) => {
                let i = s.int();
                let x = element(m, v, i).map_or(0.0, |e| m.slot.mem.reals[e]);
                m.stk.reals.push(x);
            }
            Op::StRA(v) => {
                let value = s.real();
                let i = s.int();
                if let Some(e) = element(m, v, i) {
                    m.slot.mem.reals[e] = value;
                }
            }
            Op::LdSA(v) => {
                let i = s.int();
                match element(m, v, i) {
                    Some(e) => m.stk.strs.push_str(&m.slot.mem.strs[e]),
                    None => m.stk.strs.push_str(""),
                }
            }
            Op::StSA(v) => {
                let i = s.int();
                let e = element(m, v, i);
                let text = m.stk.strs.pop();
                if let Some(e) = e {
                    let dst = &mut m.slot.mem.strs[e];
                    dst.clear();
                    dst.push_str(text);
                }
            }
            Op::Sys(v) => {
                let value = sys(m, v);
                m.stk.ints.push(value);
            }
            Op::UiId(v) => s.ints.push(m.slot.ui.var_id(v)),
            Op::Ref(r) => s.refs.push(r),
            Op::PopI => drop(s.int()),
            Op::PopR => drop(s.real()),
            Op::PopS => drop(s.strs.pop()),
            Op::IToS => {
                let n = s.int();
                let _ = std::fmt::Write::write_fmt(s.strs.push(), format_args!("{n}"));
            }
            Op::RToS => {
                let n = s.real();
                let _ = std::fmt::Write::write_fmt(s.strs.push(), format_args!("{n}"));
            }
            Op::IAdd => int2!(|a, b| a.wrapping_add(b)),
            Op::ISub => int2!(|a, b| a.wrapping_sub(b)),
            Op::IMul => int2!(|a, b| a.wrapping_mul(b)),
            Op::IDiv => {
                let b = s.int();
                let a = s.int();
                if b == 0 {
                    return Err(Fault("Division by zero"));
                }
                s.ints.push(a.wrapping_div(b));
            }
            Op::IMod => {
                let b = s.int();
                let a = s.int();
                if b == 0 {
                    return Err(Fault("Modulo by zero"));
                }
                s.ints.push(a.wrapping_rem(b));
            }
            Op::INeg => {
                let a = s.int();
                s.ints.push(a.wrapping_neg());
            }
            Op::IBitAnd => int2!(|a, b| a & b),
            Op::IBitOr => int2!(|a, b| a | b),
            Op::IBitXor => int2!(|a, b| a ^ b),
            Op::IBitNot => {
                let a = s.int();
                s.ints.push(!a);
            }
            Op::INot => {
                let a = s.int();
                s.ints.push(bool_int(a == 0));
            }
            Op::IEq => int2!(|a, b| bool_int(a == b)),
            Op::INe => int2!(|a, b| bool_int(a != b)),
            Op::ILt => int2!(|a, b| bool_int(a < b)),
            Op::IGt => int2!(|a, b| bool_int(a > b)),
            Op::ILe => int2!(|a, b| bool_int(a <= b)),
            Op::IGe => int2!(|a, b| bool_int(a >= b)),
            Op::RAdd => real2!(|a, b| a + b),
            Op::RSub => real2!(|a, b| a - b),
            Op::RMul => real2!(|a, b| a * b),
            Op::RDiv => real2!(|a, b| a / b),
            Op::RMod => real2!(|a, b| a % b),
            Op::RNeg => {
                let a = s.real();
                s.reals.push(-a);
            }
            Op::REq => cmp_real!(|a, b| a == b),
            Op::RNe => cmp_real!(|a, b| a != b),
            Op::RLt => cmp_real!(|a, b| a < b),
            Op::RGt => cmp_real!(|a, b| a > b),
            Op::RLe => cmp_real!(|a, b| a <= b),
            Op::RGe => cmp_real!(|a, b| a >= b),
            Op::SEq | Op::SNe => {
                let equal = s.strs.pop_equal();
                s.ints.push(bool_int(equal == (op == Op::SEq)));
            }
            Op::Concat => {
                s.strs.concat();
                if s.strs.top().len() > 65536 {
                    return Err(Fault("KSP string length limit"));
                }
            }
            Op::Jump(t) => {
                let back = (t as usize) < *pc;
                *pc = t as usize;
                if back && *fuel == 0 {
                    return Ok(Yield::OutOfFuel);
                }
            }
            Op::JumpIfZero(t) => {
                if s.int() == 0 {
                    *pc = t as usize;
                }
            }
            Op::JumpIfNonZero(t) => {
                if s.int() != 0 {
                    *pc = t as usize;
                }
            }
            Op::Case(c) => {
                let arm = m.prog.cases[c as usize];
                let v = s.ints[s.ints.len() - 1];
                if (arm.low..=arm.high).contains(&v) {
                    s.ints.pop();
                } else {
                    *pc = arm.miss as usize;
                }
            }
            Op::Call(f) => {
                if *fuel == 0 {
                    *pc -= 1;
                    return Ok(Yield::OutOfFuel);
                }
                let depth = m.t.depth as usize;
                if depth >= MAX_CALL_DEPTH {
                    return Err(Fault("KSP call nesting limit"));
                }
                m.t.calls[depth] = *pc as u32;
                m.t.depth += 1;
                *pc = m.prog.functions[f as usize] as usize;
            }
            Op::Ret => {
                m.t.depth -= 1;
                *pc = m.t.calls[m.t.depth as usize] as usize;
            }
            Op::Exit => {
                // NI: `exit` in a function leaves only that function.
                if m.t.depth == 0 {
                    return Ok(Yield::Done);
                }
                m.t.depth -= 1;
                *pc = m.t.calls[m.t.depth as usize] as usize;
            }
            Op::Halt => return Ok(Yield::Done),
            Op::AddVarImm(a, n) => {
                let x = &mut mem.ints[a as usize];
                *x = x.wrapping_add(n);
                *pc += 3;
            }
            Op::AddImm(n) => {
                let a = s.int();
                s.ints.push(a.wrapping_add(n));
                *pc += 1;
            }
            Op::LdIAVar(v, a) => {
                let i = mem.ints[a as usize];
                let x = element(m, v, i).map_or(0, |e| m.slot.mem.ints[e]);
                m.stk.ints.push(x);
                *pc += 1;
            }
            Op::BrImm(cmp, n, t) => {
                *pc = if cmp.test(s.int(), n) {
                    *pc + 2
                } else {
                    t as usize
                };
            }
            Op::BrVarImm(cmp, a, n, t) => {
                *pc = if cmp.test(mem.ints[a as usize], n) {
                    *pc + 3
                } else {
                    t as usize
                };
            }
            Op::Loop(l) => {
                let l = &m.prog.loops[l as usize];
                let value = match l.operand() {
                    Some(Operand::Imm(n)) => n,
                    Some(Operand::Int(v)) => mem.ints[v as usize],
                    Some(Operand::Sys(v)) => sys(m, v),
                    None => 0,
                };
                let mem = &mut m.slot.mem;
                *pc = l.run(*pc - 1, &m.prog.vars, &mut mem.ints, fuel, value);
            }
            Op::Declare(v) => declare(m, v)?,
            Op::InitArray(i) => init_array(m, i),
            Op::Builtin(b, argc) => {
                m.t.pc = *pc as u32;
                let step = match calls::call(m, b, argc) {
                    Err(f) if f == calls::NO_CONTROL || f == calls::NO_PGS_KEY => {
                        m.env.fault(m.slot.index, m.t.pc, f.0);
                        match b.sig().ret {
                            Ret::Int | Ret::Num => m.stk.ints.push(0),
                            Ret::Real => m.stk.reals.push(0.0),
                            Ret::Str => {
                                m.stk.strs.push();
                            }
                            Ret::Void => {}
                        }
                        Step::Next
                    }
                    step => step?,
                };
                match step {
                    Step::Next => {}
                    Step::Wait(at) => return Ok(Yield::Wait(at)),
                    Step::Exit => {
                        if m.t.depth == 0 {
                            return Ok(Yield::Done);
                        }
                        m.t.depth -= 1;
                        *pc = m.t.calls[m.t.depth as usize] as usize;
                    }
                }
            }
        }
    }
}

fn sys(m: &Machine, v: SysVar) -> i32 {
    let ctx = &m.t.ctx;
    let env = &*m.env;
    let event = || env.events.get(ctx.event);
    match v {
        SysVar::EventId => ctx.event,
        SysVar::EventNote => event().map_or(0, |e| e.note),
        SysVar::EventVelocity => event().map_or(0, |e| e.velocity),
        SysVar::NoteHeld => bool_int(env.events.held(ctx.event)),
        SysVar::CcNum => ctx.cc,
        SysVar::PitchBend => env.input.pitch_bend,
        SysVar::PolyAtNum => ctx.note,
        SysVar::RpnAddress => ctx.cc,
        SysVar::RpnValue => ctx.value,
        SysVar::MidiChannel => 0,
        SysVar::EngineUptime => env.micros(env.clock()).wrapping_div(1000) as i32,
        SysVar::KspTimer => env.micros(env.clock().saturating_sub(env.timer_origin)) as i32,
        SysVar::CallbackType => match ctx.kind {
            Kind::Cb(cb) => cb.type_id(),
            Kind::UiControl => super::builtins::cb::UI_CONTROL,
        },
        SysVar::CallbackId => ctx.callback_id,
        SysVar::SignalType => ctx.signal,
        SysVar::AsyncId => ctx.async_id,
        SysVar::AsyncExitStatus => ctx.async_status,
        SysVar::DurationQuarter => env.quarter_us(),
        SysVar::DurationEighth => env.quarter_us() / 2,
        SysVar::DurationSixteenth => env.quarter_us() / 4,
        SysVar::DurationQuarterTriplet => env.quarter_us() * 2 / 3,
        SysVar::DurationEighthTriplet => env.quarter_us() / 3,
        SysVar::DurationSixteenthTriplet => env.quarter_us() / 6,
        SysVar::DurationBar => env.quarter_us() * 4,
        SysVar::SongPosition => 0,
        SysVar::TransportRunning => bool_int(env.transport),
        SysVar::Tempo => env.tempo as i32,
        SysVar::CurrentScriptSlot => i32::from(m.slot.index),
    }
}

fn declare(m: &mut Machine, v: VarId) -> Exec<()> {
    let var = &m.prog.vars[v as usize];
    let first = m.slot.ui.declare(v);
    if var.ui.is_none() {
        return Ok(());
    }
    // Parameters are on the stacks in declaration order.
    let mut ints = [0i32; 8];
    let mut n = 0;
    for ty in var.params.iter().rev() {
        match ty {
            Ty::Int => {
                let x = m.stk.int();
                if n < ints.len() {
                    ints[n] = x;
                }
                n += 1;
            }
            Ty::Real => drop(m.stk.real()),
            Ty::Str => drop(m.stk.strs.pop()),
        }
    }
    let ints_used = n.min(ints.len());
    ints[..ints_used].reverse();
    if first {
        let kind = var.ui.as_deref().unwrap_or_default();
        m.slot
            .ui
            .add_control(v, kind, &ints[..ints_used])
            .map_err(Fault)?;
    }
    Ok(())
}

fn init_array(m: &mut Machine, i: u32) {
    let init = &m.prog.inits[i as usize];
    let var = &m.prog.vars[init.var as usize];
    let (base, len) = (var.slot as usize, var.len.unwrap_or(0) as usize);
    fn fill<T: Clone>(dst: &mut [T], src: &[T]) {
        let n = src.len().min(dst.len());
        dst[..n].clone_from_slice(&src[..n]);
        if let Some(last) = src.last() {
            dst[n..].fill(last.clone());
        }
    }
    let mem = &mut m.slot.mem;
    match &init.data {
        InitData::Int(d) => fill(&mut mem.ints[base..base + len], d),
        InitData::Real(d) => fill(&mut mem.reals[base..base + len], d),
        InitData::Str(d) => {
            let dst = &mut mem.strs[base..base + len];
            for (j, s) in dst.iter_mut().enumerate() {
                let k = d[j.min(d.len() - 1)];
                s.clear();
                s.push_str(&m.prog.strings[k as usize]);
            }
        }
    }
}
