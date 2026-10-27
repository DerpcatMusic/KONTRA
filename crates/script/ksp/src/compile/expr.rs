//! Expressions and builtin calls.

use super::*;

impl<'a> Compiler<'a> {
    // ---- Expressions ----

    pub(super) fn condition(&mut self, e: &Expr<'a>) -> Result<(), Error> {
        match self.expr(e)? {
            Ty::Bool => Ok(()),
            ty => Err(Error::new(
                e.span,
                format!("expected a condition, found {}", ty.name()),
            )),
        }
    }

    /// Converts the value on the stack from `got` to `want`, where KSP allows it.
    pub(super) fn convert(&mut self, got: Ty, want: Ty, span: Span) -> Result<(), Error> {
        match (got, want) {
            _ if got == want => {}
            (Ty::Bool, Ty::Int) => {}
            (Ty::Int | Ty::Bool, Ty::Text) => self.emit(Op::TFromInt),
            (Ty::Real, Ty::Text) => self.emit(Op::TFromReal),
            _ => {
                return Err(Error::new(
                    span,
                    format!("expected {}, found {}", want.name(), got.name()),
                ));
            }
        }
        Ok(())
    }

    pub(super) fn expr(&mut self, e: &Expr<'a>) -> Result<Ty, Error> {
        match &e.kind {
            ExprKind::Int(n) => Ok(self.constant(Value::Int(int_literal(*n, e.span)?))),
            ExprKind::Real(n) => Ok(self.constant(Value::Real(*n))),
            ExprKind::Str(s) => {
                let id = self.text(s);
                self.emit(Op::Text(id));
                Ok(Ty::Text)
            }
            ExprKind::Word(name) => match Builtin::from_name(name) {
                Some(b) => self.value_builtin(b, &[], e.span),
                None => Err(Error::new(e.span, format!("unknown name {name}"))),
            },
            ExprKind::Call(name, args) => match Builtin::from_name(name) {
                Some(b) => self.value_builtin(b, args, e.span),
                None if self.function_names.contains_key(&name.to_ascii_lowercase()) => {
                    Err(Error::new(e.span, "functions return no value"))
                }
                None => Err(Error::new(e.span, format!("unknown function {name}"))),
            },
            ExprKind::Var(name, index) => self.load(name, index.as_deref(), e.span),
            ExprKind::Unary(op, inner) => {
                if let (Un::Neg, ExprKind::Int(n)) = (op, &inner.kind) {
                    let n = int_literal(*n, inner.span)?.wrapping_neg();
                    return Ok(self.constant(Value::Int(n)));
                }
                let ty = self.expr(inner)?;
                match (op, ty) {
                    (Un::Neg, Ty::Int) => self.emit(Op::INeg),
                    (Un::Neg, Ty::Real) => self.emit(Op::FNeg),
                    (Un::BitNot, Ty::Int) => self.emit(Op::IBitNot),
                    (Un::Not, Ty::Bool) => self.emit(Op::Not),
                    _ => {
                        return Err(Error::new(
                            e.span,
                            format!("operator cannot take {}", ty.name()),
                        ));
                    }
                }
                Ok(ty)
            }
            ExprKind::Binary(op, a, b) => self.binary(*op, a, b, e.span),
        }
    }

    pub(super) fn value_builtin(
        &mut self,
        b: Builtin,
        args: &[Expr<'a>],
        span: Span,
    ) -> Result<Ty, Error> {
        self.builtin(b, args, span)?
            .ok_or_else(|| Error::new(span, format!("{} returns no value", b.name())))
    }

    pub(super) fn load(
        &mut self,
        name: &'a str,
        index: Option<&Expr<'a>>,
        span: Span,
    ) -> Result<Ty, Error> {
        if let Ok(v) = self.lookup(name, span) {
            let var = self.var(v)?;
            let (ty, home) = (var.ty, var.home);
            return match (home, index) {
                (Home::Cell(n), None) => {
                    self.emit(Op::Load(n));
                    Ok(ty)
                }
                (Home::Text(n), None) => {
                    self.emit(Op::TLoad(n));
                    Ok(ty)
                }
                (Home::Poly(n), None) => {
                    self.emit(Op::LoadPoly(n));
                    Ok(ty)
                }
                (Home::Const(value), None) => Ok(self.constant(value)),
                (Home::Cells { base, len }, Some(i)) => {
                    let got = self.expr(i)?;
                    self.convert(got, Ty::Int, i.span)?;
                    self.emit(Op::LoadAt { base, len });
                    Ok(ty)
                }
                (Home::Texts { base, len }, Some(i)) => {
                    let got = self.expr(i)?;
                    self.convert(got, Ty::Int, i.span)?;
                    self.emit(Op::TLoadAt { base, len });
                    Ok(ty)
                }
                (Home::Cells { .. } | Home::Texts { .. }, None) => {
                    Err(Error::new(span, "an array needs an index"))
                }
                (_, Some(i)) => Err(Error::new(i.span, format!("{name} is not an array"))),
            };
        }
        if let Some(b) = Builtin::from_name(name).filter(|b| b.name().starts_with(['$', '%'])) {
            return match (b.array_len(), index) {
                (Some(_), Some(i)) => self.value_builtin(b, std::slice::from_ref(i), span),
                (None, None) => self.value_builtin(b, &[], span),
                (Some(_), None) => Err(Error::new(span, "an array needs an index")),
                (None, Some(i)) => Err(Error::new(i.span, format!("{name} is not an array"))),
            };
        }
        if let Some(i) = index {
            return Err(Error::new(i.span, format!("{name} is not an array")));
        }
        if let Some(value) = self.named_constant(name) {
            return Ok(self.constant(value));
        }
        if consts::is_catalog(name) {
            let id = self.symbol(name);
            self.emit(Op::Int(id));
            return Ok(Ty::Int);
        }
        Err(Error::new(span, format!("{name} is not declared")))
    }

    pub(super) fn binary(
        &mut self,
        op: Bin,
        a: &Expr<'a>,
        b: &Expr<'a>,
        span: Span,
    ) -> Result<Ty, Error> {
        if op == Bin::Concat {
            let got = self.expr(a)?;
            self.convert(got, Ty::Text, a.span)?;
            let got = self.expr(b)?;
            self.convert(got, Ty::Text, b.span)?;
            self.emit(Op::TConcat);
            return Ok(Ty::Text);
        }
        let left = self.expr(a)?;
        let right = self.expr(b)?;
        let mismatch = || {
            Error::new(
                span,
                format!("operator cannot take {} and {}", left.name(), right.name()),
            )
        };
        let cmp = |op| match op {
            Bin::Eq => Some(Cmp::Eq),
            Bin::Ne => Some(Cmp::Ne),
            Bin::Lt => Some(Cmp::Lt),
            Bin::Gt => Some(Cmp::Gt),
            Bin::Le => Some(Cmp::Le),
            Bin::Ge => Some(Cmp::Ge),
            _ => None,
        };
        let int = |t| matches!(t, Ty::Int | Ty::Bool);
        let (code, ty) = match (op, left, right) {
            (Bin::Or, Ty::Bool, Ty::Bool) => (Op::IOr, Ty::Bool),
            (Bin::And, Ty::Bool, Ty::Bool) => (Op::IAnd, Ty::Bool),
            (Bin::Xor, Ty::Bool, Ty::Bool) => (Op::IXor, Ty::Bool),
            (Bin::Eq, Ty::Text, Ty::Text) => (Op::TEq, Ty::Bool),
            (Bin::Ne, Ty::Text, Ty::Text) => {
                self.emit(Op::TEq);
                (Op::Not, Ty::Bool)
            }
            (_, l, r) if int(l) && int(r) => match op {
                Bin::Add => (Op::IAdd, Ty::Int),
                Bin::Sub => (Op::ISub, Ty::Int),
                Bin::Mul => (Op::IMul, Ty::Int),
                Bin::Div => (Op::IDiv, Ty::Int),
                Bin::Mod => (Op::IMod, Ty::Int),
                Bin::BitAnd => (Op::IAnd, Ty::Int),
                Bin::BitOr => (Op::IOr, Ty::Int),
                Bin::BitXor => (Op::IXor, Ty::Int),
                _ => (Op::ICmp(cmp(op).ok_or_else(mismatch)?), Ty::Bool),
            },
            (_, Ty::Real, Ty::Real) => match op {
                Bin::Add => (Op::FAdd, Ty::Real),
                Bin::Sub => (Op::FSub, Ty::Real),
                Bin::Mul => (Op::FMul, Ty::Real),
                Bin::Div => (Op::FDiv, Ty::Real),
                _ => (Op::FCmp(cmp(op).ok_or_else(mismatch)?), Ty::Bool),
            },
            _ => return Err(mismatch()),
        };
        self.emit(code);
        Ok(ty)
    }

    // ---- Builtins ----

    /// A system array passed whole (`search(%KEY_DOWN, 1)`): copied element
    /// by element into hidden cells, which the builtin then reads.
    fn system_array(&mut self, e: &Expr<'a>) -> Result<Option<(u32, u32)>, Error> {
        let ExprKind::Var(name, None) = &e.kind else {
            return Ok(None);
        };
        if self.lookup(name, e.span).is_ok() {
            return Ok(None);
        }
        let Some((action, len)) = Builtin::from_name(name)
            .filter(|b| b.name().starts_with('%'))
            .and_then(|b| Some((b.action()?, b.array_len()?)))
        else {
            return Ok(None);
        };
        let i = Self::cells(&mut self.cells, len + 1, e.span)?;
        let base = i + 1;
        self.emit(Op::Int(0));
        self.emit(Op::Store(i));
        let top = self.here();
        self.emit(Op::LoopCheck { exit: 0 });
        self.emit(Op::Load(i));
        self.emit(Op::Int(len as i32));
        self.emit(Op::ICmp(Cmp::Lt));
        let leave = self.here();
        self.emit(Op::JumpIfZero(0));
        self.emit(Op::Load(i));
        self.emit(Op::Load(i));
        self.host(action, 1, 0, Returns::Int);
        self.emit(Op::StoreAt { base, len });
        self.emit(Op::Load(i));
        self.emit(Op::Int(1));
        self.emit(Op::IAdd);
        self.emit(Op::Store(i));
        self.emit(Op::Jump(top));
        let end = self.here();
        self.emit(Op::LoopExit);
        self.patch(top, end);
        self.patch(leave, end);
        Ok(Some((base, len)))
    }

    pub(super) fn host(&mut self, action: Action, ints: usize, texts: usize, returns: Returns) {
        self.emit(Op::Host {
            action,
            ints: ints as u8,
            texts: texts as u8,
            returns,
        });
    }

    /// The variable an argument names, and its control.
    pub(super) fn named(&self, e: &Expr<'a>) -> Result<(usize, Option<u32>), Error> {
        let ExprKind::Var(name, None) = &e.kind else {
            return Err(Error::new(e.span, "expected a variable"));
        };
        let v = self.lookup(name, e.span)?;
        Ok((v, self.var(v)?.control))
    }

    /// Emits a builtin call; returns the type it leaves on the stack, if any.
    pub(super) fn builtin(
        &mut self,
        b: Builtin,
        args: &[Expr<'a>],
        span: Span,
    ) -> Result<Option<Ty>, Error> {
        let sig = b.sig();
        let required = sig.args.len() - usize::from(sig.optional);
        if args.len() < required || args.len() > sig.args.len() {
            return Err(Error::new(
                span,
                format!("{} takes {} arguments", b.name(), sig.args.len()),
            ));
        }
        let arg = |i: usize| {
            args.get(i)
                .ok_or_else(|| Error::new(span, "missing argument"))
        };
        match b {
            Builtin::Exit => {
                self.emit(Op::Exit);
                return Ok(None);
            }
            Builtin::Continue => {
                let top = *self
                    .loops
                    .last()
                    .ok_or_else(|| Error::new(span, "continue outside a loop"))?;
                self.emit(Op::Jump(top));
                return Ok(None);
            }
            Builtin::Inc | Builtin::Dec => {
                let target = arg(0)?;
                let one = Expr {
                    span,
                    kind: ExprKind::Int(1),
                };
                let op = if b == Builtin::Inc {
                    Bin::Add
                } else {
                    Bin::Sub
                };
                let value = Expr {
                    span,
                    kind: ExprKind::Binary(op, Box::new(target.clone()), Box::new(one)),
                };
                self.assign(target, &value)?;
                return Ok(None);
            }
            Builtin::Wait => {
                let got = self.expr(arg(0)?)?;
                self.convert(got, Ty::Int, span)?;
                self.emit(Op::Wait);
                return Ok(None);
            }
            Builtin::IntToReal | Builtin::Real => {
                let got = self.expr(arg(0)?)?;
                self.convert(got, Ty::Int, span)?;
                self.emit(Op::IntToReal);
                return Ok(Some(Ty::Real));
            }
            Builtin::RealToInt | Builtin::Int => {
                let got = self.expr(arg(0)?)?;
                self.convert(got, Ty::Real, span)?;
                self.emit(Op::RealToInt);
                return Ok(Some(Ty::Int));
            }
            Builtin::ShLeft | Builtin::ShRight => {
                for i in 0..2 {
                    let got = self.expr(arg(i)?)?;
                    self.convert(got, Ty::Int, span)?;
                }
                self.emit(if b == Builtin::ShLeft {
                    Op::IShl
                } else {
                    Op::IShr
                });
                return Ok(Some(Ty::Int));
            }
            Builtin::GetUiId => {
                let (_, control) = self.named(arg(0)?)?;
                let control = control.ok_or_else(|| Error::new(span, "not a UI control"))?;
                self.emit(Op::Int(FIRST_UI_ID + control as i32));
                return Ok(Some(Ty::Int));
            }
            Builtin::MakePersistent | Builtin::MakeInstrPersistent => {
                let (v, _) = self.named(arg(0)?)?;
                if let Some(var) = self.vars.get_mut(v) {
                    var.persistence = if b == Builtin::MakePersistent {
                        Persistence::Snapshot
                    } else {
                        Persistence::Instrument
                    };
                }
                return Ok(None);
            }
            Builtin::WatchVar | Builtin::WatchArrayIdx => return Ok(None),
            Builtin::NumElements => {
                let e = arg(0)?;
                let ExprKind::Var(name, None) = &e.kind else {
                    return Err(Error::new(e.span, "expected an array"));
                };
                let len = match self.lookup(name, e.span) {
                    Ok(v) => self.var(v)?.array().map(|(_, len)| len),
                    Err(_) => Builtin::from_name(name).and_then(Builtin::array_len),
                };
                let len = len.ok_or_else(|| Error::new(e.span, "expected an array"))?;
                self.emit(Op::Int(len as i32));
                return Ok(Some(Ty::Int));
            }
            // The engine copies the variable's saved numbers into its cells now,
            // mid-init. ponytail: text variables read nothing until a script needs it.
            Builtin::ReadPersistentVar => {
                let (v, _) = self.named(arg(0)?)?;
                let (base, len) = match self.var(v)?.home {
                    Home::Cell(n) => (n, 1),
                    Home::Cells { base, len } => (base, len),
                    _ => (0, 0),
                };
                self.emit(Op::Int(base as i32));
                self.emit(Op::Int(len as i32));
                self.host(Action::ReadPersistentVar, 2, 0, Returns::Nothing);
                return Ok(None);
            }
            _ => {}
        }
        let Some(action) = b.action() else {
            return Err(Error::new(
                span,
                format!("{} cannot be used here", b.name()),
            ));
        };
        let (mut ints, mut texts) = (0, 0);
        let mut number: Option<Ty> = None;
        let mut array_len = 0;
        for (i, kind) in sig.args.iter().enumerate() {
            let Some(e) = args.get(i) else {
                // Left-out optional arguments pass zero; a left-out search or
                // sort range is the whole array.
                let last = i == 3 && matches!(b, Builtin::Search | Builtin::Sort);
                match kind {
                    _ if last => {
                        self.emit(Op::Int(array_len as i32 - 1));
                        ints += 1;
                    }
                    Arg::S | Arg::K => {
                        let id = self.text("");
                        self.emit(Op::Text(id));
                        texts += 1;
                    }
                    Arg::R => {
                        self.emit(Op::Real(0.0));
                        ints += 1;
                    }
                    _ => {
                        self.emit(Op::Int(0));
                        ints += 1;
                    }
                }
                continue;
            };
            match kind {
                Arg::I | Arg::R => {
                    let want = if *kind == Arg::I { Ty::Int } else { Ty::Real };
                    let got = self.expr(e)?;
                    self.convert(got, want, e.span)?;
                    ints += 1;
                }
                Arg::N => {
                    let got = self.expr(e)?;
                    let got = if got == Ty::Bool { Ty::Int } else { got };
                    let want = *number.get_or_insert(got);
                    if !matches!(got, Ty::Int | Ty::Real) || got != want {
                        return Err(Error::new(e.span, "expected numbers of one type"));
                    }
                    ints += 1;
                }
                Arg::S => {
                    let got = self.expr(e)?;
                    self.convert(got, Ty::Text, e.span)?;
                    texts += 1;
                }
                Arg::K => {
                    if let ExprKind::Word(word) = &e.kind {
                        let id = self.text(word);
                        self.emit(Op::Text(id));
                    } else {
                        let got = self.expr(e)?;
                        self.convert(got, Ty::Text, e.span)?;
                    }
                    texts += 1;
                }
                Arg::V => {
                    let (_, control) = self.named(e)?;
                    let control = control.ok_or_else(|| Error::new(e.span, "not a UI control"))?;
                    self.emit(Op::Int(FIRST_UI_ID + control as i32));
                    ints += 1;
                }
                Arg::A => {
                    // Files hold text arrays too; the other builtins compare numbers.
                    let files = matches!(
                        b,
                        Builtin::LoadArray
                            | Builtin::SaveArray
                            | Builtin::LoadArrayStr
                            | Builtin::SaveArrayStr
                    );
                    let (ty, base, len) = match self.system_array(e)? {
                        Some((base, len)) => (Ty::Int, base, len),
                        None => {
                            let (v, _) = self.named(e)?;
                            let var = self.var(v)?;
                            let (ty, array) = (var.ty, var.array());
                            let Some((base, len)) = array.filter(|_| files || ty != Ty::Text)
                            else {
                                return Err(Error::new(
                                    e.span,
                                    "expected an integer or real array",
                                ));
                            };
                            (ty, base, len)
                        }
                    };
                    let ty = if ty == Ty::Text { Ty::Int } else { ty };
                    if let Some(&want) = number.as_ref()
                        && want != ty
                    {
                        return Err(Error::new(e.span, "array and value types differ"));
                    }
                    number.get_or_insert(ty);
                    array_len = len;
                    self.emit(Op::Int(base as i32));
                    self.emit(Op::Int(len as i32));
                    ints += 2;
                }
            }
        }
        // Builtins over numbers get a trailing 1 when those numbers are real.
        if sig.args.iter().any(|a| matches!(a, Arg::N | Arg::A)) {
            self.emit(Op::Int(i32::from(number == Some(Ty::Real))));
            ints += 1;
        }
        let ty = match sig.ret {
            Ret::Void => None,
            Ret::Int => Some(Ty::Int),
            Ret::Bool => Some(Ty::Bool),
            Ret::Real => Some(Ty::Real),
            Ret::Text => Some(Ty::Text),
            Ret::Num => Some(number.unwrap_or(Ty::Int)),
        };
        let returns = match ty {
            None => Returns::Nothing,
            Some(Ty::Int | Ty::Bool) => Returns::Int,
            Some(Ty::Real) => Returns::Real,
            Some(Ty::Text) => Returns::Text,
        };
        if ints > usize::from(u8::MAX) || texts > usize::from(u8::MAX) {
            return Err(Error::new(span, "too many arguments"));
        }
        self.host(action, ints, texts, returns);
        Ok(ty)
    }
}
