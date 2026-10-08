//! Normalize v1's authored choice-list association into the existing articulation IR.
//! This enriches source metadata once at load time; it is never a UI preference model.
use sampler_ir as ir;
use sampler_ui_ir::{Binding, Interface, Kind, Placement, Widget};

fn position(w: &Widget) -> (i32, i32, u32, u32) {
    match w.placement {
        Placement::Grid { column, row } => (column as i32 * 92, row as i32 * 22, w.rect.width.max(92), w.rect.height.max(22)),
        Placement::Pixels => (w.rect.x, w.rect.y, w.rect.width, w.rect.height),
    }
}

/// Wide aligned selection controls, including scroll-hidden rows, and named
/// coloured keyboard controls when no recognized switching pattern supplies rows.
pub(crate) fn normalize(instrument: &mut ir::Instrument, interfaces: &[Interface], scripts: &[sampler_ksp::Script]) {
    let mut keys = vec![sampler_ksp::model::Key::default(); 128];
    for script in scripts {
        for (key, source) in keys.iter_mut().zip(&script.view().model().interface.keys) {
            if source.name.is_some() { key.name.clone_from(&source.name); }
            if source.color.is_some() { key.color = source.color; }
            if source.kind.is_some() { key.kind = source.kind; }
        }
    }
    let mut found = Vec::new();
    for ui in interfaces {
        let mut choices: Vec<_> = ui.widgets.iter().filter(|w| matches!(w.kind, Kind::Button { .. } | Kind::Switch) && matches!(w.binding, Binding::Control(_))).collect();
        choices.sort_by_key(|w| { let (x, y, width, _) = position(w); (w.page.0, w.parent.map(|p| p.0), x, width, y) });
        let mut runs: Vec<Vec<&Widget>> = Vec::new();
        for w in choices {
            let (x, y, width, height) = position(w);
            if width < height * 3 { continue; }
            // Hidden overlays at a visible row's position are alternate faces, not extra rows.
            if w.hidden && ui.widgets.iter().any(|o| !o.hidden && o.page == w.page && o.parent == w.parent && position(o) == position(w)) { continue; }
            let joins = runs.last().and_then(|r| r.last()).is_some_and(|last| {
                let (lx, ly, lw, lh) = position(last);
                last.page == w.page && last.parent == w.parent && lx == x && lw == width && y >= ly + lh as i32 && y - ly - lh as i32 <= 12
            });
            if joins { runs.last_mut().unwrap().push(w); } else { runs.push(vec![w]); }
        }
        for run in runs.into_iter().filter(|r| r.len() >= 3 && r.iter().filter(|w| !w.hidden).count() >= 2) {
            for w in run {
                let (x, y, width, height) = position(w);
                let labels: Vec<_> = ui.widgets.iter().filter(|l| matches!(l.kind, Kind::Label | Kind::TextEdit) && l.page == w.page && l.parent == w.parent && l.hidden == w.hidden && !l.text.trim().is_empty()).filter(|l| {
                    let (lx, ly, _, lh) = position(l);
                    (ly + lh as i32 / 2 >= y && ly + lh as i32 / 2 <= y + height as i32) && lx < x + width as i32 + 160 && lx >= x - 16
                }).collect();
                let name = labels.iter().find(|l| ir::parse_note(&l.text).is_none() && l.text.chars().filter(|c| c.is_alphanumeric()).count() > 1).map_or(w.text.trim(), |l| l.text.trim());
                if name.is_empty() { continue; }
                let key = labels.iter().find_map(|l| ir::parse_note(&l.text)).or_else(|| keys.iter().position(|k| k.name.as_deref().is_some_and(|s| s.trim() == name)).map(|k| k as u8));
                let Binding::Control(control) = w.binding else { continue };
                let source = format!("ksp-control:{:032x}", control.0);
                found.push(ir::Articulation { source, control: Some(control.0), name: name.into(), switch_keys: key.into_iter().collect(), ..Default::default() });
            }
        }
    }
    if instrument.articulations.is_empty() {
        if found.is_empty() {
            let mut runs: Vec<Vec<(u8, String)>> = Vec::new();
            for (n, key) in keys.iter().enumerate() {
                let Some(name) = key.name.as_ref().filter(|s| !s.trim().is_empty()) else { continue };
                if !key.color.is_some_and(|c| (0..16).contains(&c)) { continue; }
                if runs.last().and_then(|r| r.last()).is_some_and(|(last, _)| n - usize::from(*last) <= 2) { runs.last_mut().unwrap().push((n as u8, name.clone())); }
                else { runs.push(vec![(n as u8, name.clone())]); }
            }
            if let Some(run) = runs.into_iter().filter(|r| r.len() >= 2).max_by_key(Vec::len) {
                found = run.into_iter().map(|(key, name)| ir::Articulation { source: format!("named-key:{key}"), name, switch_keys: vec![key], ..Default::default() }).collect();
            }
        }
        if !found.is_empty() {
            // These choices belong to authored callbacks, never zone admission.
            instrument.articulations = found;
            instrument.switching.owner = ir::SwitchOwner::Behavior;
            instrument.assign_alternatives(super::keyswitch::CONTROLLER);
        }
    } else {
        for a in &mut instrument.articulations {
            if let Some(row) = found.iter().find(|r| r.switch_keys.iter().any(|k| a.switch_keys.contains(k))) {
                a.control = row.control;
                a.name.clone_from(&row.name);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sampler_ui_ir::{ControlId, PageRef, Rect};
    #[test]
    fn keyswitch_authored_rows_keep_control_identity_and_hidden_scroll_rows() {
        let mut ui = Interface::default();
        for n in 0..4 {
            let mut button = Widget::new(format!("$choice{n}"), PageRef(0), Rect::new(10, n * 24, 160, 24), Kind::Button { momentary: false });
            button.binding = Binding::Control(ControlId(100 + n as u128));
            button.text = "same name".into();
            button.hidden = n == 3;
            ui.widgets.push(button);
            let mut label = Widget::new(format!("$note{n}"), PageRef(0), Rect::new(180, n * 24, 40, 24), Kind::Label);
            label.text = ["C0", "C#0", "D0", "D#0"][n as usize].into();
            label.hidden = n == 3;
            ui.widgets.push(label);
        }
        let mut hidden_overlay = ui.widgets[0].clone();
        hidden_overlay.binding = Binding::Control(ControlId(999));
        hidden_overlay.hidden = true;
        ui.widgets.push(hidden_overlay);
        let mut inst = ir::Instrument::default();
        normalize(&mut inst, &[ui.clone()], &[]);
        assert_eq!(inst.articulations.len(), 4, "hidden scrolling choice retained; alternate face excluded");
        assert_eq!(inst.articulations.iter().map(|a| a.switch_keys[0]).collect::<Vec<_>>(), vec![24,25,26,27]);
        assert_eq!(inst.articulations.iter().map(|a| a.control).collect::<Vec<_>>(), (100..104).map(Some).collect::<Vec<_>>());
        let source: Vec<_> = inst.articulations.iter().map(|a| a.source.clone()).collect();
        for w in &mut ui.widgets { if matches!(w.kind, Kind::Button { .. }) { w.text = "renamed choice".into(); } }
        let mut reloaded = ir::Instrument::default();
        normalize(&mut reloaded, &[ui], &[]);
        assert_eq!(source, reloaded.articulations.iter().map(|a| a.source.clone()).collect::<Vec<_>>(), "identity follows controls, never caption/order");
    }
}
