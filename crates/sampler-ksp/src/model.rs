//! Plain-data result of `on init`: the script's interface, persistence and
//! instrument-side requests. Presentation code reads this; it owns no state.
use sampler_core::ControlId;
use std::collections::BTreeMap;

pub use crate::hir::{Persistence, WidgetKind};

#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Int(i32),
    Real(f64),
    Text(String),
}

/// A widget's script-visible value after `on init`.
#[derive(Clone, Debug, PartialEq)]
pub enum WidgetValue {
    /// Value with no script variable behind it (panel, mouse area, ...).
    None,
    Int(i32),
    Text(String),
    /// `ui_table` columns.
    Ints(Vec<i32>),
    /// `ui_xy` cursor coordinates.
    Reals(Vec<f64>),
}

/// A control declared by a Creator Tools performance view file rather than by
/// the script; the host reads the `.nckp` and passes these in `Environment`.
#[derive(Clone, Debug, PartialEq)]
pub struct PerformanceControl {
    /// Variable name with its type prefix, e.g. `$Cutoff` or `%Steps`.
    pub name: String,
    pub kind: WidgetKind,
    /// Declaration parameters as a `declare ui_*` would take them.
    pub params: Vec<i32>,
    /// Element count for array controls (`ui_table`, `ui_xy`).
    pub len: u32,
    /// `$CONTROL_PAR_*` values the view sets, applied when the script calls
    /// `load_performance_view`. `$CONTROL_PAR_PARENT_PANEL` names the panel
    /// variable as text.
    pub properties: Vec<(String, Value)>,
    pub menu: Vec<MenuItem>,
}

/// A Creator Tools performance view: its controls in declaration order (UI
/// ids are consecutive in this order, as in Kontakt) and page settings.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PerformanceView {
    pub controls: Vec<PerformanceControl>,
    pub width: Option<i32>,
    pub height: Option<i32>,
    /// `0xRRGGBB` page colour.
    pub color: Option<i32>,
    /// Picture names, as `$CONTROL_PAR_PICTURE` takes them.
    pub wallpaper: Option<String>,
    pub icon: Option<String>,
}
impl PerformanceControl {
    /// What a control the host did not describe is assumed to be.
    // ponytail: guessed shape; the .nckp reader replaces these with real kinds.
    pub fn assumed(name: &str, kind: WidgetKind) -> Self {
        let (params, len) = match kind {
            // Kontakt's 0..1000000 parameter range.
            WidgetKind::Knob | WidgetKind::Slider => (vec![0, 1_000_000], 0),
            WidgetKind::ValueEdit => (vec![0, 1_000_000, 1], 0),
            WidgetKind::Table => (vec![100, 100, 100], 128),
            _ => (vec![], 0),
        };
        Self {
            name: name.into(),
            kind,
            params,
            len,
            properties: Vec::new(),
            menu: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct MenuItem {
    pub text: String,
    pub value: i32,
    pub visible: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Widget {
    /// Unbound source handle; never a visible performance-view widget.
    pub unresolved: bool,
    /// Script variable, including its type prefix.
    pub name: String,
    pub kind: WidgetKind,
    /// The value `get_ui_id` returns.
    pub ui_id: i32,
    /// Host control owning the value (knob, slider, button, switch, menu, value edit).
    pub control: Option<ControlId>,
    pub value: WidgetValue,
    /// Folded declaration parameters, e.g. `(min, max, display_ratio)`.
    pub params: Vec<i32>,
    /// Value range for host controls.
    pub range: Option<(i32, i32)>,
    /// Every `$CONTROL_PAR_*` written in `on init`, keyed by its KSP name.
    pub properties: BTreeMap<String, Value>,
    /// Every `set_control_par*_arr` write in `on init`: KSP name -> index -> value
    /// (table cells, label lines, wavetable/waveform per-index parameters).
    pub indexed_properties: BTreeMap<String, BTreeMap<i32, Value>>,
    pub menu: Vec<MenuItem>,
    /// Entry point of `on ui_control`, an index into `Script::entries`.
    pub callback: Option<usize>,
    pub persistence: Persistence,
    pub location: Option<Location>,
}
impl Widget {
    pub fn int(&self, property: &str) -> Option<i32> {
        match self.properties.get(property)? {
            Value::Int(n) => Some(*n),
            _ => None,
        }
    }
    pub fn text(&self, property: &str) -> Option<&str> {
        match self.properties.get(property)? {
            Value::Text(s) => Some(s),
            _ => None,
        }
    }
    /// Pixel position, when the script placed the widget.
    pub fn position(&self) -> Option<(i32, i32)> {
        Some((
            self.int("$CONTROL_PAR_POS_X")?,
            self.int("$CONTROL_PAR_POS_Y")?,
        ))
    }
    pub fn hidden(&self) -> bool {
        self.int("$CONTROL_PAR_HIDE")
            .is_some_and(|h| h & crate::builtins::HIDE_WHOLE_CONTROL != 0)
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Key {
    pub color: Option<i32>,
    pub kind: Option<i32>,
    pub name: Option<String>,
    pub pressed: Option<i32>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct KeyRange {
    pub low: i32,
    pub high: i32,
    pub name: String,
}

/// Where a persistent variable's value lives.
#[derive(Clone, Debug, PartialEq)]
pub enum Location {
    Control(ControlId),
    /// Script cells in this script's instance bank (reals are IEEE-754 bits).
    Cells {
        offset: u32,
        len: u32,
    },
    /// Text cells in this script's instance bank.
    Texts {
        offset: u32,
        len: u32,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub struct Persistent {
    pub name: String,
    pub kind: Persistence,
    pub location: Location,
}

/// Instrument-side request made by `on init`, kept in order for the host.
#[derive(Clone, Debug, PartialEq)]
pub struct Request {
    /// Builtin name, e.g. `set_engine_par`.
    pub command: &'static str,
    pub args: Vec<Value>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Interface {
    pub title: Option<String>,
    pub performance_view: bool,
    pub width_px: Option<i32>,
    pub height_px: Option<i32>,
    pub height_grid: Option<i32>,
    pub skin_offset: Option<i32>,
    pub widgets: Vec<Widget>,
    /// Instrument icon/wallpaper pseudo-controls keyed by ui id.
    pub instrument: BTreeMap<i32, BTreeMap<String, Value>>,
    pub keys: Vec<Key>,
    pub key_ranges: Vec<KeyRange>,
    pub key_pressed_support: bool,
    pub messages: Vec<String>,
    pub fonts: Vec<String>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Model {
    pub interface: Interface,
    pub persistent: Vec<Persistent>,
    /// `set_listener` signals and their parameters.
    pub listeners: BTreeMap<i32, i32>,
    /// PGS integer keys created by this script with their initial values.
    pub pgs: BTreeMap<String, Vec<i32>>,
    pub pgs_text: BTreeMap<String, String>,
    pub requests: Vec<Request>,
    /// Controllers `on init` set (`set_controller`), as (controller, 0..=127).
    pub controllers: Vec<(u8, u8)>,
}

/// Assemble the model from resolved declarations and the `on init` result.
pub(crate) fn assemble(
    hir: &crate::hir::Hir,
    init: &crate::eval::Initial,
    controls: &[Option<ControlId>],
    entries: &[crate::Entry],
) -> Model {
    use crate::builtins as b;
    use crate::hir::{Home, Ty};
    let mut model = init.model.clone();
    let name = |par: i32| crate::eval::symbol_name(hir, par).unwrap_or_else(|| par.to_string());
    let mut properties: BTreeMap<i32, BTreeMap<String, Value>> = BTreeMap::new();
    for (&(id, par), &v) in &init.properties {
        properties
            .entry(id)
            .or_default()
            .insert(name(par), Value::Int(v));
    }
    for ((id, par), v) in &init.text_properties {
        properties
            .entry(*id)
            .or_default()
            .insert(name(*par), Value::Text(v.clone()));
    }
    let mut indexed: BTreeMap<i32, BTreeMap<String, BTreeMap<i32, Value>>> = BTreeMap::new();
    for (&(id, par, index), v) in &init.indexed_properties {
        indexed
            .entry(id)
            .or_default()
            .entry(name(par))
            .or_default()
            .insert(index, v.clone());
    }
    let real = |bits: i64| f64::from_bits(bits as u64);
    let mut widgets = Vec::with_capacity(hir.uis.len());
    for (i, ui) in hir.uis.iter().enumerate() {
        let var = &hir.vars[ui.var.0 as usize];
        let ui_id = b::FIRST_UI_ID + i as i32;
        let value = match (var.home, var.ty) {
            (Home::Control(_), _) => WidgetValue::Int(init.controls[i]),
            (Home::Cell(c), Ty::Real) => WidgetValue::Reals(vec![real(init.cells[c as usize])]),
            (Home::Cell(c), _) => WidgetValue::Int(init.cells[c as usize] as i32),
            (Home::Text(c), _) => WidgetValue::Text(init.texts[c as usize].clone()),
            (Home::Cells { offset, len }, Ty::Real) => WidgetValue::Reals(
                init.cells[offset as usize..(offset + len) as usize]
                    .iter()
                    .map(|&b| real(b))
                    .collect(),
            ),
            (Home::Cells { offset, len }, _) => WidgetValue::Ints(
                init.cells[offset as usize..(offset + len) as usize]
                    .iter()
                    .map(|&v| v as i32)
                    .collect(),
            ),
            _ => WidgetValue::None,
        };
        widgets.push(Widget {
            unresolved: ui.unresolved,
            name: var.name.to_string(),
            kind: ui.kind,
            ui_id,
            control: controls[i],
            value,
            params: ui.params.clone(),
            range: crate::eval::declared_range(ui).map(|(a, b)| (a.min(b), a.max(b))),
            properties: properties.remove(&ui_id).unwrap_or_default(),
            indexed_properties: indexed.remove(&ui_id).unwrap_or_default(),
            menu: init
                .model
                .interface
                .widgets
                .get(i)
                .map(|w| w.menu.clone())
                .unwrap_or_default(),
            callback: entries
                .iter()
                .position(|e| e.kind == crate::EntryKind::UiControl(i)),
            persistence: var.persistence,
            location: match var.home {
                Home::Control(ui) => controls[ui as usize].map(Location::Control),
                Home::Cell(c) => Some(Location::Cells { offset: c, len: 1 }),
                Home::Cells { offset, len } => Some(Location::Cells { offset, len }),
                Home::Text(c) => Some(Location::Texts { offset: c, len: 1 }),
                Home::Texts { offset, len } => Some(Location::Texts { offset, len }),
                _ => None,
            },
        });
    }
    model.interface.widgets = widgets;
    model.interface.instrument = properties
        .into_iter()
        .filter(|(id, _)| (b::INST_ICON_ID..=b::INST_ICON_ID + 5).contains(id))
        .collect();
    model.persistent = hir
        .vars
        .iter()
        .filter(|v| v.persistence != Persistence::None)
        .filter_map(|v| {
            let location = match v.home {
                Home::Control(ui) => Location::Control(controls[ui as usize]?),
                Home::Cell(c) => Location::Cells { offset: c, len: 1 },
                Home::Cells { offset, len } => Location::Cells { offset, len },
                Home::Text(c) => Location::Texts { offset: c, len: 1 },
                Home::Texts { offset, len } => Location::Texts { offset, len },
                Home::Note(_) | Home::Const(_) => return None,
            };
            Some(Persistent {
                name: v.name.to_string(),
                kind: v.persistence,
                location,
            })
        })
        .collect();
    model
}
