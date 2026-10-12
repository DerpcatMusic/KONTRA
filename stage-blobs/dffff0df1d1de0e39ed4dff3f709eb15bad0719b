//! Creator Tools performance view files (`Resources/performance_view/*.nckp`).
//!
//! JSON: `value.performanceView` holds the page (`size`, `background`,
//! `icon`) and a tree of `controls`, each `{ "index": type, "value": {..} }`.
//! A control's KSP variable is its panel path and id joined with `_`
//! (`Footer_` > `Macro_` > `1` is `$Footer__Macro__1`). Controls come out in
//! tree order, panels before their contents: Kontakt numbers UI ids this way
//! and scripts rely on it (`get_ui_id($First) + i`).
use crate::model::{MenuItem, PerformanceControl, PerformanceView, Value, WidgetKind};
use serde_json::Value as Json;

/// The view a script loads, so a host can read
/// `Resources/performance_view/<name>.nckp` before compiling.
pub fn view_name(source: &str) -> Option<&str> {
    let at = source.find("load_performance_view")?;
    let rest = &source[at..];
    let open = rest.find('"')? + 1;
    let len = rest[open..].find('"')?;
    Some(&rest[open..open + len])
}

/// Parse a `.nckp`. Unknown control types are skipped and reported.
pub fn parse(bytes: &[u8]) -> Result<(PerformanceView, Vec<String>), String> {
    let bytes = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(bytes);
    let root: Json = serde_json::from_slice(bytes).map_err(|e| e.to_string())?;
    let pv = &root["value"]["performanceView"];
    if !pv.is_object() {
        return Err("no value.performanceView".into());
    }
    let int = |v: &Json| v.as_i64().map(|n| n as i32);
    let text = |v: &Json| v.as_str().filter(|s| !s.is_empty()).map(String::from);
    let mut view = PerformanceView {
        width: int(&pv["size"]["width"]),
        height: int(&pv["size"]["height"]),
        color: int(&pv["background"]["color"]),
        wallpaper: text(&pv["background"]["image"]),
        icon: text(&pv["icon"]["image"]).filter(|_| pv["icon"]["show"] != false),
        ..Default::default()
    };
    let mut skipped = Vec::new();
    walk(&pv["controls"], "", None, &mut view.controls, &mut skipped);
    Ok((view, skipped))
}

fn walk(
    controls: &Json,
    path: &str,
    parent: Option<&str>,
    out: &mut Vec<PerformanceControl>,
    skipped: &mut Vec<String>,
) {
    for c in controls.as_array().into_iter().flatten() {
        let v = &c["value"];
        let id = v["common"]["id"].as_str().unwrap_or_default();
        let path = if path.is_empty() {
            id.to_string()
        } else {
            format!("{path}_{id}")
        };
        let Some(kind) = c["index"].as_i64().and_then(kind) else {
            skipped.push(format!("{path}: control type {}", c["index"]));
            continue;
        };
        let prefix = match kind {
            WidgetKind::Table => '%',
            WidgetKind::TextEdit => '@',
            _ => '$',
        };
        let name = format!("{prefix}{path}");
        out.push(control(name.clone(), kind, v, parent));
        if kind == WidgetKind::Panel {
            walk(&v["controls"], &path, Some(&name), out, skipped);
        }
    }
}

/// `index` values seen in Creator Tools files.
// ponytail: 2 and the waveform/xy/file selector types have not been seen in
// an installed library yet; they are skipped and reported until one is.
fn kind(index: i64) -> Option<WidgetKind> {
    Some(match index {
        0 => WidgetKind::Panel,
        1 => WidgetKind::Button,
        3 => WidgetKind::Knob,
        4 => WidgetKind::Label,
        5 => WidgetKind::LevelMeter,
        6 => WidgetKind::Menu,
        7 => WidgetKind::Slider,
        8 => WidgetKind::Switch,
        9 => WidgetKind::Table,
        10 => WidgetKind::TextEdit,
        11 => WidgetKind::ValueEdit,
        _ => return None,
    })
}

fn control(name: String, kind: WidgetKind, v: &Json, parent: Option<&str>) -> PerformanceControl {
    let int = |v: &Json| v.as_i64().map(|n| n as i32);
    let mut props: Vec<(String, Value)> = Vec::new();
    let mut set = |name: &str, value: Value| props.push((format!("$CONTROL_PAR_{name}"), value));
    let common = &v["common"];
    for (key, par) in [("x", "POS_X"), ("y", "POS_Y")] {
        if let Some(n) = int(&common["position"][key]) {
            set(par, Value::Int(n));
        }
    }
    // A zero size means the stock size.
    for (key, par) in [("width", "WIDTH"), ("height", "HEIGHT")] {
        if let Some(n) = int(&common["size"][key]).filter(|n| *n > 0) {
            set(par, Value::Int(n));
        }
    }
    if let Some(z) = int(&common["zLayer"]) {
        set("Z_LAYER", Value::Int(z));
    }
    let mut hide = 0;
    if common["show"] == false {
        hide |= crate::builtins::HIDE_WHOLE_CONTROL;
    }
    if let Some(help) = common["infoPaneText"].as_str().filter(|s| !s.is_empty()) {
        set("HELP", Value::Text(help.into()));
    }
    if let Some(p) = parent {
        set("PARENT_PANEL", Value::Text(p.into()));
    }
    let auto = &v["automation"];
    if auto.is_object() {
        if let Some(n) = auto["name"].as_str().filter(|s| !s.is_empty()) {
            set("AUTOMATION_NAME", Value::Text(n.into()));
        }
        set(
            "ALLOW_AUTOMATION",
            Value::Int(i32::from(auto["disableUserAssign"] != true)),
        );
        if auto["hostAutomation"]["on"] == true
            && let Some(id) = int(&auto["hostAutomation"]["id"])
        {
            set("AUTOMATION_ID", Value::Int(id));
        }
    }
    // Picture: on the control, or as its background (labels, value edits).
    let bg = &v["background"];
    let picture = v["image"].as_str().or(bg["image"].as_str());
    if let Some(p) = picture.filter(|s| !s.is_empty()) {
        set("PICTURE", Value::Text(p.into()));
        if let Some(f) = int(&bg["frameIndex"]).filter(|f| *f > 0) {
            set("PICTURE_STATE", Value::Int(f));
        }
    }
    if bg["show"] == false {
        hide |= 1;
    }
    if hide != 0 {
        set("HIDE", Value::Int(hide));
    }
    let t = &v["text"];
    let caption = t["string"].as_str().or(v["name"].as_str());
    if let Some(s) = caption.filter(|s| !s.is_empty()) {
        set("TEXT", Value::Text(s.into()));
    }
    if let Some(f) = int(&t["font"]["type"]) {
        set("FONT_TYPE", Value::Int(f));
    }
    if let Some(a) = int(&t["horizontalAlignment"]) {
        set("TEXT_ALIGNMENT", Value::Int(a));
    }
    if let Some(y) = int(&t["positionY"]).filter(|y| *y != 0) {
        set("TEXTPOS_Y", Value::Int(y));
    }
    if let Some(u) = int(&v["unit"]) {
        set("UNIT", Value::Int(u));
    }
    if let Some(a) = v["showArrows"].as_bool() {
        set("SHOW_ARROWS", Value::Int(i32::from(a)));
    }
    if let Some(m) = v["mouse"].as_object() {
        // ponytail: orientation 1 read as horizontal (negative behaviour), unverified.
        let scale = m.get("scale").and_then(Json::as_i64).unwrap_or(0) as i32;
        let horizontal = m.get("orientation").and_then(Json::as_i64) == Some(1);
        set(
            "MOUSE_BEHAVIOUR",
            Value::Int(if horizontal { -scale } else { scale }),
        );
    }
    let colors = &v["colors"];
    for (key, par) in [
        ("background", "BG_COLOR"),
        ("active", "ON_COLOR"),
        ("inactive", "OFF_COLOR"),
        ("overload", "OVERLOAD_COLOR"),
        ("peak", "PEAK_COLOR"),
        ("bars", "BAR_COLOR"),
        ("zeroLine", "ZERO_LINE_COLOR"),
    ] {
        if let Some(c) = int(&colors[key]) {
            set(par, Value::Int(c));
        }
    }
    if kind == WidgetKind::LevelMeter {
        // ponytail: orientation 1 read as vertical, unverified.
        set("VERTICAL", Value::Int(int(&v["orientation"]).unwrap_or(0)));
    }
    let value = &v["value"];
    let (min, max) = (
        int(&value["min"]).unwrap_or(0),
        int(&value["max"]).unwrap_or(0),
    );
    let ratio = int(&v["ratio"]).unwrap_or(1);
    if let Some(d) = int(&value["default"]) {
        set("DEFAULT_VALUE", Value::Int(d));
        set("VALUE", Value::Int(d));
    }
    let size = |key| int(&common["size"][key]).unwrap_or(0);
    let (params, len) = match kind {
        WidgetKind::Knob | WidgetKind::ValueEdit => (vec![min, max, ratio], 0),
        WidgetKind::Slider => (vec![min, max], 0),
        WidgetKind::Table => {
            let range = int(&v["maxValue"]).unwrap_or(100);
            let range = if v["bipolar"] == true { -range } else { range };
            let steps = int(&v["steps"]["total"]).unwrap_or(0).max(1) as u32;
            (vec![size("width"), size("height"), range], steps)
        }
        _ => (vec![], 0),
    };
    let menu = v["entries"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|e| MenuItem {
            text: e["string"].as_str().unwrap_or_default().into(),
            value: int(&e["value"]).unwrap_or(0),
            visible: e["show"] != false,
        })
        .collect();
    PerformanceControl {
        name,
        kind,
        params,
        len,
        properties: props,
        menu,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nests_names_types_and_properties() {
        let json = br#"{"value":{"performanceView":{
            "size":{"width":970,"height":499},"background":{"color":1,"image":"wall"},
            "icon":{"image":"icon","show":true},
            "controls":[{"index":0,"value":{"common":{"id":"Footer_","show":true},"controls":[
                {"index":7,"value":{"common":{"id":"Alias","position":{"x":5,"y":6},"show":false,
                  "size":{"width":0,"height":0}},"mouse":{"orientation":1,"scale":500},
                  "value":{"default":2,"max":8,"min":0}}},
                {"index":9,"value":{"common":{"id":"T","size":{"width":60,"height":40}},
                  "bipolar":true,"maxValue":1000,"steps":{"total":32}}},
                {"index":99,"value":{"common":{"id":"X"}}}]}}]}}}"#;
        let (view, skipped) = parse(json).unwrap();
        assert_eq!(
            (view.width, view.icon.as_deref()),
            (Some(970), Some("icon"))
        );
        let names: Vec<_> = view.controls.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, ["$Footer_", "$Footer__Alias", "%Footer__T"]);
        assert_eq!(skipped, ["Footer__X: control type 99"]);
        let alias = &view.controls[1];
        assert_eq!(
            (alias.kind, alias.params.clone()),
            (WidgetKind::Slider, vec![0, 8])
        );
        let prop = |n: &str| {
            alias
                .properties
                .iter()
                .find(|(k, _)| k == n)
                .map(|(_, v)| v.clone())
        };
        assert_eq!(
            prop("$CONTROL_PAR_PARENT_PANEL"),
            Some(Value::Text("$Footer_".into()))
        );
        assert_eq!(prop("$CONTROL_PAR_MOUSE_BEHAVIOUR"), Some(Value::Int(-500)));
        assert_eq!(prop("$CONTROL_PAR_HIDE"), Some(Value::Int(16)));
        assert_eq!(prop("$CONTROL_PAR_VALUE"), Some(Value::Int(2)));
        assert_eq!(prop("$CONTROL_PAR_WIDTH"), None);
        let table = &view.controls[2];
        assert_eq!((table.params.clone(), table.len), (vec![60, 40, -1000], 32));
    }
}
