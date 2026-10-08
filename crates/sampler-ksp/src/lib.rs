#![forbid(unsafe_code)]
//! KSP frontend: spanned lexer and condition preprocessor, AST, typed name
//! resolution, control-thread `on init` evaluation, and lowering of every other
//! callback to bounded sampler-core programs. Not a Kontakt fidelity claim:
//! services the engine does not own are queued as effects, and every ignored or
//! approximated call is reported in `Script::warnings` and `Script::coverage`.
use model::Value;
use sampler_core::{
    ControlCallback, ControlDefinition, ControlDomain, ControlId, ControlValue, Prepared, Program,
    ScriptInstanceId, ScriptResources,
};
use std::collections::{BTreeMap, BTreeSet};
mod ast;
mod builtins;
mod diag;
mod eval;
mod hir;
mod lexer;
mod lower;
pub mod model;
pub mod nckp;
mod parser;
mod sema;
pub mod ui;

pub use diag::{Error, Kind};
pub use eval::Environment;
pub use lower::{Coverage, LISTENER_TAG, PGS_TAG, PROPERTY_TAG};

pub const PROFILE: &str = "ksp-8.12-v2";

#[derive(Clone, Copy, Debug)]
pub struct Limits {
    pub source_bytes: usize,
    /// Total lowered instructions across all callbacks and their functions.
    pub instructions: usize,
    pub variables: usize,
    /// Total declared array elements; independent of the declaration-count budget.
    pub array_cells: usize,
}

impl Limits {
    /// For scripts from installed libraries: about four times the largest
    /// seen across the installed Kontakt libraries (sampler-kontakt's
    /// `examples/library_scan.rs`).
    pub const LIBRARY: Self = Self {
        source_bytes: LIBRARY_LIMITS[0],
        instructions: LIBRARY_LIMITS[1],
        variables: LIBRARY_LIMITS[2],
        array_cells: LIBRARY_LIMITS[3],
    };
}

/// About 4 × the largest seen (2,741 NKIs, 52 distinct scripts, 2026-10):
/// 19,264,696 source bytes and 2,881,448 array cells (Areia "Pyramid"),
/// 8,662,930 instructions and 2,039 variables (Analog Strings).
/// Refresh from the scan when libraries are added.
const LIBRARY_LIMITS: [usize; 4] = [80 << 20, 40_000_000, 1 << 16, 1 << 24];

/// Presentation of a host-owned control.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Widget {
    Knob { display_ratio: i32 },
    Slider,
    Button,
    Switch,
    Menu,
    ValueEdit { display_ratio: i32 },
}

#[derive(Clone, Debug, PartialEq)]
pub struct Control {
    pub variable: String,
    pub widget: Widget,
    pub definition: ControlDefinition,
    /// Program run by `on ui_control`.
    pub callback: Option<usize>,
}

/// Which callback a program implements.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EntryKind {
    Note,
    Release,
    Controller,
    PolyAt,
    /// `on ui_control` of the widget at this index in `model().interface.widgets`.
    UiControl(usize),
    Listener,
    PgsChanged,
    PersistenceChanged,
    AsyncComplete,
    Rpn,
    Nrpn,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Entry {
    pub kind: EntryKind,
    /// Program index within this script (offset by earlier modules after binding).
    pub program: usize,
}
/// A compiled script: programs, initial state after `on init`, and its model.
pub struct Script {
    programs: Vec<Program>,
    entries: Vec<Entry>,
    /// Programs started when the plan becomes active (listener timers).
    starts: Vec<usize>,
    /// PGS keys this script created, for the plan's shared store.
    shared: Vec<([i32; 4], i64)>,
    rate: u32,
    cells: Vec<i64>,
    resources: ScriptResources,
    note_cells: usize,
    controls: Vec<Control>,
    model: model::Model,
    warnings: Vec<Error>,
    services: Vec<&'static str>,
    coverage: Vec<(&'static str, Coverage, usize)>,
    symbols: Vec<String>,
    slot: u8,
    usage: Limits,
    /// `SET_CONDITION(NO_SYS_SCRIPT_PEDAL)`: the script, not the engine,
    /// sustains notes on CC64.
    owns_sustain: bool,
    /// `SET_CONDITION(NO_SYS_SCRIPT_RLS_TRIG)`: the script plays release
    /// samples, so native release-trigger groups stay silent.
    owns_release_triggers: bool,
}

impl Script {
    /// What this script used of each limit it compiled under.
    pub fn usage(&self) -> Limits {
        self.usage
    }
    /// The interface as format-neutral UI IR, validated. `picture` gives the
    /// metadata of a library-relative image path, e.g. from
    /// [`ui::picture_meta`] over the picture's `.txt`.
    pub fn ui(
        &self,
        picture: &dyn Fn(&str) -> Option<sampler_ui_ir::ImageMeta>,
    ) -> Result<sampler_ui_ir::Interface, sampler_ui_ir::Error> {
        ui::interface(&self.model, self.slot, picture)
    }
    pub fn has_performance_view(&self) -> bool {
        self.model.interface.performance_view
    }
    /// Host-owned controls (knob, slider, button, switch, menu, value edit).
    pub fn controls(&self) -> &[Control] {
        &self.controls
    }
    pub fn global_cells(&self) -> usize {
        self.cells.len()
    }
    pub fn note_cells(&self) -> usize {
        self.note_cells
    }
    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }
    pub fn model(&self) -> &model::Model {
        &self.model
    }
    /// Non-fatal findings from resolution, `on init` and lowering.
    pub fn warnings(&self) -> &[Error] {
        &self.warnings
    }
    /// Builtin name of each effect service id (`sampler_core::Effect::service`).
    pub fn services(&self) -> &[&'static str] {
        &self.services
    }
    /// Call sites by builtin and how each was lowered.
    pub fn coverage(&self) -> &[(&'static str, Coverage, usize)] {
        &self.coverage
    }
    /// Undeclared vendor names treated as opaque values.
    pub fn symbols(&self) -> &[String] {
        &self.symbols
    }
    /// Install the complete script on a prepared instrument of the compiled rate.
    /// Apply a `set_control_par*` request this script's callbacks emitted
    /// (an [`sampler_core::Effect`] whose `instance` is this script's) to its
    /// model, so [`Script::ui`] shows runtime UI changes such as pages,
    /// pictures and hidden panels. Returns whether the effect was one.
    pub fn apply_ui_effect(&mut self, effect: &sampler_core::Effect) -> bool {
        apply_ui_effect(&mut self.model, &self.services, &self.symbols, effect)
    }

    /// What [`Script::ui`] and [`Script::apply_ui_effect`] need, kept after
    /// the script is bound.
    pub fn view(&self) -> ScriptView {
        ScriptView {
            model: self.model.clone(),
            services: self.services.clone(),
            symbols: self.symbols.clone(),
            slot: self.slot,
        }
    }

    pub fn bind(self, plan: Prepared) -> Result<Prepared, sampler_core::Error> {
        bind_modules(vec![self], plan)
    }
    fn routed(&self, kind: EntryKind) -> Option<usize> {
        self.entries
            .iter()
            .find(|e| e.kind == kind)
            .map(|e| e.program)
    }
}

/// A bound script's interface model: [`Script::view`].
#[derive(Clone, Debug)]
pub struct ScriptView {
    model: model::Model,
    services: Vec<&'static str>,
    symbols: Vec<String>,
    slot: u8,
}

impl ScriptView {
    /// Builtin name of an effect service id (`sampler_core::Effect::service`).
    pub fn service(&self, id: u16) -> Option<&'static str> {
        self.services.get(usize::from(id)).copied()
    }
    /// Symbolic name of an opaque vendor constant (`ENGINE_PAR_*`, `NI_*`).
    pub fn symbol(&self, value: i32) -> Option<String> {
        eval::symbol_in(&self.symbols, value)
    }
    /// The model as runtime effects have left it (keys, widgets).
    pub fn model(&self) -> &model::Model {
        &self.model
    }
    /// [`Script::apply_ui_effect`].
    pub fn apply_ui_effect(&mut self, effect: &sampler_core::Effect) -> bool {
        apply_ui_effect(&mut self.model, &self.services, &self.symbols, effect)
    }
    /// [`Script::ui`].
    pub fn ui(
        &self,
        picture: &dyn Fn(&str) -> Option<sampler_ui_ir::ImageMeta>,
    ) -> Result<sampler_ui_ir::Interface, sampler_ui_ir::Error> {
        ui::interface(&self.model, self.slot, picture)
    }
}

fn apply_ui_effect(
    model: &mut model::Model,
    services: &[&'static str],
    symbols: &[String],
    effect: &sampler_core::Effect,
) -> bool {
    let Some(&service) = services.get(usize::from(effect.service)) else {
        return false;
    };
    let args = &effect.args[..usize::from(effect.count)];
    let arg = |i: usize| args.get(i).map(|&v| v as i32);
    let text = || effect.text.as_ref().map(|t| t.as_str().to_string());
    if let Some(rest) = service.strip_prefix("set_key_") {
        let Some(key) = arg(0).and_then(|k| model.interface.keys.get_mut(usize::try_from(k).ok()?))
        else {
            return false;
        };
        match rest {
            "color" => key.color = arg(1),
            "type" => key.kind = arg(1),
            "pressed" => key.pressed = arg(1),
            "name" => key.name = text(),
            _ => return false,
        }
        return true;
    }
    let (Some(id), Some(par)) = (arg(0), arg(1)) else {
        return false;
    };
    let (value, index) = match service {
        "set_control_par" => (arg(2).map(Value::Int), None),
        "set_control_par_real" => (
            args.get(2).map(|&b| Value::Real(f64::from_bits(b as u64))),
            None,
        ),
        "set_control_par_str" => (text().map(Value::Text), None),
        "set_control_par_arr" => (arg(2).map(Value::Int), arg(3)),
        "set_control_par_str_arr" => (text().map(Value::Text), arg(2)),
        _ => return false,
    };
    let (Some(value), Some(name)) = (value, eval::symbol_in(symbols, par)) else {
        return false;
    };
    let interface = &mut model.interface;
    if let Some(w) = interface.widgets.iter_mut().find(|w| w.ui_id == id) {
        match index {
            Some(i) => {
                w.indexed_properties
                    .entry(name)
                    .or_default()
                    .insert(i, value);
            }
            None => {
                if name == "$CONTROL_PAR_VALUE"
                    && let (Value::Int(v), model::WidgetValue::Int(_)) = (&value, &w.value)
                {
                    w.value = model::WidgetValue::Int(*v);
                }
                w.properties.insert(name, value);
            }
        }
    } else if (builtins::INST_ICON_ID..=builtins::INST_ICON_ID + 5).contains(&id) {
        interface
            .instrument
            .entry(id)
            .or_default()
            .insert(name, value);
    } else {
        return false;
    }
    true
}

/// Bind ordered controller-only modules with independent script state.
pub fn bind_controller_chain(
    scripts: Vec<Script>,
    plan: Prepared,
) -> Result<Prepared, sampler_core::Error> {
    if scripts
        .iter()
        .any(|s| s.routed(EntryKind::Note).is_some() || s.routed(EntryKind::Release).is_some())
    {
        return Err(sampler_core::Error::InvalidInput);
    }
    bind_modules(scripts, plan)
}

/// Bind source modules in order through the shared native routing table.
/// Note, release and controller callbacks share native module positions and
/// retain separate instance state and reached-event projections.
pub fn bind_modules(scripts: Vec<Script>, plan: Prepared) -> Result<Prepared, sampler_core::Error> {
    let mut programs = Vec::new();
    let mut instances = Vec::new();
    let mut resources = Vec::new();
    // The plan's effect slot controls stay beside the scripts' own.
    let mut controls: Vec<_> = plan
        .controls()
        .iter()
        .filter(|c| sampler_core::is_slot_control(c.id))
        .copied()
        .collect();
    let mut callbacks = Vec::new();
    let mut stages = Vec::new();
    let mut starts = Vec::new();
    let mut signals = Vec::new();
    let mut shared = Vec::new();
    let initial_controllers: Vec<(u8, u8)> = scripts
        .iter()
        .flat_map(|s| s.model().controllers.iter().copied())
        .collect();
    let owns_sustain = scripts.iter().any(|s| s.owns_sustain);
    let owns_release_triggers = scripts.iter().any(|s| s.owns_release_triggers);
    for (index, script) in scripts.into_iter().enumerate() {
        if script.rate != plan.sample_rate() {
            return Err(sampler_core::Error::InvalidInput);
        }
        let instance =
            ScriptInstanceId(u16::try_from(index).map_err(|_| sampler_core::Error::Capacity)?);
        let base = programs.len();
        stages.push(sampler_core::Stage {
            note: script.routed(EntryKind::Note).map(|p| base + p),
            release: script.routed(EntryKind::Release).map(|p| base + p),
            controller: script.routed(EntryKind::Controller).map(|p| base + p),
        });
        for control in script.controls {
            if let Some(program) = control.callback {
                callbacks.push(ControlCallback {
                    control: control.definition.id,
                    program: base + program,
                    stage: index,
                });
            }
            controls.push(control.definition);
        }
        signals.extend(
            script
                .entries
                .iter()
                .filter(|e| e.kind == EntryKind::PgsChanged)
                .map(|e| sampler_core::SignalProgram {
                    signal: lower::PGS_SIGNAL,
                    program: base + e.program,
                    stage: index,
                }),
        );
        // Keys created by several scripts keep the first script's values.
        shared.extend(script.shared.iter().copied());
        starts.extend(script.starts.iter().map(|&p| sampler_core::PlanProgram {
            program: base + p,
            stage: index,
        }));
        programs.extend(
            script
                .programs
                .into_iter()
                .map(|p| p.with_script_instance(instance).with_program_base(base)),
        );
        instances.push(script.cells);
        resources.push(script.resources);
    }
    let capacity = shared.len() + 4096;
    plan.with_initial_controllers(&initial_controllers)
        .with_script_sustain(owns_sustain)
        .with_script_release_triggers(owns_release_triggers)
        .with_programs(Vec::new(), None)?
        .with_script_instances(instances)?
        // Keep source aliases separate from marked/all-event selectors.
        .with_source_event_limit(0x0fff_ffff)?
        .with_controls(controls)?
        .with_script_resources(resources)?
        .with_programs(programs, None)?
        .with_stages(stages)?
        .with_control_programs(callbacks)?
        .with_plan_programs(starts)?
        .with_signal_programs(signals)?
        // ponytail: fixed headroom for keys created at runtime, like the script stores.
        .with_shared_store(shared, capacity)
}

/// Compile with no instrument facts and the default script slot.
pub fn compile(
    source: &str,
    rate: u32,
    limits: Limits,
    controls: &[(&str, ControlId)],
) -> Result<Script, Error> {
    compile_with(source, rate, limits, controls, &Environment::default())
}

/// Stable identity of an unbound control: FNV-1a 128 over slot and name.
pub fn derived_control_id(slot: u8, variable: &str) -> ControlId {
    let mut h: u128 = 0x6c62_272e_07bb_0142_62b8_2175_6295_c58d;
    for b in format!("ksp/{slot}/{variable}").bytes() {
        h = (h ^ u128::from(b)).wrapping_mul(0x0000_0000_0100_0000_0000_0000_0000_013b);
    }
    ControlId(h)
}

/// One `set_engine_par` write `on init` left standing, by symbolic name.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EnginePar {
    /// `ENGINE_PAR_*` name; the number when the script used a literal.
    pub parameter: String,
    pub value: i32,
    pub group: i32,
    pub slot: i32,
    pub generic: i32,
}

impl Script {
    /// Whether a callback sets effect slot bypass, output gain or dry level
    /// while playing, so the host must build slots those writes can reach.
    pub fn writes_effect_slots(&self) -> bool {
        self.programs.iter().any(Program::writes_slots)
    }
}

/// The `set_engine_par` values `on init` leaves, in parameter order, without
/// compiling the callbacks. Hosts apply them to what they translated before
/// the script runs (effect racks, buses).
pub fn init_engine_pars(
    source: &str,
    limits: Limits,
    environment: &Environment,
) -> Result<Vec<EnginePar>, Error> {
    let mut syms = lexer::Interner::default();
    (|| {
        let mut toks = lexer::lex(source, &mut syms)?;
        lexer::preprocess(&mut toks, &syms, &Default::default())?;
        let ast = parser::parse(&toks, &syms)?;
        let budget = sema::Budget {
            variables: limits.variables,
            array_cells: limits.array_cells,
        };
        let hir = sema::analyze(ast, &syms, budget, &environment.performance_view.controls)?;
        let init = eval::run(&hir, environment)?;
        let mut writes: Vec<_> = init
            .engine
            .iter()
            .map(|(&[parameter, group, slot, generic], &value)| EnginePar {
                parameter: eval::symbol_name(&hir, parameter)
                    .unwrap_or_else(|| parameter.to_string()),
                value,
                group,
                slot,
                generic,
            })
            .collect();
        writes.sort_by(|a, b| {
            (&a.parameter, a.group, a.slot, a.generic).cmp(&(
                &b.parameter,
                b.group,
                b.slot,
                b.generic,
            ))
        });
        Ok(writes)
    })()
    .map_err(|f: diag::Fault| f.locate(source))
}

/// Compile a script. `on init` runs here on the control thread against
/// `environment`; every other callback becomes a program. Controls without an
/// explicit binding get `derived_control_id(environment.slot, name)`.
pub fn compile_with(
    source: &str,
    rate: u32,
    limits: Limits,
    controls: &[(&str, ControlId)],
    environment: &Environment,
) -> Result<Script, Error> {
    let error = |message: &str| Error {
        offset: 0,
        line: 1,
        column: 1,
        kind: diag::Kind::Error,
        builtin: None,
        message: message.into(),
    };
    if source.len() > limits.source_bytes {
        return Err(error(&format!(
            "source byte budget exceeded: {} bytes, limit {}",
            source.len(),
            limits.source_bytes
        )));
    }
    if rate == 0 {
        return Err(error("sample rate must be positive"));
    }
    if controls.len() > limits.variables {
        return Err(error("control binding budget exceeded"));
    }
    let mut bindings = BTreeMap::new();
    let mut identities = BTreeSet::new();
    for &(name, id) in controls {
        if bindings.insert(name, id).is_some() || !identities.insert(id) {
            return Err(error("duplicate control name or persistent identity"));
        }
    }
    let mut syms = lexer::Interner::default();
    let (hir, init, conditions) = (|| {
        let mut toks = lexer::lex(source, &mut syms)?;
        let conditions = lexer::preprocess(&mut toks, &syms, &Default::default())?;
        let ast = parser::parse(&toks, &syms)?;
        let budget = sema::Budget {
            variables: limits.variables,
            array_cells: limits.array_cells,
        };
        let hir = sema::analyze(ast, &syms, budget, &environment.performance_view.controls)?;
        let init = eval::run(&hir, environment)?;
        Ok((hir, init, conditions))
    })()
    .map_err(|f: diag::Fault| f.locate(source))?;

    // Control identities and definitions.
    let mut ids = vec![None; hir.uis.len()];
    let mut host = Vec::new();
    for (i, ui) in hir.uis.iter().enumerate() {
        let var = &hir.vars[ui.var.0 as usize];
        if !matches!(var.home, hir::Home::Control(_)) {
            continue;
        }
        let id = bindings
            .remove(&*var.name)
            .unwrap_or_else(|| derived_control_id(environment.slot, &var.name));
        ids[i] = Some(id);
        let (min, max) =
            eval::declared_range(ui).map_or((i32::MIN, i32::MAX), |(a, b)| (a.min(b), a.max(b)));
        let widget = match ui.kind {
            hir::WidgetKind::Knob => Widget::Knob {
                display_ratio: ui.params.get(2).copied().unwrap_or(1),
            },
            hir::WidgetKind::ValueEdit => Widget::ValueEdit {
                display_ratio: ui.params.get(2).copied().unwrap_or(1),
            },
            hir::WidgetKind::Slider => Widget::Slider,
            hir::WidgetKind::Button => Widget::Button,
            hir::WidgetKind::Switch => Widget::Switch,
            _ => Widget::Menu,
        };
        host.push((
            i,
            Control {
                variable: var.name.to_string(),
                widget,
                definition: ControlDefinition {
                    id,
                    domain: ControlDomain::Integer {
                        min: i64::from(min),
                        max: i64::from(max),
                    },
                    default: ControlValue::Integer(i64::from(init.controls[i].clamp(min, max))),
                },
                callback: None,
            },
        ));
    }
    if !bindings.is_empty() {
        return Err(error("unused control identity binding"));
    }

    // Lowering.
    let mut unit = lower::Unit {
        hir: &hir,
        controls: &ids,
        groups: &environment.groups,
        slot: environment.slot,
        budget: limits.instructions,
        limit: limits.instructions,
        services: Vec::new(),
        coverage: BTreeMap::new(),
        warnings: Vec::new(),
        scratch: 0,
    };
    let mut programs = Vec::new();
    let mut entries = Vec::new();
    let mut starts = Vec::new();
    for callback in &hir.callbacks {
        use hir::CallbackKind as K;
        let (kind, context) = match callback.kind {
            K::Init => continue,
            K::Note => (EntryKind::Note, lower::Context::Note),
            K::Release => (EntryKind::Release, lower::Context::Release),
            K::Controller => (EntryKind::Controller, lower::Context::Controller),
            K::PolyAt => (EntryKind::PolyAt, lower::Context::Plan),
            K::UiControl(var) => (
                EntryKind::UiControl(hir.vars[var.0 as usize].ui.unwrap_or(0) as usize),
                lower::Context::Plan,
            ),
            K::Listener => (EntryKind::Listener, lower::Context::Plan),
            K::PgsChanged => (EntryKind::PgsChanged, lower::Context::Plan),
            K::PersistenceChanged => (EntryKind::PersistenceChanged, lower::Context::Plan),
            K::AsyncComplete => (EntryKind::AsyncComplete, lower::Context::Plan),
            K::Rpn => (EntryKind::Rpn, lower::Context::Plan),
            K::Nrpn => (EntryKind::Nrpn, lower::Context::Plan),
        };
        // A timer listener body per timer signal set in on init, each
        // started by a driver program; otherwise one unstarted program.
        let timers: Vec<i32> = if kind == EntryKind::Listener {
            init.model
                .listeners
                .keys()
                .copied()
                .filter(|s| [builtins::signal::TIMER_MS, builtins::signal::TIMER_BEAT].contains(s))
                .collect()
        } else {
            Vec::new()
        };
        for signal in timers
            .iter()
            .map(|s| Some(*s))
            .chain(timers.is_empty().then_some(None))
        {
            let program = unit
                .program(
                    &callback.body,
                    callback.span,
                    context,
                    callback.kind,
                    signal,
                )
                .map_err(|f| f.locate(source))?;
            entries.push(Entry {
                kind,
                program: programs.len(),
            });
            programs.push(program);
            if let Some(signal) = signal {
                let body = programs.len() - 1;
                let driver = unit
                    .listener_driver(signal, body, callback.span)
                    .map_err(|f| f.locate(source))?;
                starts.push(programs.len());
                programs.push(driver);
            }
        }
    }
    // Group volume, pan and tune `on init` wrote reach the runtime layers.
    let mut start: Vec<_> = init
        .engine
        .iter()
        .filter(|(k, _)| k[2] == -1 && k[3] == -1 && k[1] >= -1)
        .filter_map(|(k, &v)| {
            let target = match eval::symbol_name(&hir, k[0])?.trim_start_matches('$') {
                "ENGINE_PAR_VOLUME" => sampler_core::ModTarget::Decibels,
                "ENGINE_PAR_PAN" => sampler_core::ModTarget::Pan,
                "ENGINE_PAR_TUNE" => sampler_core::ModTarget::Pitch,
                _ => return None,
            };
            Some((target, k[1], v))
        })
        .collect();
    start.sort_by_key(|&(t, g, _)| (g, t as u8));
    if !start.is_empty() {
        let program = unit.engine_start(&start).map_err(|f| f.locate(source))?;
        starts.push(programs.len());
        programs.push(program);
    }
    for (ui, control) in &mut host {
        control.callback = entries
            .iter()
            .find(|e| e.kind == EntryKind::UiControl(*ui))
            .map(|e| e.program);
    }

    // Initial instance state: texts plus lowering scratch, the property /
    // engine / PGS mirror, and the dense control table.
    let mut texts = init.texts.clone();
    texts.resize(texts.len() + unit.scratch as usize, String::new());
    let mut store = Vec::new();
    for (&(id, par), &value) in &init.properties {
        store.push(([id, par, PROPERTY_TAG, PROPERTY_TAG], i64::from(value)));
    }
    for (i, ui) in hir.uis.iter().enumerate() {
        let id = builtins::FIRST_UI_ID + i as i32;
        if let Some((lo, hi)) = eval::declared_range(ui) {
            for (par, value) in [
                (builtins::CONTROL_PAR_MIN_VALUE, lo),
                (builtins::CONTROL_PAR_MAX_VALUE, hi),
            ] {
                if !init.properties.contains_key(&(id, par)) {
                    store.push(([id, par, PROPERTY_TAG, PROPERTY_TAG], i64::from(value)));
                }
            }
        }
    }
    for (&key, &value) in &init.engine {
        store.push((key, i64::from(value)));
    }
    for (&signal, &value) in &init.model.listeners {
        store.push(([LISTENER_TAG, signal, 0, LISTENER_TAG], i64::from(value)));
    }
    let mut shared = Vec::new();
    for (key, values) in &init.model.pgs {
        let hash = lower::name_hash(key);
        for (i, &v) in values.iter().enumerate() {
            shared.push(([PGS_TAG, hash, i as i32, PGS_TAG], i64::from(v)));
        }
    }
    // ponytail: fixed headroom for runtime-created keys; size from usage if exceeded.
    let store_capacity = store.len() + 4096;
    let resources = ScriptResources {
        texts,
        store,
        store_capacity,
        controls: ids.clone(),
    };

    let mut warnings: Vec<Error> = hir
        .warnings
        .iter()
        .chain(&init.warnings)
        .map(|f| (f, diag::Kind::Warning))
        .chain(unit.warnings.iter().map(|(f, k)| (f, *k)))
        .map(|(f, kind)| f.clone().locate_as(source, kind))
        .collect();
    // Builtin findings first so the cap never hides an unsupported builtin.
    warnings.sort_by_key(|w| (w.kind == Kind::Warning, w.offset));
    warnings.truncate(1000);
    let services = unit.services.iter().map(|b| b.name()).collect();
    let coverage = unit
        .coverage
        .iter()
        .map(|(&(name, c), &n)| (name, c, n))
        .collect();
    let model = model::assemble(&hir, &init, &ids, &entries);
    Ok(Script {
        programs,
        entries,
        starts,
        shared,
        rate,
        cells: init.cells,
        resources,
        note_cells: usize::from(hir.note_cells),
        controls: host.into_iter().map(|(_, c)| c).collect(),
        model,
        warnings,
        services,
        coverage,
        symbols: hir.symbols.iter().map(|s| s.to_string()).collect(),
        slot: environment.slot,
        owns_sustain: conditions.contains("NO_SYS_SCRIPT_PEDAL"),
        owns_release_triggers: conditions.contains("NO_SYS_SCRIPT_RLS_TRIG"),
        usage: Limits {
            source_bytes: source.len(),
            instructions: limits.instructions - unit.budget,
            variables: hir.vars.len(),
            array_cells: hir
                .vars
                .iter()
                .filter_map(|v| v.len)
                .map(|n| n as usize)
                .sum(),
        },
    })
}
