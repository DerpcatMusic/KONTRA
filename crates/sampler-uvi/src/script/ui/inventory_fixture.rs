//! Test-only transport for this benchmark's authored Label/style/page shape.
//! The UI IR deliberately has no serde dependency. Encoding must roundtrip the
//! complete Interface exactly; adding an unrepresented field fails that oracle.
use sampler_ui_ir as ir;
use serde_json::{Value, json};

pub fn encode(face: &ir::Interface) -> Value {
    let value = json!({
        "schema":"kontra-authored-inventory-labels-v1",
        "pages":face.pages.iter().map(|p|json!({"name":p.name,"size":[p.size.width,p.size.height]})).collect::<Vec<_>>(),
        "paint_order":face.paint_order.as_ref().map(|order|order.iter().map(|id|id.0).collect::<Vec<_>>()),
        "styles":face.styles.iter().map(|s|json!({"size":s.size,"color":[s.color.r,s.color.g,s.color.b,s.color.a]})).collect::<Vec<_>>(),
        "widgets":face.widgets.iter().map(|w|json!({
            "name":w.name,"source_id":w.source_id,"rect":[w.rect.x,w.rect.y,w.rect.width,w.rect.height],
            "initial_value":w.initial_value,"opacity":w.opacity,"intercepts_mouse":w.intercepts_mouse,
            "hidden":w.hidden,"enabled":w.enabled,"tooltip":w.tooltip,"text":w.text,
            "value_text":w.value_text,"mapper":w.mapper,"style":w.style.map(|id|id.0),"menu_arrow":w.menu_arrow,
            "automation_allowed":w.automation.allowed
        })).collect::<Vec<_>>(),
        "unsupported":face.unsupported.iter().map(|row|json!({"widget":row.widget.map(|id|id.0),"feature":row.feature,"value":row.value})).collect::<Vec<_>>()
    });
    assert_eq!(
        decode(&value).unwrap(),
        *face,
        "fixture transport must represent the complete authored Interface"
    );
    value
}

fn keys(value: &Value, expected: &[&str]) -> Result<(), String> {
    let object = value.as_object().ok_or("expected object")?;
    if object.len() != expected.len() || !expected.iter().all(|key| object.contains_key(*key)) {
        return Err("unrepresented fixture field".into());
    }
    Ok(())
}
fn array(value: &Value, limit: usize) -> Result<&[Value], String> {
    let values = value.as_array().ok_or("expected array")?;
    if values.len() > limit {
        return Err("fixture item limit".into());
    }
    Ok(values)
}
fn text(value: &Value) -> Result<String, String> {
    let text = value.as_str().ok_or("expected string")?;
    if text.len() > 4096 {
        return Err("fixture text limit".into());
    }
    Ok(text.to_owned())
}
fn number(value: &Value) -> Result<f64, String> {
    value
        .as_f64()
        .filter(|n| n.is_finite())
        .ok_or("expected finite number".into())
}
fn index(value: &Value) -> Result<usize, String> {
    value
        .as_u64()
        .and_then(|n| usize::try_from(n).ok())
        .ok_or("expected index".into())
}
fn boolean(value: &Value) -> Result<bool, String> {
    value.as_bool().ok_or("expected boolean".into())
}
fn optional_text(value: &Value) -> Result<Option<String>, String> {
    if value.is_null() {
        Ok(None)
    } else {
        text(value).map(Some)
    }
}

pub fn decode(value: &Value) -> Result<ir::Interface, String> {
    keys(
        value,
        &[
            "schema",
            "pages",
            "paint_order",
            "styles",
            "widgets",
            "unsupported",
        ],
    )?;
    if value["schema"] != "kontra-authored-inventory-labels-v1" {
        return Err("unexpected fixture schema".into());
    }
    let mut face = ir::Interface {
        source: ir::Source::FalconLua,
        ..Default::default()
    };
    for p in array(&value["pages"], 1)? {
        keys(p, &["name", "size"])?;
        let size = array(&p["size"], 2)?;
        if size.len() != 2 {
            return Err("expected size".into());
        }
        face.pages.push(ir::Page {
            name: text(&p["name"])?,
            size: ir::Size {
                width: number(&size[0])?,
                height: number(&size[1])?,
            },
            ..Default::default()
        });
    }
    face.paint_order = Some(
        array(&value["paint_order"], 10_145)?
            .iter()
            .map(|v| index(v).map(ir::WidgetRef))
            .collect::<Result<_, _>>()?,
    );
    for s in array(&value["styles"], 10_145)? {
        keys(s, &["size", "color"])?;
        let color = array(&s["color"], 4)?;
        if color.len() != 4 {
            return Err("expected color".into());
        }
        let component = |i: usize| -> Result<u8, String> {
            color[i]
                .as_u64()
                .and_then(|n| u8::try_from(n).ok())
                .ok_or("invalid color".into())
        };
        face.styles.push(ir::TextStyle {
            font: ir::Font::Default,
            size: if s["size"].is_null() {
                None
            } else {
                Some(number(&s["size"])? as f32)
            },
            color: ir::Rgba {
                r: component(0)?,
                g: component(1)?,
                b: component(2)?,
                a: component(3)?,
            },
            align: ir::Align::Center,
        });
    }
    for w in array(&value["widgets"], 10_145)? {
        keys(
            w,
            &[
                "name",
                "source_id",
                "rect",
                "initial_value",
                "opacity",
                "intercepts_mouse",
                "hidden",
                "enabled",
                "tooltip",
                "text",
                "value_text",
                "mapper",
                "style",
                "menu_arrow",
                "automation_allowed",
            ],
        )?;
        let rect = array(&w["rect"], 4)?;
        if rect.len() != 4 {
            return Err("expected rectangle".into());
        }
        let mut widget = ir::Widget::new(
            text(&w["name"])?,
            ir::PageRef(0),
            ir::Rect {
                x: number(&rect[0])?,
                y: number(&rect[1])?,
                width: number(&rect[2])?,
                height: number(&rect[3])?,
            },
            ir::Kind::Label,
        );
        widget.source_id = Some(
            w["source_id"]
                .as_i64()
                .and_then(|n| i32::try_from(n).ok())
                .ok_or("invalid source id")?,
        );
        widget.initial_value = number(&w["initial_value"])?;
        widget.opacity = number(&w["opacity"])? as f32;
        widget.intercepts_mouse = boolean(&w["intercepts_mouse"])?;
        widget.hidden = boolean(&w["hidden"])?;
        widget.enabled = boolean(&w["enabled"])?;
        widget.tooltip = text(&w["tooltip"])?;
        widget.text = text(&w["text"])?;
        widget.value_text = optional_text(&w["value_text"])?;
        widget.mapper = optional_text(&w["mapper"])?;
        widget.style = if w["style"].is_null() {
            None
        } else {
            Some(ir::StyleRef(index(&w["style"])?))
        };
        widget.menu_arrow = boolean(&w["menu_arrow"])?;
        widget.automation.allowed = boolean(&w["automation_allowed"])?;
        face.widgets.push(widget);
    }
    for row in array(&value["unsupported"], 10_145)? {
        keys(row, &["widget", "feature", "value"])?;
        face.unsupported.push(ir::Unsupported {
            widget: if row["widget"].is_null() {
                None
            } else {
                Some(ir::WidgetRef(index(&row["widget"])?))
            },
            feature: text(&row["feature"])?,
            value: text(&row["value"])?,
        });
    }
    face.validate().map_err(|e| e.to_string())?;
    Ok(face)
}
