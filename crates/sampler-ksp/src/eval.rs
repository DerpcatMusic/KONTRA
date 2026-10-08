//! Control-thread evaluation of `on init` (and functions it calls). Produces the
//! initial script state and the plain-data interface model. Bounded by fuel.
use crate::builtins::{self as b, Builtin};
use crate::diag::{Fault, Result, Span, fault};
use crate::hir::*;
use crate::model::{self, Key, KeyRange, MenuItem, Request, Value};
use std::collections::{BTreeMap, BTreeSet, HashMap};

/// Instrument facts the script may query while initializing.
#[derive(Clone, Debug, Default)]
pub struct Environment {
    /// Optional control-thread evaluation budget; default is the library profile.
    pub evaluation_budget: Option<u64>,
    /// Group names in instrument order (`$NUM_GROUPS`, `group_name`, `find_group`).
    pub groups: Vec<String>,
    /// Saved values by variable name, applied by `read_persistent_var`.
    pub persisted: BTreeMap<String, Value>,
    /// Host-saved semantic control values. Unlike Kontakt's menu persistence,
    /// these are values, never menu-item positions.
    pub control_values: BTreeMap<sampler_core::ControlId, Value>,
    /// Saved array contents by variable name (`%a`, `?r`); a longer save is
    /// cut to the declared length, a shorter one restores a prefix.
    pub persisted_arrays: BTreeMap<String, Vec<Value>>,
    /// Script slot (`$CURRENT_SCRIPT_SLOT`); also namespaces derived control ids.
    pub slot: u8,
    pub engine_values: BTreeMap<[i32; 4], i32>,
    pub engine_lookups: Vec<sampler_core::EngineLookup>,
    /// The Creator Tools performance view (`.nckp`, see [`crate::nckp`]) the
    /// script loads with `load_performance_view`. Names the script uses but
    /// it lacks are assumed (see `PerformanceControl::assumed`), with a warning.
    pub performance_view: model::PerformanceView,
}

/// Steps one `on init` may take before evaluation is abandoned.
pub const INIT_FUEL: u64 = 200_000_000;

pub fn int_arith(op: Arith, a: i32, b: i32) -> i32 {
    use sampler_core::IntegerBinary as I;
    match op {
        Arith::Add => I::Add.apply(a, b),
        Arith::Sub => I::Subtract.apply(a, b),
        Arith::Mul => I::Multiply.apply(a, b),
        Arith::Div => I::Divide.apply(a, b),
        Arith::Mod => I::Remainder.apply(a, b),
        Arith::BitAnd => a & b,
        Arith::BitOr => a | b,
        Arith::BitXor => a ^ b,
    }
}
pub fn real_arith(op: Arith, a: f64, b: f64) -> Option<f64> {
    Some(match op {
        Arith::Add => a + b,
        Arith::Sub => a - b,
        Arith::Mul => a * b,
        Arith::Div => a / b,
        _ => return None,
    })
}
pub fn compare_real(c: sampler_core::Comparison, a: f64, b: f64) -> bool {
    use sampler_core::Comparison::*;
    match c {
        Equal => a == b,
        NotEqual => a != b,
        Less => a < b,
        LessEqual => a <= b,
        Greater => a > b,
        GreaterEqual => a >= b,
    }
}
/// Truncating, saturating; NaN is 0.
pub fn real_to_int(x: f64) -> i32 {
    x.trunc() as i32
}
/// ponytail: Kontakt's exact real formatting is undocumented; shortest
/// round-trip text with a trailing `.0` for integral values.
pub fn real_text(x: f64) -> String {
    if x.is_finite() && x.fract() == 0.0 && x.abs() < 1e15 {
        format!("{x:.1}")
    } else {
        format!("{x}")
    }
}

#[derive(Clone, Debug)]
enum V {
    I(i32),
    R(f64),
    S(String),
}
impl V {
    fn int(&self) -> i32 {
        match self {
            V::I(n) => *n,
            V::R(r) => real_to_int(*r),
            V::S(_) => 0,
        }
    }
    fn real(&self) -> f64 {
        match self {
            V::R(r) => *r,
            V::I(n) => f64::from(*n),
            V::S(_) => 0.0,
        }
    }
    fn text(self) -> String {
        match self {
            V::S(s) => s,
            V::I(n) => n.to_string(),
            V::R(r) => real_text(r),
        }
    }
    fn value(self) -> Value {
        match self {
            V::I(n) => Value::Int(n),
            V::R(r) => Value::Real(r),
            V::S(s) => Value::Text(s),
        }
    }
}

/// Initial state after `on init`.
pub struct Initial {
    pub cells: Vec<i64>,
    pub texts: Vec<String>,
    /// Values of host-owned controls, by UI index.
    pub controls: Vec<i32>,
    pub model: model::Model,
    /// Positioned non-fatal problems (Kontakt reports these and continues).
    pub warnings: Vec<Fault>,
    /// `set_engine_par` writes keyed `(parameter, group, slot, generic)`.
    pub engine: HashMap<[i32; 4], i32>,
    /// Integer `$CONTROL_PAR_*` mirror keyed `(ui id, parameter)`.
    pub properties: HashMap<(i32, i32), i32>,
    pub text_properties: HashMap<(i32, i32), String>,
    /// `set_control_par*_arr` writes keyed `(ui id, parameter, index)`.
    pub indexed_properties: BTreeMap<(i32, i32, i32), Value>,
}

enum Flow {
    Next,
    Exit,
    Continue,
}

const MAX_TEXT_LINES: i32 = 1 << 16;

struct Eval<'h> {
    hir: &'h Hir,
    env: &'h Environment,
    st: Initial,
    fuel: u64,
    depth: usize,
    consumed: BTreeSet<VarId>,
    pending_menus: BTreeMap<usize, i32>,
    callback_type: i32,
}

pub fn run(hir: &Hir, env: &Environment) -> Result<Initial> {
    let mut model = model::Model::default();
    model.interface.keys = vec![Key::default(); 128];
    let mut e = Eval {
        hir,
        env,
        st: Initial {
            cells: vec![0; hir.cells as usize],
            texts: vec![String::new(); hir.texts as usize],
            controls: vec![0; hir.uis.len()],
            model,
            warnings: Vec::new(),
            engine: HashMap::new(),
            properties: HashMap::new(),
            text_properties: HashMap::new(),
            indexed_properties: BTreeMap::new(),
        },
        fuel: env.evaluation_budget.unwrap_or(INIT_FUEL),
        depth: 0,
        consumed: BTreeSet::new(),
        pending_menus: BTreeMap::new(),
        callback_type: b::cb::INIT,
    };
    // Kontakt's defaults: knobs/sliders start at their minimum when 0 is outside.
    for (index, ui) in hir.uis.iter().enumerate() {
        if let Some((lo, hi)) = declared_range(ui) {
            e.st.controls[index] = 0.clamp(lo.min(hi), hi.max(lo));
        }
    }
    if let Some(init) = hir.callbacks.iter().find(|c| c.kind == CallbackKind::Init) {
        e.block(&init.body)?;
    }
    // On load Kontakt restores saved persistent values, then runs
    // `on persistence_changed`, before the interface is shown.
    for (i, var) in hir.vars.iter().enumerate() {
        if !e.consumed.contains(&VarId(i as u32))
            && (var.persistence != Persistence::None
                || matches!(var.home, Home::Control(_))
                    && e.env
                        .control_values
                        .contains_key(&crate::derived_control_id(e.env.slot, &var.name)))
        {
            e.restore(VarId(i as u32));
        }
    }
    e.callback_type = b::cb::PERSISTENCE_CHANGED;
    if let Some(cb) = hir
        .callbacks
        .iter()
        .find(|c| c.kind == CallbackKind::PersistenceChanged)
    {
        e.st.model.persistence_completion = match e.block(&cb.body) {
            Ok(_) => model::PersistenceCompletion::Completed,
            Err(f) => {
                let category = if e.fuel == 0 {
                    model::EvaluationFailure::Budget
                } else {
                    model::EvaluationFailure::InvalidValue
                };
                e.warn(f.span, "on persistence_changed did not complete".to_owned());
                model::PersistenceCompletion::Failed {
                    category,
                    offset: f.span.start,
                    builtin: f.builtin,
                }
            }
        };
    }
    Ok(e.st)
}

/// Declared value range of a host-owned control.
pub fn declared_range(ui: &Ui) -> Option<(i32, i32)> {
    match ui.kind {
        WidgetKind::Button | WidgetKind::Switch => Some((0, 1)),
        WidgetKind::Knob | WidgetKind::Slider | WidgetKind::ValueEdit => {
            Some((*ui.params.first()?, *ui.params.get(1)?))
        }
        WidgetKind::Menu => Some((i32::MIN, i32::MAX)),
        _ => None,
    }
}

impl Eval<'_> {
    fn warn(&mut self, span: Span, message: impl Into<String>) {
        if self.st.warnings.len() < 1000 {
            self.st.warnings.push(Fault {
                span,
                builtin: None,
                message: message.into(),
            });
        }
    }

    fn block(&mut self, body: &[Stmt]) -> Result<Flow> {
        for s in body {
            if self.fuel == 0 {
                return fault(s.span, "on init exceeded its evaluation budget");
            }
            self.fuel -= 1;
            match self.stmt(s)? {
                Flow::Next => {}
                other => return Ok(other),
            }
        }
        Ok(Flow::Next)
    }

    fn stmt(&mut self, s: &Stmt) -> Result<Flow> {
        match &s.kind {
            StmtKind::Assign(place, value) => {
                let value = self.expr(value)?;
                self.store(place, value, s.span)?;
            }
            StmtKind::Fill(var, values) => {
                let mut last = V::I(0);
                let len = self.hir.vars[var.0 as usize].len.unwrap_or(1) as usize;
                for i in 0..len {
                    if let Some(e) = values.get(i) {
                        last = self.expr(e)?;
                    }
                    self.write_elem(*var, i as i32, last.clone(), s.span);
                }
            }
            StmtKind::If(cond, yes, no) => {
                let branch = if self.expr(cond)?.int() != 0 { yes } else { no };
                return self.block(branch);
            }
            StmtKind::While(cond, body) => {
                while self.expr(cond)?.int() != 0 {
                    if self.fuel == 0 {
                        return fault(s.span, "on init exceeded its evaluation budget");
                    }
                    self.fuel -= 1;
                    match self.block(body)? {
                        Flow::Exit => return Ok(Flow::Exit),
                        Flow::Next | Flow::Continue => {}
                    }
                }
            }
            StmtKind::Select(value, cases) => {
                let v = self.expr(value)?.int();
                if let Some(case) = cases.iter().find(|c| (c.low..=c.high).contains(&v)) {
                    return self.block(&case.body);
                }
            }
            StmtKind::Call(f) => {
                self.depth += 1;
                let flow = self.block(&self.hir.functions[f.0 as usize].body);
                self.depth -= 1;
                if let Flow::Exit = flow? {
                    return Ok(Flow::Exit);
                }
            }
            StmtKind::Builtin(Builtin::Exit, _) => return Ok(Flow::Exit),
            StmtKind::Builtin(Builtin::Continue, _) => return Ok(Flow::Continue),
            StmtKind::Builtin(builtin, args) => {
                self.builtin(*builtin, args, s.span)?;
            }
        }
        Ok(Flow::Next)
    }

    fn read_var(&self, var: VarId) -> V {
        let v = &self.hir.vars[var.0 as usize];
        let from_bits = |bits: i64| match v.ty {
            Ty::Real => V::R(f64::from_bits(bits as u64)),
            _ => V::I(bits as i32),
        };
        match v.home {
            Home::Cell(c) => from_bits(self.st.cells[c as usize]),
            Home::Text(t) => V::S(self.st.texts[t as usize].clone()),
            Home::Control(ui) => V::I(self.st.controls[ui as usize]),
            Home::Const(c) => match &self.hir.consts[c as usize] {
                Const::Int(n) => V::I(*n),
                Const::Real(r) => V::R(*r),
                Const::Str(s) => V::S(s.to_string()),
            },
            Home::Note(_) => from_bits(0),
            Home::Cells { .. } | Home::Texts { .. } => V::I(0),
        }
    }

    fn read_elem(&mut self, var: VarId, index: i32, span: Span) -> V {
        let v = &self.hir.vars[var.0 as usize];
        let len = v.len.unwrap_or(0);
        let Some(i) = u32::try_from(index).ok().filter(|i| *i < len) else {
            self.warn(span, format!("array index {index} out of bounds"));
            return if v.ty == Ty::Str {
                V::S(String::new())
            } else if v.ty == Ty::Real {
                V::R(0.0)
            } else {
                V::I(0)
            };
        };
        match v.home {
            Home::Cells { offset, .. } => {
                let bits = self.st.cells[(offset + i) as usize];
                if v.ty == Ty::Real {
                    V::R(f64::from_bits(bits as u64))
                } else {
                    V::I(bits as i32)
                }
            }
            Home::Texts { offset, .. } => V::S(self.st.texts[(offset + i) as usize].clone()),
            _ => V::I(0),
        }
    }

    /// Applies the saved value of a persistent variable, if the file has one.
    fn restore(&mut self, var: VarId) {
        let v = &self.hir.vars[var.0 as usize];
        let conv = |value: &Value| match value {
            Value::Int(n) => V::I(*n),
            Value::Real(r) => V::R(*r),
            Value::Text(s) => V::S(s.clone()),
        };
        if v.len.is_none() {
            let host = matches!(v.home, Home::Control(_))
                .then(|| {
                    self.env
                        .control_values
                        .get(&crate::derived_control_id(self.env.slot, &v.name))
                })
                .flatten();
            if let Some(saved) = host.or_else(|| self.env.persisted.get(&*v.name)) {
                let mut value = conv(saved);
                // A menu is saved as its selected item's position; the
                // variable holds that item's value (Una Corda's velocity
                // menu stores 5 for "Linear", whose value is 0).
                if host.is_none()
                    && let Home::Control(ui) = v.home
                    && self.hir.uis[ui as usize].kind == WidgetKind::Menu
                {
                    let ui = ui as usize;
                    let index = value.int();
                    let items = self.menu(ui);
                    if items.is_empty() {
                        self.pending_menus.insert(ui, index);
                        value = V::I(0);
                    } else {
                        value = V::I(
                            usize::try_from(index)
                                .ok()
                                .and_then(|i| items.get(i))
                                .or_else(|| items.first())
                                .map_or(0, |item| item.value),
                        );
                    }
                }
                self.write_var(var, value);
            }
        } else if let Some(saved) = self.env.persisted_arrays.get(&*v.name) {
            let len = v.len.unwrap_or(0) as usize;
            let mut values: Vec<V> = saved.iter().take(len).map(conv).collect();
            // Kontakt stores an ordinary array only up to its last change; the
            // cells after the stored prefix take the last stored value. UI
            // tables and XY pads store every cell.
            if matches!(v.home, Home::Cells { .. })
                && let Some(last) = values.last().cloned()
            {
                values.resize(len, last);
            }
            let span = v.span;
            for (i, value) in values.into_iter().enumerate() {
                self.write_elem(var, i as i32, value, span);
            }
        }
    }

    fn write_var(&mut self, var: VarId, value: V) {
        let v = &self.hir.vars[var.0 as usize];
        match v.home {
            Home::Cell(c) => {
                self.st.cells[c as usize] = match v.ty {
                    Ty::Real => value.real().to_bits() as i64,
                    _ => i64::from(value.int()),
                }
            }
            Home::Text(t) => self.st.texts[t as usize] = value.text(),
            Home::Control(ui) => {
                let mut n = value.int();
                if let Some((lo, hi)) = declared_range(&self.hir.uis[ui as usize]) {
                    let clamped = n.clamp(lo.min(hi), hi.max(lo));
                    if clamped != n {
                        let span = v.span;
                        self.warn(span, format!("{} clamped to its declared range", v.name));
                    }
                    n = clamped;
                }
                self.st.controls[ui as usize] = n;
            }
            Home::Note(_) => {
                let span = v.span;
                self.warn(
                    span,
                    "polyphonic variables have no value in on init; write ignored",
                );
            }
            _ => {}
        }
    }

    fn write_elem(&mut self, var: VarId, index: i32, value: V, span: Span) {
        let v = &self.hir.vars[var.0 as usize];
        let len = v.len.unwrap_or(0);
        let Some(i) = u32::try_from(index).ok().filter(|i| *i < len) else {
            self.warn(span, format!("array index {index} out of bounds"));
            return;
        };
        match v.home {
            Home::Cells { offset, .. } => {
                self.st.cells[(offset + i) as usize] = match v.ty {
                    Ty::Real => value.real().to_bits() as i64,
                    _ => i64::from(value.int()),
                }
            }
            Home::Texts { offset, .. } => self.st.texts[(offset + i) as usize] = value.text(),
            _ => {}
        }
    }

    fn store(&mut self, place: &Place, value: V, span: Span) -> Result<()> {
        match place {
            Place::Var(v) => self.write_var(*v, value),
            Place::Elem(v, i) => {
                let i = self.expr(i)?.int();
                self.write_elem(*v, i, value, span);
            }
        }
        Ok(())
    }

    fn expr(&mut self, e: &Expr) -> Result<V> {
        Ok(match &e.kind {
            ExprKind::Int(n) => V::I(*n),
            ExprKind::Real(r) => V::R(*r),
            ExprKind::Str(s) => V::S(s.to_string()),
            ExprKind::Load(v) => self.read_var(*v),
            ExprKind::LoadElem(v, i) => {
                let i = self.expr(i)?.int();
                self.read_elem(*v, i, e.span)
            }
            ExprKind::Sys(sys) => {
                use b::SysVar::*;
                if matches!(sys, EventId | EventNote | EventVelocity | NoteHeld | CcNum) {
                    self.warn(e.span, format!("{sys:?} has no event in on init; reads 0"));
                }
                V::I(self.sys(*sys))
            }
            ExprKind::SysElem(array, i) => {
                let i = self.expr(i)?.int();
                if !(0..array.len() as i32).contains(&i) {
                    self.warn(e.span, format!("array index {i} out of bounds"));
                }
                V::I(0)
            }
            ExprKind::Neg(inner) => match self.expr(inner)? {
                V::R(r) => V::R(-r),
                v => V::I(v.int().wrapping_neg()),
            },
            ExprKind::BitNot(inner) => V::I(!self.expr(inner)?.int()),
            ExprKind::Not(inner) => V::I(i32::from(self.expr(inner)?.int() == 0)),
            ExprKind::Cast(inner) => {
                let v = self.expr(inner)?.int();
                V::I(if e.ty == Ty::Bool {
                    i32::from(v != 0)
                } else {
                    v
                })
            }
            ExprKind::Arith(op, l, r) => {
                let (l, r) = (self.expr(l)?, self.expr(r)?);
                match (l, r) {
                    (V::R(a), V::R(b)) => V::R(real_arith(*op, a, b).unwrap_or(0.0)),
                    (a, b) => V::I(int_arith(*op, a.int(), b.int())),
                }
            }
            ExprKind::Compare(c, l, r) => {
                let (l, r) = (self.expr(l)?, self.expr(r)?);
                V::I(i32::from(match (l, r) {
                    (V::R(a), V::R(b)) => compare_real(*c, a, b),
                    (V::S(a), V::S(b)) => match c {
                        sampler_core::Comparison::Equal => a == b,
                        sampler_core::Comparison::NotEqual => a != b,
                        _ => false,
                    },
                    (a, b) => c.apply(i64::from(a.int()), i64::from(b.int())),
                }))
            }
            ExprKind::Logic(op, l, r) => {
                let a = self.expr(l)?.int() != 0;
                let result = match op {
                    Logic::And => a && self.expr(r)?.int() != 0,
                    Logic::Or => a || self.expr(r)?.int() != 0,
                    Logic::Xor => a != (self.expr(r)?.int() != 0),
                };
                V::I(i32::from(result))
            }
            ExprKind::Concat(parts) => {
                let mut s = String::new();
                for p in parts {
                    s.push_str(&self.expr(p)?.text());
                }
                V::S(s)
            }
            ExprKind::Builtin(builtin, args) => self.builtin(*builtin, args, e.span)?,
        })
    }

    fn sys(&self, sys: b::SysVar) -> i32 {
        use b::SysVar::*;
        match sys {
            NumGroups => self.env.groups.len() as i32,
            CallbackType => self.callback_type,
            DurationQuarter => 500_000,
            DurationEighth => 250_000,
            DurationSixteenth => 125_000,
            DurationQuarterTriplet => 333_333,
            DurationEighthTriplet => 166_666,
            DurationSixteenthTriplet => 83_333,
            DurationBar => 2_000_000,
            SignatureNum | SignatureDenom => 4,
            Tempo => 120,
            CurrentScriptSlot => i32::from(self.env.slot),
            _ => 0,
        }
    }

    fn arg(&mut self, args: &[Arg], i: usize) -> Result<V> {
        match args.get(i) {
            Some(Arg::Expr(e)) => self.expr(e),
            Some(Arg::Key(k)) => Ok(V::S(k.to_string())),
            _ => Ok(V::I(0)),
        }
    }
    fn int(&mut self, args: &[Arg], i: usize) -> Result<i32> {
        Ok(self.arg(args, i)?.int())
    }
    fn text(&mut self, args: &[Arg], i: usize) -> Result<String> {
        Ok(self.arg(args, i)?.text())
    }
    fn var(args: &[Arg], i: usize) -> VarId {
        match &args[i] {
            Arg::Var(v, _) => *v,
            Arg::Place(p) => p.var(),
            _ => unreachable!("sema checks variable arguments"),
        }
    }

    fn ui_of(&self, var: VarId) -> Option<usize> {
        self.hir.vars[var.0 as usize].ui.map(|u| u as usize)
    }
    fn ui_index(&self, id: i32) -> Option<usize> {
        let index = usize::try_from(id.checked_sub(b::FIRST_UI_ID)?).ok()?;
        (index < self.hir.uis.len()).then_some(index)
    }
    fn set_property(&mut self, id: i32, par: i32, value: V, span: Span) {
        if par == b::CONTROL_PAR_VALUE
            && let Some(ui) = self.ui_index(id)
        {
            let var = self.hir.uis[ui].var;
            if self.hir.vars[var.0 as usize].len.is_none() {
                self.write_var(var, value);
            }
            return;
        }
        match &value {
            V::S(s) => {
                self.st.text_properties.insert((id, par), s.clone());
            }
            v => {
                self.st.properties.insert((id, par), v.int());
            }
        }
        if self.ui_index(id).is_none() && !(b::INST_ICON_ID..=b::INST_ICON_ID + 5).contains(&id) {
            self.warn(span, format!("control parameter set on unknown UI id {id}"));
        }
    }

    fn get_property(&self, id: i32, par: i32) -> V {
        if par == b::CONTROL_PAR_VALUE
            && let Some(ui) = self.ui_index(id)
        {
            return self.read_var(self.hir.uis[ui].var);
        }
        if par == b::CONTROL_PAR_TYPE
            && let Some(ui) = self.ui_index(id)
        {
            return V::I(self.hir.uis[ui].kind.control_type());
        }
        if par == b::CONTROL_PAR_NUM_ITEMS
            && let Some(ui) = self.ui_index(id)
        {
            return V::I(
                self.st
                    .model
                    .interface
                    .widgets
                    .get(ui)
                    .map_or(0, |w| w.menu.len()) as i32,
            );
        }
        if let Some(n) = self.st.properties.get(&(id, par)) {
            return V::I(*n);
        }
        if let Some(s) = self.st.text_properties.get(&(id, par)) {
            return V::S(s.clone());
        }
        if let Some(ui) = self.ui_index(id) {
            let range = declared_range(&self.hir.uis[ui]);
            match par {
                b::CONTROL_PAR_MIN_VALUE => return V::I(range.map_or(0, |r| r.0)),
                b::CONTROL_PAR_MAX_VALUE => return V::I(range.map_or(0, |r| r.1)),
                _ => {}
            }
        }
        V::I(0)
    }

    /// The view's page settings and control properties, as if the script
    /// had set them right after `load_performance_view`.
    fn apply_performance_view(&mut self, span: Span) {
        let view = &self.env.performance_view;
        let ui = &mut self.st.model.interface;
        ui.width_px = view.width.or(ui.width_px);
        ui.height_px = view.height.or(ui.height_px);
        if let Some(color) = view.color {
            self.st.model.requests.push(Request {
                command: "set_ui_color",
                args: vec![Value::Int(color)],
            });
        }
        for (id, picture) in [
            (b::INST_WALLPAPER_ID, &view.wallpaper),
            (b::INST_ICON_ID, &view.icon),
        ] {
            if let Some(p) = picture {
                self.set_property(id, b::CONTROL_PAR_PICTURE, V::S(p.clone()), span);
            }
        }
        let ui_id = |name: &str| {
            let var = self
                .hir
                .vars
                .iter()
                .position(|v| v.ui.is_some() && *v.name == *name)?;
            Some(b::FIRST_UI_ID + self.hir.vars[var].ui? as i32)
        };
        for c in &view.controls {
            let Some(id) = ui_id(&c.name) else { continue };
            for (name, value) in &c.properties {
                let Some(par) = b::control_par(name) else {
                    self.warn(
                        span,
                        format!("{}: unknown performance view property {name}", c.name),
                    );
                    continue;
                };
                let value = match (par, value) {
                    (b::CONTROL_PAR_PARENT_PANEL, Value::Text(panel)) => match ui_id(panel) {
                        Some(p) => V::I(p),
                        None => continue,
                    },
                    (_, Value::Int(n)) => V::I(*n),
                    (_, Value::Real(r)) => V::R(*r),
                    (_, Value::Text(t)) => V::S(t.clone()),
                };
                self.set_property(id, par, value, span);
            }
            if !c.menu.is_empty() {
                let ui = (id - b::FIRST_UI_ID) as usize;
                self.menu(ui).extend(c.menu.iter().cloned());
            }
        }
    }

    fn menu(&mut self, ui: usize) -> &mut Vec<MenuItem> {
        let widgets = &mut self.st.model.interface.widgets;
        if widgets.len() <= ui {
            widgets.resize_with(ui + 1, placeholder);
        }
        &mut widgets[ui].menu
    }

    fn request(&mut self, builtin: Builtin, args: &[Arg]) -> Result<()> {
        let mut values = Vec::with_capacity(args.len());
        for i in 0..args.len() {
            let v = match &args[i] {
                Arg::Var(v, _) => V::S(self.hir.vars[v.0 as usize].name.to_string()),
                // Engine parameters are recorded by name: hashed ids are unreadable.
                _ if i == 0 && builtin == Builtin::SetEnginePar => {
                    let id = self.int(args, 0)?;
                    symbol_name(self.hir, id).map_or(V::I(id), V::S)
                }
                _ => self.arg(args, i)?,
            };
            values.push(v.value());
        }
        self.st.model.requests.push(Request {
            command: builtin.name(),
            args: values,
        });
        Ok(())
    }

    fn builtin(&mut self, builtin: Builtin, args: &[Arg], span: Span) -> Result<V> {
        use Builtin::*;
        if let Some(Arg::SysArray(array)) = args.first() {
            // Runtime-maintained arrays are all zero while initializing.
            return Ok(match builtin {
                NumElements => V::I(array.len() as i32),
                Search => V::I(if self.int(args, 1)? == 0 { 0 } else { -1 }),
                _ => V::I(0),
            });
        }
        let real = |x: f64| V::R(x);
        Ok(match builtin {
            Exit | Continue => V::I(0),
            Inc | Dec => {
                let Arg::Place(place) = &args[0] else {
                    unreachable!()
                };
                let current = match place {
                    Place::Var(v) => self.read_var(*v),
                    Place::Elem(v, i) => {
                        let i = self.expr(i)?.int();
                        self.read_elem(*v, i, span)
                    }
                };
                let delta = if builtin == Inc { 1 } else { -1 };
                self.store(place, V::I(current.int().wrapping_add(delta)), span)?;
                V::I(0)
            }
            Abs => match self.arg(args, 0)? {
                V::R(r) => V::R(r.abs()),
                v => V::I(v.int().wrapping_abs()),
            },
            Min | Max => match (self.arg(args, 0)?, self.arg(args, 1)?) {
                (V::R(a), V::R(b)) => V::R(if builtin == Min { a.min(b) } else { a.max(b) }),
                (a, b) => V::I(if builtin == Min {
                    a.int().min(b.int())
                } else {
                    a.int().max(b.int())
                }),
            },
            InRange => {
                let (x, lo, hi) = (self.arg(args, 0)?, self.arg(args, 1)?, self.arg(args, 2)?);
                V::I(i32::from(match x {
                    V::R(x) => lo.real() <= x && x <= hi.real(),
                    x => lo.int() <= x.int() && x.int() <= hi.int(),
                }))
            }
            Sgn => match self.arg(args, 0)? {
                V::R(r) => V::I(if r > 0.0 {
                    1
                } else if r < 0.0 {
                    -1
                } else {
                    0
                }),
                v => V::I(v.int().signum()),
            },
            Signbit => match self.arg(args, 0)? {
                V::R(r) => V::I(i32::from(r.is_sign_negative())),
                v => V::I(i32::from(v.int() < 0)),
            },
            ShLeft => {
                let (a, n) = (self.int(args, 0)?, self.int(args, 1)?);
                V::I(a.wrapping_shl(n as u32))
            }
            ShRight => {
                let (a, n) = (self.int(args, 0)?, self.int(args, 1)?);
                V::I(a.wrapping_shr(n as u32))
            }
            Random => {
                // Deterministic during init; scripts only see an in-range value.
                let (lo, hi) = (self.int(args, 0)?, self.int(args, 1)?);
                V::I(lo.min(hi))
            }
            IntToReal | Real => V::R(f64::from(self.int(args, 0)?)),
            RealToInt | Int => V::I(real_to_int(self.arg(args, 0)?.real())),
            Round => real(self.arg(args, 0)?.real().round()),
            Floor => real(self.arg(args, 0)?.real().floor()),
            Ceil => real(self.arg(args, 0)?.real().ceil()),
            Sqrt => real(self.arg(args, 0)?.real().sqrt()),
            Cbrt => real(self.arg(args, 0)?.real().cbrt()),
            Exp => real(self.arg(args, 0)?.real().exp()),
            Exp2 => real(self.arg(args, 0)?.real().exp2()),
            Log => real(self.arg(args, 0)?.real().ln()),
            Log2 => real(self.arg(args, 0)?.real().log2()),
            Log10 => real(self.arg(args, 0)?.real().log10()),
            Sin => real(self.arg(args, 0)?.real().sin()),
            Cos => real(self.arg(args, 0)?.real().cos()),
            Tan => real(self.arg(args, 0)?.real().tan()),
            Asin => real(self.arg(args, 0)?.real().asin()),
            Acos => real(self.arg(args, 0)?.real().acos()),
            Atan => real(self.arg(args, 0)?.real().atan()),
            Pow => {
                let (x, e) = (self.arg(args, 0)?.real(), self.arg(args, 1)?.real());
                real(x.powf(e))
            }
            Msb => V::I((self.int(args, 0)? >> 7) & 127),
            Lsb => V::I(self.int(args, 0)? & 127),
            MsToTicks => V::I((i64::from(self.int(args, 0)?) * 960 / 500_000) as i32),
            TicksToMs => V::I((i64::from(self.int(args, 0)?) * 500_000 / 960) as i32),
            NumElements => V::I(
                self.hir.vars[Self::var(args, 0).0 as usize]
                    .len
                    .unwrap_or(1) as i32,
            ),
            Search => {
                let var = Self::var(args, 0);
                let needle = self.arg(args, 1)?;
                let len = self.hir.vars[var.0 as usize].len.unwrap_or(0) as i32;
                let (lo, hi) = if args.len() == 4 {
                    (self.int(args, 2)?, self.int(args, 3)?)
                } else {
                    (0, len - 1)
                };
                let mut found = -1;
                for i in lo.max(0)..=hi.min(len - 1) {
                    let item = self.read_elem(var, i, span);
                    let hit = match (&item, &needle) {
                        (V::R(a), b) => *a == b.real(),
                        (V::S(a), b) => *a == b.clone().text(),
                        (a, b) => a.int() == b.int(),
                    };
                    if hit {
                        found = i;
                        break;
                    }
                }
                V::I(found)
            }
            Sort => {
                let var = Self::var(args, 0);
                let descending = self.int(args, 1)? != 0;
                let len = self.hir.vars[var.0 as usize].len.unwrap_or(0) as i32;
                let (lo, hi) = if args.len() == 4 {
                    (self.int(args, 2)?.max(0), self.int(args, 3)?.min(len - 1))
                } else {
                    (0, len - 1)
                };
                if lo <= hi {
                    let mut items: Vec<V> =
                        (lo..=hi).map(|i| self.read_elem(var, i, span)).collect();
                    items.sort_by(|a, b| match (a, b) {
                        (V::R(a), V::R(b)) => a.total_cmp(b),
                        (V::S(a), V::S(b)) => a.cmp(b),
                        (a, b) => a.int().cmp(&b.int()),
                    });
                    if descending {
                        items.reverse();
                    }
                    for (i, v) in (lo..).zip(items) {
                        self.write_elem(var, i, v, span);
                    }
                }
                V::I(0)
            }
            ArrayEqual => {
                let (a, b2) = (Self::var(args, 0), Self::var(args, 1));
                let len = self.hir.vars[a.0 as usize].len.unwrap_or(0);
                let equal = len == self.hir.vars[b2.0 as usize].len.unwrap_or(0)
                    && (0..len as i32).all(|i| {
                        format!("{:?}", self.read_elem(a, i, span))
                            == format!("{:?}", self.read_elem(b2, i, span))
                    });
                V::I(i32::from(equal))
            }
            GetUiId => V::I(b::FIRST_UI_ID + self.ui_of(Self::var(args, 0)).unwrap_or(0) as i32),
            SetControlPar | SetControlParReal | SetControlParStr => {
                let (id, par, value) = (self.int(args, 0)?, self.int(args, 1)?, self.arg(args, 2)?);
                self.set_property(id, par, value, span);
                V::I(0)
            }
            SetControlParArr | SetControlParStrArr | SetControlParRealArr => {
                let (id, par, value, index) = (
                    self.int(args, 0)?,
                    self.int(args, 1)?,
                    self.arg(args, 2)?,
                    self.int(args, 3)?,
                );
                // Text lines past 64 Ki are never shown; Conflux fills nine
                // controls with a million lines each (~700 MB of model).
                if !(builtin == SetControlParStrArr && index >= MAX_TEXT_LINES) {
                    if par == b::CONTROL_PAR_VALUE
                        && let Some(ui) = self.ui_index(id)
                    {
                        self.write_elem(self.hir.uis[ui].var, index, value.clone(), span);
                    }
                    self.st
                        .indexed_properties
                        .insert((id, par, index), value.value());
                }
                V::I(0)
            }
            GetControlPar | GetControlParReal => {
                let (id, par) = (self.int(args, 0)?, self.int(args, 1)?);
                match self.get_property(id, par) {
                    V::S(_) => V::I(0),
                    v if builtin == GetControlParReal => V::R(v.real()),
                    v => v,
                }
            }
            GetControlParStr => {
                let (id, par) = (self.int(args, 0)?, self.int(args, 1)?);
                V::S(self.get_property(id, par).text())
            }
            GetControlParArr | GetControlParRealArr | GetControlParStrArr => {
                let key = (self.int(args, 0)?, self.int(args, 1)?, self.int(args, 2)?);
                match (self.st.indexed_properties.get(&key), builtin) {
                    (Some(Value::Text(s)), GetControlParStrArr) => V::S(s.clone()),
                    (Some(Value::Int(n)), GetControlParArr) => V::I(*n),
                    (Some(Value::Real(r)), GetControlParRealArr) => V::R(*r),
                    (_, GetControlParStrArr) => V::S(String::new()),
                    (_, GetControlParRealArr) => V::R(0.0),
                    _ => V::I(0),
                }
            }
            SetText | SetKnobLabel | SetControlHelp | AddTextLine => {
                let ui = self.ui_of(Self::var(args, 0));
                let text = self.text(args, 1)?;
                let par = match builtin {
                    SetText | AddTextLine => b::CONTROL_PAR_TEXT,
                    SetKnobLabel => b::CONTROL_PAR_LABEL,
                    _ => b::CONTROL_PAR_HELP,
                };
                if let Some(ui) = ui {
                    let id = b::FIRST_UI_ID + ui as i32;
                    let text = if builtin == AddTextLine {
                        match self.st.text_properties.get(&(id, par)) {
                            Some(old) if !old.is_empty() => format!("{old}\n{text}"),
                            _ => text,
                        }
                    } else {
                        text
                    };
                    self.st.text_properties.insert((id, par), text);
                }
                V::I(0)
            }
            SetKnobUnit | SetKnobDefval | HidePart | SetTableStepsShown => {
                if let Some(ui) = self.ui_of(Self::var(args, 0)) {
                    let par = match builtin {
                        SetKnobUnit => b::CONTROL_PAR_UNIT,
                        SetKnobDefval => b::CONTROL_PAR_DEFAULT_VALUE,
                        HidePart => b::CONTROL_PAR_HIDE,
                        _ => TABLE_STEPS_SHOWN,
                    };
                    let value = self.int(args, 1)?;
                    self.st
                        .properties
                        .insert((b::FIRST_UI_ID + ui as i32, par), value);
                }
                V::I(0)
            }
            MoveControl | MoveControlPx => {
                if let Some(ui) = self.ui_of(Self::var(args, 0)) {
                    let id = b::FIRST_UI_ID + ui as i32;
                    let (x, y) = (self.int(args, 1)?, self.int(args, 2)?);
                    let (px, py) = if builtin == MoveControl {
                        (GRID_X, GRID_Y)
                    } else {
                        (b::CONTROL_PAR_POS_X, b::CONTROL_PAR_POS_Y)
                    };
                    self.st.properties.insert((id, px), x);
                    self.st.properties.insert((id, py), y);
                }
                V::I(0)
            }
            AddMenuItem => {
                let ui = self.ui_of(Self::var(args, 0));
                let (text, value) = (self.text(args, 1)?, self.int(args, 2)?);
                if let Some(ui) = ui {
                    self.menu(ui).push(MenuItem {
                        text,
                        value,
                        visible: true,
                    });
                    if let Some(&index) = self.pending_menus.get(&ui) {
                        let items = self.menu(ui);
                        let selected = usize::try_from(index)
                            .ok()
                            .and_then(|i| items.get(i))
                            .or_else(|| items.first())
                            .map_or(0, |item| item.value);
                        self.write_var(Self::var(args, 0), V::I(selected));
                    }
                }
                V::I(0)
            }
            SetMenuItemStr | SetMenuItemVisibility | SetMenuItemValue => {
                let (id, index) = (self.int(args, 0)?, self.int(args, 1)?);
                let value = self.arg(args, 2)?;
                match self.ui_index(id) {
                    Some(ui) => match self.menu(ui).get_mut(index as usize) {
                        Some(item) => match builtin {
                            SetMenuItemStr => item.text = value.text(),
                            SetMenuItemVisibility => item.visible = value.int() != 0,
                            _ => item.value = value.int(),
                        },
                        None => self.warn(span, format!("menu item {index} out of range")),
                    },
                    None => self.warn(span, format!("unknown menu id {id}")),
                }
                V::I(0)
            }
            GetMenuItemStr | GetMenuItemValue | GetMenuItemVisibility => {
                let (id, index) = (self.int(args, 0)?, self.int(args, 1)?);
                let item = self
                    .ui_index(id)
                    .and_then(|ui| self.menu(ui).get(index as usize).cloned());
                match (builtin, item) {
                    (GetMenuItemStr, item) => V::S(item.map(|i| i.text).unwrap_or_default()),
                    (GetMenuItemValue, item) => V::I(item.map_or(0, |i| i.value)),
                    (_, item) => V::I(item.map_or(0, |i| i32::from(i.visible))),
                }
            }
            GetNumMenuItems => {
                let id = self.int(args, 0)?;
                V::I(self.ui_index(id).map_or(0, |ui| self.menu(ui).len() as i32))
            }
            SetSkinOffset => {
                self.st.model.interface.skin_offset = Some(self.int(args, 0)?);
                V::I(0)
            }
            SetUiHeight => {
                self.st.model.interface.height_grid = Some(self.int(args, 0)?);
                V::I(0)
            }
            SetUiHeightPx => {
                self.st.model.interface.height_px = Some(self.int(args, 0)?);
                V::I(0)
            }
            SetUiWidthPx => {
                self.st.model.interface.width_px = Some(self.int(args, 0)?);
                V::I(0)
            }
            SetScriptTitle => {
                self.st.model.interface.title = Some(self.text(args, 0)?);
                V::I(0)
            }
            LoadPerformanceView => {
                self.request(builtin, args)?;
                self.st.model.interface.performance_view = true;
                self.apply_performance_view(span);
                V::I(0)
            }
            MakePerfview => {
                self.st.model.interface.performance_view = true;
                V::I(0)
            }
            GetFontId => {
                let name = self.text(args, 0)?;
                let fonts = &mut self.st.model.interface.fonts;
                let index = fonts.iter().position(|f| *f == name).unwrap_or_else(|| {
                    fonts.push(name);
                    fonts.len() - 1
                });
                V::I(index as i32)
            }
            SetKeyColor | SetKeyType | SetKeyPressed | SetKeyName => {
                let key = self.int(args, 0)?;
                let value = self.arg(args, 1)?;
                match self.st.model.interface.keys.get_mut(key as usize) {
                    Some(k) => match builtin {
                        SetKeyColor => k.color = Some(value.int()),
                        SetKeyType => k.kind = Some(value.int()),
                        SetKeyPressed => k.pressed = Some(value.int()),
                        _ => k.name = Some(value.text()),
                    },
                    None => self.warn(span, format!("key {key} out of range")),
                }
                V::I(0)
            }
            SetKeyPressedSupport => {
                self.st.model.interface.key_pressed_support = self.int(args, 0)? != 0;
                V::I(0)
            }
            GetKeyColor | GetKeyType | GetKeyTriggerstate => {
                let key = self.int(args, 0)?;
                let k = self.st.model.interface.keys.get(key as usize);
                V::I(match builtin {
                    GetKeyColor => k.and_then(|k| k.color).unwrap_or(16),
                    GetKeyType => k.and_then(|k| k.kind).unwrap_or(0),
                    _ => 0,
                })
            }
            GetKeyName => {
                let key = self.int(args, 0)?;
                V::S(
                    self.st
                        .model
                        .interface
                        .keys
                        .get(key as usize)
                        .and_then(|k| k.name.clone())
                        .unwrap_or_default(),
                )
            }
            SetKeyrange => {
                let (low, high, name) =
                    (self.int(args, 0)?, self.int(args, 1)?, self.text(args, 2)?);
                self.st
                    .model
                    .interface
                    .key_ranges
                    .push(KeyRange { low, high, name });
                V::I(0)
            }
            RemoveKeyrange => {
                let key = self.int(args, 0)?;
                self.st
                    .model
                    .interface
                    .key_ranges
                    .retain(|r| !(r.low..=r.high).contains(&key));
                V::I(0)
            }
            Message => {
                let text = self.text(args, 0)?;
                let messages = &mut self.st.model.interface.messages;
                if messages.len() < 1000 {
                    messages.push(text);
                }
                V::I(0)
            }
            SetSnapshotType => {
                let value = self.int(args, 0)?;
                self.st.model.snapshot_mode =
                    model::SnapshotMode::from_native(value).ok_or_else(|| Fault {
                        span,
                        builtin: Some("set_snapshot_type"),
                        message: "invalid snapshot mode".into(),
                    })?;
                V::I(0)
            }
            DisableLogging | WatchVar | WatchArrayIdx | ExposeControls | ShowLibraryTab
            | SetUiColor | ResetKspTimer => {
                if builtin == SetUiColor {
                    self.request(builtin, args)?;
                }
                V::I(0)
            }
            MakePersistent | MakeInstrPersistent => V::I(0),
            ReadPersistentVar => {
                let var = Self::var(args, 0);
                self.restore(var);
                self.consumed.insert(var);
                V::I(0)
            }
            PgsCreateKey => {
                let (key, size) = (self.text(args, 0)?, self.int(args, 1)?);
                self.st
                    .model
                    .pgs
                    .entry(key)
                    .or_insert_with(|| vec![0; size.clamp(0, 256) as usize]);
                V::I(0)
            }
            PgsCreateStrKey => {
                let key = self.text(args, 0)?;
                self.st.model.pgs_text.entry(key).or_default();
                V::I(0)
            }
            PgsKeyExists => {
                let key = self.text(args, 0)?;
                V::I(i32::from(self.st.model.pgs.contains_key(&key)))
            }
            PgsStrKeyExists => {
                let key = self.text(args, 0)?;
                V::I(i32::from(self.st.model.pgs_text.contains_key(&key)))
            }
            PgsSetKeyVal => {
                let (key, index, value) =
                    (self.text(args, 0)?, self.int(args, 1)?, self.int(args, 2)?);
                if let Some(slot) = self
                    .st
                    .model
                    .pgs
                    .get_mut(&key)
                    .and_then(|v| v.get_mut(index as usize))
                {
                    *slot = value;
                }
                V::I(0)
            }
            PgsGetKeyVal => {
                let (key, index) = (self.text(args, 0)?, self.int(args, 1)?);
                V::I(
                    self.st
                        .model
                        .pgs
                        .get(&key)
                        .and_then(|v| v.get(index as usize).copied())
                        .unwrap_or(0),
                )
            }
            PgsSetStrKeyVal => {
                let (key, value) = (self.text(args, 0)?, self.text(args, 1)?);
                if let Some(slot) = self.st.model.pgs_text.get_mut(&key) {
                    *slot = value;
                }
                V::I(0)
            }
            PgsGetStrKeyVal => {
                let key = self.text(args, 0)?;
                V::S(
                    self.st
                        .model
                        .pgs_text
                        .get(&key)
                        .cloned()
                        .unwrap_or_default(),
                )
            }
            SetListener | ChangeListenerPar => {
                let (signal, value) = (self.int(args, 0)?, self.int(args, 1)?);
                self.st.model.listeners.insert(signal, value);
                V::I(0)
            }
            SetEnginePar => {
                let key = [
                    self.int(args, 0)?,
                    self.int(args, 2)?,
                    self.int(args, 3)?,
                    self.int(args, 4)?,
                ];
                let value = self.int(args, 1)?;
                self.st.engine.insert(key, value);
                self.request(builtin, args)?;
                V::I(0)
            }
            GetEnginePar => {
                let key = [
                    self.int(args, 0)?,
                    self.int(args, 1)?,
                    self.int(args, 2)?,
                    self.int(args, 3)?,
                ];
                // Unwritten group volume, pan and tune read their neutral value.
                let neutral = match (key[2], key[3], symbol_name(self.hir, key[0])) {
                    (-1, -1, Some(n)) => match n.trim_start_matches('$') {
                        "ENGINE_PAR_VOLUME" => 630_957,
                        "ENGINE_PAR_PAN" | "ENGINE_PAR_TUNE" => 500_000,
                        _ => 0,
                    },
                    _ => 0,
                };
                let authored = symbol_name(self.hir, key[0])
                    .and_then(|name| sampler_core::engine_parameter_id(&name))
                    .and_then(|parameter| {
                        self.env
                            .engine_values
                            .get(&[parameter.into(), key[1], key[2], key[3]])
                            .copied()
                    });
                V::I(
                    self.st
                        .engine
                        .get(&key)
                        .copied()
                        .or(authored)
                        .unwrap_or(neutral),
                )
            }
            GetEngineParDisp | GetEngineParDispExt => V::S(String::new()),
            GroupName => {
                let i = self.int(args, 0)?;
                V::S(self.env.groups.get(i as usize).cloned().unwrap_or_default())
            }
            FindGroup | GetGroupIdx => {
                let name = self.text(args, 0)?;
                V::I(
                    self.env
                        .groups
                        .iter()
                        .position(|g| *g == name)
                        .map_or(b::NOT_FOUND, |i| i as i32),
                )
            }
            FindMod | GetModIdx | FindTarget | GetTargetIdx => {
                let name = self.text(args, args.len() - 1)?;
                let group = self.int(args, 0)?;
                let target = matches!(builtin, FindTarget | GetTargetIdx);
                let owner = if target { self.int(args, 1)? } else { -1 };
                V::I(
                    self.env
                        .engine_lookups
                        .iter()
                        .find(|l| {
                            l.group == group
                                && l.owner == owner
                                && l.target == target
                                && l.name.eq_ignore_ascii_case(&name)
                        })
                        .map_or(-1, |l| l.index),
                )
            }
            OutputChannelName | GetFolder | FsGetFilename => V::S(String::new()),
            FindZone => V::I(b::NOT_FOUND),
            GetNumZones | GetZoneId | GetZonePar | GetPurgeState | GetVoiceLimit
            | GetUiWfProperty | EventStatus | GetEventPar | GetEventParArr | GetEventMark
            | ByMarks => V::I(0),
            // No host consumes zone writes (FindZone finds nothing at init), and
            // Conflux issues three million of them: logging each cost ~1 GB.
            SetZonePar => V::I(0),
            PurgeGroup | SetVoiceLimit | LoadIrSample | LoadArray | SaveArray | LoadArrayStr
            | SaveArrayStr | AttachLevelMeter | AttachZone | SetUiWfProperty | FsNavigate
            | LoadNativeUi | SetNksNavName | SetNksNavPar | ResetNksNav => {
                self.request(builtin, args)?;
                V::I(0)
            }
            SetController => {
                let (controller, value) = (self.int(args, 0)?, self.int(args, 1)?);
                if let (Ok(controller), Ok(value)) = (u8::try_from(controller), u8::try_from(value))
                {
                    let set = &mut self.st.model.controllers;
                    set.retain(|&(c, _)| c != controller);
                    set.push((controller, value));
                }
                V::I(0)
            }
            PlayNote | NoteOff | IgnoreEvent | ChangeVol | ChangeTune | ChangePan | ChangeVelo
            | ChangeNote | FadeIn | FadeOut | SetEventPar | SetEventParArr | AllowGroup
            | DisallowGroup | SetEventMark | DeleteEventMark | GetEventIds | IgnoreController
            | SetNoteController | SetRpn | SetNrpn | ResetRlsTrigCounter | WillNeverTerminate
            | RedirectOutput | Wait | WaitTicks | WaitAsync | StopWait => {
                self.warn(span, format!("{} has no effect in on init", builtin.name()));
                V::I(0)
            }
        })
    }
}

/// Synthetic property keys for grid placement and table step display.
pub const GRID_X: i32 = 0x0300_0001;
pub const GRID_Y: i32 = 0x0300_0002;
pub const TABLE_STEPS_SHOWN: i32 = 0x0300_0003;

pub fn symbol_name(hir: &Hir, value: i32) -> Option<String> {
    symbol_in(&hir.symbols, value)
}

/// [`symbol_name`] over a script's symbol table.
pub(crate) fn symbol_in<S: AsRef<str>>(symbols: &[S], value: i32) -> Option<String> {
    match value {
        GRID_X => Some("grid_x".into()),
        GRID_Y => Some("grid_y".into()),
        TABLE_STEPS_SHOWN => Some("table_steps_shown".into()),
        _ => b::CONTROL_PARS
            .get(usize::try_from(value.wrapping_sub(b::SYMBOL_BASE)).ok()?)
            .map(|s| (*s).to_owned())
            .or_else(|| {
                symbols
                    .get(usize::try_from(value.wrapping_sub(OPAQUE_BASE)).ok()?)
                    .map(|s| s.as_ref().to_string())
            }),
    }
}

/// Stable opaque module/target index for a name (FNV-1a, 24 bits).
pub fn lookup_index(name: &str) -> i32 {
    sampler_core::name_index(name)
}

fn placeholder() -> model::Widget {
    model::Widget {
        name: String::new(),
        kind: WidgetKind::Label,
        ui_id: 0,
        control: None,
        value: model::WidgetValue::None,
        params: Vec::new(),
        range: None,
        properties: BTreeMap::new(),
        indexed_properties: BTreeMap::new(),
        menu: Vec::new(),
        callback: None,
        persistence: Persistence::None,
        location: None,
    }
}
