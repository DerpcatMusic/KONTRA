//! HIR -> sampler-core programs. One program per callback; functions a callback
//! reaches are appended to its program as subroutines lowered in the caller's
//! event context. Expressions evaluate into a destination register and use the
//! registers above it as temporaries, keeping callbacks within small budgets.
use crate::builtins::{self as b, Builtin, SysArray, SysVar};
use crate::diag::{Fault, Result, Span, fault};
use crate::hir::*;
use crate::sema::fold;
use sampler_core::{
    Comparison as Cmp, ControlId, Duration, DurationValue, EnvelopeStage, Inheritance,
    Instruction as I, IntegerBinary as IB, IntegerExtra, IntegerUnary as IU, ModTarget, Op,
    ParamScope, Program, RealBinary, RealUnary, ScriptArray, SlotKind, TextPart, TextRef,
    WaitLifetime, real_bits,
};
use std::collections::{BTreeMap, HashMap};

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
    /// Queued to the host through the effect outbox for an engine change
    /// the runtime does not apply itself.
    Effect,
    /// Queued to the host through the effect outbox for a service the host
    /// owns: interface properties, keyboard, text, messages, files, PGS.
    Host,
    /// Not executed at runtime; a warning was recorded.
    Ignored,
}

/// Kontakt's default name for a group's first AHDSR, its volume envelope.
// ponytail: a library that renames its volume envelope, or uses ENV_AHDSR
// for another target, is mis-addressed; take the name from the instrument
// when the loader exposes group modulators.
const AMP_ENVELOPE: &str = "ENV_AHDSR";

/// Store key tags separating UI properties and PGS values from engine keys.
pub const PROPERTY_TAG: i32 = i32::MIN;
pub const PGS_TAG: i32 = i32::MIN + 1;
/// `Instruction::Signal` of a PGS write; `on pgs_changed` is bound to it.
pub const PGS_SIGNAL: u16 = 0;

/// The shared-store key of PGS key `args[0]` at `index`.
fn pgs_key(g: &Gen, args: &[Arg], index: Key) -> [Key; 4] {
    let hash = name_hash(&g.const_text(args, 0).unwrap_or_default());
    [
        Key::Fixed(PGS_TAG),
        Key::Fixed(hash),
        index,
        Key::Fixed(PGS_TAG),
    ]
}

/// `[LISTENER_TAG, signal, 0, LISTENER_TAG]`: a listener's `set_listener` value.
pub const LISTENER_TAG: i32 = i32::MIN + 2;

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
    pub slot: u8,
    /// Remaining instruction budget for the whole script.
    pub budget: usize,
    /// The whole script's instruction limit.
    pub limit: usize,
    pub services: Vec<Builtin>,
    pub coverage: BTreeMap<(&'static str, Coverage), usize>,
    pub warnings: Vec<(Fault, crate::diag::Kind)>,
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
        signal: Option<i32>,
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
            group_table: None,
            calls: Vec::new(),
            starts: HashMap::new(),
            loops: Vec::new(),
            span,
            tdepth: 0,
            signal,
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
                builtin: None,
                message: format!("invalid lowered program: {e:?}"),
            })
    }

    /// The group volume, pan and tune writes `on init` left, as one program
    /// run at plan start so the runtime layers begin where Kontakt's engine
    /// does: `(target, group, engine value)`, group negative for the instrument.
    pub fn engine_start(
        &mut self,
        writes: &[([i32; 4], i32)],
        purges: &[(i32, i32)],
    ) -> Result<Program> {
        let mut code = Vec::new();
        for &(address, value) in writes {
            for (i, v) in address.into_iter().enumerate() {
                code.push(I::SetLocal {
                    local: i as u16,
                    value: i64::from(v),
                });
            }
            code.push(I::SetLocal {
                local: 4,
                value: i64::from(value),
            });
            code.push(I::Op(Op::EngineParameter {
                address: 0,
                local: 4,
                write: true,
            }));
        }
        for &(group, value) in purges {
            code.push(I::SetLocal {
                local: 0,
                value: i64::from(group),
            });
            code.push(I::SetLocal {
                local: 1,
                value: i64::from(value),
            });
            code.push(I::Op(Op::Purge {
                group: 0,
                local: 1,
                write: true,
            }));
        }
        Program::new(code)
            .map(|p| p.with_wait_lifetime(WaitLifetime::Callback))
            .map_err(|e| Fault {
                span: Span::default(),
                builtin: None,
                message: format!("invalid engine start: {e:?}"),
            })
    }

    /// A timer listener's driver: every period, start `body` (the listener
    /// program for `signal`); a zero period polls every 10 ms until set.
    // ponytail: $NI_SIGNAL_TIMER_BEAT assumes 120 BPM, like wait_ticks.
    pub fn listener_driver(&mut self, signal: i32, body: usize, span: Span) -> Result<Program> {
        let mut g = Gen {
            u: self,
            ctx: Context::Plan,
            callback_type: b::cb::LISTENER,
            ui_id: None,
            code: Vec::new(),
            texts: Vec::new(),
            group_table: None,
            calls: Vec::new(),
            starts: HashMap::new(),
            loops: Vec::new(),
            span,
            tdepth: 0,
            signal: Some(signal),
        };
        let (period, t) = (0, 1);
        let top = g.here();
        for (i, v) in [LISTENER_TAG, signal, 0, LISTENER_TAG]
            .into_iter()
            .enumerate()
        {
            g.set(2 + i as u16, i64::from(v))?;
        }
        g.emit(I::Op(Op::Store {
            key: 2,
            local: period,
            write: false,
        }))?;
        g.clamp(period, 0, i32::MAX)?;
        let idle = g.jump_if_zero(period)?;
        let period = if signal == b::signal::TIMER_BEAT {
            // Signals per quarter note to microseconds.
            g.set(t, 500_000)?;
            g.emit(I::Binary32 {
                lhs: t,
                rhs: period,
                operation: IB::Divide,
            })?;
            t
        } else {
            period
        };
        g.emit(I::MicrosToFrames { local: period })?;
        g.emit(I::WaitLocal { local: period })?;
        g.emit(I::StartProgram {
            program: body as u32,
        })?;
        g.emit(I::Jump { target: top })?;
        g.land(idle);
        g.emit(I::Wait(480))?;
        g.emit(I::Jump { target: top })?;
        Program::new(g.code)
            .map(|p| p.with_wait_lifetime(WaitLifetime::Callback))
            .map_err(|e| Fault {
                span,
                builtin: None,
                message: format!("invalid listener driver: {e:?}"),
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
    /// Base of the group names, appended once as consecutive text constants.
    group_table: Option<u16>,
    calls: Vec<(usize, FnId)>,
    starts: HashMap<FnId, usize>,
    /// Condition position of each enclosing `while`, for `continue`.
    loops: Vec<usize>,
    span: Span,
    tdepth: u32,
    /// The timer signal a listener body program serves.
    signal: Option<i32>,
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
        builtin: None,
        message: "register range exceeded".into(),
    })
}

impl Gen<'_, '_> {
    fn emit(&mut self, op: I) -> Result<()> {
        if self.u.budget == 0 {
            return fault(
                self.span,
                format!("instruction budget exceeded: more than {}", self.u.limit),
            );
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
        self.report(None, crate::diag::Kind::Warning, message.into());
    }
    fn report(&mut self, builtin: Option<Builtin>, kind: crate::diag::Kind, message: String) {
        if self.u.warnings.len() < 1000 {
            let fault = Fault {
                span: self.span,
                builtin: builtin.map(Builtin::name),
                message,
            };
            self.u.warnings.push((fault, kind));
        }
    }
    fn cover(&mut self, builtin: Builtin, coverage: Coverage) {
        let count = self
            .u
            .coverage
            .entry((builtin.name(), coverage))
            .or_default();
        *count += 1;
        // The first approximated call site per builtin is enough for the report;
        // `Script::coverage` keeps the totals.
        if *count == 1 && coverage == Coverage::Approximate {
            let message = format!("{} runs with simplified semantics", builtin.name());
            self.report(Some(builtin), crate::diag::Kind::Approximate, message);
        }
    }
    fn ignore(&mut self, builtin: Builtin, why: &str) {
        self.cover(builtin, Coverage::Ignored);
        let message = format!("{} {why}", builtin.name());
        self.report(Some(builtin), crate::diag::Kind::Unsupported, message);
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
    /// (base, count) of the instrument's group names, in group order, for
    /// runtime `group_name` and `find_group`.
    fn group_table(&mut self) -> (u16, u16) {
        let count = self.u.groups.len() as u16;
        if let Some(base) = self.group_table {
            return (base, count);
        }
        let base = self.texts.len() as u16;
        for name in self.u.groups.iter() {
            let mut end = name.len().min(sampler_core::TEXT_CAPACITY);
            while !name.is_char_boundary(end) {
                end -= 1;
            }
            self.texts.push(name[..end].to_owned());
        }
        self.group_table = Some(base);
        (base, count)
    }
    /// `arg` built into a scratch text; the caller releases it with `tdepth -= 1`.
    fn text_arg(&mut self, args: &[Arg], i: usize, free: u16) -> Result<Option<TextRef>> {
        let Some(Arg::Expr(e)) = args.get(i) else {
            return Ok(None);
        };
        if e.ty != Ty::Str {
            return Ok(None);
        }
        let scratch = self.scratch();
        self.emit(I::Op(Op::TextClear { text: scratch }))?;
        self.append(e, scratch, free)?;
        Ok(Some(scratch))
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
                    self.clamp(local, lo, hi)?;
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
            SysVar::WidgetInteraction(field) => {
                I::Op(Op::ReadWidgetInteraction { local: dst, field })
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
            SysVar::SignalType if self.signal.is_some() => I::SetLocal {
                local: dst,
                value: i64::from(self.signal.unwrap()),
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
        if !self.sys_readable(array) {
            self.warn(format!("{array:?} is not maintained at runtime; reads 0"));
            return self.set(dst, 0);
        }
        self.value(index, dst)?;
        self.sys_read(array, dst)
    }

    fn sys_readable(&self, array: SysArray) -> bool {
        match array {
            SysArray::Cc | SysArray::KeyDown => true,
            SysArray::EventPar => self.ui_id.is_some(),
            SysArray::CcTouched => self.ctx == Context::Controller,
            _ => false,
        }
    }

    /// Replace the index in `reg` by the element; uses `reg + 1` as scratch.
    fn sys_read(&mut self, array: SysArray, at: u16) -> Result<()> {
        match array {
            SysArray::Cc => {
                self.emit(I::ReadInputController {
                    controller: at,
                    local: at,
                })?;
                self.emit(I::ControllerToMidi7 { local: at })
            }
            SysArray::KeyDown => self.emit(I::ReadKeyHeld { local: at }),
            SysArray::EventPar => self.emit(I::Op(Op::ReadWidgetEventParameter { local: at })),
            // Kontakt marks the controllers that changed for this callback:
            // here, the one that triggered it.
            SysArray::CcTouched => {
                let number = reg(at, 1)?;
                self.emit(I::ReadControllerNumber { local: number })?;
                self.emit(I::CompareLocal {
                    lhs: at,
                    rhs: number,
                    comparison: Cmp::Equal,
                })
            }
            _ => self.set(at, 0),
        }
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
                match (name, self.expr(args, 0)) {
                    (Some(name), _) => TextPart::Constant(self.constant(&name)),
                    (None, Some(index)) if self.const_int(args, 0).is_none() => {
                        self.value(index, free)?;
                        let (base, count) = self.group_table();
                        TextPart::Table {
                            base,
                            count,
                            index: free,
                        }
                    }
                    _ => {
                        self.warn("group_name of an unknown group is empty");
                        return Ok(());
                    }
                }
            }
            ExprKind::Builtin(Builtin::GetControlParStr, args) => {
                self.property_key(args, free, None)?;
                self.emit(I::Op(Op::TextProperty {
                    key: free,
                    text: dst,
                    write: false,
                }))?;
                self.cover(Builtin::GetControlParStr, Coverage::Native);
                return Ok(());
            }
            ExprKind::Builtin(
                builtin @ (Builtin::GetEngineParDisp | Builtin::GetEngineParDispExt),
                args,
            ) => {
                self.engine_address(args, free, false)?;
                let value = if *builtin == Builtin::GetEngineParDispExt {
                    self.arg(args, 4, free + 4)?;
                    Some(free + 4)
                } else {
                    None
                };
                self.emit(I::Op(Op::EngineDisplay {
                    address: free,
                    value,
                    text: dst,
                }))?;
                self.cover(*builtin, Coverage::Native);
                return Ok(());
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
        self.emit_effect(builtin, args, dst)?;
        self.cover(builtin, host_service(builtin));
        Ok(())
    }

    fn emit_effect(&mut self, builtin: Builtin, args: &[Arg], dst: u16) -> Result<()> {
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
            if i == 0 && matches!(builtin, Builtin::SetText | Builtin::AddTextLine | Builtin::SetKnobLabel | Builtin::SetKnobUnit | Builtin::SetKnobDefval | Builtin::SetControlHelp | Builtin::MoveControl | Builtin::MoveControlPx | Builtin::HidePart | Builtin::AddMenuItem | Builtin::SetTableStepsShown) {
                if let Some(ui) = self.ui_index(args, 0) { self.set(reg(dst, count)?, i64::from(crate::builtins::FIRST_UI_ID + ui as i32))?; }
                else { self.arg(args, i, reg(dst, count)?)?; }
            } else { self.arg(args, i, reg(dst, count)?)?; }
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
        Ok(())
    }

    /// Keyed store access: key registers dst+1..=dst+4, value in dst.
    fn store(&mut self, args: &[Arg], key: [Key; 4], dst: u16, write: bool) -> Result<()> {
        self.store_in(args, key, dst, write, false)
    }

    /// `store`, on the plan's shared store with `shared` (PGS).
    fn store_in(
        &mut self,
        args: &[Arg],
        key: [Key; 4],
        dst: u16,
        write: bool,
        shared: bool,
    ) -> Result<()> {
        for (i, k) in key.into_iter().enumerate() {
            let r = reg(dst, 1 + i as u16)?;
            match k {
                Key::Arg(a) => self.arg(args, a, r)?,
                Key::Fixed(v) => self.set(r, i64::from(v))?,
            }
        }
        let key = reg(dst, 1)?;
        self.emit(I::Op(if shared {
            Op::SharedStore {
                key,
                local: dst,
                write,
            }
        } else {
            Op::Store {
                key,
                local: dst,
                write,
            }
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
                self.arg(args, 0, dst)?;
                self.emit(I::Op(Op::TimeConversion {
                    local: dst,
                    ticks_to_micros: builtin == TicksToMs,
                }))?;
                true
            }
            NumElements => {
                let len = match args.first() {
                    Some(Arg::Var(v, _)) => self.var(*v).len.unwrap_or(1),
                    Some(Arg::SysArray(a)) => a.len(),
                    _ => 0,
                };
                self.set(dst, i64::from(len))?;
                true
            }
            Search => return self.search(args, dst),
            Sort => return self.sort(args, dst),
            ArrayEqual => return self.array_equal(args, dst),
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
            GetEventPar
                if self.const_int(args, 1) == Some(b::event_par::SOURCE)
                    && !self.selects_many(builtin, args, 0) =>
            {
                // -1 for a host event, else the creating script's slot (ponytail:
                // the reading script's own slot; notes carry no creator slot).
                self.arg(args, 0, dst)?;
                self.emit(I::ReadEventInfo {
                    event: dst,
                    info: sampler_core::EventInfo::Source,
                    local: dst,
                })?;
                let host = self.jump_if_zero(dst)?;
                self.set(dst, i64::from(self.u.slot))?;
                let end = self.jump()?;
                self.land(host);
                self.set(dst, -1)?;
                self.land(end);
                true
            }
            GetEventPar
                if matches!(
                    self.const_int(args, 1),
                    Some(
                        b::event_par::ZONE_ID
                            | b::event_par::MIDI_CHANNEL
                            | b::event_par::NOTE
                            | b::event_par::VELOCITY
                    )
                ) && !self.selects_many(builtin, args, 0) =>
            {
                let info = match self.const_int(args, 1) {
                    Some(b::event_par::ZONE_ID) => sampler_core::EventInfo::ZoneId,
                    Some(b::event_par::NOTE) => sampler_core::EventInfo::Key,
                    Some(b::event_par::VELOCITY) => sampler_core::EventInfo::Velocity,
                    _ => sampler_core::EventInfo::MidiChannel,
                };
                self.arg(args, 0, dst)?;
                self.emit(I::ReadEventInfo {
                    event: dst,
                    info,
                    local: dst,
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
                self.forward()?;
                self.emit(I::WaitMicros { local: dst })?;
                if builtin == WaitTicks {
                    self.cover(builtin, Coverage::Approximate);
                    return Ok(());
                }
                true
            }
            FindGroup | GetGroupIdx if self.const_text(args, 0).is_none() => {
                let free = reg(dst, 1)?;
                let Some(text) = self.text_arg(args, 0, free)? else {
                    self.ignore(builtin, "needs a text name; -1");
                    return self.set(dst, -1);
                };
                let (base, count) = self.group_table();
                self.emit(I::Op(Op::TextFind {
                    text,
                    base,
                    count,
                    local: dst,
                }))?;
                self.tdepth -= 1;
                self.cover(builtin, Coverage::Native);
                return Ok(());
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
                let (group, owner) = (dst + 1, dst + 2);
                self.arg(args, 0, group)?;
                let target = matches!(builtin, FindTarget | GetTargetIdx);
                if target {
                    self.arg(args, 1, owner)?;
                } else {
                    self.set(owner, -1)?;
                }
                let Some(text) = self.text_arg(args, args.len() - 1, dst + 3)? else {
                    return self.set(dst, -1);
                };
                self.emit(I::Op(Op::EngineLookup {
                    group,
                    owner,
                    target,
                    text,
                    local: dst,
                }))?;
                self.tdepth -= 1;
                true
            }
            ChangeVol | ChangeTune | ChangePan if !self.selects_many(builtin, args, 0) => {
                let target = match builtin {
                    ChangeVol => ModTarget::Decibels,
                    ChangeTune => ModTarget::Pitch,
                    _ => ModTarget::Pan,
                };
                self.arg(args, 0, dst)?;
                self.arg(args, 1, t)?;
                self.write_param(ParamScope::Note, target, dst, args, Some(2))?;
                true
            }
            // Group selection of a note this callback just played.
            SetEventParArr
                if self.const_int(args, 1) == Some(b::event_par::ALLOW_GROUP)
                    && !self.selects_many(builtin, args, 0) =>
            {
                self.arg(args, 0, dst)?;
                self.arg(args, 2, t)?;
                let group = reg(dst, 2)?;
                self.arg(args, 3, group)?;
                let all = reg(dst, 3)?;
                self.set(all, i64::from(b::ALL_GROUPS))?;
                self.emit(I::CompareLocal {
                    lhs: all,
                    rhs: group,
                    comparison: Cmp::Equal,
                })?;
                let one = self.jump_if_zero(all)?;
                self.emit(I::WriteEventGroup {
                    event: dst,
                    group: None,
                    allowed: t,
                })?;
                let end = self.jump()?;
                self.land(one);
                self.emit(I::WriteEventGroup {
                    event: dst,
                    group: Some(group),
                    allowed: t,
                })?;
                self.land(end);
                true
            }
            // "From script" modulator values (Kontakt 6.6+), per source event.
            SetEventParArr | GetEventParArr
                if self.const_int(args, 1) == Some(b::event_par::MOD_VALUE_ID)
                    && !self.selects_many(builtin, args, 0) =>
            {
                let id = reg(dst, 2)?;
                self.arg(args, 0, dst)?;
                if builtin == SetEventParArr {
                    self.arg(args, 2, t)?;
                    self.arg(args, 3, id)?;
                    self.emit(I::WriteModValue {
                        event: dst,
                        id,
                        local: t,
                    })?;
                } else {
                    self.arg(args, 2, id)?;
                    self.emit(I::ReadModValue {
                        event: dst,
                        id,
                        local: dst,
                    })?;
                }
                true
            }
            // The four user parameters ($EVENT_PAR_0..3) a script keeps on an
            // event, e.g. a note's tag that its release callback reads back.
            // They share the note's modulator-value store under ids above the
            // modulator range (sampler_core::USER_EVENT_PAR).
            SetEventPar | GetEventPar
                if matches!(self.const_int(args, 1), Some(0..=3))
                    && !self.selects_many(builtin, args, 0) =>
            {
                let id = reg(dst, 2)?;
                self.set(
                    id,
                    i64::from(sampler_core::USER_EVENT_PAR)
                        + i64::from(self.const_int(args, 1).unwrap()),
                )?;
                self.arg(args, 0, dst)?;
                if builtin == SetEventPar {
                    self.arg(args, 2, t)?;
                    self.emit(I::WriteModValue {
                        event: dst,
                        id,
                        local: t,
                    })?;
                } else {
                    self.emit(I::ReadModValue {
                        event: dst,
                        id,
                        local: dst,
                    })?;
                }
                true
            }
            SetEventPar | GetEventPar
                if self.event_param(args).is_some() && !self.selects_many(builtin, args, 0) =>
            {
                let target = self.event_param(args).unwrap();
                self.arg(args, 0, dst)?;
                if builtin == SetEventPar {
                    self.arg(args, 2, t)?;
                    self.write_param(ParamScope::Note, target, dst, args, None)?;
                } else {
                    self.emit(I::ReadParam {
                        scope: ParamScope::Note,
                        index: dst,
                        target,
                        local: dst,
                    })?;
                }
                true
            }
            FadeIn | FadeOut if !self.selects_many(builtin, args, 0) => {
                self.arg(args, 0, dst)?;
                self.arg(args, 1, t)?;
                self.emit(I::MicrosToFrames { local: t })?;
                let fade = |stop| I::FadeEvent {
                    event: dst,
                    frames: t,
                    out: builtin == FadeOut,
                    stop,
                };
                match (builtin, self.const_int(args, 2)) {
                    (FadeIn, _) => self.emit(fade(false))?,
                    (_, Some(stop)) => self.emit(fade(stop != 0))?,
                    _ => {
                        let flag = reg(dst, 2)?;
                        self.arg(args, 2, flag)?;
                        let keep = self.jump_if_zero(flag)?;
                        self.emit(fade(true))?;
                        let end = self.jump()?;
                        self.land(keep);
                        self.emit(fade(false))?;
                        self.land(end);
                    }
                }
                true
            }
            PurgeGroup | GetPurgeState => {
                self.arg(args, 0, t)?;
                if builtin == PurgeGroup {
                    self.arg(args, 1, dst)?;
                }
                self.emit(I::Op(Op::Purge {
                    group: t,
                    local: dst,
                    write: builtin == PurgeGroup,
                }))?;
                true
            }
            SetEnginePar | GetEnginePar => {
                self.engine_address(args, dst + 1, builtin == SetEnginePar)?;
                if builtin == SetEnginePar {
                    self.arg(args, 1, dst)?;
                }
                self.emit(I::Op(Op::EngineParameter {
                    address: dst + 1,
                    local: dst,
                    write: builtin == SetEnginePar,
                }))?;
                if builtin == SetEnginePar {
                    self.set(dst, 0)?;
                }
                true
            }
            SetListener | ChangeListenerPar => {
                // The timer driver reads the period from the store.
                self.arg(args, 1, dst)?;
                let key = [
                    Key::Fixed(LISTENER_TAG),
                    Key::Arg(0),
                    Key::Fixed(0),
                    Key::Fixed(LISTENER_TAG),
                ];
                self.store(args, key, dst, true)?;
                true
            }
            GetUiId => {
                let id = self
                    .ui_index(args, 0)
                    .map_or(0, |ui| b::FIRST_UI_ID + ui as i32);
                self.set(dst, i64::from(id))?;
                true
            }
            SetControlParStr | SetControlParStrArr => {
                self.property_key(
                    args,
                    dst,
                    if builtin == SetControlParStrArr {
                        Some(3)
                    } else {
                        None
                    },
                )?;
                if let Some(text) = self.text_arg(args, 2, dst + 4)? {
                    self.emit(I::Op(Op::TextProperty {
                        key: dst,
                        text,
                        write: true,
                    }))?;
                    self.tdepth -= 1;
                }
                return self.effect(builtin, args, dst);
            }
            SetControlPar | SetControlParReal | SetControlParArr | SetControlParRealArr => {
                return self.set_control_par(builtin, args, dst);
            }
            GetControlPar | GetControlParArr => return self.get_control_par(builtin, args, dst),
            PgsSetKeyVal => {
                // Shared by every script slot; on pgs_changed runs in each.
                self.arg(args, 2, dst)?;
                self.store_in(args, pgs_key(self, args, Key::Arg(1)), dst, true, true)?;
                if self.callback_type != b::cb::PGS_CHANGED {
                    // ponytail: a pgs_changed that sets keys does not re-signal,
                    // which would recurse synchronously.
                    self.emit(I::Signal { signal: PGS_SIGNAL })?;
                }
                self.cover(builtin, Coverage::Native);
                return self.set(dst, 0);
            }
            PgsGetKeyVal => {
                self.set(dst, 0)?;
                self.store_in(args, pgs_key(self, args, Key::Arg(1)), dst, false, true)?;
                true
            }
            PgsKeyExists => {
                // A created key has a value at index 0.
                self.set(dst, i64::from(i32::MIN))?;
                self.store_in(args, pgs_key(self, args, Key::Fixed(0)), dst, false, true)?;
                self.set(t, i64::from(i32::MIN))?;
                self.emit(I::CompareLocal {
                    lhs: dst,
                    rhs: t,
                    comparison: Cmp::NotEqual,
                })?;
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
            | SetZonePar
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
            | SetSkinOffset
            | SetUiColor
            | AttachZone
            | SetUiWfProperty
            | FsNavigate
            | SetNksNavName
            | SetNksNavPar
            | ResetNksNav
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
                    | ShowLibraryTab | SetUiHeight | SetUiHeightPx
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

    /// Clamp register `local` into `lo..=hi`, using `local + 1` as scratch.
    fn clamp(&mut self, local: u16, lo: i32, hi: i32) -> Result<()> {
        let t = reg(local, 1)?;
        for (bound, op) in [(lo, IntegerExtra::Max), (hi, IntegerExtra::Min)] {
            self.set(t, i64::from(bound))?;
            self.emit(I::Op(Op::Integer {
                lhs: local,
                rhs: t,
                operation: op,
            }))?;
        }
        Ok(())
    }

    /// `value` in `index + 1`; `relative` is a constant or runtime flag argument.
    fn write_param(
        &mut self,
        scope: ParamScope,
        target: ModTarget,
        index: u16,
        args: &[Arg],
        relative: Option<usize>,
    ) -> Result<()> {
        let local = reg(index, 1)?;
        let write = |relative| I::WriteParam {
            scope,
            index,
            target,
            local,
            relative,
        };
        let Some(arg) = relative else {
            return self.emit(write(false));
        };
        if let Some(flag) = self.const_int(args, arg) {
            return self.emit(write(flag != 0));
        }
        let flag = reg(index, 2)?;
        self.arg(args, arg, flag)?;
        let absolute = self.jump_if_zero(flag)?;
        self.emit(write(true))?;
        let end = self.jump()?;
        self.land(absolute);
        self.emit(write(false))?;
        self.land(end);
        Ok(())
    }

    /// `$EVENT_PAR_VOLUME/TUNE/PAN` as a script layer target.
    fn event_param(&self, args: &[Arg]) -> Option<ModTarget> {
        match self.const_int(args, 1)? {
            b::event_par::VOLUME => Some(ModTarget::Decibels),
            b::event_par::TUNE => Some(ModTarget::Pitch),
            b::event_par::PAN => Some(ModTarget::Pan),
            _ => None,
        }
    }

    /// Group/instrument volume, pan and tune: `set_engine_par(p, v, group, -1, -1)`
    /// and `get_engine_par(p, group, -1, -1)`; `slot` is the slot argument.
    fn engine_param(&self, args: &[Arg], slot: usize) -> Option<ModTarget> {
        if self.const_int(args, slot) != Some(-1) || self.const_int(args, slot + 1) != Some(-1) {
            return None;
        }
        let name = crate::eval::symbol_name(self.u.hir, self.const_int(args, 0)?)?;
        match name.trim_start_matches('$') {
            "ENGINE_PAR_VOLUME" => Some(ModTarget::Decibels),
            "ENGINE_PAR_PAN" => Some(ModTarget::Pan),
            "ENGINE_PAR_TUNE" => Some(ModTarget::Pitch),
            _ => None,
        }
    }

    /// The `ENGINE_PAR_*` name of the first argument, if it is one.
    fn engine_par_name(&self, args: &[Arg]) -> Option<String> {
        let name = crate::eval::symbol_name(self.u.hir, self.const_int(args, 0)?)?;
        Some(name.trim_start_matches('$').to_string())
    }

    /// AHDSR times a script sets on a modulator (`generic` -1).
    fn envelope_param(&self, args: &[Arg]) -> Option<EnvelopeStage> {
        if self.const_int(args, 4) != Some(-1) {
            return None;
        }
        let name = crate::eval::symbol_name(self.u.hir, self.const_int(args, 0)?)?;
        match name.trim_start_matches('$') {
            "ENGINE_PAR_ATTACK" => Some(EnvelopeStage::Attack),
            "ENGINE_PAR_DECAY" => Some(EnvelopeStage::Decay),
            "ENGINE_PAR_RELEASE" => Some(EnvelopeStage::Release),
            "ENGINE_PAR_SUSTAIN" => Some(EnvelopeStage::Sustain),
            "ENGINE_PAR_ATK_CURVE" => Some(EnvelopeStage::AttackCurve),
            _ => None,
        }
    }

    /// Effect slot parameters a script sets at runtime.
    fn slot_param(&self, args: &[Arg]) -> Option<SlotKind> {
        let name = crate::eval::symbol_name(self.u.hir, self.const_int(args, 0)?)?;
        match name.trim_start_matches('$') {
            "ENGINE_PAR_EFFECT_BYPASS" | "ENGINE_PAR_SEND_EFFECT_BYPASS" => Some(SlotKind::Bypass),
            "ENGINE_PAR_INSERT_EFFECT_OUTPUT_GAIN" | "ENGINE_PAR_SEND_EFFECT_OUTPUT_GAIN" => {
                Some(SlotKind::Output)
            }
            "ENGINE_PAR_SEND_EFFECT_DRY_LEVEL" => Some(SlotKind::Dry),
            _ => None,
        }
    }

    /// Volume engine units (0..=1000000) to linear gain real bits in place,
    /// through the decibel law group volume uses.
    fn volume_gain(&mut self, local: u16) -> Result<()> {
        self.engine_units(ModTarget::Decibels, local)?;
        let t = reg(local, 1)?;
        self.emit(I::Op(Op::IntegerToReal { local }))?;
        self.set(t, real_bits(std::f64::consts::LN_10 / 20_000.0))?;
        self.emit(I::Op(Op::Real {
            lhs: local,
            rhs: t,
            operation: RealBinary::Multiply,
        }))?;
        self.emit(I::Op(Op::RealUnary {
            local,
            operation: RealUnary::Exp,
        }))
    }

    /// Effect level engine units (0..=1000000) to linear gain real bits in
    /// place: (v / 396851)^3, the law `sampler-kontakt` fitted to stored slots.
    fn effect_gain(&mut self, local: u16) -> Result<()> {
        self.clamp(local, 0, 1_000_000)?;
        let t = reg(local, 1)?;
        self.emit(I::Op(Op::IntegerToReal { local }))?;
        let real = |s: &mut Self, value: f64, operation| -> Result<()> {
            s.set(t, real_bits(value))?;
            s.emit(I::Op(Op::Real {
                lhs: local,
                rhs: t,
                operation,
            }))
        };
        real(self, 1.0 / 396_851.0, RealBinary::Multiply)?;
        self.emit(I::Op(Op::RealUnary {
            local,
            operation: RealUnary::Ln,
        }))?;
        real(self, 3.0, RealBinary::Multiply)?;
        self.emit(I::Op(Op::RealUnary {
            local,
            operation: RealUnary::Exp,
        }))
    }

    /// Sustain engine units (0..=1000000, shown in dB with 1000000 at 0 dB) to
    /// the core's 0..=1000 amplitude factor in place.
    /// ponytail: cubic amplitude like the volume and effect-gain knobs; only
    /// 1000000 (unity) is confirmed against shipping scripts. Measure the rest.
    fn envelope_sustain(&mut self, local: u16) -> Result<()> {
        self.clamp(local, 0, 1_000_000)?;
        let t = reg(local, 1)?;
        self.emit(I::Op(Op::IntegerToReal { local }))?;
        let real = |s: &mut Self, value: f64, operation| -> Result<()> {
            s.set(t, real_bits(value))?;
            s.emit(I::Op(Op::Real {
                lhs: local,
                rhs: t,
                operation,
            }))
        };
        real(self, 1e-6, RealBinary::Multiply)?;
        self.emit(I::Op(Op::RealUnary {
            local,
            operation: RealUnary::Ln,
        }))?;
        real(self, 3.0, RealBinary::Multiply)?;
        self.emit(I::Op(Op::RealUnary {
            local,
            operation: RealUnary::Exp,
        }))?;
        real(self, 1000.0, RealBinary::Multiply)?;
        self.emit(I::Op(Op::RealToInteger { local }))
    }

    /// Engine units to frames in place: ms = 2^(v·(log2(max + 2) − 1)/10^6 + 1) − 2,
    /// max 15000 ms for attack and 25000 ms for decay and release, the law
    /// shipping library scripts display.
    fn envelope_frames(&mut self, stage: EnvelopeStage, local: u16) -> Result<()> {
        self.clamp(local, 0, 1_000_000)?;
        let max: f64 = if stage == EnvelopeStage::Attack {
            15002.0
        } else {
            25002.0
        };
        let ln2 = std::f64::consts::LN_2;
        let t = reg(local, 1)?;
        self.emit(I::Op(Op::IntegerToReal { local }))?;
        let real = |s: &mut Self, value: f64, operation| -> Result<()> {
            s.set(t, real_bits(value))?;
            s.emit(I::Op(Op::Real {
                lhs: local,
                rhs: t,
                operation,
            }))
        };
        real(self, (max.log2() - 1.0) / 1e6 * ln2, RealBinary::Multiply)?;
        real(self, ln2, RealBinary::Add)?;
        self.emit(I::Op(Op::RealUnary {
            local,
            operation: RealUnary::Exp,
        }))?;
        // ms − 2, in microseconds, then frames.
        real(self, 2.0, RealBinary::Subtract)?;
        real(self, 1000.0, RealBinary::Multiply)?;
        self.emit(I::Op(Op::RealToInteger { local }))?;
        self.clamp(local, 0, i32::MAX)?;
        self.emit(I::MicrosToFrames { local })
    }

    /// Kontakt engine units (0..=1000000) to `WriteParam` units in place. The
    /// laws are the ones shipping library scripts display: volume
    /// 18 dB per octave of value, 0 dB at 629960 (+12 dB at full); pan linear
    /// about 500000; tune ±36 semitones linear about 500000.
    fn engine_units(&mut self, target: ModTarget, local: u16) -> Result<()> {
        self.clamp(local, 0, 1_000_000)?;
        let t = reg(local, 1)?;
        match target {
            ModTarget::Decibels => {
                // millidecibels = 18000·log2(max(v, 1)) − 346768.2342
                self.clamp(local, 1, 1_000_000)?;
                self.emit(I::Op(Op::IntegerToReal { local }))?;
                self.emit(I::Op(Op::RealUnary {
                    local,
                    operation: RealUnary::Ln,
                }))?;
                for (value, operation) in [
                    (18000.0 / std::f64::consts::LN_2, RealBinary::Multiply),
                    (346_768.234_247_835_1, RealBinary::Subtract),
                ] {
                    self.set(t, real_bits(value))?;
                    self.emit(I::Op(Op::Real {
                        lhs: local,
                        rhs: t,
                        operation,
                    }))?;
                }
                self.emit(I::Op(Op::RealToInteger { local }))
            }
            _ => {
                // pan: (v − 500000) / 500; tune millicents: (v − 500000) · 36 / 5
                let steps: &[(i64, IB)] = if target == ModTarget::Pan {
                    &[(500_000, IB::Subtract), (500, IB::Divide)]
                } else {
                    &[(500_000, IB::Subtract), (36, IB::Multiply), (5, IB::Divide)]
                };
                for &(value, operation) in steps {
                    self.set(t, value)?;
                    self.emit(I::Binary32 {
                        lhs: local,
                        rhs: t,
                        operation,
                    })?;
                }
                Ok(())
            }
        }
    }

    /// The inverse of [`Self::engine_units`], clamped to 0..=1000000.
    fn engine_value(&mut self, target: ModTarget, local: u16) -> Result<()> {
        let t = reg(local, 1)?;
        if target == ModTarget::Decibels {
            // v = 2^((millidecibels + 346768.2342) / 18000)
            self.emit(I::Op(Op::IntegerToReal { local }))?;
            for (value, operation) in [
                (346_768.234_247_835_1, RealBinary::Add),
                (std::f64::consts::LN_2 / 18000.0, RealBinary::Multiply),
            ] {
                self.set(t, real_bits(value))?;
                self.emit(I::Op(Op::Real {
                    lhs: local,
                    rhs: t,
                    operation,
                }))?;
            }
            self.emit(I::Op(Op::RealUnary {
                local,
                operation: RealUnary::Exp,
            }))?;
            self.set(t, real_bits(0.5))?;
            self.emit(I::Op(Op::Real {
                lhs: local,
                rhs: t,
                operation: RealBinary::Add,
            }))?;
            self.emit(I::Op(Op::RealToInteger { local }))?;
        } else {
            let steps: &[(i64, IB)] = if target == ModTarget::Pan {
                &[(500, IB::Multiply), (500_000, IB::Add)]
            } else {
                &[(5, IB::Multiply), (36, IB::Divide), (500_000, IB::Add)]
            };
            for &(value, operation) in steps {
                self.set(t, value)?;
                self.emit(I::Binary32 {
                    lhs: local,
                    rhs: t,
                    operation,
                })?;
            }
        }
        self.clamp(local, 0, 1_000_000)
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
        // Kontakt clamps: keys to 0..=127, velocities to 1..=127.
        self.clamp(local, i32::from(!note), 127)?;
        self.emit(if note {
            I::WriteEventKey { event, local }
        } else {
            I::WriteEventVelocity7 { event, local }
        })
    }

    fn search(&mut self, args: &[Arg], dst: u16) -> Result<()> {
        let (array, len) = match args.first() {
            Some(Arg::Var(v, _)) => {
                let Home::Cells { offset, len } = self.var(*v).home else {
                    self.ignore(Builtin::Search, "of a text array is not available; -1");
                    return self.set(dst, -1);
                };
                (Ok(ScriptArray { offset, len }), len)
            }
            Some(Arg::SysArray(sys)) if self.sys_readable(*sys) => (Err(*sys), sys.len()),
            _ => {
                self.ignore(
                    Builtin::Search,
                    "of this runtime-maintained array is not available; -1",
                );
                return self.set(dst, -1);
            }
        };
        let (value, end, t) = (reg(dst, 1)?, reg(dst, 2)?, reg(dst, 3)?);
        // Ascending registers: evaluation uses those above its target.
        if args.len() > 2 {
            self.arg(args, 2, dst)?;
            self.arg(args, 1, value)?;
            self.arg(args, 3, end)?;
        } else {
            self.set(dst, 0)?;
            self.arg(args, 1, value)?;
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
        match array {
            Ok(array) => self.emit(I::ReadScriptArray {
                array,
                index: dst,
                local: t,
            })?,
            Err(sys) => {
                // t (and t + 1 as scratch) are above dst, value and end.
                self.set(t, 0)?;
                self.emit(I::Binary32 {
                    lhs: t,
                    rhs: dst,
                    operation: IB::Add,
                })?;
                self.sys_read(sys, t)?;
            }
        }
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

    fn cells(&self, args: &[Arg], i: usize) -> Option<ScriptArray> {
        let Some(Arg::Var(v, _)) = args.get(i) else {
            return None;
        };
        match self.var(*v).home {
            Home::Cells { offset, len } => Some(ScriptArray { offset, len }),
            _ => None,
        }
    }

    /// `sort(array, direction[, from, to])`: in place, descending when
    /// `direction` is non-zero.
    // ponytail: insertion sort, O(n²) callback fuel on large unsorted arrays.
    fn sort(&mut self, args: &[Arg], dst: u16) -> Result<()> {
        let Some(array) = self.cells(args, 0) else {
            self.ignore(Builtin::Sort, "of a text or runtime array is not available");
            return Ok(());
        };
        let real = matches!(args.first(),Some(Arg::Var(v,_)) if self.var(*v).ty==Ty::Real);
        // Arguments in ascending registers: evaluation uses those above.
        let [end, desc, i, key, j, x, t] = [1, 2, 3, 4, 5, 6, 7].map(|n| dst + n);
        reg(dst, 8)?;
        if args.len() > 3 {
            self.arg(args, 2, dst)?;
            self.arg(args, 3, end)?;
        } else {
            self.set(dst, 0)?;
            self.set(end, i64::from(array.len) - 1)?;
        }
        self.arg(args, 1, desc)?;
        for local in [dst, end] {
            for (bound, operation) in [
                (0, IntegerExtra::Max),
                (array.len as i64 - 1, IntegerExtra::Min),
            ] {
                self.set(t, bound)?;
                self.emit(I::Op(Op::Integer {
                    lhs: local,
                    rhs: t,
                    operation,
                }))?;
            }
        }
        // i = from + 1
        self.set(i, 1)?;
        self.emit(I::Binary32 {
            lhs: i,
            rhs: dst,
            operation: IB::Add,
        })?;
        let outer = self.here();
        self.set(t, 0)?;
        self.emit(I::Binary32 {
            lhs: t,
            rhs: i,
            operation: IB::Add,
        })?;
        self.emit(I::CompareLocal {
            lhs: t,
            rhs: end,
            comparison: Cmp::LessEqual,
        })?;
        let done = self.jump_if_zero(t)?;
        self.emit(I::ReadScriptArray {
            array,
            index: i,
            local: key,
        })?;
        self.set(j, -1)?;
        self.emit(I::Binary32 {
            lhs: j,
            rhs: i,
            operation: IB::Add,
        })?;
        let inner = self.here();
        self.set(t, 0)?;
        self.emit(I::Binary32 {
            lhs: t,
            rhs: j,
            operation: IB::Add,
        })?;
        self.emit(I::CompareLocal {
            lhs: t,
            rhs: dst,
            comparison: Cmp::GreaterEqual,
        })?;
        let place = self.jump_if_zero(t)?;
        self.emit(I::ReadScriptArray {
            array,
            index: j,
            local: x,
        })?;
        // Shift while a[j] is out of order against key.
        let ascending = self.jump_if_zero(desc)?;
        let compare = |real, lhs, rhs, comparison| {
            if real {
                I::Op(Op::CompareReal {
                    lhs,
                    rhs,
                    comparison,
                })
            } else {
                I::CompareLocal {
                    lhs,
                    rhs,
                    comparison,
                }
            }
        };
        // Comparing consumes lhs; copy real bits without 32-bit arithmetic.
        self.emit(I::ReadScriptArray {
            array,
            index: j,
            local: t,
        })?;
        self.emit(compare(real, t, key, Cmp::Less))?;
        let compared = self.jump()?;
        self.land(ascending);
        self.emit(I::ReadScriptArray {
            array,
            index: j,
            local: t,
        })?;
        self.emit(compare(real, t, key, Cmp::Greater))?;
        self.land(compared);
        let place2 = self.jump_if_zero(t)?;
        self.set(t, 1)?;
        self.emit(I::Binary32 {
            lhs: t,
            rhs: j,
            operation: IB::Add,
        })?;
        self.emit(I::WriteScriptArray {
            array,
            index: t,
            local: x,
        })?;
        self.emit(I::AddLocal {
            local: j,
            value: -1,
        })?;
        self.emit(I::Jump { target: inner })?;
        self.land(place);
        self.land(place2);
        self.set(t, 1)?;
        self.emit(I::Binary32 {
            lhs: t,
            rhs: j,
            operation: IB::Add,
        })?;
        self.emit(I::WriteScriptArray {
            array,
            index: t,
            local: key,
        })?;
        self.emit(I::AddLocal { local: i, value: 1 })?;
        self.emit(I::Jump { target: outer })?;
        self.land(done);
        self.cover(Builtin::Sort, Coverage::Native);
        Ok(())
    }

    /// `array_equal(a, b)`: same length and cells.
    fn array_equal(&mut self, args: &[Arg], dst: u16) -> Result<()> {
        let (Some(a), Some(b)) = (self.cells(args, 0), self.cells(args, 1)) else {
            self.ignore(Builtin::ArrayEqual, "of text arrays is not available; 0");
            return self.set(dst, 0);
        };
        self.cover(Builtin::ArrayEqual, Coverage::Native);
        if a.len != b.len {
            return self.set(dst, 0);
        }
        let [i, x, y] = [1, 2, 3].map(|n| dst + n);
        reg(dst, 3)?;
        self.set(i, i64::from(a.len))?;
        let top = self.here();
        self.set(dst, 1)?;
        let done = self.jump_if_zero(i)?;
        self.emit(I::AddLocal {
            local: i,
            value: -1,
        })?;
        self.emit(I::ReadScriptArray {
            array: a,
            index: i,
            local: x,
        })?;
        self.emit(I::ReadScriptArray {
            array: b,
            index: i,
            local: y,
        })?;
        self.emit(I::CompareLocal {
            lhs: x,
            rhs: y,
            comparison: Cmp::Equal,
        })?;
        self.set(dst, 0)?;
        let differ = self.jump_if_zero(x)?;
        self.emit(I::Jump { target: top })?;
        self.land(done);
        self.land(differ);
        Ok(())
    }

    /// 1 in `at` when `lo <= value <= hi`.
    fn in_range(&mut self, value: u16, lo: i32, hi: i32, at: u16) -> Result<()> {
        self.set(at, 0)?;
        self.emit(I::Binary32 {
            lhs: at,
            rhs: value,
            operation: IB::Add,
        })?;
        self.clamp(at, lo, hi)?;
        self.emit(I::CompareLocal {
            lhs: at,
            rhs: value,
            comparison: Cmp::Equal,
        })
    }

    /// Kontakt ignores a `play_note` outside the MIDI ranges and returns -1;
    /// scripts pass an unset key of -1 routinely.
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
        // Constants and the event's own key/velocity are in range already.
        let in_range = |this: &Self, i: usize, lo: i32, sys: SysVar| {
            this.const_int(args, i)
                .is_some_and(|k| (lo..128).contains(&k))
                || (this.note_context()
                    && matches!(
                        this.expr(args, i),
                        Some(Expr { kind: ExprKind::Sys(s), .. }) if *s == sys
                    ))
        };
        if in_range(self, 0, 0, SysVar::EventNote) && in_range(self, 1, 1, SysVar::EventVelocity) {
            return self.play_checked(args, dst, velocity, offset_micros);
        }
        let (key_ok, velocity_ok) = (reg(frames, 2)?, reg(frames, 3)?);
        self.in_range(dst, 0, 127, key_ok)?;
        self.in_range(velocity, 1, 127, velocity_ok)?;
        self.emit(I::Binary32 {
            lhs: key_ok,
            rhs: velocity_ok,
            operation: IB::Multiply,
        })?;
        let bad = self.jump_if_zero(key_ok)?;
        self.play_checked(args, dst, velocity, offset_micros)?;
        let end = self.jump()?;
        self.land(bad);
        self.set(dst, -1)?;
        self.land(end);
        Ok(())
    }

    fn play_checked(
        &mut self,
        args: &[Arg],
        dst: u16,
        velocity: u16,
        offset_micros: Option<u16>,
    ) -> Result<()> {
        let offset = reg(dst, 2)?;
        let frames = if offset_micros.is_some() {
            reg(offset, 1)?
        } else {
            offset
        };
        let scratch = reg(frames, 1)?;
        let inheritance = Inheritance::Expression;
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

    fn property_key(&mut self, args: &[Arg], base: u16, index: Option<usize>) -> Result<()> {
        self.arg(args, 0, base)?;
        self.arg(args, 1, base + 1)?;
        if let Some(index) = index {
            self.arg(args, index, base + 2)?;
        } else {
            self.set(base + 2, i64::from(PROPERTY_TAG))?;
        }
        self.set(base + 3, i64::from(PROPERTY_TAG))
    }
    fn engine_address(&mut self, args: &[Arg], base: u16, write: bool) -> Result<()> {
        for (i, arg) in (if write { [0, 2, 3, 4] } else { [0, 1, 2, 3] })
            .into_iter()
            .enumerate()
        {
            self.arg(args, arg, reg(base, i as u16)?)?;
        }
        Ok(())
    }
    fn indexed_value(&mut self, args: &[Arg], dst: u16, write: bool) -> Result<()> {
        let widgets: Vec<_> = self
            .u
            .hir
            .uis
            .iter()
            .enumerate()
            .filter_map(|(i, ui)| match self.var(ui.var).home {
                Home::Cells { offset, len } => Some((i, ScriptArray { offset, len })),
                _ => None,
            })
            .collect();
        for (ui, array) in widgets {
            self.arg(args, 0, dst + 1)?;
            self.set(dst + 2, i64::from(b::FIRST_UI_ID + ui as i32))?;
            self.emit(I::CompareLocal {
                lhs: dst + 1,
                rhs: dst + 2,
                comparison: Cmp::Equal,
            })?;
            let skip = self.jump_if_zero(dst + 1)?;
            self.arg(args, if write { 3 } else { 2 }, dst + 1)?;
            if write {
                self.arg(args, 2, dst)?;
                self.emit(I::WriteScriptArray {
                    array,
                    index: dst + 1,
                    local: dst,
                })?;
            } else {
                self.emit(I::ReadScriptArray {
                    array,
                    index: dst + 1,
                    local: dst,
                })?;
            }
            self.land(skip);
        }
        Ok(())
    }
    fn set_control_par(&mut self, builtin: Builtin, args: &[Arg], dst: u16) -> Result<()> {
        let par = self.const_int(args, 1);
        if matches!(
            builtin,
            Builtin::SetControlParArr | Builtin::SetControlParRealArr
        ) {
            if par == Some(b::CONTROL_PAR_VALUE) {
                self.indexed_value(args, dst, true)?;
            }
            self.property_key(args, dst + 1, Some(3))?;
            self.arg(args, 2, dst)?;
            self.emit(I::Op(Op::Store {
                key: dst + 1,
                local: dst,
                write: true,
            }))?;
            return self.effect(builtin, args, dst);
        }
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
        if builtin == Builtin::GetControlParArr {
            self.property_key(args, dst + 1, Some(2))?;
            self.set(dst, 0)?;
            self.emit(I::Op(Op::Store {
                key: dst + 1,
                local: dst,
                write: false,
            }))?;
            if par == Some(b::CONTROL_PAR_VALUE) {
                self.indexed_value(args, dst, false)?;
            }
            self.cover(builtin, Coverage::Native);
            return Ok(());
        }
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

/// Outbox coverage: host-owned services, or engine changes left to the host.
fn host_service(builtin: Builtin) -> Coverage {
    use Builtin::*;
    match builtin {
        SetControlPar
        | SetControlParReal
        | SetControlParArr
        | SetControlParRealArr
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
            | SetSkinOffset
            | SetUiColor
        | SetUiWfProperty
        | AttachLevelMeter
        | FsNavigate
        | SetNksNavName
        | SetNksNavPar
        | ResetNksNav
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
        | PgsSetKeyVal
        | PgsSetStrKeyVal
        | PgsCreateKey
        | PgsCreateStrKey => Coverage::Host,
        // Silenced in the runtime; the effect only lets the host free samples.
        PurgeGroup => Coverage::Native,
        _ => Coverage::Effect,
    }
}
