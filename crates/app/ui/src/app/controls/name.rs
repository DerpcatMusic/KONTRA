//! Names for controls in the generated KONTRA view.

use kontra_ui_model::{Interface, Kind, Rect, WidgetRef};

pub(super) fn label(tree: &Interface, id: WidgetRef) -> String {
    let Some(w) = tree.widgets.get(id.0) else {
        return String::new();
    };
    if !w.text.trim().is_empty() {
        return w.text.trim().to_owned();
    }
    let rect = tree.page_rect(id);
    let nearby = tree
        .widgets
        .iter()
        .enumerate()
        .filter_map(|(i, other)| {
            let candidate = WidgetRef(i);
            if other.page != w.page
                || !matches!(other.kind, Kind::Label)
                || other.text.trim().is_empty()
                || !tree.visible(candidate)
                || other.hide.title
            {
                return None;
            }
            distance(rect, tree.page_rect(candidate)).map(|d| (d, other.text.trim()))
        })
        .min_by(|a, b| a.0.total_cmp(&b.0));
    nearby.map_or_else(|| clean(&w.name), |(_, text)| text.to_owned())
}

// A label must overlap the control on one axis and be close on the other.
fn distance(a: Rect, b: Rect) -> Option<f64> {
    let dx = (a.x - (b.x + b.width)).max(b.x - (a.x + a.width)).max(0.);
    let dy = (a.y - (b.y + b.height)).max(b.y - (a.y + a.height)).max(0.);
    ((dx == 0. || dy == 0.) && dx.max(dy) <= 48.).then_some(dx + dy)
}

fn clean(name: &str) -> String {
    let name = name.trim_start_matches(['$', '~', '@', '%', '?', '!']);
    let last = name.rsplit('_').next().unwrap_or(name);
    let tail = [
        "slider", "knob", "button", "switch", "menu", "value", "sli", "kno", "btn", "swi",
    ]
    .iter()
    .find_map(|prefix| {
        tail.strip_prefix(prefix)
            .filter(|s| s.chars().next().is_some_and(char::is_uppercase))
    })
    .unwrap_or(tail);
    let mut out = String::new();
    let mut prev = None;
    for c in tail.chars() {
        if c == '_' || c == '-' {
            if !out.ends_with(' ') && !out.is_empty() {
                out.push(' ');
            }
        } else {
            if c.is_uppercase() && prev.is_some_and(|p: char| p.is_lowercase() || p.is_numeric()) {
                out.push(' ');
            }
            out.push(c);
        }
        prev = Some(c);
    }
    out.trim().to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use kontra_ui_model::{PageRef, Widget};

    #[test]
    fn own_text_then_nearby_label_then_clean_name() {
        let mut tree = Interface::default();
        let mut control = Widget::new(
            "$Mas_sliColor",
            PageRef(0),
            Rect::new(20, 20, 20, 20),
            Kind::Switch,
        );
        let mut near = Widget::new("label", PageRef(0), Rect::new(20, 42, 20, 10), Kind::Label);
        near.text = "Tone".into();
        let mut far = Widget::new(
            "other",
            PageRef(0),
            Rect::new(100, 100, 20, 10),
            Kind::Label,
        );
        far.text = "Other".into();
        control.text = "Own".into();
        tree.widgets = vec![control, near, far];
        assert_eq!(label(&tree, WidgetRef(0)), "Own");
        tree.widgets[0].text.clear();
        assert_eq!(label(&tree, WidgetRef(0)), "Tone");
        tree.widgets[1].hidden = true;
        assert_eq!(label(&tree, WidgetRef(0)), "Color");
        assert_eq!(clean("$Mas_sliFilterCutoff"), "Filter Cutoff");
        assert_eq!(clean("$filter_cutoff"), "filter cutoff");
    }
}
