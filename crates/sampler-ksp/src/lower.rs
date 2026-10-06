//! HIR -> sampler-core programs. One program per callback; functions a callback
//! reaches are appended to its program as subroutines lowered in the caller's
//! event context. Expressions evaluate into a destination register and use the
//! registers above it as temporaries, keeping callbacks within small budgets.
use crate::builtins::{self as b, Builtin, SysArray, SysVar};
use crate::diag::{Fault, Result, Span, fault};
use crate::hir::*;
use crate::sema::fold;
use sampler_core::{
    Comparison as Cmp, ControlId, Duration, DurationValue, Inheritance, Instruction as I,
    IntegerBinary as IB, IntegerExtra, IntegerUnary as IU, Op, Program, RealBinary, RealUnary,
    ScriptArray, TextPart, TextRef, WaitLifetime, real_bits,
};
use std::collections::{BTreeMap, BTreeSet, HashMap};

/// Event context a program runs in; decides which event operands exist.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Context {
    Note,
    Release,
    Controller,
    /// Instrument-owned callback (UI control, listener, ...), no originating event.
    Plan,
}

/// How a builtin call site was lowered.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Coverage {
    /// Executed by the engine.
    Native,
    /// Executed with a documented approximation.
    Approximate,
    /// Queued to the host through the effect outbox.
    Effect,
    /// Not executed at runtime; a warning was recorded.
    Ignored,
}

/// Store key tags separating UI properties and PGS values from engine keys.
pub const PROPERTY_TAG: i32 = i32::MIN;
pub const PGS_TAG: i32 = i32::MIN + 1;

/// Host value slot for a system variable; see `Runtime::set_host_value`.
pub fn host_slot(sys: SysVar) -> Option<u8> {
    use SysVar::*;
    Some(match sys {
        PitchBend => 0,
        PolyAtNum => 1,
        RpnAddress => 2,
        RpnValue => 3,
        MidiChannel => 4,
        SignalType => 5,
        AsyncId => 6,
        AsyncExitStatus => 7,
        DurationQuarter => 8,
        DurationEighth => 9,
        DurationSixteenth => 10,
        DurationQuarterTriplet => 11,
        DurationEighthTriplet => 12,
        DurationSixteenthTriplet => 13,
        DurationBar => 14,
        SongPosition => 15,
        SignatureNum => 16,
        SignatureDenom => 17,
        TransportRunning => 18,
        Tempo => 19,
        PlayedVoicesTotal => 20,
        PlayedVoicesInst => 21,
        DistanceBarStart => 22,
        NumZones => 23,
        NumOutputChannels => 24,
        MouseOverControl => 25,
        Date(n) => 26 + n,
        Time(n) => 29 + n,
        _ => return None,
    })
}

/// FNV-1a 32-bit, for PGS key names in store keys.
pub fn name_hash(name: &str) -> i32 {
    let mut h: u32 = 0x811c_9dc5;
    for b in name.bytes() {
        h = (h ^ u32::from(b)).wrapping_mul(0x0100_0193);
    }
    h as i32
}

pub struct Unit<'h> {
    pub hir: &'h Hir,
    /// Host control per UI index.
    pub controls: &'h [Option<ControlId>],
    pub groups: &'h [String],
    pub pgs: &'h BTreeSet<String>,
    pub slot: u8,
    /// Remaining instruction budget for the whole script.
    pub budget: usize,
    pub services: Vec<Builtin>,
    pub coverage: BTreeMap<(&'static str, Coverage), usize>,
    pub warnings: Vec<Fault>,
    /// Scratch text cells used above `hir.texts`.
    pub scratch: u32,
}

impl<'h> Unit<'h> {
    pub fn program(
        &mut self,
        body: &[Stmt],
        span: Span,
        context: Context,
        kind: CallbackKind,
    ) -> Result<Program> {
        let ui_id = match kind {
            CallbackKind::UiControl(var) => self.hir.vars[var.0 as usize]
                .ui
                .map(|ui| b::FIRST_UI_ID + ui as i32),
            _ => None,
        };
        let mut g = Gen {
            u: self,
            ctx: context,
            callback_type: callback_type(kind),
            ui_id,
            code: Vec::new(),
            texts: Vec::new(),
            calls: Vec::new(),
            starts: HashMap::new(),
            loops: Vec::new(),
            span,
            tdepth: 0,
        };
        g.block(body)?;
        g.forward()?;
        g.emit(I::End)?;
        // Append reached functions breadth-first; calls are patched afterwards.
        let mut next = 0;
        while next < g.calls.len() {
            let f = g.calls[next].1;
            next += 1;
            if g.starts.contains_key(&f) {
                continue;
            }
            g.starts.insert(f, g.code.len());
            let function = &g.u.hir.functions[f.0 as usize];
            g.span = function.span;
            g.block(&function.body)?;
            g.emit(I::Op(Op::Return))?;
        }
        for &(at, f) in &g.calls {
            g.code[at] = I::Op(Op::Call {
                target: g.starts[&f] as u32,
            });
        }
        let texts: Vec<&str> = g.texts.iter().map(String::as_str).collect();
        Program::new(g.code)
            .and_then(|p| p.with_texts(&texts))
            .map(|p| p.with_wait_lifetime(WaitLifetime::Callback))
            .map_err(|e| Fault {
                span,
                message: format!("invalid lowered program: {e:?}"),
            })
    }

    fn service(&mut self, builtin: Builtin) -> u16 {
        match self.services.iter().position(|s| *s == builtin) {
            Some(i) => i as u16,
            None => {
                self.services.push(builtin);
                (self.services.len() - 1) as u16
            }
        }
    }
}

fn callback_type(kind: CallbackKind) -> i32 {
    match kind {
        CallbackKind::Init => b::cb::INIT,
        CallbackKind::Note => b::cb::NOTE,
        CallbackKind::Release => b::cb::RELEASE,
        CallbackKind::Controller => b::cb::CONTROLLER,
        CallbackKind::PolyAt => b::cb::POLY_AT,
        CallbackKind::UiControl(_) => b::cb::UI_CONTROL,
        CallbackKind::Listener => b::cb::LISTENER,
        CallbackKind::PgsChanged => b::cb::PGS_CHANGED,
        CallbackKind::PersistenceChanged => b::cb::PERSISTENCE_CHANGED,
        CallbackKind::AsyncComplete => b::cb::ASYNC_COMPLETE,
        CallbackKind::Rpn => 5,
        CallbackKind::Nrpn => 6,
    }
}

struct Gen<'u, 'h> {
    u: &'u mut Unit<'h>,
    ctx: Context,
    callback_type: i32,
    ui_id: Option<i32>,
    code: Vec<I>,
    texts: Vec<String>,
    calls: Vec<(usize, FnId)>,
    starts: HashMap<FnId, usize>,
    /// Condition position of each enclosing `while`, for `continue`.
    loops: Vec<usize>,
    span: Span,
    tdepth: u32,
}

#[derive(Clone, Copy)]
enum Key {
    Arg(usize),
    Fixed(i32),
}
/// (ui id, parameter, tag, tag).
const PROPERTY_KEY: [Key; 4] = [
    Key::Arg(0),
    Key::Arg(1),
    Key::Fixed(PROPERTY_TAG),
    Key::Fixed(PROPERTY_TAG),
];

fn reg(r: u16, n: u16) -> Result<u16> {
    r.checked_add(n).ok_or_else(|| Fault {
        span: Span::default(),
        message: "register range exceeded".into(),
    })
}

impl Gen<'_, '_> {
    fn emit(&mut self, op: I) -> Result<()> {
        if self.u.budget == 0 {
            return fault(self.span, "instruction budget exceeded");
        }
        self.u.budget -= 1;
        self.code.push(op);
        Ok(())
    }
    fn set(&mut self, local: u16, value: i64) -> Result<()> {
        self.emit(I::SetLocal { local, value })
    }
    fn here(&self) -> usize {
        self.code.len()
    }
    /// Placeholder branch, patched by `land`.
    fn jump_if_zero(&mut self, local: u16) -> Result<usize> {
        let at = self.here();
        self.emit(I::JumpIfZero { local, target: 0 })?;
        Ok(at)
    }
    fn jump(&mut self) -> Result<usize> {
        let at = self.here();
        self.emit(I::Jump { target: 0 })?;
        Ok(at)
    }
    fn land(&mut self, at: usize) {
        let target = self.here();
        match &mut self.code[at] {
            I::Jump { target: t } | I::JumpIfZero { target: t, .. } => *t = target,
            _ => unreachable!("patched instruction is a branch"),
        }
    }
    fn warn(&mut self, message: impl Into<String>) {
        if self.u.warnings.len() < 1000 {
            self.u.warnings.push(Fault {
                span: self.span,
                message: message.into(),
            });
        }
    }
    fn cover(&mut self, builtin: Builtin, coverage: Coverage) {
        *self
            .u
            .coverage
            .entry((builtin.name(), coverage))
            .or_default() += 1;
    }
    fn ignore(&mut self, builtin: Builtin, why: &str) {
        self.cover(builtin, Coverage::Ignored);
        self.warn(format!("{} {why}", builtin.name()));
    }
    fn note_context(&self) -> bool {
        matches!(self.ctx, Context::Note | Context::Release)
    }
    fn forward(&mut self) -> Result<()> {
        match self.ctx {
            Context::Note => self.emit(I::ForwardAttack),
            Context::Release => self.emit(I::ForwardReleaseGroups),
            Context::Controller => self.emit(I::ForwardController),
            Context::Plan => Ok(()),
        }
    }
    fn var(&self, v: VarId) -> &Var {
        &self.u.hir.vars[v.0 as usize]
    }
    fn constant(&mut self, text: &str) -> u16 {
        let mut text = text;
        if text.len() > sampler_core::TEXT_CAPACITY {
            let mut end = sampler_core::TEXT_CAPACITY;
            while !text.is_char_boundary(end) {
                end -= 1;
            }
            text = &text[..end];
        }
        match self.texts.iter().position(|t| t == text) {
            Some(i) => i as u16,
            None => {
                self.texts.push(text.to_owned());
                (self.texts.len() - 1) as u16
            }
        }
    }
    fn scratch(&mut self) -> TextRef {
        let n = self.tdepth;
        self.tdepth += 1;
        self.u.scratch = self.u.scratch.max(self.tdepth);
        TextRef::Cell(self.u.hir.texts + n)
    }

    // Statements.

    fn block(&mut self, body: &[Stmt]) -> Result<()> {
        for s in body {
            self.span = s.span;
            self.stmt(s)?;
        }
        Ok(())
    }

    fn stmt(&mut self, s: &Stmt) -> Result<()> {
        match &s.kind {
            StmtKind::Assign(place, value) => self.assign(place, value),
            // Declaration initializers only occur in `on init`.
            StmtKind::Fill(..) => Ok(()),
            StmtKind::If(cond, then, otherwise) => {
                self.value(cond, 0)?;
                let branch = self.jump_if_zero(0)?;
                self.block(then)?;
                if otherwise.is_empty() {
                    self.land(branch);
                } else {
                    let end = self.jump()?;
                    self.land(branch);
                    self.block(otherwise)?;
                    self.land(end);
                }
                Ok(())
            }
            StmtKind::While(cond, body) => {
                let start = self.here();
                self.value(cond, 0)?;
                let branch = self.jump_if_zero(0)?;
                self.loops.push(start);
                self.block(body)?;
                self.loops.pop();
                self.emit(I::Jump { target: start })?;
                self.land(branch);
                Ok(())
            }
            StmtKind::Select(selector, cases) => {
                self.value(selector, 0)?;
                let mut exits = Vec::new();
                for case in cases {
                    let mut misses = Vec::new();
                    if case.low == case.high {
                        self.set(1, i64::from(case.low))?;
                        self.emit(I::CompareLocal {
                            lhs: 1,
                            rhs: 0,
                            comparison: Cmp::Equal,
                        })?;
                        misses.push(self.jump_if_zero(1)?);
                    } else {
                        let (low, high) = (case.low.min(case.high), case.low.max(case.high));
                        for (value, comparison) in
                            [(low, Cmp::LessEqual), (high, Cmp::GreaterEqual)]
                        {
                            self.set(1, i64::from(value))?;
                            self.emit(I::CompareLocal {
                                lhs: 1,
                                rhs: 0,
                                comparison,
                            })?;
                            misses.push(self.jump_if_zero(1)?);
                        }
                    }
                    self.block(&case.body)?;
                    exits.push(self.jump()?);
                    for m in misses {
                        self.land(m);
                    }
                }
                for e in exits {
                    self.land(e);
                }
                Ok(())
            }
            StmtKind::Call(f) => {
                self.calls.push((self.here(), *f));
                self.emit(I::Op(Op::Return))
            }
            StmtKind::Builtin(builtin, args) => self.builtin(*builtin, args, 0, s.span),
        }
    }

    fn assign(&mut self, place: &Place, value: &Expr) -> Result<()> {
        let v = place.var();
        let var = self.var(v).clone();
        match (place, var.home) {
            (Place::Var(_), Home::Text(cell)) => self.text_into(value, TextRef::Cell(cell), 0),
            (Place::Elem(_, index), Home::Texts { offset, len }) => {
                self.value(index, 0)?;
                let array = ScriptArray { offset, len };
                self.text_into(value, TextRef::Element { array, index: 0 }, 1)
            }
            (Place::Elem(_, index), Home::Cells { offset, len }) => {
                self.value(index, 0)?;
                self.value(value, 1)?;
                self.emit(I::WriteScriptArray {
                    array: ScriptArray { offset, len },
                    index: 0,
                    local: 1,
                })
            }
            (Place::Var(_), _) => {
                if let (Home::Control(ui), Some(Const::Int(n))) =
                    (var.home, fold(self.u.hir, value))
                {
                    let (lo, hi) = self.range(ui);
                    self.set(0, i64::from(n.clamp(lo, hi)))?;
                    return self.write_var(v, 0, false);
                }
                self.value(value, 0)?;
                self.write_var(v, 0, true)
            }
            _ => {
                self.warn("assignment target has no runtime storage");
                Ok(())
            }
        }
    }

    fn range(&self, ui: u32) -> (i32, i32) {
        crate::eval::declared_range(&self.u.hir.uis[ui as usize])
            .map_or((i32::MIN, i32::MAX), |(a, b)| (a.min(b), a.max(b)))
    }

    /// Store `local` into a scalar variable. Control writes clamp to the declared
    /// range (Kontakt clamps; the engine rejects out-of-domain values).
    fn write_var(&mut self, v: VarId, local: u16, clamp: bool) -> Result<()> {
        match self.var(v).home {
            Home::Cell(cell) => self.emit(I::WriteScriptCell { cell, local }),
            Home::Note(cell) if self.note_context() => self.emit(I::WriteNoteCell { cell, local }),
            Home::Note(_) => {
                self.warn("polyphonic variable written outside a note callback is ignored");
                Ok(())
            }
            Home::Control(ui) => {
                let (lo, hi) = self.range(ui);
                if clamp && (lo, hi) != (i32::MIN, i32::MAX) {
                    let t = reg(local, 1)?;
                    for (bound, op) in [(lo, IntegerExtra::Max), (hi, IntegerExtra::Min)] {
                        self.set(t, i64::from(bound))?;
                        self.emit(I::Op(Op::Integer {
                            lhs: local,
                            rhs: t,
                            operation: op,
                        }))?;
                    }
                }
                let control = self.u.controls[ui as usize].expect("control-backed variable");
                self.emit(I::WriteControl { control, local })
            }
            _ => {
                self.warn("assignment target has no runtime storage");
                Ok(())
            }
        }
    }

    // Expressions.

    fn value(&mut self, e: &Expr, dst: u16) -> Result<()> {
        if let Some(c) = fold(self.u.hir, e) {
            match c {
                Const::Int(n) => return self.set(dst, i64::from(n)),
                Const::Real(x) => return self.set(dst, real_bits(x)),
                Const::Str(_) => {}
            }
        }
        let t = reg(dst, 1)?;
        match &e.kind {
            ExprKind::Int(n) => self.set(dst, i64::from(*n)),
            ExprKind::Real(x) => self.set(dst, real_bits(*x)),
            ExprKind::Str(_) | ExprKind::Concat(_) => {
                self.warn("text used as a number");
                self.set(dst, 0)
            }
            ExprKind::Load(v) => self.load(*v, dst),
            ExprKind::LoadElem(v, index) => match self.var(*v).home {
                Home::Cells { offset, len } => {
                    self.value(index, dst)?;
                    self.emit(I::ReadScriptArray {
                        array: ScriptArray { offset, len },
                        index: dst,
                        local: dst,
                    })
                }
                _ => {
                    self.warn("element read has no numeric storage");
                    self.set(dst, 0)
                }
            },
            ExprKind::Sys(s) => self.sys(*s, dst),
            ExprKind::SysElem(array, index) => self.sys_elem(*array, index, dst),
            ExprKind::Neg(x) => {
                self.value(x, dst)?;
                self.emit(if x.ty == Ty::Real {
                    I::Op(Op::RealUnary {
                        local: dst,
                        operation: RealUnary::Negate,
                    })
                } else {
                    I::Unary32 {
                        local: dst,
                        operation: IU::Negate,
                    }
                })
            }
            ExprKind::BitNot(x) => {
                self.value(x, dst)?;
                self.emit(I::Unary32 {
                    local: dst,
                    operation: IU::Not,
                })
            }
            ExprKind::Not(x) => {
                self.value(x, dst)?;
                self.boolean(dst, Cmp::Equal)
            }
            ExprKind::Arith(op, l, r) => {
                self.value(l, dst)?;
                self.value(r, t)?;
                self.emit(if l.ty == Ty::Real {
                    let operation = match op {
                        Arith::Add => RealBinary::Add,
                        Arith::Sub => RealBinary::Subtract,
                        Arith::Mul => RealBinary::Multiply,
                        Arith::Div => RealBinary::Divide,
                        _ => return fault(e.span, "integer operator on reals"),
                    };
                    I::Op(Op::Real {
                        lhs: dst,
                        rhs: t,
                        operation,
                    })
                } else {
                    let operation = match op {
                        Arith::Add => IB::Add,
                        Arith::Sub => IB::Subtract,
                        Arith::Mul => IB::Multiply,
                        Arith::Div => IB::Divide,
                        Arith::Mod => IB::Remainder,
                        Arith::BitAnd => IB::And,
                        Arith::BitOr => IB::Or,
                        Arith::BitXor => IB::Xor,
                    };
                    I::Binary32 {
                        lhs: dst,
                        rhs: t,
                        operation,
                    }
                })
            }
            ExprKind::Compare(comparison, l, r) if l.ty == Ty::Str => {
                let (a, b) = (self.scratch(), self.scratch());
                self.text_into(l, a, dst)?;
                self.text_into(r, b, dst)?;
                self.tdepth -= 2;
                self.emit(I::Op(Op::CompareText {
                    lhs: a,
                    rhs: b,
                    local: dst,
                    comparison: *comparison,
                }))
            }
            ExprKind::Compare(comparison, l, r) => {
                self.value(l, dst)?;
                self.value(r, t)?;
                self.emit(if l.ty == Ty::Real {
                    I::Op(Op::CompareReal {
                        lhs: dst,
                        rhs: t,
                        comparison: *comparison,
                    })
                } else {
                    I::CompareLocal {
                        lhs: dst,
                        rhs: t,
                        comparison: *comparison,
                    }
                })
            }
            ExprKind::Logic(Logic::And, l, r) => {
                self.value(l, dst)?;
                let skip = self.jump_if_zero(dst)?;
                self.value(r, dst)?;
                self.land(skip);
                Ok(())
            }
            ExprKind::Logic(Logic::Or, l, r) => {
                self.value(l, dst)?;
                let rhs = self.jump_if_zero(dst)?;
                let end = self.jump()?;
                self.land(rhs);
                self.value(r, dst)?;
                self.land(end);
                Ok(())
            }
            ExprKind::Logic(Logic::Xor, l, r) => {
                self.value(l, dst)?;
                self.value(r, t)?;
                self.emit(I::CompareLocal {
                    lhs: dst,
                    rhs: t,
                    comparison: Cmp::NotEqual,
                })
            }
            ExprKind::Cast(x) => {
                self.value(x, dst)?;
                if e.ty == Ty::Bool && x.ty == Ty::Int {
                    self.boolean(dst, Cmp::NotEqual)?;
                }
                Ok(())
            }
            ExprKind::Builtin(builtin, args) => self.builtin(*builtin, args, dst, e.span),
        }
    }

    /// local := local <comparison> 0.
    fn boolean(&mut self, local: u16, comparison: Cmp) -> Result<()> {
        let t = reg(local, 1)?;
        self.set(t, 0)?;
        self.emit(I::CompareLocal {
            lhs: local,
            rhs: t,
            comparison,
        })
    }

    fn load(&mut self, v: VarId, dst: u16) -> Result<()> {
        match self.var(v).home {
            Home::Cell(cell) => self.emit(I::ReadScriptCell { local: dst, cell }),
            Home::Note(cell) if self.note_context() => {
                self.emit(I::ReadNoteCell { local: dst, cell })
            }
            Home::Note(_) => {
                self.warn("polyphonic variable read outside a note callback is 0");
                self.set(dst, 0)
            }
            Home::Control(ui) => {
                let control = self.u.controls[ui as usize].expect("control-backed variable");
                self.emit(I::ReadControl {
                    local: dst,
                    control,
                })
            }
            Home::Const(k) => match &self.u.hir.consts[k as usize] {
                Const::Int(n) => self.set(dst, i64::from(*n)),
                Const::Real(x) => self.set(dst, real_bits(*x)),
                Const::Str(_) => self.set(dst, 0),
            },
            Home::Cells { len, .. } | Home::Texts { len, .. } => {
                self.warn("array used as a scalar");
                self.set(dst, i64::from(len))
            }
            Home::Text(_) => {
                self.warn("text used as a number");
                self.set(dst, 0)
            }
        }
    }

    fn sys(&mut self, s: SysVar, dst: u16) -> Result<()> {
        let note = self.note_context();
        let op = match s {
            SysVar::EventId | SysVar::CallbackId if note => I::ReadEventId { local: dst },
            SysVar::EventNote if note => I::ReadKey { local: dst },
            SysVar::EventVelocity if note => I::ReadVelocity7 { local: dst },
            SysVar::NoteHeld if note => I::ReadKeyDown { local: dst },
            SysVar::CcNum if self.ctx == Context::Controller => {
                I::ReadControllerNumber { local: dst }
            }
            SysVar::NumGroups => I::ReadGroupCount { local: dst },
            SysVar::EngineUptime => I::Op(Op::ReadClock {
                local: dst,
                micros: 1000,
            }),
            // ponytail: reset_ksp_timer is not honoured; the timer counts from engine start.
            SysVar::KspTimer => I::Op(Op::ReadClock {
                local: dst,
                micros: 1,
            }),
            SysVar::CallbackType => I::SetLocal {
                local: dst,
                value: i64::from(self.callback_type),
            },
            SysVar::UiId => I::SetLocal {
                local: dst,
                value: i64::from(self.ui_id.unwrap_or(0)),
            },
            SysVar::CurrentScriptSlot => I::SetLocal {
                local: dst,
                value: i64::from(self.u.slot),
            },
            s => match host_slot(s) {
                Some(slot) => I::Op(Op::ReadHost { local: dst, slot }),
                None => {
                    self.warn(format!("{s:?} has no value in this callback; reads 0"));
                    I::SetLocal {
                        local: dst,
                        value: 0,
                    }
                }
            },
        };
        self.emit(op)
    }

    fn sys_elem(&mut self, array: SysArray, index: &Expr, dst: u16) -> Result<()> {
        if array == SysArray::Cc {
            self.value(index, dst)?;
            self.emit(I::ReadInputController {
                controller: dst,
                local: dst,
            })?;
            return self.emit(I::ControllerToMidi7 { local: dst });
        }
        self.warn(format!("{array:?} is not maintained at runtime; reads 0"));
        self.set(dst, 0)
    }

    // Text.

    /// Build `e` into `dst`, using registers from `free` upward. Builds through a
    /// scratch cell unless `e` is a plain literal, so `@a := "x" & @a` is safe.
    fn text_into(&mut self, e: &Expr, dst: TextRef, free: u16) -> Result<()> {
        if let ExprKind::Str(s) = &e.kind {
            self.emit(I::Op(Op::TextClear { text: dst }))?;
            let c = self.constant(s);
            return self.emit(I::Op(Op::TextAppend {
                text: dst,
                part: TextPart::Constant(c),
            }));
        }
        let scratch = self.scratch();
        self.emit(I::Op(Op::TextClear { text: scratch }))?;
        self.append(e, scratch, free)?;
        self.tdepth -= 1;
        self.emit(I::Op(Op::TextClear { text: dst }))?;
        self.emit(I::Op(Op::TextAppend {
            text: dst,
            part: TextPart::Text(scratch),
        }))
    }

    fn append(&mut self, e: &Expr, dst: TextRef, free: u16) -> Result<()> {
        let part = match &e.kind {
            ExprKind::Str(s) => TextPart::Constant(self.constant(s)),
            ExprKind::Concat(parts) => {
                for p in parts {
                    self.append(p, dst, free)?;
                }
                return Ok(());
            }
            ExprKind::Load(v) if e.ty == Ty::Str => match self.var(*v).home {
                Home::Text(cell) => TextPart::Text(TextRef::Cell(cell)),
                Home::Const(k) => match &self.u.hir.consts[k as usize] {
                    Const::Str(s) => TextPart::Constant(self.constant(&s.clone())),
                    _ => return Ok(()),
                },
                _ => return Ok(()),
            },
            ExprKind::LoadElem(v, index) if e.ty == Ty::Str => match self.var(*v).home {
                Home::Texts { offset, len } => {
                    self.value(index, free)?;
                    TextPart::Text(TextRef::Element {
                        array: ScriptArray { offset, len },
                        index: free,
                    })
                }
                _ => return Ok(()),
            },
            ExprKind::Builtin(Builtin::GroupName, args) => {
                let name = self
                    .const_int(args, 0)
                    .and_then(|i| self.u.groups.get(usize::try_from(i).ok()?))
                    .cloned();
                self.cover(Builtin::GroupName, Coverage::Native);
                match name {
                    Some(name) => TextPart::Constant(self.constant(&name)),
                    None => {
                        self.warn("group_name of an unknown group is empty");
                        return Ok(());
                    }
                }
            }
            ExprKind::Builtin(builtin, _) if e.ty == Ty::Str => {
                self.ignore(*builtin, "text is not available at runtime; empty");
                return Ok(());
            }
            _ if e.ty == Ty::Str => {
                self.warn("text expression is not available at runtime; empty");
                return Ok(());
            }
            _ => {
                self.value(e, free)?;
                if e.ty == Ty::Real {
                    TextPart::Real(free)
                } else {
                    TextPart::Integer(free)
                }
            }
        };
        self.emit(I::Op(Op::TextAppend { text: dst, part }))
    }

    // Builtins.

    fn expr<'a>(&self, args: &'a [Arg], i: usize) -> Option<&'a Expr> {
        match args.get(i)? {
            Arg::Expr(e) => Some(e),
            _ => None,
        }
    }
    fn const_int(&self, args: &[Arg], i: usize) -> Option<i32> {
        match fold(self.u.hir, self.expr(args, i)?)? {
            Const::Int(n) => Some(n),
            _ => None,
        }
    }
    fn const_text(&self, args: &[Arg], i: usize) -> Option<String> {
        match args.get(i)? {
            Arg::Key(k) => Some(k.to_string()),
            Arg::Expr(e) => match fold(self.u.hir, e)? {
                Const::Str(s) => Some(s.into()),
                Const::Int(n) => Some(n.to_string()),
                Const::Real(x) => Some(crate::eval::real_text(x)),
            },
            _ => None,
        }
    }
    fn arg(&mut self, args: &[Arg], i: usize, dst: u16) -> Result<()> {
        match args.get(i) {
            Some(Arg::Expr(e)) => self.value(e, dst),
            Some(Arg::Var(v, _)) => {
                let id = self.var(*v).ui.map_or(0, |ui| b::FIRST_UI_ID + ui as i32);
                self.set(dst, i64::from(id))
            }
            Some(Arg::Place(p)) => match p {
                Place::Var(v) => self.load(*v, dst),
                Place::Elem(..) => self.set(dst, 0),
            },
            Some(Arg::Key(k)) => self.set(dst, i64::from(name_hash(k))),
            Some(Arg::SysArray(..)) | None => self.set(dst, 0),
        }
    }
    /// `$ALL_EVENTS` and `by_marks(...)` select several events; the engine
    /// addresses one source id, so such calls only warn.
    fn selects_many(&mut self, builtin: Builtin, args: &[Arg], i: usize) -> bool {
        let many = matches!(
            self.expr(args, i),
            Some(Expr {
                kind: ExprKind::Builtin(Builtin::ByMarks, _),
                ..
            })
        ) || self.const_int(args, i) == Some(b::ALL_EVENTS);
        if many {
            self.ignore(
                builtin,
                "on $ALL_EVENTS or by_marks is not supported; no effect",
            );
        }
        many
    }
    fn is_event_id(&self, args: &[Arg], i: usize) -> bool {
        self.note_context()
            && matches!(
                self.expr(args, i),
                Some(Expr {
                    kind: ExprKind::Sys(SysVar::EventId),
                    ..
                })
            )
    }
    /// UI index a constant id argument names.
    fn ui_index(&self, args: &[Arg], i: usize) -> Option<u32> {
        let id = match args.get(i)? {
            Arg::Var(v, _) => return self.var(*v).ui,
            _ => self.const_int(args, i)?,
        };
        let index = u32::try_from(id.checked_sub(b::FIRST_UI_ID)?).ok()?;
        ((index as usize) < self.u.hir.uis.len()).then_some(index)
    }

    /// Forward the call to the host: numeric arguments in order from `dst`,
    /// the first text argument as the effect text.
    fn effect(&mut self, builtin: Builtin, args: &[Arg], dst: u16) -> Result<()> {
        let mut count = 0u16;
        let mut text = None;
        for (i, a) in args.iter().enumerate() {
            let is_text = match a {
                Arg::Expr(e) => e.ty == Ty::Str,
                _ => false,
            };
            if is_text {
                if text.is_none() {
                    let scratch = self.scratch();
                    let free = reg(dst, sampler_core::EFFECT_ARGS as u16)?;
                    self.emit(I::Op(Op::TextClear { text: scratch }))?;
                    self.append(self.expr(args, i).unwrap(), scratch, free)?;
                    text = Some(scratch);
                }
                continue;
            }
            if usize::from(count) == sampler_core::EFFECT_ARGS {
                break;
            }
            self.arg(args, i, reg(dst, count)?)?;
            count += 1;
        }
        let service = self.u.service(builtin);
        self.emit(I::Op(Op::Emit {
            service,
            args: dst,
            count: count as u8,
            text,
        }))?;
        if text.is_some() {
            self.tdepth -= 1;
        }
        self.cover(builtin, Coverage::Effect);
        Ok(())
    }

    /// Keyed store access: key registers dst+1..=dst+4, value in dst.
    fn store(&mut self, args: &[Arg], key: [Key; 4], dst: u16, write: bool) -> Result<()> {
        for (i, k) in key.into_iter().enumerate() {
            let r = reg(dst, 1 + i as u16)?;
            match k {
                Key::Arg(a) => self.arg(args, a, r)?,
                Key::Fixed(v) => self.set(r, i64::from(v))?,
            }
        }
        self.emit(I::Op(Op::Store {
            key: reg(dst, 1)?,
            local: dst,
            write,
        }))
    }

    fn real_unary(&mut self, args: &[Arg], dst: u16, operation: RealUnary) -> Result<()> {
        self.arg(args, 0, dst)?;
        self.emit(I::Op(Op::RealUnary {
            local: dst,
            operation,
        }))
    }

    fn builtin(&mut self, builtin: Builtin, args: &[Arg], dst: u16, span: Span) -> Result<()> {
        use Builtin::*;
        self.span = span;
        let t = reg(dst, 1)?;
        let is_real = |i: usize| matches!(args.get(i), Some(Arg::Expr(e)) if e.ty == Ty::Real);
        let native = match builtin {
            Exit => {
                self.forward()?;
                self.emit(I::End)?;
                true
            }
            Continue => {
                match self.loops.last() {
                    Some(&start) => self.emit(I::Jump { target: start })?,
                    None => self.warn("continue outside while"),
                }
                true
            }
            Inc | Dec => {
                let Some(Arg::Place(place)) = args.first() else {
                    return fault(span, "inc/dec requires a variable");
                };
                let operation = if builtin == Inc {
                    IB::Add
                } else {
                    IB::Subtract
                };
                match place {
                    Place::Var(v) => {
                        self.load(*v, dst)?;
                        self.set(t, 1)?;
                        self.emit(I::Binary32 {
                            lhs: dst,
                            rhs: t,
                            operation,
                        })?;
                        self.write_var(*v, dst, true)?;
                    }
                    Place::Elem(v, index) => {
                        let Home::Cells { offset, len } = self.var(*v).home else {
                            return fault(span, "inc/dec requires an integer array");
                        };
                        let array = ScriptArray { offset, len };
                        let u = reg(dst, 2)?;
                        self.value(index, dst)?;
                        self.emit(I::ReadScriptArray {
                            array,
                            index: dst,
                            local: t,
                        })?;
                        self.set(u, 1)?;
                        self.emit(I::Binary32 {
                            lhs: t,
                            rhs: u,
                            operation,
                        })?;
                        self.emit(I::WriteScriptArray {
                            array,
                            index: dst,
                            local: t,
                        })?;
                    }
                }
                true
            }
            Abs => {
                self.arg(args, 0, dst)?;
                self.emit(if is_real(0) {
                    I::Op(Op::RealUnary {
                        local: dst,
                        operation: RealUnary::Absolute,
                    })
                } else {
                    I::Unary32 {
                        local: dst,
                        operation: IU::Absolute,
                    }
                })?;
                true
            }
            Min | Max => {
                self.arg(args, 0, dst)?;
                self.arg(args, 1, t)?;
                self.emit(if is_real(0) {
                    I::Op(Op::Real {
                        lhs: dst,
                        rhs: t,
                        operation: if builtin == Min {
                            RealBinary::Min
                        } else {
                            RealBinary::Max
                        },
                    })
                } else {
                    I::Op(Op::Integer {
                        lhs: dst,
                        rhs: t,
                        operation: if builtin == Min {
                            IntegerExtra::Min
                        } else {
                            IntegerExtra::Max
                        },
                    })
                })?;
                true
            }
            InRange => {
                let u = reg(dst, 2)?;
                self.arg(args, 0, dst)?;
                self.arg(args, 1, t)?;
                self.arg(args, 2, u)?;
                let compare = |lhs, rhs| {
                    if is_real(0) {
                        I::Op(Op::CompareReal {
                            lhs,
                            rhs,
                            comparison: Cmp::LessEqual,
                        })
                    } else {
                        I::CompareLocal {
                            lhs,
                            rhs,
                            comparison: Cmp::LessEqual,
                        }
                    }
                };
                self.emit(compare(t, dst))?;
                self.emit(compare(dst, u))?;
                self.emit(I::Binary32 {
                    lhs: dst,
                    rhs: t,
                    operation: IB::And,
                })?;
                true
            }
            Sgn | Signbit if !is_real(0) => {
                self.arg(args, 0, dst)?;
                self.emit(I::Unary32 {
                    local: dst,
                    operation: if builtin == Sgn {
                        IU::Sign
                    } else {
                        IU::SignBit
                    },
                })?;
                true
            }
            Signbit => {
                self.arg(args, 0, dst)?;
                self.set(t, real_bits(0.0))?;
                self.emit(I::Op(Op::CompareReal {
                    lhs: dst,
                    rhs: t,
                    comparison: Cmp::Less,
                }))?;
                true
            }
            Sgn => {
                // (x > 0) - (x < 0); the argument is evaluated twice.
                let u = reg(dst, 2)?;
                self.arg(args, 0, dst)?;
                self.arg(args, 0, u)?;
                self.set(t, real_bits(0.0))?;
                self.emit(I::Op(Op::CompareReal {
                    lhs: dst,
                    rhs: t,
                    comparison: Cmp::Greater,
                }))?;
                self.emit(I::Op(Op::CompareReal {
                    lhs: u,
                    rhs: t,
                    comparison: Cmp::Less,
                }))?;
                self.emit(I::Binary32 {
                    lhs: dst,
                    rhs: u,
                    operation: IB::Subtract,
                })?;
                true
            }
            ShLeft | ShRight => {
                self.arg(args, 0, dst)?;
                self.arg(args, 1, t)?;
                self.emit(I::Op(Op::Integer {
                    lhs: dst,
                    rhs: t,
                    operation: if builtin == ShLeft {
                        IntegerExtra::ShiftLeft
                    } else {
                        IntegerExtra::ShiftRight
                    },
                }))?;
                true
            }
            Random => {
                self.arg(args, 0, dst)?;
                self.arg(args, 1, t)?;
                self.emit(I::Op(Op::Random { lhs: dst, rhs: t }))?;
                true
            }
            IntToReal | Real => {
                self.arg(args, 0, dst)?;
                self.emit(I::Op(Op::IntegerToReal { local: dst }))?;
                true
            }
            RealToInt | Int => {
                self.arg(args, 0, dst)?;
                self.emit(I::Op(Op::RealToInteger { local: dst }))?;
                true
            }
            Round | Floor | Ceil | Sqrt | Cbrt | Exp | Exp2 | Log | Log2 | Log10 | Sin | Cos
            | Tan | Asin | Acos | Atan => {
                let operation = match builtin {
                    Round => RealUnary::Round,
                    Floor => RealUnary::Floor,
                    Ceil => RealUnary::Ceil,
                    Sqrt => RealUnary::Sqrt,
                    Cbrt => RealUnary::Cbrt,
                    Exp => RealUnary::Exp,
                    Exp2 => RealUnary::Exp2,
                    Log => RealUnary::Ln,
                    Log2 => RealUnary::Log2,
                    Log10 => RealUnary::Log10,
                    Sin => RealUnary::Sin,
                    Cos => RealUnary::Cos,
                    Tan => RealUnary::Tan,
                    Asin => RealUnary::Asin,
                    Acos => RealUnary::Acos,
                    _ => RealUnary::Atan,
                };
                self.real_unary(args, dst, operation)?;
                true
            }
            Pow => {
                self.arg(args, 0, dst)?;
                self.arg(args, 1, t)?;
                self.emit(I::Op(Op::Real {
                    lhs: dst,
                    rhs: t,
                    operation: RealBinary::Power,
                }))?;
                true
            }
            MsToTicks | TicksToMs => {
                // ponytail: fixed 120 BPM (960 ticks per quarter); host tempo if needed.
                let (mul, div) = if builtin == MsToTicks {
                    (48, 25)
                } else {
                    (25, 48)
                };
                self.arg(args, 0, dst)?;
                for (value, operation) in [(mul, IB::Multiply), (div, IB::Divide)] {
                    self.set(t, value)?;
                    self.emit(I::Binary32 {
                        lhs: dst,
                        rhs: t,
                        operation,
                    })?;
                }
                self.cover(builtin, Coverage::Approximate);
                return Ok(());
            }
            NumElements => {
                let len = match args.first() {
                    Some(Arg::Var(v, _)) => self.var(*v).len.unwrap_or(1),
                    Some(Arg::SysArray(a, _)) => a.len(),
                    _ => 0,
                };
                self.set(dst, i64::from(len))?;
                true
            }
            Search => return self.search(args, dst),
            ByMarks => {
                self.arg(args, 0, dst)?;
                self.set(t, i64::from(b::MARKS_FLAG))?;
                self.emit(I::Binary32 {
                    lhs: dst,
                    rhs: t,
                    operation: IB::Or,
                })?;
                true
            }
            PlayNote => return self.play(args, dst),
            NoteOff | IgnoreEvent | ChangeNote | ChangeVelo
                if self.selects_many(builtin, args, 0) =>
            {
                return Ok(());
            }
            NoteOff => {
                self.arg(args, 0, dst)?;
                let delay = if args.len() > 1 {
                    self.arg(args, 1, t)?;
                    self.emit(I::MicrosToFrames { local: t })?;
                    Some(t)
                } else {
                    None
                };
                self.emit(I::KeyUpEvent { event: dst, delay })?;
                true
            }
            IgnoreEvent if self.is_event_id(args, 0) => {
                self.emit(if self.ctx == Context::Note {
                    I::SuppressAttack
                } else {
                    I::SuppressRelease
                })?;
                true
            }
            IgnoreEvent => {
                // ponytail: another event is released rather than discarded.
                self.arg(args, 0, dst)?;
                self.emit(I::KeyUpEvent {
                    event: dst,
                    delay: None,
                })?;
                self.cover(builtin, Coverage::Approximate);
                return Ok(());
            }
            ChangeNote | ChangeVelo => {
                self.event_write(builtin == ChangeNote, args, 0, 1, dst)?;
                true
            }
            SetEventPar
                if matches!(
                    self.const_int(args, 1),
                    Some(b::event_par::NOTE | b::event_par::VELOCITY)
                ) =>
            {
                let note = self.const_int(args, 1) == Some(b::event_par::NOTE);
                self.event_write(note, args, 0, 2, dst)?;
                true
            }
            GetEventPar
                if self.is_event_id(args, 0)
                    && matches!(
                        self.const_int(args, 1),
                        Some(b::event_par::NOTE | b::event_par::VELOCITY)
                    ) =>
            {
                self.emit(if self.const_int(args, 1) == Some(b::event_par::NOTE) {
                    I::ReadKey { local: dst }
                } else {
                    I::ReadVelocity7 { local: dst }
                })?;
                true
            }
            AllowGroup | DisallowGroup if self.note_context() => {
                let allowed = builtin == AllowGroup;
                let pending_only = self.ctx == Context::Note;
                self.arg(args, 0, dst)?;
                self.set(t, i64::from(b::ALL_GROUPS))?;
                self.emit(I::CompareLocal {
                    lhs: t,
                    rhs: dst,
                    comparison: Cmp::Equal,
                })?;
                let one = self.jump_if_zero(t)?;
                self.emit(I::WriteGroup {
                    group: None,
                    allowed,
                    pending_only,
                })?;
                let end = self.jump()?;
                self.land(one);
                self.emit(I::WriteGroup {
                    group: Some(dst),
                    allowed,
                    pending_only,
                })?;
                self.land(end);
                true
            }
            IgnoreController if self.ctx == Context::Controller => {
                self.emit(I::SuppressController)?;
                true
            }
            SetController => {
                self.arg(args, 0, dst)?;
                self.arg(args, 1, t)?;
                self.emit(I::ControllerFromMidi7 { local: t })?;
                self.emit(I::WriteController {
                    controller: dst,
                    value: t,
                })?;
                true
            }
            Wait | WaitTicks => {
                self.arg(args, 0, dst)?;
                if builtin == WaitTicks {
                    // ponytail: fixed 120 BPM tick length (520 us).
                    self.set(t, 520)?;
                    self.emit(I::Binary32 {
                        lhs: dst,
                        rhs: t,
                        operation: IB::Multiply,
                    })?;
                }
                self.emit(I::MicrosToFrames { local: dst })?;
                self.forward()?;
                self.emit(I::WaitLocal { local: dst })?;
                if builtin == WaitTicks {
                    self.cover(builtin, Coverage::Approximate);
                    return Ok(());
                }
                true
            }
            FindGroup | GetGroupIdx => {
                let index = self.const_text(args, 0).and_then(|name| {
                    self.u
                        .groups
                        .iter()
                        .position(|g| g.eq_ignore_ascii_case(&name))
                });
                self.set(dst, index.map_or(-1, |i| i as i64))?;
                if index.is_none() {
                    self.warn(format!("{}: group not found; -1", builtin.name()));
                }
                true
            }
            FindMod | GetModIdx | FindTarget | GetTargetIdx => {
                let name = self.const_text(args, args.len() - 1);
                match name {
                    Some(name) => self.set(dst, i64::from(crate::eval::lookup_index(&name)))?,
                    None => {
                        self.ignore(builtin, "needs a constant name; -1");
                        return self.set(dst, -1);
                    }
                }
                self.cover(builtin, Coverage::Approximate);
                return Ok(());
            }
            GetEnginePar => {
                self.set(dst, 0)?;
                self.store(args, [0, 1, 2, 3].map(Key::Arg), dst, false)?;
                true
            }
            SetEnginePar => {
                // Mirror for get_engine_par, then hand the edit to the host.
                self.arg(args, 1, dst)?;
                self.store(args, [0, 2, 3, 4].map(Key::Arg), dst, true)?;
                self.effect(builtin, args, dst)?;
                return self.set(dst, 0);
            }
            GetUiId => {
                let id = self
                    .ui_index(args, 0)
                    .map_or(0, |ui| b::FIRST_UI_ID + ui as i32);
                self.set(dst, i64::from(id))?;
                true
            }
            SetControlPar | SetControlParReal | SetControlParArr | SetControlParRealArr => {
                return self.set_control_par(builtin, args, dst);
            }
            GetControlPar | GetControlParArr => return self.get_control_par(builtin, args, dst),
            PgsSetKeyVal => {
                self.arg(args, 2, dst)?;
                let hash = name_hash(&self.const_text(args, 0).unwrap_or_default());
                let key = [
                    Key::Fixed(PGS_TAG),
                    Key::Fixed(hash),
                    Key::Arg(1),
                    Key::Fixed(PGS_TAG),
                ];
                self.store(args, key, dst, true)?;
                self.effect(builtin, args, dst)?;
                return Ok(());
            }
            PgsGetKeyVal => {
                self.set(dst, 0)?;
                let hash = name_hash(&self.const_text(args, 0).unwrap_or_default());
                let key = [
                    Key::Fixed(PGS_TAG),
                    Key::Fixed(hash),
                    Key::Arg(1),
                    Key::Fixed(PGS_TAG),
                ];
                self.store(args, key, dst, false)?;
                true
            }
            PgsKeyExists => {
                let exists = self
                    .const_text(args, 0)
                    .is_some_and(|k| self.u.pgs.contains(&k));
                self.set(dst, i64::from(exists))?;
                true
            }
            WaitAsync | DisableLogging | WatchVar | WatchArrayIdx => {
                // Effects complete immediately; logging switches have no runtime state.
                true
            }
            // Instrument, presentation and logging services the engine does not own.
            ChangeVol
            | ChangeTune
            | ChangePan
            | FadeIn
            | FadeOut
            | SetEventPar
            | SetEventParArr
            | SetEventMark
            | DeleteEventMark
            | SetNoteController
            | SetRpn
            | SetNrpn
            | ResetRlsTrigCounter
            | WillNeverTerminate
            | RedirectOutput
            | StopWait
            | ResetKspTimer
            | SetListener
            | ChangeListenerPar
            | SetZonePar
            | PurgeGroup
            | SetVoiceLimit
            | LoadIrSample
            | AttachLevelMeter
            | SetControlParStr
            | SetControlParStrArr
            | SetText
            | AddTextLine
            | SetKnobLabel
            | SetKnobUnit
            | SetKnobDefval
            | SetControlHelp
            | MoveControl
            | MoveControlPx
            | HidePart
            | AddMenuItem
            | SetMenuItemStr
            | SetMenuItemVisibility
            | SetMenuItemValue
            | SetTableStepsShown
            | AttachZone
            | SetUiWfProperty
            | FsNavigate
            | SetNksNavName
            | SetNksNavPar
            | SetKeyColor
            | SetKeyName
            | SetKeyType
            | SetKeyPressed
            | SetKeyPressedSupport
            | SetKeyrange
            | RemoveKeyrange
            | Message
            | LoadArray
            | SaveArray
            | LoadArrayStr
            | SaveArrayStr
            | PgsSetStrKeyVal
            | PgsCreateKey
            | PgsCreateStrKey => {
                self.effect(builtin, args, dst)?;
                if builtin.sig().ret != b::Ret::Void {
                    self.set(dst, 0)?;
                }
                return Ok(());
            }
            _ => {
                let why = match builtin {
                    AllowGroup | DisallowGroup | IgnoreEvent | GetEventPar => {
                        "needs the originating note event; ignored"
                    }
                    IgnoreController => "outside on controller is ignored",
                    MakePersistent | MakeInstrPersistent | ReadPersistentVar | LoadNativeUi
                    | LoadPerformanceView | MakePerfview | ExposeControls | SetSnapshotType
                    | ShowLibraryTab | SetSkinOffset | SetUiColor | SetUiHeight | SetUiHeightPx
                    | SetUiWidthPx | SetScriptTitle | GetFontId => "only takes effect in on init",
                    _ => "is not executed at runtime; result 0",
                };
                self.ignore(builtin, why);
                if builtin.sig().ret != b::Ret::Void {
                    self.set(
                        dst,
                        if builtin.sig().ret == b::Ret::Real {
                            real_bits(0.0)
                        } else {
                            0
                        },
                    )?;
                }
                return Ok(());
            }
        };
        self.cover(
            builtin,
            if native {
                Coverage::Native
            } else {
                Coverage::Approximate
            },
        );
        Ok(())
    }

    fn event_write(
        &mut self,
        note: bool,
        args: &[Arg],
        id: usize,
        value: usize,
        dst: u16,
    ) -> Result<()> {
        let builtin = if note {
            Builtin::ChangeNote
        } else {
            Builtin::ChangeVelo
        };
        let event = if self.is_event_id(args, id) {
            None
        } else {
            self.arg(args, id, dst)?;
            Some(dst)
        };
        if event.is_none() && !self.note_context() {
            self.ignore(builtin, "needs the originating note event; ignored");
            return Ok(());
        }
        let local = reg(dst, u16::from(event.is_some()))?;
        self.arg(args, value, local)?;
        self.emit(if note {
            I::WriteEventKey { event, local }
        } else {
            I::WriteEventVelocity7 { event, local }
        })
    }

    fn search(&mut self, args: &[Arg], dst: u16) -> Result<()> {
        let Some(Arg::Var(v, _)) = args.first() else {
            self.ignore(
                Builtin::Search,
                "of a runtime-maintained array is not available; -1",
            );
            return self.set(dst, -1);
        };
        let Home::Cells { offset, len } = self.var(*v).home else {
            self.ignore(Builtin::Search, "of a text array is not available; -1");
            return self.set(dst, -1);
        };
        let array = ScriptArray { offset, len };
        let (value, end, t) = (reg(dst, 1)?, reg(dst, 2)?, reg(dst, 3)?);
        self.arg(args, 1, value)?;
        if args.len() > 2 {
            self.arg(args, 2, dst)?;
            self.arg(args, 3, end)?;
        } else {
            self.set(dst, 0)?;
            self.set(end, i64::from(len) - 1)?;
        }
        let start = self.here();
        self.set(t, 0)?;
        self.emit(I::Binary32 {
            lhs: t,
            rhs: dst,
            operation: IB::Add,
        })?;
        self.emit(I::CompareLocal {
            lhs: t,
            rhs: end,
            comparison: Cmp::LessEqual,
        })?;
        let missing = self.jump_if_zero(t)?;
        self.emit(I::ReadScriptArray {
            array,
            index: dst,
            local: t,
        })?;
        self.emit(I::CompareLocal {
            lhs: t,
            rhs: value,
            comparison: Cmp::Equal,
        })?;
        let next = self.jump_if_zero(t)?;
        let found = self.jump()?;
        self.land(next);
        self.emit(I::AddLocal {
            local: dst,
            value: 1,
        })?;
        self.emit(I::Jump { target: start })?;
        self.land(missing);
        self.set(dst, -1)?;
        self.land(found);
        self.cover(Builtin::Search, Coverage::Native);
        Ok(())
    }

    fn play(&mut self, args: &[Arg], dst: u16) -> Result<()> {
        let (velocity, offset) = (reg(dst, 1)?, reg(dst, 2)?);
        self.arg(args, 0, dst)?;
        self.arg(args, 1, velocity)?;
        let offset_micros = if self.const_int(args, 2) == Some(0) {
            None
        } else {
            self.arg(args, 2, offset)?;
            Some(offset)
        };
        let frames = if offset_micros.is_some() {
            reg(offset, 1)?
        } else {
            offset
        };
        let scratch = reg(frames, 1)?;
        let inheritance = Inheritance::Independent;
        let play = |duration| I::PlayMidi {
            key: dst,
            velocity,
            duration,
            offset_micros,
            inheritance,
            result: Some(dst),
        };
        self.cover(Builtin::PlayNote, Coverage::Native);
        match self.const_int(args, 3) {
            Some(0) => return self.emit(play(DurationValue::Fixed(Duration::UntilSilent))),
            Some(-1) if self.note_context() => {
                return self.emit(play(DurationValue::Fixed(Duration::Gate)));
            }
            Some(-1) => {
                self.warn("play_note duration -1 needs a note callback; faults at runtime");
                self.set(frames, -1)?;
                return self.emit(play(DurationValue::Frames(frames)));
            }
            Some(_) => {
                self.arg(args, 3, frames)?;
                self.emit(I::MicrosToFrames { local: frames })?;
                return self.emit(play(DurationValue::Frames(frames)));
            }
            None => {}
        }
        // Runtime-selected sentinels.
        self.arg(args, 3, frames)?;
        let mut exits = Vec::new();
        for (value, duration) in [(0, Duration::UntilSilent), (-1, Duration::Gate)] {
            self.set(scratch, value)?;
            self.emit(I::CompareLocal {
                lhs: scratch,
                rhs: frames,
                comparison: Cmp::Equal,
            })?;
            let miss = self.jump_if_zero(scratch)?;
            self.emit(play(if value == -1 && !self.note_context() {
                // A parent-gate sentinel has no meaning without a note; the
                // positive-frame check faults before publishing anything.
                DurationValue::Frames(frames)
            } else {
                DurationValue::Fixed(duration)
            }))?;
            exits.push(self.jump()?);
            self.land(miss);
        }
        self.emit(I::MicrosToFrames { local: frames })?;
        self.emit(play(DurationValue::Frames(frames)))?;
        for e in exits {
            self.land(e);
        }
        Ok(())
    }

    fn set_control_par(&mut self, builtin: Builtin, args: &[Arg], dst: u16) -> Result<()> {
        let par = self.const_int(args, 1);
        if par == Some(b::CONTROL_PAR_VALUE) && builtin != Builtin::SetControlParArr {
            if let Some(ui) = self.ui_index(args, 0) {
                let var = self.u.hir.uis[ui as usize].var;
                if self.var(var).len.is_none() && self.var(var).ty != Ty::Str {
                    self.arg(args, 2, dst)?;
                    self.write_var(var, dst, true)?;
                    self.cover(builtin, Coverage::Native);
                    return Ok(());
                }
            }
            // Dynamic id: write the control if the id names one, and the mirror.
            let (index, value) = (reg(dst, 5)?, reg(dst, 6)?);
            self.arg(args, 0, index)?;
            self.set(value, i64::from(b::FIRST_UI_ID))?;
            self.emit(I::Binary32 {
                lhs: index,
                rhs: value,
                operation: IB::Subtract,
            })?;
            self.arg(args, 2, value)?;
            self.emit(I::Op(Op::ControlAt {
                index,
                local: value,
                write: true,
            }))?;
        }
        // Property mirror (read back by get_control_par), then the host request.
        self.arg(args, 2, dst)?;
        self.store(args, PROPERTY_KEY, dst, true)?;
        self.effect(builtin, args, dst)
    }

    fn get_control_par(&mut self, builtin: Builtin, args: &[Arg], dst: u16) -> Result<()> {
        let par = self.const_int(args, 1);
        let ui = self.ui_index(args, 0);
        self.cover(builtin, Coverage::Native);
        if let Some(ui) = ui {
            let var = self.u.hir.uis[ui as usize].var;
            match par {
                Some(b::CONTROL_PAR_VALUE)
                    if self.var(var).len.is_none() && self.var(var).ty == Ty::Int =>
                {
                    return self.load(var, dst);
                }
                Some(b::CONTROL_PAR_TYPE) => {
                    let kind = self.u.hir.uis[ui as usize].kind.control_type();
                    return self.set(dst, i64::from(kind));
                }
                _ => {}
            }
        }
        self.set(dst, 0)?;
        self.store(args, PROPERTY_KEY, dst, false)?;
        if par == Some(b::CONTROL_PAR_VALUE) || par.is_none() {
            // A control's value lives in the control, not the mirror.
            let index = reg(dst, 1)?;
            let t = reg(dst, 2)?;
            self.arg(args, 0, index)?;
            self.set(t, i64::from(b::FIRST_UI_ID))?;
            self.emit(I::Binary32 {
                lhs: index,
                rhs: t,
                operation: IB::Subtract,
            })?;
            if par.is_none() {
                // Only VALUE reads consult the control table.
                self.arg(args, 1, t)?;
                self.set(reg(dst, 3)?, i64::from(b::CONTROL_PAR_VALUE))?;
                self.emit(I::CompareLocal {
                    lhs: t,
                    rhs: reg(dst, 3)?,
                    comparison: Cmp::Equal,
                })?;
                let skip = self.jump_if_zero(t)?;
                self.emit(I::Op(Op::ControlAt {
                    index,
                    local: dst,
                    write: false,
                }))?;
                self.land(skip);
            } else {
                self.emit(I::Op(Op::ControlAt {
                    index,
                    local: dst,
                    write: false,
                }))?;
            }
        }
        Ok(())
    }
}
