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
mod array_file;
mod ast;
mod builtins;
mod diag;
mod eval;
mod hir;
#[cfg(feature = "cache")]
mod init_cache;
#[cfg(feature = "cache")]
pub use init_cache::{CachedInit, restore_initialized};
mod lexer;
mod lower;
pub mod model;
pub mod nckp;
mod parser;
#[cfg(feature = "scan")]
pub mod scan;
mod sema;
pub mod ui;
mod waveform;

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
    UiControls,
    UiUpdate,
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
    midi_object: sampler_core::MidiObject,
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
    /// Source slot retained independently of the bound script-instance index.
    pub fn slot(&self) -> u8 {
        self.slot
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
            entries: self.entries.clone(),
            programs: self.programs.len(),
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
    entries: Vec<Entry>,
    programs: usize,
}

impl ScriptView {
    /// How many programs the script contributed to the plan's program table.
    pub fn program_count(&self) -> usize {
        self.programs
    }

    /// The callback of this script's `local` program (`"on note"`...), if it
    /// is one of its entry points.
    pub fn callback(&self, local: usize) -> Option<&'static str> {
        Some(
            match self.entries.iter().find(|e| e.program == local)?.kind {
                EntryKind::Note => "on note",
                EntryKind::Release => "on release",
                EntryKind::Controller => "on controller",
                EntryKind::PolyAt => "on poly_at",
                EntryKind::UiControl(_) => "on ui_control",
                EntryKind::UiControls => "on ui_controls",
                EntryKind::UiUpdate => "on ui_update",
                EntryKind::Listener => "on listener",
                EntryKind::PgsChanged => "on pgs_changed",
                EntryKind::PersistenceChanged => "on persistence_changed",
                EntryKind::AsyncComplete => "on async_complete",
                EntryKind::Rpn => "on rpn",
                EntryKind::Nrpn => "on nrpn",
            },
        )
    }

    /// The script slot.
    pub fn slot(&self) -> u8 {
        self.slot
    }

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
    /// Reject wrong plan/instance before projection. The caller must supply the
    /// live plan admitted by its part/epoch fence; this is not an epoch oracle.
    pub fn apply_ui_effect_for(
        &mut self,
        plan: sampler_core::PlanId,
        instance: ScriptInstanceId,
        effect: &sampler_core::Effect,
    ) -> bool {
        if effect.plan != plan || effect.instance != Some(instance) { return false; }
        self.apply_ui_effect(effect)
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
    let Some(args) = effect.args.get(..usize::from(effect.count)) else {
        return false;
    };
    let arg = |i: usize| args.get(i).and_then(|&v| i32::try_from(v).ok());
    let text = || effect.text.as_ref().map(|t| t.as_str().to_string());
    if matches!(service, "attach_zone" | "set_ui_wf_property") {
        use sampler_core::waveform::Property;
        let Some(id) = arg(0) else { return false; };
        let Some(widget) = model.interface.widgets.iter().find(|w| {
            w.ui_id == id && w.kind == model::WidgetKind::Waveform && !w.unresolved
        }) else { return false; };
        let name = widget.name.clone();
        if effect.text.is_some() { return false; }
        let request = if service == "attach_zone" {
            if args.len() != 3 { return false; }
            let (Some(zone), Some(flags)) = (arg(1).filter(|z| *z > 0), arg(2)) else { return false; };
            model::Request { command: "attach_zone", args: vec![Value::Int(id), Value::Int(zone), Value::Int(flags)] }
        } else {
            if args.len() != 4 { return false; }
            let (Some(property), Some(index), Some(value)) =
                (arg(1).and_then(|p| eval::symbol_in(symbols, p)), arg(2), arg(3)) else { return false; };
            let Some(p) = Property::from_name(&property) else { return false; };
            if p.validate_index(index).is_err() || ui::waveform_requests(model, id, &name).is_none() { return false; }
            model::Request { command: "set_ui_wf_property", args: vec![Value::Int(id), Value::Text(property),
                Value::Int(index), Value::Int(p.value(index, value))] }
        };
        return waveform::project(model, &name, id, request);
    }
    if let Some(rest) = service.strip_prefix("set_key_") {
        let Some(key) = arg(0).and_then(|k| model.interface.keys.get_mut(usize::try_from(k).ok()?))
        else {
            return false;
        };
        let before = key.clone();
        match rest {
            "color" => key.color = arg(1),
            "type" => key.kind = arg(1),
            "pressed" => key.pressed = arg(1),
            "name" => key.name = text(),
            _ => return false,
        }
        return *key != before;
    }
    if service == "set_skin_offset" {
        let Some(value) = arg(0) else { return false };
        let changed = model.interface.skin_offset != Some(value);
        model.interface.skin_offset = Some(value);
        return changed;
    }
    if service == "set_ui_color" {
        let Some(value) = arg(0) else { return false };
        if let Some(request) = model
            .requests
            .iter_mut()
            .rev()
            .find(|r| r.command == "set_ui_color")
        {
            if request.args == [Value::Int(value)] {
                return false;
            }
            request.args = vec![Value::Int(value)];
        } else {
            model.requests.push(model::Request {
                command: "set_ui_color",
                args: vec![Value::Int(value)],
            });
        }
        return true;
    }
    let Some(id) = arg(0) else { return false };
    let widget = model.interface.widgets.iter_mut().find(|w| w.ui_id == id);
    if matches!(
        service,
        "move_control"
            | "move_control_px"
            | "add_menu_item"
            | "set_menu_item_str"
            | "set_menu_item_visibility"
            | "set_menu_item_value"
    ) {
        let Some(w) = widget else { return false };
        let before = w.clone();
        match service {
            "move_control" | "move_control_px" => {
                let (Some(x), Some(y)) = (arg(1), arg(2)) else {
                    return false;
                };
                let (px, py) = if service == "move_control" {
                    ("grid_x", "grid_y")
                } else {
                    w.properties.remove("grid_x");
                    w.properties.remove("grid_y");
                    ("$CONTROL_PAR_POS_X", "$CONTROL_PAR_POS_Y")
                };
                w.properties.insert(px.into(), Value::Int(x));
                w.properties.insert(py.into(), Value::Int(y));
            }
            "add_menu_item" => {
                let (Some(text), Some(value)) = (text(), arg(1)) else {
                    return false;
                };
                w.menu.push(model::MenuItem {
                    text,
                    value,
                    visible: true,
                });
            }
            _ => {
                let Some(item) = arg(1).and_then(|i| w.menu.get_mut(usize::try_from(i).ok()?))
                else {
                    return false;
                };
                match service {
                    "set_menu_item_str" => {
                        let Some(value) = text() else { return false };
                        item.text = value;
                    }
                    "set_menu_item_value" => {
                        let Some(value) = arg(2) else { return false };
                        item.value = value;
                    }
                    _ => {
                        let Some(value) = arg(2) else { return false };
                        item.visible = value != 0;
                    }
                }
            }
        }
        return *w != before;
    }
    let (name, value, index) = match service {
        "set_text" | "add_text_line" => (
            Some("$CONTROL_PAR_TEXT".into()),
            text().map(Value::Text),
            None,
        ),
        "set_knob_label" => (
            Some("$CONTROL_PAR_LABEL".into()),
            text().map(Value::Text),
            None,
        ),
        "set_control_help" => (
            Some("$CONTROL_PAR_HELP".into()),
            text().map(Value::Text),
            None,
        ),
        "set_knob_unit" => (
            Some("$CONTROL_PAR_UNIT".into()),
            arg(1).map(Value::Int),
            None,
        ),
        "set_knob_defval" => (
            Some("$CONTROL_PAR_DEFAULT_VALUE".into()),
            arg(1).map(Value::Int),
            None,
        ),
        "hide_part" => (
            Some("$CONTROL_PAR_HIDE".into()),
            arg(1).map(Value::Int),
            None,
        ),
        "set_table_steps_shown" => (
            Some("table_steps_shown".into()),
            arg(1).map(Value::Int),
            None,
        ),
        "set_control_par" => (
            arg(1).and_then(|p| eval::symbol_in(symbols, p)),
            arg(2).map(Value::Int),
            None,
        ),
        "set_control_par_real" | "set_control_par_real_arr" => (
            arg(1).and_then(|p| eval::symbol_in(symbols, p)),
            args.get(2).map(|&b| Value::Real(f64::from_bits(b as u64))),
            if service.ends_with("_arr") {
                arg(3)
            } else {
                None
            },
        ),
        "set_control_par_str" => (
            arg(1).and_then(|p| eval::symbol_in(symbols, p)),
            text().map(Value::Text),
            None,
        ),
        "set_control_par_arr" => (
            arg(1).and_then(|p| eval::symbol_in(symbols, p)),
            arg(2).map(Value::Int),
            arg(3),
        ),
        "set_control_par_str_arr" => (
            arg(1).and_then(|p| eval::symbol_in(symbols, p)),
            text().map(Value::Text),
            arg(2),
        ),
        _ => return false,
    };
    let (Some(name), Some(mut value)) = (name, value) else {
        return false;
    };
    if let Some(w) = widget {
        if service == "add_text_line"
            && let Value::Text(new) = &mut value
            && let Some(Value::Text(old)) = w.properties.get(&name)
            && !old.is_empty()
        {
            *new = format!("{old}\n{new}");
        }
        if let Some(i) = index {
            let properties = w.indexed_properties.entry(name).or_default();
            if properties.get(&i) == Some(&value) {
                return false;
            }
            properties.insert(i, value);
        } else {
            if w.properties.get(&name) == Some(&value) {
                return false;
            }
            if name == "$CONTROL_PAR_VALUE" {
                match (&value, &mut w.value) {
                    (Value::Int(v), model::WidgetValue::Int(old)) => *old = *v,
                    _ => {}
                }
            }
            w.properties.insert(name, value);
        }
    } else if (builtins::INST_ICON_ID..=builtins::INST_ICON_ID + 5).contains(&id) {
        let properties = model.interface.instrument.entry(id).or_default();
        if properties.get(&name) == Some(&value) {
            return false;
        }
        properties.insert(name, value);
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

/// Name the callback a plan program (`program`, the plan's table index) belongs
/// to among `views` (bound in order): `"script 1 on note"`.
pub fn callback_of(views: &[ScriptView], program: usize) -> String {
    let mut base = 0;
    for (i, v) in views.iter().enumerate() {
        if program < base + v.programs {
            let callback = v.callback(program - base).unwrap_or("callback");
            return format!("script {} {callback}", i + 1);
        }
        base += v.programs;
    }
    format!("program {program}")
}

/// Prepare a live host-state capture off audio. Persistent locations are the
/// actual bound banks; variable names and sigils stay in `ScriptView::model`.
/// Instrument persistence includes both persistence kinds. Snapshot callers
/// may filter instrument-only locations using that authored metadata.
pub fn persistent_state_buffer(
    views: &[ScriptView],
) -> Result<sampler_core::ScriptStateBuffer, sampler_core::Error> {
    use sampler_core::{ScriptStateAddress as A, ScriptStateValue as V};
    let mut state = sampler_core::ScriptStateBuffer::default();
    let mut base = 0;
    for (i, view) in views.iter().enumerate() {
        let instance =
            ScriptInstanceId(u16::try_from(i).map_err(|_| sampler_core::Error::Capacity)?);
        for persistent in &view.model.persistent {
            match persistent.location {
                model::Location::Control(id) => state.values.push(sampler_core::ScriptStateEntry {
                    address: A::Control(id),
                    value: V::Control(sampler_core::ControlValue::Integer(0)),
                }),
                model::Location::Cells { offset, len } => {
                    for index in offset
                        ..offset
                            .checked_add(len)
                            .ok_or(sampler_core::Error::Capacity)?
                    {
                        state.values.push(sampler_core::ScriptStateEntry {
                            address: A::Cell { instance, index },
                            value: V::Cell(0),
                        });
                    }
                }
                model::Location::Texts { offset, len } => {
                    for index in offset
                        ..offset
                            .checked_add(len)
                            .ok_or(sampler_core::Error::Capacity)?
                    {
                        state.values.push(sampler_core::ScriptStateEntry {
                            address: A::Text { instance, index },
                            value: V::Text(sampler_core::Text::new("")),
                        });
                    }
                }
            }
        }
        if let Some(entry) = view
            .entries
            .iter()
            .find(|e| e.kind == EntryKind::PersistenceChanged)
        {
            state.callbacks.push(sampler_core::ScriptStateCallback {
                program: base + entry.program,
                behavior: None,
                outcome: None,
            });
        }
        base += view.programs;
    }
    state.values.sort_by_key(|entry| entry.address);
    state.values.dedup_by_key(|entry| entry.address);
    Ok(state)
}

/// Bind source modules in order through the shared native routing table.
/// Note, release and controller callbacks share native module positions and
/// retain separate instance state and reached-event projections.
pub fn bind_modules(scripts: Vec<Script>, plan: Prepared) -> Result<Prepared, sampler_core::Error> {
    let mut programs = Vec::new();
    let mut instances = Vec::new();
    let mut resources = Vec::new();
    // The plan's effect slot controls stay beside the scripts' own.
    let mut controls = plan.controls().to_vec();
    let mut callbacks = Vec::new();
    let mut widgets = Vec::new();
    let mut stages = Vec::new();
    let mut starts = Vec::new();
    let mut signals = Vec::new();
    let mut parameters = Vec::new();
    let mut shared = Vec::new();
    let mut midi_object = sampler_core::MidiObject::default();
    let mut midi_instances = Vec::new();
    let initial_controllers: Vec<(u8, u8)> = scripts
        .iter()
        .flat_map(|s| s.model().controllers.iter().copied())
        .collect();
    let owns_sustain = scripts.iter().any(|s| s.owns_sustain);
    let owns_release_triggers = scripts.iter().any(|s| s.owns_release_triggers);
    for (index, mut script) in scripts.into_iter().enumerate() {
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
        for w in &script.model.interface.widgets {
            use model::{Location, WidgetValue};
            let storage = if w.kind == model::WidgetKind::FileSelector {
                let offset = u32::try_from(script.resources.texts.len())
                    .map_err(|_| sampler_core::Error::Capacity)?;
                script.resources.texts.push(String::new());
                Some(sampler_core::WidgetStorage::FileSelection { offset })
            } else if let Some(id) = w.control {
                Some(sampler_core::WidgetStorage::Control(id))
            } else {
                w.location.as_ref().and_then(|location| match *location {
                    Location::Cells { offset, len } => Some(sampler_core::WidgetStorage::Cells {
                        offset,
                        len,
                        real: matches!(w.value, WidgetValue::Reals(_)),
                        min: if matches!(w.value, WidgetValue::Reals(_)) {
                            0.
                        } else {
                            -(w.params.get(2).copied().unwrap_or(i32::MAX).unsigned_abs() as f64)
                        },
                        max: if matches!(w.value, WidgetValue::Reals(_)) {
                            1.
                        } else {
                            w.params.get(2).copied().unwrap_or(i32::MAX).unsigned_abs() as f64
                        },
                    }),
                    Location::Texts { offset, len } => {
                        Some(sampler_core::WidgetStorage::Texts { offset, len })
                    }
                    _ => None,
                })
            };
            if let Some(storage) = storage {
                let drop = if w.kind == model::WidgetKind::MouseArea {
                    let texts = u32::try_from(script.resources.texts.len())
                        .map_err(|_| sampler_core::Error::Capacity)?;
                    let counts = u32::try_from(script.cells.len())
                        .map_err(|_| sampler_core::Error::Capacity)?;
                    script.resources.texts.resize(
                        script.resources.texts.len()
                            + 3 * sampler_core::WIDGET_DROP_CAPACITY as usize,
                        String::new(),
                    );
                    script.cells.resize(script.cells.len() + 3, 0);
                    Some(sampler_core::WidgetDropStorage {
                        texts,
                        counts,
                        accepts: [
                            "$CONTROL_PAR_DND_ACCEPT_AUDIO",
                            "$CONTROL_PAR_DND_ACCEPT_MIDI",
                            "$CONTROL_PAR_DND_ACCEPT_ARRAY",
                        ]
                        .map(|name| {
                            [
                                w.ui_id,
                                builtins::control_par(name).unwrap(),
                                lower::PROPERTY_TAG,
                                lower::PROPERTY_TAG,
                            ]
                        }),
                        receive_drag: [
                            w.ui_id,
                            builtins::control_par("$CONTROL_PAR_RECEIVE_DRAG_EVENTS").unwrap(),
                            lower::PROPERTY_TAG,
                            lower::PROPERTY_TAG,
                        ],
                    })
                } else {
                    None
                };
                widgets.push(sampler_core::WidgetDefinition {
                    id: derived_control_id(script.slot, &w.name),
                    source_slot: script.slot,
                    ui_id: w.ui_id,
                    instance,
                    storage,
                    drop,
                    program: script
                        .routed(EntryKind::UiControl(
                            (w.ui_id - builtins::FIRST_UI_ID) as usize,
                        ))
                        .map(|p| base + p),
                    stage: index,
                });
            }
        }
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
                .filter(|e| matches!(e.kind, EntryKind::PgsChanged | EntryKind::AsyncComplete))
                .map(|e| sampler_core::SignalProgram {
                    signal: if e.kind == EntryKind::AsyncComplete {
                        sampler_core::MIDI_ASYNC_SIGNAL
                    } else {
                        lower::PGS_SIGNAL
                    },
                    program: base + e.program,
                    stage: index,
                }),
        );
        parameters.extend(script.entries.iter().filter_map(|e| {
            let kind = match e.kind {
                EntryKind::Rpn => sampler_core::ParameterKind::Rpn,
                EntryKind::Nrpn => sampler_core::ParameterKind::Nrpn,
                _ => return None,
            };
            Some(sampler_core::ParameterProgram {
                kind,
                program: base + e.program,
                stage: index,
            })
        }));
        // Keys created by several scripts keep the first script's values.
        shared.extend(script.shared.iter().copied());
        starts.extend(script.starts.iter().map(|&p| sampler_core::PlanProgram {
            program: base + p,
            stage: index,
        }));
        midi_instances.push((script.slot, instance));
        midi_object = script.midi_object;
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
    midi_object.bind_initial_instances(&midi_instances)?;
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
        .with_parameter_programs(parameters)?
        .with_widgets(widgets)?
        .with_midi_object(midi_object)
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
    initialize(source, limits, environment).map(|initialized| initialized.engine_pars())
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
    compile_inner(source, rate, limits, controls, environment)
}

fn compile_inner(
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
    let initialized = initialize(source, limits, environment)?;
    compile_initialized(source, rate, limits, controls, initialized)
}

/// Rate-independent frontend state after one resource-aware `on init`.
/// Consumed by `compile_initialized`; it is never shared between instances.
pub struct Initialized {
    hir: hir::Hir,
    init: eval::Initial,
    conditions: BTreeSet<String>,
    environment: Environment,
    #[cfg(feature = "scan")]
    observation: scan::Checkpoint,
}

impl Initialized {
    pub fn midi_object(&self) -> &sampler_core::MidiObject {
        &self.init.midi_object
    }
    pub fn engine_pars(&self) -> Vec<EnginePar> {
        let mut writes: Vec<_> = self
            .init
            .engine
            .iter()
            .map(|(&[parameter, group, slot, generic], &value)| EnginePar {
                parameter: eval::symbol_name(&self.hir, parameter)
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
        writes
    }

    /// Conservatively retain addressable effect slots when runtime code writes
    /// engine parameters. No initializer or callback lowering is run to query it.
    pub fn writes_effect_slots(&self) -> bool {
        fn arg(a: &hir::Arg) -> bool {
            match a {
                hir::Arg::Expr(e) => expr(e),
                hir::Arg::Place(hir::Place::Elem(_, e)) => expr(e),
                _ => false,
            }
        }
        fn expr(e: &hir::Expr) -> bool {
            use hir::ExprKind as E;
            match &e.kind {
                E::Builtin(builtin, args) => {
                    *builtin == builtins::Builtin::SetEnginePar || args.iter().any(arg)
                }
                E::Neg(e)
                | E::BitNot(e)
                | E::Not(e)
                | E::Cast(e)
                | E::LoadElem(_, e)
                | E::SysElem(_, e) => expr(e),
                E::Arith(_, a, b) | E::Compare(_, a, b) | E::Logic(_, a, b) => expr(a) || expr(b),
                E::Concat(es) => es.iter().any(expr),
                _ => false,
            }
        }
        fn writes(body: &[hir::Stmt]) -> bool {
            body.iter().any(|s| match &s.kind {
                hir::StmtKind::Builtin(builtin, args) => {
                    *builtin == builtins::Builtin::SetEnginePar || args.iter().any(arg)
                }
                hir::StmtKind::Assign(place, value) => {
                    expr(value) || matches!(place, hir::Place::Elem(_, e) if expr(e))
                }
                hir::StmtKind::Fill(_, values) => values.iter().any(expr),
                hir::StmtKind::If(e, yes, no) => expr(e) || writes(yes) || writes(no),
                hir::StmtKind::While(e, body) => expr(e) || writes(body),
                hir::StmtKind::Select(e, cases) => expr(e) || cases.iter().any(|c| writes(&c.body)),
                _ => false,
            })
        }
        // ponytail: conservatively retain slots; precise function reachability if RAM matters.
        // Functions can be called by runtime callbacks. Conservative admission
        // avoids dropping a slot reached indirectly or through a variable.
        self.hir
            .callbacks
            .iter()
            .filter(|c| c.kind != hir::CallbackKind::Init)
            .any(|c| writes(&c.body))
            || self.hir.functions.iter().any(|f| writes(&f.body))
    }
}

pub fn initialize(
    source: &str,
    limits: Limits,
    environment: &Environment,
) -> Result<Initialized, Error> {
    #[cfg(feature = "scan")]
    scan::reset_script();
    let result = initialize_inner(source, limits, environment);
    #[cfg(feature = "scan")]
    if result.is_err() {
        scan::record(&result, source, environment.slot);
    }
    result
}
fn initialize_inner(
    source: &str,
    limits: Limits,
    environment: &Environment,
) -> Result<Initialized, Error> {
    let audit_begin = std::time::Instant::now();
    if source.len() > limits.source_bytes {
        return Err(Error {
            offset: 0,
            line: 1,
            column: 1,
            kind: diag::Kind::Error,
            builtin: None,
            message: "source byte budget exceeded".into(),
        });
    }
    let mut syms = lexer::Interner::default();
    let (hir, init, conditions) = (|| {
        let mut toks = lexer::lex(source, &mut syms)?;
        #[cfg(feature = "scan")]
        scan::stage("preprocess");
        let conditions = lexer::preprocess(&mut toks, &syms, &Default::default())?;
        #[cfg(feature = "scan")]
        scan::stage("parse");
        let ast = parser::parse(&toks, &syms)?;
        let budget = sema::Budget {
            variables: limits.variables,
            array_cells: limits.array_cells,
        };
        #[cfg(feature = "scan")]
        scan::stage("sema");
        let hir = sema::analyze(ast, &syms, budget, &environment.performance_view.controls)?;
        if std::env::var_os("KONTRA_AUDIT_LOAD").is_some() {
            eprintln!(
                "AUDIT {{\"stage\":\"ksp_frontend\",\"ms\":{}}}",
                audit_begin.elapsed().as_secs_f64() * 1000.
            );
        }
        let init_begin = std::time::Instant::now();
        let init = eval::run(&hir, environment);
        #[cfg(feature = "scan")]
        scan::initialized(init.is_ok());
        let init = init?;
        if std::env::var_os("KONTRA_AUDIT_LOAD").is_some() {
            eprintln!(
                "AUDIT {{\"stage\":\"ksp_on_init\",\"ms\":{}}}",
                init_begin.elapsed().as_secs_f64() * 1000.
            );
        }
        Ok((hir, init, conditions))
    })()
    .map_err(|f: diag::Fault| f.locate(source))?;
    Ok(Initialized {
        hir,
        init,
        conditions,
        environment: environment.clone(),
        #[cfg(feature = "scan")]
        observation: scan::checkpoint(),
    })
}

/// Lower callbacks at the actual host rate, consuming the initialized state.
pub fn compile_initialized(
    source: &str,
    rate: u32,
    limits: Limits,
    controls: &[(&str, ControlId)],
    initialized: Initialized,
) -> Result<Script, Error> {
    #[cfg(feature = "scan")]
    let slot = initialized.environment.slot;
    #[cfg(feature = "scan")]
    scan::restore(initialized.observation.clone());
    #[cfg(feature = "scan")]
    scan::stage("lower");
    let result = compile_initialized_inner(source, rate, limits, controls, initialized);
    #[cfg(feature = "scan")]
    scan::record(&result, source, slot);
    result
}
fn compile_initialized_inner(
    source: &str,
    rate: u32,
    limits: Limits,
    controls: &[(&str, ControlId)],
    initialized: Initialized,
) -> Result<Script, Error> {
    let error = |message: &str| Error {
        offset: 0,
        line: 1,
        column: 1,
        kind: diag::Kind::Error,
        builtin: None,
        message: message.into(),
    };
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
    let Initialized {
        hir,
        init,
        conditions,
        environment,
        #[cfg(feature = "scan")]
            observation: _,
    } = initialized;

    #[cfg(feature = "scan")]
    scan::stage("lower");
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
    let lower_begin = std::time::Instant::now();
    let mut unit = lower::Unit {
        hir: &hir,
        controls: &ids,
        groups: &environment.groups,
        slot: environment.slot,
        budget: limits.instructions,
        limit: limits.instructions,
        services: Vec::new(),
        array_files: BTreeSet::new(),
        coverage: BTreeMap::new(),
        warnings: Vec::new(),
        scratch: 0,
        modules: Vec::new(),
    };
    let mut programs = Vec::new();
    let mut entries = Vec::new();
    let mut starts = Vec::new();
    let profile_lower = std::env::var_os("KONTRA_AUDIT_LOWER").is_some();
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
            K::UiControls => (EntryKind::UiControls, lower::Context::Plan),
            K::UiUpdate => (EntryKind::UiUpdate, lower::Context::Plan),
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
            let started = profile_lower.then(std::time::Instant::now);
            let remaining = unit.budget;
            let program = unit
                .program(
                    programs.len(),
                    &callback.body,
                    callback.span,
                    context,
                    callback.kind,
                    signal,
                )
                .map_err(|f| f.locate(source))?;
            if let Some(started) = started {
                eprintln!(
                    "AUDIT {{\"stage\":\"ksp_callback_program\",\"context\":\"{context:?}\",\"ms\":{},\"instructions\":{}}}",
                    started.elapsed().as_secs_f64() * 1000.,
                    remaining - unit.budget,
                );
            }
            entries.push(Entry {
                kind,
                program: programs.len(),
            });
            programs.push(program);
            if kind == EntryKind::PersistenceChanged
                && init.model.persistence_completion == model::PersistenceCompletion::Scheduled
            {
                starts.push(programs.len() - 1);
            }
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
    unit.finish(&mut programs).map_err(|f| f.locate(source))?;
    let global_ui = entries
        .iter()
        .find(|e| e.kind == EntryKind::UiControls)
        .map(|e| e.program);
    let ui_update = entries
        .iter()
        .find(|e| e.kind == EntryKind::UiUpdate)
        .map(|e| e.program);
    if global_ui.is_some() || ui_update.is_some() {
        for (ui, _) in hir.uis.iter().enumerate() {
            let local = entries
                .iter()
                .find(|e| e.kind == EntryKind::UiControl(ui))
                .map(|e| e.program);
            let mut targets = Vec::new();
            if let Some(global) = global_ui {
                let clone = programs.len();
                programs.push(
                    programs[global]
                        .clone()
                        .with_callback_ui_id(builtins::FIRST_UI_ID + ui as i32),
                );
                entries.push(Entry {
                    kind: EntryKind::UiControls,
                    program: clone,
                });
                targets.push(clone);
            }
            targets.extend(local);
            targets.extend(ui_update);
            let mut code = Vec::with_capacity(targets.len() + 1);
            for target in targets {
                let program =
                    u32::try_from(target).map_err(|_| error("too many UI callback programs"))?;
                code.push(sampler_core::Instruction::StartProgram { program });
            }
            code.push(sampler_core::Instruction::End);
            if unit.budget < code.len() {
                return Err(error("UI dispatcher instruction budget exceeded"));
            }
            unit.budget -= code.len();
            let dispatcher = programs.len();
            programs.push(
                Program::new(code)
                    .map_err(|_| error("invalid UI callback dispatcher"))?
                    .with_wait_lifetime(sampler_core::WaitLifetime::Callback)
                    .with_source_slot(environment.slot),
            );
            if let Some(entry) = entries
                .iter_mut()
                .find(|e| e.kind == EntryKind::UiControl(ui))
            {
                entry.program = dispatcher;
                entries.push(Entry {
                    kind: EntryKind::UiControl(ui),
                    program: local.unwrap(),
                });
            } else {
                entries.push(Entry {
                    kind: EntryKind::UiControl(ui),
                    program: dispatcher,
                });
            }
        }
    }
    // Replay init through the same addressed service as every callback.
    let mut start: Vec<_> = init
        .engine
        .iter()
        .map(|(&key, &value)| (key, value))
        .collect();
    start.sort_by_key(|(key, _)| *key);
    let purges: Vec<_> = init
        .model
        .requests
        .iter()
        .filter(|r| r.command == "purge_group")
        .filter_map(|r| match r.args.as_slice() {
            [Value::Int(group), Value::Int(value)] => Some((*group, *value)),
            _ => None,
        })
        .collect();
    if !start.is_empty() || !purges.is_empty() {
        // Authored init writes must precede live persistence and listener callbacks.
        starts.insert(0, programs.len());
        programs.push(
            unit.engine_start(&start, &purges)
                .map_err(|f| f.locate(source))?,
        );
    }
    for (ui, control) in &mut host {
        control.callback = entries
            .iter()
            .find(|e| e.kind == EntryKind::UiControl(*ui))
            .map(|e| e.program);
    }

    if std::env::var_os("KONTRA_AUDIT_LOAD").is_some() {
        eprintln!(
            "AUDIT {{\"stage\":\"ksp_callback_lower\",\"ms\":{}}}",
            lower_begin.elapsed().as_secs_f64() * 1000.
        );
    }
    let state_begin = std::time::Instant::now();
    // Initial instance state: texts plus lowering scratch, the property /
    // engine / PGS mirror, and the dense control table.
    let mut texts = init.texts.clone();
    texts.resize(texts.len() + unit.scratch as usize, String::new());
    let mut store = Vec::new();
    waveform::seed(&hir, &init, &environment, &mut store)
        .map_err(|_| error("waveform initial attachment has no admitted physical source"))?;
    let mut text_properties: Vec<_> = init
        .text_properties
        .iter()
        .map(|(&(id, par), text)| ([id, par, PROPERTY_TAG, PROPERTY_TAG], text.clone()))
        .collect();
    for (ui, definition) in hir.uis.iter().enumerate() {
        let id = builtins::FIRST_UI_ID + ui as i32;
        let mut properties = vec![(builtins::CONTROL_PAR_TYPE, definition.kind.control_type())];
        if let Some((low, high)) = eval::declared_range(definition) {
            properties.extend([
                (builtins::CONTROL_PAR_MIN_VALUE, low),
                (builtins::CONTROL_PAR_MAX_VALUE, high),
            ]);
        }
        for (parameter, value) in properties {
            if !init.properties.contains_key(&(id, parameter)) {
                store.push((
                    [id, parameter, PROPERTY_TAG, PROPERTY_TAG],
                    i64::from(value),
                ));
            }
        }
        if definition.kind != model::WidgetKind::Menu {
            continue;
        }
        let items = init
            .model
            .interface
            .widgets
            .get(ui)
            .map_or(&[][..], |widget| widget.menu.as_slice());
        store.push((
            [id, lower::MENU_COUNT, -1, lower::MENU_TAG],
            items.len() as i64,
        ));
        for (index, item) in items.iter().enumerate() {
            for (field, value) in [
                (lower::MENU_VALUE, i64::from(item.value)),
                (lower::MENU_VISIBLE, i64::from(item.visible)),
            ] {
                store.push(([id, field, index as i32, lower::MENU_TAG], value));
            }
            text_properties.push((
                [id, lower::MENU_TEXT, index as i32, lower::MENU_TAG],
                item.text.clone(),
            ));
        }
    }
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
    for (&(id, par, index), value) in &init.indexed_properties {
        if let Value::Int(value) = value {
            store.push(([id, par, index, PROPERTY_TAG], i64::from(*value)));
        }
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
    let array_files = unit
        .array_files
        .iter()
        .map(|&v| array_file::array(&hir, v))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| error("NKA typed array exceeds explicit-path service limits"))?;
    if !array_files.is_empty() && texts.iter().any(|s| s.len() > sampler_core::TEXT_CAPACITY) {
        return Err(error("NKA script initial text exceeds runtime capacity"));
    }
    let resources = ScriptResources {
        array_files,
        texts,
        text_properties,
        store,
        store_capacity,
        controls: ids.clone(),
    };

    if std::env::var_os("KONTRA_AUDIT_LOAD").is_some() {
        eprintln!(
            "AUDIT {{\"stage\":\"ksp_state_mirror\",\"ms\":{}}}",
            state_begin.elapsed().as_secs_f64() * 1000.
        );
    }
    let warnings_begin = std::time::Instant::now();
    let mut findings: Vec<_> = hir
        .warnings
        .iter()
        .chain(&init.warnings)
        .map(|f| (f, diag::Kind::Warning))
        .chain(unit.warnings.iter().map(|(f, k)| (f, *k)))
        .collect();
    // Builtin findings first so the cap never hides an unsupported builtin.
    // Cap before positioning; one source scan serves all retained findings.
    findings.sort_by_key(|(f, kind)| (*kind == Kind::Warning, f.span.start));
    findings.truncate(1000);
    let line_starts: Vec<_> = std::iter::once(0)
        .chain(
            source
                .bytes()
                .enumerate()
                .filter_map(|(i, b)| (b == b'\n').then_some(i + 1)),
        )
        .collect();
    let warnings: Vec<Error> = findings
        .into_iter()
        .map(|(f, kind)| f.clone().locate_indexed(source, kind, &line_starts))
        .collect();
    if std::env::var_os("KONTRA_AUDIT_LOAD").is_some() {
        eprintln!(
            "AUDIT {{\"stage\":\"ksp_diagnostics\",\"ms\":{},\"warnings\":{}}}",
            warnings_begin.elapsed().as_secs_f64() * 1000.,
            warnings.len()
        );
    }
    let model_begin = std::time::Instant::now();
    let services = unit.services.iter().map(|b| b.name()).collect();
    let coverage = unit
        .coverage
        .iter()
        .map(|(&(name, c), &n)| (name, c, n))
        .collect();
    let model = model::assemble(&hir, &init, &ids, &entries);
    if std::env::var_os("KONTRA_AUDIT_LOAD").is_some() {
        eprintln!(
            "AUDIT {{\"stage\":\"ksp_model_assemble\",\"ms\":{},\"widgets\":{},\"instructions\":{}}}",
            model_begin.elapsed().as_secs_f64() * 1000.,
            model.interface.widgets.len(),
            limits.instructions - unit.budget
        );
    }
    Ok(Script {
        midi_object: init.midi_object,
        programs: programs
            .into_iter()
            .map(|p| {
                p.with_engine_symbols(
                    hir.symbols
                        .iter()
                        .enumerate()
                        .filter_map(|(i, name)| {
                            sampler_core::engine_parameter_id(name)
                                .map(|parameter| (hir::OPAQUE_BASE + i as i32, parameter))
                        })
                        .collect(),
                )
            })
            .collect(),
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
