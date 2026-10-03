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
    /// Live host-block time allowance exhausted at a statement boundary.
    OutOfTime,
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
    NoteController,
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
    /// Channel belongs to this callback, including while suspended in wait().
    pub channel: u8,
    /// Physical MIDI input that owns this performance callback; services have none.
    pub input_channel: Option<u8>,
    pub ui_id: i32,
    pub signal: i32,
    pub async_id: i32,
    pub async_status: i32,
    pub ignore_controller: bool,
    /// stop_wait(..., 1) continues this callback without subsequent waits.
    pub ignore_wait: bool,
    /// Host Panic/reset cleanup writes script state while suppressing new notes.
    pub cleanup: bool,
    /// A musical ignore_event suppressed release forwarding at an earlier wait.
    pub release_blocked: bool,
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
            channel: 0,
            input_channel: None,
            ui_id: 0,
            signal: 0,
            async_id: 0,
            async_status: 0,
            ignore_controller: false,
            ignore_wait: false,
            cleanup: false,
            release_blocked: false,
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
    /// A worker-backed operation suspends this same callback without a timer.
    pub async_wait: Option<i32>,
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
            async_wait: None,
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
    pub frozen: bool,
}

impl StrStack {
    pub fn push(&mut self) -> Exec<&mut String> {
        if self.len == self.items.len() {
            if self.frozen { return Err(Fault("KSP string stack capacity exhausted")); }
            self.items.push(String::with_capacity(64));
        }
        let s = &mut self.items[self.len];
        s.clear();
        self.len += 1;
        Ok(s)
    }

    pub fn push_str(&mut self, text: &str) -> Exec<()> {
        let loading = !self.frozen;
        put_text(self.push()?, text, loading)
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
    pub fn concat(&mut self) -> Exec<()> {
        self.len -= 1;
        let (below, top) = self.items.split_at_mut(self.len);
        append_text(&mut below[self.len - 1], &top[0], !self.frozen)
    }

    pub fn clear(&mut self) {
        self.len = 0;
    }

    pub fn prepare(&mut self, bytes: usize) {
        while self.items.len() < 64 {
            self.items.push(String::new());
        }
        for s in &mut self.items {
            s.reserve(bytes.saturating_sub(s.len()));
        }
        self.frozen = true;
    }
}

pub(super) fn append_text(dst: &mut String, src: &str, loading: bool) -> Exec<()> {
    let len = dst.len().saturating_add(src.len());
    if len > 65536 {
        return Err(Fault("KSP string length limit"));
    }
    if !loading && len > dst.capacity() {
        return Err(Fault("KSP realtime string capacity exhausted"));
    }
    dst.push_str(src);
    Ok(())
}

/// Kontakt bounds stored @ variables and ! array elements to 320 characters.
/// https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/variables
/// Overflow keeps the Unicode prefix; expression and host metadata buffers keep
/// their separate capacities. NI documents the bound, not overflow handling.
pub(super) const MAX_STRING_VAR_CHARS: usize = 320;
pub(super) const MAX_STRING_VAR_BYTES: usize = MAX_STRING_VAR_CHARS * 4;
pub(super) fn variable_text(text: &str) -> &str {
    let end = text.char_indices().nth(MAX_STRING_VAR_CHARS).map_or(text.len(), |(at,_)| at);
    &text[..end]
}
pub(super) fn put_variable_text(dst: &mut String, src: &str, loading: bool) -> Exec<()> {
    put_text(dst, variable_text(src), loading)
}

pub(super) fn put_text(dst: &mut String, src: &str, loading: bool) -> Exec<()> {
    if src.len() > 65536 {
        return Err(Fault("KSP string length limit"));
    }
    if !loading && src.len() > dst.capacity() {
        return Err(Fault("KSP realtime string capacity exhausted"));
    }
    dst.clear();
    dst.push_str(src);
    Ok(())
}

pub(super) fn format_text(dst: &mut String, args: std::fmt::Arguments, loading: bool) -> Exec<()> {
    struct Writer<'a>(&'a mut String, bool);
    impl std::fmt::Write for Writer<'_> {
        fn write_str(&mut self, text: &str) -> std::fmt::Result {
            append_text(self.0, text, self.1).map_err(|_| std::fmt::Error)
        }
    }
    std::fmt::write(&mut Writer(dst, loading), args)
        .map_err(|_| Fault("KSP formatted string capacity exhausted"))
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
            s.strs.push().unwrap();
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

/// Typed slots whose mutable borrows invalidate only their bound UI variable.
/// No mutable slice dereference is exposed: indexed, bulk and host writes all
/// pass through the same prepared owner map, including optimized VM loops.
pub struct Values<T> {
    values: Vec<T>,
    owners: std::sync::Arc<[u32]>,
    revisions: Vec<u64>,
}

impl<T: Clone> Values<T> {
    fn new(value: T, len: usize, owners: &std::sync::Arc<[u32]>, count: usize) -> Self {
        Self { values: vec![value; len], owners: owners.clone(), revisions: vec![0; count] }
    }
}

impl<T> Values<T> {
    fn touch(&mut self, index: usize) {
        if let Some(&owner) = self.owners.get(index).filter(|&&o| o != u32::MAX) {
            self.revisions[owner as usize] = self.revisions[owner as usize].wrapping_add(1);
        }
    }
    fn touch_range(&mut self, range: std::ops::Range<usize>) {
        // A contiguous variable needs only one revision bump per bulk borrow.
        let mut previous = u32::MAX;
        for index in range.start..range.end.min(self.owners.len()) {
            let owner = self.owners[index];
            if owner != u32::MAX && owner != previous {
                self.revisions[owner as usize] = self.revisions[owner as usize].wrapping_add(1);
            }
            previous = owner;
        }
    }
    pub fn revision(&self, slot: usize) -> u64 {
        self.owners.get(slot).filter(|&&o| o != u32::MAX).map_or(0, |&o| self.revisions[o as usize])
    }
}
impl<T> std::ops::Deref for Values<T> {
    type Target = [T];
    fn deref(&self) -> &[T] { &self.values }
}
impl<T, I: std::slice::SliceIndex<[T]>> std::ops::Index<I> for Values<T> {
    type Output = I::Output;
    fn index(&self, index: I) -> &Self::Output { &self.values[index] }
}
impl<T> std::ops::IndexMut<usize> for Values<T> {
    fn index_mut(&mut self, index: usize) -> &mut T { self.touch(index); &mut self.values[index] }
}
impl<T> std::ops::IndexMut<std::ops::Range<usize>> for Values<T> {
    fn index_mut(&mut self, range: std::ops::Range<usize>) -> &mut [T] {
        self.touch_range(range.clone());
        &mut self.values[range]
    }
}
impl<T> std::ops::IndexMut<std::ops::RangeFull> for Values<T> {
    fn index_mut(&mut self, _: std::ops::RangeFull) -> &mut [T] {
        self.touch_range(0..self.values.len());
        &mut self.values
    }
}
impl<'a, T> IntoIterator for &'a mut Values<T> {
    type Item = &'a mut T;
    type IntoIter = std::slice::IterMut<'a, T>;
    fn into_iter(self) -> Self::IntoIter {
        self.touch_range(0..self.values.len());
        self.values.iter_mut()
    }
}
impl Values<i32> {
    pub fn copy_within(&mut self, range: std::ops::Range<usize>, to: usize) {
        self.touch_range(to..to + range.len());
        self.values.copy_within(range, to);
    }
}

/// Typed script memory: scalars and arrays share one vector per type.
pub struct Memory {
    pub ints: Values<i32>,
    pub reals: Values<f64>,
    pub strs: Values<String>,
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
    pub snapshot_type: i32,
    pub error: Option<String>,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Listener {
    pub timer_us: i32,
    pub beats: i32,
    /// Independent start/stop subscription bits.
    pub transport: u8,
    /// Independent MS/beat generations: changing one timer preserves the other.
    pub generations: [u32; 2],
}

/// Polyphonic rows: one per event slot plus a scratch row for other callbacks.
pub const POLY_ROWS: u32 = super::runtime::EVENT_CAPACITY as u32 + 1;

impl SlotState {
    pub fn new(index: u8, p: &Program) -> Self {
        Self {
            index,
            mem: Memory {
                ints: Values::new(0, p.ints as usize, &p.revision_owners[0], p.revision_counts[0]),
                reals: Values::new(0.0, p.real_slots as usize, &p.revision_owners[1], p.revision_counts[1]),
                strs: Values::new(String::new(), p.strs as usize, &p.revision_owners[2], p.revision_counts[2]),
                poly: vec![0; (p.poly * POLY_ROWS) as usize],
            },
            ui: Ui::new(p.vars.len()),
            pgs_keys: vec![u32::MAX; p.strings.len()],
            persistent: Vec::new(),
            listener: Listener::default(),
            snapshot_type: 0,
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
/// `pc` is the running thread's, for the report's line.
fn element(m: &mut Machine, pc: usize, v: VarId, index: i32) -> Option<usize> {
    let var = &m.prog.vars[v as usize];
    match u32::try_from(index) {
        Ok(i) if i < var.len.unwrap_or(1) => Some((var.slot + i) as usize),
        _ => {
            m.env.fault_context = Some(super::runtime::FaultContext::ArrayIndex { variable: v, index, length: var.len.unwrap_or(1) });
            m.env.fault(
                m.slot.index,
                // `pc` is already past the faulting op, like a builtin's.
                pc as u32,
                "Array index out of bounds (read 0, write ignored)",
            );
            None
        }
    }
}

/// Reals are IEEE doubles, as in Kontakt: `x / 0.0` is infinite and the
/// callback goes on. Dolce and Areia divide 0.0 by 0.0 in
/// `on persistence_changed` on every load; aborting there skipped 1,400 lines.
pub const NONFINITE: &str = "Nonfinite real result (kept)";

/// Run until the callback finishes, suspends, faults or exhausts `fuel`.
pub fn exec(m: &mut Machine, fuel: &mut u64) -> Exec<Yield> {
    m.env.fault_action = Some(m.t.ctx);
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
    loop {
        if !reference()
            && let Some(y) = hot(m, pc, fuel)?
        {
            return Ok(y);
        }
        loop {
            // Fuel is only checked at loop back-edges and calls: those are statement
            // boundaries, so a preempted thread never leaves operands on the shared stacks.
            *fuel = fuel.saturating_sub(1);
            if let Some(y) = step(m, pc, fuel)? {
                return Ok(y);
            }
            if !reference() && is_hot(m.prog.code[*pc]) {
                break;
            }
        }
    }
}

/// The ops [`hot`] leaves to [`step`], as a pattern.
macro_rules! cold {
    () => {
        Op::Checkpoint
        | Op::PushR(_)
        | Op::PushS(_)
        | Op::LdR(_)
        | Op::StR(_)
        | Op::LdS(_)
        | Op::StS(_)
        | Op::LdRA(_)
        | Op::StRA(_)
        | Op::LdSA(_)
        | Op::StSA(_)
        | Op::UiId(_)
        | Op::Ref(_)
        | Op::PopR
        | Op::PopS
        | Op::IToS
        | Op::RToS
        | Op::RAdd
        | Op::RSub
        | Op::RMul
        | Op::RDiv
        | Op::RMod
        | Op::RNeg
        | Op::REq
        | Op::RNe
        | Op::RLt
        | Op::RGt
        | Op::RLe
        | Op::RGe
        | Op::SEq
        | Op::SNe
        | Op::Concat
        | Op::Builtin(..)
        | Op::Declare(_)
        | Op::InitArray(_)
        | Op::Loop(_)
    };
}

/// Whether [`hot`] runs `op`.
fn is_hot(op: Op) -> bool {
    !matches!(op, cold!())
}

/// Run the op at `pc`, already charged. This is every op's reference
/// implementation; [`hot`] repeats the integer ones without the stack's and
/// the machine's indirections.
#[inline(never)]
fn step(m: &mut Machine, pc: &mut usize, fuel: &mut u64) -> Exec<Option<Yield>> {
    {
        let op = m.prog.code[*pc].replaced();
        *pc += 1;
        let s = &mut *m.stk;
        if s.ints.len() == s.ints.capacity()
            || s.reals.len() == s.reals.capacity()
            || s.refs.len() == s.refs.capacity()
        {
            return Err(Fault("KSP operand stack capacity exhausted"));
        }
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
                let x = $e;
                if !x.is_finite() {
                    m.env.fault(m.slot.index, *pc as u32, NONFINITE);
                }
                s.reals.push(x);
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
            Op::Checkpoint => {
                if *fuel == 0 {
                    *pc -= 1;
                    return Ok(Some(Yield::OutOfFuel));
                }
                if m.env.deadline.is_some_and(|end| std::time::Instant::now() >= end) {
                    *pc -= 1;
                    return Ok(Some(Yield::OutOfTime));
                }
            }
            Op::PushI(n) => s.ints.push(n),
            Op::PushR(i) => s.reals.push(m.prog.reals[i as usize]),
            Op::PushS(i) => s.strs.push_str(&m.prog.strings[i as usize])?,
            Op::LdI(i) => s.ints.push(mem.ints[i as usize]),
            Op::StI(i) => mem.ints[i as usize] = s.int(),
            Op::LdR(i) => s.reals.push(mem.reals[i as usize]),
            Op::StR(i) => mem.reals[i as usize] = s.real(),
            Op::LdS(i) => s.strs.push_str(&mem.strs[i as usize])?,
            Op::StS(i) => {
                let dst = &mut mem.strs[i as usize];
                put_variable_text(dst, s.strs.pop(), m.env.loading)?;
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
                let x = element(m, *pc, v, i).map_or(0, |e| m.slot.mem.ints[e]);
                m.stk.ints.push(x);
            }
            Op::StIA(v) => {
                let value = s.int();
                let i = s.int();
                if let Some(e) = element(m, *pc, v, i)
                    && m.slot.mem.ints[e] != value
                {
                    m.slot.mem.ints[e] = value;
                }
            }
            Op::LdRA(v) => {
                let i = s.int();
                let x = element(m, *pc, v, i).map_or(0.0, |e| m.slot.mem.reals[e]);
                m.stk.reals.push(x);
            }
            Op::StRA(v) => {
                let value = s.real();
                let i = s.int();
                if let Some(e) = element(m, *pc, v, i) {
                    m.slot.mem.reals[e] = value;
                }
            }
            Op::LdSA(v) => {
                let i = s.int();
                match element(m, *pc, v, i) {
                    Some(e) => m.stk.strs.push_str(&m.slot.mem.strs[e])?,
                    None => m.stk.strs.push_str("")?,
                }
            }
            Op::StSA(v) => {
                let i = s.int();
                let e = element(m, *pc, v, i);
                let text = m.stk.strs.pop();
                if let Some(e) = e {
                    let dst = &mut m.slot.mem.strs[e];
                    put_variable_text(dst, text, m.env.loading)?;
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
                format_text(s.strs.push()?, format_args!("{n}"), m.env.loading)?;
            }
            Op::RToS => {
                let n = s.real();
                format_text(s.strs.push()?, format_args!("{n}"), m.env.loading)?;
            }
            Op::IAdd => int2!(|a, b| a.wrapping_add(b)),
            Op::ISub => int2!(|a, b| a.wrapping_sub(b)),
            Op::IMul => int2!(|a, b| a.wrapping_mul(b)),
            // Kontakt's integer evaluator returns 0 silently for a zero divisor.
            Op::IDiv => {
                let b = s.int();
                let a = s.int();
                s.ints.push(if b == 0 { 0 } else { a.wrapping_div(b) });
            }
            Op::IMod => {
                let b = s.int();
                let a = s.int();
                s.ints.push(if b == 0 { 0 } else { a.wrapping_rem(b) });
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
                s.strs.concat()?;
                if s.strs.top().len() > 65536 {
                    return Err(Fault("KSP string length limit"));
                }
            }
            Op::Jump(t) => {
                let back = (t as usize) < *pc;
                *pc = t as usize;
                if back && *fuel == 0 {
                    return Ok(Some(Yield::OutOfFuel));
                }
                if back && m.env.deadline.is_some_and(|end| std::time::Instant::now() >= end) {
                    return Ok(Some(Yield::OutOfTime));
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
                    return Ok(Some(Yield::OutOfFuel));
                }
                if m.env.deadline.is_some_and(|end| std::time::Instant::now() >= end) {
                    *pc -= 1;
                    return Ok(Some(Yield::OutOfTime));
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
                    return Ok(Some(Yield::Done));
                }
                m.t.depth -= 1;
                *pc = m.t.calls[m.t.depth as usize] as usize;
            }
            Op::Halt => return Ok(Some(Yield::Done)),
            Op::AddVarImm(a, n) => {
                let x = &mut mem.ints[a as usize];
                *x = x.wrapping_add(n);
                *pc += 3;
            }
            Op::MulAdd(n, a, b) => {
                let x = n.wrapping_mul(mem.ints[a as usize]);
                s.ints.push(x.wrapping_add(mem.ints[b as usize]));
                *pc += 4;
            }
            Op::AddImm(n) => {
                let a = s.int();
                s.ints.push(a.wrapping_add(n));
                *pc += 1;
            }
            Op::LdIAVar(v, a) => {
                let i = mem.ints[a as usize];
                let x = element(m, *pc, v, i).map_or(0, |e| m.slot.mem.ints[e]);
                m.stk.ints.push(x);
                *pc += 1;
            }
            Op::LdIA2(v, a, b, n) => {
                let i = i32::from(n).wrapping_mul(mem.ints[a as usize]);
                let i = i.wrapping_add(mem.ints[b as usize]);
                let x = element(m, *pc + 5, v, i).map_or(0, |e| m.slot.mem.ints[e]);
                m.stk.ints.push(x);
                *pc += 5;
            }
            Op::LdIAPoly(v, p) => {
                let i = mem.poly[(m.t.ctx.poly_row * m.prog.poly + p) as usize];
                let x = element(m, *pc + 1, v, i).map_or(0, |e| m.slot.mem.ints[e]);
                m.stk.ints.push(x);
                *pc += 1;
            }
            Op::BrIAImm(v, cmp, n, t) => {
                let i = s.int();
                let x = element(m, *pc, v, i).map_or(0, |e| m.slot.mem.ints[e]);
                *pc = if cmp.test(x, n) { *pc + 3 } else { t as usize };
            }
            Op::BrCmp(cmp, t) => {
                let b = s.int();
                let a = s.int();
                *pc = if cmp.test(a, b) { *pc + 1 } else { t as usize };
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
            Op::IncBr(..)
            | Op::BrIAVar(..)
            | Op::BrIA2(..)
            | Op::BrPolyIA2(..)
            | Op::LdIASum(..)
            | Op::AddVars(..)
            | Op::LdIAdd(..) => unreachable!("chains run as the op they replaced"),
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
            Op::InitArray(i) => init_array(m, i)?,
            Op::Builtin(b, argc) => {
                m.t.pc = *pc as u32;
                let step = match calls::call(m, b, argc, fuel) {
                    Err(f) if f == calls::NO_CONTROL || f == calls::NO_PGS_KEY => {
                        m.env.fault(m.slot.index, m.t.pc, f.0);
                        match b.sig().ret {
                            Ret::Int | Ret::Num => m.stk.ints.push(0),
                            Ret::Real => m.stk.reals.push(0.0),
                            Ret::Str => {
                                m.stk.strs.push()?;
                            }
                            Ret::Void => {}
                        }
                        Step::Next
                    }
                    step => step?,
                };
                match step {
                    Step::Next => {}
                    Step::Wait(at) => return Ok(Some(Yield::Wait(at))),
                    Step::Exit => {
                        if m.t.depth == 0 {
                            return Ok(Some(Yield::Done));
                        }
                        m.t.depth -= 1;
                        *pc = m.t.calls[m.t.depth as usize] as usize;
                    }
                }
            }
        }
    }
    Ok(None)
}

#[cfg(test)]
thread_local! {
    /// Tests: run every op through [`step`], the reference implementation.
    pub static REFERENCE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

#[inline(always)]
fn reference() -> bool {
    #[cfg(test)]
    return REFERENCE.get();
    #[cfg(not(test))]
    false
}

#[cold]
#[inline(never)]
fn out_of_bounds(env: &mut Env, slot: u8, pc: usize, variable: VarId, index: i32, length: u32) {
    env.fault_context = Some(super::runtime::FaultContext::ArrayIndex { variable, index, length });
    env.fault(slot, pc as u32, OUT_OF_BOUNDS);
}

const OUT_OF_BOUNDS: &str = "Array index out of bounds (read 0, write ignored)";

/// `if taken { yes } else { no }` as a jump, never a `cmov`: script branches
/// predict well, and a predicted jump lets the next op's loads start before
/// this one's compare resolves. As a `cmov`, every op waited on the last.
#[inline(always)]
fn pick(taken: bool, yes: usize, no: usize) -> usize {
    if taken {
        yes
    } else {
        // SAFETY: an empty asm block; it only keeps LLVM from merging the arms.
        unsafe { std::arch::asm!("", options(nomem, nostack, preserves_flags)) };
        no
    }
}

/// The `BrImm` at `p` that ends a chain, testing `x` (which it would have
/// popped); any other op gets `x` pushed and runs next.
#[inline(always)]
fn br_imm(code: &[Op], s: &mut Vec<i32>, p: &mut usize, f: &mut i64, x: i32) {
    if let Op::BrImm(cmp, n, to) = code[*p] {
        *f -= 1;
        *p = pick(cmp.test(x, n), *p + 3, to as usize);
    } else {
        s.push(x);
    }
}

/// [`step`]'s integer core, for the ops that dominate real scripts, as one
/// loop that keeps the int stack, memory and code in registers. It stops
/// before the first op it leaves to `step` (strings, reals, builtins,
/// declarations) and returns `None`, without charging for that op.
#[inline(always)]
fn hot(m: &mut Machine, pc: &mut usize, fuel: &mut u64) -> Exec<Option<Yield>> {
    let prog = m.prog;
    let (code, elems) = (&prog.code[..], &prog.elems[..]);
    let slot = m.slot.index;
    let mem = &mut m.slot.mem;
    let (ints, poly) = (&mut mem.ints, &mut mem.poly[..]);
    let env = &mut *m.env;
    let t = &mut *m.t;
    let row = t.ctx.poly_row * prog.poly;
    // Moved out so its length and capacity live in registers.
    let mut s = std::mem::take(&mut m.stk.ints);
    // Fuel as a plain count down: it may go below zero between the checks,
    // which test for `<= 0` where the saturating count tested for `== 0`.
    let (mut p, mut f) = (*pc, i64::try_from(*fuel).unwrap_or(i64::MAX));
    macro_rules! pop {
        () => {
            s.pop().expect("compiler balanced the int stack")
        };
    }
    macro_rules! int2 {
        (|$a:ident, $b:ident| $e:expr) => {{
            let $b = pop!();
            let $a = pop!();
            s.push($e);
        }};
    }
    // Like `element`.
    macro_rules! elem {
        ($v:expr, $i:expr, $at:expr) => {{
            let (base, len) = elems[$v as usize];
            let index = $i;
            match u32::try_from(index) {
                Ok(i) if i < len => Some((base + i) as usize),
                _ => {
                    out_of_bounds(env, slot, $at, $v, index, len);
                    None
                }
            }
        }};
    }
    let result = loop {
        f -= 1;
        let op = code[p];
        p += 1;
        if s.len() == s.capacity() {
            break Err(Fault("KSP operand stack capacity exhausted"));
        }
        match op {
            Op::PushI(n) => s.push(n),
            Op::LdI(i) => s.push(ints[i as usize]),
            Op::StI(i) => ints[i as usize] = pop!(),
            Op::LdPoly(i) => s.push(poly[(row + i) as usize]),
            Op::StPoly(i) => poly[(row + i) as usize] = pop!(),
            Op::LdIA(v) => {
                let i = pop!();
                let x = elem!(v, i, p).map_or(0, |e| ints[e]);
                s.push(x);
            }
            Op::StIA(v) => {
                let value = pop!();
                let i = pop!();
                if let Some(e) = elem!(v, i, p)
                    && ints[e] != value
                {
                    ints[e] = value;
                }
            }
            Op::Sys(v) => s.push(sys_of(&t.ctx, env, slot, v)),
            Op::PopI => drop(pop!()),
            Op::IAdd => int2!(|a, b| a.wrapping_add(b)),
            Op::ISub => int2!(|a, b| a.wrapping_sub(b)),
            Op::IMul => int2!(|a, b| a.wrapping_mul(b)),
            Op::IDiv => {
                let b = pop!();
                let a = pop!();
                s.push(if b == 0 { 0 } else { a.wrapping_div(b) });
            }
            Op::IMod => {
                let b = pop!();
                let a = pop!();
                s.push(if b == 0 { 0 } else { a.wrapping_rem(b) });
            }
            Op::INeg => {
                let a = pop!();
                s.push(a.wrapping_neg());
            }
            Op::IBitAnd => int2!(|a, b| a & b),
            Op::IBitOr => int2!(|a, b| a | b),
            Op::IBitXor => int2!(|a, b| a ^ b),
            Op::IBitNot => {
                let a = pop!();
                s.push(!a);
            }
            Op::INot => {
                let a = pop!();
                s.push(bool_int(a == 0));
            }
            Op::IEq => int2!(|a, b| bool_int(a == b)),
            Op::INe => int2!(|a, b| bool_int(a != b)),
            Op::ILt => int2!(|a, b| bool_int(a < b)),
            Op::IGt => int2!(|a, b| bool_int(a > b)),
            Op::ILe => int2!(|a, b| bool_int(a <= b)),
            Op::IGe => int2!(|a, b| bool_int(a >= b)),
            Op::Jump(to) => {
                let back = (to as usize) < p;
                p = to as usize;
                if back && f <= 0 {
                    break Ok(Some(Yield::OutOfFuel));
                }
                if back && env.deadline.is_some_and(|end| std::time::Instant::now() >= end) {
                    break Ok(Some(Yield::OutOfTime));
                }
            }
            Op::JumpIfZero(to) => p = pick(pop!() == 0, to as usize, p),
            Op::JumpIfNonZero(to) => p = pick(pop!() != 0, to as usize, p),
            Op::Case(c) => {
                let arm = prog.cases[c as usize];
                let v = s[s.len() - 1];
                if (arm.low..=arm.high).contains(&v) {
                    s.pop();
                } else {
                    p = arm.miss as usize;
                }
            }
            Op::Call(func) => {
                if f <= 0 {
                    p -= 1;
                    break Ok(Some(Yield::OutOfFuel));
                }
                if env.deadline.is_some_and(|end| std::time::Instant::now() >= end) {
                    p -= 1;
                    break Ok(Some(Yield::OutOfTime));
                }
                let depth = t.depth as usize;
                if depth >= MAX_CALL_DEPTH {
                    break Err(Fault("KSP call nesting limit"));
                }
                t.calls[depth] = p as u32;
                t.depth += 1;
                p = prog.functions[func as usize] as usize;
            }
            Op::Ret => {
                t.depth -= 1;
                p = t.calls[t.depth as usize] as usize;
            }
            Op::Exit => {
                if t.depth == 0 {
                    break Ok(Some(Yield::Done));
                }
                t.depth -= 1;
                p = t.calls[t.depth as usize] as usize;
            }
            Op::Halt => break Ok(Some(Yield::Done)),
            Op::AddVarImm(a, n) => {
                let x = &mut ints[a as usize];
                *x = x.wrapping_add(n);
                p += 3;
            }
            Op::MulAdd(n, a, b) => {
                let x = n.wrapping_mul(ints[a as usize]);
                s.push(x.wrapping_add(ints[b as usize]));
                p += 4;
            }
            Op::AddImm(n) => {
                let a = pop!();
                s.push(a.wrapping_add(n));
                p += 1;
            }
            Op::LdIAVar(v, a) => {
                let i = ints[a as usize];
                let x = elem!(v, i, p).map_or(0, |e| ints[e]);
                s.push(x);
                p += 1;
            }
            Op::LdIA2(v, a, b, n) => {
                let i = i32::from(n).wrapping_mul(ints[a as usize]);
                let i = i.wrapping_add(ints[b as usize]);
                let x = elem!(v, i, p + 5).map_or(0, |e| ints[e]);
                s.push(x);
                p += 5;
            }
            Op::LdIAPoly(v, at) => {
                let i = poly[(row + at) as usize];
                let x = elem!(v, i, p + 1).map_or(0, |e| ints[e]);
                s.push(x);
                p += 1;
            }
            Op::BrIAImm(v, cmp, n, to) => {
                let i = pop!();
                let x = elem!(v, i, p).map_or(0, |e| ints[e]);
                p = pick(cmp.test(x, n), p + 3, to as usize);
            }
            Op::BrCmp(cmp, to) => {
                let b = pop!();
                let a = pop!();
                p = pick(cmp.test(a, b), p + 1, to as usize);
            }
            Op::BrImm(cmp, n, to) => {
                p = pick(cmp.test(pop!(), n), p + 2, to as usize);
            }
            Op::BrVarImm(cmp, a, n, to) => {
                p = pick(cmp.test(ints[a as usize], n), p + 3, to as usize);
            }
            // Chains: each charges what the ops it runs cost one at a time,
            // and reports faults at their pcs.
            Op::IncBr(a, n, to) => {
                let x = &mut ints[a as usize];
                *x = x.wrapping_add(n);
                // The `Jump` at `p + 3`.
                f -= 1;
                let back = (to as usize) < p + 4;
                p = to as usize;
                if back && f <= 0 {
                    break Ok(Some(Yield::OutOfFuel));
                }
                if back && env.deadline.is_some_and(|end| std::time::Instant::now() >= end) {
                    break Ok(Some(Yield::OutOfTime));
                }
                if let Op::BrVarImm(cmp, b, k, exit) = code[p] {
                    f -= 1;
                    p = pick(cmp.test(ints[b as usize], k), p + 4, exit as usize);
                }
            }
            Op::BrIAVar(v, a) => {
                let x = elem!(v, ints[a as usize], p).map_or(0, |e| ints[e]);
                p += 1;
                br_imm(code, &mut s, &mut p, &mut f, x);
            }
            Op::BrIA2(v, a, b, n) => {
                let i = i32::from(n).wrapping_mul(ints[a as usize]);
                let i = i.wrapping_add(ints[b as usize]);
                let x = elem!(v, i, p + 5).map_or(0, |e| ints[e]);
                p += 5;
                br_imm(code, &mut s, &mut p, &mut f, x);
            }
            Op::BrPolyIA2(v, at) => {
                let x = elem!(v, poly[(row + at) as usize], p + 1).map_or(0, |e| ints[e]);
                p += 1;
                if let (Op::LdIA2(w, a, b, n), Op::AddImm(k), Op::BrCmp(cmp, to)) =
                    (code[p], code[p + 6], code[p + 8])
                {
                    let i = i32::from(n).wrapping_mul(ints[a as usize]);
                    let i = i.wrapping_add(ints[b as usize]);
                    let y = elem!(w, i, p + 6).map_or(0, |e| ints[e]).wrapping_add(k);
                    f -= 3;
                    p = pick(cmp.test(x, y), p + 10, to as usize);
                } else {
                    s.push(x);
                }
            }
            Op::LdIASum(v, a, b) => {
                let i = ints[a as usize].wrapping_add(ints[b as usize]);
                let x = elem!(v, i, p + 3).map_or(0, |e| ints[e]);
                s.push(x);
                f -= 3;
                p += 3;
            }
            Op::AddVars(a, b) => {
                s.push(ints[a as usize].wrapping_add(ints[b as usize]));
                f -= 2;
                p += 2;
            }
            Op::LdIAdd(a, n) => {
                s.push(ints[a as usize].wrapping_add(n));
                f -= 1;
                p += 2;
            }
            cold!() => {
                p -= 1;
                f += 1;
                break Ok(None);
            }
        }
    };
    m.stk.ints = s;
    (*pc, *fuel) = (p, f.max(0) as u64);
    result
}

fn sys(m: &Machine, v: SysVar) -> i32 {
    sys_of(&m.t.ctx, m.env, m.slot.index, v)
}

#[inline(never)]
fn sys_of(ctx: &Ctx, env: &Env, slot: u8, v: SysVar) -> i32 {
    let event = || env.events.get(ctx.event);
    match v {
        SysVar::EventId => ctx.event,
        SysVar::EventNote => event().map_or(0, |e| e.note),
        SysVar::EventVelocity => event().map_or(0, |e| e.velocity),
        SysVar::NoteHeld => bool_int(env.events.held(ctx.event)),
        SysVar::CcNum => ctx.cc,
        SysVar::PitchBend => env.input.pitch_bend,
        SysVar::PolyAtNum | SysVar::NcNote => ctx.note,
        SysVar::NcNum => ctx.cc,
        SysVar::NcValue => ctx.value,
        SysVar::RpnAddress => ctx.cc,
        SysVar::RpnValue => ctx.value,
        SysVar::MidiChannel => i32::from(ctx.channel),
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
        SysVar::DurationBar => if env.transport {
            (60e6 / env.tempo * 4.0 * f64::from(env.signature.0) / f64::from(env.signature.1)) as i32
        } else { 0 },
        SysVar::SongPosition => env.song_position(),
        SysVar::SignatureNum => i32::from(env.signature.0),
        SysVar::SignatureDenom => i32::from(env.signature.1),
        SysVar::TransportRunning => bool_int(env.transport),
        SysVar::Tempo => env.tempo as i32,
        SysVar::CurrentScriptSlot => i32::from(slot),
        SysVar::UiId => ctx.ui_id,
        SysVar::PlayedVoices => env.events.live_count() as i32,
        // No song position from the host yet, so every bar starts now.
        SysVar::DistanceBarStart => 0,
        SysVar::Date(i) => civil_now()[i as usize],
        SysVar::Time(i) => civil_now()[3 + i as usize],
    }
}

/// Year, month, day, hour, minute, second of the wall clock.
// ponytail: UTC; Kontakt reports local time, which needs a time zone database.
fn civil_now() -> [i32; 6] {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64);
    let (days, rest) = (secs.div_euclid(86_400), secs.rem_euclid(86_400) as i32);
    // Days to civil date (H. Hinnant).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as i32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as i32;
    let year = (yoe + era * 400 + i64::from(month <= 2)) as i32;
    [year, month, day, rest / 3600, rest / 60 % 60, rest % 60]
}

fn declare(m: &mut Machine, v: VarId) -> Exec<()> {
    let var = &m.prog.vars[v as usize];
    if !m.env.loading && var.ui.is_some() && m.slot.ui.var_id(v) == 0 {
        return Err(Fault(
            "KSP UI controls must be declared during initialization",
        ));
    }
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

fn init_array(m: &mut Machine, i: u32) -> Exec<()> {
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
                put_variable_text(s, &m.prog.strings[k as usize], m.env.loading)?;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod revision_tests {
    use super::*;

    #[test]
    fn shared_revision_owners_cover_sparse_slots_bulk_writes_and_all_types() {
        let setup = super::super::compile::Setup { groups: 0, zones: 0, outputs: 8 };
        let p = super::super::compile::compile(r#"on init
declare %gap[255]
declare ui_table %table[513](1,1,127)
declare %large_gap[1000000]
declare ui_slider $last(0,127)
declare ui_xy ?xy[2]
declare ui_text_edit @text
end on"#, &setup).unwrap();
        let mut first = SlotState::new(0, &p);
        let second = SlotState::new(0, &p);
        assert!(std::sync::Arc::ptr_eq(&first.mem.ints.owners, &second.mem.ints.owners));
        let slot = |name| p.vars.iter().find(|v| &*v.name == name).unwrap().slot as usize;
        let table = slot("%table");
        let last = slot("$last");
        assert_eq!(table, 255);
        for i in [0,254,768,1000000] { first.mem.ints[i] = 1; }
        assert_eq!(first.mem.ints.revision(table), 0, "unowned cells do not dirty UI values");
        for i in [255,256,511,512,767] { first.mem.ints[i] = 2; }
        assert_eq!(first.mem.ints.revision(table), 5);
        first.mem.ints[254..769].fill(3);
        assert_eq!(first.mem.ints.revision(table), 6, "bulk borrows bump a contiguous variable once");
        first.mem.ints.copy_within(255..768, 255);
        assert_eq!(first.mem.ints.revision(table), 7);
        for value in &mut first.mem.ints { *value += 1; }
        assert_eq!(first.mem.ints.revision(table), 8);
        assert_eq!(first.mem.ints.revision(last), 1);
        assert_eq!(second.mem.ints.revision(table), 0, "only ownership is shared, revisions remain local");
        let xy = slot("?xy");
        first.mem.reals[xy..xy+2].fill(0.5);
        first.mem.strs[slot("@text")].push_str("updated");
        assert_eq!(first.mem.reals.revision(xy), 1);
        assert_eq!(first.mem.strs.revision(slot("@text")), 1);

        let dense = super::super::compile::compile("on init\ndeclare ui_table %dense[4096](1,1,127)\nend on", &setup).unwrap();
        let mut dense = SlotState::new(0, &dense);
        dense.mem.ints[..].fill(1);
        assert_eq!(dense.mem.ints.revision(4095), 1);
        let plain = super::super::compile::compile("on init\ndeclare %plain[4096]\nend on", &setup).unwrap();
        let mut plain = SlotState::new(0, &plain);
        plain.mem.ints[..].fill(1);
        assert_eq!(plain.mem.ints.revision(4095), 0);
        assert_eq!(plain.mem.ints.owners.len(), 0);
    }
}
