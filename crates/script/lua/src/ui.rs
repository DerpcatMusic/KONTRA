//! The interface a script builds as it loads (`__ui` in the prelude), read
//! out as plain data for the editor, and the live reads of its widgets the
//! engine makes for each look.

use std::collections::HashMap;

use kontra_vm::{Text, WidgetLook};
use mlua::{Lua, LuaString, Table, Value};

/// The page a script built: its size, picture and colour, and its widgets
/// in the order it made them (a widget's index names it to the engine).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Ui {
    /// The script asked for its own performance view.
    pub performance: bool,
    pub width: f64,
    pub height: f64,
    /// A picture path relative to the script's folder.
    pub background: Option<String>,
    /// `RRGGBB` or `AARRGGBB` hex.
    pub colour: Option<String>,
    pub widgets: Vec<Widget>,
}

/// One widget as the script left it after loading.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Widget {
    /// The constructor: `Knob`, `Menu`, `OnOffButton`, ...
    pub kind: String,
    pub name: String,
    /// The panel it was made in, by index.
    pub parent: Option<usize>,
    /// x, y, width, height, relative to the parent.
    pub rect: [f64; 4],
    /// Whether the script gave the width and height.
    pub sized: [bool; 2],
    pub value: f64,
    pub min: f64,
    pub max: f64,
    pub default: Option<f64>,
    pub integer: bool,
    pub vertical: bool,
    pub items: Vec<String>,
    /// What a label or button says, when the script set it (else its
    /// name, unless a picture draws it).
    pub text: String,
    pub tooltip: String,
    pub unit: String,
    pub visible: bool,
    pub enabled: bool,
    pub alpha: f64,
    pub persistent: bool,
    /// Pictures by role, relative to the script's folder.
    pub image: Option<String>,
    pub normal: Option<String>,
    pub pressed: Option<String>,
    pub background: Option<String>,
    /// A strip of value frames, and how many.
    pub strip: Option<(String, u32)>,
    /// Hex colours by field: background, text, fill.
    pub colours: [Option<String>; 3],
    pub cells: Vec<f64>,
    pub slider_colour: Option<String>,
    pub inner_edge: Option<bool>,
    pub fill_style: Option<String>,
    pub font_size: Option<f64>,
    pub font: Option<String>,
    pub align: Option<String>,
}

/// A Lua number or boolean, as a number.
fn number(v: &Value) -> Option<f64> {
    match v {
        Value::Integer(i) => Some(*i as f64),
        Value::Number(n) => Some(*n),
        Value::Boolean(b) => Some(f64::from(u8::from(*b))),
        _ => None,
    }
}

/// Reads `__ui` from a state whose script body ran.
pub(crate) fn read(lua: &Lua) -> Ui {
    let Ok(root) = lua.globals().raw_get::<Table>("__ui") else {
        return Ui::default();
    };
    let text = |t: &Table, k: &str| match t.raw_get::<Value>(k) {
        Ok(Value::String(s)) => Some(s.to_string_lossy()),
        Ok(v @ (Value::Integer(_) | Value::Number(_))) => number(&v).map(|n| n.to_string()),
        _ => None,
    };
    let num = |t: &Table, k: &str| number(&t.raw_get::<Value>(k).ok()?);
    let flag = |t: &Table, k: &str, or: bool| match t.raw_get::<Value>(k) {
        Ok(Value::Boolean(b)) => b,
        _ => or,
    };
    let list: Vec<Table> = root
        .raw_get::<Table>("widgets")
        .map(|t| t.sequence_values::<Table>().flatten().collect())
        .unwrap_or_default();
    let index: HashMap<usize, usize> = (list.iter().enumerate())
        .map(|(n, t)| (t.to_pointer() as usize, n))
        .collect();
    let widgets = list
        .iter()
        .map(|w| {
            let kind = text(w, "kind").unwrap_or_default();
            let name = text(w, "name").unwrap_or_default();
            let parent = match w.raw_get::<Value>("__parent") {
                Ok(Value::Table(p)) => index.get(&(p.to_pointer() as usize)).copied(),
                _ => None,
            };
            let strip = text(w, "stripImage")
                .map(|p| (p, num(w, "stripSteps").map_or(1, |n| n.max(1.) as u32)));
            let items = w
                .raw_get::<Table>("items")
                .map(|t| t.sequence_values::<String>().flatten().collect())
                .unwrap_or_default();
            let caption = text(w, "text")
                .filter(|t| !t.is_empty())
                .or_else(|| text(w, "displayName"))
                .unwrap_or_default();
            Widget {
                parent,
                rect: ["x", "y", "width", "height"].map(|k| num(w, k).unwrap_or(0.)),
                sized: [flag(w, "width_set", false), flag(w, "height_set", false)],
                value: num(w, "value").unwrap_or(0.),
                min: num(w, "min").unwrap_or(0.),
                max: num(w, "max").unwrap_or(1.),
                default: num(w, "default"),
                integer: flag(w, "integer", false),
                vertical: flag(w, "vertical", false),
                items,
                text: caption,
                tooltip: text(w, "tooltip").unwrap_or_default(),
                unit: text(w, "unit").unwrap_or_default(),
                visible: flag(w, "visible", true),
                enabled: flag(w, "enabled", true),
                alpha: num(w, "alpha").unwrap_or(1.),
                persistent: flag(w, "persistent", true),
                image: text(w, "image"),
                normal: text(w, "normalImage"),
                pressed: text(w, "pressedImage"),
                background: text(w, "backgroundImage"),
                strip,
                colours: ["backgroundColour", "textColour", "fillColour"].map(|k| text(w, k)),
                cells: w
                    .raw_get::<Table>("values")
                    .map(|t| {
                        t.sequence_values::<Value>()
                            .flatten()
                            .filter_map(|v| number(&v))
                            .collect()
                    })
                    .unwrap_or_default(),
                slider_colour: text(w, "sliderColour"),
                inner_edge: match w.raw_get::<Value>("drawInnerEdge") {
                    Ok(Value::Boolean(b)) => Some(b),
                    _ => None,
                },
                fill_style: text(w, "fillStyle"),
                font_size: num(w, "fontSize"),
                font: text(w, "font"),
                align: text(w, "align"),
                kind,
                name,
            }
        })
        .collect();
    Ui {
        performance: flag(&root, "performance", false),
        width: num(&root, "width").unwrap_or(720.),
        height: num(&root, "height").unwrap_or(480.),
        background: text(&root, "background"),
        colour: text(&root, "colour"),
        widgets,
    }
}

/// What a machine keeps to read its widgets without allocating: the list
/// and the field names, made once.
pub(crate) struct Reads {
    widgets: Table,
    value: LuaString,
    visible: LuaString,
    enabled: LuaString,
    text: LuaString,
    values: LuaString,
}

impl Reads {
    pub(crate) fn new(lua: &Lua) -> Option<Self> {
        let widgets = lua
            .globals()
            .raw_get::<Table>("__ui")
            .and_then(|ui| ui.raw_get::<Table>("widgets"))
            .ok()?;
        let key = |k: &str| lua.create_string(k).ok();
        Some(Self {
            widgets,
            value: key("value")?,
            visible: key("visible")?,
            enabled: key("enabled")?,
            text: key("text")?,
            values: key("values")?,
        })
    }

    fn table(&self, n: u32) -> Option<Table> {
        self.widgets.raw_get::<Table>(i64::from(n) + 1).ok()
    }

    /// Widget `n` (from 0) as it stands.
    pub(crate) fn widget(&self, n: u32) -> Option<WidgetLook> {
        let w = self.table(n)?;
        let flag = |k: &LuaString| !matches!(w.raw_get::<Value>(k), Ok(Value::Boolean(false)));
        Some(WidgetLook {
            value: number(&w.raw_get::<Value>(&self.value).ok()?).unwrap_or(0.),
            visible: flag(&self.visible),
            enabled: flag(&self.enabled),
        })
    }

    /// Copy a table's numeric cells into preallocated storage.
    pub(crate) fn cells(&self, n: u32, out: &mut [f64]) -> bool {
        let Some(values) = self
            .table(n)
            .and_then(|w| w.raw_get::<Table>(&self.values).ok())
        else {
            return false;
        };
        for (i, dst) in out.iter_mut().enumerate() {
            let Ok(v) = values.raw_get::<Value>(i as i64 + 1) else {
                return false;
            };
            let Some(v) = number(&v) else {
                return false;
            };
            *dst = v;
        }
        true
    }

    /// Widget `n`'s `text` into `out`.
    pub(crate) fn text(&self, n: u32, out: &mut Text) -> bool {
        let Some(Ok(Value::String(s))) = self.table(n).map(|w| w.raw_get::<Value>(&self.text))
        else {
            return false;
        };
        let Ok(s) = s.to_str() else {
            return false;
        };
        *out = Text::from_str_cut(&s).0;
        true
    }
}

#[cfg(test)]
mod tests {
    use kontra_vm::{Foreign, HostApi, HostCall, HostValue, Step, Text};

    use crate::{Callbacks, compile};

    struct Quiet;

    impl HostApi for Quiet {
        fn call(&mut self, _: HostCall<'_>) -> HostValue {
            HostValue::Nothing
        }
    }

    #[test]
    fn a_page_lists_its_widgets_and_a_moved_one_runs_its_changed() {
        let source = r#"
            setSize(600, 300) setBackground("../art/back.png")
            local p = Panel{"Main", bounds={10, 20, 300, 200}, backgroundImage="../art/p.png"}
            local k = p:Knob{"Cutoff", 0.5, 0, 2, pos={5, 6}}
            k:setStripImage("../art/knob.png", 64)
            local on = OnOffButton{"Hold", false, normalImage="../art/off.png"}
            local shown = Label{"Shown", text="-"}
            k.changed = function(self) shown.text = string.format("%.2f", self.value) end
            on.changed = function(self) p.visible = not self.value end
        "#;
        let compiled = compile(source, &[], &[]).unwrap();
        let ui = &compiled.ui;
        assert_eq!((ui.width, ui.height), (600., 300.));
        assert_eq!(ui.background.as_deref(), Some("../art/back.png"));
        let names: Vec<_> = ui.widgets.iter().map(|w| w.name.as_str()).collect();
        assert_eq!(names, ["Main", "Cutoff", "Hold", "Shown"]);
        let knob = &ui.widgets[1];
        assert_eq!(
            (knob.parent, knob.rect, knob.sized),
            (Some(0), [5., 6., 0., 0.], [false; 2])
        );
        assert_eq!(knob.strip, Some(("../art/knob.png".into(), 64)));
        assert_eq!((knob.min, knob.max, knob.integer), (0., 2., false));
        assert_eq!(ui.widgets[0].rect, [10., 20., 300., 200.]);

        let mut m = compiled.machine();
        let moved = |m: &mut Box<dyn kontra_vm::Machine>, n: u32, v: f64| {
            m.stage_widget(n, v, None);
            let co = m.start(Callbacks::WIDGET, 0).expect("start");
            assert_eq!(m.run(co, &mut Quiet), Step::Done);
            assert_eq!(m.faults().bad_code, 0);
        };
        moved(&mut m, 1, 0.25);
        assert_eq!(m.widget(1).map(|w| w.value), Some(0.25));
        let mut text = Text::EMPTY;
        assert!(m.widget_text(3, &mut text));
        assert_eq!(text.as_str(), "0.25");
        // A button's 1 is `true`: its `changed` hides the panel.
        moved(&mut m, 2, 1.);
        assert_eq!(m.widget(2).map(|w| w.value), Some(1.));
        assert_eq!(m.widget(0).map(|w| w.visible), Some(false));
    }

    #[test]
    fn table_cells_keep_fractional_values_and_follow_script_writes() {
        let compiled = compile(r#"
            local t = Table{"curve", 3, 0.25, -1, 1, false, sliderColour="deb25d", drawInnerEdge=false, fillStyle="solid"}
            t:setValue(2, -0.75)
            local b = Button{"change"}
            local label = Label{"result", text="none"}
            t.changed = function(self, index) label.text = tostring(index) .. ":" .. tostring(self:getValue(index)) end
            b.changed = function() t:setValue(3, 0.5) end
        "#, &[], &[]).unwrap();
        let t = &compiled.ui.widgets[0];
        assert_eq!((t.min, t.max, t.integer), (-1., 1., false));
        assert_eq!(t.cells, [0.25, -0.75, 0.25]);
        assert_eq!(t.slider_colour.as_deref(), Some("deb25d"));
        assert_eq!(t.inner_edge, Some(false));
        let mut machine = compiled.machine();
        let mut cells = [0.; 3];
        assert!(machine.widget_cells(0, &mut cells));
        assert_eq!(cells, [0.25, -0.75, 0.25]);
        machine.stage_widget(1, 1., None);
        let co = machine.start(Callbacks::WIDGET, 0).expect("start");
        assert_eq!(machine.run(co, &mut Quiet), Step::Done);
        assert!(machine.widget_cells(0, &mut cells));
        assert_eq!(cells, [0.25, -0.75, 0.5]);
        machine.stage_widget(0, 0.875, Some(1));
        let co = machine.start(Callbacks::WIDGET, 0).expect("start");
        assert_eq!(machine.run(co, &mut Quiet), Step::Done);
        assert!(machine.widget_cells(0, &mut cells));
        assert_eq!(cells, [0.25, 0.875, 0.5]);
        let mut text = Text::EMPTY;
        assert!(machine.widget_text(2, &mut text));
        assert_eq!(text.as_str(), "2:0.875");
    }

    #[test]
    fn a_picture_set_in_on_init_shows_on_the_page_read_after_init() {
        let source = r#"
            local b = OnOffButton{"B", false}
            function onInit() b.normalImage = "../art/off.png" setBackground("../art/bg.png") end
        "#;
        let compiled = compile(source, &[], &[]).unwrap();
        assert_eq!(compiled.ui.widgets[0].normal, None);
        let mut m = compiled.machine();
        let co = m.start(Callbacks::INIT, 0).expect("start");
        assert_eq!(m.run(co, &mut Quiet), Step::Done);
        let page = m
            .own_page()
            .and_then(|p| p.downcast::<crate::Ui>().ok())
            .unwrap();
        assert_eq!(page.widgets[0].normal.as_deref(), Some("../art/off.png"));
        assert_eq!(page.background.as_deref(), Some("../art/bg.png"));
    }

    #[test]
    fn what_on_save_returns_comes_back_to_on_load_after_a_save() {
        let source = r#"
            level = 1
            function onSave() return {level = level, seats = {5, 6}, name = 'a "b"'} end
            function onLoad(data) level = data.level + #data.seats end
        "#;
        let mut m = compile(source, &[], &[]).unwrap().machine();
        let mut room = [0u8; 256];
        let len = m.save_state(&mut room).expect("a state");
        let state = std::str::from_utf8(&room[..len]).unwrap().to_owned();
        assert!(m.save_state(&mut [0u8; 4]).is_none(), "too small");
        let saved = [format!("{}={state}", crate::STATE)];
        let mut m = compile(source, &[], &saved).unwrap().machine();
        let co = m.start(Callbacks::INIT, 0).expect("start");
        assert_eq!(m.run(co, &mut Quiet), Step::Done);
        let mut room = [0u8; 256];
        let len = m.save_state(&mut room).unwrap();
        let again = std::str::from_utf8(&room[..len]).unwrap();
        assert!(again.contains(r#"["level"]=3"#), "{state} then {again}");
        assert!(again.contains(r#""a \"b\"""#), "{again}");
    }
}
