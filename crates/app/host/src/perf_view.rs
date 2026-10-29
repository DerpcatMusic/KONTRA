//! A Creator Tools performance view (`.nckp`): the controls a script loads
//! with `load_performance_view` and uses by name without declaring them.
//! Ported from v2's `sampler-ksp/src/nckp.rs` (itself from v1 0cb7a8a0,
//! `src/ksp/performance_view.rs`). The view gives each control its type and
//! look; `on init` then sets on top of it.

use kontra_ksp::ControlKind;
use serde_json::Value as Json;

/// The view's page and controls, in file order (a panel before its children).
#[derive(Clone, Debug, Default)]
pub struct View {
    pub width: Option<i32>,
    pub height: Option<i32>,
    pub color: Option<i32>,
    pub wallpaper: Option<String>,
    pub icon: Option<String>,
    pub controls: Vec<ViewControl>,
    /// Controls of a type this does not read, by path.
    pub skipped: Vec<String>,
}

/// One control: `name` is its script name (`$panel_knob`), properties are
/// by `$CONTROL_PAR_*` name.
#[derive(Clone, Debug)]
pub struct ViewControl {
    pub name: String,
    pub kind: ControlKind,
    /// Declaration parameters, as `declare ui_knob $x(min, max, ratio)`.
    pub params: Vec<i32>,
    pub ints: Vec<(String, i32)>,
    pub texts: Vec<(String, String)>,
    /// The panel it sits in, by script name.
    pub parent: Option<String>,
    pub menu: Vec<(String, i32, bool)>,
}

/// Reads a `.nckp`; `None` when it is not one.
#[must_use]
pub fn parse(bytes: &[u8]) -> Option<View> {
    let bytes = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(bytes);
    let root: Json = serde_json::from_slice(bytes).ok()?;
    let pv = root.get("value")?.get("performanceView")?;
    let text = |v: &Json| v.as_str().filter(|s| !s.is_empty()).map(String::from);
    let mut view = View {
        width: int(pv.at("size").at("width")),
        height: int(pv.at("size").at("height")),
        color: int(pv.at("background").at("color")),
        wallpaper: text(pv.at("background").at("image")),
        icon: text(pv.at("icon").at("image")).filter(|_| pv.at("icon").at("show") != false),
        ..View::default()
    };
    walk(pv.at("controls"), "", None, &mut view);
    Some(view)
}

/// `v[key]`, Null when absent (serde_json's `Index` without the lint).
trait At {
    fn at(&self, key: &str) -> &Json;
}

impl At for Json {
    fn at(&self, key: &str) -> &Json {
        static NULL: Json = Json::Null;
        self.get(key).unwrap_or(&NULL)
    }
}

fn nonempty(v: &Json) -> Option<&str> {
    v.as_str().filter(|s| !s.is_empty())
}

fn int(v: &Json) -> Option<i32> {
    v.as_i64().and_then(|n| i32::try_from(n).ok())
}

fn walk(controls: &Json, path: &str, parent: Option<&str>, view: &mut View) {
    for c in controls.as_array().into_iter().flatten() {
        let v = c.at("value");
        let id = v.at("common").at("id").as_str().unwrap_or_default();
        let path = if path.is_empty() {
            id.to_string()
        } else {
            format!("{path}_{id}")
        };
        let Some(kind) = c.at("index").as_i64().and_then(kind) else {
            view.skipped
                .push(format!("{path}: control type {}", c.at("index")));
            continue;
        };
        let sigil = match kind {
            ControlKind::Table => '%',
            ControlKind::TextEdit => '@',
            _ => '$',
        };
        let name = format!("{sigil}{path}");
        view.controls.push(control(name.clone(), kind, v, parent));
        if kind == ControlKind::Panel {
            walk(v.at("controls"), &path, Some(&name), view);
        }
    }
}

/// `index` values seen in Creator Tools files.
// ponytail: 2 and the waveform/xy/file selector types have not been seen in
// an installed library yet; they are skipped and reported until one is.
fn kind(index: i64) -> Option<ControlKind> {
    Some(match index {
        0 => ControlKind::Panel,
        1 => ControlKind::Button,
        3 => ControlKind::Knob,
        4 => ControlKind::Label,
        5 => ControlKind::LevelMeter,
        6 => ControlKind::Menu,
        7 => ControlKind::Slider,
        8 => ControlKind::Switch,
        9 => ControlKind::Table,
        10 => ControlKind::TextEdit,
        11 => ControlKind::ValueEdit,
        _ => return None,
    })
}

/// `$HIDE_WHOLE_CONTROL` and `$HIDE_PART_BG` (constants.tsv).
const HIDE_WHOLE: i32 = 16;
const HIDE_BG: i32 = 1;

fn control(name: String, kind: ControlKind, v: &Json, parent: Option<&str>) -> ViewControl {
    let mut ints = Vec::new();
    let mut texts = Vec::new();
    let par = |name: &str| format!("$CONTROL_PAR_{name}");
    let mut set = |name: &str, value: i32| ints.push((par(name), value));
    let common = v.at("common");
    for (key, name) in [("x", "POS_X"), ("y", "POS_Y")] {
        if let Some(n) = int(common.at("position").at(key)) {
            set(name, n);
        }
    }
    // A zero size means the stock size.
    for (key, name) in [("width", "WIDTH"), ("height", "HEIGHT")] {
        if let Some(n) = int(common.at("size").at(key)).filter(|n| *n > 0) {
            set(name, n);
        }
    }
    if let Some(z) = int(common.at("zLayer")) {
        set("Z_LAYER", z);
    }
    let mut hide = 0;
    if common.at("show") == false {
        hide |= HIDE_WHOLE;
    }
    let auto = v.at("automation");
    if auto.is_object() {
        set(
            "ALLOW_AUTOMATION",
            i32::from(auto.at("disableUserAssign") != true),
        );
        if auto.at("hostAutomation").at("on") == true
            && let Some(id) = int(auto.at("hostAutomation").at("id"))
        {
            set("AUTOMATION_ID", id);
        }
    }
    let bg = v.at("background");
    if let Some(f) = int(bg.at("frameIndex")).filter(|f| *f > 0) {
        set("PICTURE_STATE", f);
    }
    if bg.at("show") == false {
        hide |= HIDE_BG;
    }
    if hide != 0 {
        set("HIDE", hide);
    }
    let t = v.at("text");
    // Custom font names need the script's font registry: reported, not mapped.
    for (state, name) in [
        ("off", "FONT_TYPE"),
        ("on", "FONT_TYPE_ON"),
        ("pressedOff", "FONT_TYPE_OFF_PRESSED"),
        ("pressedOn", "FONT_TYPE_ON_PRESSED"),
        ("hoverOff", "FONT_TYPE_OFF_HOVER"),
        ("hoverOn", "FONT_TYPE_ON_HOVER"),
    ] {
        let font = if state == "off" {
            t.at("font")
        } else {
            t.at("fonts").at(state)
        };
        if let Some(f) = int(font.at("type")) {
            set(name, f);
        }
    }
    if let Some(shift) = t.at("shiftOnPress").as_bool() {
        set("DISABLE_TEXT_SHIFTING", i32::from(!shift));
    }
    if let Some(a) = int(t.at("horizontalAlignment")) {
        set("TEXT_ALIGNMENT", a);
    }
    if let Some(y) = int(t.at("positionY")).filter(|y| *y != 0) {
        set("TEXTPOS_Y", y);
    }
    if let Some(u) = int(v.at("unit")) {
        set("UNIT", u);
    }
    if let Some(a) = v.at("showArrows").as_bool() {
        set("SHOW_ARROWS", i32::from(a));
    }
    if let Some(m) = v.at("mouse").as_object() {
        // ponytail: orientation 1 read as horizontal (negative behaviour), unverified.
        let scale = m.get("scale").and_then(int).unwrap_or(0);
        let horizontal = m.get("orientation").and_then(Json::as_i64) == Some(1);
        set("MOUSE_BEHAVIOUR", if horizontal { -scale } else { scale });
    }
    for (key, name) in [
        ("background", "BG_COLOR"),
        ("active", "ON_COLOR"),
        ("inactive", "OFF_COLOR"),
        ("overload", "OVERLOAD_COLOR"),
        ("peak", "PEAK_COLOR"),
        ("bars", "BAR_COLOR"),
        ("zeroLine", "ZERO_LINE_COLOR"),
    ] {
        if let Some(c) = int(v.at("colors").at(key)) {
            set(name, c);
        }
    }
    if kind == ControlKind::LevelMeter {
        // ponytail: orientation 1 read as vertical, unverified.
        set("VERTICAL", int(v.at("orientation")).unwrap_or(0));
    }
    let value = v.at("value");
    if let Some(d) = int(value.at("default")) {
        set("DEFAULT_VALUE", d);
    }
    if kind == ControlKind::Table
        && let Some(visible) = int(v.at("steps").at("visible"))
    {
        ints.push(("table_steps_shown".into(), visible));
    }
    let mut text = |name: &str, s: Option<&str>| {
        if let Some(s) = s {
            texts.push((par(name), s.to_owned()));
        }
    };
    text("HELP", nonempty(common.at("infoPaneText")));
    text("AUTOMATION_NAME", nonempty(auto.at("name")));
    // Picture: on the control, or as its background (labels, value edits).
    text(
        "PICTURE",
        nonempty(v.at("image")).or(nonempty(bg.at("image"))),
    );
    // An authored empty string is no text; leaving TEXT unset would show the
    // variable name, Kontakt's default label for script-declared controls.
    text("TEXT", t.at("string").as_str().or(v.at("name").as_str()));
    let (min, max) = (
        int(value.at("min")).unwrap_or(0),
        int(value.at("max")).unwrap_or(0),
    );
    let size = |key| int(common.at("size").at(key)).unwrap_or(0);
    let params = match kind {
        ControlKind::Knob | ControlKind::ValueEdit => {
            vec![min, max, int(v.at("ratio")).unwrap_or(1)]
        }
        ControlKind::Slider => vec![min, max],
        ControlKind::Table => {
            let range = int(v.at("maxValue")).unwrap_or(100);
            let range = if v.at("bipolar") == true {
                -range
            } else {
                range
            };
            vec![size("width"), size("height"), range]
        }
        _ => vec![],
    };
    let menu = v
        .at("entries")
        .as_array()
        .into_iter()
        .flatten()
        .map(|e| {
            let text = e.at("string").as_str().unwrap_or_default().to_owned();
            (text, int(e.at("value")).unwrap_or(0), e.at("show") != false)
        })
        .collect();
    ViewControl {
        name,
        kind,
        params,
        ints,
        texts,
        parent: parent.map(Into::into),
        menu,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nests_names_types_and_properties() {
        let view = parse(
            br#"{"value":{"performanceView":{"size":{"width":700,"height":300},
            "background":{"image":"bg"},"controls":[
            {"index":0,"value":{"common":{"id":"main","position":{"x":5,"y":6}},"controls":[
              {"index":3,"value":{"common":{"id":"cut","size":{"width":40,"height":0}},
                "value":{"min":0,"max":100,"default":50},"image":"knob","text":{"string":""}}}]}},
            {"index":42,"value":{"common":{"id":"odd"}}}]}}}"#,
        )
        .unwrap();
        assert_eq!((view.width, view.height), (Some(700), Some(300)));
        assert_eq!(view.wallpaper.as_deref(), Some("bg"));
        let [panel, knob] = view.controls.as_slice() else {
            panic!("{:?}", view.controls)
        };
        assert_eq!(
            (panel.name.as_str(), panel.kind),
            ("$main", ControlKind::Panel)
        );
        assert_eq!(
            (knob.name.as_str(), knob.kind),
            ("$main_cut", ControlKind::Knob)
        );
        assert_eq!(knob.parent.as_deref(), Some("$main"));
        assert_eq!(knob.params, [0, 100, 1]);
        assert!(knob.ints.contains(&("$CONTROL_PAR_WIDTH".into(), 40)));
        assert!(!knob.ints.iter().any(|(p, _)| p == "$CONTROL_PAR_HEIGHT"));
        assert!(
            knob.texts
                .contains(&("$CONTROL_PAR_TEXT".into(), String::new()))
        );
        assert_eq!(view.skipped, ["odd: control type 42"]);
    }
}
