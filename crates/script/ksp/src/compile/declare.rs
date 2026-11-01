//! Declarations and the constant folding their sizes and values need.

use super::*;

impl<'a> Compiler<'a> {
    // ---- Declarations ----

    pub(super) fn cells(counter: &mut u32, len: u32, span: Span) -> Result<u32, Error> {
        let base = *counter;
        *counter = base
            .checked_add(len)
            .filter(|&n| n < u32::MAX / 2)
            .ok_or_else(|| Error::new(span, "too many variables"))?;
        Ok(base)
    }

    pub(super) fn declarations(&mut self, body: &[Stmt<'a>]) -> Result<(), Error> {
        for s in body {
            match &s.kind {
                StmtKind::Declare(d) => self.declare(d)?,
                StmtKind::If(_, yes, no) => {
                    self.declarations(yes)?;
                    self.declarations(no)?;
                }
                StmtKind::While(_, body) => self.declarations(body)?,
                StmtKind::Select(_, cases) => {
                    for case in cases {
                        self.declarations(&case.body)?;
                    }
                }
                _ => {}
            }
        }
        Ok(())
    }

    pub(super) fn declare(&mut self, d: &Declare<'a>) -> Result<(), Error> {
        let span = d.span;
        let key = d.name.to_ascii_lowercase();
        if self.names.contains_key(&key) {
            return Err(Error::new(span, format!("{} is declared twice", d.name)));
        }
        if Builtin::from_name(d.name).is_some()
            || consts::int(d.name).is_some()
            || consts::is_catalog(d.name)
        {
            return Err(Error::new(span, format!("{} is a reserved name", d.name)));
        }
        let (ty, array) = prefix(d.name);
        let len = match (&d.size, array) {
            (Some(size), true) => {
                let n = self.fold_int(size)?;
                match u32::try_from(n)
                    .ok()
                    .filter(|n| (1..=MAX_ARRAY).contains(n))
                {
                    Some(n) => Some(n),
                    None => return Err(Error::new(size.span, "array size must be 1 to 1000000")),
                }
            }
            (None, true) => return Err(Error::new(span, "an array needs a size")),
            (Some(size), false) => return Err(Error::new(size.span, "only arrays take a size")),
            (None, false) => None,
        };
        let persistence = match d.storage {
            Storage::Persistent => Persistence::Snapshot,
            Storage::InstrumentPersistent => Persistence::Instrument,
            _ => Persistence::None,
        };
        let home = match (d.storage, len, ty) {
            (Storage::Const, None, _) => {
                let [init] = d.init.as_slice() else {
                    return Err(Error::new(span, "a constant needs one value"));
                };
                let value = self.fold(init)?;
                if value.ty() != ty && !(ty == Ty::Int && value.ty() == Ty::Bool) {
                    return Err(Error::new(init.span, format!("expected {}", ty.name())));
                }
                Home::Const(value)
            }
            (Storage::Const, Some(_), _) => {
                return Err(Error::new(span, "constants cannot be arrays"));
            }
            (Storage::Polyphonic, None, Ty::Int | Ty::Real) => {
                let n = self.poly;
                self.poly = n
                    .checked_add(1)
                    .ok_or_else(|| Error::new(span, "too many polyphonic variables"))?;
                Home::Poly(n)
            }
            (Storage::Polyphonic, ..) => {
                return Err(Error::new(
                    span,
                    "polyphonic variables are integer or real scalars",
                ));
            }
            (_, None, Ty::Text) => Home::Text(Self::cells(&mut self.text_cells, 1, span)?),
            (_, None, _) => Home::Cell(Self::cells(&mut self.cells, 1, span)?),
            (_, Some(len), Ty::Text) => Home::Texts {
                base: Self::cells(&mut self.text_cells, len, span)?,
                len,
            },
            (_, Some(len), _) => Home::Cells {
                base: Self::cells(&mut self.cells, len, span)?,
                len,
            },
        };
        let control = match d.ui {
            Some((word, at)) => {
                let kind = control_kind(word)
                    .ok_or_else(|| Error::new(at, format!("unknown declaration keyword {word}")))?;
                let expected = match kind {
                    ControlKind::Table => (Ty::Int, true),
                    ControlKind::Xy => (Ty::Real, true),
                    ControlKind::TextEdit => (Ty::Text, false),
                    _ => (Ty::Int, false),
                };
                if (ty, array) != expected || matches!(home, Home::Const(_) | Home::Poly(_)) {
                    return Err(Error::new(
                        span,
                        format!("{word} needs a different variable"),
                    ));
                }
                let mut params = Vec::with_capacity(d.params.len());
                for p in &d.params {
                    // A range read from variables is set when on init reaches the declaration.
                    params.push(self.fold_int(p).unwrap_or(0));
                }
                let index = self.controls.len() as u32;
                self.controls.push(Control {
                    kind,
                    variable: self.vars.len() as u32,
                    params,
                });
                Some(index)
            }
            None if !d.params.is_empty() => {
                return Err(Error::new(span, "only UI controls take parameters"));
            }
            None => None,
        };
        self.declared.insert(span.start, self.vars.len());
        self.names.insert(key, self.vars.len());
        self.vars.push(Var {
            name: d.name.into(),
            ty,
            home,
            control,
            persistence,
        });
        Ok(())
    }

    /// A control of the loaded performance view, declared on first sight: a
    /// knob, a table for a `%` name, a text edit for an `@` name. ponytail: the view's own defaults, ranges
    /// and table sizes are not read from its `.nckp`; controls start at 0 and
    /// tables take KSP's largest size.
    pub(super) fn view_control(&mut self, name: &'a str, span: Span) -> Result<(), Error> {
        let key = name.to_ascii_lowercase();
        if self.names.contains_key(&key)
            || Builtin::from_name(name).is_some()
            || self.named_constant(name).is_some()
            || consts::is_catalog(name)
        {
            return Ok(());
        }
        let (kind, ty, home) = match name.as_bytes().first() {
            Some(b'%') => {
                let len = 128;
                let base = Self::cells(&mut self.cells, len, span)?;
                (ControlKind::Table, Ty::Int, Home::Cells { base, len })
            }
            Some(b'@') => {
                let n = Self::cells(&mut self.text_cells, 1, span)?;
                (ControlKind::TextEdit, Ty::Text, Home::Text(n))
            }
            _ => {
                let n = Self::cells(&mut self.cells, 1, span)?;
                (ControlKind::Knob, Ty::Int, Home::Cell(n))
            }
        };
        let index = self.controls.len() as u32;
        self.controls.push(Control {
            kind,
            variable: self.vars.len() as u32,
            params: Vec::new(),
        });
        self.names.insert(key, self.vars.len());
        self.vars.push(Var {
            name: name.into(),
            ty,
            home,
            control: Some(index),
            persistence: Persistence::None,
        });
        Ok(())
    }

    /// The code a declaration runs where `on init` reaches it.
    pub(super) fn declaration(&mut self, d: &Declare<'a>) -> Result<(), Error> {
        let Some(&v) = self.declared.get(&d.span.start) else {
            return Ok(());
        };
        let Some(var) = self.vars.get(v) else {
            return Ok(());
        };
        let (ty, home, control) = (var.ty, var.home, var.control);
        if let Some(control) = control {
            self.control_params(d, control)?;
            // A knob, slider or value edit starts at its minimum (Kontakt
            // 8.13.1 oracle: (2, 4) reads 2, (-9, -3) reads -9).
            let kind = self.controls.get(control as usize).map(|c| c.kind);
            let ranged = matches!(
                kind,
                Some(ControlKind::Knob | ControlKind::Slider | ControlKind::ValueEdit)
            );
            if let (true, Some(min), None) = (ranged, d.params.first(), d.init.first()) {
                let got = self.expr(min)?;
                self.convert(got, Ty::Int, min.span)?;
                self.store_scalar(home);
            }
        }
        match home {
            Home::Const(_) | Home::Poly(_) => {}
            Home::Cell(_) | Home::Text(_) => {
                if d.list {
                    return Err(Error::new(d.span, "a scalar takes one value"));
                }
                if let Some(init) = d.init.first() {
                    let got = self.expr(init)?;
                    self.convert(got, ty, init.span)?;
                    self.store_scalar(home);
                }
            }
            Home::Cells { base, len } | Home::Texts { base, len } => {
                if d.init.is_empty() {
                    return Ok(());
                }
                if !d.list {
                    return Err(Error::new(d.span, "an array takes a list: := (a, b, ...)"));
                }
                if d.init.len() > len as usize {
                    return Err(Error::new(d.span, "more values than array elements"));
                }
                let text = ty == Ty::Text;
                let mut last_zero = false;
                for (i, init) in d.init.iter().enumerate() {
                    last_zero = matches!(init.kind, ExprKind::Int(0));
                    let got = self.expr(init)?;
                    self.convert(got, ty, init.span)?;
                    self.emit(if text {
                        Op::TStore(base + i as u32)
                    } else {
                        Op::Store(base + i as u32)
                    });
                }
                // The last value repeats to the end (Kontakt).
                let given = d.init.len() as u32;
                if given < len && !(last_zero && !text) {
                    self.fill(base, len, given, text);
                }
            }
        }
        Ok(())
    }

    /// Copies element `from - 1` into `from..len`.
    pub(super) fn fill(&mut self, base: u32, len: u32, from: u32, text: bool) {
        let last = base + from - 1;
        let (load, store) = if text {
            (Op::TLoad(last), Op::TStoreAt { base, len })
        } else {
            (Op::Load(last), Op::StoreAt { base, len })
        };
        if len - from <= UNROLL {
            for i in from..len {
                self.emit(Op::Int(i as i32));
                self.emit(load);
                self.emit(store);
            }
            return;
        }
        let i = self.scratch_poly();
        self.emit(Op::Int(from as i32));
        self.emit(Op::StorePoly(i));
        let top = self.here();
        self.emit(Op::LoadPoly(i));
        self.emit(Op::Int(len as i32));
        self.emit(Op::ICmp(Cmp::Lt));
        let done = self.here();
        self.emit(Op::JumpIfZero(0));
        self.emit(Op::LoadPoly(i));
        self.emit(load);
        self.emit(store);
        self.emit(Op::LoadPoly(i));
        self.emit(Op::Int(1));
        self.emit(Op::IAdd);
        self.emit(Op::StorePoly(i));
        self.emit(Op::Jump(top));
        let end = self.here();
        self.patch(done, end);
    }

    /// A knob, slider or value edit whose range comes from variables gets it
    /// set where the declaration runs.
    pub(super) fn control_params(&mut self, d: &Declare<'a>, control: u32) -> Result<(), Error> {
        const RANGE: [&str; 2] = ["$CONTROL_PAR_MIN_VALUE", "$CONTROL_PAR_MAX_VALUE"];
        let kind = self.controls.get(control as usize).map(|c| c.kind);
        let ranged = matches!(
            kind,
            Some(ControlKind::Knob | ControlKind::Slider | ControlKind::ValueEdit)
        );
        for (i, p) in d.params.iter().enumerate() {
            if self.fold_int(p).is_ok() {
                continue;
            }
            let Some(par) = RANGE.get(i).filter(|_| ranged) else {
                return Err(Error::new(p.span, "expected a constant"));
            };
            let par = self.symbol(par);
            let variable = self
                .controls
                .get(control as usize)
                .map_or(0, |c| c.variable);
            self.emit(Op::Int(super::ui_id(variable as usize)));
            self.emit(Op::Int(par));
            let got = self.expr(p)?;
            self.convert(got, Ty::Int, p.span)?;
            self.host(Action::SetControlPar, 3, 0, Returns::Nothing);
        }
        Ok(())
    }

    // ---- Constant folding (sizes, constants, UI parameters) ----

    pub(super) fn fold_int(&mut self, e: &Expr<'a>) -> Result<i32, Error> {
        match self.fold(e)? {
            Value::Int(n) => Ok(n),
            _ => Err(Error::new(e.span, "expected an integer constant")),
        }
    }

    pub(super) fn fold(&mut self, e: &Expr<'a>) -> Result<Value, Error> {
        let not_constant = || Error::new(e.span, "expected a constant");
        Ok(match &e.kind {
            ExprKind::Int(n) => Value::Int(int_literal(*n, e.span)?),
            ExprKind::Real(n) => Value::Real(*n),
            ExprKind::Var(name, None) => match self.named_constant(name) {
                Some(value) => value,
                None if consts::is_catalog(name) && Builtin::from_name(name).is_none() => {
                    Value::Int(self.symbol(name))
                }
                None => return Err(not_constant()),
            },
            ExprKind::Unary(Un::Neg, inner) => match (&inner.kind, self.fold(inner)?) {
                (ExprKind::Int(n), _) => Value::Int(int_literal(*n, e.span)?.wrapping_neg()),
                (_, Value::Int(n)) => Value::Int(n.wrapping_neg()),
                (_, Value::Real(n)) => Value::Real(-n),
                _ => return Err(not_constant()),
            },
            ExprKind::Unary(Un::BitNot, inner) => match self.fold(inner)? {
                Value::Int(n) => Value::Int(!n),
                _ => return Err(not_constant()),
            },
            ExprKind::Unary(Un::Not, inner) => match self.fold(inner)? {
                Value::Bool(b) => Value::Bool(!b),
                _ => return Err(not_constant()),
            },
            ExprKind::Binary(op, a, b) => {
                let (a, b) = (self.fold(a)?, self.fold(b)?);
                fold_binary(*op, a, b).ok_or_else(not_constant)?
            }
            ExprKind::Call(name, args) => {
                let mut values = Vec::with_capacity(args.len());
                for a in args {
                    values.push(self.fold(a)?);
                }
                fold_call(name, &values).ok_or_else(not_constant)?
            }
            _ => return Err(not_constant()),
        })
    }

    pub(super) fn named_constant(&self, name: &str) -> Option<Value> {
        if let Some(&v) = self.names.get(&name.to_ascii_lowercase())
            && let Some(Var {
                home: Home::Const(value),
                ..
            }) = self.vars.get(v)
        {
            return Some(*value);
        }
        consts::int(name)
            .map(Value::Int)
            .or_else(|| consts::real(name).map(Value::Real))
    }
}

fn control_kind(word: &str) -> Option<ControlKind> {
    Some(match word {
        "ui_button" => ControlKind::Button,
        "ui_knob" => ControlKind::Knob,
        "ui_menu" => ControlKind::Menu,
        "ui_value_edit" => ControlKind::ValueEdit,
        "ui_label" => ControlKind::Label,
        "ui_table" => ControlKind::Table,
        "ui_waveform" => ControlKind::Waveform,
        "ui_wavetable" => ControlKind::Wavetable,
        "ui_slider" => ControlKind::Slider,
        "ui_text_edit" => ControlKind::TextEdit,
        "ui_file_selector" => ControlKind::FileSelector,
        "ui_switch" => ControlKind::Switch,
        "ui_xy" => ControlKind::Xy,
        "ui_level_meter" => ControlKind::LevelMeter,
        "ui_mouse_area" => ControlKind::MouseArea,
        "ui_panel" => ControlKind::Panel,
        _ => return None,
    })
}

fn fold_call(name: &str, args: &[Value]) -> Option<Value> {
    use Value::{Int, Real};
    Some(match (Builtin::from_name(name)?, args) {
        (Builtin::ShLeft, [Int(a), Int(n)]) => Int(a.wrapping_shl(*n as u32)),
        (Builtin::ShRight, [Int(a), Int(n)]) => Int(a.wrapping_shr(*n as u32)),
        (Builtin::Abs, [Int(a)]) => Int(a.wrapping_abs()),
        (Builtin::Min, [Int(a), Int(b)]) => Int(*a.min(b)),
        (Builtin::Max, [Int(a), Int(b)]) => Int(*a.max(b)),
        (Builtin::IntToReal | Builtin::Real, [Int(a)]) => Real(f64::from(*a)),
        (Builtin::RealToInt | Builtin::Int, [Real(a)]) => Int(*a as i32),
        _ => return None,
    })
}

fn fold_binary(op: Bin, a: Value, b: Value) -> Option<Value> {
    use Value::{Bool, Int, Real};
    Some(match (op, a, b) {
        (Bin::Add, Int(a), Int(b)) => Int(a.wrapping_add(b)),
        (Bin::Sub, Int(a), Int(b)) => Int(a.wrapping_sub(b)),
        (Bin::Mul, Int(a), Int(b)) => Int(a.wrapping_mul(b)),
        (Bin::Div, Int(a), Int(b)) => Int(a.checked_div(b)?),
        (Bin::Mod, Int(a), Int(b)) => Int(a.checked_rem(b)?),
        (Bin::BitAnd, Int(a), Int(b)) => Int(a & b),
        (Bin::BitOr, Int(a), Int(b)) => Int(a | b),
        (Bin::BitXor, Int(a), Int(b)) => Int(a ^ b),
        (Bin::Add, Real(a), Real(b)) => Real(a + b),
        (Bin::Sub, Real(a), Real(b)) => Real(a - b),
        (Bin::Mul, Real(a), Real(b)) => Real(a * b),
        (Bin::Div, Real(a), Real(b)) => Real(a / b),
        (Bin::And, Bool(a), Bool(b)) => Bool(a && b),
        (Bin::Or, Bool(a), Bool(b)) => Bool(a || b),
        (Bin::Xor, Bool(a), Bool(b)) => Bool(a != b),
        (Bin::Eq, Int(a), Int(b)) => Bool(a == b),
        (Bin::Ne, Int(a), Int(b)) => Bool(a != b),
        (Bin::Lt, Int(a), Int(b)) => Bool(a < b),
        (Bin::Gt, Int(a), Int(b)) => Bool(a > b),
        (Bin::Le, Int(a), Int(b)) => Bool(a <= b),
        (Bin::Ge, Int(a), Int(b)) => Bool(a >= b),
        _ => return None,
    })
}
